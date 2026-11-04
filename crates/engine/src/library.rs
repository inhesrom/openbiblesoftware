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
    /// The saved Preferred order: resource ids, best first. Resources not in it (new imports)
    /// go to the bottom of their type. It may name Resources that are not here right now.
    ranking: Vec<String>,
    /// The library directory, once scanned: Remove deletes only `.obr` files inside it.
    dir: Option<PathBuf>,
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

    fn rank(&self, resource_id: &str) -> usize {
        self.ranking
            .iter()
            .position(|r| r == resource_id)
            .unwrap_or(usize::MAX)
    }

    /// Add a Resource in its Preferred-order place (the bottom of its type if it has none),
    /// or update its entry if it is already listed.
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
        let rank = self.rank(&entry.manifest.resource_id);
        let at = self
            .entries
            .iter()
            .position(|e| {
                e.manifest.resource_type == ty && self.rank(&e.manifest.resource_id) > rank
            })
            .or_else(|| {
                self.entries
                    .iter()
                    .rposition(|e| e.manifest.resource_type == ty)
                    .map(|i| i + 1)
            })
            .unwrap_or(self.entries.len());
        self.entries.insert(at, entry);
    }

    /// Resources of one type, in Preferred order.
    pub fn of_type(&self, ty: ResourceType) -> impl Iterator<Item = &LibraryEntry> {
        self.entries
            .iter()
            .filter(move |e| e.manifest.resource_type == ty)
    }

    /// The first Resource of a type in Preferred order (★): the one used for lookups.
    pub fn preferred(&self, ty: ResourceType) -> Option<&LibraryEntry> {
        self.of_type(ty).next()
    }

    /// Where a Resource is within its type (0 is ★).
    pub fn position_in_type(&self, resource_id: &str) -> Option<usize> {
        let ty = self.get(resource_id)?.manifest.resource_type;
        self.of_type(ty)
            .position(|e| e.manifest.resource_id == resource_id)
    }

    /// The Preferred order to save: every listed Resource in order, then the saved ranking of
    /// Resources that are not here right now.
    pub fn preferred_order(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .entries
            .iter()
            .map(|e| e.manifest.resource_id.clone())
            .collect();
        for r in &self.ranking {
            if !ids.contains(r) {
                ids.push(r.clone());
            }
        }
        ids
    }

    /// Apply a saved Preferred order: Resources it names move to their rank within their
    /// type; the rest keep their order below them.
    pub fn set_preferred_order(&mut self, ranking: Vec<String>) {
        self.ranking = ranking;
        let entries = std::mem::take(&mut self.entries);
        let mut ranked: Vec<(usize, usize, LibraryEntry)> = entries
            .into_iter()
            .enumerate()
            .map(|(i, e)| (self.rank(&e.manifest.resource_id), i, e))
            .collect();
        ranked.sort_by_key(|(rank, i, _)| (*rank, *i));
        for (_, _, e) in ranked {
            self.add(e);
        }
    }

    /// Move a Resource to position `to` within its type (clamped to the bottom). Returns its
    /// new position, or `None` if it is not in the library.
    pub fn move_to(&mut self, resource_id: &str, to: usize) -> Option<usize> {
        let from = self
            .entries
            .iter()
            .position(|e| e.manifest.resource_id == resource_id)?;
        let entry = self.entries.remove(from);
        let ty = entry.manifest.resource_type;
        let same: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.manifest.resource_type == ty)
            .map(|(i, _)| i)
            .collect();
        let to = to.min(same.len());
        let at = match same.get(to) {
            Some(i) => *i,
            None => same.last().map_or(from.min(self.entries.len()), |i| i + 1),
        };
        self.entries.insert(at, entry);
        self.ranking = self.preferred_order();
        Some(to)
    }

    /// Take a Resource out of the list and out of the Preferred order (a later Import of it
    /// goes to the bottom, like any new Import).
    pub fn remove(&mut self, resource_id: &str) -> Option<LibraryEntry> {
        let i = self
            .entries
            .iter()
            .position(|e| e.manifest.resource_id == resource_id)?;
        self.ranking.retain(|r| r != resource_id);
        Some(self.entries.remove(i))
    }

    /// Whether `path` is a file directly inside the library directory (Remove deletes those;
    /// a file opened from elsewhere is only taken off the list). Resolve directory aliases,
    /// not the file itself: Remove also handles files already deleted outside the app.
    pub fn holds_file(&self, path: &std::path::Path) -> bool {
        self.dir
            .as_deref()
            .zip(path.parent())
            .is_some_and(
                |(dir, parent)| match (dir.canonicalize(), parent.canonicalize()) {
                    (Ok(dir), Ok(parent)) => dir == parent,
                    _ => false,
                },
            )
    }

    /// The language chips: the [`language_name`]s of the library's Resources, alphabetically.
    /// Tags with the same name (`he`, `hbo`) share a chip.
    pub fn languages(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self
            .entries
            .iter()
            .map(|e| language_name(&e.manifest.language))
            .collect();
        out.sort_unstable();
        out.dedup();
        out
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

    /// Add every `.obr` file in `dir` (sorted by file name), skipping unreadable ones, and
    /// remember `dir` as the library directory. Returns the files that could not be read,
    /// with the reason.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn scan(&mut self, dir: &std::path::Path) -> Vec<(PathBuf, String)> {
        self.dir = Some(dir.to_path_buf());
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

/// The Library Tab's filter: header text and language chips, applied together.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LibraryFilter {
    /// Matched case-insensitively against abbreviation, title and resource id.
    pub text: String,
    /// Selected language chips ([`language_name`]s); none selected shows every language.
    pub languages: Vec<String>,
}

impl LibraryFilter {
    pub fn matches(&self, m: &Manifest) -> bool {
        let lang = self.languages.is_empty()
            || self
                .languages
                .iter()
                .any(|l| l == language_name(&m.language));
        let needle = self.text.trim().to_lowercase();
        let text = needle.is_empty()
            || [&m.abbreviation, &m.title, &m.resource_id]
                .iter()
                .any(|f| f.to_lowercase().contains(&needle));
        lang && text
    }

    /// Turn a language chip on or off.
    pub fn toggle_language(&mut self, name: &str) {
        if let Some(i) = self.languages.iter().position(|l| l == name) {
            self.languages.remove(i);
        } else {
            self.languages.push(name.to_string());
        }
    }
}

/// A language chip's label: the language's English name for tags the app knows, else the tag.
pub fn language_name(tag: &str) -> &str {
    match tag.split('-').next().unwrap_or(tag) {
        "en" => "English",
        "hbo" | "he" => "Hebrew",
        "grc" | "el" => "Greek",
        "arc" => "Aramaic",
        "la" => "Latin",
        "de" => "German",
        "es" => "Spanish",
        "fr" => "French",
        _ => tag,
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
