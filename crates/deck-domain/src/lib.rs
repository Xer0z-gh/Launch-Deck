//! Core domain model for Launch Deck.
//!
//! This crate is deliberately free of I/O. It has no filesystem access, no
//! database, no process spawning and no Tauri. Everything here is a type, a
//! trait, or a pure function over those types, which means the whole domain is
//! testable in isolation and the dependency arrows in the workspace all point
//! inward to here.
//!
//! The one rule when extending this crate: if a change would require adding a
//! dependency that touches the outside world, the change belongs elsewhere.

#![deny(unsafe_code)]
#![warn(missing_docs, clippy::all, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod command;
pub mod error;
pub mod log;
pub mod manifest;
pub mod project;
pub mod runner;
pub mod runtime;

pub use command::{parse_command_line, CommandSpec, Lifecycle, ResolvedCommand, TemplateVars};
pub use error::{DeckError, Result};
pub use log::{strip_ansi, LogLine, LogStream, RunOutcome};
pub use manifest::{
    DetectionRule, HealthProbe, RunnerManifest, RunnerMeta, ValueSource, VarCase, VarRule,
};
pub use project::{
    DetectedFacts, EnvVar, Project, ProjectDraft, ProjectId, ProjectOverrides, RestartPolicy,
};
pub use runner::{DetectionMatch, Runner};
pub use runtime::{ProcessSnapshot, RunId, RunRecord, RunState, StopMode};
