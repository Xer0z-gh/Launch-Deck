//! The developer diagnostics report.
//!
//! # Why this exists, and why it judges rather than dumps
//!
//! A diagnostics screen that prints forty facts and leaves the reader to spot
//! the bad one is a log, not a diagnostic. Launch Deck has already lost a
//! project registry to a setting that was *visible the whole time* -- the WAL
//! autocheckpoint sat at its 1000-page default while every row waited in an
//! un-checkpointed journal, and nothing in the app was in a position to say
//! "that number is wrong".
//!
//! So every section below produces facts AND a set of [`Check`]s. A check
//! states what was expected, what was found, and how bad the difference is.
//! The screen leads with the failures; the raw facts are underneath for when
//! the checks do not cover whatever is actually broken today.
//!
//! # What is deliberately NOT here
//!
//! Nothing in this module mutates state, kills a process, or repairs anything.
//! A diagnostic that also fixes things cannot be run safely when you are unsure
//! what is wrong, which is exactly when you want to run it.

use std::path::{Path, PathBuf};

use deck_runtime::program::{self, Invocation};
use serde::Serialize;

use crate::AppState;

/// Severity of a single check. Ordered worst-first for sorting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Something is broken and will cause visible misbehaviour.
    Fail,
    /// Working, but outside its intended envelope -- worth knowing.
    Warn,
    /// Behaving as designed.
    Ok,
    /// Context with no pass/fail meaning.
    Info,
}

/// One evaluated statement about the app's health.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    /// Stable identifier, for referring to a finding without quoting prose.
    pub id: String,
    /// Which section of the report it belongs to.
    pub section: String,
    /// One-line human statement of what was checked.
    pub label: String,
    pub severity: Severity,
    /// What was found, and where relevant what was expected instead.
    pub detail: String,
}

impl Check {
    fn new(
        id: &str,
        section: &str,
        label: &str,
        severity: Severity,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            id: id.to_owned(),
            section: section.to_owned(),
            label: label.to_owned(),
            severity,
            detail: detail.into(),
        }
    }
}

/// A labelled fact. Kept as strings because this is a report, not an API --
/// the frontend renders rows and never does arithmetic on these.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fact {
    /// Field name as displayed.
    pub label: String,
    /// Formatted value.
    pub value: String,
}

fn fact(label: &str, value: impl Into<String>) -> Fact {
    Fact {
        label: label.to_owned(),
        value: value.into(),
    }
}

/// A group of related facts.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Section {
    /// Section heading.
    pub title: String,
    /// Rows in display order.
    pub facts: Vec<Fact>,
}

/// The whole report.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsReport {
    /// RFC3339 timestamp of collection.
    pub generated_at: String,
    /// Every evaluated check, worst first.
    pub checks: Vec<Check>,
    /// Raw facts, grouped.
    pub sections: Vec<Section>,
}

fn human_bytes(n: u64) -> String {
    #[allow(clippy::cast_precision_loss)] // display only
    let f = n as f64;
    if n < 1024 {
        format!("{n} B")
    } else if f < 1024.0 * 1024.0 {
        format!("{:.1} KB", f / 1024.0)
    } else if f < 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} MB", f / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", f / (1024.0 * 1024.0 * 1024.0))
    }
}

fn file_len(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// Total bytes and file count under a directory, recursively.
fn dir_size(root: &Path) -> (u64, u64) {
    let mut bytes = 0;
    let mut files = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(std::result::Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                bytes += file_len(&path);
                files += 1;
            }
        }
    }
    (bytes, files)
}

fn ms(d: std::time::Duration) -> String {
    let v = d.as_secs_f64() * 1000.0;
    if v < 10.0 {
        format!("{v:.1} ms")
    } else {
        format!("{v:.0} ms")
    }
}

/// Programs worth knowing about, because a project that will not start is
/// usually a project whose interpreter is not on PATH.
const TOOLCHAIN: &[&str] = &[
    "node", "npm", "python", "cargo", "rustc", "git", "code", "dotnet", "go", "java",
];

/// Collects the full report.
///
/// Every filesystem and database read here is fallible and none of them abort
/// the report: a diagnostics screen that refuses to render because one probe
/// failed is useless precisely when it is needed.
#[allow(clippy::too_many_lines)] // a report is a long list by nature
pub async fn collect(state: &AppState) -> DiagnosticsReport {
    let mut checks = Vec::new();
    let mut sections = Vec::new();

    // ---- Application ------------------------------------------------------
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("<unknown>"));
    let profile = if cfg!(debug_assertions) { "debug" } else { "release" };
    sections.push(Section {
        title: "Application".to_owned(),
        facts: vec![
            fact("Version", env!("CARGO_PKG_VERSION")),
            fact("Build profile", profile),
            fact("Executable", exe.display().to_string()),
            fact("OS", format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)),
            fact(
                "WebView2",
                tauri::webview_version().unwrap_or_else(|_| "unavailable".to_owned()),
            ),
            fact("Tauri", tauri::VERSION),
        ],
    });

    // A debug binary in daily use is worth flagging: it is slower, and it
    // exposes the dev hooks the release build deliberately withholds.
    if profile == "debug" {
        checks.push(Check::new(
            "app.profile",
            "Application",
            "Running a debug build",
            Severity::Warn,
            "Debug builds are slower and expose dev-only hooks. Fine while developing.",
        ));
    }

    // ---- Database ---------------------------------------------------------
    match state.store.diagnostics().await {
        Ok(d) => {
            let wal = d.path.with_extension("db-wal");
            let shm = d.path.with_extension("db-shm");
            let wal_bytes = file_len(&wal);
            let db_bytes = file_len(&d.path);

            sections.push(Section {
                title: "Database".to_owned(),
                facts: vec![
                    fact("Path", d.path.display().to_string()),
                    fact("File size", human_bytes(db_bytes)),
                    fact("WAL size", human_bytes(wal_bytes)),
                    fact("Shared-memory file", human_bytes(file_len(&shm))),
                    fact("Journal mode", d.journal_mode.clone()),
                    fact("Synchronous", d.synchronous_label()),
                    fact("WAL autocheckpoint", format!("{} pages", d.wal_autocheckpoint)),
                    fact("Foreign keys", if d.foreign_keys { "on" } else { "OFF" }),
                    fact("Page size", human_bytes(u64::try_from(d.page_size).unwrap_or(0))),
                    fact("Pages", d.page_count.to_string()),
                    fact("Free pages", d.freelist_count.to_string()),
                    fact(
                        "Schema version",
                        d.schema_version.map_or_else(|| "none".to_owned(), |v| v.to_string()),
                    ),
                    fact("Projects", d.projects.to_string()),
                    fact("Runs recorded", d.runs.to_string()),
                    fact("Settings", d.settings.to_string()),
                ],
            });

            checks.push(if d.integrity == "ok" {
                Check::new(
                    "db.integrity",
                    "Database",
                    "Database integrity",
                    Severity::Ok,
                    "PRAGMA integrity_check returned ok",
                )
            } else {
                Check::new(
                    "db.integrity",
                    "Database",
                    "Database integrity",
                    Severity::Fail,
                    d.integrity.clone(),
                )
            });

            // The exact configuration whose absence cost a registry.
            let durable = d.journal_mode.eq_ignore_ascii_case("wal")
                && d.synchronous >= 2
                && d.wal_autocheckpoint > 0
                && d.wal_autocheckpoint <= 16;
            checks.push(Check::new(
                "db.durability",
                "Database",
                "Crash-safe write settings",
                if durable { Severity::Ok } else { Severity::Fail },
                format!(
                    "journal={} synchronous={} autocheckpoint={} (want wal / FULL / <=16)",
                    d.journal_mode,
                    d.synchronous_label(),
                    d.wal_autocheckpoint
                ),
            ));

            // A WAL larger than the database means writes are piling up
            // uncheckpointed -- the shape of the original data-loss incident.
            checks.push(if wal_bytes > db_bytes && db_bytes > 0 {
                Check::new(
                    "db.wal_growth",
                    "Database",
                    "WAL is larger than the database",
                    Severity::Warn,
                    format!(
                        "WAL {} vs database {} -- writes may not be checkpointing",
                        human_bytes(wal_bytes),
                        human_bytes(db_bytes)
                    ),
                )
            } else {
                Check::new(
                    "db.wal_growth",
                    "Database",
                    "WAL size is sane",
                    Severity::Ok,
                    format!("WAL {}", human_bytes(wal_bytes)),
                )
            });

            checks.push(if d.foreign_keys {
                Check::new(
                    "db.foreign_keys",
                    "Database",
                    "Foreign keys enforced",
                    Severity::Ok,
                    "on",
                )
            } else {
                Check::new(
                    "db.foreign_keys",
                    "Database",
                    "Foreign keys enforced",
                    Severity::Fail,
                    "off -- orphaned rows can accumulate",
                )
            });

            // ---- Snapshots ---------------------------------------------------
            let backups = d.path.parent().unwrap_or(Path::new(".")).join("backups");
            let mut snaps: Vec<PathBuf> = std::fs::read_dir(&backups)
                .map(|rd| {
                    rd.filter_map(std::result::Result::ok)
                        .map(|e| e.path())
                        .filter(|p| p.extension().is_some_and(|e| e == "db"))
                        .collect()
                })
                .unwrap_or_default();
            snaps.sort();
            let newest = snaps.last().cloned();
            let total: u64 = snaps.iter().map(|p| file_len(p)).sum();

            sections.push(Section {
                title: "Snapshots".to_owned(),
                facts: vec![
                    fact("Directory", backups.display().to_string()),
                    fact("Count", snaps.len().to_string()),
                    fact("Total size", human_bytes(total)),
                    fact(
                        "Newest",
                        newest.as_ref().map_or_else(
                            || "none".to_owned(),
                            |p| {
                                p.file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_default()
                            },
                        ),
                    ),
                ],
            });

            checks.push(if snaps.is_empty() {
                Check::new(
                    "db.snapshots",
                    "Snapshots",
                    "Recovery snapshot exists",
                    if d.projects > 0 { Severity::Fail } else { Severity::Info },
                    "No snapshot found. A corrupted or emptied database could not self-heal.",
                )
            } else {
                Check::new(
                    "db.snapshots",
                    "Snapshots",
                    "Recovery snapshot exists",
                    Severity::Ok,
                    format!("{} snapshot(s), newest {}", snaps.len(), human_bytes(
                        newest.as_ref().map_or(0, |p| file_len(p))
                    )),
                )
            });
        }
        Err(e) => {
            checks.push(Check::new(
                "db.unreachable",
                "Database",
                "Database could not be queried",
                Severity::Fail,
                e.to_string(),
            ));
        }
    }

    // ---- Runners ----------------------------------------------------------
    let runners = state.registry.runners();
    let problems = state.registry.problems();
    sections.push(Section {
        title: "Runners".to_owned(),
        facts: vec![
            fact("Manifests loaded", runners.len().to_string()),
            fact("Load problems", problems.len().to_string()),
        ],
    });
    checks.push(if problems.is_empty() {
        Check::new(
            "runners.manifests",
            "Runners",
            "All runner manifests parsed",
            Severity::Ok,
            format!("{} loaded", runners.len()),
        )
    } else {
        Check::new(
            "runners.manifests",
            "Runners",
            "Some runner manifests failed to load",
            Severity::Fail,
            problems
                .iter()
                .map(|p| {
                    let who = p
                        .id
                        .clone()
                        .unwrap_or_else(|| p.path.display().to_string());
                    format!("{who}: {}", p.reason)
                })
                .collect::<Vec<_>>()
                .join("; "),
        )
    });
    checks.push(if runners.is_empty() {
        Check::new(
            "runners.present",
            "Runners",
            "Runner manifests present",
            Severity::Fail,
            "No runners loaded -- nothing can be detected or launched.",
        )
    } else {
        Check::new(
            "runners.present",
            "Runners",
            "Runner manifests present",
            Severity::Ok,
            format!("{} runners", runners.len()),
        )
    });

    // ---- Runtime ----------------------------------------------------------
    let states = state.supervisor.states();
    let live = states.iter().filter(|(_, s)| s.is_live()).count();
    sections.push(Section {
        title: "Runtime".to_owned(),
        facts: vec![
            fact("Tracked run states", states.len().to_string()),
            fact("Currently live", live.to_string()),
            fact("Log pumps", state.pumps.len().to_string()),
            fact("Cached icons", state.icons.lock().len().to_string()),
        ],
    });

    // A pump with no live process is a leak: it holds a task and a channel.
    checks.push(if state.pumps.len() >= live {
        Check::new(
            "runtime.pumps",
            "Runtime",
            "Log pumps match live processes",
            Severity::Ok,
            format!("{} pump(s), {live} live", state.pumps.len()),
        )
    } else {
        Check::new(
            "runtime.pumps",
            "Runtime",
            "Fewer log pumps than live processes",
            Severity::Warn,
            format!(
                "{} pump(s) for {live} live process(es) -- output may be missing",
                state.pumps.len()
            ),
        )
    });

    // ---- Storage on disk --------------------------------------------------
    let (log_bytes, log_files) = dir_size(&state.logs_root);
    sections.push(Section {
        title: "Log storage".to_owned(),
        facts: vec![
            fact("Directory", state.logs_root.display().to_string()),
            fact("Files", log_files.to_string()),
            fact("Total size", human_bytes(log_bytes)),
        ],
    });
    const LOG_BUDGET: u64 = 512 * 1024 * 1024;
    checks.push(if log_bytes > LOG_BUDGET {
        Check::new(
            "logs.disk",
            "Log storage",
            "Log directory is large",
            Severity::Warn,
            format!(
                "{} across {log_files} files. Logs are pruned per project but not capped overall.",
                human_bytes(log_bytes)
            ),
        )
    } else {
        Check::new(
            "logs.disk",
            "Log storage",
            "Log directory size is sane",
            Severity::Ok,
            format!("{} across {log_files} files", human_bytes(log_bytes)),
        )
    });

    // Writability decides whether anything can be saved at all, so probe it
    // for real rather than inferring it from a permissions bit.
    let probe = state.logs_root.join(".deck-write-probe");
    let writable = std::fs::create_dir_all(&state.logs_root)
        .and_then(|()| std::fs::write(&probe, b"probe"))
        .is_ok();
    let _ = std::fs::remove_file(&probe);
    checks.push(if writable {
        Check::new(
            "fs.writable",
            "Log storage",
            "Data directory is writable",
            Severity::Ok,
            state.logs_root.display().to_string(),
        )
    } else {
        Check::new(
            "fs.writable",
            "Log storage",
            "Data directory is NOT writable",
            Severity::Fail,
            format!("Cannot write to {}", state.logs_root.display()),
        )
    });

    // ---- Toolchain --------------------------------------------------------
    let mut tool_facts = Vec::new();
    let mut missing = Vec::new();
    for name in TOOLCHAIN {
        match program::resolve(name) {
            Some(Invocation::Direct(p)) => tool_facts.push(fact(name, p.display().to_string())),
            Some(Invocation::ViaShell(p)) => {
                tool_facts.push(fact(name, format!("{} (via shell)", p.display())));
            }
            None => {
                tool_facts.push(fact(name, "not on PATH"));
                missing.push((*name).to_owned());
            }
        }
    }
    sections.push(Section {
        title: "Toolchain".to_owned(),
        facts: tool_facts,
    });
    checks.push(Check::new(
        "env.toolchain",
        "Toolchain",
        "Interpreters and build tools on PATH",
        Severity::Info,
        if missing.is_empty() {
            "All probed tools resolved.".to_owned()
        } else {
            format!(
                "Not found: {}. Projects needing these cannot start.",
                missing.join(", ")
            )
        },
    ));

    // ---- Startup performance ----------------------------------------------
    let phases = state.timings.phases();
    let uptime = state.timings.uptime();
    // Whatever setup spent that no phase claims. Saturating because the phases
    // are measured independently and a scheduling hiccup can make them sum to
    // slightly more than the whole.
    let unattributed = phases
        .setup
        .saturating_sub(phases.registry)
        .saturating_sub(phases.store_open)
        .saturating_sub(phases.tray);
    sections.push(Section {
        title: "Startup".to_owned(),
        facts: vec![
            fact("Tauri build (window + WebView2)", ms(phases.builder)),
            fact("Runner manifests parsed", ms(phases.registry)),
            fact("Database opened", ms(phases.store_open)),
            fact("Tray icon built", ms(phases.tray)),
            fact("Setup total", ms(phases.setup)),
            // The number that would have caught a 254 ms regression the day it
            // landed. "Setup total: 1120 ms" named nothing; a report that
            // cannot say WHICH part got slow is a log, not a diagnostic.
            fact("  of which unattributed", ms(unattributed)),
            fact("Process start to ready", ms(phases.to_ready)),
            fact(
                "Process start to first rows",
                if phases.to_first_paint.is_zero() {
                    "not reported".to_owned()
                } else {
                    ms(phases.to_first_paint)
                },
            ),
            fact(
                "  webview: HTML parsed",
                format!("{} ms", phases.dom_interactive_ms),
            ),
            fact("  webview: our JS starts", format!("{} ms", phases.script_eval_ms)),
            fact("  webview: React mounts", format!("{} ms", phases.react_mount_ms)),
            fact("  webview: rows painted", format!("{} ms", phases.painted_ms)),
            fact(
                "Uptime",
                format!("{}h {}m", uptime.as_secs() / 3600, (uptime.as_secs() % 3600) / 60),
            ),
        ],
    });

    // A slow boot is the difference between a launcher you reach for and one
    // you avoid, so it gets a real threshold rather than only being reported.
    checks.push(if phases.to_ready.as_millis() < 600 {
        Check::new(
            "perf.boot",
            "Startup",
            "Backend ready quickly",
            Severity::Ok,
            format!("{} from process start", ms(phases.to_ready)),
        )
    } else {
        Check::new(
            "perf.boot",
            "Startup",
            "Backend was slow to become ready",
            Severity::Warn,
            format!(
                "{} from process start (registry {}, database {})",
                ms(phases.to_ready),
                ms(phases.registry),
                ms(phases.store_open)
            ),
        )
    });

    // Setup is OUR code. Everything else in the boot figure is Tauri and
    // WebView2 starting a browser engine, which we do not control -- so this is
    // the part a regression can actually be pinned on.
    checks.push(if unattributed.as_millis() < 120 {
        Check::new(
            "perf.setup-attributed",
            "Startup",
            "Setup time is accounted for",
            Severity::Ok,
            format!("{} unattributed of {} total", ms(unattributed), ms(phases.setup)),
        )
    } else {
        Check::new(
            "perf.setup-attributed",
            "Startup",
            "Setup spent time no phase claims",
            Severity::Warn,
            format!(
                "{} of {} is unattributed -- something was added to setup without                  being measured. Instrument it before optimising it.",
                ms(unattributed),
                ms(phases.setup)
            ),
        )
    });

    // ---- Launch performance -----------------------------------------------
    let launches = state.timings.launches();
    let mut launch_facts = vec![fact("Launches this session", launches.len().to_string())];
    if let Some(mean) = state.timings.mean_launch() {
        launch_facts.push(fact("Mean time to spawn", ms(mean)));
    }
    for l in launches.iter().rev().take(8) {
        launch_facts.push(fact(
            &l.project,
            format!(
                "{} total  (lookup {}, plan {}, ports {}, spawn {})",
                ms(l.total),
                ms(l.lookup),
                ms(l.plan),
                ms(l.port_check),
                ms(l.spawn)
            ),
        ));
    }
    sections.push(Section {
        title: "Launch performance".to_owned(),
        facts: launch_facts,
    });

    if let Some(mean) = state.timings.mean_launch() {
        checks.push(if mean.as_millis() < 150 {
            Check::new(
                "perf.launch",
                "Launch performance",
                "Projects spawn promptly",
                Severity::Ok,
                format!("mean {} from click to spawned process", ms(mean)),
            )
        } else {
            Check::new(
                "perf.launch",
                "Launch performance",
                "Projects are slow to spawn",
                Severity::Warn,
                format!("mean {} -- see the per-launch breakdown below", ms(mean)),
            )
        });
    }

    // ---- This process ------------------------------------------------------
    state.sampler.refresh();
    let mut self_facts = vec![fact("PID", std::process::id().to_string())];
    if let Some((mem, procs, cpu)) = state.sampler.self_usage() {
        self_facts.push(fact("Memory", human_bytes(mem)));
        self_facts.push(fact("Processes in tree", procs.to_string()));
        self_facts.push(fact("CPU", format!("{cpu:.1}%")));

        // WebView2 is a browser engine, so a few hundred MB is normal. Growth
        // without bound is the shape of a leak.
        checks.push(if mem < 900 * 1024 * 1024 {
            Check::new(
                "perf.memory",
                "This process",
                "Memory use is reasonable",
                Severity::Ok,
                human_bytes(mem),
            )
        } else {
            Check::new(
                "perf.memory",
                "This process",
                "Memory use is high",
                Severity::Warn,
                format!("{} across the app's own process tree", human_bytes(mem)),
            )
        });
    }
    sections.push(Section {
        title: "This process".to_owned(),
        facts: self_facts,
    });

    // ---- Machine -----------------------------------------------------------
    let mut machine = vec![fact(
        "Logical CPUs",
        std::thread::available_parallelism()
            .map_or_else(|_| "unknown".to_owned(), |n| n.to_string()),
    )];
    if let Some((free, total)) = deck_runtime::disk_space(&state.logs_root) {
        machine.push(fact("Disk free", human_bytes(free)));
        machine.push(fact("Disk total", human_bytes(total)));

        // Everything this app does -- snapshots, logs, the database -- needs
        // somewhere to land.
        const LOW_DISK: u64 = 2 * 1024 * 1024 * 1024;
        checks.push(if free > LOW_DISK {
            Check::new("fs.free", "Machine", "Free disk space", Severity::Ok, human_bytes(free))
        } else {
            Check::new(
                "fs.free",
                "Machine",
                "Very little free disk space",
                Severity::Fail,
                format!("{} free -- snapshots and logs may fail to write", human_bytes(free)),
            )
        });
    }
    sections.push(Section {
        title: "Machine".to_owned(),
        facts: machine,
    });

    // ---- Environment -------------------------------------------------------
    let path_var = std::env::var("PATH").unwrap_or_default();
    let path_entries = path_var.split(';').filter(|p| !p.is_empty()).count();
    let no_default_cwd = std::env::var("NoDefaultCurrentDirectoryInExePath").is_ok();
    sections.push(Section {
        title: "Environment".to_owned(),
        facts: vec![
            fact("PATH entries", path_entries.to_string()),
            fact("PATH length", format!("{} chars", path_var.len())),
            fact(
                "NoDefaultCurrentDirectoryInExePath",
                if no_default_cwd { "SET" } else { "not set" },
            ),
            fact("TEMP", std::env::var("TEMP").unwrap_or_else(|_| "unset".to_owned())),
            fact(
                "Working directory",
                std::env::current_dir()
                    .map_or_else(|_| "unknown".to_owned(), |p| p.display().to_string()),
            ),
        ],
    });

    // This one has bitten this project before: Git Bash exports it, children
    // inherit it, and a `cmd /c run.bat` that resolves everywhere else stops
    // resolving here.
    checks.push(if no_default_cwd {
        Check::new(
            "env.cwd_resolution",
            "Environment",
            "NoDefaultCurrentDirectoryInExePath is set",
            Severity::Warn,
            "Launched children inherit it. Commands that resolve a program from the working directory will fail. Git Bash sets this.",
        )
    } else {
        Check::new(
            "env.cwd_resolution",
            "Environment",
            "Program resolution is not restricted",
            Severity::Ok,
            "NoDefaultCurrentDirectoryInExePath is not set",
        )
    });

    // ---- Live processes ----------------------------------------------------
    let mut live_facts = Vec::new();
    for (project_id, run_state) in &states {
        if !run_state.is_live() {
            continue;
        }
        let name = state
            .store
            .project(*project_id)
            .await
            .map_or_else(|_| project_id.to_hyphenated(), |p| p.name);
        let detail = run_state.pid().map_or_else(
            || "no pid".to_owned(),
            |pid| {
                let ports = deck_runtime::ports_for_pids(&state.sampler.tree_pids(pid));
                let mem = state
                    .sampler
                    .latest(*project_id)
                    .map_or_else(String::new, |s| format!(", {}", human_bytes(s.memory_bytes)));
                let port_text = if ports.is_empty() {
                    String::new()
                } else {
                    format!(
                        ", ports {}",
                        ports.iter().map(u16::to_string).collect::<Vec<_>>().join("/")
                    )
                };
                format!("pid {pid}{mem}{port_text}")
            },
        );
        live_facts.push(fact(&name, detail));
    }
    if live_facts.is_empty() {
        live_facts.push(fact("Running", "nothing"));
    }
    sections.push(Section {
        title: "Live processes".to_owned(),
        facts: live_facts,
    });

    // Worst first, so the screen opens on whatever is actually wrong.
    checks.sort_by(|a, b| a.severity.cmp(&b.severity).then_with(|| a.id.cmp(&b.id)));

    DiagnosticsReport {
        generated_at: chrono::Utc::now().to_rfc3339(),
        checks,
        sections,
    }
}
