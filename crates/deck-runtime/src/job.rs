//! Windows Job Objects: whole-tree process control.
//!
//! This module exists because of the single most common bug in process managers
//! on Windows. `npm run dev` is not one process -- it is `npm`, which starts
//! `node`, which starts Vite, which starts esbuild workers. Killing the pid you
//! spawned leaves every descendant running, still holding the port, invisible to
//! the user, and orphaned until they find it in Task Manager.
//!
//! A Job Object solves it properly. The child is assigned to a job at spawn
//! time; every process it later creates is automatically in that job too, and
//! [`JobObject::terminate`] ends all of them atomically. The job is created with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, so even if Launch Deck is killed
//! outright, Windows tears down the tree when the handle closes.
//!
//! # Why the child is spawned suspended
//!
//! There is a window between `CreateProcess` returning and the assignment call
//! landing. A child that spawns a grandchild inside that window escapes the job
//! permanently. In practice it takes milliseconds for a launcher process to get
//! going and the assignment happens in microseconds, so the race almost never
//! bites -- but "almost never" produces exactly the orphaned-process bug this
//! module exists to prevent. Spawning suspended and resuming after assignment
//! closes it completely.
//!
//! # On graceful shutdown
//!
//! Windows has no `SIGTERM`. `GenerateConsoleCtrlEvent` is the nearest thing and
//! requires the caller to share a console with the target, which a windowed
//! Tauri app does not have. So the graceful path here is to close the child's
//! stdin -- which a great many CLI tools treat as "shut down" -- wait out a grace
//! period, and then terminate the job. That is an honest limitation of the
//! platform rather than a shortcut: see [`crate::supervisor`] for how the two
//! stages are sequenced.

use std::io;
use std::os::windows::io::RawHandle;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject, TerminateJobObject,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_BREAKAWAY_OK,
    JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JobObjectExtendedLimitInformation,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

/// Creation flag: start the process with its main thread suspended.
pub const CREATE_SUSPENDED: u32 = 0x0000_0004;

/// Creation flag: give the child its own process group.
///
/// Keeps a Ctrl-C in any console Launch Deck may have from propagating into
/// supervised children, so stopping the app does not scatter interrupts.
pub const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

/// Creation flag: do not allocate a console window for the child.
///
/// Without this, every launched CLI project flashes a console window on screen.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The creation flags every supervised child is spawned with.
pub const SPAWN_FLAGS: u32 = CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW;

// Both Win32 structures are on the order of a hundred bytes, so these casts are
// provably lossless and are evaluated at compile time. Computing them as consts
// rather than with a runtime `expect` keeps both public functions panic-free.
#[allow(clippy::cast_possible_truncation)]
const EXTENDED_LIMIT_INFO_SIZE: u32 =
    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32;
#[allow(clippy::cast_possible_truncation)]
const THREAD_ENTRY_SIZE: u32 = std::mem::size_of::<THREADENTRY32>() as u32;

/// Closes a `ToolHelp` snapshot handle on every exit path.
struct SnapshotGuard(HANDLE);

impl Drop for SnapshotGuard {
    fn drop(&mut self) {
        // SAFETY: valid handle from `CreateToolhelp32Snapshot`, closed once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// An owned Windows Job Object holding one project's process tree.
///
/// Dropping this terminates every process still in the job, by virtue of
/// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. That is the desired behaviour and the
/// reason the type has no `leak` escape hatch: a supervisor that loses track of
/// a job should not thereby leak a dev server.
#[derive(Debug)]
pub struct JobObject {
    handle: HANDLE,
}

// SAFETY: a job object handle is a kernel handle with no thread affinity. Every
// Win32 call made through it here is documented as thread-safe, and the handle is
// only closed once, in `Drop`.
unsafe impl Send for JobObject {}
unsafe impl Sync for JobObject {}

impl JobObject {
    /// Creates an anonymous job configured to kill its members when closed.
    ///
    /// # Errors
    ///
    /// Returns an [`io::Error`] if the job cannot be created or configured.
    pub fn new() -> io::Result<Self> {
        // SAFETY: both arguments are optional and passed as null/None, which
        // `CreateJobObjectW` documents as "anonymous job, default security".
        let handle = unsafe { CreateJobObjectW(None, None) }
            .map_err(|e| io::Error::other(format!("CreateJobObject failed: {e}")))?;

        let job = Self { handle };

        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags =
            // The whole point: closing the handle takes the tree with it, so a
            // crash of Launch Deck cannot leave orphans behind.
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            // A child that deliberately asks to escape (an installer relaunching
            // itself elevated, say) is allowed to. Without this such a process
            // fails to start at all rather than detaching.
            | JOB_OBJECT_LIMIT_BREAKAWAY_OK
            // Suppress the Windows Error Reporting dialog for a crashing child;
            // the crash belongs in our log, not in a modal the user must dismiss.
            | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;

        // SAFETY: `info` is a correctly-sized, fully-initialised structure of the
        // type named by `JobObjectExtendedLimitInformation`, and the length passed
        // matches it exactly. `job.handle` is valid for the duration of the call.
        unsafe {
            SetInformationJobObject(
                job.handle,
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(&info).cast(),
                EXTENDED_LIMIT_INFO_SIZE,
            )
        }
        .map_err(|e| io::Error::other(format!("SetInformationJobObject failed: {e}")))?;

        Ok(job)
    }

    /// Adds a process to the job.
    ///
    /// Must be called before the process is resumed. Descendants created after
    /// this point join the job automatically.
    ///
    /// # Errors
    ///
    /// Returns an [`io::Error`] if the assignment is refused -- most plausibly
    /// because the process already belongs to another job that forbids nesting.
    pub fn assign(&self, process: RawHandle) -> io::Result<()> {
        // SAFETY: `process` is the raw handle of a live `Child` owned by the
        // caller, so it outlives this call. `self.handle` is valid by construction.
        unsafe { AssignProcessToJobObject(self.handle, HANDLE(process.cast())) }
            .map_err(|e| io::Error::other(format!("AssignProcessToJobObject failed: {e}")))
    }

    /// Terminates every process in the job.
    ///
    /// Idempotent: terminating an already-empty job succeeds.
    ///
    /// # Errors
    ///
    /// Returns an [`io::Error`] if the call is refused by the OS.
    pub fn terminate(&self, exit_code: u32) -> io::Result<()> {
        // SAFETY: `self.handle` is a valid job handle for the lifetime of `self`.
        unsafe { TerminateJobObject(self.handle, exit_code) }
            .map_err(|e| io::Error::other(format!("TerminateJobObject failed: {e}")))
    }
}

impl Drop for JobObject {
    fn drop(&mut self) {
        // SAFETY: closed exactly once, here, and `self.handle` came from
        // `CreateJobObjectW`. The close is what triggers KILL_ON_JOB_CLOSE.
        let _ = unsafe { CloseHandle(self.handle) };
    }
}

/// Resumes a suspended child, by handle where possible and by pid otherwise.
///
/// **This is the fast path, and the reason it exists is measured.** The
/// fallback below enumerates every thread on the machine to find the child's
/// one suspended thread -- 33.3 ms mean over 5,923 threads on this desktop,
/// which was roughly three quarters of the entire launch cost. `NtResumeProcess`
/// takes the process handle we already hold and resumes its threads directly,
/// touching nothing else on the system.
///
/// # Why an undocumented API is the right call here
///
/// `NtResumeProcess` is not in the Win32 headers. It is, however, the routine
/// `kernel32` itself has always been built on, it has been present and
/// unchanged since NT 3.1, and it is what every process explorer on Windows
/// uses. The documented alternative is not "a slower supported call" -- it is
/// `CreateProcessW` with `PROC_THREAD_ATTRIBUTE_JOB_LIST`, which means giving
/// up `tokio::process` and hand-rolling the three stdio pipes, the async
/// readers and the exit-status plumbing that the log pipeline depends on.
///
/// So the risk is bounded instead of taken: the symbol is resolved at runtime,
/// and if it is ever missing the `ToolHelp` walk still runs. A launch is never
/// lost to this optimisation -- at worst it costs what it used to.
///
/// # Errors
///
/// Returns an [`io::Error`] only when **both** paths fail, which would leave the
/// child suspended forever, so the caller must treat it as a failed spawn.
pub fn resume_child(handle: RawHandle, pid: u32) -> io::Result<()> {
    if ntdll::resume_process_by_handle(handle) {
        return Ok(());
    }
    // Either the symbol was missing or the call was refused. Neither is
    // expected; both are survivable, and the slow path is known to work.
    tracing::debug!(pid, "NtResumeProcess unavailable, falling back to thread walk");
    resume_process(pid)
}

/// The runtime-resolved `NtResumeProcess` entry point.
mod ntdll {
    use std::os::windows::io::RawHandle;
    use std::sync::OnceLock;

    use windows::core::{s, PCSTR};
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

    /// `NTSTATUS NtResumeProcess(HANDLE ProcessHandle)`.
    type NtResumeProcess = unsafe extern "system" fn(HANDLE) -> i32;

    /// Resolved once. `ntdll` is mapped into every Windows process before any
    /// user code runs, so this cannot fail for the usual reason a `LoadLibrary`
    /// would -- and `GetModuleHandleA` deliberately does not take a reference,
    /// because unloading `ntdll` is not a thing that happens.
    static ENTRY: OnceLock<Option<NtResumeProcess>> = OnceLock::new();

    fn entry() -> Option<NtResumeProcess> {
        *ENTRY.get_or_init(|| {
            // SAFETY: both calls take static, NUL-terminated literals, and the
            // returned address is transmuted to the documented signature of
            // `NtResumeProcess`, which has been stable since NT 3.1.
            unsafe {
                let module = GetModuleHandleA(s!("ntdll.dll")).ok()?;
                let proc: PCSTR = s!("NtResumeProcess");
                let address = GetProcAddress(module, proc)?;
                Some(std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    super::ntdll::NtResumeProcess,
                >(address))
            }
        })
    }

    /// Whether the fast path resolved. Used by a test to prove the
    /// optimisation is live rather than silently falling back forever.
    ///
    /// Test-only: production code never asks, because the answer only changes
    /// which of two working paths runs.
    #[cfg(test)]
    pub fn available() -> bool {
        entry().is_some()
    }

    /// Resumes every thread in the process. Returns false if the routine is
    /// unavailable or the call was refused, so the caller can fall back.
    pub fn resume_process_by_handle(handle: RawHandle) -> bool {
        let Some(resume) = entry() else { return false };
        // SAFETY: `handle` is the live process handle `CreateProcess` returned,
        // which carries PROCESS_SUSPEND_RESUME. The call does not take
        // ownership and we do not close the handle here -- `Child` owns it.
        let status = unsafe { resume(HANDLE(handle.cast())) };
        // NTSTATUS: negative is an error. STATUS_SUCCESS is 0; a process whose
        // threads were already running returns a non-negative informational
        // status, which is still success for our purposes.
        status >= 0
    }
}

/// Resumes a process spawned with [`CREATE_SUSPENDED`] by walking every thread.
///
/// A suspended process has exactly one thread -- its main thread, created
/// suspended by `CreateProcess`. `std::process::Child` does not expose that
/// thread handle, so it is located by enumerating system threads and matching on
/// owner pid. Every thread found for the pid is resumed, which is correct: at
/// this point there is only the one, and resuming a non-suspended thread is a
/// no-op that decrements a count already at zero and returns an error we ignore.
///
/// **Kept as the fallback for [`resume_child`], not as the normal path.** It
/// costs a full-machine thread snapshot; see that function for the measurement.
///
/// # Errors
///
/// Returns an [`io::Error`] if the thread snapshot cannot be taken, or if no
/// thread for `pid` could be resumed -- which would leave the child suspended
/// forever, so the caller must treat it as a failed spawn.
pub fn resume_process(pid: u32) -> io::Result<()> {
    // SAFETY: TH32CS_SNAPTHREAD with pid 0 snapshots all threads, which is the
    // documented usage; the returned handle is closed before every return path.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }
        .map_err(|e| io::Error::other(format!("CreateToolhelp32Snapshot failed: {e}")))?;

    let _guard = SnapshotGuard(snapshot);

    let mut entry = THREADENTRY32 {
        dwSize: THREAD_ENTRY_SIZE,
        ..Default::default()
    };

    // SAFETY: `entry.dwSize` is set as the API requires, and `entry` is a valid
    // mutable pointer for the duration of the call.
    if unsafe { Thread32First(snapshot, &raw mut entry) }.is_err() {
        return Err(io::Error::other("Thread32First returned no threads"));
    }

    let mut resumed = false;
    loop {
        if entry.th32OwnerProcessID == pid {
            // SAFETY: opening a thread by id with only SUSPEND_RESUME rights.
            // A failure is returned as an error rather than dereferenced.
            if let Ok(thread) = unsafe { OpenThread(THREAD_SUSPEND_RESUME, false, entry.th32ThreadID) }
            {
                // SAFETY: `thread` is a valid handle with SUSPEND_RESUME access.
                // u32::MAX signals failure and is ignored deliberately -- see the
                // doc comment on resuming an already-running thread.
                let previous = unsafe { ResumeThread(thread) };
                // SAFETY: handle from OpenThread, closed exactly once.
                let _ = unsafe { CloseHandle(thread) };
                if previous != u32::MAX {
                    resumed = true;
                }
            }
        }
        // SAFETY: same contract as Thread32First; iteration ends when it fails.
        if unsafe { Thread32Next(snapshot, &raw mut entry) }.is_err() {
            break;
        }
    }

    if resumed {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "no resumable thread found for pid {pid}; process would stay suspended"
        )))
    }
}

#[cfg(test)]
mod snapshot_cost {
    //! Measures what the suspended-spawn dance actually costs.
    //!
    //! `resume_main_thread` finds the child's main thread by taking a `ToolHelp`
    //! snapshot of EVERY thread on the machine -- several thousand of them on a
    //! normal desktop -- because `std::process::Child` does not expose the
    //! thread handle that `CreateProcessW` already returned.
    //!
    //! This test exists so the cost is a recorded number rather than a hunch,
    //! and so a future change to the spawn path can be judged against it. It
    //! asserts nothing about speed (that would be flaky on a loaded machine);
    //! it prints, and fails only if the mechanism breaks entirely.

    /// The fast path must actually start the child, not merely return `Ok`.
    ///
    /// This is the assertion that matters: a resume which reports success but
    /// leaves the process suspended is indistinguishable from a working one
    /// until a user watches a project hang forever.
    ///
    /// Proof is a distinctive exit code rather than a file the child writes.
    /// The file version failed on `cmd`'s redirect quoting and reported exit 1
    /// -- which proved the child ran, but by breaking the wrong half of the
    /// test. An exit code needs no shell parsing, no filesystem, and no path
    /// with spaces in it.
    #[test]
    fn handle_resume_actually_starts_the_child() {
        use std::os::windows::io::AsRawHandle;
        use std::os::windows::process::CommandExt;
        use std::process::Command;

        let job = super::JobObject::new().expect("job object");
        let child = Command::new("cmd.exe")
            .args(["/c", "exit", "7"])
            .creation_flags(super::SPAWN_FLAGS)
            .spawn()
            .expect("spawn suspended");

        job.assign(child.as_raw_handle()).expect("assign to job");
        super::resume_child(child.as_raw_handle(), child.id()).expect("resume via handle");

        let mut child = child;
        // Reaching this line at all is half the proof: a process that was never
        // resumed never exits, so `wait` would block until the test timed out.
        let status = child.wait().expect("child should exit once resumed");
        assert_eq!(
            status.code(),
            Some(7),
            "resume reported success but the child did not run its command"
        );
    }

    /// The fast path must be the one that actually runs on this machine.
    ///
    /// Without this, a silently-failing symbol lookup would fall back to the
    /// slow walk on every launch and every other test would still pass -- the
    /// optimisation would be dead code that nobody noticed.
    #[test]
    fn the_fast_path_is_the_one_in_use() {
        assert!(
            super::ntdll::available(),
            "NtResumeProcess did not resolve, so every launch is paying the              33 ms thread-snapshot cost this optimisation exists to remove"
        );
    }

    #[test]
    fn handle_resume_beats_the_thread_walk() {
        use std::os::windows::io::AsRawHandle;
        use std::os::windows::process::CommandExt;
        use std::process::Command;
        use std::time::Instant;

        fn spawn_suspended() -> std::process::Child {
            Command::new("cmd.exe")
                .args(["/c", "exit"])
                .creation_flags(super::SPAWN_FLAGS)
                .spawn()
                .expect("spawn suspended")
        }

        let mut fast = Vec::new();
        let mut slow = Vec::new();
        for _ in 0..5 {
            let child = spawn_suspended();
            let t = Instant::now();
            super::resume_child(child.as_raw_handle(), child.id()).expect("fast resume");
            fast.push(t.elapsed());
            let _ = child.wait_with_output();

            let child = spawn_suspended();
            let t = Instant::now();
            super::resume_process(child.id()).expect("slow resume");
            slow.push(t.elapsed());
            let _ = child.wait_with_output();
        }

        let mean = |v: &Vec<std::time::Duration>| {
            v.iter().sum::<std::time::Duration>() / u32::try_from(v.len()).unwrap_or(1)
        };
        let f = mean(&fast);
        let sl = mean(&slow);
        println!(
            "resume: handle {:.2} ms vs thread walk {:.2} ms ({:.0}x)",
            f.as_secs_f64() * 1000.0,
            sl.as_secs_f64() * 1000.0,
            sl.as_secs_f64() / f.as_secs_f64().max(1e-9)
        );
        // Deliberately a weak bound. The point is to catch a regression that
        // makes the fast path no faster, not to assert a ratio that a loaded
        // CI machine could miss.
        assert!(
            f < sl,
            "handle resume ({f:?}) was not faster than the thread walk ({sl:?})"
        );
    }

    #[test]
    fn thread_snapshot_cost_is_recorded() {
        use std::time::Instant;

        let mut samples = Vec::new();
        for _ in 0..5 {
            let t = Instant::now();
            let count = super::tests::count_all_threads();
            samples.push((t.elapsed(), count));
        }
        let total: std::time::Duration = samples.iter().map(|(d, _)| *d).sum();
        let mean = total / 5;
        let threads = samples.first().map_or(0, |(_, c)| *c);
        println!(
            "ToolHelp thread snapshot: {:.1} ms mean over {} threads",
            mean.as_secs_f64() * 1000.0,
            threads
        );
        assert!(threads > 0, "snapshot returned no threads at all");
    }
}

#[cfg(test)]
mod tests {
    /// Counts every thread the `ToolHelp` snapshot returns. Mirrors exactly what
    /// `resume_main_thread` walks on every single launch.
    #[allow(unsafe_code)]
    pub(super) fn count_all_threads() -> usize {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD,
            THREADENTRY32,
        };

        // SAFETY: standard ToolHelp usage; the handle is closed below.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        let Ok(snapshot) = snapshot else { return 0 };
        let mut entry = THREADENTRY32 {
            dwSize: u32::try_from(std::mem::size_of::<THREADENTRY32>()).unwrap_or(0),
            ..Default::default()
        };
        let mut n = 0;
        // SAFETY: `entry.dwSize` is set as the API requires; iteration ends on error.
        if unsafe { Thread32First(snapshot, &raw mut entry) }.is_ok() {
            n += 1;
            // SAFETY: same contract as Thread32First.
            while unsafe { Thread32Next(snapshot, &raw mut entry) }.is_ok() {
                n += 1;
            }
        }
        // SAFETY: handle came from CreateToolhelp32Snapshot and is closed once.
        unsafe { let _ = CloseHandle(snapshot); }
        n
    }

    use super::*;
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    use std::time::{Duration, Instant};

    /// Spawns a long-lived child the way the supervisor does: suspended, in a
    /// job, then resumed.
    fn spawn_supervised(job: &JobObject, args: &[&str]) -> std::process::Child {
        let child = Command::new("cmd.exe")
            .args(args)
            .creation_flags(SPAWN_FLAGS)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("cmd.exe should spawn");
        job.assign(child.as_raw_handle()).expect("assign to job");
        resume_process(child.id()).expect("resume main thread");
        // Give the child a moment to actually begin executing.
        std::thread::sleep(Duration::from_millis(150));
        child
    }

    fn is_alive(pid: u32) -> bool {
        // `tasklist` is the least intrusive way to ask from a test.
        let out = Command::new("tasklist.exe")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .expect("tasklist should run");
        String::from_utf8_lossy(&out.stdout).contains(&pid.to_string())
    }

    #[test]
    fn creates_and_drops_a_job_cleanly() {
        let job = JobObject::new().expect("job creation should succeed");
        // Terminating an empty job is a no-op, not an error.
        job.terminate(0).expect("terminate empty job");
        drop(job);
    }

    #[test]
    fn assigns_and_terminates_a_single_process() {
        let job = JobObject::new().unwrap();
        let mut child = spawn_supervised(&job, &["/c", "ping -n 30 127.0.0.1 >nul"]);
        let pid = child.id();
        assert!(is_alive(pid), "child should be running after resume");

        job.terminate(1).expect("terminate job");

        let deadline = Instant::now() + Duration::from_secs(5);
        while is_alive(pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!is_alive(pid), "child survived TerminateJobObject");
        let _ = child.wait();
    }

    #[test]
    fn suspended_child_does_not_run_before_being_resumed() {
        // The property that closes the assignment race: nothing executes until
        // after the process is in the job.
        let job = JobObject::new().unwrap();
        let mut child = Command::new("cmd.exe")
            .args(["/c", "ping -n 30 127.0.0.1 >nul"])
            .creation_flags(SPAWN_FLAGS)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let pid = child.id();
        job.assign(child.as_raw_handle()).unwrap();

        // Still suspended: it exists but has produced nothing and cannot have
        // spawned the grandchild `ping` yet.
        assert!(is_alive(pid));
        assert!(
            child.try_wait().unwrap().is_none(),
            "suspended process must not have exited"
        );

        resume_process(pid).unwrap();
        job.terminate(1).unwrap();
        let _ = child.wait();
    }

    #[test]
    fn terminates_the_whole_tree_not_just_the_root() {
        // The bug this module exists to prevent: `cmd` spawns `ping` as a child,
        // and killing only `cmd` would leave `ping` orphaned.
        let job = JobObject::new().unwrap();
        let mut child = spawn_supervised(&job, &["/c", "ping -n 60 127.0.0.1 >nul"]);
        let root_pid = child.id();

        // Find the grandchild: a ping.exe whose parent is our cmd.
        let out = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "(Get-CimInstance Win32_Process -Filter \
                     \"ParentProcessId={root_pid}\").ProcessId"
                ),
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .expect("powershell should run");
        let grandchild: Option<u32> = String::from_utf8_lossy(&out.stdout)
            .lines().find_map(|l| l.trim().parse::<u32>().ok());

        let grandchild = grandchild.expect("cmd should have spawned a ping grandchild");
        assert!(is_alive(grandchild), "grandchild should be running");

        job.terminate(1).expect("terminate job");

        let deadline = Instant::now() + Duration::from_secs(5);
        while (is_alive(root_pid) || is_alive(grandchild)) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!is_alive(root_pid), "root survived");
        assert!(
            !is_alive(grandchild),
            "grandchild survived -- the job did not cover the tree"
        );
        let _ = child.wait();
    }

    #[test]
    fn dropping_the_job_kills_its_processes() {
        // KILL_ON_JOB_CLOSE: this is what protects against Launch Deck itself
        // dying and leaving dev servers behind.
        let job = JobObject::new().unwrap();
        let mut child = spawn_supervised(&job, &["/c", "ping -n 60 127.0.0.1 >nul"]);
        let pid = child.id();
        assert!(is_alive(pid));

        drop(job);

        let deadline = Instant::now() + Duration::from_secs(5);
        while is_alive(pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!is_alive(pid), "closing the job handle must kill its members");
        let _ = child.wait();
    }

    #[test]
    fn resuming_an_unknown_pid_is_an_error_not_a_hang() {
        // pid 0 is the idle process and owns no resumable user thread.
        assert!(resume_process(0xFFFF_FFF0).is_err());
    }
}
