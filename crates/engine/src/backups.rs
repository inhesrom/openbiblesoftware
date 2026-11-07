//! Back up, restore and export all user data (#217).
//!
//! Everything the user makes or chooses is in the User database, so a **Backup** is one file
//! holding a copy of it (see `obs_store`'s `backup_store.rs`): Notes, Highlights, Notebooks,
//! Layouts, Reading plans, Favorites, History and every setting. The Back up & restore Tab
//! makes one (`data.back_up`), restores one after a preview of what will change
//! (`data.restore`), lists the Backups the app keeps by itself (at each launch, before an app
//! upgrade, and the **Safety backup** taken before every restore) and exports everything as a
//! zip of Markdown and JSON (`data.export`) for reading elsewhere.
//!
//! A restore replaces the whole User database and then reloads everything the Session keeps
//! of it, as at launch; the open Workspace stays as it is. **Undo restore**
//! (`data.undo_restore`) restores the Safety backup the last restore took.

use std::io::Write as _;
use std::path::{Path, PathBuf};

pub use obs_store::{
    BACKUP_EXTENSION, BackupContents, BackupFile, BackupKind, ROLLING_BACKUPS, SAFETY_BACKUPS,
};
use serde_json::{Value, json};

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params};
use crate::notes::NotesFilter;
use crate::notes_export::NotesSource;
use crate::session::Session;

/// The format version of an Export everything zip's `manifest.json`.
pub const DATA_EXPORT_VERSION: u32 = 1;

/// One line of a restore preview: how many of something there are now and after restoring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreLine {
    pub what: &'static str,
    pub now: u64,
    pub after: u64,
    /// What comes back and what goes, by name ("brings back “Romans”"); empty if nothing.
    pub detail: String,
}

/// What restoring a Backup would change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestorePreview {
    pub path: PathBuf,
    pub now: BackupContents,
    pub backup: BackupContents,
    pub lines: Vec<RestoreLine>,
    /// The Backup was written by an older app and is upgraded as it is restored.
    pub older_format: bool,
}

impl RestorePreview {
    /// Whether restoring would change any of the counted things.
    pub fn changes_anything(&self) -> bool {
        self.lines
            .iter()
            .any(|l| l.now != l.after || !l.detail.is_empty())
    }
}

/// The last restore, which Undo restore takes back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastRestore {
    /// The Backup restored.
    pub from: PathBuf,
    /// The Safety backup taken just before.
    pub safety: PathBuf,
}

/// What Export everything wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataExport {
    /// File names in the zip, in order.
    pub files: Vec<String>,
    pub annotations: usize,
}

fn count(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// "brings back “A”, “B”; removes “C”" for the names only in `after` and only in `now`.
fn name_changes(now: &[String], after: &[String]) -> String {
    let quoted = |v: Vec<&String>| {
        v.iter()
            .map(|n| format!("“{n}”"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let back: Vec<&String> = after.iter().filter(|n| !now.contains(n)).collect();
    let gone: Vec<&String> = now.iter().filter(|n| !after.contains(n)).collect();
    let mut parts = Vec::new();
    if !back.is_empty() {
        parts.push(format!("brings back {}", quoted(back)));
    }
    if !gone.is_empty() {
        parts.push(format!("removes {}", quoted(gone)));
    }
    parts.join("; ")
}

/// The preview's lines for the User database `now` and a Backup holding `backup`.
pub fn restore_lines(now: &BackupContents, backup: &BackupContents) -> Vec<RestoreLine> {
    let named = |what, n: &[String], a: &[String]| RestoreLine {
        what,
        now: count(n.len()),
        after: count(a.len()),
        detail: name_changes(n, a),
    };
    let counted = |what, now, after| RestoreLine {
        what,
        now,
        after,
        detail: String::new(),
    };
    vec![
        named("Notebooks", &now.notebooks, &backup.notebooks),
        counted("Notes", now.notes, backup.notes),
        counted("Highlights", now.highlights, backup.highlights),
        named("Layouts", &now.layouts, &backup.layouts),
        named("Reading plans", &now.reading_plans, &backup.reading_plans),
        counted("Favorites", now.favorites, backup.favorites),
        counted("History visits", now.visits, backup.visits),
        counted("Settings", now.settings, backup.settings),
    ]
}

impl Session {
    /// The Backups the app keeps by itself, newest first (none without a User database).
    pub fn kept_backups(&self) -> Vec<BackupFile> {
        self.user
            .as_ref()
            .and_then(|u| u.backups().ok())
            .unwrap_or_default()
    }

    /// When a Backup was taken, in local time: `YYYY-MM-DD HH:MM`.
    pub fn backup_time(&self, b: &BackupFile) -> String {
        self.user
            .as_ref()
            .and_then(|u| u.local_time(b.taken_at).ok())
            .unwrap_or_default()
    }

    /// The file name Back up now suggests: `Open Bible backup 2025-06-01.obsbackup`.
    pub fn backup_file_name(&self) -> String {
        format!("Open Bible backup {}.{BACKUP_EXTENSION}", self.today())
    }

    /// The file name Export everything suggests: `Open Bible data 2025-06-01.zip`.
    pub fn data_export_file_name(&self) -> String {
        format!("Open Bible data {}.zip", self.today())
    }

    fn today(&self) -> String {
        let now = i64::try_from(crate::get_bibles::now_secs()).unwrap_or(0);
        self.user
            .as_ref()
            .and_then(|u| u.local_day(now).ok())
            .unwrap_or_default()
    }

    /// Back up now: a Backup of everything to `dest`.
    pub fn back_up(&self, dest: &Path) -> Result<(), String> {
        self.user_db()?.back_up(dest)
    }

    /// What restoring the Backup at `path` would change. Reading it changes nothing.
    pub fn restore_preview(&self, path: &Path) -> Result<RestorePreview, String> {
        let user = self.user_db()?;
        let now = user.contents()?;
        let backup = user.backup_contents(path)?;
        Ok(RestorePreview {
            path: path.to_path_buf(),
            lines: restore_lines(&now, &backup),
            older_format: backup.version < now.version,
            now,
            backup,
        })
    }

    /// Restore the Backup at `path`: a Safety backup is taken, everything is replaced, and
    /// the Session reloads what it keeps of the User database. Returns the Safety backup.
    pub fn restore_backup(&mut self, path: &Path) -> Result<PathBuf, String> {
        let Some(mut user) = self.user.take() else {
            return Err(NO_USER.into());
        };
        let safety = match user.restore(path) {
            Ok(safety) => safety,
            Err(e) => {
                // Nothing changed: keep the Session as it was, undo history included.
                self.user = Some(user);
                return Err(e);
            }
        };
        // Reload everything the Session keeps of the User database, as at launch: a
        // preference the Backup does not have is back at its default.
        self.forget_user_preferences();
        self.attach_user(user);
        self.after_restore();
        self.last_restore = Some(LastRestore {
            from: path.to_path_buf(),
            safety: safety.clone(),
        });
        Ok(safety)
    }

    /// Undo restore: restore the Safety backup the last restore took. Its own Safety backup
    /// (of the restored data) is kept like any other.
    pub fn undo_restore(&mut self) -> Result<PathBuf, String> {
        let Some(last) = self.last_restore.clone() else {
            return Err("There is no restore to undo".into());
        };
        let safety = self.restore_backup(&last.safety)?;
        self.last_restore = None;
        Ok(safety)
    }

    /// The last restore of this run, which Undo restore takes back.
    pub fn last_restore(&self) -> Option<&LastRestore> {
        self.last_restore.as_ref()
    }

    /// After a restore: nothing the Session wrote of the old data may be taken as saved.
    fn after_restore(&mut self) {
        self.forget_saved_session();
        if let NotesFilter::Notebook(id) = &self.notes.filter
            && !self
                .notebooks()
                .unwrap_or_default()
                .iter()
                .any(|n| &n.id == id)
        {
            self.notes.filter = NotesFilter::All;
        }
    }

    fn user_db(&self) -> Result<&dyn crate::persist::UserData, String> {
        self.user.as_deref().ok_or_else(|| NO_USER.to_string())
    }

    /// Export everything: a zip of Markdown (each Notebook's Notes) and JSON (every table of
    /// the User database) at `dest`. It is a copy for reading elsewhere; the app restores
    /// Backups, not exports. `dest` is never the User database itself.
    pub fn export_everything(&self, dest: &Path) -> Result<DataExport, String> {
        self.user_db()?.refuse_own_file(dest)?;
        let files = self.data_export_files()?;
        let annotations = self.user_db()?.annotations(None)?.len();
        let mut out = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut out);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for (name, text) in &files {
                zip.start_file(name.as_str(), options)
                    .map_err(|e| e.to_string())?;
                zip.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
            }
            zip.finish().map_err(|e| e.to_string())?;
        }
        std::fs::write(dest, out.into_inner()).map_err(|e| format!("{}: {e}", dest.display()))?;
        Ok(DataExport {
            files: files.into_iter().map(|(n, _)| n).collect(),
            annotations,
        })
    }

    /// The files of Export everything, `(name in the zip, text)`.
    pub fn data_export_files(&self) -> Result<Vec<(String, String)>, String> {
        let user = self.user_db()?;
        let now = i64::try_from(crate::get_bibles::now_secs()).unwrap_or(0);
        let contents = user.contents()?;
        let pretty = |v: &Value| serde_json::to_string_pretty(v).unwrap_or_default();
        // Saved JSON stays JSON; anything else is kept as text.
        let parsed = |s: &str| serde_json::from_str::<Value>(s).unwrap_or_else(|_| json!(s));
        let mut files = Vec::new();

        files.push((
            "manifest.json".to_string(),
            pretty(&json!({
                "app": "Open Bible Software",
                "app_version": env!("CARGO_PKG_VERSION"),
                "export_version": DATA_EXPORT_VERSION,
                "user_database_version": contents.version,
                "made_at": now,
                "made_at_local": user.local_time(now).unwrap_or_default(),
            })),
        ));
        files.push(("README.md".to_string(), README.to_string()));

        let notebooks = user.notebooks()?;
        let mut used = Vec::<String>::new();
        for n in &notebooks {
            let export = self.notes_export(&NotesSource::Notebook(Some(n.id.clone())))?;
            let file = export.file_name(crate::NotesFormat::Markdown);
            let mut name = format!("notes/{file}");
            let mut k = 2;
            while used.contains(&name) {
                let stem = file.trim_end_matches(".md");
                name = format!("notes/{stem} ({k}).md");
                k += 1;
            }
            used.push(name.clone());
            files.push((name, export.markdown()));
        }
        files.push((
            "annotations.json".to_string(),
            pretty(&json!({
                "notebooks": notebooks,
                "highlight_palettes": user.highlight_palettes()?,
                "highlight_styles": user.highlight_styles()?,
                "annotations": user.annotations(None)?,
            })),
        ));

        let layouts: Vec<Value> = user
            .layouts()?
            .into_iter()
            .map(|name| {
                let snapshot = user.layout(&name).ok().flatten().unwrap_or_default();
                json!({ "name": name, "snapshot": parsed(&snapshot) })
            })
            .collect();
        files.push(("layouts.json".to_string(), pretty(&json!(layouts))));
        let last = user.last_session()?;
        files.push((
            "last-session.json".to_string(),
            pretty(&last.as_deref().map_or(Value::Null, parsed)),
        ));

        let favorites: Vec<Value> = user
            .favorites()?
            .into_iter()
            .map(|f| {
                json!({
                    "folder": f.folder,
                    "label": f.label,
                    "location": parsed(&f.location),
                    "added_at": f.added_at,
                })
            })
            .collect();
        files.push(("favorites.json".to_string(), pretty(&json!(favorites))));
        let history: Vec<Value> = user
            .history_as_of(now)?
            .into_iter()
            .map(|v| {
                json!({
                    "visited_at": v.visited_at,
                    "day": v.day,
                    "time": v.time,
                    "label": v.label,
                    "location": parsed(&v.location),
                })
            })
            .collect();
        files.push(("history.json".to_string(), pretty(&json!(history))));

        let mut plans = Vec::new();
        for p in user.reading_plans()? {
            let readings: Vec<Value> = user
                .plan_readings(p.id)?
                .into_iter()
                .map(|r| {
                    json!({
                        "passages": r.passages,
                        "day": r.day,
                        "read_at": r.read_at,
                    })
                })
                .collect();
            plans.push(json!({
                "name": p.name,
                "template": p.template,
                "bible": p.bible_id,
                "start_day": p.start_day,
                "weekdays": p.weekdays,
                "created_at": p.created_at,
                "readings": readings,
            }));
        }
        files.push(("reading-plans.json".to_string(), pretty(&json!(plans))));

        files.push((
            "preferred-order.json".to_string(),
            pretty(&json!(user.preferred_order()?)),
        ));
        let settings: serde_json::Map<String, Value> = user
            .preferences()?
            .into_iter()
            .map(|(k, v)| (k, parsed(&v)))
            .collect();
        files.push((
            "settings.json".to_string(),
            pretty(&Value::Object(settings)),
        ));
        Ok(files)
    }
}

const NO_USER: &str = "Your data is kept in the User database, which is not open";

/// The README of an Export everything zip.
const README: &str = "# Your Open Bible data\n\n\
This zip is a copy of everything you made in Open Bible Software, for reading or keeping \
elsewhere:\n\n\
- `notes/`: each Notebook's Notes and Highlights as Markdown, in Bible order.\n\
- `annotations.json`: every Notebook, Highlight style and palette, and Annotation with its \
anchor.\n\
- `layouts.json` and `last-session.json`: your Layouts and the Workspace as you left it.\n\
- `favorites.json`, `history.json` and `reading-plans.json`.\n\
- `preferred-order.json` and `settings.json`: your Preferred order and every setting.\n\
- `manifest.json`: when and by which version this was made.\n\n\
The app does not read this zip back. To move your data to another computer or keep a copy \
you can restore, use Back up now, which writes a `.obsbackup` file.\n";

// ------------------------------------------------------------------------------ commands

fn has_user(s: &Session) -> bool {
    s.user.is_some()
}

fn has_last_restore(s: &Session) -> bool {
    s.user.is_some() && s.last_restore.is_some()
}

const PATH: &[ParamSpec] = &[ParamSpec {
    name: "path",
    kind: ParamKind::String,
    required: true,
    help: "The file",
}];

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "data.back_up",
        label: "Back up now",
        shortcut: None,
        params: PATH,
        enabled: has_user,
        run: |s, p| {
            let path = path_of("data.back_up", &p)?;
            s.back_up(&path).map_err(|e| failed("data.back_up", e))?;
            Ok(json!({ "path": path.to_string_lossy() }))
        },
    },
    CommandSpec {
        id: "data.restore",
        label: "Restore from backup",
        shortcut: None,
        params: PATH,
        enabled: has_user,
        run: |s, p| {
            let path = path_of("data.restore", &p)?;
            let safety = s
                .restore_backup(&path)
                .map_err(|e| failed("data.restore", e))?;
            Ok(json!({ "safety_backup": safety.to_string_lossy() }))
        },
    },
    CommandSpec {
        id: "data.undo_restore",
        label: "Undo restore",
        shortcut: None,
        params: &[],
        enabled: has_last_restore,
        run: |s, _| {
            let safety = s
                .undo_restore()
                .map_err(|e| failed("data.undo_restore", e))?;
            Ok(json!({ "safety_backup": safety.to_string_lossy() }))
        },
    },
    CommandSpec {
        id: "data.export",
        label: "Export everything",
        shortcut: None,
        params: PATH,
        enabled: has_user,
        run: |s, p| {
            let path = path_of("data.export", &p)?;
            let out = s
                .export_everything(&path)
                .map_err(|e| failed("data.export", e))?;
            Ok(json!({ "files": out.files, "annotations": out.annotations }))
        },
    },
];

fn path_of(command: &'static str, p: &Params<'_>) -> Result<PathBuf, CommandError> {
    p.str("path")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| failed(command, "no file given"))
}

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}
