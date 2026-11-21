//! The Sync folder as the engine reaches it (ADR 0022, plan A8): a folder in the user's own
//! cloud storage, read and written only through [`SyncFolder`]. The desktop app implements it
//! with `std::fs` ([`FsSyncFolder`]); the Mobile app implements it in Swift, with
//! security-scoped access, `NSFileCoordinator` and iCloud downloads; tests use
//! [`MemSyncFolder`]. Every rule about the files (names, hash lines, clocks) stays in Rust.
//!
//! Paths are relative to the folder the user picked, with `/` between their parts, and never
//! hold an empty part, `.` or `..`.

#[cfg(any(test, feature = "test-support"))]
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

/// Why the Sync folder could not do what was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderError {
    /// No file at that path.
    NotFound(String),
    /// The path is not one of the Sync folder's (empty, absolute, or with `.` or `..`).
    BadPath(String),
    /// The folder or the platform refused, or the disk failed; the message is theirs.
    Io(String),
}

impl fmt::Display for FolderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FolderError::NotFound(path) => write!(f, "{path} is not in the Sync folder"),
            FolderError::BadPath(path) => write!(f, "`{path}` is not a path in the Sync folder"),
            FolderError::Io(why) => write!(f, "the Sync folder: {why}"),
        }
    }
}

impl std::error::Error for FolderError {}

/// The folder the user picked for Sync, as the engine reads and writes it. Each method does
/// one whole thing, so a shell implements it with its platform's file access.
pub trait SyncFolder: Send + Sync {
    /// The names of the files and folders directly in `dir` (`""` is the folder itself), in
    /// any order; none when `dir` does not exist yet.
    fn list(&self, dir: &str) -> Result<Vec<String>, FolderError>;
    /// The whole file at `path`.
    fn read(&self, path: &str) -> Result<Vec<u8>, FolderError>;
    /// Write `bytes` as the file at `path`, whole: to a temporary name in the same folder
    /// that starts with `.`, then renamed to `path` (replacing a file there), so no reader
    /// ever sees part of it under its name. Makes the folders it needs.
    fn write_new(&self, path: &str, bytes: &[u8]) -> Result<(), FolderError>;
    /// Delete the file at `path` (nothing when there is none).
    fn remove(&self, path: &str) -> Result<(), FolderError>;
    /// Whether the file at `path` is on this device to be read now. A file the provider keeps
    /// only in the cloud (an iCloud placeholder) starts downloading and is `false` until it
    /// is here; the next sync reads it.
    fn ensure_available(&self, path: &str) -> Result<bool, FolderError>;
}

/// The parts of `path`, or why it is not a path in the Sync folder.
fn parts(path: &str) -> Result<Vec<&str>, FolderError> {
    let parts: Vec<&str> = path.split('/').collect();
    if path.is_empty()
        || parts
            .iter()
            .any(|p| p.is_empty() || *p == "." || *p == ".." || p.contains('\\'))
    {
        return Err(FolderError::BadPath(path.to_string()));
    }
    Ok(parts)
}

/// A Sync folder on a disk this computer reaches through `std::fs` (the desktop app's).
#[derive(Debug, Clone)]
pub struct FsSyncFolder {
    root: PathBuf,
}

impl FsSyncFolder {
    /// The Sync folder at `root`, the folder the user picked.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn resolve(&self, path: &str) -> Result<PathBuf, FolderError> {
        Ok(parts(path)?
            .into_iter()
            .fold(self.root.clone(), |p, part| p.join(part)))
    }
}

fn io(e: &std::io::Error) -> FolderError {
    FolderError::Io(e.to_string())
}

impl SyncFolder for FsSyncFolder {
    fn list(&self, dir: &str) -> Result<Vec<String>, FolderError> {
        let dir = if dir.is_empty() {
            self.root.clone()
        } else {
            self.resolve(dir)?
        };
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io(&e)),
        };
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| io(&e))?;
            if let Some(name) = entry.file_name().to_str() {
                names.push(name.to_string());
            }
        }
        Ok(names)
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, FolderError> {
        std::fs::read(self.resolve(path)?).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => FolderError::NotFound(path.to_string()),
            _ => io(&e),
        })
    }

    fn write_new(&self, path: &str, bytes: &[u8]) -> Result<(), FolderError> {
        let target = self.resolve(path)?;
        let (Some(dir), Some(name)) = (target.parent(), target.file_name()) else {
            return Err(FolderError::BadPath(path.to_string()));
        };
        std::fs::create_dir_all(dir).map_err(|e| io(&e))?;
        let temp = dir.join(format!(
            ".{}.{}.tmp",
            name.to_string_lossy(),
            obs_store::annotation::new_id()
        ));
        let written = (|| {
            use std::io::Write as _;
            let mut file = std::fs::File::create(&temp)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temp, &target)
        })();
        if let Err(e) = written {
            let _ = std::fs::remove_file(&temp);
            return Err(io(&e));
        }
        Ok(())
    }

    fn remove(&self, path: &str) -> Result<(), FolderError> {
        match std::fs::remove_file(self.resolve(path)?) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io(&e)),
            _ => Ok(()),
        }
    }

    fn ensure_available(&self, path: &str) -> Result<bool, FolderError> {
        if self.resolve(path)?.is_file() {
            Ok(true)
        } else {
            Err(FolderError::NotFound(path.to_string()))
        }
    }
}

/// A Sync folder in memory, for tests: files by path, and the paths that are only in the
/// cloud (placeholders) until [`MemSyncFolder::download`].
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Default)]
pub struct MemSyncFolder {
    files: std::sync::Mutex<BTreeMap<String, Vec<u8>>>,
    in_cloud: std::sync::Mutex<BTreeSet<String>>,
}

#[cfg(any(test, feature = "test-support"))]
impl MemSyncFolder {
    pub fn new() -> Self {
        Self::default()
    }

    fn files(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Vec<u8>>> {
        self.files.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn in_cloud(&self) -> std::sync::MutexGuard<'_, BTreeSet<String>> {
        self.in_cloud.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Put `bytes` at `path` as anything else might: another device's sync client, a half
    /// copied file.
    pub fn put(&self, path: &str, bytes: &[u8]) {
        self.files().insert(path.to_string(), bytes.to_vec());
    }

    /// The file at `path`, if there is one.
    pub fn get(&self, path: &str) -> Option<Vec<u8>> {
        self.files().get(path).cloned()
    }

    /// Every file's path, sorted.
    pub fn paths(&self) -> Vec<String> {
        self.files().keys().cloned().collect()
    }

    /// Make the file at `path` a placeholder: listed, but not here until downloaded.
    pub fn keep_in_cloud(&self, path: &str) {
        self.in_cloud().insert(path.to_string());
    }

    /// The placeholder at `path` has downloaded.
    pub fn download(&self, path: &str) {
        self.in_cloud().remove(path);
    }
}

#[cfg(any(test, feature = "test-support"))]
impl SyncFolder for MemSyncFolder {
    fn list(&self, dir: &str) -> Result<Vec<String>, FolderError> {
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            parts(dir)?;
            format!("{dir}/")
        };
        let names: BTreeSet<String> = self
            .files()
            .keys()
            .filter_map(|p| p.strip_prefix(&prefix))
            .filter_map(|rest| rest.split('/').next())
            .map(str::to_string)
            .collect();
        Ok(names.into_iter().collect())
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, FolderError> {
        parts(path)?;
        if self.in_cloud().contains(path) {
            return Err(FolderError::Io(format!("{path} is not downloaded")));
        }
        self.get(path)
            .ok_or_else(|| FolderError::NotFound(path.to_string()))
    }

    fn write_new(&self, path: &str, bytes: &[u8]) -> Result<(), FolderError> {
        parts(path)?;
        self.put(path, bytes);
        self.in_cloud().remove(path);
        Ok(())
    }

    fn remove(&self, path: &str) -> Result<(), FolderError> {
        parts(path)?;
        self.files().remove(path);
        Ok(())
    }

    fn ensure_available(&self, path: &str) -> Result<bool, FolderError> {
        parts(path)?;
        if !self.files().contains_key(path) {
            return Err(FolderError::NotFound(path.to_string()));
        }
        Ok(!self.in_cloud().contains(path))
    }
}
