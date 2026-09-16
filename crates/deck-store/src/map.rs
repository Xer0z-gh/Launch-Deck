//! Row ⇄ domain mapping. The only module allowed to know column names.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use deck_domain::command::Lifecycle;
use deck_domain::error::{DeckError, Result};
use deck_domain::log::RunOutcome;
use deck_domain::project::{DetectedFacts, Project, ProjectId, ProjectOverrides, RestartPolicy};
use deck_domain::runtime::{RunId, RunRecord};
use sqlx::sqlite::SqliteRow;
use sqlx::Row;

/// Formats a timestamp for storage.
pub(crate) fn ts(at: DateTime<Utc>) -> String {
    at.to_rfc3339()
}

/// Parses a stored timestamp.
pub(crate) fn parse_ts(raw: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| DeckError::Storage(format!("bad timestamp `{raw}`: {e}")))
}

/// Serialises a JSON-document column.
pub(crate) fn to_json<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|e| DeckError::Storage(e.to_string()))
}

/// Deserialises a JSON-document column.
pub(crate) fn from_json<T: serde::de::DeserializeOwned>(column: &str, raw: &str) -> Result<T> {
    serde_json::from_str(raw)
        .map_err(|e| DeckError::Storage(format!("bad JSON in `{column}`: {e}")))
}

/// Stored text form of a lifecycle step. Reuses `Display` ("run", "build", ...).
pub(crate) fn lifecycle_str(step: Lifecycle) -> String {
    step.to_string()
}

/// Parses a stored lifecycle step.
pub(crate) fn parse_lifecycle(raw: &str) -> Result<Lifecycle> {
    Lifecycle::ALL
        .into_iter()
        .find(|s| s.to_string() == raw)
        .ok_or_else(|| DeckError::Storage(format!("unknown lifecycle `{raw}`")))
}

/// Stored text form of a run outcome.
pub(crate) fn outcome_str(outcome: RunOutcome) -> &'static str {
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

/// Parses a stored run outcome.
pub(crate) fn parse_outcome(raw: &str) -> Result<RunOutcome> {
    Ok(match raw {
        "running" => RunOutcome::Running,
        "succeeded" => RunOutcome::Succeeded,
        "failed" => RunOutcome::Failed,
        "stopped" => RunOutcome::Stopped,
        "killed" => RunOutcome::Killed,
        "failed_to_start" => RunOutcome::FailedToStart,
        "interrupted" => RunOutcome::Interrupted,
        other => return Err(DeckError::Storage(format!("unknown outcome `{other}`"))),
    })
}

/// Rehydrates a project from its row.
pub(crate) fn project_from_row(row: &SqliteRow) -> Result<Project> {
    let id: String = row.get("id");
    let overrides_raw: String = row.get("overrides");
    let policy_raw: String = row.get("restart_policy");
    let tags_raw: String = row.get("tags");
    let last_launched: Option<String> = row.get("last_launched_at");

    Ok(Project {
        id: parse_project_id(&id)?,
        name: row.get("name"),
        description: row.get("description"),
        root: PathBuf::from(row.get::<String, _>("root")),
        detected: DetectedFacts {
            runner_id: row.get("runner_id"),
            language: row.get("language"),
            framework: row.get("framework"),
            package_manager: row.get("package_manager"),
            version: row.get("version"),
            detected_at: parse_ts(&row.get::<String, _>("detected_at"))?,
        },
        overrides: from_json::<ProjectOverrides>("overrides", &overrides_raw)?,
        tags: from_json::<Vec<String>>("tags", &tags_raw)?,
        category: row.get("category"),
        favorite: row.get::<i64, _>("favorite") != 0,
        pinned: row.get::<i64, _>("pinned") != 0,
        archived: row.get::<i64, _>("archived") != 0,
        restart_policy: from_json::<RestartPolicy>("restart_policy", &policy_raw)?,
        notes: row.get("notes"),
        created_at: parse_ts(&row.get::<String, _>("created_at"))?,
        updated_at: parse_ts(&row.get::<String, _>("updated_at"))?,
        last_launched_at: last_launched.as_deref().map(parse_ts).transpose()?,
    })
}

/// Rehydrates a run record from its row.
pub(crate) fn run_from_row(row: &SqliteRow) -> Result<RunRecord> {
    let run_id: String = row.get("id");
    let project_id: String = row.get("project_id");
    let finished: Option<String> = row.get("finished_at");

    Ok(RunRecord {
        run_id: run_id
            .parse::<RunId>()
            .map_err(|e| DeckError::Storage(format!("bad run id `{run_id}`: {e}")))?,
        project_id: parse_project_id(&project_id)?,
        lifecycle: parse_lifecycle(&row.get::<String, _>("lifecycle"))?,
        command: row.get("command"),
        started_at: parse_ts(&row.get::<String, _>("started_at"))?,
        finished_at: finished.as_deref().map(parse_ts).transpose()?,
        // Exit codes are written from an `i32`; the round-trip cannot truncate.
        #[allow(clippy::cast_possible_truncation)]
        exit_code: row.get::<Option<i64>, _>("exit_code").map(|c| c as i32),
        outcome: parse_outcome(&row.get::<String, _>("outcome"))?,
        log_path: PathBuf::from(row.get::<String, _>("log_path")),
    })
}

pub(crate) fn parse_project_id(raw: &str) -> Result<ProjectId> {
    raw.parse::<ProjectId>()
        .map_err(|e| DeckError::Storage(format!("bad project id `{raw}`: {e}")))
}
