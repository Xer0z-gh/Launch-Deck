//! Process supervision: spawning, watching, stopping, and log capture.
//!
//! This crate is where Launch Deck actually runs things. It takes a
//! [`deck_domain::command::ResolvedCommand`] and owns everything after that: the
//! job object, the pipes, the state machine, the restart policy.
//!
//! The two things worth reading before changing anything here:
//!
//! - [`job`] explains why children are spawned suspended and assigned to a
//!   Windows Job Object, and why that is the difference between stopping a
//!   project and leaving a tree of orphaned processes holding ports.
//! - [`logs`] explains the volume constraints that shape the log pipeline: files
//!   on disk, a bounded ring in memory, and a deliberately lossy broadcast so a
//!   slow log viewer can never apply backpressure to a build.
//!
//! Everything is testable without a GUI. The supervisor tests spawn real
//! processes and assert on real exits, including that a grandchild dies with its
//! parent -- the bug this design exists to prevent.

#![warn(missing_docs, clippy::all, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod appicon;
pub mod disk;
pub mod facts;
pub mod health;
pub mod job;
pub mod logs;
pub mod manual;
pub mod metrics;
pub mod ports;
pub mod preflight;
pub mod program;
pub mod supervisor;
pub mod studio;
pub mod surface;

pub use appicon::{find_icon_source, load_icon, IconSource};
pub use health::{evaluate as evaluate_readiness, Readiness};
pub use job::JobObject;
pub use logs::LogSink;
pub use metrics::Sampler;
pub use ports::{
    disk_space, first_conflict, first_conflict_in, ports_for_pids, taken_ports,
};
pub use preflight::{cargo_ambiguity, missing_entry};
pub use program::{resolve as resolve_program, Invocation};
pub use supervisor::{SpawnRequest, Supervisor, SupervisorEvent};
pub use studio::{append_handoff, error_lines, newest_file, read_tail, TailReading};
pub use disk::{children as dir_children, usage as dir_usage, Child as DirChild, Usage as DirUsage};
pub use facts::{read_facts, SurfaceFact};
pub use manual::{read as read_manual, DocFile, Manual, Script};
pub use surface::{log_mtime, ping_url, probe_port, PingOutcome};
