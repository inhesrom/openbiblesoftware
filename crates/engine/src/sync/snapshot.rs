//! Snapshots and trimming (ADR 0022 rule 5, plan S28c).
//!
//! A Synced device sometimes writes a **snapshot** of its synced rows to
//! `devices/<device-id>/snapshots/<seq>-<sha256>.obsbackup`: a Backup (ADR 0013) holding only
//! Synced data and what merging it needs (`obs_store`'s `snapshot_store.rs`), `seq` the last
//! of the device's own op files it covers and `sha256` the hash of its bytes (the Backup
//! format carries none), so a file that is incomplete or changed is skipped and read again
//! later. Every other name in `snapshots/` is ignored. Its format is its User database schema
//! (`PRAGMA user_version`): one newer than this build reads is left alone, and the app asks
//! the user to update to sync with that device (rule 6).
//!
//! - **Written** when the device joins (its first, which brings the rows made before it
//!   recorded ops to the others) and then whenever it has written enough op files since its
//!   newest one, or the first of them is old enough ([`SnapshotPolicy`]); only with an empty
//!   outbox, right after its ops are written, so it covers exactly its own files up to `seq`.
//!   Its older snapshots are deleted once it is written.
//! - **Merged** row by row, by the same rules as ops ([`merge_snapshot`]), by a device that
//!   has not met its device yet (none of its files or snapshots merged), or whose op files
//!   after the last it merged are gone (trimmed). A device merges none of another device's op
//!   files before that device's first snapshot.
//! - **Trimming** ([`trim_own_op_files`]): a device deletes its own op file when its newest
//!   snapshot in the folder covers it, its last op is older than 90 days, and every other
//!   device seen in the last 90 days (`device.json`'s `synced_at`) has merged it (its
//!   `applied`). A device silent for longer catches up from snapshots. Tombstones are never
//!   trimmed: they are kept for good in every database and snapshot.

use obs_store::{Hlc, Row, RowKey, SnapshotContents, StoreError, UserStore, list_key_of};

use super::folder::FolderError;
use super::merge::{MergeError, MergeReport, Outcome, Recorded, merge_keyed, merge_op};
use super::ops::{Op, decode_op_file, op_file_path, op_file_seq, ops_dir};
use super::{DEVICES_DIR, SyncError, SyncFolder};

/// A snapshot's file extension: it is a Backup.
pub const SNAPSHOT_EXTENSION: &str = obs_store::BACKUP_EXTENSION;
/// How old an own op file must be before it may be trimmed (ADR 0022 rule 5), and how
/// recently another device must have synced to count as seen.
pub const TRIM_AFTER_MS: i64 = 90 * 24 * 60 * 60 * 1000;

/// When a device writes a new snapshot of its own (ADR 0022 rule 5's "sometimes").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotPolicy {
    /// After this many own op files written since its newest snapshot.
    pub after_files: u64,
    /// Or when the first own op file its newest snapshot does not cover is this old
    /// (milliseconds), so files reach the trimming age covered.
    pub after_ms: i64,
    /// Own op files older than this may be trimmed, and devices that synced within it are
    /// seen ([`TRIM_AFTER_MS`]).
    pub trim_after_ms: i64,
}

impl Default for SnapshotPolicy {
    fn default() -> Self {
        Self {
            after_files: 100,
            after_ms: 30 * 24 * 60 * 60 * 1000,
            trim_after_ms: TRIM_AFTER_MS,
        }
    }
}

/// The folder of `device`'s snapshots.
pub fn snapshots_dir(device: &str) -> String {
    format!("{DEVICES_DIR}/{device}/snapshots")
}

/// The name of the snapshot covering its device's op files up to `seq`, with bytes hashing to
/// `sha256`.
pub fn snapshot_file_name(seq: u64, sha256: &str) -> String {
    format!("{seq}-{sha256}.{SNAPSHOT_EXTENSION}")
}

/// The op files covered and the hash a snapshot's file `name` says (`<seq>-<sha256>.obsbackup`,
/// `seq` in decimal without leading zeros, `sha256` 64 lowercase hexadecimal digits); every
/// other name is none.
pub fn snapshot_file_seq(name: &str) -> Option<(u64, &str)> {
    let stem = name.strip_suffix(&format!(".{SNAPSHOT_EXTENSION}"))?;
    let (digits, hash) = stem.split_once('-')?;
    let number_ok = !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'));
    let hash_ok = hash.len() == 64 && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    if !number_ok || !hash_ok {
        return None;
    }
    Some((digits.parse().ok()?, hash))
}

/// `device`'s newest snapshot in `folder`: the op files it covers and its file's name (of two
/// covering the same, the larger name).
pub fn newest_snapshot(
    folder: &dyn SyncFolder,
    device: &str,
) -> Result<Option<(u64, String)>, FolderError> {
    Ok(folder
        .list(&snapshots_dir(device))?
        .into_iter()
        .filter_map(|name| snapshot_file_seq(&name).map(|(seq, _)| (seq, name.clone())))
        .max())
}

/// Write a snapshot of this device's data to `folder`, covering its own op files written so
/// far, and delete its older snapshots. It must record ops and have nothing in its outbox
/// ([`super::ops::write_pending_ops`] first). Returns the op files it covers.
pub fn write_snapshot(user: &UserStore, folder: &dyn SyncFolder) -> Result<u64, SyncError> {
    let file = snapshot_file(user)?;
    put_snapshot(folder, &user.device_id()?, &file)?;
    Ok(file.seq)
}

/// A snapshot of this device's data as its file: the own op files it covers, its name and
/// its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotFile {
    pub seq: u64,
    pub name: String,
    pub bytes: Vec<u8>,
}

/// This device's snapshot now ([`write_snapshot`] writes it): it must record ops and have
/// nothing in its outbox.
pub fn snapshot_file(user: &UserStore) -> Result<SnapshotFile, SyncError> {
    let device = user.device_id()?;
    if !user.records_ops()? {
        return Err(StoreError::Rejected("this device does not record ops".into()).into());
    }
    if user.has_pending_ops()? {
        return Err(StoreError::Rejected(
            "a snapshot is written only once every op is in an op file".into(),
        )
        .into());
    }
    let seq = user.applied_up_to(&device)?;
    let bytes = user.snapshot_bytes()?;
    let name = snapshot_file_name(seq, &crate::starter::sha256_hex(&bytes));
    Ok(SnapshotFile { seq, name, bytes })
}

/// Write `device`'s snapshot `file` to `folder` (unless it is there with its bytes: one of
/// its name with other bytes, damaged, is replaced) and delete its older ones.
pub fn put_snapshot(
    folder: &dyn SyncFolder,
    device: &str,
    file: &SnapshotFile,
) -> Result<(), SyncError> {
    let SnapshotFile { name, bytes, .. } = file;
    let name = name.clone();
    let dir = snapshots_dir(device);
    let names = folder.list(&dir)?;
    let path = format!("{dir}/{name}");
    let there = names.contains(&name);
    if !(there && folder.read(&path).is_ok_and(|b| b == *bytes)) {
        if there {
            folder.remove(&path)?;
        }
        folder.write_new(&path, bytes)?;
    }
    for old in names {
        if old != name && snapshot_file_seq(&old).is_some() {
            folder.remove(&format!("{dir}/{old}"))?;
        }
    }
    Ok(())
}

/// What reading a snapshot gave.
#[derive(Debug)]
pub enum SnapshotRead {
    /// It is whole: what it holds.
    Whole(Box<SnapshotContents>),
    /// A newer app wrote it (its schema): it is left alone (ADR 0022 rule 6).
    Newer { format: u32 },
    /// The provider has it only in the cloud; it is downloading.
    NotDownloaded,
    /// Incomplete, changed or damaged: why. It is read again next time.
    NotWhole(String),
}

/// Read `device`'s snapshot file `name` from `folder`, checking its hash, as a Backup.
pub fn read_snapshot(
    user: &UserStore,
    folder: &dyn SyncFolder,
    device: &str,
    name: &str,
) -> Result<SnapshotRead, SyncError> {
    let Some((_, hash)) = snapshot_file_seq(name) else {
        return Ok(SnapshotRead::NotWhole(format!(
            "{name} is not a snapshot's name"
        )));
    };
    let path = format!("{}/{name}", snapshots_dir(device));
    match folder.ensure_available(&path) {
        Ok(true) => {}
        Ok(false) => return Ok(SnapshotRead::NotDownloaded),
        Err(FolderError::NotFound(_)) => {
            return Ok(SnapshotRead::NotWhole(format!("{path} is gone")));
        }
        Err(e) => return Err(e.into()),
    }
    let bytes = match folder.read(&path) {
        Ok(bytes) => bytes,
        Err(FolderError::NotFound(_)) => {
            return Ok(SnapshotRead::NotWhole(format!("{path} is gone")));
        }
        Err(e) => return Err(e.into()),
    };
    if crate::starter::sha256_hex(&bytes) != hash {
        return Ok(SnapshotRead::NotWhole(
            "its hash is not its name's".to_string(),
        ));
    }
    let snapshot = match user.open_snapshot(&bytes) {
        Ok((snapshot, _)) => snapshot,
        Err(StoreError::NewerFormat { found, .. }) => {
            return Ok(SnapshotRead::Newer { format: found });
        }
        Err(e @ (StoreError::Corrupt(_) | StoreError::Sqlite(_))) => {
            return Ok(SnapshotRead::NotWhole(e.to_string()));
        }
        Err(e) => return Err(e.into()),
    };
    let contents = snapshot.snapshot_contents()?;
    if contents.device != device {
        return Ok(SnapshotRead::NotWhole(format!(
            "it is another device's ({}) than {device}",
            contents.device
        )));
    }
    Ok(SnapshotRead::Whole(Box::new(contents)))
}

/// One of a snapshot's rows, tombstones or held ops as an op of its device, with the row it
/// is about. A row is a write at its clock; a list's base a whole-value write at the base's
/// clock (its items still at that clock); a tombstone a delete at its newest clock covering
/// the newest version any delete of it covered, after (a parent's) the delete whose row it
/// keeps; a held op as it was. `before` is a parent's kept row, else none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotOp {
    pub key: RowKey,
    pub op: Op,
    /// For a list's base: its items still at its clock, with their positions.
    pub base_items: Vec<(RowKey, Option<i64>)>,
    /// A held op of the snapshot's (merged as any op of another device's).
    pub held: bool,
}

/// The ops a snapshot's contents merge as, in order: the rows (parents first) but list items,
/// the lists' bases, the list items, the tombstones, the held ops. Rows with no clock (none
/// after a join stamps them) are left out.
pub fn snapshot_ops(contents: &SnapshotContents) -> Vec<SnapshotOp> {
    let device = &contents.device;
    let op = |clock: &Hlc, before: Option<Row>, after: Option<Row>, before_clock: Option<Hlc>| Op {
        device: device.clone(),
        clock: clock.clone(),
        before,
        after,
        before_clock,
        absent: Vec::new(),
    };
    let mut out = Vec::new();
    let mut items = Vec::new();
    for row in &contents.rows {
        let Some(clock) = &row.clock else {
            continue;
        };
        let key = row.row.key();
        let snapshot_op = SnapshotOp {
            key: key.clone(),
            op: op(clock, None, Some(row.row.clone()), None),
            base_items: Vec::new(),
            held: false,
        };
        if list_key_of(&key).is_some() {
            items.push(snapshot_op);
        } else {
            out.push(snapshot_op);
        }
    }
    for base in &contents.list_bases {
        out.push(SnapshotOp {
            key: RowKey::Preference(base.key.clone()),
            op: op(&base.clock, None, base.value.clone(), None),
            base_items: base.items.clone(),
            held: false,
        });
    }
    out.extend(items);
    for t in &contents.tombstones {
        let mut last = None;
        if let Some((row, clock, covers)) = &t.row {
            out.push(SnapshotOp {
                key: t.key.clone(),
                op: op(clock, Some(row.clone()), None, covers.clone()),
                base_items: Vec::new(),
                held: false,
            });
            last = Some((clock.clone(), covers.clone()));
        }
        if last != Some((t.clock.clone(), t.covers.clone())) {
            out.push(SnapshotOp {
                key: t.key.clone(),
                op: op(&t.clock, None, None, t.covers.clone()),
                base_items: Vec::new(),
                held: false,
            });
        }
    }
    for held in &contents.held {
        if let Some(key) = held.change.key() {
            out.push(SnapshotOp {
                key,
                op: Op {
                    device: held.clock.device_id.clone(),
                    clock: held.clock.clone(),
                    before: held.change.before.clone(),
                    after: held.change.after.clone(),
                    before_clock: held.before_clock.clone(),
                    absent: held.absent.clone(),
                },
                base_items: Vec::new(),
                held: true,
            });
        }
    }
    out
}

/// Merge another device's snapshot (its `contents`, covering its op files up to `seq`) in one
/// transaction that notes it merged (`UserStore::merge_snapshot`). Its rows merge by the same
/// rules as ops (`merge_keyed`), each as a version this device's own rows were not seen by: a
/// snapshot does not say what each of its versions replaced, and a version later by its clock
/// may still have been written before its device merged this one's (a clock running ahead).
/// So this device's own Note text or list item that a snapshot's version replaces is kept as
/// a Sync conflict's copy or "Name (2)", even when the other device had seen it (a copy no
/// one needed, which is safe; a lost text is not).
pub fn merge_snapshot(
    user: &UserStore,
    seq: u64,
    contents: &SnapshotContents,
    report: &mut MergeReport,
) -> Result<(), MergeError> {
    let ops = snapshot_ops(contents);
    let mut outcomes = Vec::with_capacity(ops.len());
    let mut not_observed = 0;
    let mut recorded = Recorded::default();
    user.merge_snapshot(&contents.device, seq, &contents.applied, |tx| {
        for snapshot_op in &ops {
            let op = &snapshot_op.op;
            if !user.observe_clock(&op.clock) {
                not_observed += 1;
            }
            tx.begin_op()?;
            let mut by_op = Recorded::default();
            let merged = if snapshot_op.held {
                merge_op(tx, op, &[], &mut by_op)
            } else {
                merge_keyed(tx, op, &snapshot_op.key, &[], &mut by_op)
            };
            match merged {
                Ok(outcome) => {
                    if outcome == Outcome::Written
                        && let RowKey::Preference(list) = &snapshot_op.key
                        && !snapshot_op.base_items.is_empty()
                    {
                        tx.keep_base_positions(list, &op.clock, &snapshot_op.base_items)?;
                    }
                    tx.end_op(true)?;
                    recorded.add(&by_op);
                    outcomes.push(outcome);
                }
                Err(MergeError::Store(StoreError::Rejected(why) | StoreError::Conflict(why))) => {
                    tx.end_op(false)?;
                    tx.hold(&super::merge::held_of(op, why))?;
                    outcomes.push(Outcome::Held);
                }
                Err(e) => {
                    tx.end_op(false)?;
                    return Err(e);
                }
            }
        }
        for (list, clock) in &contents.list_clocks {
            tx.note_list_op(list, clock)?;
        }
        Ok::<(), MergeError>(())
    })?;
    for outcome in outcomes {
        *report.outcomes.entry(outcome).or_default() += 1;
    }
    report.not_observed += not_observed;
    report.recorded.add(&recorded);
    report.snapshots.push((contents.device.clone(), seq));
    Ok(())
}

/// Whether this device should write a new snapshot now (`policy`): it records ops, its outbox
/// is empty, and it has none in the folder, or its newest there is not whole
/// ([`read_snapshot`]), or it has written `after_files` op files since its newest, or the
/// first of them is older than `after_ms` at `now_ms`.
pub fn snapshot_due(
    user: &UserStore,
    folder: &dyn SyncFolder,
    policy: &SnapshotPolicy,
    now_ms: i64,
) -> Result<bool, SyncError> {
    let device = user.device_id()?;
    if !user.records_ops()? || user.has_pending_ops()? {
        return Ok(false);
    }
    let written = user.applied_up_to(&device)?;
    let Some((covered, name)) = newest_snapshot(folder, &device)? else {
        return Ok(true);
    };
    let by_policy = written > covered
        && (written - covered >= policy.after_files
            || last_op_wall_ms(folder, &device, covered + 1)?
                .is_some_and(|wall| now_ms.saturating_sub(wall) >= policy.after_ms));
    if by_policy {
        return Ok(true);
    }
    // One the others cannot read (partly written, changed, damaged) is written again: a
    // device writing few ops or none would otherwise leave one joining stopped on it.
    Ok(matches!(
        read_snapshot(user, folder, &device, &name)?,
        SnapshotRead::NotWhole(_)
    ))
}

/// The wall time of the last op in `device`'s op file `seq`, if it is there and whole.
fn last_op_wall_ms(
    folder: &dyn SyncFolder,
    device: &str,
    seq: u64,
) -> Result<Option<i64>, SyncError> {
    let path = op_file_path(device, seq);
    match folder.ensure_available(&path) {
        Ok(true) => {}
        Ok(false) | Err(FolderError::NotFound(_)) => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let bytes = match folder.read(&path) {
        Ok(bytes) => bytes,
        Err(FolderError::NotFound(_)) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    Ok(match decode_op_file(&bytes) {
        super::ops::OpFileRead::Whole(ops) => ops
            .last()
            .and_then(|op| i64::try_from(op.clock.wall_ms).ok()),
        _ => None,
    })
}

/// Delete this device's own op files that may go (ADR 0022 rule 5, module docs): covered by
/// its newest snapshot in the folder, their last op older than `policy.trim_after_ms` at
/// `now_ms`, and merged by every other device seen within it. A device whose `device.json`
/// cannot be read holds every file back. Returns the numbers deleted, oldest first.
pub fn trim_own_op_files(
    user: &UserStore,
    folder: &dyn SyncFolder,
    policy: &SnapshotPolicy,
    now_ms: i64,
) -> Result<Vec<u64>, SyncError> {
    let device = user.device_id()?;
    let Some((covered, _)) = newest_snapshot(folder, &device)? else {
        return Ok(Vec::new());
    };
    // What every other device seen lately has merged of this device's files.
    let mut merged_by_all = u64::MAX;
    for other in super::read_devices(folder)? {
        if other.id == device {
            continue;
        }
        let Ok(file) = other.file else {
            return Ok(Vec::new());
        };
        let seen = file
            .synced_at
            .is_some_and(|at| now_ms.saturating_sub(at) < policy.trim_after_ms);
        if seen {
            merged_by_all = merged_by_all.min(file.applied.get(&device).copied().unwrap_or(0));
        }
    }
    let mut seqs: Vec<u64> = folder
        .list(&ops_dir(&device))?
        .iter()
        .filter_map(|name| op_file_seq(name))
        .filter(|&seq| seq <= covered && seq <= merged_by_all)
        .collect();
    seqs.sort_unstable();
    let mut trimmed = Vec::new();
    for seq in seqs {
        match last_op_wall_ms(folder, &device, seq)? {
            Some(wall) if now_ms.saturating_sub(wall) >= policy.trim_after_ms => {
                folder.remove(&op_file_path(&device, seq))?;
                trimmed.push(seq);
            }
            // Files are in time order: the rest are newer.
            _ => break,
        }
    }
    Ok(trimmed)
}
