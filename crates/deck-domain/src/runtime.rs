//! Live process state: what is running, and how it is doing.
//!
//! These types are the contract between the supervisor and the UI. They are
//! snapshots rather than handles -- nothing here holds an OS resource -- so they
//! can be cloned freely, serialized over IPC, and kept in frontend state
//! without any lifetime coupling to the process itself.

use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::command::Lifecycle;
use crate::project::ProjectId;

/// Identifier for one execution of one project.
///
/// A project accumulates many runs; log files and history rows are keyed by
/// this, not by project, so a crash from three launches ago stays readable.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct RunId(uuid::Uuid);

impl RunId {
    /// Mints a new identifier.
    #[must_use]
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }

    /// Wraps an existing UUID, for rehydrating from storage.
    #[must_use]
    pub const fn from_uuid(id: uuid::Uuid) -> Self {
        Self(id)
    }

    /// The underlying UUID.
    #[must_use]
    pub const fn as_uuid(&self) -> &uuid::Uuid {
        &self.0
    }
}

impl Default for RunId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for RunId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::str::FromStr for RunId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        s.parse().map(Self)
    }
}

/// Where a project is in its lifecycle.
///
/// Modelled as a state machine rather than a pair of booleans so the UI can
/// never render an impossible combination like "stopped but has a pid", and so
/// transitional states get their own honest presentation instead of flickering
/// between running and stopped.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum RunState {
    /// Not running, and not scheduled to. The default.
    #[default]
    Idle,

    /// Spawn requested; the OS has not confirmed a process yet.
    Starting,

    /// Process is alive. Health is tracked separately from liveness because a
    /// dev server exists for several seconds before it serves anything.
    Running {
        /// Operating-system process id of the tree root.
        pid: u32,
        /// When the process started.
        started_at: DateTime<Utc>,
        /// Readiness, when the runner defines a probe.
        healthy: Option<bool>,
    },

    /// Graceful stop requested; waiting for the process to exit on its own.
    Stopping {
        /// Process being asked to stop.
        pid: u32,
    },

    /// Exited on request or of its own accord, with a zero code.
    Exited {
        /// Exit code, absent if the process was terminated without one.
        code: Option<i32>,
        /// When it exited.
        at: DateTime<Utc>,
    },

    /// Exited with a non-zero code, or vanished unexpectedly.
    Crashed {
        /// Exit code, absent if terminated without one.
        code: Option<i32>,
        /// When it exited.
        at: DateTime<Utc>,
        /// Best-effort explanation, e.g. a detected port conflict.
        reason: Option<String>,
    },

    /// Waiting out a restart backoff after a crash.
    Restarting {
        /// Which consecutive attempt this is, 1-based.
        attempt: u32,
        /// When the next spawn will happen.
        next_attempt_at: DateTime<Utc>,
    },
}

impl RunState {
    /// Whether a process currently exists for this project.
    ///
    /// True during `Stopping` -- the process is still there until it is not,
    /// and a UI that hides the stop affordance too early invites a second click
    /// that force-kills something already shutting down cleanly.
    #[must_use]
    pub const fn is_live(&self) -> bool {
        matches!(self, Self::Running { .. } | Self::Stopping { .. })
    }

    /// Whether a transition is in flight, so the UI should show progress rather
    /// than an actionable control.
    #[must_use]
    pub const fn is_transitional(&self) -> bool {
        matches!(
            self,
            Self::Starting | Self::Stopping { .. } | Self::Restarting { .. }
        )
    }

    /// Whether starting this project is a valid action right now.
    #[must_use]
    pub const fn can_start(&self) -> bool {
        matches!(
            self,
            Self::Idle | Self::Exited { .. } | Self::Crashed { .. }
        )
    }

    /// Whether stopping this project is a valid action right now.
    #[must_use]
    pub const fn can_stop(&self) -> bool {
        matches!(
            self,
            Self::Running { .. } | Self::Starting | Self::Restarting { .. }
        )
    }

    /// The live process id, when there is one.
    #[must_use]
    pub const fn pid(&self) -> Option<u32> {
        match self {
            Self::Running { pid, .. } | Self::Stopping { pid } => Some(*pid),
            _ => None,
        }
    }

    /// Short lower-case discriminant for the frontend to switch on.
    #[must_use]
    pub const fn tag(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Starting => "starting",
            Self::Running { .. } => "running",
            Self::Stopping { .. } => "stopping",
            Self::Exited { .. } => "exited",
            Self::Crashed { .. } => "crashed",
            Self::Restarting { .. } => "restarting",
        }
    }
}

/// How forcefully to stop a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopMode {
    /// Ask the process to shut down, then terminate the tree if it does not.
    ///
    /// The default, and what the Stop button does. `grace_ms` is how long the
    /// process gets to exit on its own; a Vite server needs a few hundred
    /// milliseconds to release its port cleanly.
    Graceful {
        /// Milliseconds to wait before escalating to termination.
        grace_ms: u64,
    },

    /// Terminate the whole tree immediately, no signal first.
    Immediate,
}

impl Default for StopMode {
    fn default() -> Self {
        Self::Graceful { grace_ms: 5_000 }
    }
}

/// A point-in-time resource sample for a running process tree.
///
/// Values are aggregated across the whole tree, not just the root: `npm run dev`
/// is a shell whose child does the work, and reporting the shell's 0% CPU would
/// be accurate and useless.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessSnapshot {
    /// Which project this belongs to.
    pub project_id: ProjectId,

    /// Which run this belongs to.
    pub run_id: RunId,

    /// Root process id of the tree.
    pub pid: u32,

    /// When the sample was taken.
    pub at: DateTime<Utc>,

    /// Total CPU across the tree, as a percentage of one core.
    ///
    /// Can exceed 100 on a multi-core machine; a bundler using four cores fully
    /// reads as 400, which is the truth and worth showing rather than clamping.
    pub cpu_percent: f32,

    /// Resident memory across the tree, in bytes.
    pub memory_bytes: u64,

    /// Bytes read from disk since the process started.
    pub disk_read_bytes: u64,

    /// Bytes written to disk since the process started.
    pub disk_write_bytes: u64,

    /// Live processes in the tree, including the root.
    pub process_count: u32,

    /// Total OS threads across the tree.
    pub thread_count: u32,

    /// TCP ports the tree is currently listening on.
    pub listening_ports: Vec<u16>,

    /// Seconds since the process started.
    pub uptime_secs: u64,
}

/// A finished lifecycle step, for build and crash history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRecord {
    /// Identifier of the run.
    pub run_id: RunId,

    /// Project the run belongs to.
    pub project_id: ProjectId,

    /// Which lifecycle step was executed.
    pub lifecycle: Lifecycle,

    /// The command as executed, for reproducing it by hand.
    pub command: String,

    /// When it started.
    pub started_at: DateTime<Utc>,

    /// When it finished, absent while still running.
    pub finished_at: Option<DateTime<Utc>>,

    /// Exit code, absent while running or if terminated without one.
    pub exit_code: Option<i32>,

    /// How it ended.
    pub outcome: crate::log::RunOutcome,

    /// Path to this run's log file on disk.
    pub log_path: std::path::PathBuf,
}

impl RunRecord {
    /// Wall-clock duration, if the run has finished.
    #[must_use]
    pub fn duration(&self) -> Option<chrono::Duration> {
        self.finished_at.map(|end| end - self.started_at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running() -> RunState {
        RunState::Running {
            pid: 4242,
            started_at: Utc::now(),
            healthy: None,
        }
    }

    #[test]
    fn run_ids_are_unique_and_round_trip() {
        let a = RunId::new();
        assert_ne!(a, RunId::new());
        assert_eq!(a.to_string().parse::<RunId>().unwrap(), a);
    }

    #[test]
    fn liveness_includes_stopping() {
        assert!(running().is_live());
        assert!(RunState::Stopping { pid: 1 }.is_live());
        assert!(!RunState::Idle.is_live());
        assert!(!RunState::Starting.is_live());
    }

    #[test]
    fn start_and_stop_affordances_are_mutually_exclusive() {
        // No state may offer both Start and Stop; that is what makes a single
        // contextual primary button on a card unambiguous.
        let states = [
            RunState::Idle,
            RunState::Starting,
            running(),
            RunState::Stopping { pid: 1 },
            RunState::Exited {
                code: Some(0),
                at: Utc::now(),
            },
            RunState::Crashed {
                code: Some(1),
                at: Utc::now(),
                reason: None,
            },
            RunState::Restarting {
                attempt: 2,
                next_attempt_at: Utc::now(),
            },
        ];
        for s in states {
            assert!(
                !(s.can_start() && s.can_stop()),
                "{} offered both start and stop",
                s.tag()
            );
        }
    }

    #[test]
    fn stopping_offers_neither_action() {
        let s = RunState::Stopping { pid: 9 };
        assert!(!s.can_start());
        assert!(!s.can_stop());
    }

    #[test]
    fn crashed_projects_can_be_restarted() {
        let s = RunState::Crashed {
            code: Some(1),
            at: Utc::now(),
            reason: Some("port 5173 in use".into()),
        };
        assert!(s.can_start());
    }

    #[test]
    fn pid_is_present_exactly_when_live() {
        assert_eq!(running().pid(), Some(4242));
        assert_eq!(RunState::Stopping { pid: 7 }.pid(), Some(7));
        assert_eq!(RunState::Idle.pid(), None);
        assert_eq!(RunState::Starting.pid(), None);
    }

    #[test]
    fn transitional_states_are_the_three_in_flight_ones() {
        assert!(RunState::Starting.is_transitional());
        assert!(RunState::Stopping { pid: 1 }.is_transitional());
        assert!(RunState::Restarting {
            attempt: 1,
            next_attempt_at: Utc::now()
        }
        .is_transitional());
        assert!(!running().is_transitional());
        assert!(!RunState::Idle.is_transitional());
    }

    #[test]
    fn state_serializes_with_a_discriminant_field() {
        let json = serde_json::to_value(running()).unwrap();
        assert_eq!(json["state"], "running");
        assert_eq!(json["pid"], 4242);
    }

    #[test]
    fn default_stop_mode_is_graceful_with_a_real_grace_period() {
        match StopMode::default() {
            StopMode::Graceful { grace_ms } => assert!(grace_ms >= 1_000),
            StopMode::Immediate => panic!("default stop must not be immediate"),
        }
    }
}
