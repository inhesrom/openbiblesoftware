//! The SQLite side of the rows in `synced.rs` (ADR 0022): reading and writing each kind of
//! [`Row`] that is not an annotation table's, and the tombstones every delete of Synced data
//! leaves. [`UserStore::apply_changes`](crate::UserStore::apply_changes) is the only caller of
//! the writers; the store's own methods (`save_layout`, `add_favorite`, `set_preference` …)
//! plan their edit as [`Change`]s and apply them.

use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension, Row as SqlRow, params};

use crate::annotation::{Change, Row, RowKey};
use crate::error::StoreError;
use crate::preference_items::{ListPreference, diff};
use crate::synced::{
    SyncedFavorite, SyncedLayout, SyncedPreference, SyncedPreferredOrder, SyncedReading,
    SyncedReadingPlan,
};
use crate::user::sql;

/// The clock of a tombstone written before clocks (schema 13, before migration 14): empty
/// text, which sorts before any clock, as a row's `NULL` clock means "changed before this
/// device recorded clocks" (`sync_schema.rs`). The `tombstone.clock` column cannot be
/// `NULL`. From schema 14 every tombstone gets its delete's clock (`hlc.rs`).
pub(crate) const NO_CLOCK: &str = "";

/// Whether the database has table `name` (one opened at an older schema, for a migration
/// test, may not).
pub(crate) fn has_table(c: &Connection, name: &str) -> Result<bool, StoreError> {
    c.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [name],
        |r| r.get(0),
    )
    .map_err(sql)
}

/// A delete leaves a tombstone of the row with the delete's `clock` and, from schema 15, what
/// it `covers` (the clock of the version deleted, `NO_CLOCK` for one from before clocks).
/// From schema 15 a tombstone is kept for good (ADR 0022 rule 3): a row made again (an undone
/// delete, another device's edit the delete did not see) lives beside it, since its clock is
/// later than what the tombstone covers, and a later delete keeps the newer clock and the
/// newer of what they cover, so every device ends with the same tombstone whatever order its
/// deletes and writes arrived in. Before schema 15 a row made again took its tombstone away.
/// An update changes neither.
pub(crate) fn mark_tombstone(
    c: &Connection,
    change: &Change,
    key: &RowKey,
    clock: &str,
    covers: Option<&str>,
) -> Result<(), StoreError> {
    let (kind, id) = key.tombstone();
    match (&change.before, &change.after, covers) {
        (Some(_), None, Some(covers)) => c.execute(
            "INSERT INTO tombstone (row_table, row_id, clock, covers) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (row_table, row_id) DO UPDATE SET
                 clock = max(clock, excluded.clock),
                 covers = max(coalesce(covers, clock), excluded.covers)",
            params![kind, id, clock, covers],
        ),
        (Some(_), None, None) => c.execute(
            "INSERT INTO tombstone (row_table, row_id, clock) VALUES (?1, ?2, ?3)
             ON CONFLICT (row_table, row_id) DO UPDATE SET clock = excluded.clock",
            params![kind, id, clock],
        ),
        (None, Some(_), None) => c.execute(
            "DELETE FROM tombstone WHERE row_table = ?1 AND row_id = ?2",
            params![kind, id],
        ),
        _ => Ok(0),
    }
    .map(drop)
    .map_err(sql)
}

// ------------------------------------------------------------------------- reading

const FAVORITE_COLUMNS: &str = "uid, folder, label, location, position, added_at";

fn favorite_from(r: &SqlRow<'_>) -> rusqlite::Result<SyncedFavorite> {
    Ok(SyncedFavorite {
        uid: r.get(0)?,
        folder: r.get(1)?,
        label: r.get(2)?,
        location: r.get(3)?,
        position: r.get(4)?,
        added_at: r.get(5)?,
    })
}

/// The Favorite with `uid`.
pub(crate) fn read_favorite(
    c: &Connection,
    uid: &str,
) -> Result<Option<SyncedFavorite>, StoreError> {
    c.prepare_cached(&format!(
        "SELECT {FAVORITE_COLUMNS} FROM favorite WHERE uid = ?1"
    ))
    .and_then(|mut s| s.query_row([uid], favorite_from).optional())
    .map_err(sql)
}

/// The Favorite this device numbers `id`.
pub(crate) fn read_favorite_by_id(
    c: &Connection,
    id: i64,
) -> Result<Option<SyncedFavorite>, StoreError> {
    c.prepare_cached(&format!(
        "SELECT {FAVORITE_COLUMNS} FROM favorite WHERE id = ?1"
    ))
    .and_then(|mut s| s.query_row([id], favorite_from).optional())
    .map_err(sql)
}

const PLAN_COLUMNS: &str = "uid, name, template, bible_id, start_day, weekdays, created_at";

fn plan_from(r: &SqlRow<'_>) -> rusqlite::Result<SyncedReadingPlan> {
    Ok(SyncedReadingPlan {
        uid: r.get(0)?,
        name: r.get(1)?,
        template: r.get(2)?,
        bible_id: r.get(3)?,
        start_day: r.get(4)?,
        weekdays: r.get(5)?,
        created_at: r.get(6)?,
    })
}

fn read_plan(c: &Connection, uid: &str) -> Result<Option<SyncedReadingPlan>, StoreError> {
    c.prepare_cached(&format!(
        "SELECT {PLAN_COLUMNS} FROM reading_plan WHERE uid = ?1"
    ))
    .and_then(|mut s| s.query_row([uid], plan_from).optional())
    .map_err(sql)
}

/// The Reading plan this device numbers `id`.
pub(crate) fn read_plan_by_id(
    c: &Connection,
    id: i64,
) -> Result<Option<SyncedReadingPlan>, StoreError> {
    c.prepare_cached(&format!(
        "SELECT {PLAN_COLUMNS} FROM reading_plan WHERE id = ?1"
    ))
    .and_then(|mut s| s.query_row([id], plan_from).optional())
    .map_err(sql)
}

/// The number this device gives the row with `uid` in `table` (`favorite` or
/// `reading_plan`).
pub(crate) fn local_id(c: &Connection, table: &str, uid: &str) -> Result<i64, StoreError> {
    c.query_row(
        &format!("SELECT id FROM {table} WHERE uid = ?1"),
        [uid],
        |r| r.get(0),
    )
    .map_err(sql)
}

const READING_FROM: &str = "SELECT p.uid, r.position, r.passages, r.day, r.read_at
     FROM plan_reading r JOIN reading_plan p ON p.id = r.plan_id";

fn reading_from(r: &SqlRow<'_>) -> rusqlite::Result<SyncedReading> {
    Ok(SyncedReading {
        plan_uid: r.get(0)?,
        position: r.get(1)?,
        passages: r.get(2)?,
        day: r.get(3)?,
        read_at: r.get(4)?,
    })
}

/// Reading `position` of the plan with `plan_uid`.
pub(crate) fn read_reading(
    c: &Connection,
    plan_uid: &str,
    position: u32,
) -> Result<Option<SyncedReading>, StoreError> {
    c.prepare_cached(&format!(
        "{READING_FROM} WHERE p.uid = ?1 AND r.position = ?2"
    ))
    .and_then(|mut s| {
        s.query_row(params![plan_uid, position], reading_from)
            .optional()
    })
    .map_err(sql)
}

/// Every Reading of the plan with `plan_uid`, in order.
pub(crate) fn read_readings(
    c: &Connection,
    plan_uid: &str,
) -> Result<Vec<SyncedReading>, StoreError> {
    let mut stmt = c
        .prepare_cached(&format!(
            "{READING_FROM} WHERE p.uid = ?1 ORDER BY r.position"
        ))
        .map_err(sql)?;
    let rows = stmt.query_map([plan_uid], reading_from).map_err(sql)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(sql)
}

const LAYOUT_COLUMNS: &str = "id, name, snapshot, saved_at";

fn layout_from(r: &SqlRow<'_>) -> rusqlite::Result<SyncedLayout> {
    Ok(SyncedLayout {
        id: r.get(0)?,
        name: r.get(1)?,
        snapshot: r.get(2)?,
        saved_at: r.get(3)?,
    })
}

fn read_layout(c: &Connection, id: &str) -> Result<Option<SyncedLayout>, StoreError> {
    c.prepare_cached(&format!(
        "SELECT {LAYOUT_COLUMNS} FROM layout WHERE id = ?1"
    ))
    .and_then(|mut s| s.query_row([id], layout_from).optional())
    .map_err(sql)
}

/// The Layout named `name` (names ignore case).
pub(crate) fn read_layout_by_name(
    c: &Connection,
    name: &str,
) -> Result<Option<SyncedLayout>, StoreError> {
    c.prepare_cached(&format!(
        "SELECT {LAYOUT_COLUMNS} FROM layout WHERE name = ?1"
    ))
    .and_then(|mut s| s.query_row([name], layout_from).optional())
    .map_err(sql)
}

/// The Preferred order (empty when none is saved).
pub(crate) fn read_preferred_order(c: &Connection) -> Result<SyncedPreferredOrder, StoreError> {
    let mut stmt = c
        .prepare_cached("SELECT resource_id FROM preferred_order ORDER BY rank, resource_id")
        .map_err(sql)?;
    let rows = stmt.query_map([], |r| r.get(0)).map_err(sql)?;
    Ok(SyncedPreferredOrder {
        resource_ids: rows.collect::<Result<Vec<String>, _>>().map_err(sql)?,
    })
}

fn read_preference(c: &Connection, key: &str) -> Result<Option<String>, StoreError> {
    c.prepare_cached("SELECT value FROM preference WHERE key = ?1")
        .and_then(|mut s| s.query_row([key], |r| r.get(0)).optional())
        .map_err(sql)
}

/// The items of the list preferences one `apply_changes` call has touched, with the
/// positions its Changes gave them. A list's value holds only the order of its items, so read
/// again it numbers them 0, 1, 2 …; within one call the positions stay as written, so a
/// list of Changes that moves several items (planned against each other) applies in any
/// order it was planned in, and so does its inverse.
#[derive(Default)]
pub(crate) struct ListItems(HashMap<String, Vec<Row>>);

impl ListItems {
    /// The items of list preference `list`; refused when its value is not in shape (it
    /// changes only whole, as a [`SyncedPreference`]).
    fn get(&mut self, c: &Connection, list: &ListPreference) -> Result<&mut Vec<Row>, StoreError> {
        let key = list.key();
        if !self.0.contains_key(&key) {
            let value = read_preference(c, &key)?;
            let mut items = list.items(value.as_deref()).ok_or_else(|| {
                StoreError::Rejected(format!(
                    "the preference {key} is not a list the app wrote, so it is changed only whole"
                ))
            })?;
            stored_positions(c, &mut items)?;
            self.0.insert(key.clone(), items);
        }
        self.0
            .get_mut(&key)
            .ok_or_else(|| StoreError::Rejected(format!("no items of {key}")))
    }
}

/// Whether table `table` has column `column` (one opened at an older schema may not).
pub(crate) fn has_column(c: &Connection, table: &str, column: &str) -> Result<bool, StoreError> {
    c.query_row(
        "SELECT EXISTS (SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2)",
        params![table, column],
        |r| r.get(0),
    )
    .map_err(sql)
}

/// Give each list item the position its last write gave it, which `item_clock` keeps (from
/// schema 15), in place of its index in the stored value. A list is composed by (position,
/// name), so two devices holding the same last writes of its items compose the same order,
/// whatever order they merged them in. An item with none (written before schema 15, or the
/// list last written whole) keeps its index.
pub(crate) fn stored_positions(c: &Connection, items: &mut [Row]) -> Result<(), StoreError> {
    if items.is_empty() || !has_column(c, "item_clock", "position")? {
        return Ok(());
    }
    let mut stmt = c
        .prepare_cached("SELECT position FROM item_clock WHERE row_table = ?1 AND row_id = ?2")
        .map_err(sql)?;
    for item in items {
        let (kind, id) = item.key().tombstone();
        let position = match item {
            Row::Collection(i) | Row::CopyPreset(i) | Row::SavedSearch(i) => &mut i.position,
            Row::GuideSection(s) => &mut s.position,
            _ => continue,
        };
        let stored: Option<i64> = stmt
            .query_row(params![kind, id], |r| r.get(0))
            .optional()
            .map_err(sql)?
            .flatten();
        if let Some(p) = stored.and_then(|p| u32::try_from(p).ok()) {
            *position = p;
        }
    }
    Ok(())
}

/// A list preference written whole: its items' stored positions no longer say where they
/// are, so they go back to their index in the value.
fn forget_positions(c: &Connection, key: &str) -> Result<(), StoreError> {
    let Some(list) = ListPreference::of_key(key) else {
        return Ok(());
    };
    if !has_column(c, "item_clock", "position")? {
        return Ok(());
    }
    let (kind, prefix) = match &list {
        ListPreference::Collections => ("collection", String::new()),
        ListPreference::CopyPresets => ("copy_preset", String::new()),
        ListPreference::SavedSearches => ("saved_search", String::new()),
        ListPreference::GuideArrangement(guide) => ("guide_section", format!("{guide}/")),
        ListPreference::ResourceTags => return Ok(()),
    };
    c.execute(
        "UPDATE item_clock SET position = NULL
         WHERE row_table = ?1 AND substr(row_id, 1, length(?2)) = ?2",
        params![kind, prefix],
    )
    .map(drop)
    .map_err(sql)
}

/// The row `key` names, of a kind `synced.rs` holds.
pub(crate) fn read_synced_row(
    c: &Connection,
    lists: &mut ListItems,
    key: &RowKey,
) -> Result<Option<Row>, StoreError> {
    Ok(match key {
        RowKey::Favorite(uid) => read_favorite(c, uid)?.map(Row::Favorite),
        RowKey::ReadingPlan(uid) => read_plan(c, uid)?.map(Row::ReadingPlan),
        RowKey::Reading(plan, position) => read_reading(c, plan, *position)?.map(Row::Reading),
        RowKey::Layout(id) => read_layout(c, id)?.map(Row::Layout),
        RowKey::PreferredOrder => Some(Row::PreferredOrder(read_preferred_order(c)?)),
        RowKey::Preference(key) => read_preference(c, key)?.map(|value| {
            Row::Preference(SyncedPreference {
                key: key.clone(),
                value,
            })
        }),
        RowKey::Notebook(_)
        | RowKey::Annotation(_)
        | RowKey::HighlightStyle(_)
        | RowKey::HighlightPalette(_) => None,
        RowKey::Collection(_)
        | RowKey::CopyPreset(_)
        | RowKey::SavedSearch(_)
        | RowKey::GuideSection(..)
        | RowKey::ResourceTags(_) => {
            let Some(list) = ListPreference::of_row(key) else {
                return Ok(None);
            };
            lists
                .get(c, &list)?
                .iter()
                .find(|r| r.key() == *key)
                .cloned()
        }
    })
}

// ------------------------------------------------------------------------- writing

fn mismatched(key: &RowKey) -> StoreError {
    StoreError::Rejected(format!("{} was given another kind of row", key.describe()))
}

/// Write the row `key` names as `after` (`None`: delete it), for a kind `synced.rs` holds.
pub(crate) fn write_synced_row(
    c: &Connection,
    lists: &mut ListItems,
    key: &RowKey,
    after: Option<&Row>,
) -> Result<(), StoreError> {
    match (key, after) {
        (RowKey::Favorite(uid), None) => c
            .execute("DELETE FROM favorite WHERE uid = ?1", [uid])
            .map(drop)
            .map_err(sql),
        (RowKey::Favorite(_), Some(Row::Favorite(f))) => c
            .execute(
                "INSERT INTO favorite (uid, folder, label, location, position, added_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (uid) DO UPDATE SET folder = excluded.folder,
                     label = excluded.label, location = excluded.location,
                     position = excluded.position, added_at = excluded.added_at",
                params![f.uid, f.folder, f.label, f.location, f.position, f.added_at],
            )
            .map(drop)
            .map_err(sql),
        (RowKey::ReadingPlan(uid), None) => {
            if !read_readings(c, uid)?.is_empty() {
                return Err(StoreError::Rejected(format!(
                    "{} still has Readings: delete them first, in the same Change list",
                    key.describe()
                )));
            }
            c.execute("DELETE FROM reading_plan WHERE uid = ?1", [uid])
                .map(drop)
                .map_err(sql)
        }
        (RowKey::ReadingPlan(_), Some(Row::ReadingPlan(p))) => c
            .execute(
                "INSERT INTO reading_plan (uid, name, template, bible_id, start_day, weekdays,
                                           created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT (uid) DO UPDATE SET name = excluded.name,
                     template = excluded.template, bible_id = excluded.bible_id,
                     start_day = excluded.start_day, weekdays = excluded.weekdays,
                     created_at = excluded.created_at",
                params![
                    p.uid,
                    p.name,
                    p.template,
                    p.bible_id,
                    p.start_day,
                    p.weekdays,
                    p.created_at
                ],
            )
            .map(drop)
            .map_err(sql),
        (RowKey::Reading(plan, position), after) => {
            let plan_id: Option<i64> = c
                .query_row("SELECT id FROM reading_plan WHERE uid = ?1", [plan], |r| {
                    r.get(0)
                })
                .optional()
                .map_err(sql)?;
            let Some(plan_id) = plan_id else {
                return Err(StoreError::Rejected(format!(
                    "there is no Reading plan {plan}"
                )));
            };
            match after {
                None => c
                    .execute(
                        "DELETE FROM plan_reading WHERE plan_id = ?1 AND position = ?2",
                        params![plan_id, position],
                    )
                    .map(drop)
                    .map_err(sql),
                Some(Row::Reading(r)) => c
                    .execute(
                        "INSERT INTO plan_reading (plan_id, position, passages, day, read_at)
                         VALUES (?1, ?2, ?3, ?4, ?5)
                         ON CONFLICT (plan_id, position) DO UPDATE SET
                             passages = excluded.passages, day = excluded.day,
                             read_at = excluded.read_at",
                        params![plan_id, r.position, r.passages, r.day, r.read_at],
                    )
                    .map(drop)
                    .map_err(sql),
                Some(_) => Err(mismatched(key)),
            }
        }
        (RowKey::Layout(id), None) => c
            .execute("DELETE FROM layout WHERE id = ?1", [id])
            .map(drop)
            .map_err(sql),
        (RowKey::Layout(_), Some(Row::Layout(l))) => c
            .execute(
                "INSERT INTO layout (id, name, snapshot, saved_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (id) DO UPDATE SET name = excluded.name,
                     snapshot = excluded.snapshot, saved_at = excluded.saved_at",
                params![l.id, l.name, l.snapshot, l.saved_at],
            )
            .map(drop)
            .map_err(sql),
        (RowKey::PreferredOrder, None) => Err(StoreError::Rejected(
            "the Preferred order is replaced, never deleted".into(),
        )),
        (RowKey::PreferredOrder, Some(Row::PreferredOrder(order))) => {
            // One value (ADR 0022 rule 3): its rows are replaced, and leave no tombstones.
            c.execute("DELETE FROM preferred_order", []).map_err(sql)?;
            let mut stmt = c
                .prepare_cached("INSERT INTO preferred_order (resource_id, rank) VALUES (?1, ?2)")
                .map_err(sql)?;
            for (rank, id) in order.resource_ids.iter().enumerate() {
                let rank = i64::try_from(rank).unwrap_or(i64::MAX);
                stmt.execute(params![id, rank]).map_err(sql)?;
            }
            Ok(())
        }
        (RowKey::Preference(key), None) => {
            lists.0.remove(key);
            forget_positions(c, key)?;
            c.execute("DELETE FROM preference WHERE key = ?1", [key])
                .map(drop)
                .map_err(sql)
        }
        (RowKey::Preference(_), Some(Row::Preference(p))) => {
            lists.0.remove(&p.key);
            forget_positions(c, &p.key)?;
            write_preference(c, &p.key, &p.value)
        }
        (
            RowKey::Collection(_)
            | RowKey::CopyPreset(_)
            | RowKey::SavedSearch(_)
            | RowKey::GuideSection(..)
            | RowKey::ResourceTags(_),
            after,
        ) => {
            let Some(list) = ListPreference::of_row(key) else {
                return Err(mismatched(key));
            };
            let items = lists.get(c, &list)?;
            items.retain(|r| r.key() != *key);
            items.extend(after.cloned());
            let value = list.compose(items);
            write_preference(c, &list.key(), &value)
        }
        _ => Err(mismatched(key)),
    }
}

/// Write preference `key`. Only `apply_changes` writes a synced one; one that is not Synced
/// data stays on this device and is no [`Change`].
pub(crate) fn write_preference(c: &Connection, key: &str, value: &str) -> Result<(), StoreError> {
    c.execute(
        "INSERT INTO preference (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )
    .map(drop)
    .map_err(sql)
}

// ------------------------------------------------------------------------- planning

/// The Changes that set synced preference `key` to `value`: one per item of a list
/// preference whose old and new values are both in shape, else one for the whole value.
/// None when it already holds `value`.
pub(crate) fn preference_changes(
    c: &Connection,
    key: &str,
    value: &str,
) -> Result<Vec<Change>, StoreError> {
    let old = read_preference(c, key)?;
    if old.as_deref() == Some(value) {
        return Ok(Vec::new());
    }
    if let Some(list) = ListPreference::of_key(key)
        && let (Some(mut before), Some(after)) =
            (list.items(old.as_deref()), list.items(Some(value)))
    {
        // The items as `apply_changes` reads them, at their stored positions; an item the
        // new value puts elsewhere (or whose stored position a merge tied with another's) gets
        // its index.
        stored_positions(c, &mut before)?;
        let changes = diff(&before, &after);
        // An empty list written where there was no value has no items to add: it is
        // written whole, so the value is there as the engine wrote it.
        if !changes.is_empty() {
            return Ok(changes);
        }
    }
    let row = |value: String| {
        Row::Preference(SyncedPreference {
            key: key.to_string(),
            value,
        })
    };
    Ok(vec![Change {
        before: old.map(row),
        after: Some(row(value.to_string())),
    }])
}

/// `ids` without repeats, the first of each kept: the Preferred order as it is stored.
pub(crate) fn preferred_order_of(ids: &[String]) -> SyncedPreferredOrder {
    let mut seen = HashSet::new();
    SyncedPreferredOrder {
        resource_ids: ids
            .iter()
            .filter(|id| seen.insert(id.as_str()))
            .cloned()
            .collect(),
    }
}
