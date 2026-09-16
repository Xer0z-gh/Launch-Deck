//! Project-type detection and the declarative runner plugin system.
//!
//! This crate turns a directory into "here is what this project is, and here is
//! the command that starts it". It reads the filesystem and parses config files;
//! it never executes anything. Spawning belongs to `deck-runtime`.
//!
//! # The plugin model
//!
//! A runner is a `.toml` file, not code. [`registry::RunnerRegistry`] loads the
//! bundled manifests compiled into the binary, then overlays user manifests from
//! the app data directory -- a user file replaces a bundled one with the same id.
//! Adding support for a new language is dropping in a file; no crate changes and
//! no recompile.
//!
//! [`deck_domain::runner::Runner`] remains the internal interface, and
//! [`manifest_runner::ManifestRunner`] is the single implementation behind it. If
//! an ecosystem ever proves genuinely undescribable declaratively, a second
//! implementation slots in behind the same trait without the core changing.
//!
//! # Why detection cannot execute
//!
//! Detection runs automatically over directories the user merely pointed at, so
//! a manifest able to run a command during detection would turn "scan this
//! folder" into "run whatever this folder says". Every
//! [`DetectionRule`](deck_domain::manifest::DetectionRule) is therefore a
//! read-only predicate, rule paths cannot escape the project root, and file
//! reads are size-capped.

#![deny(unsafe_code)]
#![warn(missing_docs, clippy::all, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod builtin;
pub mod glob;
pub mod launch;
pub mod manifest_runner;
pub mod probe;
pub mod registry;
pub mod rule;
pub mod scan;

pub use launch::{parse_env_file, plan, LaunchPlan};
pub use manifest_runner::ManifestRunner;
pub use probe::ProbeCache;
pub use registry::{ManifestProblem, RunnerRegistry};
pub use scan::{scan, ScanOptions, ScanResult};
