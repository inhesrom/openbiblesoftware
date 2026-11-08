//! Search indexes (#58): one rebuildable cache per Resource (ADR 0003, #16 rule 8).
//!
//! The engine tracks each library Resource's index status. A cache is looked up by its key
//! (`resource_id`, content hash, index version), first in the cache directory, then in a
//! `search/` folder beside the Resource's `.obr` (where the Starter library ships its prebuilt
//! caches). A Resource without a usable cache is queued for a background rebuild, one at a
//! time so index builds never take every job worker. Meanwhile searches use the indexes that
//! are ready, and a Resource being rebuilt after an index version change keeps being searched
//! through its old cache (reported as stale) until the new one is Ready, when the old one is
//! swapped out and deleted. A cache that has gone missing, is corrupt or belongs to another
//! key is deleted (from the cache directory) and rebuilt.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use obs_search::{HitOrder, SearchHit};
use serde_json::{Value, json};

use crate::command::{CommandError, CommandSpec, Params, always};
use crate::{JobEvent, JobEventKind, JobId, Session};

pub use obs_search::INDEX_VERSION;

/// Where one Resource's Search index stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexStatus {
    /// Waiting for, or running, a background build; `done` of `total` documents.
    Indexing { done: u64, total: u64 },
    /// Searchable.
    Ready,
    /// The last build failed; retried at the next refresh, and once per session by a search.
    Failed(String),
    /// Nothing can build it here (no cache directory).
    Unavailable,
}

/// Per-Resource Search index status and the background rebuild queue.
#[derive(Debug)]
pub struct SearchIndexes {
    cache_dir: Option<PathBuf>,
    version: u32,
    status: BTreeMap<String, IndexStatus>,
    /// The cache file of each Ready index.
    ready: BTreeMap<String, PathBuf>,
    /// An older cache (same Resource and content, another index version) of a Resource being
    /// rebuilt, searched until the rebuild is Ready.
    stale: BTreeMap<String, PathBuf>,
    /// Resources whose Failed build a search has already retried this session.
    retried: BTreeSet<String>,
    queue: VecDeque<String>,
    running: Option<Running>,
}

/// The background build in flight.
#[derive(Debug, Clone)]
struct Running {
    job: JobId,
    id: String,
    /// The Resource was forgotten (removed) while this build ran: its outcome is ignored.
    cancelled: bool,
}

impl Default for SearchIndexes {
    fn default() -> Self {
        Self {
            cache_dir: None,
            version: INDEX_VERSION,
            status: BTreeMap::new(),
            ready: BTreeMap::new(),
            stale: BTreeMap::new(),
            retried: BTreeSet::new(),
            queue: VecDeque::new(),
            running: None,
        }
    }
}

impl SearchIndexes {
    pub fn status(&self, resource_id: &str) -> Option<&IndexStatus> {
        self.status.get(resource_id)
    }

    /// Every tracked Resource's status, by resource id.
    pub fn statuses(&self) -> impl Iterator<Item = (&str, &IndexStatus)> {
        self.status.iter().map(|(id, s)| (id.as_str(), s))
    }

    pub fn is_ready(&self, resource_id: &str) -> bool {
        self.status.get(resource_id) == Some(&IndexStatus::Ready)
    }

    /// The cache file of a Ready index.
    pub fn path(&self, resource_id: &str) -> Option<&Path> {
        self.ready.get(resource_id).map(PathBuf::as_path)
    }

    /// The older cache searched while a Resource's index is rebuilt (see [`IndexStatus::Indexing`]).
    pub fn stale_path(&self, resource_id: &str) -> Option<&Path> {
        self.stale.get(resource_id).map(PathBuf::as_path)
    }

    pub fn cache_dir(&self) -> Option<&Path> {
        self.cache_dir.as_deref()
    }

    /// The index version caches must have ([`INDEX_VERSION`] unless changed for a test).
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Whether a background build is queued or running.
    pub fn is_building(&self) -> bool {
        self.running.is_some() || !self.queue.is_empty()
    }

    fn set_indexing(&mut self, id: &str) {
        self.ready.remove(id);
        self.status
            .insert(id.to_string(), IndexStatus::Indexing { done: 0, total: 0 });
        let running = self
            .running
            .as_ref()
            .is_some_and(|r| r.id == id && !r.cancelled);
        if !running && !self.queue.iter().any(|q| q == id) {
            self.queue.push_back(id.to_string());
        }
    }

    /// Whether `path` is a file of the cache directory (ours to delete; prebuilt caches beside
    /// the library are not).
    fn in_cache_dir(&self, path: &Path) -> bool {
        self.cache_dir
            .as_deref()
            .is_some_and(|d| path.parent() == Some(d))
    }

    /// Delete `path` if it is in the cache directory (best effort).
    fn discard(&self, path: &Path) {
        if self.in_cache_dir(path) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// The folders a Resource's cache may be in: the cache directory, then `search/` beside its
/// `.obr` (prebuilt).
#[cfg(not(target_arch = "wasm32"))]
fn cache_folders(cache_dir: Option<&Path>, obr: &Path) -> Vec<PathBuf> {
    let beside = obr.parent().map(|d| d.join("search"));
    cache_dir
        .map(Path::to_path_buf)
        .into_iter()
        .chain(beside)
        .collect()
}

/// A usable cache with exactly `key`, if there is one. A file with the right name that does
/// not open as that key (truncated, foreign, mismatched) is deleted if it is in the cache
/// directory, and skipped otherwise.
#[cfg(not(target_arch = "wasm32"))]
fn find_cache(indexes: &SearchIndexes, obr: &Path, key: &obs_store::IndexKey) -> Option<PathBuf> {
    let name = key.file_name();
    for dir in cache_folders(indexes.cache_dir(), obr) {
        let path = dir.join(&name);
        if !path.is_file() {
            continue;
        }
        if obs_store::SqliteSearchIndex::open(&path, Some(key)).is_ok() {
            return Some(path);
        }
        indexes.discard(&path);
    }
    None
}

/// An older cache of the same Resource and content at another index version, to search
/// while the current one is rebuilt: the newest such version, the cache directory first.
/// Caches of other content are never used: their offsets point into the old text.
#[cfg(not(target_arch = "wasm32"))]
fn find_stale(cache_dir: Option<&Path>, obr: &Path, key: &obs_store::IndexKey) -> Option<PathBuf> {
    let mut best: Option<(u32, PathBuf)> = None;
    for dir in cache_folders(cache_dir, obr) {
        for path in obs_store::search_index_files(&dir, &key.resource_id) {
            let Ok(index) = obs_store::SqliteSearchIndex::open(&path, None) else {
                continue;
            };
            let k = index.key();
            if k.resource_id == key.resource_id
                && k.content_hash == key.content_hash
                && k.version != key.version
                && best.as_ref().is_none_or(|(v, _)| k.version > *v)
            {
                best = Some((k.version, path));
            }
        }
    }
    best.map(|(_, p)| p)
}

/// Whether an open cache is an older index of `key`'s Resource and content.
#[cfg(not(target_arch = "wasm32"))]
fn is_stale_of(index: &obs_store::SqliteSearchIndex, key: &obs_store::IndexKey) -> bool {
    let k = index.key();
    k.resource_id == key.resource_id && k.content_hash == key.content_hash
}

/// Delete the prebuilt caches in `search/` beside a Resource's `.obr` (Remove from library
/// deletes the `.obr` itself).
pub(crate) fn remove_prebuilt_search_indexes(obr: &Path, resource_id: &str) {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(dir) = obr.parent() {
        obs_store::remove_search_indexes(&dir.join("search"), resource_id, None);
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (obr, resource_id);
}

/// The Import indexer (#47's seam): builds the new Resource's cache in the cache directory
/// during the Import's Index phase, with real progress, and stops when the Import is
/// cancelled. Older caches of the Resource stay until the Import is in the library
/// ([`Session::remove_older_search_indexes`]), so a failed update still searches.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone)]
pub struct CacheIndexer {
    pub dir: PathBuf,
    pub version: u32,
}

#[cfg(not(target_arch = "wasm32"))]
impl crate::ImportIndexer for CacheIndexer {
    fn build(
        &self,
        resource: &obs_model::Resource,
        _: &Path,
        ctx: &crate::JobContext,
    ) -> Result<crate::ImportIndexStatus, String> {
        obs_store::write_search_index(resource, self.version, &self.dir, &mut |done, total| {
            ctx.progress(done, total);
            !ctx.is_cancelled()
        })
        .map(|_| crate::ImportIndexStatus::Built)
        .map_err(|e| e.to_string())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn build_cache(
    resource: &obs_model::Resource,
    version: u32,
    dir: &Path,
    ctx: &crate::JobContext,
) -> Result<PathBuf, String> {
    obs_store::build_search_index(resource, version, dir, &mut |done, total| {
        ctx.progress(done, total);
        !ctx.is_cancelled()
    })
    .map_err(|e| e.to_string())
}

/// One Resource's hits.
#[derive(Debug, Clone, PartialEq)]
pub struct ResourceHits {
    pub resource_id: String,
    pub hits: Vec<SearchHit>,
}

/// What a search found, and which of the requested Resources it could not search yet.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SearchResults {
    /// In the order the Resources were requested.
    pub resources: Vec<ResourceHits>,
    /// Requested Resources whose index is being built and that have no older index to search
    /// meanwhile ("Indexing *Resource*…").
    pub indexing: Vec<String>,
    /// Requested Resources searched through their older (stale) index while theirs is
    /// rebuilt; their hits are in [`Self::resources`].
    pub stale: Vec<String>,
    /// Requested Resources whose index could not be built, with the reason.
    pub failed: Vec<(String, String)>,
}

impl SearchResults {
    pub fn total(&self) -> usize {
        self.resources.iter().map(|r| r.hits.len()).sum()
    }
}

/// A search over some Resources of the library.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchRequest {
    /// The query, in the search grammar (#16).
    pub query: String,
    /// Resource ids, in result order; empty for every Resource in the library.
    pub resources: Vec<String>,
    pub order: HitOrder,
    /// At most this many hits per Resource.
    pub limit: Option<usize>,
}

impl Session {
    /// Use `dir` as the Search index cache directory: Imports build their cache there
    /// ([`CacheIndexer`]) and background rebuilds write there. The app calls this at startup
    /// with [`crate::default_search_cache_dir`].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_search_cache_dir(&mut self, dir: PathBuf) {
        self.imports.set_indexer(std::sync::Arc::new(CacheIndexer {
            dir: dir.clone(),
            version: self.search.version,
        }));
        self.search.cache_dir = Some(dir);
    }

    /// Require caches of another index version (normally [`INDEX_VERSION`]); every cache of
    /// the old version becomes stale. Call [`Self::refresh_search_indexes`] after.
    pub fn set_search_index_version(&mut self, version: u32) {
        self.search.version = version;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(dir) = self.search.cache_dir.clone() {
            self.set_search_cache_dir(dir);
        }
    }

    /// Check every library Resource's cache: Ready if a usable cache with its current key
    /// exists, otherwise queue a background rebuild (searching its older cache meanwhile, if it
    /// has one). Indexes already Ready keep serving while others build. Resources no longer in
    /// the library are forgotten.
    pub fn refresh_search_indexes(&mut self) {
        let ids: Vec<String> = self
            .library
            .entries()
            .iter()
            .map(|e| e.manifest.resource_id.clone())
            .collect();
        let gone: Vec<String> = self
            .search
            .status
            .keys()
            .filter(|id| !ids.contains(id))
            .cloned()
            .collect();
        for id in gone {
            self.forget_search_index(&id);
        }
        for id in ids {
            self.refresh_search_index(&id);
        }
    }

    /// [`Self::refresh_search_indexes`] for one Resource.
    pub fn refresh_search_index(&mut self, resource_id: &str) {
        if matches!(
            self.search.status.get(resource_id),
            Some(IndexStatus::Indexing { .. })
        ) {
            return;
        }
        let Some(entry) = self.library.get(resource_id) else {
            return;
        };
        #[cfg(not(target_arch = "wasm32"))]
        let stale = {
            let key = obs_store::IndexKey::new(&entry.manifest, self.search.version);
            if let Some(path) = find_cache(&self.search, &entry.path, &key) {
                self.search
                    .status
                    .insert(resource_id.to_string(), IndexStatus::Ready);
                self.search.ready.insert(resource_id.to_string(), path);
                if let Some(old) = self.search.stale.remove(resource_id) {
                    self.search.discard(&old);
                }
                self.read_outline_ahead(resource_id);
                return;
            }
            find_stale(self.search.cache_dir.as_deref(), &entry.path, &key)
        };
        let _ = entry;
        if self.search.cache_dir.is_none() {
            self.search.ready.remove(resource_id);
            self.search.stale.remove(resource_id);
            self.search
                .status
                .insert(resource_id.to_string(), IndexStatus::Unavailable);
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        match stale {
            Some(old) => {
                self.search.stale.insert(resource_id.to_string(), old);
                self.read_outline_ahead(resource_id);
            }
            None => {
                self.search.stale.remove(resource_id);
            }
        }
        self.search.set_indexing(resource_id);
        self.start_next_index_build();
    }

    /// Delete a Resource's caches in the cache directory other than the one for its revision
    /// in the library: once an Import is in, the caches of the revision before it.
    pub(crate) fn remove_older_search_indexes(&self, resource_id: &str) {
        #[cfg(not(target_arch = "wasm32"))]
        if let (Some(dir), Some(entry)) = (&self.search.cache_dir, self.library.get(resource_id)) {
            let keep = obs_store::IndexKey::new(&entry.manifest, self.search.version).file_name();
            obs_store::remove_search_indexes(dir, resource_id, Some(&keep));
        }
        #[cfg(target_arch = "wasm32")]
        let _ = resource_id;
    }

    /// Stop tracking a Resource's index and delete its caches from the cache directory
    /// (Remove from library).
    pub fn forget_search_index(&mut self, resource_id: &str) {
        self.search.status.remove(resource_id);
        self.search.ready.remove(resource_id);
        self.search.stale.remove(resource_id);
        self.search.retried.remove(resource_id);
        self.search.queue.retain(|q| q != resource_id);
        if let Some(run) = self.search.running.as_mut()
            && run.id == resource_id
            && !run.cancelled
        {
            run.cancelled = true;
            let job = run.job;
            self.jobs().cancel(job);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(dir) = &self.search.cache_dir {
            obs_store::remove_search_indexes(dir, resource_id, None);
        }
    }

    /// Submit the next queued rebuild if none is running.
    fn start_next_index_build(&mut self) {
        if self.search.running.is_some() {
            return;
        }
        while let Some(id) = self.search.queue.pop_front() {
            let Some(entry) = self.library.get(&id) else {
                self.search.status.remove(&id);
                continue;
            };
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(dir) = self.search.cache_dir.clone() {
                let obr = entry.path.clone();
                let version = self.search.version;
                let job = self.jobs().submit_fn("Search index", move |ctx| {
                    use obs_store::ResourceStore;
                    let resource = obs_store::ObrReader::open(&obr)
                        .and_then(|r| r.load())
                        .map_err(|e| format!("{}: {e}", obr.display()))?;
                    build_cache(&resource, version, &dir, ctx)
                });
                self.search.running = Some(Running {
                    job,
                    id,
                    cancelled: false,
                });
                return;
            }
            let _ = entry;
            self.search
                .status
                .insert(id.clone(), IndexStatus::Unavailable);
        }
    }

    /// Apply Search index build events; return the others.
    pub(crate) fn poll_search_indexes(&mut self, events: Vec<JobEvent>) -> Vec<JobEvent> {
        let mut other = Vec::new();
        for event in events {
            let Some(run) = self.search.running.clone() else {
                other.push(event);
                continue;
            };
            if event.id != run.job {
                other.push(event);
                continue;
            }
            let id = run.id;
            let tracked = self.library.get(&id).is_some() && self.search.status.contains_key(&id);
            // A build cancelled by Remove from library: if the Resource is back, its own
            // build is queued, so this outcome is not its status.
            let current = tracked && !run.cancelled;
            match event.kind {
                JobEventKind::Progress { done, total } if current => {
                    self.search
                        .status
                        .insert(id.clone(), IndexStatus::Indexing { done, total });
                }
                JobEventKind::Finished(out) => {
                    self.search.running = None;
                    if let Ok(path) = out.downcast::<PathBuf>() {
                        if current {
                            self.index_built(&id, *path);
                        } else if !tracked {
                            // Removed from the library while it was being built.
                            let _ = std::fs::remove_file(*path);
                        }
                    }
                }
                JobEventKind::Failed(e) => {
                    self.search.running = None;
                    if current {
                        self.search.status.insert(id, IndexStatus::Failed(e));
                    }
                }
                JobEventKind::Cancelled => {
                    self.search.running = None;
                    if current {
                        self.search
                            .status
                            .insert(id, IndexStatus::Failed("cancelled".into()));
                    }
                }
                _ => {}
            }
            if self.search.running.is_none() {
                self.start_next_index_build();
            }
        }
        other
    }

    /// A background build finished: swap the new cache in for the stale one, which is
    /// deleted. If the Resource changed meanwhile (the cache is for older content), check
    /// again instead.
    fn index_built(&mut self, id: &str, path: PathBuf) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let want = self
                .library
                .get(id)
                .map(|e| obs_store::IndexKey::new(&e.manifest, self.search.version).file_name());
            let got = path.file_name().map(|n| n.to_string_lossy().into_owned());
            if want.is_none() || want != got {
                self.search.status.remove(id);
                self.search.ready.remove(id);
                self.refresh_search_index(id);
                return;
            }
        }
        self.search
            .status
            .insert(id.to_string(), IndexStatus::Ready);
        if let Some(old) = self.search.stale.remove(id)
            && old != path
        {
            self.search.discard(&old);
        }
        self.search.ready.insert(id.to_string(), path);
        self.read_outline_ahead(id);
    }

    /// Search the requested Resources' ready indexes now, on this thread. Resources still
    /// indexing (or whose cache has gone missing, which queues a rebuild) are listed in
    /// [`SearchResults::indexing`] instead. An invalid query is an error to show inline.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn search(
        &mut self,
        request: &SearchRequest,
    ) -> Result<SearchResults, obs_search::QueryError> {
        use obs_search::{SearchIndex, SearchOptions};
        let lexicons: Vec<PathBuf> = self
            .library
            .of_type(obs_model::ResourceType::Lexicon)
            .map(|e| e.path.clone())
            .collect();
        let lookup = move |translit: &str| -> Vec<String> {
            use obs_store::ResourceStore;
            let mut out = Vec::new();
            for path in &lexicons {
                if let Ok(reader) = obs_store::ObrReader::open(path)
                    && let Ok(found) = reader.lemmas_for_transliteration(translit)
                {
                    out.extend(found);
                }
            }
            out
        };
        let compiled = obs_search::to_fts5(&request.query, &lookup)?;
        let ids: Vec<String> = if request.resources.is_empty() {
            self.library
                .entries()
                .iter()
                .map(|e| e.manifest.resource_id.clone())
                .collect()
        } else {
            request.resources.clone()
        };
        let options = SearchOptions {
            documents: Vec::new(),
            order: request.order,
            limit: request.limit,
        };
        let mut results = SearchResults::default();
        for id in ids {
            let index = match self.open_search_index(&id) {
                Ok(Opened::Current(index)) => index,
                Ok(Opened::Stale(index)) => {
                    results.stale.push(id.clone());
                    index
                }
                Err(NotReady::Indexing) => {
                    results.indexing.push(id);
                    continue;
                }
                Err(NotReady::Failed(e)) => {
                    results.failed.push((id, e));
                    continue;
                }
                Err(NotReady::NotInLibrary) => continue,
            };
            match index.search(&compiled, &options) {
                Ok(hits) => results.resources.push(ResourceHits {
                    resource_id: id,
                    hits,
                }),
                Err(e) => results.failed.push((id, e.to_string())),
            }
        }
        Ok(results)
    }

    /// Open a Resource's ready index, or its stale one while it is rebuilt. A cache that has
    /// gone missing, been replaced or become unreadable is deleted (from the cache directory)
    /// and rebuilt (or found again, e.g. prebuilt) instead of failing the search. A Failed
    /// build is retried once per session.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn open_search_index(&mut self, id: &str) -> Result<Opened, NotReady> {
        let Some(entry) = self.library.get(id) else {
            return Err(NotReady::NotInLibrary);
        };
        let key = obs_store::IndexKey::new(&entry.manifest, self.search.version);
        for _ in 0..3 {
            match self.search.status.get(id).cloned() {
                Some(IndexStatus::Ready) => {
                    if let Some(path) = self.search.ready.get(id).cloned() {
                        match obs_store::SqliteSearchIndex::open(&path, Some(&key)) {
                            Ok(index) => return Ok(Opened::Current(index)),
                            Err(_) => self.search.discard(&path),
                        }
                    }
                    // Deleted, replaced or corrupt under us: look again, else rebuild.
                    self.search.status.remove(id);
                    self.search.ready.remove(id);
                    self.refresh_search_index(id);
                }
                Some(IndexStatus::Indexing { .. }) => {
                    if let Some(path) = self.search.stale.get(id).cloned() {
                        if let Ok(index) = obs_store::SqliteSearchIndex::open(&path, None)
                            && is_stale_of(&index, &key)
                        {
                            return Ok(Opened::Stale(index));
                        }
                        self.search.stale.remove(id);
                    }
                    return Err(NotReady::Indexing);
                }
                Some(IndexStatus::Failed(e)) => {
                    if !self.search.retried.insert(id.to_string()) {
                        return Err(NotReady::Failed(e));
                    }
                    self.search.status.remove(id);
                    self.refresh_search_index(id);
                }
                Some(IndexStatus::Unavailable) => {
                    return Err(NotReady::Failed("no Search index cache directory".into()));
                }
                None => self.refresh_search_index(id),
            }
        }
        Err(NotReady::Indexing)
    }
}

/// An index a search can use.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) enum Opened {
    /// The current cache.
    Current(obs_store::SqliteSearchIndex),
    /// An older index version's cache, while the current one is rebuilt.
    Stale(obs_store::SqliteSearchIndex),
}

/// Why a Resource could not be searched now.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) enum NotReady {
    Indexing,
    Failed(String),
    NotInLibrary,
}

/// The platform cache directory for Search indexes: `$OBS_SEARCH_CACHE`, else `search` in the
/// app's cache folder (`$XDG_CACHE_HOME/OpenBible` or `~/.cache/OpenBible` on Linux,
/// `~/Library/Caches/OpenBible` on macOS, `%LOCALAPPDATA%\OpenBible\cache` on Windows).
pub fn default_search_cache_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("OBS_SEARCH_CACHE") {
        return Some(PathBuf::from(d));
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let base = if cfg!(target_os = "windows") {
        std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("OpenBible").join("cache"))
    } else if cfg!(target_os = "macos") {
        home.map(|h| h.join("Library/Caches/OpenBible"))
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| home.map(|h| h.join(".cache")))
            .map(|d| d.join("OpenBible"))
    }?;
    Some(base.join("search"))
}

fn status_json(s: &IndexStatus) -> Value {
    match s {
        IndexStatus::Indexing { done, total } => {
            json!({ "status": "indexing", "done": done, "total": total })
        }
        IndexStatus::Ready => json!({ "status": "ready" }),
        IndexStatus::Failed(e) => json!({ "status": "failed", "error": e }),
        IndexStatus::Unavailable => json!({ "status": "unavailable" }),
    }
}

pub(crate) const COMMANDS: &[CommandSpec] = &[CommandSpec {
    id: "search.indexes",
    label: "Search index status",
    shortcut: None,
    params: &[],
    enabled: always,
    run: indexes,
}];

fn indexes(s: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    let list: Vec<Value> = s
        .library
        .entries()
        .iter()
        .map(|e| {
            let id = &e.manifest.resource_id;
            let mut v = s
                .search
                .status(id)
                .map_or_else(|| json!({ "status": "unknown" }), status_json);
            v["resource_id"] = json!(id);
            if let Some(p) = s.search.path(id) {
                v["path"] = json!(p.display().to_string());
            }
            if let Some(p) = s.search.stale_path(id) {
                v["stale"] = json!(p.display().to_string());
            }
            v
        })
        .collect();
    Ok(json!({
        "version": s.search.version(),
        "cache_dir": s.search.cache_dir().map(|d| d.display().to_string()),
        "indexes": list,
    }))
}
