//! The User database's side of Sync's merge (ADR 0022 rule 3, plan S28b): another device's
//! op file is merged in one transaction, together with noting it applied
//! ([`UserStore::merge_op_file`]). The rules themselves (last writer wins, edit beats delete,
//! two default Notebooks, Layout names …) are the engine's (`obs_engine::sync::merge`); this
//! module gives them what they read and write through a [`MergeTx`]:
//!
//! - [`MergeTx::state`]: a row as it is here, its clock, and its tombstone;
//! - [`MergeTx::write_merged`]: a row written with the op's own clock (not a new one), never
//!   kept in the outbox, since the op is already in its device's op file;
//! - [`MergeTx::note_delete`]: a delete of a row that is not here, remembered in its
//!   tombstone so a write it covers that arrives later does not bring the row back;
//! - [`MergeTx::write_kept`]: a row rewritten as every device rewrites it, keeping its clock;
//! - [`MergeTx::record`]: a change this device makes while merging (a rename for a name
//!   clash, a Sync conflict's copy, a deleted parent restored), applied like any other with
//!   the next clock and kept in the outbox;
//! - [`MergeTx::hold`]: an op this device cannot apply, kept in `sync_held` and tried again
//!   at the next sync ([`UserStore::merge_held`]), so the rest of its file merges;
//! - [`MergeTx::write_list_base`]: a list preference written whole, the base its items' later
//!   ops go on top of (D7).
//!
//! [`MergeTx::begin_op`] and [`MergeTx::end_op`] put each op in a savepoint, so one that fails
//! leaves nothing of itself behind.

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::annotation::{Change, Notebook, Row, RowKey};
use crate::annotation_store::read_default_notebook;
use crate::error::StoreError;
use crate::hlc::Hlc;
use crate::preference_items::{ListPreference, renamed_item};
use crate::sync_clock::{
    HELD_SCHEMA, Stamp, read_device_id, read_list_base, read_row_clock, records_ops,
};
use crate::synced::SyncedLayout;
use crate::synced_store::{ListItems, read_layout_by_name, read_preference, stored_positions};
use crate::user::{UserStore, sql};

/// A row's tombstone: the clock of its delete, and the clock of the newest version a delete
/// of it removed (`None`: only a version from before clocks).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tombstone {
    pub clock: Option<Hlc>,
    pub covers: Option<Hlc>,
}

/// A row of Synced data as it is in this User database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowState {
    /// The row, if it is here.
    pub row: Option<crate::annotation::Row>,
    /// Its clock (`None`: last changed before clocks, or not here).
    pub clock: Option<Hlc>,
    /// Its tombstone, if it was deleted (a row made again has none).
    pub tombstone: Option<Tombstone>,
}

/// One op of another device that this device could not apply (`sync_held`, schema 16): a
/// synced preference a later app added, an item of a list whose value is not in its list's
/// shape, a child of a parent deleted before schema 16 (whose tombstone has no row). It is
/// tried again at every sync and listed in Needs attention until it applies or a later write
/// supersedes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldOp {
    /// The op's clock (its device's id is in it).
    pub clock: Hlc,
    pub change: Change,
    /// The clock its `before` had on its device (what a delete covers).
    pub before_clock: Option<Hlc>,
    /// The top-level fields of `after` its op line did not carry (an older app's).
    pub absent: Vec<String>,
    /// Why it is held, in words.
    pub why: String,
}

/// A `sync_held` row's `change` JSON.
#[derive(Serialize, Deserialize)]
struct HeldChange {
    before: Option<Row>,
    after: Option<Row>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    before_clock: Option<Hlc>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    absent: Vec<String>,
    why: String,
}

/// The row a held op is filed under: the row its Change is about, else its first side's.
fn held_key(change: &Change) -> (&'static str, String) {
    change
        .key()
        .or_else(|| {
            change
                .after
                .as_ref()
                .or(change.before.as_ref())
                .map(Row::key)
        })
        .map_or(("", String::new()), |k| k.tombstone())
}

/// The list preference key of a list's row: an item's (a Collection …) or the list's whole
/// value (`RowKey::Preference` of a list key); `None` for any other row.
pub fn list_key_of(key: &RowKey) -> Option<String> {
    match key {
        RowKey::Preference(k) => ListPreference::of_key(k).map(|l| l.key()),
        _ => ListPreference::of_row(key).map(|l| l.key()),
    }
}

/// One op file's merge, in one transaction ([`UserStore::merge_op_file`]).
pub struct MergeTx<'a> {
    store: &'a UserStore,
    tx: &'a Connection,
    lists: ListItems,
    device_id: String,
    /// This device's id and its former ones (`sync_device.rs`): a clock naming any of them is
    /// this device's own.
    own_ids: Vec<String>,
    recording: bool,
    /// The Changes [`MergeTx::record`] applied, for the test-support recorder.
    #[cfg(feature = "test-support")]
    pub(crate) recorded: Vec<Change>,
    /// Where `recorded` stood at each open savepoint ([`MergeTx::begin_op`]).
    #[cfg(feature = "test-support")]
    marks: Vec<usize>,
}

impl MergeTx<'_> {
    /// This device's id.
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    /// The connection the merge writes through.
    pub(crate) fn conn(&self) -> &Connection {
        self.tx
    }

    /// Read the list preferences' items again at their next use (they were written past
    /// [`ListItems`]).
    pub(crate) fn forget_lists(&mut self) {
        self.lists = ListItems::default();
    }

    /// Whether a clock naming `device` is this device's own: its id, or one this database had
    /// before a restore or a copy gave it a new one (ADR 0022 rule 7).
    pub fn is_own(&self, device: &str) -> bool {
        self.own_ids.iter().any(|id| id == device)
    }

    /// The name the user gave this device (empty until then).
    pub fn device_name(&self) -> Result<String, StoreError> {
        self.tx
            .query_row("SELECT name FROM device WHERE id = 1", [], |r| r.get(0))
            .map_err(sql)
    }

    /// The row `key` names as it is here, with its clock and tombstone.
    pub fn state(&mut self, key: &RowKey) -> Result<RowState, StoreError> {
        // An item of a list whose value is not in its shape is not there as a row: the value
        // is one whole row then (D7).
        let whole = match ListPreference::of_row(key) {
            Some(list) => list
                .items(read_preference(self.tx, &list.key())?.as_deref())
                .is_none(),
            None => false,
        };
        let row = if whole {
            None
        } else {
            crate::annotation_store::read_row(self.tx, &mut self.lists, key)?
        };
        let clock = match row {
            Some(_) => read_row_clock(self.tx, key)?
                .as_deref()
                .and_then(Hlc::parse),
            None => None,
        };
        let (kind, id) = key.tombstone();
        let tombstone = self
            .tx
            .prepare_cached(
                "SELECT clock, covers FROM tombstone WHERE row_table = ?1 AND row_id = ?2",
            )
            .and_then(|mut s| {
                s.query_row(params![kind, id], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
                })
                .optional()
            })
            .map_err(sql)?
            .map(|(clock, covers)| {
                let clock = Hlc::parse(&clock);
                Tombstone {
                    // A tombstone from before schema 15 covers up to its own clock.
                    covers: match covers {
                        Some(covers) => Hlc::parse(&covers),
                        None => clock.clone(),
                    },
                    clock,
                }
            });
        Ok(RowState {
            row,
            clock,
            tombstone,
        })
    }

    /// Write `change` (planned from [`MergeTx::state`], so its `before` is the row here) with
    /// another device's op `clock`; a delete covers `covers`. Not kept in the outbox.
    pub fn write_merged(
        &mut self,
        change: &Change,
        clock: &Hlc,
        covers: Option<&Hlc>,
    ) -> Result<(), StoreError> {
        let columns = self.store.sync_columns(self.tx)?;
        self.store.apply_one(
            self.tx,
            &mut self.lists,
            columns,
            change,
            &Stamp::Merged {
                clock,
                covers,
                deleted: None,
            },
            true,
        )
    }

    /// Delete the row here (`here`, as [`MergeTx::state`] read it) for another device's op at
    /// `clock` covering `covers`, which deleted it as `deleted` (its `before`): a parent's
    /// tombstone keeps that row, as on the device that deleted it, so every device keeps the
    /// same whatever version it had.
    pub fn delete_merged(
        &mut self,
        here: &Row,
        deleted: Option<&Row>,
        clock: &Hlc,
        covers: Option<&Hlc>,
    ) -> Result<(), StoreError> {
        let columns = self.store.sync_columns(self.tx)?;
        self.store.apply_one(
            self.tx,
            &mut self.lists,
            columns,
            &Change::delete(here.clone()),
            &Stamp::Merged {
                clock,
                covers,
                deleted,
            },
            true,
        )
    }

    /// Remember a delete (at `clock`, covering `covers`) of the row `key` names that removes
    /// nothing here: the row is not here, or is a later version than the delete covers (edit
    /// beats delete) and stays beside the tombstone. The tombstone keeps the newer delete
    /// clock and the newer of what they cover, as every device's does.
    pub fn note_delete(
        &mut self,
        key: &RowKey,
        clock: &Hlc,
        covers: Option<&Hlc>,
        row: Option<&Row>,
    ) -> Result<(), StoreError> {
        // A parent's tombstone keeps the row deleted (schema 16), as `apply_changes` does.
        let row = match row {
            Some(row) if crate::synced_store::keeps_row(key) => Some(
                serde_json::to_string(row)
                    .map_err(|e| StoreError::Rejected(format!("a row that is not JSON: {e}")))?,
            ),
            _ => None,
        };
        let delete_clock = clock.encode()?;
        let delete_covers = covers.map(Hlc::encode).transpose()?.unwrap_or_default();
        let keep_row = match &row {
            Some(row) => crate::synced_store::tombstone_row_wins(
                self.tx,
                key,
                &delete_clock,
                &delete_covers,
            )?
            .then_some(row.as_str()),
            None => None,
        };
        let old = self.state(key)?.tombstone;
        let clock = old
            .as_ref()
            .and_then(|t| t.clock.clone())
            .max(Some(clock.clone()));
        let covers = old
            .as_ref()
            .and_then(|t| t.covers.clone())
            .max(covers.cloned());
        let text = |c: Option<Hlc>| c.map(|c| c.encode()).transpose();
        let (kind, id) = key.tombstone();
        self.tx
            .execute(
                "INSERT INTO tombstone (row_table, row_id, clock, covers) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (row_table, row_id) DO UPDATE SET clock = excluded.clock,
                     covers = excluded.covers",
                params![
                    kind,
                    id,
                    text(clock)?.unwrap_or_default(),
                    text(covers)?.unwrap_or_default()
                ],
            )
            .map_err(sql)?;
        if let Some(row) = keep_row {
            crate::synced_store::keep_tombstone_row(
                self.tx,
                key,
                row,
                &delete_covers,
                &delete_clock,
            )?;
        }
        Ok(())
    }

    /// Note that another device's op at `clock` touched the list item `key` names (whatever
    /// it did here): the list's `preference.clock` is the newest clock of any op on its items,
    /// and the list has a value (empty, if no item is here) once any op on one has been
    /// merged, so every device holds the same however its ops arrived. Nothing for a row
    /// that is not a list item.
    pub fn note_item_op(&mut self, key: &RowKey, clock: &Hlc) -> Result<(), StoreError> {
        let Some(list) = ListPreference::of_row(key) else {
            return Ok(());
        };
        let clock = clock.encode()?;
        self.tx
            .execute(
                "INSERT INTO preference (key, value, clock) VALUES (?1, ?2, ?3)
                 ON CONFLICT (key) DO UPDATE SET clock = max(coalesce(clock, ''), excluded.clock)",
                params![list.key(), list.compose(&[]), clock],
            )
            .map(drop)
            .map_err(sql)
    }

    /// Note that another device's op at `clock` on list preference `list`'s whole value was
    /// superseded by its base here: the list's `preference.clock` is still the newest clock of
    /// any op on it, as on a device that merged that op first.
    pub fn note_list_op(&mut self, list: &str, clock: &Hlc) -> Result<(), StoreError> {
        self.tx
            .execute(
                "UPDATE preference SET clock = max(coalesce(clock, ''), ?1) WHERE key = ?2",
                params![clock.encode()?, list],
            )
            .map(drop)
            .map_err(sql)
    }

    /// Rewrite a row (an update) the way every device rewrites it from what they all merged,
    /// keeping its clock: not an edit of its own, so not kept in the outbox. The default
    /// Notebook may stop being the default.
    pub fn write_kept(&mut self, change: &Change) -> Result<(), StoreError> {
        let columns = self.store.sync_columns(self.tx)?;
        self.store.apply_one(
            self.tx,
            &mut self.lists,
            columns,
            change,
            &Stamp::Kept,
            true,
        )
    }

    /// Apply `changes` as this device's own edit (the next clocks, kept in the outbox while it
    /// records ops): what the merge decides here for every device, such as a rename for a name
    /// clash. The default Notebook may stop being the default.
    pub fn record(&mut self, changes: &[Change]) -> Result<(), StoreError> {
        let columns = self.store.sync_columns(self.tx)?;
        let stamp = Stamp::Local {
            recording: self.recording,
        };
        for change in changes {
            self.store
                .apply_one(self.tx, &mut self.lists, columns, change, &stamp, true)?;
        }
        #[cfg(feature = "test-support")]
        self.recorded.extend_from_slice(changes);
        Ok(())
    }

    /// The default Notebook.
    pub fn default_notebook(&self) -> Result<Option<Notebook>, StoreError> {
        read_default_notebook(self.tx)
    }

    /// The Layout named `name` (names ignore case).
    pub fn layout_named(&self, name: &str) -> Result<Option<SyncedLayout>, StoreError> {
        read_layout_by_name(self.tx, name)
    }

    /// Whether the row `key` names has rows here that need it: a Notebook its Annotations, a
    /// Highlight palette its styles, a Highlight style its Highlights, a Reading plan its
    /// Readings.
    pub fn has_children(&self, key: &RowKey) -> Result<bool, StoreError> {
        let query = match key {
            RowKey::Notebook(_) => {
                "SELECT EXISTS (SELECT 1 FROM annotations WHERE notebook_id = ?1)"
            }
            RowKey::HighlightPalette(_) => {
                "SELECT EXISTS (SELECT 1 FROM highlight_styles WHERE palette_id = ?1)"
            }
            RowKey::HighlightStyle(_) => {
                "SELECT EXISTS (SELECT 1 FROM annotations WHERE kind = 'highlight' AND style = ?1)"
            }
            RowKey::ReadingPlan(_) => {
                "SELECT EXISTS (SELECT 1 FROM plan_reading r JOIN reading_plan p
                     ON p.id = r.plan_id WHERE p.uid = ?1)"
            }
            _ => return Ok(false),
        };
        let (_, id) = key.tombstone();
        self.tx.query_row(query, [id], |r| r.get(0)).map_err(sql)
    }
}

impl MergeTx<'_> {
    /// Open a savepoint for one op: [`MergeTx::end_op`] keeps or undoes what it wrote.
    pub fn begin_op(&mut self) -> Result<(), StoreError> {
        self.tx.execute_batch("SAVEPOINT merge_op").map_err(sql)?;
        #[cfg(feature = "test-support")]
        self.marks.push(self.recorded.len());
        Ok(())
    }

    /// Close the savepoint [`MergeTx::begin_op`] opened, keeping what the op wrote, or (not
    /// `keep`) undoing all of it.
    pub fn end_op(&mut self, keep: bool) -> Result<(), StoreError> {
        #[cfg(feature = "test-support")]
        let mark = self.marks.pop().unwrap_or(self.recorded.len());
        if keep {
            return self.tx.execute_batch("RELEASE merge_op").map_err(sql);
        }
        self.tx
            .execute_batch("ROLLBACK TO merge_op; RELEASE merge_op")
            .map_err(sql)?;
        self.lists = ListItems::default();
        #[cfg(feature = "test-support")]
        self.recorded.truncate(mark);
        Ok(())
    }

    /// Keep `op` in `sync_held`, to be tried again at the next sync (an op held again
    /// replaces itself). Of a row's held writes only the newest is kept: it supersedes the
    /// older by last writer wins once it applies, so a row a later app keeps writing holds one
    /// op, not one per write. A held delete is always kept (it notes its tombstone when it
    /// applies), and so is a write whose op line lacked fields (the older write may hold them).
    pub fn hold(&mut self, op: &HeldOp) -> Result<(), StoreError> {
        let (kind, id) = held_key(&op.change);
        if op.change.after.is_some() && op.absent.is_empty() {
            for other in read_held_of(self.tx, kind, &id)? {
                if other.change.after.is_none() || other.clock == op.clock {
                    continue;
                }
                if other.clock > op.clock {
                    if other.absent.is_empty() {
                        return Ok(());
                    }
                } else {
                    self.unhold(&other)?;
                }
            }
        }
        let change = serde_json::to_string(&HeldChange {
            before: op.change.before.clone(),
            after: op.change.after.clone(),
            before_clock: op.before_clock.clone(),
            absent: op.absent.clone(),
            why: op.why.clone(),
        })
        .map_err(|e| StoreError::Rejected(format!("a change that is not JSON: {e}")))?;
        self.tx
            .execute(
                "INSERT INTO sync_held (row_table, row_id, clock, change) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (row_table, row_id, clock) DO UPDATE SET change = excluded.change",
                params![kind, id, op.clock.encode()?, change],
            )
            .map(drop)
            .map_err(sql)
    }

    /// Whether an op of the row `key` names is held here.
    pub fn is_held(&self, key: &RowKey) -> Result<bool, StoreError> {
        let (kind, id) = key.tombstone();
        self.tx
            .prepare_cached(
                "SELECT EXISTS (SELECT 1 FROM sync_held WHERE row_table = ?1 AND row_id = ?2)",
            )
            .and_then(|mut s| s.query_row(params![kind, id], |r| r.get(0)))
            .map_err(sql)
    }

    /// `op` (held before) is applied or superseded: it leaves `sync_held`.
    pub fn unhold(&mut self, op: &HeldOp) -> Result<(), StoreError> {
        let (kind, id) = held_key(&op.change);
        self.tx
            .execute(
                "DELETE FROM sync_held WHERE row_table = ?1 AND row_id = ?2 AND clock = ?3",
                params![kind, id, op.clock.encode()?],
            )
            .map(drop)
            .map_err(sql)
    }

    /// The ops held here, by clock: all of them, or those of the items of list preference
    /// `list` (its key).
    pub fn held(&self, list: Option<&str>) -> Result<Vec<HeldOp>, StoreError> {
        read_held(self.tx, list)
    }

    /// The clock of list preference `list`'s base (its newest whole-value write), if any.
    pub fn list_base(&self, list: &str) -> Result<Option<Hlc>, StoreError> {
        match ListPreference::of_key(list) {
            Some(l) => read_list_base(self.tx, &l),
            None => Ok(None),
        }
    }

    /// Whether list preference `list`'s value here is in its list's shape (no value is: an
    /// empty list), so its items can be read and written one by one.
    pub fn list_in_shape(&self, list: &str) -> Result<bool, StoreError> {
        let Some(l) = ListPreference::of_key(list) else {
            return Ok(false);
        };
        Ok(l.items(read_preference(self.tx, list)?.as_deref())
            .is_some())
    }

    /// Write another device's whole value of a list preference (`change`, planned from
    /// [`MergeTx::state`], or its delete) at `clock`, later than the list's base here: it is
    /// the new base (ADR 0022 rule 3, D7). The items here changed after it (by ops later than
    /// `clock`) go on top of it: an item written later is written again into it, and one
    /// deleted later by a delete that covered the base's version goes from it. When the new
    /// value is not in its list's shape, the items written later are held instead
    /// ([`MergeTx::hold`]) until a value in shape comes. Every device thus holds the base and
    /// the later item ops, whatever order they arrived in. The items held before are the
    /// engine's to try again.
    pub fn write_list_base(
        &mut self,
        change: &Change,
        clock: &Hlc,
        covers: Option<&Hlc>,
    ) -> Result<(), StoreError> {
        let Some(RowKey::Preference(key)) = change.key() else {
            return Err(StoreError::Rejected("not a list preference's value".into()));
        };
        let Some(list) = ListPreference::of_key(&key) else {
            return Err(StoreError::Rejected(format!(
                "{key} is not a list preference"
            )));
        };
        let base = clock.encode()?;
        let item_clock = |row: &Row| -> Result<Option<String>, StoreError> {
            let (kind, id) = row.key().tombstone();
            self.tx
                .prepare_cached("SELECT clock FROM item_clock WHERE row_table = ?1 AND row_id = ?2")
                .and_then(|mut s| s.query_row(params![kind, id], |r| r.get(0)).optional())
                .map_err(sql)
        };
        // The items written after the base, as they are here (only a list in shape has any).
        let mut later = Vec::new();
        if let Some(mut items) = list.items(read_preference(self.tx, &key)?.as_deref()) {
            stored_positions(self.tx, &mut items)?;
            for item in items {
                if let Some(at) = item_clock(&item)?.filter(|c| *c > base) {
                    let at = Hlc::parse(&at)
                        .ok_or_else(|| StoreError::Corrupt(format!("item clock `{at}`")))?;
                    later.push((item, at));
                }
            }
        }
        // The items deleted after the base by deletes that covered its version.
        let (kind, prefix) = list.item_rows();
        let deleted: Vec<(String, String, String)> = self
            .tx
            .prepare(
                "SELECT row_id, clock, coalesce(covers, clock) FROM tombstone
                 WHERE row_table = ?1 AND substr(row_id, 1, length(?2)) = ?2 AND clock > ?3
                     AND coalesce(covers, clock) >= ?3
                 ORDER BY row_id",
            )
            .and_then(|mut s| {
                s.query_map(params![kind, prefix, base], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?
                .collect()
            })
            .map_err(sql)?;
        self.write_merged(change, clock, covers)?;
        let in_shape = match &change.after {
            Some(Row::Preference(p)) => list.items(Some(&p.value)).is_some(),
            _ => true,
        };
        if !in_shape {
            for (item, at) in later {
                self.hold(&HeldOp {
                    clock: at,
                    change: Change::update(item.clone(), item.clone()),
                    before_clock: None,
                    absent: Vec::new(),
                    why: format!("the value of {key} is not a list this app reads"),
                })?;
                let (kind, id) = item.key().tombstone();
                self.tx
                    .execute(
                        "DELETE FROM item_clock WHERE row_table = ?1 AND row_id = ?2",
                        params![kind, id],
                    )
                    .map_err(sql)?;
            }
            return Ok(());
        }
        for (row_id, at, covered) in deleted {
            let Some(item) = list.item_key(&row_id) else {
                continue;
            };
            let Some(here) = self.state(&item)?.row else {
                continue;
            };
            let (Some(at), covered) = (Hlc::parse(&at), Hlc::parse(&covered)) else {
                continue;
            };
            self.write_merged(&Change::delete(here), &at, covered.as_ref())?;
        }
        for (item, at) in later {
            let here = self.state(&item.key())?.row;
            self.write_merged(
                &Change {
                    before: here,
                    after: Some(item),
                },
                &at,
                None,
            )?;
        }
        Ok(())
    }

    /// The row the tombstone of `key` keeps (a deleted parent's, schema 16), if any.
    pub fn tombstone_row(&self, key: &RowKey) -> Result<Option<Row>, StoreError> {
        let (kind, id) = key.tombstone();
        let row: Option<String> = self
            .tx
            .prepare_cached("SELECT row FROM tombstone WHERE row_table = ?1 AND row_id = ?2")
            .and_then(|mut s| s.query_row(params![kind, id], |r| r.get(0)).optional())
            .map_err(sql)?
            .flatten();
        row.map(|row| {
            serde_json::from_str(&row)
                .map_err(|e| StoreError::Corrupt(format!("the row of a tombstone: {e}")))
        })
        .transpose()
    }

    /// List item `row` (a Collection, Copy preset or Saved search) under the first of
    /// "`name` (2)", "`name` (3)" … that no item of its list here has, nor holds an op of
    /// (an item held while its list's value was damaged comes back under its name).
    pub fn renamed_for_clash(&mut self, row: &Row) -> Result<Option<Row>, StoreError> {
        let name = match row {
            Row::Collection(i) | Row::CopyPreset(i) | Row::SavedSearch(i) => i.name.clone(),
            _ => return Ok(None),
        };
        for n in 2u32..10_000 {
            let candidate = format!("{name} ({n})");
            let Some(renamed) = renamed_item(row, &candidate) else {
                return Ok(None);
            };
            if self.state(&renamed.key())?.row.is_none() && !self.is_held(&renamed.key())? {
                return Ok(Some(renamed));
            }
        }
        Ok(None)
    }
}

/// The ops held in `sync_held` for the row `kind` / `id` names, by clock.
fn read_held_of(c: &Connection, kind: &str, id: &str) -> Result<Vec<HeldOp>, StoreError> {
    let rows: Vec<(String, String)> = c
        .prepare_cached(
            "SELECT clock, change FROM sync_held WHERE row_table = ?1 AND row_id = ?2
             ORDER BY clock",
        )
        .and_then(|mut s| {
            s.query_map(params![kind, id], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect()
        })
        .map_err(sql)?;
    rows.into_iter()
        .map(|(clock, change)| held_from(&clock, &change))
        .collect()
}

/// A `sync_held` row's op, from its `clock` and `change` columns.
fn held_from(clock: &str, change: &str) -> Result<HeldOp, StoreError> {
    let held: HeldChange =
        serde_json::from_str(change).map_err(|e| StoreError::Corrupt(format!("a held op: {e}")))?;
    Ok(HeldOp {
        clock: Hlc::parse(clock)
            .ok_or_else(|| StoreError::Corrupt(format!("held clock `{clock}`")))?,
        change: Change {
            before: held.before,
            after: held.after,
        },
        before_clock: held.before_clock,
        absent: held.absent,
        why: held.why,
    })
}

/// The ops held in `sync_held`, by clock: all, or those of list preference `list`'s items.
fn read_held(c: &Connection, list: Option<&str>) -> Result<Vec<HeldOp>, StoreError> {
    let (kind, prefix) = match list.map(ListPreference::of_key) {
        Some(Some(l)) => {
            let (kind, prefix) = l.item_rows();
            (Some(kind), prefix)
        }
        Some(None) => return Ok(Vec::new()),
        None => (None, String::new()),
    };
    let rows: Vec<(String, String)> = c
        .prepare(
            "SELECT clock, change FROM sync_held
             WHERE ?1 IS NULL OR (row_table = ?1 AND substr(row_id, 1, length(?2)) = ?2)
             ORDER BY clock, row_table, row_id",
        )
        .and_then(|mut s| {
            s.query_map(params![kind, prefix], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect()
        })
        .map_err(sql)?;
    rows.into_iter()
        .map(|(clock, change)| held_from(&clock, &change))
        .collect()
}

impl UserStore {
    /// The row `key` names as it is here, with its clock and tombstone (what the merge reads,
    /// [`MergeTx::state`]).
    pub fn row_state(&self, key: &RowKey) -> Result<RowState, StoreError> {
        let own = read_device_id(&self.conn)?;
        self.merge_tx(&self.conn, own, false)?.state(key)
    }

    pub(crate) fn merge_tx<'a>(
        &'a self,
        tx: &'a Connection,
        own: String,
        recording: bool,
    ) -> Result<MergeTx<'a>, StoreError> {
        Ok(MergeTx {
            store: self,
            tx,
            lists: ListItems::default(),
            own_ids: crate::sync_device::read_own_ids(tx)?,
            device_id: own,
            recording,
            #[cfg(feature = "test-support")]
            recorded: Vec::new(),
            #[cfg(feature = "test-support")]
            marks: Vec::new(),
        })
    }

    /// The ops of other devices held here, by clock ([`HeldOp`]): what Needs attention lists
    /// (S29, S30) until they apply.
    pub fn sync_held(&self) -> Result<Vec<HeldOp>, StoreError> {
        if self.schema_version()? < HELD_SCHEMA {
            return Ok(Vec::new());
        }
        read_held(&self.conn, None)
    }

    /// Whether any op of another device is held here (`sync_held` is not empty).
    pub fn has_sync_held(&self) -> Result<bool, StoreError> {
        if self.schema_version()? < HELD_SCHEMA {
            return Ok(false);
        }
        self.conn
            .query_row("SELECT EXISTS (SELECT 1 FROM sync_held)", [], |r| r.get(0))
            .map_err(sql)
    }

    /// Try the held ops again with `merge` ([`MergeTx::held`], [`MergeTx::unhold`]), in one
    /// transaction: all of it or (`merge` fails) none.
    pub fn merge_held<T, E: From<StoreError>>(
        &self,
        merge: impl FnOnce(&mut MergeTx<'_>) -> Result<T, E>,
    ) -> Result<T, E> {
        if self.schema_version()? < HELD_SCHEMA {
            return Err(
                StoreError::Rejected("this User database is too old to merge".into()).into(),
            );
        }
        let tx = crate::user::write_tx(&self.conn)?;
        let own = read_device_id(&tx)?;
        let recording = records_ops(&tx, &own)?;
        let mut m = self.merge_tx(&tx, own, recording)?;
        let out = merge(&mut m)?;
        #[cfg(feature = "test-support")]
        let recorded = std::mem::take(&mut m.recorded);
        drop(m);
        tx.commit().map_err(sql)?;
        #[cfg(feature = "test-support")]
        self.recorded.borrow_mut().extend(recorded);
        Ok(out)
    }

    /// Every row of Synced data, with its clock, and every tombstone and item clock, as
    /// text, each table's rows sorted; `anchors` and `notes_fts` too. Two devices that have
    /// merged each other's ops have the same. Device-local numbers (a Favorite's or Reading
    /// plan's `id`) are left out; a Reading names its plan by `uid`.
    #[cfg(feature = "test-support")]
    pub fn synced_dump(&self) -> Result<String, StoreError> {
        use rusqlite::types::Value;
        use std::fmt::Write as _;
        let queries = [
            ("notebooks", "SELECT * FROM notebooks".to_string()),
            ("annotations", "SELECT * FROM annotations".into()),
            ("anchors", "SELECT * FROM anchors".into()),
            (
                "notes_fts",
                "SELECT annotation_id, body FROM notes_fts".into(),
            ),
            (
                "highlight_palettes",
                "SELECT * FROM highlight_palettes".into(),
            ),
            ("highlight_styles", "SELECT * FROM highlight_styles".into()),
            (
                "favorite",
                "SELECT uid, folder, label, location, position, added_at, clock FROM favorite"
                    .into(),
            ),
            (
                "reading_plan",
                "SELECT uid, name, template, bible_id, start_day, weekdays, created_at, clock
                 FROM reading_plan"
                    .into(),
            ),
            (
                "plan_reading",
                "SELECT p.uid, r.position, r.passages, r.day, r.read_at, r.clock
                 FROM plan_reading r JOIN reading_plan p ON p.id = r.plan_id"
                    .into(),
            ),
            ("layout", "SELECT * FROM layout".into()),
            ("preferred_order", "SELECT * FROM preferred_order".into()),
            (
                "preference",
                "SELECT key, value, clock FROM preference".into(),
            ),
            ("tombstone", "SELECT * FROM tombstone".into()),
            ("item_clock", "SELECT * FROM item_clock".into()),
        ];
        let mut out = String::new();
        for (table, query) in queries {
            let _ = writeln!(out, "[{table}]");
            let mut stmt = self.conn.prepare(&query).map_err(sql)?;
            let n = stmt.column_count();
            let mut rows: Vec<(Option<String>, String)> = stmt
                .query_map([], |r| {
                    let text = (0..n)
                        .map(|i| r.get::<_, Value>(i).map(|v| format!("{v:?}")))
                        .collect::<Result<Vec<_>, _>>()?
                        .join(" | ");
                    // Only the synced preferences are compared.
                    let key = (table == "preference")
                        .then(|| r.get::<_, String>(0))
                        .transpose()?;
                    Ok((key, text))
                })
                .map_err(sql)?
                .collect::<Result<_, _>>()
                .map_err(sql)?;
            rows.retain(|(key, _)| {
                key.as_deref()
                    .is_none_or(crate::sync_schema::is_synced_preference)
            });
            let mut rows: Vec<String> = rows.into_iter().map(|(_, text)| text).collect();
            rows.sort();
            for row in rows {
                let _ = writeln!(out, "{row}");
            }
        }
        Ok(out)
    }

    /// The key of every row of Synced data here, sorted: a list preference in shape by its
    /// items, one not in shape (and every other synced preference) as a whole value.
    #[cfg(feature = "test-support")]
    pub fn synced_keys(&self) -> Result<Vec<RowKey>, StoreError> {
        let column = |query: &str| -> Result<Vec<String>, StoreError> {
            let mut stmt = self.conn.prepare(query).map_err(sql)?;
            let rows = stmt.query_map([], |r| r.get(0)).map_err(sql)?;
            rows.collect::<Result<_, _>>().map_err(sql)
        };
        let mut keys = vec![RowKey::PreferredOrder];
        type KeyOf = fn(String) -> RowKey;
        let tables: [(&str, KeyOf); 7] = [
            ("SELECT id FROM notebooks", RowKey::Notebook),
            ("SELECT id FROM annotations", RowKey::Annotation),
            ("SELECT id FROM highlight_styles", RowKey::HighlightStyle),
            (
                "SELECT id FROM highlight_palettes",
                RowKey::HighlightPalette,
            ),
            ("SELECT uid FROM favorite", RowKey::Favorite),
            ("SELECT uid FROM reading_plan", RowKey::ReadingPlan),
            ("SELECT id FROM layout", RowKey::Layout),
        ];
        for (query, key) in tables {
            keys.extend(column(query)?.into_iter().map(key));
        }
        let mut stmt = self
            .conn
            .prepare(
                "SELECT p.uid, r.position FROM plan_reading r JOIN reading_plan p ON p.id = r.plan_id",
            )
            .map_err(sql)?;
        let readings = stmt
            .query_map([], |r| Ok(RowKey::Reading(r.get(0)?, r.get(1)?)))
            .map_err(sql)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql)?;
        keys.extend(readings);
        let mut stmt = self
            .conn
            .prepare("SELECT key, value FROM preference")
            .map_err(sql)?;
        let preferences = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(sql)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql)?;
        for (key, value) in preferences {
            if !crate::sync_schema::is_synced_preference(&key) {
                continue;
            }
            match ListPreference::of_key(&key).and_then(|list| list.items(Some(&value))) {
                Some(items) => keys.extend(items.iter().map(crate::annotation::Row::key)),
                None => keys.push(RowKey::Preference(key)),
            }
        }
        keys.sort_by_key(RowKey::tombstone);
        Ok(keys)
    }

    /// The last op file of Synced device `device` merged here (0: none). For this device's
    /// own id it is the last of its own op files written (`sync_clock.rs`).
    pub fn applied_up_to(&self, device: &str) -> Result<u64, StoreError> {
        let seq: Option<i64> = self
            .conn
            .query_row(
                "SELECT seq FROM applied_up_to WHERE device_id = ?1",
                [device],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)?;
        Ok(seq.and_then(|s| u64::try_from(s).ok()).unwrap_or(0))
    }

    /// Merge op file `seq` of another Synced device `device` with `merge`, in one transaction
    /// that also notes it applied: either all of it is merged and noted, or (`merge` fails)
    /// nothing is, and it is merged again later. `seq` must be the next one after
    /// [`UserStore::applied_up_to`], so a device's ops are applied in order.
    pub fn merge_op_file<T, E: From<StoreError>>(
        &self,
        device: &str,
        seq: u64,
        merge: impl FnOnce(&mut MergeTx<'_>) -> Result<T, E>,
    ) -> Result<T, E> {
        if self.schema_version()? < HELD_SCHEMA {
            return Err(
                StoreError::Rejected("this User database is too old to merge".into()).into(),
            );
        }
        let tx = crate::user::write_tx(&self.conn)?;
        let own = read_device_id(&tx)?;
        if device == own {
            return Err(StoreError::Rejected(format!(
                "op file {seq} is this device's own, so it is not merged"
            ))
            .into());
        }
        let applied = self.applied_up_to(device)?;
        if seq != applied + 1 {
            return Err(StoreError::Rejected(format!(
                "op file {seq} of {device} is not the next to merge (the last merged is {applied})"
            ))
            .into());
        }
        let recording = records_ops(&tx, &own)?;
        let mut m = self.merge_tx(&tx, own, recording)?;
        let out = merge(&mut m)?;
        #[cfg(feature = "test-support")]
        let recorded = std::mem::take(&mut m.recorded);
        drop(m);
        let seq_i64 = i64::try_from(seq)
            .map_err(|_| StoreError::Rejected(format!("op file number {seq}")))?;
        tx.execute(
            "INSERT INTO applied_up_to (device_id, seq) VALUES (?1, ?2)
             ON CONFLICT (device_id) DO UPDATE SET seq = excluded.seq",
            params![device, seq_i64],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)?;
        #[cfg(feature = "test-support")]
        self.recorded.borrow_mut().extend(recorded);
        Ok(out)
    }
}
