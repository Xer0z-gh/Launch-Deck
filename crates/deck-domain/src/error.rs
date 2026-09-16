//! The single error type crossing crate boundaries.
//!
//! Each layer keeps its own private error detail where useful, but everything
//! that reaches the IPC surface converges here so the frontend deals with one
//! shape. Variants carry enough context to render an actionable message --
//! "Port 5173 is already in use" rather than "exit code 1".

use std::path::PathBuf;

use crate::project::ProjectId;
use crate::runtime::RunId;

/// Convenience alias used throughout the workspace.
pub type Result<T> = std::result::Result<T, DeckError>;

/// Every failure Launch Deck can surface to the user.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DeckError {
    /// The path the user chose is not a directory we can read.
    #[error("`{path}` is not a readable directory")]
    UnreadableDirectory {
        /// The offending path.
        path: PathBuf,
    },

    /// A project is already registered against this directory.
    #[error("`{path}` is already registered as \"{existing_name}\"")]
    DuplicateProject {
        /// The directory that collided.
        path: PathBuf,
        /// Name of the project already occupying it.
        existing_name: String,
    },

    /// No runner claimed the directory and the user supplied no override.
    #[error("could not identify a project type in `{path}` -- set a run command manually")]
    NoRunnerMatched {
        /// The directory that was scanned.
        path: PathBuf,
    },

    /// A runner manifest exists but does not define the requested lifecycle step.
    #[error("runner `{runner}` has no {lifecycle} command")]
    LifecycleNotSupported {
        /// Runner identifier.
        runner: String,
        /// The lifecycle step that was requested.
        lifecycle: crate::command::Lifecycle,
    },

    /// A manifest on disk is malformed.
    #[error("runner manifest `{path}` is invalid: {reason}")]
    InvalidManifest {
        /// Path to the manifest file.
        path: PathBuf,
        /// Why it was rejected.
        reason: String,
    },

    /// A command template referenced a variable we cannot resolve.
    #[error("command template references unknown variable `{{{variable}}}`")]
    UnknownTemplateVar {
        /// The unresolved variable name.
        variable: String,
    },

    /// Asked to act on a project that is not registered.
    #[error("no project with id {0}")]
    ProjectNotFound(ProjectId),

    /// Asked to act on a run that is not tracked.
    #[error("no run with id {0}")]
    RunNotFound(RunId),

    /// Tried to start a project that is already running.
    #[error("\"{name}\" is already running (pid {pid})")]
    AlreadyRunning {
        /// Project name, for the message.
        name: String,
        /// The live process id.
        pid: u32,
    },

    /// Tried to stop a project that is not running.
    #[error("\"{name}\" is not running")]
    NotRunning {
        /// Project name, for the message.
        name: String,
    },

    /// The executable named by the run command is not on PATH.
    #[error("`{program}` was not found on PATH -- is it installed?")]
    ProgramNotFound {
        /// The program we failed to launch.
        program: String,
    },

    /// A port the project needs is occupied by something else.
    #[error("port {port} is already in use")]
    PortInUse {
        /// The contested port.
        port: u16,
    },

    /// Auto-restart tripped its circuit breaker.
    #[error("\"{name}\" crashed {attempts} times in a row -- auto-restart disabled")]
    RestartLoopBrokenOut {
        /// Project name, for the message.
        name: String,
        /// How many consecutive failures were seen.
        attempts: u32,
    },

    /// Persistence failed.
    #[error("storage error: {0}")]
    Storage(String),

    /// An operating-system call failed.
    #[error("{context}: {source}")]
    Io {
        /// What we were trying to do.
        context: String,
        /// The underlying failure.
        #[source]
        source: std::io::Error,
    },
}

impl DeckError {
    /// Wraps an [`std::io::Error`] with a description of the attempted action.
    ///
    /// Bare io errors ("access is denied") are close to useless in a UI; this
    /// keeps the operation attached to the cause.
    pub fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }

    /// A stable machine-readable discriminant for the frontend.
    ///
    /// The frontend switches on this rather than matching message text, so
    /// wording can change freely without breaking behaviour.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnreadableDirectory { .. } => "unreadable_directory",
            Self::DuplicateProject { .. } => "duplicate_project",
            Self::NoRunnerMatched { .. } => "no_runner_matched",
            Self::LifecycleNotSupported { .. } => "lifecycle_not_supported",
            Self::InvalidManifest { .. } => "invalid_manifest",
            Self::UnknownTemplateVar { .. } => "unknown_template_var",
            Self::ProjectNotFound(_) => "project_not_found",
            Self::RunNotFound(_) => "run_not_found",
            Self::AlreadyRunning { .. } => "already_running",
            Self::NotRunning { .. } => "not_running",
            Self::ProgramNotFound { .. } => "program_not_found",
            Self::PortInUse { .. } => "port_in_use",
            Self::RestartLoopBrokenOut { .. } => "restart_loop_broken_out",
            Self::Storage(_) => "storage",
            Self::Io { .. } => "io",
        }
    }

    /// Whether retrying the same operation could plausibly succeed.
    ///
    /// Drives both the UI's "Try again" affordance and its automatic retry. A
    /// storage failure counts because the common one is a briefly-locked `SQLite`
    /// file, which clears on its own; a port conflict counts because the other
    /// process may release the port.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::PortInUse { .. } | Self::Io { .. } | Self::Storage(_)
        )
    }
}
