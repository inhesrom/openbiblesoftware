//! Sync for the desktop Session (plan S29, ADR 0022): the Sync folder this device chose, when
//! it syncs (`SyncSchedule`, driven by [`Session::sync_tick`]), what Settings → Sync, the
//! status indicator, the restore prompt and Needs attention show of it, and the commands they
//! run. Every word is in [`crate::sync::texts`] and every rule here or in [`crate::sync`]; the
//! shell lays these out and forwards input.
//!
//! - The Sync folder is a preference that stays on the device ([`SYNC_FOLDER_PREF`]); a
//!   restore or its undo puts this device's back (`backups.rs`), so restoring a Backup never
//!   starts or stops syncing, nor moves it to another folder.
//! - A sync runs off the UI thread (plan S29b, `sync_worker.rs`): a due sync starts on its
//!   worker at a [`Session::sync_tick`], shows "Syncing…" until its result is taken at a
//!   later tick, and every read of the Sync folder is there. Choosing a folder and Stop syncing
//!   wait for a sync running (still "Syncing…") and then apply; a restore waits for it, and for
//!   the sync it runs first, at most [`RESTORE_SYNC_WAIT`].
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
#[cfg(not(target_arch = "wasm32"))]
pub use crate::sync_worker::RESTORE_SYNC_WAIT;
#[cfg(not(target_arch = "wasm32"))]
use crate::sync_worker::{SyncOutcome, SyncTask, SyncWorker};

/// The preference holding the Sync folder this device chose (its path; empty or none: this
/// device does not sync). It stays on the device (`DEVICE_PREFERENCES`).
pub const SYNC_FOLDER_PREF: &str = "sync.folder";

/// The platform this build is for, as `device.json` names it.
pub const PLATFORM: &str = std::env::consts::OS;

/// Why the last sync failed when a restore did not wait for it (its Details).
const RESTORE_SYNC_UNFINISHED: &str = "The sync before the restore did not finish in time, so \
     the restore went ahead without it; the Safety backup holds the edits it had not written.";

/// How often, in milliseconds, the frames look for the result of a sync running on its worker.
pub(crate) const SYNC_POLL_MS: i64 = 50;

/// Where the Session's sync is between two frames.
#[derive(Debug, Default)]
enum Phase {
    #[default]
    Idle,
    /// A sync runs on its worker ("Syncing…").
    #[cfg(not(target_arch = "wasm32"))]
    Running(SyncWorker),
}

/// What waits for the sync running to end, then applies (still "Syncing…" meanwhile).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
enum Deferred {
    ChooseFolder(PathBuf),
    StopSyncing,
}

/// How the last sync ended (none syncs on wasm).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
enum LastSync {
    /// It synced (when: this device's `device.json`).
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
    /// The platform `device.json` names and whose folder guidance Settings shows
    /// ([`PLATFORM`]; the gallery draws one platform's on every computer).
    platform: &'static str,
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
    /// Settings → Sync's action waiting for the sync running.
    deferred: Option<Deferred>,
    /// How long a restore waits for the sync before it ([`RESTORE_SYNC_WAIT`]; tests wait
    /// less).
    #[cfg(not(target_arch = "wasm32"))]
    restore_wait: std::time::Duration,
    /// The Sync folder tests give the worker instead of the one at [`SessionSync::folder`].
    #[cfg(all(test, not(target_arch = "wasm32")))]
    test_folder: Option<TestFolder>,
}

/// A Sync folder a test gives the worker.
#[cfg(all(test, not(target_arch = "wasm32")))]
#[derive(Clone)]
pub(crate) struct TestFolder(pub(crate) std::sync::Arc<dyn crate::sync::SyncFolder>);

#[cfg(all(test, not(target_arch = "wasm32")))]
impl std::fmt::Debug for TestFolder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TestFolder")
    }
}

impl SessionSync {
    fn syncing(&self) -> bool {
        !matches!(self.phase, Phase::Idle)
    }
}

impl Default for SessionSync {
    fn default() -> Self {
        Self {
            platform: PLATFORM,
            folder: None,
            schedule: SyncSchedule::at_launch(true),
            phase: Phase::Idle,
            last: None,
            edit_noted: false,
            devices: Vec::new(),
            newer_app: Vec::new(),
            restore_pending: false,
            last_duration_ms: None,
            deferred: None,
            #[cfg(not(target_arch = "wasm32"))]
            restore_wait: RESTORE_SYNC_WAIT,
            #[cfg(all(test, not(target_arch = "wasm32")))]
            test_folder: None,
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

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.id() == id)
    }
}

/// A Sync conflict's actions, each with its button (and its undo step's name): the four while
/// the Note in place is there (`in_place`), else, when another device deleted it, Keep this
/// text (the copy becomes an ordinary Note) and Let it go (the copy is deleted).
pub fn conflict_actions(in_place: bool) -> &'static [(ConflictChoice, &'static str)] {
    if in_place {
        &[
            (ConflictChoice::KeepThis, texts::KEEP_THIS),
            (ConflictChoice::KeepOther, texts::KEEP_OTHER),
            (ConflictChoice::KeepBoth, texts::KEEP_BOTH),
            (ConflictChoice::Combine, texts::COMBINE),
        ]
    } else {
        &[
            (ConflictChoice::KeepBoth, texts::KEEP_THIS_TEXT),
            (ConflictChoice::KeepOther, texts::LET_IT_GO),
        ]
    }
}

impl Session {
    /// The Sync folder this device chose, if it syncs.
    pub fn sync_folder(&self) -> Option<&std::path::Path> {
        self.sync.folder.as_deref()
    }

    /// Sync as an app on `platform` (`macos`, `windows`, `linux`, `ios`): its `device.json` and
    /// the folder guidance Settings shows. Tests and the gallery draw one platform's words on
    /// every computer; the app keeps [`PLATFORM`].
    pub fn set_sync_platform(&mut self, platform: &'static str) {
        self.sync.platform = platform;
    }

    /// Have the worker sync through `folder` rather than the Sync folder's path.
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn set_sync_test_folder(
        &mut self,
        folder: std::sync::Arc<dyn crate::sync::SyncFolder>,
    ) {
        self.sync.test_folder = Some(TestFolder(folder));
    }

    /// Have a restore wait `wait` for its sync first rather than [`RESTORE_SYNC_WAIT`].
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn set_restore_sync_wait(&mut self, wait: std::time::Duration) {
        self.sync.restore_wait = wait;
    }

    /// Read the Sync folder and the restore's question from the User database just attached
    /// (at launch, and after a restore): a sync is due now, as at launch.
    pub(crate) fn load_sync(&mut self) {
        let folder = (self.user.as_ref())
            .and_then(|u| u.preference(SYNC_FOLDER_PREF).ok().flatten())
            .filter(|p| !p.trim().is_empty())
            .map(PathBuf::from);
        let old = std::mem::take(&mut self.sync);
        // A sync still running was on the User database this replaces.
        #[cfg(not(target_arch = "wasm32"))]
        if let Phase::Running(worker) = old.phase {
            worker.abandon();
        }
        self.sync = SessionSync {
            platform: old.platform,
            folder,
            #[cfg(not(target_arch = "wasm32"))]
            restore_wait: old.restore_wait,
            #[cfg(all(test, not(target_arch = "wasm32")))]
            test_folder: old.test_folder,
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

    /// Called every frame: notes an edit not yet synced, starts a sync on its worker when one
    /// is due, and takes a running sync's result once it is there (then reloads the Synced
    /// data it merged, and applies what waited for it). Returns in how many milliseconds to
    /// call again (the next sync due, or soon while one runs), or `None` without a Sync folder.
    pub fn sync_tick(&mut self, now_ms: i64) -> Option<i64> {
        self.sync.folder.as_ref()?;
        if self.sync.syncing() {
            if !self.take_sync_result() {
                return Some(SYNC_POLL_MS);
            }
            self.sync.folder.as_ref()?;
        }
        self.note_sync_edit(now_ms);
        if self.sync.schedule.is_due(now_ms) && !self.sync.restore_pending {
            self.start_sync(now_ms);
            if self.sync.syncing() {
                return Some(SYNC_POLL_MS);
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
    fn start_sync(&mut self, now_ms: i64) {
        self.sync.schedule.sync_failed(now_ms);
    }

    #[cfg(target_arch = "wasm32")]
    fn take_sync_result(&mut self) -> bool {
        true
    }

    /// Start the sync that is due on its worker.
    #[cfg(not(target_arch = "wasm32"))]
    fn start_sync(&mut self, now_ms: i64) {
        if let Err(e) = self.start_worker(SyncTask::Sync) {
            self.after_sync(
                SyncOutcome {
                    state: Err(e),
                    devices: None,
                },
                now_ms,
            );
        }
    }

    /// Start `task` on a worker, through the Sync folder (tests: theirs), on a second
    /// connection to the User database.
    #[cfg(not(target_arch = "wasm32"))]
    fn start_worker(&mut self, task: SyncTask) -> Result<(), crate::sync::SyncError> {
        let Some(path) = self.sync.folder.clone() else {
            return Err(crate::sync::SyncError::Damaged(
                "this device does not sync".into(),
            ));
        };
        let folder: crate::sync_worker::FolderMaker =
            Box::new(move || crate::sync_worker::reachable(&path));
        #[cfg(test)]
        let folder: crate::sync_worker::FolderMaker = match self.sync.test_folder.clone() {
            Some(TestFolder(test)) => Box::new(move || Ok(test)),
            None => folder,
        };
        let worker = SyncWorker::start(self.sync_store()?, folder, self.sync.platform, task)?;
        self.sync.phase = Phase::Running(worker);
        Ok(())
    }

    /// Take the running sync's result when it is there; whether no sync runs now.
    #[cfg(not(target_arch = "wasm32"))]
    fn take_sync_result(&mut self) -> bool {
        let Phase::Running(worker) = &self.sync.phase else {
            return true;
        };
        let Some(outcome) = worker.take() else {
            return false;
        };
        self.finish_sync(outcome);
        true
    }

    /// The running sync ended with `outcome`: take it, then apply what waited for it.
    #[cfg(not(target_arch = "wasm32"))]
    fn finish_sync(&mut self, outcome: SyncOutcome) {
        let Phase::Running(worker) = std::mem::take(&mut self.sync.phase) else {
            return;
        };
        self.after_sync(outcome, worker.started);
        if let SyncTask::RestoreChoice(_) = worker.task {
            // A choice that could not reach the folder is asked again.
            self.sync.restore_pending = self.read_restore_pending();
        }
        match self.sync.deferred.take() {
            Some(Deferred::ChooseFolder(folder)) => {
                if let Err(e) = self.choose_sync_folder(&folder) {
                    self.sync.last = Some(LastSync::Failed(e));
                }
            }
            Some(Deferred::StopSyncing) => {
                if let Err(e) = self.stop_syncing() {
                    self.sync.last = Some(LastSync::Failed(e));
                }
            }
            None => {}
        }
    }

    /// Wait, until `deadline`, for the sync running to end, and take its result; whether no
    /// sync runs now.
    #[cfg(not(target_arch = "wasm32"))]
    fn wait_for_sync(&mut self, deadline: std::time::Instant) -> bool {
        let Phase::Running(worker) = &self.sync.phase else {
            return true;
        };
        let Some(outcome) = worker.wait(deadline) else {
            return false;
        };
        self.finish_sync(outcome);
        true
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
    fn after_sync(&mut self, outcome: SyncOutcome, started: i64) {
        use crate::sync::SyncState;
        let finished = obs_store::clock::now_ms();
        self.sync.last_duration_ms = Some(finished.saturating_sub(started));
        match outcome.state {
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
                if let Some(devices) = outcome.devices {
                    self.set_folder_devices(devices);
                }
            }
            Err(e) => {
                log::warn!("sync failed: {e}");
                self.sync.schedule.sync_failed(finished);
                self.sync.last = Some(LastSync::Failed(e.to_string()));
            }
        }
    }

    /// The Synced devices of the Sync folder (their `device.json`), as the worker read them.
    #[cfg(not(target_arch = "wasm32"))]
    fn set_folder_devices(&mut self, devices: Vec<crate::sync::FolderDevice>) {
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
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
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
        // Word forms (a synced setting) is read again when next asked for.
        self.bible_search.forget_recent();
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
    /// at once, joins it). While a sync runs, it is chosen once that sync ends.
    pub fn choose_sync_folder(&mut self, folder: &std::path::Path) -> Result<(), String> {
        if !folder.is_dir() {
            return Err(format!("{} is not a folder", folder.display()));
        }
        if self.sync.syncing() {
            self.sync.deferred = Some(Deferred::ChooseFolder(folder.to_path_buf()));
            return Ok(());
        }
        let user = self.user_data()?;
        user.set_preference(SYNC_FOLDER_PREF, &folder.to_string_lossy())?;
        self.load_sync();
        Ok(())
    }

    /// Stop syncing: this device records no more ops (a new device id, its outbox emptied, a
    /// restore's question dropped), then forgets the folder; its files stay in the folder. The
    /// new id comes first, so a failure leaves the device syncing as before rather than
    /// recording into an outbox nothing reads. While a sync runs, it stops once that sync
    /// ends.
    pub fn stop_syncing(&mut self) -> Result<(), String> {
        if self.sync.syncing() {
            self.user_data()?;
            self.sync.deferred = Some(Deferred::StopSyncing);
            return Ok(());
        }
        let user = self.user_data()?;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(store) = user.user_store() {
            store.take_new_device_id().map_err(|e| e.to_string())?;
        }
        user.set_preference(SYNC_FOLDER_PREF, "")?;
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
            return store
                .set_device_name(name.trim())
                .map_err(|e| e.to_string());
        }
        let _ = name;
        Err("There is no User database".into())
    }

    /// Catch up or Replace, after a Backup was restored on this syncing device (rule 7),
    /// then a sync, on its worker ("Syncing…"; the prompt is gone meanwhile, and comes back
    /// when the choice could not reach the folder).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn choose_restore(&mut self, choice: crate::sync::RestoreChoice) -> Result<(), String> {
        if self.sync.folder.is_none() {
            return Err("This device does not sync".into());
        }
        if self.sync.syncing() {
            return Err("A sync is running".into());
        }
        self.start_worker(SyncTask::RestoreChoice(choice))
            .map_err(|e| e.to_string())?;
        self.sync.restore_pending = false;
        Ok(())
    }

    /// Before a restore (or its undo) replaces the User database: sync first when the folder
    /// is reachable, so edits not yet written reach it (a failure does not hold the restore
    /// back: the Safety backup has them). A sync running is waited for, then this one runs on
    /// its worker (the folder is never read on the UI thread); the restore waits for both at
    /// most [`RESTORE_SYNC_WAIT`], then goes ahead, the sync left running abandoned (it
    /// writes nothing more). Returns this device's Sync folder preference, which the restore
    /// puts back, and whether the sync was left unfinished
    /// ([`Session::after_restore_sync_unfinished`]).
    pub(crate) fn sync_before_restore(&mut self) -> (String, bool) {
        let kept = (self.user.as_ref())
            .and_then(|u| u.preference(SYNC_FOLDER_PREF).ok().flatten())
            .unwrap_or_default();
        #[cfg(not(target_arch = "wasm32"))]
        if self.sync.folder.is_some() {
            let deadline = std::time::Instant::now() + self.sync.restore_wait;
            if self.wait_for_sync(deadline) {
                self.start_sync(obs_store::clock::now_ms());
                if self.wait_for_sync(deadline)
                    && let Some(LastSync::Failed(e)) = &self.sync.last
                {
                    log::warn!("sync before the restore failed: {e}");
                }
            }
            if let Phase::Running(worker) = std::mem::take(&mut self.sync.phase) {
                worker.abandon();
                log::warn!("the sync before the restore did not finish in time");
                return (kept, true);
            }
        }
        (kept, false)
    }

    /// After a restore whose sync first did not finish in time: say so (Settings → Sync's
    /// status and Details), and try again as after a failed sync.
    pub(crate) fn after_restore_sync_unfinished(&mut self) {
        self.sync.schedule.sync_failed(obs_store::clock::now_ms());
        self.sync.last = Some(LastSync::Failed(RESTORE_SYNC_UNFINISHED.into()));
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
                let platform = if this {
                    self.sync.platform
                } else {
                    d.platform.as_str()
                };
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
            guidance: texts::folder_guidance(self.sync.platform),
            folder: (self.sync.folder.as_ref()).map(|f| f.display().to_string()),
            device_name: self.sync_device_name(),
            status,
            details,
            warn,
            syncing: self.sync.syncing(),
            folder_newer: (self.sync.last == Some(LastSync::FolderNewer))
                .then_some(texts::FOLDER_NEWER),
            restore_pending: self.sync.restore_pending,
            devices,
        }
    }

    /// The status line, the error behind it, and whether it is a problem.
    fn sync_status(&self, now_ms: i64) -> (String, Option<String>, bool) {
        if self.sync.syncing() {
            return (texts::SYNCING.to_string(), None, false);
        }
        if self.sync.restore_pending {
            // A restore whose sync first did not finish says so too.
            let failed = match &self.sync.last {
                Some(LastSync::Failed(e)) => Some(e.clone()),
                _ => None,
            };
            return (texts::RESTORE_TITLE.to_string(), failed, true);
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
        let text = if !self.sync.newer_app.is_empty()
            && !matches!(self.sync.last, Some(LastSync::Failed(_)))
        {
            texts::NEEDS_NEWER_APP.to_string()
        } else {
            text
        };
        Some(SyncIndicator {
            text,
            warn,
            syncing: self.sync.syncing(),
        })
    }

    /// Whether the restore prompt shows: a Backup was restored while this device synced,
    /// and it waits for Catch up or Replace.
    pub fn sync_restore_pending(&self) -> bool {
        self.sync.folder.is_some() && self.sync.restore_pending && !self.sync.syncing()
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
    pub fn resolve_sync_conflict(
        &mut self,
        id: &str,
        choice: ConflictChoice,
    ) -> Result<(), String> {
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
        // Without the Note in place, keeping this version or combining keeps this text.
        let actions = conflict_actions(in_place.is_some());
        let label = (actions.iter().find(|(c, _)| *c == choice))
            .or(actions.first())
            .map_or(texts::KEEP_BOTH, |(_, label)| *label);
        self.commit(label, changes)?;
        if let (ConflictChoice::Combine, Some(note)) = (choice, &in_place) {
            self.notes.reveal = Some((note.id.clone(), true));
        }
        Ok(())
    }
}

// ------------------------------------------------------------------------------ commands

/// Whether Help → Keyboard shortcuts lists command `id`: Sync now only once this device
/// syncs, so a desktop without a Sync folder shows nothing of Sync (plan S29).
pub(crate) fn shown_on_keys(s: &Session, id: &str) -> bool {
    id != "sync.now" || s.sync.folder.is_some()
}

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

fn required<'a>(
    command: &'static str,
    p: &Params<'a>,
    name: &str,
) -> Result<&'a str, CommandError> {
    p.str(name)
        .ok_or_else(|| failed(command, format!("no {name} given")))
}

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "sync.choose_folder",
        // The command is given the folder; Settings' button, which asks for it, is
        // `texts::CHOOSE_FOLDER` ("Choose Sync folder…").
        label: "Choose Sync folder",
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
        // Settings → Sync asks first; no key or Command box entry stops it unasked.
        params: &[ParamSpec {
            name: "confirm",
            kind: ParamKind::Boolean,
            required: true,
            help: "true: the user confirmed",
        }],
        enabled: has_folder,
        run: |s, p| {
            if p.bool("confirm") != Some(true) {
                return Err(failed("sync.stop", "Stop syncing asks first"));
            }
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
