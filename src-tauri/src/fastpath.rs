//! Second-launch detection that does not depend on a visible window.
//!
//! # The bug this fixes
//!
//! `tauri_plugin_single_instance` stops a duplicate reliably while the main
//! window is showing. Once the window is HIDDEN -- which close-to-tray made the
//! normal state -- relaunching from the Start menu starts a whole second
//! instance instead of revealing the first. Measured: five processes alive and
//! still alive twenty seconds later, each with its own tray icon, all sharing
//! one SQLite file. That is the same multi-instance contention that cost a
//! 27-project registry once already.
//!
//! Close-to-tray created this path, so the fix belongs with it.
//!
//! # Why a named event rather than a better window search
//!
//! A kernel event has no window, no visibility state and no message pump to
//! miss. Whoever can *open* it knows an instance is live; the primary wakes and
//! reveals. Kernel objects are refcounted by handle, so a primary that was
//! killed leaves nothing behind and the next launch correctly becomes primary --
//! no stale lock file, no timeout to tune.
//!
//! The plugin stays registered. It is the correctness guarantee for the
//! database, and this runs in front of it as the case it does not cover.
//!
//! # The part that made the first attempt fail
//!
//! An earlier version of this module worked exactly once and then stopped, and
//! the cause was not here at all: [`crate::tray::reveal`] was calling `show()`
//! from whatever thread signalled it. Win32 window operations must run on the
//! thread owning the window. `reveal` now marshals; without that, this file is
//! useless.

use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Threading::{
    CreateEventW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE, INFINITE,
};
use windows::Win32::Foundation::{WAIT_FAILED, WAIT_OBJECT_0};

/// `Local\` scopes this to the logon session, so two users signed in at once
/// each get their own Launch Deck rather than fighting over one.
const EVENT_NAME: windows::core::PCWSTR = w!("Local\\LaunchDeck.Reveal.v1");

/// Signals a running instance, reporting whether one existed.
///
/// Called as the first statement of `run()`. `true` means the caller must
/// return immediately, having touched nothing -- no tracing, no database, no
/// Tauri.
#[must_use]
pub fn signal_existing_instance() -> bool {
    // SAFETY: opens a named object by static literal. Failure means no such
    // object, which is the ordinary case of being the first instance.
    unsafe {
        let Ok(handle) = OpenEventW(EVENT_MODIFY_STATE, false, EVENT_NAME) else {
            return false;
        };
        let signalled = SetEvent(handle).is_ok();
        let _ = CloseHandle(handle);
        signalled
    }
}

/// Creates the event and serves reveal requests for the life of the process.
///
/// Failure is non-fatal and quiet: without the event, second launches fall back
/// to the plugin, which is what shipped before. A degraded fast path must never
/// cost a launch.
pub fn serve_reveals<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    // SAFETY: creates a named event with default security. `manual_reset:
    // false` makes it auto-reset, so each signal releases the waiter exactly
    // once and nothing has to reset it.
    let handle: HANDLE = match unsafe { CreateEventW(None, false, false, EVENT_NAME) } {
        Ok(h) => h,
        Err(e) => {
            tracing::debug!(error = %e, "reveal event unavailable; falling back to the plugin");
            return;
        }
    };

    // `HANDLE` wraps a raw pointer so it is not `Send`, but a kernel handle is a
    // process-wide table index with no thread affinity. Moving it as an integer
    // states that reasoning in code rather than hiding it behind an
    // `unsafe impl Send` on someone else's type.
    let raw = handle.0 as usize;

    std::thread::spawn(move || {
        let handle = HANDLE(raw as *mut core::ffi::c_void);
        loop {
            // A dedicated OS thread, not a tokio task: this blocks on a kernel
            // object with INFINITE and would hold a runtime worker forever.
            //
            // SAFETY: the handle is owned by this thread for the process's life.
            let wait = unsafe { WaitForSingleObject(handle, INFINITE) };

            // Break ONLY on WAIT_FAILED. An earlier version bailed on any
            // non-zero result, which quietly disabled the fast path for the
            // rest of the process's life: the thread exited, its handle closed,
            // the last reference to the named event went with it, and every
            // later launch fell through to the plugin -- measured as two
            // consecutive reveals that did nothing at cycles five and six.
            //
            // Waiting on an auto-reset event with INFINITE can only yield
            // WAIT_OBJECT_0 (0) or WAIT_FAILED; WAIT_TIMEOUT is unreachable and
            // WAIT_ABANDONED applies to mutexes. Treating anything else as
            // fatal was defending against a case that cannot happen, at the
            // cost of the case that does.
            if wait == WAIT_FAILED {
                tracing::warn!("reveal watcher stopped; second launches fall back to the plugin");
                break;
            }
            if wait != WAIT_OBJECT_0 {
                continue;
            }
            // Marshals to the main thread internally -- see the module note.
            crate::tray::reveal(&app);
        }
        // SAFETY: closed exactly once, only when the loop gives up.
        unsafe {
            let _ = CloseHandle(handle);
        }
    });
}
