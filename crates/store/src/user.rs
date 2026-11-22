//! The User database (#17): one SQLite file in the platform data folder holding everything the
//! user makes: preferences, the Last session, named Layouts, the Preferred order, the
//! Notebooks, Annotations and Anchors with the `notes_fts` index of Note bodies (#61),
//! Favorites and History (#155), Reading plans (#147), and Highlight styles and palettes
//! (#181), and the clocks, tombstones, device id and outbox Sync needs (ADR 0022,
//! `sync_schema.rs`, `sync_clock.rs`).
//! Backups the user makes and restores are `backup_store.rs` (#217).
//!
//! Safety rules: WAL journaling; `PRAGMA user_version` with forward-only migrations; a copy of
//! the file is taken before any migration runs; and at every launch a rolling backup (the
//! newest [`ROLLING_BACKUPS`] are kept) is written with SQLite's online backup API.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};

use crate::annotation::{Change, Row};
use crate::error::StoreError;
use crate::synced::SyncedLayout;
use crate::synced_store;

/// `PRAGMA application_id` of a User database ("OBSU").
pub const USER_APPLICATION_ID: i32 = 0x4F42_5355;

/// Schema version this build writes.
pub const USER_SCHEMA_VERSION: u32 = 18;

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
    // 12 → 13: what Sync needs (ADR 0022): clocks, tombstones, the device id, how far other
    // devices' ops are applied, and ids for Layouts, Favorites and Reading plans.
    crate::sync_schema::SCHEMA_13,
    // 13 → 14: the clocks of list-preference items and the outbox of ops for the Sync folder
    // (ADR 0022 rule 2).
    crate::sync_schema::SCHEMA_14,
    // 14 → 15: what each delete of Synced data covers, so an edit it did not see beats it
    // (ADR 0022 rule 3, S28b).
    crate::sync_schema::SCHEMA_15,
    // 15 → 16: Sync conflicts' copies, the rows of deleted parents and the ops held back
    // (ADR 0022 rule 3, S28b-2).
    crate::sync_schema::SCHEMA_16,
    // 16 → 17: the device ids this database had before, and a restore's pending choice
    // (ADR 0022 rule 7, S28c).
    crate::sync_schema::SCHEMA_17,
    // 17 → 18: the order this database gave up its former device ids (ADR 0022 rule 7).
    crate::sync_schema::SCHEMA_18,
];

/// How long a write on one connection to the User database waits for another connection's
/// write transaction to finish (the sync worker's, [`UserStore::second_connection`]) before it
/// fails with "database is locked". Long on purpose: a user's edit waits out the longest merge
/// transaction (a snapshot merged at a join) rather than be refused; it waits for no more than
/// the one transaction it found running ([`WriteTurns`]).
pub const BUSY_TIMEOUT: Duration = Duration::from_secs(10);

/// How long after the first connection last found the User database locked the second one
/// waits before its next write transaction, so the first (the UI's, a user's edit) goes
/// next rather than wait for every transaction of a merge in turn.
const FIRST_GOES_FIRST: Duration = Duration::from_millis(20);

/// How long the first connection sleeps each time it finds the User database locked.
const BUSY_RETRY: Duration = Duration::from_micros(100);

/// How a User database's connections take turns writing: the first (the [`UserStore`]
/// opened by [`UserStore::open`], the UI's) goes first. SQLite's own busy wait backs off
/// further each time it finds the database locked, so a write waiting while the sync
/// worker's merge runs one transaction after another would wait for most of them.
#[derive(Debug)]
pub(crate) struct WriteTurns {
    epoch: Instant,
    /// When the first connection last found the database locked, in microseconds after
    /// `epoch` (0: never).
    first_waited_us: AtomicU64,
}

impl Default for WriteTurns {
    fn default() -> Self {
        Self {
            epoch: Instant::now(),
            first_waited_us: AtomicU64::new(0),
        }
    }
}

impl WriteTurns {
    fn now_us(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_micros())
            .unwrap_or(u64::MAX)
            .max(1)
    }

    /// The first connection's busy handler: note that it waits, and try again soon, for at
    /// most [`BUSY_TIMEOUT`].
    fn first_busy_handler(turns: Arc<Self>) -> impl FnMut(i32) -> bool + Send + 'static {
        let mut since = Instant::now();
        move |count| {
            if count == 0 {
                since = Instant::now();
            }
            turns
                .first_waited_us
                .store(turns.now_us(), Ordering::SeqCst);
            if since.elapsed() >= BUSY_TIMEOUT {
                return false;
            }
            std::thread::sleep(BUSY_RETRY);
            true
        }
    }

    /// Before the second connection's write transaction: wait while the first waits to write
    /// (it found the database locked within [`FIRST_GOES_FIRST`]), at most [`BUSY_TIMEOUT`].
    fn let_the_first_go(&self) {
        let deadline = Instant::now() + BUSY_TIMEOUT;
        let window = u64::try_from(FIRST_GOES_FIRST.as_micros()).unwrap_or(u64::MAX);
        loop {
            let waited = self.first_waited_us.load(Ordering::SeqCst);
            if waited == 0 || self.now_us().saturating_sub(waited) > window {
                return;
            }
            if Instant::now() >= deadline {
                return;
            }
            std::thread::sleep(BUSY_RETRY);
        }
    }
}

pub(crate) fn sql(e: rusqlite::Error) -> StoreError {
    StoreError::Sqlite(e.to_string())
}

pub(crate) fn now() -> i64 {
    crate::clock::now_secs()
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
    /// Every Change applied, for tests (`take_applied_changes`).
    #[cfg(feature = "test-support")]
    pub(crate) recorded: std::cell::RefCell<Vec<Change>>,
    /// The clock `apply_changes` stamps Changes with (`sync_clock.rs`), shared with every
    /// [`UserStore::second_connection`] opened from this one, so the connections hand out one
    /// sequence of clocks between them.
    pub(crate) clock: Arc<Mutex<crate::sync_clock::ClockState>>,
    /// How this connection and the others to the database take turns writing.
    turns: Arc<WriteTurns>,
    /// This is a [`SecondConnection`]: it lets the first write first.
    second: bool,
}

/// A second connection to a User database, to open on another thread (the sync worker, plan
/// S29b): the database's path and the clock it shares with the [`UserStore`] it came from.
/// Every clock either connection hands out is after every clock the other handed out or
/// observed. Its clones share one switch, [`SecondConnection::refuse_writes`], for when the
/// database is about to be replaced (a restore) while the other thread still works.
#[derive(Debug, Clone)]
pub struct SecondConnection {
    path: PathBuf,
    schema: u32,
    clock: Arc<Mutex<crate::sync_clock::ClockState>>,
    turns: Arc<WriteTurns>,
    refused: Arc<AtomicBool>,
}

impl SecondConnection {
    /// From now on every transaction a connection opened from this one commits is rolled back
    /// instead (an SQLite commit hook), so work left running on the old data never writes into
    /// what replaces it.
    pub fn refuse_writes(&self) {
        self.refused.store(true, Ordering::SeqCst);
    }

    /// Whether [`SecondConnection::refuse_writes`] was called.
    pub fn writes_refused(&self) -> bool {
        self.refused.load(Ordering::SeqCst)
    }

    /// Open it: the database must be there, at the schema the first connection had. Nothing
    /// is migrated or backed up (the first connection did that at its open).
    pub fn open(&self) -> Result<UserStore, StoreError> {
        let mut conn = Connection::open_with_flags(
            &self.path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(sql)?;
        let refused = Arc::clone(&self.refused);
        // The hook runs while the commit holds the write lock, which the restore needs too:
        // a commit it lets through is done before the restore starts copying.
        conn.commit_hook(Some(move || refused.load(Ordering::SeqCst)))
            .map_err(sql)?;
        let found: u32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(sql)?;
        if found != self.schema {
            return Err(StoreError::Rejected(format!(
                "the User database is at schema {found}, not {}",
                self.schema
            )));
        }
        conn.pragma_update(None, "foreign_keys", true)
            .map_err(sql)?;
        conn.busy_timeout(BUSY_TIMEOUT).map_err(sql)?;
        Ok(UserStore {
            clock: Arc::clone(&self.clock),
            turns: Arc::clone(&self.turns),
            second: true,
            ..UserStore::from_parts(conn, self.path.clone())
        })
    }
}

impl UserStore {
    pub(crate) fn from_parts(conn: Connection, path: PathBuf) -> Self {
        Self {
            conn,
            path,
            #[cfg(feature = "test-support")]
            recorded: Default::default(),
            clock: Default::default(),
            turns: Default::default(),
            second: false,
        }
    }

    /// A write transaction on the User database: `BEGIN IMMEDIATE`, so it takes the write
    /// lock before it reads. A deferred one that read before another connection committed
    /// could not write at all (`SQLITE_BUSY_SNAPSHOT`, which no busy wait waits out). On a
    /// second connection, it lets a write waiting on the first go first ([`WriteTurns`]).
    pub(crate) fn write_tx(&self) -> Result<Transaction<'_>, StoreError> {
        if self.second {
            self.turns.let_the_first_go();
        }
        Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate).map_err(sql)
    }

    /// What another thread needs to open a second connection to this User database sharing
    /// its clock (the sync worker, plan S29b).
    pub fn second_connection(&self) -> Result<SecondConnection, StoreError> {
        Ok(SecondConnection {
            path: self.path.clone(),
            schema: self.schema_version()?,
            clock: Arc::clone(&self.clock),
            turns: Arc::clone(&self.turns),
            refused: Arc::default(),
        })
    }

    /// The clock's state, for this connection to hand out the next clock or take one in.
    pub(crate) fn clock_state(&self) -> MutexGuard<'_, crate::sync_clock::ClockState> {
        // A panic while it was held left it as it was: a clock handed out was noted first.
        self.clock.lock().unwrap_or_else(PoisonError::into_inner)
    }

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
        // Which database it is and its format are read before anything is written (WAL
        // journaling persists in the file), so a refused file is left exactly as it was.
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
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(sql)?;
        conn.pragma_update(None, "foreign_keys", true)
            .map_err(sql)?;
        let mut store = Self::from_parts(conn, path.to_path_buf());
        let handler = WriteTurns::first_busy_handler(Arc::clone(&store.turns));
        store.conn.busy_handler(Some(handler)).map_err(sql)?;
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
        if store.schema_version()? >= 13 {
            store.ensure_device()?;
        }
        if store.schema_version()? >= 4 {
            store.ensure_default_notebook()?;
        }
        report.launch_backup = Some(store.rolling_backup()?);
        Ok((store, report))
    }

    /// Give the database this device's id if it has none (`sync_schema::ensure_device`).
    pub(crate) fn ensure_device(&self) -> Result<(), StoreError> {
        crate::sync_schema::ensure_device(&self.conn)
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

    /// Set preference `key`. A synced one (`SYNCED_PREFERENCES`) is written as [`Change`]s:
    /// item by item for a list preference (`preference_items.rs`), else whole.
    pub fn set_preference(&self, key: &str, value: &str) -> Result<(), StoreError> {
        if !crate::sync_schema::is_synced_preference(key) {
            return synced_store::write_preference(&self.conn, key, value);
        }
        let changes = synced_store::preference_changes(&self.conn, key, value)?;
        if changes.is_empty() {
            return Ok(());
        }
        self.apply_changes(&changes)
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

    /// Save a Layout, replacing one of the same name (names ignore case). A new Layout gets
    /// an id; one replaced keeps its own.
    pub fn save_layout(&self, name: &str, snapshot: &str) -> Result<(), StoreError> {
        let before = synced_store::read_layout_by_name(&self.conn, name)?;
        let after = SyncedLayout {
            id: before
                .as_ref()
                .map_or_else(crate::annotation::new_id, |l| l.id.clone()),
            name: name.to_string(),
            snapshot: snapshot.to_string(),
            saved_at: now(),
        };
        self.apply_changes(&[Change {
            before: before.map(Row::Layout),
            after: Some(Row::Layout(after)),
        }])
    }

    /// Rename a Layout. Fails if `to` is taken by another Layout; returns false if `from`
    /// does not exist.
    pub fn rename_layout(&self, from: &str, to: &str) -> Result<bool, StoreError> {
        let Some(before) = synced_store::read_layout_by_name(&self.conn, from)? else {
            return Ok(false);
        };
        if before.name == to {
            return Ok(true);
        }
        let after = SyncedLayout {
            name: to.to_string(),
            ..before.clone()
        };
        self.apply_changes(&[Change::update(Row::Layout(before), Row::Layout(after))])?;
        Ok(true)
    }

    pub fn delete_layout(&self, name: &str) -> Result<bool, StoreError> {
        let Some(before) = synced_store::read_layout_by_name(&self.conn, name)? else {
            return Ok(false);
        };
        self.apply_changes(&[Change::delete(Row::Layout(before))])?;
        Ok(true)
    }
}

impl UserStore {
    // ---------------------------------------------------------------- Preferred order

    /// The saved Preferred order: resource ids, best first.
    pub fn preferred_order(&self) -> Result<Vec<String>, StoreError> {
        Ok(synced_store::read_preferred_order(&self.conn)?.resource_ids)
    }

    /// Replace the saved Preferred order with `ids`, best first (an id given twice keeps its
    /// first place). It is one row (ADR 0022 rule 3).
    pub fn save_preferred_order(&self, ids: &[String]) -> Result<(), StoreError> {
        let before = synced_store::read_preferred_order(&self.conn)?;
        let after = synced_store::preferred_order_of(ids);
        if before == after {
            return Ok(());
        }
        self.apply_changes(&[Change::update(
            Row::PreferredOrder(before),
            Row::PreferredOrder(after),
        )])
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
        let tx = self.write_tx()?;
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
        if v == 12 {
            crate::sync_schema::give_rows_ids(&tx)?;
        }
        tx.execute_batch(step).map_err(sql)?;
        if v == 12 {
            crate::sync_schema::make_device(&tx)?;
        }
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
