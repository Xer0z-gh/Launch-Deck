//! Filename wildcard matching for `glob_matches` detection rules.
//!
//! Deliberately not a full glob implementation and deliberately not a
//! dependency. Detection patterns are filenames within a single directory --
//! `*.csproj`, `*.sln`, `CMakeLists.txt` -- so `*` and `?` over one path segment
//! is the entire requirement. Pulling in a glob crate would add a regex engine
//! to support path traversal and character classes that no manifest needs.
//!
//! Matching is case-insensitive. That is correct on Windows, and on a
//! case-sensitive filesystem the only effect is that detection is slightly more
//! willing to match, which is the safe direction for a heuristic.

/// Matches `name` against a wildcard `pattern`.
///
/// `*` matches any run of characters including none; `?` matches exactly one.
/// No other character is special -- a `.` is a literal dot, not "any character".
///
/// Uses the standard two-pointer algorithm with a single backtrack point, which
/// is linear in practice and cannot blow up on patterns like `*a*a*a*` the way a
/// naive recursive matcher does.
#[must_use]
pub fn matches(name: &str, pattern: &str) -> bool {
    // Compare over lowercased char vectors. Filenames here are short, so the
    // allocation is cheaper than repeatedly re-casing during comparison.
    let name: Vec<char> = name.chars().flat_map(char::to_lowercase).collect();
    let pat: Vec<char> = pattern.chars().flat_map(char::to_lowercase).collect();

    let mut n = 0usize;
    let mut p = 0usize;
    // How much of `name` the most recent `*` currently absorbs. `None` until we
    // have seen a star, which is what makes an unmatchable literal fail fast
    // instead of backtracking.
    let mut star_absorbed: Option<usize> = None;
    // Position in `pat` immediately after the most recent `*`.
    let mut after_star = 0usize;

    while n < name.len() {
        match pat.get(p) {
            Some('*') => {
                after_star = p + 1;
                star_absorbed = Some(n);
                p += 1;
            }
            Some('?') => {
                n += 1;
                p += 1;
            }
            Some(&c) if c == name[n] => {
                n += 1;
                p += 1;
            }
            // Mismatch: let the most recent `*` swallow one more character and
            // retry the pattern from just after it.
            _ => match star_absorbed {
                Some(absorbed) => {
                    let widened = absorbed + 1;
                    star_absorbed = Some(widened);
                    n = widened;
                    p = after_star;
                }
                None => return false,
            },
        }
    }

    // `name` is exhausted; whatever pattern remains must be stars only.
    // `p <= pat.len()` holds by construction: it only advances past indices that
    // `pat.get` returned `Some` for.
    pat[p..].iter().all(|&c| c == '*')
}

/// Splits a pattern into an optional literal directory and a filename pattern.
///
/// `src-tauri/target/release/*.exe` -> `(Some("src-tauri/target/release"), "*.exe")`
/// `*.csproj`                       -> `(None, "*.csproj")`
///
/// Only the final segment may contain wildcards. That keeps this the small
/// single-segment matcher this module documents, while letting a manifest point
/// at a build output rather than only the project root.
#[must_use]
pub fn split_dir(pattern: &str) -> (Option<&str>, &str) {
    match pattern.rsplit_once(['/', '\\']) {
        Some((dir, name)) => (Some(dir), name),
        None => (None, pattern),
    }
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn literal_names_match_exactly() {
        assert!(matches("CMakeLists.txt", "CMakeLists.txt"));
        assert!(!matches("CMakeLists.txt", "Makefile"));
    }

    #[test]
    fn matching_is_case_insensitive() {
        assert!(matches("Cargo.toml", "cargo.toml"));
        assert!(matches("MAKEFILE", "Makefile"));
    }

    #[test]
    fn trailing_extension_wildcards() {
        assert!(matches("Server.csproj", "*.csproj"));
        assert!(matches("App.sln", "*.sln"));
        assert!(!matches("Server.csproj.user", "*.csproj"));
    }

    #[test]
    fn star_matches_empty_run() {
        assert!(matches(".csproj", "*.csproj"));
        assert!(matches("abc", "abc*"));
        assert!(matches("abc", "*abc"));
    }

    #[test]
    fn question_mark_matches_exactly_one() {
        assert!(matches("a.c", "?.c"));
        assert!(!matches("ab.c", "?.c"));
        assert!(!matches(".c", "?.c"));
    }

    #[test]
    fn interior_wildcards() {
        assert!(matches("my-app.esproj", "*app*"));
        assert!(matches("build.gradle.kts", "build.gradle*"));
        assert!(!matches("gradle.build", "build.gradle*"));
    }

    #[test]
    fn multiple_stars_backtrack_correctly() {
        assert!(matches("aXbXc", "a*b*c"));
        assert!(matches("abc", "a*b*c"));
        assert!(!matches("acb", "a*b*c"));
    }

    #[test]
    fn pathological_pattern_terminates() {
        // A naive recursive matcher goes exponential here.
        let name = "a".repeat(40);
        assert!(!matches(&name, "*a*a*a*a*a*a*b"));
    }

    #[test]
    fn dot_is_literal_not_any_char() {
        assert!(!matches("axcsproj", "*.csproj"));
    }

    #[test]
    fn bare_star_matches_anything_including_empty() {
        assert!(matches("", "*"));
        assert!(matches("whatever.ext", "*"));
    }

    #[test]
    fn empty_pattern_matches_only_empty_name() {
        assert!(matches("", ""));
        assert!(!matches("x", ""));
    }

    #[test]
    fn handles_non_ascii_names() {
        assert!(matches("café.csproj", "*.csproj"));
        assert!(matches("Ünïcode.sln", "*.SLN"));
    }
}
