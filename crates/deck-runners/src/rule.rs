//! Evaluation of [`DetectionRule`] predicates against a project directory.
//!
//! Every rule is a read-only filesystem or parse check, evaluated through a
//! [`ProbeCache`]. Nothing here executes anything -- that property is what lets
//! detection run automatically over directories a user merely pointed at.

use deck_domain::manifest::{DetectionRule, ValueSource};

use crate::glob;
use crate::probe::ProbeCache;

/// Evaluates a rule against a directory.
///
/// Returns a plain `bool` rather than a `Result`: a rule that cannot be
/// evaluated -- unreadable file, malformed JSON -- is a non-match, not an error.
/// The alternative would abort a whole workspace scan because one project has a
/// trailing comma in its `package.json`. Where the reason matters,
/// [`ProbeCache::parse_error`] surfaces it after the fact.
#[must_use]
pub fn evaluate(rule: &DetectionRule, cache: &ProbeCache) -> bool {
    match rule {
        DetectionRule::FileExists(path) => cache.exists(path) && !cache.is_dir(path),

        DetectionRule::DirExists(path) => cache.is_dir(path),

        DetectionRule::GlobMatches(pattern) => {
            // Patterns may name a subdirectory (`target/release/*.exe`), so
            // detection and variable resolution go through the same splitter.
            // Two matchers that disagree would let a runner claim a project it
            // then cannot launch.
            let (dir, name_pattern) = glob::split_dir(pattern);
            let entries = match dir {
                None => cache.root_entries(),
                Some(dir) => cache.entries_in(dir),
            };
            entries.iter().any(|name| glob::matches(name, name_pattern))
        }

        DetectionRule::JsonKeyExists { file, pointer } => {
            cache.json_pointer(file, pointer).is_some()
        }

        DetectionRule::JsonKeyEquals {
            file,
            pointer,
            value,
        } => cache
            .json_pointer(file, pointer)
            .as_ref()
            .and_then(json_as_string)
            .is_some_and(|found| found == *value),

        DetectionRule::TomlKeyExists { file, path } => cache.toml_path(file, path).is_some(),

        DetectionRule::FileContains { file, text } => {
            cache.text(file).is_some_and(|body| body.contains(text))
        }

        DetectionRule::Not(inner) => !evaluate(inner, cache),

        // An empty `all_of` is vacuously true, matching the mathematical
        // convention. Note this differs from an empty top-level `detect` list,
        // which the registry rejects at load time -- there, an empty list is
        // overwhelmingly a mistake rather than an intentional "match anything".
        DetectionRule::AllOf(rules) => rules.iter().all(|r| evaluate(r, cache)),

        DetectionRule::AnyOf(rules) => rules.iter().any(|r| evaluate(r, cache)),
    }
}

/// Evaluates every rule in a top-level `detect` list.
///
/// An empty list never matches, so a manifest that forgot its rules -- or wrote
/// them in a form TOML folded into another table -- claims nothing rather than
/// claiming everything.
#[must_use]
pub fn evaluate_all(rules: &[DetectionRule], cache: &ProbeCache) -> bool {
    !rules.is_empty() && rules.iter().all(|r| evaluate(r, cache))
}

/// Reads a string value out of a project's metadata.
#[must_use]
pub fn read_value(source: &ValueSource, cache: &ProbeCache) -> Option<String> {
    match source {
        ValueSource::Json { file, pointer } => cache
            .json_pointer(file, pointer)
            .as_ref()
            .and_then(json_as_string),
        ValueSource::Toml { file, path } => cache
            .toml_path(file, path)
            .as_ref()
            .and_then(toml_as_string),
    }
}

/// Coerces a JSON scalar to a string.
///
/// Numbers and booleans are stringified so a manifest can compare against a
/// version written as `2` rather than `"2"`. Arrays and objects have no sensible
/// scalar form and yield `None`.
fn json_as_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        serde_json::Value::Null
        | serde_json::Value::Array(_)
        | serde_json::Value::Object(_) => None,
    }
}

/// Coerces a TOML scalar to a string.
fn toml_as_string(value: &toml::Value) -> Option<String> {
    match value {
        toml::Value::String(s) => Some(s.clone()),
        toml::Value::Integer(i) => Some(i.to_string()),
        toml::Value::Float(f) => Some(f.to_string()),
        toml::Value::Boolean(b) => Some(b.to_string()),
        toml::Value::Datetime(d) => Some(d.to_string()),
        toml::Value::Array(_) | toml::Value::Table(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    struct Fixture(PathBuf);

    impl Fixture {
        fn new(tag: &str) -> Self {
            use std::time::{SystemTime, UNIX_EPOCH};
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let mut path = std::env::temp_dir();
            path.push(format!("deck-rule-{tag}-{nanos:x}"));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn write(&self, name: &str, contents: &str) -> &Self {
            let target = self.0.join(name);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(target, contents).unwrap();
            self
        }

        fn cache(&self) -> ProbeCache {
            ProbeCache::new(&self.0)
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

    fn node_fixture() -> Fixture {
        let f = Fixture::new("node");
        f.write(
            "package.json",
            r#"{
                "name": "web",
                "version": "2.1.0",
                "private": true,
                "scripts": { "dev": "vite", "build": "vite build" },
                "dependencies": { "next": "15.1.0" }
            }"#,
        );
        f.write("pnpm-lock.yaml", "lockfileVersion: '9.0'");
        f.write("src/index.ts", "export {};");
        f
    }

    #[test]
    fn file_exists_matches_files_not_directories() {
        let f = node_fixture();
        let c = f.cache();
        assert!(evaluate(
            &DetectionRule::FileExists("package.json".into()),
            &c
        ));
        // `src` exists but is a directory; file_exists must not claim it.
        assert!(!evaluate(&DetectionRule::FileExists("src".into()), &c));
        assert!(!evaluate(&DetectionRule::FileExists("Cargo.toml".into()), &c));
    }

    #[test]
    fn dir_exists_matches_directories_only() {
        let f = node_fixture();
        let c = f.cache();
        assert!(evaluate(&DetectionRule::DirExists("src".into()), &c));
        assert!(!evaluate(
            &DetectionRule::DirExists("package.json".into()),
            &c
        ));
    }

    #[test]
    fn glob_matches_only_the_root_directory() {
        let f = Fixture::new("glob");
        f.write("App.csproj", "<Project/>");
        f.write("nested/Deep.csproj", "<Project/>");
        let c = f.cache();
        assert!(evaluate(&DetectionRule::GlobMatches("*.csproj".into()), &c));
        assert!(!evaluate(&DetectionRule::GlobMatches("*.sln".into()), &c));

        // A directory containing only a nested project must not be claimed --
        // otherwise every parent folder in a tree looks like a C# project.
        let g = Fixture::new("glob-nested");
        g.write("nested/Deep.csproj", "<Project/>");
        assert!(!evaluate(
            &DetectionRule::GlobMatches("*.csproj".into()),
            &g.cache()
        ));
    }

    #[test]
    fn json_key_exists_walks_nested_pointers() {
        let f = node_fixture();
        let c = f.cache();
        assert!(evaluate(
            &DetectionRule::JsonKeyExists {
                file: "package.json".into(),
                pointer: "scripts/dev".into(),
            },
            &c
        ));
        assert!(evaluate(
            &DetectionRule::JsonKeyExists {
                file: "package.json".into(),
                pointer: "dependencies/next".into(),
            },
            &c
        ));
        assert!(!evaluate(
            &DetectionRule::JsonKeyExists {
                file: "package.json".into(),
                pointer: "dependencies/svelte".into(),
            },
            &c
        ));
    }

    #[test]
    fn json_key_equals_compares_values() {
        let f = node_fixture();
        let c = f.cache();
        assert!(evaluate(
            &DetectionRule::JsonKeyEquals {
                file: "package.json".into(),
                pointer: "scripts/dev".into(),
                value: "vite".into(),
            },
            &c
        ));
        assert!(!evaluate(
            &DetectionRule::JsonKeyEquals {
                file: "package.json".into(),
                pointer: "scripts/dev".into(),
                value: "next dev".into(),
            },
            &c
        ));
    }

    #[test]
    fn json_key_equals_coerces_non_string_scalars() {
        let f = node_fixture();
        let c = f.cache();
        // `"private": true` compared against the string "true".
        assert!(evaluate(
            &DetectionRule::JsonKeyEquals {
                file: "package.json".into(),
                pointer: "private".into(),
                value: "true".into(),
            },
            &c
        ));
    }

    #[test]
    fn toml_key_exists_walks_dotted_paths() {
        let f = Fixture::new("poetry");
        f.write(
            "pyproject.toml",
            "[tool.poetry]\nname = \"svc\"\nversion = \"0.9.1\"\n",
        );
        let c = f.cache();
        assert!(evaluate(
            &DetectionRule::TomlKeyExists {
                file: "pyproject.toml".into(),
                path: "tool.poetry".into(),
            },
            &c
        ));
        assert!(!evaluate(
            &DetectionRule::TomlKeyExists {
                file: "pyproject.toml".into(),
                path: "tool.pdm".into(),
            },
            &c
        ));
    }

    #[test]
    fn file_contains_searches_text() {
        let f = Fixture::new("contains");
        f.write("Makefile", "all:\n\tgcc -o app main.c\n");
        let c = f.cache();
        assert!(evaluate(
            &DetectionRule::FileContains {
                file: "Makefile".into(),
                text: "gcc".into(),
            },
            &c
        ));
        assert!(!evaluate(
            &DetectionRule::FileContains {
                file: "Makefile".into(),
                text: "clang".into(),
            },
            &c
        ));
    }

    #[test]
    fn not_inverts() {
        let f = node_fixture();
        let c = f.cache();
        assert!(evaluate(
            &DetectionRule::Not(Box::new(DetectionRule::FileExists("Cargo.toml".into()))),
            &c
        ));
        assert!(!evaluate(
            &DetectionRule::Not(Box::new(DetectionRule::FileExists("package.json".into()))),
            &c
        ));
    }

    #[test]
    fn all_of_and_any_of_combine() {
        let f = node_fixture();
        let c = f.cache();
        assert!(evaluate(
            &DetectionRule::AllOf(vec![
                DetectionRule::FileExists("package.json".into()),
                DetectionRule::FileExists("pnpm-lock.yaml".into()),
            ]),
            &c
        ));
        assert!(!evaluate(
            &DetectionRule::AllOf(vec![
                DetectionRule::FileExists("package.json".into()),
                DetectionRule::FileExists("yarn.lock".into()),
            ]),
            &c
        ));
        assert!(evaluate(
            &DetectionRule::AnyOf(vec![
                DetectionRule::FileExists("yarn.lock".into()),
                DetectionRule::FileExists("pnpm-lock.yaml".into()),
            ]),
            &c
        ));
        assert!(!evaluate(&DetectionRule::AnyOf(vec![]), &c));
    }

    #[test]
    fn malformed_json_is_a_non_match_not_a_panic() {
        let f = Fixture::new("broken");
        f.write("package.json", "{ not json at all ");
        let c = f.cache();
        // The file exists, so file_exists still matches...
        assert!(evaluate(
            &DetectionRule::FileExists("package.json".into()),
            &c
        ));
        // ...but no key can be read from it, and nothing panics.
        assert!(!evaluate(
            &DetectionRule::JsonKeyExists {
                file: "package.json".into(),
                pointer: "scripts/dev".into(),
            },
            &c
        ));
        assert!(c.parse_error("package.json").is_some());
    }

    #[test]
    fn empty_top_level_detect_list_never_matches() {
        let f = node_fixture();
        assert!(!evaluate_all(&[], &f.cache()));
    }

    #[test]
    fn evaluate_all_requires_every_rule() {
        let f = node_fixture();
        let c = f.cache();
        assert!(evaluate_all(
            &[
                DetectionRule::FileExists("package.json".into()),
                DetectionRule::DirExists("src".into()),
            ],
            &c
        ));
        assert!(!evaluate_all(
            &[
                DetectionRule::FileExists("package.json".into()),
                DetectionRule::DirExists("nope".into()),
            ],
            &c
        ));
    }

    #[test]
    fn reads_versions_from_json_and_toml() {
        let f = node_fixture();
        assert_eq!(
            read_value(
                &ValueSource::Json {
                    file: "package.json".into(),
                    pointer: "version".into(),
                },
                &f.cache()
            )
            .as_deref(),
            Some("2.1.0")
        );

        let g = Fixture::new("cargo");
        g.write(
            "Cargo.toml",
            "[package]\nname = \"thing\"\nversion = \"0.3.7\"\n",
        );
        assert_eq!(
            read_value(
                &ValueSource::Toml {
                    file: "Cargo.toml".into(),
                    path: "package.version".into(),
                },
                &g.cache()
            )
            .as_deref(),
            Some("0.3.7")
        );
    }

    #[test]
    fn value_read_from_a_missing_file_is_none() {
        let f = Fixture::new("novalue");
        assert!(read_value(
            &ValueSource::Json {
                file: "package.json".into(),
                pointer: "version".into(),
            },
            &f.cache()
        )
        .is_none());
        // Directory exists; only the file is absent.
        assert!(f.path().is_dir());
    }

    #[test]
    fn rules_cannot_escape_the_project_root() {
        let f = Fixture::new("escape");
        let c = f.cache();
        assert!(!evaluate(
            &DetectionRule::FileExists(
                "..\\..\\..\\Windows\\System32\\drivers\\etc\\hosts".into()
            ),
            &c
        ));
        assert!(!evaluate(
            &DetectionRule::FileContains {
                file: "C:\\Windows\\win.ini".into(),
                text: "[fonts]".into(),
            },
            &c
        ));
    }
}
