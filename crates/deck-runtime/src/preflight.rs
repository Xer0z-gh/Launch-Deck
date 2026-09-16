//! Can this project actually start, without starting it?
//!
//! # Why this exists
//!
//! Launch Deck already knew two ways a launch could fail before it was tried:
//! the project folder is gone, and the runner's declared setup marker is
//! missing (no `node_modules`, no venv). Real run history showed three more,
//! and all three were only discovered by pressing Run and reading a log:
//!
//! - **The program is not there.** Lyzee ran `npm run dev`, which ran `vite`,
//!   which is not on PATH because `node_modules` was never installed. Eleven
//!   characters of stderr, after a launch, for a fact knowable beforehand.
//! - **The entry file does not exist.** Video-Forge is a packaged project with
//!   no `main.py`, and the Python runner's `entry` variable falls back to
//!   `main.py` when nothing matches. Launch Deck offered a Run button for
//!   `python main.py` against a file it could have seen was absent.
//! - **The binary is ambiguous.** Pulse has two bin targets and no
//!   `default-run`, so `cargo run` exits 101 with "could not determine which
//!   binary to run". A correct command for a project that needs a more
//!   specific one.
//!
//! Every check here is a **filesystem read**, never an execution. The same
//! discipline detection follows, and for the same reason: this runs over the
//! whole library automatically, so a probe that could execute would turn
//! "open the app" into "run whatever fifty folders say".

use std::path::Path;

/// File extensions that make an argument a script rather than a subcommand.
///
/// `npm run dev` and `cargo run` take words; `python main.py` takes a file.
/// Matching on a known extension is what separates them, and keeps this from
/// flagging `pytest` or `--bin` as a missing file.
const SCRIPT_EXTENSIONS: &[&str] = &[
    "py", "js", "mjs", "cjs", "ts", "tsx", "ps1", "bat", "cmd", "exe", "jar", "rb", "php",
    "lua", "sh", "pl", "R",
];

/// An argument that names a file the project does not have.
///
/// Returns the offending argument, so the caller can say which one. Arguments
/// that are flags, or that carry no recognised script extension, are left
/// alone -- a false "missing file" on a working project is worse than the
/// silence this replaces, because it would block a Run that works.
#[must_use]
pub fn missing_entry(root: &Path, args: &[String]) -> Option<String> {
    args.iter()
        .filter(|a| !a.starts_with('-'))
        .find(|a| {
            let Some(ext) = Path::new(a.as_str()).extension().and_then(|e| e.to_str()) else {
                return false;
            };
            if !SCRIPT_EXTENSIONS.iter().any(|k| k.eq_ignore_ascii_case(ext)) {
                return false;
            }
            // Absolute paths are the runner's own resolution (`{projectRoot}\x.exe`)
            // and are checked as given; relative ones hang off the project.
            let candidate = Path::new(a.as_str());
            let full = if candidate.is_absolute() {
                candidate.to_path_buf()
            } else {
                root.join(candidate)
            };
            !full.exists()
        })
        .cloned()
}

/// The bin targets a Cargo project offers, when `cargo run` would be ambiguous.
///
/// `None` means the command is fine: not a bare `cargo run`, or the project
/// has a `default-run`, or there is at most one target. `Some(names)` means
/// cargo will refuse and these are the choices -- which is exactly what the
/// repair dialog needs to offer.
///
/// Targets are counted the way cargo does: `src/main.rs` is one, and every
/// `src/bin/*.rs` is another. Explicit `[[bin]]` names win when present.
#[must_use]
pub fn cargo_ambiguity(root: &Path, program: &str, args: &[String]) -> Option<Vec<String>> {
    if !program.eq_ignore_ascii_case("cargo") {
        return None;
    }
    if !args.iter().any(|a| a == "run") {
        return None;
    }
    // Already disambiguated by the command itself.
    if args.iter().any(|a| a == "--bin" || a == "--example") {
        return None;
    }

    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).ok()?;
    let table: toml::Table = manifest.parse().ok()?;

    let mut names = if let Some(package) = table.get("package") {
        // A single package: a declared default settles it, otherwise count
        // this package's own targets.
        if package.get("default-run").is_some() {
            return None;
        }
        bin_targets(root, &table, package.get("name").and_then(|n| n.as_str()))
    } else {
        // A VIRTUAL WORKSPACE -- no package of its own, only members. This is
        // the shape that actually bit: Pulse is `[workspace] members = [...]`
        // with no `[package]`, and `cargo run` there exits 101 with "could not
        // determine which binary to run". An earlier version of this function
        // returned None for exactly this case, so the one project that
        // demonstrated the bug was the one it could not see.
        let members = table.get("workspace")?.get("members")?.as_array()?;
        let mut all = Vec::new();
        for member in members.iter().filter_map(|m| m.as_str()) {
            for dir in expand_member(root, member) {
                let Ok(text) = std::fs::read_to_string(dir.join("Cargo.toml")) else {
                    continue;
                };
                let Ok(member_table) = text.parse::<toml::Table>() else {
                    continue;
                };
                let name = member_table
                    .get("package")
                    .and_then(|p| p.get("name"))
                    .and_then(|n| n.as_str());
                all.extend(bin_targets(&dir, &member_table, name));
            }
        }
        all
    };

    if names.len() > 1 {
        names.sort();
        names.dedup();
    }
    if names.len() > 1 {
        Some(names)
    } else {
        None
    }
}

/// Expands one `members` entry, which may end in a `*` glob.
///
/// Only a trailing `*` is handled, because that is the only form cargo
/// actually sees in practice (`crates/*`) and a full glob engine here would be
/// more machinery than the question deserves.
fn expand_member(root: &Path, member: &str) -> Vec<std::path::PathBuf> {
    if let Some(prefix) = member.strip_suffix("/*").or_else(|| member.strip_suffix("\\*")) {
        let base = root.join(prefix);
        return std::fs::read_dir(base)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
    }
    vec![root.join(member)]
}

/// Binary target names a single package directory produces.
///
/// Counted the way cargo does: explicit `[[bin]]` names win; otherwise
/// `src/main.rs` contributes the package name and every `src/bin/*.rs`
/// contributes its file stem.
fn bin_targets(dir: &Path, table: &toml::Table, package_name: Option<&str>) -> Vec<String> {
    if let Some(bins) = table.get("bin").and_then(|b| b.as_array()) {
        let declared: Vec<String> = bins
            .iter()
            .filter_map(|b| b.get("name").and_then(|n| n.as_str()))
            .map(str::to_owned)
            .collect();
        if !declared.is_empty() {
            return declared;
        }
    }

    let mut names = Vec::new();
    if dir.join("src/main.rs").is_file() {
        names.push(package_name.unwrap_or("main").to_owned());
    }
    if let Ok(entries) = std::fs::read_dir(dir.join("src/bin")) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    names.push(stem.to_owned());
                }
            }
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "deck-preflight-{tag}-{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    /// The Video-Forge case: a packaged project with no `main.py`, for which
    /// the runner's `entry` variable fell back to `main.py` anyway.
    #[test]
    fn a_missing_script_argument_is_named() {
        let dir = scratch("entry");
        assert_eq!(
            missing_entry(&dir, &args(&["main.py"])).as_deref(),
            Some("main.py")
        );

        std::fs::write(dir.join("main.py"), "print(1)").unwrap();
        assert_eq!(missing_entry(&dir, &args(&["main.py"])), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The check must not fire on subcommands, flags or module names, or it
    /// would block projects that launch perfectly.
    #[test]
    fn subcommands_and_flags_are_not_files() {
        let dir = scratch("words");
        assert_eq!(missing_entry(&dir, &args(&["run", "dev"])), None);
        assert_eq!(missing_entry(&dir, &args(&["-m", "pytest"])), None);
        assert_eq!(missing_entry(&dir, &args(&["run", "--bin", "pulse"])), None);
        // A dotted module path is not a file, and has no script extension.
        assert_eq!(missing_entry(&dir, &args(&["-m", "videoforge.cli"])), None);
        // A flag that happens to end in a script extension is still a flag.
        assert_eq!(missing_entry(&dir, &args(&["--config=x.py"])), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The Pulse case: two auto-discovered targets and no `default-run`.
    #[test]
    fn two_cargo_targets_without_a_default_are_ambiguous() {
        let dir = scratch("cargo");
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"pulse\"\n").unwrap();
        std::fs::create_dir_all(dir.join("src/bin")).unwrap();
        std::fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.join("src/bin/pulse-app.rs"), "fn main() {}").unwrap();

        let found = cargo_ambiguity(&dir, "cargo", &args(&["run"])).expect("ambiguous");
        assert_eq!(found, vec!["pulse".to_owned(), "pulse-app".to_owned()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_default_run_or_an_explicit_bin_settles_it() {
        let dir = scratch("cargo-ok");
        std::fs::create_dir_all(dir.join("src/bin")).unwrap();
        std::fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.join("src/bin/other.rs"), "fn main() {}").unwrap();

        // An explicit --bin is never ambiguous, whatever the manifest says.
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        assert_eq!(cargo_ambiguity(&dir, "cargo", &args(&["run", "--bin", "x"])), None);

        // Nor is a declared default.
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"x\"\ndefault-run = \"x\"\n",
        )
        .unwrap();
        assert_eq!(cargo_ambiguity(&dir, "cargo", &args(&["run"])), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn one_target_is_not_ambiguous_and_other_programs_are_ignored() {
        let dir = scratch("cargo-one");
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"solo\"\n").unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();
        assert_eq!(cargo_ambiguity(&dir, "cargo", &args(&["run"])), None);
        // Not cargo at all.
        assert_eq!(cargo_ambiguity(&dir, "npm", &args(&["run"])), None);
        // Cargo, but not running.
        assert_eq!(cargo_ambiguity(&dir, "cargo", &args(&["build"])), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Pulse's real shape: a virtual workspace whose members each build a
    /// binary. `cargo run` exits 101 here, and the first version of this
    /// function could not see it because it bailed on the missing `[package]`.
    #[test]
    fn a_virtual_workspace_with_several_binary_members_is_ambiguous() {
        let dir = scratch("workspace");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nresolver = \"2\"\nmembers = [\"crates/*\", \"src-tauri\"]\n",
        )
        .unwrap();

        for (rel, name) in [
            ("crates/pulse-core", "pulse-core"),
            ("crates/pulse-cli", "pulse-cli"),
            ("src-tauri", "pulse-app"),
        ] {
            let member = dir.join(rel);
            std::fs::create_dir_all(member.join("src")).unwrap();
            std::fs::write(
                member.join("Cargo.toml"),
                format!("[package]\nname = \"{name}\"\n"),
            )
            .unwrap();
            // Only two of the three produce a binary; a lib-only member must
            // not be counted as a choice.
            if name != "pulse-core" {
                std::fs::write(member.join("src/main.rs"), "fn main() {}").unwrap();
            }
        }

        let found = cargo_ambiguity(&dir, "cargo", &args(&["run"])).expect("ambiguous");
        assert_eq!(found, vec!["pulse-app".to_owned(), "pulse-cli".to_owned()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_project_with_no_cargo_manifest_reports_nothing() {
        let dir = scratch("no-manifest");
        assert_eq!(cargo_ambiguity(&dir, "cargo", &args(&["run"])), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
