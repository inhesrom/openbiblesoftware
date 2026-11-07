//! Backups of the User database (#217); the SQLite side is `backup_store.rs`.
//!
//! A Backup is one file: a copy of the User database, with its `application_id` and its
//! format version (`user_version`), so a Backup from an older app is migrated when it is
//! restored and one from a newer app is refused.

use std::path::PathBuf;

/// The file name extension of a Backup made with Back up now.
pub const BACKUP_EXTENSION: &str = "obsbackup";

/// How many launch backups are kept.
pub const ROLLING_BACKUPS: usize = 5;

/// How many Safety backups are kept.
pub const SAFETY_BACKUPS: usize = 5;

/// Why the app kept a Backup by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupKind {
    /// Taken at a launch (the newest few are kept).
    Launch,
    /// Taken just before a restore, so the restore can be undone.
    Safety,
    /// Taken before the app upgraded the User database's format.
    BeforeUpgrade,
}

impl BackupKind {
    /// How the Back up & restore Tab names it.
    pub fn label(self) -> &'static str {
        match self {
            BackupKind::Launch => "At launch",
            BackupKind::Safety => "Safety backup",
            BackupKind::BeforeUpgrade => "Before an app upgrade",
        }
    }
}

/// A Backup the app keeps by itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupFile {
    pub path: PathBuf,
    pub kind: BackupKind,
    /// When it was taken (its file's modification time), Unix seconds.
    pub taken_at: i64,
}

/// What a User database or a Backup holds, as a restore preview compares them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BackupContents {
    /// The format version it was written in.
    pub version: u32,
    /// Notebook names, the default first.
    pub notebooks: Vec<String>,
    pub notes: u64,
    pub highlights: u64,
    /// Layout names.
    pub layouts: Vec<String>,
    /// Reading plan names.
    pub reading_plans: Vec<String>,
    pub favorites: u64,
    /// History's Visits.
    pub visits: u64,
    /// Saved preferences (View settings, Collections, Saved searches, keys and the rest).
    pub settings: u64,
}
