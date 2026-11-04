//! Import orchestration: filesystem/parsing/writing/indexing stay on the engine job queue.
//! The report is user history, saved in the User database, never inside a Resource.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

pub use obs_formats::ImportReport;
use obs_model::Resource;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::{
    Effect, JobContext, JobEvent, JobEventKind, JobId, LibraryEntry, OpenRequest, Session, TabKind,
};

pub const DEFERRED_INDEX: &str = "Search index: built when Search arrives (#58)";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImportPhase {
    Parse,
    Index,
    Complete,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImportIndexStatus {
    Deferred,
    Built,
    Failed(String),
}

/// #58 fills this hook. It runs on the Import worker after the immutable `.obr` is written.
/// Implementations report actual index progress through `ctx.progress`; no simulated work.
pub trait ImportIndexer: std::fmt::Debug + Send + Sync {
    fn build(
        &self,
        resource: &Resource,
        path: &Path,
        ctx: &JobContext,
    ) -> Result<ImportIndexStatus, String>;
}

#[derive(Debug)]
struct DeferredIndexer;
impl ImportIndexer for DeferredIndexer {
    fn build(&self, _: &Resource, _: &Path, _: &JobContext) -> Result<ImportIndexStatus, String> {
        Ok(ImportIndexStatus::Deferred)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportRecord {
    pub resource_id: String,
    pub source: String,
    pub phase: ImportPhase,
    pub done: u64,
    pub total: u64,
    pub report: Option<ImportReport>,
    pub index: ImportIndexStatus,
    pub error: Option<String>,
}

#[derive(Debug)]
pub struct Imports {
    pub(crate) reports: BTreeMap<String, ImportRecord>,
    active: BTreeMap<JobId, String>,
    indexer: Arc<dyn ImportIndexer>,
}

impl Default for Imports {
    fn default() -> Self {
        Self {
            reports: BTreeMap::new(),
            active: BTreeMap::new(),
            indexer: Arc::new(DeferredIndexer),
        }
    }
}

impl Imports {
    pub fn report(&self, id: &str) -> Option<&ImportRecord> {
        self.reports.get(id)
    }
    pub fn is_running(&self) -> bool {
        !self.active.is_empty()
    }
    pub fn set_indexer(&mut self, indexer: Arc<dyn ImportIndexer>) {
        self.indexer = indexer;
    }
}

fn pref(id: &str) -> String {
    format!("import-report/{id}")
}

impl Session {
    /// Restore latest reports alongside Resource metadata at startup.
    pub(crate) fn restore_import_reports(&mut self) {
        let Some(user) = &self.user else {
            return;
        };
        for entry in self.library.entries() {
            let id = &entry.manifest.resource_id;
            if let Ok(Some(json)) = user.preference(&pref(id))
                && let Ok(record) = serde_json::from_str::<ImportRecord>(&json)
                && record.resource_id == *id
            {
                self.imports.reports.insert(id.clone(), record);
            }
        }
    }

    /// Apply Import events without blocking. Return unrelated events for the host's other
    /// job consumers; do not silently swallow Search/Guide results.
    pub fn poll_imports(&mut self) -> Vec<JobEvent> {
        let mut other = Vec::new();
        for event in self.jobs().drain_events() {
            let Some(id) = self.imports.active.get(&event.id).cloned() else {
                other.push(event);
                continue;
            };
            let Some(record) = self.imports.reports.get_mut(&id) else {
                continue;
            };
            let terminal = event.kind.is_terminal();
            match event.kind {
                JobEventKind::Phase(name) if name == "Index" => {
                    record.phase = ImportPhase::Index;
                    record.done = 0;
                    record.total = 0;
                }
                JobEventKind::Progress { done, total } => {
                    record.done = done;
                    record.total = total;
                }
                JobEventKind::Finished(out) => {
                    if let Ok(out) = out.downcast::<Imported>() {
                        record.phase = ImportPhase::Complete;
                        record.report = Some(out.report);
                        record.index = out.index;
                        self.library.add(out.entry);
                        if let Err(e) = self.save_preferred_order() {
                            self.workspace.push_effect(Effect::Message(format!(
                                "Could not save the Preferred order: {e}"
                            )));
                        }
                        if let Some(user) = &self.user
                            && let Some(record) = self.imports.reports.get(&id)
                        {
                            let result = serde_json::to_string(record)
                                .map_err(|e| e.to_string())
                                .and_then(|json| user.set_preference(&pref(&id), &json));
                            if let Err(e) = result {
                                self.workspace.push_effect(Effect::Message(format!(
                                    "Could not save the Import report: {e}"
                                )));
                            }
                        }
                    }
                }
                JobEventKind::Failed(error) => {
                    record.phase = ImportPhase::Failed;
                    record.error = Some(error);
                }
                JobEventKind::Cancelled => {
                    record.phase = ImportPhase::Failed;
                    record.error = Some("Import cancelled".into());
                }
                _ => {}
            }
            if terminal {
                self.imports.active.remove(&event.id);
            }
        }
        other
    }
}

/// Only desktop builds construct it; wasm still matches the job output type.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
struct Imported {
    entry: LibraryEntry,
    report: ImportReport,
    index: ImportIndexStatus,
}

const SOURCE: ParamSpec = ParamSpec {
    name: "path",
    kind: ParamKind::String,
    required: true,
    help: "USFM file or folder on this machine",
};
const ID: ParamSpec = ParamSpec {
    name: "id",
    kind: ParamKind::String,
    required: true,
    help: "Import report's Resource id",
};

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "library.import",
        label: "Import…",
        shortcut: None,
        params: &[],
        enabled: desktop,
        run: |s, _| {
            s.workspace.push_effect(Effect::PickImport);
            Ok(Value::Null)
        },
    },
    CommandSpec {
        id: "import.start",
        label: "Import selected source",
        shortcut: None,
        params: &[SOURCE],
        enabled: desktop,
        run: start,
    },
    CommandSpec {
        id: "import.report",
        label: "Latest Import report",
        shortcut: None,
        params: &[ID],
        enabled: always,
        run: open_report,
    },
];

fn desktop(_: &Session) -> bool {
    cfg!(not(target_arch = "wasm32"))
}
fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}
fn open_report(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let id = p.str("id").unwrap_or_default();
    if s.imports.report(id).is_none() {
        return Err(failed(
            "import.report",
            "No Import report for this Resource",
        ));
    }
    let tab = s.workspace.open(
        TabKind::ImportReport(id.into()),
        "Import report".into(),
        OpenRequest::default(),
    );
    Ok(json!({"tab": tab.0}))
}

/// The browser has no local files to read and no `.obr` writer; `desktop` keeps the
/// command disabled there.
#[cfg(target_arch = "wasm32")]
fn start(_: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    Err(failed("import.start", "Local file Import requires desktop"))
}

#[cfg(not(target_arch = "wasm32"))]
fn start(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let path = std::path::PathBuf::from(p.str("path").unwrap_or_default());
    if path.as_os_str().is_empty() {
        return Err(failed("import.start", "Choose a USFM file or folder"));
    }
    let dir = s
        .library
        .directory()
        .map(Path::to_path_buf)
        .or_else(crate::default_library_dir)
        .ok_or_else(|| failed("import.start", "No library directory"))?;
    s.library.set_directory(dir.clone());
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("imported");
    let slug = obs_formats::slugify(stem);
    // Every Import is a new edition, never an implicit revision or overwrite of user text.
    let mut n = 1u64;
    let id = loop {
        let id = format!("{}-{n}", slug.chars().take(44).collect::<String>());
        if s.library.get(&id).is_none() && s.imports.report(&id).is_none() {
            break id;
        }
        n += 1;
    };
    let record = ImportRecord {
        resource_id: id.clone(),
        source: path.display().to_string(),
        phase: ImportPhase::Parse,
        done: 0,
        total: 0,
        report: None,
        index: ImportIndexStatus::Deferred,
        error: None,
    };
    let indexer = Arc::clone(&s.imports.indexer);
    let worker_id = id.clone();
    let job = s.jobs().submit_fn("Import", move |ctx| {
        import_source(&path, &dir, &worker_id, indexer.as_ref(), ctx)
    });
    s.imports.active.insert(job, id.clone());
    s.imports.reports.insert(id.clone(), record);
    let tab = s.workspace.open(
        TabKind::ImportReport(id.clone()),
        "Import report".into(),
        OpenRequest::default(),
    );
    Ok(json!({"id": id, "tab": tab.0, "job": job.0}))
}

#[cfg(not(target_arch = "wasm32"))]
fn import_source(
    path: &Path,
    dir: &Path,
    id: &str,
    indexer: &dyn ImportIndexer,
    ctx: &JobContext,
) -> Result<Imported, String> {
    use obs_formats::{UsfmImport, UsfmOptions};
    let run = || -> Result<Imported, Box<dyn std::error::Error>> {
        ctx.phase("Parse");
        let mut files = if path.is_dir() {
            let mut files = Vec::new();
            for entry in std::fs::read_dir(path)? {
                let p = entry?.path();
                if p.is_file()
                    && p.extension()
                        .and_then(|s| s.to_str())
                        .is_some_and(|s| matches!(s.to_ascii_lowercase().as_str(), "sfm" | "usfm"))
                {
                    files.push(p);
                }
            }
            files
        } else {
            vec![path.to_path_buf()]
        };
        files.sort();
        let source = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let mut import = UsfmImport::new(UsfmOptions {
            resource_id: id.into(),
            title: source.clone(),
            source_file: source,
            tool_version: format!("Open Bible {}", env!("CARGO_PKG_VERSION")),
            ..Default::default()
        });
        for file in files {
            if ctx.is_cancelled() {
                return Err("Import cancelled".into());
            }
            import.add_book(
                &file.file_name().unwrap_or_default().to_string_lossy(),
                std::fs::read_to_string(&file)?,
            )?;
        }
        let (resource, report) = import.finish_with_progress(|done, total| {
            ctx.progress(done as u64, total as u64);
            !ctx.is_cancelled()
        })?;
        if ctx.is_cancelled() {
            return Err("Import cancelled".into());
        }
        std::fs::create_dir_all(dir)?;
        let out = dir.join(format!("{id}.obr"));
        // Reserve filename, including files not scanned (or written by another app instance).
        let reservation = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&out)?;
        drop(reservation);
        if let Err(e) = obs_store::write_obr(&resource, &out) {
            let _ = std::fs::remove_file(&out);
            return Err(e.into());
        }
        ctx.phase("Index");
        let index = indexer
            .build(&resource, &out, ctx)
            .unwrap_or_else(ImportIndexStatus::Failed);
        Ok(Imported {
            entry: LibraryEntry {
                manifest: resource.manifest,
                path: out,
            },
            report,
            index,
        })
    };
    run().map_err(|e| format!("{}: {e}", path.display()))
}
