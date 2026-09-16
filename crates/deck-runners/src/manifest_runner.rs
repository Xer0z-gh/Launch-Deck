//! [`ManifestRunner`]: a [`Runner`] whose behaviour comes entirely from a
//! `.toml` file.
//!
//! This is the only `Runner` implementation Launch Deck ships, and it is what
//! makes the plugin promise real -- every supported language, from Node to
//! `PowerShell`, is this same type with different data behind it. Adding a
//! language adds a file.

use std::path::Path;

use deck_domain::command::{Lifecycle, ResolvedCommand, TemplateVars};
use deck_domain::error::{DeckError, Result};
use deck_domain::manifest::{RunnerManifest, RunnerMeta};
use deck_domain::runner::{DetectionMatch, Runner};

use crate::probe::ProbeCache;
use crate::rule;

/// A runner defined by a manifest.
#[derive(Debug, Clone)]
pub struct ManifestRunner {
    manifest: RunnerManifest,
    /// Directory the manifest was loaded from, used to resolve `script` paths.
    base_dir: std::path::PathBuf,
}

impl ManifestRunner {
    /// Wraps a validated manifest.
    ///
    /// Validation lives in [`crate::registry`]; by the time a manifest reaches
    /// here it is known to have detection rules and a run command.
    #[must_use]
    pub fn new(manifest: RunnerManifest, base_dir: impl Into<std::path::PathBuf>) -> Self {
        Self {
            manifest,
            base_dir: base_dir.into(),
        }
    }

    /// The underlying manifest.
    #[must_use]
    pub const fn manifest(&self) -> &RunnerManifest {
        &self.manifest
    }

    /// Directory the manifest was loaded from.
    #[must_use]
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Resolves this runner's declared variables against a directory,
    /// regardless of whether detection would match it.
    ///
    /// Used at launch time: even when the directory has drifted away from the
    /// stored detection, variables still resolve from lockfile cases, values
    /// and `first_match` rules -- or fall back to their declared defaults -- so
    /// a launch degrades gracefully instead of failing on an unresolved `{pm}`.
    #[must_use]
    pub fn vars_for(&self, root: &std::path::Path) -> TemplateVars {
        self.resolve_vars(&ProbeCache::new(root))
    }

    /// Resolves this runner's declared variables against a directory.
    ///
    /// Each [`VarRule`](deck_domain::manifest::VarRule)'s cases are tried in
    /// order and the first match wins, so a lockfile priority list reads top to
    /// bottom exactly as written.
    fn resolve_vars(&self, cache: &ProbeCache) -> TemplateVars {
        let mut vars = TemplateVars::new();
        for var in &self.manifest.vars {
            let chosen = var
                .cases
                .iter()
                .find(|case| rule::evaluate(&case.when, cache))
                .map(|case| case.value.clone())
                .or_else(|| {
                    var.from_value
                        .as_ref()
                        .and_then(|source| rule::read_value(source, cache))
                })
                .or_else(|| {
                    var.first_match
                        .as_ref()
                        .and_then(|pattern| first_matching_entry(cache, pattern, &vars))
                })
                .or_else(|| {
                    var.first_match_any
                        .iter()
                        .find_map(|pattern| first_matching_entry(cache, pattern, &vars))
                })
                .unwrap_or_else(|| var.default.clone());
            vars.set(&var.name, chosen);
        }
        vars
    }
}

/// The lexicographically first root entry matching `pattern`, if any.
///
/// Sorted rather than "whatever `read_dir` yielded first" so a directory with
/// both `build.ps1` and `deploy.ps1` resolves the same way on every launch.
fn first_matching_entry(
    cache: &ProbeCache,
    pattern: &str,
    vars: &TemplateVars,
) -> Option<String> {
    // Patterns may reference variables resolved above them, which is what lets
    // a manifest say "the binary named after this package" rather than "any
    // binary in this directory". `tauri-built` needs exactly that: in a Cargo
    // workspace `target/release/` holds every member's executable, so a bare
    // `*.exe` picked a CLI tool and launched it instead of the app.
    //
    // Vars resolve top to bottom, so a pattern can only use one declared
    // earlier -- the same rule command templates already follow. An unknown
    // name leaves the pattern unexpanded and it simply matches nothing.
    let expanded = vars.expand(pattern).unwrap_or_else(|_| pattern.to_owned());
    let pattern = expanded.as_str();
    let (dir, name_pattern) = crate::glob::split_dir(pattern);
    let entries = match dir {
        None => cache.root_entries(),
        Some(dir) => cache.entries_in(dir),
    };

    let mut matches: Vec<String> = entries
        .into_iter()
        .filter(|name| crate::glob::matches(name, name_pattern))
        .collect();
    // Sorted so the choice never depends on directory iteration order.
    matches.sort();
    let first = matches.into_iter().next()?;

    // Return the path as written so a template can use it directly; a bare
    // filename stays bare, which is what the script and executable runners
    // already depend on.
    Some(match dir {
        Some(dir) => format!("{dir}/{first}"),
        None => first,
    })
}

impl Runner for ManifestRunner {
    fn meta(&self) -> &RunnerMeta {
        &self.manifest.meta
    }

    fn detect(&self, root: &Path) -> Result<Option<DetectionMatch>> {
        Ok(self.detect_with_cache(&ProbeCache::new(root)))
    }

    fn supported(&self) -> Vec<Lifecycle> {
        self.manifest.supported()
    }

    fn command(&self, step: Lifecycle, vars: &TemplateVars) -> Result<ResolvedCommand> {
        let spec = self
            .manifest
            .command(step)
            .ok_or_else(|| DeckError::LifecycleNotSupported {
                runner: self.manifest.meta.id.clone(),
                lifecycle: step,
            })?;

        let mut resolved = spec.resolve(vars)?;

        // A `script` command is relative to the manifest that declared it, not
        // to the project. Rebase it here so a plugin author can ship a helper
        // script alongside their `.toml` and reference it by bare name.
        if let deck_domain::command::CommandSpec::Script { .. } = spec {
            if let Some(script_arg) = resolved.args.iter_mut().find(|a| {
                Path::new(a.as_str())
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| {
                        ["ps1", "sh", "py", "js"]
                            .iter()
                            .any(|k| e.eq_ignore_ascii_case(k))
                    })
            }) {
                let candidate = self.base_dir.join(&*script_arg);
                if candidate.exists() {
                    *script_arg = candidate.display().to_string();
                }
            }
        }

        Ok(resolved)
    }
}

impl ManifestRunner {
    /// Detection against a caller-supplied cache.
    ///
    /// The registry consults many runners against the same directory, and
    /// sharing one cache across them is what keeps `package.json` parsed once
    /// per directory instead of once per runner.
    ///
    /// Infallible, unlike the `Runner::detect` it backs: a manifest runner has no
    /// failure mode, since a malformed project file is a non-match. The trait
    /// keeps its `Result` for implementations that could genuinely fail.
    pub(crate) fn detect_with_cache(&self, cache: &ProbeCache) -> Option<DetectionMatch> {
        if !rule::evaluate_all(&self.manifest.detect, cache) {
            return None;
        }

        let version = self
            .manifest
            .version
            .as_ref()
            .and_then(|source| rule::read_value(source, cache));

        Some(DetectionMatch {
            meta: self.manifest.meta.clone(),
            vars: self.resolve_vars(cache),
            version,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new(tag: &str) -> Self {
            use std::time::{SystemTime, UNIX_EPOCH};
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let mut path = std::env::temp_dir();
            path.push(format!("deck-mr-{tag}-{nanos:x}"));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn write(&self, name: &str, contents: &str) -> &Self {
            std::fs::write(self.0.join(name), contents).unwrap();
            self
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const NODE_MANIFEST: &str = r#"
        [meta]
        id = "node"
        name = "Node.js"
        language = "JavaScript"
        priority = 100

        [[detect]]
        file_exists = "package.json"

        [version]
        json = { file = "package.json", pointer = "version" }

        [[vars]]
        name = "pm"
        default = "npm"
        cases = [
          { value = "pnpm", when = { file_exists = "pnpm-lock.yaml" } },
          { value = "yarn", when = { file_exists = "yarn.lock" } },
          { value = "bun",  when = { file_exists = "bun.lockb" } },
        ]

        [[vars]]
        name = "script"
        default = "start"
        cases = [
          { value = "dev",   when = { json_key_exists = { file = "package.json", pointer = "scripts/dev" } } },
          { value = "serve", when = { json_key_exists = { file = "package.json", pointer = "scripts/serve" } } },
        ]

        [commands.run]
        exec = { program = "{pm}", args = ["run", "{script}"] }

        [commands.install]
        exec = { program = "{pm}", args = ["install"] }
    "#;

    fn runner() -> ManifestRunner {
        let manifest: RunnerManifest = toml::from_str(NODE_MANIFEST).unwrap();
        ManifestRunner::new(manifest, std::env::temp_dir())
    }

    #[test]
    fn detects_a_matching_directory() {
        let f = Fixture::new("match");
        f.write("package.json", r#"{"version":"1.4.2","scripts":{"dev":"vite"}}"#);
        let m = runner().detect(f.path()).unwrap().expect("should match");
        assert_eq!(m.meta.id, "node");
        assert_eq!(m.version.as_deref(), Some("1.4.2"));
    }

    #[test]
    fn declines_a_non_matching_directory() {
        let f = Fixture::new("nomatch");
        f.write("Cargo.toml", "[package]\nname=\"x\"\n");
        assert!(runner().detect(f.path()).unwrap().is_none());
    }

    #[test]
    fn resolves_package_manager_from_the_lockfile() {
        let f = Fixture::new("pnpm");
        f.write("package.json", r#"{"scripts":{"dev":"vite"}}"#);
        f.write("pnpm-lock.yaml", "lockfileVersion: '9.0'");
        let m = runner().detect(f.path()).unwrap().unwrap();
        assert_eq!(m.vars.get("pm"), Some("pnpm"));
    }

    #[test]
    fn first_matching_var_case_wins() {
        let f = Fixture::new("both-locks");
        f.write("package.json", "{}");
        // Both present: pnpm is listed first, so pnpm must win deterministically.
        f.write("pnpm-lock.yaml", "");
        f.write("yarn.lock", "");
        let m = runner().detect(f.path()).unwrap().unwrap();
        assert_eq!(m.vars.get("pm"), Some("pnpm"));
    }

    #[test]
    fn var_falls_back_to_default_when_no_case_matches() {
        let f = Fixture::new("nolock");
        f.write("package.json", "{}");
        let m = runner().detect(f.path()).unwrap().unwrap();
        assert_eq!(m.vars.get("pm"), Some("npm"));
        // No dev or serve script either.
        assert_eq!(m.vars.get("script"), Some("start"));
    }

    #[test]
    fn builds_the_run_command_from_resolved_vars() {
        let f = Fixture::new("cmd");
        f.write("package.json", r#"{"scripts":{"dev":"vite"}}"#);
        f.write("pnpm-lock.yaml", "");
        let r = runner();
        let m = r.detect(f.path()).unwrap().unwrap();
        let cmd = r.command(Lifecycle::Run, &m.vars).unwrap();
        assert_eq!(cmd.program, "pnpm");
        assert_eq!(cmd.args, vec!["run", "dev"]);
        assert_eq!(cmd.to_string(), "pnpm run dev");
    }

    #[test]
    fn unsupported_lifecycle_reports_which_step_is_missing() {
        let r = runner();
        let err = r.command(Lifecycle::Clean, &TemplateVars::new()).unwrap_err();
        assert_eq!(err.code(), "lifecycle_not_supported");
        assert!(err.to_string().contains("clean"));
    }

    #[test]
    fn supported_reports_only_declared_steps() {
        let r = runner();
        assert_eq!(r.supported(), vec![Lifecycle::Install, Lifecycle::Run]);
        assert!(r.supports(Lifecycle::Run));
        assert!(!r.supports(Lifecycle::Debug));
    }

    #[test]
    fn version_is_none_when_the_project_declares_none() {
        let f = Fixture::new("noversion");
        f.write("package.json", r#"{"scripts":{"dev":"vite"}}"#);
        let m = runner().detect(f.path()).unwrap().unwrap();
        assert!(m.version.is_none());
    }

    #[test]
    fn detection_priority_is_carried_through() {
        let f = Fixture::new("prio");
        f.write("package.json", "{}");
        let m = runner().detect(f.path()).unwrap().unwrap();
        assert_eq!(m.priority(), 100);
    }
}

#[cfg(test)]
mod built_binary_selection {
    //! The `tauri-built` runner exists because pressing Run used to trigger a
    //! full cargo debug build -- over a minute on a real project -- while a
    //! finished binary sat in `target/release`. These cover the primitive that
    //! makes pointing at a build output expressible in a manifest.

    use super::*;
    use std::fs;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("deck-glob-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[test]
    fn a_pattern_may_reach_into_a_build_directory() {
        let root = scratch("nested");
        let out = root.join("src-tauri/target/release");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("chess-scout.exe"), b"").unwrap();

        let cache = ProbeCache::new(&root);
        assert_eq!(
            first_matching_entry(&cache, "src-tauri/target/release/*.exe", &TemplateVars::new()).as_deref(),
            Some("src-tauri/target/release/chess-scout.exe"),
            "the path must come back whole, so a template can use it directly"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_bare_pattern_still_returns_a_bare_filename() {
        // The script and executable runners depend on this: they template a
        // filename relative to the working directory, not a path.
        let root = scratch("bare");
        fs::write(root.join("deploy.ps1"), b"").unwrap();

        let cache = ProbeCache::new(&root);
        assert_eq!(
            first_matching_entry(&cache, "*.ps1", &TemplateVars::new()).as_deref(),
            Some("deploy.ps1")
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_build_directory_yields_nothing_rather_than_erroring() {
        // An unbuilt project must simply not match, so the lower-priority
        // `tauri` manifest gets its turn with the dev-server command.
        let root = scratch("unbuilt");
        let cache = ProbeCache::new(&root);
        assert!(first_matching_entry(&cache, "src-tauri/target/release/*.exe", &TemplateVars::new()).is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn directories_are_never_offered_as_binaries() {
        let root = scratch("dirs");
        let out = root.join("target/release");
        fs::create_dir_all(out.join("weird.exe")).unwrap();
        fs::write(out.join("real.exe"), b"").unwrap();

        let cache = ProbeCache::new(&root);
        assert_eq!(
            first_matching_entry(&cache, "target/release/*.exe", &TemplateVars::new()).as_deref(),
            Some("target/release/real.exe"),
            "a directory named *.exe cannot be launched and must be skipped"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_choice_is_stable_rather_than_iteration_order() {
        let root = scratch("stable");
        let out = root.join("target/release");
        fs::create_dir_all(&out).unwrap();
        for n in ["zeta.exe", "alpha.exe", "mid.exe"] {
            fs::write(out.join(n), b"").unwrap();
        }
        let cache = ProbeCache::new(&root);
        assert_eq!(
            first_matching_entry(&cache, "target/release/*.exe", &TemplateVars::new()).as_deref(),
            Some("target/release/alpha.exe")
        );
        let _ = fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod glob_var_tests {
    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        use std::time::{SystemTime, UNIX_EPOCH};
        let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let d = std::env::temp_dir().join(format!("deck-globvar-{tag}-{n:x}"));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A workspace `target/release/` holds every member's binary, so the app
    /// has to be asked for by name. Without this, Pulse launched its `pulse`
    /// CLI, watched it print usage, and reported a successful launch.
    #[test]
    fn a_pattern_can_name_a_binary_through_a_variable() {
        let dir = scratch("pick");
        let rel = dir.join("target").join("release");
        std::fs::create_dir_all(&rel).unwrap();
        // Alphabetically first, so a bare `*.exe` would take it.
        std::fs::write(rel.join("aaa-cli.exe"), b"MZ").unwrap();
        std::fs::write(rel.join("pulse-app.exe"), b"MZ").unwrap();

        let mut vars = TemplateVars::new();
        vars.set("appname", "pulse-app");
        let cache = ProbeCache::new(&dir);

        let picked = first_matching_entry(&cache, "target/release/{appname}.exe", &vars);
        assert_eq!(picked.as_deref(), Some("target/release/pulse-app.exe"));

        // The unqualified pattern is what used to happen, and it picks wrong.
        let bare = first_matching_entry(&cache, "target/release/*.exe", &TemplateVars::new());
        assert_eq!(bare.as_deref(), Some("target/release/aaa-cli.exe"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An unresolvable name must match nothing rather than match everything --
    /// a pattern that quietly became `*` would reintroduce the original bug.
    #[test]
    fn an_unknown_variable_matches_nothing() {
        let dir = scratch("unknown");
        let rel = dir.join("target").join("release");
        std::fs::create_dir_all(&rel).unwrap();
        std::fs::write(rel.join("something.exe"), b"MZ").unwrap();

        let cache = ProbeCache::new(&dir);
        let picked =
            first_matching_entry(&cache, "target/release/{nosuchvar}.exe", &TemplateVars::new());
        assert_eq!(picked, None);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
