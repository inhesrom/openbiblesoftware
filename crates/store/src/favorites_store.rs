//! Favorites and History in the User database (#155).
//!
//! A Favorite and a Visit each keep where to go back to as an opaque `location` (JSON the
//! engine writes: a Tab kind and its Current location) and a `label` to show for it. Favorites
//! sit in Favorites folders (`folder`, empty for none); History keeps the Visits of the last
//! [`HISTORY_DAYS`] days, at most [`HISTORY_LIMIT`] of them, each with the local day it was
//! made on so History can group by day.

use rusqlite::{OptionalExtension, params};

use crate::annotation::{Change, Row};
use crate::error::StoreError;
use crate::favorites::{FavoriteRow, HISTORY_DAYS, HISTORY_LIMIT, VisitRow};
use crate::synced::SyncedFavorite;
use crate::synced_store;
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
        let favorite = SyncedFavorite {
            uid: crate::annotation::new_id(),
            folder: self.folder_spelling(folder)?,
            label: label.to_string(),
            location: location.to_string(),
            position: self.next_favorite_position()?,
            added_at: crate::user::now(),
        };
        let uid = favorite.uid.clone();
        self.apply_changes(&[Change::insert(Row::Favorite(favorite))])?;
        synced_store::local_id(&self.conn, "favorite", &uid)
    }

    /// Remove a Favorite; false if there was none with that id.
    pub fn remove_favorite(&self, id: i64) -> Result<bool, StoreError> {
        let Some(before) = synced_store::read_favorite_by_id(&self.conn, id)? else {
            return Ok(false);
        };
        self.apply_changes(&[Change::delete(Row::Favorite(before))])?;
        Ok(true)
    }

    /// Move a Favorite to the end of `folder` (empty: none); false if there was none with
    /// that id.
    pub fn move_favorite(&self, id: i64, folder: &str) -> Result<bool, StoreError> {
        let Some(before) = synced_store::read_favorite_by_id(&self.conn, id)? else {
            return Ok(false);
        };
        let after = SyncedFavorite {
            folder: self.folder_spelling(folder)?,
            position: self.next_favorite_position()?,
            ..before.clone()
        };
        self.apply_changes(&[Change::update(Row::Favorite(before), Row::Favorite(after))])?;
        Ok(true)
    }

    /// The position after every Favorite's.
    fn next_favorite_position(&self) -> Result<i64, StoreError> {
        self.conn
            .query_row(
                "SELECT COALESCE(MAX(position), 0) + 1 FROM favorite",
                [],
                |r| r.get(0),
            )
            .map_err(sql)
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
        let tx = crate::user::write_tx(&self.conn)?;
        tx.execute(
            "INSERT INTO history (visited_at, day, label, location)
             VALUES (?1, date(?1, 'unixepoch', ?4), ?2, ?3)",
            params![at, label, location, crate::clock::to_local()],
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
        self.history_as_of(crate::user::now())
    }

    /// History, newest first, its `days_ago` counted to the local day of `at` (Unix seconds).
    pub fn history_as_of(&self, at: i64) -> Result<Vec<VisitRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT id, visited_at, day,
                        CAST(julianday(date(?1, 'unixepoch', ?2)) - julianday(day) AS INTEGER),
                        strftime('%H:%M', visited_at, 'unixepoch', ?2), label, location
                 FROM history ORDER BY visited_at DESC, id DESC",
            )
            .map_err(sql)?;
        let rows = stmt
            .query_map(params![at, crate::clock::to_local()], |r| {
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

    /// The local day of `at` (Unix seconds), `YYYY-MM-DD`, as History's days are kept.
    pub fn local_day(&self, at: i64) -> Result<String, StoreError> {
        self.conn
            .query_row(
                "SELECT date(?1, 'unixepoch', ?2)",
                params![at, crate::clock::to_local()],
                |r| r.get(0),
            )
            .map_err(sql)
    }

    /// Forget every Visit.
    pub fn clear_history(&self) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM history", [])
            .map(drop)
            .map_err(sql)
    }
}
