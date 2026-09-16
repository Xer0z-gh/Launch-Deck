//! Project repository.

use std::path::Path;

use chrono::{DateTime, Utc};
use deck_domain::error::{DeckError, Result};
use deck_domain::project::{Project, ProjectId};

use crate::map;
use crate::store::{db_err, Store};

impl Store {
    /// Registers a project.
    ///
    /// # Errors
    ///
    /// Returns [`DeckError::DuplicateProject`] if another project already
    /// occupies the same root directory, naming the occupant so the message is
    /// actionable.
    pub async fn insert_project(&self, project: &Project) -> Result<()> {
        let result = sqlx::query(
            "INSERT INTO projects (
                id, name, description, root,
                runner_id, language, framework, package_manager, version, detected_at,
                overrides, restart_policy,
                tags, category, favorite, pinned, archived, notes,
                created_at, updated_at, last_launched_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                       ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
        )
        .bind(project.id.to_hyphenated())
        .bind(&project.name)
        .bind(&project.description)
        .bind(project.root.display().to_string())
        .bind(&project.detected.runner_id)
        .bind(&project.detected.language)
        .bind(&project.detected.framework)
        .bind(&project.detected.package_manager)
        .bind(&project.detected.version)
        .bind(map::ts(project.detected.detected_at))
        .bind(map::to_json(&project.overrides)?)
        .bind(map::to_json(&project.restart_policy)?)
        .bind(map::to_json(&project.tags)?)
        .bind(&project.category)
        .bind(i64::from(project.favorite))
        .bind(i64::from(project.pinned))
        .bind(i64::from(project.archived))
        .bind(&project.notes)
        .bind(map::ts(project.created_at))
        .bind(map::ts(project.updated_at))
        .bind(project.last_launched_at.map(map::ts))
        .execute(self.pool())
        .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) if is_unique_violation(&e) => {
                let existing_name = self
                    .project_by_root(&project.root)
                    .await?
                    .map_or_else(|| "another project".to_owned(), |p| p.name);
                Err(DeckError::DuplicateProject {
                    path: project.root.clone(),
                    existing_name,
                })
            }
            Err(e) => Err(db_err(&e)),
        }
    }

    /// A project by id.
    ///
    /// # Errors
    ///
    /// Returns [`DeckError::ProjectNotFound`] when no row exists.
    pub async fn project(&self, id: ProjectId) -> Result<Project> {
        let row = sqlx::query("SELECT * FROM projects WHERE id = ?1")
            .bind(id.to_hyphenated())
            .fetch_optional(self.pool())
            .await
            .map_err(|e| db_err(&e))?;
        row.as_ref()
            .map(map::project_from_row)
            .transpose()?
            .ok_or(DeckError::ProjectNotFound(id))
    }

    /// The project registered at `root`, if any.
    ///
    /// # Errors
    ///
    /// Returns [`DeckError::Storage`] on a database failure.
    pub async fn project_by_root(&self, root: &Path) -> Result<Option<Project>> {
        let row = sqlx::query("SELECT * FROM projects WHERE root = ?1")
            .bind(root.display().to_string())
            .fetch_optional(self.pool())
            .await
            .map_err(|e| db_err(&e))?;
        row.as_ref().map(map::project_from_row).transpose()
    }

    /// Every registered project, name order.
    ///
    /// Ordering here is only a stable baseline -- pinned-first, favourites and
    /// user sorts are view concerns applied in the frontend.
    ///
    /// # Errors
    ///
    /// Returns [`DeckError::Storage`] on a database failure.
    pub async fn projects(&self) -> Result<Vec<Project>> {
        let rows = sqlx::query("SELECT * FROM projects ORDER BY name COLLATE NOCASE ASC")
            .fetch_all(self.pool())
            .await
            .map_err(|e| db_err(&e))?;
        rows.iter().map(map::project_from_row).collect()
    }

    /// Persists every mutable field of a project (everything except `id`,
    /// `root` and `created_at`).
    ///
    /// # Errors
    ///
    /// Returns [`DeckError::ProjectNotFound`] when no row was updated.
    pub async fn update_project(&self, project: &Project) -> Result<()> {
        let result = sqlx::query(
            "UPDATE projects SET
                name = ?2, description = ?3,
                runner_id = ?4, language = ?5, framework = ?6,
                package_manager = ?7, version = ?8, detected_at = ?9,
                overrides = ?10, restart_policy = ?11,
                tags = ?12, category = ?13,
                favorite = ?14, pinned = ?15, archived = ?16, notes = ?17,
                updated_at = ?18, last_launched_at = ?19
             WHERE id = ?1",
        )
        .bind(project.id.to_hyphenated())
        .bind(&project.name)
        .bind(&project.description)
        .bind(&project.detected.runner_id)
        .bind(&project.detected.language)
        .bind(&project.detected.framework)
        .bind(&project.detected.package_manager)
        .bind(&project.detected.version)
        .bind(map::ts(project.detected.detected_at))
        .bind(map::to_json(&project.overrides)?)
        .bind(map::to_json(&project.restart_policy)?)
        .bind(map::to_json(&project.tags)?)
        .bind(&project.category)
        .bind(i64::from(project.favorite))
        .bind(i64::from(project.pinned))
        .bind(i64::from(project.archived))
        .bind(&project.notes)
        .bind(map::ts(Utc::now()))
        .bind(project.last_launched_at.map(map::ts))
        .execute(self.pool())
        .await
        .map_err(|e| db_err(&e))?;

        if result.rows_affected() == 0 {
            return Err(DeckError::ProjectNotFound(project.id));
        }
        Ok(())
    }

    /// Removes a project. Run history cascades.
    ///
    /// # Errors
    ///
    /// Returns [`DeckError::ProjectNotFound`] when no row was deleted.
    pub async fn delete_project(&self, id: ProjectId) -> Result<()> {
        let result = sqlx::query("DELETE FROM projects WHERE id = ?1")
            .bind(id.to_hyphenated())
            .execute(self.pool())
            .await
            .map_err(|e| db_err(&e))?;
        if result.rows_affected() == 0 {
            return Err(DeckError::ProjectNotFound(id));
        }
        Ok(())
    }

    /// Stamps a project as launched at `at`.
    ///
    /// # Errors
    ///
    /// Returns [`DeckError::Storage`] on a database failure.
    pub async fn touch_last_launched(&self, id: ProjectId, at: DateTime<Utc>) -> Result<()> {
        sqlx::query(
            "UPDATE projects SET last_launched_at = ?2, updated_at = ?2 WHERE id = ?1",
        )
        .bind(id.to_hyphenated())
        .bind(map::ts(at))
        .execute(self.pool())
        .await
        .map_err(|e| db_err(&e))?;
        Ok(())
    }
}

fn is_unique_violation(e: &sqlx::Error) -> bool {
    e.as_database_error().is_some_and(sqlx::error::DatabaseError::is_unique_violation)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chrono::Utc;
    use deck_domain::project::{
        DetectedFacts, EnvVar, Project, ProjectId, ProjectOverrides, RestartPolicy,
    };

    use crate::store::test_support::TempStore;

    fn sample(root: &str) -> Project {
        let now = Utc::now();
        Project {
            id: ProjectId::new(),
            name: "Coupon Hunter".into(),
            description: Some("stacks codes".into()),
            root: PathBuf::from(root),
            detected: DetectedFacts {
                runner_id: "node".into(),
                language: "TypeScript".into(),
                framework: Some("Vite".into()),
                package_manager: Some("pnpm".into()),
                version: Some("1.2.0".into()),
                detected_at: now,
            },
            overrides: ProjectOverrides::default(),
            tags: vec!["extension".into()],
            category: Some("Browser".into()),
            favorite: true,
            pinned: false,
            archived: false,
            restart_policy: RestartPolicy::OnFailure { max_attempts: 3 },
            notes: None,
            created_at: now,
            updated_at: now,
            last_launched_at: None,
        }
    }

    #[tokio::test]
    async fn round_trips_every_field() {
        let t = TempStore::new("roundtrip").await;
        let mut p = sample(r"D:\W\one");
        p.overrides.env.push(EnvVar::new("API_TOKEN", "shh"));
        p.overrides.args = vec!["--port".into(), "4000".into()];

        t.store.insert_project(&p).await.unwrap();
        let back = t.store.project(p.id).await.unwrap();

        assert_eq!(back, p);
        // The env var kept its inferred secret flag through the JSON column.
        assert!(back.overrides.env[0].secret);
    }

    #[tokio::test]
    async fn duplicate_root_names_the_existing_project() {
        let t = TempStore::new("dup").await;
        let first = sample(r"D:\W\same");
        t.store.insert_project(&first).await.unwrap();

        let second = sample(r"D:\W\same");
        let err = t.store.insert_project(&second).await.unwrap_err();
        assert_eq!(err.code(), "duplicate_project");
        assert!(err.to_string().contains("Coupon Hunter"));
    }

    #[tokio::test]
    async fn missing_project_is_not_found() {
        let t = TempStore::new("missing").await;
        let err = t.store.project(ProjectId::new()).await.unwrap_err();
        assert_eq!(err.code(), "project_not_found");
    }

    #[tokio::test]
    async fn lookup_by_root_finds_and_misses() {
        let t = TempStore::new("byroot").await;
        let p = sample(r"D:\W\lookup");
        t.store.insert_project(&p).await.unwrap();

        let hit = t.store.project_by_root(&p.root).await.unwrap();
        assert_eq!(hit.map(|p| p.id), Some(p.id));
        let miss = t
            .store
            .project_by_root(std::path::Path::new(r"D:\W\elsewhere"))
            .await
            .unwrap();
        assert!(miss.is_none());
    }

    #[tokio::test]
    async fn lists_in_case_insensitive_name_order() {
        let t = TempStore::new("list").await;
        for (name, root) in [("zeta", r"D:\W\z"), ("Alpha", r"D:\W\a"), ("mid", r"D:\W\m")] {
            let mut p = sample(root);
            p.name = name.into();
            t.store.insert_project(&p).await.unwrap();
        }
        let names: Vec<String> = t
            .store
            .projects()
            .await
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(names, vec!["Alpha", "mid", "zeta"]);
    }

    #[tokio::test]
    async fn update_persists_changes_and_bumps_updated_at() {
        let t = TempStore::new("update").await;
        let mut p = sample(r"D:\W\upd");
        t.store.insert_project(&p).await.unwrap();

        p.name = "Renamed".into();
        p.favorite = false;
        p.tags.push("new-tag".into());
        t.store.update_project(&p).await.unwrap();

        let back = t.store.project(p.id).await.unwrap();
        assert_eq!(back.name, "Renamed");
        assert!(!back.favorite);
        assert!(back.tags.contains(&"new-tag".to_owned()));
        assert!(back.updated_at >= p.updated_at);
    }

    #[tokio::test]
    async fn updating_a_deleted_project_reports_not_found() {
        let t = TempStore::new("updmissing").await;
        let p = sample(r"D:\W\ghost");
        let err = t.store.update_project(&p).await.unwrap_err();
        assert_eq!(err.code(), "project_not_found");
    }

    #[tokio::test]
    async fn delete_removes_the_row() {
        let t = TempStore::new("del").await;
        let p = sample(r"D:\W\gone");
        t.store.insert_project(&p).await.unwrap();
        t.store.delete_project(p.id).await.unwrap();
        assert_eq!(
            t.store.project(p.id).await.unwrap_err().code(),
            "project_not_found"
        );
        // Deleting again is an error, not a silent success.
        assert_eq!(
            t.store.delete_project(p.id).await.unwrap_err().code(),
            "project_not_found"
        );
    }

    #[tokio::test]
    async fn touch_last_launched_sets_the_stamp() {
        let t = TempStore::new("touch").await;
        let p = sample(r"D:\W\touch");
        t.store.insert_project(&p).await.unwrap();

        let at = Utc::now();
        t.store.touch_last_launched(p.id, at).await.unwrap();
        let back = t.store.project(p.id).await.unwrap();
        let stamped = back.last_launched_at.expect("stamp should be set");
        assert!((stamped - at).num_milliseconds().abs() < 5);
    }
}
