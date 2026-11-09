//! The Starter library in the user's Library (#131).
//!
//! - **Bundle.** [`find_starter_bundle`] looks, in order, at `$OBS_STARTER_DIR`, `starter/`
//!   beside the executable, the macOS bundle's `Resources/starter/`, and, in debug builds,
//!   `dist/starter/` in the workspace (so `cargo run -p openbible` works after
//!   `cargo xtask starter`). A bundle is what `cargo xtask starter` writes: the
//!   [`MANIFEST_FILE`], one `.obr` per Starter Resource, the prebuilt Search index caches in
//!   `search/`, the alias caches in `aliases/` and the change records.
//! - **First run.** When the Library has never been set up ([`StarterState`] is not in the
//!   User database), [`Session::sync_starter_library`] adds every Starter Resource of the
//!   bundle, with its caches and change record, through the same Library entry path as Import
//!   ([`Session::added_to_library`]), so [`crate::STARTER_PREFERRED_ORDER`] applies.
//! - **Removals.** Removing a Starter Resource records it in the User database; it is never
//!   added back silently. The Starter library Tab lists it as Removed, with Add back.
//! - **Revisions.** A bundled or released `.obr` with a higher revision than the one in the
//!   Library replaces it under the same `resource_id`, so the entry keeps its Preferred order
//!   and its Annotations re-anchor to the new text (#65).
//! - **Download.** With no bundle, the Tab offers the Starter library from the release assets
//!   (`<base><id>.obr`, [`default_download_base`]). Each file's sha256 is checked against the
//!   released manifest; a mismatch, a failure or a cancel removes every file the download
//!   wrote, so nothing is ever half-added. Network access stays in the app: the bytes come
//!   through the app's `Fetch` ([`Session::set_fetch`]).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use obs_model::ResourceType;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params};
use crate::library::LibraryEntry;
use crate::session::Session;
use crate::{JobEvent, JobEventKind, JobId};

/// The manifest `cargo xtask starter` writes beside the `.obr` files and publishes with each
/// release.
pub const MANIFEST_FILE: &str = "starter-manifest.json";

/// Release manifest format this app reads.
pub const MANIFEST_FORMAT: u32 = 1;

/// Where each release publishes the Starter library (the `release` workflow uploads every
/// `.obr`, `starter-manifest.json` and the change records).
pub const RELEASE_DOWNLOADS: &str = crate::credits::PUBLISHED_CHANGES;

/// User database preference holding the [`StarterState`].
pub(crate) const STATE_PREF: &str = "starter-library";

/// One Starter Resource as a release manifest lists it. Fields the app does not need (unit
/// ids) are ignored; the descriptive ones are absent from manifests written before #131.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct StarterItem {
    pub resource_id: String,
    pub revision: u32,
    pub content_hash: String,
    /// sha256 of the `.obr` file, lowercase hex.
    pub file_sha256: String,
    /// Size of the `.obr` file.
    pub bytes: u64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub abbreviation: String,
    #[serde(default)]
    pub resource_type: Option<ResourceType>,
    #[serde(default)]
    pub language: String,
    /// SPDX licence expression.
    #[serde(default)]
    pub license: String,
}

/// `starter-manifest.json`: every Resource one Starter library build shipped.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct StarterManifest {
    pub format: u32,
    pub resources: Vec<StarterItem>,
}

impl StarterManifest {
    pub fn parse(json: &str) -> Result<Self, String> {
        let m: Self = serde_json::from_str(json)
            .map_err(|e| format!("not a Starter library manifest: {e}"))?;
        if m.format != MANIFEST_FORMAT {
            return Err(format!(
                "Starter library manifest format {} (this app reads {MANIFEST_FORMAT})",
                m.format
            ));
        }
        check_resource_ids(&m.resources)?;
        Ok(m)
    }

    pub fn get(&self, resource_id: &str) -> Option<&StarterItem> {
        self.resources.iter().find(|r| r.resource_id == resource_id)
    }
}

/// What the User database remembers about the Starter library. Its absence means the
/// Library has never been set up.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StarterState {
    /// Starter Resources that have been in this Library. A known one that is missing again
    /// (its file deleted outside the app) is offered, not added back.
    #[serde(default)]
    pub known: BTreeSet<String>,
    /// Starter Resources the user removed: listed as Removed and never added back silently.
    #[serde(default)]
    pub removed: BTreeSet<String>,
    /// The Starter library Tab was opened once on its own to offer the download.
    #[serde(default)]
    pub offered: bool,
}

/// Where one Starter Resource stands in the Library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StarterStatus {
    InLibrary,
    /// The Library has an older revision.
    Outdated {
        installed: u32,
        offered: u32,
    },
    /// The user removed it.
    Removed,
    /// Not in the Library (never added, or its file is gone).
    Missing,
    /// Being downloaded.
    Adding,
}

/// One row of the Starter library Tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StarterRow {
    pub resource_id: String,
    pub title: String,
    pub abbreviation: String,
    pub resource_type: Option<ResourceType>,
    pub language: String,
    /// SPDX licence expression ("" when the manifest does not say).
    pub license: String,
    pub bytes: u64,
    /// The features this Resource powers ("Word Study: Lexicon entries").
    pub features: Vec<&'static str>,
    pub status: StarterStatus,
}

/// A running download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StarterDownload {
    pub job: JobId,
    pub ids: Vec<String>,
    pub done: u64,
    pub total: u64,
}

/// What [`Session::sync_starter_library`] did at launch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StarterSync {
    /// Starter Resources added to the Library.
    pub added: Vec<String>,
    /// Starter Resources replaced by a newer revision.
    pub updated: Vec<String>,
    /// No bundle and nothing set up yet: open the Starter library Tab to offer the download.
    pub offer_download: bool,
    /// What went wrong, for a message.
    pub problems: Vec<String>,
}

/// The Session's Starter library: the bundle (if any), the released manifest, and the
/// download in flight.
#[derive(Debug)]
pub struct StarterLibrary {
    bundle: Option<PathBuf>,
    bundled: Option<StarterManifest>,
    download_base: String,
    released: Option<StarterManifest>,
    checking: Option<JobId>,
    download: Option<StarterDownload>,
    error: Option<String>,
    /// What the last attempt was (the manifest, or Add with the ids asked for), for Retry.
    attempt: Option<StarterAttempt>,
    /// As loaded from (and saved to) the User database; `None`: never set up.
    state: Option<StarterState>,
    state_loaded: bool,
}

/// What Retry does again after a failure or a cancel.
#[derive(Debug, Clone, PartialEq, Eq)]
enum StarterAttempt {
    /// Fetch the released manifest.
    Check,
    /// Add these Starter Resources (`None`: every one [`Session::starter_wanted`] lists).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    Add(Option<Vec<String>>),
}

impl Default for StarterLibrary {
    fn default() -> Self {
        Self {
            bundle: None,
            bundled: None,
            download_base: RELEASE_DOWNLOADS.to_string(),
            released: None,
            checking: None,
            download: None,
            error: None,
            attempt: None,
            state: None,
            state_loaded: false,
        }
    }
}

impl StarterLibrary {
    /// Use the bundle in `dir`: read its manifest.
    pub fn set_bundle(&mut self, dir: &Path) -> Result<(), String> {
        let path = dir.join(MANIFEST_FILE);
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let manifest =
            StarterManifest::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        self.bundle = Some(dir.to_path_buf());
        self.bundled = Some(manifest);
        Ok(())
    }

    /// The bundled Starter library folder, when there is one.
    pub fn bundle(&self) -> Option<&Path> {
        self.bundle.as_deref()
    }

    /// Where downloads come from: a base URL that `<id>.obr` and [`MANIFEST_FILE`] are
    /// appended to.
    pub fn set_download_base(&mut self, base: &str) {
        let mut base = base.to_string();
        if !base.ends_with('/') {
            base.push('/');
        }
        self.download_base = base;
    }

    pub fn download_base(&self) -> &str {
        &self.download_base
    }

    /// The manifest the rows come from: the bundle's, else the released one once fetched.
    pub fn manifest(&self) -> Option<&StarterManifest> {
        self.bundled.as_ref().or(self.released.as_ref())
    }

    /// Whether the released manifest is being fetched.
    pub fn is_checking(&self) -> bool {
        self.checking.is_some()
    }

    pub fn download(&self) -> Option<&StarterDownload> {
        self.download.as_ref()
    }

    /// The last failure (a download, a verification, the released manifest), until the next
    /// attempt.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The remembered state; `None` when the Library has never been set up.
    pub fn state(&self) -> Option<&StarterState> {
        self.state.as_ref()
    }

    /// Whether a job event belongs to the Starter library.
    fn owns(&self, job: JobId) -> bool {
        self.checking == Some(job) || self.download.as_ref().is_some_and(|d| d.job == job)
    }
}

/// What a Starter Resource powers, for the Starter library Tab.
pub fn features(
    resource_type: Option<ResourceType>,
    resource_id: &str,
    language: &str,
) -> Vec<&'static str> {
    let original = matches!(
        language.split('-').next(),
        Some("hbo" | "he" | "grc" | "el" | "arc")
    );
    match resource_type {
        Some(ResourceType::Bible) if original => vec![
            "Reading the Hebrew or Greek text",
            "Word Info: Lemma, Morphology and Gloss",
            "Word Study: Forms",
        ],
        // The tagged English Bible Word Study and Key words read (decision #15).
        Some(ResourceType::Bible)
            if crate::STARTER_PREFERRED_ORDER.first() == Some(&resource_id) =>
        {
            vec![
                "Reading",
                "Word Study: Translated as and Occurrences",
                "Passage Guide: Key words",
            ]
        }
        Some(ResourceType::Bible) => vec!["Reading", "Text Comparison", "Search"],
        Some(ResourceType::Commentary) => vec!["Passage Guide: Commentaries"],
        Some(ResourceType::Lexicon) => vec!["Word Study: Lexicon entries", "Word Info: Gloss"],
        Some(ResourceType::Dictionary) => {
            vec![
                "Dictionary articles",
                "Passage Guide: People & places articles",
            ]
        }
        Some(ResourceType::CrossReferenceSet) => {
            vec!["Passage Guide: Cross-references", "Cross-references Tool"]
        }
        Some(ResourceType::PeopleAndPlaces) => vec!["Passage Guide: People & places"],
        None => Vec::new(),
    }
}

/// The folders a Starter bundle may be in, in the order they are tried: `env` (the value of
/// `$OBS_STARTER_DIR`) alone when set; else `starter/` beside the executable, the macOS
/// bundle's `Resources/starter/` (the executable is in `Contents/MacOS/`), and `workspace`
/// (`dist/starter/` for debug builds).
pub fn bundle_candidates(
    env: Option<PathBuf>,
    exe: Option<&Path>,
    workspace: Option<PathBuf>,
) -> Vec<PathBuf> {
    if let Some(dir) = env {
        return vec![dir];
    }
    let mut out = Vec::new();
    if let Some(dir) = exe.and_then(Path::parent) {
        out.push(dir.join("starter"));
        out.push(dir.join("../Resources/starter"));
    }
    out.extend(workspace);
    out
}

/// The bundled Starter library: the first of [`bundle_candidates`] that holds a
/// [`MANIFEST_FILE`]. `$OBS_STARTER_DIR` set to a folder without one means no bundle.
pub fn find_starter_bundle() -> Option<PathBuf> {
    let workspace = cfg!(debug_assertions)
        .then(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist/starter"));
    let exe = std::env::current_exe().ok();
    bundle_candidates(
        std::env::var_os("OBS_STARTER_DIR").map(PathBuf::from),
        exe.as_deref(),
        workspace,
    )
    .into_iter()
    .find(|d| d.join(MANIFEST_FILE).is_file())
}

/// Where Starter downloads come from: `$OBS_STARTER_URL` (a mirror, or a `file://` folder
/// for testing), else the latest release's assets.
pub fn default_download_base() -> String {
    std::env::var("OBS_STARTER_URL").unwrap_or_else(|_| RELEASE_DOWNLOADS.to_string())
}

/// Every id is a valid Resource id (it names a file in the library folder, so `../x` or `/x`
/// must never get that far) and none is listed twice.
fn check_resource_ids(items: &[StarterItem]) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for item in items {
        let id = &item.resource_id;
        if !obs_model::is_valid_resource_id(id) {
            return Err(format!(
                "the Starter library manifest lists {id:?}, which is not a Resource id"
            ));
        }
        if !seen.insert(id.as_str()) {
            return Err(format!(
                "the Starter library manifest lists {id} more than once"
            ));
        }
    }
    Ok(())
}

/// Lowercase hex sha256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The staging name of a Starter `.obr` while it is verified (never scanned: not `.obr`).
#[cfg(not(target_arch = "wasm32"))]
fn part_file(lib: &Path, id: &str) -> PathBuf {
    lib.join(format!("{id}.obr.part"))
}

/// Where an earlier revision waits while its replacement is put in place (never scanned).
#[cfg(not(target_arch = "wasm32"))]
fn replaced_file(lib: &Path, id: &str) -> PathBuf {
    lib.join(format!("{id}.obr.replaced"))
}

/// Verify each item's bytes (from `read`, in order) against its sha256, stage them in `lib`,
/// and only when every one is verified, put them all in place as `<id>.obr`. On any
/// failure every staged file is removed and every file already put in place is taken back
/// (an earlier revision it replaced is restored), so nothing is half-added. Returns the new
/// entries.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn install(
    lib: &Path,
    items: &[StarterItem],
    read: &mut dyn FnMut(&StarterItem) -> Result<Vec<u8>, String>,
) -> Result<Vec<LibraryEntry>, String> {
    use obs_store::ResourceStore;
    // Ids become file names: checked before anything touches the disk.
    check_resource_ids(items)?;
    std::fs::create_dir_all(lib).map_err(|e| format!("{}: {e}", lib.display()))?;
    let mut staged: Vec<PathBuf> = Vec::new();
    let cleanup = |staged: &[PathBuf]| {
        for p in staged {
            let _ = std::fs::remove_file(p);
        }
    };
    for item in items {
        let id = &item.resource_id;
        let bytes = match read(item) {
            Ok(b) => b,
            Err(e) => {
                cleanup(&staged);
                return Err(e);
            }
        };
        let got = sha256_hex(&bytes);
        if !got.eq_ignore_ascii_case(&item.file_sha256) {
            cleanup(&staged);
            let short = |h: &str| h.chars().take(16).collect::<String>();
            return Err(format!(
                "{id}.obr failed verification: its sha256 is {}…, but the Starter library \
                 manifest says {}…. Nothing was added.",
                short(&got),
                short(&item.file_sha256)
            ));
        }
        let part = part_file(lib, id);
        staged.push(part.clone());
        if let Err(e) = std::fs::write(&part, &bytes) {
            cleanup(&staged);
            return Err(format!("{}: {e}", part.display()));
        }
    }
    // Every file is verified: open each before anything is put in place.
    let mut entries = Vec::with_capacity(items.len());
    for (item, part) in items.iter().zip(&staged) {
        let manifest = match obs_store::ObrReader::open(part) {
            Ok(r) => r.manifest().clone(),
            Err(e) => {
                cleanup(&staged);
                return Err(format!("{}.obr: {e}", item.resource_id));
            }
        };
        if manifest.resource_id != item.resource_id {
            cleanup(&staged);
            return Err(format!(
                "{}.obr holds the Resource {}, not {}. Nothing was added.",
                item.resource_id, manifest.resource_id, item.resource_id
            ));
        }
        entries.push(LibraryEntry {
            manifest,
            path: lib.join(format!("{}.obr", item.resource_id)),
        });
    }
    if let Err(e) = commit(lib, items, &staged, &entries) {
        cleanup(&staged);
        return Err(e);
    }
    Ok(entries)
}

/// Put every staged file in place, all or nothing: an earlier revision is moved aside first,
/// and if any rename fails, the ones done are undone and the earlier revisions restored.
#[cfg(not(target_arch = "wasm32"))]
fn commit(
    lib: &Path,
    items: &[StarterItem],
    staged: &[PathBuf],
    entries: &[LibraryEntry],
) -> Result<(), String> {
    // Each one put in place, with where its earlier revision waits.
    let mut done: Vec<(&Path, Option<PathBuf>)> = Vec::new();
    let undo = |done: &[(&Path, Option<PathBuf>)]| {
        for (path, earlier) in done.iter().rev() {
            let _ = std::fs::remove_file(path);
            if let Some(earlier) = earlier {
                let _ = std::fs::rename(earlier, path);
            }
        }
    };
    for ((item, part), entry) in items.iter().zip(staged).zip(entries) {
        let path = entry.path.as_path();
        let earlier = if path.is_file() {
            let aside = replaced_file(lib, &item.resource_id);
            if let Err(e) = std::fs::rename(path, &aside) {
                undo(&done);
                return Err(format!("{}: {e}", path.display()));
            }
            Some(aside)
        } else {
            None
        };
        if let Err(e) = std::fs::rename(part, path) {
            if let Some(earlier) = &earlier {
                let _ = std::fs::rename(earlier, path);
            }
            undo(&done);
            return Err(format!("{}: {e}. Nothing was added.", path.display()));
        }
        done.push((path, earlier));
    }
    for (_, earlier) in done {
        if let Some(earlier) = earlier {
            let _ = std::fs::remove_file(earlier);
        }
    }
    Ok(())
}

/// Copy what the bundle has beside a Starter Resource into the library folder: its prebuilt
/// Search index caches, the alias caches and its change record. Best effort: these are
/// rebuilt (caches) or found online (the record) when missing.
#[cfg(not(target_arch = "wasm32"))]
fn copy_bundle_extras(bundle: &Path, lib: &Path, id: &str) {
    let copy_new = |from: &Path, to_dir: &Path| {
        let Some(name) = from.file_name() else {
            return;
        };
        let to = to_dir.join(name);
        if !to.exists() && std::fs::create_dir_all(to_dir).is_ok() {
            let _ = std::fs::copy(from, &to);
        }
    };
    for cache in obs_store::search_index_files(&bundle.join("search"), id) {
        copy_new(&cache, &lib.join("search"));
    }
    let aliases = crate::aliases::ALIAS_DIR;
    if let Ok(rd) = std::fs::read_dir(bundle.join(aliases)) {
        for e in rd.filter_map(Result::ok) {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with(&format!("{id}.")) || name.contains(&format!("--{id}.")) {
                copy_new(&e.path(), &lib.join(aliases));
            }
        }
    }
    // A newer revision's record replaces the older one.
    let name = obs_model::changes_record_file(id);
    if bundle.join(&name).is_file() {
        let _ = std::fs::copy(bundle.join(&name), lib.join(&name));
    }
}

/// A finished download: the entries now in the library folder.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
struct Installed(Vec<LibraryEntry>);

impl Session {
    /// Read the [`StarterState`] from the User database just attached.
    pub(crate) fn reload_starter_state(&mut self) {
        self.starter.state_loaded = false;
        self.load_starter_state();
    }

    fn load_starter_state(&mut self) {
        if self.starter.state_loaded {
            return;
        }
        self.starter.state_loaded = true;
        self.starter.state = self
            .user
            .as_ref()
            .and_then(|u| u.preference(STATE_PREF).ok().flatten())
            .and_then(|json| serde_json::from_str(&json).ok());
    }

    fn save_starter_state(&mut self) {
        let Some(state) = &self.starter.state else {
            return;
        };
        if let Some(user) = &self.user {
            let result = serde_json::to_string(state)
                .map_err(|e| e.to_string())
                .and_then(|json| user.set_preference(STATE_PREF, &json));
            if let Err(e) = result {
                self.workspace.push_effect(crate::Effect::Message(format!(
                    "Could not save the Starter library state: {e}"
                )));
            }
        }
    }

    fn starter_state_mut(&mut self) -> &mut StarterState {
        self.load_starter_state();
        self.starter.state.get_or_insert_with(StarterState::default)
    }

    /// Where a Starter Resource stands in the Library.
    pub fn starter_status(&self, item: &StarterItem) -> StarterStatus {
        if self.is_starter_downloading(&item.resource_id) {
            return StarterStatus::Adding;
        }
        match self.library.get(&item.resource_id) {
            Some(e) if e.manifest.revision < item.revision => StarterStatus::Outdated {
                installed: e.manifest.revision,
                offered: item.revision,
            },
            Some(_) => StarterStatus::InLibrary,
            None if self
                .starter
                .state
                .as_ref()
                .is_some_and(|s| s.removed.contains(&item.resource_id)) =>
            {
                StarterStatus::Removed
            }
            None => StarterStatus::Missing,
        }
    }

    /// The Starter library Tab's rows, in manifest order. Empty until there is a manifest.
    pub fn starter_rows(&self) -> Vec<StarterRow> {
        let Some(manifest) = self.starter.manifest() else {
            return Vec::new();
        };
        manifest
            .resources
            .iter()
            .map(|item| {
                let installed = self.library.get(&item.resource_id).map(|e| &e.manifest);
                let pick = |own: &str, theirs: Option<&String>| {
                    if own.is_empty() {
                        theirs.cloned().unwrap_or_default()
                    } else {
                        own.to_string()
                    }
                };
                let resource_type = item.resource_type.or(installed.map(|m| m.resource_type));
                let language = pick(&item.language, installed.map(|m| &m.language));
                let mut title = pick(&item.title, installed.map(|m| &m.title));
                if title.is_empty() {
                    title.clone_from(&item.resource_id);
                }
                StarterRow {
                    resource_id: item.resource_id.clone(),
                    abbreviation: pick(&item.abbreviation, installed.map(|m| &m.abbreviation)),
                    license: pick(&item.license, installed.map(|m| &m.license)),
                    bytes: item.bytes,
                    features: features(resource_type, &item.resource_id, &language),
                    status: self.starter_status(item),
                    title,
                    resource_type,
                    language,
                }
            })
            .collect()
    }

    /// What one click adds: every Starter Resource that is missing or outdated, except the
    /// ones the user removed.
    pub fn starter_wanted(&self) -> Vec<StarterItem> {
        let Some(manifest) = self.starter.manifest() else {
            return Vec::new();
        };
        manifest
            .resources
            .iter()
            .filter(|i| {
                matches!(
                    self.starter_status(i),
                    StarterStatus::Missing | StarterStatus::Outdated { .. }
                )
            })
            .cloned()
            .collect()
    }

    /// Size of [`Self::starter_wanted`], for "Download Starter library (≈N MB)".
    pub fn starter_wanted_bytes(&self) -> u64 {
        self.starter_wanted().iter().map(|i| i.bytes).sum()
    }

    /// Add Resources to the Library the way an Import does: in their Preferred-order place
    /// (or in place, for a newer revision of one already listed), then save the order,
    /// remember their names, and bring their Search indexes and Annotations up to date.
    pub(crate) fn added_to_library(&mut self, entries: Vec<LibraryEntry>) {
        let ids: Vec<String> = entries
            .iter()
            .map(|e| e.manifest.resource_id.clone())
            .collect();
        for entry in entries {
            let id = entry.manifest.resource_id.clone();
            let replaced = self.library.get(&id).is_some();
            self.library.add(entry);
            if replaced {
                self.workspace.push_effect(crate::Effect::Replaced(id));
            }
        }
        if let Err(e) = self.save_preferred_order() {
            self.workspace.push_effect(crate::Effect::Message(format!(
                "Could not save the Preferred order: {e}"
            )));
        }
        self.finish_adding(&ids);
    }

    /// After Resources joined (or were replaced in) the Library: remember their names, build
    /// or find their Search indexes, and re-anchor their Annotations (#65).
    pub(crate) fn finish_adding(&mut self, ids: &[String]) {
        if ids.is_empty() {
            return;
        }
        self.remember_resource_names();
        for id in ids {
            self.refresh_search_index(id);
            self.reanchor_resource(id);
        }
    }

    /// Starter Resources joined the Library: they are known and no longer removed.
    fn starter_added(&mut self, entries: Vec<LibraryEntry>) -> Vec<String> {
        let ids: Vec<String> = entries
            .iter()
            .map(|e| e.manifest.resource_id.clone())
            .collect();
        let state = self.starter_state_mut();
        for id in &ids {
            state.known.insert(id.clone());
            state.removed.remove(id);
        }
        self.save_starter_state();
        self.added_to_library(entries);
        ids
    }

    /// The user removed a Resource: if it is a Starter one, remember not to add it back.
    pub(crate) fn starter_removed(&mut self, resource_id: &str) {
        let state = self.starter_state_mut();
        state.removed.insert(resource_id.to_string());
        state.known.insert(resource_id.to_string());
        self.save_starter_state();
    }

    /// At launch, after the library folder is scanned and the User database attached: use
    /// the bundled Starter library in `bundle`, if any. On the first run every Starter
    /// Resource is added; later, Starter Resources new to the bundle are added and newer
    /// revisions replace older ones. Removed ones stay removed.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn sync_starter_library(&mut self, bundle: Option<&Path>) -> StarterSync {
        let mut out = StarterSync::default();
        self.load_starter_state();
        if let Some(dir) = bundle
            && let Err(e) = self.starter.set_bundle(dir)
        {
            out.problems.push(e);
        }
        let Some(manifest) = self.starter.bundled.clone() else {
            let never = self.starter.state.is_none();
            let any_starter = self
                .library
                .entries()
                .iter()
                .any(|e| e.manifest.origin == obs_model::Origin::Starter);
            if never && !any_starter && self.user.is_some() {
                out.offer_download = true;
                self.starter_state_mut().offered = true;
                self.save_starter_state();
            }
            return out;
        };
        let first_run = self.starter.state.is_none();
        let state = self.starter.state.clone().unwrap_or_default();
        let mut add = Vec::new();
        for item in &manifest.resources {
            let id = &item.resource_id;
            match self.library.get(id) {
                Some(e) if e.manifest.revision < item.revision && !state.removed.contains(id) => {
                    add.push(item.clone());
                }
                Some(_) => {}
                None if state.removed.contains(id) => {}
                None if first_run || !state.known.contains(id) => add.push(item.clone()),
                None => {}
            }
        }
        let updated: BTreeSet<String> = add
            .iter()
            .filter(|i| self.library.get(&i.resource_id).is_some())
            .map(|i| i.resource_id.clone())
            .collect();
        // Resources already here (from an earlier install) count as known too.
        let present: Vec<String> = manifest
            .resources
            .iter()
            .filter(|i| self.library.get(&i.resource_id).is_some())
            .map(|i| i.resource_id.clone())
            .collect();
        self.starter_state_mut().known.extend(present);
        if add.is_empty() {
            self.save_starter_state();
            return out;
        }
        match self.install_from_bundle(&add) {
            Ok(ids) => {
                for id in ids {
                    if updated.contains(&id) {
                        out.updated.push(id);
                    } else {
                        out.added.push(id);
                    }
                }
            }
            Err(e) => {
                // Still set up: the failure is reported, and the Tab offers Add.
                self.save_starter_state();
                out.problems.push(format!("Starter library: {e}"));
            }
        }
        out
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn library_dir(&mut self) -> Result<PathBuf, String> {
        let dir = self
            .library
            .directory()
            .map(Path::to_path_buf)
            .or_else(crate::default_library_dir)
            .ok_or("No library directory")?;
        self.library.set_directory(dir.clone());
        Ok(dir)
    }

    /// Copy `items` from the bundle into the library folder (verified, with their caches and
    /// change records) and add them. Returns their ids.
    #[cfg(not(target_arch = "wasm32"))]
    fn install_from_bundle(&mut self, items: &[StarterItem]) -> Result<Vec<String>, String> {
        let bundle = self
            .starter
            .bundle
            .clone()
            .ok_or("No bundled Starter library")?;
        let lib = self.library_dir()?;
        let entries = install(&lib, items, &mut |item| {
            let path = bundle.join(format!("{}.obr", item.resource_id));
            std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))
        })?;
        for item in items {
            copy_bundle_extras(&bundle, &lib, &item.resource_id);
        }
        Ok(self.starter_added(entries))
    }

    /// Add Starter Resources: `ids`, or every one [`Self::starter_wanted`] lists. From the
    /// bundle this happens at once; otherwise a download job starts (its progress is in
    /// [`StarterLibrary::download`]). Returns the ids and the job, if one started.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn add_starter(
        &mut self,
        ids: Option<&[String]>,
    ) -> Result<(Vec<String>, Option<JobId>), String> {
        if self.starter.download.is_some() {
            return Err("The Starter library is already downloading".into());
        }
        self.starter.attempt = Some(StarterAttempt::Add(ids.map(<[String]>::to_vec)));
        let manifest = self
            .starter
            .manifest()
            .cloned()
            .ok_or("The Starter library manifest has not been loaded yet")?;
        let items: Vec<StarterItem> = match ids {
            Some(ids) => ids
                .iter()
                .map(|id| {
                    manifest
                        .get(id)
                        .cloned()
                        .ok_or_else(|| format!("{id} is not part of the Starter library"))
                })
                .collect::<Result<_, _>>()?,
            None => self.starter_wanted(),
        };
        if items.is_empty() {
            return Ok((Vec::new(), None));
        }
        self.starter.error = None;
        if self.starter.bundle.is_some() {
            return match self.install_from_bundle(&items) {
                Ok(ids) => Ok((ids, None)),
                Err(e) => {
                    self.starter.error = Some(e.clone());
                    Err(e)
                }
            };
        }
        let lib = self.library_dir()?;
        let base = self.starter.download_base.clone();
        let fetch = self.fetch();
        let total: u64 = items.iter().map(|i| i.bytes).sum();
        let ids: Vec<String> = items.iter().map(|i| i.resource_id.clone()).collect();
        let job = self.jobs().submit_fn("Starter library", move |ctx| {
            let mut before = 0u64;
            install(&lib, &items, &mut |item| {
                let url = format!("{base}{}.obr", item.resource_id);
                let bytes = fetch
                    .get(&url, &mut |done, _| {
                        ctx.progress(before + done.min(item.bytes), total);
                        !ctx.is_cancelled()
                    })
                    .map_err(|e| match e {
                        obs_formats::sword_repo::FetchError::Cancelled => {
                            "Download cancelled".to_string()
                        }
                        e => format!("{url}: {e}"),
                    })?;
                before += item.bytes;
                Ok(bytes)
            })
            .map(Installed)
        });
        self.starter.download = Some(StarterDownload {
            job,
            ids: ids.clone(),
            done: 0,
            total,
        });
        Ok((ids, Some(job)))
    }

    /// Fetch the released manifest (no bundle). Returns the job, or `None` when one is
    /// running already.
    pub fn check_starter_release(&mut self) -> Option<JobId> {
        if self.starter.checking.is_some() {
            return None;
        }
        self.starter.error = None;
        self.starter.attempt = Some(StarterAttempt::Check);
        let url = format!("{}{MANIFEST_FILE}", self.starter.download_base);
        let fetch = self.fetch();
        let job = self
            .jobs()
            .submit_fn("Starter library manifest", move |ctx| {
                let bytes = fetch
                    .get(&url, &mut |_, _| !ctx.is_cancelled())
                    .map_err(|e| format!("{url}: {e}"))?;
                StarterManifest::parse(&String::from_utf8_lossy(&bytes))
            });
        self.starter.checking = Some(job);
        Some(job)
    }

    /// A Starter library Tab is open (restored with the Last session or a Layout) and has no
    /// manifest yet: fetch the released one. Not while one is being fetched, and not after a
    /// failure, which the Tab shows with Retry. Called by the app each frame, once its
    /// transport is attached.
    pub fn load_starter_manifest_for_open_tab(&mut self) -> Option<JobId> {
        if self.starter.manifest().is_some()
            || self.starter.checking.is_some()
            || self.starter.error.is_some()
            || !self
                .workspace
                .tabs()
                .any(|t| t.kind == crate::TabKind::StarterLibrary)
        {
            return None;
        }
        self.check_starter_release()
    }

    /// Whether `resource_id` is part of the Starter library download in flight.
    pub fn is_starter_downloading(&self, resource_id: &str) -> bool {
        self.starter
            .download
            .as_ref()
            .is_some_and(|d| d.ids.iter().any(|id| id == resource_id))
    }

    /// Retry: do the last failed or cancelled attempt again, with the same Starter Resources
    /// (an Add back stays an Add back). Returns the ids and the job, as [`Self::add_starter`].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn retry_starter(&mut self) -> Result<(Vec<String>, Option<JobId>), String> {
        let attempt = self.starter.attempt.clone().unwrap_or_else(|| {
            if self.starter.manifest().is_none() {
                StarterAttempt::Check
            } else {
                StarterAttempt::Add(None)
            }
        });
        match attempt {
            StarterAttempt::Check => Ok((Vec::new(), self.check_starter_release())),
            StarterAttempt::Add(ids) => self.add_starter(ids.as_deref()),
        }
    }

    /// Apply Starter library job events; return the rest.
    pub(crate) fn poll_starter(&mut self, events: Vec<JobEvent>) -> Vec<JobEvent> {
        let mut other = Vec::new();
        for event in events {
            if !self.starter.owns(event.id) {
                other.push(event);
                continue;
            }
            let checking = self.starter.checking == Some(event.id);
            match event.kind {
                JobEventKind::Progress { done, total } => {
                    if let Some(d) = &mut self.starter.download {
                        d.done = done;
                        d.total = total;
                    }
                }
                JobEventKind::Finished(out) => {
                    if checking {
                        self.starter.checking = None;
                        if let Ok(m) = out.downcast::<StarterManifest>() {
                            self.starter.released = Some(*m);
                        }
                    } else {
                        self.starter.download = None;
                        if let Ok(installed) = out.downcast::<Installed>() {
                            let ids = self.starter_added(installed.0);
                            self.workspace.push_effect(crate::Effect::Message(format!(
                                "Added {} Starter Resource{} to your Library",
                                ids.len(),
                                if ids.len() == 1 { "" } else { "s" }
                            )));
                        }
                    }
                }
                JobEventKind::Failed(e) => {
                    if checking {
                        self.starter.checking = None;
                        self.starter.error =
                            Some(format!("Could not load the Starter library list: {e}"));
                    } else {
                        self.starter.download = None;
                        self.workspace
                            .push_effect(crate::Effect::Message(format!("Starter library: {e}")));
                        self.starter.error = Some(e);
                    }
                }
                JobEventKind::Cancelled => {
                    if checking {
                        self.starter.checking = None;
                    } else {
                        self.starter.download = None;
                        self.starter.error = Some("Download cancelled".into());
                    }
                }
                _ => {}
            }
        }
        other
    }
}

const IDS: ParamSpec = ParamSpec {
    name: "id",
    kind: ParamKind::String,
    required: false,
    help: "Starter Resource to add back; all missing ones when omitted",
};

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "starter.add",
        label: "Add Starter library",
        shortcut: None,
        params: &[IDS],
        enabled: desktop,
        run: add,
    },
    CommandSpec {
        id: "starter.cancel",
        label: "Cancel Starter library download",
        shortcut: None,
        params: &[],
        enabled: crate::command::always,
        run: |s, _| {
            let job = s.starter.download.as_ref().map(|d| d.job).ok_or_else(|| {
                failed("starter.cancel", "No Starter library download is running")
            })?;
            s.jobs().cancel(job);
            Ok(json!({ "job": job.0 }))
        },
    },
    CommandSpec {
        id: "starter.retry",
        label: "Retry the Starter library",
        shortcut: None,
        params: &[],
        enabled: desktop,
        run: retry,
    },
    CommandSpec {
        id: "starter.check",
        label: "Check the released Starter library",
        shortcut: None,
        params: &[],
        enabled: crate::command::always,
        run: |s, _| {
            let job = s.check_starter_release();
            Ok(json!({ "job": job.map(|j| j.0) }))
        },
    },
    CommandSpec {
        id: "starter.list",
        label: "List the Starter library",
        shortcut: None,
        params: &[],
        enabled: crate::command::always,
        run: list,
    },
];

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

fn desktop(_: &Session) -> bool {
    cfg!(not(target_arch = "wasm32"))
}

#[cfg(target_arch = "wasm32")]
fn add(_: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    Err(failed(
        "starter.add",
        "Adding the Starter library requires desktop",
    ))
}

#[cfg(target_arch = "wasm32")]
fn retry(_: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    Err(failed(
        "starter.retry",
        "Adding the Starter library requires desktop",
    ))
}

#[cfg(not(target_arch = "wasm32"))]
fn retry(s: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    let (ids, job) = s.retry_starter().map_err(|e| failed("starter.retry", e))?;
    Ok(json!({ "ids": ids, "job": job.map(|j| j.0) }))
}

#[cfg(not(target_arch = "wasm32"))]
fn add(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let one: Option<Vec<String>> = p
        .str("id")
        .filter(|id| !id.is_empty())
        .map(|id| vec![id.to_string()]);
    let (ids, job) = s
        .add_starter(one.as_deref())
        .map_err(|e| failed("starter.add", e))?;
    Ok(json!({ "ids": ids, "job": job.map(|j| j.0) }))
}

fn list(s: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    let rows: Vec<Value> = s
        .starter_rows()
        .iter()
        .map(|r| {
            let status = match r.status {
                StarterStatus::InLibrary => "in_library",
                StarterStatus::Outdated { .. } => "outdated",
                StarterStatus::Removed => "removed",
                StarterStatus::Missing => "missing",
                StarterStatus::Adding => "adding",
            };
            json!({
                "id": r.resource_id, "title": r.title, "abbreviation": r.abbreviation,
                "type": r.resource_type, "license": r.license, "bytes": r.bytes,
                "features": r.features, "status": status,
            })
        })
        .collect();
    Ok(json!({
        "bundle": s.starter.bundle().map(|b| b.display().to_string()),
        "wanted_bytes": s.starter_wanted_bytes(),
        "resources": rows,
    }))
}
