//! The system tray, and the reason closing the window no longer kills anything.
//!
//! # The bug this module exists to fix
//!
//! Launch Deck had no `CloseRequested` handler. Tauri exits when the last window
//! closes, `RunEvent::Exit` calls [`Supervisor::shutdown`], and every supervised
//! tree is terminated -- and even without that, the job objects carry
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, so the handles closing would have done
//! it anyway.
//!
//! The observable result was that **clicking the X killed every project you had
//! launched.** Reproduced deliberately: a fixture running as pid 9936 was gone
//! three seconds after the window closed. From the user's seat that is not a
//! window closing, it is Launch Deck crashing their apps.
//!
//! # Why the fix is a tray icon rather than "don't kill on exit"
//!
//! Two guarantees are in tension and both are worth keeping:
//!
//! - **Nothing outlives the supervisor.** `npm run dev` is npm → node → Vite →
//!   esbuild; a launcher that leaks those on exit leaves ports held by processes
//!   the user cannot find. That is the bug the job objects exist to prevent, and
//!   relaxing `KILL_ON_JOB_CLOSE` would reintroduce it.
//! - **Closing a window is not a request to stop working.** Nobody expects
//!   closing Docker Desktop's window to stop their containers.
//!
//! Keeping both means the app must not *exit* when the window closes. So the
//! window hides, the process stays, the jobs stay, and quitting becomes an
//! explicit act that says how much it is about to stop.
//!
//! A hidden window with no tray icon is strictly worse than the bug -- the app
//! becomes unreachable and looks like it crashed. The tray icon is not a nicety
//! here; it is what makes hiding safe.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime};

use crate::AppState;

/// Menu item ids. Matched as strings by Tauri, so they live in one place.
const SHOW: &str = "show";
const STOP_ALL: &str = "stop-all";
const QUIT: &str = "quit";

/// Brings the main window back from hidden, minimized, or merely unfocused.
///
/// All three calls are needed and in this order. `set_focus` alone does nothing
/// to a hidden window, and `show` alone leaves a minimized one in the taskbar --
/// which reads as "clicking the tray icon did nothing".
pub fn reveal<R: Runtime>(app: &AppHandle<R>) {
    // Marshalled to the main thread. Win32 window operations must run on the
    // thread that owns the window, and both callers here arrive on other
    // threads: the single-instance plugin dispatches on its own listener
    // thread, and the tray menu on a message-pump thread.
    //
    // Calling show() off-thread appears to work exactly once and then silently
    // stops -- measured 4 failures in 5 hide/reveal cycles with a realistic
    // three-second gap. That is the worst kind of bug for this feature: the
    // window hides to the tray and never comes back, which reads as the app
    // having crashed.
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(window) = handle.get_webview_window("main") {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
    });
}

/// How many projects are live right now, for the tooltip and the quit warning.
fn running_count<R: Runtime>(app: &AppHandle<R>) -> usize {
    app.try_state::<AppState>()
        .map_or(0, |state| state.supervisor.running_count())
}

/// Updates the tray tooltip so a hidden app still says what it is doing.
///
/// Without this, hiding to the tray makes running projects invisible: the
/// window is gone and the icon is a static logo. The tooltip is the only
/// surface left, so it carries the count.
pub fn refresh_tooltip<R: Runtime>(app: &AppHandle<R>) {
    let Some(tray) = app.tray_by_id("main") else {
        return;
    };
    let count = running_count(app);
    let tip = match count {
        0 => "Launch Deck — nothing running".to_owned(),
        1 => "Launch Deck — 1 project running".to_owned(),
        n => format!("Launch Deck — {n} projects running"),
    };
    let _ = tray.set_tooltip(Some(&tip));
}

/// Builds the tray icon and its menu.
///
/// # Errors
///
/// Returns an error if the menu or icon cannot be created, which the caller
/// treats as non-fatal: an app with no tray is degraded, but an app that
/// refuses to start because of a tray is worse.
pub fn install<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, SHOW, "Show Launch Deck", true, None::<&str>)?;
    let stop_all = MenuItem::with_id(app, STOP_ALL, "Stop all projects", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, QUIT, "Quit Launch Deck", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;

    let menu = Menu::with_items(app, &[&show, &separator, &stop_all, &quit])?;

    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().cloned().ok_or_else(|| {
            tauri::Error::AssetNotFound("default window icon missing".into())
        })?)
        .tooltip("Launch Deck")
        .menu(&menu)
        // The menu must NOT open on a left click: left click is "show me the
        // window", which is what a tray icon is for 99% of the time.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            SHOW => reveal(app),
            STOP_ALL => {
                if let Some(state) = app.try_state::<AppState>() {
                    state.supervisor.shutdown();
                }
                refresh_tooltip(app);
            }
            QUIT => {
                // The real exit. `RunEvent::Exit` still runs, so the supervisor
                // shuts down and the database checkpoints -- quitting is
                // allowed to stop everything, because the user just said so.
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // Only a completed left click. `Down` fires on press and would
            // raise the window before the user had finished deciding.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                reveal(tray.app_handle());
            }
        })
        .build(app)?;

    refresh_tooltip(app);
    Ok(())
}
