//! The Library on the device (#333, plan A7): Library → Starter library and Get Resources'
//! Open catalogue, the downloads, Remove and Add back, and Library → Storage. Every rule is
//! here or in the engine: the manifests and their merge, the Study set, sha256 pinning, each
//! download's state, "Wi-Fi only" and "ask first over cellular", "won't fit". The shell owns
//! only the transfers: small JSON (the Starter manifest, the releases list) through its
//! transport ([`MobileApp::set_fetch`], a `Fetch` the FFI makes from the shell's `Transport`),
//! and each `.obr` or Open catalogue file through a background transfer to the
//! [`DownloadRequest::file`] the core names, then [`MobileApp::install_downloaded`].
//!
//! **A download's states** ([`DownloadState`]): `Queued` when planned, `Downloading` as the
//! shell reports progress, `Verifying` and `Installing` while the core checks the file and
//! puts it in place (a Starter file within [`MobileApp::install_downloaded`]; an Open
//! catalogue file on the job queue), `Indexing` while its Search index is built, then `Done`;
//! `Failed { retry }` from any of them. A failed check deletes the file, so Retry downloads it
//! again; Cancel forgets the download and deletes what the shell wrote.

use std::path::{Path, PathBuf};

use obs_engine::licence::{LicenceClass, LicenceFilter};
use obs_engine::starter::{MobileSet, StarterItem, StarterStatus};
use obs_engine::{ImportPhase, IndexStatus};
use obs_model::ResourceType;
use serde_json::json;

use crate::app::failed;
use crate::{Changed, MobileApp, MobileError};

/// Where a download comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DownloadOrigin {
    /// A Starter library Resource, from the release's assets.
    Starter,
    /// An Open catalogue entry, from its publisher.
    Open,
}

/// Where one download stands (module docs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadState {
    Queued,
    Downloading {
        done: u64,
        total: u64,
    },
    Verifying,
    Installing,
    Indexing,
    Done,
    /// What went wrong, in the engine's words; `retry`: downloading it again may work.
    Failed {
        message: String,
        retry: bool,
    },
}

impl DownloadState {
    /// Queued, downloading, verifying or installing: not yet in the library.
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            DownloadState::Queued
                | DownloadState::Downloading { .. }
                | DownloadState::Verifying
                | DownloadState::Installing
        )
    }
}

/// The device's network as the shell sees it, and the "Use cellular data" setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Network {
    pub is_wifi: bool,
    /// "Use cellular data" (off by default, #333).
    pub allow_cellular: bool,
}

/// One transfer for the shell to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadRequest {
    /// The Starter Resource id, or the Open catalogue entry's id.
    pub resource_id: String,
    pub origin: DownloadOrigin,
    pub url: String,
    /// The file's size, when known (a Starter file's, from its manifest).
    pub bytes: Option<u64>,
    /// Where the shell puts the finished file (in the downloads folder).
    pub file: PathBuf,
    /// Whether the transfer may use cellular data (otherwise it waits for Wi-Fi).
    pub cellular: bool,
}

/// What a Download button does, given the network and the free space.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadPlan {
    /// Start these transfers (each is `Queued` now). Resources the Bundled set holds were
    /// added from it at once and are not among them.
    Start(Vec<DownloadRequest>),
    /// On cellular with "Use cellular data" off: ask first, showing the size. Asking again
    /// with `cellular_confirmed` starts it.
    AskCellular { bytes: Option<u64>, message: String },
    /// Not enough free space: said before anything starts.
    WontFit {
        needed: u64,
        free: u64,
        message: String,
    },
    /// Nothing is missing (or everything asked for came from the Bundled set).
    Nothing,
}

/// Which Starter Resources a Download button asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StarterChoice {
    /// Every missing or outdated, not removed, Resource of a Mobile set (the Study set offer).
    Set(MobileSet),
    /// One Resource (its row's Download, Update or Add back).
    One(String),
}

/// Where a Starter Resource stands, for its row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StarterRowStatus {
    InLibrary,
    /// A newer revision is offered: Update.
    Outdated {
        installed: u32,
        offered: u32,
    },
    /// The user removed it: Add back.
    Removed,
    /// Not on this device: Download.
    Missing,
}

/// One row of Library → Starter library.
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
    /// "26.0 MB".
    pub size: String,
    /// What it powers, as on the desktop ("Word Study: Lexicon entries").
    pub features: Vec<String>,
    pub set: Option<MobileSet>,
    pub status: StarterRowStatus,
    /// Its download, while there is one.
    pub download: Option<DownloadState>,
}

/// Library → Starter library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StarterLibraryView {
    /// In manifest order: the released manifest once fetched, merged with the Bundled set's.
    pub rows: Vec<StarterRow>,
    /// The released manifest is being fetched.
    pub checking: bool,
    /// The released manifest has been fetched this run.
    pub released: bool,
    /// The last failure (the manifest, a download from the engine), until the next attempt.
    pub error: Option<String>,
    /// No release has a Starter manifest this app reads: update the app first.
    pub needs_app_update: bool,
    /// What the Study set offer downloads: Resources and bytes (missing or outdated, not
    /// removed).
    pub study_set: SetSummary,
}

/// What a Mobile set's Download would fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetSummary {
    pub resources: u32,
    pub bytes: u64,
    /// "15 Resources, 136.6 MB".
    pub label: String,
}

/// One Open catalogue row of Get Resources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenRow {
    pub id: String,
    pub title: String,
    pub abbreviation: String,
    pub language: String,
    pub resource_type: Option<ResourceType>,
    /// "from eBible.org".
    pub publisher: String,
    /// The licence as the publisher names it.
    pub licence: String,
    pub licence_class: LicenceClass,
    /// The archived licence evidence.
    pub evidence_url: String,
    /// The library's Resource made from it, when it is in the library.
    pub installed: Option<String>,
    pub download: Option<DownloadState>,
}

/// One Resource of Library → Storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageRow {
    pub resource_id: String,
    pub title: String,
    pub obr: u64,
    pub search_index: u64,
    pub aliases: u64,
    pub total: u64,
    /// "26.0 MB".
    pub size: String,
}

/// Library → Storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageView {
    pub rows: Vec<StorageRow>,
    /// Every file once (a shared alias cache counts once).
    pub total: u64,
    pub size: String,
}

/// What Remove did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoveAnswer {
    /// Not confirmed yet: the question to ask, in the engine's words.
    Confirm(String),
    Removed,
}

/// A download the core is tracking.
#[derive(Debug, Clone)]
pub(crate) struct Download {
    pub(crate) resource_id: String,
    pub(crate) origin: DownloadOrigin,
    pub(crate) file: PathBuf,
    pub(crate) state: DownloadState,
    /// The library's Resource it became (an Open catalogue import's new id).
    pub(crate) installed: Option<String>,
}

/// One download as the shell lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadRow {
    pub resource_id: String,
    pub origin: DownloadOrigin,
    pub state: DownloadState,
}

/// "26.0 MB", as the desktop writes sizes.
pub fn megabytes(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let mb = bytes as f64 / 1_000_000.0;
    format!("{mb:.1} MB")
}

fn resources(n: usize) -> String {
    format!("{n} Resource{}", if n == 1 { "" } else { "s" })
}

/// The rule for a planned download (#333): free space first, then the network. Wi-Fi only by
/// default: on cellular with "Use cellular data" off, the size is shown and the user asked
/// first; `cellular_confirmed` is their yes.
pub fn download_rule(
    bytes: Option<u64>,
    what: &str,
    network: Network,
    cellular_confirmed: bool,
    free: Option<u64>,
) -> Result<bool, DownloadPlan> {
    if let (Some(bytes), Some(free)) = (bytes, free) {
        let needed = obs_engine::library_storage::space_needed(bytes);
        if needed > free {
            return Err(DownloadPlan::WontFit {
                needed,
                free,
                message: format!(
                    "{what} needs {} of free space; this device has {}. Free some space, then \
                     try again.",
                    megabytes(needed),
                    megabytes(free)
                ),
            });
        }
    }
    if network.is_wifi {
        return Ok(network.allow_cellular);
    }
    if network.allow_cellular || cellular_confirmed {
        return Ok(true);
    }
    let size = bytes.map_or_else(|| "its size is not known".to_string(), megabytes);
    Err(DownloadPlan::AskCellular {
        bytes,
        message: format!(
            "Download {what} ({size}) over cellular data? You are not on Wi-Fi, and Use \
             cellular data is off."
        ),
    })
}

impl MobileApp {
    /// The transport for small downloads (the Starter manifest, the releases list), run on job
    /// threads. Without one, only `file://` is fetched.
    pub fn set_fetch(&mut self, fetch: std::sync::Arc<dyn obs_engine::sword_repo::Fetch>) {
        self.session.set_fetch(fetch);
    }

    /// Where the Starter library is downloaded from: a base URL its manifest and `.obr` files
    /// are under, and the releases API the format selection reads (a local mirror in tests;
    /// the latest release and GitHub's API by default).
    pub fn set_starter_source(&mut self, base: &str, releases_api: Option<&str>) {
        self.session.starter.set_download_base(base);
        if let Some(api) = releases_api {
            self.session.starter.set_releases_api(api);
        }
    }

    /// Fetch the released Starter manifest (when Library → Starter library or the Study set
    /// offer opens). `false` when it is being fetched already.
    pub fn check_starter(&mut self) -> bool {
        self.session.check_starter_release().is_some()
    }

    /// Library → Starter library.
    pub fn starter_library(&self) -> StarterLibraryView {
        let starter = &self.session.starter;
        let rows = (self.session.starter_rows().into_iter())
            .map(|r| {
                let set = (starter.manifest())
                    .and_then(|m| m.get(&r.resource_id))
                    .and_then(StarterItem::set);
                let download = self
                    .download(&r.resource_id, DownloadOrigin::Starter)
                    .map(|d| d.state.clone());
                StarterRow {
                    size: megabytes(r.bytes),
                    features: r.features.iter().map(|f| (*f).to_string()).collect(),
                    set,
                    status: match r.status {
                        StarterStatus::InLibrary => StarterRowStatus::InLibrary,
                        StarterStatus::Outdated { installed, offered } => {
                            StarterRowStatus::Outdated { installed, offered }
                        }
                        StarterStatus::Removed => StarterRowStatus::Removed,
                        StarterStatus::Missing | StarterStatus::Adding => StarterRowStatus::Missing,
                    },
                    download,
                    resource_id: r.resource_id,
                    title: r.title,
                    abbreviation: r.abbreviation,
                    resource_type: r.resource_type,
                    language: r.language,
                    license: r.license,
                    bytes: r.bytes,
                }
            })
            .collect();
        let study = self.session.starter_wanted_set(MobileSet::Study);
        let bytes = study.iter().map(|i| i.bytes).sum();
        StarterLibraryView {
            rows,
            checking: starter.is_checking(),
            released: starter.released().is_some(),
            error: starter.error().map(str::to_string),
            needs_app_update: starter.needs_app_update(),
            study_set: SetSummary {
                resources: study.len() as u32,
                bytes,
                label: format!("{}, {}", resources(study.len()), megabytes(bytes)),
            },
        }
    }

    fn download(&self, id: &str, origin: DownloadOrigin) -> Option<&Download> {
        (self.downloads.iter()).find(|d| d.resource_id == id && d.origin == origin)
    }

    fn download_mut(&mut self, id: &str, origin: DownloadOrigin) -> Option<&mut Download> {
        (self.downloads.iter_mut()).find(|d| d.resource_id == id && d.origin == origin)
    }

    /// Every download this run, in the order they were planned.
    pub fn downloads(&self) -> Vec<DownloadRow> {
        (self.downloads.iter())
            .map(|d| DownloadRow {
                resource_id: d.resource_id.clone(),
                origin: d.origin,
                state: d.state.clone(),
            })
            .collect()
    }

    /// Plan the download of `choice`. The Bundled set's own Resources (an Add back) are added
    /// from it at once; the rest need the released manifest, the free space and the network
    /// rule ([`download_rule`]). Started ones are `Queued`.
    pub fn plan_starter_download(
        &mut self,
        choice: &StarterChoice,
        network: Network,
        cellular_confirmed: bool,
        free: Option<u64>,
    ) -> Result<DownloadPlan, MobileError> {
        let items: Vec<StarterItem> = match choice {
            StarterChoice::Set(set) => self.session.starter_wanted_set(*set),
            StarterChoice::One(id) => {
                let item = (self.session.starter.manifest())
                    .and_then(|m| m.get(id))
                    .cloned()
                    .ok_or_else(|| {
                        MobileError::Failed(format!("{id} is not part of the Starter library"))
                    })?;
                // In the library at the offered revision: nothing to do.
                if matches!(
                    self.session.starter_status(&item),
                    StarterStatus::InLibrary | StarterStatus::Adding
                ) {
                    return Ok(DownloadPlan::Nothing);
                }
                vec![item]
            }
        };
        let (bundled, items): (Vec<StarterItem>, Vec<StarterItem>) =
            (items.into_iter()).partition(|i| self.session.starter.bundle_holds(i));
        if !bundled.is_empty() {
            let ids: Vec<String> = bundled.iter().map(|i| i.resource_id.clone()).collect();
            self.add_from_bundle(&ids)?;
        }
        let items: Vec<StarterItem> = (items.into_iter())
            .filter(|i| {
                self.download(&i.resource_id, DownloadOrigin::Starter)
                    .is_none_or(|d| !d.state.is_active())
            })
            .collect();
        if items.is_empty() {
            return Ok(DownloadPlan::Nothing);
        }
        let bytes: u64 = items.iter().map(|i| i.bytes).sum();
        let what = match (choice, items.as_slice()) {
            (StarterChoice::Set(MobileSet::Study), _) => "the Study set".to_string(),
            (_, [one]) => one.abbreviation.clone(),
            _ => resources(items.len()),
        };
        let cellular = match download_rule(Some(bytes), &what, network, cellular_confirmed, free) {
            Ok(cellular) => cellular,
            Err(plan) => return Ok(plan),
        };
        let base = self.session.starter.files_base().to_string();
        let requests: Vec<DownloadRequest> = (items.iter())
            .map(|i| DownloadRequest {
                resource_id: i.resource_id.clone(),
                origin: DownloadOrigin::Starter,
                url: format!("{base}{}.obr", i.resource_id),
                bytes: Some(i.bytes),
                file: self.paths.downloads.join(format!("{}.obr", i.resource_id)),
                cellular,
            })
            .collect();
        self.queue(&requests);
        Ok(DownloadPlan::Start(requests))
    }

    /// Plan the download of Open catalogue entry `id` (Get Resources), as
    /// [`Self::plan_starter_download`] does; its size is not known before it downloads.
    pub fn plan_open_download(
        &mut self,
        id: &str,
        network: Network,
        cellular_confirmed: bool,
        free: Option<u64>,
    ) -> Result<DownloadPlan, MobileError> {
        let source = (self.session.get_bibles.open_source(id).cloned()).ok_or_else(|| {
            MobileError::Failed(format!("The Open catalogue has no entry {id:?}"))
        })?;
        if self.open_installed(&source.id).is_some() {
            return Err(MobileError::Failed(format!(
                "{} is already in your Library",
                source.title
            )));
        }
        if self
            .download(id, DownloadOrigin::Open)
            .is_some_and(|d| d.state.is_active())
        {
            return Ok(DownloadPlan::Nothing);
        }
        let cellular = match download_rule(
            None,
            &source.abbreviation,
            network,
            cellular_confirmed,
            free,
        ) {
            Ok(cellular) => cellular,
            Err(plan) => return Ok(plan),
        };
        let request = DownloadRequest {
            resource_id: source.id.clone(),
            origin: DownloadOrigin::Open,
            url: source.url.clone(),
            bytes: None,
            file: self
                .paths
                .downloads
                .join(format!("open-{}.download", source.id)),
            cellular,
        };
        self.queue(std::slice::from_ref(&request));
        Ok(DownloadPlan::Start(vec![request]))
    }

    fn queue(&mut self, requests: &[DownloadRequest]) {
        for r in requests {
            self.downloads
                .retain(|d| !(d.resource_id == r.resource_id && d.origin == r.origin));
            self.downloads.push(Download {
                resource_id: r.resource_id.clone(),
                origin: r.origin,
                file: r.file.clone(),
                state: DownloadState::Queued,
                installed: None,
            });
        }
        self.events.push(Changed::Downloads);
    }

    fn tracked(&mut self, id: &str, origin: DownloadOrigin) -> Result<&mut Download, MobileError> {
        self.download_mut(id, origin)
            .ok_or_else(|| MobileError::Failed(format!("No download of {id} is planned")))
    }

    /// The shell's transfer of `id` has `done` of `total` bytes.
    pub fn download_progress(
        &mut self,
        id: &str,
        origin: DownloadOrigin,
        done: u64,
        total: u64,
    ) -> Result<(), MobileError> {
        let d = self.tracked(id, origin)?;
        if matches!(
            d.state,
            DownloadState::Queued | DownloadState::Downloading { .. }
        ) {
            d.state = DownloadState::Downloading { done, total };
            self.events.push(Changed::Downloads);
        }
        Ok(())
    }

    /// The shell's transfer of `id` failed (no network, a server error): Retry downloads it
    /// again. A partial file is deleted.
    pub fn download_failed(
        &mut self,
        id: &str,
        origin: DownloadOrigin,
        message: &str,
    ) -> Result<(), MobileError> {
        let d = self.tracked(id, origin)?;
        let _ = std::fs::remove_file(&d.file);
        d.state = DownloadState::Failed {
            message: message.to_string(),
            retry: true,
        };
        self.events.push(Changed::Downloads);
        Ok(())
    }

    /// Cancel: the download is forgotten and what the shell wrote of it is deleted.
    pub fn cancel_download(&mut self, id: &str, origin: DownloadOrigin) -> Result<(), MobileError> {
        let d = self.tracked(id, origin)?;
        if !matches!(
            d.state,
            DownloadState::Queued
                | DownloadState::Downloading { .. }
                | DownloadState::Failed { .. }
        ) {
            return Err(MobileError::Failed(format!(
                "{id} is already being added to your Library"
            )));
        }
        let _ = std::fs::remove_file(&d.file);
        self.downloads
            .retain(|d| !(d.resource_id == id && d.origin == origin));
        self.events.push(Changed::Downloads);
        Ok(())
    }

    /// The shell's transfer of `id` finished at `path`: check it against its pinned sha256 (or
    /// licence notice) and add it to the library (module docs). A Starter file is checked and
    /// put in place before this returns; an Open catalogue file is imported on the job queue.
    /// The file is deleted once read. Returns the download's state.
    pub fn install_downloaded(
        &mut self,
        id: &str,
        path: &Path,
        origin: DownloadOrigin,
    ) -> Result<DownloadState, MobileError> {
        let file = path.to_path_buf();
        if self.download(id, origin).is_none() {
            self.downloads.push(Download {
                resource_id: id.to_string(),
                origin,
                file: file.clone(),
                state: DownloadState::Queued,
                installed: None,
            });
        }
        self.tracked(id, origin)?.state = DownloadState::Verifying;
        let outcome = match origin {
            DownloadOrigin::Starter => self.install_starter_file(id, &file),
            DownloadOrigin::Open => self.install_open_file(id, &file),
        };
        let state = match outcome {
            Ok((installed, state)) => {
                self.tracked(id, origin)?.installed = Some(installed);
                state
            }
            Err(message) => {
                let _ = std::fs::remove_file(&file);
                DownloadState::Failed {
                    message,
                    retry: true,
                }
            }
        };
        self.tracked(id, origin)?.state = state.clone();
        self.events.push(Changed::Downloads);
        Ok(state)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn install_starter_file(
        &mut self,
        id: &str,
        file: &Path,
    ) -> Result<(String, DownloadState), String> {
        let item = (self.session.starter.manifest())
            .and_then(|m| m.get(id))
            .cloned()
            .ok_or_else(|| format!("{id} is not part of the Starter library"))?;
        self.session
            .add_starter_from_files(&[item], &[(id.to_string(), file.to_path_buf())])?;
        Ok((id.to_string(), self.indexing_state(id)))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn install_open_file(
        &mut self,
        id: &str,
        file: &Path,
    ) -> Result<(String, DownloadState), String> {
        let (resource, _job) = self.session.install_open_file(id, file)?;
        Ok((resource, DownloadState::Installing))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn add_from_bundle(&mut self, ids: &[String]) -> Result<(), MobileError> {
        self.session
            .add_starter(Some(ids))
            .map(|_| self.events.push(Changed::Downloads))
            .map_err(MobileError::Failed)
    }

    #[cfg(target_arch = "wasm32")]
    fn add_from_bundle(&mut self, _: &[String]) -> Result<(), MobileError> {
        Err(MobileError::Failed(
            "Adding from the Bundled set needs files".into(),
        ))
    }

    #[cfg(target_arch = "wasm32")]
    fn install_starter_file(
        &mut self,
        _: &str,
        _: &Path,
    ) -> Result<(String, DownloadState), String> {
        Err("Installing a download needs files".into())
    }

    #[cfg(target_arch = "wasm32")]
    fn install_open_file(&mut self, _: &str, _: &Path) -> Result<(String, DownloadState), String> {
        Err("Installing a download needs files".into())
    }

    /// `Indexing` while the Resource's Search index is being built, else `Done`.
    fn indexing_state(&self, resource_id: &str) -> DownloadState {
        match self.session.search.status(resource_id) {
            Some(IndexStatus::Indexing { .. }) => DownloadState::Indexing,
            _ => DownloadState::Done,
        }
    }

    /// Move each download on from what the engine did since (the pump calls it): an Open
    /// catalogue import's phases, and Search indexes finished.
    pub(crate) fn advance_downloads(&mut self) {
        let mut changed = false;
        for i in 0..self.downloads.len() {
            let d = &self.downloads[i];
            let Some(resource) = d.installed.clone() else {
                continue;
            };
            let next = match (&d.state, d.origin) {
                (DownloadState::Installing | DownloadState::Indexing, DownloadOrigin::Open) => {
                    match self.session.imports.report(&resource) {
                        Some(r) => match r.phase {
                            ImportPhase::Failed => DownloadState::Failed {
                                message: r.error.clone().unwrap_or_else(|| "Import failed".into()),
                                retry: true,
                            },
                            ImportPhase::Complete => self.indexing_state(&resource),
                            ImportPhase::Index => DownloadState::Indexing,
                            ImportPhase::Download | ImportPhase::Parse => DownloadState::Installing,
                        },
                        None => DownloadState::Failed {
                            message: "The Import was stopped".into(),
                            retry: true,
                        },
                    }
                }
                (DownloadState::Indexing, DownloadOrigin::Starter) => {
                    self.indexing_state(&resource)
                }
                (state, _) => state.clone(),
            };
            if next != self.downloads[i].state {
                self.downloads[i].state = next;
                changed = true;
            }
        }
        if changed {
            self.events.push(Changed::Downloads);
        }
    }

    /// The small banner over the Library while downloads run (#333): `None` when there is
    /// nothing to say.
    pub fn download_banner(&self) -> Option<String> {
        let active: Vec<&Download> = self
            .downloads
            .iter()
            .filter(|d| d.state.is_active())
            .collect();
        let failed = (self.downloads.iter())
            .filter(|d| matches!(d.state, DownloadState::Failed { .. }))
            .count();
        let indexing = (self.downloads.iter())
            .filter(|d| d.state == DownloadState::Indexing)
            .count();
        let failed_note = (failed > 0).then(|| {
            format!(
                "{failed} download{} failed",
                if failed == 1 { "" } else { "s" }
            )
        });
        if !active.is_empty() {
            let (done, total) = active.iter().fold((0u64, 0u64), |(d, t), x| match x.state {
                DownloadState::Downloading { done, total } => (d + done, t + total),
                _ => (d, t),
            });
            let mut text = format!("Downloading {}", resources(active.len()));
            if total > 0 {
                text.push_str(&format!(" · {} of {}", megabytes(done), megabytes(total)));
            }
            if let Some(f) = failed_note {
                text.push_str(&format!(" · {f}"));
            }
            return Some(text);
        }
        if indexing > 0 {
            let mut text = format!("Building the Search index of {}", resources(indexing));
            if let Some(f) = failed_note {
                text.push_str(&format!(" · {f}"));
            }
            return Some(text);
        }
        failed_note
    }

    /// The library's Resource made from Open catalogue entry `entry`, if it is in the library.
    fn open_installed(&self, entry: &str) -> Option<String> {
        let source = self.session.get_bibles.open_source(entry)?;
        match self.session.listing_status(&source.listing()) {
            obs_engine::get_bibles::ListingStatus::Installed { resource_id }
            | obs_engine::get_bibles::ListingStatus::UpdateAvailable { resource_id, .. } => {
                Some(resource_id)
            }
            _ => None,
        }
    }

    /// Get Resources' Open catalogue rows that `licence` admits, in the catalogue's order, with
    /// the same licence filter as the desktop.
    pub fn open_catalogue(&self, licence: LicenceFilter) -> Vec<OpenRow> {
        (self.session.get_bibles.open_sources().iter())
            .filter(|s| licence.accepts(s.licence_class))
            .map(|s| OpenRow {
                id: s.id.clone(),
                title: s.title.clone(),
                abbreviation: s.abbreviation.clone(),
                language: s.language.clone(),
                resource_type: s.resource_type(),
                publisher: format!("from {}", s.publisher),
                licence: s.licence.clone(),
                licence_class: s.licence_class,
                evidence_url: s.evidence_url.clone(),
                installed: self.open_installed(&s.id),
                download: self
                    .download(&s.id, DownloadOrigin::Open)
                    .map(|d| d.state.clone()),
            })
            .collect()
    }

    /// Remove Resource `id` from the library (Library → Starter library or Storage). Without
    /// `confirmed`, only the question (what goes and how many Annotations refer to it). A
    /// removed Starter Resource stays removed: it is never added back silently, and the Study
    /// set leaves it out. Not while a reader shows it.
    pub fn remove_resource(
        &mut self,
        id: &str,
        confirmed: bool,
    ) -> Result<RemoveAnswer, MobileError> {
        if let Some(entry) = self.session.library.get(id)
            && (self.readers.iter()).any(|r| r.view.doc.manifest.resource_id == id)
        {
            return Err(MobileError::Failed(format!(
                "{} is open in the reader. Show another Resource there, then remove it.",
                entry.manifest.abbreviation
            )));
        }
        let out = self
            .session
            .run("library.remove", &json!({ "id": id, "confirm": confirmed }))
            .map_err(failed)?;
        // The question is the shell's to ask; the desktop's dialog effect is dropped.
        self.apply_effects();
        if let Some(question) = out["confirm"].as_str() {
            return Ok(RemoveAnswer::Confirm(question.to_string()));
        }
        self.docs.remove(id);
        self.events.push(Changed::Downloads);
        Ok(RemoveAnswer::Removed)
    }

    /// Library → Storage: the space each Resource takes, in Preferred order.
    pub fn storage(&self) -> StorageView {
        let storage = self.session.library_storage();
        let rows = (storage.resources.into_iter())
            .map(|r| StorageRow {
                title: (self.session.library.get(&r.resource_id))
                    .map(|e| e.manifest.title.clone())
                    .unwrap_or_default(),
                total: r.total(),
                size: megabytes(r.total()),
                obr: r.obr,
                search_index: r.search_index,
                aliases: r.aliases,
                resource_id: r.resource_id,
            })
            .collect();
        StorageView {
            rows,
            total: storage.total,
            size: megabytes(storage.total),
        }
    }
}
