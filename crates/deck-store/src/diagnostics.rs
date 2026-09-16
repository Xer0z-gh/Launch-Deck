//! Database self-report: what the file actually is, and whether it is healthy.
//!
//! Every value here is read back from the LIVE connection rather than repeated
//! from the constants in `store.rs`. That distinction is the entire point: this
//! crate already asks for WAL, `synchronous=FULL` and a small autocheckpoint at
//! open time, and a report that simply echoed those requests could not tell you
//! whether `SQLite` honoured them. Launch Deck has already lost a project
//! registry once to exactly that gap -- the settings looked right in the source
//! while the file on disk was running with a 1000-page checkpoint threshold and
//! every row sitting in an un-checkpointed WAL.

use std::path::PathBuf;

use deck_domain::error::{DeckError, Result};

use crate::store::Store;

/// A point-in-time description of the database file backing this store.
#[derive(Debug, Clone)]
pub struct StoreDiagnostics {
    /// Absolute path `SQLite` reports for the main database.
    pub path: PathBuf,
    /// Should be `wal`. Anything else means writes are not crash-safe here.
    pub journal_mode: String,
    /// `0` = OFF, `1` = NORMAL, `2` = FULL, `3` = EXTRA.
    pub synchronous: i64,
    /// Pages the WAL may grow to before an automatic checkpoint. The default
    /// of 1000 is what stranded a whole registry in an un-checkpointed WAL.
    pub wal_autocheckpoint: i64,
    /// Whether foreign-key enforcement is actually on for this connection.
    pub foreign_keys: bool,
    /// Bytes per page.
    pub page_size: i64,
    /// Pages currently allocated.
    pub page_count: i64,
    /// Pages allocated but unused -- large values mean the file wants a `VACUUM`.
    pub freelist_count: i64,
    /// `"ok"`, or `SQLite`'s first complaint.
    pub integrity: String,
    /// Rows in `projects`, or `-1` if the table could not be counted.
    pub projects: i64,
    /// Rows in `runs`, or `-1` if the table could not be counted.
    pub runs: i64,
    /// Rows in `settings`, or `-1` if the table could not be counted.
    pub settings: i64,
    /// Highest applied migration version, or `None` on a database with no
    /// migration table (which would itself be a finding).
    pub schema_version: Option<i64>,
}

impl StoreDiagnostics {
    /// Bytes the main database file occupies, per `SQLite`'s own page maths.
    #[must_use]
    pub fn size_bytes(&self) -> i64 {
        self.page_size * self.page_count
    }

    /// Human name for the `synchronous` level, since the raw integer is opaque.
    #[must_use]
    pub fn synchronous_label(&self) -> &'static str {
        match self.synchronous {
            0 => "OFF",
            1 => "NORMAL",
            2 => "FULL",
            3 => "EXTRA",
            _ => "unknown",
        }
    }
}

impl Store {
    /// Reads the database's real configuration and runs an integrity check.
    ///
    /// `PRAGMA integrity_check` walks the whole file, so this is deliberately
    /// only ever called from the diagnostics screen -- never on a hot path.
    ///
    /// # Errors
    /// Returns an error if the database cannot be queried at all.
    pub async fn diagnostics(&self) -> Result<StoreDiagnostics> {
        async fn pragma_i64(pool: &sqlx::SqlitePool, name: &str) -> Result<i64> {
            sqlx::query_scalar::<_, i64>(&format!("PRAGMA {name}"))
                .fetch_one(pool)
                .await
                .map_err(|e| DeckError::Storage(format!("reading PRAGMA {name}: {e}")))
        }

        let pool = self.pool();

        let journal_mode = sqlx::query_scalar::<_, String>("PRAGMA journal_mode")
            .fetch_one(pool)
            .await
            .map_err(|e| DeckError::Storage(e.to_string()))?;

        // `integrity_check` returns one row per problem, or a single "ok".
        let integrity_rows = sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
            .fetch_all(pool)
            .await
            .map_err(|e| DeckError::Storage(e.to_string()))?;
        let integrity = if integrity_rows.iter().all(|r| r == "ok") {
            "ok".to_owned()
        } else {
            integrity_rows.join("; ")
        };

        // `database_list` gives the file SQLite actually has open, which is the
        // only trustworthy answer if a restore ever repointed it.
        let path: String =
            sqlx::query_scalar("SELECT file FROM pragma_database_list WHERE name = 'main'")
                .fetch_one(pool)
                .await
                .map_err(|e| DeckError::Storage(e.to_string()))?;

        let count = |sql: &'static str| async move {
            sqlx::query_scalar::<_, i64>(sql)
                .fetch_one(pool)
                .await
                .unwrap_or(-1)
        };

        // A missing migration table is a finding, not an error: report it.
        let schema_version = sqlx::query_scalar::<_, i64>(
            "SELECT MAX(version) FROM _sqlx_migrations",
        )
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();

        Ok(StoreDiagnostics {
            path: PathBuf::from(path),
            journal_mode,
            synchronous: pragma_i64(pool, "synchronous").await?,
            wal_autocheckpoint: pragma_i64(pool, "wal_autocheckpoint").await?,
            foreign_keys: pragma_i64(pool, "foreign_keys").await? != 0,
            page_size: pragma_i64(pool, "page_size").await?,
            page_count: pragma_i64(pool, "page_count").await?,
            freelist_count: pragma_i64(pool, "freelist_count").await?,
            integrity,
            projects: count("SELECT COUNT(*) FROM projects").await,
            runs: count("SELECT COUNT(*) FROM runs").await,
            settings: count("SELECT COUNT(*) FROM settings").await,
            schema_version,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::store::test_support::TempStore;

    #[tokio::test]
    async fn reports_the_durability_settings_actually_in_force() {
        let t = TempStore::new("diag-durability").await;
        let d = t.store.diagnostics().await.expect("diagnostics");

        // These are the exact settings the data-loss incident was traced to.
        // Assert the FILE has them, not that the source asked for them -- a
        // report that echoed its own constants could not have caught that bug.
        assert_eq!(d.journal_mode.to_lowercase(), "wal");
        assert_eq!(d.synchronous, 2, "synchronous must be FULL");
        assert!(
            d.wal_autocheckpoint > 0 && d.wal_autocheckpoint <= 16,
            "autocheckpoint should be small, was {}",
            d.wal_autocheckpoint
        );
        assert!(d.foreign_keys, "foreign keys must be on");
        assert_eq!(d.integrity, "ok");
        assert!(d.schema_version.is_some(), "migrations should have run");
    }

    #[tokio::test]
    async fn reports_row_counts_and_the_real_file_path() {
        let t = TempStore::new("diag-counts").await;
        let d = t.store.diagnostics().await.expect("diagnostics");

        assert_eq!(d.projects, 0);
        assert!(d.page_size > 0 && d.page_count > 0);
        assert!(d.size_bytes() > 0);
        assert_eq!(d.synchronous_label(), "FULL");
        assert!(
            d.path.ends_with("deck.db"),
            "unexpected path {}",
            d.path.display()
        );
    }
}
