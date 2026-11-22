//! A sync, and the device-id rules around it (ADR 0022 rules 5–8, plan S28c). The shells only
//! call [`sync_now`] (at launch, on resume, from a timer and on demand: `schedule.rs`) and,
//! after a Backup was restored on a syncing device, [`restore_choice`] with the user's answer.
//!
//! [`sync_now`], in order:
//!
//! 1. `sync.json`: a folder a newer app set up is left alone ([`SyncState::FolderNewer`]).
//! 2. A restore waiting for the user's choice syncs nothing ([`SyncState::NeedsRestoreChoice`]).
//! 3. **A copied database** (rule 7): op files or a snapshot under this device's id numbered
//!    above both the last of its own op files written and the last reserved were not written
//!    here (after a stop between writing a file and noting it, the file is above the first
//!    but still reserved); on a device that has not joined yet, any are another copy's (the
//!    database copied before either synced). The device takes a new id and catches up, as
//!    after a restore, before it joins.
//! 4. **Join** (rule 5): a device not recording ops gives every row without a clock one
//!    (`UserStore::join_sync`) and records ops from then; its first snapshot, written next,
//!    brings what it made before to the others.
//! 5. Its pending ops are written (an own file reserved here found there with other ops is
//!    a copy's too: a new id, as in 3); then its snapshot, when one is due
//!    (`snapshot::snapshot_due`: none yet, or enough written since).
//! 6. Every other device's snapshots and op files are merged (`merge::merge_folder`); what the
//!    merge recorded (a Sync conflict's copy, a rename, a restored parent …) is written at once.
//! 7. A snapshot again if one is now due, then this device's own op files that may go are
//!    trimmed (`snapshot::trim_own_op_files`), and its `device.json` (name, platform, app
//!    version, what it has merged of each device, when it synced) is written when it changed.

use obs_store::{StoreError, UserStore};

use super::merge::{MergeReport, merge_folder};
use super::ops::{op_file_seq, ops_dir, write_pending_ops};
use super::snapshot::{
    SnapshotPolicy, snapshot_due, snapshot_file_seq, snapshots_dir, trim_own_op_files,
    write_snapshot,
};
use super::{DeviceJson, FolderFormat, SyncError, SyncFolder, ensure_sync_json, write_device_json};

/// What [`sync_now`] needs of the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncOptions {
    /// The platform its `device.json` names (`macos`, `windows`, `linux`, `ios` …).
    pub platform: String,
    /// When it writes snapshots and trims its op files.
    pub policy: SnapshotPolicy,
}

impl SyncOptions {
    /// The options of an app on `platform`, with the default snapshot policy.
    pub fn new(platform: &str) -> Self {
        Self {
            platform: platform.to_string(),
            policy: SnapshotPolicy::default(),
        }
    }
}

/// What a sync did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// The new id this device took for being a copied database (rule 7).
    pub new_device_id: Option<String>,
    /// It joined: it records ops from now on.
    pub joined: bool,
    /// The own op files written.
    pub written: Vec<u64>,
    /// Its snapshot written, by the own op files it covers.
    pub snapshot: Option<u64>,
    /// What merging the other devices' files did.
    pub merge: MergeReport,
    /// The own op files trimmed.
    pub trimmed: Vec<u64>,
}

/// Where a sync stands, for the shells to show (S29, S30).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncState {
    /// It synced. `report.merge.needs_newer_app()` names the devices whose files a newer app
    /// wrote (left alone; this device's own ops are still written, rule 6).
    Synced(Box<SyncReport>),
    /// A newer app set up the Sync folder: nothing is read or written until this app is
    /// updated (rule 6).
    FolderNewer { format: u32 },
    /// A Backup was restored while this device was syncing: nothing syncs until the user
    /// chooses to catch up or replace ([`restore_choice`]).
    NeedsRestoreChoice,
}

/// After a Backup is restored on a syncing device (ADR 0022 rule 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreChoice {
    /// The restored data catches up with the other devices (the default): it joins as a new
    /// device, and every change in the Sync folder later than it applies.
    CatchUp,
    /// The restored data replaces what is in the Sync folder: it is given new clocks and its
    /// snapshot wins everywhere. Rows it deleted are deleted everywhere; rows it never knew
    /// (made since on any device) stay; another device's own Note text or list item it
    /// replaces stays there as a Sync conflict's copy or "Name (2)".
    Replace,
}

/// Whether a Backup restored on this device waits for the user to choose to catch up or
/// replace ([`restore_choice`]); the shells ask (S29, S30).
pub fn needs_restore_choice(user: &UserStore) -> Result<bool, SyncError> {
    Ok(user.restore_choice_pending()?)
}

/// The user's answer after a restore ([`needs_restore_choice`]), then a sync.
pub fn restore_choice(
    user: &UserStore,
    folder: &dyn SyncFolder,
    choice: RestoreChoice,
    options: &SyncOptions,
) -> Result<SyncState, SyncError> {
    if !user.restore_choice_pending()? {
        return Err(StoreError::Rejected("no restore waits for a choice".into()).into());
    }
    match choice {
        RestoreChoice::CatchUp => user.clear_restore_choice()?,
        RestoreChoice::Replace => user.restamp_for_replace()?,
    }
    sync_now(user, folder, options)
}

/// Whether the Sync folder holds op files or a snapshot under this device's id that it did
/// not write (rule 7, a copied database): a device that has not joined yet has written none
/// (a new id, a UUIDv7, never has files), so any is another copy's; one that records ops
/// wrote those numbered up to the last of its own op files written or the last reserved.
fn written_elsewhere(user: &UserStore, folder: &dyn SyncFolder) -> Result<bool, SyncError> {
    let device = user.device_id()?;
    if !user.records_ops()? {
        return Ok(folder
            .list(&ops_dir(&device))?
            .iter()
            .any(|n| op_file_seq(n).is_some())
            || folder
                .list(&snapshots_dir(&device))?
                .iter()
                .any(|n| snapshot_file_seq(n).is_some()));
    }
    let mine = user
        .applied_up_to(&device)?
        .max(user.last_reserved_op_file()?.unwrap_or(0));
    let files = folder.list(&ops_dir(&device))?;
    if files
        .iter()
        .filter_map(|n| op_file_seq(n))
        .any(|seq| seq > mine)
    {
        return Ok(true);
    }
    let written = user.applied_up_to(&device)?;
    Ok(folder
        .list(&snapshots_dir(&device))?
        .iter()
        .filter_map(|n| snapshot_file_seq(n))
        .any(|(seq, _)| seq > written))
}

/// Sync this device with `folder` now (module docs).
pub fn sync_now(
    user: &UserStore,
    folder: &dyn SyncFolder,
    options: &SyncOptions,
) -> Result<SyncState, SyncError> {
    if let FolderFormat::Newer { format } = ensure_sync_json(folder)? {
        return Ok(SyncState::FolderNewer { format });
    }
    if user.restore_choice_pending()? {
        return Ok(SyncState::NeedsRestoreChoice);
    }
    let mut report = SyncReport::default();
    if written_elsewhere(user, folder)? {
        report.new_device_id = Some(user.take_new_device_id()?);
    }
    if !user.records_ops()? {
        user.join_sync()?;
        report.joined = true;
    }
    let now = obs_store::clock::now_ms();
    report.written = match write_pending_ops(user, folder) {
        // An own file reserved here is there with other ops: a copy of this database wrote it
        // first (rule 7).
        Err(SyncError::OwnFileDiffers(_)) if report.new_device_id.is_none() => {
            report.new_device_id = Some(user.take_new_device_id()?);
            user.join_sync()?;
            report.joined = true;
            write_pending_ops(user, folder)?
        }
        written => written?,
    };
    if snapshot_due(user, folder, &options.policy, now)? {
        report.snapshot = Some(write_snapshot(user, folder)?);
    }
    report.merge = merge_folder(user, folder)?;
    if user.has_pending_ops()? {
        report.written.extend(write_pending_ops(user, folder)?);
    }
    if snapshot_due(user, folder, &options.policy, now)? {
        report.snapshot = Some(write_snapshot(user, folder)?);
    }
    report.trimmed = trim_own_op_files(user, folder, &options.policy, now)?;
    let device = user.device_id()?;
    let mut json = DeviceJson::new(&device, &user.device_name()?, &options.platform);
    json.applied = user.merged_devices()?;
    json.synced_at = Some(now);
    write_device_json(folder, &json)?;
    Ok(SyncState::Synced(Box::new(report)))
}
