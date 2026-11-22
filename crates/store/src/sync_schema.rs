//! What the User database keeps for Sync (ADR 0022), from migration 12 → 13: which tables and
//! preferences are Synced data, the clock column each synced row carries, tombstones, this
//! Synced device's id and how far each other device's ops are applied.
//!
//! Migration 13 only adds the schema. Nothing writes a clock, a tombstone or `applied_up_to`
//! yet (S27b moves the synced writes onto `apply_changes`, S28a writes the clocks); the only
//! values it fills in are ids: each Layout, Favorite and Reading plan gets a UUIDv7 and the
//! device gets its id.

use rusqlite::{Connection, params};

use crate::annotation::new_id;
use crate::error::StoreError;
use crate::user::sql;

/// The tables of Synced data. Each has a nullable `clock` column: the hybrid logical clock of
/// the row's last change, as text whose encoding Sync defines (S28a). `NULL` means the row
/// was last changed before this device recorded clocks, which is older than any clock.
///
/// `anchors` has none: an Anchor is part of its Annotation's row and goes with its clock.
/// In `preference` only the rows [`is_synced_preference`] names are Synced data; the clock of
/// any other row means nothing.
pub const SYNCED_TABLES: &[&str] = &[
    "notebooks",
    "annotations",
    "highlight_palettes",
    "highlight_styles",
    "reading_plan",
    "plan_reading",
    "favorite",
    "layout",
    "preferred_order",
    "preference",
];

/// The preferences that are Synced data (ADR 0022 rule 4): the active Highlight palette, the
/// Guide arrangements, Copy presets, Saved searches, Collections, Resource tags and Search's
/// Word forms. An entry ending in `/` or `.` names every key that starts with it. A key in
/// neither this list nor [`DEVICE_PREFERENCES`] stays on the device: a preference syncs only
/// if it is listed as one that does.
pub const SYNCED_PREFERENCES: &[&str] = &[
    "highlights.palette",
    "guide-arrangement/",
    "copy.presets",
    "search.saved",
    "library.collections",
    "library.tags",
    "search.word_forms",
];

/// The preferences that stay on each device: display preferences (View settings and fonts,
/// Own Text sizes, the Custom theme, the toolbar, Keyboard shortcuts, Home), the Search
/// history and Recent searches, the Command box's recent entries, Removed Starter Resources,
/// the released Starter library list last fetched and the download settings (the Mobile
/// app: Use cellular data, the Study set offer shown), what belongs to the
/// Resources installed here (Import reports, confirmed Attributions), the app's own settings
/// (updates, Log level, the tour), the Copy verses options last used, the last Notebook used
/// and the Visual filters (they need ids that are the same on every device before they can
/// sync, S27b). Entries ending in `/` or `.` are prefixes, as in [`SYNCED_PREFERENCES`].
pub const DEVICE_PREFERENCES: &[&str] = &[
    "view",
    "view.resource_text_sizes",
    "custom-theme",
    "page-tone-hint",
    "toolbar",
    "key_bindings",
    "home",
    "search.history",
    "search.recent.",
    "command-box.recent",
    "starter-library",
    "starter-library.released",
    "mobile.downloads",
    "import-report/",
    "resource-attribution/",
    "updates",
    "log_level",
    "tour",
    "copy.options",
    "annotations.last_notebook",
    "reading.visual_filters",
    "reading.visual_filters_next_id",
];

/// Whether the preference `key` is Synced data.
pub fn is_synced_preference(key: &str) -> bool {
    listed(SYNCED_PREFERENCES, key)
}

fn listed(list: &[&str], key: &str) -> bool {
    list.iter().any(|entry| {
        if entry.ends_with(['/', '.']) {
            key.starts_with(entry)
        } else {
            key == *entry
        }
    })
}

/// Migration 12 → 13, before [`SCHEMA_13`]: the columns of the new ids, each row given one.
/// Layouts were keyed by their name; Favorites and Reading plans by a number that another
/// device also hands out. The numbers stay as they are (everything that names a Favorite or a
/// Reading plan uses them); the `uid` is what devices know them by.
pub(crate) fn give_rows_ids(c: &Connection) -> Result<(), StoreError> {
    c.execute_batch(
        "ALTER TABLE layout ADD COLUMN id TEXT;
         ALTER TABLE favorite ADD COLUMN uid TEXT;
         ALTER TABLE reading_plan ADD COLUMN uid TEXT;",
    )
    .map_err(sql)?;
    for (table, column) in [
        ("layout", "id"),
        ("favorite", "uid"),
        ("reading_plan", "uid"),
    ] {
        let rows: Vec<i64> = c
            .prepare(&format!("SELECT rowid FROM {table} ORDER BY rowid"))
            .map_err(sql)?
            .query_map([], |r| r.get(0))
            .map_err(sql)?
            .collect::<Result<_, _>>()
            .map_err(sql)?;
        let mut stmt = c
            .prepare(&format!(
                "UPDATE {table} SET {column} = ?1 WHERE rowid = ?2"
            ))
            .map_err(sql)?;
        for rowid in rows {
            stmt.execute(params![new_id(), rowid]).map_err(sql)?;
        }
    }
    Ok(())
}

/// Migration 12 → 13 (ADR 0022 Consequences).
///
/// - `clock` on every table of [`SYNCED_TABLES`], `NULL` on every row there is.
/// - `layout` is keyed by its `id`; its name stays unique (ignoring case), so every Layout
///   keeps its name and everything that finds one by name still does.
/// - `favorite.uid` and `reading_plan.uid` are unique.
/// - `tombstone`: a deleted synced row's table, id and the clock of its delete, kept for good.
/// - `device`: this Synced device's id (a UUIDv7, made by [`make_device`]) and the name the
///   user gives it (empty until then).
/// - `applied_up_to`: for each other Synced device, the last of its op files applied here.
pub(crate) const SCHEMA_13: &str = "
ALTER TABLE preference ADD COLUMN clock TEXT;
ALTER TABLE preferred_order ADD COLUMN clock TEXT;
ALTER TABLE notebooks ADD COLUMN clock TEXT;
ALTER TABLE annotations ADD COLUMN clock TEXT;
ALTER TABLE highlight_palettes ADD COLUMN clock TEXT;
ALTER TABLE highlight_styles ADD COLUMN clock TEXT;
ALTER TABLE favorite ADD COLUMN clock TEXT;
ALTER TABLE reading_plan ADD COLUMN clock TEXT;
ALTER TABLE plan_reading ADD COLUMN clock TEXT;
CREATE UNIQUE INDEX favorite_uid ON favorite (uid);
CREATE UNIQUE INDEX reading_plan_uid ON reading_plan (uid);
CREATE TABLE layout_v13 (
    id       TEXT PRIMARY KEY NOT NULL,
    name     TEXT NOT NULL UNIQUE COLLATE NOCASE,
    snapshot TEXT NOT NULL,
    saved_at INTEGER NOT NULL,
    clock    TEXT
);
INSERT INTO layout_v13 (id, name, snapshot, saved_at)
    SELECT id, name, snapshot, saved_at FROM layout;
DROP TABLE layout;
ALTER TABLE layout_v13 RENAME TO layout;
CREATE TABLE tombstone (
    row_table TEXT NOT NULL,
    row_id    TEXT NOT NULL,
    clock     TEXT NOT NULL,
    PRIMARY KEY (row_table, row_id)
);
CREATE TABLE device (
    id        INTEGER PRIMARY KEY CHECK (id = 1),
    device_id TEXT NOT NULL,
    name      TEXT NOT NULL DEFAULT ''
);
CREATE TABLE applied_up_to (
    device_id TEXT PRIMARY KEY,
    seq       INTEGER NOT NULL
);
";

/// Migration 12 → 13, after [`SCHEMA_13`]: this device's id.
pub(crate) fn make_device(c: &Connection) -> Result<(), StoreError> {
    c.execute(
        "INSERT INTO device (id, device_id) VALUES (1, ?1)",
        [new_id()],
    )
    .map(drop)
    .map_err(sql)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn a_preference_is_synced_only_when_listed() {
        assert!(is_synced_preference("highlights.palette"));
        assert!(is_synced_preference("guide-arrangement/passage"));
        assert!(is_synced_preference("search.word_forms"));
        assert!(!is_synced_preference("guide-arrangement"));
        assert!(!is_synced_preference("view"));
        assert!(!is_synced_preference("search.recent.bible"));
        assert!(!is_synced_preference("a key nobody listed"));
        for key in SYNCED_PREFERENCES {
            assert!(
                !DEVICE_PREFERENCES.contains(key),
                "`{key}` is in both lists"
            );
        }
    }
}
