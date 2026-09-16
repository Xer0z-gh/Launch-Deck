//! Opening the database: connection options, pool, migrations.

use std::path::Path;

use deck_domain::error::{DeckError, Result};
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous,
};
use sqlx::SqlitePool;

/// Handle to the Launch Deck database.
///
/// Cheap to share behind an [`std::sync::Arc`]; the pool is internally
/// synchronised. All repository methods live in `impl Store` blocks across the
/// sibling modules.
pub struct Store {
    pool: SqlitePool,
}

/// Connection options shared by the initial open and any reopen after a
/// restore, so both paths get identical durability settings.
///
/// # Why these are tuned for durability over throughput
///
/// This database is a registry the user curated by hand: a few dozen rows,
/// written rarely, and expensive to rebuild. Throughput is irrelevant; losing a
/// commit is not. The defaults are wrong for that shape, and got a real registry
/// wiped:
///
/// * `synchronous = NORMAL` in WAL mode does **not** fsync on commit -- it
///   fsyncs at checkpoints. A hard kill between commit and checkpoint can drop
///   everything since the last one. `FULL` fsyncs each commit, so a committed
///   project survives the process being killed outright.
/// * `wal_autocheckpoint` defaults to 1000 pages. A registry this small never
///   reaches that, so the WAL grew to eight times the size of the database file
///   while the database itself stayed nearly empty. Anything that discarded the
///   WAL reverted the registry to almost nothing. Checkpointing every 4 pages
///   keeps the database file itself continuously current, which also means a
///   plain file copy of `deck.db` is a usable backup.
///
/// WAL is kept (rather than a rollback journal) because the pool has several
/// connections and WAL is what stops a reader blocking the writer.
fn connect_options(path: &Path) -> SqliteConnectOptions {
    SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Full)
        .pragma("wal_autocheckpoint", "4")
        .foreign_keys(true)
        .busy_timeout(std::time::Duration::from_secs(5))
}

/// Where snapshots live for a given database file.
fn backups_dir(db_path: &Path) -> std::path::PathBuf {
    db_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("backups")
}

/// How many database snapshots to keep. Three covers "it broke and I did not
/// notice until the next launch" without unbounded disk use.
const BACKUPS_KEPT: usize = 3;

/// The newest snapshot in a backup directory, by filename (timestamps sort).
fn newest_backup(dir: &Path) -> Option<std::path::PathBuf> {
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "db"))
        .collect();
    files.sort();
    files.pop()
}

/// Keeps the newest `keep` snapshots and removes the rest.
fn prune_backups(dir: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<std::path::PathBuf> = entries
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "db"))
        .collect();
    if files.len() <= keep {
        return;
    }
    files.sort();
    for stale in &files[..files.len() - keep] {
        if let Err(e) = std::fs::remove_file(stale) {
            tracing::warn!(path = %stale.display(), error = %e, "could not prune snapshot");
        }
    }
}

/// Collapses an `sqlx` failure into the one storage variant the domain exposes.
///
/// A free function rather than a `From` impl because both types are foreign to
/// this crate (orphan rule), and because call sites reading `map_err(db_err)`
/// stay honest about where database errors enter the domain.
pub(crate) fn db_err(e: &sqlx::Error) -> DeckError {
    DeckError::Storage(e.to_string())
}

impl Store {
    /// Opens (creating if necessary) the database at `path` and runs any
    /// pending migrations.
    ///
    /// Durability settings live in [`connect_options`] -- read the rationale
    /// there before changing any of them; the defaults cost a real registry.
    ///
    /// Opening also self-heals: an empty database beside a populated snapshot is
    /// restored automatically, and a populated database is snapshotted.
    ///
    /// # Errors
    ///
    /// Returns [`DeckError::Io`] if the parent directory cannot be created,
    /// or [`DeckError::Storage`] if the database cannot be opened or a
    /// migration fails.
    pub async fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| DeckError::io(format!("creating {}", parent.display()), e))?;
        }

        let options = connect_options(path);

        let pool = SqlitePoolOptions::new()
            // SQLite serialises writes regardless; a small pool covers
            // concurrent reads without pretending to more parallelism than
            // the engine has.
            .max_connections(4)
            .connect_with(options)
            .await
            .map_err(|e| db_err(&e))?;

        sqlx::migrate!()
            .run(&pool)
            .await
            .map_err(|e| DeckError::Storage(format!("migration failed: {e}")))?;

        let store = Self { pool };

        // Self-heal before anything reads the registry: an empty database
        // sitting beside a populated snapshot means data was lost, and the user
        // should never have to notice, let alone rebuild by hand.
        if store.is_empty().await && newest_backup(&backups_dir(path)).is_some() {
            match store.restore_newest_snapshot(path).await {
                Ok(from) => tracing::warn!(
                    snapshot = %from.display(),
                    restored = store.projects().await.map(|p| p.len()).unwrap_or(0),
                    "registry was empty; restored the newest snapshot"
                ),
                Err(e) => tracing::warn!(error = %e, "snapshot restore failed"),
            }
        }

        // NOTE: the rotating snapshot deliberately does NOT run here.
        //
        // `VACUUM INTO` copies the entire database, and running it inside
        // `open()` put a full file copy on the critical path of every launch --
        // the window could not appear until it finished. The caller now invokes
        // `snapshot()` on a background task once the UI is up, so the same
        // snapshot is still taken on every launch, just not in front of the
        // user. See `snapshot` for why it is still per-launch.
        Ok(store)
    }

    /// Whether the registry holds no projects.
    async fn is_empty(&self) -> bool {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM projects")
            .fetch_one(self.pool())
            .await
            .unwrap_or(0)
            == 0
    }

    /// Takes a rotating snapshot of the database on open.
    ///
    /// This exists because a registry the user has curated is expensive to
    /// rebuild and cheap to copy. A snapshot per launch, three kept, means any
    /// loss -- corruption, a bad migration, an operator mistake, or a cause
    /// nobody has diagnosed yet -- costs one file copy to undo instead of an
    /// afternoon of re-registering projects.
    ///
    /// `VACUUM INTO` rather than a file copy: it produces a transactionally
    /// consistent snapshot even with a live WAL, so the backup is never a
    /// half-written page.
    ///
    /// Deliberately infallible and deliberately skipped for an empty database.
    /// A failed backup must never block startup, and snapshotting an empty
    /// database would otherwise rotate the good snapshots out of existence --
    /// which is precisely when they are needed.
    /// Takes the rotating snapshot. Call once per launch, off the hot path.
    ///
    /// Separated from [`Store::open`] so startup never waits on a full database
    /// copy. Still infallible and still per-launch: the durability argument
    /// below is unchanged, only the timing is.
    pub async fn snapshot(&self, db_path: &Path) {
        self.snapshot_if_populated(db_path).await;
    }

    async fn snapshot_if_populated(&self, db_path: &Path) {
        let populated = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM projects")
            .fetch_one(self.pool())
            .await
            .unwrap_or(0)
            > 0;
        if !populated {
            tracing::debug!("database is empty; skipping snapshot to preserve existing ones");
            return;
        }

        let backups = backups_dir(db_path);
        if let Err(e) = std::fs::create_dir_all(&backups) {
            tracing::warn!(error = %e, "could not create backup directory");
            return;
        }

        let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
        let dest = backups.join(format!("deck-{stamp}.db"));
        if dest.exists() {
            return; // already snapshotted this second
        }

        // `VACUUM INTO` takes a literal, so the path is inlined with quotes
        // doubled. The path is ours, never user input.
        let escaped = dest.display().to_string().replace('\'', "''");
        match sqlx::query(&format!("VACUUM INTO '{escaped}'"))
            .execute(self.pool())
            .await
        {
            Ok(_) => {
                tracing::info!(path = %dest.display(), "database snapshot written");
                prune_backups(&backups, BACKUPS_KEPT);
            }
            Err(e) => tracing::warn!(error = %e, "database snapshot failed"),
        }
    }

    /// Restores the newest snapshot into this database.
    ///
    /// Copies rows via `ATTACH`, not by copying the file. A file copy has to
    /// close the pool, delete the WAL and `-shm` side files, overwrite the
    /// database and reopen -- and on Windows that races the OS releasing the
    /// handles. Measured: it worked roughly two times in three, which for a
    /// recovery path is worse than not having one, because it fails precisely
    /// when it is needed.
    ///
    /// `ATTACH` needs no file surgery, runs in one transaction, and works with
    /// the pool open. The live database is empty whenever this is called (that
    /// is the trigger), so plain inserts cannot collide.
    ///
    /// # Errors
    ///
    /// Returns [`DeckError::Storage`] if there is no snapshot, or if the
    /// attach/copy fails. On failure the transaction rolls back, leaving the
    /// database exactly as it was.
    pub async fn restore_newest_snapshot(&self, db_path: &Path) -> Result<std::path::PathBuf> {
        let backups = backups_dir(db_path);
        let newest = newest_backup(&backups).ok_or_else(|| {
            DeckError::Storage(format!("no snapshot found in {}", backups.display()))
        })?;

        let escaped = newest.display().to_string().replace('\'', "''");
        let mut tx = self.pool().begin().await.map_err(|e| db_err(&e))?;

        sqlx::query(&format!("ATTACH DATABASE '{escaped}' AS snap"))
            .execute(&mut *tx)
            .await
            .map_err(|e| db_err(&e))?;

        // Order matters: runs reference projects.
        for statement in [
            "INSERT INTO projects SELECT * FROM snap.projects",
            "INSERT INTO runs SELECT * FROM snap.runs",
            "INSERT OR REPLACE INTO settings SELECT * FROM snap.settings",
        ] {
            sqlx::query(statement)
                .execute(&mut *tx)
                .await
                .map_err(|e| db_err(&e))?;
        }

        tx.commit().await.map_err(|e| db_err(&e))?;

        // DETACH cannot run inside the transaction that attached it.
        let _ = sqlx::query("DETACH DATABASE snap").execute(self.pool()).await;

        Ok(newest)
    }

    /// Folds the write-ahead log into the database file and truncates it.
    ///
    /// Called on app exit so the database file on disk is complete and the WAL
    /// is empty -- nothing important is left only in a side file. Best-effort:
    /// a failed checkpoint is logged, never fatal, because the data is already
    /// committed and durable by this point.
    pub async fn checkpoint(&self) {
        match sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(self.pool())
            .await
        {
            Ok(_) => tracing::debug!("wal checkpointed and truncated"),
            Err(e) => tracing::warn!(error = %e, "wal checkpoint failed"),
        }
    }

    /// The underlying pool, for the sibling repository modules.
    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    //! Shared fixture: a real database in a temp directory, removed on drop.

    use std::path::PathBuf;

    use super::Store;

    pub struct TempStore {
        pub store: Store,
        dir: PathBuf,
    }

    impl TempStore {
        pub async fn new(tag: &str) -> Self {
            use std::time::{SystemTime, UNIX_EPOCH};
            sweep_stale_fixtures();
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let mut dir = std::env::temp_dir();
            dir.push(format!("deck-store-{tag}-{nanos:x}"));
            let store = Store::open(&dir.join("deck.db")).await.expect("open temp store");
            Self { store, dir }
        }
    }

    /// Removes fixture directories left by earlier runs.
    ///
    /// `Drop` cannot clean up reliably: the pool still holds the database open,
    /// Windows refuses to delete an open file, and blocking on an async close
    /// from `Drop` deadlocks (it runs on a tokio worker thread that the close
    /// needs). Sweeping on creation instead keeps the leak bounded to one test
    /// run without any of that -- several hundred directories had accumulated
    /// under %TEMP% before this existed.
    ///
    /// Only touches directories older than an hour, so it can never delete a
    /// fixture belonging to a concurrently running suite.
    fn sweep_stale_fixtures() {
        const MAX_AGE: std::time::Duration = std::time::Duration::from_secs(3600);
        let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
            return;
        };
        for entry in entries.filter_map(std::result::Result::ok) {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !name.starts_with("deck-store-") {
                continue;
            }
            let stale = entry
                .metadata()
                .and_then(|m| m.modified())
                .map(|t| t.elapsed().unwrap_or_default() > MAX_AGE)
                .unwrap_or(false);
            if stale {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }

    impl Drop for TempStore {
        fn drop(&mut self) {
            // Best-effort, and deliberately NOT blocking on an async close:
            // `block_on(pool.close())` here deadlocks, because Drop runs on a
            // tokio worker thread and closing the pool needs that same runtime
            // to make progress.
            //
            // Windows will refuse while the pool still holds the file, so a
            // temp directory may survive the test. That is acceptable -- it is
            // a directory under %TEMP% -- and far preferable to a hung suite.
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::TempStore;

    /// Builds a project row good enough to make the registry non-empty.
    async fn seed(store: &super::Store, name: &str) {
        use chrono::Utc;
        use deck_domain::project::{
            DetectedFacts, Project, ProjectId, ProjectOverrides, RestartPolicy,
        };
        let now = Utc::now();
        store
            .insert_project(&Project {
                id: ProjectId::new(),
                name: name.to_owned(),
                description: None,
                root: std::path::PathBuf::from(format!(r"D:\seed\{name}")),
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
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn the_database_file_alone_carries_the_data() {
        // The failure this guards against: rows living only in the WAL, so the
        // database file reverts to near-empty if that side file is ever lost.
        let t = TempStore::new("durable").await;
        seed(&t.store, "must-survive").await;
        t.store.checkpoint().await;

        let db = t.store.pool().connect_options().get_filename().to_path_buf();
        // Copy ONLY the .db file -- deliberately not -wal or -shm.
        let solo = db.with_file_name("solo.db");
        std::fs::copy(&db, &solo).unwrap();

        let reopened = super::Store::open(&solo).await.unwrap();
        let names: Vec<String> = reopened
            .projects()
            .await
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert!(
            names.contains(&"must-survive".to_owned()),
            "data was not in the database file itself: {names:?}"
        );
        reopened.pool().close().await;
    }

    #[tokio::test]
    async fn a_populated_database_is_snapshotted_once_asked() {
        let t = TempStore::new("snap").await;
        seed(&t.store, "alpha").await;
        let db = t.store.pool().connect_options().get_filename().to_path_buf();

        // `open` deliberately no longer snapshots: `VACUUM INTO` copies the
        // whole file, and doing that inside `open` put a full copy on the
        // critical path of every launch. The app now calls `snapshot` on a
        // background task instead, so the guarantee is unchanged (one snapshot
        // per launch) while startup no longer waits for it.
        t.store.pool().close().await;
        let reopened = super::Store::open(&db).await.unwrap();
        reopened.snapshot(&db).await;
        reopened.pool().close().await;

        let backups = super::backups_dir(&db);
        let snaps: Vec<_> = std::fs::read_dir(&backups)
            .unwrap()
            .filter_map(std::result::Result::ok)
            .collect();
        assert!(!snaps.is_empty(), "no snapshot written to {}", backups.display());
    }

    #[tokio::test]
    async fn open_does_not_snapshot_because_that_would_block_startup() {
        let t = TempStore::new("nosnap").await;
        seed(&t.store, "alpha").await;
        let db = t.store.pool().connect_options().get_filename().to_path_buf();
        t.store.pool().close().await;

        let reopened = super::Store::open(&db).await.unwrap();
        reopened.pool().close().await;

        // No snapshot yet: taking one is the caller's job, off the hot path.
        let backups = super::backups_dir(&db);
        let count = std::fs::read_dir(&backups)
            .map(|rd| rd.filter_map(std::result::Result::ok).count())
            .unwrap_or(0);
        assert_eq!(count, 0, "open() wrote a snapshot; that belongs off the boot path");
    }

    #[tokio::test]
    async fn an_empty_database_beside_a_snapshot_self_heals() {
        // The scenario that cost a real registry: the file is intact and
        // migrated but holds nothing, while a good snapshot sits next to it.
        let t = TempStore::new("heal").await;
        seed(&t.store, "recovered-project").await;
        let db = t.store.pool().connect_options().get_filename().to_path_buf();
        t.store.pool().close().await;

        // Open once to produce the snapshot, closing the pool afterwards --
        // a dropped Store does not close its connections synchronously, and a
        // stray handle would block the restore copy on Windows.
        {
            let snapshotting = super::Store::open(&db).await.unwrap();
            snapshotting.snapshot(&db).await;
            snapshotting.pool().close().await;
        }
        {
            let store = super::Store::open(&db).await.unwrap();
            sqlx::query("DELETE FROM projects")
                .execute(store.pool())
                .await
                .unwrap();
            assert!(store.is_empty().await);
            store.pool().close().await;
        }

        // The next open must restore rather than present an empty library.
        let healed = super::Store::open(&db).await.unwrap();
        let names: Vec<String> = healed
            .projects()
            .await
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert!(
            names.contains(&"recovered-project".to_owned()),
            "auto-restore did not bring the project back: {names:?}"
        );
    }

    #[tokio::test]
    async fn an_empty_database_with_no_snapshot_stays_empty_and_starts_cleanly() {
        let t = TempStore::new("first-run").await;
        assert!(t.store.is_empty().await);
        assert!(t.store.projects().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn snapshots_are_pruned_to_the_keep_count() {
        let t = TempStore::new("prune-snap").await;
        seed(&t.store, "alpha").await;
        let db = t.store.pool().connect_options().get_filename().to_path_buf();
        let backups = super::backups_dir(&db);
        std::fs::create_dir_all(&backups).unwrap();
        // Fabricate more snapshots than the ceiling.
        for i in 0..8 {
            std::fs::write(backups.join(format!("deck-2020010{i}-000000.db")), b"x").unwrap();
        }
        super::prune_backups(&backups, super::BACKUPS_KEPT);
        let left = std::fs::read_dir(&backups).unwrap().count();
        assert_eq!(left, super::BACKUPS_KEPT);
    }

    #[tokio::test]
    async fn opens_creates_and_migrates() {
        let t = TempStore::new("open").await;
        // Reopening the same file must be idempotent (migrations already applied).
        let path = t.store.pool().connect_options().get_filename().to_path_buf();
        let again = super::Store::open(&path).await;
        assert!(again.is_ok());
    }
}
