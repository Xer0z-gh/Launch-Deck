//! Prompt Studio's pure logic: filtering a log tail down to the lines worth
//! showing Claude, and merging a hand-off prompt into a repo's `NEXT.md`
//! without destroying what is already there.
//!
//! File IO stays in the command layer; everything here is deterministic and
//! tested. The one honesty rule: a filter that finds nothing falls back to
//! the raw tail rather than pretending the log was clean — an empty "Recent
//! errors" section under a crashing app would be a lie of omission.

use std::path::{Path, PathBuf};

/// Lines an error filter keeps, beyond anything matching [`ERROR_MARKS`]:
/// when nothing matches, the last `RAW_FALLBACK` raw lines stand in, because
/// "no lines matched my regex" and "the log is clean" are different claims.
const RAW_FALLBACK: usize = 20;

/// Substrings (lowercased haystack) that mark a line as error-ish. `warn` is
/// deliberately absent: warnings flood real logs and drown the signal the
/// debug prompt exists to carry.
const ERROR_MARKS: [&str; 7] = [
    "error", "panic", "exception", "fatal", "failed", "failure", "traceback",
];

/// One source's tail plus the fact that decides what to call it.
///
/// `matched` is the honesty bit: `true` means the lines carried error marks,
/// `false` means this is a plain raw tail because nothing matched. The UI and
/// the generated prompt MUST label the two differently — a raw tail under a
/// "Recent errors" heading invents errors that are not there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailReading {
    /// The tail itself, oldest first.
    pub lines: Vec<String>,
    /// Whether the lines carried error marks (vs a raw-tail fallback).
    pub matched: bool,
}

/// The error-worthy tail of `text`: up to `max` matching lines, newest last.
///
/// Falls back to the raw tail (bounded by [`RAW_FALLBACK`]) when nothing
/// matches — with `matched: false` so the caller can say so.
#[must_use]
pub fn error_lines(text: &str, max: usize) -> TailReading {
    let matched: Vec<&str> = text
        .lines()
        .filter(|l| {
            let lower = l.to_lowercase();
            ERROR_MARKS.iter().any(|m| lower.contains(m))
        })
        .collect();

    let take = |lines: &[&str], n: usize| -> Vec<String> {
        lines
            .iter()
            .skip(lines.len().saturating_sub(n))
            .map(|l| (*l).to_string())
            .collect()
    };

    if matched.is_empty() {
        let all: Vec<&str> = text.lines().collect();
        TailReading {
            lines: take(&all, RAW_FALLBACK.min(max)),
            matched: false,
        }
    } else {
        TailReading {
            lines: take(&matched, max),
            matched: true,
        }
    }
}

/// Merges a hand-off `prompt` into an existing `NEXT.md`, Fleet-style: the
/// backlog above survives untouched and the paste-ready prompt lands at the
/// bottom under a dated heading. `existing = None` creates the file fresh.
///
/// Appending rather than replacing is the whole design: Fleet's `NEXT.md`
/// carries a prioritized backlog, and a hand-off that overwrote it would be
/// a data-loss button dressed as a convenience.
#[must_use]
pub fn append_handoff(existing: Option<&str>, prompt: &str, stamp: &str) -> String {
    let heading = format!("## Handed off from Launch Deck ({stamp})");
    match existing {
        Some(body) => format!("{}\n\n---\n\n{heading}\n\n{prompt}\n", body.trim_end()),
        None => format!("# NEXT\n\n{heading}\n\n{prompt}\n"),
    }
}

/// The most recently modified file directly inside `dir`, if any.
///
/// The supervisor writes one log file per run into a project's log folder;
/// "the newest file" is therefore "the last run", which is what a debug
/// prompt wants when nothing is running right now.
#[must_use]
pub fn newest_file(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| {
            let modified = e.metadata().ok()?.modified().ok()?;
            Some((modified, e.path()))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

/// The last `max_bytes` of the file as text, tolerating a cut-off first line.
///
/// Reading a whole multi-hundred-MB log to show forty lines would be waste;
/// seeking to the tail keeps the command instant regardless of log size.
#[must_use]
pub fn read_tail(path: &Path, max_bytes: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    if len > max_bytes {
        file.seek(SeekFrom::Start(len - max_bytes)).ok()?;
    }
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).ok()?;
    let mut text = String::from_utf8_lossy(&buf).to_string();
    // A seek lands mid-line; the partial first line is noise, drop it.
    if len > max_bytes {
        if let Some((_, rest)) = text.split_once('\n') {
            text = rest.to_string();
        }
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_lines_keeps_matches_and_bounds_them() {
        let log = "ok line\nERROR: one\nfine\npanic: two\nError three\n";
        let got = error_lines(log, 2);
        assert!(got.matched, "error-marked lines must report matched");
        assert_eq!(got.lines, vec!["panic: two", "Error three"]);
    }

    /// A clean log falls back to the raw tail — never an empty section that
    /// would read as "nothing to see here" under a broken app.
    #[test]
    fn a_clean_log_falls_back_to_the_raw_tail() {
        let mut log = String::new();
        for i in 0..40 {
            use std::fmt::Write;
            let _ = writeln!(log, "line {i}");
        }
        let got = error_lines(&log, 50);
        assert!(!got.matched, "a raw-tail fallback must say it is one");
        assert_eq!(got.lines.len(), 20);
        assert_eq!(got.lines.first().unwrap(), "line 20");
        assert_eq!(got.lines.last().unwrap(), "line 39");
    }

    #[test]
    fn handoff_appends_below_an_existing_backlog() {
        let existing = "# NEXT\n\n1. real backlog item\n";
        let got = append_handoff(Some(existing), "Do the thing.", "2026-08-31");
        assert!(got.starts_with("# NEXT\n\n1. real backlog item"));
        assert!(got.contains("## Handed off from Launch Deck (2026-08-31)"));
        assert!(got.trim_end().ends_with("Do the thing."));
    }

    #[test]
    fn handoff_creates_the_file_when_absent() {
        let got = append_handoff(None, "Fresh prompt.", "2026-08-31");
        assert!(got.starts_with("# NEXT\n"));
        assert!(got.contains("Fresh prompt."));
    }

    #[test]
    fn newest_file_picks_by_mtime_and_ignores_dirs() {
        let dir = std::env::temp_dir().join(format!(
            "deck-studio-{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("old.log"), "a").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::write(dir.join("new.log"), "b").unwrap();

        let got = newest_file(&dir).unwrap();
        assert!(got.ends_with("new.log"), "got {got:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_tail_returns_whole_small_files_and_seeks_large_ones() {
        let dir = std::env::temp_dir().join(format!(
            "deck-tail-{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.log");
        std::fs::write(&p, "first\nsecond\nthird\n").unwrap();

        assert_eq!(read_tail(&p, 1024).unwrap(), "first\nsecond\nthird\n");
        // A tail cut mid-line drops the partial first line.
        assert_eq!(read_tail(&p, 13).unwrap(), "third\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
