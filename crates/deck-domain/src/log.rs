//! Log lines and the outcome of a run.
//!
//! A [`LogLine`] is intentionally small. A busy dev server emits thousands per
//! second, each one is cloned into a ring buffer, batched, serialized, and sent
//! over IPC -- so every field here is paid for many times over. Anything
//! derivable on the frontend (formatted timestamps, ANSI-stripped text) is
//! derived there rather than carried.

use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Which stream a line came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogStream {
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
    /// Emitted by Launch Deck itself, not the child process.
    ///
    /// Used for "process started", "stop requested", "exited with code 1" so
    /// the log reads as a complete account of the run rather than only the
    /// child's half of it. Rendered distinctly from process output.
    Deck,
}

impl LogStream {
    /// Short lower-case name, used as the filter key in the UI.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
            Self::Deck => "deck",
        }
    }
}

impl fmt::Display for LogStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One line of output from a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogLine {
    /// Monotonic per-run sequence number.
    ///
    /// The frontend keys on this and uses it to detect gaps after a buffer
    /// overflow. Timestamps are not sufficient: at this volume many lines share
    /// a millisecond, and ordering must be exact.
    pub seq: u64,

    /// Which stream produced the line.
    pub stream: LogStream,

    /// Line content, with ANSI escapes preserved.
    ///
    /// Kept raw so the viewer can render colour. Stripping happens at search
    /// and export time, never on ingest -- once discarded the colour is gone.
    pub text: String,

    /// When the line was read.
    pub at: DateTime<Utc>,
}

impl LogLine {
    /// Builds a line originating from the supervisor rather than a child.
    #[must_use]
    pub fn deck(seq: u64, text: impl Into<String>) -> Self {
        Self {
            seq,
            stream: LogStream::Deck,
            text: text.into(),
            at: Utc::now(),
        }
    }

    /// Whether the line looks like an error, for highlighting.
    ///
    /// A heuristic, and only ever used to *emphasise* a line -- never to hide
    /// one, and never to decide an exit status. Matches leading level markers
    /// and common prefixes rather than any occurrence of the word "error", so a
    /// line reading "0 errors" does not light up red.
    #[must_use]
    pub fn looks_like_error(&self) -> bool {
        const PREFIXES: [&str; 10] = [
            "error", "error:", "err!", "fatal", "panic", "exception",
            "traceback", "uncaught", "unhandled", "failed",
        ];

        let text = strip_ansi(&self.text);
        let lower = text.trim_start().to_ascii_lowercase();
        if PREFIXES.iter().any(|p| lower.starts_with(p)) {
            return true;
        }
        // Bracketed and level-prefixed forms: "[ERROR]", "ERROR ", "E/tag:".
        lower.starts_with("[error")
            || lower.starts_with("[fatal")
            || lower.starts_with("error ")
            || lower.contains("] error")
    }
}

/// Removes ANSI escape sequences from a string.
///
/// Handles CSI sequences (`ESC [ ... final`) and OSC sequences (`ESC ] ... BEL`
/// or `ESC ] ... ESC \`), which together cover what build tools emit: colour,
/// cursor movement, and terminal-title updates. Used for search, export and the
/// error heuristic -- the stored line always keeps its escapes.
#[must_use]
pub fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            out.push(ch);
            continue;
        }
        match chars.peek() {
            Some('[') => {
                chars.next();
                // CSI: parameters and intermediates, terminated by @-~.
                for c in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next();
                // OSC: terminated by BEL or ST (ESC \).
                while let Some(c) = chars.next() {
                    if c == '\u{7}' {
                        break;
                    }
                    if c == '\u{1b}' {
                        if chars.peek() == Some(&'\\') {
                            chars.next();
                        }
                        break;
                    }
                }
            }
            // A lone ESC, or a two-character sequence like ESC c. Drop the ESC
            // and, for known two-char forms, the character after it.
            Some('c' | '7' | '8' | '=' | '>') => {
                chars.next();
            }
            _ => {}
        }
    }

    out
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcome {
    /// Still going.
    Running,
    /// Exited with code 0.
    Succeeded,
    /// Exited with a non-zero code.
    Failed,
    /// Stopped because the user asked.
    Stopped,
    /// Terminated after ignoring a graceful stop.
    Killed,
    /// Could not be started at all.
    FailedToStart,
    /// The run's end was never recorded: the app was killed, or the machine
    /// went down, before the supervisor could write a finish. Distinct from
    /// every other variant because it is an ABSENCE of knowledge, not an
    /// observation -- and distinct from `Running`, which claims the process
    /// is alive right now. Rows are moved here by the startup reconciliation
    /// in `deck_store`, since nothing is running at boot by definition.
    Interrupted,
}

impl RunOutcome {
    /// Whether this outcome represents an unwanted ending.
    ///
    /// A user-requested stop is not a failure, which matters: crash history
    /// that fills with intentional stops is history nobody reads.
    #[must_use]
    pub const fn is_failure(self) -> bool {
        matches!(self, Self::Failed | Self::FailedToStart)
    }

    /// Whether the run has finished.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        !matches!(self, Self::Running)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str) -> LogLine {
        LogLine {
            seq: 1,
            stream: LogStream::Stdout,
            text: text.to_owned(),
            at: Utc::now(),
        }
    }

    #[test]
    fn strips_colour_sequences() {
        assert_eq!(strip_ansi("\u{1b}[31mred\u{1b}[0m"), "red");
    }

    #[test]
    fn strips_multi_parameter_sequences() {
        assert_eq!(strip_ansi("\u{1b}[1;32;40mgo\u{1b}[m"), "go");
    }

    #[test]
    fn strips_cursor_movement() {
        assert_eq!(strip_ansi("a\u{1b}[2Kb"), "ab");
    }

    #[test]
    fn strips_osc_title_sequences_terminated_by_bel() {
        assert_eq!(strip_ansi("\u{1b}]0;title\u{7}text"), "text");
    }

    #[test]
    fn strips_osc_terminated_by_string_terminator() {
        assert_eq!(strip_ansi("\u{1b}]0;t\u{1b}\\after"), "after");
    }

    #[test]
    fn leaves_plain_text_untouched() {
        let s = "ready in 431ms";
        assert_eq!(strip_ansi(s), s);
    }

    #[test]
    fn preserves_non_ascii() {
        assert_eq!(strip_ansi("\u{1b}[32m✓ done\u{1b}[0m"), "✓ done");
    }

    #[test]
    fn error_heuristic_catches_common_shapes() {
        assert!(line("Error: cannot find module").looks_like_error());
        assert!(line("ERROR  Build failed").looks_like_error());
        assert!(line("[ERROR] compilation failed").looks_like_error());
        assert!(line("FATAL: unreachable").looks_like_error());
        assert!(line("Traceback (most recent call last):").looks_like_error());
        assert!(line("panic: runtime error").looks_like_error());
    }

    #[test]
    fn error_heuristic_sees_through_colour_codes() {
        assert!(line("\u{1b}[31mError: boom\u{1b}[0m").looks_like_error());
    }

    #[test]
    fn error_heuristic_does_not_fire_on_incidental_mentions() {
        // The whole point of anchoring on prefixes: a success line that happens
        // to contain the word must not render as a failure.
        assert!(!line("0 errors, 0 warnings").looks_like_error());
        assert!(!line("added error handling to the parser").looks_like_error());
        assert!(!line("compiled successfully").looks_like_error());
    }

    #[test]
    fn user_stop_is_not_counted_as_a_failure() {
        assert!(!RunOutcome::Stopped.is_failure());
        assert!(!RunOutcome::Killed.is_failure());
        assert!(RunOutcome::Failed.is_failure());
        assert!(RunOutcome::FailedToStart.is_failure());
    }

    #[test]
    fn only_running_is_non_terminal() {
        assert!(!RunOutcome::Running.is_terminal());
        for o in [
            RunOutcome::Succeeded,
            RunOutcome::Failed,
            RunOutcome::Stopped,
            RunOutcome::Killed,
            RunOutcome::FailedToStart,
        ] {
            assert!(o.is_terminal());
        }
    }

    #[test]
    fn deck_lines_are_tagged_as_ours() {
        assert_eq!(LogLine::deck(0, "started").stream, LogStream::Deck);
    }
}
