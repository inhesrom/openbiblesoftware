//! Favorites and History in the User database (#155).
//!
//! A Favorite and a Visit each keep where to go back to as an opaque `location` (JSON the
//! engine writes: a Tab kind and its Current location) and a `label` to show for it. Favorites
//! sit in Favorites folders (`folder`, empty for none); History keeps the Visits of the last
//! [`HISTORY_DAYS`] days, at most [`HISTORY_LIMIT`] of them, each with the local day it was
//! made on so History can group by day.

use rusqlite::{OptionalExtension, params};

use crate::error::StoreError;
use crate::favorites::{FavoriteRow, HISTORY_DAYS, HISTORY_LIMIT, VisitRow};
use crate::user::{UserStore, sql};

/// 6 → 7: Favorites and History.
pub(crate) const SCHEMA_7: &str = "
CREATE TABLE favorite (
    id INTEGER PRIMARY KEY,
    folder TEXT NOT NULL DEFAULT '',
    label TEXT NOT NULL,
    location TEXT NOT NULL,
    position INTEGER NOT NULL,
    added_at INTEGER NOT NULL
);
CREATE TABLE history (
    id INTEGER PRIMARY KEY,
    visited_at INTEGER NOT NULL,
    day TEXT NOT NULL,
    label TEXT NOT NULL,
    location TEXT NOT NULL
);
CREATE INDEX history_visited_at ON history (visited_at);
";

impl UserStore {
    /// Every Favorite: by folder (none first, then by name), then in the order added.
    pub fn favorites(&self) -> Result<Vec<FavoriteRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT id, folder, label, location, added_at FROM favorite
                 ORDER BY folder <> '', folder COLLATE NOCASE, position, id",
            )
            .map_err(sql)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(FavoriteRow {
                    id: r.get(0)?,
                    folder: r.get(1)?,
                    label: r.get(2)?,
                    location: r.get(3)?,
                    added_at: r.get(4)?,
                })
            })
            .map_err(sql)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql)
    }

    /// Add a Favorite at the end of `folder` (empty: none). Returns its id.
    pub fn add_favorite(
        &self,
        folder: &str,
        label: &str,
        location: &str,
    ) -> Result<i64, StoreError> {
        let folder = self.folder_spelling(folder)?;
        self.conn
            .execute(
                "INSERT INTO favorite (folder, label, location, position, added_at)
                 VALUES (?1, ?2, ?3, (SELECT COALESCE(MAX(position), 0) + 1 FROM favorite), ?4)",
                params![folder, label, location, crate::user::now()],
            )
            .map_err(sql)?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Remove a Favorite; false if there was none with that id.
    pub fn remove_favorite(&self, id: i64) -> Result<bool, StoreError> {
        let n = self
            .conn
            .execute("DELETE FROM favorite WHERE id = ?1", [id])
            .map_err(sql)?;
        Ok(n > 0)
    }

    /// Move a Favorite to the end of `folder` (empty: none); false if there was none with
    /// that id.
    pub fn move_favorite(&self, id: i64, folder: &str) -> Result<bool, StoreError> {
        let folder = self.folder_spelling(folder)?;
        let n = self
            .conn
            .execute(
                "UPDATE favorite SET folder = ?2,
                     position = (SELECT COALESCE(MAX(position), 0) + 1 FROM favorite)
                 WHERE id = ?1",
                params![id, folder],
            )
            .map_err(sql)?;
        Ok(n > 0)
    }

    /// Folder names ignore case: an existing folder keeps its spelling.
    fn folder_spelling(&self, folder: &str) -> Result<String, StoreError> {
        let folder = folder.trim();
        let existing: Option<String> = self
            .conn
            .query_row(
                "SELECT folder FROM favorite WHERE folder = ?1 COLLATE NOCASE LIMIT 1",
                [folder],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)?;
        Ok(existing.unwrap_or_else(|| folder.to_string()))
    }

    /// Add a Visit made now to History, and forget Visits past the cap.
    pub fn record_visit(&self, label: &str, location: &str) -> Result<(), StoreError> {
        self.record_visit_at(label, location, crate::user::now())
    }

    /// Add a Visit made at `at` (Unix seconds) to History, and forget Visits past the cap
    /// as of `at`. Tests make old Visits with this.
    #[doc(hidden)]
    pub fn record_visit_at(&self, label: &str, location: &str, at: i64) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction().map_err(sql)?;
        tx.execute(
            "INSERT INTO history (visited_at, day, label, location)
             VALUES (?1, date(?1, 'unixepoch', 'localtime'), ?2, ?3)",
            params![at, label, location],
        )
        .map_err(sql)?;
        tx.execute(
            "DELETE FROM history WHERE visited_at < ?1",
            [at - HISTORY_DAYS * 86_400],
        )
        .map_err(sql)?;
        tx.execute(
            "DELETE FROM history WHERE id NOT IN
                 (SELECT id FROM history ORDER BY visited_at DESC, id DESC LIMIT ?1)",
            [HISTORY_LIMIT],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)
    }

    /// History, newest first.
    pub fn history(&self) -> Result<Vec<VisitRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT id, visited_at, day,
                        CAST(julianday(date('now', 'localtime')) - julianday(day) AS INTEGER),
                        strftime('%H:%M', visited_at, 'unixepoch', 'localtime'), label, location
                 FROM history ORDER BY visited_at DESC, id DESC",
            )
            .map_err(sql)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(VisitRow {
                    id: r.get(0)?,
                    visited_at: r.get(1)?,
                    day: r.get(2)?,
                    days_ago: r.get(3)?,
                    time: r.get(4)?,
                    label: r.get(5)?,
                    location: r.get(6)?,
                })
            })
            .map_err(sql)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql)
    }

    /// Forget every Visit.
    pub fn clear_history(&self) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM history", [])
            .map(drop)
            .map_err(sql)
    }
}
