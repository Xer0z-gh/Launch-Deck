//! Run-history repository: build history, crash history, and the pointer to
//! each run's log file on disk.

use chrono::{DateTime, Utc};
use deck_domain::error::Result;
use deck_domain::log::RunOutcome;
use deck_domain::project::ProjectId;
use deck_domain::runtime::{RunId, RunRecord};
use sqlx::Row;

use crate::map;
use crate::store::{db_err, Store};

impl Store {
    /// Records a run at the moment it starts.
    ///
    /// # Errors
    ///
    /// Returns [`deck_domain::DeckError::Storage`] on a database failure --
    /// including a run for a project id that does not exist, which the foreign
    /// key rejects.
    pub async fn insert_run(&self, record: &RunRecord) -> Result<()> {
        sqlx::query(
            "INSERT INTO runs (id, project_id, lifecycle, command, started_at,
                               finished_at, exit_code, outcome, log_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .bind(record.run_id.to_string())
        .bind(record.project_id.to_hyphenated())
        .bind(map::lifecycle_str(record.lifecycle))
        .bind(&record.command)
        .bind(map::ts(record.started_at))
        .bind(record.finished_at.map(map::ts))
        .bind(record.exit_code.map(i64::from))
        .bind(map::outcome_str(record.outcome))
        .bind(record.log_path.display().to_string())
        .execute(self.pool())
        .await
        .map_err(|e| db_err(&e))?;
        Ok(())
    }

    /// Closes out a run with its final disposition.
    ///
    /// Idempotent in effect: finishing an unknown run updates nothing and is
    /// not an error, because the supervisor can outlive a project deletion and
    /// its final event must not crash the pump.
    ///
    /// # Errors
    ///
    /// Returns [`deck_domain::DeckError::Storage`] on a database failure.
    pub async fn finish_run(
        &self,
        run_id: RunId,
        finished_at: DateTime<Utc>,
        exit_code: Option<i32>,
        outcome: RunOutcome,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE runs SET finished_at = ?2, exit_code = ?3, outcome = ?4 WHERE id = ?1",
        )
        .bind(run_id.to_string())
        .bind(map::ts(finished_at))
        .bind(exit_code.map(i64::from))
        .bind(map::outcome_str(outcome))
        .execute(self.pool())
        .await
        .map_err(|e| db_err(&e))?;
        Ok(())
    }

    /// The most recent runs for a project, newest first.
    ///
    /// # Errors
    ///
    /// Returns [`deck_domain::DeckError::Storage`] on a database failure.
    pub async fn run_history(&self, project_id: ProjectId, limit: u32) -> Result<Vec<RunRecord>> {
        let rows = sqlx::query(
            "SELECT * FROM runs WHERE project_id = ?1
             ORDER BY started_at DESC LIMIT ?2",
        )
        .bind(project_id.to_hyphenated())
        .bind(i64::from(limit))
        .fetch_all(self.pool())
        .await
        .map_err(|e| db_err(&e))?;
        rows.iter().map(map::run_from_row).collect()
    }

    /// The most recent *failed* runs for a project, newest first.
    ///
    /// User-requested stops are excluded by construction -- `stopped` and
    /// `killed` are not failures, and crash history that fills with intentional
    /// stops is history nobody reads.
    ///
    /// # Errors
    ///
    /// Returns [`deck_domain::DeckError::Storage`] on a database failure.
    pub async fn crash_history(
        &self,
        project_id: ProjectId,
        limit: u32,
    ) -> Result<Vec<RunRecord>> {
        let rows = sqlx::query(
            "SELECT * FROM runs
             WHERE project_id = ?1 AND outcome IN ('failed', 'failed_to_start')
             ORDER BY started_at DESC LIMIT ?2",
        )
        .bind(project_id.to_hyphenated())
        .bind(i64::from(limit))
        .fetch_all(self.pool())
        .await
        .map_err(|e| db_err(&e))?;
        rows.iter().map(map::run_from_row).collect()
    }

    /// Failures across EVERY project since `since`, newest first.
    ///
    /// The Today strip's query: "what died while I was away" is a question
    /// about the library, not about one row, and asking it per-project would
    /// be forty-two round-trips every morning. Same failure semantics as
    /// [`Self::crash_history`] -- user-requested stops are not deaths.
    ///
    /// The window is on WHEN IT DIED (`finished_at`, falling back to
    /// `started_at` for a failed-to-start that never got a finish stamp).
    /// Windowing on start time silently dropped the most important case: a
    /// server that ran for days and died an hour ago. Both columns are RFC
    /// 3339 UTC text, so string comparison is chronological comparison.
    ///
    /// # Errors
    ///
    /// Returns [`deck_domain::DeckError::Storage`] on a database failure.
    pub async fn failures_since(
        &self,
        since: chrono::DateTime<chrono::Utc>,
        limit: u32,
    ) -> Result<Vec<RunRecord>> {
        let rows = sqlx::query(
            "SELECT * FROM runs
             WHERE outcome IN ('failed', 'failed_to_start')
               AND COALESCE(finished_at, started_at) >= ?1
             ORDER BY COALESCE(finished_at, started_at) DESC LIMIT ?2",
        )
        .bind(map::ts(since))
        .bind(i64::from(limit))
        .fetch_all(self.pool())
        .await
        .map_err(|e| db_err(&e))?;
        rows.iter().map(map::run_from_row).collect()
    }

    /// Closes runs left marked `running` by a process that never wrote a
    /// finish, returning how many were repaired.
    ///
    /// Called once at startup, where the claim is exact rather than
    /// heuristic: the supervisor spawns nothing during boot, so ANY row
    /// still marked `running` at that moment describes a process that is not
    /// alive. Without this the rows accumulate forever -- eighteen of them on
    /// this machine before the sweep existed -- and every surface that reads
    /// run history has to work around them, which is how a dashboard ends up
    /// announcing "running x18" beside a header saying nothing is running.
    ///
    /// `finished_at` is deliberately left as it is: the end time is precisely
    /// what was never observed, and inventing one would be the fabrication
    /// this codebase refuses everywhere else.
    ///
    /// `started_before` should be the moment this process started. The sweep
    /// runs on a background task rather than inline in boot, so "nothing has
    /// been spawned yet" is true in practice but is not an invariant -- a
    /// slower disk or a larger database widens the gap between the sweep and
    /// the first clickable Run button. Bounding on start time makes the
    /// claim exact instead of relying on winning a race.
    ///
    /// # Errors
    ///
    /// Returns [`deck_domain::DeckError::Storage`] on a database failure.
    pub async fn reconcile_orphaned_runs(
        &self,
        started_before: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64> {
        let result = sqlx::query(
            "UPDATE runs SET outcome = 'interrupted'
             WHERE outcome = 'running' AND started_at < ?1",
        )
        .bind(map::ts(started_before))
        .execute(self.pool())
        .await
        .map_err(|e| db_err(&e))?;
        Ok(result.rows_affected())
    }

    /// The most recent runs across EVERY project, newest first.
    ///
    /// The dashboard's activity feed. Ordered by start time deliberately --
    /// unlike `failures_since`, which answers "what died" and therefore keys
    /// on death, this answers "what have I been doing", and a run that is
    /// still going belongs at the top of that list rather than nowhere.
    ///
    /// # Errors
    ///
    /// Returns [`deck_domain::DeckError::Storage`] on a database failure.
    pub async fn recent_runs(&self, limit: u32) -> Result<Vec<RunRecord>> {
        let rows = sqlx::query(
            "SELECT * FROM runs ORDER BY started_at DESC LIMIT ?1",
        )
        .bind(i64::from(limit))
        .fetch_all(self.pool())
        .await
        .map_err(|e| db_err(&e))?;
        rows.iter().map(map::run_from_row).collect()
    }

    /// How many times each project has been launched, ever.
    ///
    /// The library is sorted by what Tanner actually uses, and "actually uses"
    /// has to come from somewhere. 475 run records existed before anything
    /// read them for this: the evidence was already on disk while the list
    /// stayed alphabetical.
    ///
    /// One GROUP BY for the whole library rather than a count per row.
    ///
    /// # Errors
    ///
    /// Propagates any database failure.
    pub async fn run_counts(&self) -> Result<Vec<(ProjectId, u32)>> {
        let rows = sqlx::query("SELECT project_id, COUNT(*) AS n FROM runs GROUP BY project_id")
            .fetch_all(self.pool())
            .await
            .map_err(|e| db_err(&e))?;
        rows.iter()
            .map(|row| {
                let id: String = row.try_get("project_id").map_err(|e| db_err(&e))?;
                let n: i64 = row.try_get("n").map_err(|e| db_err(&e))?;
                let id = id.parse::<uuid::Uuid>().map_err(|e| {
                    deck_domain::error::DeckError::Storage(format!("bad project id: {e}"))
                })?;
                Ok((ProjectId::from_uuid(id), u32::try_from(n).unwrap_or(u32::MAX)))
            })
            .collect()
    }

    /// The most recent run of every project, whatever its outcome.
    ///
    /// # Why the LAST run rather than the last failure
    ///
    /// "Has this ever failed" is nearly useless -- almost everything has, once.
    /// "Did the last attempt fail" is the question a launcher should answer,
    /// because it is the one that predicts the next attempt. Transcriber failed
    /// fourteen times in a row on a missing Python module; nothing on the row
    /// said so, because a run's state resets when the app restarts and the
    /// history was only reachable through the log panel.
    ///
    /// A window function rather than a query per project: the caller wants the
    /// whole library at once, and fifty round trips to answer one question is
    /// how a list gets slow.
    ///
    /// # Errors
    ///
    /// Propagates any database failure.
    pub async fn latest_run_per_project(&self) -> Result<Vec<RunRecord>> {
        let rows = sqlx::query(
            "SELECT * FROM (
               SELECT *, ROW_NUMBER() OVER (
                 PARTITION BY project_id ORDER BY started_at DESC
               ) AS rn FROM runs
             ) WHERE rn = 1",
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| db_err(&e))?;
        rows.iter().map(map::run_from_row).collect()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chrono::{Duration, Utc};
    use deck_domain::command::Lifecycle;
    use deck_domain::log::RunOutcome;
    use deck_domain::project::{DetectedFacts, Project, ProjectId, ProjectOverrides, RestartPolicy};
    use deck_domain::runtime::{RunId, RunRecord};

    use crate::store::test_support::TempStore;

    async fn project(t: &TempStore) -> ProjectId {
        let now = Utc::now();
        let p = Project {
            id: ProjectId::new(),
            name: "svc".into(),
            description: None,
            root: PathBuf::from(format!(r"D:\W\{}", ProjectId::new())),
            detected: DetectedFacts {
                runner_id: "go".into(),
                language: "Go".into(),
                framework: None,
                package_manager: None,
                version: None,
                detected_at: now,
            },
            overrides: ProjectOverrides::default(),
            tags: vec![],
            category: None,
            favorite: false,
            pinned: false,
            archived: false,
            restart_policy: RestartPolicy::Never,
            notes: None,
            created_at: now,
            updated_at: now,
            last_launched_at: None,
        };
        t.store.insert_project(&p).await.unwrap();
        p.id
    }

    fn record(project_id: ProjectId, started_offset_secs: i64, outcome: RunOutcome) -> RunRecord {
        RunRecord {
            run_id: RunId::new(),
            project_id,
            lifecycle: Lifecycle::Run,
            command: "go run .".into(),
            started_at: Utc::now() + Duration::seconds(started_offset_secs),
            finished_at: None,
            exit_code: None,
            outcome,
            log_path: PathBuf::from(r"D:\logs\x.log"),
        }
    }

    /// The question the launcher asks: did the LAST attempt fail?
    ///
    /// Not "has it ever failed" -- almost everything has, once, and answering
    /// that would mark half a healthy library as broken.
    #[tokio::test]
    async fn latest_run_per_project_returns_one_row_each_newest_first() {
        let t = TempStore::new("latest-run").await;
        let broken = project(&t).await;
        let recovered = project(&t).await;
        let untouched = project(&t).await;

        // `broken`: an old success, then a recent failure. Last = failed.
        t.store
            .insert_run(&record(broken, -600, RunOutcome::Succeeded))
            .await
            .unwrap();
        let mut latest_fail = record(broken, -60, RunOutcome::Failed);
        latest_fail.exit_code = Some(1);
        t.store.insert_run(&latest_fail).await.unwrap();

        // `recovered`: failed repeatedly, then worked. Last = succeeded, and
        // it must NOT be reported as broken.
        t.store
            .insert_run(&record(recovered, -600, RunOutcome::Failed))
            .await
            .unwrap();
        t.store
            .insert_run(&record(recovered, -500, RunOutcome::Failed))
            .await
            .unwrap();
        t.store
            .insert_run(&record(recovered, -30, RunOutcome::Succeeded))
            .await
            .unwrap();

        let rows = t.store.latest_run_per_project().await.unwrap();

        // One row per project that has ever run; `untouched` has none.
        assert_eq!(rows.len(), 2, "one row per project with history");
        assert!(
            !rows.iter().any(|r| r.project_id == untouched),
            "a project that never ran contributes no row"
        );

        let b = rows.iter().find(|r| r.project_id == broken).expect("broken");
        assert_eq!(b.outcome, RunOutcome::Failed);
        assert_eq!(b.exit_code, Some(1));

        let r = rows
            .iter()
            .find(|r| r.project_id == recovered)
            .expect("recovered");
        assert_eq!(
            r.outcome,
            RunOutcome::Succeeded,
            "the newest run wins, so a recovery clears the failure"
        );
    }

    /// Startup reconciliation: every `running` row is an orphan at boot,
    /// because nothing has been spawned yet. Finished rows must not move.
    #[tokio::test]
    async fn reconcile_closes_orphans_and_leaves_finished_runs_alone() {
        let t = TempStore::new("reconcile").await;
        let a = project(&t).await;

        let orphan_one = record(a, -600, RunOutcome::Running);
        let orphan_two = record(a, -300, RunOutcome::Running);
        let done = record(a, -900, RunOutcome::Succeeded);
        let failed = record(a, -800, RunOutcome::Failed);
        for r in [&orphan_one, &orphan_two, &done, &failed] {
            t.store.insert_run(r).await.unwrap();
        }

        // A run started after the boundary belongs to the current session and
        // must survive, even though it is also marked `running`.
        let this_session = record(a, 60, RunOutcome::Running);
        t.store.insert_run(&this_session).await.unwrap();

        let boundary = Utc::now();
        let repaired = t.store.reconcile_orphaned_runs(boundary).await.unwrap();
        assert_eq!(repaired, 2, "both old orphans, and nothing else");

        let history = t.store.run_history(a, 50).await.unwrap();
        let by_id = |id| history.iter().find(|r| r.run_id == id).unwrap();
        assert_eq!(by_id(orphan_one.run_id).outcome, RunOutcome::Interrupted);
        assert_eq!(by_id(orphan_two.run_id).outcome, RunOutcome::Interrupted);
        assert_eq!(by_id(done.run_id).outcome, RunOutcome::Succeeded);
        assert_eq!(by_id(failed.run_id).outcome, RunOutcome::Failed);
        // The end time was never observed, so none was invented.
        assert!(by_id(orphan_one.run_id).finished_at.is_none());

        assert_eq!(
            by_id(this_session.run_id).outcome,
            RunOutcome::Running,
            "a run started after the boundary is this session's, not an orphan"
        );

        // Idempotent: a second boot finds nothing left to repair.
        assert_eq!(t.store.reconcile_orphaned_runs(boundary).await.unwrap(), 0);
    }

    /// The dashboard feed: every project's runs interleaved, newest first,
    /// including one still running (which a "finished only" query would hide
    /// exactly when it is most interesting).
    #[tokio::test]
    async fn recent_runs_interleaves_projects_newest_first() {
        let t = TempStore::new("recent-runs").await;
        let a = project(&t).await;
        let b = project(&t).await;

        t.store.insert_run(&record(a, -300, RunOutcome::Succeeded)).await.unwrap();
        t.store.insert_run(&record(b, -200, RunOutcome::Failed)).await.unwrap();
        let live = record(a, -10, RunOutcome::Running);
        t.store.insert_run(&live).await.unwrap();

        let got = t.store.recent_runs(10).await.unwrap();
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].run_id, live.run_id, "the in-flight run leads");
        assert_eq!(got[1].project_id, b);
        assert_eq!(got[2].project_id, a);

        let capped = t.store.recent_runs(2).await.unwrap();
        assert_eq!(capped.len(), 2, "limit is honoured");
    }

    /// The Today-strip query: failures inside the window, across projects,
    /// with stops excluded and old failures aged out -- and the window keyed
    /// on DEATH time: a server that started two days ago and died an hour
    /// ago is precisely "what died while I was away".
    #[tokio::test]
    async fn failures_since_windows_on_death_time_across_projects() {
        let t = TempStore::new("failures-since").await;
        let a = project(&t).await;
        let b = project(&t).await;

        // In-window failures on both projects; a stop and a success that must
        // not count; one ancient failure outside the window.
        t.store.insert_run(&record(a, -60, RunOutcome::Failed)).await.unwrap();
        t.store
            .insert_run(&record(b, -120, RunOutcome::FailedToStart))
            .await
            .unwrap();
        t.store.insert_run(&record(a, -30, RunOutcome::Stopped)).await.unwrap();
        t.store
            .insert_run(&record(b, -30, RunOutcome::Succeeded))
            .await
            .unwrap();
        t.store
            .insert_run(&record(a, -60 * 60 * 48, RunOutcome::Failed))
            .await
            .unwrap();

        // The long-runner: started 48h ago (outside the window), died 1h
        // ago (inside). Windowing on start time silently dropped exactly
        // this case.
        let mut long_runner = record(b, -60 * 60 * 48, RunOutcome::Failed);
        long_runner.finished_at = Some(Utc::now() - Duration::hours(1));
        t.store.insert_run(&long_runner).await.unwrap();

        let since = Utc::now() - Duration::hours(24);
        let got = t.store.failures_since(since, 50).await.unwrap();
        assert_eq!(got.len(), 3, "two short failures + the long-runner");
        assert!(
            got.iter().any(|r| r.run_id == long_runner.run_id),
            "a long-lived run that died inside the window must be reported"
        );
        assert_eq!(got[0].project_id, a, "newest death first");
    }

    #[tokio::test]
    async fn insert_and_finish_round_trip() {
        let t = TempStore::new("runs").await;
        let pid = project(&t).await;
        let rec = record(pid, 0, RunOutcome::Running);
        t.store.insert_run(&rec).await.unwrap();

        let finished_at = Utc::now();
        t.store
            .finish_run(rec.run_id, finished_at, Some(0), RunOutcome::Stopped)
            .await
            .unwrap();

        let history = t.store.run_history(pid, 10).await.unwrap();
        assert_eq!(history.len(), 1);
        let back = &history[0];
        assert_eq!(back.run_id, rec.run_id);
        assert_eq!(back.exit_code, Some(0));
        assert_eq!(back.outcome, RunOutcome::Stopped);
        assert!(back.finished_at.is_some());
        assert_eq!(back.log_path, rec.log_path);
    }

    #[tokio::test]
    async fn history_is_newest_first_and_limited() {
        let t = TempStore::new("order").await;
        let pid = project(&t).await;
        for i in 0..5 {
            t.store
                .insert_run(&record(pid, i, RunOutcome::Succeeded))
                .await
                .unwrap();
        }
        let history = t.store.run_history(pid, 3).await.unwrap();
        assert_eq!(history.len(), 3);
        assert!(history[0].started_at > history[1].started_at);
        assert!(history[1].started_at > history[2].started_at);
    }

    #[tokio::test]
    async fn crash_history_excludes_intentional_stops() {
        let t = TempStore::new("crash").await;
        let pid = project(&t).await;
        for outcome in [
            RunOutcome::Failed,
            RunOutcome::Stopped,
            RunOutcome::Killed,
            RunOutcome::FailedToStart,
            RunOutcome::Succeeded,
        ] {
            t.store.insert_run(&record(pid, 0, outcome)).await.unwrap();
        }
        let crashes = t.store.crash_history(pid, 10).await.unwrap();
        assert_eq!(crashes.len(), 2);
        assert!(crashes.iter().all(|r| r.outcome.is_failure()));
    }

    #[tokio::test]
    async fn deleting_a_project_cascades_its_runs() {
        let t = TempStore::new("cascade").await;
        let pid = project(&t).await;
        t.store
            .insert_run(&record(pid, 0, RunOutcome::Failed))
            .await
            .unwrap();

        t.store.delete_project(pid).await.unwrap();
        assert!(t.store.run_history(pid, 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_run_for_an_unknown_project_is_rejected_by_the_foreign_key() {
        let t = TempStore::new("fk").await;
        let err = t
            .store
            .insert_run(&record(ProjectId::new(), 0, RunOutcome::Running))
            .await
            .unwrap_err();
        assert_eq!(err.code(), "storage");
    }

    #[tokio::test]
    async fn finishing_an_unknown_run_is_a_quiet_no_op() {
        let t = TempStore::new("finish-missing").await;
        t.store
            .finish_run(RunId::new(), Utc::now(), Some(1), RunOutcome::Failed)
            .await
            .unwrap();
    }
}
