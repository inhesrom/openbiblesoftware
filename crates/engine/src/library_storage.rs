//! The space each Resource takes (#333: Library → Storage shows space used per Resource, and
//! Remove frees it) and whether a download fits in the free space the shell reports (#333:
//! "a download that won't fit says so before it starts").

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::session::Session;

/// What one Resource takes on disk, in bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResourceStorage {
    pub resource_id: String,
    /// Its `.obr` file.
    pub obr: u64,
    /// Its Search index caches: the app's cache folder and the prebuilt ones beside the `.obr`.
    pub search_index: u64,
    /// The alias caches it is one side of (a People & places index and a Dictionary). A cache
    /// is shared by its two Resources, so it counts for each.
    pub aliases: u64,
}

impl ResourceStorage {
    pub fn total(&self) -> u64 {
        self.obr + self.search_index + self.aliases
    }
}

/// What the library takes: each Resource in Preferred order, and the total, where a file
/// shared by two Resources (an alias cache) counts once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LibraryStorage {
    pub resources: Vec<ResourceStorage>,
    pub total: u64,
}

/// The free space a download of `bytes` needs before it starts: the downloaded file, its
/// staged copy in the library folder while it is put in place, and room for the Search index
/// built after it (half the file; the Study set's indexes are 0.34 of its files, S04).
pub fn space_needed(bytes: u64) -> u64 {
    bytes.saturating_mul(2).saturating_add(bytes / 2)
}

/// Whether a download of `bytes` fits in `free` bytes ([`space_needed`]).
pub fn fits(bytes: u64, free: u64) -> bool {
    space_needed(bytes) <= free
}

/// Whether an alias cache file name has `resource_id` as one side
/// (`<index>.<hash>--<dictionary>.<hash>.v<N>.aliases.json`).
fn alias_cache_of(name: &str, resource_id: &str) -> bool {
    name.ends_with(crate::aliases::ALIAS_EXTENSION)
        && (name.starts_with(&format!("{resource_id}."))
            || name.contains(&format!("--{resource_id}.")))
}

fn files_in(dir: &Path, keep: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    rd.filter_map(Result::ok)
        .filter(|e| keep(&e.file_name().to_string_lossy()))
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect()
}

fn size_of(path: &Path) -> u64 {
    std::fs::metadata(path).map_or(0, |m| m.len())
}

impl Session {
    /// What each Resource of the library takes on disk, in Preferred order.
    pub fn library_storage(&self) -> LibraryStorage {
        let search_cache = self.search.cache_dir().map(Path::to_path_buf);
        let alias_cache = self.guides.aliases().dir();
        let mut counted: BTreeSet<PathBuf> = BTreeSet::new();
        let mut out = LibraryStorage::default();
        for id in self.library.preferred_order() {
            let Some(entry) = self.library.get(&id) else {
                continue;
            };
            let beside = entry.path.parent().map(Path::to_path_buf);
            let folders = |own: &Option<PathBuf>, name: &str| -> Vec<PathBuf> {
                let mut dirs: Vec<PathBuf> = own.iter().cloned().collect();
                if let Some(b) = &beside {
                    let b = b.join(name);
                    if !dirs.contains(&b) {
                        dirs.push(b);
                    }
                }
                dirs
            };
            let mut index_files: Vec<PathBuf> = Vec::new();
            for dir in folders(&search_cache, "search") {
                index_files.extend(obs_store::search_index_files(&dir, &id));
            }
            let mut alias_files: Vec<PathBuf> = Vec::new();
            for dir in folders(&alias_cache, crate::aliases::ALIAS_DIR) {
                alias_files.extend(files_in(&dir, |name| alias_cache_of(name, &id)));
            }
            let row = ResourceStorage {
                resource_id: id.clone(),
                obr: size_of(&entry.path),
                search_index: index_files.iter().map(|p| size_of(p)).sum(),
                aliases: alias_files.iter().map(|p| size_of(p)).sum(),
            };
            for path in std::iter::once(entry.path.clone())
                .chain(index_files)
                .chain(alias_files)
            {
                if counted.insert(path.clone()) {
                    out.total += size_of(&path);
                }
            }
            out.resources.push(row);
        }
        out
    }
}
