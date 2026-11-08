//! The library: the Resources the user has, listed from their manifests. The order of the list
//! is the Preferred order within each Resource type (#18 rule 5).

use std::path::PathBuf;

use obs_model::{Manifest, ResourceType};

/// The Starter library's default Preferred order where it matters for lookups, before the
/// user ranks anything (decision #15): the KJV with Strong's numbers is the ★ Bible, the
/// tagged English Bible Word Study and Key words read; the Lexicons go TBESH/TBESG, BDB,
/// Abbott-Smith, Strong's, so the ★ Lexicon and a Word Study's Lexicon entries lead with the
/// modern lexicons; TIPNR leads the People & places indexes, so where the geocoding (#150)
/// lists a place under TIPNR's id, the Factbook and the Passage Guide show TIPNR's record;
/// OpenBible.info's Cross-references lead the Cross-reference sets, ahead of the BSB Parallel
/// Passages (#212), which has none. Other Starter Resources keep the library's file order.
pub const STARTER_PREFERRED_ORDER: &[&str] = &[
    "kjv-strongs",
    "tbesh",
    "tbesg",
    "bdb",
    "abbott-smith",
    "strongs-hebrew",
    "strongs-greek",
    "tipnr",
    "openbible-geo",
    "theographic",
    "openbible-xrefs",
];

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
    /// Each id's place in `ranking` (its first), so ranking a library of thousands stays fast
    /// (#227).
    ranks: std::collections::HashMap<String, usize>,
    /// The library directory, once scanned: Remove deletes only `.obr` files inside it.
    dir: Option<PathBuf>,
    /// Last known abbreviations of Resources not in the library (removed in this session, or
    /// remembered in the User database), so their dormant Annotations can say which Resource
    /// is missing ("WEB is not in the library", #17 rule 13).
    departed: std::collections::BTreeMap<String, String>,
    /// The user's Collections and Resource tags (#153), from the User database.
    collections: crate::collections::CollectionSet,
}

impl Library {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn directory(&self) -> Option<&std::path::Path> {
        self.dir.as_deref()
    }

    pub fn set_directory(&mut self, dir: PathBuf) {
        self.dir = Some(dir);
    }

    /// The Collections and Resource tags ([`crate::collections`]).
    pub fn collection_set(&self) -> &crate::collections::CollectionSet {
        &self.collections
    }

    pub fn set_collection_set(&mut self, set: crate::collections::CollectionSet) {
        self.collections = set;
    }

    pub fn entries(&self) -> &[LibraryEntry] {
        &self.entries
    }

    pub fn get(&self, resource_id: &str) -> Option<&LibraryEntry> {
        self.entries
            .iter()
            .find(|e| e.manifest.resource_id == resource_id)
    }

    /// A Resource's abbreviation: from the library, else as it was last known (removed in
    /// this session, or [remembered](Self::remember_abbreviation)), else `None`.
    pub fn abbreviation_of(&self, resource_id: &str) -> Option<&str> {
        self.get(resource_id)
            .map(|e| e.manifest.abbreviation.as_str())
            .or_else(|| self.departed.get(resource_id).map(String::as_str))
    }

    /// Remember the abbreviation of a Resource that may not be in the library (as the User
    /// database last knew it), for [`Self::abbreviation_of`].
    pub fn remember_abbreviation(&mut self, resource_id: &str, abbreviation: &str) {
        self.departed
            .insert(resource_id.to_string(), abbreviation.to_string());
    }

    /// A Resource's place in the Preferred order: its saved rank; else, for a Starter
    /// Resource the user has not ranked, its place in [`STARTER_PREFERRED_ORDER`] after every
    /// saved one; else the bottom.
    fn rank(&self, resource_id: &str) -> usize {
        self.ranks
            .get(resource_id)
            .copied()
            .or_else(|| {
                STARTER_PREFERRED_ORDER
                    .iter()
                    .position(|r| *r == resource_id)
                    .map(|p| self.ranking.len() + p)
            })
            .unwrap_or(usize::MAX)
    }

    /// Add a Resource in its Preferred-order place (the bottom of its type if it has none),
    /// or update its entry if it is already listed.
    pub fn add(&mut self, entry: LibraryEntry) {
        self.departed.remove(&entry.manifest.resource_id);
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
        let mut seen: std::collections::HashSet<String> = ids.iter().cloned().collect();
        for r in &self.ranking {
            if seen.insert(r.clone()) {
                ids.push(r.clone());
            }
        }
        ids
    }

    /// The saved Preferred order becomes `ranking`.
    fn set_ranking(&mut self, ranking: Vec<String>) {
        self.ranks.clear();
        for (i, id) in ranking.iter().enumerate() {
            self.ranks.entry(id.clone()).or_insert(i);
        }
        self.ranking = ranking;
    }

    /// Apply a saved Preferred order: Resources it names move to their rank within their
    /// type; the rest keep their order below them.
    pub fn set_preferred_order(&mut self, ranking: Vec<String>) {
        self.set_ranking(ranking);
        let entries = std::mem::take(&mut self.entries);
        let mut ranked: Vec<(usize, usize, LibraryEntry)> = entries
            .into_iter()
            .enumerate()
            .map(|(i, e)| (self.rank(&e.manifest.resource_id), i, e))
            .collect();
        ranked.sort_by_key(|(rank, i, _)| (*rank, *i));
        // As adding them one by one in that order would: each type's Resources together, in
        // that order, the types in the order their first Resource comes.
        let mut types: Vec<ResourceType> = Vec::new();
        for (_, _, e) in &ranked {
            if !types.contains(&e.manifest.resource_type) {
                types.push(e.manifest.resource_type);
            }
        }
        let mut by_type: Vec<Vec<LibraryEntry>> = types.iter().map(|_| Vec::new()).collect();
        for (_, _, e) in ranked {
            if let Some(t) = types.iter().position(|t| *t == e.manifest.resource_type) {
                by_type[t].push(e);
            }
        }
        self.entries = by_type.into_iter().flatten().collect();
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
        self.set_ranking(self.preferred_order());
        Some(to)
    }

    /// Take a Resource out of the list and out of the Preferred order (a later Import of it
    /// goes to the bottom, like any new Import).
    pub fn remove(&mut self, resource_id: &str) -> Option<LibraryEntry> {
        let i = self
            .entries
            .iter()
            .position(|e| e.manifest.resource_id == resource_id)?;
        let mut ranking = std::mem::take(&mut self.ranking);
        ranking.retain(|r| r != resource_id);
        self.set_ranking(ranking);
        let entry = self.entries.remove(i);
        self.departed
            .insert(resource_id.to_string(), entry.manifest.abbreviation.clone());
        Some(entry)
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

/// The Library Tab's filter: header text, language chips and a Collection, applied together.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LibraryFilter {
    /// Matched case-insensitively against abbreviation, title and resource id.
    pub text: String,
    /// Selected language chips ([`language_name`]s); none selected shows every language.
    pub languages: Vec<String>,
    /// The Collection chosen (#153): only its Resources show. `None`: every Resource.
    pub collection: Option<String>,
}

impl LibraryFilter {
    /// [`Self::matches`], and in the chosen Collection of `library`.
    pub fn matches_in(&self, library: &Library, m: &Manifest) -> bool {
        self.matches(m)
            && self
                .collection
                .as_deref()
                .is_none_or(|c| library.in_collection(c, &m.resource_id))
    }

    /// The header text and language chips (the Collection needs the library:
    /// [`Self::matches_in`]).
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
pub(crate) fn data_dir() -> Option<PathBuf> {
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
