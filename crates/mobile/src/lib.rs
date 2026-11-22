//! The Mobile app (CONTEXT.md § Mobile) below its platform shells: what each screen shows and
//! what each action does, as plain Rust types (ADR 0021 rule 3). The shells reach it through
//! `apps/obs-ffi` only; this crate never depends on an FFI crate and compiles for wasm32
//! (`docs/dev/mobile-core.md`; boot reads files and is native only).
//!
//! [`MobileApp`] drives the engine's `Session` and its Workspace, with one Panel and at most
//! [`MAX_READERS`] reading Tabs, and speaks only Mobile vocabulary: readers, the Study sheet's
//! hits, History, Settings. Modules:
//!
//! - [`paths`]: the files under the app's one root folder;
//! - `app`: boot, the library, problems;
//! - `readers`: the readers, each a Tab and its reading view;
//! - `navigation`: typed references and Keys, the grid, Back/Forward, History;
//! - `frame`: the hosted text view's frames, tiles and hits; `text_input`: its text input
//!   and accessibility answers;
//! - `session`: the Mobile Last session;
//! - `settings`: Theme, Page tone, Text size and Dynamic Type, fonts, Reduce motion;
//! - [`library`]: Library → Starter library, the Open catalogue, downloads and their states,
//!   Remove and Add back, Library → Storage;
//! - [`download_settings`]: "Use cellular data" and the first-launch Study set offer;
//! - [`import`]: Import of a file the user picked or shared, and its Import report;
//! - [`background`]: the work a background processing task carries on (Search indexes,
//!   alias caches) and its request;
//! - `events`: the pump and its [`Changed`] events.

mod app;
pub mod background;
pub mod download_settings;
mod events;
mod frame;
pub mod import;
pub mod library;
mod navigation;
pub mod paths;
mod readers;
mod session;
mod settings;
mod text_input;
pub mod tokens;

pub use app::{LibraryResource, MobileApp, MobileError};
pub use background::{BACKGROUND_STEP, BackgroundRequest, BackgroundWork, INDEX_REQUEST};
pub use download_settings::{DOWNLOADS_PREF, StudySetOffer};
pub use events::Changed;
pub use frame::{
    AROUND, BAND, Footnote, FootnoteMarkerHit, Hit, HitVerse, HitWord, MAX_TILE_PX, ReaderFrame,
    SystemAppearance, TileKey, TilePlace, TimedTile, Viewport,
};
pub use import::{IMPORT_KINDS, ImportReportView, ImportState, ReportSection};
pub use library::{
    DownloadOrigin, DownloadPlan, DownloadRequest, DownloadRow, DownloadState, Network, OpenRow,
    RemoveAnswer, SearchIndexState, SearchIndexStatus, SetSummary, StarterChoice,
    StarterLibraryView, StarterRow, StarterRowStatus, StorageRow, StorageView, download_label,
};
pub use navigation::{BookChapters, HistoryDay, HistoryVisit, ParsedReference};
pub use obs_engine::licence::{LicenceClass, LicenceFilter};
pub use obs_engine::starter::MobileSet;
pub use obs_engine::sword_repo;
pub use obs_model::ResourceType;
pub use obs_reading::{DocPos, Tile};
pub use paths::Paths;
pub use readers::{MAX_READERS, ReaderId, ReaderSnapshot};
pub use settings::{Choice, Choices, ReadingFont, Settings, text_size_for_content_size};
pub use text_input::{AccessibilityItem, ContentRect, TextSpan, TextUnitKind};

#[cfg(all(test, not(target_arch = "wasm32")))]
mod background_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod import_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod library_tests;
#[cfg(test)]
mod tests;
