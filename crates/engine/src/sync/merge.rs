//! Merging other Synced devices' ops into this User database (ADR 0022 rule 3, plan S28b-1).
//!
//! Each device's op files are merged in order, each file in one transaction with noting it
//! applied ([`UserStore::merge_op_file`]); a file that needs a row another device's file has
//! not brought yet (an Annotation's Notebook, a Highlight's style, a Reading's plan) waits,
//! rolled back, and is merged again once the other devices' next files are in, in the same
//! sync if they are there ([`merge_folder`]). The rules, per op ([`merge_op`]):
//!
//! - **Last writer wins** per row, by clock: an op writes its row only when its clock is later
//!   than the row's here, and the row keeps the op's clock (not a new one).
//! - **Edit beats delete:** a delete covers the version of the row it removed (the op's
//!   `before_clock`). A row exists when its latest write is later than everything its
//!   deletes cover, so an edit the deleting device had not seen keeps or brings back the row,
//!   whichever arrives first.
//! - **Fields an op does not carry** (an older app's) are left as they are here.
//! - **Reading done-marks** are one row per Reading, and the **Preferred order** one row, so
//!   both are last writer wins as above.
//! - **Two default Notebooks:** the older (by `created_at`, then id) stays the default on every
//!   device; whether a Notebook is the default is settled here, not by the op. The device
//!   whose own default stops being it records that, renamed after the device when the two
//!   names clash.
//! - **Two Layouts of one name** both stay: the later (the larger id, a UUIDv7) is renamed
//!   "Name (2)" by the device that finds the clash, recorded as its own edit.
//!
//! Every op's clock is observed (`UserStore::observe_clock`), so an edit made here after a
//! merge is later than what it merged. `docs/dev/sync.md` has the merge table.

use std::collections::{BTreeMap, VecDeque};

use obs_store::{Change, Content, MergeTx, Notebook, Row, RowKey, RowState, StoreError, UserStore};

use super::ops::{Op, OpsStop, read_ops_after};
use super::{DEVICES_DIR, SyncError, SyncFolder};

/// Why an op file was not merged (it is merged again later).
#[derive(Debug)]
pub enum MergeError {
    /// It needs a row that is not here yet (another device's op file has it), or its merge
    /// must wait for another device's (a delete of this device's default Notebook waits for
    /// the older default that replaces it).
    Waits(String),
    Store(StoreError),
}

impl From<StoreError> for MergeError {
    fn from(e: StoreError) -> Self {
        MergeError::Store(e)
    }
}

impl std::fmt::Display for MergeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MergeError::Waits(why) => f.write_str(why),
            MergeError::Store(e) => e.fmt(f),
        }
    }
}

/// What merging one op did here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Outcome {
    /// Its row was written (or deleted) with its clock.
    Written,
    /// The row here is as new or newer: last writer wins, it stays.
    Superseded,
    /// A write that a delete here covers: the row stays deleted.
    Covered,
    /// A delete of a version older than the row here (it had not seen this edit): the row
    /// stays.
    EditKept,
    /// A delete of a row not here: its tombstone remembers what it covers.
    DeleteNoted,
    /// A delete of a row that rows here still need (a Notebook with Annotations …): it stays,
    /// so nothing is lost (plan S28b-2 settles these).
    ChildrenKept,
}

/// What a merge did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergeReport {
    /// The op files merged, by device, in the order merged.
    pub merged: Vec<(String, u64)>,
    /// How many ops had each outcome.
    pub outcomes: BTreeMap<Outcome, usize>,
    /// The clocks not observed for being more than `MAX_FORWARD_SKEW_MS` ahead (their rows
    /// merged all the same).
    pub not_observed: usize,
    /// Devices whose next op file waits, with its number and why.
    pub waiting: Vec<(String, u64, String)>,
    /// How many times an op file waited for another device's and was merged later in the
    /// same merge.
    pub retried: usize,
    /// Devices whose op files could not all be read, where and why (`read_ops_after`).
    pub stopped: Vec<(String, u64, OpsStop)>,
}

/// A device's op files still to merge, by number, and why the first waits (if it does).
type Pending = (String, VecDeque<(u64, Vec<Op>)>, Option<String>);

/// Merge every other device's op files in `folder` not yet merged here. Each file is merged
/// whole or not at all; one that waits for another device's rows is tried again after the
/// other devices' files, until no more can be merged.
pub fn merge_folder(user: &UserStore, folder: &dyn SyncFolder) -> Result<MergeReport, SyncError> {
    let own = user.device_id()?;
    let mut ids: Vec<String> = folder
        .list(DEVICES_DIR)?
        .into_iter()
        .filter(|id| obs_store::annotation::is_valid_id(id) && *id != own)
        .collect();
    ids.sort();
    let mut report = MergeReport::default();
    let mut pending: Vec<Pending> = Vec::new();
    for id in ids {
        let ops = read_ops_after(folder, &id, user.applied_up_to(&id)?)?;
        if let Some((seq, stop)) = ops.stopped {
            report.stopped.push((id.clone(), seq, stop));
        }
        pending.push((id, ops.files.into(), None));
    }
    loop {
        let mut progress = false;
        for (device, files, waits) in &mut pending {
            while let Some((seq, ops)) = files.front() {
                match merge_file(user, device, *seq, ops, &mut report) {
                    Ok(()) => {
                        report.merged.push((device.clone(), *seq));
                        files.pop_front();
                        if waits.take().is_some() {
                            report.retried += 1;
                        }
                        progress = true;
                    }
                    Err(MergeError::Waits(why)) => {
                        *waits = Some(why);
                        break;
                    }
                    // An op this build cannot apply (it names two rows, or a row of another
                    // kind): the file waits, untouched, rather than stop every device's.
                    Err(MergeError::Store(
                        StoreError::Rejected(why) | StoreError::Conflict(why),
                    )) => {
                        *waits = Some(why);
                        break;
                    }
                    Err(MergeError::Store(e)) => return Err(e.into()),
                }
            }
        }
        if !progress {
            break;
        }
    }
    for (device, files, waits) in pending {
        if let (Some((seq, _)), Some(why)) = (files.front(), waits) {
            report.waiting.push((device, *seq, why));
        }
    }
    Ok(report)
}

/// Merge op file `seq` of `device` (its `ops`, in order) in one transaction.
fn merge_file(
    user: &UserStore,
    device: &str,
    seq: u64,
    ops: &[Op],
    report: &mut MergeReport,
) -> Result<(), MergeError> {
    let mut outcomes = Vec::with_capacity(ops.len());
    let mut not_observed = 0;
    user.merge_op_file(device, seq, |tx| {
        for op in ops {
            if !user.observe_clock(&op.clock) {
                not_observed += 1;
            }
            outcomes.push(merge_op(tx, op)?);
        }
        Ok::<(), MergeError>(())
    })?;
    for outcome in outcomes {
        *report.outcomes.entry(outcome).or_default() += 1;
    }
    report.not_observed += not_observed;
    Ok(())
}

/// Merge one op of another device (its clock already observed).
pub fn merge_op(tx: &mut MergeTx<'_>, op: &Op) -> Result<Outcome, MergeError> {
    let key = op.change().key().ok_or_else(|| {
        StoreError::Rejected(format!(
            "the op at {} of {} is not about one row",
            op.clock, op.device
        ))
    })?;
    let state = tx.state(&key)?;
    let outcome = match &op.after {
        Some(after) => merge_write(tx, op, &state, after)?,
        None => merge_delete(tx, op, &key, &state)?,
    };
    tx.note_item_op(&key, &op.clock)?;
    if std::env::var("OBS_MERGE_DEBUG").is_ok() {
        eprintln!(
            "{} merges {:?} {} at {}: {outcome:?}",
            tx.device_id(),
            key,
            if op.after.is_some() {
                "write"
            } else {
                "delete"
            },
            op.clock
        );
    }
    Ok(outcome)
}

/// A write: last writer wins, unless a delete here covers it.
fn merge_write(
    tx: &mut MergeTx<'_>,
    op: &Op,
    state: &RowState,
    after: &Row,
) -> Result<Outcome, MergeError> {
    let clock = Some(&op.clock);
    if state.row.is_some() {
        if state.clock.as_ref() >= clock {
            return Ok(Outcome::Superseded);
        }
    } else if let Some(tombstone) = &state.tombstone
        && tombstone.covers.as_ref() >= clock
    {
        return Ok(Outcome::Covered);
    }
    let mut row = with_fields_here(after, state.row.as_ref(), &op.absent)?;
    needs_parents(tx, &row, state.row.as_ref())?;
    match &mut row {
        Row::Notebook(nb) => settle_default(tx, nb, state)?,
        Row::Layout(layout) => {
            let clash = tx
                .layout_named(&layout.name)?
                .filter(|other| other.id != layout.id);
            if let Some(other) = clash {
                if layout.id > other.id {
                    // This one is the later: it is written under a free name, and the rename
                    // is recorded as this device's edit, so every device takes it.
                    layout.name = free_layout_name(tx, &layout.name)?;
                    let renamed = Row::Layout(layout.clone());
                    tx.write_merged(
                        &Change {
                            before: state.row.clone(),
                            after: Some(renamed.clone()),
                        },
                        &op.clock,
                        None,
                    )?;
                    tx.record(&[Change::update(renamed.clone(), renamed)])?;
                    return Ok(Outcome::Written);
                }
                let mut moved = other.clone();
                moved.name = free_layout_name(tx, &other.name)?;
                tx.record(&[Change::update(Row::Layout(other), Row::Layout(moved))])?;
            }
        }
        _ => {}
    }
    tx.write_merged(
        &Change {
            before: state.row.clone(),
            after: Some(row),
        },
        &op.clock,
        None,
    )?;
    Ok(Outcome::Written)
}

/// A delete: it removes the row here only if it covers the row's version.
fn merge_delete(
    tx: &mut MergeTx<'_>,
    op: &Op,
    key: &RowKey,
    state: &RowState,
) -> Result<Outcome, MergeError> {
    let covers = op.before_clock.as_ref();
    let Some(here) = &state.row else {
        tx.note_delete(key, &op.clock, covers)?;
        return Ok(Outcome::DeleteNoted);
    };
    if state.clock.as_ref() > covers {
        tx.note_delete(key, &op.clock, covers)?;
        return Ok(Outcome::EditKept);
    }
    if tx.has_children(key)? {
        return Ok(Outcome::ChildrenKept);
    }
    if let Row::Notebook(nb) = here
        && nb.is_default
    {
        // Another device deletes it as an ordinary Notebook: it has an older default, which
        // this device has not merged yet. Once it has, this one is ordinary here too.
        return Err(MergeError::Waits(format!(
            "the delete of Notebook {} waits for the default Notebook that replaces it",
            nb.id
        )));
    }
    tx.write_merged(&Change::delete(here.clone()), &op.clock, covers)?;
    Ok(Outcome::Written)
}

/// `after` with the top-level fields it did not carry (`absent`) taken from the row `here`,
/// so an older app's op does not wipe a field it does not know (ADR 0022 rule 3).
fn with_fields_here(after: &Row, here: Option<&Row>, absent: &[String]) -> Result<Row, StoreError> {
    let Some(here) = here else {
        return Ok(after.clone());
    };
    if absent.is_empty() {
        return Ok(after.clone());
    }
    let json = |e: serde_json::Error| StoreError::Rejected(format!("a row that is not JSON: {e}"));
    let (serde_json::Value::Object(mut merged), serde_json::Value::Object(kept)) = (
        serde_json::to_value(after).map_err(json)?,
        serde_json::to_value(here).map_err(json)?,
    ) else {
        return Ok(after.clone());
    };
    if merged.get("row") != kept.get("row") {
        return Ok(after.clone());
    }
    for field in absent {
        if let Some(value) = kept.get(field) {
            merged.insert(field.clone(), value.clone());
        }
    }
    serde_json::from_value(serde_json::Value::Object(merged)).map_err(json)
}

/// The rows `row` needs that are not here yet: another device's op file brings them, so this
/// file waits.
fn needs_parents(tx: &mut MergeTx<'_>, row: &Row, here: Option<&Row>) -> Result<(), MergeError> {
    let mut missing = |key: RowKey| -> Result<Option<String>, MergeError> {
        Ok(tx.state(&key)?.row.is_none().then(|| key.describe()))
    };
    let need = match row {
        Row::Annotation(a) => {
            let notebook = missing(RowKey::Notebook(a.notebook_id.clone()))?;
            match (&a.content, here) {
                _ if notebook.is_some() => notebook,
                // A Highlight not changing style is not checked again, as `apply_changes`
                // does not: its style may be gone here, and the Highlight still moves.
                (Content::Highlight { .. }, Some(Row::Annotation(h))) if h.content == a.content => {
                    None
                }
                (Content::Highlight { style }, _) => {
                    missing(RowKey::HighlightStyle(style.clone()))?
                }
                (Content::Note { .. }, _) => None,
            }
        }
        Row::HighlightStyle(s) => missing(RowKey::HighlightPalette(s.palette_id.clone()))?,
        Row::Reading(r) => missing(RowKey::ReadingPlan(r.plan_uid.clone()))?,
        _ => None,
    };
    match need {
        Some(what) => Err(MergeError::Waits(format!(
            "{} needs {what}, which another device's op file brings",
            row.key().describe()
        ))),
        None => Ok(()),
    }
}

/// Whether Notebook `a` is older than `b` (`created_at`, then id): of two default Notebooks,
/// the older stays the default.
fn older(a: &Notebook, b: &Notebook) -> bool {
    (a.created_at, &a.id) < (b.created_at, &b.id)
}

/// Settle whether merged Notebook `nb` is the default here. A Notebook already here keeps
/// what it is (the op's device may not have met the older default yet). A new one that its
/// device made its default is the default only if it is older than the one here; else the
/// one here stops being the default (every device does the same), and if it is this
/// device's own, this device records that, renamed after the device when the names clash.
fn settle_default(
    tx: &mut MergeTx<'_>,
    nb: &mut Notebook,
    state: &RowState,
) -> Result<(), MergeError> {
    if let Some(Row::Notebook(here)) = &state.row {
        nb.is_default = here.is_default;
        return Ok(());
    }
    if !nb.is_default {
        return Ok(());
    }
    let Some(default) = tx.default_notebook()? else {
        return Ok(());
    };
    if !older(nb, &default) {
        nb.is_default = false;
        return Ok(());
    }
    let mut ordinary = Notebook {
        is_default: false,
        ..default.clone()
    };
    let own = tx
        .state(&RowKey::Notebook(default.id.clone()))?
        .clock
        .is_none_or(|c| c.device_id == tx.device_id());
    let change = |after: &Notebook| {
        Change::update(Row::Notebook(default.clone()), Row::Notebook(after.clone()))
    };
    if own {
        if ordinary.name.to_lowercase() == nb.name.to_lowercase() {
            let device = tx.device_name()?;
            ordinary.name = if device.trim().is_empty() {
                format!("{} (2)", default.name)
            } else {
                device
            };
        }
        tx.record(&[change(&ordinary)])?;
    } else {
        tx.write_kept(&change(&ordinary))?;
    }
    Ok(())
}

/// The first of "`name` (2)", "`name` (3)" … that no Layout here has.
fn free_layout_name(tx: &MergeTx<'_>, name: &str) -> Result<String, StoreError> {
    for n in 2u32.. {
        let candidate = format!("{name} ({n})");
        if tx.layout_named(&candidate)?.is_none() {
            return Ok(candidate);
        }
    }
    Err(StoreError::Rejected(format!("no free name like {name}")))
}
