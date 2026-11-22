//! Import across the FFI (`docs/dev/mobile-library.md` § Import): where the shell copies a
//! picked or shared file, handing it in, and the Import report. Mirror Records and Enums of
//! `obs_mobile::import`; every rule and every word of the report stays there.

use std::path::PathBuf;

use crate::{MobileApp, MobileError};

/// Where an Import's report is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ImportState {
    Running,
    Complete,
    Failed,
}

impl From<obs_mobile::ImportState> for ImportState {
    fn from(s: obs_mobile::ImportState) -> Self {
        match s {
            obs_mobile::ImportState::Running => Self::Running,
            obs_mobile::ImportState::Complete => Self::Complete,
            obs_mobile::ImportState::Failed => Self::Failed,
        }
    }
}

/// One group of an Import report's lines, under its title.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ReportSection {
    pub title: String,
    pub lines: Vec<String>,
    /// Whether the lines warn (no licence found).
    pub warning: bool,
    /// Whether it starts folded, its title alone shown.
    pub collapsed: bool,
}

/// The Import report of one picked file (`obs_mobile::ImportReportView`).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ImportReport {
    pub import: u64,
    pub heading: String,
    pub source: String,
    pub state: ImportState,
    /// 0 to 1 while running, when known.
    pub progress: Option<f32>,
    pub status: String,
    pub failure: Option<String>,
    pub details: Option<String>,
    /// The Resource Open shows, once complete.
    pub resource_id: Option<String>,
    pub sections: Vec<ReportSection>,
}

impl From<obs_mobile::ImportReportView> for ImportReport {
    fn from(v: obs_mobile::ImportReportView) -> Self {
        Self {
            import: v.import,
            heading: v.heading,
            source: v.source,
            state: v.state.into(),
            progress: v.progress,
            status: v.status,
            failure: v.failure,
            details: v.details,
            resource_id: v.resource_id,
            sections: (v.sections.into_iter())
                .map(|s| ReportSection {
                    title: s.title,
                    lines: s.lines,
                    warning: s.warning,
                    collapsed: s.collapsed,
                })
                .collect(),
        }
    }
}

#[uniffi::export]
impl MobileApp {
    /// Where to copy a file the user picked or shared, named `file_name`: a folder of its
    /// own under the library's `imports/`. Copy it there (with the file's security-scoped
    /// access), then hand that path to [`MobileApp::start_import`].
    pub fn import_destination(&self, file_name: String) -> Result<String, MobileError> {
        self.with(|a| a.import_destination(&file_name))
            .map(|p| p.to_string_lossy().into_owned())
    }

    /// Import the file copied to `path` (the one `import_destination` named; any other is
    /// refused and left alone). Returns the Import's number; `poll_changed` reports `imports`
    /// as its report moves on.
    pub fn start_import(&self, path: String) -> Result<u64, MobileError> {
        self.with(|a| a.start_import(&PathBuf::from(path)))
    }

    /// The shell could not copy the picked file to `path` (the file `import_destination`
    /// named): its Import fails, `error` under Details. Returns the Import's number.
    pub fn import_copy_failed(&self, path: String, error: String) -> Result<u64, MobileError> {
        self.with(|a| a.import_copy_failed(&PathBuf::from(path), &error))
    }

    /// Import `import`'s report.
    pub fn import_report(&self, import: u64) -> Result<ImportReport, MobileError> {
        self.read(|a| a.import_report(import))?
            .map(ImportReport::from)
            .map_err(MobileError::from)
    }

    /// Stop a running Import (its report then says it was cancelled).
    pub fn cancel_import(&self, import: u64) -> Result<(), MobileError> {
        self.with(|a| a.cancel_import(import))
    }
}
