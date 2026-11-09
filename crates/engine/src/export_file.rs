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
//!   scratch file is created owner-only, so no one else can open it while it is written,
//!   and the export keeps the permissions of the file it replaces, so a private export
//!   stays private. A read-only file is not replaced: the export fails and leaves it as it
//!   was.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::backups::file_name_key;
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
    // The replaced file's permissions, so a private export stays private; a new file gets
    // the permissions any new file gets. A read-only file is not replaced (as writing into
    // it would fail), and so the scratch file is never made read-only, which Windows would
    // then not let this remove after a failed rename.
    let previous = match std::fs::metadata(dest) {
        Ok(previous) if previous.permissions().readonly() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "the file there is read-only, so the export did not replace it",
            ));
        }
        Ok(previous) => Some(previous.permissions()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    let (scratch, mut file) = new_scratch_file(dest, &refuse, Scratch::OwnerOnly)?;
    let written = (|| {
        let permissions = match previous {
            Some(previous) => previous,
            None => new_file_permissions(dest, &refuse)?,
        };
        file.write_all(bytes)?;
        file.sync_all()?;
        file.set_permissions(permissions)?;
        drop(file);
        std::fs::rename(&scratch, dest)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&scratch);
    }
    written
}

/// The permissions a new file beside `dest` gets (on Unix `0666` less the umask, as
/// `std::fs::write` would give it), read from an empty scratch file made for that and removed
/// at once, so the process umask is never changed and the export's own scratch file is
/// owner-only from the moment it exists.
fn new_file_permissions(
    dest: &Path,
    refuse: impl Fn(&Path) -> Result<(), String>,
) -> std::io::Result<std::fs::Permissions> {
    let (probe, file) = new_scratch_file(dest, refuse, Scratch::NewFileDefaults)?;
    let permissions = file.metadata().map(|m| m.permissions());
    drop(file);
    let removed = std::fs::remove_file(&probe);
    let permissions = permissions?;
    removed?;
    Ok(permissions)
}

/// Whom a scratch file is readable by from the moment it is created.
#[derive(Clone, Copy)]
pub(crate) enum Scratch {
    /// Its owner only (`0600` on Unix): the file an export is written to.
    OwnerOnly,
    /// Whoever a new file is readable by: an empty file made to learn that.
    NewFileDefaults,
}

/// Create a scratch file beside `dest` under a name not already taken (a file there is
/// never touched), so removing it afterwards only removes what this export made. A name a
/// disk that ignores case and Unicode normalisation, or Windows's trailing periods and
/// spaces, could take for `dest` itself is skipped, so the export is never written straight
/// into `dest` and renamed onto itself. A name `dest` turns out to be another name for all
/// the same (`dest` was absent and now exists: a Windows short `8.3` name, a link) is
/// removed again and skipped.
pub(crate) fn new_scratch_file(
    dest: &Path,
    refuse: impl Fn(&Path) -> Result<(), String>,
    readable_by: Scratch,
) -> std::io::Result<(PathBuf, std::fs::File)> {
    let dest_key = dest
        .file_name()
        .map(|name| file_name_key(&name.to_string_lossy()));
    let mut last = None;
    for n in 0..1000u32 {
        let name = format!(
            "{SCRATCH_PREFIX}-{}-{n}.{SCRATCH_EXTENSION}",
            std::process::id()
        );
        if dest_key.as_deref() == Some(file_name_key(&name).as_str()) {
            continue;
        }
        let scratch = dest.with_file_name(name);
        if refuse(&scratch).is_err() {
            continue;
        }
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        set_readable_by(&mut options, readable_by);
        let dest_was_absent = is_absent(dest);
        match options.open(&scratch) {
            Ok(file) if dest_was_absent && !is_absent(dest) => {
                drop(file);
                std::fs::remove_file(&scratch)?;
            }
            Ok(file) => return Ok((scratch, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("no free name for a scratch file")))
}

/// Whether nothing is at `path` (a link to nothing counts as nothing).
fn is_absent(path: &Path) -> bool {
    matches!(std::fs::metadata(path), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
}

/// Create the file readable by its owner only (`0600`) if so asked: set at creation, so no
/// other user can open it between its creation and a later change of permissions.
#[cfg(unix)]
fn set_readable_by(options: &mut std::fs::OpenOptions, readable_by: Scratch) {
    use std::os::unix::fs::OpenOptionsExt as _;
    if let Scratch::OwnerOnly = readable_by {
        options.mode(0o600);
    }
}

/// Off Unix a file has no permission bits for other users to be kept from.
#[cfg(not(unix))]
fn set_readable_by(_options: &mut std::fs::OpenOptions, _readable_by: Scratch) {}
