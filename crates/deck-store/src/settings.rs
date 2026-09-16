//! App settings: a string key-value table.
//!
//! Values are opaque strings; callers that need structure store JSON. Theme,
//! view preferences and window state all live here, which is what makes them
//! survive a reinstall of the frontend cache.

use deck_domain::error::Result;
use sqlx::Row;

use crate::store::{db_err, Store};

/// Settings key holding the JSON array of dismissed folders.
const DISMISSED_KEY: &str = "scan.dismissed";

impl Store {
    /// Reads a setting.
    ///
    /// # Errors
    ///
    /// Returns [`deck_domain::DeckError::Storage`] on a database failure.
    pub async fn setting(&self, key: &str) -> Result<Option<String>> {
        let row = sqlx::query("SELECT value FROM settings WHERE key = ?1")
            .bind(key)
            .fetch_optional(self.pool())
            .await
            .map_err(|e| db_err(&e))?;
        Ok(row.map(|r| r.get::<String, _>("value")))
    }

    /// Writes a setting, replacing any existing value.
    ///
    /// # Errors
    ///
    /// Returns [`deck_domain::DeckError::Storage`] on a database failure.
    pub async fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(value)
        .execute(self.pool())
        .await
        .map_err(|e| db_err(&e))?;
        Ok(())
    }
}

/// Settings key holding the workspace roots the user has scanned.
const SCAN_ROOTS_KEY: &str = "scan_roots";

impl Store {
    /// The workspace roots the user has scanned, in insertion order.
    ///
    /// These are the durable source of truth for the library: given the roots,
    /// the whole project list can be re-derived by scanning. That makes the
    /// registry reconstructible rather than irreplaceable.
    ///
    /// # Errors
    ///
    /// Returns [`deck_domain::DeckError::Storage`] on a database failure. A
    /// malformed stored value yields an empty list rather than an error, so a
    /// corrupt setting cannot stop the app from starting.
    pub async fn scan_roots(&self) -> Result<Vec<std::path::PathBuf>> {
        let Some(raw) = self.setting(SCAN_ROOTS_KEY).await? else {
            return Ok(Vec::new());
        };
        Ok(serde_json::from_str::<Vec<String>>(&raw)
            .unwrap_or_default()
            .into_iter()
            .map(std::path::PathBuf::from)
            .collect())
    }

    /// Records a scanned workspace root, keeping the list unique and ordered.
    ///
    /// # Errors
    ///
    /// Returns [`deck_domain::DeckError::Storage`] on a database failure.
    pub async fn remember_scan_root(&self, root: &std::path::Path) -> Result<()> {
        let mut roots = self.scan_roots().await?;
        if roots.iter().any(|r| r == root) {
            return Ok(());
        }
        roots.push(root.to_path_buf());
        let encoded: Vec<String> = roots.iter().map(|r| r.display().to_string()).collect();
        let json = serde_json::to_string(&encoded)
            .map_err(|e| deck_domain::DeckError::Storage(e.to_string()))?;
        self.set_setting(SCAN_ROOTS_KEY, &json).await
    }

    /// Forgets a scanned root.
    ///
    /// # Errors
    ///
    /// Returns [`deck_domain::DeckError::Storage`] on a database failure.
    pub async fn forget_scan_root(&self, root: &std::path::Path) -> Result<()> {
        let roots: Vec<String> = self
            .scan_roots()
            .await?
            .into_iter()
            .filter(|r| r != root)
            .map(|r| r.display().to_string())
            .collect();
        let json = serde_json::to_string(&roots)
            .map_err(|e| deck_domain::DeckError::Storage(e.to_string()))?;
        self.set_setting(SCAN_ROOTS_KEY, &json).await
    }

    /// Folders the user removed, which the scanner must not offer again.
    ///
    /// # Why removing needed a memory
    ///
    /// The scan runs every ten minutes and adds any project-shaped folder it
    /// finds that is not already registered. Removing an entry deleted the
    /// row -- and the next sweep found the folder, saw no row, and put it
    /// straight back. Removing junk was therefore futile: the Zig toolchain,
    /// a `gitleaks` binary and six date-stamped agent snapshots could be
    /// deleted all morning and be back by lunch.
    ///
    /// Archiving already survived a rescan (the row still exists, so the
    /// scanner skips it), but archiving keeps it in the library. "I never want
    /// to see this" needed somewhere to live, and this is it.
    ///
    /// Stored as a JSON array in `settings` rather than as its own table: it
    /// is a short list read once per sweep, and a table plus a migration for
    /// that is more machinery than the question deserves.
    ///
    /// # Errors
    ///
    /// Propagates any database failure.
    pub async fn dismissed_roots(&self) -> Result<Vec<String>> {
        let raw = self.setting(DISMISSED_KEY).await?;
        Ok(raw
            .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
            .unwrap_or_default())
    }

    /// Adds a folder to the dismissed list, if it is not already there.
    ///
    /// # Errors
    ///
    /// Propagates any database failure.
    pub async fn dismiss_root(&self, root: &str) -> Result<()> {
        let mut all = self.dismissed_roots().await?;
        if all.iter().any(|r| r.eq_ignore_ascii_case(root)) {
            return Ok(());
        }
        all.push(root.to_owned());
        let encoded = serde_json::to_string(&all)
            .map_err(|e| deck_domain::error::DeckError::Storage(e.to_string()))?;
        self.set_setting(DISMISSED_KEY, &encoded).await
    }

    /// Forgets a dismissal, so the folder can be found again.
    ///
    /// Adding a folder by hand clears it: asking for something explicitly is
    /// the clearest possible statement that the earlier "never show me this"
    /// no longer applies.
    ///
    /// # Errors
    ///
    /// Propagates any database failure.
    pub async fn undismiss_root(&self, root: &str) -> Result<()> {
        let all: Vec<String> = self
            .dismissed_roots()
            .await?
            .into_iter()
            .filter(|r| !r.eq_ignore_ascii_case(root))
            .collect();
        let encoded = serde_json::to_string(&all)
            .map_err(|e| deck_domain::error::DeckError::Storage(e.to_string()))?;
        self.set_setting(DISMISSED_KEY, &encoded).await
    }
}

#[cfg(test)]
mod tests {
    use crate::store::test_support::TempStore;

    #[tokio::test]
    async fn missing_setting_is_none() {
        let t = TempStore::new("set-none").await;
        assert_eq!(t.store.setting("theme").await.unwrap(), None);
    }

    #[tokio::test]
    async fn scan_roots_round_trip_and_dedupe() {
        let t = TempStore::new("roots").await;
        assert!(t.store.scan_roots().await.unwrap().is_empty());

        let a = std::path::Path::new(r"D:\Workspace\Dev");
        let b = std::path::Path::new(r"D:\Other");
        t.store.remember_scan_root(a).await.unwrap();
        t.store.remember_scan_root(b).await.unwrap();
        // Re-remembering must not duplicate: the list is a set with an order.
        t.store.remember_scan_root(a).await.unwrap();

        let roots = t.store.scan_roots().await.unwrap();
        assert_eq!(roots, vec![a.to_path_buf(), b.to_path_buf()]);

        t.store.forget_scan_root(a).await.unwrap();
        assert_eq!(t.store.scan_roots().await.unwrap(), vec![b.to_path_buf()]);
    }

    #[tokio::test]
    async fn a_corrupt_scan_roots_setting_yields_an_empty_list_not_an_error() {
        // A malformed setting must never stop the app from starting.
        let t = TempStore::new("roots-bad").await;
        t.store.set_setting("scan_roots", "not json at all").await.unwrap();
        assert!(t.store.scan_roots().await.unwrap().is_empty());
    }

    /// Removing junk has to outlast the next ten-minute sweep, or it is not
    /// removal -- it is a pause.
    #[tokio::test]
    async fn a_dismissed_root_is_remembered_and_can_be_taken_back() {
        let t = TempStore::new("dismiss").await;
        assert!(t.store.dismissed_roots().await.unwrap().is_empty());

        t.store.dismiss_root(r"D:\Workspace\Tools\zig").await.unwrap();
        t.store.dismiss_root(r"D:\Workspace\Ops\Snapshot").await.unwrap();
        // Dismissing twice must not duplicate; the scanner compares this list
        // against every draft on every sweep.
        t.store.dismiss_root(r"D:\Workspace\Tools\zig").await.unwrap();

        let all = t.store.dismissed_roots().await.unwrap();
        assert_eq!(all.len(), 2, "no duplicates: {all:?}");

        // Windows paths are case-insensitive, and the scanner's casing is not
        // guaranteed to match whatever was stored.
        t.store.dismiss_root(r"d:\workspace\tools\ZIG").await.unwrap();
        assert_eq!(t.store.dismissed_roots().await.unwrap().len(), 2);

        t.store.undismiss_root(r"D:\Workspace\Tools\zig").await.unwrap();
        let left = t.store.dismissed_roots().await.unwrap();
        assert_eq!(left, vec![r"D:\Workspace\Ops\Snapshot".to_owned()]);
    }

    #[tokio::test]
    async fn a_corrupt_dismissed_setting_yields_an_empty_list_not_an_error() {
        // Same rule as the scan roots above: a malformed setting must never
        // stop the library from being scanned at all.
        let t = TempStore::new("dismiss-bad").await;
        t.store.set_setting("scan.dismissed", "{ not an array }").await.unwrap();
        assert!(t.store.dismissed_roots().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn set_get_and_overwrite() {
        let t = TempStore::new("set-rw").await;
        t.store.set_setting("theme", "dark").await.unwrap();
        assert_eq!(t.store.setting("theme").await.unwrap().as_deref(), Some("dark"));
        t.store.set_setting("theme", "system").await.unwrap();
        assert_eq!(
            t.store.setting("theme").await.unwrap().as_deref(),
            Some("system")
        );
    }
}
