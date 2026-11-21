//! The rows of Synced data outside the annotation tables (ADR 0022 rule 4), as
//! [`Row`](crate::Row)s: Favorites, Reading plans and their Readings, Layouts, the Preferred
//! order and the synced preferences. Every write of them is a [`Change`](crate::Change)
//! applied by `UserStore::apply_changes`, so Sync has one stream of edits to record.
//!
//! A row is known by an id that is the same on every device: a Favorite's and a Reading plan's
//! `uid`, a Layout's `id`. The integer ids the app names Favorites and Reading plans by are
//! this device's own and are not part of the row.
//!
//! The preferences that hold a list (Collections, Copy presets, Saved searches, Resource tags
//! and each Guide arrangement) are rows item by item, so two devices adding one each keep
//! both (ADR 0006). They are still kept as one JSON value per preference key: an item row
//! carries the item's JSON exactly as the engine wrote it, and applying it rewrites the
//! value byte for byte as the engine would. A value that is not such a list (damaged, edited
//! by hand, or from a later version) is one whole-value [`SyncedPreference`] row instead,
//! so nothing in it is lost.
//!
//! These types compile everywhere (wasm included); `synced_store.rs` is the SQLite side.

use serde::{Deserialize, Serialize};

/// A Favorite (#155).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncedFavorite {
    /// UUIDv7 (`favorite.uid`).
    pub uid: String,
    /// Its Favorites folder; empty when it is in none.
    pub folder: String,
    pub label: String,
    /// Where it goes: the engine's JSON.
    pub location: String,
    /// Its place among the Favorites: they list in this order within a folder.
    pub position: i64,
    /// Unix seconds.
    pub added_at: i64,
}

/// A Reading plan (#147), without its Readings (each is a [`SyncedReading`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncedReadingPlan {
    /// UUIDv7 (`reading_plan.uid`).
    pub uid: String,
    pub name: String,
    pub template: String,
    pub bible_id: String,
    /// `YYYY-MM-DD`.
    pub start_day: String,
    /// Bit 0 Monday … bit 6 Sunday.
    pub weekdays: u8,
    /// Unix seconds.
    pub created_at: i64,
}

/// One Reading of a Reading plan, with its done-mark: Readings merge one by one
/// (ADR 0022 rule 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncedReading {
    /// The `uid` of its Reading plan.
    pub plan_uid: String,
    /// Its place in the plan, from 0.
    pub position: u32,
    /// Its Passages, as a Bible reference in the Standard versification.
    pub passages: String,
    /// The day it is due, `YYYY-MM-DD`.
    pub day: String,
    /// When it was marked read (Unix seconds), if it has been.
    pub read_at: Option<i64>,
}

/// A named Layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncedLayout {
    /// UUIDv7 (`layout.id`).
    pub id: String,
    /// Unique among the Layouts, ignoring case.
    pub name: String,
    /// The engine's Workspace snapshot JSON.
    pub snapshot: String,
    /// Unix seconds.
    pub saved_at: i64,
}

/// The whole Preferred order, one value (ADR 0022 rule 3): replacing it is one row's update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncedPreferredOrder {
    /// Resource ids, best first, each once.
    pub resource_ids: Vec<String>,
}

/// A synced preference's whole value: one that is not a list of items, or a list
/// preference whose value is not in its list's shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncedPreference {
    pub key: String,
    pub value: String,
}

/// One named item of a list preference: a Collection (`library.collections`), a Copy preset
/// (`copy.presets`) or a Saved search (`search.saved`), known by its name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncedListItem {
    /// Its `name`, exactly as written.
    pub name: String,
    /// Its place in the list, from 0.
    pub position: u32,
    /// The item's JSON as the engine wrote it.
    pub json: String,
}

/// One section of a Guide arrangement (`guide-arrangement/<guide>`): its place, and whether
/// it is collapsed, hidden or filtered to a Collection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncedGuideSection {
    /// The kind of Guide: the key after `guide-arrangement/`.
    pub guide: String,
    /// The section's `id`.
    pub section: String,
    /// Its place in the Guide, from 0.
    pub position: u32,
    /// The section's JSON as the engine wrote it.
    pub json: String,
}

/// One Resource's Resource tags (`library.tags`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncedResourceTags {
    pub resource_id: String,
    /// Its tags' JSON as the engine wrote it.
    pub json: String,
}
