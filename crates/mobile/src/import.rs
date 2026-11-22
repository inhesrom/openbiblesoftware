//! Import (#333 "Import"; `docs/dev/mobile-library.md` § Import): a file the user picks in the
//! system document picker, or shares to the app ("Open in Open Bible"), joins the library
//! through the desktop's importers. The shell asks [`MobileApp::import_destination`] where
//! to copy the file (a folder of its own in the library's `imports/`), copies it there with
//! its security-scoped access, and hands it in with [`MobileApp::start_import`]. Rust tells
//! the kind from the file name and, for a zip, from what it holds: `.usfm` and `.sfm` are USFM
//! books, a `.zip` a SWORD module or USFM books (`obs_engine::import_zip`), a `.obr` a
//! Resource file moved into the library as it is (`Session::add_obr_copy`). Every word of the
//! Import report ([`ImportReportView`]) is Rust's; the copy is deleted once the Import is
//! over.

// The browser build has no files to import: its stubs refuse.
#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

use std::path::{Path, PathBuf};

use obs_engine::import::{
    IMPORTS_DIR, LICENCE_MISSING, LICENCE_RECORDED, source_line, summary_counts,
};
use obs_engine::{
    DEFERRED_INDEX, ImportIndexStatus, ImportPhase, ImportRecord, ImportReport, IndexStatus,
    ObrCopy, UNAVAILABLE_INDEX, failure_message,
};

use crate::{Changed, MobileApp, MobileError};

/// Where an Import's report is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportState {
    /// Reading the file or building its Search index.
    Running,
    /// In the library.
    Complete,
    /// Nothing was added.
    Failed,
}

/// One group of an Import report's lines, under its title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportSection {
    pub title: String,
    pub lines: Vec<String>,
    /// Whether the lines warn (no licence found).
    pub warning: bool,
    /// Whether it starts folded, its title alone shown (as the desktop's report folds its
    /// warnings, dropped markup and files).
    pub collapsed: bool,
}

/// The Import report of one picked file, as the report sheet shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportReportView {
    /// The Import, as [`MobileApp::start_import`] numbered it.
    pub import: u64,
    /// "Import in progress", "Import complete", "Import failed".
    pub heading: String,
    /// The picked file's name.
    pub source: String,
    pub state: ImportState,
    /// How far it is (0 to 1) while running, when that is known.
    pub progress: Option<f32>,
    /// What it is doing, or the Search index once complete.
    pub status: String,
    /// What went wrong, in plain words.
    pub failure: Option<String>,
    /// The error itself, for Details.
    pub details: Option<String>,
    /// The Resource in the library once complete (Open).
    pub resource_id: Option<String>,
    pub sections: Vec<ReportSection>,
}

/// A file the core named for an Import, and where its Import is.
#[derive(Debug, Clone)]
pub(crate) struct Picked {
    id: u64,
    /// The file the shell copies the picked file to.
    file: PathBuf,
    /// Its folder in `imports/`, deleted once the Import is over.
    folder: PathBuf,
    state: PickedState,
    /// The report last reported, to tell when it changes.
    seen: Option<ImportReportView>,
    /// Whether `seen` is its final report (finished, its Search index line too), so the
    /// pump no longer rebuilds it.
    pub(crate) settled: bool,
}

impl Picked {
    /// An Import job that is not over yet (or its Search index still being built).
    pub(crate) fn is_running(&self) -> bool {
        !self.settled && matches!(self.state, PickedState::Importing(_))
    }
}

#[derive(Debug, Clone)]
enum PickedState {
    /// Named, not handed in yet.
    Named,
    /// An Import job of Resource `0`.
    Importing(String),
    /// A `.obr` taken into the library (or already there).
    Added {
        manifest: Box<obs_model::Manifest>,
        outcome: ObrCopy,
    },
    /// Refused before any job: (plain words, the error).
    Refused(String, String),
}

/// What the app imports, for a file of another kind (the picker offers only these).
pub const IMPORT_KINDS: &str = "The app imports USFM books (.usfm, .sfm), a zip of USFM books \
     or of one SWORD module (.zip), and Resource files (.obr).";

/// The plain words for a picked file the shell could not copy into the app.
const COPY_FAILED: &str = "The app could not read that file, so nothing was added to your Library.";

impl MobileApp {
    /// The folder where Imports unpack and picked files wait: `<library>/imports/`.
    pub fn imports_folder(&self) -> PathBuf {
        self.paths.library.join(IMPORTS_DIR)
    }

    /// Where the shell copies a file the user picked or shared, named `file_name` (only its
    /// last component is kept): a new folder of its own in [`Self::imports_folder`], so two
    /// files of one name never meet and the Import reads the name the user knows.
    pub fn import_destination(&mut self, file_name: &str) -> Result<PathBuf, MobileError> {
        let name = file_name
            .rsplit(['/', '\\'])
            .next()
            .filter(|n| !n.is_empty() && *n != "." && *n != "..")
            .ok_or_else(|| MobileError::Failed(format!("{file_name:?} is not a file name")))?
            .to_string();
        let imports = self.imports_folder();
        let id = self.next_import;
        self.next_import += 1;
        let folder = imports.join(format!("picked-{id}"));
        std::fs::create_dir_all(&folder).map_err(|e| MobileError::Folder {
            path: folder.clone(),
            error: e.to_string(),
        })?;
        let file = folder.join(name);
        self.picked.push(Picked {
            id,
            file: file.clone(),
            folder,
            state: PickedState::Named,
            seen: None,
            settled: false,
        });
        Ok(file)
    }

    /// Import the file the shell copied to `path` (the one [`Self::import_destination`]
    /// named; any other is refused and left alone). Returns the Import's number for
    /// [`Self::import_report`]; [`Changed::Imports`] tells when its report changes.
    pub fn start_import(&mut self, path: &Path) -> Result<u64, MobileError> {
        let Some(i) = (self.picked.iter()).position(|p| p.file == path) else {
            return Err(MobileError::Failed(format!(
                "{} is not a file the app named for an Import; it was left where it is",
                path.display()
            )));
        };
        if !matches!(self.picked[i].state, PickedState::Named) {
            return Ok(self.picked[i].id);
        }
        let extension = (path.extension())
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let state = match extension.as_str() {
            "obr" => self.add_obr(path),
            "usfm" | "sfm" => self.import_job(obs_engine::ImportSource::Usfm(path.to_path_buf())),
            "zip" => self.import_job(zip_source(path)),
            _ => PickedState::Refused(IMPORT_KINDS.to_string(), path_name(path)),
        };
        let done = !matches!(state, PickedState::Importing(_));
        self.picked[i].state = state;
        if done {
            let _ = std::fs::remove_dir_all(&self.picked[i].folder);
        }
        self.events.push(Changed::Imports);
        Ok(self.picked[i].id)
    }

    /// The shell could not copy the picked file to `path` (the file
    /// [`Self::import_destination`] named): its Import fails with `error` under Details.
    /// Returns the Import's number.
    pub fn import_copy_failed(&mut self, path: &Path, error: &str) -> Result<u64, MobileError> {
        let Some(picked) = (self.picked.iter_mut()).find(|p| p.file == path) else {
            return Err(MobileError::Failed(format!(
                "{} is not a file the app named for an Import",
                path.display()
            )));
        };
        if matches!(picked.state, PickedState::Named) {
            picked.state = PickedState::Refused(COPY_FAILED.to_string(), error.to_string());
            let _ = std::fs::remove_dir_all(&picked.folder);
            self.events.push(Changed::Imports);
        }
        Ok(picked.id)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn import_job(&mut self, source: obs_engine::ImportSource) -> PickedState {
        match self.session.start_import_job(source) {
            Ok((resource, _job)) => PickedState::Importing(resource),
            Err(e) => PickedState::Refused(failure_message(&e).to_string(), e),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn add_obr(&mut self, path: &Path) -> PickedState {
        match self.session.add_obr_copy(path) {
            Ok(added) => PickedState::Added {
                manifest: Box::new(added.manifest),
                outcome: added.outcome,
            },
            Err(e) => PickedState::Refused(e.message(), e.to_string()),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn import_job(&mut self, _: obs_engine::ImportSource) -> PickedState {
        PickedState::Refused("Import needs files".into(), String::new())
    }

    #[cfg(target_arch = "wasm32")]
    fn add_obr(&mut self, _: &Path) -> PickedState {
        PickedState::Refused("Import needs files".into(), String::new())
    }

    /// Stop a running Import; its report then says it was cancelled.
    pub fn cancel_import(&mut self, import: u64) -> Result<(), MobileError> {
        let picked = self.picked_import(import)?;
        let PickedState::Importing(resource) = &picked.state else {
            return Ok(());
        };
        let resource = resource.clone();
        if let Some(job) = self.session.imports.job_of(&resource) {
            self.session.jobs().cancel(job);
        }
        Ok(())
    }

    fn picked_import(&self, import: u64) -> Result<&Picked, MobileError> {
        (self.picked.iter())
            .find(|p| p.id == import)
            .ok_or_else(|| MobileError::Failed(format!("no Import {import}")))
    }

    /// The Import report of Import `import`.
    pub fn import_report(&self, import: u64) -> Result<ImportReportView, MobileError> {
        let picked = self.picked_import(import)?;
        Ok(self.report_of(picked))
    }

    fn report_of(&self, picked: &Picked) -> ImportReportView {
        let mut view = ImportReportView {
            import: picked.id,
            heading: "Import in progress".into(),
            source: path_name(&picked.file),
            state: ImportState::Running,
            progress: None,
            status: "Waiting for the file…".into(),
            failure: None,
            details: None,
            resource_id: None,
            sections: Vec::new(),
        };
        match &picked.state {
            PickedState::Named => {}
            PickedState::Refused(plain, error) => fail(&mut view, plain, error),
            PickedState::Added { manifest, outcome } => {
                view.heading = "Import complete".into();
                view.state = ImportState::Complete;
                view.status = self.index_line(&manifest.resource_id, None);
                view.resource_id = Some(manifest.resource_id.clone());
                let added = match outcome {
                    ObrCopy::Added => "Added to your Library as it is.".to_string(),
                    ObrCopy::Replaced => format!(
                        "Replaced {} in your Library with revision {}.",
                        manifest.abbreviation, manifest.revision
                    ),
                    ObrCopy::AlreadyThere => format!(
                        "Your Library already has this file ({} revision {}), so nothing \
                         was changed.",
                        manifest.abbreviation, manifest.revision
                    ),
                };
                view.sections.push(section(
                    "Summary",
                    vec![
                        manifest.title.clone(),
                        format!(
                            "{} · {}",
                            obs_engine::command_box::type_name(manifest.resource_type),
                            manifest.abbreviation
                        ),
                        added,
                    ],
                ));
            }
            PickedState::Importing(resource) => match self.session.imports.report(resource) {
                None => fail(&mut view, "The Import was stopped.", ""),
                Some(record) => self.record_view(&mut view, resource, record),
            },
        }
        view
    }

    fn record_view(&self, view: &mut ImportReportView, resource: &str, record: &ImportRecord) {
        let fraction = (record.total > 0).then(|| record.done as f32 / record.total as f32);
        match record.phase {
            ImportPhase::Failed => {
                let error = record.error.as_deref().unwrap_or_default();
                fail(view, failure_message(error), error);
            }
            ImportPhase::Download | ImportPhase::Parse => {
                view.progress = fraction;
                view.status = if record.total == 0 {
                    "Reading the file…".into()
                } else {
                    format!("{} / {} Bible books", record.done, record.total)
                };
            }
            ImportPhase::Index => {
                view.progress = fraction;
                view.status = if record.total == 0 {
                    "Search index: starting…".into()
                } else {
                    format!("Search index: {} / {}", record.done, record.total)
                };
            }
            ImportPhase::Complete => {
                view.heading = "Import complete".into();
                view.state = ImportState::Complete;
                view.status = self.index_line(resource, Some(&record.index));
                view.resource_id = Some(resource.to_string());
            }
        }
        if let Some(report) = &record.report {
            view.sections = report_sections(report);
        }
    }

    /// The Search index of `resource` in words, as the desktop's report says it: built by the
    /// Import, or the background build's state.
    fn index_line(&self, resource: &str, built: Option<&ImportIndexStatus>) -> String {
        if built == Some(&ImportIndexStatus::Built) {
            return "Search index: built".into();
        }
        match (self.session.search.status(resource), built) {
            (Some(IndexStatus::Ready), _) => "Search index: built".into(),
            (Some(IndexStatus::Indexing { done, total }), _) if *total > 0 => {
                format!("Search index: {done} / {total}")
            }
            (Some(IndexStatus::Unavailable), _) => UNAVAILABLE_INDEX.into(),
            (Some(IndexStatus::Failed(e)), _) | (_, Some(ImportIndexStatus::Failed(e))) => {
                format!("Search index failed: {e}")
            }
            _ => DEFERRED_INDEX.into(),
        }
    }

    /// Move each Import on (the pump calls it): a finished one's copy is deleted, and
    /// [`Changed::Imports`] is reported when any report changed. An Import whose final report
    /// was seen is skipped.
    pub(crate) fn advance_imports(&mut self) {
        let mut changed = false;
        for i in 0..self.picked.len() {
            if self.picked[i].settled || matches!(self.picked[i].state, PickedState::Named) {
                continue;
            }
            let view = self.report_of(&self.picked[i]);
            self.picked[i].settled = self.is_final(&view);
            if view.state != ImportState::Running {
                let folder = &self.picked[i].folder;
                if folder.exists() {
                    let _ = std::fs::remove_dir_all(folder);
                }
            }
            if self.picked[i].seen.as_ref() != Some(&view) {
                self.picked[i].seen = Some(view);
                changed = true;
            }
        }
        if changed {
            self.events.push(Changed::Imports);
        }
    }

    /// Whether `view` can no longer change: failed, or complete with its Search index built,
    /// failed or unavailable (until then its index line moves on).
    fn is_final(&self, view: &ImportReportView) -> bool {
        match (view.state, &view.resource_id) {
            (ImportState::Running, _) => false,
            (ImportState::Complete, Some(id)) => matches!(
                self.session.search.status(id),
                Some(IndexStatus::Ready | IndexStatus::Failed(_) | IndexStatus::Unavailable)
            ),
            _ => true,
        }
    }
}

/// A SWORD zip or a USFM zip, as what the zip holds says.
#[cfg(not(target_arch = "wasm32"))]
fn zip_source(path: &Path) -> obs_engine::ImportSource {
    obs_engine::ImportSource::zip(path.to_path_buf())
}

#[cfg(target_arch = "wasm32")]
fn zip_source(path: &Path) -> obs_engine::ImportSource {
    obs_engine::ImportSource::UsfmZip(path.to_path_buf())
}

fn path_name(path: &Path) -> String {
    (path.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn fail(view: &mut ImportReportView, plain: &str, error: &str) {
    view.heading = "Import failed".into();
    view.state = ImportState::Failed;
    view.progress = None;
    view.status = String::new();
    view.failure = Some(plain.to_string());
    view.details = (!error.is_empty()).then(|| error.to_string());
}

fn section(title: &str, lines: Vec<String>) -> ReportSection {
    ReportSection {
        title: title.to_string(),
        lines,
        warning: false,
        collapsed: false,
    }
}

fn folded(title: &str, lines: Vec<String>) -> ReportSection {
    ReportSection {
        collapsed: true,
        ..section(title, lines)
    }
}

/// What an Import brought in, in the desktop report's words and order: the summary, the
/// licence, the warnings, the dropped markup, and the files and Bible books read.
fn report_sections(report: &ImportReport) -> Vec<ReportSection> {
    let mut summary = vec![
        report.title.clone(),
        source_line(report),
        summary_counts(report),
    ];
    if let Some(counts) = report.verse_map {
        summary.push(format!("Unmapped verses: {}", counts.unmapped));
        summary.push(format!("Absent verses: {}", counts.absent));
    }
    let licence = ReportSection {
        title: "Licence".into(),
        lines: vec![if report.license_missing {
            LICENCE_MISSING.to_string()
        } else {
            LICENCE_RECORDED.to_string()
        }],
        warning: report.license_missing,
        collapsed: false,
    };
    let warnings = if report.warnings.is_empty() {
        vec!["No other warnings.".to_string()]
    } else {
        report.warnings.clone()
    };
    let dropped: u64 = report.dropped_markup.values().sum();
    let markup = if report.dropped_markup.is_empty() {
        vec!["No markup dropped.".to_string()]
    } else {
        (report.dropped_markup.iter())
            .map(|(marker, count)| format!("{marker} ×{count}"))
            .collect()
    };
    let mut files = report.source_files.clone();
    files.extend(
        report
            .books
            .iter()
            .map(|b| format!("{} · {} chapters · {} Verses", b.code, b.chapters, b.verses)),
    );
    vec![
        section("Summary", summary),
        licence,
        folded(&format!("Warnings ({})", report.warnings.len()), warnings),
        folded(&format!("Dropped markup ({dropped})"), markup),
        folded("Source files and Bible books", files),
    ]
}

/// Delete what an Import left in `imports/` (picked files, unpacked modules): no Import
/// outlives the run that started it.
pub(crate) fn sweep_imports(folder: &Path) {
    let _ = std::fs::remove_dir_all(folder);
}
