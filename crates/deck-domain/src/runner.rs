//! The `Runner` abstraction.
//!
//! This is the seam the whole plugin story hangs on. A runner answers two
//! questions: *is this directory mine?* and *what command performs this
//! lifecycle step?* It does not spawn anything, does not touch the database,
//! and does not know the UI exists -- the supervisor takes a [`ResolvedCommand`]
//! and owns everything after that.
//!
//! Keeping the trait this narrow is what makes the declarative manifest format
//! sufficient for essentially every language. A runner that needed to *run* code
//! to answer either question would force the plugin format to become a
//! programming language; because it does not, a `.toml` file is enough, and
//! adding Deno support requires no Rust at all.
//!
//! The trait remains the internal interface regardless. `deck-runners` provides
//! one implementation backed by manifests, and that covers the shipped runners.
//! If a future ecosystem genuinely cannot be described declaratively, a second
//! implementation slots in behind this same trait without the core changing.

use std::path::Path;

use crate::command::{Lifecycle, ResolvedCommand, TemplateVars};
use crate::error::Result;
use crate::manifest::RunnerMeta;

/// A successful detection: this runner claims the directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectionMatch {
    /// Identity of the runner that matched.
    pub meta: RunnerMeta,

    /// Variables resolved from the directory, e.g. `pm = "pnpm"`.
    ///
    /// Carried forward so command resolution does not re-read the filesystem,
    /// and so the settings UI can show *why* a command looks the way it does.
    pub vars: TemplateVars,

    /// Version declared by the project's own metadata, when it declares one.
    pub version: Option<String>,
}

impl DetectionMatch {
    /// Priority of the matched runner, for resolving competing matches.
    #[must_use]
    pub const fn priority(&self) -> i32 {
        self.meta.priority
    }
}

/// Something that can identify and drive a class of project.
///
/// Implementations must be cheap to clone or be used behind a reference; the
/// detection engine holds many of them and consults each per candidate
/// directory.
pub trait Runner: Send + Sync {
    /// Stable identity and presentation for this runner.
    fn meta(&self) -> &RunnerMeta;

    /// Decides whether `root` is a project this runner can drive.
    ///
    /// Returns `Ok(None)` for a clean non-match. An `Err` means detection could
    /// not be completed -- an unreadable file, malformed JSON where a rule
    /// expected valid JSON -- and is distinct from "this is not my project", so
    /// a permissions problem surfaces as a problem instead of silently
    /// presenting as "unknown project type".
    ///
    /// # Errors
    ///
    /// Propagates I/O and parse failures encountered while evaluating rules.
    fn detect(&self, root: &Path) -> Result<Option<DetectionMatch>>;

    /// The lifecycle steps this runner implements.
    fn supported(&self) -> Vec<Lifecycle>;

    /// Builds the command for a lifecycle step.
    ///
    /// `vars` comes from the [`DetectionMatch`], extended by the caller with
    /// project-level variables such as `{projectRoot}`.
    ///
    /// # Errors
    ///
    /// Returns [`crate::DeckError::LifecycleNotSupported`] if this runner does
    /// not implement `step`, or [`crate::DeckError::UnknownTemplateVar`] if the
    /// template references a variable absent from `vars`.
    fn command(&self, step: Lifecycle, vars: &TemplateVars) -> Result<ResolvedCommand>;

    /// Whether this runner implements a step. Convenience over [`Self::supported`].
    fn supports(&self, step: Lifecycle) -> bool {
        self.supported().contains(&step)
    }
}
