//! The User database's side of Sync's clocks and ops (ADR 0022 rules 2–3, S28a): from schema
//! 14, [`UserStore::apply_changes`] gives every `Change` the next [hybrid logical
//! clock](crate::Hlc), writes it into the row's `clock` (an item of a list preference: its
//! `item_clock` row; a delete: its tombstone) and, while this device records ops, keeps the
//! `Change` with its clock in `op_outbox` until it is written to an op file in the Sync folder.
//!
//! The clock never goes back: it is seeded from the largest clock the database holds, at the
//! first change after opening or restoring it, and in between it only counts on and passes
//! every clock it observes ([`UserStore::observe_clock`], a merge's). A clock more than
//! [`MAX_FORWARD_SKEW_MS`](crate::hlc::MAX_FORWARD_SKEW_MS) ahead of the wall clock, or one
//! that does not parse, is not taken in; its row is left as it is.

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::annotation::is_valid_id;
use crate::annotation::{Change, Row, RowKey};
use crate::error::StoreError;
use crate::hlc::{Hlc, latest_wall_taken_in};
use crate::preference_items::ListPreference;
use crate::sync_schema::SYNCED_TABLES;
use crate::user::{UserStore, sql};

/// The first schema whose `apply_changes` stamps clocks and records ops.
pub(crate) const CLOCK_SCHEMA: u32 = 14;

/// The first schema whose tombstones say what their delete covers (`tombstone.covers`).
pub(crate) const COVERS_SCHEMA: u32 = 15;

/// The first schema with Sync conflicts' copies, deleted parents' rows (`tombstone.row`), the
/// ops held back (`sync_held`) and a list's base (`item_clock ('preference', key)`).
pub(crate) const HELD_SCHEMA: u32 = 16;

/// Which of Sync's tables and columns a database has (one opened at an older schema, for a
/// migration test, lacks the later ones).
#[derive(Debug, Clone, Copy)]
pub(crate) struct SyncColumns {
    pub(crate) tombstones: bool,
    pub(crate) clocks: bool,
    /// `tombstone.covers` and `item_clock.position` (schema 15).
    pub(crate) schema_15: bool,
    /// `tombstone.row`, `sync_held` and a list's base (schema 16).
    pub(crate) schema_16: bool,
}

/// How a `Change` applied gets its clock.
pub(crate) enum Stamp<'a> {
    /// Made on this device: the next clock, kept in the outbox while `recording`.
    Local { recording: bool },
    /// Merged from another device's op: the op's clock, and for a delete the clock of the
    /// version it removed (`None`: one from before clocks).
    Merged {
        clock: &'a Hlc,
        covers: Option<&'a Hlc>,
        /// For a delete, the row as the op's device deleted it (its `before`), which a
        /// parent's tombstone keeps; `None`: the row here.
        deleted: Option<&'a Row>,
    },
    /// Rewritten here the way every device rewrites it from what they all merged (a default
    /// Notebook another device's older one replaces): an update, and the row keeps its clock.
    Kept,
}

/// Where this process's clock is: the device it names, the last wall time and counter it
/// handed out, and whether it has been seeded from the database since it was opened or
/// restored.
#[derive(Debug, Default)]
pub(crate) struct ClockState {
    device_id: Option<String>,
    last: Option<(u64, u16)>,
    seeded: bool,
}

/// One `Change` waiting in the outbox for its op file, with its clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxOp {
    pub clock: Hlc,
    pub change: Change,
    /// The clock its `before` row had here (none: no `before`, or a row from before clocks):
    /// what a delete covers, so another device's edit it did not see beats it.
    pub before_clock: Option<Hlc>,
}

/// An outbox row's `change` JSON: the `Change`, and the clock of its `before` row when it has
/// one (an outbox row written before schema 15 has none).
#[derive(Serialize, Deserialize)]
struct OutboxChange {
    before: Option<Row>,
    after: Option<Row>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    before_clock: Option<Hlc>,
}

/// An op file reserved for the outbox's ops: its number and its ops, in the order they were
/// applied. Written again, it has the same ops in the same order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReservedOpFile {
    pub seq: u64,
    pub ops: Vec<OutboxOp>,
}

/// The tables whose `clock` the seed reads: every synced table, the tombstones, the item
/// clocks, the outbox and the ops held back (schema 16; their clocks were observed).
fn clocked_tables(c: &Connection) -> Result<Vec<&'static str>, StoreError> {
    let mut tables: Vec<&'static str> = SYNCED_TABLES
        .iter()
        .copied()
        .chain(["tombstone", "item_clock", "op_outbox"])
        .collect();
    if crate::synced_store::has_table(c, "sync_held")? {
        tables.push("sync_held");
    }
    Ok(tables)
}

/// A `GLOB` pattern matching exactly the text form of a clock ([`Hlc::parse`]).
fn clock_glob() -> String {
    let hex = |n: usize| "[0-9a-f]".repeat(n);
    [12, 4, 8, 4, 4, 4, 12].map(hex).join("-")
}

/// The largest wall time and counter of any clock in the database that the clock takes in at
/// wall time `now_ms`: one that parses, at most
/// [`MAX_FORWARD_SKEW_MS`](crate::hlc::MAX_FORWARD_SKEW_MS) ahead.
fn largest_clock(c: &Connection, now_ms: i64) -> Result<Option<(u64, u16)>, StoreError> {
    let glob = clock_glob();
    let latest = format!("{:012x}", latest_wall_taken_in(now_ms));
    let mut largest = None;
    for table in clocked_tables(c)? {
        let clock: Option<String> = c
            .query_row(
                &format!(
                    "SELECT MAX(clock) FROM {table} WHERE clock GLOB ?1 AND substr(clock, 1, 12) <= ?2"
                ),
                params![glob, latest],
                |r| r.get(0),
            )
            .map_err(sql)?;
        if let Some(tick) = clock.as_deref().and_then(Hlc::parse).map(|h| h.tick()) {
            largest = largest.max(Some(tick));
        }
    }
    Ok(largest)
}

/// The largest wall time and counter of any clock in the database that parses, however far
/// ahead it is (a restore's replace must pass every clock its data holds, S28c).
pub(crate) fn largest_held_clock(c: &Connection) -> Result<Option<(u64, u16)>, StoreError> {
    let glob = clock_glob();
    let mut largest = None;
    for table in clocked_tables(c)? {
        let clock: Option<String> = c
            .query_row(
                &format!("SELECT MAX(clock) FROM {table} WHERE clock GLOB ?1"),
                [&glob],
                |r| r.get(0),
            )
            .map_err(sql)?;
        if let Some(tick) = clock.as_deref().and_then(Hlc::parse).map(|h| h.tick()) {
            largest = largest.max(Some(tick));
        }
    }
    Ok(largest)
}

/// This device's id; [`StoreError::Corrupt`] when there is none or it is not a UUID
/// (`ensure_device` at open replaces both).
pub(crate) fn read_device_id(c: &Connection) -> Result<String, StoreError> {
    let id: String = c
        .query_row("SELECT device_id FROM device WHERE id = 1", [], |r| {
            r.get(0)
        })
        .optional()
        .map_err(sql)?
        .ok_or_else(|| StoreError::Corrupt("the User database has no device id".into()))?;
    if !is_valid_id(&id) {
        return Err(StoreError::Corrupt(format!(
            "the User database's device id `{id}` is not one"
        )));
    }
    Ok(id)
}

impl UserStore {
    /// The next clock, from the database `c` this store writes through.
    pub(crate) fn next_clock(&self, c: &Connection) -> Result<Hlc, StoreError> {
        let now = crate::clock::now_ms();
        let mut state = self.clock.borrow_mut();
        if !state.seeded {
            state.device_id = Some(read_device_id(c)?);
            state.last = state.last.max(largest_clock(c, now)?);
            state.seeded = true;
        }
        let device_id = state
            .device_id
            .clone()
            .ok_or_else(|| StoreError::Corrupt("the User database has no device id".into()))?;
        let clock = Hlc::next(state.last, now, &device_id)?;
        state.last = Some(clock.tick());
        Ok(clock)
    }

    /// The clock of a local write that replaces a version at clock `replaced` (the row's, or
    /// what its tombstone covers): the next clock, or, when `replaced` is at or after it (a
    /// clock merged from a device more than
    /// [`MAX_FORWARD_SKEW_MS`](crate::hlc::MAX_FORWARD_SKEW_MS) ahead, which is not observed),
    /// the clock just after `replaced`, so every device's merge takes the write too (last
    /// writer wins). The device's own clock does not follow it: only this write passes it.
    pub(crate) fn next_clock_after(
        &self,
        c: &Connection,
        replaced: Option<&Hlc>,
    ) -> Result<Hlc, StoreError> {
        let clock = self.next_clock(c)?;
        match replaced {
            Some(replaced) if *replaced >= clock => Hlc::next(
                Some(replaced.tick()),
                crate::clock::now_ms(),
                &clock.device_id,
            ),
            _ => Ok(clock),
        }
    }

    /// Take in `clock`, one merged in from another device (the HLC receive rule): every
    /// clock handed out after it passes it. A clock more than
    /// [`MAX_FORWARD_SKEW_MS`](crate::hlc::MAX_FORWARD_SKEW_MS) ahead of the wall clock is
    /// not taken in; returns whether it was.
    pub fn observe_clock(&self, clock: &Hlc) -> bool {
        if clock.wall_ms > latest_wall_taken_in(crate::clock::now_ms()) {
            return false;
        }
        let mut state = self.clock.borrow_mut();
        state.last = state.last.max(Some(clock.tick()));
        true
    }

    /// Seed the clock from the database again at the next change (after a restore replaced
    /// it); it still never hands out a clock below one it handed out before.
    pub(crate) fn reseed_clock(&self) {
        let mut state = self.clock.borrow_mut();
        state.seeded = false;
        state.device_id = None;
    }

    /// This Synced device's id.
    pub fn device_id(&self) -> Result<String, StoreError> {
        read_device_id(&self.conn)
    }

    /// Start keeping every `Change` applied in the outbox for the Sync folder; the op files
    /// written so far are none. Sync's join calls it after writing its snapshot; it does
    /// nothing when this device already records ops.
    pub fn start_recording_ops(&self) -> Result<(), StoreError> {
        let device = self.device_id()?;
        self.conn
            .execute(
                "INSERT INTO applied_up_to (device_id, seq) VALUES (?1, 0)
                 ON CONFLICT (device_id) DO NOTHING",
                [device],
            )
            .map(drop)
            .map_err(sql)
    }

    /// Whether this device keeps the `Change`s applied for the Sync folder.
    pub fn records_ops(&self) -> Result<bool, StoreError> {
        records_ops(&self.conn, &self.device_id()?)
    }

    /// Reserve the next op file for the ops in the outbox not yet in one, and return every op
    /// file reserved and not yet written, by number. An op file reserved before (a write that
    /// stopped half way) comes back with exactly the ops it had.
    pub fn reserve_op_files(&self) -> Result<Vec<ReservedOpFile>, StoreError> {
        let tx = self.conn.unchecked_transaction().map_err(sql)?;
        let device = read_device_id(&tx)?;
        let written: Option<i64> = tx
            .query_row(
                "SELECT seq FROM applied_up_to WHERE device_id = ?1",
                [&device],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)?;
        let Some(written) = written else {
            return Ok(Vec::new());
        };
        let reserved: Option<i64> = tx
            .query_row("SELECT MAX(file_seq) FROM op_outbox", [], |r| r.get(0))
            .map_err(sql)?;
        let next = written.max(reserved.unwrap_or(0)) + 1;
        tx.execute(
            "UPDATE op_outbox SET file_seq = ?1 WHERE file_seq IS NULL",
            [next],
        )
        .map_err(sql)?;
        let rows: Vec<(i64, String, String)> = tx
            .prepare("SELECT file_seq, clock, change FROM op_outbox ORDER BY file_seq, id")
            .map_err(sql)?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(sql)?
            .collect::<Result<_, _>>()
            .map_err(sql)?;
        tx.commit().map_err(sql)?;
        let mut files: Vec<ReservedOpFile> = Vec::new();
        for (seq, clock, change) in rows {
            let seq = u64::try_from(seq)
                .map_err(|_| StoreError::Corrupt(format!("op file number {seq}")))?;
            let OutboxChange {
                before,
                after,
                before_clock,
            } = serde_json::from_str(&change)
                .map_err(|e| StoreError::Corrupt(format!("outbox change: {e}")))?;
            let op = OutboxOp {
                clock: Hlc::parse(&clock)
                    .ok_or_else(|| StoreError::Corrupt(format!("outbox clock `{clock}`")))?,
                change: Change { before, after },
                before_clock,
            };
            match files.last_mut() {
                Some(file) if file.seq == seq => file.ops.push(op),
                _ => files.push(ReservedOpFile { seq, ops: vec![op] }),
            }
        }
        Ok(files)
    }

    /// Op file `seq` is in the Sync folder: its ops leave the outbox, and it is the last of
    /// this device's op files written. [`StoreError::Rejected`] unless `seq` is reserved
    /// ([`UserStore::reserve_op_files`]) and at most one after the last written, so the own
    /// files never skip a number.
    pub fn op_file_written(&self, seq: u64) -> Result<(), StoreError> {
        let seq = i64::try_from(seq)
            .map_err(|_| StoreError::Rejected(format!("op file number {seq}")))?;
        let tx = self.conn.unchecked_transaction().map_err(sql)?;
        let device = read_device_id(&tx)?;
        let (written, reserved): (Option<i64>, bool) = tx
            .query_row(
                "SELECT (SELECT seq FROM applied_up_to WHERE device_id = ?1),
                        EXISTS (SELECT 1 FROM op_outbox WHERE file_seq = ?2)",
                params![device, seq],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(sql)?;
        let Some(written) = written else {
            return Err(StoreError::Rejected(
                "this device does not record ops".into(),
            ));
        };
        if !reserved || seq > written + 1 {
            return Err(StoreError::Rejected(format!(
                "op file {seq} is not the next one reserved (the last written is {written})"
            )));
        }
        tx.execute("DELETE FROM op_outbox WHERE file_seq = ?1", [seq])
            .map_err(sql)?;
        tx.execute(
            "UPDATE applied_up_to SET seq = max(seq, ?2) WHERE device_id = ?1",
            params![device, seq],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)
    }
}

/// Whether `device` records ops: `applied_up_to` has a row for it, the last of its own op
/// files written.
pub(crate) fn records_ops(c: &Connection, device: &str) -> Result<bool, StoreError> {
    c.query_row(
        "SELECT EXISTS (SELECT 1 FROM applied_up_to WHERE device_id = ?1)",
        [device],
        |r| r.get(0),
    )
    .map_err(sql)
}

/// Keep `change`, applied with `clock`, in the outbox, with the clock its `before` row had.
pub(crate) fn record_op(
    c: &Connection,
    clock: &Hlc,
    change: &Change,
    before_clock: Option<&Hlc>,
) -> Result<(), StoreError> {
    let json = serde_json::to_string(&OutboxChange {
        before: change.before.clone(),
        after: change.after.clone(),
        before_clock: before_clock.cloned(),
    })
    .map_err(|e| StoreError::Rejected(format!("a change that is not JSON: {e}")))?;
    c.execute(
        "INSERT INTO op_outbox (clock, change) VALUES (?1, ?2)",
        params![clock.encode()?, json],
    )
    .map(drop)
    .map_err(sql)
}

/// Write `clock` as the clock of the row `key` names, which `change` has just written. A
/// deleted row has none (its tombstone has it); a list preference's item has its
/// `item_clock` row, and its preference's row the same clock; the Preferred order has its
/// `item_clock` row (`preferred_order`, `''`), kept when the order is empty, and every
/// `preferred_order` row the same clock. From schema 15 a list item's `item_clock` row keeps
/// its position too (`synced_store::stored_positions`). From schema 16 a list preference
/// written whole is its list's **base** (ADR 0022 rule 3, D7): its clock is kept as
/// `item_clock ('preference', key)`, every item op at or before it is superseded (so the
/// item clocks at or before it go: the base holds their data), and its `preference.clock`
/// stays the newest of any op on the list.
pub(crate) fn stamp_row(
    c: &Connection,
    key: &RowKey,
    change: &Change,
    clock: &Hlc,
    columns: SyncColumns,
) -> Result<(), StoreError> {
    let positions = columns.schema_15;
    let clock = clock.encode()?;
    let deleted = change.after.is_none();
    let set = |table: &str, column: &str, id: &str| {
        if deleted {
            return Ok(());
        }
        c.execute(
            &format!("UPDATE {table} SET clock = ?1 WHERE {column} = ?2"),
            params![clock, id],
        )
        .map(drop)
        .map_err(sql)
    };
    match key {
        RowKey::Notebook(id) => set("notebooks", "id", id),
        RowKey::Annotation(id) => set("annotations", "id", id),
        RowKey::HighlightStyle(id) => set("highlight_styles", "id", id),
        RowKey::HighlightPalette(id) => set("highlight_palettes", "id", id),
        RowKey::Favorite(uid) => set("favorite", "uid", uid),
        RowKey::ReadingPlan(uid) => set("reading_plan", "uid", uid),
        RowKey::Layout(id) => set("layout", "id", id),
        RowKey::Preference(k) => match ListPreference::of_key(k) {
            Some(list) if columns.schema_16 => {
                let value = match &change.after {
                    Some(Row::Preference(p)) => Some(p.value.as_str()),
                    _ => None,
                };
                stamp_list_base(c, &list, &clock, value, deleted)
            }
            _ => set("preference", "key", k),
        },
        RowKey::Reading(plan, position) if !deleted => c
            .execute(
                "UPDATE plan_reading SET clock = ?1
                 WHERE plan_id = (SELECT id FROM reading_plan WHERE uid = ?2) AND position = ?3",
                params![clock, plan, position],
            )
            .map(drop)
            .map_err(sql),
        RowKey::Reading(..) => Ok(()),
        RowKey::PreferredOrder => {
            let (kind, id) = key.tombstone();
            c.execute(
                "INSERT INTO item_clock (row_table, row_id, clock) VALUES (?1, ?2, ?3)
                 ON CONFLICT (row_table, row_id) DO UPDATE SET clock = excluded.clock",
                params![kind, id, clock],
            )
            .map_err(sql)?;
            c.execute("UPDATE preferred_order SET clock = ?1", [&clock])
                .map(drop)
                .map_err(sql)
        }
        RowKey::Collection(_)
        | RowKey::CopyPreset(_)
        | RowKey::SavedSearch(_)
        | RowKey::GuideSection(..)
        | RowKey::ResourceTags(_) => {
            let (kind, id) = key.tombstone();
            let position = match &change.after {
                Some(Row::Collection(i) | Row::CopyPreset(i) | Row::SavedSearch(i)) => {
                    Some(i64::from(i.position))
                }
                Some(Row::GuideSection(s)) => Some(i64::from(s.position)),
                _ => None,
            };
            if deleted {
                c.execute(
                    "DELETE FROM item_clock WHERE row_table = ?1 AND row_id = ?2",
                    params![kind, id],
                )
            } else if positions {
                c.execute(
                    "INSERT INTO item_clock (row_table, row_id, clock, position)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (row_table, row_id) DO UPDATE SET clock = excluded.clock,
                         position = excluded.position",
                    params![kind, id, clock, position],
                )
            } else {
                c.execute(
                    "INSERT INTO item_clock (row_table, row_id, clock) VALUES (?1, ?2, ?3)
                     ON CONFLICT (row_table, row_id) DO UPDATE SET clock = excluded.clock",
                    params![kind, id, clock],
                )
            }
            .map_err(sql)?;
            // The list's clock is its newest item's: a merged item can be older than one here.
            match ListPreference::of_row(key) {
                Some(list) => c
                    .execute(
                        "UPDATE preference SET clock = max(coalesce(clock, ''), ?1) WHERE key = ?2",
                        params![clock, list.key()],
                    )
                    .map(drop)
                    .map_err(sql),
                None => Ok(()),
            }
        }
    }
}

/// The tombstone key (`row_table`, `row_id`) of the base of list preference `key`
/// ([`stamp_row`]).
pub(crate) const LIST_BASE_TABLE: &str = "preference";

/// Make the whole value of `list` just written at `clock` (text form), `value` (`None`:
/// deleted), its base: the list's `preference.clock` is the newest of it and every item op,
/// the base's clock is kept, and the item clocks at or before it go; each item of `value`
/// (when in shape) gets the base's clock and its index in `value` as its position, so every
/// device composes the list in the same order however its later items arrived.
fn stamp_list_base(
    c: &Connection,
    list: &ListPreference,
    clock: &str,
    value: Option<&str>,
    deleted: bool,
) -> Result<(), StoreError> {
    let key = list.key();
    if !deleted {
        c.execute(
            "UPDATE preference SET clock = max(coalesce(clock, ''), ?1) WHERE key = ?2",
            params![clock, key],
        )
        .map_err(sql)?;
    }
    c.execute(
        "INSERT INTO item_clock (row_table, row_id, clock) VALUES (?1, ?2, ?3)
         ON CONFLICT (row_table, row_id) DO UPDATE SET clock = max(clock, excluded.clock)",
        params![LIST_BASE_TABLE, key, clock],
    )
    .map_err(sql)?;
    let (kind, prefix) = list.item_rows();
    c.execute(
        "DELETE FROM item_clock WHERE row_table = ?1 AND substr(row_id, 1, length(?2)) = ?2
             AND clock <= ?3",
        params![kind, prefix, clock],
    )
    .map_err(sql)?;
    let Some(items) = value.and_then(|v| list.items(Some(v))) else {
        return Ok(());
    };
    let mut stmt = c
        .prepare_cached(
            "INSERT INTO item_clock (row_table, row_id, clock, position) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (row_table, row_id) DO NOTHING",
        )
        .map_err(sql)?;
    for (i, item) in items.iter().enumerate() {
        let (kind, id) = item.key().tombstone();
        let position = match item {
            Row::ResourceTags(_) => None,
            _ => i64::try_from(i).ok(),
        };
        stmt.execute(params![kind, id, clock, position])
            .map_err(sql)?;
    }
    Ok(())
}

/// The clock of the base of `list` (its newest whole-value write, from schema 16), if any.
pub(crate) fn read_list_base(
    c: &Connection,
    list: &ListPreference,
) -> Result<Option<Hlc>, StoreError> {
    let clock: Option<String> = c
        .prepare_cached("SELECT clock FROM item_clock WHERE row_table = ?1 AND row_id = ?2")
        .and_then(|mut s| {
            s.query_row(params![LIST_BASE_TABLE, list.key()], |r| r.get(0))
                .optional()
        })
        .map_err(sql)?;
    Ok(clock.as_deref().and_then(Hlc::parse))
}

/// The newest version a delete of the row `key` names covered (its tombstone's `covers`, or
/// its own clock before schema 15 filled `covers`), if it has a tombstone with a clock.
pub(crate) fn read_tombstone_covers(
    c: &Connection,
    key: &RowKey,
) -> Result<Option<Hlc>, StoreError> {
    let (kind, id) = key.tombstone();
    let covers: Option<String> = c
        .prepare_cached(
            "SELECT coalesce(covers, clock) FROM tombstone WHERE row_table = ?1 AND row_id = ?2",
        )
        .and_then(|mut s| {
            s.query_row(params![kind, id], |r| r.get::<_, Option<String>>(0))
                .optional()
        })
        .map_err(sql)?
        .flatten();
    Ok(covers.as_deref().and_then(Hlc::parse))
}

/// The clock of the row `key` names as stored (its `clock` column, or its `item_clock` row for
/// a list item and the Preferred order): `None` when it has none (`NULL`, a row from before
/// clocks) or there is no such row. Text that is not a clock is returned as it is.
pub(crate) fn read_row_clock(c: &Connection, key: &RowKey) -> Result<Option<String>, StoreError> {
    let one = |sql_text: &str, p: &[&dyn rusqlite::ToSql]| -> Result<Option<String>, StoreError> {
        c.prepare_cached(sql_text)
            .and_then(|mut s| s.query_row(p, |r| r.get::<_, Option<String>>(0)).optional())
            .map(Option::flatten)
            .map_err(sql)
    };
    match key {
        RowKey::Notebook(id) => one("SELECT clock FROM notebooks WHERE id = ?1", &[id]),
        RowKey::Annotation(id) => one("SELECT clock FROM annotations WHERE id = ?1", &[id]),
        RowKey::HighlightStyle(id) => {
            one("SELECT clock FROM highlight_styles WHERE id = ?1", &[id])
        }
        RowKey::HighlightPalette(id) => {
            one("SELECT clock FROM highlight_palettes WHERE id = ?1", &[id])
        }
        RowKey::Favorite(uid) => one("SELECT clock FROM favorite WHERE uid = ?1", &[uid]),
        RowKey::ReadingPlan(uid) => one("SELECT clock FROM reading_plan WHERE uid = ?1", &[uid]),
        RowKey::Reading(plan, position) => one(
            "SELECT r.clock FROM plan_reading r JOIN reading_plan p ON p.id = r.plan_id
             WHERE p.uid = ?1 AND r.position = ?2",
            &[plan, position],
        ),
        RowKey::Layout(id) => one("SELECT clock FROM layout WHERE id = ?1", &[id]),
        RowKey::Preference(k) => one("SELECT clock FROM preference WHERE key = ?1", &[k]),
        RowKey::PreferredOrder => {
            let (kind, id) = key.tombstone();
            one(
                "SELECT clock FROM item_clock WHERE row_table = ?1 AND row_id = ?2",
                &[&kind, &id],
            )
        }
        // An item is at least as new as its list's base, which holds it unless a later op
        // on it does.
        RowKey::Collection(_)
        | RowKey::CopyPreset(_)
        | RowKey::SavedSearch(_)
        | RowKey::GuideSection(..)
        | RowKey::ResourceTags(_) => {
            let (kind, id) = key.tombstone();
            let base = ListPreference::of_row(key)
                .map(|l| l.key())
                .unwrap_or_default();
            one(
                "SELECT max(coalesce((SELECT clock FROM item_clock
                                      WHERE row_table = ?1 AND row_id = ?2), ''),
                            coalesce((SELECT clock FROM item_clock
                                      WHERE row_table = ?3 AND row_id = ?4), ''))",
                &[&kind, &id, &LIST_BASE_TABLE, &base],
            )
            .map(|c| c.filter(|c| !c.is_empty()))
        }
    }
}
