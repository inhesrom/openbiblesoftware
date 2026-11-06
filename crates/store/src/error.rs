use std::fmt;

use obs_model::ValidationErrors;

/// Why a Resource file could not be written or read.
#[derive(Debug)]
pub enum StoreError {
    Io(std::io::Error),
    /// The SQLite layer failed; the message is SQLite's.
    Sqlite(String),
    /// The file is not a `.obr` (wrong application id or missing tables).
    NotAnObr(String),
    /// Written by a newer build; refuse rather than misread it.
    NewerFormat {
        found: u32,
        supported: u32,
    },
    /// Written by an older build; re-import from the original source.
    OlderFormat {
        found: u32,
        /// The oldest format version this build reads.
        supported: u32,
    },
    /// Data in the file is malformed.
    Corrupt(String),
    /// The Resource in the file does not pass validation.
    Invalid(ValidationErrors),
    UnknownUnit(String),
    /// A User database edit was planned against rows that have changed since; nothing was
    /// written.
    Conflict(String),
    /// A User database edit breaks a rule (such as deleting the default Notebook); nothing
    /// was written.
    Rejected(String),
    /// The caller cancelled a long build (a Search index).
    Cancelled,
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Io(e) => write!(f, "file error: {e}"),
            StoreError::Sqlite(e) => write!(f, "database error: {e}"),
            StoreError::NotAnObr(why) => write!(f, "not an Open Bible resource file: {why}"),
            StoreError::NewerFormat { found, supported } => write!(
                f,
                "this resource uses format version {found}, but this app reads up to version {supported}; update the app to open it"
            ),
            StoreError::OlderFormat { found, supported } => write!(
                f,
                "this resource uses old format version {found} (this app reads version {supported} and newer); import it again from its source"
            ),
            StoreError::Corrupt(why) => write!(f, "resource file is damaged: {why}"),
            StoreError::Invalid(errors) => write!(f, "resource file is invalid: {errors}"),
            StoreError::UnknownUnit(id) => write!(f, "no text unit `{id}` in this resource"),
            StoreError::Conflict(why) => write!(f, "{why} changed since; nothing was saved"),
            StoreError::Rejected(why) => f.write_str(why),
            StoreError::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Io(e)
    }
}
