//! Writing an export to a file the user chose (#252).
//!
//! Every export command (`notes.export`, `highlight_palette.export`, `search.export_csv`,
//! `data.export`) writes through [`Session::write_export`], so each one:
//!
//! - refuses the User database and SQLite's files beside it (`-wal`, `-shm`, `-journal`),
//!   by whatever path names them, as a Backup does; and
//! - replaces a file already there whole or not at all: the export is written to a scratch
//!   file beside the destination and renamed over it only once complete, so a failed
//!   export (a full disk, a file size limit) leaves the previous file byte-identical. The
//!   scratch file is owner-only while it is written, and the export keeps the permissions
//!   of the file it replaces, so a private export stays private.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::session::Session;

/// The scratch file an export is written to before it is renamed into place is
/// `.obs-export-<pid>-<n>.partial`: hidden while it exists, and short whatever the
/// destination is called, so any name a disk accepts for the export also fits it.
const SCRATCH_PREFIX: &str = ".obs-export";
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
    let written = (|| {
        let permissions = owner_only(&file)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        // The replaced file's permissions, so a private export stays private; a new file
        // gets the permissions any new file gets.
        let permissions = match std::fs::metadata(dest) {
            Ok(previous) => previous.permissions(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => permissions,
            Err(e) => return Err(e),
        };
        file.set_permissions(permissions)?;
        drop(file);
        std::fs::rename(&scratch, dest)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&scratch);
    }
    written
}

/// Make the new, still empty scratch `file` readable by its owner only while the export is
/// written to it, and return the permissions it was created with (a new file's defaults).
#[cfg(unix)]
fn owner_only(file: &std::fs::File) -> std::io::Result<std::fs::Permissions> {
    use std::os::unix::fs::PermissionsExt as _;
    let created = file.metadata()?.permissions();
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    Ok(created)
}

#[cfg(not(unix))]
fn owner_only(file: &std::fs::File) -> std::io::Result<std::fs::Permissions> {
    Ok(file.metadata()?.permissions())
}

/// Create a scratch file beside `dest` under a name not already taken (a file there is
/// never touched), so removing it afterwards only removes what this export made.
fn new_scratch_file(
    dest: &Path,
    refuse: impl Fn(&Path) -> Result<(), String>,
) -> std::io::Result<(PathBuf, std::fs::File)> {
    let mut last = None;
    for n in 0..1000u32 {
        let scratch = dest.with_file_name(format!(
            "{SCRATCH_PREFIX}-{}-{n}.{SCRATCH_EXTENSION}",
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
