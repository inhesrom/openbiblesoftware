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

/// Shown when the Import did not build the Search index itself (no cache directory was set
/// up for it); the Session builds it in the background instead (#58).
pub const DEFERRED_INDEX: &str = "Search index: built in the background";

/// Shown when nothing can build the Search index here (no cache directory).
pub const UNAVAILABLE_INDEX: &str = "Search index: unavailable (no cache folder)";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImportPhase {
    /// Fetching a SWORD module from its repository (Get Bibles, #91).
    Download,
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

/// Builds the new Resource's Search index during the Import's Index phase. It runs on the
/// Import worker after the immutable `.obr` is written, reports real progress through
/// `ctx.progress` and stops when `ctx.is_cancelled()`. The app installs
/// [`crate::CacheIndexer`] (via [`Session::set_search_cache_dir`]); without it the index is
/// [`ImportIndexStatus::Deferred`] to a background rebuild.
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
    /// The repository and module a Get Bibles download came from (#91).
    #[serde(default)]
    pub origin: Option<RepoOrigin>,
}

/// Where a downloaded Resource came from: a module of a SWORD repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoOrigin {
    pub repo: String,
    pub module: String,
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
    /// Every Import record, latest state.
    pub fn records(&self) -> impl Iterator<Item = &ImportRecord> {
        self.reports.values()
    }
    /// The job of a running Import.
    pub fn job_of(&self, id: &str) -> Option<JobId> {
        self.active
            .iter()
            .find_map(|(job, r)| (r == id).then_some(*job))
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
        let mut imported = Vec::new();
        let events = self.jobs().drain_events();
        let events = self.poll_get_bibles(events);
        let events = self.poll_search_indexes(events);
        let events = self.poll_guides(events);
        let events = self.poll_bible_searches(events);
        let events = self.poll_word_studies(events);
        let events = self.poll_reanchoring(events);
        for event in events {
            let Some(id) = self.imports.active.get(&event.id).cloned() else {
                other.push(event);
                continue;
            };
            let Some(record) = self.imports.reports.get_mut(&id) else {
                continue;
            };
            let terminal = event.kind.is_terminal();
            match event.kind {
                JobEventKind::Phase(name) => {
                    let phase = match name.as_str() {
                        "Download" => Some(ImportPhase::Download),
                        "Parse" => Some(ImportPhase::Parse),
                        "Index" => Some(ImportPhase::Index),
                        _ => None,
                    };
                    if let Some(phase) = phase {
                        record.phase = phase;
                        record.done = 0;
                        record.total = 0;
                    }
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
                        imported.push(id.clone());
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
        if !imported.is_empty() {
            self.remember_resource_names();
        }
        // The Import built the cache (or the background rebuild now will).
        for id in imported {
            self.refresh_search_index(&id);
            // A re-import of a Resource with Annotations re-anchors them (#65).
            self.reanchor_resource(&id);
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
    help: "USFM file or folder, SWORD library folder or module .conf file on this machine",
};
const MODULE: ParamSpec = ParamSpec {
    name: "module",
    kind: ParamKind::String,
    required: false,
    help: "SWORD module to import from a library folder",
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
        params: &[SOURCE, MODULE],
        enabled: desktop,
        run: start,
    },
    CommandSpec {
        id: "import.cancel",
        label: "Cancel Import",
        shortcut: None,
        params: &[ID],
        enabled: always,
        run: |s, p| {
            let id = p.str("id").unwrap_or_default();
            let job = s
                .imports
                .job_of(id)
                .ok_or_else(|| failed("import.cancel", "This Import is not running"))?;
            s.jobs().cancel(job);
            Ok(Value::Null)
        },
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

/// What one Import reads: USFM books, or one module of a SWORD library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportSource {
    /// A USFM file or a folder of USFM books.
    Usfm(std::path::PathBuf),
    /// A SWORD library folder (holding `mods.d/`) or a module's `.conf` file, and the module
    /// to import (`None`: the conf file's module, or the library's only module).
    Sword {
        path: std::path::PathBuf,
        module: Option<String>,
    },
    /// A module downloaded from a SWORD repository into `sword_dir` first (Get Bibles, #91).
    Repository {
        repo: crate::get_bibles::RepoRef,
        module: String,
        sword_dir: std::path::PathBuf,
    },
}

// Desktop Import uses these; the browser build has no local Import.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
impl ImportSource {
    /// Tell the source kind from the path: a `.conf` file, or a folder holding `mods.d/`, is
    /// SWORD; anything else is USFM.
    pub fn detect(path: std::path::PathBuf, module: Option<String>) -> Self {
        let is_conf = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("conf"));
        if is_conf || path.join("mods.d").is_dir() || module.is_some() {
            Self::Sword { path, module }
        } else {
            Self::Usfm(path)
        }
    }

    fn path(&self) -> &Path {
        match self {
            Self::Usfm(p) | Self::Sword { path: p, .. } | Self::Repository { sword_dir: p, .. } => {
                p
            }
        }
    }

    /// The name the new Resource id is made from.
    fn stem(&self) -> String {
        match self {
            Self::Sword {
                module: Some(m), ..
            }
            | Self::Repository { module: m, .. } => m.clone(),
            Self::Sword { path, .. } if path.is_dir() => path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            _ => self
                .path()
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
        }
    }

    /// Shown on the Import report.
    fn describe(&self) -> String {
        match self {
            Self::Sword {
                path,
                module: Some(m),
            } => format!("{} ({m})", path.display()),
            Self::Repository { repo, module, .. } => format!("{module} from {}", repo.name),
            _ => self.path().display().to_string(),
        }
    }
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
        return Err(failed(
            "import.start",
            "Choose a USFM file or folder, or a SWORD module",
        ));
    }
    let module = p
        .str("module")
        .filter(|m| !m.is_empty())
        .map(str::to_string);
    let (id, tab, job) = s
        .start_import(ImportSource::detect(path, module))
        .map_err(|e| failed("import.start", e))?;
    Ok(json!({"id": id, "tab": tab, "job": job.0}))
}

impl Session {
    /// Start an Import job and open its live report Tab. Return the new Resource id, the
    /// Tab id and the job.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn start_import(&mut self, source: ImportSource) -> Result<(String, u64, JobId), String> {
        let dir = self
            .library
            .directory()
            .map(Path::to_path_buf)
            .or_else(crate::default_library_dir)
            .ok_or("No library directory")?;
        self.library.set_directory(dir.clone());
        let slug = obs_formats::slugify(&source.stem());
        // Every Import is a new edition, never an implicit revision or overwrite of user text.
        let mut n = 1u64;
        let id = loop {
            let id = format!("{}-{n}", slug.chars().take(44).collect::<String>());
            if self.library.get(&id).is_none() && self.imports.report(&id).is_none() {
                break id;
            }
            n += 1;
        };
        let (phase, origin) = match &source {
            ImportSource::Repository { repo, module, .. } => (
                ImportPhase::Download,
                Some(RepoOrigin {
                    repo: repo.name.clone(),
                    module: module.clone(),
                }),
            ),
            _ => (ImportPhase::Parse, None),
        };
        let record = ImportRecord {
            resource_id: id.clone(),
            source: source.describe(),
            phase,
            done: 0,
            total: 0,
            report: None,
            index: ImportIndexStatus::Deferred,
            error: None,
            origin,
        };
        let indexer = Arc::clone(&self.imports.indexer);
        let fetch = self.fetch();
        let worker_id = id.clone();
        let job = self.jobs().submit_fn("Import", move |ctx| {
            import_source(&source, &dir, &worker_id, indexer.as_ref(), fetch, ctx)
        });
        self.imports.active.insert(job, id.clone());
        self.imports.reports.insert(id.clone(), record);
        let tab = self.workspace.open(
            TabKind::ImportReport(id.clone()),
            "Import report".into(),
            OpenRequest::default(),
        );
        Ok((id, tab.0, job))
    }
}

#[cfg(not(target_arch = "wasm32"))]
type Parsed = (Resource, ImportReport);

#[cfg(not(target_arch = "wasm32"))]
fn tool_version() -> String {
    format!("Open Bible {}", env!("CARGO_PKG_VERSION"))
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_usfm(
    path: &Path,
    id: &str,
    ctx: &JobContext,
) -> Result<Parsed, Box<dyn std::error::Error>> {
    use obs_formats::{UsfmImport, UsfmOptions};
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
        tool_version: tool_version(),
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
    Ok(import.finish_with_progress(|done, total| {
        ctx.progress(done as u64, total as u64);
        !ctx.is_cancelled()
    })?)
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_sword(
    path: &Path,
    module: Option<&str>,
    id: &str,
) -> Result<Parsed, Box<dyn std::error::Error>> {
    use obs_formats::{SwordOptions, SwordSource, import_sword};
    let source = SwordSource::open(path)?;
    let module = match module
        .map(str::to_string)
        .or_else(|| source.default_module())
    {
        Some(m) => m,
        None => {
            let names: Vec<String> = source
                .modules()
                .into_iter()
                .filter(|m| m.resource_type.is_some())
                .map(|m| m.name)
                .collect();
            return Err(format!(
                "this SWORD folder holds several modules; choose one module's .conf file in mods.d/ ({})",
                names.join(", ")
            )
            .into());
        }
    };
    let options = SwordOptions {
        resource_id: id.into(),
        tool_version: tool_version(),
        ..Default::default()
    };
    Ok(import_sword(&source, &module, &options)?)
}

/// Files an Import made, removed again if it fails or is cancelled.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct Made {
    obr: Option<std::path::PathBuf>,
    /// (SWORD folder, module) installed by this Import's download.
    sword: Option<(std::path::PathBuf, String)>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Made {
    fn undo(self) {
        if let Some(obr) = self.obr {
            let _ = std::fs::remove_file(obr);
        }
        if let Some((dir, module)) = self.sword {
            let _ = obs_formats::sword_repo::uninstall(&dir, &module);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn cancelled() -> Box<dyn std::error::Error> {
    "Import cancelled".into()
}

#[cfg(not(target_arch = "wasm32"))]
fn download(
    repo: &crate::get_bibles::RepoRef,
    module: &str,
    sword_dir: &Path,
    fetch: Arc<dyn obs_formats::sword_repo::Fetch>,
    ctx: &JobContext,
) -> Result<(), Box<dyn std::error::Error>> {
    let installer = crate::sword_installer(fetch, ctx);
    let fail = |e: obs_formats::sword_repo::RepoError| -> Box<dyn std::error::Error> {
        if obs_formats::sword_repo::is_cancelled(&e) || ctx.is_cancelled() {
            cancelled()
        } else {
            format!("download from {}: {e}", repo.name).into()
        }
    };
    let catalog = installer.catalog(&repo.repository()).map_err(fail)?;
    installer
        .install(&catalog, module, sword_dir)
        .map_err(fail)?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn import_source(
    source: &ImportSource,
    dir: &Path,
    id: &str,
    indexer: &dyn ImportIndexer,
    fetch: Arc<dyn obs_formats::sword_repo::Fetch>,
    ctx: &JobContext,
) -> Result<Imported, String> {
    let mut made = Made::default();
    let run = || -> Result<Imported, Box<dyn std::error::Error>> {
        let (resource, report) = match source {
            ImportSource::Usfm(path) => {
                ctx.phase("Parse");
                parse_usfm(path, id, ctx)?
            }
            ImportSource::Sword { path, module } => {
                ctx.phase("Parse");
                parse_sword(path, module.as_deref(), id)?
            }
            ImportSource::Repository {
                repo,
                module,
                sword_dir,
            } => {
                ctx.phase("Download");
                // A module already in the SWORD folder (an earlier download) is kept.
                let had = obs_formats::SwordSource::open(sword_dir)
                    .map(|s| {
                        s.modules()
                            .iter()
                            .any(|m| m.name.eq_ignore_ascii_case(module))
                    })
                    .unwrap_or(false);
                if !had {
                    made.sword = Some((sword_dir.clone(), module.clone()));
                }
                download(repo, module, sword_dir, fetch, ctx)?;
                if ctx.is_cancelled() {
                    return Err(cancelled());
                }
                ctx.phase("Parse");
                parse_sword(sword_dir, Some(module), id)?
            }
        };
        if ctx.is_cancelled() {
            return Err(cancelled());
        }
        std::fs::create_dir_all(dir)?;
        let out = dir.join(format!("{id}.obr"));
        // Reserve filename, including files not scanned (or written by another app instance).
        let reservation = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&out)?;
        drop(reservation);
        made.obr = Some(out.clone());
        obs_store::write_obr(&resource, &out)?;
        ctx.phase("Index");
        let index = indexer
            .build(&resource, &out, ctx)
            .unwrap_or_else(ImportIndexStatus::Failed);
        if ctx.is_cancelled() {
            return Err(cancelled());
        }
        Ok(Imported {
            entry: LibraryEntry {
                manifest: resource.manifest,
                path: out,
            },
            report,
            index,
        })
    };
    match run() {
        Ok(imported) => Ok(imported),
        Err(e) => {
            made.undo();
            Err(format!("{}: {e}", source.describe()))
        }
    }
}
