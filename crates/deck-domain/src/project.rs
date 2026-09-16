//! The registered project: what the user added, and how they want it run.
//!
//! Detection produces a starting point; the user's overrides always win. The
//! two are kept separate rather than merged at write time so re-detecting a
//! project (after it gains a framework, say) can refresh the detected values
//! without silently discarding a command the user typed by hand.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::command::{CommandSpec, Lifecycle};

/// Stable identifier for a registered project.
///
/// A UUID rather than a database rowid: ids appear in log directory names and
/// in frontend state, and must never be reused after a delete.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct ProjectId(uuid::Uuid);

impl ProjectId {
    /// Mints a new random identifier.
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

    /// Hyphenated string form, as stored and as sent over IPC.
    #[must_use]
    pub fn to_hyphenated(&self) -> String {
        self.0.to_string()
    }
}

impl Default for ProjectId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ProjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::str::FromStr for ProjectId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        s.parse().map(Self)
    }
}

/// What detection found, before the user changed anything.
///
/// Refreshed by a re-detect; never written to by the settings UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectedFacts {
    /// Id of the runner that claimed the directory.
    pub runner_id: String,
    /// Language reported by that runner.
    pub language: String,
    /// Framework, when the runner identified one.
    pub framework: Option<String>,
    /// Package manager resolved from lockfiles, when applicable.
    pub package_manager: Option<String>,
    /// Version read from the project's own metadata, when it declares one.
    pub version: Option<String>,
    /// When detection last ran.
    pub detected_at: DateTime<Utc>,
}

/// A single environment variable for a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvVar {
    /// Variable name.
    pub key: String,
    /// Variable value.
    pub value: String,
    /// Whether the value should be masked in the UI and scrubbed from logs.
    ///
    /// Defaults to true for anything whose name looks credential-shaped -- see
    /// [`EnvVar::looks_secret`]. Getting this wrong leaks a token into a log
    /// file, so the default errs toward masking.
    #[serde(default)]
    pub secret: bool,
}

impl EnvVar {
    /// Builds a variable, inferring `secret` from the key.
    #[must_use]
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        let key = key.into();
        let secret = Self::looks_secret(&key);
        Self {
            key,
            value: value.into(),
            secret,
        }
    }

    /// Whether a variable name suggests it holds a credential.
    ///
    /// Substring matching on an upper-cased key. Deliberately broad: a false
    /// positive costs a click to unmask, a false negative writes a secret to
    /// disk in plain text.
    #[must_use]
    pub fn looks_secret(key: &str) -> bool {
        const NEEDLES: [&str; 10] = [
            "SECRET", "TOKEN", "PASSWORD", "PASSWD", "APIKEY", "API_KEY", "PRIVATE",
            "CREDENTIAL", "AUTH", "SIGNING",
        ];
        let upper = key.to_ascii_uppercase();
        NEEDLES.iter().any(|n| upper.contains(n))
    }
}

/// Everything the user can change about how a project runs.
///
/// `Default` is implemented by hand rather than derived: `kill_on_app_exit`
/// defaults to true, and a derived impl would give false, so the struct
/// literal and a deserialized `{}` would disagree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectOverrides {
    /// Replaces the runner's command for a given lifecycle step.
    #[serde(default)]
    pub commands: BTreeMap<Lifecycle, CommandSpec>,

    /// Arguments appended to the run command.
    #[serde(default)]
    pub args: Vec<String>,

    /// Arguments the user switched OFF without deleting.
    ///
    /// A flag you are toggling is a flag you expect to toggle back -- `--debug`
    /// or `--port 4000` come and go across a week. Deleting it means retyping
    /// it, and retyping is where the typo comes from, so a disabled flag is
    /// remembered rather than discarded.
    ///
    /// A second list rather than making `args` a list of `{value, enabled}`
    /// objects: that would be a storage-format change and a migration across
    /// every existing project, to buy the ordering of flags that are not in
    /// the command anyway. The cost is that re-enabling appends at the end
    /// rather than restoring the original position, which for independent
    /// flags is not a difference.
    #[serde(default)]
    pub disabled_args: Vec<String>,

    /// Working directory, when it differs from the project root.
    ///
    /// Needed for monorepos where the project is registered at the repo root
    /// but the dev server must run inside a package.
    #[serde(default)]
    pub working_dir: Option<PathBuf>,

    /// Environment variables layered over the inherited environment.
    #[serde(default)]
    pub env: Vec<EnvVar>,

    /// A `.env` file to load before applying `env`.
    #[serde(default)]
    pub env_file: Option<PathBuf>,

    /// Ports this project is expected to bind, checked for conflicts pre-launch.
    #[serde(default)]
    pub ports: Vec<u16>,

    /// Whether to terminate this project when Launch Deck itself exits.
    ///
    /// Defaults to true. Leaving orphaned dev servers holding ports after the
    /// control centre closes is the more surprising behaviour of the two.
    #[serde(default = "default_true")]
    pub kill_on_app_exit: bool,

    /// What this project's tile watches beyond the supervisor: a status source.
    ///
    /// `None` for the common case of a project that only exists when Launch
    /// Deck spawns it. Lives inside `overrides` because that is already the
    /// JSON column for per-project configuration, so adding it costs no
    /// migration and old rows deserialize unchanged.
    #[serde(default)]
    pub surface: Option<SurfaceConfig>,
}

/// A status source the supervisor does not own.
///
/// Every field is optional and independent: a deployed site has only a URL, a
/// panel has a URL and a port, an ops script may have only a log. Whatever is
/// unset is simply not probed -- the tile never invents a reading for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceConfig {
    /// Opened by the tile's open action and pinged for reachability.
    /// User-typed; the only URLs this app ever contacts.
    #[serde(default)]
    pub url: Option<String>,

    /// Local TCP port probed for "running even though we did not start it".
    #[serde(default)]
    pub probe_port: Option<u16>,

    /// A log file written by the app itself, watched by modification time.
    #[serde(default)]
    pub external_log: Option<std::path::PathBuf>,

    /// Which app-specific reader turns this source into glanceable facts
    /// ("84% of the 5-hour window", "3 models loaded"). None means the tile
    /// shows reachability only. User-chosen, never inferred: the reader
    /// derives its endpoints from `url` (or reads `external_log`), so the
    /// network boundary in `deck_runtime::surface` still holds -- only
    /// origins the user typed are ever contacted.
    #[serde(default)]
    pub facts: Option<FactsSource>,
}

/// The app-specific fact readers Launch Deck knows how to talk to.
///
/// Each variant names one of Tanner's own apps and the exact endpoints or
/// file it reads. There is deliberately no "generic JSON" variant: a fact is
/// a field somebody chose because it matters, and a dashboard that renders
/// whatever keys a payload happens to have is a JSON viewer, not a glance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FactsSource {
    /// Fleet's panel: `GET {origin}/api/health` (unauthenticated by design).
    Fleet,
    /// Ollama: `GET {origin}/api/tags` and `GET {origin}/api/ps`.
    Ollama,
    /// Lathe: `GET {origin}/health` and `GET {origin}/stats`.
    Lathe,
    /// `FocusForge`: `GET {origin}/api/status`.
    FocusForge,
    /// Pavlok alerts: the one-line `pavlok.status` file at `external_log`.
    PavlokStatus,
}

const fn default_true() -> bool {
    true
}

impl Default for ProjectOverrides {
    fn default() -> Self {
        Self {
            commands: BTreeMap::new(),
            args: Vec::new(),
            disabled_args: Vec::new(),
            working_dir: None,
            env: Vec::new(),
            env_file: None,
            surface: None,
            ports: Vec::new(),
            kill_on_app_exit: default_true(),
        }
    }
}

impl ProjectOverrides {
    /// The command for a step, preferring the user's override.
    #[must_use]
    pub fn command(&self, step: Lifecycle) -> Option<&CommandSpec> {
        self.commands.get(&step)
    }
}

/// What to do when a project's process exits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum RestartPolicy {
    /// Never restart automatically. The default.
    #[default]
    Never,

    /// Restart only on a non-zero exit.
    OnFailure {
        /// Consecutive failures tolerated before giving up.
        max_attempts: u32,
    },

    /// Restart on any exit, including a clean one.
    Always {
        /// Consecutive restarts tolerated before giving up.
        max_attempts: u32,
    },
}

impl RestartPolicy {
    /// Whether an exit with this code should trigger a restart.
    #[must_use]
    pub const fn should_restart(self, exit_code: Option<i32>) -> bool {
        match self {
            Self::Never => false,
            Self::Always { .. } => true,
            // A missing exit code means the process was signalled or the code
            // could not be read; treat that as a failure, since a supervised
            // process vanishing is not a clean shutdown.
            Self::OnFailure { .. } => !matches!(exit_code, Some(0)),
        }
    }

    /// The consecutive-failure ceiling, if this policy restarts at all.
    #[must_use]
    pub const fn max_attempts(self) -> Option<u32> {
        match self {
            Self::Never => None,
            Self::OnFailure { max_attempts } | Self::Always { max_attempts } => {
                Some(max_attempts)
            }
        }
    }

    /// Delay before restart attempt `attempt` (1-based).
    ///
    /// Exponential with a 30s ceiling. Without backoff a crash-on-startup loop
    /// becomes a fork bomb that saturates a core and floods the log pipeline --
    /// the failure mode PM2 exists to prevent.
    #[must_use]
    pub const fn backoff_ms(attempt: u32) -> u64 {
        const BASE_MS: u64 = 500;
        const CEILING_MS: u64 = 30_000;
        let shift = if attempt > 6 { 6 } else { attempt.saturating_sub(1) };
        let delay = BASE_MS << shift;
        if delay > CEILING_MS {
            CEILING_MS
        } else {
            delay
        }
    }
}

/// A registered project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    /// Stable identifier.
    pub id: ProjectId,

    /// Display name. Defaults to the directory name, renameable.
    pub name: String,

    /// Optional description, shown on the detail page.
    pub description: Option<String>,

    /// Absolute path to the project root.
    pub root: PathBuf,

    /// What detection found.
    pub detected: DetectedFacts,

    /// What the user changed.
    pub overrides: ProjectOverrides,

    /// Free-form tags, used by search and filtering.
    pub tags: Vec<String>,

    /// Optional single category, used for grouping.
    pub category: Option<String>,

    /// Starred by the user.
    pub favorite: bool,

    /// Held at the top of the list regardless of sort.
    pub pinned: bool,

    /// Hidden from the default view without being deleted.
    pub archived: bool,

    /// Restart behaviour for this project.
    pub restart_policy: RestartPolicy,

    /// User notes, free-form markdown.
    pub notes: Option<String>,

    /// When the project was registered.
    pub created_at: DateTime<Utc>,

    /// When any field last changed.
    pub updated_at: DateTime<Utc>,

    /// When the project was last launched, if ever.
    pub last_launched_at: Option<DateTime<Utc>>,
}

impl Project {
    /// The directory a command should run in: the override, else the root.
    #[must_use]
    pub fn effective_working_dir(&self) -> &PathBuf {
        self.overrides.working_dir.as_ref().unwrap_or(&self.root)
    }

    /// Ports to check before launch: the user's list, else the runner's defaults
    /// as recorded at detection time.
    #[must_use]
    pub fn effective_ports(&self) -> &[u16] {
        &self.overrides.ports
    }

    /// The framework if one was detected, else the language.
    ///
    /// What a card should show on its single metadata line -- "Next.js" is more
    /// informative than "TypeScript" when both are known.
    #[must_use]
    pub fn kind_label(&self) -> &str {
        self.detected
            .framework
            .as_deref()
            .unwrap_or(&self.detected.language)
    }

    /// Whether `needle` matches this project, for global search.
    ///
    /// Case-insensitive substring across name, description, language,
    /// framework, tags, category and path. Path is included because "which of
    /// these lives under `Dev/Rust`" is a real way people look for a project.
    #[must_use]
    pub fn matches_query(&self, needle: &str) -> bool {
        let needle = needle.trim().to_ascii_lowercase();
        if needle.is_empty() {
            return true;
        }
        let contains = |hay: &str| hay.to_ascii_lowercase().contains(&needle);

        contains(&self.name)
            || self.description.as_deref().is_some_and(contains)
            || contains(&self.detected.language)
            || self.detected.framework.as_deref().is_some_and(contains)
            || self.category.as_deref().is_some_and(contains)
            || self.tags.iter().any(|t| contains(t))
            || contains(&self.root.display().to_string())
    }
}

/// A project about to be registered: detection has run, nothing is persisted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectDraft {
    /// Proposed name, from the directory name.
    pub name: String,
    /// Absolute project root.
    pub root: PathBuf,
    /// What detection found, if anything did.
    pub detected: Option<DetectedFacts>,
    /// The resolved run command, for the confirmation step.
    ///
    /// Shown before registering so the user sees exactly what will execute --
    /// detection proposes, it never silently commits.
    pub proposed_run: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> DetectedFacts {
        DetectedFacts {
            runner_id: "node".into(),
            language: "TypeScript".into(),
            framework: Some("Next.js".into()),
            package_manager: Some("pnpm".into()),
            version: None,
            detected_at: Utc::now(),
        }
    }

    fn project() -> Project {
        let now = Utc::now();
        Project {
            id: ProjectId::new(),
            name: "Coupon Hunter".into(),
            description: Some("Browser extension for stacking codes".into()),
            root: PathBuf::from(r"D:\Workspace\Dev\JS\Extensions\Coupon-Hunter"),
            detected: facts(),
            overrides: ProjectOverrides::default(),
            tags: vec!["extension".into(), "mv3".into()],
            category: Some("Browser".into()),
            favorite: false,
            pinned: false,
            archived: false,
            restart_policy: RestartPolicy::default(),
            notes: None,
            created_at: now,
            updated_at: now,
            last_launched_at: None,
        }
    }

    #[test]
    fn ids_are_unique() {
        assert_ne!(ProjectId::new(), ProjectId::new());
    }

    #[test]
    fn id_round_trips_through_string() {
        let id = ProjectId::new();
        let parsed: ProjectId = id.to_hyphenated().parse().unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn working_dir_falls_back_to_root() {
        let p = project();
        assert_eq!(p.effective_working_dir(), &p.root);
    }

    #[test]
    fn working_dir_override_wins() {
        let mut p = project();
        let sub = PathBuf::from(r"D:\Workspace\Dev\JS\Extensions\Coupon-Hunter\web");
        p.overrides.working_dir = Some(sub.clone());
        assert_eq!(p.effective_working_dir(), &sub);
    }

    #[test]
    fn kind_label_prefers_framework() {
        assert_eq!(project().kind_label(), "Next.js");
    }

    #[test]
    fn kind_label_falls_back_to_language() {
        let mut p = project();
        p.detected.framework = None;
        assert_eq!(p.kind_label(), "TypeScript");
    }

    #[test]
    fn search_matches_name_case_insensitively() {
        assert!(project().matches_query("coupon"));
        assert!(project().matches_query("COUPON"));
    }

    #[test]
    fn search_matches_tags_framework_and_path() {
        let p = project();
        assert!(p.matches_query("mv3"));
        assert!(p.matches_query("next.js"));
        assert!(p.matches_query("Extensions"));
    }

    #[test]
    fn search_rejects_non_matches() {
        assert!(!project().matches_query("factorio"));
    }

    #[test]
    fn empty_search_matches_everything() {
        assert!(project().matches_query("   "));
    }

    #[test]
    fn credential_shaped_env_names_default_to_masked() {
        assert!(EnvVar::new("STRIPE_SECRET_KEY", "sk_x").secret);
        assert!(EnvVar::new("github_token", "ghp_x").secret);
        assert!(EnvVar::new("DATABASE_PASSWORD", "x").secret);
        assert!(!EnvVar::new("PORT", "3000").secret);
        assert!(!EnvVar::new("NODE_ENV", "development").secret);
    }

    #[test]
    fn never_policy_never_restarts() {
        assert!(!RestartPolicy::Never.should_restart(Some(1)));
        assert!(!RestartPolicy::Never.should_restart(Some(0)));
    }

    #[test]
    fn on_failure_ignores_clean_exit_but_catches_signals() {
        let p = RestartPolicy::OnFailure { max_attempts: 3 };
        assert!(!p.should_restart(Some(0)));
        assert!(p.should_restart(Some(1)));
        assert!(p.should_restart(None));
    }

    #[test]
    fn always_policy_restarts_on_clean_exit() {
        let p = RestartPolicy::Always { max_attempts: 5 };
        assert!(p.should_restart(Some(0)));
    }

    #[test]
    fn backoff_grows_then_saturates() {
        assert_eq!(RestartPolicy::backoff_ms(1), 500);
        assert_eq!(RestartPolicy::backoff_ms(2), 1_000);
        assert_eq!(RestartPolicy::backoff_ms(3), 2_000);
        // Ceiling holds, and never overflows however many attempts are passed.
        assert_eq!(RestartPolicy::backoff_ms(7), 30_000);
        assert_eq!(RestartPolicy::backoff_ms(u32::MAX), 30_000);
    }

    #[test]
    fn kill_on_app_exit_defaults_on_via_both_paths() {
        // The struct literal and a deserialized empty object must agree; they
        // would not if Default were derived.
        let deserialized: ProjectOverrides = serde_json::from_str("{}").unwrap();
        assert!(deserialized.kill_on_app_exit);
        assert!(ProjectOverrides::default().kill_on_app_exit);
        assert_eq!(deserialized, ProjectOverrides::default());
    }
}
