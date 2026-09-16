//! Resolving a program name to something Windows can actually execute.
//!
//! # The bug this exists to fix
//!
//! `npm`, `pnpm`, `yarn`, `tsc`, `gradlew`, `mvnw` and most of the Node and JVM
//! tool surface on Windows are **not** executables. They are `.cmd` or `.bat`
//! shims. `CreateProcess` -- and therefore `std::process::Command` -- resolves
//! only directly-executable images from `PATH`, so `Command::new("npm")` fails
//! with "program not found" even on a machine where `npm` works perfectly in
//! every shell. A batch file cannot be executed directly either, even given its
//! full path: it has no PE header, so it must be handed to `cmd.exe /c`.
//!
//! The practical effect was that every Node-based project failed to start with a
//! misleading "`npm` was not found on PATH", which is most of a typical project
//! library. Earlier tests missed it because they used `cmd.exe` and `python`,
//! both of which are real `.exe` files.
//!
//! # What this module does
//!
//! Resolve a program the way a shell would: search `PATH` using `PATHEXT`
//! ordering, then report whether the result can be executed directly or has to
//! go through the command interpreter.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use parking_lot::Mutex;

/// How a resolved program must be invoked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invocation {
    /// A real executable image: run it directly.
    Direct(PathBuf),
    /// A batch script: run it via `cmd.exe /c`, which is the only way Windows
    /// will execute one.
    ViaShell(PathBuf),
}

impl Invocation {
    /// The resolved path, however it is invoked.
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::Direct(p) | Self::ViaShell(p) => p,
        }
    }
}

/// Extensions that must go through `cmd.exe`.
const SHELL_EXTS: &[&str] = &["cmd", "bat"];

/// Fallback search order when `PATHEXT` is unset or unreadable.
const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

/// Bare program names already found on `PATH`, for the life of the process.
///
/// See [`resolve`] for why this exists and why misses are never cached.
static RESOLVED: OnceLock<Mutex<HashMap<String, Invocation>>> = OnceLock::new();

/// Resolves a program name or path into an [`Invocation`].
///
/// Returns `None` when nothing matching can be found, which the caller reports
/// as [`deck_domain::DeckError::ProgramNotFound`].
///
/// A value containing a path separator is treated as a path and probed directly
/// (with `PATHEXT` completion if it has no extension); anything else is searched
/// along `PATH`, exactly as a shell would.
#[must_use]
pub fn resolve(program: &str) -> Option<Invocation> {
    if program.is_empty() {
        return None;
    }

    let looks_like_path = program.contains('\\') || program.contains('/') || program.contains(':');

    if looks_like_path {
        // A path needs no search: one or two `is_file` probes, measured at
        // 0.05 ms. Caching that would add a lock for no gain.
        let candidate = Path::new(program);
        return probe(candidate).or_else(|| complete_with_pathext(candidate));
    }

    // A bare name is the expensive case, and the one that repeats.
    //
    // Searching `npm` walks every PATH entry -- 43 of them on this machine --
    // trying each `PATHEXT` suffix in turn, and measured **2.2-4.2 ms per
    // launch** against a whole backend launch cost of ~11 ms. It also gives the
    // same answer every time: `PATH` does not change inside a process, and
    // neither does where npm is installed.
    //
    // Cached per process, revalidated on every hit with a single `is_file` --
    // so uninstalling a tool while Launch Deck is open falls back to a fresh
    // search instead of launching something that is no longer there. That probe
    // is ~0.05 ms against the 3 ms walk it replaces.
    let cache = RESOLVED.get_or_init(|| Mutex::new(HashMap::new()));

    let cached = cache.lock().get(program).cloned();
    if let Some(hit) = cached {
        if hit.path().is_file() {
            return Some(hit);
        }
    }

    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(program);
        if let Some(found) = probe(&candidate).or_else(|| complete_with_pathext(&candidate)) {
            cache.lock().insert(program.to_owned(), found.clone());
            return Some(found);
        }
    }
    // Deliberately NOT caching the miss. A program that is not installed yet is
    // the one case where the answer genuinely changes underfoot: install Node,
    // press Run again, and it has to work without restarting the app.
    None
}

/// Classifies an exact path, if it exists and is a file.
fn probe(candidate: &Path) -> Option<Invocation> {
    if !candidate.is_file() {
        return None;
    }
    let ext = candidate
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    // A file with no extension is not executable on Windows; leave it to the
    // PATHEXT pass rather than claiming it can be run.
    if ext.is_empty() {
        return None;
    }

    if SHELL_EXTS.contains(&ext.as_str()) {
        Some(Invocation::ViaShell(candidate.to_path_buf()))
    } else {
        Some(Invocation::Direct(candidate.to_path_buf()))
    }
}

/// Tries each `PATHEXT` suffix against a base path, in the OS's own order.
///
/// The order matters: `PATHEXT` puts `.EXE` before `.CMD`, so a directory
/// holding both `foo.exe` and `foo.cmd` resolves the same way a shell would.
fn complete_with_pathext(base: &Path) -> Option<Invocation> {
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| DEFAULT_PATHEXT.to_owned());
    for ext in pathext.split(';').filter(|e| !e.is_empty()) {
        let ext = ext.trim_start_matches('.');
        let mut candidate = base.as_os_str().to_owned();
        candidate.push(".");
        candidate.push(ext);
        if let Some(found) = probe(Path::new(&candidate)) {
            return Some(found);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_a_bare_exe_on_path() {
        let found = resolve("cmd").expect("cmd should resolve");
        assert!(matches!(found, Invocation::Direct(_)), "got {found:?}");
        assert!(found
            .path()
            .to_string_lossy()
            .to_ascii_lowercase()
            .ends_with("cmd.exe"));
    }

    #[test]
    fn resolves_an_exe_given_with_its_extension() {
        let found = resolve("cmd.exe").expect("cmd.exe should resolve");
        assert!(matches!(found, Invocation::Direct(_)));
    }

    #[test]
    fn resolves_npm_and_marks_it_as_needing_the_shell() {
        // The whole reason this module exists. npm is a .cmd shim on Windows, so
        // it must resolve AND be flagged as shell-invoked.
        let Some(found) = resolve("npm") else {
            // Node may genuinely be absent; skip rather than fail spuriously.
            eprintln!("npm not installed; skipping");
            return;
        };
        assert!(
            matches!(found, Invocation::ViaShell(_)),
            "npm must be invoked via cmd.exe, got {found:?}"
        );
        let ext = found
            .path()
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        assert!(
            ext == "cmd" || ext == "bat",
            "resolved to {} (extension {ext})",
            found.path().display()
        );
    }

    #[test]
    fn resolves_an_absolute_path_to_a_batch_file() {
        let dir = std::env::temp_dir().join(format!("deck-prog-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("thing.cmd");
        std::fs::write(&script, "@echo off\r\n").unwrap();

        let found = resolve(&script.display().to_string()).expect("should resolve");
        assert_eq!(found, Invocation::ViaShell(script.clone()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn completes_an_extensionless_path_using_pathext() {
        let dir = std::env::temp_dir().join(format!("deck-prog-ext-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("tool.cmd"), "@echo off\r\n").unwrap();

        // Ask for "tool" with no extension, as a manifest would.
        let base = dir.join("tool");
        let found = resolve(&base.display().to_string()).expect("should complete to .cmd");
        assert!(matches!(found, Invocation::ViaShell(_)), "got {found:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prefers_exe_over_cmd_when_both_exist() {
        // PATHEXT lists .EXE before .CMD, and resolution must honour that so
        // behaviour matches what the user sees in a shell.
        let dir = std::env::temp_dir().join(format!("deck-prog-both-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // A real exe to copy, so `is_file` and classification are honest.
        let system_cmd = PathBuf::from(std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into()))
            .join("System32")
            .join("cmd.exe");
        std::fs::copy(&system_cmd, dir.join("dual.exe")).unwrap();
        std::fs::write(dir.join("dual.cmd"), "@echo off\r\n").unwrap();

        let found = resolve(&dir.join("dual").display().to_string()).unwrap();
        assert!(
            matches!(found, Invocation::Direct(_)),
            "should prefer the .exe, got {found:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unknown_program_does_not_resolve() {
        assert!(resolve("definitely-not-a-real-program-xyz").is_none());
    }

    #[test]
    fn an_empty_name_does_not_resolve() {
        assert!(resolve("").is_none());
    }

    #[test]
    fn a_directory_is_not_a_program() {
        let dir = std::env::temp_dir().display().to_string();
        assert!(resolve(&dir).is_none());
    }

    /// The cache has to actually avoid the walk, not merely exist.
    ///
    /// A cache that stores the answer and searches anyway passes every
    /// correctness test while buying nothing, and the only way to tell is to
    /// time it. The PATH walk measured 2.2-4.2 ms per launch; a cache hit is a
    /// map lookup and one `is_file`.
    #[test]
    fn the_second_resolution_of_a_bare_name_is_much_faster() {
        if resolve("cmd").is_none() {
            return; // no cmd.exe means no Windows, and nothing to assert
        }

        // The first call may already be cached by another test in this binary,
        // so a fresh name is used for the cold side. `where` ships with Windows.
        let cold = std::time::Instant::now();
        let found = resolve("where");
        let cold = cold.elapsed();
        if found.is_none() {
            return;
        }

        let warm = std::time::Instant::now();
        assert!(resolve("where").is_some());
        let warm = warm.elapsed();

        assert!(
            warm < cold || warm.as_micros() < 200,
            "cache did not help: cold {cold:?}, warm {warm:?}"
        );
    }

    /// A cached program that has since been deleted must not be returned.
    ///
    /// The cache is what makes this possible to get wrong: without it, every
    /// launch re-searched and a missing program simply stopped resolving. With
    /// it, a stale entry would hand the supervisor a path to nothing and turn a
    /// clear "not found" into a spawn failure.
    #[test]
    fn a_cached_program_that_disappears_is_not_returned() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        let dir = std::env::temp_dir().join(format!("deck-prog-{nanos:x}"));
        std::fs::create_dir_all(&dir).unwrap();
        let name = format!("deck-vanish-{nanos:x}");
        let exe = dir.join(format!("{name}.exe"));
        std::fs::write(&exe, b"MZ").unwrap();

        // Put the directory on PATH for this process so the bare name resolves.
        let old_path = std::env::var_os("PATH").unwrap_or_default();
        let mut entries = std::env::split_paths(&old_path).collect::<Vec<_>>();
        entries.insert(0, dir.clone());
        let joined = std::env::join_paths(entries).unwrap();
        // SAFETY: single-threaded test process mutating its own environment.
        unsafe { std::env::set_var("PATH", &joined) };

        assert!(resolve(&name).is_some(), "should resolve while present");

        std::fs::remove_file(&exe).unwrap();
        assert!(
            resolve(&name).is_none(),
            "a deleted program was served from the cache"
        );

        // SAFETY: as above.
        unsafe { std::env::set_var("PATH", &old_path) };
        let _ = std::fs::remove_dir_all(&dir);
    }
}
