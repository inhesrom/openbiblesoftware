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
//!   clash), applied like any other with the next clock and kept in the outbox.

use rusqlite::{Connection, OptionalExtension, params};

use crate::annotation::{Change, Notebook, RowKey};
use crate::annotation_store::read_default_notebook;
use crate::error::StoreError;
use crate::hlc::Hlc;
use crate::preference_items::ListPreference;
use crate::sync_clock::{COVERS_SCHEMA, Stamp, read_device_id, read_row_clock, records_ops};
use crate::synced::SyncedLayout;
use crate::synced_store::{ListItems, read_layout_by_name};
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

/// One op file's merge, in one transaction ([`UserStore::merge_op_file`]).
pub struct MergeTx<'a> {
    store: &'a UserStore,
    tx: &'a Connection,
    lists: ListItems,
    device_id: String,
    recording: bool,
    /// The Changes [`MergeTx::record`] applied, for the test-support recorder.
    #[cfg(feature = "test-support")]
    recorded: Vec<Change>,
}

impl MergeTx<'_> {
    /// This device's id.
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    /// The name the user gave this device (empty until then).
    pub fn device_name(&self) -> Result<String, StoreError> {
        self.tx
            .query_row("SELECT name FROM device WHERE id = 1", [], |r| r.get(0))
            .map_err(sql)
    }

    /// The row `key` names as it is here, with its clock and tombstone.
    pub fn state(&mut self, key: &RowKey) -> Result<RowState, StoreError> {
        let row = crate::annotation_store::read_row(self.tx, &mut self.lists, key)?;
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
            &Stamp::Merged { clock, covers },
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
    ) -> Result<(), StoreError> {
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
            .map(drop)
            .map_err(sql)
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

impl UserStore {
    /// The row `key` names as it is here, with its clock and tombstone (what the merge reads,
    /// [`MergeTx::state`]).
    pub fn row_state(&self, key: &RowKey) -> Result<RowState, StoreError> {
        let own = read_device_id(&self.conn)?;
        let mut m = MergeTx {
            store: self,
            tx: &self.conn,
            lists: ListItems::default(),
            device_id: own,
            recording: false,
            #[cfg(feature = "test-support")]
            recorded: Vec::new(),
        };
        m.state(key)
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
        if self.schema_version()? < COVERS_SCHEMA {
            return Err(
                StoreError::Rejected("this User database is too old to merge".into()).into(),
            );
        }
        let tx = self.conn.unchecked_transaction().map_err(sql)?;
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
        let mut m = MergeTx {
            store: self,
            tx: &tx,
            lists: ListItems::default(),
            device_id: own,
            recording,
            #[cfg(feature = "test-support")]
            recorded: Vec::new(),
        };
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
