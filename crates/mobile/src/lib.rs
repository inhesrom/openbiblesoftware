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
//! - `events`: the pump and its [`Changed`] events.

mod app;
mod events;
mod frame;
mod navigation;
pub mod paths;
mod readers;
mod session;
mod settings;
mod text_input;

pub use app::{LibraryItem, MobileApp, MobileError};
pub use events::Changed;
pub use frame::{
    AROUND, BAND, Footnote, FootnoteMarkerHit, Hit, HitVerse, HitWord, MAX_TILE_PX, ReaderFrame,
    SystemAppearance, TileKey, TilePlace, TimedTile, Viewport,
};
pub use navigation::{BookChapters, HistoryDay, HistoryVisit, ParsedReference};
pub use obs_model::ResourceType;
pub use obs_reading::{DocPos, Tile};
pub use paths::Paths;
pub use readers::{MAX_READERS, ReaderId, ReaderSnapshot};
pub use settings::{Choice, Choices, ReadingFont, Settings, text_size_for_content_size};
pub use text_input::{AccessibilityItem, ContentRect, TextSpan, TextUnitKind};

#[cfg(test)]
mod tests;
