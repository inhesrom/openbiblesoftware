//! The User database (#17): one SQLite file in the platform data folder holding everything the
//! user makes: preferences, the Last session, named Layouts, the Preferred order, the
//! Notebooks, Annotations and Anchors with the `notes_fts` index of Note bodies (#61),
//! Favorites and History (#155), Reading plans (#147), and Highlight styles and palettes
//! (#181). Backups the user makes and restores are `backup_store.rs` (#217).
//!
//! Safety rules: WAL journaling; `PRAGMA user_version` with forward-only migrations; a copy of
//! the file is taken before any migration runs; and at every launch a rolling backup (the
//! newest [`ROLLING_BACKUPS`] are kept) is written with SQLite's online backup API.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::StoreError;

/// `PRAGMA application_id` of a User database ("OBSU").
pub const USER_APPLICATION_ID: i32 = 0x4F42_5355;

/// Schema version this build writes.
pub const USER_SCHEMA_VERSION: u32 = 12;

pub use crate::backup::ROLLING_BACKUPS;

/// Forward-only migrations; entry `n` upgrades version `n` to `n + 1`.
const MIGRATIONS: &[&str] = &[
    // 0 → 1: preferences and the Last session.
    "CREATE TABLE preference (key TEXT PRIMARY KEY, value TEXT NOT NULL);
     CREATE TABLE last_session (id INTEGER PRIMARY KEY CHECK (id = 1), snapshot TEXT NOT NULL,
                                saved_at INTEGER NOT NULL);",
    // 1 → 2: named Layouts.
    "CREATE TABLE layout (name TEXT PRIMARY KEY COLLATE NOCASE, snapshot TEXT NOT NULL,
                          saved_at INTEGER NOT NULL);",
    // 2 → 3: Preferred order (rank 0 is the best; ranks are only compared within a type).
    "CREATE TABLE preferred_order (resource_id TEXT PRIMARY KEY, rank INTEGER NOT NULL);",
    // 3 → 4: Annotations (#61). See `annotation_store.rs`.
    crate::annotation_store::SCHEMA,
    // 4 → 5: Show in text per Notebook, whole-Verse coverage of Text ranges (#63).
    crate::annotation_store::SCHEMA_5,
    // 5 → 6: Orphaned annotations: an anchor status (#65).
    crate::annotation_store::SCHEMA_6,
    // 6 → 7: Favorites and History (#155).
    crate::favorites_store::SCHEMA_7,
    // 7 → 8: Reading plans (#147).
    crate::reading_plans_store::SCHEMA_8,
    // 8 → 9: a stored Text range must cover some text (#115).
    crate::annotation_store::SCHEMA_9,
    // 9 → 10: Highlight styles and palettes, the five built-ins seeded (#181).
    crate::highlight_style_store::SCHEMA_10,
    // 10 → 11: Note text is Note formatting; plain-text Notes are escaped to look the same
    // (#186).
    crate::annotation_store::SCHEMA_11,
    // 11 → 12: the revision an Orphaned annotation's range was last looked for in (#259).
    crate::annotation_store::SCHEMA_12,
];

pub(crate) fn sql(e: rusqlite::Error) -> StoreError {
    StoreError::Sqlite(e.to_string())
}

pub(crate) fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// What happened when the User database was opened.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenReport {
    /// The schema version found (0 for a new file).
    pub found_version: u32,
    /// The copy taken before migrating, if a migration ran on an existing file.
    pub migration_backup: Option<PathBuf>,
    /// The rolling backup written at this launch.
    pub launch_backup: Option<PathBuf>,
}

/// The User database.
#[derive(Debug)]
pub struct UserStore {
    pub(crate) conn: Connection,
    pub(crate) path: PathBuf,
}

impl UserStore {
    /// Open (creating if needed) the User database at `path`, migrate it, and write this
    /// launch's rolling backup into `path`'s folder under `backups/`.
    pub fn open(path: &Path) -> Result<(Self, OpenReport), StoreError> {
        Self::open_to(path, USER_SCHEMA_VERSION)
    }

    /// Open and migrate only up to `target` (tests make old databases with this).
    #[doc(hidden)]
    pub fn open_to(path: &Path, target: u32) -> Result<(Self, OpenReport), StoreError> {
        if let Some(dir) = path.parent()
            && !dir.as_os_str().is_empty()
        {
            std::fs::create_dir_all(dir).map_err(StoreError::Io)?;
        }
        let existed = path.is_file();
        let conn = Connection::open(path).map_err(sql)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(sql)?;
        conn.pragma_update(None, "foreign_keys", true)
            .map_err(sql)?;
        let app_id: i32 = conn
            .query_row("PRAGMA application_id", [], |r| r.get(0))
            .map_err(sql)?;
        let found: u32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(sql)?;
        if existed && found > 0 && app_id != USER_APPLICATION_ID {
            return Err(StoreError::Corrupt(format!(
                "{} is not a User database (application id {app_id:#x})",
                path.display()
            )));
        }
        if found > USER_SCHEMA_VERSION {
            return Err(StoreError::NewerFormat {
                found,
                supported: USER_SCHEMA_VERSION,
            });
        }
        let mut store = Self {
            conn,
            path: path.to_path_buf(),
        };
        let mut report = OpenReport {
            found_version: found,
            ..OpenReport::default()
        };
        if found < target {
            if existed && found > 0 {
                let backup = path.with_extension(format!("v{found}-{}.bak", now()));
                store.back_up(&backup)?;
                report.migration_backup = Some(backup);
            }
            store.migrate(found, target)?;
        }
        if store.schema_version()? >= 4 {
            store.ensure_default_notebook()?;
        }
        report.launch_backup = Some(store.rolling_backup()?);
        Ok((store, report))
    }

    fn migrate(&mut self, from: u32, to: u32) -> Result<(), StoreError> {
        migrate(&mut self.conn, from, to)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn schema_version(&self) -> Result<u32, StoreError> {
        self.conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(sql)
    }

    /// Copy the whole database to `dest` with the online backup API.
    pub fn backup_to(&self, dest: &Path) -> Result<(), StoreError> {
        let mut out = Connection::open(dest).map_err(sql)?;
        let backup = rusqlite::backup::Backup::new(&self.conn, &mut out).map_err(sql)?;
        backup
            .run_to_completion(256, std::time::Duration::ZERO, None)
            .map_err(sql)
    }

    /// The folder rolling backups go in.
    pub fn backup_dir(&self) -> PathBuf {
        self.path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("backups")
    }

    /// Write this launch's backup and delete all but the newest [`ROLLING_BACKUPS`].
    fn rolling_backup(&self) -> Result<PathBuf, StoreError> {
        let dir = self.backup_dir();
        std::fs::create_dir_all(&dir).map_err(StoreError::Io)?;
        let stem = self
            .path
            .file_stem()
            .map_or_else(|| "user".to_string(), |s| s.to_string_lossy().into_owned());
        // A sequence number keeps names ordered even within one second.
        let existing = backups_in(&dir, &stem)?;
        let next = existing.last().map_or(1, |(n, _)| n + 1);
        let dest = dir.join(format!("{stem}-{next:06}.db"));
        self.back_up(&dest)?;
        let mut all = backups_in(&dir, &stem)?;
        while all.len() > ROLLING_BACKUPS {
            let (_, oldest) = all.remove(0);
            std::fs::remove_file(&oldest).map_err(StoreError::Io)?;
        }
        Ok(dest)
    }

    // ---------------------------------------------------------------- preferences

    pub fn preference(&self, key: &str) -> Result<Option<String>, StoreError> {
        self.conn
            .query_row("SELECT value FROM preference WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()
            .map_err(sql)
    }

    pub fn set_preference(&self, key: &str, value: &str) -> Result<(), StoreError> {
        self.conn
            .execute(
                "INSERT INTO preference (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map(drop)
            .map_err(sql)
    }

    // ---------------------------------------------------------------- Last session

    pub fn last_session(&self) -> Result<Option<String>, StoreError> {
        self.conn
            .query_row("SELECT snapshot FROM last_session WHERE id = 1", [], |r| {
                r.get(0)
            })
            .optional()
            .map_err(sql)
    }

    pub fn save_last_session(&self, snapshot: &str) -> Result<(), StoreError> {
        self.conn
            .execute(
                "INSERT INTO last_session (id, snapshot, saved_at) VALUES (1, ?1, ?2)
                 ON CONFLICT (id) DO UPDATE SET snapshot = excluded.snapshot,
                                                saved_at = excluded.saved_at",
                params![snapshot, now()],
            )
            .map(drop)
            .map_err(sql)
    }

    // ---------------------------------------------------------------- Layouts

    /// Layout names, alphabetically.
    pub fn layouts(&self) -> Result<Vec<String>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT name FROM layout ORDER BY name COLLATE NOCASE")
            .map_err(sql)?;
        let rows = stmt.query_map([], |r| r.get(0)).map_err(sql)?;
        rows.collect::<Result<Vec<String>, _>>().map_err(sql)
    }

    pub fn layout(&self, name: &str) -> Result<Option<String>, StoreError> {
        self.conn
            .query_row("SELECT snapshot FROM layout WHERE name = ?1", [name], |r| {
                r.get(0)
            })
            .optional()
            .map_err(sql)
    }

    /// Save a Layout, replacing one of the same name (names ignore case).
    pub fn save_layout(&self, name: &str, snapshot: &str) -> Result<(), StoreError> {
        self.conn
            .execute(
                "INSERT INTO layout (name, snapshot, saved_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT (name) DO UPDATE SET name = excluded.name,
                     snapshot = excluded.snapshot, saved_at = excluded.saved_at",
                params![name, snapshot, now()],
            )
            .map(drop)
            .map_err(sql)
    }

    /// Rename a Layout. Fails if `to` is taken by another Layout; returns false if `from`
    /// does not exist.
    pub fn rename_layout(&self, from: &str, to: &str) -> Result<bool, StoreError> {
        let n = self
            .conn
            .execute(
                "UPDATE layout SET name = ?2 WHERE name = ?1",
                params![from, to],
            )
            .map_err(sql)?;
        Ok(n > 0)
    }

    pub fn delete_layout(&self, name: &str) -> Result<bool, StoreError> {
        let n = self
            .conn
            .execute("DELETE FROM layout WHERE name = ?1", [name])
            .map_err(sql)?;
        Ok(n > 0)
    }
}

impl UserStore {
    // ---------------------------------------------------------------- Preferred order

    /// The saved Preferred order: resource ids, best first.
    pub fn preferred_order(&self) -> Result<Vec<String>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT resource_id FROM preferred_order ORDER BY rank, resource_id")
            .map_err(sql)?;
        let rows = stmt.query_map([], |r| r.get(0)).map_err(sql)?;
        rows.collect::<Result<Vec<String>, _>>().map_err(sql)
    }

    /// Replace the saved Preferred order with `ids`, best first.
    pub fn save_preferred_order(&self, ids: &[String]) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction().map_err(sql)?;
        tx.execute("DELETE FROM preferred_order", []).map_err(sql)?;
        {
            let mut stmt = tx
                .prepare_cached(
                    "INSERT OR IGNORE INTO preferred_order (resource_id, rank) VALUES (?1, ?2)",
                )
                .map_err(sql)?;
            for (rank, id) in ids.iter().enumerate() {
                let rank = i64::try_from(rank).unwrap_or(i64::MAX);
                stmt.execute(params![id, rank]).map_err(sql)?;
            }
        }
        tx.commit().map_err(sql)
    }

    /// The last known `(resource_id, abbreviation, title)` of every Resource seen (#65): how
    /// the cards of a Resource no longer in the library name it.
    pub fn resource_names(&self) -> Result<Vec<(String, String, String)>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT resource_id, abbreviation, title FROM resource_names ORDER BY resource_id",
            )
            .map_err(sql)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(sql)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql)
    }

    /// Remember Resources' abbreviations and titles, `(resource_id, abbreviation, title)`,
    /// replacing what was known of them. Others are kept.
    pub fn save_resource_names(
        &self,
        names: &[(String, String, String)],
    ) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction().map_err(sql)?;
        {
            let mut stmt = tx
                .prepare_cached(
                    "INSERT INTO resource_names (resource_id, abbreviation, title)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT (resource_id) DO UPDATE
                     SET abbreviation = excluded.abbreviation, title = excluded.title",
                )
                .map_err(sql)?;
            for (id, abbreviation, title) in names {
                stmt.execute(params![id, abbreviation, title])
                    .map_err(sql)?;
            }
        }
        tx.commit().map_err(sql)
    }
}

/// Run the migrations from version `from` to `to` in one transaction (a User database or a
/// Backup being restored, #217).
pub(crate) fn migrate(conn: &mut Connection, from: u32, to: u32) -> Result<(), StoreError> {
    let tx = conn.transaction().map_err(sql)?;
    for v in from..to {
        let step = MIGRATIONS
            .get(v as usize)
            .ok_or_else(|| StoreError::Corrupt(format!("no migration from version {v}")))?;
        // 8 → 9 refuses, rather than drops, rows its stricter CHECK would refuse.
        if v == 8 {
            crate::annotation_store::refuse_text_ranges_covering_no_text(&tx)?;
        }
        if v == 10 {
            crate::annotation_store::escape_plain_notes(&tx)?;
        }
        tx.execute_batch(step).map_err(sql)?;
    }
    tx.pragma_update(None, "application_id", USER_APPLICATION_ID)
        .map_err(sql)?;
    tx.pragma_update(None, "user_version", to).map_err(sql)?;
    tx.commit().map_err(sql)
}

/// Rolling backups of `stem` in `dir`, oldest first, with their sequence numbers.
pub(crate) fn backups_in(dir: &Path, stem: &str) -> Result<Vec<(u64, PathBuf)>, StoreError> {
    let prefix = format!("{stem}-");
    let mut out: Vec<(u64, PathBuf)> = std::fs::read_dir(dir)
        .map_err(StoreError::Io)?
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let n = name
                .strip_prefix(&prefix)?
                .strip_suffix(".db")?
                .parse()
                .ok()?;
            Some((n, e.path()))
        })
        .collect();
    out.sort();
    Ok(out)
}
