//! The Session's sync off the UI thread (plan S29b): a sync (or a restore's Catch up or
//! Replace) runs on a thread of its own, on a second connection to the User database
//! ([`obs_store::SecondConnection`]), which shares the first's clock: the two hand out one
//! sequence of clocks, every clock the merge takes in is passed by the next edit's, and a new
//! device id taken there is the next edit's at once. Every read of the Sync folder (an iCloud
//! placeholder's download can block for seconds) is on that thread; the UI thread only takes
//! the result ([`SyncWorker::take`]).
//!
//! One sync runs at a time. A user's edit made meanwhile waits at most for the merge
//! transaction in progress (`obs_store::BUSY_TIMEOUT`), is stamped after every clock merged
//! so far and goes in the outbox for the next sync. A sync left running when the User
//! database is about to be replaced (a restore that waited [`RESTORE_SYNC_WAIT`] for it) is
//! [`SyncWorker::abandon`]ed: it commits nothing more and touches the folder no more.

use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError, channel};
use std::time::{Duration, Instant};

use obs_store::{SecondConnection, UserStore};

use crate::sync::{
    FolderDevice, FolderError, FsSyncFolder, RestoreChoice, SyncError, SyncFolder, SyncOptions,
    SyncState,
};

/// How long a restore waits for the sync it runs first (and one already running) before it
/// goes ahead without it: the Safety backup holds the edits that sync would have written.
pub const RESTORE_SYNC_WAIT: Duration = Duration::from_secs(30);

/// Makes the Sync folder a worker syncs through, on its thread (the desktop's: [`reachable`]).
pub(crate) type FolderMaker = Box<dyn FnOnce() -> Result<Arc<dyn SyncFolder>, SyncError> + Send>;

/// What a worker runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SyncTask {
    /// `sync_now`.
    Sync,
    /// The user's answer after a restore (`restore_choice`), then a sync.
    RestoreChoice(RestoreChoice),
}

/// What a worker found: the sync's state, and the Synced devices of the folder read after it.
#[derive(Debug)]
pub(crate) struct SyncOutcome {
    pub(crate) state: Result<SyncState, SyncError>,
    /// The folder's devices (their `device.json`), read after a sync that did not fail.
    pub(crate) devices: Option<Vec<FolderDevice>>,
}

/// A sync running on its thread.
#[derive(Debug)]
pub(crate) struct SyncWorker {
    pub(crate) task: SyncTask,
    /// When it started (Unix milliseconds).
    pub(crate) started: i64,
    connection: SecondConnection,
    outcome: Receiver<SyncOutcome>,
}

impl SyncWorker {
    /// Start `task` on a thread of its own, on a second connection to `user`, through the
    /// Sync folder `folder` makes there.
    pub(crate) fn start(
        user: &UserStore,
        folder: FolderMaker,
        platform: &'static str,
        task: SyncTask,
    ) -> Result<Self, SyncError> {
        let connection = user.second_connection()?;
        let (send, outcome) = channel();
        let worker = connection.clone();
        std::thread::Builder::new()
            .name("sync".into())
            .spawn(move || {
                // The Session may have stopped waiting; then no one reads it.
                let _ = send.send(run(&worker, folder, platform, task));
            })
            .map_err(|e| SyncError::Damaged(format!("the sync could not start: {e}")))?;
        Ok(Self {
            task,
            started: obs_store::clock::now_ms(),
            connection,
            outcome,
        })
    }

    /// Its outcome, once it is done (`None`: it is still running). A worker that stopped
    /// without one (it panicked) failed.
    pub(crate) fn take(&self) -> Option<SyncOutcome> {
        match self.outcome.try_recv() {
            Ok(outcome) => Some(outcome),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(SyncOutcome::stopped()),
        }
    }

    /// Its outcome, waiting for it until `deadline` (`None`: still running then).
    pub(crate) fn wait(&self, deadline: Instant) -> Option<SyncOutcome> {
        let left = deadline.saturating_duration_since(Instant::now());
        match self.outcome.recv_timeout(left) {
            Ok(outcome) => Some(outcome),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => Some(SyncOutcome::stopped()),
        }
    }

    /// Leave it running, harmless: it commits nothing more to the User database and reads or
    /// writes nothing more in the Sync folder (the database is about to be replaced).
    pub(crate) fn abandon(self) {
        self.connection.refuse_writes();
    }
}

impl SyncOutcome {
    fn stopped() -> Self {
        Self {
            state: Err(SyncError::Damaged(
                "the sync stopped before it finished".into(),
            )),
            devices: None,
        }
    }
}

/// The worker thread: reach the folder, open the second connection, run `task`, then read the
/// folder's devices.
fn run(
    connection: &SecondConnection,
    folder: FolderMaker,
    platform: &'static str,
    task: SyncTask,
) -> SyncOutcome {
    let fs = match folder() {
        Ok(fs) => fs,
        Err(e) => {
            return SyncOutcome {
                state: Err(e),
                devices: None,
            };
        }
    };
    let fs = UnlessAbandoned {
        folder: &*fs,
        connection,
    };
    let state = connection.open().map_err(SyncError::from).and_then(|user| {
        let options = SyncOptions::new(platform);
        match task {
            SyncTask::Sync => crate::sync::sync_now(&user, &fs, &options),
            SyncTask::RestoreChoice(choice) => {
                crate::sync::restore_choice(&user, &fs, choice, &options)
            }
        }
    });
    let devices = match &state {
        Ok(_) => crate::sync::read_devices(&fs).ok(),
        Err(_) => None,
    };
    SyncOutcome { state, devices }
}

/// The Sync folder at `folder`, when it is there; nothing is made at a path that is not (a
/// disk not mounted, a cloud folder moved).
pub(crate) fn reachable(folder: &Path) -> Result<Arc<dyn SyncFolder>, SyncError> {
    if folder.is_dir() {
        Ok(Arc::new(FsSyncFolder::new(folder)))
    } else {
        Err(SyncError::Damaged(format!(
            "{} is not there",
            folder.display()
        )))
    }
}

/// A Sync folder that refuses every call once the worker is abandoned.
struct UnlessAbandoned<'a> {
    folder: &'a dyn SyncFolder,
    connection: &'a SecondConnection,
}

impl UnlessAbandoned<'_> {
    fn check(&self) -> Result<(), FolderError> {
        if self.connection.writes_refused() {
            return Err(FolderError::Io(
                "the sync was stopped: the User database is being replaced".into(),
            ));
        }
        Ok(())
    }
}

impl SyncFolder for UnlessAbandoned<'_> {
    fn list(&self, dir: &str) -> Result<Vec<String>, FolderError> {
        self.check()?;
        self.folder.list(dir)
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, FolderError> {
        self.check()?;
        self.folder.read(path)
    }

    fn write_new(&self, path: &str, bytes: &[u8]) -> Result<(), FolderError> {
        self.check()?;
        self.folder.write_new(path, bytes)
    }

    fn remove(&self, path: &str) -> Result<(), FolderError> {
        self.check()?;
        self.folder.remove(path)
    }

    fn ensure_available(&self, path: &str) -> Result<bool, FolderError> {
        self.check()?;
        self.folder.ensure_available(path)
    }
}
