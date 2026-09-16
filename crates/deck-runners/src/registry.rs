//! The runner registry: loading manifests, validating them, and arbitrating
//! between competing detections.
//!
//! Manifests come from two places, in this order of precedence:
//!
//! 1. **Bundled** -- compiled into the binary via `include_str!`, so a fresh
//!    install detects Node, Rust, Python and the rest with no setup.
//! 2. **User** -- `.toml` files in the app's runners directory. A user manifest
//!    with the same id as a bundled one *replaces* it, which is how someone
//!    overrides a shipped default without editing the install.
//!
//! Arbitration is by `priority`, then by id for stability. Ties broken by id
//! rather than by load order matter more than they sound: without it, two
//! equal-priority runners would resolve according to directory iteration order,
//! and a project's detected type could change between launches.

use std::path::{Path, PathBuf};

use deck_domain::command::Lifecycle;
use deck_domain::error::{DeckError, Result};
use deck_domain::manifest::RunnerManifest;
use deck_domain::runner::{DetectionMatch, Runner};

use crate::builtin;
use crate::manifest_runner::ManifestRunner;
use crate::probe::ProbeCache;

/// A problem with one manifest, collected rather than fatal.
///
/// One malformed user manifest must not stop the app from starting -- it starts,
/// works with the runners that loaded, and reports what it could not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestProblem {
    /// Where the manifest came from. Empty for bundled manifests.
    pub path: PathBuf,
    /// Manifest id, when it parsed far enough to have one.
    pub id: Option<String>,
    /// Why it was rejected.
    pub reason: String,
}

/// Every runner available to the app.
pub struct RunnerRegistry {
    runners: Vec<ManifestRunner>,
    problems: Vec<ManifestProblem>,
}

impl RunnerRegistry {
    /// Loads only the bundled manifests.
    ///
    /// # Errors
    ///
    /// Returns an error only if a *bundled* manifest is invalid, which is a
    /// build-time mistake rather than a runtime condition -- the accompanying
    /// test asserts they all load, so this cannot ship broken.
    pub fn bundled() -> Result<Self> {
        let mut registry = Self {
            runners: Vec::new(),
            problems: Vec::new(),
        };

        for (name, source) in builtin::MANIFESTS {
            let manifest = toml::from_str::<RunnerManifest>(source).map_err(|e| {
                DeckError::InvalidManifest {
                    path: PathBuf::from(*name),
                    reason: e.to_string(),
                }
            })?;
            validate(&manifest).map_err(|reason| DeckError::InvalidManifest {
                path: PathBuf::from(*name),
                reason,
            })?;
            registry.insert(ManifestRunner::new(manifest, PathBuf::new()));
        }

        Ok(registry)
    }

    /// Loads bundled manifests, then overlays any user manifests from `dir`.
    ///
    /// A missing directory is not an error: most users never add a runner, and
    /// the app must start cleanly for them.
    ///
    /// # Errors
    ///
    /// Propagates a failure only from the bundled set. Problems with user
    /// manifests are collected into [`Self::problems`].
    pub fn load(dir: &Path) -> Result<Self> {
        let mut registry = Self::bundled()?;

        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(registry),
            Err(e) => {
                registry.problems.push(ManifestProblem {
                    path: dir.to_path_buf(),
                    id: None,
                    reason: format!("could not read runners directory: {e}"),
                });
                return Ok(registry);
            }
        };

        // Sort by path so load order is deterministic across filesystems.
        let mut paths: Vec<PathBuf> = entries
            .filter_map(std::result::Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("toml")))
            .collect();
        paths.sort();

        for path in paths {
            match load_one(&path) {
                Ok(runner) => {
                    tracing::info!(
                        runner = %runner.meta().id,
                        path = %path.display(),
                        "loaded user runner"
                    );
                    registry.insert(runner);
                }
                Err(problem) => {
                    tracing::warn!(
                        path = %problem.path.display(),
                        reason = %problem.reason,
                        "skipping invalid runner manifest"
                    );
                    registry.problems.push(problem);
                }
            }
        }

        Ok(registry)
    }

    /// Inserts a runner, replacing any existing one with the same id.
    fn insert(&mut self, runner: ManifestRunner) {
        let id = runner.meta().id.clone();
        if let Some(slot) = self.runners.iter_mut().find(|r| r.meta().id == id) {
            *slot = runner;
        } else {
            self.runners.push(runner);
        }
    }

    /// All loaded runners.
    #[must_use]
    pub fn runners(&self) -> &[ManifestRunner] {
        &self.runners
    }

    /// Manifests that could not be loaded.
    #[must_use]
    pub fn problems(&self) -> &[ManifestProblem] {
        &self.problems
    }

    /// A runner by id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&ManifestRunner> {
        self.runners.iter().find(|r| r.meta().id == id)
    }

    /// Identifies the project type of a directory.
    ///
    /// Consults every runner against one shared [`ProbeCache`], then returns the
    /// highest-priority match. `None` means nothing claimed the directory -- an
    /// ordinary outcome the add-project flow handles by asking the user for a run
    /// command, not an error.
    #[must_use]
    pub fn detect(&self, root: &Path) -> Option<DetectionMatch> {
        let cache = ProbeCache::new(root);
        let mut best: Option<DetectionMatch> = None;

        for runner in &self.runners {
            let Some(candidate) = runner.detect_with_cache(&cache) else {
                continue;
            };
            let better = match &best {
                None => true,
                Some(current) => {
                    (candidate.priority(), candidate.meta.id.as_str())
                        > (current.priority(), current.meta.id.as_str())
                }
            };
            if better {
                best = Some(candidate);
            }
        }

        if let Some(m) = &best {
            tracing::debug!(
                root = %root.display(),
                runner = %m.meta.id,
                priority = m.priority(),
                "detected project type"
            );
        }

        best
    }

    /// Every runner that claims a directory, best first.
    ///
    /// Used by the add-project dialog to offer alternatives when the automatic
    /// choice is wrong -- a Vite app that the user would rather treat as generic
    /// Node, say. Detection proposes; the user decides.
    #[must_use]
    pub fn detect_all(&self, root: &Path) -> Vec<DetectionMatch> {
        let cache = ProbeCache::new(root);
        let mut matches = Vec::new();
        for runner in &self.runners {
            if let Some(m) = runner.detect_with_cache(&cache) {
                matches.push(m);
            }
        }
        matches.sort_by(|a, b| {
            b.priority()
                .cmp(&a.priority())
                .then_with(|| b.meta.id.cmp(&a.meta.id))
        });
        matches
    }
}

/// Reads and validates one manifest file.
fn load_one(path: &Path) -> std::result::Result<ManifestRunner, ManifestProblem> {
    let problem = |reason: String, id: Option<String>| ManifestProblem {
        path: path.to_path_buf(),
        id,
        reason,
    };

    let source = std::fs::read_to_string(path)
        .map_err(|e| problem(format!("could not read file: {e}"), None))?;

    let manifest: RunnerManifest =
        toml::from_str(&source).map_err(|e| problem(e.to_string(), None))?;

    let id = Some(manifest.meta.id.clone());
    validate(&manifest).map_err(|reason| problem(reason, id))?;

    let base = path.parent().unwrap_or(Path::new("")).to_path_buf();
    Ok(ManifestRunner::new(manifest, base))
}

/// Rejects manifests that would misbehave at runtime.
///
/// These checks exist because each corresponds to a mistake a plugin author will
/// actually make, and each fails confusingly rather than obviously if allowed
/// through. In particular the empty-`detect` check catches writing
/// `detect = [...]` after `[meta]`, where TOML folds it into the `meta` table and
/// silently leaves the real list empty.
fn validate(manifest: &RunnerManifest) -> std::result::Result<(), String> {
    if manifest.meta.id.trim().is_empty() {
        return Err("meta.id must not be empty".to_owned());
    }

    if !manifest
        .meta
        .id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!(
            "meta.id `{}` must contain only letters, digits, hyphen and underscore \
             (it is used in file paths)",
            manifest.meta.id
        ));
    }

    if manifest.detect.is_empty() {
        return Err(
            "no detection rules -- a runner that matched every directory would make \
             detection meaningless. Note that `detect = [...]` written after the \
             `[meta]` table is parsed as `meta.detect`; use `[[detect]]` blocks instead"
                .to_owned(),
        );
    }

    if !manifest.commands.contains_key(&Lifecycle::Run) {
        return Err("no [commands.run] -- a runner must be able to start something".to_owned());
    }

    for var in &manifest.vars {
        if var.name.trim().is_empty() {
            return Err("a [[vars]] entry has an empty name".to_owned());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new(tag: &str) -> Self {
            use std::time::{SystemTime, UNIX_EPOCH};
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let mut path = std::env::temp_dir();
            path.push(format!("deck-reg-{tag}-{nanos:x}"));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn sub(&self, name: &str) -> PathBuf {
            let p = self.0.join(name);
            std::fs::create_dir_all(&p).unwrap();
            p
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn manifest_src(id: &str, priority: i32, detect_file: &str) -> String {
        format!(
            r#"
            [meta]
            id = "{id}"
            name = "{id}"
            language = "Test"
            priority = {priority}

            [[detect]]
            file_exists = "{detect_file}"

            [commands.run]
            exec = {{ program = "{id}", args = [] }}
            "#
        )
    }

    // ---- Bundled manifests -------------------------------------------------

    #[test]
    fn every_bundled_manifest_is_valid() {
        // This is the guard that keeps a broken shipped runner from ever
        // reaching a user: `bundled()` returns Err on the first bad manifest.
        let registry = RunnerRegistry::bundled().expect("bundled manifests must all be valid");
        assert!(
            registry.runners().len() >= 15,
            "expected a broad set of bundled runners, got {}",
            registry.runners().len()
        );
        assert!(registry.problems().is_empty());
    }

    #[test]
    fn bundled_runner_ids_are_unique() {
        let registry = RunnerRegistry::bundled().unwrap();
        let mut ids: Vec<&str> = registry.runners().iter().map(|r| r.meta().id.as_str()).collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total, "duplicate runner id among bundled manifests");
    }

    #[test]
    fn bundled_set_covers_the_expected_ecosystems() {
        let registry = RunnerRegistry::bundled().unwrap();
        for id in [
            "node", "python", "rust", "go", "dotnet", "java-maven", "cmake", "make",
            "powershell", "shell", "php", "ruby", "lua", "deno", "bun", "flutter",
            "executable",
        ] {
            assert!(registry.get(id).is_some(), "missing bundled runner `{id}`");
        }
    }

    // ---- Validation --------------------------------------------------------

    #[test]
    fn manifest_without_detection_rules_is_rejected() {
        let src = r#"
            [meta]
            id = "bad"
            name = "Bad"
            language = "X"
            [commands.run]
            exec = { program = "x", args = [] }
        "#;
        let m: RunnerManifest = toml::from_str(src).unwrap();
        let err = validate(&m).unwrap_err();
        assert!(err.contains("detection rules"));
        // And it points at the TOML folding trap that causes this in practice.
        assert!(err.contains("[[detect]]"));
    }

    #[test]
    fn manifest_without_a_run_command_is_rejected() {
        let src = r#"
            [meta]
            id = "bad"
            name = "Bad"
            language = "X"
            [[detect]]
            file_exists = "x"
            [commands.build]
            exec = { program = "x", args = [] }
        "#;
        let m: RunnerManifest = toml::from_str(src).unwrap();
        assert!(validate(&m).unwrap_err().contains("commands.run"));
    }

    #[test]
    fn manifest_with_a_path_unsafe_id_is_rejected() {
        let m: RunnerManifest =
            toml::from_str(&manifest_src("../evil", 100, "x")).unwrap();
        assert!(validate(&m).unwrap_err().contains("letters, digits"));
    }

    #[test]
    fn the_inline_detect_after_meta_trap_is_caught() {
        // This is valid TOML but folds `detect` into `meta`, leaving the real
        // rule list empty. Validation must catch it rather than shipping a
        // runner that claims nothing (or, worse, everything).
        let src = r#"
            [meta]
            id = "trap"
            name = "Trap"
            language = "X"
            detect = [{ file_exists = "package.json" }]

            [commands.run]
            exec = { program = "x", args = [] }
        "#;
        let m: RunnerManifest = toml::from_str(src).unwrap();
        assert!(m.detect.is_empty());
        assert!(validate(&m).is_err());
    }

    // ---- Loading -----------------------------------------------------------

    #[test]
    fn missing_runners_directory_is_not_an_error() {
        let registry =
            RunnerRegistry::load(Path::new(r"D:\no-such-runners-dir-xyz")).unwrap();
        assert!(registry.problems().is_empty());
        assert!(!registry.runners().is_empty());
    }

    #[test]
    fn a_user_manifest_is_loaded_alongside_the_bundled_ones() {
        let f = Fixture::new("userload");
        let dir = f.sub("runners");
        std::fs::write(dir.join("zig.toml"), manifest_src("zig", 150, "build.zig")).unwrap();

        let registry = RunnerRegistry::load(&dir).unwrap();
        assert!(registry.problems().is_empty());
        assert!(registry.get("zig").is_some());
        // Bundled runners are still there.
        assert!(registry.get("node").is_some());
    }

    #[test]
    fn a_user_manifest_replaces_a_bundled_one_with_the_same_id() {
        let f = Fixture::new("override");
        let dir = f.sub("runners");
        std::fs::write(dir.join("node.toml"), manifest_src("node", 100, "package.json"))
            .unwrap();

        let registry = RunnerRegistry::load(&dir).unwrap();
        // Exactly one `node`, and it is the user's (language "Test", not
        // "JavaScript"), proving replacement rather than duplication.
        let nodes: Vec<_> = registry
            .runners()
            .iter()
            .filter(|r| r.meta().id == "node")
            .collect();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].meta().language, "Test");
    }

    #[test]
    fn one_broken_manifest_does_not_stop_the_others_loading() {
        let f = Fixture::new("broken");
        let dir = f.sub("runners");
        std::fs::write(dir.join("aaa-broken.toml"), "this is not [ valid toml").unwrap();
        std::fs::write(dir.join("zzz-good.toml"), manifest_src("zig", 150, "build.zig"))
            .unwrap();

        let registry = RunnerRegistry::load(&dir).unwrap();
        assert_eq!(registry.problems().len(), 1);
        assert!(registry.problems()[0].path.ends_with("aaa-broken.toml"));
        // The good one still loaded, despite sorting after the broken one.
        assert!(registry.get("zig").is_some());
    }

    #[test]
    fn non_toml_files_are_ignored() {
        let f = Fixture::new("nontoml");
        let dir = f.sub("runners");
        std::fs::write(dir.join("README.md"), "not a manifest").unwrap();
        std::fs::write(dir.join("notes.txt"), "also not").unwrap();
        let registry = RunnerRegistry::load(&dir).unwrap();
        assert!(registry.problems().is_empty());
    }

    // ---- Arbitration -------------------------------------------------------

    #[test]
    fn highest_priority_runner_wins() {
        let f = Fixture::new("priority");
        let dir = f.sub("runners");
        std::fs::write(dir.join("a-low.toml"), manifest_src("aaa-low", 10, "marker")).unwrap();
        std::fs::write(dir.join("b-high.toml"), manifest_src("bbb-high", 900, "marker"))
            .unwrap();
        let project = f.sub("proj");
        std::fs::write(project.join("marker"), "").unwrap();

        let registry = RunnerRegistry::load(&dir).unwrap();
        let m = registry.detect(&project).unwrap();
        assert_eq!(m.meta.id, "bbb-high");
    }

    #[test]
    fn equal_priority_ties_break_deterministically_by_id() {
        let f = Fixture::new("tie");
        let dir = f.sub("runners");
        std::fs::write(dir.join("one.toml"), manifest_src("alpha", 500, "marker")).unwrap();
        std::fs::write(dir.join("two.toml"), manifest_src("omega", 500, "marker")).unwrap();
        let project = f.sub("proj");
        std::fs::write(project.join("marker"), "").unwrap();

        let registry = RunnerRegistry::load(&dir).unwrap();
        // Repeated detection must give the same answer every time -- otherwise a
        // project's type could change between app launches.
        for _ in 0..5 {
            assert_eq!(registry.detect(&project).unwrap().meta.id, "omega");
        }
    }

    #[test]
    fn unclaimed_directory_detects_as_none() {
        let f = Fixture::new("unknown");
        let project = f.sub("mystery");
        std::fs::write(project.join("notes.txt"), "just some text").unwrap();
        let registry = RunnerRegistry::bundled().unwrap();
        assert!(registry.detect(&project).is_none());
    }

    #[test]
    fn detect_all_returns_every_match_best_first() {
        let f = Fixture::new("all");
        let dir = f.sub("runners");
        std::fs::write(dir.join("lo.toml"), manifest_src("lo", 10, "marker")).unwrap();
        std::fs::write(dir.join("mid.toml"), manifest_src("mid", 300, "marker")).unwrap();
        std::fs::write(dir.join("hi.toml"), manifest_src("hi", 800, "marker")).unwrap();
        let project = f.sub("proj");
        std::fs::write(project.join("marker"), "").unwrap();

        let registry = RunnerRegistry::load(&dir).unwrap();
        let matches = registry.detect_all(&project);
        let ids: Vec<&str> = matches.iter().map(|m| m.meta.id.as_str()).collect();
        assert_eq!(ids, vec!["hi", "mid", "lo"]);
    }

    // ---- Real-world detection ---------------------------------------------

    #[test]
    fn a_next_js_project_beats_generic_node() {
        let f = Fixture::new("nextjs");
        let project = f.sub("web");
        std::fs::write(
            project.join("package.json"),
            r#"{"name":"web","version":"1.0.0",
                "scripts":{"dev":"next dev","build":"next build"},
                "dependencies":{"next":"15.1.0","react":"19.0.0"}}"#,
        )
        .unwrap();

        let registry = RunnerRegistry::bundled().unwrap();
        let m = registry.detect(&project).unwrap();
        assert_eq!(m.meta.framework.as_deref(), Some("Next.js"));
        assert!(m.priority() > 100, "framework runner must outrank generic node");
    }

    #[test]
    fn a_plain_node_project_detects_as_node() {
        let f = Fixture::new("plainnode");
        let project = f.sub("api");
        std::fs::write(
            project.join("package.json"),
            r#"{"name":"api","scripts":{"start":"node index.js"}}"#,
        )
        .unwrap();
        let registry = RunnerRegistry::bundled().unwrap();
        let m = registry.detect(&project).unwrap();
        assert_eq!(m.meta.id, "node");
        assert_eq!(m.vars.get("script"), Some("start"));
    }

    #[test]
    fn a_cargo_project_detects_as_rust_with_its_version() {
        let f = Fixture::new("cargo");
        let project = f.sub("cli");
        std::fs::write(
            project.join("Cargo.toml"),
            "[package]\nname = \"cli\"\nversion = \"0.5.2\"\nedition = \"2021\"\n",
        )
        .unwrap();
        let registry = RunnerRegistry::bundled().unwrap();
        let m = registry.detect(&project).unwrap();
        assert_eq!(m.meta.id, "rust");
        assert_eq!(m.version.as_deref(), Some("0.5.2"));
    }

    #[test]
    fn a_tauri_project_beats_plain_rust_and_plain_node() {
        let f = Fixture::new("tauri");
        let project = f.sub("app");
        std::fs::write(
            project.join("package.json"),
            r#"{"name":"app","scripts":{"tauri":"tauri","dev":"vite"},
                "devDependencies":{"@tauri-apps/cli":"^2"}}"#,
        )
        .unwrap();
        std::fs::create_dir_all(project.join("src-tauri")).unwrap();
        std::fs::write(
            project.join("src-tauri/Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();

        let registry = RunnerRegistry::bundled().unwrap();
        let m = registry.detect(&project).unwrap();
        assert_eq!(m.meta.id, "tauri");
    }

    #[test]
    fn a_poetry_project_beats_generic_python() {
        let f = Fixture::new("poetry");
        let project = f.sub("svc");
        std::fs::write(
            project.join("pyproject.toml"),
            "[tool.poetry]\nname = \"svc\"\nversion = \"1.1.0\"\n",
        )
        .unwrap();
        let registry = RunnerRegistry::bundled().unwrap();
        let m = registry.detect(&project).unwrap();
        assert_eq!(m.meta.id, "python-poetry");
        assert_eq!(m.version.as_deref(), Some("1.1.0"));
    }

    #[test]
    fn a_dotnet_project_is_found_by_glob() {
        let f = Fixture::new("dotnet");
        let project = f.sub("Service");
        std::fs::write(project.join("Service.csproj"), "<Project Sdk=\"Microsoft.NET.Sdk\"/>")
            .unwrap();
        let registry = RunnerRegistry::bundled().unwrap();
        assert_eq!(registry.detect(&project).unwrap().meta.id, "dotnet");
    }

    #[test]
    fn detection_is_stable_across_repeated_calls() {
        let f = Fixture::new("stable");
        let project = f.sub("web");
        std::fs::write(
            project.join("package.json"),
            r#"{"scripts":{"dev":"vite"},"devDependencies":{"vite":"^6"}}"#,
        )
        .unwrap();
        let registry = RunnerRegistry::bundled().unwrap();
        let first = registry.detect(&project).unwrap();
        for _ in 0..5 {
            let again = registry.detect(&project).unwrap();
            assert_eq!(again.meta.id, first.meta.id);
            assert_eq!(again.vars, first.vars);
        }
    }
}
