//! The library: the Resources the user has, listed from their manifests. The order of the list
//! is the Preferred order within each Resource type (#18 rule 5).

use std::path::PathBuf;

use obs_model::{Manifest, ResourceType};

/// One Resource in the library.
#[derive(Debug, Clone)]
pub struct LibraryEntry {
    pub manifest: Manifest,
    /// Where its `.obr` file is.
    pub path: PathBuf,
}

/// The Resources the Session knows about.
#[derive(Debug, Clone, Default)]
pub struct Library {
    entries: Vec<LibraryEntry>,
}

impl Library {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entries(&self) -> &[LibraryEntry] {
        &self.entries
    }

    pub fn get(&self, resource_id: &str) -> Option<&LibraryEntry> {
        self.entries
            .iter()
            .find(|e| e.manifest.resource_id == resource_id)
    }

    /// Add a Resource at the bottom of its type, or update its path if it is already listed.
    pub fn add(&mut self, entry: LibraryEntry) {
        if let Some(e) = self
            .entries
            .iter_mut()
            .find(|e| e.manifest.resource_id == entry.manifest.resource_id)
        {
            *e = entry;
            return;
        }
        let ty = entry.manifest.resource_type;
        let at = self
            .entries
            .iter()
            .rposition(|e| e.manifest.resource_type == ty)
            .map_or(self.entries.len(), |i| i + 1);
        self.entries.insert(at, entry);
    }

    /// Resources of one type, in Preferred order.
    pub fn of_type(&self, ty: ResourceType) -> impl Iterator<Item = &LibraryEntry> {
        self.entries
            .iter()
            .filter(move |e| e.manifest.resource_type == ty)
    }

    /// Read the manifest of one `.obr` file and add it.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn add_file(
        &mut self,
        path: &std::path::Path,
    ) -> Result<&LibraryEntry, obs_store::StoreError> {
        use obs_store::ResourceStore;
        let reader = obs_store::ObrReader::open(path)?;
        let manifest = reader.manifest().clone();
        let id = manifest.resource_id.clone();
        self.add(LibraryEntry {
            manifest,
            path: path.to_path_buf(),
        });
        self.get(&id).ok_or_else(|| {
            obs_store::StoreError::Io(std::io::Error::other("library entry vanished"))
        })
    }

    /// Add every `.obr` file in `dir` (sorted by file name), skipping unreadable ones.
    /// Returns the files that could not be read, with the reason.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn scan(&mut self, dir: &std::path::Path) -> Vec<(PathBuf, String)> {
        let mut failed = Vec::new();
        let Ok(rd) = std::fs::read_dir(dir) else {
            return failed;
        };
        let mut files: Vec<PathBuf> = rd
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "obr"))
            .collect();
        files.sort();
        for f in files {
            if let Err(e) = self.add_file(&f) {
                failed.push((f, e.to_string()));
            }
        }
        failed
    }
}

/// The default library directory: `$OBS_LIBRARY`, else `library` in the app's data folder.
pub fn default_library_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("OBS_LIBRARY") {
        return Some(PathBuf::from(d));
    }
    Some(data_dir()?.join("library"))
}

/// The User database: `$OBS_USER_DB`, else `user.db` in the app's data folder.
pub fn default_user_db() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("OBS_USER_DB") {
        return Some(PathBuf::from(d));
    }
    Some(data_dir()?.join("user.db"))
}

/// The app's per-user data folder on this platform.
fn data_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let base = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        home.map(|h| h.join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| home.map(|h| h.join(".local/share")))
    }?;
    Some(base.join("OpenBible"))
}
