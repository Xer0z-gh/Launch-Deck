//! The log pipeline: capture, bound, persist, broadcast.
//!
//! Volume is the whole design problem. A webpack build in watch mode can emit
//! megabytes a minute, and a crash loop emits the same stack trace forever. Three
//! decisions follow from that:
//!
//! 1. **Lines live in files, not the database.** `SQLite` stores a path and some
//!    metadata per run; the bytes go to a per-run file on disk. A schema holding
//!    individual log lines would grow without bound and make every query slow.
//! 2. **Memory is a bounded ring.** The live view needs recent history, not all
//!    of it. [`LogSink`] keeps a fixed number of lines and counts what it dropped,
//!    so the UI can say "1,204 earlier lines" instead of silently lying.
//! 3. **Broadcast is lossy on purpose.** A subscriber that cannot keep up misses
//!    lines rather than applying backpressure to the child process. Blocking a
//!    build because a log viewer is slow would be the wrong trade every time.
//!
//! Coalescing lives at the IPC boundary rather than here: emitting one Tauri
//! event per line saturates the bridge, so the consumer batches on a short timer.

use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use deck_domain::log::{LogLine, LogStream};
use parking_lot::Mutex;

/// How many lines the live ring buffer holds per run.
///
/// 5,000 lines is roughly a screen's worth of scrollback several times over --
/// enough to diagnose a failure without holding a build's entire output in RAM
/// for every registered project.
pub const RING_CAPACITY: usize = 5_000;

/// Capacity of the live broadcast channel, in lines.
pub const BROADCAST_CAPACITY: usize = 1_024;

/// Longest single line retained.
///
/// A tool emitting a megabyte-long minified bundle on one line would otherwise
/// put that megabyte in the ring buffer, the file, and every IPC frame.
pub const MAX_LINE_BYTES: usize = 16 * 1024;

/// Most bytes one run may write to its log file.
///
/// The in-memory ring has always been bounded; the FILE was not, and a program
/// in an output loop can outrun any human reaction. Measured, not imagined:
/// Circle-Calculator, an interactive C++ menu, wrote **4,151,426 lines and
/// 208.7 MB in five seconds** after its stdin was closed -- it read EOF,
/// printed "Invalid choice!", redrew its menu, and did that as fast as the
/// pipe allowed until the stop grace period expired and it was killed.
///
/// 32 MiB is far more than any honest run needs -- the next largest log this
/// machine has ever produced is 4 KB -- and small enough that a runaway costs a
/// fraction of a second of disk instead of filling the drive. On reaching it
/// the file is closed with a final line saying so, and the run keeps going:
/// truncating the record of a misbehaving program must not also stop it.
pub const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;

/// The in-memory tail of a run's output, plus its on-disk record.
///
/// Cloneable handles share one sink via [`std::sync::Arc`]; the stdout and stderr
/// reader tasks each hold one.
pub struct LogSink {
    ring: Mutex<VecDeque<LogLine>>,
    file: Mutex<Option<std::io::BufWriter<std::fs::File>>>,
    path: PathBuf,
    next_seq: AtomicU64,
    dropped: AtomicU64,
    /// Bytes written to the file so far, for the [`MAX_FILE_BYTES`] cap.
    file_bytes: AtomicU64,
    broadcast: tokio::sync::broadcast::Sender<LogLine>,
}

impl LogSink {
    /// Creates a sink writing to `path`.
    ///
    /// The parent directory is created if absent. If the file cannot be opened,
    /// the sink still works in memory and the failure is logged -- losing the
    /// on-disk copy is much better than failing the launch the user asked for.
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        let file = open_log_file(&path).map_or_else(
            |e| {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "could not open log file; continuing with in-memory logs only"
                );
                None
            },
            Some,
        );

        let (broadcast, _) = tokio::sync::broadcast::channel(BROADCAST_CAPACITY);

        Self {
            ring: Mutex::new(VecDeque::with_capacity(RING_CAPACITY.min(512))),
            file: Mutex::new(file),
            path,
            next_seq: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            file_bytes: AtomicU64::new(0),
            broadcast,
        }
    }

    /// Where this run's log is written.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends a line from a child process stream.
    ///
    /// Returns the stored line, which carries the assigned sequence number.
    pub fn push(&self, stream: LogStream, text: impl Into<String>) -> LogLine {
        let mut text = text.into();
        if text.len() > MAX_LINE_BYTES {
            // Truncate on a character boundary so the result stays valid UTF-8.
            let mut cut = MAX_LINE_BYTES;
            while cut > 0 && !text.is_char_boundary(cut) {
                cut -= 1;
            }
            text.truncate(cut);
            text.push_str(" ... [line truncated]");
        }

        let line = LogLine {
            seq: self.next_seq.fetch_add(1, Ordering::Relaxed),
            stream,
            text,
            at: chrono::Utc::now(),
        };

        self.write_to_file(&line);

        {
            let mut ring = self.ring.lock();
            if ring.len() >= RING_CAPACITY {
                ring.pop_front();
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
            ring.push_back(line.clone());
        }

        // A send error only means nobody is listening, which is the common case.
        let _ = self.broadcast.send(line.clone());

        line
    }

    /// Appends a line originating from Launch Deck itself.
    ///
    /// Used for "started", "stop requested", "exited with code 1", so the log
    /// reads as a full account of the run rather than only the child's half.
    pub fn push_deck(&self, text: impl Into<String>) -> LogLine {
        self.push(LogStream::Deck, text)
    }

    /// The retained tail, oldest first.
    #[must_use]
    pub fn snapshot(&self) -> Vec<LogLine> {
        self.ring.lock().iter().cloned().collect()
    }

    /// Retained lines with a sequence number at or above `after`.
    ///
    /// Lets a reconnecting viewer resume without re-fetching everything.
    #[must_use]
    pub fn since(&self, after: u64) -> Vec<LogLine> {
        self.ring
            .lock()
            .iter()
            .filter(|l| l.seq >= after)
            .cloned()
            .collect()
    }

    /// How many lines have been evicted from the ring.
    ///
    /// Surfaced in the viewer so truncated history is visible rather than
    /// silently missing.
    #[must_use]
    pub fn dropped_count(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Total lines ever pushed.
    #[must_use]
    pub fn total_count(&self) -> u64 {
        self.next_seq.load(Ordering::Relaxed)
    }

    /// Subscribes to live lines.
    ///
    /// The receiver is lossy: a subscriber that falls behind by more than
    /// [`BROADCAST_CAPACITY`] receives a lag error and resumes from the newest
    /// line. That is deliberate -- see the module docs.
    #[must_use]
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<LogLine> {
        self.broadcast.subscribe()
    }

    /// Flushes buffered output to disk.
    ///
    /// Called when a run ends, so a crash log is complete on disk by the time the
    /// UI shows the crash.
    pub fn flush(&self) {
        if let Some(file) = self.file.lock().as_mut() {
            if let Err(e) = file.flush() {
                tracing::warn!(path = %self.path.display(), error = %e, "log flush failed");
            }
        }
    }

    fn write_to_file(&self, line: &LogLine) {
        let mut guard = self.file.lock();
        let Some(file) = guard.as_mut() else {
            return;
        };

        // Stop at the budget rather than letting a looping program fill the
        // disk. Checked before the write so the cap is a ceiling, not a
        // threshold crossed by however long the last line happened to be.
        if self.file_bytes.load(Ordering::Relaxed) >= MAX_FILE_BYTES {
            let _ = writeln!(
                file,
                "{} [deck] log file capped at {} MiB -- this run is still going, \
                 but its output is no longer being written to disk",
                line.at.format("%Y-%m-%dT%H:%M:%S%.3f"),
                MAX_FILE_BYTES / (1024 * 1024),
            );
            let _ = file.flush();
            tracing::warn!(
                path = %self.path.display(),
                cap_bytes = MAX_FILE_BYTES,
                "log file hit its size cap; continuing in memory only"
            );
            // Dropping the writer is what makes this cheap: every later line
            // takes the `else` branch above and costs nothing.
            *guard = None;
            return;
        }

        // Prefixed with stream and timestamp so an exported log is readable
        // standalone, outside the app.
        let stamp = line.at.format("%Y-%m-%dT%H:%M:%S%.3f");
        let stream = line.stream.as_str();
        let result = writeln!(file, "{stamp} [{stream}] {}", line.text);
        if let Err(e) = result {
            tracing::warn!(path = %self.path.display(), error = %e, "log write failed");
            // Stop retrying on every subsequent line: one warning, then memory
            // only. A full disk should not produce a warning per log line.
            *guard = None;
            return;
        }

        // Close enough to exact: the separators and the newline are fixed, and
        // the timestamp is a constant width. Counting written bytes here avoids
        // a `metadata()` syscall per line, which at four million lines a second
        // would itself be the problem.
        let written = 3 + stream.len() + line.text.len() + 25;
        self.file_bytes
            .fetch_add(written as u64, Ordering::Relaxed);
    }
}

impl std::fmt::Debug for LogSink {
    /// Summarises the sink rather than dumping it.
    ///
    /// Hand-written because a derived impl would print the entire ring buffer --
    /// up to [`RING_CAPACITY`] lines of build output -- into any log line or panic
    /// message that formats a sink.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogSink")
            .field("path", &self.path)
            .field("total", &self.total_count())
            .field("dropped", &self.dropped_count())
            .finish_non_exhaustive()
    }
}

fn open_log_file(path: &Path) -> std::io::Result<std::io::BufWriter<std::fs::File>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    Ok(std::io::BufWriter::new(file))
}

/// Removes log files for runs older than `keep_per_project` for one project.
///
/// Called after a run finishes. Without this, a project launched many times a day
/// accumulates log files indefinitely -- the kind of slow leak that is invisible
/// until a disk fills.
///
/// # Errors
///
/// Returns an [`std::io::Error`] if the directory cannot be read. Individual
/// files that cannot be deleted are logged and skipped.
pub fn prune_logs(dir: &Path, keep_per_project: usize) -> std::io::Result<usize> {
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(dir)?
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "log"))
        .filter_map(|e| {
            let modified = e.metadata().and_then(|m| m.modified()).ok()?;
            Some((modified, e.path()))
        })
        .collect();

    if files.len() <= keep_per_project {
        return Ok(0);
    }

    // Newest first, then delete everything past the keep count.
    files.sort_by(|a, b| b.0.cmp(&a.0));
    let mut removed = 0;
    for (_, path) in files.into_iter().skip(keep_per_project) {
        match std::fs::remove_file(&path) {
            Ok(()) => removed += 1,
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "could not prune log file");
            }
        }
    }
    Ok(removed)
}

/// Default ceiling for the whole log tree.
///
/// Per-project retention alone is not a bound: ten runs each, across as many
/// projects as the user registers, times a per-run cap of 32 MiB. Measured on
/// this machine after a week the tree held **90 MB**, most of it ten retained
/// copies of one program's runaway output at ~4 MB each -- each individually
/// within every limit that existed.
pub const MAX_TOTAL_LOG_BYTES: u64 = 256 * 1024 * 1024;

/// Deletes the oldest run logs until the whole tree fits in `max_total`.
///
/// The companion to [`prune_logs`], which bounds one project's history and
/// therefore cannot bound the sum of them. Oldest-first because recency is the
/// only ordering that matters for a log: the run you want to read is almost
/// always the last one.
///
/// Returns the number of bytes reclaimed.
///
/// # Errors
///
/// Returns an [`std::io::Error`] if the root directory cannot be read.
/// Individual files that cannot be deleted are logged and skipped, since a
/// locked file is a reason to reclaim less, not to give up.
pub fn prune_logs_total(root: &Path, max_total: u64) -> std::io::Result<u64> {
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = Vec::new();
    let mut total: u64 = 0;

    // One level of project directories, then their logs. A recursive walk would
    // be more general and would also follow whatever else ends up in here.
    for project in std::fs::read_dir(root)?.filter_map(Result::ok) {
        let Ok(entries) = std::fs::read_dir(project.path()) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().is_none_or(|x| x != "log") {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            let Ok(modified) = meta.modified() else {
                continue;
            };
            total += meta.len();
            files.push((modified, meta.len(), path));
        }
    }

    if total <= max_total {
        return Ok(0);
    }

    // Oldest first, so the newest run survives whatever the budget is.
    files.sort_by_key(|(modified, _, _)| *modified);

    let mut reclaimed = 0;
    for (_, size, path) in files {
        if total - reclaimed <= max_total {
            break;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => reclaimed += size,
            Err(e) => tracing::warn!(path = %path.display(), error = %e, "could not prune log"),
        }
    }
    Ok(reclaimed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let mut p = std::env::temp_dir();
        p.push(format!("deck-logs-{tag}-{nanos:x}"));
        p
    }

    struct Scratch(PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn sink(tag: &str) -> (LogSink, Scratch) {
        let dir = temp_path(tag);
        std::fs::create_dir_all(&dir).unwrap();
        let sink = LogSink::new(dir.join("run.log"));
        (sink, Scratch(dir))
    }

    #[test]
    fn assigns_monotonic_sequence_numbers() {
        let (s, _g) = sink("seq");
        assert_eq!(s.push(LogStream::Stdout, "a").seq, 0);
        assert_eq!(s.push(LogStream::Stdout, "b").seq, 1);
        assert_eq!(s.push(LogStream::Stderr, "c").seq, 2);
        assert_eq!(s.total_count(), 3);
    }

    #[test]
    fn sequence_is_shared_across_streams_so_ordering_is_exact() {
        // Interleaved stdout/stderr must have a single total order; timestamps
        // are not enough because many lines land in the same millisecond.
        let (s, _g) = sink("interleave");
        let a = s.push(LogStream::Stdout, "out");
        let b = s.push(LogStream::Stderr, "err");
        let c = s.push(LogStream::Stdout, "out2");
        assert!(a.seq < b.seq && b.seq < c.seq);
    }

    #[test]
    fn snapshot_returns_lines_oldest_first() {
        let (s, _g) = sink("snap");
        for i in 0..5 {
            s.push(LogStream::Stdout, format!("line {i}"));
        }
        let lines = s.snapshot();
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[0].text, "line 0");
        assert_eq!(lines[4].text, "line 4");
    }

    #[test]
    fn ring_is_bounded_and_reports_what_it_dropped() {
        let (s, _g) = sink("ring");
        let overflow = 50;
        for i in 0..RING_CAPACITY + overflow {
            s.push(LogStream::Stdout, format!("l{i}"));
        }
        let lines = s.snapshot();
        assert_eq!(lines.len(), RING_CAPACITY, "ring exceeded its capacity");
        assert_eq!(s.dropped_count(), overflow as u64);
        // Oldest retained line is the one just after the dropped range.
        assert_eq!(lines[0].text, format!("l{overflow}"));
        // Sequence numbers still reflect everything ever pushed.
        assert_eq!(s.total_count(), (RING_CAPACITY + overflow) as u64);
    }

    /// A program in an output loop must not be able to fill the disk.
    ///
    /// This is the bug Tanner reported, reproduced at 1/1000th scale.
    /// Circle-Calculator -- an interactive C++ menu -- read its closed stdin as
    /// an invalid menu choice and redrew the menu 4,151,426 times in five
    /// seconds, writing 208.7 MB. The ring buffer was bounded the whole time;
    /// the FILE was not, which is why the in-memory limits hid the problem.
    ///
    /// The cap is temporarily lowered rather than writing 32 MiB in a unit
    /// test, which would take longer than the whole suite.
    #[test]
    fn a_runaway_program_cannot_fill_the_disk() {
        let (s, _g) = sink("runaway");

        // Roughly 100 bytes per line, so this is far past any sane cap.
        let line = "x".repeat(80);
        for _ in 0..(MAX_FILE_BYTES / 100) + 5_000 {
            s.push(LogStream::Stdout, line.clone());
        }

        let on_disk = std::fs::metadata(s.path()).expect("log file").len();
        assert!(
            on_disk <= MAX_FILE_BYTES + MAX_LINE_BYTES as u64,
            "log file grew to {on_disk} bytes, past the {MAX_FILE_BYTES} cap"
        );

        // The run keeps going: capping the record of a misbehaving program must
        // not also stop it, and the live view must still work.
        assert_eq!(s.snapshot().len(), RING_CAPACITY);
        assert!(s.total_count() > MAX_FILE_BYTES / 100);
    }

    /// The cap must announce itself in the file it truncates.
    ///
    /// A log that simply stops is indistinguishable from a crash, and someone
    /// reading it later has no way to know the tail is missing rather than
    /// never written.
    #[test]
    fn the_cap_says_so_in_the_log() {
        let (s, _g) = sink("capnote");
        let line = "y".repeat(80);
        for _ in 0..(MAX_FILE_BYTES / 100) + 1_000 {
            s.push(LogStream::Stdout, line.clone());
        }
        let text = std::fs::read_to_string(s.path()).expect("log file");
        assert!(
            text.contains("log file capped at"),
            "the truncated log does not say it was truncated"
        );
    }

    #[test]
    fn since_resumes_from_a_sequence_number() {
        let (s, _g) = sink("since");
        for i in 0..10 {
            s.push(LogStream::Stdout, format!("l{i}"));
        }
        let tail = s.since(7);
        assert_eq!(tail.len(), 3);
        assert_eq!(tail[0].text, "l7");
    }

    #[test]
    fn over_long_lines_are_truncated_on_a_char_boundary() {
        let (s, _g) = sink("long");
        // Multi-byte characters, so a naive byte truncation would split one.
        let huge = "é".repeat(MAX_LINE_BYTES);
        let line = s.push(LogStream::Stdout, huge);
        assert!(line.text.len() <= MAX_LINE_BYTES + 32);
        assert!(line.text.ends_with("[line truncated]"));
        // Still valid UTF-8 by construction -- this would have panicked otherwise.
        assert!(line.text.chars().count() > 0);
    }

    #[test]
    fn ansi_escapes_are_preserved_in_storage() {
        // The viewer needs the colour; stripping happens at search/export time.
        let (s, _g) = sink("ansi");
        let line = s.push(LogStream::Stdout, "\u{1b}[31mfailed\u{1b}[0m");
        assert!(line.text.contains('\u{1b}'));
    }

    #[test]
    fn lines_are_written_to_disk_with_stream_and_timestamp() {
        let (s, _g) = sink("file");
        s.push(LogStream::Stdout, "hello");
        s.push(LogStream::Stderr, "oops");
        s.push_deck("exited with code 0");
        s.flush();

        let body = std::fs::read_to_string(s.path()).unwrap();
        assert!(body.contains("[stdout] hello"));
        assert!(body.contains("[stderr] oops"));
        assert!(body.contains("[deck] exited with code 0"));
    }

    #[test]
    fn deck_lines_are_tagged_distinctly_from_process_output() {
        let (s, _g) = sink("deck");
        assert_eq!(s.push_deck("started").stream, LogStream::Deck);
    }

    #[test]
    fn an_unopenable_log_path_still_yields_a_working_sink() {
        // A directory where the file should be: opening must fail, and the sink
        // must degrade to memory rather than failing the user's launch.
        let dir = temp_path("blocked");
        std::fs::create_dir_all(&dir).unwrap();
        let _g = Scratch(dir.clone());
        let s = LogSink::new(dir.clone()); // path is an existing directory
        let line = s.push(LogStream::Stdout, "still works");
        assert_eq!(line.text, "still works");
        assert_eq!(s.snapshot().len(), 1);
    }

    #[tokio::test]
    async fn subscribers_receive_live_lines() {
        let (s, _g) = sink("sub");
        let mut rx = s.subscribe();
        s.push(LogStream::Stdout, "live");
        let got = rx.recv().await.unwrap();
        assert_eq!(got.text, "live");
        assert_eq!(got.seq, 0);
    }

    #[tokio::test]
    async fn a_slow_subscriber_lags_rather_than_blocking_the_writer() {
        // The property that matters: a stalled log viewer must never apply
        // backpressure to a build.
        let (s, _g) = sink("lag");
        let mut rx = s.subscribe();
        for i in 0..BROADCAST_CAPACITY + 10 {
            s.push(LogStream::Stdout, format!("l{i}"));
        }
        // All writes completed without the subscriber reading anything.
        assert_eq!(s.total_count(), (BROADCAST_CAPACITY + 10) as u64);
        match rx.recv().await {
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => assert!(n > 0),
            other => panic!("expected a lag error, got {other:?}"),
        }
    }

    #[test]
    fn prune_keeps_the_newest_logs_and_removes_the_rest() {
        let dir = temp_path("prune");
        std::fs::create_dir_all(&dir).unwrap();
        let _g = Scratch(dir.clone());
        for i in 0..8 {
            std::fs::write(dir.join(format!("run{i}.log")), "x").unwrap();
            // Distinct mtimes so ordering is well-defined.
            std::thread::sleep(std::time::Duration::from_millis(12));
        }
        let removed = prune_logs(&dir, 3).unwrap();
        assert_eq!(removed, 5);
        let left = std::fs::read_dir(&dir).unwrap().count();
        assert_eq!(left, 3);
        // The three newest survived.
        assert!(dir.join("run7.log").exists());
        assert!(!dir.join("run0.log").exists());
    }

    #[test]
    fn prune_is_a_no_op_below_the_keep_threshold() {
        let dir = temp_path("prune-noop");
        std::fs::create_dir_all(&dir).unwrap();
        let _g = Scratch(dir.clone());
        std::fs::write(dir.join("a.log"), "x").unwrap();
        assert_eq!(prune_logs(&dir, 5).unwrap(), 0);
        assert!(dir.join("a.log").exists());
    }

    /// Per-project retention cannot bound the sum of projects: ten runs each,
    /// times however many projects exist, times a 32 MiB per-run cap. This
    /// machine's tree reached 90 MB with every individual file inside every
    /// limit that existed.
    #[test]
    fn the_whole_tree_is_bounded_and_the_newest_run_survives() {
        let root = temp_path("total");
        std::fs::create_dir_all(&root).unwrap();
        let _guard = Scratch(root.clone());

        // Two projects, five runs each, 1 KiB per run.
        let mut newest = None;
        for project in 0..2 {
            let dir = root.join(format!("project-{project}"));
            std::fs::create_dir_all(&dir).unwrap();
            for run in 0..5 {
                let f = dir.join(format!("run-{run}.log"));
                std::fs::write(&f, vec![b'x'; 1024]).unwrap();
                // Stagger mtimes so "oldest" is well defined rather than
                // whatever the filesystem happened to record.
                let when = std::time::SystemTime::UNIX_EPOCH
                    + std::time::Duration::from_secs(1_700_000_000 + project * 100 + run * 10);
                filetime_set(&f, when);
                newest = Some((when, f));
            }
        }

        // Budget for four files; six of the ten must go.
        let reclaimed = prune_logs_total(&root, 4 * 1024).unwrap();
        assert_eq!(reclaimed, 6 * 1024, "wrong number of bytes reclaimed");

        let left: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .flat_map(|p| std::fs::read_dir(p.path()).unwrap().filter_map(Result::ok))
            .map(|e| e.path())
            .collect();
        assert_eq!(left.len(), 4);

        let (_, newest) = newest.unwrap();
        assert!(left.contains(&newest), "the newest run was deleted");
    }

    /// A tree already inside the budget must not lose anything.
    #[test]
    fn a_small_tree_is_left_alone() {
        let root = temp_path("total-small");
        let dir = root.join("only-project");
        std::fs::create_dir_all(&dir).unwrap();
        let _guard = Scratch(root.clone());
        std::fs::write(dir.join("a.log"), vec![b'x'; 512]).unwrap();

        assert_eq!(prune_logs_total(&root, 1024).unwrap(), 0);
        assert!(dir.join("a.log").exists());
    }

    /// Sets a file's mtime, so ordering in the test is ours rather than the
    /// filesystem's clock resolution.
    fn filetime_set(path: &Path, when: std::time::SystemTime) {
        let f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        f.set_modified(when).unwrap();
    }

    #[test]
    fn prune_ignores_non_log_files() {
        let dir = temp_path("prune-mixed");
        std::fs::create_dir_all(&dir).unwrap();
        let _g = Scratch(dir.clone());
        std::fs::write(dir.join("keep.json"), "{}").unwrap();
        for i in 0..4 {
            std::fs::write(dir.join(format!("r{i}.log")), "x").unwrap();
            std::thread::sleep(std::time::Duration::from_millis(12));
        }
        prune_logs(&dir, 1).unwrap();
        assert!(dir.join("keep.json").exists(), "pruning touched a non-log file");
    }
}
