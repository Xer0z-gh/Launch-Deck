//! The process supervisor.
//!
//! Owns every running project: spawns them into job objects, streams their
//! output, tracks their state, notices when they die, and restarts them when
//! policy says to.
//!
//! # Spawn sequence
//!
//! The order matters and is not negotiable:
//!
//! 1. Create a [`JobObject`].
//! 2. Spawn the child **suspended** ([`job::SPAWN_FLAGS`]).
//! 3. Assign the child to the job.
//! 4. Resume the child.
//!
//! Steps 2-4 exist to close the window in which a child could spawn a grandchild
//! that escapes the job. See [`crate::job`] for why that matters.
//!
//! # Stopping
//!
//! Two stages, because Windows has no `SIGTERM`:
//!
//! 1. Close the child's stdin. Many CLI tools treat EOF as "shut down" and get
//!    to release ports and flush output cleanly.
//! 2. After the grace period, terminate the job -- the whole tree, atomically.
//!
//! The state machine models the interval between them as
//! [`RunState::Stopping`], so the UI shows progress rather than flickering
//! between running and stopped.
//!
//! # Crash-loop protection
//!
//! Auto-restart without backoff turns a project that crashes on startup into a
//! process fork bomb that saturates a core and floods the log pipeline. Restarts
//! use exponential backoff via [`RestartPolicy::backoff_ms`] and stop entirely at
//! the policy's attempt ceiling, reporting
//! [`DeckError::RestartLoopBrokenOut`](deck_domain::DeckError::RestartLoopBrokenOut)
//! in the log. A successful run resets the counter.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use deck_domain::command::{Lifecycle, ResolvedCommand};
use deck_domain::error::{DeckError, Result};
use deck_domain::log::{LogStream, RunOutcome};
use deck_domain::project::{ProjectId, RestartPolicy};
use deck_domain::runtime::{RunId, RunState, StopMode};
use parking_lot::Mutex;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::job::{self, JobObject};
use crate::logs::LogSink;

/// How often output is sampled while waiting for a graceful stop.
///
/// 100ms rather than 250ms because the window length is most of the response
/// time, and response time is bytes. Circle-Calculator writes roughly 40 MB a
/// second once it is looping, so every window spent confirming costs 4 MB.
const RUNAWAY_WINDOW_MS: u64 = 100;

/// Lines within one [`RUNAWAY_WINDOW_MS`] window that mean "not shutting down".
///
/// 200 lines in 100ms is 2,000 a second. No program winding down says that
/// much; the case this exists for said roughly 800,000 a second.
const RUNAWAY_LINES_PER_WINDOW: u64 = 200;

/// Output in one window so far past normal that it needs no confirmation.
///
/// The two-window rule exists so a noisy final flush on the way out is not
/// mistaken for a loop. A flush is bounded, though -- it is whatever the
/// program had buffered -- so beyond some volume the ambiguity is gone and
/// waiting another window only buys megabytes. Ten times the threshold is
/// 20,000 lines a second, which nothing shutting down produces.
///
/// This is what took the real case from 19.55 MB to a fraction of it: the
/// two-window rule alone meant 500ms of flooding at 40 MB/s before anything
/// happened.
const RUNAWAY_LINES_IMMEDIATE: u64 = RUNAWAY_LINES_PER_WINDOW * 10;

/// Run logs kept per project, oldest deleted after each run finishes.
///
/// Ten covers "what went wrong the last few times I ran this", which is every
/// question a log answers here, without keeping a file per launch forever.
const LOGS_KEPT_PER_PROJECT: usize = 10;

/// Everything needed to launch one lifecycle step of one project.
#[derive(Debug, Clone)]
pub struct SpawnRequest {
    /// Project being launched.
    pub project_id: ProjectId,
    /// Project name, used in messages.
    pub project_name: String,
    /// Which lifecycle step this is.
    pub lifecycle: Lifecycle,
    /// The fully-resolved command.
    pub command: ResolvedCommand,
    /// Directory to run in.
    pub working_dir: PathBuf,
    /// Environment additions, layered over the inherited environment.
    pub env: Vec<(String, String)>,
    /// What to do if it exits.
    pub restart_policy: RestartPolicy,
    /// Directory this project's log files live in.
    pub log_dir: PathBuf,
    /// Runner that produced this command, for looking up its health probe.
    pub runner_id: String,
    /// Ports this project is expected to bind, configured or conventional.
    ///
    /// Carried on the request so the 1 Hz readiness check needs no database
    /// read: at one query per running project per second, that cost would
    /// dwarf the check itself.
    pub expected_ports: Vec<u16>,
}

/// Something the supervisor wants the rest of the app to know about.
#[derive(Debug, Clone)]
pub enum SupervisorEvent {
    /// A project's run state changed.
    StateChanged {
        /// Which project.
        project_id: ProjectId,
        /// Which run.
        run_id: RunId,
        /// The new state.
        state: RunState,
    },
    /// A run finished, with its final disposition.
    RunFinished {
        /// Which project.
        project_id: ProjectId,
        /// Which run.
        run_id: RunId,
        /// How it ended.
        outcome: RunOutcome,
        /// Exit code, when the OS reported one.
        exit_code: Option<i32>,
    },
}

/// Live bookkeeping for one running project.
struct RunHandle {
    run_id: RunId,
    lifecycle: Lifecycle,
    state: RunState,
    /// Kept alive for the run's duration: dropping it kills the tree.
    job: Arc<JobObject>,
    log: Arc<LogSink>,
    /// Taken and dropped to signal EOF during a graceful stop.
    stdin: Option<tokio::process::ChildStdin>,
    /// Set when the user asked to stop, so an exit is reported as Stopped
    /// rather than as a crash, and no restart is attempted.
    stop_requested: bool,
    /// Consecutive failed starts, for backoff and the circuit breaker.
    restart_attempt: u32,
    request: SpawnRequest,
}

/// Manages every running project.
///
/// Cheap to clone behind an [`Arc`]; all state is internally synchronised.
pub struct Supervisor {
    running: Mutex<HashMap<ProjectId, RunHandle>>,
    events: tokio::sync::broadcast::Sender<SupervisorEvent>,
}

impl Supervisor {
    /// Creates an empty supervisor.
    #[must_use]
    pub fn new() -> Arc<Self> {
        let (events, _) = tokio::sync::broadcast::channel(256);
        Arc::new(Self {
            running: Mutex::new(HashMap::new()),
            events,
        })
    }

    /// Subscribes to state changes.
    #[must_use]
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<SupervisorEvent> {
        self.events.subscribe()
    }

    /// How many projects are live right now.
    ///
    /// Used by the tray tooltip and the quit path: once the window can hide,
    /// this count is the only thing telling the user that quitting is about to
    /// stop real work.
    #[must_use]
    pub fn running_count(&self) -> usize {
        self.running
            .lock()
            .values()
            .filter(|h| h.state.is_live())
            .count()
    }

    /// The current state of a project.
    ///
    /// Projects with no run on record are [`RunState::Idle`].
    #[must_use]
    pub fn state(&self, project_id: ProjectId) -> RunState {
        self.running
            .lock()
            .get(&project_id)
            .map_or(RunState::Idle, |h| h.state.clone())
    }

    /// Every project the supervisor is tracking, with its state.
    #[must_use]
    pub fn states(&self) -> Vec<(ProjectId, RunState)> {
        self.running
            .lock()
            .iter()
            .map(|(id, h)| (*id, h.state.clone()))
            .collect()
    }

    /// The runner id and expected ports for a project's current run.
    ///
    /// Read from the in-memory spawn request rather than the store, so the
    /// health tick performs no I/O.
    #[must_use]
    pub fn health_inputs(&self, project_id: ProjectId) -> Option<(String, Vec<u16>)> {
        self.running
            .lock()
            .get(&project_id)
            .map(|h| (h.request.runner_id.clone(), h.request.expected_ports.clone()))
    }

    /// Records a readiness verdict, returning the new state if it changed.
    ///
    /// Returns `None` when nothing changed, so the caller can emit an event
    /// only on a real transition. Without that, a 1 Hz health tick would
    /// broadcast an identical state every second for the entire life of every
    /// running project.
    ///
    /// Only touches `Running`: a project that exited while the health tick was
    /// in flight must not be resurrected into a healthy state by a stale
    /// sample.
    pub fn set_healthy(&self, project_id: ProjectId, healthy: Option<bool>) -> Option<RunState> {
        let mut running = self.running.lock();
        let handle = running.get_mut(&project_id)?;
        match &mut handle.state {
            RunState::Running { healthy: current, .. } if *current != healthy => {
                *current = healthy;
                Some(handle.state.clone())
            }
            _ => None,
        }
    }

    /// The log sink for a project's current or most recent run.
    #[must_use]
    pub fn logs(&self, project_id: ProjectId) -> Option<Arc<LogSink>> {
        self.running.lock().get(&project_id).map(|h| Arc::clone(&h.log))
    }

    /// Which lifecycle step the current or most recent run is executing.
    ///
    /// The UI needs this to distinguish states that look identical otherwise:
    /// "Building" and "Running" are both a live process, and conflating them
    /// makes a long build look like a started server.
    #[must_use]
    pub fn current_lifecycle(&self, project_id: ProjectId) -> Option<Lifecycle> {
        self.running.lock().get(&project_id).map(|h| h.lifecycle)
    }

    /// Identifier of the current or most recent run for a project.
    #[must_use]
    pub fn current_run(&self, project_id: ProjectId) -> Option<RunId> {
        self.running.lock().get(&project_id).map(|h| h.run_id)
    }

    /// Launches a project.
    ///
    /// # Errors
    ///
    /// - [`DeckError::AlreadyRunning`] if a process for this project is live.
    /// - [`DeckError::ProgramNotFound`] if the executable is not on PATH.
    /// - [`DeckError::Io`] if the job object or spawn fails.
    pub fn start(self: &Arc<Self>, request: SpawnRequest) -> Result<RunId> {
        if let Some(existing) = self.running.lock().get(&request.project_id) {
            if let Some(pid) = existing.state.pid() {
                return Err(DeckError::AlreadyRunning {
                    name: request.project_name.clone(),
                    pid,
                });
            }
        }
        self.spawn_inner(request, 0)
    }

    /// Stops a project.
    ///
    /// Returns as soon as the stop is initiated; completion arrives as a
    /// [`SupervisorEvent::StateChanged`]. Escalation to termination happens on a
    /// background task after the grace period.
    ///
    /// # Errors
    ///
    /// Returns [`DeckError::NotRunning`] if there is nothing to stop.
    pub fn stop(self: &Arc<Self>, project_id: ProjectId, mode: StopMode) -> Result<()> {
        let (job, log, run_id, name) = {
            let mut running = self.running.lock();
            let handle = running
                .get_mut(&project_id)
                .filter(|h| h.state.is_live() || h.state.is_transitional())
                .ok_or_else(|| DeckError::NotRunning {
                    name: project_id.to_hyphenated(),
                })?;

            handle.stop_requested = true;
            // Cancel any pending restart: an explicit stop overrides policy.
            handle.restart_attempt = 0;

            let pid = handle.state.pid().unwrap_or_default();
            handle.state = RunState::Stopping { pid };

            // Dropping stdin closes the pipe, which many tools read as "exit".
            handle.stdin.take();

            (
                Arc::clone(&handle.job),
                Arc::clone(&handle.log),
                handle.run_id,
                handle.request.project_name.clone(),
            )
        };

        self.emit(SupervisorEvent::StateChanged {
            project_id,
            run_id,
            state: self.state(project_id),
        });

        let grace_ms = match mode {
            StopMode::Immediate => 0,
            StopMode::Graceful { grace_ms } => grace_ms,
        };

        log.push_deck(if grace_ms == 0 {
            format!("Terminating \"{name}\" immediately")
        } else {
            format!("Stopping \"{name}\" (up to {grace_ms}ms to exit cleanly)")
        });

        let supervisor = Arc::clone(self);
        tokio::spawn(async move {
            let mut reason = "Did not exit within the grace period";
            if grace_ms > 0 {
                // Poll instead of sleeping the whole grace period, so a child
                // that reacts to EOF by SPINNING can be cut short.
                //
                // The grace period exists to let a well-behaved program finish
                // shutting down. A program emitting thousands of lines a second
                // after a stop request is demonstrably not shutting down --
                // waiting out the remaining grace buys nothing and costs a
                // flood. Measured: Circle-Calculator, an interactive C++ menu,
                // read EOF as an invalid menu choice and redrew its menu
                // 4,151,426 times in five seconds -- 208.7 MB on disk -- purely
                // because we asked it to stop and then waited politely.
                let deadline =
                    tokio::time::Instant::now() + std::time::Duration::from_millis(grace_ms);
                let mut last = log.total_count();
                let mut floods = 0u32;
                loop {
                    let tick = std::time::Duration::from_millis(RUNAWAY_WINDOW_MS)
                        .min(deadline.saturating_duration_since(tokio::time::Instant::now()));
                    if tick.is_zero() {
                        break;
                    }
                    tokio::time::sleep(tick).await;

                    let now = log.total_count();
                    let produced = now - last;
                    last = now;

                    if !supervisor
                        .running
                        .lock()
                        .get(&project_id)
                        .is_some_and(|h| matches!(h.state, RunState::Stopping { .. }))
                    {
                        return; // exited cleanly; nothing to terminate
                    }

                    // Two consecutive windows, so one noisy final flush on the
                    // way out -- which a shutting-down program legitimately
                    // produces -- is not mistaken for a loop. Except when the
                    // volume is past anything a flush can be, where confirming
                    // costs megabytes and proves nothing.
                    if produced >= RUNAWAY_LINES_IMMEDIATE {
                        reason = "Flooding output instead of exiting";
                        break;
                    }
                    if produced >= RUNAWAY_LINES_PER_WINDOW {
                        floods += 1;
                        if floods >= 2 {
                            reason = "Flooding output instead of exiting";
                            break;
                        }
                    } else {
                        floods = 0;
                    }
                }
            }

            // Still tracked as stopping? Then it ignored EOF; take the tree down.
            let still_stopping = supervisor
                .running
                .lock()
                .get(&project_id)
                .is_some_and(|h| matches!(h.state, RunState::Stopping { .. }));

            if still_stopping {
                if grace_ms > 0 {
                    log.push_deck(format!("{reason}; terminating process tree"));
                }
                if let Err(e) = job.terminate(1) {
                    tracing::error!(error = %e, "TerminateJobObject failed");
                    log.push_deck(format!("Failed to terminate process tree: {e}"));
                }
            }
        });

        Ok(())
    }

    /// Stops a project if it is running, then starts it again.
    ///
    /// # Errors
    ///
    /// Propagates failures from the stop or the subsequent start.
    pub async fn restart(
        self: &Arc<Self>,
        project_id: ProjectId,
        request: SpawnRequest,
    ) -> Result<RunId> {
        if self.state(project_id).is_live() {
            self.stop(project_id, StopMode::default())?;
            self.await_idle(project_id, std::time::Duration::from_secs(15)).await;
        }
        self.start(request)
    }

    /// Terminates everything. Called when the app exits.
    ///
    /// Best-effort and infallible: a failure to kill one tree must not prevent
    /// the attempt on the rest.
    pub fn shutdown(&self) {
        let handles: Vec<(ProjectId, Arc<JobObject>)> = self
            .running
            .lock()
            .iter()
            .filter(|(_, h)| h.state.is_live())
            .map(|(id, h)| (*id, Arc::clone(&h.job)))
            .collect();

        for (project_id, job) in handles {
            if let Err(e) = job.terminate(1) {
                tracing::warn!(%project_id, error = %e, "failed to terminate job on shutdown");
            }
        }
        self.running.lock().clear();
    }

    /// Waits until a project is no longer live, or `timeout` elapses.
    async fn await_idle(&self, project_id: ProjectId, timeout: std::time::Duration) {
        let deadline = tokio::time::Instant::now() + timeout;
        while tokio::time::Instant::now() < deadline {
            if !self.state(project_id).is_live() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        tracing::warn!(%project_id, "timed out waiting for project to stop");
    }

    fn emit(&self, event: SupervisorEvent) {
        // An error means nobody is subscribed, which is normal at startup.
        let _ = self.events.send(event);
    }

    /// The real spawn, shared by `start` and the restart path.
    fn spawn_inner(self: &Arc<Self>, request: SpawnRequest, attempt: u32) -> Result<RunId> {
        let run_id = RunId::new();

        // Attributed, because `spawn` was one opaque number and this project
        // has been bitten by exactly that before: a total nobody can break
        // down is a log, not a diagnostic. The parts are recorded whether or
        // not anyone is looking, so the next thing added here announces itself.
        let t_log = std::time::Instant::now();
        let log_path = request.log_dir.join(format!("{run_id}.log"));
        let log = Arc::new(LogSink::new(log_path));

        log.push_deck(format!("$ {}", request.command));
        log.push_deck(format!("in {}", request.working_dir.display()));
        let log_ms = t_log.elapsed();

        let t_job = std::time::Instant::now();
        let job = Arc::new(JobObject::new().map_err(|e| {
            DeckError::io(
                format!("creating job object for \"{}\"", request.project_name),
                e,
            )
        })?);
        let job_ms = t_job.elapsed();

        let t_resolve = std::time::Instant::now();
        let built = build_command(&request);
        let resolve_ms = t_resolve.elapsed();

        let t_create = std::time::Instant::now();
        let mut child = match built.and_then(|mut cmd| {
            cmd.spawn().map_err(|e| classify_spawn_error(&request, e))
        }) {
            Ok(child) => child,
            Err(err) => {
                log.push_deck(err.to_string());
                log.flush();
                self.record_failed_start(&request, run_id, log, &err);
                return Err(err);
            }
        };
        // Read HERE, not at the log line further down. The first version of
        // this took `t_create.elapsed()` where it was logged, which is after
        // the job assignment, the resume, and two reader tasks -- so it
        // reported all of that as `CreateProcess` and made a 4 ms syscall look
        // like 447 ms.
        let create_ms = t_create.elapsed();

        let pid = child.id().unwrap_or_default();
        let t_after = std::time::Instant::now();

        // Assign before resuming: this is the whole point of CREATE_SUSPENDED.
        if let Some(handle) = child.raw_handle() {
            if let Err(e) = job.assign(handle) {
                tracing::error!(error = %e, "could not assign child to job object");
                log.push_deck(format!(
                    "Warning: could not place process in a job object ({e}); \
                     child processes may survive a stop"
                ));
            }
        }

        // By handle, not by pid: the handle path resumes the child directly,
        // where the pid path has to enumerate every thread on the machine to
        // find it (33 ms mean here). Falls back automatically if the fast path
        // is unavailable, so a missing symbol costs speed, never a launch.
        let resume = match child.raw_handle() {
            Some(handle) => job::resume_child(handle, pid),
            None => job::resume_process(pid),
        };
        if let Err(e) = resume {
            // The child is suspended and cannot be resumed: it would hang
            // forever, so tear it down and report a failed start.
            let _ = job.terminate(1);
            let err = DeckError::io(format!("resuming \"{}\"", request.project_name), e);
            log.push_deck(err.to_string());
            log.flush();
            self.record_failed_start(&request, run_id, log, &err);
            return Err(err);
        }

        let started_at = chrono::Utc::now();
        let state = RunState::Running {
            pid,
            started_at,
            healthy: None,
        };

        // Reader tasks: one per stream, each pushing into the shared sink.
        if let Some(stdout) = child.stdout.take() {
            spawn_reader(stdout, Arc::clone(&log), LogStream::Stdout);
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_reader(stderr, Arc::clone(&log), LogStream::Stderr);
        }

        let handle = RunHandle {
            run_id,
            lifecycle: request.lifecycle,
            state: state.clone(),
            job: Arc::clone(&job),
            log: Arc::clone(&log),
            stdin: child.stdin.take(),
            stop_requested: false,
            restart_attempt: attempt,
            request: request.clone(),
        };

        self.running.lock().insert(request.project_id, handle);

        tracing::info!(
            project = %request.project_name,
            pid,
            %run_id,
            command = %request.command,
            log_us = log_ms.as_micros(),
            job_us = job_ms.as_micros(),
            resolve_us = resolve_ms.as_micros(),
            create_us = create_ms.as_micros(),
            after_us = t_after.elapsed().as_micros(),
            "started"
        );

        self.emit(SupervisorEvent::StateChanged {
            project_id: request.project_id,
            run_id,
            state,
        });

        // Waiter: owns the child, awaits exit, then decides what happens next.
        let supervisor = Arc::clone(self);
        tokio::spawn(async move {
            supervisor.watch(child, request, run_id, job, log).await;
        });

        Ok(run_id)
    }

    /// Records a start that never produced a process.
    fn record_failed_start(
        &self,
        request: &SpawnRequest,
        run_id: RunId,
        log: Arc<LogSink>,
        err: &DeckError,
    ) {
        let state = RunState::Crashed {
            code: None,
            at: chrono::Utc::now(),
            reason: Some(err.to_string()),
        };
        // Keep the handle so the UI can show why it failed and read the log.
        self.running.lock().insert(
            request.project_id,
            RunHandle {
                run_id,
                lifecycle: request.lifecycle,
                state: state.clone(),
                job: Arc::new(match JobObject::new() {
                    Ok(j) => j,
                    Err(_) => return,
                }),
                log,
                stdin: None,
                stop_requested: false,
                restart_attempt: 0,
                request: request.clone(),
            },
        );
        self.emit(SupervisorEvent::StateChanged {
            project_id: request.project_id,
            run_id,
            state,
        });
        self.emit(SupervisorEvent::RunFinished {
            project_id: request.project_id,
            run_id,
            outcome: RunOutcome::FailedToStart,
            exit_code: None,
        });
    }

    /// Awaits a child's exit and applies restart policy.
    async fn watch(
        self: Arc<Self>,
        mut child: tokio::process::Child,
        request: SpawnRequest,
        run_id: RunId,
        job: Arc<JobObject>,
        log: Arc<LogSink>,
    ) {
        let status = child.wait().await;
        let exit_code = status.as_ref().ok().and_then(std::process::ExitStatus::code);
        let at = chrono::Utc::now();

        let (stop_requested, attempt) = {
            let running = self.running.lock();
            running
                .get(&request.project_id)
                .filter(|h| h.run_id == run_id)
                .map_or((false, 0), |h| (h.stop_requested, h.restart_attempt))
        };

        // A long-running step exiting on its own is a crash even with code 0: a
        // dev server that returns is not a dev server that succeeded. A task step
        // (build, install, test) exiting 0 genuinely succeeded.
        let clean_exit = exit_code == Some(0);
        let outcome = if stop_requested {
            if clean_exit {
                RunOutcome::Stopped
            } else {
                RunOutcome::Killed
            }
        } else if clean_exit && !request.lifecycle.is_long_running() {
            RunOutcome::Succeeded
        } else if clean_exit {
            RunOutcome::Stopped
        } else {
            RunOutcome::Failed
        };

        let reason = detect_exit_reason(&log, exit_code);

        let state = if outcome.is_failure() {
            RunState::Crashed {
                code: exit_code,
                at,
                reason: reason.clone(),
            }
        } else {
            RunState::Exited { code: exit_code, at }
        };

        log.push_deck(match exit_code {
            Some(code) => format!("Exited with code {code}"),
            None => "Exited without a status code (terminated)".to_owned(),
        });
        if let Some(r) = &reason {
            log.push_deck(format!("Likely cause: {r}"));
        }
        log.flush();

        // Prune this project's older run logs.
        //
        // `prune_logs` has documented itself as "called after a run finishes"
        // since it was written, and was called from nowhere but its own tests.
        // The result was exactly the slow leak its own doc comment warns
        // about: 30-odd log files accumulating over a week on this machine,
        // including a 208.7 MB one from a program that got stuck in an output
        // loop. Retention nobody invokes is not retention.
        if let Some(dir) = log.path().parent() {
            match crate::logs::prune_logs(dir, LOGS_KEPT_PER_PROJECT) {
                Ok(0) => {}
                Ok(n) => tracing::debug!(removed = n, dir = %dir.display(), "pruned old run logs"),
                Err(e) => {
                    // Never fatal: losing housekeeping must not affect the run
                    // that just finished, or the next one.
                    tracing::warn!(error = %e, dir = %dir.display(), "could not prune run logs");
                }
            }
        }

        tracing::info!(
            project = %request.project_name,
            %run_id,
            ?exit_code,
            ?outcome,
            "run finished"
        );

        if let Some(handle) = self.running.lock().get_mut(&request.project_id) {
            if handle.run_id == run_id {
                handle.state = state.clone();
                handle.stdin = None;
            }
        }

        self.emit(SupervisorEvent::StateChanged {
            project_id: request.project_id,
            run_id,
            state,
        });
        self.emit(SupervisorEvent::RunFinished {
            project_id: request.project_id,
            run_id,
            outcome,
            exit_code,
        });

        // Closing the job releases anything the child left behind.
        drop(job);

        if stop_requested {
            return;
        }
        self.maybe_restart(request, log, outcome, exit_code, attempt).await;
    }

    /// Applies restart policy, with backoff and a circuit breaker.
    async fn maybe_restart(
        self: &Arc<Self>,
        request: SpawnRequest,
        log: Arc<LogSink>,
        outcome: RunOutcome,
        exit_code: Option<i32>,
        previous_attempt: u32,
    ) {
        if !request.restart_policy.should_restart(exit_code) {
            return;
        }
        let Some(max) = request.restart_policy.max_attempts() else {
            return;
        };

        // A run that succeeded resets the counter; only failures accumulate.
        let attempt = if outcome.is_failure() {
            previous_attempt + 1
        } else {
            1
        };

        if attempt > max {
            let err = DeckError::RestartLoopBrokenOut {
                name: request.project_name.clone(),
                attempts: previous_attempt,
            };
            tracing::warn!(project = %request.project_name, attempts = previous_attempt, "restart circuit breaker tripped");
            log.push_deck(err.to_string());
            log.flush();
            return;
        }

        let delay = RestartPolicy::backoff_ms(attempt);
        log.push_deck(format!(
            "Restarting in {delay}ms (attempt {attempt} of {max})"
        ));

        // `backoff_ms` is capped at 30s, so this conversion is always exact.
        let next_attempt_at =
            chrono::Utc::now() + chrono::Duration::milliseconds(i64::try_from(delay).unwrap_or(30_000));
        let state = RunState::Restarting {
            attempt,
            next_attempt_at,
        };
        if let Some(handle) = self.running.lock().get_mut(&request.project_id) {
            handle.state = state.clone();
            handle.restart_attempt = attempt;
        }
        self.emit(SupervisorEvent::StateChanged {
            project_id: request.project_id,
            run_id: RunId::new(),
            state,
        });

        tokio::time::sleep(std::time::Duration::from_millis(delay)).await;

        // An explicit stop during the backoff wins.
        let cancelled = self
            .running
            .lock()
            .get(&request.project_id)
            .is_some_and(|h| h.stop_requested);
        if cancelled {
            return;
        }

        if let Err(e) = self.spawn_inner(request.clone(), attempt) {
            tracing::warn!(project = %request.project_name, error = %e, "restart failed");
        }
    }
}

/// Builds the child process command for a spawn request.
///
/// All three pipes are captured, and the creation flags are always
/// [`job::SPAWN_FLAGS`] -- suspended, own process group, no console window. None of
/// those is optional: see [`crate::job`].
///
/// The program is resolved through [`crate::program`] first. That is what makes
/// `npm`, `pnpm`, `yarn` and the rest of the `.cmd`-shim tool surface launchable
/// at all: Windows cannot execute a batch file directly, so those are routed
/// through `cmd.exe /c`.
///
/// # Errors
///
/// Returns [`DeckError::ProgramNotFound`] if the program cannot be found, which
/// is reported before anything is spawned.
fn build_command(request: &SpawnRequest) -> Result<tokio::process::Command> {
    let resolved = crate::program::resolve(&request.command.program).ok_or_else(|| {
        DeckError::ProgramNotFound {
            program: request.command.program.clone(),
        }
    })?;

    let mut cmd = match &resolved {
        crate::program::Invocation::Direct(path) => {
            let mut c = tokio::process::Command::new(path);
            c.args(&request.command.args);
            c
        }
        crate::program::Invocation::ViaShell(path) => {
            // `cmd /c <script> <args>`: each piece stays a separate argv entry
            // so quoting is handled once, by the standard library.
            let mut c = tokio::process::Command::new("cmd.exe");
            c.arg("/c").arg(path).args(&request.command.args);
            c
        }
    };

    cmd.current_dir(&request.working_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .creation_flags(job::SPAWN_FLAGS)
        // Reaped explicitly by the waiter task, not on drop.
        .kill_on_drop(false);

    for (key, value) in &request.env {
        cmd.env(key, value);
    }
    Ok(cmd)
}

/// Turns a spawn failure into an error the UI can act on.
///
/// "Not found" is by far the most common and the most fixable, so it gets its own
/// variant and its own message rather than surfacing as a generic I/O error.
fn classify_spawn_error(request: &SpawnRequest, error: std::io::Error) -> DeckError {
    if error.kind() == std::io::ErrorKind::NotFound {
        DeckError::ProgramNotFound {
            program: request.command.program.clone(),
        }
    } else {
        DeckError::io(format!("spawning \"{}\"", request.command.program), error)
    }
}

/// Reads a stream line by line into a sink.
///
/// Uses `read_until(b'\n')` with lossy UTF-8 decoding rather than a `Lines`
/// iterator: build tools emit invalid UTF-8 (and lone `\r` progress bars), and a
/// strict decoder would drop the rest of the stream on the first bad byte.
fn spawn_reader<R>(reader: R, log: Arc<LogSink>, stream: LogStream)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut buf_reader = BufReader::new(reader);
        let mut buffer = Vec::with_capacity(256);
        loop {
            buffer.clear();
            match buf_reader.read_until(b'\n', &mut buffer).await {
                Ok(0) => break,
                Ok(_) => {
                    while matches!(buffer.last(), Some(b'\n' | b'\r')) {
                        buffer.pop();
                    }
                    log.push(stream, String::from_utf8_lossy(&buffer).into_owned());
                }
                Err(e) => {
                    tracing::debug!(error = %e, ?stream, "stream read ended");
                    break;
                }
            }
        }
    });
}

/// Guesses why a run failed, from its own output.
///
/// A bare "exit code 1" tells the user nothing. Port conflicts and missing
/// modules are the two overwhelmingly common causes, and both announce
/// themselves in the log. This only ever adds an explanation -- it never changes
/// the exit status or hides a line.
fn detect_exit_reason(log: &LogSink, exit_code: Option<i32>) -> Option<String> {
    if exit_code == Some(0) {
        return None;
    }

    // Only the tail matters; the failure is at the end.
    let tail = log.snapshot();
    let recent = tail.iter().rev().take(60);

    for line in recent {
        let text = deck_domain::log::strip_ansi(&line.text).to_ascii_lowercase();

        if text.contains("eaddrinuse")
            || text.contains("address already in use")
            || text.contains("only one usage of each socket address")
            || (text.contains("port") && text.contains("already in use"))
        {
            let port = extract_port(&text);
            return Some(match port {
                Some(p) => format!("port {p} is already in use"),
                None => "a port it needs is already in use".to_owned(),
            });
        }

        // Python says "No module named 'mutagen'", which matches none of the
        // Node phrasings below and so went undiagnosed -- the two most common
        // Python failures in this library both landed as a bare "Exited with
        // code 1". Naming the module is the difference between a diagnosis and
        // an observation: `pip install mutagen` is a command, "a dependency is
        // missing" is a shrug.
        if text.contains("no module named") {
            return Some(match quoted_after(&text, "no module named") {
                Some(module) => format!("the Python module `{module}` is not installed"),
                None => "a Python module it imports is not installed".to_owned(),
            });
        }
        if text.contains("cannot find module") || text.contains("module not found") {
            return Some("a dependency is missing -- try Install".to_owned());
        }
        if text.contains("command not found") || text.contains("is not recognized as") {
            return Some("the command is not installed or not on PATH".to_owned());
        }
        if text.contains("permission denied") || text.contains("access is denied") {
            return Some("permission denied".to_owned());
        }
        if text.contains("out of memory") || text.contains("heap out of memory") {
            return Some("the process ran out of memory".to_owned());
        }
    }

    None
}

/// The quoted word following `marker`, if there is one.
///
/// Python quotes the module name (`No module named 'mutagen'`), which is the
/// only part of that message worth repeating back.
fn quoted_after(text: &str, marker: &str) -> Option<String> {
    let rest = text.split_once(marker)?.1;
    let start = rest.find(['\'', '"'])?;
    let quote = rest.as_bytes()[start] as char;
    let inner = &rest[start + 1..];
    let end = inner.find(quote)?;
    let name = inner[..end].trim();
    // A submodule failure names the whole path (`a.b.c`); the installable
    // package is the first segment.
    let name = name.split('.').next().unwrap_or(name);
    (!name.is_empty() && name.len() <= 64).then(|| name.to_owned())
}

/// Pulls a port number out of an error line.
fn extract_port(text: &str) -> Option<u16> {
    // Matches the ":3000" in "127.0.0.1:3000" and the "5173" in "port 5173".
    let mut digits = String::new();
    let mut best: Option<u16> = None;
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else {
            if digits.len() >= 2 {
                if let Ok(p) = digits.parse::<u16>() {
                    // Prefer a plausible service port over an arbitrary number.
                    if p >= 80 {
                        best = Some(p);
                    }
                }
            }
            digits.clear();
        }
    }
    if digits.len() >= 2 {
        if let Ok(p) = digits.parse::<u16>() {
            if p >= 80 {
                best = Some(p);
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let mut p = std::env::temp_dir();
        p.push(format!("deck-sup-{tag}-{nanos:x}"));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn request(dir: &Path, args: &[&str]) -> SpawnRequest {
        SpawnRequest {
            runner_id: "test".into(),
            expected_ports: Vec::new(),
            project_id: ProjectId::new(),
            project_name: "Test Project".to_owned(),
            lifecycle: Lifecycle::Run,
            command: ResolvedCommand {
                program: "cmd.exe".to_owned(),
                args: args.iter().map(|s| (*s).to_owned()).collect(),
            },
            working_dir: dir.to_path_buf(),
            env: Vec::new(),
            restart_policy: RestartPolicy::Never,
            log_dir: dir.join("logs"),
        }
    }

    /// Waits for a predicate on the project's state, or panics on timeout.
    async fn wait_for(
        sup: &Arc<Supervisor>,
        id: ProjectId,
        label: &str,
        pred: impl Fn(&RunState) -> bool,
    ) -> RunState {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            let state = sup.state(id);
            if pred(&state) {
                return state;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for {label}; state is {}",
                state.tag()
            );
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    }

    #[tokio::test]
    async fn captures_stdout_and_reports_success() {
        let dir = scratch("stdout");
        let sup = Supervisor::new();
        let mut req = request(&dir, &["/c", "echo hello from the child"]);
        // A task step, not a server: exiting 0 is success.
        req.lifecycle = Lifecycle::Build;
        let id = req.project_id;

        sup.start(req).unwrap();
        wait_for(&sup, id, "exit", |s| {
            matches!(s, RunState::Exited { .. } | RunState::Crashed { .. })
        })
        .await;

        let log = sup.logs(id).unwrap();
        let text: String = log.snapshot().iter().map(|l| l.text.clone()).collect::<Vec<_>>().join("\n");
        assert!(text.contains("hello from the child"), "log was: {text}");
        assert!(matches!(sup.state(id), RunState::Exited { code: Some(0), .. }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn captures_stderr_separately_from_stdout() {
        let dir = scratch("stderr");
        let sup = Supervisor::new();
        let mut req = request(&dir, &["/c", "echo to-err 1>&2"]);
        req.lifecycle = Lifecycle::Build;
        let id = req.project_id;

        sup.start(req).unwrap();
        wait_for(&sup, id, "exit", |s| !s.is_live() && !s.is_transitional()).await;

        let log = sup.logs(id).unwrap();
        let err_lines: Vec<_> = log
            .snapshot()
            .into_iter()
            .filter(|l| l.stream == LogStream::Stderr)
            .collect();
        assert!(
            err_lines.iter().any(|l| l.text.contains("to-err")),
            "stderr not captured on the stderr stream"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_nonzero_exit_is_reported_as_a_crash() {
        let dir = scratch("fail");
        let sup = Supervisor::new();
        let mut req = request(&dir, &["/c", "exit 3"]);
        req.lifecycle = Lifecycle::Build;
        let id = req.project_id;

        sup.start(req).unwrap();
        let state = wait_for(&sup, id, "crash", |s| {
            matches!(s, RunState::Crashed { .. } | RunState::Exited { .. })
        })
        .await;
        assert!(
            matches!(state, RunState::Crashed { code: Some(3), .. }),
            "expected crash with code 3, got {state:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_long_running_step_exiting_cleanly_is_not_a_success() {
        // A dev server that returns 0 has stopped serving, which is not success.
        let dir = scratch("longrun");
        let sup = Supervisor::new();
        let req = request(&dir, &["/c", "echo starting"]); // Lifecycle::Run
        let id = req.project_id;
        let mut events = sup.subscribe();

        sup.start(req).unwrap();
        wait_for(&sup, id, "exit", |s| !s.is_live() && !s.is_transitional()).await;

        let mut outcome = None;
        while let Ok(event) = events.try_recv() {
            if let SupervisorEvent::RunFinished { outcome: o, .. } = event {
                outcome = Some(o);
            }
        }
        assert_eq!(outcome, Some(RunOutcome::Stopped));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn refuses_to_start_a_project_twice() {
        let dir = scratch("double");
        let sup = Supervisor::new();
        let req = request(&dir, &["/c", "ping -n 30 127.0.0.1 >nul"]);
        let id = req.project_id;

        sup.start(req.clone()).unwrap();
        wait_for(&sup, id, "running", RunState::is_live).await;

        let err = sup.start(req).unwrap_err();
        assert_eq!(err.code(), "already_running");

        sup.stop(id, StopMode::Immediate).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn stop_terminates_a_process_that_ignores_eof() {
        let dir = scratch("stop");
        let sup = Supervisor::new();
        let req = request(&dir, &["/c", "ping -n 60 127.0.0.1 >nul"]);
        let id = req.project_id;

        sup.start(req).unwrap();
        wait_for(&sup, id, "running", RunState::is_live).await;

        sup.stop(id, StopMode::Graceful { grace_ms: 300 }).unwrap();
        let state = wait_for(&sup, id, "stopped", |s| !s.is_live()).await;
        assert!(
            matches!(state, RunState::Exited { .. } | RunState::Crashed { .. }),
            "expected a terminal state, got {state:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn stopping_is_reported_as_stopped_not_crashed() {
        let dir = scratch("stopclean");
        let sup = Supervisor::new();
        let req = request(&dir, &["/c", "ping -n 60 127.0.0.1 >nul"]);
        let id = req.project_id;
        let mut events = sup.subscribe();

        sup.start(req).unwrap();
        wait_for(&sup, id, "running", RunState::is_live).await;
        sup.stop(id, StopMode::Immediate).unwrap();
        wait_for(&sup, id, "stopped", |s| !s.is_live()).await;

        let mut outcomes = Vec::new();
        while let Ok(event) = events.try_recv() {
            if let SupervisorEvent::RunFinished { outcome, .. } = event {
                outcomes.push(outcome);
            }
        }
        // A user-requested stop must never be filed as a failure, or crash
        // history fills with intentional stops.
        assert!(
            outcomes.iter().all(|o| !o.is_failure()),
            "a requested stop was recorded as a failure: {outcomes:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn stopping_something_not_running_is_an_error() {
        let sup = Supervisor::new();
        let err = sup.stop(ProjectId::new(), StopMode::default()).unwrap_err();
        assert_eq!(err.code(), "not_running");
    }

    #[tokio::test]
    async fn launches_a_real_npm_script_through_the_shell_shim() {
        // The regression that mattered most: npm is a .cmd on Windows, so
        // `Command::new("npm")` fails with "not found" on a machine where npm
        // works everywhere else. Every Node project depends on this path.
        if crate::program::resolve("npm").is_none() {
            eprintln!("npm not installed; skipping");
            return;
        }

        let dir = scratch("npm");
        std::fs::write(
            dir.join("package.json"),
            r#"{"name":"t","version":"1.0.0","scripts":{"hello":"node -e \"console.log('npm-shim-works')\""}}"#,
        )
        .unwrap();

        let sup = Supervisor::new();
        let mut req = request(&dir, &[]);
        req.command = ResolvedCommand {
            program: "npm".to_owned(),
            args: vec!["run".to_owned(), "hello".to_owned(), "--silent".to_owned()],
        };
        req.lifecycle = Lifecycle::Build; // a task: exit 0 is success
        let id = req.project_id;

        sup.start(req).expect("npm should launch via the shell shim");
        wait_for(&sup, id, "exit", |s| !s.is_live() && !s.is_transitional()).await;

        let log = sup.logs(id).unwrap();
        let text: String = log
            .snapshot()
            .iter()
            .map(|l| l.text.clone())
            .collect::<Vec<_>>()
            .join("
");
        assert!(
            text.contains("npm-shim-works"),
            "npm script produced no output; log was:
{text}"
        );
        assert!(
            matches!(sup.state(id), RunState::Exited { code: Some(0), .. }),
            "expected a clean exit, got {:?}",
            sup.state(id)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_missing_program_reports_program_not_found() {
        let dir = scratch("missing");
        let sup = Supervisor::new();
        let mut req = request(&dir, &[]);
        req.command.program = "definitely-not-a-real-program-xyz.exe".to_owned();
        let id = req.project_id;

        let err = sup.start(req).unwrap_err();
        assert_eq!(err.code(), "program_not_found");
        // The failure is visible in state and in the log, not just returned.
        assert!(matches!(sup.state(id), RunState::Crashed { .. }));
        let log = sup.logs(id).unwrap();
        assert!(log.snapshot().iter().any(|l| l.text.contains("not found")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn environment_variables_reach_the_child() {
        let dir = scratch("env");
        let sup = Supervisor::new();
        let mut req = request(&dir, &["/c", "echo VALUE=%DECK_TEST_VAR%"]);
        req.lifecycle = Lifecycle::Build;
        req.env = vec![("DECK_TEST_VAR".to_owned(), "propagated".to_owned())];
        let id = req.project_id;

        sup.start(req).unwrap();
        wait_for(&sup, id, "exit", |s| !s.is_live() && !s.is_transitional()).await;

        let log = sup.logs(id).unwrap();
        let text: String = log.snapshot().iter().map(|l| l.text.clone()).collect::<Vec<_>>().join("\n");
        assert!(text.contains("VALUE=propagated"), "log was: {text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn the_working_directory_is_honoured() {
        let dir = scratch("cwd");
        let sub = dir.join("nested");
        std::fs::create_dir_all(&sub).unwrap();
        let sup = Supervisor::new();
        let mut req = request(&sub, &["/c", "cd"]); // `cd` with no args prints cwd
        req.lifecycle = Lifecycle::Build;
        let id = req.project_id;

        sup.start(req).unwrap();
        wait_for(&sup, id, "exit", |s| !s.is_live() && !s.is_transitional()).await;

        let log = sup.logs(id).unwrap();
        let text: String = log.snapshot().iter().map(|l| l.text.clone()).collect::<Vec<_>>().join("\n");
        assert!(text.contains("nested"), "log was: {text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn the_command_is_recorded_at_the_top_of_the_log() {
        let dir = scratch("cmdlog");
        let sup = Supervisor::new();
        let mut req = request(&dir, &["/c", "echo x"]);
        req.lifecycle = Lifecycle::Build;
        let id = req.project_id;

        sup.start(req).unwrap();
        wait_for(&sup, id, "exit", |s| !s.is_live() && !s.is_transitional()).await;

        let log = sup.logs(id).unwrap();
        let first = &log.snapshot()[0];
        assert_eq!(first.stream, LogStream::Deck);
        assert!(first.text.starts_with("$ cmd.exe"), "got {:?}", first.text);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A child that answers a stop request by flooding is killed at once, not
    /// after the full grace period.
    ///
    /// The reported bug: Circle-Calculator, an interactive C++ menu, treated
    /// its closed stdin as an invalid menu choice and redrew its menu
    /// 4,151,426 times in the five seconds we spent politely waiting -- 208.7 MB
    /// on disk. The grace period is for programs that are shutting down. One
    /// producing thousands of lines a second is not, and waiting out the rest
    /// of the grace buys nothing.
    ///
    /// The fixture is a batch loop printing as fast as `cmd` can, which is the
    /// same shape as the real case: output that only starts after the stop.
    #[tokio::test]
    async fn a_child_that_floods_instead_of_exiting_is_cut_short() {
        let dir = scratch("flood");
        let sup = Supervisor::new();
        let req = request(&dir, &["/c", "for /L %i in (1,1,100000000) do @echo spam"]);
        let id = req.project_id;

        sup.start(req).unwrap();
        wait_for(&sup, id, "running", RunState::is_live).await;

        // A grace period far longer than the test should take, so passing
        // cannot mean "the grace expired" -- only "the flood was detected".
        let grace_ms = 30_000;
        let began = std::time::Instant::now();
        sup.stop(id, StopMode::Graceful { grace_ms }).unwrap();

        wait_for(&sup, id, "stopped", |s| !s.is_live()).await;
        let took = began.elapsed();

        assert!(
            took < std::time::Duration::from_millis(grace_ms / 2),
            "took {took:?} of a {grace_ms}ms grace period -- the flood was not detected"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn shutdown_terminates_everything_running() {
        let dir = scratch("shutdown");
        let sup = Supervisor::new();
        let a = request(&dir, &["/c", "ping -n 60 127.0.0.1 >nul"]);
        let b = request(&dir, &["/c", "ping -n 60 127.0.0.1 >nul"]);
        let (ida, idb) = (a.project_id, b.project_id);

        sup.start(a).unwrap();
        sup.start(b).unwrap();
        wait_for(&sup, ida, "a running", RunState::is_live).await;
        wait_for(&sup, idb, "b running", RunState::is_live).await;

        sup.shutdown();
        assert!(sup.states().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn auto_restart_stops_at_the_attempt_ceiling() {
        // The crash-loop guard: a project that always fails must stop being
        // restarted rather than looping forever.
        let dir = scratch("loop");
        let sup = Supervisor::new();
        let mut req = request(&dir, &["/c", "exit 1"]);
        req.restart_policy = RestartPolicy::OnFailure { max_attempts: 2 };
        let id = req.project_id;

        sup.start(req).unwrap();

        // Wait for the breaker message rather than a fixed sleep.
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let tripped = sup.logs(id).is_some_and(|log| {
                log.snapshot()
                    .iter()
                    .any(|l| l.text.contains("auto-restart disabled"))
            });
            if tripped {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "circuit breaker never tripped"
            );
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        // And it stays stopped.
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
        assert!(!sup.state(id).is_live());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn never_policy_does_not_restart() {
        let dir = scratch("norestart");
        let sup = Supervisor::new();
        let mut req = request(&dir, &["/c", "exit 1"]);
        req.restart_policy = RestartPolicy::Never;
        let id = req.project_id;

        sup.start(req).unwrap();
        wait_for(&sup, id, "crash", |s| matches!(s, RunState::Crashed { .. })).await;
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        assert!(matches!(sup.state(id), RunState::Crashed { .. }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- Exit-reason heuristics -------------------------------------------

    #[test]
    fn port_conflicts_are_named_with_the_port() {
        let dir = scratch("reason-port");
        let log = LogSink::new(dir.join("r.log"));
        log.push(LogStream::Stderr, "Error: listen EADDRINUSE: address already in use :::5173");
        let reason = detect_exit_reason(&log, Some(1)).unwrap();
        assert!(reason.contains("5173"), "got {reason}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn windows_socket_error_wording_is_recognised() {
        let dir = scratch("reason-winsock");
        let log = LogSink::new(dir.join("r.log"));
        log.push(
            LogStream::Stderr,
            "Only one usage of each socket address (protocol/network address/port) is normally permitted. 127.0.0.1:3000",
        );
        assert!(detect_exit_reason(&log, Some(1)).unwrap().contains("3000"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_dependencies_suggest_install() {
        let dir = scratch("reason-mod");
        let log = LogSink::new(dir.join("r.log"));
        log.push(LogStream::Stderr, "Error: Cannot find module 'express'");
        assert!(detect_exit_reason(&log, Some(1)).unwrap().contains("Install"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The two most common Python failures in this library both landed as a
    /// bare "Exited with code 1", because every existing rule matched Node's
    /// phrasing and Python says something else entirely.
    #[test]
    fn a_missing_python_module_is_named() {
        let dir = scratch("reason-py");
        let log = LogSink::new(dir.join("r.log"));
        log.push(
            LogStream::Stderr,
            "ModuleNotFoundError: No module named 'mutagen'",
        );
        let reason = detect_exit_reason(&log, Some(1)).expect("should diagnose");
        assert!(reason.contains("mutagen"), "got {reason}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A submodule failure names the whole dotted path, but the thing you
    /// install is the first segment -- `pip install google.protobuf` is not a
    /// command that works.
    #[test]
    fn a_submodule_failure_reports_the_installable_package() {
        let dir = scratch("reason-sub");
        let log = LogSink::new(dir.join("r.log"));
        log.push(
            LogStream::Stderr,
            "ModuleNotFoundError: No module named 'google.protobuf'",
        );
        let reason = detect_exit_reason(&log, Some(1)).expect("should diagnose");
        assert!(reason.contains("google"), "got {reason}");
        assert!(!reason.contains("protobuf"), "should not name a submodule: {reason}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_clean_exit_gets_no_diagnosis() {
        let dir = scratch("reason-clean");
        let log = LogSink::new(dir.join("r.log"));
        log.push(LogStream::Stdout, "address already in use somewhere in prose");
        assert!(detect_exit_reason(&log, Some(0)).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unrecognised_failure_gets_no_invented_reason() {
        let dir = scratch("reason-none");
        let log = LogSink::new(dir.join("r.log"));
        log.push(LogStream::Stderr, "something went wrong in a way we do not model");
        assert!(detect_exit_reason(&log, Some(1)).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn port_extraction_ignores_small_numbers() {
        assert_eq!(extract_port("port 5173 in use"), Some(5173));
        assert_eq!(extract_port("listen on :::3000"), Some(3000));
        assert_eq!(extract_port("error 1 of 2"), None);
    }
}
