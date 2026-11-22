//! Background work (#333 "Storage", plan S25): the Search indexes and alias caches the device
//! builds after a download or an Import carry on through a background processing task when
//! the app is closed. Which work is pending, what the task asks the system for and how the
//! work is stepped are here; the shell registers the task, submits the request this returns
//! and calls [`MobileApp::step_background`] until it says the work is done or the system
//! takes the time back.
//!
//! The jobs themselves run on the engine's job threads whatever the app's state; the task
//! keeps the process running while they do, and each step moves the work on as the pump
//! does (a finished index build starts the next one), leaving the [`crate::Changed`]s it
//! makes for the next [`MobileApp::pump`]. Work cut short (the task expired, the app was
//! killed) is not lost: a build in flight carries on when the app runs again, and a launch
//! queues every index without a cache again ([`obs_engine::Session::refresh_search_indexes`]).

use crate::MobileApp;

/// What the background processing task asks the system for (iOS `BGProcessingTaskRequest`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackgroundRequest {
    /// Only while the device charges. No: the builds take seconds to a minute, and the
    /// indexes are wanted soon after a download.
    pub requires_external_power: bool,
    /// Only with a network. No: every build reads files already on the device.
    pub requires_network: bool,
}

/// The request for the work the device builds by itself.
pub const INDEX_REQUEST: BackgroundRequest = BackgroundRequest {
    requires_external_power: false,
    requires_network: false,
};

/// How long one step of the background task runs ([`MobileApp::step_background`]'s
/// deadline, set by the FFI's step): short, so the task stops soon after the system takes its
/// time back.
pub const BACKGROUND_STEP: std::time::Duration = std::time::Duration::from_millis(500);

/// What background work there is, by kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BackgroundWork {
    /// A Search index build is queued or running.
    pub search_indexes: bool,
    /// The People & places ↔ Dictionary alias caches are being made current.
    pub alias_caches: bool,
    /// Downloaded files being checked and put in place (their index follows).
    pub installs: usize,
    /// Imports not over yet (their index follows).
    pub imports: usize,
}

impl BackgroundWork {
    pub fn is_pending(&self) -> bool {
        self.search_indexes || self.alias_caches || self.installs > 0 || self.imports > 0
    }
}

impl MobileApp {
    /// The background work there is now. Transfers are not here: the system's background
    /// downloads carry those on by themselves.
    pub fn background_work(&self) -> BackgroundWork {
        use crate::DownloadState;
        BackgroundWork {
            search_indexes: self.session.search.is_building(),
            alias_caches: self.session.guides.is_refreshing_aliases(),
            installs: (self.downloads.iter())
                .filter(|d| {
                    matches!(
                        d.state,
                        DownloadState::Verifying | DownloadState::Installing
                    )
                })
                .count(),
            imports: self.picked.iter().filter(|p| p.is_running()).count(),
        }
    }

    /// The background processing task to ask for, while there is work it would carry on.
    pub fn pending_background_work(&self) -> Option<BackgroundRequest> {
        self.background_work().is_pending().then_some(INDEX_REQUEST)
    }

    /// Move the background work on until `deadline` at the latest, and say whether any is
    /// left. With jobs on threads (the app) this is one round of the pump's work, so the
    /// caller releases the app between steps; with jobs inline (tests) it runs job steps one
    /// at a time until the deadline has passed, so it ends at most one job step after it.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn step_background(&mut self, deadline: std::time::Instant) -> bool {
        if self.session.jobs().is_threaded() {
            self.advance();
            return self.background_work().is_pending();
        }
        loop {
            self.advance();
            if !self.background_work().is_pending() {
                return false;
            }
            let ran = self.session.jobs().pump(1);
            if ran == 0 || std::time::Instant::now() >= deadline {
                self.advance();
                return self.background_work().is_pending();
            }
        }
    }
}
