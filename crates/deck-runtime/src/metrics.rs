//! Resource sampling for a supervised process tree.
//!
//! # Why the whole tree, not the pid we spawned
//!
//! `npm run dev` is a launcher: it starts `node`, which starts Vite, which
//! starts esbuild workers. The pid Launch Deck holds sits at ~0% CPU and a few
//! megabytes forever, while its descendants do all the work. Reporting the root
//! alone would be accurate and useless -- a bundler saturating four cores would
//! display as idle.
//!
//! So every sample walks the descendants of the root pid and sums them. CPU is
//! reported as a percentage of a single core, which means a four-core bundle
//! reads as ~400%. That is the truth and worth showing; clamping it to 100 would
//! hide the most interesting thing on the screen.
//!
//! # Why samples are not persisted
//!
//! At 1 Hz across a handful of running projects this produces thousands of rows
//! an hour, none of which anyone queries. Samples live in a bounded in-memory
//! ring per project and are dropped when the run ends. The database stores run
//! outcomes, not telemetry.

use std::collections::{HashMap, HashSet, VecDeque};

use deck_domain::project::ProjectId;
use deck_domain::runtime::{ProcessSnapshot, RunId};
use parking_lot::Mutex;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

/// How many samples to retain per project.
///
/// 120 at 1 Hz is two minutes of history -- enough for a sparkline to show a
/// build spike and settle, without holding telemetry nobody reads.
pub const HISTORY: usize = 120;

/// The sampling interval.
pub const INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

/// A tree walker and sampler over one shared [`System`] snapshot.
///
/// One instance serves every running project: refreshing `sysinfo` is the
/// expensive part, so it happens once per tick rather than once per project.
pub struct Sampler {
    system: Mutex<System>,
    history: Mutex<HashMap<ProjectId, VecDeque<ProcessSnapshot>>>,
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Sampler {
    /// Creates a sampler with an empty history.
    #[must_use]
    pub fn new() -> Self {
        Self {
            system: Mutex::new(System::new()),
            history: Mutex::new(HashMap::new()),
        }
    }

    /// Refreshes the process table once, ready for a round of [`Self::sample`].
    ///
    /// CPU percentages are deltas between refreshes, so this must be called on a
    /// steady interval; a single refresh in isolation reports 0% for everything.
    pub fn refresh(&self) {
        self.system.lock().refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory().with_disk_usage(),
        );
    }

    /// This process's own footprint: (memory bytes, threads, cpu percent).
    ///
    /// Diagnostics reports what Launch Deck itself costs, because "is the app
    /// misbehaving" is often really "is the app leaking". Uses the same sampler
    /// the project trees use, so the numbers are directly comparable.
    #[must_use]
    pub fn self_usage(&self) -> Option<(u64, usize, f32)> {
        // `tree_pids` takes the same lock, and `parking_lot::Mutex` is NOT
        // reentrant -- calling it while holding the guard deadlocks the process
        // outright. Gather the tree FIRST, then take the lock for the rest.
        let pids = self.tree_pids(std::process::id());

        let me = sysinfo::Pid::from_u32(std::process::id());
        let system = self.system.lock();

        // Sum the whole tree, not just this pid: WebView2 runs the UI in
        // separate processes, so the parent alone understates the real cost by
        // an order of magnitude.
        let mut memory = 0;
        let mut cpu = 0.0;
        for pid in &pids {
            if let Some(p) = system.process(sysinfo::Pid::from_u32(*pid)) {
                memory += p.memory();
                cpu += p.cpu_usage();
            }
        }
        if memory == 0 {
            let p = system.process(me)?;
            return Some((p.memory(), 1, p.cpu_usage()));
        }
        Some((memory, pids.len(), cpu))
    }

    /// Every pid in the tree rooted at `root`, including `root` itself.
    ///
    /// Returns an empty set if the root is gone, which is how a caller learns
    /// the tree has exited.
    #[must_use]
    pub fn tree_pids(&self, root: u32) -> HashSet<u32> {
        let system = self.system.lock();
        let root_pid = Pid::from_u32(root);
        if system.process(root_pid).is_none() {
            return HashSet::new();
        }

        // Index children by parent so the walk is linear rather than quadratic
        // in the number of processes on the machine.
        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
        for (pid, process) in system.processes() {
            if let Some(parent) = process.parent() {
                children.entry(parent.as_u32()).or_default().push(pid.as_u32());
            }
        }

        let mut found = HashSet::from([root]);
        let mut queue = vec![root];
        while let Some(pid) = queue.pop() {
            for &child in children.get(&pid).into_iter().flatten() {
                // `insert` returning true guards against a parent cycle, which
                // pid reuse can genuinely produce.
                if found.insert(child) {
                    queue.push(child);
                }
            }
        }
        found
    }

    /// Samples the tree rooted at `root`, recording it in the project's history.
    ///
    /// Returns `None` when no process in the tree is alive, which the caller
    /// treats as "stop sampling this project".
    pub fn sample(
        &self,
        project_id: ProjectId,
        run_id: RunId,
        root: u32,
        started_at: chrono::DateTime<chrono::Utc>,
    ) -> Option<ProcessSnapshot> {
        let pids = self.tree_pids(root);
        if pids.is_empty() {
            return None;
        }

        let (cpu, memory, threads, read, written, count) = {
            let system = self.system.lock();
            let mut cpu = 0.0f32;
            let mut memory = 0u64;
            let mut threads = 0u32;
            let mut read = 0u64;
            let mut written = 0u64;
            let mut count = 0u32;

            for pid in &pids {
                if let Some(process) = system.process(Pid::from_u32(*pid)) {
                    cpu += process.cpu_usage();
                    memory += process.memory();
                    let disk = process.disk_usage();
                    read += disk.total_read_bytes;
                    written += disk.total_written_bytes;
                    // Not every platform reports a thread count; absence is 0
                    // rather than a guess.
                    threads += u32::try_from(process.tasks().map_or(0, std::collections::HashSet::len))
                        .unwrap_or(0);
                    count += 1;
                }
            }
            (cpu, memory, threads, read, written, count)
        };

        let now = chrono::Utc::now();
        let snapshot = ProcessSnapshot {
            project_id,
            run_id,
            pid: root,
            at: now,
            cpu_percent: cpu,
            memory_bytes: memory,
            disk_read_bytes: read,
            disk_write_bytes: written,
            process_count: count,
            thread_count: threads,
            listening_ports: crate::ports::ports_for_pids(&pids),
            uptime_secs: u64::try_from((now - started_at).num_seconds().max(0)).unwrap_or(0),
        };

        let mut history = self.history.lock();
        let ring = history.entry(project_id).or_default();
        if ring.len() >= HISTORY {
            ring.pop_front();
        }
        ring.push_back(snapshot.clone());

        Some(snapshot)
    }

    /// The retained samples for a project, oldest first.
    #[must_use]
    pub fn history(&self, project_id: ProjectId) -> Vec<ProcessSnapshot> {
        self.history
            .lock()
            .get(&project_id)
            .map(|ring| ring.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// The most recent sample for a project.
    #[must_use]
    pub fn latest(&self, project_id: ProjectId) -> Option<ProcessSnapshot> {
        self.history.lock().get(&project_id)?.back().cloned()
    }

    /// Drops a project's samples. Called when a run ends or a project is removed.
    pub fn forget(&self, project_id: ProjectId) {
        self.history.lock().remove(&project_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    fn spawn_tree() -> std::process::Child {
        // cmd spawns ping as a child, giving a two-level tree to walk.
        Command::new("cmd.exe")
            .args(["/c", "ping -n 30 127.0.0.1 >nul"])
            .creation_flags(crate::job::CREATE_NO_WINDOW)
            .spawn()
            .expect("spawn a test tree")
    }

    #[test]
    fn walks_a_real_process_tree() {
        let sampler = Sampler::new();
        let mut child = spawn_tree();
        std::thread::sleep(std::time::Duration::from_millis(600));
        sampler.refresh();

        let pids = sampler.tree_pids(child.id());
        assert!(pids.contains(&child.id()), "root missing from its own tree");
        assert!(
            pids.len() >= 2,
            "expected the ping grandchild in the tree, got {pids:?}"
        );

        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn a_dead_root_yields_an_empty_tree() {
        let sampler = Sampler::new();
        let mut child = spawn_tree();
        let pid = child.id();
        let _ = child.kill();
        let _ = child.wait();
        std::thread::sleep(std::time::Duration::from_millis(300));
        sampler.refresh();
        assert!(sampler.tree_pids(pid).is_empty());
    }

    #[test]
    fn sampling_aggregates_the_tree_and_records_history() {
        let sampler = Sampler::new();
        let mut child = spawn_tree();
        let project = ProjectId::new();
        let run = RunId::new();
        let started = chrono::Utc::now();

        std::thread::sleep(std::time::Duration::from_millis(600));
        sampler.refresh();
        let first = sampler
            .sample(project, run, child.id(), started)
            .expect("live tree should sample");

        // Memory is summed across the tree, so it must exceed a single process.
        assert!(first.memory_bytes > 0, "no memory reported");
        assert!(first.process_count >= 2, "tree not aggregated: {first:?}");
        assert_eq!(first.pid, child.id());

        // A second tick appends rather than replacing.
        std::thread::sleep(super::INTERVAL);
        sampler.refresh();
        sampler.sample(project, run, child.id(), started).unwrap();
        assert_eq!(sampler.history(project).len(), 2);
        assert_eq!(sampler.latest(project).map(|s| s.pid), Some(child.id()));

        sampler.forget(project);
        assert!(sampler.history(project).is_empty());

        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn sampling_a_dead_tree_returns_none() {
        let sampler = Sampler::new();
        let mut child = spawn_tree();
        let pid = child.id();
        let _ = child.kill();
        let _ = child.wait();
        std::thread::sleep(std::time::Duration::from_millis(300));
        sampler.refresh();
        assert!(sampler
            .sample(ProjectId::new(), RunId::new(), pid, chrono::Utc::now())
            .is_none());
    }

    #[test]
    fn history_is_bounded_to_the_retention_window() {
        let sampler = Sampler::new();
        let project = ProjectId::new();
        // Drive the ring directly: spawning 120 real samples would take minutes.
        {
            let mut history = sampler.history.lock();
            let ring = history.entry(project).or_default();
            for i in 0..HISTORY + 25 {
                if ring.len() >= HISTORY {
                    ring.pop_front();
                }
                ring.push_back(ProcessSnapshot {
                    project_id: project,
                    run_id: RunId::new(),
                    pid: 1,
                    at: chrono::Utc::now(),
                    #[allow(clippy::cast_precision_loss)]
                    cpu_percent: i as f32,
                    memory_bytes: 0,
                    disk_read_bytes: 0,
                    disk_write_bytes: 0,
                    process_count: 1,
                    thread_count: 0,
                    listening_ports: vec![],
                    uptime_secs: 0,
                });
            }
        }
        let history = sampler.history(project);
        assert_eq!(history.len(), HISTORY);
        // The oldest retained sample is the one just past the dropped range.
        assert!(
            (history[0].cpu_percent - 25.0).abs() < f32::EPSILON,
            "oldest retained sample was {}",
            history[0].cpu_percent
        );
    }

    #[test]
    fn uptime_counts_from_the_recorded_start() {
        let sampler = Sampler::new();
        let mut child = spawn_tree();
        std::thread::sleep(std::time::Duration::from_millis(400));
        sampler.refresh();
        let started = chrono::Utc::now() - chrono::Duration::seconds(42);
        let snapshot = sampler
            .sample(ProjectId::new(), RunId::new(), child.id(), started)
            .unwrap();
        assert!(
            (42..=45).contains(&snapshot.uptime_secs),
            "uptime was {}",
            snapshot.uptime_secs
        );
        let _ = child.kill();
        let _ = child.wait();
    }
}
