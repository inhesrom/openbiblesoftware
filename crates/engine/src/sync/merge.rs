//! Merging other Synced devices' ops into this User database (ADR 0022 rule 3, plan S28b-1).
//!
//! A device not met yet, or whose op files after the last merged here are gone (trimmed), is
//! first merged from its newest snapshot (`snapshot.rs`, S28c), by the same rules. Each
//! device's op files are merged in order, each file in one transaction with noting it
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
//!   whichever arrives first. Tombstones are kept for good, beside a row made again, each
//!   with the newest delete clock and the newest version any delete of it covered.
//! - **List-preference items** (Collections …) are rows as above; a list is composed by each
//!   item's stored position (its last write's), then name, so every device builds the same
//!   order, and its `preference.clock` is the newest of any op on its items.
//! - **Fields an op does not carry** (an older app's) are left as they are here.
//! - **Reading done-marks** are one row per Reading, and the **Preferred order** one row, so
//!   both are last writer wins as above.
//! - **Two default Notebooks:** the older (by `created_at`, then id) stays the default on every
//!   device; whether a Notebook is the default is settled here, not by the op. The device
//!   whose own default stops being it records that, renamed after the device when the two
//!   names clash.
//! - **Two Layouts of one name** both stay: the later (the larger id, a UUIDv7) is renamed
//!   "Name (2)" by the device that finds the clash, recorded as its own edit.
//! - **A Note's text edited on two devices** (plan S28b-2, D2): the later text stays in place;
//!   the device whose own text lost (its own row, by its id or a former one, replaced or
//!   deleted by another device's op that had not seen its text) records that text as a second Note on the same Anchor in
//!   the same Notebook, its `conflict_of` naming the Note ([`conflict_copy`]).
//! - **Deleted parents** (D3): a child arriving at a deleted Notebook, Reading plan, Highlight
//!   palette or style restores it from its tombstone's row, and a parent's delete meeting a
//!   live child here keeps it, with the rows its op file deleted with it; either way this
//!   device records the restore, so every device takes it.
//! - **Two list items of one name** (D6): the later stays under the name; the device whose own
//!   item lost to another's write that had not seen it (an add, or an edit of another version)
//!   records it again as "Name (2)"; one deleted by another device's delete that had not seen
//!   it (it deleted another version of the name) is recorded again under its own name.
//! - **A list's whole value** (D7) is its base: item ops at or before it are superseded, later
//!   ones go on top of it, or are held while it is not in its list's shape.
//! - **An op this build cannot apply** (a synced preference a later app added …) is held in
//!   `sync_held`, so the rest of its file merges; held ops are tried again at every sync.
//!
//! Every op's clock is observed (`UserStore::observe_clock`), so an edit made here after a
//! merge is later than what it merged. `docs/dev/sync.md` has the merge table.

use std::collections::{BTreeMap, VecDeque};

use obs_store::{
    Annotation, Change, Content, HeldOp, Hlc, MergeTx, Notebook, Row, RowKey, RowState, StoreError,
    UserStore, list_key_of,
};

use super::ops::{Op, OpsStop, read_ops_after};
use super::snapshot::{SnapshotRead, merge_snapshot, newest_snapshot, read_snapshot};
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
    /// A delete of a parent that rows here still need (a Notebook with Annotations, a palette
    /// with styles, a style with Highlights, a plan with Readings): it stays, and this device
    /// records it again with the rows its op file deleted with it (D3).
    ParentKept,
    /// An op this device cannot apply (a synced preference a later app added, an item of a
    /// list whose value is not in its shape): held in `sync_held` and tried again later.
    Held,
}

/// What a merge recorded or held beside the ops' outcomes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Recorded {
    /// Sync conflicts' copies recorded: this device's own Note text lost (D2).
    pub conflicts: usize,
    /// Deleted parents restored for a child arriving from another device (D3).
    pub parents_restored: usize,
    /// Rows deleted with a parent recorded again, the parent kept for a live child (D3).
    pub children_restored: usize,
    /// List items of this device's recorded again under a free name, another device's write
    /// of that name that had not seen them having won (D6).
    pub items_renamed: usize,
    /// List items of this device's recorded again under their own name, another device's
    /// delete of that name not having seen them (edit beats delete).
    pub items_kept: usize,
    /// Held ops applied or superseded on a later try.
    pub unheld: usize,
}

impl Recorded {
    pub(crate) fn add(&mut self, other: &Recorded) {
        self.conflicts += other.conflicts;
        self.parents_restored += other.parents_restored;
        self.children_restored += other.children_restored;
        self.items_renamed += other.items_renamed;
        self.items_kept += other.items_kept;
        self.unheld += other.unheld;
    }
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
    /// What the merge recorded as this device's own edits, and held ops that applied.
    pub recorded: Recorded,
    /// The other devices' snapshots merged (ADR 0022 rule 5), by device, with the op files
    /// each covers.
    pub snapshots: Vec<(String, u64)>,
}

impl MergeReport {
    /// The devices a newer app writes for, whose op files or snapshot this build left alone:
    /// update the app to sync with them (ADR 0022 rule 6).
    pub fn needs_newer_app(&self) -> Vec<String> {
        self.stopped
            .iter()
            .filter(|(_, _, stop)| {
                matches!(stop, OpsStop::Newer { .. } | OpsStop::SnapshotNewer { .. })
            })
            .map(|(device, _, _)| device.clone())
            .collect()
    }
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
    retry_held(user, &mut report)?;
    // Snapshots before other devices' op files (ADR 0022 rule 5): a device not met yet, or
    // whose op files after the last merged here are gone, is merged from its newest
    // snapshot, which also says how far it covers the other devices' files.
    let own_ids = user.own_device_ids()?;
    let mut ready = Vec::new();
    let mut snapshots = Vec::new();
    let mut no_snapshot = Vec::new();
    for id in ids {
        match snapshot_to_merge(user, folder, &id)? {
            SnapshotStep::Ops => ready.push(id),
            SnapshotStep::Nothing => {}
            SnapshotStep::Stop(_, OpsStop::NoSnapshot) => no_snapshot.push(id),
            SnapshotStep::Stop(seq, stop) => report.stopped.push((id, seq, stop)),
            SnapshotStep::Merge(seq, contents) => snapshots.push((id, seq, contents)),
        }
    }
    // This device's former ids first (a restore or a copy gave it a new id), before any
    // snapshot too, the last given up first (`own_device_ids`): their op files are its own
    // history, so their versions are here as its own before another device's replace them,
    // and the own checks see them (ADR 0022 rule 7). A snapshot, or an older former id's
    // later files (after a copy, the other copy's), merged first could make the merge record
    // again, at a new clock, a row the last id made and later edited (a Sync conflict's copy,
    // an item kept), and that record would beat the later edit.
    let mut pending = Vec::new();
    for id in own_ids.iter().filter(|id| ready.contains(id)) {
        pending.push(pending_of(user, folder, id, &mut report)?);
    }
    ready.retain(|id| !own_ids.contains(id));
    merge_pending(user, &mut pending, &mut report)?;
    // A snapshot that waits (for a row another device's file brings) is tried again after
    // the others.
    let mut waits: Vec<Option<String>> = vec![None; snapshots.len()];
    let mut merged = vec![false; snapshots.len()];
    loop {
        let mut progress = false;
        for (i, (id, seq, contents)) in snapshots.iter().enumerate() {
            if merged[i] {
                continue;
            }
            match merge_snapshot(user, *seq, contents, &mut report) {
                Ok(()) => {
                    if waits[i].take().is_some() {
                        report.retried += 1;
                    }
                    merged[i] = true;
                    ready.push(id.clone());
                    progress = true;
                }
                Err(
                    MergeError::Waits(why)
                    | MergeError::Store(StoreError::Rejected(why) | StoreError::Conflict(why)),
                ) => waits[i] = Some(why),
                Err(MergeError::Store(e)) => return Err(e.into()),
            }
        }
        if !progress {
            break;
        }
    }
    for ((id, seq, _), why) in snapshots.into_iter().zip(waits) {
        if let Some(why) = why {
            report.waiting.push((id, seq, why));
        }
    }
    // A device met through another's snapshot (which covers some of its files) needs its own
    // no more: its rows made before it recorded ops are in that snapshot too.
    for id in no_snapshot {
        if user.merged_up_to(&id)?.is_some() {
            ready.push(id);
        } else {
            report.stopped.push((id, 0, OpsStop::NoSnapshot));
        }
    }
    // Former ids met through a snapshot (theirs, their files trimmed) go first again, the
    // last given up first.
    let former_rank = |id: &String| own_ids.iter().position(|o| o == id).unwrap_or(usize::MAX);
    ready.sort_by_key(|id| (former_rank(id), id.clone()));
    for id in ready {
        pending.push(pending_of(user, folder, &id, &mut report)?);
    }
    merge_pending(user, &mut pending, &mut report)?;
    for (device, files, waits) in pending {
        if let (Some((seq, _)), Some(why)) = (files.front(), waits) {
            report.waiting.push((device, *seq, why));
        }
    }
    retry_held(user, &mut report)?;
    Ok(report)
}

/// The op files of `device` after the last merged here, to merge.
fn pending_of(
    user: &UserStore,
    folder: &dyn SyncFolder,
    device: &str,
    report: &mut MergeReport,
) -> Result<Pending, SyncError> {
    let ops = read_ops_after(folder, device, user.applied_up_to(device)?)?;
    if let Some((seq, stop)) = ops.stopped {
        report.stopped.push((device.to_string(), seq, stop));
    }
    Ok((device.to_string(), ops.files.into(), None))
}

/// Merge the `pending` op files, each device's in order, pass after pass while any merges: a
/// file that waits for another device's rows is tried again after the others.
fn merge_pending(
    user: &UserStore,
    pending: &mut [Pending],
    report: &mut MergeReport,
) -> Result<(), SyncError> {
    loop {
        let mut progress = false;
        for (device, files, waits) in pending.iter_mut() {
            while let Some((seq, ops)) = files.front() {
                match merge_file(user, device, *seq, ops, report) {
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
            return Ok(());
        }
    }
}

/// What a merge does first about another device's files.
enum SnapshotStep {
    /// Merge its op files after the last merged here.
    Ops,
    /// Nothing: it has no file yet.
    Nothing,
    /// Merge its newest snapshot (covering its op files up to the number), then its op files.
    Merge(u64, Box<obs_store::SnapshotContents>),
    /// Neither: why, at which file or snapshot.
    Stop(u64, OpsStop),
}

/// What to merge first of `device`'s: its newest snapshot when this device has not met it
/// (none of its files or snapshots merged) or its op file after the last merged here is gone
/// while a later snapshot is there (trimmed); a device not met with op files but no snapshot
/// waits for it (its first snapshot brings the rows it made before recording ops).
fn snapshot_to_merge(
    user: &UserStore,
    folder: &dyn SyncFolder,
    device: &str,
) -> Result<SnapshotStep, SyncError> {
    let met = user.merged_up_to(device)?;
    let newest = newest_snapshot(folder, device)?;
    let has_op_file = |seq: Option<u64>| -> Result<bool, SyncError> {
        Ok(folder
            .list(&super::ops::ops_dir(device))?
            .iter()
            .filter_map(|n| super::ops::op_file_seq(n))
            .any(|s| seq.is_none_or(|seq| s == seq)))
    };
    let (seq, name) = match (met, newest) {
        (None, Some(newest)) => newest,
        (None, None) if has_op_file(None)? => {
            return Ok(SnapshotStep::Stop(0, OpsStop::NoSnapshot));
        }
        (None, None) => return Ok(SnapshotStep::Nothing),
        (Some(merged), Some((seq, name))) if seq > merged && !has_op_file(Some(merged + 1))? => {
            (seq, name)
        }
        (Some(_), _) => return Ok(SnapshotStep::Ops),
    };
    Ok(match read_snapshot(user, folder, device, &name)? {
        SnapshotRead::Whole(contents) => SnapshotStep::Merge(seq, contents),
        SnapshotRead::Newer { format } => {
            SnapshotStep::Stop(seq, OpsStop::SnapshotNewer { format })
        }
        SnapshotRead::NotDownloaded => SnapshotStep::Stop(seq, OpsStop::SnapshotNotDownloaded),
        SnapshotRead::NotWhole(why) => SnapshotStep::Stop(seq, OpsStop::SnapshotNotWhole(why)),
    })
}

/// The op `op` held as.
pub(crate) fn held_of(op: &Op, why: String) -> HeldOp {
    HeldOp {
        clock: op.clock.clone(),
        change: op.change(),
        before_clock: op.before_clock.clone(),
        absent: op.absent.clone(),
        why,
    }
}

/// The op a held one was.
fn op_of(held: &HeldOp) -> Op {
    Op {
        device: held.clock.device_id.clone(),
        clock: held.clock.clone(),
        before: held.change.before.clone(),
        after: held.change.after.clone(),
        before_clock: held.before_clock.clone(),
        absent: held.absent.clone(),
    }
}

/// Try a held op again, in a savepoint: whether it applied (or a later write superseded it),
/// so it leaves `sync_held`. What it records is counted in `recorded` only if it applied, as
/// the savepoint keeps it only then.
fn retry_one(
    tx: &mut MergeTx<'_>,
    held: &HeldOp,
    recorded: &mut Recorded,
) -> Result<bool, MergeError> {
    tx.begin_op()?;
    let mut by_op = Recorded::default();
    match merge_op(tx, &op_of(held), &[], &mut by_op) {
        Ok(Outcome::Held) => {
            tx.end_op(false)?;
            Ok(false)
        }
        Ok(_) => {
            tx.end_op(true)?;
            tx.unhold(held)?;
            recorded.add(&by_op);
            recorded.unheld += 1;
            Ok(true)
        }
        Err(
            MergeError::Waits(_)
            | MergeError::Store(StoreError::Rejected(_) | StoreError::Conflict(_)),
        ) => {
            tx.end_op(false)?;
            Ok(false)
        }
        Err(e) => {
            tx.end_op(false)?;
            Err(e)
        }
    }
}

/// Try every held op again, in one transaction (nothing when none is held).
fn retry_held(user: &UserStore, report: &mut MergeReport) -> Result<(), SyncError> {
    if !user.has_sync_held()? {
        return Ok(());
    }
    let mut recorded = Recorded::default();
    user.merge_held(|tx| {
        for held in tx.held(None)? {
            retry_one(tx, &held, &mut recorded)?;
        }
        Ok::<(), MergeError>(())
    })
    .map_err(|e| match e {
        MergeError::Store(e) => SyncError::from(e),
        MergeError::Waits(why) => SyncError::Damaged(why),
    })?;
    report.recorded.add(&recorded);
    Ok(())
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
    let mut recorded = Recorded::default();
    user.merge_op_file(device, seq, |tx| {
        for op in ops {
            if !user.observe_clock(&op.clock) {
                not_observed += 1;
            }
            // Each op in a savepoint: one this build cannot apply is held, leaving nothing of
            // itself, and the rest of the file merges.
            tx.begin_op()?;
            // Counted only if the savepoint keeps what it recorded.
            let mut by_op = Recorded::default();
            match merge_op(tx, op, ops, &mut by_op) {
                Ok(outcome) => {
                    tx.end_op(true)?;
                    recorded.add(&by_op);
                    outcomes.push(outcome);
                }
                Err(MergeError::Store(StoreError::Rejected(why) | StoreError::Conflict(why))) => {
                    tx.end_op(false)?;
                    tx.hold(&held_of(op, why))?;
                    outcomes.push(Outcome::Held);
                }
                Err(e) => {
                    tx.end_op(false)?;
                    return Err(e);
                }
            }
        }
        Ok::<(), MergeError>(())
    })?;
    for outcome in outcomes {
        *report.outcomes.entry(outcome).or_default() += 1;
    }
    report.not_observed += not_observed;
    report.recorded.add(&recorded);
    Ok(())
}

/// Merge one op of another device (its clock already observed), one of the ops of `file` (the
/// op file it is in: a parent's delete keeps the rows deleted with it, D3). What it records
/// is counted in `recorded`.
pub fn merge_op(
    tx: &mut MergeTx<'_>,
    op: &Op,
    file: &[Op],
    recorded: &mut Recorded,
) -> Result<Outcome, MergeError> {
    let key = op.change().key().ok_or_else(|| {
        StoreError::Rejected(format!(
            "the op at {} of {} is not about one row",
            op.clock, op.device
        ))
    })?;
    merge_keyed(tx, op, &key, file, recorded)
}

/// Merge one op about the row `key` names (`merge_op`): a snapshot's delete of a row whose
/// deleted version it does not keep has no `before` to name it by.
pub(crate) fn merge_keyed(
    tx: &mut MergeTx<'_>,
    op: &Op,
    key: &RowKey,
    file: &[Op],
    recorded: &mut Recorded,
) -> Result<Outcome, MergeError> {
    let key = key.clone();
    if let Some(list) = list_key_of(&key) {
        if let RowKey::Preference(_) = key {
            return merge_list_base(tx, op, &key, &list, recorded);
        }
        // An item: its list's base (a whole-value write) supersedes every write before it.
        if op.after.is_some() && tx.list_base(&list)?.as_ref() >= Some(&op.clock) {
            tx.note_item_op(&key, &op.clock)?;
            return Ok(Outcome::Superseded);
        }
        if !tx.list_in_shape(&list)? {
            tx.note_item_op(&key, &op.clock)?;
            if op.after.is_none() {
                // Its tombstone says what it covers, which the value in shape to come obeys.
                tx.note_delete(&key, &op.clock, op.before_clock.as_ref(), None)?;
                return Ok(Outcome::DeleteNoted);
            }
            tx.hold(&held_of(
                op,
                format!("the value of {list} is not a list this app reads"),
            ))?;
            return Ok(Outcome::Held);
        }
    }
    let state = tx.state(&key)?;
    let outcome = match &op.after {
        Some(after) => merge_write(tx, op, &state, after, recorded)?,
        None => merge_delete(tx, op, &key, &state, file, recorded)?,
    };
    tx.note_item_op(&key, &op.clock)?;
    Ok(outcome)
}

/// A list preference's whole value (or its delete): later than the list's base here, it is
/// the new base, with the items written after it on top ([`MergeTx::write_list_base`]); the
/// items held for the list are then tried again, or dropped when it supersedes them.
fn merge_list_base(
    tx: &mut MergeTx<'_>,
    op: &Op,
    key: &RowKey,
    list: &str,
    recorded: &mut Recorded,
) -> Result<Outcome, MergeError> {
    if tx.list_base(list)?.as_ref() >= Some(&op.clock) {
        if op.after.is_none() {
            tx.note_delete(key, &op.clock, op.before_clock.as_ref(), None)?;
        }
        tx.note_list_op(list, &op.clock)?;
        return Ok(Outcome::Superseded);
    }
    let here = tx.state(key)?.row;
    tx.write_list_base(
        &Change {
            before: here,
            after: op.after.clone(),
        },
        &op.clock,
        op.before_clock.as_ref(),
    )?;
    let in_shape = tx.list_in_shape(list)?;
    for held in tx.held(Some(list))? {
        if held.clock <= op.clock {
            tx.unhold(&held)?;
            recorded.unheld += 1;
        } else if in_shape {
            retry_one(tx, &held, recorded)?;
        }
    }
    Ok(Outcome::Written)
}

/// A write: last writer wins, unless a delete here covers it.
fn merge_write(
    tx: &mut MergeTx<'_>,
    op: &Op,
    state: &RowState,
    after: &Row,
    recorded: &mut Recorded,
) -> Result<Outcome, MergeError> {
    let clock = Some(&op.clock);
    if state.row.is_some() {
        if state.clock.as_ref() >= clock {
            former_own_lost(tx, op, state, after, recorded)?;
            return Ok(Outcome::Superseded);
        }
    } else if let Some(tombstone) = &state.tombstone
        && tombstone.covers.as_ref() >= clock
    {
        former_own_lost(tx, op, state, after, recorded)?;
        return Ok(Outcome::Covered);
    }
    let mut row = with_fields_here(after, state.row.as_ref(), &op.absent)?;
    ensure_parents(tx, &row, state.row.as_ref(), recorded)?;
    let copy = conflict_copy(tx, op, state, Some(&row))?;
    let lost_item = own_item_lost(tx, op, state, &row);
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
    if let Some(copy) = copy {
        tx.record(&[Change::insert(Row::Annotation(copy))])?;
        recorded.conflicts += 1;
    }
    if let Some(item) = lost_item
        && let Some(renamed) = tx.renamed_for_clash(&item)?
    {
        tx.record_after(&[Change::insert(renamed)], state.clock.as_ref())?;
        recorded.items_renamed += 1;
    }
    Ok(Outcome::Written)
}

/// An op of one of this device's former ids (written before a restore or a copy gave it a
/// new id, ADR 0022 rule 7) that loses here to another device's version (`state`, later, or a
/// delete covering it that is not this op's own version): the device that wrote it would have
/// kept its Note text as a Sync conflict's copy (D2) or its list item as "Name (2)", or under
/// its own name against a delete (D6), when that version replaced it. That device is this one
/// now, so this one does, whatever order the ops come in. Nothing when the version here came
/// after the op in this device's own history (its id now, the op's id, or a former id given up
/// after the op's: [`MergeTx::is_own_after`]; an older former id's can be a copied database's
/// original's) or holds the op's text. Such a copy may be one no one needed (the other device
/// had seen the op's text), which is safe.
fn former_own_lost(
    tx: &mut MergeTx<'_>,
    op: &Op,
    state: &RowState,
    after: &Row,
    recorded: &mut Recorded,
) -> Result<(), MergeError> {
    let writer = &op.clock.device_id;
    if writer == tx.device_id() || !tx.is_own(writer) {
        return Ok(());
    }
    if state
        .clock
        .as_ref()
        .is_some_and(|c| tx.is_own_after(&c.device_id, writer))
    {
        return Ok(());
    }
    if state.row.is_none()
        && state.tombstone.as_ref().is_some_and(|t| {
            t.covers.as_ref() == Some(&op.clock)
                || t.covers
                    .as_ref()
                    .is_some_and(|c| tx.is_own_after(&c.device_id, writer))
        })
    {
        // The delete removed this version, or a later one of this device's own history: it
        // had seen it.
        return Ok(());
    }
    match (after, &state.row) {
        (Row::Annotation(mine), here) if mine.is_note() => {
            let place = match here {
                Some(Row::Annotation(here)) if here.is_note() && here.body() == mine.body() => {
                    return Ok(());
                }
                Some(Row::Annotation(here)) => here,
                _ => mine,
            };
            let id = conflict_id(&mine.id, &op.clock);
            let there = tx.state(&RowKey::Annotation(id.clone()))?;
            if there.row.is_some() || there.tombstone.is_some() {
                return Ok(());
            }
            let copy = Row::Annotation(Annotation {
                id,
                notebook_id: place.notebook_id.clone(),
                content: Content::Note {
                    body: mine.body().to_string(),
                },
                anchor: place.anchor.clone(),
                created_at: mine.created_at,
                modified_at: mine.modified_at,
                conflict_of: Some(mine.id.clone()),
            });
            ensure_parents(tx, &copy, None, recorded)?;
            tx.record(&[Change::insert(copy)])?;
            recorded.conflicts += 1;
        }
        (
            Row::Collection(mine) | Row::CopyPreset(mine) | Row::SavedSearch(mine),
            Some(Row::Collection(here) | Row::CopyPreset(here) | Row::SavedSearch(here)),
        ) => {
            if here.json != mine.json
                && let Some(renamed) = tx.renamed_for_clash(after)?
            {
                tx.record_after(&[Change::insert(renamed)], Some(&op.clock))?;
                recorded.items_renamed += 1;
            }
        }
        (Row::Collection(_) | Row::CopyPreset(_) | Row::SavedSearch(_), None) => {
            // Deleted by a delete that never saw it: recorded again under its own name, after
            // what the tombstone covers and the op's own clock.
            tx.record_after(&[Change::insert(after.clone())], Some(&op.clock))?;
            recorded.items_kept += 1;
        }
        _ => {}
    }
    Ok(())
}

/// The id of the Sync conflict's copy of Note `note` holding its text at `lost` (the clock of
/// the version that lost): a UUID-shaped hash of both, so it is the same wherever it is made.
pub(crate) fn conflict_id(note: &str, lost: &Hlc) -> String {
    let hex = crate::starter::sha256_hex(format!("{note} {lost}").as_bytes());
    let h = hex.as_bytes();
    let part = |r: std::ops::Range<usize>| String::from_utf8_lossy(&h[r]).into_owned();
    // RFC 9562 version 8 (custom), variant 10.
    let variant = ["8", "9", "a", "b"][usize::from(h[16] % 4)];
    format!(
        "{}-{}-8{}-{variant}{}-{}",
        part(0..8),
        part(8..12),
        part(13..16),
        part(17..20),
        part(20..32)
    )
}

/// The Sync conflict's copy to record when `op` replaces (`winner`, the row it writes) or
/// deletes (`None`) this device's own Note whose text it had not seen (its `before` text is
/// another, or it has none: an insert) and that text would be lost (D2): the text that lost,
/// on the Anchor and in the Notebook of the Note in place (or of the Note here, when it is
/// deleted), `conflict_of` naming it. None when the copy is here already, or was deleted.
fn conflict_copy(
    tx: &mut MergeTx<'_>,
    op: &Op,
    state: &RowState,
    winner: Option<&Row>,
) -> Result<Option<Annotation>, MergeError> {
    let (Some(Row::Annotation(ours)), Some(lost)) = (&state.row, &state.clock) else {
        return Ok(None);
    };
    let Content::Note { body: mine } = &ours.content else {
        return Ok(None);
    };
    if !tx.is_own(&lost.device_id) {
        return Ok(None);
    }
    let note_text = |row: Option<&Row>| match row {
        Some(Row::Annotation(a)) if a.is_note() => Some(a.body().to_string()),
        _ => None,
    };
    if note_text(op.before.as_ref()).as_ref() == Some(mine)
        || note_text(winner).as_ref() == Some(mine)
    {
        return Ok(None);
    }
    let id = conflict_id(&ours.id, lost);
    let there = tx.state(&RowKey::Annotation(id.clone()))?;
    if there.row.is_some() || there.tombstone.is_some() {
        return Ok(None);
    }
    let place = match winner {
        Some(Row::Annotation(a)) => a,
        _ => ours,
    };
    Ok(Some(Annotation {
        id,
        notebook_id: place.notebook_id.clone(),
        content: Content::Note { body: mine.clone() },
        anchor: place.anchor.clone(),
        created_at: ours.created_at,
        modified_at: ours.modified_at,
        conflict_of: Some(ours.id.clone()),
    }))
}

/// This device's own list item here (a Collection, Copy preset or Saved search: the row's
/// clock is this device's) that `op` had not seen: it is an add (no `before`), or its `before`
/// is another version (its `before_clock` not this row's clock, its JSON another). A delete
/// with no `before` (a snapshot's tombstone) had seen it when it covers this row's clock.
fn own_unseen_item<'s>(
    tx: &MergeTx<'_>,
    op: &Op,
    state: &'s RowState,
) -> Option<&'s obs_store::SyncedListItem> {
    let (Row::Collection(ours) | Row::CopyPreset(ours) | Row::SavedSearch(ours)) =
        state.row.as_ref()?
    else {
        return None;
    };
    if !tx.is_own(&state.clock.as_ref()?.device_id) {
        return None;
    }
    let unseen = match &op.before {
        None if op.after.is_none() => op.before_clock != state.clock,
        None => true,
        Some(Row::Collection(before) | Row::CopyPreset(before) | Row::SavedSearch(before)) => {
            op.before_clock != state.clock && before.json != ours.json
        }
        Some(_) => false,
    };
    unseen.then_some(ours)
}

/// This device's own list item that `op`, another device's write of an item of that name with
/// other JSON, replaces without having seen it ([`own_unseen_item`], D6). It is recorded
/// again under a free name, after its own clock ([`MergeTx::record_after`]). So two devices' concurrent edits of one item keep both, the
/// earlier as "Name (2)", as D2 keeps both Note texts.
fn own_item_lost(tx: &MergeTx<'_>, op: &Op, state: &RowState, winner: &Row) -> Option<Row> {
    let ours = own_unseen_item(tx, op, state)?;
    match winner {
        Row::Collection(w) | Row::CopyPreset(w) | Row::SavedSearch(w) if w.json != ours.json => {
            state.row.clone()
        }
        _ => None,
    }
}

/// A delete: it removes the row here only if it covers the row's version.
fn merge_delete(
    tx: &mut MergeTx<'_>,
    op: &Op,
    key: &RowKey,
    state: &RowState,
    file: &[Op],
    recorded: &mut Recorded,
) -> Result<Outcome, MergeError> {
    let covers = op.before_clock.as_ref();
    let deleted = op.before.as_ref();
    let Some(here) = &state.row else {
        tx.note_delete(key, &op.clock, covers, deleted)?;
        return Ok(Outcome::DeleteNoted);
    };
    if state.clock.as_ref() > covers {
        tx.note_delete(key, &op.clock, covers, deleted)?;
        return Ok(Outcome::EditKept);
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
    if tx.has_children(key)? {
        // D3: a child here the delete had not seen keeps its parent. The delete is noted, and
        // this device records the parent again (after what the tombstone covers) with the
        // rows its op file deleted with it, so every device keeps them.
        tx.note_delete(key, &op.clock, covers, deleted)?;
        tx.record(&[Change::update(here.clone(), here.clone())])?;
        restore_deleted_with(tx, key, file, recorded)?;
        return Ok(Outcome::ParentKept);
    }
    if own_unseen_item(tx, op, state).is_some() {
        // This device's own list item, which the delete never saw (it deleted another
        // version of that name): edit beats delete. The delete is noted, and this device
        // records its item again, after what the tombstone covers, so every device keeps it.
        tx.note_delete(key, &op.clock, covers, deleted)?;
        tx.record(&[Change::update(here.clone(), here.clone())])?;
        recorded.items_kept += 1;
        return Ok(Outcome::EditKept);
    }
    if let Some(copy) = conflict_copy(tx, op, state, None)? {
        tx.record(&[Change::insert(Row::Annotation(copy))])?;
        recorded.conflicts += 1;
    }
    tx.delete_merged(here, deleted, &op.clock, covers)?;
    Ok(Outcome::Written)
}

/// Whether `child` is a row that needs the parent `parent` names.
fn is_child_of(child: &Row, parent: &RowKey) -> bool {
    match (child, parent) {
        (Row::Annotation(a), RowKey::Notebook(id)) => a.notebook_id == *id,
        (Row::Annotation(a), RowKey::HighlightStyle(id)) => {
            matches!(&a.content, Content::Highlight { style } if style == id)
        }
        (Row::HighlightStyle(s), RowKey::HighlightPalette(id)) => s.palette_id == *id,
        (Row::Reading(r), RowKey::ReadingPlan(uid)) => r.plan_uid == *uid,
        _ => false,
    }
}

/// Record again the children of `parent` that the op file `file` deleted and that are not
/// here (D3: "a deleted Notebook and its Annotations" come back). One whose other parent is
/// gone for good is left deleted, and so is one this device's user deleted too, later: its
/// tombstone here has a clock other than the file's delete.
fn restore_deleted_with(
    tx: &mut MergeTx<'_>,
    parent: &RowKey,
    file: &[Op],
    recorded: &mut Recorded,
) -> Result<(), MergeError> {
    for op in file {
        let (Some(child), None) = (&op.before, &op.after) else {
            continue;
        };
        if !is_child_of(child, parent) {
            continue;
        }
        let here = tx.state(&child.key())?;
        let deleted_by_this_op = here
            .tombstone
            .as_ref()
            .is_some_and(|t| t.clock.as_ref() == Some(&op.clock));
        if here.row.is_some() || !deleted_by_this_op {
            continue;
        }
        match ensure_parents(tx, child, None, recorded) {
            Ok(()) => {}
            Err(MergeError::Waits(_) | MergeError::Store(StoreError::Rejected(_))) => continue,
            Err(e) => return Err(e),
        }
        tx.record(&[Change::insert(child.clone())])?;
        recorded.children_restored += 1;
    }
    Ok(())
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

/// Make sure the rows `row` needs are here: a deleted one is restored from its tombstone's
/// row and recorded (D3; its own parents first: a style's palette); one not here yet waits
/// for another device's op file to bring it, unless its own op is held here; one deleted
/// before schema 16 (its tombstone has no row) or held is refused, so the op is held until
/// the parent is here.
fn ensure_parents(
    tx: &mut MergeTx<'_>,
    row: &Row,
    here: Option<&Row>,
    recorded: &mut Recorded,
) -> Result<(), MergeError> {
    let mut needs = Vec::new();
    match row {
        Row::Annotation(a) => {
            needs.push(RowKey::Notebook(a.notebook_id.clone()));
            match (&a.content, here) {
                // A Highlight not changing style is not checked again, as `apply_changes`
                // does not: its style may be gone here, and the Highlight still moves.
                (Content::Highlight { .. }, Some(Row::Annotation(h))) if h.content == a.content => {
                }
                (Content::Highlight { style }, _) => {
                    needs.push(RowKey::HighlightStyle(style.clone()))
                }
                (Content::Note { .. }, _) => {}
            }
        }
        Row::HighlightStyle(s) => needs.push(RowKey::HighlightPalette(s.palette_id.clone())),
        Row::Reading(r) => needs.push(RowKey::ReadingPlan(r.plan_uid.clone())),
        _ => {}
    }
    for parent in needs {
        let state = tx.state(&parent)?;
        if state.row.is_some() {
            continue;
        }
        match tx.tombstone_row(&parent)? {
            Some(restored) => {
                ensure_parents(tx, &restored, None, recorded)?;
                tx.record(&[Change::insert(restored)])?;
                recorded.parents_restored += 1;
            }
            None if state.tombstone.is_some() => {
                return Err(StoreError::Rejected(format!(
                    "{} needs {}, deleted here before this app kept deleted rows: it is held \
                     until another device brings it back",
                    row.key().describe(),
                    parent.describe()
                ))
                .into());
            }
            // Held here itself (a child of a parent deleted before schema 16, or an op this
            // build refuses): the child is held too, so its file merges on, and they are tried
            // again in clock order, the parent first.
            None if tx.is_held(&parent)? => {
                return Err(StoreError::Rejected(format!(
                    "{} needs {}, which is held here: it is held until that applies",
                    row.key().describe(),
                    parent.describe()
                ))
                .into());
            }
            None => {
                return Err(MergeError::Waits(format!(
                    "{} needs {}, which another device's op file brings",
                    row.key().describe(),
                    parent.describe()
                )));
            }
        }
    }
    Ok(())
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
        .is_none_or(|c| tx.is_own(&c.device_id));
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
