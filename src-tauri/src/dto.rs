//! Wire types: what the frontend actually sees.
//!
//! Domain types stay snake_case Rust; everything here is camelCase JSON. The
//! conversion is one-way (domain -> DTO) except for the patch type, which is
//! the one shape the frontend writes back.

use chrono::{DateTime, Utc};
use deck_domain::command::Lifecycle;
use deck_domain::log::{LogLine, RunOutcome};
use deck_domain::project::{Project, ProjectDraft};
use deck_domain::runtime::{RunRecord, RunState};
use deck_runners::RunnerRegistry;
use serde::{Deserialize, Serialize};

fn ts(at: DateTime<Utc>) -> String {
    at.to_rfc3339()
}

/// A registered project, ready to render.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDto {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub root: String,
    pub runner_id: String,
    pub language: String,
    pub framework: Option<String>,
    /// Framework if known, else language -- the one line a card shows.
    pub kind_label: String,
    pub package_manager: Option<String>,
    pub version: Option<String>,
    pub tags: Vec<String>,
    pub category: Option<String>,
    pub favorite: bool,
    pub pinned: bool,
    pub archived: bool,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_launched_at: Option<String>,
    /// The exact command Run would execute right now, for display.
    pub run_command: Option<String>,
    /// Lifecycle steps available (runner's plus user overrides).
    pub supported: Vec<String>,
    /// The project's status source, when one is wired. See `SurfaceConfigDto`.
    pub surface: Option<SurfaceConfigDto>,
    /// Lucide glyph name from the runner manifest, for the fallback tile.
    pub glyph: Option<String>,

    /// Extra arguments appended to the run command, in order.
    pub args: Vec<String>,
    /// Arguments switched off but remembered.
    pub disabled_args: Vec<String>,

    /// How many times this has ever been launched from here.
    ///
    /// Stamped by `list_projects` after building, because the count lives in
    /// the runs table and this type is built from a `Project` alone. Zero for
    /// a project that has never run, which is true of 24 of the 51 in the real
    /// library and is exactly the group a launcher should show last.
    #[serde(default)]
    pub launch_count: u32,
}

impl ProjectDto {
    pub fn build(project: &Project, registry: &RunnerRegistry) -> Self {
        let run_command = deck_runners::plan(project, registry, Lifecycle::Run)
            .ok()
            .map(|p| p.command.to_string());

        // Steps the runner implements, plus any the user added by override.
        let mut supported: Vec<Lifecycle> = registry
            .get(&project.detected.runner_id)
            .map(|r| r.manifest().supported())
            .unwrap_or_default();
        for step in project.overrides.commands.keys() {
            if !supported.contains(step) {
                supported.push(*step);
            }
        }
        supported.sort();

        Self {
            id: project.id.to_hyphenated(),
            name: project.name.clone(),
            description: project.description.clone(),
            root: project.root.display().to_string(),
            runner_id: project.detected.runner_id.clone(),
            language: project.detected.language.clone(),
            framework: project.detected.framework.clone(),
            kind_label: project.kind_label().to_owned(),
            package_manager: project.detected.package_manager.clone(),
            version: project.detected.version.clone(),
            tags: project.tags.clone(),
            category: project.category.clone(),
            favorite: project.favorite,
            pinned: project.pinned,
            archived: project.archived,
            notes: project.notes.clone(),
            created_at: ts(project.created_at),
            updated_at: ts(project.updated_at),
            last_launched_at: project.last_launched_at.map(ts),
            run_command,
            args: project.overrides.args.clone(),
            disabled_args: project.overrides.disabled_args.clone(),
            launch_count: 0,
            supported: supported.iter().map(ToString::to_string).collect(),
            surface: project.overrides.surface.clone().map(SurfaceConfigDto::from),
            glyph: registry
                .get(&project.detected.runner_id)
                .and_then(|r| r.manifest().meta.icon.clone()),
        }
    }
}

/// The user-editable subset of a project.
///
/// `description`, `category` and `notes` distinguish "not sent" (leave alone)
/// from JSON `null` (clear): the double-Option pattern, decoded by
/// [`double_option`].
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPatch {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub description: Option<Option<String>>,
    #[serde(default)]
    pub favorite: Option<bool>,
    #[serde(default)]
    pub pinned: Option<bool>,
    #[serde(default)]
    pub archived: Option<bool>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub category: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub notes: Option<Option<String>>,
    /// Replacement run command, parsed from a user-typed line.
    /// `Some(None)` clears the override, restoring the runner default.
    #[serde(default, deserialize_with = "double_option")]
    pub run_command: Option<Option<String>>,
    /// `Some(None)` clears the status source; `Some(Some(_))` replaces it.
    #[serde(default, deserialize_with = "double_option")]
    pub surface: Option<Option<SurfaceConfigDto>>,

    /// Replaces the extra-argument list wholesale.
    #[serde(default)]
    pub args: Option<Vec<String>>,
    /// Replaces the switched-off argument list wholesale.
    #[serde(default)]
    pub disabled_args: Option<Vec<String>>,

    /// A new project root, for a folder that moved.
    ///
    /// The alternative was "remove it and add it again", which throws away the
    /// run history, the status source, the notes and the name -- everything
    /// that made it worth registering. A moved folder is the same project.
    #[serde(default)]
    pub root: Option<String>,

}

/// A status source over the wire. Mirrors `deck_domain::project::SurfaceConfig`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceConfigDto {
    pub url: Option<String>,
    pub probe_port: Option<u16>,
    pub external_log: Option<String>,
    /// Which app-specific reader turns this source into facts, if any.
    /// `#[serde(default)]` so a client that predates facts still deserialises.
    #[serde(default)]
    pub facts: Option<deck_domain::project::FactsSource>,
}

impl From<deck_domain::project::SurfaceConfig> for SurfaceConfigDto {
    fn from(s: deck_domain::project::SurfaceConfig) -> Self {
        Self {
            url: s.url,
            probe_port: s.probe_port,
            external_log: s.external_log.map(|p| p.display().to_string()),
            facts: s.facts,
        }
    }
}

impl From<SurfaceConfigDto> for deck_domain::project::SurfaceConfig {
    fn from(s: SurfaceConfigDto) -> Self {
        Self {
            url: s.url,
            probe_port: s.probe_port,
            external_log: s.external_log.map(std::path::PathBuf::from),
            facts: s.facts,
        }
    }
}

fn double_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Deserialize::deserialize(deserializer).map(Some)
}

/// Live run state, flattened for the wire.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunStateDto {
    pub tag: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub healthy: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
}

impl From<&RunState> for RunStateDto {
    fn from(state: &RunState) -> Self {
        let mut dto = Self {
            tag: state.tag().to_owned(),
            pid: state.pid(),
            started_at: None,
            healthy: None,
            code: None,
            at: None,
            reason: None,
            attempt: None,
        };
        match state {
            RunState::Running {
                started_at, healthy, ..
            } => {
                dto.started_at = Some(ts(*started_at));
                dto.healthy = *healthy;
            }
            RunState::Exited { code, at } => {
                dto.code = *code;
                dto.at = Some(ts(*at));
            }
            RunState::Crashed { code, at, reason } => {
                dto.code = *code;
                dto.at = Some(ts(*at));
                dto.reason.clone_from(reason);
            }
            RunState::Restarting { attempt, .. } => dto.attempt = Some(*attempt),
            RunState::Idle | RunState::Starting | RunState::Stopping { .. } => {}
        }
        dto
    }
}

/// One log line on the wire.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLineDto {
    pub seq: u64,
    pub stream: String,
    pub text: String,
    pub at: String,
}

impl From<&LogLine> for LogLineDto {
    fn from(line: &LogLine) -> Self {
        Self {
            seq: line.seq,
            stream: line.stream.as_str().to_owned(),
            text: line.text.clone(),
            at: ts(line.at),
        }
    }
}

/// Reply to a log fetch: the retained tail plus honesty about what is gone.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogsDto {
    pub lines: Vec<LogLineDto>,
    /// Lines evicted from the ring; the viewer says "N earlier lines".
    pub dropped: u64,
    pub total: u64,
}

/// A finished (or running) entry in run history.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRecordDto {
    pub run_id: String,
    /// Which project the run belongs to -- cross-project consumers (the
    /// Today strip) need it; per-project history simply ignores it.
    pub project_id: String,
    pub lifecycle: String,
    pub command: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub exit_code: Option<i32>,
    pub outcome: String,
    pub duration_ms: Option<i64>,
}

impl From<&RunRecord> for RunRecordDto {
    fn from(r: &RunRecord) -> Self {
        Self {
            run_id: r.run_id.to_string(),
            project_id: r.project_id.to_hyphenated(),
            lifecycle: r.lifecycle.to_string(),
            command: r.command.clone(),
            started_at: ts(r.started_at),
            finished_at: r.finished_at.map(ts),
            exit_code: r.exit_code,
            outcome: outcome_str(r.outcome).to_owned(),
            duration_ms: r.duration().map(|d| d.num_milliseconds()),
        }
    }
}

fn outcome_str(outcome: RunOutcome) -> &'static str {
    match outcome {
        RunOutcome::Running => "running",
        RunOutcome::Succeeded => "succeeded",
        RunOutcome::Failed => "failed",
        RunOutcome::Stopped => "stopped",
        RunOutcome::Killed => "killed",
        RunOutcome::FailedToStart => "failed_to_start",
        RunOutcome::Interrupted => "interrupted",
    }
}

/// What `inspect_path` returns: the automatic choice plus the alternatives.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectDto {
    pub name: String,
    pub root: String,
    /// Best match first; empty when nothing claimed the directory.
    pub matches: Vec<DetectionDto>,
    /// Already registered here? The flow offers to open it instead.
    pub already_registered: Option<String>,
}

/// One runner's claim on a directory.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectionDto {
    pub runner_id: String,
    pub runner_name: String,
    pub language: String,
    pub framework: Option<String>,
    pub version: Option<String>,
    /// The exact run command this choice would execute.
    pub proposed_run: Option<String>,
}

/// A scan hit, ready for the review list.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanHitDto {
    pub name: String,
    pub root: String,
    pub runner_id: Option<String>,
    pub kind_label: Option<String>,
    pub proposed_run: Option<String>,
    pub already_registered: bool,
}

impl ScanHitDto {
    pub fn build(draft: &ProjectDraft, already_registered: bool) -> Self {
        Self {
            name: draft.name.clone(),
            root: draft.root.display().to_string(),
            runner_id: draft.detected.as_ref().map(|d| d.runner_id.clone()),
            kind_label: draft
                .detected
                .as_ref()
                .map(|d| d.framework.clone().unwrap_or_else(|| d.language.clone())),
            proposed_run: draft.proposed_run.clone(),
            already_registered,
        }
    }
}

/// Scan results plus the honesty flags.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResultDto {
    pub hits: Vec<ScanHitDto>,
    pub directories_visited: usize,
    pub truncated: bool,
}

/// A resource sample on the wire.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsDto {
    pub project_id: String,
    pub pid: u32,
    pub at: String,
    /// Percentage of ONE core, so a four-core build reads ~400 rather than
    /// being clamped to 100 and hiding the interesting case.
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    pub process_count: u32,
    pub thread_count: u32,
    pub listening_ports: Vec<u16>,
    pub uptime_secs: u64,
}

impl From<&deck_domain::runtime::ProcessSnapshot> for MetricsDto {
    fn from(s: &deck_domain::runtime::ProcessSnapshot) -> Self {
        Self {
            project_id: s.project_id.to_hyphenated(),
            pid: s.pid,
            at: ts(s.at),
            cpu_percent: s.cpu_percent,
            memory_bytes: s.memory_bytes,
            process_count: s.process_count,
            thread_count: s.thread_count,
            listening_ports: s.listening_ports.clone(),
            uptime_secs: s.uptime_secs,
        }
    }
}

/// `deck://metrics` -- one batch of samples, one per running project.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsEvent {
    pub samples: Vec<MetricsDto>,
}

// ---- Event payloads --------------------------------------------------------

/// `deck://state` -- a project's run state changed.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateEvent {
    pub project_id: String,
    pub run_id: String,
    pub state: RunStateDto,
}

/// `deck://logs` -- a coalesced batch of log lines.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogsEvent {
    pub project_id: String,
    pub lines: Vec<LogLineDto>,
}

/// `deck://run-finished` -- a run reached a terminal outcome.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunFinishedEvent {
    pub project_id: String,
    pub run_id: String,
    pub outcome: String,
    pub exit_code: Option<i32>,
}

pub fn outcome_wire(outcome: RunOutcome) -> String {
    outcome_str(outcome).to_owned()
}
