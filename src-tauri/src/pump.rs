//! Event pumps: the bridge from supervisor broadcasts to Tauri events.
//!
//! Two pumps with two different shapes, because the traffic differs:
//!
//! * **State changes** are rare (a few per run) and land immediately -- latency
//!   matters, volume does not.
//! * **Log lines** arrive in the thousands per second. One Tauri event per line
//!   saturates the IPC bridge, so lines COALESCE: a batch flushes every ~60 ms,
//!   or sooner when it grows past a size bound. 60 ms is under two frames --
//!   visually "live" -- while cutting event volume by orders of magnitude.

use std::collections::HashSet;
use std::sync::Arc;

use chrono::Utc;
use deck_domain::project::ProjectId;
use deck_runtime::SupervisorEvent;
use parking_lot::Mutex;
use tauri::{AppHandle, Emitter, Manager};

use crate::dto::{
    outcome_wire, LogLineDto, LogsEvent, MetricsDto, MetricsEvent, RunFinishedEvent,
    RunStateDto, StateEvent,
};
use crate::AppState;

/// Flush interval for coalesced log batches.
const FLUSH_MS: u64 = 60;
/// A batch this large flushes immediately rather than waiting for the tick.
const FLUSH_LINES: usize = 400;

/// Guards against two pumps for the same project's sink.
///
/// `start` and `restart` both ask for a pump; the set makes the second ask a
/// no-op while the first is still draining the same sink.
#[derive(Default)]
pub struct PumpRegistry {
    active: Mutex<HashSet<ProjectId>>,
}

impl PumpRegistry {
    /// How many log pumps are currently running.
    ///
    /// Diagnostics compares this against the number of live processes: a pump
    /// without a process is a leaked task and a channel, and fewer pumps than
    /// processes means someone's output is going nowhere.
    pub fn len(&self) -> usize {
        self.active.lock().len()
    }

    /// True when no log pump is running.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Forwards supervisor state changes to the frontend and closes out run rows.
///
/// One global task for the app's lifetime.
pub fn spawn_state_pump(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut rx = app.state::<AppState>().supervisor.subscribe();

        loop {
            let event = {
                match rx.recv().await {
                    Ok(event) => event,
                    // A lagged receiver missed low-frequency events; resync from
                    // the supervisor's current truth rather than giving up.
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            };

            match event {
                SupervisorEvent::StateChanged {
                    project_id,
                    run_id,
                    state: run_state,
                } => {
                    let _ = app.emit(
                        "deck://state",
                        StateEvent {
                            project_id: project_id.to_hyphenated(),
                            run_id: run_id.to_string(),
                            state: RunStateDto::from(&run_state),
                        },
                    );
                    // The window can be hidden in the tray, in which case the
                    // tooltip is the ONLY surface saying what is still running.
                    // Refreshing it here keeps it truthful for free -- this
                    // fires exactly when the count can have changed.
                    crate::tray::refresh_tooltip(&app);
                }
                SupervisorEvent::RunFinished {
                    project_id,
                    run_id,
                    outcome,
                    exit_code,
                } => {
                    let state = app.state::<AppState>();
                    if let Err(e) = state
                        .store
                        .finish_run(run_id, Utc::now(), exit_code, outcome)
                        .await
                    {
                        tracing::warn!(error = %e, %run_id, "could not close out run row");
                    }
                    state.sampler.forget(project_id);
                    let _ = app.emit(
                        "deck://run-finished",
                        RunFinishedEvent {
                            project_id: project_id.to_hyphenated(),
                            run_id: run_id.to_string(),
                            outcome: outcome_wire(outcome),
                            exit_code,
                        },
                    );
                }
            }
        }
    });
}

/// Samples every running project once a second and emits one batched event.
///
/// One task for the whole app rather than one per project: refreshing the
/// process table is the expensive part of sampling, so it happens once per tick
/// and every project reads from that snapshot. The event carries all samples
/// together, which keeps the IPC cost flat as the number of running projects
/// grows.
///
/// Emits nothing when nothing is running, so an idle Launch Deck is genuinely
/// idle rather than waking the webview every second.
pub fn spawn_metrics_pump(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(deck_runtime::metrics::INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tick.tick().await;

            let state = app.state::<AppState>();
            let live: Vec<(deck_domain::project::ProjectId, u32, chrono::DateTime<chrono::Utc>)> =
                state
                    .supervisor
                    .states()
                    .into_iter()
                    .filter_map(|(id, run_state)| match run_state {
                        deck_domain::runtime::RunState::Running {
                            pid, started_at, ..
                        } => Some((id, pid, started_at)),
                        _ => None,
                    })
                    .collect();

            if live.is_empty() {
                continue;
            }

            // Refresh once, then attribute to each project from that snapshot.
            state.sampler.refresh();

            let mut samples = Vec::with_capacity(live.len());
            for (project_id, pid, started_at) in live {
                let run_id = state
                    .supervisor
                    .current_run(project_id)
                    .unwrap_or_else(deck_domain::runtime::RunId::new);
                if let Some(snapshot) =
                    state.sampler.sample(project_id, run_id, pid, started_at)
                {
                    // Readiness rides on the sample that was taken anyway. The
                    // listening ports are already in it, which is a stronger
                    // signal than an HTTP probe and costs nothing extra.
                    evaluate_health(&app, &state, project_id, &snapshot.listening_ports);
                    samples.push(MetricsDto::from(&snapshot));
                }
            }

            if !samples.is_empty() {
                let _ = app.emit("deck://metrics", MetricsEvent { samples });
            }
        }
    });
}

/// Streams a project's log sink to the frontend in coalesced batches.
///
/// Ends when the sink closes (the run's handle was replaced by a newer run) --
/// at which point the project is deregistered from the pump set so the next
/// run gets a fresh pump.
pub fn spawn_log_pump(app: &AppHandle, state: &AppState, project_id: ProjectId) {
    if !state.pumps.active.lock().insert(project_id) {
        return; // already pumping this project
    }
    let Some(sink) = state.supervisor.logs(project_id) else {
        state.pumps.active.lock().remove(&project_id);
        return;
    };

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        pump_sink(&app, project_id, &sink).await;
        let state = app.state::<AppState>();
        state.pumps.active.lock().remove(&project_id);
    });
}

async fn pump_sink(app: &AppHandle, project_id: ProjectId, sink: &Arc<deck_runtime::LogSink>) {
    let mut rx = sink.subscribe();
    let mut batch: Vec<LogLineDto> = Vec::new();
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(FLUSH_MS));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let flush = |batch: &mut Vec<LogLineDto>| {
        if batch.is_empty() {
            return;
        }
        let _ = app.emit(
            "deck://logs",
            LogsEvent {
                project_id: project_id.to_hyphenated(),
                lines: std::mem::take(batch),
            },
        );
    };

    loop {
        tokio::select! {
            received = rx.recv() => match received {
                Ok(line) => {
                    batch.push(LogLineDto::from(&line));
                    if batch.len() >= FLUSH_LINES {
                        flush(&mut batch);
                    }
                }
                // Lagging is survivable: the viewer heals the gap from the ring
                // via `get_logs(after)`; sequence numbers make the gap visible.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {},
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    flush(&mut batch);
                    break;
                }
            },
            _ = tick.tick() => flush(&mut batch),
        }
    }
}

/// Decides whether a running project is actually serving, and emits on change.
///
/// A project is marked Running the instant its process spawns -- 11 ms after
/// the click -- but `npm run dev` needs several seconds to bind a port. For
/// that window the UI said Running while nothing was listening, which is a
/// status that is right about the process and wrong about the service.
fn evaluate_health(
    app: &AppHandle,
    state: &AppState,
    project_id: deck_domain::project::ProjectId,
    listening: &[u16],
) {
    let Some(lifecycle) = state.supervisor.current_lifecycle(project_id) else {
        return;
    };
    // Only `Run` has a notion of "serving". An install or a build is doing
    // exactly what it should while listening on nothing at all.
    if lifecycle != deck_domain::command::Lifecycle::Run {
        return;
    }

    let Some((runner_id, declared)) = state.supervisor.health_inputs(project_id) else {
        return;
    };
    let runner = state.registry.get(&runner_id);
    let probe = runner.and_then(|r| r.manifest().health.as_ref());

    // Only read the log ring when a probe actually needs it -- it is a
    // per-project allocation and this runs once a second per running project.
    let log_text = match probe {
        Some(deck_domain::manifest::HealthProbe::LogContains { .. }) => state
            .supervisor
            .logs(project_id)
            .map(|sink| {
                sink.snapshot()
                    .iter()
                    .map(|l| l.text.as_str())
                    .collect::<Vec<_>>()
                    .join("
")
            })
            .unwrap_or_default(),
        _ => String::new(),
    };

    let readiness = deck_runtime::evaluate_readiness(probe, &declared, listening, &log_text);

    // `set_healthy` returns `None` when nothing changed, so a steady state does
    // not broadcast an identical event every second for the life of the run.
    if let Some(new_state) = state.supervisor.set_healthy(project_id, readiness.as_healthy()) {
        let run_id = state.supervisor.current_run(project_id).unwrap_or_default();
        let _ = app.emit(
            "deck://state",
            StateEvent {
                project_id: project_id.to_hyphenated(),
                run_id: run_id.to_string(),
                state: RunStateDto::from(&new_state),
            },
        );
    }
}
