//! Zips the Mobile app imports (#333 "Import"; `docs/dev/mobile-library.md`): a SWORD module
//! zip, unpacked into a temp folder under the library's [`IMPORTS_DIR`] and read as a SWORD
//! folder, and a zip of USFM books, read in memory by `obs_formats::usfm_zip_books`. The
//! desktop's Import never makes these sources ([`crate::ImportSource::detect`] is unchanged).
//!
//! **Where a SWORD module may sit.** A zip holding a `mods.d/*.conf` is a SWORD zip. Its
//! `mods.d/` is at the top of the zip (CrossWire's package zips) or in one folder at its top
//! (what Files' Compress makes of a folder); deeper, or under several folders, it is refused.
//! One module per zip: a zip of several is refused (there is no module chooser on mobile).
//! USFM files beside the module are not imported, and the Import report says so. macOS
//! resource forks (`__MACOSX/`, `._*` files) are left out.
//!
//! **What is never unpacked.** Every entry's path is checked before anything is written: an
//! absolute path, a drive letter, a backslash, a `..` component or a symbolic link refuses the
//! whole zip. At most [`ZIP_IMPORT_ENTRIES`] entries and [`ZIP_IMPORT_LIMIT`] bytes
//! decompressed (counted as read, not as the zip declares them). The temp folder is removed
//! whatever the outcome.

use std::io::Read;
use std::path::{Path, PathBuf};

pub use crate::import::IMPORTS_DIR;

/// Most bytes a zip Import decompresses (a SWORD module unpacked, or a USFM zip's file read).
pub const ZIP_IMPORT_LIMIT: u64 = 512 * 1024 * 1024;

/// Most entries (files and folders) a zip Import reads.
pub const ZIP_IMPORT_ENTRIES: usize = 10_000;

/// How much one zip Import may unpack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ZipBounds {
    /// Bytes decompressed, in all.
    pub(crate) bytes: u64,
    /// Entries, files and folders.
    pub(crate) entries: usize,
}

impl ZipBounds {
    pub(crate) const DEFAULT: ZipBounds = ZipBounds {
        bytes: ZIP_IMPORT_LIMIT,
        entries: ZIP_IMPORT_ENTRIES,
    };

    fn too_large(self) -> String {
        format!("this zip unpacks to more than {}", mib(self.bytes))
    }

    fn too_many(self) -> String {
        format!("this zip has more than {} files", self.entries)
    }
}

fn mib(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{} MiB", bytes / (1024 * 1024))
    } else {
        format!("{bytes} bytes")
    }
}

/// Added to a SWORD zip's Import report when the zip also holds USFM files.
pub const USFM_BESIDE_SWORD: &str =
    "This zip also holds USFM files; they were not imported. Zip them on their own to import them.";

/// What a zip holds, as far as Import is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZipKind {
    /// It holds a SWORD module's `mods.d/*.conf`.
    Sword,
    /// Anything else, read as a zip of USFM books (a file that is not a zip fails there).
    Usfm,
}

/// What `path` holds: a SWORD zip if any entry is a `mods.d/*.conf` (resource forks aside),
/// else a USFM zip. A file that cannot be read as a zip is a USFM zip, whose Import then
/// says so.
pub fn zip_kind(path: &Path) -> ZipKind {
    let Ok(file) = std::fs::File::open(path) else {
        return ZipKind::Usfm;
    };
    let Ok(zip) = zip::ZipArchive::new(std::io::BufReader::new(file)) else {
        return ZipKind::Usfm;
    };
    let sword = zip.file_names().any(|name| {
        let parts: Vec<&str> = name.split('/').filter(|p| !p.is_empty()).collect();
        !ignored(&parts) && conf_root(&parts).is_some()
    });
    if sword { ZipKind::Sword } else { ZipKind::Usfm }
}

/// A macOS resource fork: anything under `__MACOSX/`, or a `._name` file.
fn ignored(parts: &[&str]) -> bool {
    parts.first() == Some(&"__MACOSX") || parts.last().is_some_and(|p| p.starts_with("._"))
}

/// For `…/mods.d/<name>.conf`, the folders before `mods.d`.
fn conf_root<'a>(parts: &'a [&'a str]) -> Option<&'a [&'a str]> {
    match parts {
        [root @ .., dir, file]
            if dir.eq_ignore_ascii_case("mods.d")
                && file.to_ascii_lowercase().ends_with(".conf") =>
        {
            Some(root)
        }
        _ => None,
    }
}

/// An entry's path as safe components, or why it is refused: absolute, a drive letter, a
/// backslash, a NUL or a `..` component would let it land outside the folder it is unpacked
/// into. `.` components are dropped.
fn safe_parts(name: &str) -> Result<Vec<&str>, String> {
    let outside = || format!("this zip has a file that would land outside its folder ({name})");
    if name.starts_with('/') || name.contains('\\') || name.contains('\0') {
        return Err(outside());
    }
    let parts: Vec<&str> = name
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    let drive = parts.first().is_some_and(|p| p.contains(':'));
    if drive || parts.contains(&"..") {
        return Err(outside());
    }
    Ok(parts)
}

/// Where a SWORD zip's module is, once every entry is checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SwordLayout {
    /// The folders above `mods.d/` (none, or the one folder at the top).
    pub(crate) root: Vec<String>,
    /// Whether the zip also holds USFM files.
    pub(crate) usfm_beside: bool,
}

/// Check every entry of a SWORD zip and find its module (module docs), before anything is
/// unpacked.
pub(crate) fn sword_layout<R: Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    bounds: ZipBounds,
) -> Result<SwordLayout, String> {
    if zip.len() > bounds.entries {
        return Err(bounds.too_many());
    }
    let mut declared: u64 = 0;
    let mut confs: Vec<(Vec<String>, String)> = Vec::new();
    let mut usfm_beside = false;
    for i in 0..zip.len() {
        let entry = zip.by_index_raw(i).map_err(|e| e.to_string())?;
        let name = entry.name().to_string();
        let parts = safe_parts(&name)?;
        if entry.is_symlink() {
            return Err(format!("this zip has a symbolic link ({name})"));
        }
        if ignored(&parts) {
            continue;
        }
        declared = declared.saturating_add(entry.size());
        if entry.is_dir() {
            continue;
        }
        if let Some(root) = conf_root(&parts) {
            let module = parts.last().map_or("", |f| &f[..f.len() - ".conf".len()]);
            confs.push((
                root.iter().map(|p| (*p).to_string()).collect(),
                module.to_string(),
            ));
        }
        let lower = name.to_ascii_lowercase();
        usfm_beside |= lower.ends_with(".usfm") || lower.ends_with(".sfm");
    }
    if declared > bounds.bytes {
        return Err(bounds.too_large());
    }
    let (root, _) = confs.first().cloned().ok_or(
        "this zip holds no USFM books (.usfm or .sfm files) and no SWORD module (mods.d/)",
    )?;
    if confs.len() > 1 {
        let names: Vec<&str> = confs.iter().map(|(_, m)| m.as_str()).collect();
        return Err(format!(
            "this zip holds several SWORD modules ({}); zip one module, then import it",
            names.join(", ")
        ));
    }
    if root.len() > 1 {
        return Err(
            "this zip's SWORD module is not where the app looks: mods.d/ must be at the top of \
             the zip, or in one folder at its top"
                .into(),
        );
    }
    Ok(SwordLayout { root, usfm_beside })
}

/// Unpack the SWORD zip at `path` into `dest` (made afresh; the caller removes it), checking
/// it first ([`sword_layout`]), until `is_cancelled`. Returns the SWORD folder in it (the one holding `mods.d/`) and
/// whether the zip also held USFM files.
pub(crate) fn unpack_sword_zip(
    path: &Path,
    dest: &Path,
    bounds: ZipBounds,
    is_cancelled: impl Fn() -> bool,
) -> Result<(PathBuf, bool), Box<dyn std::error::Error>> {
    let file = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file))
        .map_err(|e| format!("archive: not a zip archive: {e}"))?;
    let layout = sword_layout(&mut zip, bounds)?;
    if dest.exists() {
        std::fs::remove_dir_all(dest)?;
    }
    std::fs::create_dir_all(dest)?;
    let mut used: u64 = 0;
    for i in 0..zip.len() {
        if is_cancelled() {
            return Err("Import cancelled".into());
        }
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().to_string();
        let parts = safe_parts(&name)?;
        if ignored(&parts) || parts.is_empty() {
            continue;
        }
        let out: PathBuf = parts.iter().fold(dest.to_path_buf(), |p, c| p.join(c));
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let left = bounds.bytes.saturating_sub(used);
        let mut writer = std::fs::File::create_new(&out)?;
        let read = std::io::copy(&mut (&mut entry).take(left + 1), &mut writer)?;
        if read > left {
            return Err(bounds.too_large().into());
        }
        used += read;
    }
    let root = layout
        .root
        .iter()
        .fold(dest.to_path_buf(), |p, c| p.join(c));
    Ok((root, layout.usfm_beside))
}

/// The bytes of the USFM zip at `path`, refused past [`ZIP_IMPORT_LIMIT`] bytes or
/// [`ZIP_IMPORT_ENTRIES`] entries (`usfm_zip_books` bounds what it decompresses itself).
pub(crate) fn read_usfm_zip(
    path: &Path,
    bounds: ZipBounds,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(bounds.bytes.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > bounds.bytes {
        return Err(bounds.too_large().into());
    }
    if let Ok(zip) = zip::ZipArchive::new(std::io::Cursor::new(bytes.as_slice()))
        && zip.len() > bounds.entries
    {
        return Err(bounds.too_many().into());
    }
    Ok(bytes)
}
