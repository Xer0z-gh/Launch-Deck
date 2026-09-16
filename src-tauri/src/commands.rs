//! Tauri command adapters.
//!
//! Each command validates its input, calls into the crates, and maps the
//! result. Anything resembling a business rule lives below this file --
//! command resolution in `deck_runners::launch`, persistence in `deck_store`,
//! process control in `deck_runtime`.

use std::path::{Path, PathBuf};

use chrono::Utc;
use deck_domain::command::{parse_command_line, CommandSpec, Lifecycle};
use deck_domain::error::DeckError;
use deck_domain::log::RunOutcome;
use deck_domain::project::{DetectedFacts, Project, ProjectId};
use deck_domain::runtime::{RunRecord, StopMode};
use deck_runners::scan::{draft_for, scan, ScanOptions};
use deck_runtime::SpawnRequest;
use tauri::State;

use crate::dto::{
    DetectionDto, InspectDto, LogsDto, MetricsDto, ProjectDto, ProjectPatch, RunRecordDto,
    RunStateDto, ScanHitDto, ScanResultDto, StateEvent,
};
use crate::error::{IpcError, IpcResult};
use crate::pump;
use crate::AppState;

fn parse_id(raw: &str) -> IpcResult<ProjectId> {
    raw.parse::<ProjectId>()
        .map_err(|_| IpcError::from(DeckError::Storage(format!("malformed project id `{raw}`"))))
}

fn parse_step(raw: Option<&str>) -> IpcResult<Lifecycle> {
    let Some(raw) = raw else {
        return Ok(Lifecycle::Run);
    };
    Lifecycle::ALL
        .into_iter()
        .find(|s| s.to_string() == raw)
        .ok_or_else(|| IpcError::from(DeckError::Storage(format!("unknown lifecycle `{raw}`"))))
}

// ---- Reading ----------------------------------------------------------------

#[tauri::command]
pub async fn list_projects(state: State<'_, AppState>) -> IpcResult<Vec<ProjectDto>> {
    let t_db = std::time::Instant::now();
    let projects = state.store.projects().await?;
    let db = t_db.elapsed();

    // How often each has been launched, for the library's resting order.
    // One GROUP BY, not one count per project.
    let counts: std::collections::HashMap<ProjectId, u32> =
        state.store.run_counts().await?.into_iter().collect();

    let t_map = std::time::Instant::now();
    let dtos: Vec<ProjectDto> = projects
        .iter()
        .map(|p| {
            let mut dto = ProjectDto::build(p, &state.registry);
            dto.launch_count = counts.get(&p.id).copied().unwrap_or(0);
            dto
        })
        .collect();
    // This is the first thing the UI asks for and nothing paints until it
    // answers, so the split between "read the database" and "build the view
    // model" needs to be visible rather than inferred.
    tracing::debug!(
        projects = dtos.len(),
        db_ms = db.as_millis(),
        map_ms = t_map.elapsed().as_millis(),
        "list_projects"
    );
    Ok(dtos)
}

#[tauri::command]
pub fn run_states(state: State<'_, AppState>) -> Vec<StateEvent> {
    state
        .supervisor
        .states()
        .into_iter()
        .map(|(id, s)| StateEvent {
            project_id: id.to_hyphenated(),
            run_id: state
                .supervisor
                .current_run(id)
                .map(|r| r.to_string())
                .unwrap_or_default(),
            state: RunStateDto::from(&s),
        })
        .collect()
}

#[tauri::command]
pub async fn run_history(
    state: State<'_, AppState>,
    id: String,
    limit: Option<u32>,
) -> IpcResult<Vec<RunRecordDto>> {
    let id = parse_id(&id)?;
    let records = state.store.run_history(id, limit.unwrap_or(50)).await?;
    Ok(records.iter().map(RunRecordDto::from).collect())
}

#[tauri::command]
pub fn get_logs(state: State<'_, AppState>, id: String, after: Option<u64>) -> IpcResult<LogsDto> {
    let id = parse_id(&id)?;
    let Some(sink) = state.supervisor.logs(id) else {
        return Ok(LogsDto {
            lines: Vec::new(),
            dropped: 0,
            total: 0,
        });
    };
    let lines = match after {
        Some(seq) => sink.since(seq),
        None => sink.snapshot(),
    };
    Ok(LogsDto {
        lines: lines.iter().map(Into::into).collect(),
        dropped: sink.dropped_count(),
        total: sink.total_count(),
    })
}

// ---- Discovery --------------------------------------------------------------

#[tauri::command]
pub async fn inspect_path(state: State<'_, AppState>, path: String) -> IpcResult<InspectDto> {
    let root = PathBuf::from(&path);
    if !root.is_dir() {
        return Err(DeckError::UnreadableDirectory { path: root }.into());
    }

    let name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Untitled")
        .to_owned();

    let matches = state
        .registry
        .detect_all(&root)
        .iter()
        .map(|m| {
            let draft = draft_for(&root, Some(m), &state.registry);
            DetectionDto {
                runner_id: m.meta.id.clone(),
                runner_name: m.meta.name.clone(),
                language: m.meta.language.clone(),
                framework: m.meta.framework.clone(),
                version: m.version.clone(),
                proposed_run: draft.proposed_run,
            }
        })
        .collect();

    let already_registered = state
        .store
        .project_by_root(&root)
        .await?
        .map(|p| p.id.to_hyphenated());

    Ok(InspectDto {
        name,
        root: root.display().to_string(),
        matches,
        already_registered,
    })
}

#[tauri::command]
pub async fn scan_path(
    state: State<'_, AppState>,
    path: String,
    prune_nested: Option<bool>,
) -> IpcResult<ScanResultDto> {
    let root = PathBuf::from(&path);
    if !root.is_dir() {
        return Err(DeckError::UnreadableDirectory { path: root }.into());
    }

    // Remember the root before scanning. These are the durable seeds: given
    // them, the entire library can be rebuilt, so the registry is
    // reconstructible rather than irreplaceable.
    state.store.remember_scan_root(&root).await?;

    let options = ScanOptions {
        prune_nested: prune_nested.unwrap_or(true),
        ..Default::default()
    };
    // The walk is pure filesystem work; keep it off the IPC thread.
    let registry = &state.registry;
    let result = scan(&root, registry, &options);

    let mut hits = Vec::with_capacity(result.drafts.len());
    for draft in &result.drafts {
        let taken = state.store.project_by_root(&draft.root).await?.is_some();
        hits.push(ScanHitDto::build(draft, taken));
    }

    Ok(ScanResultDto {
        hits,
        directories_visited: result.directories_visited,
        truncated: result.truncated,
    })
}

// ---- Registration and editing ------------------------------------------------

#[tauri::command]
pub async fn register_project(
    state: State<'_, AppState>,
    path: String,
    name: Option<String>,
    runner_id: Option<String>,
    custom_command: Option<String>,
) -> IpcResult<ProjectDto> {
    let root = PathBuf::from(&path);
    if !root.is_dir() {
        return Err(DeckError::UnreadableDirectory { path: root }.into());
    }

    // Re-detect at registration time; honour an explicit runner choice.
    let detected = match &runner_id {
        Some(id) => state
            .registry
            .detect_all(&root)
            .into_iter()
            .find(|m| &m.meta.id == id),
        None => state.registry.detect(&root),
    };

    let custom = custom_command.as_deref().map(str::trim).filter(|s| !s.is_empty());
    if detected.is_none() && custom.is_none() {
        return Err(DeckError::NoRunnerMatched { path: root }.into());
    }

    let now = Utc::now();
    let facts = detected.as_ref().map_or_else(
        || DetectedFacts {
            // No runner claimed it and the user supplied a command instead;
            // `custom` is a reserved id no manifest may use (validated ids
            // exclude nothing, but bundled ids are known -- document this).
            runner_id: "custom".to_owned(),
            language: "Custom".to_owned(),
            framework: None,
            package_manager: None,
            version: None,
            detected_at: now,
        },
        |m| DetectedFacts {
            runner_id: m.meta.id.clone(),
            language: m.meta.language.clone(),
            framework: m.meta.framework.clone(),
            package_manager: m.vars.get("pm").map(str::to_owned),
            version: m.version.clone(),
            detected_at: now,
        },
    );

    let mut project = Project {
        id: ProjectId::new(),
        name: name
            .map(|n| n.trim().to_owned())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| {
                root.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Untitled")
                    .to_owned()
            }),
        description: None,
        root,
        detected: facts,
        overrides: deck_domain::project::ProjectOverrides::default(),
        tags: Vec::new(),
        category: None,
        favorite: false,
        pinned: false,
        archived: false,
        restart_policy: deck_domain::project::RestartPolicy::default(),
        notes: None,
        created_at: now,
        updated_at: now,
        last_launched_at: None,
    };

    if let Some(line) = custom {
        let parsed = parse_command_line(line).ok_or_else(|| {
            IpcError::from(DeckError::Storage("run command is empty".to_owned()))
        })?;
        project.overrides.commands.insert(
            Lifecycle::Run,
            CommandSpec::Exec {
                program: parsed.program,
                args: parsed.args,
            },
        );
    }

    // Asking for it by hand overrides any earlier "never show me this".
    state
        .store
        .undismiss_root(&project.root.display().to_string())
        .await?;
    state.store.insert_project(&project).await?;
    tracing::info!(project = %project.name, root = %project.root.display(), "registered");
    Ok(ProjectDto::build(&project, &state.registry))
}

#[tauri::command]
pub async fn update_project(
    state: State<'_, AppState>,
    id: String,
    patch: ProjectPatch,
) -> IpcResult<ProjectDto> {
    let id = parse_id(&id)?;
    let mut project = state.store.project(id).await?;

    if let Some(name) = patch.name {
        let name = name.trim().to_owned();
        if !name.is_empty() {
            project.name = name;
        }
    }
    if let Some(description) = patch.description {
        project.description = description.filter(|d| !d.trim().is_empty());
    }
    if let Some(favorite) = patch.favorite {
        project.favorite = favorite;
    }
    if let Some(pinned) = patch.pinned {
        project.pinned = pinned;
    }
    if let Some(archived) = patch.archived {
        project.archived = archived;
    }
    if let Some(args) = patch.args {
        // Blank entries are what an empty input box produces; storing one
        // would append an empty argument to every launch.
        project.overrides.args = args
            .into_iter()
            .map(|a| a.trim().to_owned())
            .filter(|a| !a.is_empty())
            .collect();
    }
    if let Some(args) = patch.disabled_args {
        project.overrides.disabled_args = args
            .into_iter()
            .map(|a| a.trim().to_owned())
            .filter(|a| !a.is_empty())
            .collect();
    }

    if let Some(new_root) = patch.root {
        let root = PathBuf::from(new_root.trim());
        if !root.is_dir() {
            return Err(DeckError::UnreadableDirectory { path: root }.into());
        }
        project.root = root;
        // Re-detect after a move: a project relocated into a different layout
        // (or one that was re-scaffolded before being pointed at) can easily be
        // a different runner now, and keeping the old one would hand it a
        // command for a project that no longer exists.
        if let Some(found) = state.registry.detect(&project.root) {
            project.detected = DetectedFacts {
                runner_id: found.meta.id.clone(),
                language: found.meta.language.clone(),
                framework: found.meta.framework.clone(),
                package_manager: found.vars.get("pm").map(str::to_owned),
                version: found.version.clone(),
                detected_at: Utc::now(),
            };
        }
    }

    if let Some(surface) = patch.surface {
        // `Some(None)` clears; a config with every field empty is the same
        // statement as "no status source", so it normalises to None rather
        // than leaving a hollow object that renders an unwired chip.
        project.overrides.surface = surface
            .map(deck_domain::project::SurfaceConfig::from)
            .filter(|s| s.url.is_some() || s.probe_port.is_some() || s.external_log.is_some());
    }
    if let Some(tags) = patch.tags {
        project.tags = tags;
    }
    if let Some(category) = patch.category {
        project.category = category.filter(|c| !c.trim().is_empty());
    }
    if let Some(notes) = patch.notes {
        project.notes = notes.filter(|n| !n.trim().is_empty());
    }
    if let Some(run_command) = patch.run_command {
        match run_command.as_deref().and_then(parse_command_line) {
            Some(parsed) => {
                project.overrides.commands.insert(
                    Lifecycle::Run,
                    CommandSpec::Exec {
                        program: parsed.program,
                        args: parsed.args,
                    },
                );
            }
            None => {
                project.overrides.commands.remove(&Lifecycle::Run);
            }
        }
    }

    project.updated_at = Utc::now();
    state.store.update_project(&project).await?;
    Ok(ProjectDto::build(&project, &state.registry))
}

#[tauri::command]
pub async fn remove_project(state: State<'_, AppState>, id: String) -> IpcResult<()> {
    let id = parse_id(&id)?;
    let project = state.store.project(id).await?;

    // A live process must not outlive its registration.
    if state.supervisor.state(id).is_live() {
        state.supervisor.stop(id, StopMode::Immediate)?;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        while state.supervisor.state(id).is_live() && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    // Remember the folder BEFORE the row goes, or there is nothing left to
    // remember it from. Without this the ten-minute rescan finds the folder,
    // sees no row, and re-adds it -- which made removing junk pointless.
    let root = project.root.display().to_string();
    state.store.dismiss_root(&root).await?;

    state.store.delete_project(id).await?;
    // Logs are the project's private artefacts; remove them with it.
    let _ = std::fs::remove_dir_all(state.logs_root.join(id.to_hyphenated()));
    tracing::info!(root = %root, "removed and dismissed");
    Ok(())
}

// ---- Process control ----------------------------------------------------------

#[tauri::command]
pub async fn start_project(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    lifecycle: Option<String>,
) -> IpcResult<String> {
    let t_total = std::time::Instant::now();
    let id = parse_id(&id)?;
    let step = parse_step(lifecycle.as_deref())?;

    let t_lookup = std::time::Instant::now();
    let project = state.store.project(id).await?;
    let lookup = t_lookup.elapsed();

    let t_plan = std::time::Instant::now();
    let plan = deck_runners::plan(&project, &state.registry, step)?;
    let plan_ms = t_plan.elapsed();

    // ONE snapshot of the TCP table for both checks below. Enumerating it is a
    // syscall over every endpoint on the machine, and doing it twice per launch
    // was pure latency in front of the spawn.
    let t_ports = std::time::Instant::now();
    let taken = if plan.required_ports.is_empty() && plan.likely_ports.is_empty() {
        // Nothing to check means nothing to enumerate. Most projects declare no
        // ports at all, so this skips the syscall entirely for them.
        std::collections::HashSet::new()
    } else {
        deck_runtime::taken_ports()
    };
    let port_check = t_ports.elapsed();

    // A port the USER configured is a hard gate: we know the launch cannot
    // work, so refuse before spawning anything.
    if let Some(port) = deck_runtime::first_conflict_in(&taken, &plan.required_ports) {
        return Err(DeckError::PortInUse { port }.into());
    }

    // A port the RUNNER merely considers conventional only warns. The project
    // may not use it at all, and blocking on a guess would be worse than the
    // failure it is trying to pre-empt. If the port really was needed, the
    // child fails and `detect_exit_reason` names it in the log.
    let likely_conflict = deck_runtime::first_conflict_in(&taken, &plan.likely_ports);

    let request = SpawnRequest {
        project_id: id,
        project_name: project.name.clone(),
        lifecycle: step,
        command: plan.command,
        working_dir: plan.working_dir,
        env: plan.env,
        restart_policy: project.restart_policy,
        log_dir: state.logs_root.join(id.to_hyphenated()),
        runner_id: project.detected.runner_id.clone(),
        // Configured ports first; the runner's conventional list is the
        // fallback. Both feed readiness, which treats "listening anywhere" as
        // serving -- see `deck_runtime::health`.
        expected_ports: if plan.required_ports.is_empty() {
            plan.likely_ports.clone()
        } else {
            plan.required_ports.clone()
        },
    };

    let command_display = request.command.to_string();
    let t_spawn = std::time::Instant::now();
    let run_id = state.supervisor.start(request)?;
    let spawn = t_spawn.elapsed();

    state.timings.record_launch(crate::timings::LaunchTiming {
        project: project.name.clone(),
        lookup,
        plan: plan_ms,
        port_check,
        spawn,
        total: t_total.elapsed(),
    });

    if let Some(port) = likely_conflict {
        if let Some(sink) = state.supervisor.logs(id) {
            sink.push_deck(format!(
                "Note: port {port} is already in use by another process. \
                 If this project needs it, expect a bind failure."
            ));
        }
    }

    let record = RunRecord {
        run_id,
        project_id: id,
        lifecycle: step,
        command: command_display,
        started_at: Utc::now(),
        finished_at: None,
        exit_code: None,
        outcome: RunOutcome::Running,
        log_path: state
            .logs_root
            .join(id.to_hyphenated())
            .join(format!("{run_id}.log")),
    };
    state.store.insert_run(&record).await?;
    state.store.touch_last_launched(id, record.started_at).await?;

    pump::spawn_log_pump(&app, &state, id);
    Ok(run_id.to_string())
}

// Async on purpose: `Supervisor::stop` spawns its grace-period escalation task
// with `tokio::spawn`, which requires runtime context. A sync command runs on
// the IPC thread, outside the runtime, and would panic there.
#[tauri::command]
pub async fn stop_project(
    state: State<'_, AppState>,
    id: String,
    force: Option<bool>,
) -> IpcResult<()> {
    let id = parse_id(&id)?;
    let mode = if force.unwrap_or(false) {
        StopMode::Immediate
    } else {
        StopMode::default()
    };
    state.supervisor.stop(id, mode)?;
    Ok(())
}

#[tauri::command]
pub async fn restart_project(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> IpcResult<String> {
    let id = parse_id(&id)?;
    let project = state.store.project(id).await?;
    let plan = deck_runners::plan(&project, &state.registry, Lifecycle::Run)?;
    let request = SpawnRequest {
        project_id: id,
        project_name: project.name.clone(),
        lifecycle: Lifecycle::Run,
        command: plan.command,
        working_dir: plan.working_dir,
        env: plan.env,
        restart_policy: project.restart_policy,
        log_dir: state.logs_root.join(id.to_hyphenated()),
        runner_id: project.detected.runner_id.clone(),
        // Configured ports first; the runner's conventional list is the
        // fallback. Both feed readiness, which treats "listening anywhere" as
        // serving -- see `deck_runtime::health`.
        expected_ports: if plan.required_ports.is_empty() {
            plan.likely_ports.clone()
        } else {
            plan.required_ports.clone()
        },
    };
    let command_display = request.command.to_string();

    let run_id = state.supervisor.restart(id, request).await?;

    let record = RunRecord {
        run_id,
        project_id: id,
        lifecycle: Lifecycle::Run,
        command: command_display,
        started_at: Utc::now(),
        finished_at: None,
        exit_code: None,
        outcome: RunOutcome::Running,
        log_path: state
            .logs_root
            .join(id.to_hyphenated())
            .join(format!("{run_id}.log")),
    };
    state.store.insert_run(&record).await?;
    state.store.touch_last_launched(id, record.started_at).await?;

    pump::spawn_log_pump(&app, &state, id);
    Ok(run_id.to_string())
}

// ---- Desktop integration -------------------------------------------------------

#[tauri::command]
pub async fn open_project_folder(state: State<'_, AppState>, id: String) -> IpcResult<()> {
    let id = parse_id(&id)?;
    let project = state.store.project(id).await?;
    spawn_detached("explorer.exe", &[&project.root.display().to_string()], None)
        .map_err(|e| IpcError::from(DeckError::io("opening Explorer", e)))
}

#[tauri::command]
pub async fn open_project_terminal(state: State<'_, AppState>, id: String) -> IpcResult<()> {
    let id = parse_id(&id)?;
    let project = state.store.project(id).await?;
    let dir = project.effective_working_dir().display().to_string();

    // Windows Terminal when present, classic console host otherwise.
    if spawn_detached("wt.exe", &["-d", &dir], None).is_ok() {
        return Ok(());
    }
    spawn_detached("cmd.exe", &["/c", "start", "cmd.exe"], Some(Path::new(&dir)))
        .map_err(|e| IpcError::from(DeckError::io("opening a terminal", e)))
}

#[tauri::command]
pub async fn open_in_editor(state: State<'_, AppState>, id: String) -> IpcResult<()> {
    let id = parse_id(&id)?;
    let project = state.store.project(id).await?;
    let dir = project.root.display().to_string();
    // `code` is a .cmd shim, so it needs the shell host to resolve it.
    spawn_detached("cmd.exe", &["/c", "code", &dir], None).map_err(|_| {
        IpcError::from(DeckError::ProgramNotFound {
            program: "code".to_owned(),
        })
    })
}

#[tauri::command]
pub fn export_logs(state: State<'_, AppState>, id: String, dest: String) -> IpcResult<()> {
    let id = parse_id(&id)?;
    let sink = state
        .supervisor
        .logs(id)
        .ok_or_else(|| IpcError::from(DeckError::NotRunning {
            name: id.to_hyphenated(),
        }))?;
    sink.flush();
    std::fs::copy(sink.path(), &dest)
        .map(|_| ())
        .map_err(|e| IpcError::from(DeckError::io(format!("exporting log to {dest}"), e)))
}

/// The workspace roots the user has scanned.
#[tauri::command]
pub async fn scan_roots(state: State<'_, AppState>) -> IpcResult<Vec<String>> {
    Ok(state
        .store
        .scan_roots()
        .await?
        .iter()
        .map(|p| p.display().to_string())
        .collect())
}

/// Re-scans every remembered root and registers anything not already present.
///
/// The recovery path: if the registry is ever lost, this rebuilds it from the
/// roots without the user re-picking folders. Existing projects are left
/// untouched, so it is safe to run at any time -- it only ever adds.
///
/// Returns the number of projects added.
#[tauri::command]
pub async fn rescan_roots(state: State<'_, AppState>) -> IpcResult<usize> {
    let roots = state.store.scan_roots().await?;
    let mut added = 0usize;

    // Names already in use, so a silent add cannot produce two identical rows.
    // `Dev/Python/Chess-Scout` and `Dev/Rust/Chess-Scout` are both real, and
    // before this ran automatically the user was there to notice; now they are
    // not, so the second one is qualified with its parent directory.
    let mut names: std::collections::HashSet<String> = state
        .store
        .projects()
        .await?
        .into_iter()
        .map(|p| p.name)
        .collect();

    let dismissed = state.store.dismissed_roots().await?;
    let is_dismissed = |p: &std::path::Path| {
        let s = p.display().to_string();
        dismissed.iter().any(|d| d.eq_ignore_ascii_case(&s))
    };

    for root in roots {
        if !root.is_dir() {
            tracing::warn!(root = %root.display(), "remembered scan root no longer exists");
            continue;
        }
        let result = scan(&root, &state.registry, &ScanOptions::default());
        for draft in &result.drafts {
            if state.store.project_by_root(&draft.root).await?.is_some() {
                continue;
            }
            // Removed once means removed for good; see `dismissed_roots`.
            if is_dismissed(&draft.root) {
                continue;
            }
            let Some(facts) = draft.detected.as_ref() else {
                continue;
            };
            let name = if names.contains(&draft.name) {
                draft
                    .root
                    .parent()
                    .and_then(|p| p.file_name())
                    .map_or_else(|| draft.name.clone(), |d| format!("{} ({})", draft.name, d.to_string_lossy()))
            } else {
                draft.name.clone()
            };
            names.insert(name.clone());

            let now = Utc::now();
            let project = Project {
                id: ProjectId::new(),
                name,
                description: None,
                root: draft.root.clone(),
                detected: facts.clone(),
                overrides: deck_domain::project::ProjectOverrides::default(),
                tags: Vec::new(),
                category: None,
                favorite: false,
                pinned: false,
                archived: false,
                restart_policy: deck_domain::project::RestartPolicy::default(),
                notes: None,
                created_at: now,
                updated_at: now,
                last_launched_at: None,
            };
            match state.store.insert_project(&project).await {
                Ok(()) => added += 1,
                Err(e) => tracing::warn!(
                    project = %project.name, error = %e, "rescan could not register"
                ),
            }
        }
    }

    tracing::info!(added, "rescan complete");
    Ok(added)
}

/// The retained resource samples for a project, oldest first.
///
/// Used to backfill a sparkline when a panel opens mid-run; live updates arrive
/// on `deck://metrics`.
#[tauri::command]
pub fn project_metrics(state: State<'_, AppState>, id: String) -> IpcResult<Vec<MetricsDto>> {
    let id = parse_id(&id)?;
    Ok(state
        .sampler
        .history(id)
        .iter()
        .map(MetricsDto::from)
        .collect())
}

// ---- Icons ----------------------------------------------------------------------

/// The project's own icon as a data URL, if it has one.
///
/// Sources, best first: a shipped icon file (Tauri icon set, `icon.ico`,
/// favicon), else the icon embedded in a built executable. Cached per session;
/// extraction is bounded filesystem reads plus one Win32 call, never execution.
#[tauri::command]
pub async fn project_icon(state: State<'_, AppState>, id: String) -> IpcResult<Option<String>> {
    let id = parse_id(&id)?;
    if let Some(cached) = state.icons.lock().get(&id) {
        return Ok(cached.clone());
    }

    let project = state.store.project(id).await?;
    let root = project.root.clone();
    let name = project.name.clone();
    let data_url = tokio::task::spawn_blocking(move || extract_icon(&root, &name))
        .await
        .unwrap_or(None);

    state.icons.lock().insert(id, data_url.clone());
    Ok(data_url)
}

/// Every project's icon in one call.
///
/// The per-project command above was invoked once per row: with 29 projects
/// that is 29 IPC round-trips, 29 database reads and 29 filesystem probes, all
/// during first paint. This does one database read, probes only what is not
/// already cached, and does that probing on the blocking pool -- icon
/// extraction is synchronous filesystem work plus a Win32 call, and running it
/// on the async runtime stalls every other command behind it.
#[tauri::command]
pub async fn project_icons(
    state: State<'_, AppState>,
) -> IpcResult<std::collections::HashMap<String, Option<String>>> {
    let projects = state.store.projects().await?;

    // Split first so the cache is consulted without holding its lock across an
    // await -- the probe below is slow and would block every other reader.
    let mut out = std::collections::HashMap::with_capacity(projects.len());
    let mut to_probe = Vec::new();
    {
        let cache = state.icons.lock();
        for project in &projects {
            match cache.get(&project.id) {
                Some(cached) => {
                    out.insert(project.id.to_hyphenated(), cached.clone());
                }
                None => to_probe.push((project.id, project.root.clone(), project.name.clone())),
            }
        }
    }

    if !to_probe.is_empty() {
        let probed = tokio::task::spawn_blocking(move || {
            to_probe
                .into_iter()
                .map(|(id, root, name)| {
                    let icon = extract_icon(&root, &name);
                    (id, icon)
                })
                .collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default();

        let mut cache = state.icons.lock();
        for (id, data_url) in probed {
            cache.insert(id, data_url.clone());
            out.insert(id.to_hyphenated(), data_url);
        }
    }

    Ok(out)
}

/// Shared icon extraction. Bounded filesystem reads plus one Win32 call --
/// never execution, because this runs automatically over the whole library.
fn extract_icon(root: &std::path::Path, name: &str) -> Option<String> {
    use base64::Engine as _;

    deck_runtime::find_icon_source(root).and_then(|source| {
        match deck_runtime::load_icon(&source) {
            Ok((mime, bytes)) => Some(format!(
                "data:{mime};base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            )),
            Err(e) => {
                tracing::debug!(project = %name, error = %e, "icon extraction failed");
                None
            }
        }
    })
}

// ---- Settings -------------------------------------------------------------------

#[tauri::command]
pub async fn get_setting(state: State<'_, AppState>, key: String) -> IpcResult<Option<String>> {
    Ok(state.store.setting(&key).await?)
}

#[tauri::command]
pub async fn set_setting(state: State<'_, AppState>, key: String, value: String) -> IpcResult<()> {
    state.store.set_setting(&key, &value).await?;
    Ok(())
}

/// Spawns a detached desktop process (Explorer, a terminal, an editor).
///
/// Deliberately NOT supervised and NOT in a job object: these are the user's
/// own windows and must outlive Launch Deck.
fn spawn_detached(program: &str, args: &[&str], cwd: Option<&Path>) -> std::io::Result<()> {
    let mut cmd = std::process::Command::new(program);
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.spawn().map(|_| ())
}

// ---- Diagnostics ---------------------------------------------------------

/// Collects the developer diagnostics report.
///
/// Runs `PRAGMA integrity_check` and walks the log directory, so it is
/// deliberately on-demand only -- never polled.
#[tauri::command]
pub async fn diagnostics(
    state: State<'_, AppState>,
) -> IpcResult<crate::diagnostics::DiagnosticsReport> {
    Ok(crate::diagnostics::collect(&state).await)
}

// ---- Surfaces --------------------------------------------------------------

/// One reading of a project's status source.
///
/// Every field is `None` when its probe is not configured -- the tile renders
/// "not wired" for those, never a fabricated value. `checked_at` is carried so
/// the UI can say how stale a reading is instead of implying "now".
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceStatus {
    /// Something is accepting connections on the configured local port.
    pub port_open: Option<bool>,
    /// HTTP status the configured URL answered with, when it answered.
    pub http_status: Option<u16>,
    /// Request round-trip in whole milliseconds, when it answered.
    pub http_ms: Option<u32>,
    /// Transport-level failure text, when it did not answer at all.
    pub http_error: Option<String>,
    /// The `Content-Type` media type the URL returned (parameters stripped).
    /// The embed pane needs `text/html` — an API endpoint must not be framed
    /// as if it were a page.
    pub content_type: Option<String>,
    /// RFC 3339 mtime of the configured external log, when it exists.
    pub log_mtime: Option<String>,
    /// App-specific facts the tile can render alongside the reachability chip,
    /// only present when the project's surface has a `facts` reader configured.
    pub facts: Vec<deck_runtime::SurfaceFact>,
    /// When this reading was taken.
    pub checked_at: String,
}

/// Probes a project's configured status source, on demand.
///
/// The first network-touching command in the app. The rule from
/// `deck_runtime::surface` applies verbatim: only user-configured URLs, only
/// when the UI asks. The UI (row chips and the Today strip, sharing one
/// query cache) polls each wired tile once a minute while the window is up,
/// plus manual refresh -- the backend never schedules anything.
#[tauri::command]
pub async fn surface_status(state: State<'_, AppState>, id: String) -> IpcResult<SurfaceStatus> {
    let id = parse_id(&id)?;
    let project = state.store.project(id).await?;
    let Some(config) = project.overrides.surface else {
        // Nothing configured: an all-None reading, which the UI has no reason
        // to request and renders as fully unwired if it does.
        return Ok(SurfaceStatus {
            port_open: None,
            http_status: None,
            http_ms: None,
            http_error: None,
            content_type: None,
            log_mtime: None,
            facts: Vec::new(),
            checked_at: Utc::now().to_rfc3339(),
        });
    };

    // Blocking pool: the ping can legitimately take seconds against a dead
    // deployed URL, and that must never stall the IPC thread or a launch.
    let reading = tokio::task::spawn_blocking(move || {
        let port_open = config.probe_port.map(deck_runtime::probe_port);
        let (http_status, http_ms, http_error, content_type) = match config.url.as_deref() {
            None => (None, None, None, None),
            Some(url) => match deck_runtime::ping_url(url) {
                deck_runtime::PingOutcome::Responded { status, ms, content_type } => {
                    (Some(status), Some(ms), None, content_type)
                }
                deck_runtime::PingOutcome::Failed(reason) => (None, None, Some(reason), None),
            },
        };
        let log_mtime = config
            .external_log
            .as_deref()
            .and_then(deck_runtime::log_mtime)
            .map(|t| chrono::DateTime::<Utc>::from(t).to_rfc3339());
        // Facts are only fetched when the user chose a reader; the URL/log
        // fields the reader needs are exactly those already in `config`.
        let facts = config.facts.map_or_else(Vec::new, |src| {
            deck_runtime::read_facts(src, config.url.as_deref(), config.external_log.as_deref())
        });
        (port_open, http_status, http_ms, http_error, content_type, log_mtime, facts)
    })
    .await
    .map_err(|e| IpcError::from(DeckError::Storage(format!("surface probe panicked: {e}"))))?;

    let (port_open, http_status, http_ms, http_error, content_type, log_mtime, facts) = reading;
    Ok(SurfaceStatus {
        port_open,
        http_status,
        http_ms,
        http_error,
        content_type,
        log_mtime,
        facts,
        checked_at: Utc::now().to_rfc3339(),
    })
}

/// Opens the project's configured URL in the default browser.
///
/// Scheme-gated to http/https: this hands a string to the shell, and the gate
/// is what keeps a mistyped config from becoming "run an arbitrary protocol
/// handler". A URL that fails the gate is a config error worth saying, not
/// silently ignoring.
#[tauri::command]
pub async fn open_surface_url(state: State<'_, AppState>, id: String) -> IpcResult<()> {
    let id = parse_id(&id)?;
    let project = state.store.project(id).await?;
    let url = project
        .overrides
        .surface
        .and_then(|s| s.url)
        .ok_or_else(|| IpcError::from(DeckError::Storage("no URL configured".to_owned())))?;
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(DeckError::Storage(format!(
            "refusing to open `{url}` -- only http(s) URLs are opened"
        ))
        .into());
    }
    spawn_detached("explorer.exe", &[&url], None)
        .map_err(|e| IpcError::from(DeckError::io("opening the browser", e)))
}

// ---- Today strip -----------------------------------------------------------

/// Failures across every project in the last 24 hours, newest first.
///
/// The morning question -- "what died while I was away" -- answered from run
/// history in one query. Stops and kills are excluded at the store layer:
/// a shutdown the user asked for is not a death.
#[tauri::command]
pub async fn overnight_report(state: State<'_, AppState>) -> IpcResult<Vec<RunRecordDto>> {
    let since = Utc::now() - chrono::Duration::hours(24);
    let records = state.store.failures_since(since, 50).await?;
    Ok(records.iter().map(RunRecordDto::from).collect())
}

/// The most recent runs across every project, newest first.
///
/// The dashboard's activity feed. Capped at 40 -- a feed nobody scrolls past
/// forty rows of is a feed, and an unbounded one is a memory leak with a
/// scrollbar.
#[tauri::command]
pub async fn recent_runs(state: State<'_, AppState>) -> IpcResult<Vec<RunRecordDto>> {
    let records = state.store.recent_runs(40).await?;
    Ok(records.iter().map(RunRecordDto::from).collect())
}

/// Opens the project's configured URL in an app window instead of the
/// browser -- the "embed my apps" path. Same http(s) gate as
/// `open_surface_url`; the created window gets no IPC capabilities, so a
/// remote page can render but cannot reach into Launch Deck.
#[tauri::command]
pub async fn open_surface_window(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> IpcResult<()> {
    let pid = parse_id(&id)?;
    let project = state.store.project(pid).await?;
    let url = project
        .overrides
        .surface
        .and_then(|s| s.url)
        .ok_or_else(|| IpcError::from(DeckError::Storage("no URL configured".to_owned())))?;
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(DeckError::Storage(format!(
            "refusing to open `{url}` -- only http(s) URLs are opened"
        ))
        .into());
    }

    // One embed window per project: focus the existing one on repeat opens
    // rather than stacking duplicates.
    let label = format!("embed-{}", pid.to_hyphenated());
    {
        use tauri::Manager;
        if let Some(existing) = app.get_webview_window(&label) {
            let _ = existing.set_focus();
            return Ok(());
        }
    }

    let parsed = url
        .parse()
        .map_err(|e| IpcError::from(DeckError::Storage(format!("bad URL `{url}`: {e}"))))?;
    tauri::WebviewWindowBuilder::new(&app, &label, tauri::WebviewUrl::External(parsed))
        .title(format!("{} — Launch Deck", project.name))
        .inner_size(1100.0, 750.0)
        .build()
        .map_err(|e| IpcError::from(DeckError::Storage(format!("opening app window: {e}"))))?;
    Ok(())
}

// ---- Prompt Studio ---------------------------------------------------------

/// A project's context card: `LAUNCHDECK.md` at the repo root.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextCardDto {
    /// Absolute path the card lives at (whether or not it exists yet).
    pub path: String,
    pub exists: bool,
    /// File content, empty when the card does not exist.
    pub content: String,
}

/// One source's worth of recent error lines for the debug prompt.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorTailDto {
    /// Where the lines came from, in words the prompt can carry verbatim
    /// ("live console", a log file name, an external log path).
    pub source: String,
    pub lines: Vec<String>,
    /// Whether the lines actually carried error marks. `false` means this is
    /// a plain raw tail, and the UI/prompt must call it that -- never
    /// "errors".
    pub matched: bool,
}

/// How much of a log file the error tail reads, from the end.
const TAIL_BUDGET: u64 = 64 * 1024;
/// Ceiling on error lines per source; beyond this a prompt stops informing
/// and starts drowning.
const TAIL_MAX_LINES: usize = 40;

/// Reads the project's `LAUNCHDECK.md`, existing or not.
#[tauri::command]
pub async fn context_card(state: State<'_, AppState>, id: String) -> IpcResult<ContextCardDto> {
    let id = parse_id(&id)?;
    let project = state.store.project(id).await?;
    let path = project.root.join("LAUNCHDECK.md");
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    Ok(ContextCardDto {
        path: path.display().to_string(),
        exists: path.is_file(),
        content,
    })
}

/// Writes the project's `LAUNCHDECK.md` wholesale.
///
/// The card is plain markdown edited by a human (or a Claude session working
/// in that repo); the dashboard stores it verbatim and never parses it.
#[tauri::command]
pub async fn save_context_card(
    state: State<'_, AppState>,
    id: String,
    content: String,
) -> IpcResult<()> {
    let id = parse_id(&id)?;
    let project = state.store.project(id).await?;
    let path = project.root.join("LAUNCHDECK.md");
    std::fs::write(&path, content)
        .map_err(|e| IpcError::from(DeckError::io("writing the context card", e)))
}

/// Collects recent error-ish lines from every log this project has.
///
/// Sources, most current first: the live console when the supervisor is
/// running the app, otherwise the newest run log file on disk; plus the
/// configured external log, for apps that run outside the supervisor
/// entirely (Fleet writes `fleet.log` on its own). Each source is labeled so
/// the generated prompt says where its evidence came from.
#[tauri::command]
pub async fn error_tail(state: State<'_, AppState>, id: String) -> IpcResult<Vec<ErrorTailDto>> {
    let id = parse_id(&id)?;
    let project = state.store.project(id).await?;

    let mut out = Vec::new();

    if let Some(sink) = state.supervisor.logs(id) {
        let text: String = sink
            .snapshot()
            .iter()
            .map(|l| format!("{}\n", strip_ansi(&l.text)))
            .collect();
        let reading = deck_runtime::error_lines(&text, TAIL_MAX_LINES);
        if !reading.lines.is_empty() {
            out.push(ErrorTailDto {
                source: "live console".to_owned(),
                lines: reading.lines,
                matched: reading.matched,
            });
        }
    } else if let Some(file) = deck_runtime::newest_file(&state.logs_root.join(id.to_hyphenated()))
    {
        if let Some(text) = deck_runtime::read_tail(&file, TAIL_BUDGET) {
            let reading = deck_runtime::error_lines(&strip_ansi(&text), TAIL_MAX_LINES);
            if !reading.lines.is_empty() {
                let name = file.file_name().map_or_else(
                    || "last run log".to_owned(),
                    |n| n.to_string_lossy().into_owned(),
                );
                out.push(ErrorTailDto {
                    source: format!("last run log, {name}"),
                    lines: reading.lines,
                    matched: reading.matched,
                });
            }
        }
    }

    if let Some(external) = project.overrides.surface.and_then(|s| s.external_log) {
        if let Some(text) = deck_runtime::read_tail(&external, TAIL_BUDGET) {
            let reading = deck_runtime::error_lines(&strip_ansi(&text), TAIL_MAX_LINES);
            if !reading.lines.is_empty() {
                out.push(ErrorTailDto {
                    source: external.display().to_string(),
                    lines: reading.lines,
                    matched: reading.matched,
                });
            }
        }
    }

    Ok(out)
}

/// Drops ANSI escape sequences so log lines read clean inside a prompt.
///
/// A minimal CSI/OSC skipper, not a terminal emulator: colour and cursor
/// codes vanish, everything else passes through.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            // CSI: ESC [ ... final byte in @..~
            Some('[') => {
                chars.next();
                for f in chars.by_ref() {
                    if ('@'..='~').contains(&f) {
                        break;
                    }
                }
            }
            // OSC: ESC ] ... BEL (or the ESC of ESC \)
            Some(']') => {
                chars.next();
                for f in chars.by_ref() {
                    if f == '\u{7}' || f == '\u{1b}' {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Appends a hand-off prompt to the repo's `NEXT.md` and returns the path.
///
/// Append, never replace: `NEXT.md` is Fleet's backlog convention, and the
/// backlog above the hand-off belongs to its repo, not to this button.
#[tauri::command]
pub async fn hand_off(state: State<'_, AppState>, id: String, prompt: String) -> IpcResult<String> {
    let id = parse_id(&id)?;
    let project = state.store.project(id).await?;
    let path = project.root.join("NEXT.md");
    let existing = std::fs::read_to_string(&path).ok();
    let stamp = Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
    let merged = deck_runtime::append_handoff(existing.as_deref(), &prompt, &stamp);
    std::fs::write(&path, merged)
        .map_err(|e| IpcError::from(DeckError::io("writing NEXT.md", e)))?;
    Ok(path.display().to_string())
}

/// Opens a terminal in the project's repo with `claude` running and the
/// prompt loaded.
///
/// The prompt goes through a temp file, never through a command line -- a
/// multi-kilobyte prompt with every kind of quote in it must not meet shell
/// parsing. The launched PowerShell also puts the prompt on the clipboard
/// first, so even if `claude` fails to start the prompt is one Ctrl+V away;
/// the native `claude.exe` is preferred because its argv carries multiline
/// text intact where the npm shim may not.
#[tauri::command]
pub async fn open_claude_terminal(
    state: State<'_, AppState>,
    id: String,
    prompt: String,
) -> IpcResult<()> {
    let id = parse_id(&id)?;
    let project = state.store.project(id).await?;
    let dir = project.effective_working_dir().clone();

    let file = std::env::temp_dir().join(format!("deck-prompt-{}.md", id.to_hyphenated()));
    std::fs::write(&file, &prompt)
        .map_err(|e| IpcError::from(DeckError::io("writing the prompt file", e)))?;

    // Single-quoted PowerShell literal: the path is ours (temp dir + uuid),
    // no quotes in it by construction.
    let script = format!(
        "$p = Get-Content -Raw '{file}'; Set-Clipboard -Value $p; \
         $exe = Get-Command claude.exe -ErrorAction SilentlyContinue; \
         if ($exe) {{ & $exe.Source $p }} else {{ claude $p }}",
        file = file.display()
    );

    // CREATE_NEW_CONSOLE: the shell gets its own window (hosted by Windows
    // Terminal when that is the default), with no cmd/start quoting layer
    // between us and PowerShell's argv.
    let mut cmd = std::process::Command::new("powershell.exe");
    cmd.args(["-NoLogo", "-NoExit", "-Command", &script]);
    cmd.current_dir(&dir);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0000_0010); // CREATE_NEW_CONSOLE
    }
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| IpcError::from(DeckError::io("opening the Claude terminal", e)))
}

// ---- Setup readiness ------------------------------------------------------

/// Whether a project's dependencies are installed, and how to fix it.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupReport {
    /// Project this describes.
    pub project_id: String,
    /// True when launching is expected to fail for a knowable reason.
    pub needs_setup: bool,
    /// One line naming what is missing.
    pub hint: Option<String>,
    /// The path whose absence triggered the finding.
    pub missing: Option<String>,
    /// The exact command that would fix it, for display on the button.
    pub fix_command: Option<String>,
    /// False when the project directory is gone entirely, which no install
    /// fixes and which must not be offered as if it were the same problem.
    pub root_exists: bool,

    /// What stops this project launching, if anything.
    ///
    /// `None` means Run is expected to work. Otherwise one of:
    /// `"root"` (the folder is gone), `"setup"` (dependencies missing),
    /// `"program"` (the command's program is not installed or not on PATH),
    /// `"entry"` (the command names a file the project does not have),
    /// `"ambiguous"` (cargo has several binaries and no default), or
    /// `"unbuilt"` (a built artefact the runner launches does not exist yet).
    ///
    /// Deliberately a small closed set rather than a free-form severity: each
    /// value maps to one repair the UI can actually offer, and a category with
    /// no repair behind it is a label, not a diagnosis.
    pub blocker: Option<String>,

    /// One sentence naming exactly what is wrong, written for the row.
    pub reason: Option<String>,

    /// For `"ambiguous"`, the binaries to choose between. Empty otherwise.
    pub choices: Vec<String>,

    /// Exit code of the most recent run, when that run FAILED.
    ///
    /// Static checks cannot see everything: Transcriber's `import whisper`
    /// fails at runtime and no filesystem probe can know it without executing
    /// the project, which this deliberately never does. So the last outcome
    /// stands in -- it is evidence rather than prediction, and it is labelled
    /// as such in the UI ("last run failed"), never as "this will fail".
    pub last_failed_exit: Option<i32>,
}

/// Reports setup readiness for every registered project.
///
/// Purely filesystem checks -- the same read-only discipline as detection.
/// Nothing here executes a project's code, because this runs automatically over
/// the whole library and a probe that could execute would turn "open the app"
/// into "run whatever 29 folders say".
#[tauri::command]
pub async fn setup_reports(state: State<'_, AppState>) -> IpcResult<Vec<SetupReport>> {
    let projects = state.store.projects().await?;
    let mut out = Vec::with_capacity(projects.len());

    // One query for the whole library; see `latest_run_per_project`.
    let last_failures: std::collections::HashMap<ProjectId, i32> = state
        .store
        .latest_run_per_project()
        .await?
        .into_iter()
        .filter(|r| r.outcome == RunOutcome::Failed)
        .filter_map(|r| r.exit_code.map(|code| (r.project_id, code)))
        .collect();

    for project in projects {
        let root = std::path::Path::new(&project.root);
        let root_exists = root.is_dir();

        let Some(runner) = state.registry.get(&project.detected.runner_id) else {
            out.push(SetupReport {
                project_id: project.id.to_hyphenated(),
                needs_setup: false,
                hint: None,
                missing: None,
                fix_command: None,
                root_exists,
                last_failed_exit: last_failures.get(&project.id).copied(),
                blocker: if root_exists { None } else { Some("root".to_owned()) },
                reason: if root_exists {
                    None
                } else {
                    Some(format!("The folder is gone: {}", project.root.display()))
                },
                choices: Vec::new(),
            });
            continue;
        };

        let state_of_setup = if root_exists {
            // Globs allowed, because some markers have no fixed name: a
            // packaged Python project proves it is installed with
            // `<something>.egg-info`, and only the suffix is knowable.
            let exists = |relative: &str| {
                if !relative.contains('*') {
                    return root.join(relative).exists();
                }
                let (dir, pattern) = deck_runners::glob::split_dir(relative);
                let base = dir.map_or_else(|| root.to_path_buf(), |d| root.join(d));
                std::fs::read_dir(base).is_ok_and(|entries| {
                    entries.filter_map(Result::ok).any(|e| {
                        deck_runners::glob::matches(&e.file_name().to_string_lossy(), pattern)
                    })
                })
            };
            runner.manifest().setup_state(&exists)
        } else {
            deck_domain::manifest::SetupState::Ready
        };

        let (hint, missing) = match &state_of_setup {
            deck_domain::manifest::SetupState::NeedsSetup { hint, missing } => {
                (Some(hint.clone()), Some(missing.clone()))
            }
            deck_domain::manifest::SetupState::Ready => (None, None),
        };

        // The install command has existed in every manifest from the start; it
        // simply had no way to reach the user.
        let fix_command = if state_of_setup.needs_setup() {
            // `vars_for` re-runs the manifest's variable probes against the
            // root, which is how `{pm}` becomes the project's actual package
            // manager rather than a hardcoded "npm" -- a pnpm project told to
            // run `npm install` is worse than no button at all.
            let vars = runner.vars_for(root);
            deck_domain::runner::Runner::command(
                runner,
                deck_domain::command::Lifecycle::Install,
                &vars,
            )
            .ok()
            .map(|c| c.to_string())
        } else {
            None
        };

        // ---- The verdict --------------------------------------------------
        //
        // Ordered by what the user should fix FIRST, not by what is easiest to
        // detect. A project with no `node_modules` also has no `vite` on PATH;
        // reporting the missing program there would send them chasing an
        // install of vite when the answer is `npm install`. So setup wins over
        // program, and the folder being gone wins over everything.
        let (blocker, reason, choices) = if !root_exists {
            (
                Some("root"),
                Some(format!("The folder is gone: {}", project.root.display())),
                Vec::new(),
            )
        } else if state_of_setup.needs_setup() {
            (
                Some("setup"),
                hint.clone(),
                Vec::new(),
            )
        } else {
            // Resolve the command Run would really use -- overrides included,
            // which is why this goes through `plan` rather than the manifest.
            match deck_runners::plan(&project, &state.registry, Lifecycle::Run) {
                Err(_) => (None, None, Vec::new()),
                Ok(p) => {
                    let program = p.command.program.clone();
                    let args = p.command.args.clone();
                    if deck_runtime::resolve_program(&program).is_none() {
                        // Three different failures wear the same shape here,
                        // and saying "not on PATH" for all of them sent the
                        // user looking in the wrong place. Pulse resolved to
                        // `D:\Workspace\Dev\Rust\Pulse/` -- a PATH lookup
                        // was never going to find that, and the real answer is
                        // that the app has not been built.
                        let trailing = program.ends_with('/') || program.ends_with('\\');
                        let is_path = program.contains('/')
                            || program.contains('\\')
                            || program.contains(':');
                        if trailing {
                            // A runner variable resolved to nothing: the built
                            // binary this manifest launches is not there yet.
                            // `tauri-built` documents this case explicitly and
                            // leaves the path bare rather than guessing at a
                            // binary, which is how Pulse once launched its CLI
                            // and called it a successful app start.
                            (
                                Some("unbuilt"),
                                Some("This project has not been built yet".to_owned()),
                                Vec::new(),
                            )
                        } else if is_path {
                            (
                                Some("program"),
                                Some(format!("`{program}` does not exist")),
                                Vec::new(),
                            )
                        } else {
                            (
                                Some("program"),
                                Some(format!(
                                    "`{program}` is not installed, or not on PATH"
                                )),
                                Vec::new(),
                            )
                        }
                    } else if let Some(missing_file) =
                        deck_runtime::missing_entry(root, &args)
                    {
                        (
                            Some("entry"),
                            Some(format!(
                                "`{missing_file}` does not exist in this project"
                            )),
                            Vec::new(),
                        )
                    } else if let Some(bins) =
                        deck_runtime::cargo_ambiguity(root, &program, &args)
                    {
                        (
                            Some("ambiguous"),
                            Some(format!(
                                "{} binaries here and no default -- pick one",
                                bins.len()
                            )),
                            bins,
                        )
                    } else {
                        (None, None, Vec::new())
                    }
                }
            }
        };

        out.push(SetupReport {
            project_id: project.id.to_hyphenated(),
            needs_setup: state_of_setup.needs_setup(),
            hint,
            missing,
            fix_command,
            root_exists,
            last_failed_exit: last_failures.get(&project.id).copied(),
            blocker: blocker.map(str::to_owned),
            reason,
            choices,
        });
    }

    Ok(out)
}

/// Called once by the frontend when the project list has first painted.
///
/// This exists because every other boot figure measures something the user does
/// not wait for: the window appears before `setup()` finishes, and CDP-based
/// timing distorts WebView2 startup. This is the honest number.
#[tauri::command]
pub async fn ui_ready(
    state: State<'_, AppState>,
    marks: crate::timings::FrontendMarks,
) -> IpcResult<()> {
    let elapsed = state.timings.process_start.elapsed();
    state.timings.set_first_paint(elapsed, marks);
    // Logged as well as stored so the figure can be read from a run with NO
    // debugging port attached -- which is the only run that reflects a real
    // boot, since enabling CDP measurably slows WebView2 startup.
    tracing::info!(
        ms = elapsed.as_millis(),
        dom_interactive = marks.dom_interactive_ms,
        script_eval = marks.script_eval_ms,
        react_mount = marks.react_mount_ms,
        fetch_start = marks.fetch_start_ms,
        data = marks.data_arrived_ms,
        painted = marks.painted_ms,
        "ui ready: first project rows painted"
    );
    Ok(())
}

/// Warms the PATH-resolution cache for a project's program.
///
/// Called when the user selects a row, and for the few most recently launched
/// projects at startup. Resolving a bare name like `npm` walks all ~43 PATH
/// entries against every PATHEXT suffix -- a measured **2.2-4.2 ms on every
/// launch** of an interpreter-based project -- and the answer cannot change
/// inside a process, so paying it once here makes the eventual launch cheaper.
///
/// This used to also read the program's bytes into the OS file cache. Cut in
/// the 2026-08 re-scope: the byte-read's own controlled measurement (fresh
/// copies of one binary, arms alternating) put its worth at **6% of a cold
/// launch**, and an in-app A/B across sixteen projects found no difference.
/// The resolve half is the part that ever paid.
///
/// Never executes anything; resolving is a filesystem lookup. Errors are
/// swallowed -- a project whose program cannot be resolved has a real error to
/// report at launch, from a code path that can explain it properly.
#[tauri::command]
pub async fn prewarm_project(state: State<'_, AppState>, id: String) -> IpcResult<()> {
    let id = parse_id(&id)?;
    let Ok(project) = state.store.project(id).await else {
        return Ok(());
    };
    // Through `plan`, not the raw manifest: the program actually launched
    // depends on the resolved variables and any command the user overrode, and
    // warming a different name than the one that runs would be a pure cost.
    let Ok(plan) = deck_runners::plan(&project, &state.registry, Lifecycle::Run) else {
        return Ok(());
    };
    let _ = deck_runtime::resolve_program(&plan.command.program);
    Ok(())
}

/// Re-runs detection on an existing project and stores the result.
///
/// Without this, a project's runner is frozen at the moment it was registered.
/// That matters because runners are DATA: dropping a new `.toml` into the
/// runners directory is the supported way to teach Launch Deck something, and
/// until now a manifest that would have matched better could never reach a
/// project already in the library. `rescan_roots` does not help -- it looks for
/// projects that are not registered yet.
///
/// The concrete case: `tauri-built` launches an already-compiled Tauri app
/// instead of running `npm run tauri dev`, which rebuilds from source and took
/// over a minute. Every existing Tauri project stayed on the slow path.
///
/// Preserves everything the user set. Only the detected runner and its derived
/// fields change; name, tags, favourite, notes and any custom command survive,
/// because re-detection is a correction to OUR guess, not a reset of THEIR
/// configuration.
#[tauri::command]
pub async fn redetect_project(state: State<'_, AppState>, id: String) -> IpcResult<ProjectDto> {
    let id = parse_id(&id)?;
    let mut project = state.store.project(id).await?;

    if !project.root.is_dir() {
        return Err(DeckError::UnreadableDirectory { path: project.root }.into());
    }

    let Some(detected) = state.registry.detect(&project.root) else {
        // Nothing matched. Leaving the previous detection in place is the safe
        // outcome: a project that stops being recognised should keep working
        // with what it had, not become unlaunchable.
        return Ok(ProjectDto::build(&project, &state.registry));
    };

    let previous = project.detected.runner_id.clone();
    // Built the same way `register_project` builds it, so a re-detected project
    // is indistinguishable from a freshly registered one.
    project.detected = deck_domain::project::DetectedFacts {
        runner_id: detected.meta.id.clone(),
        language: detected.meta.language.clone(),
        framework: detected.meta.framework.clone(),
        package_manager: detected.vars.get("pm").map(str::to_owned),
        version: detected.version.clone(),
        detected_at: Utc::now(),
    };
    state.store.update_project(&project).await?;

    if previous != project.detected.runner_id {
        tracing::info!(
            project = %project.name,
            from = %previous,
            to = %project.detected.runner_id,
            "re-detected runner"
        );
    }

    Ok(ProjectDto::build(&project, &state.registry))
}

/// Whether `requested` really sits inside one of `roots`.
///
/// # Why this canonicalises instead of comparing components
///
/// `Path::starts_with` compares whole components, which correctly rejects
/// `D:\Work\App2` as "inside" `D:\Work\App` -- a plain string prefix would
/// not. That much the first version got right, and it is why the guard looked
/// sound.
///
/// What it cannot do is resolve `..`, because Rust deliberately does not
/// normalise `Component::ParentDir` without touching the filesystem (the
/// parent of a symlink is not its lexical parent). So
/// `<project>\..\..\..\CLAUDE.md` passed the check: every component of the
/// root is present, and the `..` components are just more components after
/// them. A review measured it resolving to `D:\Workspace\CLAUDE.md` and
/// enumerating `D:\Workspace\Career\` -- personal files, outside every
/// project -- through a guard whose comment claimed the drill-down "cannot be
/// steered somewhere else entirely".
///
/// Canonicalising both sides resolves `..` and symlinks against the real
/// filesystem, and does it on BOTH sides so the `\\?\` prefix Windows adds
/// is present in each. A path that cannot be canonicalised does not exist, and
/// is refused rather than assumed innocent.
fn within_roots(roots: &[PathBuf], requested: &Path) -> bool {
    let Ok(real) = std::fs::canonicalize(requested) else {
        return false;
    };
    roots
        .iter()
        .filter_map(|r| std::fs::canonicalize(r).ok())
        .any(|root| real.starts_with(&root))
}

// ---- Storage ---------------------------------------------------------------
//
// "What is eating the disk" is a question about the library as a whole, but it
// is answered one project at a time on purpose. A single sweep of every root
// takes minutes on a cold cache -- measured at over five on this workspace --
// and there is no honest progress bar for a walk whose size is unknown until
// it finishes. Per-project commands let the UI fire one query per row and fill
// the list in as answers land, so the screen is useful from the first result
// instead of blank until the last.

/// Disk usage of one project's root.
#[tauri::command]
pub async fn project_usage(
    state: State<'_, AppState>,
    id: String,
) -> IpcResult<deck_runtime::DirUsage> {
    let id = parse_id(&id)?;
    let project = state.store.project(id).await?;
    // Blocking pool: a cold walk is seconds of pure I/O, and the IPC thread
    // must stay free for the launches happening while it runs.
    tokio::task::spawn_blocking(move || deck_runtime::dir_usage(&project.root))
        .await
        .map_err(|e| IpcError::from(DeckError::Storage(format!("size walk panicked: {e}"))))
}

/// Immediate children of a directory, each measured, largest first.
///
/// # Why the path is checked against the registry
///
/// Every other command here takes a project id and resolves the path itself.
/// This one takes a path, because drilling from `target/` into `target/release`
/// needs to address a directory the registry has never heard of. So the path
/// is required to sit inside a registered project root: the drill-down can go
/// as deep as it likes, and cannot be steered somewhere else entirely.
#[tauri::command]
pub async fn folder_children(
    state: State<'_, AppState>,
    path: String,
) -> IpcResult<Vec<deck_runtime::DirChild>> {
    let requested = PathBuf::from(&path);
    let roots: Vec<PathBuf> = state
        .store
        .projects()
        .await?
        .into_iter()
        .map(|p| p.root)
        .collect();

    if !within_roots(&roots, &requested) {
        return Err(DeckError::Storage(format!(
            "refusing to list `{path}` -- it is not inside any registered project"
        ))
        .into());
    }

    tokio::task::spawn_blocking(move || deck_runtime::dir_children(&requested))
        .await
        .map_err(|e| IpcError::from(DeckError::Storage(format!("size walk panicked: {e}"))))
}

// ---- Manual ----------------------------------------------------------------

/// Opens a file with its default handler.
///
/// Gated to files inside a registered project root, for the same reason
/// `folder_children` is: this one takes a path rather than an id, because the
/// manual lists docs found by walking, and those paths are not in the
/// registry. The gate keeps "open the doc I just listed" from becoming "open
/// anything on this disk".
#[tauri::command]
pub async fn open_path(state: State<'_, AppState>, path: String) -> IpcResult<()> {
    let requested = PathBuf::from(&path);
    let roots: Vec<PathBuf> = state
        .store
        .projects()
        .await?
        .into_iter()
        .map(|p| p.root)
        .collect();
    if !within_roots(&roots, &requested) {
        return Err(DeckError::Storage(format!(
            "refusing to open `{path}` -- it is not inside any registered project"
        ))
        .into());
    }
    if !requested.is_file() {
        return Err(DeckError::Storage(format!("`{path}` is not a file")).into());
    }
    spawn_detached("explorer.exe", &[&path], None)
        .map_err(|e| IpcError::from(DeckError::io("opening the file", e)))
}

/// What a project says about itself: README summary, docs, declared scripts.
#[tauri::command]
pub async fn project_manual(
    state: State<'_, AppState>,
    id: String,
) -> IpcResult<deck_runtime::Manual> {
    let id = parse_id(&id)?;
    let project = state.store.project(id).await?;
    tokio::task::spawn_blocking(move || deck_runtime::read_manual(&project.root))
        .await
        .map_err(|e| IpcError::from(DeckError::Storage(format!("manual read panicked: {e}"))))
}

// ---- Web apps --------------------------------------------------------------

/// Saves a URL as a launchable project.
///
/// The folder it creates is a real one holding a real Windows `.url` shortcut,
/// so the entry is an artefact on disk rather than a row that exists only in
/// the database -- it survives a lost database, opens from Explorer, and is
/// detected by the `web` runner like any other project.
#[tauri::command]
pub async fn register_web_app(
    state: State<'_, AppState>,
    name: String,
    url: String,
) -> IpcResult<ProjectDto> {
    let url = url.trim().to_owned();
    // This string is interpolated into an INI file the shell parses, so a
    // control character in it is not a bad URL -- it is a second line. A URL
    // carrying CR or LF would inject arbitrary `[InternetShortcut]` keys
    // (`IconFile=`, `WorkingDirectory=`) into the shortcut. Rejected before
    // the scheme check, because "starts with http://" says nothing about
    // what follows.
    if url.chars().any(char::is_control) {
        return Err(DeckError::Storage(
            "refusing to save that address -- it contains a control character".to_owned(),
        )
        .into());
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(DeckError::Storage(format!(
            "refusing to save `{url}` -- only http(s) URLs are saved as web apps"
        ))
        .into());
    }
    let name = name.trim().to_owned();
    if name.is_empty() {
        return Err(DeckError::Storage("a web app needs a name".to_owned()).into());
    }

    let dir = state.data_dir.join("web-apps").join(slugify(&name));
    std::fs::create_dir_all(&dir).map_err(|e| {
        IpcError::from(DeckError::Storage(format!(
            "creating {}: {e}",
            dir.display()
        )))
    })?;
    // The .url format Windows has used since IE4. CRLF because Explorer's
    // parser is the audience, not a text editor.
    std::fs::write(dir.join("link.url"), format!("[InternetShortcut]\r\nURL={url}\r\n"))
        .map_err(|e| IpcError::from(DeckError::Storage(format!("writing the shortcut: {e}"))))?;

    let now = Utc::now();
    let project = Project {
        id: ProjectId::new(),
        name,
        description: None,
        root: dir,
        detected: DetectedFacts {
            runner_id: "web".to_owned(),
            language: "Web".to_owned(),
            framework: None,
            package_manager: None,
            version: None,
            detected_at: now,
        },
        overrides: deck_domain::project::ProjectOverrides {
            // Wired to itself: the URL is both what the tile watches and what
            // the embedded window opens, so a saved web app is watched from
            // the moment it is added rather than needing a second setup step.
            surface: Some(deck_domain::project::SurfaceConfig {
                url: Some(url),
                probe_port: None,
                external_log: None,
                facts: None,
            }),
            ..Default::default()
        },
        tags: Vec::new(),
        category: None,
        favorite: false,
        pinned: false,
        archived: false,
        restart_policy: deck_domain::project::RestartPolicy::default(),
        notes: None,
        created_at: now,
        updated_at: now,
        last_launched_at: None,
    };

    state.store.insert_project(&project).await?;
    tracing::info!(project = %project.name, "registered web app");
    Ok(ProjectDto::build(&project, &state.registry))
}

/// Folder-safe form of a name: lowercase, ASCII alphanumerics and dashes.
///
/// Collisions are fine and deliberate -- two apps named the same thing share a
/// folder and the second overwrites the first's shortcut, which is what the
/// name already said would happen.
fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            // Runs collapse to one separator. Without this, an em dash
            // surrounded by spaces contributes three dashes, and a name
            // typed with punctuation produces a folder full of them.
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-').to_owned();
    if trimmed.is_empty() { "web-app".to_owned() } else { trimmed }
}

#[cfg(test)]
mod web_app_tests {
    use super::slugify;

    #[test]
    fn slugify_makes_a_folder_name_out_of_anything() {
        assert_eq!(slugify("Vendsuite"), "vendsuite");
        assert_eq!(slugify("Etsy — Reset Sheet"), "etsy-reset-sheet");
        assert_eq!(slugify("a   b"), "a-b", "runs of separators collapse");
        assert_eq!(slugify("  spaces  "), "spaces");
        assert_eq!(slugify("///"), "web-app", "never an empty folder name");
        assert_eq!(slugify("A/B"), "a-b");
    }
}

#[cfg(test)]
mod gate_tests {
    use super::within_roots;
    use std::path::PathBuf;

    /// Creates `<tmp>/deck-gate-<n>/{root/inner, outside}` and returns the base.
    fn scratch(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "deck-gate-{tag}-{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(base.join("root").join("inner")).unwrap();
        std::fs::create_dir_all(base.join("outside")).unwrap();
        std::fs::write(base.join("outside").join("secret.txt"), "x").unwrap();
        std::fs::write(base.join("root").join("inner").join("ok.txt"), "x").unwrap();
        base
    }

    #[test]
    fn a_path_inside_a_root_is_allowed() {
        let base = scratch("in");
        let roots = vec![base.join("root")];
        assert!(within_roots(&roots, &base.join("root").join("inner")));
        assert!(within_roots(&roots, &base.join("root").join("inner").join("ok.txt")));
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The defect a review measured: `..` is a component like any other, so a
    /// component-wise `starts_with` walks straight out of the root.
    #[test]
    fn dot_dot_cannot_escape_the_root() {
        let base = scratch("escape");
        let roots = vec![base.join("root")];

        let escaped = base.join("root").join("..").join("outside").join("secret.txt");
        // Prove the old check would have allowed it, so this test fails if the
        // canonicalising guard is ever swapped back for a component compare.
        assert!(
            escaped.starts_with(&roots[0]),
            "the lexical check passes -- this is exactly why it was not enough"
        );
        assert!(!within_roots(&roots, &escaped), "canonicalised, it is outside");

        let up = base.join("root").join("..").join("..");
        assert!(!within_roots(&roots, &up));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_sibling_with_a_shared_prefix_is_still_rejected() {
        let base = scratch("sibling");
        std::fs::create_dir_all(base.join("root2")).unwrap();
        let roots = vec![base.join("root")];
        assert!(!within_roots(&roots, &base.join("root2")));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_path_that_does_not_exist_is_refused_rather_than_assumed() {
        let base = scratch("missing");
        let roots = vec![base.join("root")];
        assert!(!within_roots(&roots, &base.join("root").join("nope.txt")));
        assert!(!within_roots(&[], &base.join("root")));
        let _ = std::fs::remove_dir_all(&base);
    }
}
