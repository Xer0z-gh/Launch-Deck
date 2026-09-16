//! Startup and launch timings, measured rather than assumed.
//!
//! These exist because "it feels slow" is not actionable and "I optimised the
//! thing I guessed was slow" is not progress. Every number here is recorded by
//! the code path it describes and surfaced in Diagnostics, so a regression is
//! visible in the app instead of being noticed months later.
//!
//! Cost of keeping them: a few `Instant::elapsed` calls at startup and one per
//! launch. Nothing here samples, polls, or allocates on a hot path.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How many recent launches to keep. Enough to see a pattern, small enough that
/// the whole thing stays a fixed-size allocation.
const LAUNCH_HISTORY: usize = 20;

/// One measured project launch, from IPC entry to the child being spawned.
#[derive(Debug, Clone)]
pub struct LaunchTiming {
    /// Project name, for reading the list without cross-referencing ids.
    pub project: String,
    /// Reading the project row out of SQLite.
    pub lookup: Duration,
    /// Detection and command resolution.
    pub plan: Duration,
    /// The TCP-table snapshot, when ports had to be checked at all.
    pub port_check: Duration,
    /// `CreateProcess`, job-object assignment, and resume.
    pub spawn: Duration,
    /// Everything above, plus the glue between.
    pub total: Duration,
}

/// Phase timings captured once, during startup.
#[derive(Debug, Default, Clone, Copy)]
pub struct StartupPhases {
    /// Parsing the bundled and user runner manifests.
    pub registry: Duration,
    /// Opening the pool, running migrations, and self-healing if needed.
    pub store_open: Duration,
    /// `run()` entry to the end of `Builder::build()` -- plugins, capability
    /// parsing, window creation and the WebView2 environment.
    pub builder: Duration,
    /// Building the tray icon and its menu.
    pub tray: Duration,
    /// The whole of Tauri's `setup` callback.
    pub setup: Duration,
    /// Process start to the end of setup.
    pub to_ready: Duration,
    /// Frontend milestones, in ms from webview navigation start.
    ///
    /// Split out for the same reason `setup` was: one opaque "the frontend took
    /// 140 ms" cannot be optimised, only worried about.
    pub dom_interactive_ms: u32,
    /// When our own first line of JS ran. Everything before it is HTML parse
    /// plus module fetch and parse -- reducible only by shipping less.
    pub script_eval_ms: u32,
    /// When React was handed the tree.
    pub react_mount_ms: u32,
    /// When the project-list IPC was issued.
    pub fetch_start_ms: u32,
    /// When it answered. The gap is backend latency; the gap after it is React.
    pub data_arrived_ms: u32,
    /// When the project list had painted.
    pub painted_ms: u32,
    /// Process start to the frontend's first painted project list.
    ///
    /// The only number here that matches what a person waits for. Measured by
    /// the frontend calling `ui_ready`, NOT over CDP: enabling the debugging
    /// port measurably slows WebView2 startup, so a CDP-driven benchmark
    /// reports a boot the user never experiences.
    pub to_first_paint: Duration,
}

/// Timing state shared through `AppState`.
#[derive(Debug)]
pub struct Timings {
    /// When this process began, for uptime and for the boot figures.
    pub process_start: Instant,
    phases: Mutex<StartupPhases>,
    launches: Mutex<Vec<LaunchTiming>>,
}

impl Timings {
    /// Records the frontend's first painted list. First call wins -- later
    /// renders are not boots.
    pub fn set_first_paint(&self, elapsed: std::time::Duration, marks: FrontendMarks) {
        if let Ok(mut phases) = self.phases.lock() {
            if phases.to_first_paint.is_zero() {
                phases.to_first_paint = elapsed;
                phases.dom_interactive_ms = marks.dom_interactive_ms;
                phases.script_eval_ms = marks.script_eval_ms;
                phases.react_mount_ms = marks.react_mount_ms;
                phases.fetch_start_ms = marks.fetch_start_ms;
                phases.data_arrived_ms = marks.data_arrived_ms;
                phases.painted_ms = marks.painted_ms;
            }
        }
    }

    /// Records how long `Builder::build()` took.
    ///
    /// Set after the fact because `build()` runs the setup callback inside
    /// itself, so the figure is only available once it returns.
    pub fn set_builder(&self, elapsed: std::time::Duration) {
        if let Ok(mut phases) = self.phases.lock() {
            phases.builder = elapsed;
        }
    }

    /// Records how long the host monitor took to build.
    ///
    /// Set from the warm-up thread rather than at `set_phases` time, because
    /// Starts the clock. Called as early in `run()` as possible.
    #[must_use]
    pub fn start() -> Self {
        Self {
            process_start: Instant::now(),
            phases: Mutex::new(StartupPhases::default()),
            launches: Mutex::new(Vec::with_capacity(LAUNCH_HISTORY)),
        }
    }

    /// Records the startup phase durations once setup completes.
    pub fn set_phases(&self, phases: StartupPhases) {
        if let Ok(mut slot) = self.phases.lock() {
            *slot = phases;
        }
    }

    /// The recorded startup phases.
    #[must_use]
    pub fn phases(&self) -> StartupPhases {
        self.phases.lock().map(|p| *p).unwrap_or_default()
    }

    /// How long this process has been alive.
    #[must_use]
    pub fn uptime(&self) -> Duration {
        self.process_start.elapsed()
    }

    /// Records one launch, dropping the oldest once the history is full.
    pub fn record_launch(&self, timing: LaunchTiming) {
        if let Ok(mut list) = self.launches.lock() {
            if list.len() == LAUNCH_HISTORY {
                list.remove(0);
            }
            list.push(timing);
        }
    }

    /// Recent launches, newest last.
    #[must_use]
    pub fn launches(&self) -> Vec<LaunchTiming> {
        self.launches.lock().map(|l| l.clone()).unwrap_or_default()
    }

    /// Mean total launch time, or `None` before anything has been launched.
    #[must_use]
    pub fn mean_launch(&self) -> Option<Duration> {
        let list = self.launches();
        if list.is_empty() {
            return None;
        }
        let total: Duration = list.iter().map(|l| l.total).sum();
        u32::try_from(list.len()).ok().map(|n| total / n)
    }
}

impl Default for Timings {
    fn default() -> Self {
        Self::start()
    }
}

/// Frontend timing marks, in ms from webview navigation start.
#[derive(Debug, Clone, Copy, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontendMarks {
    /// HTML parsed and scripts ready to run.
    pub dom_interactive_ms: u32,
    /// Our first line of JS.
    pub script_eval_ms: u32,
    /// React handed the tree.
    pub react_mount_ms: u32,
    /// Project-list IPC issued.
    pub fetch_start_ms: u32,
    /// Project-list IPC answered.
    pub data_arrived_ms: u32,
    /// Project list painted.
    pub painted_ms: u32,
}
