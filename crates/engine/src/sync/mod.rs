//! Sync (ADR 0022): the file layer. Each Synced device keeps its User database private and
//! adds only files of its own to the Sync folder, under `Open Bible Sync/`:
//!
//! - `sync.json`: the folder's format version, written once by the first device;
//! - `devices/<device-id>/device.json`: the device's name, platform and app version, the only
//!   file a device ever writes again (its own, whole);
//! - `devices/<device-id>/ops/<seq>.jsonl`: its op files ([`ops`]), each written once;
//! - `devices/<device-id>/snapshots/<seq>-<sha256>.obsbackup`: its newest snapshot
//!   ([`snapshot`]), a Backup of its Synced data covering its op files up to `seq`.
//!
//! [`folder`] is how the engine reaches the folder, [`clock`] the hybrid logical clock every
//! op carries, [`merge`] how other devices' ops and snapshots are merged here (ADR 0022 rule
//! 3), [`snapshot`] snapshots and trimming (rule 5), [`lifecycle`] a sync and the device-id
//! rules (`sync_now`, the restore choice, rules 6–7), [`schedule`] when sync runs (rule 8).
//! `docs/dev/sync.md` documents the files, the merge and the contract the shells use.

pub mod clock;
pub mod folder;
#[cfg(not(target_arch = "wasm32"))]
pub mod lifecycle;
// The merge writes the User database, which is SQLite: not on wasm (`obs-store`).
#[cfg(all(test, not(target_arch = "wasm32")))]
mod lifecycle_tests;
#[cfg(not(target_arch = "wasm32"))]
pub mod merge;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod merge_props;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod merge_tests;
pub mod ops;
pub mod schedule;
#[cfg(not(target_arch = "wasm32"))]
pub mod snapshot;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

#[cfg(any(test, feature = "test-support"))]
pub use folder::MemSyncFolder;
pub use folder::{FolderError, FsSyncFolder, SyncFolder};
#[cfg(not(target_arch = "wasm32"))]
pub use lifecycle::{
    RestoreChoice, SyncOptions, SyncReport, SyncState, needs_restore_choice, restore_choice,
    sync_now,
};
pub use schedule::SyncSchedule;

/// The folder in the Sync folder that holds every file of ours.
pub const SYNC_ROOT: &str = "Open Bible Sync";
/// The Sync folder's `sync.json`.
pub const SYNC_JSON: &str = "Open Bible Sync/sync.json";
/// The folder of the Synced devices' folders.
pub const DEVICES_DIR: &str = "Open Bible Sync/devices";
/// The Sync folder format this build writes and reads (`sync.json`, `device.json`).
pub const SYNC_FORMAT: u32 = 1;

/// Why a sync could not go on.
#[derive(Debug)]
pub enum SyncError {
    Folder(FolderError),
    Store(obs_store::StoreError),
    /// A file of the Sync folder is not what it should be; it is left as it is.
    Damaged(String),
    /// The file at this path is only in the cloud (a placeholder); its download has started,
    /// and the next sync reads it.
    NotDownloaded(String),
    /// This device's own op file at this path, reserved here and not yet noted written, is in
    /// the Sync folder with other ops: another copy of this database wrote it under the same
    /// id (ADR 0022 rule 7). It is left as it is.
    OwnFileDiffers(String),
}

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyncError::Folder(e) => e.fmt(f),
            SyncError::Store(e) => e.fmt(f),
            SyncError::Damaged(why) => f.write_str(why),
            SyncError::NotDownloaded(path) => write!(f, "{path} is still downloading"),
            SyncError::OwnFileDiffers(path) => write!(
                f,
                "{path} is in the Sync folder with other ops than it was reserved for; it is left as it is"
            ),
        }
    }
}

impl std::error::Error for SyncError {}

impl From<FolderError> for SyncError {
    fn from(e: FolderError) -> Self {
        SyncError::Folder(e)
    }
}

impl From<obs_store::StoreError> for SyncError {
    fn from(e: obs_store::StoreError) -> Self {
        SyncError::Store(e)
    }
}

/// `sync.json`: the Sync folder's format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncJson {
    pub format: u32,
}

/// Whether this build can sync through a folder, by its `sync.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderFormat {
    /// The folder is in a format this build reads.
    Known,
    /// A newer app set the folder up: update the app to sync through it (ADR 0022 rule 6).
    Newer { format: u32 },
}

/// The bytes of a JSON file of the Sync folder: compact JSON and a newline.
fn json_file<T: Serialize>(value: &T) -> Result<Vec<u8>, SyncError> {
    let mut bytes =
        serde_json::to_vec(value).map_err(|e| SyncError::Damaged(format!("not JSON: {e}")))?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Read the folder's `sync.json`, writing it first when there is none (the first device to
/// join), and say whether this build reads the folder's format. A `sync.json` that is not one
/// is left as it is; one still downloading is [`SyncError::NotDownloaded`].
pub fn ensure_sync_json(folder: &dyn SyncFolder) -> Result<FolderFormat, SyncError> {
    if !folder.list(SYNC_ROOT)?.iter().any(|n| n == "sync.json") {
        folder.write_new(
            SYNC_JSON,
            &json_file(&SyncJson {
                format: SYNC_FORMAT,
            })?,
        )?;
        return Ok(FolderFormat::Known);
    }
    if !folder.ensure_available(SYNC_JSON)? {
        return Err(SyncError::NotDownloaded(SYNC_JSON.to_string()));
    }
    let bytes = folder.read(SYNC_JSON)?;
    let sync: SyncJson = serde_json::from_slice(&bytes)
        .map_err(|e| SyncError::Damaged(format!("{SYNC_JSON} is not one: {e}")))?;
    Ok(if sync.format > SYNC_FORMAT {
        FolderFormat::Newer {
            format: sync.format,
        }
    } else {
        FolderFormat::Known
    })
}

/// `devices/<device-id>/device.json`: what other devices show of a Synced device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceJson {
    pub format: u32,
    /// Its device id (the folder's name).
    pub device: String,
    /// The name the user gave it ("Ian's iPhone"); empty until then.
    pub name: String,
    /// `macos`, `windows`, `linux`, `ios` …
    pub platform: String,
    /// The version of the app that last wrote it.
    pub app_version: String,
    /// The last of each other device's op files it has merged, by device id (S28c): a device
    /// trims none of its own op files another device seen lately has not merged.
    #[serde(default)]
    pub applied: BTreeMap<String, u64>,
    /// When it last synced (Unix milliseconds, its own wall clock), if it has.
    #[serde(default)]
    pub synced_at: Option<i64>,
}

impl DeviceJson {
    /// This device's file, written by this build.
    pub fn new(device: &str, name: &str, platform: &str) -> Self {
        Self {
            format: SYNC_FORMAT,
            device: device.to_string(),
            name: name.to_string(),
            platform: platform.to_string(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            applied: BTreeMap::new(),
            synced_at: None,
        }
    }
}

/// The path of `device`'s `device.json`.
pub fn device_json_path(device: &str) -> String {
    format!("{DEVICES_DIR}/{device}/device.json")
}

/// Write this device's `device.json` when it is not already `device` (the only file a device
/// writes again, and only its own); returns whether it wrote.
pub fn write_device_json(folder: &dyn SyncFolder, device: &DeviceJson) -> Result<bool, SyncError> {
    let path = device_json_path(&device.device);
    let bytes = json_file(device)?;
    match folder.read(&path) {
        Ok(old) if old == bytes => return Ok(false),
        Ok(_) | Err(FolderError::NotFound(_)) => {}
        Err(e) => return Err(e.into()),
    }
    folder.write_new(&path, &bytes)?;
    Ok(true)
}

/// A Synced device with a folder in the Sync folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderDevice {
    pub id: String,
    /// Its `device.json`, or why it could not be read (not there yet, still downloading,
    /// damaged).
    pub file: Result<DeviceJson, String>,
}

/// Every Synced device with a folder in the Sync folder, by id.
pub fn read_devices(folder: &dyn SyncFolder) -> Result<Vec<FolderDevice>, SyncError> {
    let mut ids: Vec<String> = folder
        .list(DEVICES_DIR)?
        .into_iter()
        .filter(|id| obs_store::annotation::is_valid_id(id))
        .collect();
    ids.sort();
    let mut out = Vec::new();
    for id in ids {
        let path = device_json_path(&id);
        let file = match folder.ensure_available(&path) {
            Ok(true) => match folder.read(&path) {
                Ok(bytes) => serde_json::from_slice::<DeviceJson>(&bytes)
                    .map_err(|e| format!("its device.json is damaged: {e}")),
                Err(e) => Err(e.to_string()),
            },
            Ok(false) => Err("its device.json is downloading".to_string()),
            Err(e) => Err(e.to_string()),
        };
        out.push(FolderDevice { id, file });
    }
    Ok(out)
}
