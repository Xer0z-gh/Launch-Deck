//! Lifecycle commands and the template language that produces them.
//!
//! A runner manifest cannot contain arbitrary code, so command strings are
//! templated instead. The language is deliberately tiny -- `{var}` substitution
//! and nothing else. No conditionals, no loops, no shelling out to evaluate an
//! expression. Anything a manifest genuinely cannot express delegates to a
//! script via [`CommandSpec::script`], which keeps the escape hatch explicit
//! and visible rather than smuggled into a clever template syntax.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{DeckError, Result};

/// The lifecycle steps a runner may implement.
///
/// A runner declares only the steps that make sense for it: a shell script has
/// nothing to install and nothing to build, and that is expressed by absence
/// rather than by a stub that does nothing.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    /// Fetch dependencies.
    Install,
    /// Produce build artefacts.
    Build,
    /// Start the project. The only step every runner must define.
    Run,
    /// Start the project under a debugger or with debug flags.
    Debug,
    /// Remove build artefacts and caches.
    Clean,
    /// Run the project's tests.
    Test,
}

impl Lifecycle {
    /// Every lifecycle step, in the order a UI should present them.
    pub const ALL: [Self; 6] = [
        Self::Install,
        Self::Build,
        Self::Run,
        Self::Debug,
        Self::Clean,
        Self::Test,
    ];

    /// Whether this step is expected to be long-lived rather than to complete.
    ///
    /// Run and Debug hold a process open; the rest are tasks that finish. The
    /// supervisor uses this to decide whether an exit code of 0 means "done"
    /// or "it stopped unexpectedly".
    #[must_use]
    pub const fn is_long_running(self) -> bool {
        matches!(self, Self::Run | Self::Debug)
    }
}

impl fmt::Display for Lifecycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Install => "install",
            Self::Build => "build",
            Self::Run => "run",
            Self::Debug => "debug",
            Self::Clean => "clean",
            Self::Test => "test",
        };
        f.write_str(s)
    }
}

/// How a lifecycle step is invoked.
///
/// Note there is no shell-string variant. Commands are always an explicit
/// program plus an argument vector, so nothing is ever handed to `cmd /c` for
/// re-parsing. A project directory named `My Project & Co` cannot become two
/// commands, and a manifest cannot smuggle in a shell operator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandSpec {
    /// Run a program directly with the given arguments.
    Exec {
        /// Program name or absolute path. Resolved against PATH if bare.
        program: String,
        /// Arguments, each templated independently.
        #[serde(default)]
        args: Vec<String>,
    },
    /// Delegate to a user script. The escape hatch for anything templates
    /// cannot express.
    Script {
        /// Path to the script, relative to the runner manifest's directory.
        path: String,
        /// Arguments passed after the script path.
        #[serde(default)]
        args: Vec<String>,
    },
}

impl CommandSpec {
    /// Builds an `Exec` spec from a program and borrowed argument list.
    pub fn exec<I, S>(program: impl Into<String>, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::Exec {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }

    /// Resolves every template in the spec against `vars`.
    ///
    /// # Errors
    ///
    /// Returns [`DeckError::UnknownTemplateVar`] if any `{name}` in the program
    /// or arguments has no corresponding entry in `vars`. Failing loudly here
    /// is deliberate: a silently-empty argument turns `npm run {script}` into
    /// `npm run`, which would start the wrong thing.
    pub fn resolve(&self, vars: &TemplateVars) -> Result<ResolvedCommand> {
        match self {
            Self::Exec { program, args } => Ok(ResolvedCommand {
                program: vars.expand(program)?,
                args: args
                    .iter()
                    .map(|a| vars.expand(a))
                    .collect::<Result<Vec<_>>>()?,
            }),
            Self::Script { path, args } => {
                let script = vars.expand(path)?;
                // A script is launched by the platform's script host rather
                // than executed directly, so a .ps1 does not depend on the
                // user's file associations.
                let (program, mut prefix) = script_host(&script);
                prefix.push(script);
                let mut resolved_args = prefix;
                for a in args {
                    resolved_args.push(vars.expand(a)?);
                }
                Ok(ResolvedCommand {
                    program,
                    args: resolved_args,
                })
            }
        }
    }
}

/// Chooses an interpreter for a script based on its extension.
///
/// Returns the program to launch and any arguments that must precede the script
/// path itself.
fn script_host(script: &str) -> (String, Vec<String>) {
    let ext = std::path::Path::new(script)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match ext.as_str() {
        "ps1" => (
            "powershell.exe".to_owned(),
            vec![
                "-NoProfile".to_owned(),
                "-NonInteractive".to_owned(),
                "-ExecutionPolicy".to_owned(),
                "Bypass".to_owned(),
                "-File".to_owned(),
            ],
        ),
        "cmd" | "bat" => ("cmd.exe".to_owned(), vec!["/c".to_owned()]),
        "sh" | "bash" => ("bash".to_owned(), Vec::new()),
        "py" => ("python".to_owned(), Vec::new()),
        "js" | "mjs" | "cjs" => ("node".to_owned(), Vec::new()),
        // No extension, or an unknown one: run it directly and let the OS
        // decide. Covers bare executables named in a manifest.
        _ => (script.to_owned(), Vec::new()),
    }
}

/// A command with every template resolved, ready to hand to the supervisor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedCommand {
    /// Program to launch.
    pub program: String,
    /// Fully-resolved arguments.
    pub args: Vec<String>,
}

impl fmt::Display for ResolvedCommand {
    /// Renders the command the way a user would type it, quoting only what
    /// needs quoting. Display only -- never parsed back.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", quote_if_needed(&self.program))?;
        for arg in &self.args {
            write!(f, " {}", quote_if_needed(arg))?;
        }
        Ok(())
    }
}

fn quote_if_needed(s: &str) -> String {
    if s.is_empty() {
        return "\"\"".to_owned();
    }
    if s.chars().any(char::is_whitespace) {
        format!("\"{s}\"")
    } else {
        s.to_owned()
    }
}

/// Parses a user-typed command line into a program and argument vector.
///
/// The inverse of [`ResolvedCommand`]'s `Display`, for the add-project flow
/// where detection failed and the user types the run command by hand. Double
/// quotes group words; there is deliberately no operator support -- `&&`, `|`
/// and friends are ordinary characters, because commands here are never handed
/// to a shell for re-parsing.
///
/// Returns `None` for an empty or whitespace-only line.
#[must_use]
pub fn parse_command_line(input: &str) -> Option<ResolvedCommand> {
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut saw_token = false;

    for ch in input.trim().chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                saw_token = true;
            }
            c if c.is_whitespace() && !in_quotes => {
                if saw_token {
                    parts.push(std::mem::take(&mut current));
                    saw_token = false;
                }
            }
            c => {
                current.push(c);
                saw_token = true;
            }
        }
    }
    if saw_token {
        parts.push(current);
    }

    let mut iter = parts.into_iter();
    let program = iter.next()?;
    if program.is_empty() {
        return None;
    }
    Some(ResolvedCommand {
        program,
        args: iter.collect(),
    })
}

/// Variables available to command templates.
///
/// Kept as an explicit map rather than a struct with fixed fields so a runner
/// manifest can introduce its own variables (a detection rule that reads a
/// version out of a config file, for instance) without changing this crate.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateVars(BTreeMap<String, String>);

impl TemplateVars {
    /// An empty variable set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Seeds the standard variables every runner can rely on.
    #[must_use]
    pub fn with_project(root: &Path, name: &str) -> Self {
        let mut vars = Self::new();
        vars.set("projectRoot", root.display().to_string());
        vars.set("projectName", name.to_owned());
        if let Some(dir) = root.file_name().and_then(|s| s.to_str()) {
            vars.set("dirName", dir.to_owned());
        }
        vars
    }

    /// Inserts or replaces a variable.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.0.insert(key.into(), value.into());
    }

    /// Looks up a variable.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }

    /// Expands every `{name}` occurrence in `template`.
    ///
    /// `{{` and `}}` are literal braces. An unmatched `{` is treated as a
    /// literal too rather than an error, since a stray brace in an argument is
    /// far more likely to be intentional than a typo'd variable.
    ///
    /// # Errors
    ///
    /// Returns [`DeckError::UnknownTemplateVar`] for a well-formed `{name}`
    /// with no matching variable.
    pub fn expand(&self, template: &str) -> Result<String> {
        let mut out = String::with_capacity(template.len());
        let mut chars = template.chars().peekable();

        while let Some(ch) = chars.next() {
            match ch {
                '{' if chars.peek() == Some(&'{') => {
                    chars.next();
                    out.push('{');
                }
                '}' if chars.peek() == Some(&'}') => {
                    chars.next();
                    out.push('}');
                }
                '{' => {
                    let mut name = String::new();
                    let mut closed = false;
                    for c in chars.by_ref() {
                        if c == '}' {
                            closed = true;
                            break;
                        }
                        name.push(c);
                    }
                    if !closed {
                        // Unterminated: emit verbatim.
                        out.push('{');
                        out.push_str(&name);
                        continue;
                    }
                    let key = name.trim();
                    let value = self.get(key).ok_or_else(|| DeckError::UnknownTemplateVar {
                        variable: key.to_owned(),
                    })?;
                    out.push_str(value);
                }
                other => out.push(other),
            }
        }

        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> TemplateVars {
        let mut v = TemplateVars::new();
        v.set("projectRoot", r"D:\Workspace\My App");
        v.set("script", "dev");
        v
    }

    #[test]
    fn expands_known_variables() {
        assert_eq!(vars().expand("run {script}").unwrap(), "run dev");
    }

    #[test]
    fn expands_multiple_occurrences() {
        assert_eq!(vars().expand("{script}-{script}").unwrap(), "dev-dev");
    }

    #[test]
    fn preserves_values_containing_spaces() {
        assert_eq!(
            vars().expand("{projectRoot}").unwrap(),
            r"D:\Workspace\My App"
        );
    }

    #[test]
    fn unknown_variable_is_an_error_not_an_empty_string() {
        let err = vars().expand("npm run {missing}").unwrap_err();
        assert_eq!(err.code(), "unknown_template_var");
    }

    #[test]
    fn doubled_braces_are_literal() {
        assert_eq!(vars().expand("{{script}}").unwrap(), "{script}");
    }

    #[test]
    fn unterminated_brace_is_literal() {
        assert_eq!(vars().expand("a {b c").unwrap(), "a {b c");
    }

    #[test]
    fn whitespace_inside_braces_is_tolerated() {
        assert_eq!(vars().expand("{ script }").unwrap(), "dev");
    }

    #[test]
    fn resolves_exec_spec() {
        let spec = CommandSpec::exec("npm", ["run", "{script}"]);
        let resolved = spec.resolve(&vars()).unwrap();
        assert_eq!(resolved.program, "npm");
        assert_eq!(resolved.args, vec!["run", "dev"]);
    }

    #[test]
    fn powershell_scripts_go_through_the_script_host() {
        let spec = CommandSpec::Script {
            path: "start.ps1".to_owned(),
            args: vec!["{script}".to_owned()],
        };
        let resolved = spec.resolve(&vars()).unwrap();
        assert_eq!(resolved.program, "powershell.exe");
        assert!(resolved.args.contains(&"-File".to_owned()));
        assert_eq!(resolved.args.last().unwrap(), "dev");
    }

    #[test]
    fn display_quotes_only_arguments_with_spaces() {
        let cmd = ResolvedCommand {
            program: "npm".to_owned(),
            args: vec!["run".to_owned(), "my task".to_owned()],
        };
        assert_eq!(cmd.to_string(), "npm run \"my task\"");
    }

    #[test]
    fn parses_a_plain_command_line() {
        let cmd = parse_command_line("npm run dev").unwrap();
        assert_eq!(cmd.program, "npm");
        assert_eq!(cmd.args, vec!["run", "dev"]);
    }

    #[test]
    fn parses_quoted_arguments_and_round_trips_display() {
        let cmd = parse_command_line(r#""C:\My Tools\serve.exe" --root "D:\a b" -v"#).unwrap();
        assert_eq!(cmd.program, r"C:\My Tools\serve.exe");
        assert_eq!(cmd.args, vec!["--root", r"D:\a b", "-v"]);
        // Display quotes what needs quoting; parsing that again is identity.
        assert_eq!(parse_command_line(&cmd.to_string()).unwrap(), cmd);
    }

    #[test]
    fn empty_and_whitespace_lines_parse_to_none() {
        assert!(parse_command_line("").is_none());
        assert!(parse_command_line("   ").is_none());
    }

    #[test]
    fn shell_operators_are_ordinary_characters() {
        // No shell ever re-parses these, so `&&` is just an argument.
        let cmd = parse_command_line("echo a && del /f").unwrap();
        assert_eq!(cmd.args, vec!["a", "&&", "del", "/f"]);
    }

    #[test]
    fn adjacent_quotes_keep_one_token() {
        let cmd = parse_command_line(r#"py"thon" -V"#).unwrap();
        assert_eq!(cmd.program, "python");
        assert_eq!(cmd.args, vec!["-V"]);
    }

    #[test]
    fn long_running_steps_are_exactly_run_and_debug() {
        let long: Vec<_> = Lifecycle::ALL
            .into_iter()
            .filter(|s| s.is_long_running())
            .collect();
        assert_eq!(long, vec![Lifecycle::Run, Lifecycle::Debug]);
    }
}
