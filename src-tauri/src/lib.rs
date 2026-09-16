//! Launch Deck's Tauri shell: wiring, no logic.
//!
//! Everything interesting lives in the `deck-*` crates. This crate owns the
//! window, the managed [`AppState`], the command surface (`commands`), the DTO
//! layer (`dto`) and the event pumps (`pump`). If a function in here grows a
//! branch that isn't error mapping, it is in the wrong crate.

mod commands;
mod diagnostics;
mod dto;
mod error;
mod pump;
mod fastpath;
mod tray;
mod timings;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use deck_domain::project::ProjectId;
use parking_lot::Mutex;

use deck_runners::RunnerRegistry;
use deck_runtime::{Sampler, Supervisor};
use deck_domain::command::Lifecycle;
use deck_store::Store;
use tauri::{Emitter, Manager};

/// How many recently launched projects to prime at startup.
///
/// Small on purpose. This is a bet on what the user will do next, made before
/// they have done anything, so a wrong bet must cost almost nothing. Five
/// projects is a few megabytes of background reads -- measured at 7.6 MB here,
/// because they deduplicate to three distinct programs -- and it also warms the
/// PATH-resolution cache for them, which is the larger half of what priming
/// buys. Raising it trades certain I/O for a progressively less likely saving.
const RECENT_PREWARM: usize = 5;

/// Shared state behind every command.
pub struct AppState {
    pub store: Arc<Store>,
    pub registry: Arc<RunnerRegistry>,
    pub supervisor: Arc<Supervisor>,
    pub logs_root: PathBuf,
    /// The app data directory itself, for the things that live beside the
    /// database: saved web apps, user runner manifests.
    pub data_dir: PathBuf,
    pub pumps: pump::PumpRegistry,
    /// Live resource sampling for running process trees.
    pub sampler: Arc<Sampler>,
    /// Session cache of extracted project icons as data URLs.
    ///
    /// `None` records "looked, found nothing" so absent icons cost one
    /// filesystem probe per session, not one per render.
    pub icons: Mutex<HashMap<ProjectId, Option<String>>>,
    /// Measured startup and launch timings, surfaced in Diagnostics.
    pub timings: Arc<timings::Timings>,
}

/// The app data directory: `%APPDATA%\Launch Deck`.
///
/// Deliberately the product-name folder rather than the reverse-domain
/// identifier path -- it is what a Windows user expects to find in AppData, and
/// it is the path the runner-plugin docs promise (`...\Launch Deck\runners`).
fn data_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Launch Deck")
}

/// Builds and runs the application.
///
/// Ordering matters: the single-instance plugin is registered before anything
/// opens the database, and the store is created in `setup()` so only the
/// surviving instance ever touches it.
///
/// # Panics
///
/// Panics if the bundled runner manifests are invalid, the database cannot be
/// opened, or the webview cannot be created -- all unrecoverable, and all
/// before any window exists to report into.
pub fn run() {
    // FIRST STATEMENT. Before tracing, before Tauri, before the database.
    //
    // The single-instance plugin does not catch a relaunch while the window is
    // HIDDEN -- which close-to-tray made the normal state -- and five full
    // instances were measured sharing one SQLite file as a result. This closes
    // that with a named kernel event, which has no window state to miss.
    if fastpath::signal_existing_instance() {
        return;
    }

    // Started before anything else so the boot figure covers the whole process,
    // not just the part after logging is configured.
    let timings = Arc::new(timings::Timings::start());

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,launch_deck=debug".into()),
        )
        .init();

    let t_builder = std::time::Instant::now();
    // Cloned before the setup closure moves the original.
    let builder_timings = Arc::clone(&timings);
    let app = tauri::Builder::default()
        // FIRST, before anything touches the database. The single-instance
        // plugin decides during `build()`, and a losing instance exits there --
        // so everything that opens the database must happen after this point,
        // in `setup()`.
        //
        // Getting this wrong was a real bug: opening the store up-front meant a
        // second launch created its own connection pool, ran migrations and
        // could write a snapshot before the plugin got to exit it. Two processes
        // contending for one SQLite file surfaces as "the database did not
        // answer" on whichever loses.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // A second launch focuses the existing window instead of racing it
            // for the database.
            // `reveal`, not just focus: the window can now be HIDDEN in the
            // tray, and `set_focus` on a hidden window does nothing at all --
            // relaunching from the Start menu would appear to do nothing.
            tray::reveal(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::list_projects,
            commands::run_states,
            commands::run_history,
            commands::get_logs,
            commands::inspect_path,
            commands::scan_path,
            commands::register_project,
            commands::update_project,
            commands::remove_project,
            commands::start_project,
            commands::stop_project,
            commands::restart_project,
            commands::open_project_folder,
            commands::open_project_terminal,
            commands::open_in_editor,
            commands::export_logs,
            commands::project_metrics,
            commands::scan_roots,
            commands::rescan_roots,
            commands::redetect_project,
            commands::prewarm_project,
            commands::project_icon,
            commands::project_icons,
            commands::get_setting,
            commands::set_setting,
            commands::diagnostics,
            commands::surface_status,
            commands::context_card,
            commands::save_context_card,
            commands::error_tail,
            commands::hand_off,
            commands::open_claude_terminal,
            commands::overnight_report,
            commands::recent_runs,
            commands::open_surface_window,
            commands::project_usage,
            commands::folder_children,
            commands::project_manual,
            commands::open_path,
            commands::register_web_app,
            commands::open_surface_url,
            commands::setup_reports,
            commands::ui_ready,
        ])
        .setup(move |app| {
            let setup_started = std::time::Instant::now();

            // Only the surviving instance reaches here, so this is the earliest
            // safe place to open the database.
            let data = data_dir();

            let t_registry = std::time::Instant::now();
            let registry = RunnerRegistry::load(&data.join("runners")).unwrap_or_else(|e| {
                // Only a broken *bundled* manifest reaches here, which is a
                // build defect; refuse to start half-blind.
                panic!("bundled runner manifests failed to load: {e}");
            });
            for problem in registry.problems() {
                tracing::warn!(
                    path = %problem.path.display(),
                    reason = %problem.reason,
                    "user runner manifest skipped"
                );
            }

            let registry_ms = t_registry.elapsed();

            let db_path = data.join("deck.db");
            let t_store = std::time::Instant::now();
            let store = tauri::async_runtime::block_on(Store::open(&db_path))
                .expect("database must open");
            let store_ms = t_store.elapsed();

            let store = Arc::new(store);

            // The rotating snapshot is a full `VACUUM INTO` copy. It used to run
            // inside `Store::open`, which meant the window could not appear
            // until a copy of the whole database had been written. It still
            // happens on every launch -- just behind the UI instead of in front
            // of it.
            {
                let store = Arc::clone(&store);
                tauri::async_runtime::spawn(async move {
                    store.snapshot(&db_path).await;
                });
            }

            app.manage(AppState {
                store,
                registry: Arc::new(registry),
                supervisor: Supervisor::new(),
                logs_root: data.join("logs"),
                data_dir: data.clone(),
                pumps: pump::PumpRegistry::default(),
                sampler: Arc::new(Sampler::new()),
                icons: Mutex::new(HashMap::new()),
                timings: Arc::clone(&timings),
            });

            pump::spawn_state_pump(app.handle());
            pump::spawn_metrics_pump(app.handle());


            // Warm the programs the user is most likely to run next.
            //
            // Selection-driven priming only helps someone who selects a row
            // first. Reaching for a project you launch every day is muscle
            // memory -- pointer straight to the Run button -- and that path
            // gets no lead time at all.
            //
            // So the few most recently launched projects are primed right after
            // startup, while the window is already up and the user is still
            // reading the list. This is also what puts their programs in the
            // PATH-resolution cache before the first launch of the session,
            // which is the larger half of what priming buys. Bounded hard: it
            // is a bet on what they will do next, and a wrong bet must cost
            // almost nothing.
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let Some(state) = handle.try_state::<AppState>() else {
                        return;
                    };
                    let Ok(projects) = state.store.projects().await else {
                        return;
                    };
                    let mut recent: Vec<_> = projects
                        .into_iter()
                        .filter(|p| p.last_launched_at.is_some() && !p.archived)
                        .collect();
                    recent.sort_by(|a, b| b.last_launched_at.cmp(&a.last_launched_at));

                    for project in recent.into_iter().take(RECENT_PREWARM) {
                        if let Ok(plan) =
                            deck_runners::plan(&project, &state.registry, Lifecycle::Run)
                        {
                            let _ = deck_runtime::resolve_program(&plan.command.program);
                        }
                    }
                    tracing::debug!("warmed PATH resolution for recent projects");
                });
            }

            // Pick up projects added to a watched folder since last time.
            //
            // The scan roots are already remembered and `rescan_roots` already
            // knows how to add what is new -- it was just behind a button, so
            // a project created yesterday stayed invisible until someone
            // thought to press it. Background, after the window is up, because
            // a filesystem walk is not something to boot behind.
            {
                let handle = app.handle().clone();
                // Captured on the setup thread, before the task is queued.
                let started_at = chrono::Utc::now();
                tauri::async_runtime::spawn(async move {
                    let Some(state) = handle.try_state::<AppState>() else {
                        return;
                    };

                    // Close runs whose end was never written.
                    //
                    // Nothing has been spawned at this point in boot, so any
                    // row still marked `running` describes a process that is
                    // not alive -- the app was killed, or the machine went
                    // down. Left alone they accumulate forever and every
                    // surface reading run history has to work around them.
                    // Bounded by when THIS process started, so a run launched
                    // while the sweep is still queued can never be swept.
                    match state.store.reconcile_orphaned_runs(started_at).await {
                        Ok(0) => {}
                        Ok(n) => tracing::info!(
                            repaired = n,
                            "closed runs left open by a previous session"
                        ),
                        Err(e) => tracing::warn!(error = ?e, "run reconciliation failed"),
                    }

                    // Bound the whole log tree.
                    //
                    // Per-project retention keeps ten runs each, which is not a
                    // bound on the sum: this machine's tree reached 90 MB, most
                    // of it ten retained copies of one program's runaway output
                    // at ~4 MB apiece -- every file inside every limit that
                    // existed. Oldest-first, because the run worth reading is
                    // almost always the last one.
                    match deck_runtime::logs::prune_logs_total(
                        &state.logs_root,
                        deck_runtime::logs::MAX_TOTAL_LOG_BYTES,
                    ) {
                        Ok(0) => {}
                        Ok(bytes) => tracing::info!(
                            reclaimed_mb = bytes / (1024 * 1024),
                            "pruned the log tree to its size cap"
                        ),
                        Err(e) => tracing::warn!(error = %e, "could not prune the log tree"),
                    }

                    // Refresh stale runners while we are here.
                    //
                    // Detection is frozen at registration, and runners are
                    // DATA -- dropping in a better manifest is the supported
                    // way to teach Launch Deck something. Pulse was registered
                    // before the Tauri manifest could claim it, so it kept
                    // running `cargo run` in a workspace with two binaries and
                    // failed with "available binaries: pulse, pulse-app" every
                    // time. `redetect_project` already fixed that, from a menu
                    // nobody had a reason to open.
                    if let Ok(projects) = state.store.projects().await {
                        for project in projects {
                            let before = project.detected.runner_id.clone();
                            let id = project.id.to_hyphenated();
                            if let Ok(after) = commands::redetect_project(state.clone(), id).await {
                                if after.runner_id != before {
                                    tracing::info!(
                                        project = %after.name,
                                        from = %before,
                                        to = %after.runner_id,
                                        "startup re-detect corrected a stale runner"
                                    );
                                }
                            }
                        }
                    }

                    match commands::rescan_roots(state).await {
                        // Only tell the UI when there is something to see.
                        Ok(n) if n > 0 => {
                            tracing::info!(added = n, "startup rescan found new projects");
                            let _ = handle.emit("deck://projects", n);
                        }
                        Ok(_) => {}
                        Err(e) => tracing::warn!(error = ?e, "startup rescan failed"),
                    }
                });

                // Keep the library current without being asked: re-walk the
                // scan roots every ten minutes so a freshly created project
                // appears on its own. LOCAL FILESYSTEM ONLY -- the network
                // boundary in `deck_runtime::surface` (no background network,
                // ever) is untouched by this; walking Tanner's own folders is
                // the app doing its filing, not the app phoning anywhere.
                {
                    let handle = app.handle().clone();
                    tauri::async_runtime::spawn(async move {
                        let mut tick =
                            tokio::time::interval(std::time::Duration::from_secs(600));
                        tick.set_missed_tick_behavior(
                            tokio::time::MissedTickBehavior::Delay,
                        );
                        // The first tick fires immediately; the startup rescan
                        // above already covered that, so consume it.
                        tick.tick().await;
                        loop {
                            tick.tick().await;
                            let state = handle.state::<AppState>();
                            match commands::rescan_roots(state).await {
                                Ok(n) if n > 0 => {
                                    tracing::info!(
                                        added = n,
                                        "periodic rescan found new projects"
                                    );
                                    let _ = handle.emit("deck://projects", n);
                                }
                                Ok(_) => {}
                                Err(e) => {
                                    tracing::warn!(error = ?e, "periodic rescan failed");
                                }
                            }
                        }
                    });
                }
            }

            // Non-fatal: an app with no tray is degraded, an app that refuses
            // to start because of a tray is worse.
            let t_tray = std::time::Instant::now();
            // Serve reveal requests raised by a second launch.
            fastpath::serve_reveals(app.handle().clone());

            if let Err(e) = tray::install(app.handle()) {
                tracing::error!(error = %e, "tray icon unavailable; close will quit the app");
            }
            let tray_ms = t_tray.elapsed();

            timings.set_phases(timings::StartupPhases {
                registry: registry_ms,
                store_open: store_ms,
                // Filled in after `build()` returns -- it cannot know its own
                // duration from inside the setup callback.
                builder: std::time::Duration::ZERO,
                // Filled in by the warm-up thread; zero until it lands.
                // Filled in by the frontend's `ui_ready` call.
                to_first_paint: std::time::Duration::ZERO,
                dom_interactive_ms: 0,
                script_eval_ms: 0,
                react_mount_ms: 0,
                fetch_start_ms: 0,
                data_arrived_ms: 0,
                painted_ms: 0,
                tray: tray_ms,
                setup: setup_started.elapsed(),
                to_ready: timings.process_start.elapsed(),
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            // THE FIX. Without this, closing the window exits the process,
            // `RunEvent::Exit` runs `supervisor.shutdown()`, and every project
            // the user launched is terminated -- reproduced deliberately: a
            // fixture running as pid 9936 was gone three seconds after the
            // window closed.
            //
            // Hiding instead keeps the process, the job objects and therefore
            // the projects alive. Quitting stays available and explicit, from
            // the tray menu, where it can say what it is about to stop.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
                tray::refresh_tooltip(window.app_handle());
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building Launch Deck");

    // Everything before the window: plugin init, capability parsing, window
    // creation and the WebView2 environment. Setup runs inside this, so the
    // difference between the two is the part that is not ours.
    builder_timings.set_builder(t_builder.elapsed());
    tracing::info!(ms = t_builder.elapsed().as_millis(), "tauri builder complete");

    app.run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                // `try_state`, not `state`: an instance that lost the
                // single-instance race never managed any state, and panicking
                // on the way out would turn a clean exit into a crash dialog.
                let Some(state) = app_handle.try_state::<AppState>() else {
                    return;
                };

                // Take every supervised tree down with the app. Children are in
                // kill-on-close job objects, so even a hard kill of this
                // process could not orphan them -- this is the polite version.
                state.supervisor.shutdown();

                // Fold the write-ahead log into the database file so what is on
                // disk is complete and self-contained.
                let store = Arc::clone(&state.store);
                tauri::async_runtime::block_on(async move { store.checkpoint().await });
            }
        });
}
