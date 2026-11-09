//! The only crate that uses SQLite: the `.obr` Resource file reader and writer (ADR 0003), the
//! User database with its Annotations, and the per-Resource Search index caches (#58).
//!
//! Layer L2. The storage traits ([`ResourceStore`]) compile everywhere, including
//! `wasm32-unknown-unknown`. The SQLite implementations ([`ObrReader`], [`write_obr`]) need
//! rusqlite's bundled C SQLite, which does not build for `wasm32-unknown-unknown`; they are
//! compiled only off wasm until a sqlite-wasm-rs backend lands (#12 rule 5).

pub mod annotation;
#[cfg(not(target_arch = "wasm32"))]
mod annotation_store;
mod backup;
#[cfg(not(target_arch = "wasm32"))]
mod backup_store;
pub mod clock;
// Pure column encodings; the wasm backend will use them too once it exists.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
mod codec;
mod error;
mod favorites;
#[cfg(not(target_arch = "wasm32"))]
mod favorites_store;
pub mod highlight_style;
#[cfg(not(target_arch = "wasm32"))]
mod highlight_style_store;
#[cfg(not(target_arch = "wasm32"))]
mod obr;
mod reading_plans;
#[cfg(not(target_arch = "wasm32"))]
mod reading_plans_store;
mod resource_store;
#[cfg(not(target_arch = "wasm32"))]
mod sample;
#[cfg(not(target_arch = "wasm32"))]
mod search_index;
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
mod span_codec;
#[cfg(not(target_arch = "wasm32"))]
mod user;

pub use annotation::{
    Anchor, Annotation, CONTEXT_CHARS, Change, Content, Notebook, Row, RowKey, TextRange,
};
#[cfg(not(target_arch = "wasm32"))]
pub use annotation_store::DEFAULT_NOTEBOOK_NAME;
pub use backup::{
    BACKUP_EXTENSION, BackupContents, BackupFile, BackupKind, ROLLING_BACKUPS, SAFETY_BACKUPS,
    UPDATE_BACKUPS,
};
pub use error::StoreError;
pub use favorites::{FavoriteRow, HISTORY_DAYS, HISTORY_LIMIT, VisitRow};
pub use highlight_style::{
    HighlightLook, HighlightPalette, HighlightStyle, MAX_STYLE_NAME, Rgb, STANDARD_PALETTE_ID,
    STANDARD_PALETTE_NAME, StyleAppearance, built_in_styles, fallback_appearance, standard_palette,
};
#[cfg(not(target_arch = "wasm32"))]
pub use obr::{APPLICATION_ID, ObrReader, copy_obr_as, write_obr};
pub use reading_plans::{PlanReadingRow, ReadingPlanRow};
pub use resource_store::{MappedVerse, MentionStats, ResourceStore, Summary};
#[cfg(not(target_arch = "wasm32"))]
pub use sample::sample_resource;
#[cfg(all(feature = "test-support", not(target_arch = "wasm32")))]
pub use search_index::damage_search_index;
#[cfg(not(target_arch = "wasm32"))]
pub use search_index::{
    IndexKey, SEARCH_APPLICATION_ID, SEARCH_INDEX_EXTENSION, SqliteSearchIndex, build_search_index,
    copy_search_index_as, remove_search_indexes, search_index_files, write_search_index,
};
#[cfg(not(target_arch = "wasm32"))]
pub use user::{OpenReport, USER_APPLICATION_ID, USER_SCHEMA_VERSION, UserStore};
