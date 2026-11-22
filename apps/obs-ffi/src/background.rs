//! The background processing task's side of the core (`obs_mobile::background`): whether to
//! ask for one, and stepping its work under a deadline with the app unlocked between steps.
//! Search index states for Library → Storage and Search. Every rule stays in `obs-mobile`.

use std::time::Instant;

use crate::{MobileApp, MobileError, PUMP_BUSY};

/// What the background processing task asks the system for (`BGProcessingTaskRequest`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct BackgroundRequest {
    pub requires_external_power: bool,
    pub requires_network: bool,
}

impl From<obs_mobile::BackgroundRequest> for BackgroundRequest {
    fn from(r: obs_mobile::BackgroundRequest) -> Self {
        Self {
            requires_external_power: r.requires_external_power,
            requires_network: r.requires_network,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SearchIndexState {
    Ready,
    Building,
    Failed,
    Unavailable,
}

impl From<obs_mobile::SearchIndexState> for SearchIndexState {
    fn from(s: obs_mobile::SearchIndexState) -> Self {
        match s {
            obs_mobile::SearchIndexState::Ready => Self::Ready,
            obs_mobile::SearchIndexState::Building => Self::Building,
            obs_mobile::SearchIndexState::Failed => Self::Failed,
            obs_mobile::SearchIndexState::Unavailable => Self::Unavailable,
        }
    }
}

/// A Resource's Search index: `label` for its Storage row, `note` for Search.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SearchIndexStatus {
    pub resource_id: String,
    pub state: SearchIndexState,
    pub label: String,
    pub searchable: bool,
    pub note: Option<String>,
}

impl From<obs_mobile::SearchIndexStatus> for SearchIndexStatus {
    fn from(s: obs_mobile::SearchIndexStatus) -> Self {
        Self {
            resource_id: s.resource_id,
            state: s.state.into(),
            label: s.label,
            searchable: s.searchable,
            note: s.note,
        }
    }
}

#[uniffi::export]
impl MobileApp {
    /// The background processing task to ask for, while there is work it would carry on.
    pub fn pending_background_work(&self) -> Result<Option<BackgroundRequest>, MobileError> {
        self.read(|a| a.pending_background_work().map(Into::into))
    }

    /// One step of the background task: the work moved on for `obs_mobile::BACKGROUND_STEP`
    /// at most, or until none is left, with the app unlocked between rounds (as the pump
    /// thread waits). Returns whether work is left. Blocks: call it off the main thread.
    pub fn step_background(&self) -> Result<bool, MobileError> {
        let deadline = Instant::now() + obs_mobile::BACKGROUND_STEP;
        loop {
            let pending = self.with(|a| Ok(a.step_background(deadline)))?;
            let now = Instant::now();
            if !pending || now >= deadline {
                return Ok(pending);
            }
            std::thread::sleep((deadline - now).min(PUMP_BUSY));
        }
    }

    /// Where Resource `resource_id`'s Search index stands (`None`: none is tracked for it).
    pub fn search_index_status(
        &self,
        resource_id: String,
    ) -> Result<Option<SearchIndexStatus>, MobileError> {
        self.read(|a| a.search_index_status(&resource_id).map(Into::into))
    }
}
