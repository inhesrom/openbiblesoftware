//! The User database's side of Sync's snapshots (ADR 0022 rule 5, S28c).
//!
//! A **snapshot** is a Backup (ADR 0013: the User database file, `OBSU` and its schema as its
//! format version) holding only what Synced devices share and what merging it needs, with the
//! op files it covers: [`UserStore::snapshot_bytes`] copies the database, empties what stays on
//! the device ([`SNAPSHOT_EMPTIED`], and every preference that is not Synced data) and
//! vacuums it, so nothing local leaves the device (rule 4). It keeps the synced tables with
//! their clocks, the Anchors and `notes_fts`, every tombstone (what it covers, a parent's
//! row), the item clocks (positions, list bases), the `device` row (the id of the device that
//! wrote it), `applied_up_to` (the clocks it covers: for each device the last of its op files
//! merged, for its own id the last of its own written) and `sync_held` (ops of files it covers
//! that it could not apply yet, which a device starting from it would otherwise never see).
//!
//! [`UserStore::open_snapshot`] reads one back as [`UserStore::open_backup`] reads a Backup
//! (a newer format is [`StoreError::NewerFormat`]), and [`UserStore::snapshot_contents`] lists
//! what it holds for the engine's merge, which merges it row by row by the same rules as ops
//! ([`UserStore::merge_snapshot`]). [`UserStore::join_sync`] starts a device syncing: every
//! row of Synced data with no clock yet gets one, so its snapshot carries a clock for each, and
//! it records ops from then on. [`UserStore::restamp_for_replace`] gives a restored database
//! new clocks, so its snapshot wins everywhere (rule 7, "replace").

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, params};

use crate::annotation::{Row, RowKey};
use crate::backup_store::ScratchFile;
use crate::error::StoreError;
use crate::hlc::Hlc;
use crate::merge_store::{HeldOp, MergeTx};
use crate::preference_items::ListPreference;
use crate::sync_clock::{LIST_BASE_TABLE, read_device_id, read_row_clock, records_ops};
use crate::sync_schema::{SYNCED_TABLES, is_synced_preference};
use crate::synced_store::{ListItems, read_preference};
use crate::user::{UserStore, sql};

/// The tables a snapshot holds empty: what stays on the device (History, the Last session,
/// the Resources' remembered names) and what is this database's own (its outbox of ops not
/// yet written, the ids it had before).
pub const SNAPSHOT_EMPTIED: &[&str] = &[
    "history",
    "last_session",
    "resource_names",
    "op_outbox",
    "former_device",
];

/// A row of Synced data in a snapshot, with its clock (`None`: changed before clocks).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotRow {
    pub row: Row,
    pub clock: Option<Hlc>,
}

/// A tombstone in a snapshot: its newest delete's clock and the newest version any delete of
/// it covered (`None`: only a version from before clocks), and a parent's kept row with the
/// clock and covers of the delete it is from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotTombstone {
    pub key: RowKey,
    pub clock: Hlc,
    pub covers: Option<Hlc>,
    pub row: Option<(Row, Hlc, Option<Hlc>)>,
}

/// A list preference's base in a snapshot (its newest whole-value write, D7): its clock, and
/// the value as it was written as far as the snapshot still holds it: the items still at the
/// base's clock (`items`, with their positions), or the whole value when it is not in its
/// list's shape, or none when it was deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotListBase {
    /// The list preference's key.
    pub key: String,
    pub clock: Hlc,
    /// The whole value to merge at the base's clock (`None`: the value deleted).
    pub value: Option<Row>,
    /// The base's items still at its clock, with their positions as stored (`None`: an item
    /// without one, the Resource tags).
    pub items: Vec<(RowKey, Option<i64>)>,
}

/// What a snapshot holds, for the merge ([`UserStore::snapshot_contents`]).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SnapshotContents {
    /// The id of the device that wrote it.
    pub device: String,
    /// The op files it covers: for each device the last merged, for its own the last written.
    pub applied: BTreeMap<String, u64>,
    /// Every row of Synced data with its clock, parents before their children; a list
    /// preference in its shape as its items, one not in shape as its whole value (it is
    /// also its list's base).
    pub rows: Vec<SnapshotRow>,
    /// The lists' bases.
    pub list_bases: Vec<SnapshotListBase>,
    /// Each list preference's clock (the newest of any op on it).
    pub list_clocks: Vec<(String, Hlc)>,
    pub tombstones: Vec<SnapshotTombstone>,
    /// The ops of other devices it holds back (`sync_held`).
    pub held: Vec<HeldOp>,
}

/// The key a tombstone (or item clock) names by its `row_table` and `row_id`.
pub fn row_key_of(kind: &str, id: &str) -> Option<RowKey> {
    let id_ = || id.to_string();
    Some(match kind {
        "notebook" => RowKey::Notebook(id_()),
        "annotation" => RowKey::Annotation(id_()),
        "highlight_style" => RowKey::HighlightStyle(id_()),
        "highlight_palette" => RowKey::HighlightPalette(id_()),
        "favorite" => RowKey::Favorite(id_()),
        "reading_plan" => RowKey::ReadingPlan(id_()),
        "reading" => {
            let (plan, position) = id.rsplit_once('/')?;
            RowKey::Reading(plan.to_string(), position.parse().ok()?)
        }
        "layout" => RowKey::Layout(id_()),
        "preferred_order" => RowKey::PreferredOrder,
        "preference" => RowKey::Preference(id_()),
        "collection" => RowKey::Collection(id_()),
        "copy_preset" => RowKey::CopyPreset(id_()),
        "saved_search" => RowKey::SavedSearch(id_()),
        "guide_section" => {
            let (guide, section) = id.split_once('/')?;
            RowKey::GuideSection(guide.to_string(), section.to_string())
        }
        "resource_tags" => RowKey::ResourceTags(id_()),
        _ => return None,
    })
}

/// One column of text of every row `query` returns.
fn texts(c: &Connection, query: &str) -> Result<Vec<String>, StoreError> {
    c.prepare(query)
        .and_then(|mut s| s.query_map([], |r| r.get(0))?.collect())
        .map_err(sql)
}

/// The key of every row of Synced data in `c`, parents before their children: Notebooks,
/// palettes, styles and Reading plans, then the rows that may need them, then the rest; a
/// list preference in its shape by its items, one not in shape (and every other synced
/// preference) as its whole value.
pub(crate) fn synced_row_keys(c: &Connection) -> Result<Vec<RowKey>, StoreError> {
    let mut keys = Vec::new();
    type KeyOf = fn(String) -> RowKey;
    let tables: [(&str, KeyOf); 7] = [
        ("SELECT id FROM notebooks ORDER BY id", RowKey::Notebook),
        (
            "SELECT id FROM highlight_palettes ORDER BY id",
            RowKey::HighlightPalette,
        ),
        (
            "SELECT id FROM highlight_styles ORDER BY id",
            RowKey::HighlightStyle,
        ),
        ("SELECT uid FROM reading_plan ORDER BY uid", RowKey::ReadingPlan),
        ("SELECT id FROM annotations ORDER BY id", RowKey::Annotation),
        ("SELECT uid FROM favorite ORDER BY uid", RowKey::Favorite),
        ("SELECT id FROM layout ORDER BY id", RowKey::Layout),
    ];
    for (query, key) in tables {
        keys.extend(texts(c, query)?.into_iter().map(key));
    }
    let readings: Vec<RowKey> = c
        .prepare(
            "SELECT p.uid, r.position FROM plan_reading r JOIN reading_plan p ON p.id = r.plan_id
             ORDER BY p.uid, r.position",
        )
        .and_then(|mut s| {
            s.query_map([], |r| Ok(RowKey::Reading(r.get(0)?, r.get(1)?)))?
                .collect()
        })
        .map_err(sql)?;
    keys.extend(readings);
    keys.push(RowKey::PreferredOrder);
    let preferences: Vec<(String, String)> = c
        .prepare("SELECT key, value FROM preference ORDER BY key")
        .and_then(|mut s| {
            s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect()
        })
        .map_err(sql)?;
    for (key, value) in preferences {
        if !is_synced_preference(&key) {
            continue;
        }
        match ListPreference::of_key(&key).and_then(|list| list.items(Some(&value))) {
            Some(items) => keys.extend(items.iter().map(Row::key)),
            None => keys.push(RowKey::Preference(key)),
        }
    }
    Ok(keys)
}

/// Give every row of Synced data in `c` that has no clock (last changed before schema 14) the
/// clock `clock`, and every list item without a position its index; a tombstone's empty clock
/// stays (it covers only a version from before clocks).
fn stamp_unclocked(c: &Connection, clock: &str) -> Result<(), StoreError> {
    for table in SYNCED_TABLES {
        if *table == "preference" {
            continue;
        }
        c.execute(
            &format!("UPDATE {table} SET clock = ?1 WHERE clock IS NULL"),
            [clock],
        )
        .map_err(sql)?;
    }
    for key in texts(c, "SELECT key FROM preference WHERE clock IS NULL")? {
        if is_synced_preference(&key) {
            c.execute(
                "UPDATE preference SET clock = ?1 WHERE key = ?2",
                params![clock, key],
            )
            .map_err(sql)?;
        }
    }
    let order_rows: i64 = c
        .query_row("SELECT COUNT(*) FROM preferred_order", [], |r| r.get(0))
        .map_err(sql)?;
    if order_rows > 0 {
        c.execute(
            "INSERT INTO item_clock (row_table, row_id, clock) VALUES ('preferred_order', '', ?1)
             ON CONFLICT DO NOTHING",
            [clock],
        )
        .map_err(sql)?;
    }
    let mut lists = ListItems::default();
    for key in synced_row_keys(c)? {
        if ListPreference::of_row(&key).is_none() {
            continue;
        }
        let (kind, id) = key.tombstone();
        let position = match crate::annotation_store::read_row(c, &mut lists, &key)? {
            Some(Row::Collection(i) | Row::CopyPreset(i) | Row::SavedSearch(i)) => {
                Some(i64::from(i.position))
            }
            Some(Row::GuideSection(s)) => Some(i64::from(s.position)),
            _ => None,
        };
        if read_row_clock(c, &key)?.is_none() {
            c.execute(
                "INSERT INTO item_clock (row_table, row_id, clock, position) VALUES (?1, ?2, ?3, ?4)",
                params![kind, id, clock, position],
            )
            .map_err(sql)?;
        } else if position.is_some() {
            c.execute(
                "UPDATE item_clock SET position = ?3
                 WHERE row_table = ?1 AND row_id = ?2 AND position IS NULL",
                params![kind, id, position],
            )
            .map_err(sql)?;
        }
    }
    Ok(())
}

impl UserStore {
    /// The bytes of a snapshot of this database (module docs): a Backup holding only Synced
    /// data and what merging it needs, written through a scratch file in the backups folder.
    pub fn snapshot_bytes(&self) -> Result<Vec<u8>, StoreError> {
        let copy = self.copy_in_memory()?;
        for table in SNAPSHOT_EMPTIED {
            copy.conn
                .execute(&format!("DELETE FROM {table}"), [])
                .map_err(sql)?;
        }
        for key in texts(&copy.conn, "SELECT key FROM preference")? {
            if !is_synced_preference(&key) {
                copy.conn
                    .execute("DELETE FROM preference WHERE key = ?1", [key])
                    .map_err(sql)?;
            }
        }
        copy.conn
            .execute_batch("UPDATE device SET restore_choice = 0; VACUUM;")
            .map_err(sql)?;
        std::fs::create_dir_all(self.backup_dir()).map_err(StoreError::Io)?;
        let file = ScratchFile(self.new_scratch_file(
            &self.backup_dir().join(".snapshot-writing"),
            crate::backup::BACKUP_EXTENSION,
        )?);
        copy.back_up(&file.0)?;
        std::fs::read(&file.0).map_err(StoreError::Io)
    }

    /// The snapshot `bytes` read as a User database in memory, migrated to this build's
    /// format, with the format version it was written in: checked as a Backup is
    /// ([`UserStore::open_backup`]), so one that is not whole is refused, and one of a newer
    /// format is [`StoreError::NewerFormat`]. Read through a scratch file in the backups folder.
    pub fn open_snapshot(&self, bytes: &[u8]) -> Result<(UserStore, u32), StoreError> {
        std::fs::create_dir_all(self.backup_dir()).map_err(StoreError::Io)?;
        let file = ScratchFile(self.new_scratch_file(
            &self.backup_dir().join(".snapshot-reading"),
            crate::backup::BACKUP_EXTENSION,
        )?);
        std::fs::write(&file.0, bytes).map_err(StoreError::Io)?;
        UserStore::open_backup(&file.0)
    }

    /// What this database (a snapshot [`UserStore::open_snapshot`] read) holds for the merge.
    pub fn snapshot_contents(&self) -> Result<SnapshotContents, StoreError> {
        let c = &self.conn;
        let mut out = SnapshotContents {
            device: read_device_id(c)?,
            ..SnapshotContents::default()
        };
        let applied: Vec<(String, i64)> = c
            .prepare("SELECT device_id, seq FROM applied_up_to ORDER BY device_id")
            .and_then(|mut s| {
                s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect()
            })
            .map_err(sql)?;
        for (device, seq) in applied {
            out.applied
                .insert(device, u64::try_from(seq).unwrap_or_default());
        }
        // The lists' bases and clocks.
        let bases: Vec<(String, String)> = c
            .prepare("SELECT row_id, clock FROM item_clock WHERE row_table = ?1 ORDER BY row_id")
            .and_then(|mut s| {
                s.query_map([LIST_BASE_TABLE], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect()
            })
            .map_err(sql)?;
        for (key, clock) in bases {
            let (Some(list), Some(clock)) = (ListPreference::of_key(&key), Hlc::parse(&clock))
            else {
                continue;
            };
            let value = read_preference(c, &key)?;
            let (value, items) = match list.items(value.as_deref()) {
                Some(mut items) if value.is_some() => {
                    crate::synced_store::stored_positions(c, &mut items)?;
                    let mut at_base = Vec::new();
                    let mut positions = Vec::new();
                    for item in items {
                        let (kind, id) = item.key().tombstone();
                        let stored: Option<(String, Option<i64>)> = c
                            .prepare_cached(
                                "SELECT clock, position FROM item_clock
                                 WHERE row_table = ?1 AND row_id = ?2",
                            )
                            .and_then(|mut s| {
                                s.query_row(params![kind, id], |r| Ok((r.get(0)?, r.get(1)?)))
                                    .optional()
                            })
                            .map_err(sql)?;
                        if let Some((at, position)) = stored
                            && Hlc::parse(&at).as_ref() == Some(&clock)
                        {
                            positions.push((item.key(), position));
                            at_base.push(item);
                        }
                    }
                    let value = list.compose(&at_base);
                    (
                        Some(Row::Preference(crate::synced::SyncedPreference {
                            key: key.clone(),
                            value,
                        })),
                        positions,
                    )
                }
                _ => (
                    value.map(|value| {
                        Row::Preference(crate::synced::SyncedPreference {
                            key: key.clone(),
                            value,
                        })
                    }),
                    Vec::new(),
                ),
            };
            out.list_bases.push(SnapshotListBase {
                key,
                clock,
                value,
                items,
            });
        }
        // The rows; a list preference's whole value (not in its list's shape) is its base.
        let mut lists = ListItems::default();
        for key in synced_row_keys(c)? {
            let Some(row) = crate::annotation_store::read_row(c, &mut lists, &key)? else {
                continue;
            };
            let clock = read_row_clock(c, &key)?.as_deref().and_then(Hlc::parse);
            if let RowKey::Preference(k) = &key
                && ListPreference::of_key(k).is_some()
            {
                // One written whole before schema 16 has no base: its clock is the value's.
                if let Some(clock) = clock
                    && !out.list_bases.iter().any(|b| b.key == *k)
                {
                    out.list_bases.push(SnapshotListBase {
                        key: k.clone(),
                        clock,
                        value: Some(row),
                        items: Vec::new(),
                    });
                }
                continue;
            }
            out.rows.push(SnapshotRow { row, clock });
        }
        for (key, clock) in texts2(c, "SELECT key, clock FROM preference ORDER BY key")? {
            if ListPreference::of_key(&key).is_some()
                && let Some(clock) = clock.as_deref().and_then(Hlc::parse)
            {
                out.list_clocks.push((key, clock));
            }
        }
        // The tombstones.
        type TombstoneColumns = (
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        );
        let tombstones: Vec<TombstoneColumns> = c
            .prepare(
                "SELECT row_table, row_id, clock, coalesce(covers, clock), row, row_covers,
                        row_clock
                 FROM tombstone ORDER BY row_table, row_id",
            )
            .and_then(|mut s| {
                s.query_map([], |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                    ))
                })?
                .collect()
            })
            .map_err(sql)?;
        for (kind, id, clock, covers, row, row_covers, row_clock) in tombstones {
            // A delete from before clocks covers only a version from before clocks, which no
            // other device has of a row of this database's: it is not merged.
            let (Some(key), Some(clock)) = (row_key_of(&kind, &id), Hlc::parse(&clock)) else {
                continue;
            };
            let row = match (row, row_clock.as_deref().and_then(Hlc::parse)) {
                (Some(row), Some(row_clock)) => Some((
                    serde_json::from_str(&row)
                        .map_err(|e| StoreError::Corrupt(format!("the row of a tombstone: {e}")))?,
                    row_clock,
                    row_covers.as_deref().and_then(Hlc::parse),
                )),
                _ => None,
            };
            out.tombstones.push(SnapshotTombstone {
                key,
                clock,
                covers: covers.as_deref().and_then(Hlc::parse),
                row,
            });
        }
        out.held = self.sync_held()?;
        Ok(out)
    }

    /// Merge a snapshot of another Synced device `device` that covers its op files up to `seq`
    /// and, for every device, `applied` (its `applied_up_to`) with `merge`, in one
    /// transaction that also notes it merged: `device` merged up to `seq`, and every other
    /// device (not this one) up to the larger of what is noted here and what the snapshot
    /// covers. All of it, or (`merge` fails) none.
    pub fn merge_snapshot<T, E: From<StoreError>>(
        &self,
        device: &str,
        seq: u64,
        applied: &BTreeMap<String, u64>,
        merge: impl FnOnce(&mut MergeTx<'_>) -> Result<T, E>,
    ) -> Result<T, E> {
        let tx = self.conn.unchecked_transaction().map_err(sql)?;
        let own = read_device_id(&tx)?;
        if device == own {
            return Err(StoreError::Rejected(
                "a snapshot of this device's own is not merged".into(),
            )
            .into());
        }
        let recording = records_ops(&tx, &own)?;
        let mut m = self.merge_tx(&tx, own.clone(), recording)?;
        let out = merge(&mut m)?;
        #[cfg(feature = "test-support")]
        let recorded = std::mem::take(&mut m.recorded);
        drop(m);
        let as_i64 = |seq: u64| {
            i64::try_from(seq).map_err(|_| StoreError::Rejected(format!("op file number {seq}")))
        };
        tx.execute(
            "INSERT INTO applied_up_to (device_id, seq) VALUES (?1, ?2)
             ON CONFLICT (device_id) DO UPDATE SET seq = max(seq, excluded.seq)",
            params![device, as_i64(seq)?],
        )
        .map_err(sql)?;
        for (other, seq) in applied {
            if *other == own || other == device || !crate::annotation::is_valid_id(other) {
                continue;
            }
            tx.execute(
                "INSERT INTO applied_up_to (device_id, seq) VALUES (?1, ?2)
                 ON CONFLICT (device_id) DO UPDATE SET seq = max(seq, excluded.seq)",
                params![other, as_i64(*seq)?],
            )
            .map_err(sql)?;
        }
        tx.commit().map_err(sql)?;
        #[cfg(feature = "test-support")]
        self.recorded.borrow_mut().extend(recorded);
        Ok(out)
    }

    /// The last op file of Synced device `device` merged here, or `None` when this device has
    /// never merged any of its files or snapshots (it has not met it).
    pub fn merged_up_to(&self, device: &str) -> Result<Option<u64>, StoreError> {
        let seq: Option<i64> = self
            .conn
            .query_row(
                "SELECT seq FROM applied_up_to WHERE device_id = ?1",
                [device],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)?;
        Ok(seq.map(|s| u64::try_from(s).unwrap_or_default()))
    }

    /// The rows the outbox holds ops of (their tombstone names), not yet in an op file of the
    /// Sync folder.
    pub fn outbox_rows(&self) -> Result<Vec<(String, String)>, StoreError> {
        let changes = texts(&self.conn, "SELECT change FROM op_outbox ORDER BY id")?;
        let mut out = Vec::new();
        for change in changes {
            let value: serde_json::Value = serde_json::from_str(&change)
                .map_err(|e| StoreError::Corrupt(format!("outbox change: {e}")))?;
            for side in ["after", "before"] {
                if let Some(row) = value.get(side).filter(|v| !v.is_null()) {
                    let row: Row = serde_json::from_value(row.clone())
                        .map_err(|e| StoreError::Corrupt(format!("outbox change: {e}")))?;
                    let (kind, id) = row.key().tombstone();
                    out.push((kind.to_string(), id));
                    break;
                }
            }
        }
        out.sort();
        out.dedup();
        Ok(out)
    }

    /// Start syncing (ADR 0022 rule 5, the join): every row of Synced data with no clock yet
    /// gets one of this device's, so the snapshot that brings the rows made here to the other
    /// devices carries a clock for each, and this device records ops from now on. Nothing when
    /// it already records them.
    pub fn join_sync(&self) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction().map_err(sql)?;
        let own = read_device_id(&tx)?;
        if records_ops(&tx, &own)? {
            return Ok(());
        }
        let clock = self.next_clock(&tx)?.encode()?;
        stamp_unclocked(&tx, &clock)?;
        tx.execute(
            "INSERT INTO applied_up_to (device_id, seq) VALUES (?1, 0)
             ON CONFLICT (device_id) DO NOTHING",
            [own],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)
    }

    /// Give this database (a Backup restored, before it joins) new clocks so that its snapshot
    /// wins everywhere (ADR 0022 rule 7, "replace"): every row of Synced data, every item
    /// clock and list base, and every tombstone of a row that is not here, with what it
    /// covers, get the next clock. A row the restored data holds then replaces every version
    /// another device has; a row it deleted is deleted everywhere; a row it never knew (made
    /// since on any device) is left alone. Another device's own Note text or list item it
    /// replaces is kept there as a Sync conflict's copy or "Name (2)" (its merge never saw
    /// the snapshot's version replace it). A device whose clock runs ahead of this one's can
    /// still win with an edit stamped later.
    pub fn restamp_for_replace(&self) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction().map_err(sql)?;
        let clock = self.next_clock(&tx)?.encode()?;
        for table in SYNCED_TABLES {
            if *table == "preference" {
                continue;
            }
            tx.execute(&format!("UPDATE {table} SET clock = ?1"), [&clock])
                .map_err(sql)?;
        }
        for key in texts(&tx, "SELECT key FROM preference")? {
            if is_synced_preference(&key) {
                tx.execute(
                    "UPDATE preference SET clock = ?1 WHERE key = ?2",
                    params![clock, key],
                )
                .map_err(sql)?;
            }
        }
        stamp_unclocked(&tx, &clock)?;
        tx.execute("UPDATE item_clock SET clock = ?1", [&clock])
            .map_err(sql)?;
        let mut lists = ListItems::default();
        let tombstones: Vec<(String, String)> = tx
            .prepare("SELECT row_table, row_id FROM tombstone")
            .and_then(|mut s| {
                s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect()
            })
            .map_err(sql)?;
        for (kind, id) in tombstones {
            let here = match row_key_of(&kind, &id) {
                Some(key) => match crate::annotation_store::read_row(&tx, &mut lists, &key) {
                    Ok(row) => row.is_some(),
                    // An item of a list not in its shape is not here as a row.
                    Err(StoreError::Rejected(_)) => false,
                    Err(e) => return Err(e),
                },
                None => continue,
            };
            if !here {
                tx.execute(
                    "UPDATE tombstone SET clock = ?3, covers = ?3,
                         row_clock = CASE WHEN row IS NULL THEN row_clock ELSE ?3 END,
                         row_covers = CASE WHEN row IS NULL THEN row_covers ELSE ?3 END
                     WHERE row_table = ?1 AND row_id = ?2",
                    params![kind, id, clock],
                )
                .map_err(sql)?;
            }
        }
        tx.commit().map_err(sql)
    }
}

impl MergeTx<'_> {
    /// Give the items of list preference `list` still at its base's clock `base` the positions
    /// `items` names (a snapshot's, [`SnapshotListBase::items`]): a base merged from a
    /// snapshot holds only the items still at its clock, so their indexes in it are not the
    /// positions the whole value gave them.
    pub fn keep_base_positions(
        &mut self,
        list: &str,
        base: &Hlc,
        items: &[(RowKey, Option<i64>)],
    ) -> Result<(), StoreError> {
        if ListPreference::of_key(list).is_none() {
            return Ok(());
        }
        let base = base.encode()?;
        for (key, position) in items {
            let (kind, id) = key.tombstone();
            self.conn()
                .execute(
                    "UPDATE item_clock SET position = ?4
                     WHERE row_table = ?1 AND row_id = ?2 AND clock = ?3",
                    params![kind, id, base, position],
                )
                .map_err(sql)?;
        }
        self.forget_lists();
        Ok(())
    }
}

/// Two columns (text, optional text) of every row `query` returns.
fn texts2(c: &Connection, query: &str) -> Result<Vec<(String, Option<String>)>, StoreError> {
    c.prepare(query)
        .and_then(|mut s| {
            s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect()
        })
        .map_err(sql)
}
