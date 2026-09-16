//! Turning a registered project into something the supervisor can spawn.
//!
//! This is the composition point for the precedence rules scattered across the
//! system, stated once:
//!
//! * **Command**: the user's override for the step, else the runner's template.
//! * **Variables**: freshly resolved from the directory (lockfiles may have
//!   changed since registration), overlaid with the project-level variables
//!   (`{projectRoot}`, `{projectName}`, `{dirName}`).
//! * **Arguments**: `overrides.args` are appended to Run and Debug commands --
//!   they configure how the project *runs*, not how it builds.
//! * **Environment**: the `.env` file first, explicit overrides second, so a
//!   value typed in settings always beats the file.
//! * **Working directory**: the override, else the project root.
//!
//! Living here rather than in the Tauri layer keeps `src-tauri` logic-free and
//! makes every rule testable without a GUI.

use std::path::Path;

use deck_domain::command::{Lifecycle, ResolvedCommand, TemplateVars};
use deck_domain::error::{DeckError, Result};
use deck_domain::project::Project;
use deck_domain::runner::Runner;

use crate::registry::RunnerRegistry;

/// Everything the supervisor needs to spawn one lifecycle step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    /// The fully-resolved command.
    pub command: ResolvedCommand,
    /// Environment additions, in application order.
    pub env: Vec<(String, String)>,
    /// Directory to run in.
    pub working_dir: std::path::PathBuf,
    /// Ports the user explicitly configured for this project.
    ///
    /// A conflict here is a hard error: the user told us the port, so we know
    /// the launch cannot work and can refuse before spawning anything.
    pub required_ports: Vec<u16>,

    /// Ports the runner merely considers conventional for this project type.
    ///
    /// A conflict here is only a warning. `node.toml` lists 3000, 5173 and 8080
    /// because Node projects commonly use one of them -- not because any given
    /// project uses all three. Refusing to launch because an unrelated app holds
    /// 8080 would block a project that never touches it, so this is surfaced in
    /// the log and the launch proceeds. If the port really was needed, the child
    /// fails and the exit-reason diagnosis names it.
    pub likely_ports: Vec<u16>,
}

/// Builds the launch plan for one lifecycle step of a project.
///
/// # Errors
///
/// - [`DeckError::LifecycleNotSupported`] when neither the user nor the runner
///   defines a command for `step`.
/// - [`DeckError::UnknownTemplateVar`] when a template references a variable
///   that cannot be resolved.
/// - [`DeckError::Io`] when a configured `.env` file exists but cannot be read.
pub fn plan(project: &Project, registry: &RunnerRegistry, step: Lifecycle) -> Result<LaunchPlan> {
    let vars = resolve_vars(project, registry);

    let mut command = if let Some(spec) = project.overrides.command(step) {
        spec.resolve(&vars)?
    } else {
        let runner = registry.get(&project.detected.runner_id).ok_or_else(|| {
            DeckError::LifecycleNotSupported {
                runner: project.detected.runner_id.clone(),
                lifecycle: step,
            }
        })?;
        runner.command(step, &vars)?
    };

    // Extra arguments configure how the project runs; appending them to a
    // build or install would hand `--port 4000` to a compiler.
    if step.is_long_running() {
        command.args.extend(project.overrides.args.iter().cloned());
    }

    // Only long-running steps bind ports; a build does not.
    let (required_ports, likely_ports) = if step.is_long_running() {
        let configured = project.effective_ports().to_vec();
        let conventional = if configured.is_empty() {
            registry
                .get(&project.detected.runner_id)
                .map(|r| r.manifest().meta.default_ports.clone())
                .unwrap_or_default()
        } else {
            // An explicit list supersedes the guess entirely; warning about
            // ports the user did not ask for would be noise.
            Vec::new()
        };
        (configured, conventional)
    } else {
        (Vec::new(), Vec::new())
    };

    unwrap_package_script(&mut command, project.effective_working_dir());

    Ok(LaunchPlan {
        command,
        env: resolve_env(project)?,
        working_dir: project.effective_working_dir().clone(),
        required_ports,
        likely_ports,
    })
}

/// Replaces `npm run dev` with the script itself when it is a plain `node` call.
///
/// Measured on a script reading `node --watch server.js`: **342 ms to first
/// output through npm, 254 ms direct.** The 88 ms is `cmd.exe` starting,
/// `npm.cmd` starting, and a whole Node process booting the npm CLI purely to
/// spawn a second Node process.
///
/// `node` only, deliberately. A script calling `vite` or `tsx` by bare name
/// works *because* the package manager puts `node_modules/.bin` on PATH, so
/// unwrapping those would break the project to save 88 ms. `node` is on PATH
/// by definition anywhere npm runs.
///
/// ponytail: covers `node ...` scripts; widen to local `.bin` binaries by
/// resolving against `node_modules/.bin` if those turn out to matter.
fn unwrap_package_script(command: &mut ResolvedCommand, root: &Path) {
    if !["npm", "pnpm", "yarn", "bun"].contains(&command.program.as_str())
        || command.args.first().map(String::as_str) != Some("run")
    {
        return;
    }
    let [_, script, rest @ ..] = command.args.as_slice() else {
        return;
    };
    let Ok(raw) = std::fs::read_to_string(root.join("package.json")) else {
        return;
    };
    let Ok(doc) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return;
    };
    let Some(line) = doc["scripts"][script].as_str() else {
        return;
    };
    // Shell syntax needs a shell. Anything else is a guess that breaks projects.
    if line.contains(|c| "&|><;$%\"'".contains(c)) {
        return;
    }
    let mut parts = line.split_whitespace();
    if parts.next() != Some("node") {
        return;
    }
    "node".clone_into(&mut command.program);
    command.args = parts.map(str::to_owned).chain(rest.iter().cloned()).collect();
}

/// Freshly resolves template variables for a project.
///
/// Detection is re-run at launch time on purpose: it is one directory probe,
/// and it means a lockfile swap (npm -> pnpm) takes effect on the next Run
/// without a manual re-detect. If the stored runner no longer matches the
/// directory, its variables still resolve from defaults and `first_match`
/// rules, so the launch degrades gracefully instead of failing on `{pm}`.
fn resolve_vars(project: &Project, registry: &RunnerRegistry) -> TemplateVars {
    let mut vars = registry
        .get(&project.detected.runner_id)
        .map(|runner| runner.vars_for(&project.root))
        .unwrap_or_default();

    // Project-level variables always win over runner-derived ones.
    let base = TemplateVars::with_project(&project.root, &project.name);
    for key in ["projectRoot", "projectName", "dirName"] {
        if let Some(value) = base.get(key) {
            vars.set(key, value);
        }
    }
    vars
}

/// Assembles the child environment: `.env` file first, explicit vars second.
fn resolve_env(project: &Project) -> Result<Vec<(String, String)>> {
    let mut env = Vec::new();

    if let Some(rel) = &project.overrides.env_file {
        let path = if rel.is_absolute() {
            rel.clone()
        } else {
            project.root.join(rel)
        };
        env.extend(parse_env_file(&path)?);
    }

    for var in &project.overrides.env {
        env.push((var.key.clone(), var.value.clone()));
    }

    Ok(env)
}

/// Parses a `.env` file: `KEY=VALUE` lines, `#` comments, optional quotes.
///
/// Deliberately no variable expansion and no multi-line values -- the simple
/// dialect every tool agrees on. The file is read, never written.
///
/// # Errors
///
/// Returns [`DeckError::Io`] if the file cannot be read. A missing file is an
/// error too: the user pointed at it explicitly, and silently launching
/// without those variables would produce a confusing half-configured run.
pub fn parse_env_file(path: &Path) -> Result<Vec<(String, String)>> {
    let body = std::fs::read_to_string(path)
        .map_err(|e| DeckError::io(format!("reading env file {}", path.display()), e))?;

    let mut vars = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Tolerate the common `export KEY=...` prefix.
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value);
        vars.push((key.to_owned(), value.to_owned()));
    }
    Ok(vars)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chrono::Utc;
    use deck_domain::command::CommandSpec;
    use deck_domain::project::{DetectedFacts, EnvVar, ProjectId, ProjectOverrides, RestartPolicy};

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
            path.push(format!("deck-launch-{tag}-{nanos:x}"));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn file(&self, name: &str, contents: &str) -> &Self {
            std::fs::write(self.0.join(name), contents).unwrap();
            self
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

    fn node_project(root: &Path) -> Project {
        let now = Utc::now();
        Project {
            id: ProjectId::new(),
            name: "Web App".into(),
            description: None,
            root: root.to_path_buf(),
            detected: DetectedFacts {
                runner_id: "node".into(),
                language: "JavaScript".into(),
                framework: None,
                package_manager: Some("npm".into()),
                version: None,
                detected_at: now,
            },
            overrides: ProjectOverrides::default(),
            tags: vec![],
            category: None,
            favorite: false,
            pinned: false,
            archived: false,
            restart_policy: RestartPolicy::Never,
            notes: None,
            created_at: now,
            updated_at: now,
            last_launched_at: None,
        }
    }

    #[test]
    fn runner_default_ports_are_likely_never_required() {
        // The distinction that stops an unrelated app on 8080 blocking a launch:
        // a runner's conventional ports are a guess, so they may only warn.
        let t = Tree::new("ports");
        t.file("package.json", r#"{"scripts":{"dev":"vite"}}"#);
        let p = node_project(&t.0);

        let run = plan(&p, &registry(), Lifecycle::Run).unwrap();
        assert!(
            run.required_ports.is_empty(),
            "a guessed port must never gate a launch: {:?}",
            run.required_ports
        );
        assert!(!run.likely_ports.is_empty(), "runner defaults should warn");

        // A build binds nothing, so neither list applies.
        let build = plan(&p, &registry(), Lifecycle::Build).unwrap();
        assert!(build.required_ports.is_empty());
        assert!(build.likely_ports.is_empty());
    }

    #[test]
    fn a_user_configured_port_is_required_and_supersedes_the_guess() {
        let t = Tree::new("ports-override");
        t.file("package.json", r#"{"scripts":{"dev":"vite"}}"#);
        let mut p = node_project(&t.0);
        p.overrides.ports = vec![4321];

        let run = plan(&p, &registry(), Lifecycle::Run).unwrap();
        assert_eq!(run.required_ports, vec![4321]);
        assert!(
            run.likely_ports.is_empty(),
            "an explicit list should not also warn about guessed ports"
        );
    }

    #[test]
    fn plans_the_runner_command_with_fresh_vars() {
        let t = Tree::new("fresh");
        t.file("package.json", r#"{"scripts":{"dev":"vite"}}"#);
        t.file("pnpm-lock.yaml", "");
        let p = node_project(&t.0);

        let plan = plan(&p, &registry(), Lifecycle::Run).unwrap();
        // Detection re-ran: pnpm from the lockfile, even though the stored
        // facts said npm at registration time.
        assert_eq!(plan.command.to_string(), "pnpm run dev");
        assert_eq!(plan.working_dir, t.0);
    }

    #[test]
    fn a_user_override_beats_the_runner() {
        let t = Tree::new("override");
        t.file("package.json", r#"{"scripts":{"dev":"vite"}}"#);
        let mut p = node_project(&t.0);
        p.overrides.commands.insert(
            Lifecycle::Run,
            CommandSpec::exec("node", ["server.js"]),
        );

        let plan = plan(&p, &registry(), Lifecycle::Run).unwrap();
        assert_eq!(plan.command.to_string(), "node server.js");
    }

    #[test]
    fn extra_args_reach_run_but_not_build() {
        let t = Tree::new("args");
        t.file("package.json", r#"{"scripts":{"dev":"vite"}}"#);
        let mut p = node_project(&t.0);
        p.overrides.args = vec!["--port".into(), "4100".into()];

        let run = plan(&p, &registry(), Lifecycle::Run).unwrap();
        assert!(run.command.args.ends_with(&["--port".into(), "4100".into()]));

        let build = plan(&p, &registry(), Lifecycle::Build).unwrap();
        assert!(!build.command.args.contains(&"--port".to_owned()));
    }

    #[test]
    fn unsupported_step_is_reported_not_guessed() {
        let t = Tree::new("nostep");
        t.file("main.lua", "print('hi')");
        let mut p = node_project(&t.0);
        p.detected.runner_id = "lua".into();

        let err = plan(&p, &registry(), Lifecycle::Build).unwrap_err();
        assert_eq!(err.code(), "lifecycle_not_supported");
    }

    #[test]
    fn a_vanished_runner_id_is_reported() {
        let t = Tree::new("gone");
        let mut p = node_project(&t.0);
        p.detected.runner_id = "runner-that-was-uninstalled".into();
        let err = plan(&p, &registry(), Lifecycle::Run).unwrap_err();
        assert_eq!(err.code(), "lifecycle_not_supported");
    }

    #[test]
    fn working_dir_override_is_honoured() {
        let t = Tree::new("wd");
        t.file("package.json", "{}");
        let sub = t.0.join("packages").join("web");
        std::fs::create_dir_all(&sub).unwrap();
        let mut p = node_project(&t.0);
        p.overrides.working_dir = Some(sub.clone());

        let plan = plan(&p, &registry(), Lifecycle::Run).unwrap();
        assert_eq!(plan.working_dir, sub);
    }

    #[test]
    fn env_file_loads_and_explicit_vars_win() {
        let t = Tree::new("env");
        t.file("package.json", "{}");
        t.file(
            ".env",
            "# comment\nPORT=3000\nexport API_URL=\"http://localhost:9000\"\nEMPTY=\n",
        );
        let mut p = node_project(&t.0);
        p.overrides.env_file = Some(PathBuf::from(".env"));
        p.overrides.env.push(EnvVar::new("PORT", "4000"));

        let plan = plan(&p, &registry(), Lifecycle::Run).unwrap();
        // File first, explicit second: the supervisor applies them in order,
        // so the later PORT=4000 wins in the child environment.
        let ports: Vec<&str> = plan
            .env
            .iter()
            .filter(|(k, _)| k == "PORT")
            .map(|(_, v)| v.as_str())
            .collect();
        assert_eq!(ports, vec!["3000", "4000"]);
        assert!(plan
            .env
            .contains(&("API_URL".into(), "http://localhost:9000".into())));
        assert!(plan.env.contains(&("EMPTY".into(), String::new())));
    }

    #[test]
    fn a_missing_env_file_is_an_error_not_a_silent_skip() {
        let t = Tree::new("noenv");
        t.file("package.json", "{}");
        let mut p = node_project(&t.0);
        p.overrides.env_file = Some(PathBuf::from(".env.production"));
        let err = plan(&p, &registry(), Lifecycle::Run).unwrap_err();
        assert_eq!(err.code(), "io");
    }

    #[test]
    fn script_runners_resolve_entries_to_absolute_paths() {
        // Regression: `cmd /c run.bat` depends on cwd search, which the
        // NoDefaultCurrentDirectoryInExePath env var (exported by Git Bash and
        // inherited by children) silently disables. The command must therefore
        // carry the absolute path and never rely on resolution-by-cwd.
        let t = Tree::new("absbat");
        t.file("run.bat", "@echo off
echo hi
");
        let mut p = node_project(&t.0);
        p.detected.runner_id = "batch".into();

        let plan = plan(&p, &registry(), Lifecycle::Run).unwrap();
        assert_eq!(plan.command.program, "cmd.exe");
        let script = &plan.command.args[1];
        assert!(
            std::path::Path::new(script).is_absolute(),
            "batch entry must be absolute, got `{script}`"
        );
        assert!(script.ends_with("run.bat"));
    }

    #[test]
    fn env_file_dialect_details() {
        let t = Tree::new("dialect");
        t.file(
            "vars.env",
            "A='single quoted'\nB=\"double quoted\"\nC=un quoted with spaces\nnot a var line\n",
        );
        let vars = parse_env_file(&t.0.join("vars.env")).unwrap();
        assert_eq!(
            vars,
            vec![
                ("A".into(), "single quoted".into()),
                ("B".into(), "double quoted".into()),
                ("C".into(), "un quoted with spaces".into()),
            ]
        );
    }
}

#[cfg(test)]
mod unwrap_tests {
    use super::*;

    fn plan_for(script: &str) -> ResolvedCommand {
        use std::time::{SystemTime, UNIX_EPOCH};
        let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("deck-unwrap-{n:x}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("package.json"),
            format!(r#"{{"scripts":{{"dev":{}}}}}"#, serde_json::to_string(script).unwrap()),
        )
        .unwrap();

        let mut cmd = ResolvedCommand {
            program: "npm".to_owned(),
            args: vec!["run".to_owned(), "dev".to_owned()],
        };
        unwrap_package_script(&mut cmd, &dir);
        let _ = std::fs::remove_dir_all(&dir);
        cmd
    }

    /// The whole point: skip cmd.exe -> npm.cmd -> node just to reach node.
    #[test]
    fn a_plain_node_script_is_run_directly() {
        let cmd = plan_for("node --watch server.js");
        assert_eq!(cmd.program, "node");
        assert_eq!(cmd.args, ["--watch", "server.js"]);
    }

    /// `vite` is only on PATH because the package manager put `node_modules/.bin`
    /// there. Unwrapping it would trade 88 ms for a project that cannot start.
    #[test]
    fn a_local_binary_keeps_the_package_manager() {
        let cmd = plan_for("vite --host");
        assert_eq!(cmd.program, "npm");
        assert_eq!(cmd.args, ["run", "dev"]);
    }

    /// Shell syntax needs a shell.
    #[test]
    fn shell_syntax_keeps_the_package_manager() {
        for script in ["node a.js && node b.js", "node a.js > out.log", "node $FOO.js"] {
            assert_eq!(plan_for(script).program, "npm", "rewrote {script}");
        }
    }
}
