//! The Library across the FFI (`docs/dev/mobile-library.md`): the shell's [`Transport`] for
//! small JSON, the Starter library and Open catalogue screens, the download plan and states,
//! Remove, Storage. Mirror Records and Enums of `obs_mobile::library`; every rule stays there.

use std::path::PathBuf;
use std::sync::Arc;

use obs_mobile::sword_repo::{Fetch, FetchError, Progress};

use crate::{MobileApp, MobileError, ResourceType};

// ---- The shell's transport ----------------------------------------------------------------

/// Why the shell's transport could not fetch a URL, as `apps/openbible/src/fetch.rs` tells
/// statuses apart.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
pub enum TransportError {
    /// HTTP 404 or 410.
    NotFound,
    /// HTTP 429, or GitHub's 403 with no requests remaining.
    RateLimited,
    /// Anything else: offline, refused, timed out, another status.
    Network { message: String },
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransportError::NotFound => f.write_str("not found"),
            TransportError::RateLimited => f.write_str("rate limited"),
            TransportError::Network { message } => f.write_str(message),
        }
    }
}

impl std::error::Error for TransportError {}

/// A Swift error the bindings do not know (any `Error` but a `TransportError`) arrives as a
/// network error instead of stopping the core.
impl From<uniffi::UnexpectedUniFFICallbackError> for TransportError {
    fn from(e: uniffi::UnexpectedUniFFICallbackError) -> Self {
        TransportError::Network { message: e.reason }
    }
}

/// What the shell implements to fetch small files whole (the Starter manifest, GitHub's
/// releases list). Called on the core's job threads, never on the main thread, and never
/// with the core locked; it blocks until the answer is in.
#[uniffi::export(with_foreign)]
pub trait Transport: Send + Sync {
    fn get(&self, url: String) -> Result<Vec<u8>, TransportError>;
}

/// The shell's [`Transport`] as the engine's `Fetch` (plan A7).
pub(crate) struct TransportFetch(pub(crate) Arc<dyn Transport>);

impl std::fmt::Debug for TransportFetch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TransportFetch")
    }
}

impl Fetch for TransportFetch {
    fn get(&self, url: &str, progress: Progress<'_>) -> Result<Vec<u8>, FetchError> {
        if !progress(0, None) {
            return Err(FetchError::Cancelled);
        }
        let bytes = self.0.get(url.to_string()).map_err(|e| match e {
            TransportError::NotFound => FetchError::NotFound,
            TransportError::RateLimited => FetchError::RateLimited,
            TransportError::Network { message } => FetchError::Network(message),
        })?;
        let n = bytes.len() as u64;
        if !progress(n, Some(n)) {
            return Err(FetchError::Cancelled);
        }
        Ok(bytes)
    }
}

// ---- Records and Enums --------------------------------------------------------------------

/// The Mobile app's sets of the Starter library (#333).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileSet {
    Bundled,
    Study,
    More,
}

impl From<obs_mobile::MobileSet> for MobileSet {
    fn from(s: obs_mobile::MobileSet) -> Self {
        match s {
            obs_mobile::MobileSet::Bundled => Self::Bundled,
            obs_mobile::MobileSet::Study => Self::Study,
            obs_mobile::MobileSet::More => Self::More,
        }
    }
}

impl From<MobileSet> for obs_mobile::MobileSet {
    fn from(s: MobileSet) -> Self {
        match s {
            MobileSet::Bundled => Self::Bundled,
            MobileSet::Study => Self::Study,
            MobileSet::More => Self::More,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum DownloadOrigin {
    Starter,
    Open,
}

impl From<obs_mobile::DownloadOrigin> for DownloadOrigin {
    fn from(o: obs_mobile::DownloadOrigin) -> Self {
        match o {
            obs_mobile::DownloadOrigin::Starter => Self::Starter,
            obs_mobile::DownloadOrigin::Open => Self::Open,
        }
    }
}

impl From<DownloadOrigin> for obs_mobile::DownloadOrigin {
    fn from(o: DownloadOrigin) -> Self {
        match o {
            DownloadOrigin::Starter => Self::Starter,
            DownloadOrigin::Open => Self::Open,
        }
    }
}

/// Where one download stands.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum DownloadState {
    Queued,
    Downloading { done: u64, total: u64 },
    Verifying,
    Installing,
    Indexing,
    Done,
    Failed { message: String, retry: bool },
}

impl From<obs_mobile::DownloadState> for DownloadState {
    fn from(s: obs_mobile::DownloadState) -> Self {
        use obs_mobile::DownloadState as S;
        match s {
            S::Queued => Self::Queued,
            S::Downloading { done, total } => Self::Downloading { done, total },
            S::Verifying => Self::Verifying,
            S::Installing => Self::Installing,
            S::Indexing => Self::Indexing,
            S::Done => Self::Done,
            S::Failed { message, retry } => Self::Failed { message, retry },
        }
    }
}

impl From<DownloadState> for obs_mobile::DownloadState {
    fn from(s: DownloadState) -> Self {
        match s {
            DownloadState::Queued => Self::Queued,
            DownloadState::Downloading { done, total } => Self::Downloading { done, total },
            DownloadState::Verifying => Self::Verifying,
            DownloadState::Installing => Self::Installing,
            DownloadState::Indexing => Self::Indexing,
            DownloadState::Done => Self::Done,
            DownloadState::Failed { message, retry } => Self::Failed { message, retry },
        }
    }
}

/// A download's state in words, for its row.
#[uniffi::export]
pub fn download_label(state: DownloadState) -> String {
    obs_mobile::download_label(&state.into())
}

/// The network as the shell sees it, and "Use cellular data".
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct Network {
    /// `None` while the shell does not know yet: counts as not Wi-Fi.
    pub is_wifi: Option<bool>,
    pub allow_cellular: bool,
}

impl From<Network> for obs_mobile::Network {
    fn from(n: Network) -> Self {
        Self {
            is_wifi: n.is_wifi,
            allow_cellular: n.allow_cellular,
        }
    }
}

/// One transfer for the shell to start: `url` to `file`, cellular only when `cellular`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DownloadRequest {
    pub resource_id: String,
    pub origin: DownloadOrigin,
    pub url: String,
    pub bytes: Option<u64>,
    pub file: String,
    pub cellular: bool,
}

impl From<obs_mobile::DownloadRequest> for DownloadRequest {
    fn from(r: obs_mobile::DownloadRequest) -> Self {
        Self {
            resource_id: r.resource_id,
            origin: r.origin.into(),
            url: r.url,
            bytes: r.bytes,
            file: r.file.to_string_lossy().into_owned(),
            cellular: r.cellular,
        }
    }
}

/// What a Download button does.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum DownloadPlan {
    Start {
        requests: Vec<DownloadRequest>,
    },
    AskCellular {
        bytes: Option<u64>,
        message: String,
    },
    WontFit {
        needed: u64,
        free: u64,
        message: String,
    },
    Nothing,
}

impl From<obs_mobile::DownloadPlan> for DownloadPlan {
    fn from(p: obs_mobile::DownloadPlan) -> Self {
        use obs_mobile::DownloadPlan as P;
        match p {
            P::Start(requests) => Self::Start {
                requests: requests.into_iter().map(DownloadRequest::from).collect(),
            },
            P::AskCellular { bytes, message } => Self::AskCellular { bytes, message },
            P::WontFit {
                needed,
                free,
                message,
            } => Self::WontFit {
                needed,
                free,
                message,
            },
            P::Nothing => Self::Nothing,
        }
    }
}

/// Which Starter Resources a Download button asks for.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum StarterChoice {
    Set { set: MobileSet },
    One { resource_id: String },
}

impl From<StarterChoice> for obs_mobile::StarterChoice {
    fn from(c: StarterChoice) -> Self {
        match c {
            StarterChoice::Set { set } => Self::Set(set.into()),
            StarterChoice::One { resource_id } => Self::One(resource_id),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum StarterRowStatus {
    InLibrary,
    Outdated { installed: u32, offered: u32 },
    Removed,
    Missing,
}

impl From<obs_mobile::StarterRowStatus> for StarterRowStatus {
    fn from(s: obs_mobile::StarterRowStatus) -> Self {
        use obs_mobile::StarterRowStatus as S;
        match s {
            S::InLibrary => Self::InLibrary,
            S::Outdated { installed, offered } => Self::Outdated { installed, offered },
            S::Removed => Self::Removed,
            S::Missing => Self::Missing,
        }
    }
}

/// One row of Library → Starter library.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct StarterRow {
    pub resource_id: String,
    pub title: String,
    pub abbreviation: String,
    pub resource_type: Option<ResourceType>,
    pub language: String,
    pub license: String,
    pub bytes: u64,
    pub size: String,
    pub features: Vec<String>,
    pub set: Option<MobileSet>,
    pub status: StarterRowStatus,
    pub download: Option<DownloadState>,
}

impl From<obs_mobile::StarterRow> for StarterRow {
    fn from(r: obs_mobile::StarterRow) -> Self {
        Self {
            resource_id: r.resource_id,
            title: r.title,
            abbreviation: r.abbreviation,
            resource_type: r.resource_type.map(ResourceType::from),
            language: r.language,
            license: r.license,
            bytes: r.bytes,
            size: r.size,
            features: r.features,
            set: r.set.map(MobileSet::from),
            status: r.status.into(),
            download: r.download.map(DownloadState::from),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SetSummary {
    pub resources: u32,
    pub bytes: u64,
    pub label: String,
}

/// Library → Starter library.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct StarterLibraryView {
    pub rows: Vec<StarterRow>,
    pub checking: bool,
    pub released: bool,
    pub error: Option<String>,
    pub needs_app_update: bool,
    pub study_set: SetSummary,
}

impl From<obs_mobile::StarterLibraryView> for StarterLibraryView {
    fn from(v: obs_mobile::StarterLibraryView) -> Self {
        Self {
            rows: v.rows.into_iter().map(StarterRow::from).collect(),
            checking: v.checking,
            released: v.released,
            error: v.error,
            needs_app_update: v.needs_app_update,
            study_set: SetSummary {
                resources: v.study_set.resources,
                bytes: v.study_set.bytes,
                label: v.study_set.label,
            },
        }
    }
}

/// A Licence class (#134).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum LicenceClass {
    FreeToShare,
    PersonalUse,
    Unclear,
}

impl From<obs_mobile::LicenceClass> for LicenceClass {
    fn from(c: obs_mobile::LicenceClass) -> Self {
        match c {
            obs_mobile::LicenceClass::FreeToShare => Self::FreeToShare,
            obs_mobile::LicenceClass::PersonalUse => Self::PersonalUse,
            obs_mobile::LicenceClass::Unclear => Self::Unclear,
        }
    }
}

impl From<LicenceClass> for obs_mobile::LicenceClass {
    fn from(c: LicenceClass) -> Self {
        match c {
            LicenceClass::FreeToShare => Self::FreeToShare,
            LicenceClass::PersonalUse => Self::PersonalUse,
            LicenceClass::Unclear => Self::Unclear,
        }
    }
}

/// A Licence class in words, as the desktop's licence badge says it.
#[uniffi::export]
pub fn licence_class_label(class: LicenceClass) -> String {
    obs_mobile::LicenceClass::from(class).label().to_string()
}

/// Get Resources' licence filter (#134): each choice includes the stricter ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum LicenceFilter {
    FreeToShare,
    PersonalUse,
    All,
}

impl From<LicenceFilter> for obs_mobile::LicenceFilter {
    fn from(f: LicenceFilter) -> Self {
        match f {
            LicenceFilter::FreeToShare => Self::FreeToShare,
            LicenceFilter::PersonalUse => Self::PersonalUse,
            LicenceFilter::All => Self::All,
        }
    }
}

impl From<obs_mobile::LicenceFilter> for LicenceFilter {
    fn from(f: obs_mobile::LicenceFilter) -> Self {
        match f {
            obs_mobile::LicenceFilter::FreeToShare => Self::FreeToShare,
            obs_mobile::LicenceFilter::PersonalUse => Self::PersonalUse,
            obs_mobile::LicenceFilter::All => Self::All,
        }
    }
}

/// One choice of the licence filter, with the desktop's words.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LicenceFilterChoice {
    pub filter: LicenceFilter,
    pub label: String,
    /// What the choice shows.
    pub detail: String,
}

/// The licence filter's choices in order, and the one it starts on (the desktop's default).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LicenceFilters {
    pub choices: Vec<LicenceFilterChoice>,
    pub default: LicenceFilter,
}

#[uniffi::export]
pub fn licence_filters() -> LicenceFilters {
    LicenceFilters {
        choices: (obs_mobile::LicenceFilter::CHOICES.into_iter())
            .map(|f| LicenceFilterChoice {
                filter: f.into(),
                label: f.label().to_string(),
                detail: f.hover().to_string(),
            })
            .collect(),
        default: obs_mobile::LicenceFilter::default().into(),
    }
}

/// The first-launch Study set offer, every word of it (`obs_mobile::StudySetOffer`).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct StudySetOffer {
    pub title: String,
    pub message: String,
    pub size: Option<String>,
    pub status: Option<String>,
    pub network: String,
    pub later: String,
    pub download: String,
    pub not_now: String,
    pub retry: Option<String>,
    pub looking_up: bool,
}

impl From<obs_mobile::StudySetOffer> for StudySetOffer {
    fn from(o: obs_mobile::StudySetOffer) -> Self {
        Self {
            title: o.title,
            message: o.message,
            size: o.size,
            status: o.status,
            network: o.network,
            later: o.later,
            download: o.download,
            not_now: o.not_now,
            retry: o.retry,
            looking_up: o.looking_up,
        }
    }
}

/// One Open catalogue row of Get Resources.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct OpenRow {
    pub id: String,
    pub title: String,
    pub abbreviation: String,
    pub language: String,
    pub resource_type: Option<ResourceType>,
    pub publisher: String,
    pub licence: String,
    pub licence_class: LicenceClass,
    pub evidence_url: String,
    pub installed: Option<String>,
    pub download: Option<DownloadState>,
}

impl From<obs_mobile::OpenRow> for OpenRow {
    fn from(r: obs_mobile::OpenRow) -> Self {
        Self {
            id: r.id,
            title: r.title,
            abbreviation: r.abbreviation,
            language: r.language,
            resource_type: r.resource_type.map(ResourceType::from),
            publisher: r.publisher,
            licence: r.licence,
            licence_class: r.licence_class.into(),
            evidence_url: r.evidence_url,
            installed: r.installed,
            download: r.download.map(DownloadState::from),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DownloadRow {
    pub resource_id: String,
    pub origin: DownloadOrigin,
    pub state: DownloadState,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct StorageRow {
    pub resource_id: String,
    pub title: String,
    pub obr: u64,
    pub search_index: u64,
    pub aliases: u64,
    pub total: u64,
    pub size: String,
    /// Its Search index (`None` if it has none to build).
    pub index: Option<crate::SearchIndexStatus>,
}

/// Library → Storage.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct StorageView {
    pub rows: Vec<StorageRow>,
    pub total: u64,
    pub size: String,
}

impl From<obs_mobile::StorageView> for StorageView {
    fn from(v: obs_mobile::StorageView) -> Self {
        Self {
            rows: (v.rows.into_iter())
                .map(|r| StorageRow {
                    resource_id: r.resource_id,
                    title: r.title,
                    obr: r.obr,
                    search_index: r.search_index,
                    aliases: r.aliases,
                    total: r.total,
                    size: r.size,
                    index: r.index.map(Into::into),
                })
                .collect(),
            total: v.total,
            size: v.size,
        }
    }
}

/// What Remove did: the question to ask first, or done.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum RemoveAnswer {
    Confirm { question: String },
    Removed,
}

// ---- The Object's Library methods ---------------------------------------------------------

#[uniffi::export]
impl MobileApp {
    /// Fetch small files (the Starter manifest, the releases list) through the shell's
    /// transport from now on.
    pub fn set_transport(&self, transport: Arc<dyn Transport>) -> Result<(), MobileError> {
        self.with(|a| {
            a.set_fetch(Arc::new(TransportFetch(transport)));
            Ok(())
        })
    }

    /// Download the Starter library from `base` (a mirror), and read the releases list from
    /// `releases_api` when given (the latest release and GitHub's API by default).
    pub fn set_starter_source(
        &self,
        base: String,
        releases_api: Option<String>,
    ) -> Result<(), MobileError> {
        self.with(|a| {
            a.set_starter_source(&base, releases_api.as_deref());
            Ok(())
        })
    }

    /// Fetch the released Starter manifest; `false` when it is being fetched already.
    pub fn check_starter(&self) -> Result<bool, MobileError> {
        self.with(|a| Ok(a.check_starter()))
    }

    pub fn starter_library(&self) -> Result<StarterLibraryView, MobileError> {
        self.read(|a| a.starter_library().into())
    }

    /// What a Starter library Download button does, given the network and the free bytes
    /// (`None`: unknown). Started transfers are `Queued`.
    pub fn plan_starter_download(
        &self,
        choice: StarterChoice,
        network: Network,
        cellular_confirmed: bool,
        free_bytes: Option<u64>,
    ) -> Result<DownloadPlan, MobileError> {
        self.with(|a| {
            a.plan_starter_download(
                &choice.into(),
                network.into(),
                cellular_confirmed,
                free_bytes,
            )
        })
        .map(DownloadPlan::from)
    }

    /// What an Open catalogue row's Download does.
    pub fn plan_open_download(
        &self,
        id: String,
        network: Network,
        cellular_confirmed: bool,
        free_bytes: Option<u64>,
    ) -> Result<DownloadPlan, MobileError> {
        self.with(|a| a.plan_open_download(&id, network.into(), cellular_confirmed, free_bytes))
            .map(DownloadPlan::from)
    }

    pub fn download_progress(
        &self,
        resource_id: String,
        origin: DownloadOrigin,
        done: u64,
        total: u64,
    ) -> Result<(), MobileError> {
        self.with(|a| a.download_progress(&resource_id, origin.into(), done, total))
    }

    pub fn download_failed(
        &self,
        resource_id: String,
        origin: DownloadOrigin,
        message: String,
    ) -> Result<(), MobileError> {
        self.with(|a| a.download_failed(&resource_id, origin.into(), &message))
    }

    /// Cancel: the core deletes what the shell wrote of it.
    pub fn cancel_download(
        &self,
        resource_id: String,
        origin: DownloadOrigin,
    ) -> Result<(), MobileError> {
        self.with(|a| a.cancel_download(&resource_id, origin.into()))
    }

    /// The transfer of `resource_id` finished at `path` (the request's `file`; any other path
    /// is refused and left alone): a job checks it (sha256 or the licence notice), adds it
    /// and deletes the file; `poll_changed` reports `downloads` as it moves on. Handing it in
    /// again once it is verifying, installing or done changes nothing.
    pub fn install_downloaded(
        &self,
        resource_id: String,
        path: String,
        origin: DownloadOrigin,
    ) -> Result<DownloadState, MobileError> {
        self.with(|a| a.install_downloaded(&resource_id, &PathBuf::from(path), origin.into()))
            .map(DownloadState::from)
    }

    pub fn downloads(&self) -> Result<Vec<DownloadRow>, MobileError> {
        self.read(|a| {
            (a.downloads().into_iter())
                .map(|d| DownloadRow {
                    resource_id: d.resource_id,
                    origin: d.origin.into(),
                    state: d.state.into(),
                })
                .collect()
        })
    }

    /// The Library's download banner, when there is something to say.
    pub fn download_banner(&self) -> Result<Option<String>, MobileError> {
        self.read(obs_mobile::MobileApp::download_banner)
    }

    pub fn open_catalogue(&self, licence: LicenceFilter) -> Result<Vec<OpenRow>, MobileError> {
        self.read(|a| {
            (a.open_catalogue(licence.into()).into_iter())
                .map(OpenRow::from)
                .collect()
        })
    }

    /// Remove a Resource: without `confirmed`, the question to ask. A reader that shows it
    /// switches to another Resource first (the second reader of a Split view closes).
    pub fn remove_resource(
        &self,
        resource_id: String,
        confirmed: bool,
    ) -> Result<RemoveAnswer, MobileError> {
        self.with(|a| a.remove_resource(&resource_id, confirmed))
            .map(|r| match r {
                obs_mobile::RemoveAnswer::Confirm(question) => RemoveAnswer::Confirm { question },
                obs_mobile::RemoveAnswer::Removed => RemoveAnswer::Removed,
            })
    }

    pub fn storage(&self) -> Result<StorageView, MobileError> {
        self.read(|a| a.storage().into())
    }

    /// "Use cellular data" (off by default): pass it as every `Network`'s `allow_cellular`.
    pub fn use_cellular(&self) -> Result<bool, MobileError> {
        self.read(obs_mobile::MobileApp::use_cellular)
    }

    pub fn set_use_cellular(&self, on: bool) -> Result<(), MobileError> {
        self.with(|a| a.set_use_cellular(on))
    }

    /// The first-launch Study set offer while there is one to make (show the sheet while
    /// this answers one; ask again on `Changed.downloads`).
    pub fn study_set_offer(&self, network: Network) -> Result<Option<StudySetOffer>, MobileError> {
        self.read(|a| a.study_set_offer(network.into()).map(StudySetOffer::from))
    }

    /// The offer's sheet appeared: it is not made at a later launch.
    pub fn study_set_offer_shown(&self) -> Result<(), MobileError> {
        self.with(obs_mobile::MobileApp::study_set_offer_shown)
    }
}
