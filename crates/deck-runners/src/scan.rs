//! Guarded directory scanning for bulk project discovery.
//!
//! A naive recursive walk of a workspace is not merely slow, it is unusable. One
//! `node_modules` holds tens of thousands of directories, a Rust `target/` holds
//! more, and a `.git` object store holds more again. Pointing an unguarded walker
//! at `D:\Workspace` means hundreds of thousands of `stat` calls to find perhaps
//! forty projects.
//!
//! Three guards make it fast and predictable:
//!
//! 1. **Depth cap.** Projects live near the top of a workspace tree, not twelve
//!    levels down.
//! 2. **Ignore list.** Dependency, build and VCS directories are never entered.
//! 3. **Stop on match.** Once a directory is identified as a project, its
//!    children are not scanned. This is the important one: without it a Cargo
//!    workspace reports every member crate, and a Tauri app reports itself plus
//!    its `src-tauri` -- technically true, useless in a list.
//!
//! The `prune_nested` option exists because "stop on match" is occasionally the
//! wrong call: someone scanning a monorepo may genuinely want each package.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use deck_domain::project::ProjectDraft;

use crate::registry::RunnerRegistry;

/// Directory names never entered during a scan.
///
/// Dependency stores, build outputs, VCS metadata, virtualenvs and editor state.
/// Every entry here is a directory that cannot itself be a project the user means
/// to register, and that is expensive to walk.
pub const IGNORED_DIRS: &[&str] = &[
    // Dependency stores
    "node_modules",
    "bower_components",
    "vendor",
    "packages",
    ".pnpm-store",
    ".yarn",
    // Build output
    "target",
    "build",
    "dist",
    "out",
    "bin",
    "obj",
    ".next",
    ".nuxt",
    ".svelte-kit",
    ".turbo",
    ".parcel-cache",
    ".gradle",
    "cmake-build-debug",
    "cmake-build-release",
    // VCS and tooling
    ".git",
    ".svn",
    ".hg",
    ".idea",
    ".vscode",
    ".vs",
    ".claude",
    // Python
    "__pycache__",
    ".venv",
    "venv",
    "env",
    ".tox",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    // Misc
    ".cache",
    ".terraform",
    "Pods",
    "DerivedData",
];

/// How to scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanOptions {
    /// How many levels below the root to descend. 0 checks only the root itself.
    pub max_depth: usize,

    /// Stop descending into a directory once it is identified as a project.
    ///
    /// On by default. Turning it off surfaces nested projects, which is what a
    /// monorepo scan wants and what a workspace scan does not.
    pub prune_nested: bool,

    /// Upper bound on results, so a scan of an unexpectedly huge tree returns
    /// rather than running until the user gives up.
    pub max_results: usize,

    /// Extra directory names to skip, on top of [`IGNORED_DIRS`].
    pub extra_ignores: Vec<String>,

    /// Include directories no runner claimed.
    ///
    /// Off by default: a scan that reports every unidentified folder buries the
    /// real findings. The single-folder add flow does surface them, because
    /// there the user has pointed at one specific directory.
    pub include_unknown: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            max_depth: 4,
            prune_nested: true,
            max_results: 500,
            extra_ignores: Vec::new(),
            include_unknown: false,
        }
    }
}

impl ScanOptions {
    /// Whether a directory name should be skipped.
    fn is_ignored(&self, name: &str) -> bool {
        // Dotted directories are skipped wholesale beyond the explicit list:
        // there are hundreds of them and none hold a registerable project.
        if name.starts_with('.') && name.len() > 1 {
            return true;
        }
        IGNORED_DIRS.iter().any(|d| d.eq_ignore_ascii_case(name))
            || self
                .extra_ignores
                .iter()
                .any(|d| d.eq_ignore_ascii_case(name))
    }
}

/// What a scan found.
#[derive(Debug, Clone, Default)]
pub struct ScanResult {
    /// Candidate projects, in discovery order (shallowest first).
    pub drafts: Vec<ProjectDraft>,

    /// Directories entered.
    pub directories_visited: usize,

    /// Directories skipped by the ignore list.
    pub directories_skipped: usize,

    /// Whether [`ScanOptions::max_results`] was reached and the scan stopped
    /// early. Surfaced in the UI so a truncated result never looks complete.
    pub truncated: bool,
}

/// Scans `root` for projects.
///
/// Never returns an error. A directory that cannot be read is counted as skipped
/// and the scan continues -- one permissions problem partway through a workspace
/// must not discard everything already found.
#[must_use]
pub fn scan(root: &Path, registry: &RunnerRegistry, options: &ScanOptions) -> ScanResult {
    let mut result = ScanResult::default();
    // Breadth-first so results arrive shallowest first, which matches how a user
    // thinks about a workspace, and so `max_results` truncates the deep tail
    // rather than an arbitrary branch.
    let mut queue = std::collections::VecDeque::from([(root.to_path_buf(), 0usize)]);
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();

    while let Some((dir, depth)) = queue.pop_front() {
        if result.drafts.len() >= options.max_results {
            result.truncated = true;
            break;
        }

        // Guard against a symlink or junction loop revisiting a directory.
        let key = std::fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
        if !seen.insert(key) {
            continue;
        }

        result.directories_visited += 1;

        let detected = registry.detect(&dir);
        let is_project = detected.is_some();

        if is_project || options.include_unknown {
            result.drafts.push(draft_for(&dir, detected.as_ref(), registry));
        }

        // Stop here if this is a project and nesting is pruned.
        if is_project && options.prune_nested {
            continue;
        }

        if depth >= options.max_depth {
            continue;
        }

        let Ok(entries) = std::fs::read_dir(&dir) else {
            result.directories_skipped += 1;
            continue;
        };

        // Sort children so scan output is stable across runs.
        let mut children: Vec<PathBuf> = Vec::new();
        for entry in entries.filter_map(std::result::Result::ok) {
            let path = entry.path();
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if options.is_ignored(name) {
                result.directories_skipped += 1;
                continue;
            }
            children.push(path);
        }
        children.sort();
        for child in children {
            queue.push_back((child, depth + 1));
        }
    }

    tracing::info!(
        root = %root.display(),
        found = result.drafts.len(),
        visited = result.directories_visited,
        skipped = result.directories_skipped,
        truncated = result.truncated,
        "scan complete"
    );

    result
}

/// Builds a draft for one directory, resolving the run command it would use.
///
/// The resolved command is carried on the draft so the add flow can show exactly
/// what will execute before anything is registered. Detection proposes; it never
/// silently commits.
#[must_use]
pub fn draft_for(
    dir: &Path,
    detected: Option<&deck_domain::runner::DetectionMatch>,
    registry: &RunnerRegistry,
) -> ProjectDraft {
    use deck_domain::command::{Lifecycle, TemplateVars};
    use deck_domain::project::DetectedFacts;
    use deck_domain::runner::Runner;

    let name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Untitled")
        .to_owned();

    let Some(m) = detected else {
        return ProjectDraft {
            name,
            root: dir.to_path_buf(),
            detected: None,
            proposed_run: None,
        };
    };

    // Runner variables plus the project-level ones the templates may reference.
    let mut vars = m.vars.clone();
    let project_vars = TemplateVars::with_project(dir, &name);
    for key in ["projectRoot", "projectName", "dirName"] {
        if let Some(value) = project_vars.get(key) {
            vars.set(key, value);
        }
    }

    let proposed_run = registry
        .get(&m.meta.id)
        .and_then(|runner| runner.command(Lifecycle::Run, &vars).ok())
        .map(|cmd| cmd.to_string());

    ProjectDraft {
        name,
        root: dir.to_path_buf(),
        detected: Some(DetectedFacts {
            runner_id: m.meta.id.clone(),
            language: m.meta.language.clone(),
            framework: m.meta.framework.clone(),
            package_manager: m.vars.get("pm").map(str::to_owned),
            version: m.version.clone(),
            detected_at: chrono::Utc::now(),
        }),
        proposed_run,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tree(PathBuf);

    impl Tree {
        fn new(tag: &str) -> Self {
            use std::time::{SystemTime, UNIX_EPOCH};
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let mut path = std::env::temp_dir();
            path.push(format!("deck-scan-{tag}-{nanos:x}"));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn file(&self, rel: &str, contents: &str) -> &Self {
            let target = self.0.join(rel);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(target, contents).unwrap();
            self
        }

        fn dir(&self, rel: &str) -> &Self {
            std::fs::create_dir_all(self.0.join(rel)).unwrap();
            self
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn registry() -> RunnerRegistry {
        RunnerRegistry::bundled().unwrap()
    }

    fn names(result: &ScanResult) -> Vec<&str> {
        result.drafts.iter().map(|d| d.name.as_str()).collect()
    }

    #[test]
    fn finds_projects_one_level_down() {
        let t = Tree::new("basic");
        t.file("web/package.json", r#"{"scripts":{"dev":"vite"}}"#);
        t.file("cli/Cargo.toml", "[package]\nname=\"cli\"\nversion=\"0.1.0\"\n");
        t.file("svc/go.mod", "module svc\n");

        let r = scan(t.path(), &registry(), &ScanOptions::default());
        let mut found = names(&r);
        found.sort_unstable();
        assert_eq!(found, vec!["cli", "svc", "web"]);
    }

    #[test]
    fn never_enters_ignored_directories() {
        let t = Tree::new("ignore");
        t.file("app/package.json", "{}");
        // A package.json inside node_modules must not be reported.
        t.file("app/node_modules/left-pad/package.json", r#"{"name":"left-pad"}"#);
        t.file("app/node_modules/.bin/placeholder", "");

        let opts = ScanOptions {
            prune_nested: false,
            ..Default::default()
        };
        let r = scan(t.path(), &registry(), &opts);
        assert_eq!(names(&r), vec!["app"]);
        assert!(
            !r.drafts.iter().any(|d| d.root.to_string_lossy().contains("node_modules")),
            "scan descended into node_modules"
        );
    }

    #[test]
    fn skips_build_output_and_vcs_directories() {
        let t = Tree::new("build");
        t.file("app/Cargo.toml", "[package]\nname=\"a\"\nversion=\"0.1.0\"\n");
        t.file("app/target/debug/build/x/Cargo.toml", "[package]\nname=\"gen\"\nversion=\"0\"\n");
        t.dir("app/.git/objects");

        let opts = ScanOptions {
            prune_nested: false,
            ..Default::default()
        };
        let r = scan(t.path(), &registry(), &opts);
        assert_eq!(names(&r), vec!["app"]);
        assert!(r.directories_skipped > 0);
    }

    #[test]
    fn dotted_directories_are_skipped_wholesale() {
        let t = Tree::new("dotted");
        t.file("real/package.json", "{}");
        t.file(".hidden-tooling/package.json", "{}");
        let r = scan(t.path(), &registry(), &ScanOptions::default());
        assert_eq!(names(&r), vec!["real"]);
    }

    #[test]
    fn pruning_stops_at_the_outermost_project() {
        let t = Tree::new("prune");
        // A Tauri app: the root is the project, src-tauri is an implementation
        // detail that must not appear as a second entry.
        t.file(
            "app/package.json",
            r#"{"devDependencies":{"@tauri-apps/cli":"^2"}}"#,
        );
        t.file("app/src-tauri/Cargo.toml", "[package]\nname=\"app\"\nversion=\"0.1.0\"\n");

        let r = scan(t.path(), &registry(), &ScanOptions::default());
        assert_eq!(names(&r), vec!["app"]);
        assert_eq!(r.drafts[0].detected.as_ref().unwrap().runner_id, "tauri");
    }

    #[test]
    fn disabling_pruning_surfaces_nested_projects() {
        let t = Tree::new("nested");
        t.file("mono/package.json", r#"{"name":"mono"}"#);
        t.file("mono/packages-a/package.json", r#"{"name":"a"}"#);
        t.file("mono/apps/web/package.json", r#"{"name":"web"}"#);

        let pruned = scan(t.path(), &registry(), &ScanOptions::default());
        assert_eq!(names(&pruned), vec!["mono"]);

        let opts = ScanOptions {
            prune_nested: false,
            ..Default::default()
        };
        let all = scan(t.path(), &registry(), &opts);
        let mut found = names(&all);
        found.sort_unstable();
        assert_eq!(found, vec!["mono", "packages-a", "web"]);
    }

    #[test]
    fn depth_cap_is_respected() {
        let t = Tree::new("depth");
        t.file("a/b/c/d/e/package.json", "{}");

        let shallow = ScanOptions {
            max_depth: 2,
            ..Default::default()
        };
        assert!(scan(t.path(), &registry(), &shallow).drafts.is_empty());

        let deep = ScanOptions {
            max_depth: 6,
            ..Default::default()
        };
        assert_eq!(names(&scan(t.path(), &registry(), &deep)), vec!["e"]);
    }

    #[test]
    fn unknown_directories_are_excluded_by_default_and_optional() {
        let t = Tree::new("unknown");
        t.file("real/go.mod", "module real\n");
        t.file("notes/todo.txt", "buy milk");

        let default_scan = scan(t.path(), &registry(), &ScanOptions::default());
        assert_eq!(names(&default_scan), vec!["real"]);

        let opts = ScanOptions {
            include_unknown: true,
            ..Default::default()
        };
        let with_unknown = scan(t.path(), &registry(), &opts);
        let mut found = names(&with_unknown);
        found.sort_unstable();
        // The scan root itself is unidentified too, hence three entries.
        assert!(found.contains(&"notes"));
        assert!(found.contains(&"real"));
    }

    #[test]
    fn max_results_truncates_and_says_so() {
        let t = Tree::new("cap");
        for i in 0..12 {
            t.file(&format!("p{i:02}/go.mod"), &format!("module p{i}\n"));
        }
        let opts = ScanOptions {
            max_results: 5,
            ..Default::default()
        };
        let r = scan(t.path(), &registry(), &opts);
        assert_eq!(r.drafts.len(), 5);
        assert!(r.truncated);
    }

    #[test]
    fn complete_scan_is_not_marked_truncated() {
        let t = Tree::new("complete");
        t.file("one/go.mod", "module one\n");
        let r = scan(t.path(), &registry(), &ScanOptions::default());
        assert!(!r.truncated);
    }

    #[test]
    fn extra_ignores_are_honoured() {
        let t = Tree::new("extra");
        t.file("keep/go.mod", "module keep\n");
        t.file("scratch/go.mod", "module scratch\n");
        let opts = ScanOptions {
            extra_ignores: vec!["scratch".to_owned()],
            ..Default::default()
        };
        assert_eq!(names(&scan(t.path(), &registry(), &opts)), vec!["keep"]);
    }

    #[test]
    fn drafts_carry_the_command_that_would_run() {
        let t = Tree::new("draft");
        t.file("web/package.json", r#"{"version":"3.0.0","scripts":{"dev":"vite"}}"#);
        t.file("web/pnpm-lock.yaml", "");

        let r = scan(t.path(), &registry(), &ScanOptions::default());
        let draft = &r.drafts[0];
        let facts = draft.detected.as_ref().unwrap();
        assert_eq!(facts.package_manager.as_deref(), Some("pnpm"));
        assert_eq!(facts.version.as_deref(), Some("3.0.0"));
        // The exact string the user is shown before registering anything.
        assert_eq!(draft.proposed_run.as_deref(), Some("pnpm run dev"));
    }

    #[test]
    fn scanning_a_missing_root_yields_nothing_and_does_not_panic() {
        let r = scan(
            Path::new(r"D:\no-such-scan-root-xyz"),
            &registry(),
            &ScanOptions::default(),
        );
        assert!(r.drafts.is_empty());
    }

    #[test]
    fn a_project_at_the_scan_root_itself_is_found() {
        let t = Tree::new("selfroot");
        t.file("Cargo.toml", "[package]\nname=\"here\"\nversion=\"1.0.0\"\n");
        let r = scan(t.path(), &registry(), &ScanOptions::default());
        assert_eq!(r.drafts.len(), 1);
        assert_eq!(r.drafts[0].detected.as_ref().unwrap().runner_id, "rust");
    }

    #[test]
    fn results_are_stable_across_repeated_scans() {
        let t = Tree::new("stable");
        t.file("a/go.mod", "module a\n");
        t.file("b/Cargo.toml", "[package]\nname=\"b\"\nversion=\"0.1.0\"\n");
        t.file("c/package.json", "{}");

        let first = names(&scan(t.path(), &registry(), &ScanOptions::default()))
            .iter()
            .map(|s| (*s).to_owned())
            .collect::<Vec<_>>();
        for _ in 0..3 {
            let again = names(&scan(t.path(), &registry(), &ScanOptions::default()))
                .iter()
                .map(|s| (*s).to_owned())
                .collect::<Vec<_>>();
            assert_eq!(again, first);
        }
    }
}
