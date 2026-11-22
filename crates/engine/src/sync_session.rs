//! Sync for the desktop Session (plan S29, ADR 0022): the Sync folder this device chose, when
//! it syncs (`SyncSchedule`, driven by [`Session::sync_tick`]), what Settings → Sync, the
//! status indicator, the restore prompt and Needs attention show of it, and the commands they
//! run. Every word is in [`crate::sync::texts`] and every rule here or in [`crate::sync`]; the
//! shell lays these out and forwards input.
//!
//! - The Sync folder is a preference that stays on the device ([`SYNC_FOLDER_PREF`]); a
//!   restore or its undo puts this device's back (`backups.rs`), so restoring a Backup never
//!   starts or stops syncing, nor moves it to another folder.
//! - A sync runs on the UI thread on the Session's own connection (supervisor decision D3):
//!   the User database's clock and device id live in that connection. A due sync is marked
//!   "Syncing…" for one frame, then runs at the next [`Session::sync_tick`].
//! - **Stop syncing** gives the device a new id (`UserStore::take_new_device_id`): it records
//!   no more ops, its outbox is emptied, a restore's question is dropped, and its files stay
//!   in the folder. Choosing a folder again joins as a new Synced device.
//! - A Sync conflict's copy is resolved by [`Session::resolve_sync_conflict`]: keep this
//!   version, keep the other, keep both or combine them, each one undo step that syncs like
//!   any edit.

use std::path::PathBuf;

use obs_store::annotation::{Annotation, Change, Content, Row};
use serde_json::{Value, json};

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params};
use crate::notes::NotesFilter;
use crate::session::Session;
use crate::sync::SyncSchedule;
use crate::sync::texts;

/// The preference holding the Sync folder this device chose (its path; empty or none: this
/// device does not sync). It stays on the device (`DEVICE_PREFERENCES`).
pub const SYNC_FOLDER_PREF: &str = "sync.folder";

/// The platform this build is for, as `device.json` names it.
pub const PLATFORM: &str = std::env::consts::OS;

/// Where the Session's sync is between two frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Phase {
    #[default]
    Idle,
    /// A sync is due: shown as "Syncing…" for a frame, it runs at the next tick.
    Starting,
}

/// How the last sync ended.
#[derive(Debug, Clone, PartialEq, Eq)]
enum LastSync {
    /// It synced, at this time (Unix milliseconds).
    Synced,
    /// A newer app set up the Sync folder (rule 6).
    FolderNewer,
    /// A restore waits for Catch up or Replace (rule 7).
    NeedsRestoreChoice,
    /// The folder could not be reached or read: why.
    Failed(String),
}

/// One Synced device of the Sync folder, as read after the last sync.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FolderDeviceRow {
    id: String,
    name: String,
    platform: String,
    synced_at: Option<i64>,
    /// Why its `device.json` could not be read.
    problem: Option<String>,
}

/// The Session's sync: the folder, the schedule and what the last sync found.
#[derive(Debug)]
pub struct SessionSync {
    folder: Option<PathBuf>,
    schedule: SyncSchedule,
    phase: Phase,
    last: Option<LastSync>,
    /// An edit not yet synced was told to the schedule.
    edit_noted: bool,
    /// The Synced devices of the folder, as last read.
    devices: Vec<FolderDeviceRow>,
    /// Devices whose files a newer app wrote (rule 6), by id.
    newer_app: Vec<String>,
    /// A Backup restored here waits for Catch up or Replace.
    restore_pending: bool,
    /// How long the last sync took, in milliseconds.
    last_duration_ms: Option<i64>,
}

impl Default for SessionSync {
    fn default() -> Self {
        Self {
            folder: None,
            schedule: SyncSchedule::at_launch(true),
            phase: Phase::Idle,
            last: None,
            edit_noted: false,
            devices: Vec::new(),
            newer_app: Vec::new(),
            restore_pending: false,
            last_duration_ms: None,
        }
    }
}

/// Everything Settings → Sync shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncPanel {
    /// Which folders work on this platform.
    pub guidance: &'static str,
    /// The Sync folder; `None`: only the onboarding and Choose Sync folder… show.
    pub folder: Option<String>,
    /// The name the user gave this device (empty: none yet).
    pub device_name: String,
    /// "Last synced 2 minutes ago", "Syncing…", "Not synced yet" or why it failed.
    pub status: String,
    /// The error itself, shown on Details (#223).
    pub details: Option<String>,
    /// The status is a problem.
    pub warn: bool,
    /// A sync is under way.
    pub syncing: bool,
    /// A newer app set up the folder: update the app.
    pub folder_newer: Option<&'static str>,
    /// A restore waits for Catch up or Replace.
    pub restore_pending: bool,
    /// The Synced devices, this one first.
    pub devices: Vec<SyncDeviceRow>,
}

/// One row of Synced devices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncDeviceRow {
    /// "Ian's iPhone · iOS · last seen 5 minutes ago" ("(this device)" after this one's).
    pub line: String,
    pub this_device: bool,
    /// "Uses a newer version of Open Bible: update this app to sync with it."
    pub needs_newer_app: Option<&'static str>,
    /// Why its `device.json` could not be read (on hover).
    pub problem: Option<String>,
}

/// The status indicator in the menu bar, once a Sync folder is chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncIndicator {
    /// Its tooltip and accessible name.
    pub text: String,
    /// Something needs the user (a failure, an update, the restore's question).
    pub warn: bool,
    pub syncing: bool,
}

/// What a Sync conflict's action keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictChoice {
    /// The copy's text goes into the Note in place; the copy goes.
    KeepThis,
    /// The copy goes.
    KeepOther,
    /// The copy becomes an ordinary Note.
    KeepBoth,
    /// The Note in place gets its text, a blank line and the copy's; the copy goes.
    Combine,
}

impl ConflictChoice {
    pub const ALL: [ConflictChoice; 4] = [
        ConflictChoice::KeepThis,
        ConflictChoice::KeepOther,
        ConflictChoice::KeepBoth,
        ConflictChoice::Combine,
    ];

    pub fn id(self) -> &'static str {
        match self {
            ConflictChoice::KeepThis => "this",
            ConflictChoice::KeepOther => "other",
            ConflictChoice::KeepBoth => "both",
            ConflictChoice::Combine => "combine",
        }
    }

    /// Its button, and its undo step's name.
    pub fn label(self) -> &'static str {
        match self {
            ConflictChoice::KeepThis => texts::KEEP_THIS,
            ConflictChoice::KeepOther => texts::KEEP_OTHER,
            ConflictChoice::KeepBoth => texts::KEEP_BOTH,
            ConflictChoice::Combine => texts::COMBINE,
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.id() == id)
    }
}

impl Session {
    /// The Sync folder this device chose, if it syncs.
    pub fn sync_folder(&self) -> Option<&std::path::Path> {
        self.sync.folder.as_deref()
    }

    /// Read the Sync folder and the restore's question from the User database just attached
    /// (at launch, and after a restore): a sync is due now, as at launch.
    pub(crate) fn load_sync(&mut self) {
        let folder = (self.user.as_ref())
            .and_then(|u| u.preference(SYNC_FOLDER_PREF).ok().flatten())
            .filter(|p| !p.trim().is_empty())
            .map(PathBuf::from);
        self.sync = SessionSync {
            folder,
            ..SessionSync::default()
        };
        self.sync.restore_pending = self.read_restore_pending();
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn read_restore_pending(&self) -> bool {
        (self.user.as_ref())
            .and_then(|u| u.user_store())
            .is_some_and(|s| crate::sync::needs_restore_choice(s).unwrap_or(false))
    }

    #[cfg(target_arch = "wasm32")]
    fn read_restore_pending(&self) -> bool {
        false
    }

    /// Called every frame: notes an edit not yet synced, and runs a sync when one is due (a
    /// due sync shows "Syncing…" for a frame first). Returns in how many milliseconds the next
    /// is due (0: draw again now), or `None` without a Sync folder.
    pub fn sync_tick(&mut self, now_ms: i64) -> Option<i64> {
        self.sync.folder.as_ref()?;
        match self.sync.phase {
            Phase::Starting => self.run_due_sync(),
            Phase::Idle => {
                self.note_sync_edit(now_ms);
                if self.sync.schedule.is_due(now_ms) && !self.sync.restore_pending {
                    self.sync.phase = Phase::Starting;
                    return Some(0);
                }
            }
        }
        if self.sync.restore_pending {
            return None;
        }
        (self.sync.schedule.next_sync_due(now_ms)).map(|due| due.saturating_sub(now_ms).max(0))
    }

    /// Tell the schedule about the first edit of Synced data not yet synced (the outbox has
    /// ops), so a sync is due 30 seconds after it.
    fn note_sync_edit(&mut self, now_ms: i64) {
        if self.sync.edit_noted {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if (self.user.as_ref())
            .and_then(|u| u.user_store())
            .is_some_and(|s| s.has_pending_ops().unwrap_or(false))
        {
            self.sync.schedule.edited(now_ms);
            self.sync.edit_noted = true;
        }
        #[cfg(target_arch = "wasm32")]
        let _ = now_ms;
    }

    #[cfg(target_arch = "wasm32")]
    fn run_due_sync(&mut self) {
        self.sync.phase = Phase::Idle;
    }

    /// Run the sync that is due, on this Session's connection.
    #[cfg(not(target_arch = "wasm32"))]
    fn run_due_sync(&mut self) {
        self.sync.phase = Phase::Idle;
        let Some(folder) = self.sync.folder.clone() else {
            return;
        };
        let started = obs_store::clock::now_ms();
        let result = self.sync_once(&folder);
        self.after_sync(&folder, result, started);
    }

    /// One `sync_now` through the Sync folder at `folder`; a folder that is not there (a
    /// disk not mounted, a cloud folder moved) is not reached, and nothing is made there.
    #[cfg(not(target_arch = "wasm32"))]
    fn sync_once(
        &self,
        folder: &std::path::Path,
    ) -> Result<crate::sync::SyncState, crate::sync::SyncError> {
        let store = self.sync_store()?;
        let fs = reachable(folder)?;
        crate::sync::sync_now(store, &fs, &crate::sync::SyncOptions::new(PLATFORM))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn sync_store(&self) -> Result<&obs_store::UserStore, crate::sync::SyncError> {
        (self.user.as_ref())
            .and_then(|u| u.user_store())
            .ok_or_else(|| crate::sync::SyncError::Damaged("there is no User database".into()))
    }

    /// Take what a sync (or a restore's choice) found: its state, the devices, and the Synced
    /// data it merged, which the Session reloads.
    #[cfg(not(target_arch = "wasm32"))]
    fn after_sync(
        &mut self,
        folder: &std::path::Path,
        result: Result<crate::sync::SyncState, crate::sync::SyncError>,
        started: i64,
    ) {
        use crate::sync::SyncState;
        let finished = obs_store::clock::now_ms();
        self.sync.last_duration_ms = Some(finished.saturating_sub(started));
        match result {
            Ok(state) => {
                self.sync.schedule.synced(started, finished);
                self.sync.edit_noted = false;
                self.sync.restore_pending = matches!(state, SyncState::NeedsRestoreChoice);
                match state {
                    SyncState::Synced(report) => {
                        log::info!(
                            "synced in {} ms: {} op files and {} snapshots merged, {} written",
                            finished.saturating_sub(started),
                            report.merge.merged.len(),
                            report.merge.snapshots.len(),
                            report.written.len()
                        );
                        self.sync.last = Some(LastSync::Synced);
                        self.sync.newer_app = report.merge.needs_newer_app();
                        let merged = !report.merge.merged.is_empty()
                            || !report.merge.snapshots.is_empty()
                            || report.merge.recorded.unheld > 0;
                        if merged {
                            self.reload_synced_data();
                        }
                    }
                    SyncState::FolderNewer { .. } => self.sync.last = Some(LastSync::FolderNewer),
                    SyncState::NeedsRestoreChoice => {
                        self.sync.last = Some(LastSync::NeedsRestoreChoice);
                    }
                }
                if let Ok(fs) = reachable(folder) {
                    self.read_folder_devices(&fs);
                }
            }
            Err(e) => {
                log::warn!("sync failed: {e}");
                self.sync.schedule.sync_failed(finished);
                self.sync.last = Some(LastSync::Failed(e.to_string()));
            }
        }
    }

    /// The Synced devices of the Sync folder (their `device.json`).
    #[cfg(not(target_arch = "wasm32"))]
    fn read_folder_devices(&mut self, folder: &crate::sync::FsSyncFolder) {
        let Ok(devices) = crate::sync::read_devices(folder) else {
            return;
        };
        self.sync.devices = devices
            .into_iter()
            .map(|d| match d.file {
                Ok(file) => FolderDeviceRow {
                    id: d.id,
                    name: file.name,
                    platform: file.platform,
                    synced_at: file.synced_at,
                    problem: None,
                },
                Err(why) => FolderDeviceRow {
                    id: d.id,
                    name: String::new(),
                    platform: String::new(),
                    synced_at: None,
                    problem: Some(why),
                },
            })
            .collect();
    }

    /// Synced data changed under the Session (a merge): reload what it keeps of it, as at
    /// launch, and have views and Notes searches read it again. Preferences that stay on the
    /// device are not touched.
    pub(crate) fn reload_synced_data(&mut self) {
        if let Some(order) = (self.user.as_ref()).and_then(|u| u.preferred_order().ok()) {
            self.library.set_preferred_order(order);
        }
        self.guides.forget_arrangements();
        self.load_guide_arrangements();
        self.load_favorites_and_history();
        self.load_reading_plans();
        self.load_copy_verses();
        self.load_collections();
        self.load_saved_searches();
        self.annotations_revision += 1;
        self.rerun_searches_of_user_data();
        if let NotesFilter::Notebook(id) = &self.notes.filter
            && !(self.notebooks().unwrap_or_default().iter()).any(|n| &n.id == id)
        {
            self.notes.filter = NotesFilter::All;
        }
        // Annotations made on another revision of a Resource move with their text (#65).
        self.reanchor_library();
    }

    /// Choose the Sync folder: this device syncs through it from now on (the first sync, due
    /// at once, joins it).
    pub fn choose_sync_folder(&mut self, folder: &std::path::Path) -> Result<(), String> {
        if !folder.is_dir() {
            return Err(format!("{} is not a folder", folder.display()));
        }
        let user = self.user_data()?;
        user.set_preference(SYNC_FOLDER_PREF, &folder.to_string_lossy())?;
        self.load_sync();
        Ok(())
    }

    /// Stop syncing: this device forgets the folder and records no more ops (a new device id,
    /// its outbox emptied, a restore's question dropped); its files stay in the folder.
    pub fn stop_syncing(&mut self) -> Result<(), String> {
        let user = self.user_data()?;
        user.set_preference(SYNC_FOLDER_PREF, "")?;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(store) = user.user_store() {
            store.take_new_device_id().map_err(|e| e.to_string())?;
        }
        self.load_sync();
        Ok(())
    }

    /// Sync now (on demand): it runs at the next frames.
    pub fn request_sync(&mut self) {
        self.sync.schedule.requested();
    }

    /// The name others show of this device.
    pub fn sync_device_name(&self) -> String {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(store) = (self.user.as_ref()).and_then(|u| u.user_store()) {
            return store.device_name().unwrap_or_default();
        }
        String::new()
    }

    /// Name this device; its `device.json` takes it at the next sync.
    pub fn set_sync_device_name(&mut self, name: &str) -> Result<(), String> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(store) = (self.user.as_ref()).and_then(|u| u.user_store()) {
            return store.set_device_name(name.trim()).map_err(|e| e.to_string());
        }
        let _ = name;
        Err("There is no User database".into())
    }

    /// Catch up or Replace, after a Backup was restored on this syncing device (rule 7),
    /// then a sync.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn choose_restore(&mut self, choice: crate::sync::RestoreChoice) -> Result<(), String> {
        let folder = self.sync.folder.clone().ok_or("This device does not sync")?;
        let started = obs_store::clock::now_ms();
        let result = (|| {
            let store = self.sync_store()?;
            let fs = reachable(&folder)?;
            crate::sync::restore_choice(
                store,
                &fs,
                choice,
                &crate::sync::SyncOptions::new(PLATFORM),
            )
        })();
        let failed = result.as_ref().err().map(ToString::to_string);
        self.after_sync(&folder, result, started);
        // A choice that could not reach the folder is asked again.
        self.sync.restore_pending = self.read_restore_pending();
        match failed {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Before a restore (or its undo) replaces the User database: sync first when the folder
    /// is reachable, so edits not yet written reach it (a failure does not hold the restore
    /// back: the Safety backup has them). Returns this device's Sync folder preference, which
    /// the restore puts back.
    pub(crate) fn sync_before_restore(&mut self) -> String {
        let kept = (self.user.as_ref())
            .and_then(|u| u.preference(SYNC_FOLDER_PREF).ok().flatten())
            .unwrap_or_default();
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(folder) = self.sync.folder.clone() {
            let started = obs_store::clock::now_ms();
            let result = self.sync_once(&folder);
            if let Err(e) = &result {
                log::warn!("sync before the restore failed: {e}");
            }
            self.after_sync(&folder, result, started);
        }
        kept
    }

    /// What Settings → Sync shows, at `now_ms`.
    pub fn sync_panel(&self, now_ms: i64) -> SyncPanel {
        let (status, details, warn) = self.sync_status(now_ms);
        let own = self.own_device_id();
        let mut devices: Vec<SyncDeviceRow> = (self.sync.devices.iter())
            .map(|d| {
                let this = own.as_deref() == Some(d.id.as_str());
                let name = if this {
                    self.sync_device_name()
                } else {
                    d.name.clone()
                };
                let seen = d.synced_at.map(|at| texts::ago(now_ms, at));
                let platform = if this { PLATFORM } else { d.platform.as_str() };
                let mut line = texts::device_line(&name, platform, seen.as_deref());
                if this {
                    line.push_str(&format!(" ({})", texts::THIS_DEVICE));
                }
                SyncDeviceRow {
                    line,
                    this_device: this,
                    needs_newer_app: self
                        .sync
                        .newer_app
                        .contains(&d.id)
                        .then_some(texts::NEEDS_NEWER_APP),
                    problem: d.problem.clone(),
                }
            })
            .collect();
        devices.sort_by_key(|d| !d.this_device);
        SyncPanel {
            guidance: texts::folder_guidance(PLATFORM),
            folder: (self.sync.folder.as_ref()).map(|f| f.display().to_string()),
            device_name: self.sync_device_name(),
            status,
            details,
            warn,
            syncing: self.sync.phase == Phase::Starting,
            folder_newer: (self.sync.last == Some(LastSync::FolderNewer))
                .then_some(texts::FOLDER_NEWER),
            restore_pending: self.sync.restore_pending,
            devices,
        }
    }

    /// The status line, the error behind it, and whether it is a problem.
    fn sync_status(&self, now_ms: i64) -> (String, Option<String>, bool) {
        if self.sync.phase == Phase::Starting {
            return (texts::SYNCING.to_string(), None, false);
        }
        if self.sync.restore_pending {
            return (texts::RESTORE_TITLE.to_string(), None, true);
        }
        match &self.sync.last {
            Some(LastSync::Failed(e)) => (texts::UNREACHABLE.to_string(), Some(e.clone()), true),
            Some(LastSync::FolderNewer) => (texts::FOLDER_NEWER.to_string(), None, true),
            _ => match self.last_synced_at() {
                Some(at) => (
                    texts::last_synced(&texts::ago(now_ms, at)),
                    None,
                    !self.sync.newer_app.is_empty(),
                ),
                None => (texts::NOT_SYNCED_YET.to_string(), None, false),
            },
        }
    }

    /// When this device last synced, by its own `device.json` (kept across launches).
    fn last_synced_at(&self) -> Option<i64> {
        let own = self.own_device_id()?;
        (self.sync.devices.iter())
            .find(|d| d.id == own)
            .and_then(|d| d.synced_at)
    }

    fn own_device_id(&self) -> Option<String> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(store) = (self.user.as_ref()).and_then(|u| u.user_store()) {
            return store.device_id().ok();
        }
        None
    }

    /// The status indicator, once a Sync folder is chosen.
    pub fn sync_indicator(&self, now_ms: i64) -> Option<SyncIndicator> {
        self.sync.folder.as_ref()?;
        let (text, _, warn) = self.sync_status(now_ms);
        let warn = warn || !self.sync.newer_app.is_empty();
        let text = if !self.sync.newer_app.is_empty() && !matches!(self.sync.last, Some(LastSync::Failed(_))) {
            texts::NEEDS_NEWER_APP.to_string()
        } else {
            text
        };
        Some(SyncIndicator {
            text,
            warn,
            syncing: self.sync.phase == Phase::Starting,
        })
    }

    /// Whether the restore prompt shows: a Backup was restored while this device synced,
    /// and it waits for Catch up or Replace.
    pub fn sync_restore_pending(&self) -> bool {
        self.sync.folder.is_some() && self.sync.restore_pending
    }

    /// How long the last sync took, in milliseconds.
    pub fn last_sync_duration_ms(&self) -> Option<i64> {
        self.sync.last_duration_ms
    }

    /// The other devices' ops this device holds back (Needs attention), each as a line.
    pub fn sync_held_lines(&self) -> Vec<String> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(store) = (self.user.as_ref()).and_then(|u| u.user_store()) {
            return (store.sync_held().unwrap_or_default().iter())
                .map(|held| {
                    let name = (self.sync.devices.iter())
                        .find(|d| d.id == held.clock.device_id)
                        .map(|d| d.name.trim().to_string())
                        .filter(|n| !n.is_empty())
                        .unwrap_or_else(|| texts::UNNAMED_DEVICE.to_string());
                    texts::held_line(&name, &texts::held_thing(&held.change), &held.why)
                })
                .collect();
        }
        Vec::new()
    }

    /// The Sync conflicts' copies (ADR 0022 rule 3).
    pub fn sync_conflicts(&self) -> Vec<Annotation> {
        (self.user.as_ref())
            .and_then(|u| u.annotations(None).ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|a| a.conflict_of.is_some())
            .collect()
    }

    /// What Needs attention counts: Orphaned annotations, Sync conflicts and held ops.
    pub fn needs_attention_count(&self) -> usize {
        let annotations = (self.user.as_ref())
            .and_then(|u| u.annotations(None).ok())
            .unwrap_or_default();
        let marked = (annotations.iter())
            .filter(|a| a.is_orphaned() || a.conflict_of.is_some())
            .count();
        marked + self.sync_held_lines().len()
    }

    /// Resolve the Sync conflict whose copy is `id`, as one undo step (it syncs like any
    /// edit). Combine opens the Note in place in the Notes Tool to edit.
    pub fn resolve_sync_conflict(&mut self, id: &str, choice: ConflictChoice) -> Result<(), String> {
        let copy = self.annotation(id)?;
        let Some(of) = copy.conflict_of.clone() else {
            return Err("This Note is not a Sync conflict".into());
        };
        // The Note in place, unless it was deleted since: then the copy is all there is.
        let in_place = self.annotation(&of).ok().filter(Annotation::is_note);
        let now = obs_store::annotation::now_ms();
        let with_body = |a: &Annotation, body: String| Annotation {
            content: Content::Note { body },
            modified_at: now.max(a.modified_at),
            ..a.clone()
        };
        let delete_copy = Change::delete(Row::Annotation(copy.clone()));
        let keep_copy = Change::update(
            Row::Annotation(copy.clone()),
            Row::Annotation(Annotation {
                conflict_of: None,
                ..copy.clone()
            }),
        );
        let changes = match (choice, &in_place) {
            (ConflictChoice::KeepOther, _) => vec![delete_copy],
            (ConflictChoice::KeepBoth, _) | (_, None) => vec![keep_copy],
            (ConflictChoice::KeepThis, Some(note)) => vec![
                Change::update(
                    Row::Annotation(note.clone()),
                    Row::Annotation(with_body(note, copy.body().to_string())),
                ),
                delete_copy,
            ],
            (ConflictChoice::Combine, Some(note)) => vec![
                Change::update(
                    Row::Annotation(note.clone()),
                    Row::Annotation(with_body(
                        note,
                        format!("{}\n\n{}", note.body(), copy.body()),
                    )),
                ),
                delete_copy,
            ],
        };
        self.commit(choice.label(), changes)?;
        if let (ConflictChoice::Combine, Some(note)) = (choice, &in_place) {
            self.notes.reveal = Some((note.id.clone(), true));
        }
        Ok(())
    }
}

/// The Sync folder at `folder`, when it is there.
#[cfg(not(target_arch = "wasm32"))]
fn reachable(folder: &std::path::Path) -> Result<crate::sync::FsSyncFolder, crate::sync::SyncError> {
    if folder.is_dir() {
        Ok(crate::sync::FsSyncFolder::new(folder))
    } else {
        Err(crate::sync::SyncError::Damaged(format!(
            "{} is not there",
            folder.display()
        )))
    }
}

// ------------------------------------------------------------------------------ commands

fn has_folder(s: &Session) -> bool {
    s.user.is_some() && s.sync.folder.is_some()
}

fn has_user(s: &Session) -> bool {
    s.user.is_some()
}

fn restore_waits(s: &Session) -> bool {
    s.sync_restore_pending()
}

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

fn required<'a>(command: &'static str, p: &Params<'a>, name: &str) -> Result<&'a str, CommandError> {
    p.str(name)
        .ok_or_else(|| failed(command, format!("no {name} given")))
}

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "sync.choose_folder",
        label: texts::CHOOSE_FOLDER,
        shortcut: None,
        params: &[ParamSpec {
            name: "path",
            kind: ParamKind::String,
            required: true,
            help: "The Sync folder",
        }],
        enabled: has_user,
        run: |s, p| {
            let path = required("sync.choose_folder", &p, "path")?;
            s.choose_sync_folder(std::path::Path::new(path))
                .map_err(|e| failed("sync.choose_folder", e))?;
            Ok(json!({ "folder": path }))
        },
    },
    CommandSpec {
        id: "sync.now",
        label: texts::SYNC_NOW,
        shortcut: None,
        params: &[],
        enabled: has_folder,
        run: |s, _| {
            s.request_sync();
            Ok(Value::Null)
        },
    },
    CommandSpec {
        id: "sync.stop",
        label: texts::STOP_SYNCING,
        shortcut: None,
        params: &[],
        enabled: has_folder,
        run: |s, _| {
            s.stop_syncing().map_err(|e| failed("sync.stop", e))?;
            Ok(Value::Null)
        },
    },
    CommandSpec {
        id: "sync.device_name",
        label: texts::DEVICE_NAME_LABEL,
        shortcut: None,
        params: &[ParamSpec {
            name: "name",
            kind: ParamKind::String,
            required: true,
            help: "The name other devices show",
        }],
        enabled: has_user,
        run: |s, p| {
            let name = required("sync.device_name", &p, "name")?;
            s.set_sync_device_name(name)
                .map_err(|e| failed("sync.device_name", e))?;
            Ok(Value::Null)
        },
    },
    CommandSpec {
        id: "sync.restore_choice",
        label: texts::RESTORE_TITLE,
        shortcut: None,
        params: &[ParamSpec {
            name: "choice",
            kind: ParamKind::String,
            required: true,
            help: "`catch_up` (the default) or `replace`",
        }],
        enabled: restore_waits,
        run: |s, p| {
            let choice = required("sync.restore_choice", &p, "choice")?;
            #[cfg(not(target_arch = "wasm32"))]
            {
                let choice = match choice {
                    "catch_up" => crate::sync::RestoreChoice::CatchUp,
                    "replace" => crate::sync::RestoreChoice::Replace,
                    other => {
                        return Err(failed(
                            "sync.restore_choice",
                            format!("`{other}` is not catch_up or replace"),
                        ));
                    }
                };
                s.choose_restore(choice)
                    .map_err(|e| failed("sync.restore_choice", e))?;
                Ok(Value::Null)
            }
            #[cfg(target_arch = "wasm32")]
            {
                let _ = (s, choice);
                Err(failed("sync.restore_choice", "Sync is not available here"))
            }
        },
    },
    CommandSpec {
        id: "sync.resolve_conflict",
        label: "Resolve a Sync conflict",
        shortcut: None,
        params: &[
            ParamSpec {
                name: "id",
                kind: ParamKind::String,
                required: true,
                help: "The Sync conflict's copy",
            },
            ParamSpec {
                name: "keep",
                kind: ParamKind::String,
                required: true,
                help: "`this`, `other`, `both` or `combine`",
            },
        ],
        enabled: has_user,
        run: |s, p| {
            let id = required("sync.resolve_conflict", &p, "id")?;
            let keep = required("sync.resolve_conflict", &p, "keep")?;
            let choice = ConflictChoice::from_id(keep).ok_or_else(|| {
                failed(
                    "sync.resolve_conflict",
                    format!("`{keep}` is not this, other, both or combine"),
                )
            })?;
            s.resolve_sync_conflict(id, choice)
                .map_err(|e| failed("sync.resolve_conflict", e))?;
            Ok(Value::Null)
        },
    },
];
