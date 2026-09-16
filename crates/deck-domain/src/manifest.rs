//! Runner manifests: the declarative plugin format.
//!
//! A runner is a data file, not code. Adding support for a new language means
//! dropping a `.toml` into the runners directory -- no recompile, no rebuild,
//! no change to any crate. This module defines the shape of that file; the
//! engine that evaluates it lives in `deck-runners`.
//!
//! Two design decisions worth stating, because both are load-bearing:
//!
//! 1. **Detection rules are a closed set of predicates.** A rule can test for a
//!    file, glob a directory, or probe a key in a JSON/TOML file. It cannot run
//!    a command. Detection happens automatically over directories the user
//!    points us at, so a manifest that could execute during detection would
//!    turn "scan this folder" into "run whatever this folder says". Execution
//!    only ever happens from an explicit user action.
//!
//! 2. **Identity is separate from configuration.** A framework is a distinct
//!    manifest with a higher priority (Next.js beats generic Node), while a
//!    package manager is a *variable* resolved from lockfiles. That keeps one
//!    manifest per ecosystem rather than a combinatorial explosion of
//!    node-npm, node-pnpm, node-yarn files.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::command::{CommandSpec, Lifecycle};

/// A complete runner definition, as parsed from a `.toml` file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunnerManifest {
    /// Identity and presentation.
    pub meta: RunnerMeta,

    /// Predicates that must *all* pass for this runner to claim a directory.
    ///
    /// An empty list never matches. A runner that claims everything would make
    /// detection meaningless, so the engine rejects it at load time rather than
    /// treating it as a wildcard.
    #[serde(default)]
    pub detect: Vec<DetectionRule>,

    /// Variables available to this runner's command templates, resolved
    /// against the project directory at detection time.
    #[serde(default)]
    pub vars: Vec<VarRule>,

    /// The lifecycle steps this runner implements.
    ///
    /// [`Lifecycle::Run`] is required; the engine rejects a manifest without it
    /// because a runner that cannot start anything has no purpose here.
    #[serde(default)]
    pub commands: BTreeMap<Lifecycle, CommandSpec>,

    /// Where to read the project's own declared version, when it has one.
    #[serde(default)]
    pub version: Option<ValueSource>,

    /// Optional readiness probe, for distinguishing "process alive" from
    /// "actually serving".
    #[serde(default)]
    pub health: Option<HealthProbe>,

    /// Filesystem markers that say whether the project's dependencies are
    /// installed.
    ///
    /// This lives in the manifest rather than in Rust for the same reason
    /// detection does: adding an ecosystem must mean writing a `.toml`, never
    /// editing a crate. Like detection, it is **read-only** -- a setup probe
    /// that could execute would turn "show me my projects" into "run whatever
    /// they say".
    #[serde(default)]
    pub setup: Vec<SetupRule>,
}

/// One "are the dependencies installed?" rule.
///
/// Exists because 6 of 29 real projects in a live registry could not start, and
/// every one of them surfaced as a *crash* several seconds after the user hit
/// Run -- indistinguishable from the launcher breaking their app. The
/// information was on disk the whole time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupRule {
    /// Only meaningful when this file exists in the project root.
    ///
    /// Without it, a Python runner would report "no virtual environment" for a
    /// single-file script that never needed one.
    pub when_file_exists: String,

    /// Setup is satisfied if **any** of these paths exist, relative to the root.
    ///
    /// A list rather than one value because conventions genuinely differ:
    /// `.venv`, `venv` and `env` are all normal, and picking one would report a
    /// working project as broken.
    pub requires_any: Vec<String>,

    /// One line naming what is missing, shown directly to the user.
    pub hint: String,
}

/// Where to read a string value out of a project's own metadata.
///
/// Distinct from [`DetectionRule`], which answers yes/no. This extracts a value
/// -- a version, a name -- so `version` need not be a hardcoded per-ecosystem
/// special case in the engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueSource {
    /// Read a JSON file at an RFC 6901 pointer; a leading `/` is optional.
    Json {
        /// JSON file, relative to the project root.
        file: String,
        /// Key path within the document.
        pointer: String,
    },
    /// Read a TOML file at a dotted key path.
    Toml {
        /// TOML file, relative to the project root.
        file: String,
        /// Dotted key path within the document.
        path: String,
    },
}

/// Whether a project's dependencies are in place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum SetupState {
    /// No rule applied, or every rule is satisfied.
    Ready,
    /// A rule fired: the project will fail to start until this is fixed.
    NeedsSetup {
        /// What is missing, in one line.
        hint: String,
        /// The path whose absence triggered the rule, for the detail line.
        missing: String,
    },
}

impl SetupState {
    /// Whether launching is expected to fail.
    #[must_use]
    pub const fn needs_setup(&self) -> bool {
        matches!(self, Self::NeedsSetup { .. })
    }
}

impl SetupRule {
    /// Evaluates this rule against a project root.
    ///
    /// `exists` is injected rather than calling the filesystem directly so the
    /// logic is testable without building directory trees on disk -- and so
    /// this stays in `deck-domain`, which does no I/O by design.
    #[must_use]
    pub fn evaluate(&self, exists: &impl Fn(&str) -> bool) -> SetupState {
        // The trigger gates everything: a Python script with no
        // requirements.txt does not need a virtual environment, and saying so
        // would be noise on most of the list.
        if !exists(&self.when_file_exists) {
            return SetupState::Ready;
        }
        if self.requires_any.iter().any(|p| exists(p)) {
            return SetupState::Ready;
        }
        SetupState::NeedsSetup {
            hint: self.hint.clone(),
            missing: self.requires_any.join(" or "),
        }
    }
}

impl RunnerManifest {
    /// The first unsatisfied setup rule, or [`SetupState::Ready`].
    ///
    /// First rather than all: the user needs the next action, not an inventory.
    #[must_use]
    pub fn setup_state(&self, exists: &impl Fn(&str) -> bool) -> SetupState {
        self.setup
            .iter()
            .map(|rule| rule.evaluate(exists))
            .find(SetupState::needs_setup)
            .unwrap_or(SetupState::Ready)
    }

    /// The command for a lifecycle step, if this runner implements it.
    #[must_use]
    pub fn command(&self, step: Lifecycle) -> Option<&CommandSpec> {
        self.commands.get(&step)
    }

    /// The lifecycle steps this runner implements, in presentation order.
    #[must_use]
    pub fn supported(&self) -> Vec<Lifecycle> {
        Lifecycle::ALL
            .into_iter()
            .filter(|s| self.commands.contains_key(s))
            .collect()
    }
}

/// Identity and presentation for a runner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunnerMeta {
    /// Stable identifier, e.g. `node`, `python-poetry`, `dotnet`.
    ///
    /// Referenced by projects in the database, so renaming one orphans
    /// existing project rows. Treat as permanent once shipped.
    pub id: String,

    /// Human-readable name shown in the UI, e.g. `Node.js`.
    pub name: String,

    /// Language this runner covers, e.g. `TypeScript`, `Rust`.
    pub language: String,

    /// Framework, when the runner identifies one specifically.
    #[serde(default)]
    pub framework: Option<String>,

    /// Lucide icon name used on cards and rows.
    #[serde(default)]
    pub icon: Option<String>,

    /// Tie-break when several runners match the same directory. Higher wins.
    ///
    /// Convention: generic ecosystem runners sit at 100, framework-specific
    /// refinements at 200+, so a Next.js manifest naturally outranks the
    /// generic Node one without either knowing about the other.
    #[serde(default = "default_priority")]
    pub priority: i32,

    /// Ports this project type conventionally uses, for the port-conflict
    /// check before launch.
    #[serde(default)]
    pub default_ports: Vec<u16>,
}

const fn default_priority() -> i32 {
    100
}

/// A predicate over the contents of a project directory.
///
/// Every variant is a pure filesystem or parse check. Deliberately no variant
/// executes anything -- see the module docs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectionRule {
    /// A file exists at this path, relative to the project root.
    FileExists(String),

    /// A directory exists at this path, relative to the project root.
    DirExists(String),

    /// At least one file in the project root matches this glob.
    ///
    /// Matches the root only, not recursively: `*.csproj` should identify a
    /// project directory, not any directory with a `.csproj` buried somewhere
    /// beneath it.
    GlobMatches(String),

    /// A JSON file contains this key path, e.g. `scripts/dev`.
    ///
    /// The pointer is RFC 6901; a leading `/` is optional for readability.
    JsonKeyExists {
        /// JSON file, relative to the project root.
        file: String,
        /// Key path within the document.
        pointer: String,
    },

    /// A JSON file contains this key path with this exact string value.
    JsonKeyEquals {
        /// JSON file, relative to the project root.
        file: String,
        /// Key path within the document.
        pointer: String,
        /// Expected value.
        value: String,
    },

    /// A TOML file contains this dotted key path, e.g. `tool.poetry`.
    TomlKeyExists {
        /// TOML file, relative to the project root.
        file: String,
        /// Dotted key path within the document.
        path: String,
    },

    /// A text file contains this substring.
    ///
    /// Reads at most the first 256 KiB, so pointing this at a large file is
    /// bounded rather than catastrophic.
    FileContains {
        /// File, relative to the project root.
        file: String,
        /// Substring to search for.
        text: String,
    },

    /// Inverts the wrapped rule.
    Not(Box<DetectionRule>),

    /// Every wrapped rule must pass.
    AllOf(Vec<DetectionRule>),

    /// At least one wrapped rule must pass.
    AnyOf(Vec<DetectionRule>),
}

/// A template variable resolved from the project directory.
///
/// Resolution order is `cases`, then `from_value`, then `first_match`, then
/// `default` -- most specific to least. Cases are evaluated in order and the
/// first match wins, so a lockfile priority list reads top to bottom exactly as
/// written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VarRule {
    /// Variable name, referenced in command templates as `{name}`.
    pub name: String,

    /// Value when nothing else resolves.
    pub default: String,

    /// Ordered candidate values with the condition that selects each.
    #[serde(default)]
    pub cases: Vec<VarCase>,

    /// Read the value out of the project's own metadata.
    #[serde(default)]
    pub from_value: Option<ValueSource>,

    /// Use the first filename in the project root matching this glob.
    ///
    /// The primitive that makes script and executable runners work: a directory
    /// containing `deploy.ps1` needs a run command naming *that* file, which no
    /// fixed template can express. Matches are sorted before taking the first,
    /// so the choice does not depend on directory iteration order.
    #[serde(default)]
    pub first_match: Option<String>,

    /// Ordered [`Self::first_match`] patterns; the first that matches wins.
    ///
    /// Exists because the same artefact lives in different places depending on
    /// project layout: a standalone Tauri app builds to
    /// `src-tauri/target/release/`, but one inside a Cargo workspace builds to
    /// `target/release/` at the workspace root. Expressing that as two runners
    /// would duplicate an entire manifest to vary one path.
    #[serde(default)]
    pub first_match_any: Vec<String>,
}

/// One branch of a [`VarRule`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VarCase {
    /// Value to use when `when` passes.
    pub value: String,

    /// Condition selecting this value.
    pub when: DetectionRule,
}

/// An optional readiness check, distinguishing "started" from "serving".
///
/// Without this a dev server counts as up the instant the process exists, which
/// is wrong by several seconds and makes "open in browser" fail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthProbe {
    /// Poll an HTTP endpoint until it answers.
    Http {
        /// URL to poll. Supports template variables, typically `{port}`.
        url: String,
        /// Status codes counted as healthy. Defaults to 200-399.
        #[serde(default)]
        expect_status: Vec<u16>,
    },
    /// Wait until the process is listening on a TCP port.
    TcpPort {
        /// Port to test, templated.
        port: String,
    },
    /// Wait for a line matching this substring on stdout or stderr.
    ///
    /// The pragmatic option for tools that print "ready in 431ms" and bind an
    /// unpredictable port.
    LogContains {
        /// Substring marking readiness.
        text: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_representative_manifest() {
        // Note the `[[detect]]` form. An inline `detect = [...]` placed after
        // `[meta]` would be parsed as `meta.detect` and silently leave the real
        // rule list empty; the array-of-tables form is order-independent, so it
        // is the one the bundled manifests and the docs use.
        let src = r#"
            [meta]
            id = "node"
            name = "Node.js"
            language = "JavaScript"
            icon = "hexagon"
            priority = 100
            default_ports = [3000, 5173]

            [[detect]]
            file_exists = "package.json"

            [[vars]]
            name = "pm"
            default = "npm"
            cases = [
              { value = "pnpm", when = { file_exists = "pnpm-lock.yaml" } },
              { value = "yarn", when = { file_exists = "yarn.lock" } },
            ]

            [commands.run]
            exec = { program = "{pm}", args = ["run", "{script}"] }
        "#;

        let m: RunnerManifest = toml::from_str(src).expect("manifest should parse");
        assert_eq!(m.meta.id, "node");
        assert_eq!(m.meta.priority, 100);
        assert_eq!(m.meta.default_ports, vec![3000, 5173]);
        assert_eq!(m.detect.len(), 1);
        assert_eq!(m.vars[0].cases.len(), 2);
        assert!(m.command(Lifecycle::Run).is_some());
        assert!(m.command(Lifecycle::Build).is_none());
    }

    #[test]
    fn priority_defaults_when_absent() {
        let src = r#"
            [meta]
            id = "x"
            name = "X"
            language = "X"
            [[detect]]
            file_exists = "x"
            [commands.run]
            exec = { program = "x", args = [] }
        "#;
        let m: RunnerManifest = toml::from_str(src).unwrap();
        assert_eq!(m.meta.priority, default_priority());
    }

    #[test]
    fn supported_steps_are_returned_in_presentation_order() {
        let src = r#"
            [meta]
            id = "x"
            name = "X"
            language = "X"
            [[detect]]
            file_exists = "x"
            [commands.run]
            exec = { program = "x", args = [] }
            [commands.install]
            exec = { program = "x", args = ["i"] }
        "#;
        let m: RunnerManifest = toml::from_str(src).unwrap();
        // Install precedes Run in Lifecycle::ALL, and must here too.
        assert_eq!(m.supported(), vec![Lifecycle::Install, Lifecycle::Run]);
    }

    #[test]
    fn nested_rule_combinators_round_trip() {
        let rule = DetectionRule::AllOf(vec![
            DetectionRule::FileExists("pyproject.toml".into()),
            DetectionRule::Not(Box::new(DetectionRule::FileExists("Pipfile".into()))),
            DetectionRule::AnyOf(vec![DetectionRule::TomlKeyExists {
                file: "pyproject.toml".into(),
                path: "tool.poetry".into(),
            }]),
        ]);
        let encoded = toml::to_string(&VarCase {
            value: "poetry".into(),
            when: rule.clone(),
        })
        .unwrap();
        let decoded: VarCase = toml::from_str(&encoded).unwrap();
        assert_eq!(decoded.when, rule);
    }
}

#[cfg(test)]
mod setup_rules {
    use super::*;

    fn rule() -> SetupRule {
        SetupRule {
            when_file_exists: "package.json".into(),
            requires_any: vec!["node_modules".into()],
            hint: "Dependencies are not installed".into(),
        }
    }

    #[test]
    fn a_project_without_the_trigger_file_needs_nothing() {
        // A single-file script has no package.json, so "no node_modules" is
        // not a finding -- it is the normal state of most of the list.
        let state = rule().evaluate(&|_: &str| false);
        assert_eq!(state, SetupState::Ready);
    }

    #[test]
    fn the_trigger_without_the_marker_is_a_finding() {
        let state = rule().evaluate(&|p: &str| p == "package.json");
        assert!(state.needs_setup(), "missing node_modules went unreported");
        match state {
            SetupState::NeedsSetup { missing, .. } => assert_eq!(missing, "node_modules"),
            SetupState::Ready => unreachable!(),
        }
    }

    #[test]
    fn the_marker_present_is_ready() {
        let state = rule().evaluate(&|p: &str| p == "package.json" || p == "node_modules");
        assert_eq!(state, SetupState::Ready);
    }

    #[test]
    fn any_of_the_accepted_markers_satisfies_the_rule() {
        // Picking one convention would report a perfectly working project as
        // broken: .venv, venv and env are all normal.
        let py = SetupRule {
            when_file_exists: "requirements.txt".into(),
            requires_any: vec![".venv".into(), "venv".into(), "env".into()],
            hint: "No virtual environment".into(),
        };
        for marker in [".venv", "venv", "env"] {
            let state = py.evaluate(&|p: &str| p == "requirements.txt" || p == marker);
            assert_eq!(state, SetupState::Ready, "{marker} should satisfy the rule");
        }
        assert!(py.evaluate(&|p: &str| p == "requirements.txt").needs_setup());
    }

    #[test]
    fn the_first_unsatisfied_rule_wins() {
        // Parsed from TOML rather than constructed, so this also proves the
        // schema deserialises -- a rule the engine cannot read is a rule that
        // silently never fires.
        let manifest: RunnerManifest = toml::from_str(
            r#"
            [meta]
            id = "t"
            name = "T"
            language = "T"

            [commands.run]
            exec = { program = "x", args = [] }

            [[setup]]
            when_file_exists = "a"
            requires_any = ["ok"]
            hint = "first"

            [[setup]]
            when_file_exists = "b"
            requires_any = ["nope"]
            hint = "second"
            "#,
        )
        .expect("manifest with setup rules should parse");

        assert_eq!(manifest.setup.len(), 2, "setup rules were dropped on parse");

        // Only the second rule fires; the user gets one next action, not a list.
        let state = manifest.setup_state(&|p: &str| matches!(p, "a" | "b" | "ok"));
        match state {
            SetupState::NeedsSetup { hint, .. } => assert_eq!(hint, "second"),
            SetupState::Ready => panic!("the unsatisfied second rule was not reported"),
        }

        // Everything satisfied is Ready, not "the last rule's verdict".
        assert_eq!(
            manifest.setup_state(&|p: &str| matches!(p, "a" | "b" | "ok" | "nope")),
            SetupState::Ready
        );
    }
}
