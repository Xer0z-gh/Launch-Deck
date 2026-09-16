//! The project manual: what a repo says about itself, read off disk.
//!
//! This backs the right-hand info panel -- the "what is this, how do I run it,
//! what else can it do" surface. Everything here is **read**, never authored:
//! the summary is the project's own README prose, the extra commands are the
//! scripts really declared in its manifest, and the docs are files that exist.
//!
//! That constraint is the whole design. A generated description of a project
//! is a plausible-sounding guess that ages badly and cannot be corrected by
//! editing the repo; a README paragraph is the truth the project maintains
//! about itself. When there is no README, the panel says so rather than
//! inventing a purpose.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// Longest summary worth showing in a side panel before it stops being a
/// summary. Prose past this is a click away in the README itself.
const SUMMARY_CAP: usize = 420;

/// A named command the project declares beyond its runner's lifecycles.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Script {
    /// Script name as declared (`dev`, `test:watch`).
    pub name: String,
    /// The command it maps to, verbatim from the manifest.
    pub command: String,
}

/// A documentation file found at the project root or in `docs/`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocFile {
    /// Path relative to the project root, forward-slashed for display.
    pub name: String,
    /// Absolute path, so the panel can open it.
    pub path: PathBuf,
}

/// Everything the manual panel needs that is not already on the project DTO.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Manual {
    /// First real paragraph of the README, markdown stripped. `None` when the
    /// project has no README or it opens with no prose.
    pub summary: Option<String>,
    /// The README that `summary` came from, when there was one.
    pub readme: Option<PathBuf>,
    /// Doc files worth linking, README first.
    pub docs: Vec<DocFile>,
    /// Scripts declared in `package.json`, or binaries in `Cargo.toml`.
    pub scripts: Vec<Script>,
}

/// Reads whatever `root` says about itself.
///
/// Never fails: an unreadable project yields an empty manual, which the panel
/// renders as "this project has no README", not as an error.
#[must_use]
pub fn read(root: &Path) -> Manual {
    let mut manual = Manual::default();

    if let Some(readme) = find_readme(root) {
        if let Ok(text) = std::fs::read_to_string(&readme) {
            manual.summary = first_paragraph(&text);
        }
        manual.readme = Some(readme);
    }
    manual.docs = docs(root);
    manual.scripts = scripts(root);
    manual
}

/// The root README under any of the usual spellings.
fn find_readme(root: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(root).ok()?;
    entries
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.eq_ignore_ascii_case("readme"))
                && p.is_file()
        })
}

/// The first paragraph of prose, skipping title and badge furniture.
///
/// READMEs open with a heading, then often a row of shield badges, then
/// sometimes an HTML block, before saying anything. Taking "the first
/// non-empty line" gets the title back, which the panel already shows as the
/// project name -- so the whole job here is knowing what to skip.
fn first_paragraph(markdown: &str) -> Option<String> {
    let mut para: Vec<&str> = Vec::new();

    for raw in markdown.lines() {
        let line = raw.trim();
        if para.is_empty() {
            // Still hunting for the start: skip headings, badges, images,
            // HTML, block quotes, front matter fences and rules.
            let skip = line.is_empty()
                || line.starts_with('#')
                || line.starts_with("![")
                || line.starts_with("[![")
                || line.starts_with('<')
                || line.starts_with('>')
                || line.starts_with("---")
                || line.starts_with("===")
                || line.starts_with("```");
            if skip {
                continue;
            }
            para.push(line);
        } else if line.is_empty() {
            break; // paragraph ended
        } else {
            para.push(line);
        }
    }

    if para.is_empty() {
        return None;
    }
    let text = strip_markdown(&para.join(" "));
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(truncate_on_word(trimmed, SUMMARY_CAP))
}

/// Flattens the inline markdown a one-paragraph summary can contain.
///
/// Link text is kept and the target dropped (`[Vesper](docs/x.md)` -> `Vesper`);
/// emphasis and code ticks are removed. Deliberately not a markdown parser:
/// this handles the inline constructs that appear in a first paragraph, and
/// anything it misses degrades to showing the raw characters, which is
/// readable rather than wrong.
fn strip_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            // `[label](target)` and `![alt](src)` -> keep the label only.
            '[' => {
                let label: String = chars.by_ref().take_while(|&c| c != ']').collect();
                out.push_str(&label);
                if chars.peek() == Some(&'(') {
                    // Consume the target, including nested parens in URLs.
                    let mut depth = 0_usize;
                    for c in chars.by_ref() {
                        match c {
                            '(' => depth += 1,
                            ')' => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            '*' | '_' | '`' => {}
            _ => out.push(c),
        }
    }
    // Collapse the runs of whitespace that stripping can leave behind.
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Cuts at the last word boundary under `cap`, adding an ellipsis.
fn truncate_on_word(text: &str, cap: usize) -> String {
    if text.chars().count() <= cap {
        return text.to_owned();
    }
    let head: String = text.chars().take(cap).collect();
    let cut = head.rfind(' ').unwrap_or(head.len());
    format!("{}…", head[..cut].trim_end_matches(['.', ',', ';']))
}

/// Markdown docs at the root and one level into `docs/`.
fn docs(root: &Path) -> Vec<DocFile> {
    let mut out = Vec::new();

    let mut collect = |dir: &Path, prefix: &str| {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut found: Vec<DocFile> = entries
            .flatten()
            .filter(|e| e.path().is_file())
            .filter(|e| {
                e.path()
                    .extension()
                    .and_then(|x| x.to_str())
                    .is_some_and(|x| x.eq_ignore_ascii_case("md"))
            })
            .map(|e| DocFile {
                name: format!("{prefix}{}", e.file_name().to_string_lossy()),
                path: e.path(),
            })
            .collect();
        found.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        out.extend(found);
    };

    collect(root, "");
    let docs_dir = root.join("docs");
    if docs_dir.is_dir() {
        collect(&docs_dir, "docs/");
    }

    // README first: it is the one a reader wants, and alphabetical order
    // buries it under CHANGELOG and CONTRIBUTING.
    out.sort_by_key(|d| !d.name.to_lowercase().starts_with("readme"));
    out
}

/// Named commands the project declares for itself.
///
/// `package.json` scripts and `Cargo.toml` binaries, because those are the two
/// manifests that actually list runnable things by name. A Python project's
/// entry points live in too many competing places to read honestly, so none
/// are claimed rather than guessing at one convention.
fn scripts(root: &Path) -> Vec<Script> {
    let mut out = Vec::new();

    if let Ok(text) = std::fs::read_to_string(root.join("package.json")) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(map) = v.get("scripts").and_then(|s| s.as_object()) {
                for (name, cmd) in map {
                    if let Some(cmd) = cmd.as_str() {
                        out.push(Script { name: name.clone(), command: cmd.to_owned() });
                    }
                }
            }
        }
    }

    if let Ok(text) = std::fs::read_to_string(root.join("Cargo.toml")) {
        // `Table`, not `Value`: in toml 1.x `Value::from_str` parses a single
        // VALUE, so a whole document starts with `[package]` and is read as an
        // array literal -- it fails at column 10 of line 1 every time.
        if let Ok(v) = text.parse::<toml::Table>() {
            // `[[bin]] name = "x"` -> `cargo run --bin x`
            if let Some(bins) = v.get("bin").and_then(|b| b.as_array()) {
                for bin in bins {
                    if let Some(name) = bin.get("name").and_then(|n| n.as_str()) {
                        out.push(Script {
                            name: name.to_owned(),
                            command: format!("cargo run --bin {name}"),
                        });
                    }
                }
            }
        }
    }

    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "deck-manual-{tag}-{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The shape a real README opens with: title, badge row, then the prose.
    /// Returning the title here would duplicate the project name the panel
    /// already shows, which is what made this worth a function.
    #[test]
    fn the_summary_skips_the_title_and_badges() {
        let md = "# Launch Deck\n\n\
                  [![build](https://img.shields.io/x.svg)](https://ci.example)\n\
                  ![screenshot](docs/shot.png)\n\n\
                  A local control centre for **running**, debugging and\n\
                  prompting the apps on this machine.\n\n\
                  Second paragraph is not part of the summary.";
        let s = first_paragraph(md).expect("a summary");
        assert_eq!(
            s,
            "A local control centre for running, debugging and prompting the apps on this machine."
        );
    }

    #[test]
    fn link_targets_are_dropped_and_labels_kept() {
        let md = "Text\n\nSee the [roadmap](docs/ROADMAP.md) and `tools/build.ps1`.";
        // First paragraph is "Text"; check the stripper directly for links.
        assert_eq!(
            strip_markdown("See the [roadmap](docs/ROADMAP.md) and `tools/build.ps1`."),
            "See the roadmap and tools/build.ps1."
        );
        assert_eq!(first_paragraph(md).unwrap(), "Text");
    }

    #[test]
    fn a_readme_with_only_a_title_yields_no_summary() {
        assert!(first_paragraph("# Just a title\n").is_none());
        assert!(first_paragraph("").is_none());
    }

    #[test]
    fn a_long_paragraph_is_cut_on_a_word_boundary() {
        let long = "word ".repeat(200);
        let s = truncate_on_word(long.trim(), 50);
        assert!(s.chars().count() <= 51, "cap plus the ellipsis");
        assert!(s.ends_with('…'));
        assert!(!s.contains("wor…"), "must not cut mid-word");
    }

    #[test]
    fn scripts_come_from_the_real_manifests() {
        let dir = scratch("scripts");
        std::fs::write(
            dir.join("package.json"),
            r#"{"name":"x","scripts":{"dev":"vite","build":"tsc && vite build"}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"x\"\n\n[[bin]]\nname = \"probe\"\npath = \"src/bin/probe.rs\"\n",
        )
        .unwrap();

        let found = scripts(&dir);
        let by = |n: &str| found.iter().find(|s| s.name == n).map(|s| s.command.clone());
        assert_eq!(by("dev").as_deref(), Some("vite"));
        assert_eq!(by("build").as_deref(), Some("tsc && vite build"));
        assert_eq!(by("probe").as_deref(), Some("cargo run --bin probe"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn docs_list_readme_first_and_include_the_docs_folder() {
        let dir = scratch("docs");
        std::fs::write(dir.join("CHANGELOG.md"), "x").unwrap();
        std::fs::write(dir.join("README.md"), "# T\n\nProse.").unwrap();
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::write(dir.join("docs/ROADMAP.md"), "x").unwrap();
        std::fs::write(dir.join("notes.txt"), "not markdown").unwrap();

        let found = docs(&dir);
        assert_eq!(found[0].name, "README.md", "the README leads");
        let names: Vec<&str> = found.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"docs/ROADMAP.md"));
        assert!(!names.iter().any(|n| n.to_lowercase().ends_with(".txt")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_project_with_nothing_reads_as_empty_not_as_an_error() {
        let dir = scratch("bare");
        let m = read(&dir);
        assert!(m.summary.is_none());
        assert!(m.readme.is_none());
        assert!(m.docs.is_empty());
        assert!(m.scripts.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_pulls_the_summary_and_the_readme_path_together() {
        let dir = scratch("read");
        std::fs::write(dir.join("readme.MD"), "# X\n\nDoes a thing.\n").unwrap();
        let m = read(&dir);
        assert_eq!(m.summary.as_deref(), Some("Does a thing."));
        assert!(m.readme.is_some(), "any spelling of readme counts");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
