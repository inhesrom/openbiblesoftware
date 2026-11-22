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

/// The folder in the library folder where Imports unpack and the Mobile app's picked files
/// wait (`<library>/imports/`, `crate::import_zip`). The library's scan reads only `.obr`
/// files beside it.
pub const IMPORTS_DIR: &str = "imports";

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
    /// The Open catalogue entry a download came from (#134).
    #[serde(default)]
    pub open_origin: Option<OpenOrigin>,
}

/// Where a Resource downloaded from the Open catalogue came from (#134).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenOrigin {
    /// The Open catalogue entry's id.
    pub id: String,
    /// The publisher's URL it was downloaded from.
    pub url: String,
    /// The downloaded file's sha256, once downloaded and checked.
    #[serde(default)]
    pub sha256: Option<String>,
}

/// Where a downloaded Resource came from: a module of a SWORD repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoOrigin {
    pub repo: String,
    pub module: String,
    /// The module's conf `Version` when it was imported (empty when the conf has none), so
    /// Get Resources can tell when its repository offers a newer one (#136). `None` for a
    /// download made before #136 until its SWORD copy is read
    /// ([`Session::fill_installed_versions`]).
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Debug)]
pub struct Imports {
    pub(crate) reports: BTreeMap<String, ImportRecord>,
    active: BTreeMap<JobId, String>,
    /// Resources being replaced by a new Resource revision (an update, #136), with the
    /// record of the revision in the library: put back if the update fails.
    replacing: BTreeMap<String, ImportRecord>,
    indexer: Arc<dyn ImportIndexer>,
}

impl Default for Imports {
    fn default() -> Self {
        Self {
            reports: BTreeMap::new(),
            active: BTreeMap::new(),
            replacing: BTreeMap::new(),
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
    /// Whether the running Import of `id` is an update to a new Resource revision.
    pub fn is_update(&self, id: &str) -> bool {
        self.replacing.contains_key(id)
    }
    /// The record of the revision in the library that the running update of `id` replaces.
    pub(crate) fn replaced(&self, id: &str) -> Option<&ImportRecord> {
        self.replacing.get(id)
    }
}

/// An Import report Tab's title, naming what was imported ("Import report · GEN-1") so two
/// reports side by side can be told apart: the source's name while it runs, the Resource's
/// abbreviation once it is in the Library.
pub(crate) fn report_title(name: &str) -> String {
    format!("Import report · {name}")
}

/// What a failed Import's report says, in plain words; the error itself shows only under
/// Details. `error` is the job's message, as [`ImportRecord::error`] keeps it: the source's
/// description, ": ", then the error. Only the error is read, so a file or folder whose name
/// holds one of the phrases below is not mistaken for that failure.
pub fn failure_message(error: &str) -> &'static str {
    let e = error
        .split_once(": ")
        .map_or(error, |(_, e)| e)
        .to_ascii_lowercase();
    if e.ends_with("import cancelled") {
        "The Import was cancelled."
    } else if let Some(message) = zip_failure(&e) {
        message
    } else if e.contains("no such file or directory") || e.contains("cannot find the") {
        "That file or folder does not exist. It may have been moved, renamed or deleted."
    } else if e.contains("permission denied") || e.contains("access is denied") {
        "The app is not allowed to read that file or folder."
    } else if e.contains("no \\id line")
        || e.contains("no bible books found")
        || e.contains("stream did not contain valid utf-8")
    {
        "That is not a USFM file or folder of Bible books, nor a SWORD module's .conf file."
    } else if e.contains("several modules") {
        "That SWORD folder holds several modules. Choose one module's .conf file in mods.d/."
    } else if e.contains("cannot be imported") || e.contains("has no entries") {
        "The app cannot import this SWORD module yet."
    } else if e.contains("download from ") {
        "The download did not finish. Check the connection, then try again."
    } else {
        "The app could not read this source, so nothing was added to your Library."
    }
}

/// What a failed zip Import (the Mobile app's, `crate::import_zip`) says; `None` for any
/// other failure. Each phrase is one only `crate::import_zip` writes. The desktop's Open
/// catalogue Import reads zips too, and its download that is not a zip fails with
/// `obs_formats::usfm_zip`'s "not a zip archive", which keeps the generic words: so the
/// Mobile app's own not-a-zip error says [`NOT_A_ZIP`], and only that is matched.
fn zip_failure(e: &str) -> Option<&'static str> {
    Some(
        if e.contains("would land outside its folder") || e.contains("has a symbolic link") {
            "This zip holds a file the app will not unpack (a link, or a file outside the zip's \
         folder), so nothing was added to your Library."
        } else if e.contains("unpacks to more than") || e.contains("this zip has more than") {
            "This zip is too large to import: more than 512 MiB unpacked, or more than 10,000 \
         files."
        } else if e.contains("zip holds several sword modules") {
            "This zip holds several SWORD modules. Zip one module, then import it."
        } else if e.contains("sword module is not where the app looks") {
            "This zip's SWORD module is not where the app looks for it: mods.d/ must be at the \
         top of the zip, or in one folder at its top."
        } else if e.contains("zip holds no usfm books") {
            "This zip holds no USFM books (.usfm or .sfm files) and no SWORD module, so nothing \
         was added to your Library."
        } else if e.contains(DATA_PATH_OUTSIDE) {
            "This zip's SWORD module keeps its text outside the module's own folder in \
         modules/, so nothing was added to your Library."
        } else if e.contains(NOT_A_ZIP) {
            "That file is not a zip archive, so nothing was added to your Library."
        } else {
            return None;
        },
    )
}

/// The Mobile app's zip Import of a file that is not a zip (`crate::import_zip`), unlike
/// `obs_formats::usfm_zip`'s "not a zip archive", which the desktop's Open catalogue
/// reports with the generic words.
pub(crate) const NOT_A_ZIP: &str = "this file is not a zip archive";

/// A SWORD zip whose conf's `DataPath` is not a module folder inside `modules/`
/// (`sword_modules::install::module_data_dir`), so its text would be read from elsewhere.
pub(crate) const DATA_PATH_OUTSIDE: &str = "this zip's sword module keeps its data outside";

/// An Import report's licence line when the source carried no licence.
pub const LICENCE_MISSING: &str =
    "Licence: unknown (user-imported). No licence found in the source; for personal use only.";

/// An Import report's licence line when the source carried one.
pub const LICENCE_RECORDED: &str = "Licence recorded from the source; see About.";

/// "1 note", "3 notes".
fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("{n} {one}")
    } else {
        format!("{n} {many}")
    }
}

/// What an Import brought in, in the words of what it is (LIBRARY-10): a Bible's Bible books,
/// Verses, notes and Tagged words; a Commentary's, Lexicon's or Dictionary's entries; a
/// dataset's cross-references, people and places, or events. Only a Bible's counts show when
/// they are zero, so a Commentary never reads "0 Bible books · 0 Verses".
pub fn summary_counts(report: &ImportReport) -> String {
    let mut parts = Vec::new();
    let mut nonzero = |n: usize, one: &str, many: &str| {
        if n > 0 {
            parts.push(plural(n, one, many));
        }
    };
    let dataset = report.cross_references
        + report.entities
        + report.mentions
        + report.events
        + report.parallel_sections
        > 0;
    if report.entries > 0 {
        nonzero(report.entries, "entry", "entries");
        nonzero(report.notes, "note", "notes");
        nonzero(report.scripture_refs, "Bible reference", "Bible references");
        nonzero(report.tagged_words, "Strong's link", "Strong's links");
    } else if dataset {
        nonzero(
            report.cross_references,
            "Cross-reference",
            "Cross-references",
        );
        nonzero(
            report.parallel_sections,
            "section with Parallel passages",
            "sections with Parallel passages",
        );
        nonzero(report.entities, "person or place", "people and places");
        nonzero(report.mentions, "mention", "mentions");
        nonzero(
            report.events,
            "era, lifespan or event",
            "eras, lifespans and events",
        );
    } else {
        parts.push(plural(report.books.len(), "Bible book", "Bible books"));
        parts.push(plural(report.verses, "Verse", "Verses"));
        parts.push(plural(report.notes, "note", "notes"));
        parts.push(plural(report.tagged_words, "Tagged word", "Tagged words"));
    }
    if parts.is_empty() {
        "Nothing to read was found.".to_string()
    } else {
        parts.join(" · ")
    }
}

/// "2 source files · usfm": how many files an Import read, and their format.
pub fn source_line(report: &ImportReport) -> String {
    format!(
        "{} · {}",
        plural(report.source_files.len(), "source file", "source files"),
        report.source_format
    )
}

/// The start of the preference key of each Resource's latest Import report.
pub(crate) const REPORT_PREF_PREFIX: &str = "import-report/";

pub(crate) fn pref(id: &str) -> String {
    format!("{REPORT_PREF_PREFIX}{id}")
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
        self.fill_installed_versions();
    }

    /// Import report Tabs of `id` take its abbreviation once it is in the Library.
    fn retitle_import_reports(&mut self, id: &str) {
        let kind = TabKind::ImportReport(id.to_string());
        let title = crate::workspace_commands::title_of(self, &kind);
        let tabs: Vec<_> = self
            .workspace
            .tabs()
            .filter(|t| t.kind == kind)
            .map(|t| t.id)
            .collect();
        for tab in tabs {
            if let Some(t) = self.workspace.tab_mut(tab) {
                t.title.clone_from(&title);
            }
        }
    }

    /// Save the latest Import record of `id` in the User database.
    pub(crate) fn save_import_record(&mut self, id: &str) {
        if let Some(user) = &self.user
            && let Some(record) = self.imports.reports.get(id)
        {
            let result = serde_json::to_string(record)
                .map_err(|e| e.to_string())
                .and_then(|json| user.set_preference(&pref(id), &json));
            if let Err(e) = result {
                self.workspace.push_effect(Effect::Message(format!(
                    "Could not save the Import report: {e}"
                )));
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
        let events = self.poll_starter(events);
        let events = self.poll_updates(events);
        let events = self.poll_search_indexes(events);
        let events = self.poll_guides(events);
        let events = self.poll_bible_searches(events);
        let events = self.poll_search_preview(events);
        let events = self.poll_word_studies(events);
        let events = self.poll_factbooks(events);
        let events = self.poll_topic_guides(events);
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
                        if let Some(origin) = record.origin.as_mut() {
                            origin.version = out.version;
                        }
                        if let Some(open) = record.open_origin.as_mut() {
                            open.sha256 = out.sha256;
                        }
                        let replaced = self.library.get(&id).is_some();
                        self.library.add(out.entry);
                        self.retitle_import_reports(&id);
                        self.remove_older_search_indexes(&id);
                        // An update is in: nothing to put back.
                        self.imports.replacing.remove(&id);
                        if replaced {
                            self.workspace.push_effect(Effect::Replaced(id.clone()));
                        }
                        imported.push(id.clone());
                        if let Err(e) = self.save_preferred_order() {
                            self.workspace.push_effect(Effect::Message(format!(
                                "Could not save the Preferred order: {e}"
                            )));
                        }
                        self.save_import_record(&id);
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
            // A failed update leaves the revision in the library as it was.
            if terminal
                && let Some(before) = self.imports.replacing.remove(&id)
                && let Some(failed) = self.imports.reports.insert(id.clone(), before)
                && failed.phase == ImportPhase::Failed
            {
                let name = self.library.abbreviation_of(&id).unwrap_or(&id).to_string();
                let why = failed.error.unwrap_or_default();
                self.workspace
                    .push_effect(Effect::Message(format!("Could not update {name}: {why}")));
            }
            if terminal {
                self.imports.active.remove(&event.id);
            }
        }
        // The Import built the cache (or the background rebuild now will); a re-import of a
        // Resource with Annotations re-anchors them (#65). The Starter library adds its
        // Resources through the same path (#131).
        self.finish_adding(&imported);
        other
    }
}

/// Only desktop builds construct it; wasm still matches the job output type.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
struct Imported {
    entry: LibraryEntry,
    report: ImportReport,
    index: ImportIndexStatus,
    /// A downloaded module's conf `Version` (empty when it has none).
    version: Option<String>,
    /// An Open catalogue download's sha256.
    sha256: Option<String>,
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
    let title = crate::workspace_commands::title_of(s, &TabKind::ImportReport(id.into()));
    let tab = s.workspace.open(
        TabKind::ImportReport(id.into()),
        title,
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
    /// An Open catalogue entry, downloaded from its publisher and checked first (#134).
    Open(Box<crate::open_catalogue::OpenSource>),
    /// An Open catalogue entry the app already downloaded to `file` (the Mobile app's
    /// background transfers, #333), checked as [`Self::Open`] is. The file is deleted once
    /// read.
    OpenDownloaded {
        source: Box<crate::open_catalogue::OpenSource>,
        file: std::path::PathBuf,
    },
    /// A zip of one SWORD module (the Mobile app's Import, #333), unpacked into a temp folder
    /// under the library's `imports/` and read as [`Self::Sword`] (`crate::import_zip`).
    SwordZip(std::path::PathBuf),
    /// A zip of USFM books (the Mobile app's Import, #333), read in memory.
    UsfmZip(std::path::PathBuf),
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

    /// A SWORD zip if the zip at `path` holds a `mods.d/*.conf`, else a USFM zip
    /// (`crate::import_zip`). The desktop's [`Self::detect`] never makes either.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn zip(path: std::path::PathBuf) -> Self {
        match crate::import_zip::zip_kind(&path) {
            crate::import_zip::ZipKind::Sword => Self::SwordZip(path),
            crate::import_zip::ZipKind::Usfm => Self::UsfmZip(path),
        }
    }

    fn path(&self) -> &Path {
        match self {
            Self::Usfm(p)
            | Self::SwordZip(p)
            | Self::UsfmZip(p)
            | Self::Sword { path: p, .. }
            | Self::Repository { sword_dir: p, .. } => p,
            Self::Open(source) | Self::OpenDownloaded { source, .. } => Path::new(&source.url),
        }
    }

    /// The name the new Resource id is made from.
    fn stem(&self) -> String {
        match self {
            Self::Sword {
                module: Some(m), ..
            }
            | Self::Repository { module: m, .. } => m.clone(),
            Self::Open(source) | Self::OpenDownloaded { source, .. } => source.id.clone(),
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
            Self::Open(source) | Self::OpenDownloaded { source, .. } => {
                format!("{} from {}", source.url, source.publisher)
            }
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
        let name = source.stem();
        let (id, job) = self.start_import_job(source)?;
        let tab = self.workspace.open(
            TabKind::ImportReport(id.clone()),
            report_title(&name),
            OpenRequest::default(),
        );
        Ok((id, tab.0, job))
    }

    /// [`Self::start_import`] without its report Tab (the Mobile app shows progress itself;
    /// the report is kept). Returns the new Resource id and the job.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn start_import_job(&mut self, source: ImportSource) -> Result<(String, JobId), String> {
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
        let job = self.submit_import(source, &dir, &id, None);
        Ok((id, job))
    }

    /// Move the Resource file at `path` (the app's own copy of a `.obr` the user picked or
    /// shared, the Mobile app's Import, #333) into the library folder for good and add it:
    /// its Search index refreshed, its name remembered, its Annotations re-anchored if it
    /// replaced a revision. Unlike the desktop's drop (`obs_ui_egui::App::dropped`, which
    /// adds the dropped file where it is for the session), it changes the library folder, so
    /// a Resource already in the library is replaced only by a greater revision, the Starter
    /// sync's rule: its file is replaced (wherever the library folder holds it). The same
    /// revision with the same content changes nothing ([`ObrCopy::AlreadyThere`]); the same
    /// or a lower revision with other content is refused ([`ObrRefused::NotNewer`]). A new
    /// Resource is `<id>.obr`, or `<id>-2.obr` when another Resource has that name. A file
    /// that is refused, or already there, is left where it is.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn add_obr_copy(&mut self, path: &Path) -> Result<ObrAdded, ObrRefused> {
        use obs_store::ResourceStore;
        let manifest = obs_store::ObrReader::open(path)
            .map(|r| r.manifest().clone())
            .map_err(|e| ObrRefused::Unreadable(e.to_string()))?;
        let id = manifest.resource_id.clone();
        if !obs_model::is_valid_resource_id(&id) {
            return Err(ObrRefused::Unreadable(format!(
                "{id:?} is not a valid Resource id"
            )));
        }
        if let Some(have) = self.library.get(&id).map(|e| &e.manifest)
            && manifest.revision <= have.revision
        {
            if manifest.revision == have.revision && manifest.content_hash == have.content_hash {
                return Ok(ObrAdded {
                    manifest: have.clone(),
                    outcome: ObrCopy::AlreadyThere,
                });
            }
            return Err(ObrRefused::NotNewer {
                abbreviation: have.abbreviation.clone(),
                revision: have.revision,
                offered: manifest.revision,
            });
        }
        let dir = self
            .library
            .directory()
            .map(Path::to_path_buf)
            .ok_or_else(|| ObrRefused::Unreadable("No library directory".into()))?;
        let previous = self.library.get(&id).map(|e| e.path.clone());
        let dest = match &previous {
            Some(p) if p.parent() == Some(dir.as_path()) => p.clone(),
            _ => {
                let mut n = 1u32;
                loop {
                    let name = if n == 1 {
                        format!("{id}.obr")
                    } else {
                        format!("{id}-{n}.obr")
                    };
                    let candidate = dir.join(name);
                    if !candidate.exists() {
                        break candidate;
                    }
                    n += 1;
                }
            }
        };
        let mut staged = dest.clone().into_os_string();
        staged.push(".part");
        let staged = std::path::PathBuf::from(staged);
        let moved = std::fs::rename(path, &staged)
            .or_else(|_| std::fs::copy(path, &staged).map(|_| ()))
            .and_then(|()| std::fs::rename(&staged, &dest));
        if let Err(e) = moved {
            let _ = std::fs::remove_file(&staged);
            return Err(ObrRefused::Unreadable(format!("{}: {e}", dest.display())));
        }
        let added = self
            .library
            .add_file(&dest)
            .map(|e| e.manifest.clone())
            .map_err(|e| ObrRefused::Unreadable(e.to_string()))?;
        self.refresh_search_index(&id);
        self.remember_resource_names();
        self.reanchor_resource(&id);
        let replaced = previous.is_some();
        if replaced {
            self.workspace.push_effect(Effect::Replaced(id.clone()));
        }
        if let Err(e) = self.save_preferred_order() {
            self.workspace.push_effect(Effect::Message(format!(
                "Could not save the Preferred order: {e}"
            )));
        }
        Ok(ObrAdded {
            manifest: added,
            outcome: if replaced {
                ObrCopy::Replaced
            } else {
                ObrCopy::Added
            },
        })
    }

    /// Download a newer version of the module a Resource was downloaded from and import it
    /// as the next Resource revision of that Resource (#136): same id, the same `.obr` file
    /// (whatever it is named; the library does not tie file names to ids) and place in the
    /// Preferred order, and its Annotations are re-anchored once it is in. The row on Get
    /// Resources shows the progress; no Import report Tab opens. If it fails, the revision
    /// in the library stays as it was.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn start_update(
        &mut self,
        resource_id: &str,
        repo: crate::get_bibles::RepoRef,
        sword_dir: std::path::PathBuf,
    ) -> Result<JobId, String> {
        let entry = self
            .library
            .get(resource_id)
            .ok_or_else(|| format!("{resource_id} is not in the library"))?;
        let path = entry.path.clone();
        let dir = path
            .parent()
            .map(Path::to_path_buf)
            .ok_or("No library directory")?;
        let revision = Revision {
            number: entry.manifest.revision.saturating_add(1),
            path,
        };
        let record = self
            .imports
            .report(resource_id)
            .cloned()
            .ok_or_else(|| format!("{resource_id} was not downloaded"))?;
        let module = record
            .origin
            .as_ref()
            .map(|o| o.module.clone())
            .ok_or_else(|| format!("{resource_id} was not downloaded"))?;
        let source = ImportSource::Repository {
            repo,
            module,
            sword_dir,
        };
        self.imports
            .replacing
            .insert(resource_id.to_string(), record);
        Ok(self.submit_import(source, &dir, resource_id, Some(revision)))
    }

    /// Record a running Import of `source` as Resource `id` and queue its job. `revision`:
    /// write it as that Resource revision of `id`, replacing its `.obr` in the library.
    #[cfg(not(target_arch = "wasm32"))]
    fn submit_import(
        &mut self,
        source: ImportSource,
        dir: &Path,
        id: &str,
        revision: Option<Revision>,
    ) -> JobId {
        let id = id.to_string();
        let (phase, origin, open_origin) = match &source {
            ImportSource::Repository { repo, module, .. } => (
                ImportPhase::Download,
                Some(RepoOrigin {
                    repo: repo.name.clone(),
                    module: module.clone(),
                    version: None,
                }),
                None,
            ),
            ImportSource::Open(source) => (
                ImportPhase::Download,
                None,
                Some(OpenOrigin {
                    id: source.id.clone(),
                    url: source.url.clone(),
                    sha256: None,
                }),
            ),
            ImportSource::OpenDownloaded { source, .. } => (
                ImportPhase::Parse,
                None,
                Some(OpenOrigin {
                    id: source.id.clone(),
                    url: source.url.clone(),
                    sha256: None,
                }),
            ),
            _ => (ImportPhase::Parse, None, None),
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
            open_origin,
        };
        let indexer = Arc::clone(&self.imports.indexer);
        let fetch = self.fetch();
        let worker_id = id.clone();
        let dir = dir.to_path_buf();
        let job = self.jobs().submit_fn("Import", move |ctx| {
            let to = Target {
                dir: &dir,
                id: &worker_id,
                revision,
            };
            import_source(&source, &to, indexer.as_ref(), fetch, ctx)
        });
        self.imports.active.insert(job, id.clone());
        self.imports.reports.insert(id, record);
        job
    }
}

/// A Resource file [`Session::add_obr_copy`] took: the Resource now in the library, and
/// what was done.
#[derive(Debug, Clone, PartialEq)]
pub struct ObrAdded {
    pub manifest: obs_model::Manifest,
    pub outcome: ObrCopy,
}

/// What [`Session::add_obr_copy`] did with a Resource file it took.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObrCopy {
    /// A Resource new to the library, moved in.
    Added,
    /// A greater revision of a Resource in the library: its file was replaced.
    Replaced,
    /// The library has this revision with this content already: nothing changed.
    AlreadyThere,
}

/// Why [`Session::add_obr_copy`] added nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObrRefused {
    /// The library has this Resource at `revision`, with other content, and the file's
    /// revision (`offered`) is not greater (the Starter sync's rule).
    NotNewer {
        abbreviation: String,
        revision: u32,
        offered: u32,
    },
    /// Not a Resource file the library can read, or it could not be moved in: the error.
    Unreadable(String),
}

impl ObrRefused {
    /// What the Import report says, in plain words.
    pub fn message(&self) -> String {
        match self {
            Self::NotNewer {
                abbreviation,
                revision,
                offered,
            } if offered == revision => format!(
                "Your Library already has {abbreviation} revision {revision}. This file is \
                 revision {offered} too, with other content, so nothing was changed."
            ),
            Self::NotNewer {
                abbreviation,
                revision,
                offered,
            } => format!(
                "Your Library already has {abbreviation} revision {revision}. This file is an \
                 older revision ({offered}), so nothing was changed."
            ),
            Self::Unreadable(_) => "That is not a Resource file this app can read, so nothing \
                 was added to your Library."
                .to_string(),
        }
    }
}

impl std::fmt::Display for ObrRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotNewer {
                abbreviation,
                revision,
                offered,
            } => write!(
                f,
                "{abbreviation}: the library has revision {revision}; this file is revision \
                 {offered}, not greater"
            ),
            Self::Unreadable(e) => f.write_str(e),
        }
    }
}

/// Where an Import writes: Resource `id` in library folder `dir`, as a new Resource, or
/// (`revision`) as that Resource revision of the Resource already there.
#[cfg(not(target_arch = "wasm32"))]
struct Target<'a> {
    dir: &'a Path,
    id: &'a str,
    revision: Option<Revision>,
}

/// A Resource revision replacing the `.obr` file at `path`: the file the library read the
/// Resource from, which need not be named after its id.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone)]
struct Revision {
    number: u32,
    path: std::path::PathBuf,
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
    let mut import = usfm_import(path, id);
    for file in files {
        if ctx.is_cancelled() {
            return Err("Import cancelled".into());
        }
        import.add_book(
            &file.file_name().unwrap_or_default().to_string_lossy(),
            std::fs::read_to_string(&file)?,
        )?;
    }
    finish_usfm(import, ctx)
}

/// A USFM Import of the books from `path` (a file, a folder or a zip) as Resource `id`,
/// titled with the file or folder name.
#[cfg(not(target_arch = "wasm32"))]
fn usfm_import(path: &Path, id: &str) -> obs_formats::UsfmImport {
    let source = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    obs_formats::UsfmImport::new(obs_formats::UsfmOptions {
        resource_id: id.into(),
        title: source.clone(),
        source_file: source,
        tool_version: tool_version(),
        ..Default::default()
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn finish_usfm(
    import: obs_formats::UsfmImport,
    ctx: &JobContext,
) -> Result<Parsed, Box<dyn std::error::Error>> {
    Ok(import.finish_with_progress(|done, total| {
        ctx.progress(done as u64, total as u64);
        !ctx.is_cancelled()
    })?)
}

/// The USFM books of the zip at `path` (`obs_formats::usfm_zip_books`, which reads `.usfm`
/// and `.sfm` files from any folder of it).
#[cfg(not(target_arch = "wasm32"))]
fn parse_usfm_zip(
    path: &Path,
    id: &str,
    ctx: &JobContext,
) -> Result<Parsed, Box<dyn std::error::Error>> {
    let bytes = crate::import_zip::read_usfm_zip(path, crate::import_zip::ZipBounds::DEFAULT)?;
    let books = obs_formats::usfm_zip_books(&bytes)?;
    drop(bytes);
    if books.is_empty() {
        return Err(
            "this zip holds no USFM books (.usfm or .sfm files) and no SWORD module (mods.d/)"
                .into(),
        );
    }
    let mut import = usfm_import(path, id);
    for (name, text) in books {
        if ctx.is_cancelled() {
            return Err("Import cancelled".into());
        }
        import.add_book(&name, text)?;
    }
    finish_usfm(import, ctx)
}

/// The one module of the SWORD zip at `path`, unpacked into `<dir>/imports/<id>.sword/`,
/// which is removed again whatever the outcome, a panic too (`crate::import_zip`). A conf
/// whose `DataPath` leaves its module folder is refused before the module is read.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn parse_sword_zip(
    path: &Path,
    dir: &Path,
    id: &str,
    bounds: crate::import_zip::ZipBounds,
    is_cancelled: impl Fn() -> bool,
) -> Result<Parsed, Box<dyn std::error::Error>> {
    let temp = crate::import_zip::TempFolder(
        dir.join(crate::import_zip::IMPORTS_DIR)
            .join(format!("{id}.sword")),
    );
    let (root, usfm_beside) =
        crate::import_zip::unpack_sword_zip(path, &temp.0, bounds, &is_cancelled)?;
    if is_cancelled() {
        return Err(cancelled());
    }
    obs_formats::SwordSource::open(&root)?
        .check_data_paths()
        .map_err(|e| {
            format!("this zip's SWORD module keeps its data outside its folder in modules/ ({e})")
        })?;
    let (resource, mut report) = parse_sword(&root, None, id)?;
    if usfm_beside {
        report
            .warnings
            .push(crate::import_zip::USFM_BESIDE_SWORD.to_string());
    }
    Ok((resource, report))
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
    to: &Target<'_>,
    indexer: &dyn ImportIndexer,
    fetch: Arc<dyn obs_formats::sword_repo::Fetch>,
    ctx: &JobContext,
) -> Result<Imported, String> {
    let (dir, id) = (to.dir, to.id);
    let mut made = Made::default();
    let mut version = None;
    let mut sha256 = None;
    let run = || -> Result<Imported, Box<dyn std::error::Error>> {
        let (mut resource, report) = match source {
            ImportSource::Usfm(path) => {
                ctx.phase("Parse");
                parse_usfm(path, id, ctx)?
            }
            ImportSource::Sword { path, module } => {
                ctx.phase("Parse");
                parse_sword(path, module.as_deref(), id)?
            }
            ImportSource::SwordZip(path) => {
                ctx.phase("Parse");
                parse_sword_zip(path, dir, id, crate::import_zip::ZipBounds::DEFAULT, || {
                    ctx.is_cancelled()
                })?
            }
            ImportSource::UsfmZip(path) => {
                ctx.phase("Parse");
                parse_usfm_zip(path, id, ctx)?
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
                version = obs_formats::SwordSource::open(sword_dir)
                    .ok()
                    .and_then(|s| {
                        s.modules()
                            .into_iter()
                            .find(|m| m.name.eq_ignore_ascii_case(module))
                    })
                    .map(|m| m.version.unwrap_or_default());
                parse_sword(sword_dir, Some(module), id)?
            }
            ImportSource::Open(open) => {
                ctx.phase("Download");
                let bytes = fetch
                    .get(&open.url, &mut |done, total| {
                        ctx.progress(done, total.unwrap_or(0));
                        !ctx.is_cancelled()
                    })
                    .map_err(|e| -> Box<dyn std::error::Error> {
                        if e == obs_formats::sword_repo::FetchError::Cancelled || ctx.is_cancelled()
                        {
                            cancelled()
                        } else {
                            format!("download from {}: {e}", open.publisher).into()
                        }
                    })?;
                if ctx.is_cancelled() {
                    return Err(cancelled());
                }
                sha256 = Some(open.check_download(&bytes)?);
                ctx.phase("Parse");
                open.import(&bytes, id, &tool_version(), |done, total| {
                    ctx.progress(done as u64, total as u64);
                    !ctx.is_cancelled()
                })?
            }
            ImportSource::OpenDownloaded { source: open, file } => {
                ctx.phase("Parse");
                let bytes = std::fs::read(file);
                let _ = std::fs::remove_file(file);
                let bytes = bytes.map_err(|e| format!("{}: {e}", file.display()))?;
                sha256 = Some(open.check_download(&bytes)?);
                open.import(&bytes, id, &tool_version(), |done, total| {
                    ctx.progress(done as u64, total as u64);
                    !ctx.is_cancelled()
                })?
            }
        };
        if ctx.is_cancelled() {
            return Err(cancelled());
        }
        std::fs::create_dir_all(dir)?;
        // A new Resource revision is written beside the file in the library (a name the
        // library scan skips) and replaces that exact file in one rename once indexed, so the
        // revision before it stays whole if the update fails or is cancelled.
        let (out, written) = if let Some(revision) = &to.revision {
            resource.manifest.revision = revision.number;
            let mut staged = revision.path.clone().into_os_string();
            staged.push(".update");
            (revision.path.clone(), std::path::PathBuf::from(staged))
        } else {
            let out = dir.join(format!("{id}.obr"));
            // Reserve filename, including files not scanned (or written by another app
            // instance).
            let reservation = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&out)?;
            drop(reservation);
            (out.clone(), out)
        };
        made.obr = Some(written.clone());
        obs_store::write_obr(&resource, &written)?;
        ctx.phase("Index");
        let index = indexer
            .build(&resource, &out, ctx)
            .unwrap_or_else(ImportIndexStatus::Failed);
        if ctx.is_cancelled() {
            return Err(cancelled());
        }
        if written != out {
            std::fs::rename(&written, &out)?;
            made.obr = None;
        }
        Ok(Imported {
            entry: LibraryEntry {
                manifest: resource.manifest,
                path: out,
            },
            report,
            index,
            version: version.take(),
            sha256: sha256.take(),
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
