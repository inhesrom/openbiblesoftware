//! When a sync runs (ADR 0022 rule 8): at launch, on resume, about 30 seconds after an edit,
//! every 5 minutes while the app is open on the desktop, and on demand. Sync is periodic, not
//! live. The shells keep a [`SyncSchedule`], tell it what happened, and call `sync_now` when
//! [`SyncSchedule::next_sync_due`] comes (plan A8); the rule is here, not in Swift.

/// How long after the first edit not yet synced a sync is due.
pub const EDIT_DELAY_MS: i64 = 30_000;
/// How often a sync is due while the desktop app is open.
pub const DESKTOP_PERIOD_MS: i64 = 5 * 60_000;

/// What a shell tells the schedule, and when the next sync is due (Unix milliseconds).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncSchedule {
    /// Due every [`DESKTOP_PERIOD_MS`] (the desktop app while open); the Mobile app is not.
    periodic: bool,
    /// Due now: launched, resumed or asked for, and not synced since.
    now: bool,
    last_sync_ms: Option<i64>,
    /// The first edit since the last sync, if any.
    first_edit_ms: Option<i64>,
}

impl SyncSchedule {
    /// A schedule at launch: a sync is due now. `periodic` on the desktop.
    pub fn at_launch(periodic: bool) -> Self {
        Self {
            periodic,
            now: true,
            last_sync_ms: None,
            first_edit_ms: None,
        }
    }

    /// The app came back to the foreground: a sync is due now.
    pub fn resumed(&mut self) {
        self.now = true;
    }

    /// The user asked to sync (Sync now): a sync is due now.
    pub fn requested(&mut self) {
        self.now = true;
    }

    /// Synced data changed at `now_ms`: a sync is due [`EDIT_DELAY_MS`] after the first edit
    /// not yet synced (later edits do not push it back, so typing never holds it off).
    pub fn edited(&mut self, now_ms: i64) {
        self.first_edit_ms.get_or_insert(now_ms);
    }

    /// A sync ran to its end at `now_ms`: what it was due for is done.
    pub fn synced(&mut self, now_ms: i64) {
        self.now = false;
        self.last_sync_ms = Some(now_ms);
        self.first_edit_ms = None;
    }

    /// A sync failed at `now_ms` (the folder unreachable): it is due again as if the edits
    /// not synced were made now, or at the next period, resume or request.
    pub fn sync_failed(&mut self, now_ms: i64) {
        self.now = false;
        self.last_sync_ms = Some(now_ms);
        if self.first_edit_ms.is_some() {
            self.first_edit_ms = Some(now_ms);
        }
    }

    /// When the next sync is due (at or before `now_ms`: now), or `None` when nothing will be
    /// due until something happens (the Mobile app with no edits since its last sync).
    pub fn next_sync_due(&self, now_ms: i64) -> Option<i64> {
        let mut due = self
            .first_edit_ms
            .map(|at| at.saturating_add(EDIT_DELAY_MS));
        if self.now {
            due = Some(now_ms);
        }
        if self.periodic {
            let periodic = self
                .last_sync_ms
                .map_or(now_ms, |at| at.saturating_add(DESKTOP_PERIOD_MS));
            due = Some(due.map_or(periodic, |d| d.min(periodic)));
        }
        due
    }

    /// Whether a sync is due at `now_ms`.
    pub fn is_due(&self, now_ms: i64) -> bool {
        self.next_sync_due(now_ms).is_some_and(|due| due <= now_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sync_is_due_at_launch_on_resume_and_on_demand() {
        let mut s = SyncSchedule::at_launch(false);
        assert!(s.is_due(1_000));
        s.synced(1_000);
        assert_eq!(
            s.next_sync_due(2_000),
            None,
            "the Mobile app has nothing due"
        );
        s.resumed();
        assert!(s.is_due(3_000));
        s.synced(3_000);
        s.requested();
        assert_eq!(s.next_sync_due(4_000), Some(4_000));
    }

    #[test]
    fn a_sync_is_due_30_seconds_after_the_first_edit_not_synced() {
        let mut s = SyncSchedule::at_launch(false);
        s.synced(0);
        s.edited(10_000);
        s.edited(25_000);
        assert_eq!(s.next_sync_due(26_000), Some(10_000 + EDIT_DELAY_MS));
        assert!(!s.is_due(39_999));
        assert!(s.is_due(40_000));
        s.synced(40_000);
        assert_eq!(s.next_sync_due(41_000), None);
    }

    #[test]
    fn the_desktop_syncs_every_5_minutes_and_sooner_after_an_edit() {
        let mut s = SyncSchedule::at_launch(true);
        s.synced(0);
        assert_eq!(s.next_sync_due(1), Some(DESKTOP_PERIOD_MS));
        s.edited(60_000);
        assert_eq!(s.next_sync_due(60_001), Some(60_000 + EDIT_DELAY_MS));
        s.synced(90_000);
        assert_eq!(s.next_sync_due(90_001), Some(90_000 + DESKTOP_PERIOD_MS));
    }

    #[test]
    fn a_failed_sync_is_due_again_after_the_edit_delay_or_the_period() {
        let mut s = SyncSchedule::at_launch(false);
        s.edited(0);
        s.sync_failed(30_000);
        assert_eq!(s.next_sync_due(30_001), Some(30_000 + EDIT_DELAY_MS));
        let mut d = SyncSchedule::at_launch(true);
        d.sync_failed(5);
        assert_eq!(d.next_sync_due(6), Some(5 + DESKTOP_PERIOD_MS));
    }
}
