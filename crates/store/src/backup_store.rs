//! Back up, list and restore the User database (#217).
//!
//! - **Back up now** copies the whole database with SQLite's online backup API into one
//!   self-contained file (rollback journal, so no `-wal` file belongs with it), written to
//!   a new scratch file beside its destination and renamed into place, so a failed write never
//!   leaves half a Backup and no file already there is removed.
//! - **Restore** first reads the Backup into memory, checks it is a User database this build
//!   can read (a newer format is refused), runs the migrations on it and checks the result is
//!   whole (every table, column, index and trigger this build's format has, every row readable,
//!   SQLite's integrity check passing), so nothing is touched if the file is unusable. Then it takes a Safety backup of the database as it is, and
//!   copies the Backup's pages over the live database in one step (rebuilding the copy with
//!   the live database's page size first, if they differ).
//! - Nothing is ever written over the live database or SQLite's files beside it
//!   ([`UserStore::refuse_own_file`]), whatever path names them.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use rusqlite::{Connection, OpenFlags, params};

use crate::backup::{BackupContents, BackupFile, BackupKind, SAFETY_BACKUPS};
use crate::error::StoreError;
use crate::user::{USER_APPLICATION_ID, USER_SCHEMA_VERSION, UserStore, backups_in, migrate, sql};

/// The part of a Safety backup's file name after the database's stem.
const SAFETY_INFIX: &str = "safety-";

impl UserStore {
    /// Write a Backup of the whole database to `dest`, replacing a file there (but never the
    /// database itself, see [`UserStore::refuse_own_file`]).
    pub fn back_up(&self, dest: &Path) -> Result<(), StoreError> {
        self.refuse_own_file(dest)?;
        let name = dest
            .file_name()
            .map_or_else(|| "backup".into(), |n| n.to_string_lossy().into_owned());
        let partial = self.new_scratch_file(&dest.with_file_name(format!(".{name}")), "partial")?;
        let written = self.backup_to(&partial).and_then(|()| {
            let out = Connection::open(&partial).map_err(sql)?;
            // One file: a WAL database would want its `-wal` and `-shm` beside it.
            out.pragma_update(None, "journal_mode", "DELETE")
                .map_err(sql)?;
            drop(out);
            std::fs::rename(&partial, dest).map_err(StoreError::Io)
        });
        if written.is_err() {
            let _ = std::fs::remove_file(&partial);
        }
        written
    }

    /// Read the Backup at `path` into memory and migrate it to this build's format. Returns
    /// it with the format version it was written in. The file itself is never changed.
    pub fn open_backup(path: &Path) -> Result<(UserStore, u32), StoreError> {
        let not_a_backup = |why: &str| {
            StoreError::Corrupt(format!(
                "{} is not a backup of your data: {why}",
                path.display()
            ))
        };
        if !path.is_file() {
            return Err(StoreError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("{} not found", path.display()),
            )));
        }
        let src = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(sql)?;
        let mut conn = Connection::open_in_memory().map_err(sql)?;
        rusqlite::backup::Backup::new(&src, &mut conn)
            .and_then(|b| b.run_to_completion(256, std::time::Duration::ZERO, None))
            .map_err(|e| not_a_backup(&e.to_string()))?;
        drop(src);
        conn.pragma_update(None, "foreign_keys", true)
            .map_err(sql)?;
        let app_id: i32 = conn
            .query_row("PRAGMA application_id", [], |r| r.get(0))
            .map_err(sql)?;
        let found: u32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(sql)?;
        if app_id != USER_APPLICATION_ID || found == 0 {
            return Err(not_a_backup("it was not made by Open Bible"));
        }
        if found > USER_SCHEMA_VERSION {
            return Err(StoreError::NewerFormat {
                found,
                supported: USER_SCHEMA_VERSION,
            });
        }
        if found < USER_SCHEMA_VERSION {
            migrate(&mut conn, found, USER_SCHEMA_VERSION)?;
        }
        check_whole(&conn).map_err(|why| not_a_backup(&why))?;
        let store = UserStore {
            conn,
            path: path.to_path_buf(),
        };
        store.ensure_default_notebook()?;
        store
            .read_everything()
            .map_err(|e| not_a_backup(&format!("it is damaged ({e})")))?;
        Ok((store, found))
    }

    /// Read everything the app reads of a User database, so a row it could not read refuses
    /// the Backup now rather than failing after the restore.
    fn read_everything(&self) -> Result<(), StoreError> {
        self.contents()?;
        self.annotations(None)?;
        self.notebook_counts()?;
        for name in self.layouts()? {
            self.layout(&name)?;
        }
        self.last_session()?;
        self.highlight_palettes()?;
        self.highlight_styles()?;
        self.highlight_style_uses()?;
        self.preferred_order()?;
        self.preferences()?;
        self.resource_names()?;
        self.favorites()?;
        self.history()?;
        for plan in self.reading_plans()? {
            self.plan_readings(plan.id)?;
        }
        Ok(())
    }

    /// Refuse `dest` if writing there would overwrite this database or one of SQLite's files
    /// beside it (`-wal`, `-shm`, `-journal`), by any path naming it (a link, `..`, another
    /// case on a case-insensitive disk). Writing there would destroy the data being saved.
    ///
    /// SQLite names its files beside the database's real file: opened through a symbolic
    /// link, they sit beside the link's target, not beside the link. So both are protected.
    pub fn refuse_own_file(&self, dest: &Path) -> Result<(), StoreError> {
        let mut databases = vec![self.path.clone()];
        if let Some(real) = self.conn.path().filter(|p| !p.is_empty()) {
            databases.push(PathBuf::from(real));
        }
        if let Ok(real) = std::fs::canonicalize(&self.path) {
            databases.push(real);
        }
        let mut own = Vec::new();
        for db in databases {
            for suffix in ["-wal", "-shm", "-journal"] {
                let mut name = db.as_os_str().to_os_string();
                name.push(suffix);
                own.push(PathBuf::from(name));
            }
            own.push(db);
        }
        if own.iter().any(|f| names_one_file(dest, f)) {
            return Err(StoreError::Rejected(format!(
                "{} is where your data is kept, so nothing was written there; choose another file",
                dest.display()
            )));
        }
        Ok(())
    }

    /// Create a new, empty file named `<base>.<n>.<extension>` (`n` makes it unique) for a
    /// file being written, and return its path. A file already there is never touched, and
    /// a name that is this database or SQLite's files beside it is skipped, so removing the
    /// scratch file afterwards can only remove what this call created.
    fn new_scratch_file(&self, base: &Path, extension: &str) -> Result<PathBuf, StoreError> {
        let base = base.as_os_str().to_string_lossy().into_owned();
        let mut last = None;
        for n in 0..1000u32 {
            let path = PathBuf::from(format!("{base}.{}-{n}.{extension}", std::process::id()));
            if self.refuse_own_file(&path).is_err() {
                continue;
            }
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => return Ok(path),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
                Err(e) => return Err(StoreError::Io(e)),
            }
        }
        Err(StoreError::Io(last.unwrap_or_else(|| {
            std::io::Error::other(format!("no free name for {base}.{extension}"))
        })))
    }

    /// What the Backup at `path` holds, read as [`UserStore::open_backup`] reads it.
    pub fn backup_contents(path: &Path) -> Result<BackupContents, StoreError> {
        let (backup, found) = Self::open_backup(path)?;
        Ok(BackupContents {
            version: found,
            ..backup.contents()?
        })
    }

    /// Replace everything in this database with the Backup at `path`, migrated to this
    /// build's format. A Safety backup of the database as it was is taken first; its path is
    /// returned. An unreadable or newer Backup changes nothing.
    pub fn restore(&mut self, path: &Path) -> Result<PathBuf, StoreError> {
        let (backup, _) = Self::open_backup(path)?;
        let page_size = |c: &Connection| -> Result<i64, StoreError> {
            c.query_row("PRAGMA page_size", [], |r| r.get(0))
                .map_err(sql)
        };
        let live = page_size(&self.conn)?;
        // Pages copy only between databases of one page size. A Backup made with another is
        // rebuilt with this database's in a file beside it (an in-memory database cannot
        // change its page size).
        let rebuilt = if page_size(&backup.conn)? == live {
            None
        } else {
            std::fs::create_dir_all(self.backup_dir()).map_err(StoreError::Io)?;
            let file = ScratchFile(
                self.new_scratch_file(
                    &self
                        .backup_dir()
                        .join(format!(".{}-restoring", self.stem())),
                    "db",
                )?,
            );
            backup.backup_to(&file.0)?;
            let c = Connection::open(&file.0).map_err(sql)?;
            c.pragma_update(None, "journal_mode", "DELETE")
                .map_err(sql)?;
            c.pragma_update(None, "page_size", live).map_err(sql)?;
            c.execute_batch("VACUUM").map_err(sql)?;
            // The connection is closed before the file is removed: fields drop in order.
            Some((c, file))
        };
        let source = rebuilt.as_ref().map_or(&backup.conn, |(c, _)| c);
        let safety = self.safety_backup()?;
        rusqlite::backup::Backup::new(source, &mut self.conn)
            .and_then(|b| b.run_to_completion(256, std::time::Duration::ZERO, None))
            .map_err(sql)?;
        Ok(safety)
    }

    /// What this database holds.
    pub fn contents(&self) -> Result<BackupContents, StoreError> {
        let count = |table: &str, filter: &str| -> Result<u64, StoreError> {
            self.conn
                .query_row(&format!("SELECT COUNT(*) FROM {table} {filter}"), [], |r| {
                    r.get::<_, i64>(0)
                })
                .map(|n| u64::try_from(n).unwrap_or(0))
                .map_err(sql)
        };
        let names = |query: &str| -> Result<Vec<String>, StoreError> {
            let mut stmt = self.conn.prepare(query).map_err(sql)?;
            let rows = stmt.query_map([], |r| r.get(0)).map_err(sql)?;
            rows.collect::<Result<Vec<String>, _>>().map_err(sql)
        };
        Ok(BackupContents {
            version: self.schema_version()?,
            notebooks: self.notebooks()?.into_iter().map(|n| n.name).collect(),
            notes: count("annotations", "WHERE kind = 'note'")?,
            highlights: count("annotations", "WHERE kind = 'highlight'")?,
            layouts: self.layouts()?,
            reading_plans: names("SELECT name FROM reading_plan ORDER BY id")?,
            favorites: count("favorite", "")?,
            visits: count("history", "")?,
            settings: count("preference", "")?,
        })
    }

    /// Every saved preference, `(key, value)`, by key.
    pub fn preferences(&self) -> Result<Vec<(String, String)>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT key, value FROM preference ORDER BY key")
            .map_err(sql)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(sql)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql)
    }

    /// The local date and time of `at` (Unix seconds), `YYYY-MM-DD HH:MM`.
    pub fn local_time(&self, at: i64) -> Result<String, StoreError> {
        self.conn
            .query_row(
                "SELECT strftime('%Y-%m-%d %H:%M', ?1, 'unixepoch', 'localtime')",
                params![at],
                |r| r.get(0),
            )
            .map_err(sql)
    }

    /// The Backups the app keeps by itself, newest first: the launch backups, the Safety
    /// backups and the copies taken before an upgrade.
    pub fn backups(&self) -> Result<Vec<BackupFile>, StoreError> {
        let stem = self.stem();
        let mut out = Vec::new();
        let dir = self.backup_dir();
        if dir.is_dir() {
            for (_, path) in backups_in(&dir, &stem)? {
                out.push(backup_file(path, BackupKind::Launch));
            }
            for (_, path) in safety_backups_in(&dir, &stem)? {
                out.push(backup_file(path, BackupKind::Safety));
            }
        }
        // Before-upgrade copies sit beside the database: `user.v8-<time>.bak`.
        let folder = self.path.parent().unwrap_or_else(|| Path::new("."));
        let prefix = format!("{stem}.v");
        if let Ok(entries) = std::fs::read_dir(folder) {
            for e in entries.filter_map(Result::ok) {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with(&prefix) && name.ends_with(".bak") {
                    out.push(backup_file(e.path(), BackupKind::BeforeUpgrade));
                }
            }
        }
        out.sort_by(|a, b| b.taken_at.cmp(&a.taken_at).then(b.path.cmp(&a.path)));
        Ok(out)
    }

    /// The database file's name without its extension (`user`).
    fn stem(&self) -> String {
        self.path
            .file_stem()
            .map_or_else(|| "user".to_string(), |s| s.to_string_lossy().into_owned())
    }

    /// Take a Safety backup and delete all but the newest [`SAFETY_BACKUPS`].
    fn safety_backup(&self) -> Result<PathBuf, StoreError> {
        let dir = self.backup_dir();
        std::fs::create_dir_all(&dir).map_err(StoreError::Io)?;
        let stem = self.stem();
        let existing = safety_backups_in(&dir, &stem)?;
        let next = existing.last().map_or(1, |(n, _)| n + 1);
        let dest = dir.join(format!("{stem}-{SAFETY_INFIX}{next:06}.db"));
        self.back_up(&dest)?;
        let mut all = safety_backups_in(&dir, &stem)?;
        while all.len() > SAFETY_BACKUPS {
            let (_, oldest) = all.remove(0);
            std::fs::remove_file(&oldest).map_err(StoreError::Io)?;
        }
        Ok(dest)
    }
}

/// A file [`UserStore::new_scratch_file`] created, removed when dropped however the work on
/// it ended, so an error part way never leaves it behind.
struct ScratchFile(PathBuf);

impl Drop for ScratchFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Why the migrated database `conn` is not whole, if it is not: a table, column, index or
/// trigger this build's format has is missing, a row cannot be read, or SQLite's integrity
/// check (and the Notes search index's own) fails.
fn check_whole(conn: &Connection) -> Result<(), String> {
    let integrity: Vec<String> = conn
        .prepare("PRAGMA integrity_check")
        .and_then(|mut s| {
            s.query_map([], |r| r.get(0))?
                .collect::<Result<Vec<String>, _>>()
        })
        .map_err(|e| format!("it is damaged ({e})"))?;
    if integrity != ["ok"] {
        return Err(format!("it is damaged ({})", integrity.join("; ")));
    }
    let mut reference = Connection::open_in_memory().map_err(|e| e.to_string())?;
    migrate(&mut reference, 0, USER_SCHEMA_VERSION).map_err(|e| e.to_string())?;
    let objects = |c: &Connection| -> rusqlite::Result<Vec<(String, String)>> {
        c.prepare(
            "SELECT type, name FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY name",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect()
    };
    let columns = |c: &Connection, table: &str| -> rusqlite::Result<Vec<String>> {
        c.prepare("SELECT name FROM pragma_table_info(?1)")?
            .query_map([table], |r| r.get(0))?
            .collect()
    };
    let have = objects(conn).map_err(|e| format!("it is damaged ({e})"))?;
    for (kind, name) in objects(&reference).map_err(|e| e.to_string())? {
        if !have.iter().any(|(k, n)| *k == kind && *n == name) {
            return Err(format!("it has no {kind} `{name}`"));
        }
        if kind != "table" {
            continue;
        }
        let found = columns(conn, &name).map_err(|e| format!("it is damaged ({e})"))?;
        let wanted = columns(&reference, &name).map_err(|e| e.to_string())?;
        if let Some(missing) = wanted.iter().find(|c| !found.contains(c)) {
            return Err(format!("its table `{name}` has no column `{missing}`"));
        }
        // Every row of every table can be read.
        let read = || -> rusqlite::Result<()> {
            let mut stmt = conn.prepare(&format!("SELECT * FROM \"{name}\""))?;
            let n = stmt.column_count();
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                for i in 0..n {
                    row.get_ref(i)?;
                }
            }
            Ok(())
        };
        read().map_err(|e| format!("its table `{name}` cannot be read ({e})"))?;
    }
    conn.execute(
        "INSERT INTO notes_fts (notes_fts, rank) VALUES ('integrity-check', 1)",
        [],
    )
    .map_err(|e| format!("its Notes search index is damaged ({e})"))?;
    Ok(())
}

/// Whether `a` and `b` name one file, existing or not.
fn names_one_file(a: &Path, b: &Path) -> bool {
    if a.exists() && b.exists() {
        // By the file system's identity of the file: a hard link has its own path but is the
        // same file.
        return same_file::is_same_file(a, b).unwrap_or(false);
    }
    match (resolved(a), resolved(b)) {
        (Some(a), Some(b)) if cfg!(any(windows, target_os = "macos")) => {
            // Their disks ignore case by default.
            a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
        }
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// `path` with links, `.` and `..` resolved: the file itself, or (if it does not exist yet)
/// its folder and its name.
fn resolved(path: &Path) -> Option<PathBuf> {
    if let Ok(p) = std::fs::canonicalize(path) {
        return Some(p);
    }
    let name = path.file_name()?;
    let dir = match path.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => Path::new("."),
    };
    Some(std::fs::canonicalize(dir).ok()?.join(name))
}

fn backup_file(path: PathBuf, kind: BackupKind) -> BackupFile {
    let taken_at = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
    BackupFile {
        path,
        kind,
        taken_at,
    }
}

/// Safety backups of `stem` in `dir`, oldest first, with their sequence numbers.
fn safety_backups_in(dir: &Path, stem: &str) -> Result<Vec<(u64, PathBuf)>, StoreError> {
    let prefix = format!("{stem}-{SAFETY_INFIX}");
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
