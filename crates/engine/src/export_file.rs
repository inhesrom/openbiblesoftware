//! Writing an export to a file the user chose (#252).
//!
//! Every export command (`notes.export`, `highlight_palette.export`, `search.export_csv`,
//! `data.export`) writes through [`Session::write_export`], so each one:
//!
//! - refuses the User database and SQLite's files beside it (`-wal`, `-shm`, `-journal`),
//!   by whatever path names them, as a Backup does; and
//! - replaces a file already there whole or not at all: the export is written to a scratch
//!   file beside the destination and renamed over it only once complete, so a failed
//!   export (a full disk, a file size limit) leaves the previous file byte-identical.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::session::Session;

/// The extension of the scratch file an export is written to before it is renamed into
/// place. The scratch file's name starts with a `.`, so it is hidden while it exists.
const SCRATCH_EXTENSION: &str = "partial";

impl Session {
    /// Write `bytes` to `dest`, replacing a file there, unless `dest` is the User database
    /// or one of SQLite's files beside it. A failure leaves `dest` as it was.
    pub fn write_export(&self, dest: &Path, bytes: &[u8]) -> Result<(), String> {
        let own_file = |path: &Path| match &self.user {
            Some(user) => user.refuse_own_file(path),
            None => Ok(()),
        };
        own_file(dest)?;
        replace_file(dest, bytes, own_file).map_err(|e| format!("{}: {e}", dest.display()))
    }
}

/// Write `bytes` to a new scratch file beside `dest`, then rename it over `dest`. A scratch
/// name `refuse` rejects is skipped, and the scratch file is removed if anything fails.
fn replace_file(
    dest: &Path,
    bytes: &[u8],
    refuse: impl Fn(&Path) -> Result<(), String>,
) -> std::io::Result<()> {
    let (scratch, mut file) = new_scratch_file(dest, refuse)?;
    let written = file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .and_then(|()| {
            drop(file);
            std::fs::rename(&scratch, dest)
        });
    if written.is_err() {
        let _ = std::fs::remove_file(&scratch);
    }
    written
}

/// Create `.<name>.<pid>-<n>.partial` beside `dest`, a name not already taken (a file there
/// is never touched), so removing it afterwards only removes what this export made.
fn new_scratch_file(
    dest: &Path,
    refuse: impl Fn(&Path) -> Result<(), String>,
) -> std::io::Result<(PathBuf, std::fs::File)> {
    let name = dest
        .file_name()
        .map_or_else(|| "export".into(), |n| n.to_string_lossy().into_owned());
    let mut last = None;
    for n in 0..1000u32 {
        let scratch = dest.with_file_name(format!(
            ".{name}.{}-{n}.{SCRATCH_EXTENSION}",
            std::process::id()
        ));
        if refuse(&scratch).is_err() {
            continue;
        }
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&scratch)
        {
            Ok(file) => return Ok((scratch, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("no free name for a scratch file")))
}
