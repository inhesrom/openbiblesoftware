//! The Session: everything the engine knows for one running app, and the single entry point
//! for commands.

use serde_json::Value;

use crate::builtin;
use crate::command::{CommandError, CommandRegistry, CommandSpec};
use crate::jobs::JobQueue;
use crate::library::Library;
use crate::workspace::Workspace;
use obs_layout::TextSettings;

use crate::persist::{UserData, WorkspaceSnapshot};

/// How the Session runs background jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobMode {
    /// A worker pool of this many threads (desktop).
    Threads(usize),
    /// No threads; the host calls [`JobQueue::pump`] (wasm, tests).
    Inline,
}

impl JobMode {
    /// Threads for the machine: available parallelism minus one for the UI, at least one.
    pub fn default_for_host() -> Self {
        let n = std::thread::available_parallelism().map_or(2, |n| n.get());
        JobMode::Threads(n.saturating_sub(1).clamp(1, 8))
    }
}

/// Preference key of the View menu's settings.
const VIEW_PREF: &str = "view";

/// One running engine: library, services, the command registry and the job queue.
#[derive(Debug)]
pub struct Session {
    commands: CommandRegistry,
    jobs: JobQueue,
    pub library: Library,
    pub imports: crate::import::Imports,
    /// Per-Resource Search index status and background rebuilds (#58).
    pub search: crate::search::SearchIndexes,
    /// Bible searches in flight, recent queries and Verse outlines (#59).
    pub bible_search: crate::bible_search::BibleSearches,
    /// Get Bibles catalog (#91).
    pub get_bibles: crate::get_bibles::GetBibles,
    pub workspace: Workspace,
    /// Text settings for Resource text (the View menu).
    pub text: TextSettings,
    /// Theme and Page tone (the View menu).
    pub appearance: crate::appearance::Appearance,
    /// What Word Info shows and whether it is pinned (#50).
    pub word_info: crate::word_info::WordInfo,
    /// Guide arrangements and Guide sections computed on the job queue (#53).
    pub guides: crate::guide::Guides,
    /// Word Study headers and English-word choosers computed on the job queue (#55).
    pub word_study: crate::word_study::WordStudies,
    /// The Notes Tool's Notebook choice, card order and card to reveal (#64).
    pub notes: crate::notes::NotesTool,
    /// Command ids run so far, oldest first (bounded); for diagnostics and tests.
    log: Vec<&'static str>,
    /// The User database, when there is one.
    pub user: Option<Box<dyn UserData>>,
    /// Undo and redo of annotation edits (#61).
    pub(crate) history: crate::annotations::UndoHistory,
    /// Bumped by every change to the annotation tables.
    pub(crate) annotations_revision: u64,
    /// The Last session as last written, to skip writes that change nothing.
    saved_session: Option<String>,
    /// How SWORD repositories are reached (#91). The app installs its network transport;
    /// until then only local `file://` repositories answer.
    fetch: std::sync::Arc<dyn obs_formats::sword_repo::Fetch>,
}

impl Session {
    /// A Session with every built-in command registered.
    pub fn new(mode: JobMode) -> Self {
        let jobs = match mode {
            JobMode::Threads(n) => JobQueue::threaded(n),
            JobMode::Inline => JobQueue::inline(),
        };
        Self {
            commands: builtin::registry(),
            jobs,
            library: Library::new(),
            imports: crate::import::Imports::default(),
            search: crate::search::SearchIndexes::default(),
            bible_search: crate::bible_search::BibleSearches::default(),
            get_bibles: crate::get_bibles::GetBibles::default(),
            workspace: Workspace::new(),
            text: TextSettings::default(),
            appearance: crate::appearance::Appearance::default(),
            word_info: crate::word_info::WordInfo::default(),
            guides: crate::guide::Guides::default(),
            word_study: crate::word_study::WordStudies::default(),
            notes: crate::notes::NotesTool::default(),
            log: Vec::new(),
            user: None,
            history: crate::annotations::UndoHistory::default(),
            annotations_revision: 0,
            saved_session: None,
            fetch: std::sync::Arc::new(obs_formats::sword_repo::LocalFetch),
        }
    }

    /// Install the app's transport for SWORD repositories (Get Bibles, #91). The engine
    /// never opens network connections itself.
    pub fn set_fetch(&mut self, fetch: std::sync::Arc<dyn obs_formats::sword_repo::Fetch>) {
        self.fetch = fetch;
    }

    /// The transport for SWORD repositories, for handing to a job.
    pub fn fetch(&self) -> std::sync::Arc<dyn obs_formats::sword_repo::Fetch> {
        std::sync::Arc::clone(&self.fetch)
    }

    pub fn commands(&self) -> &CommandRegistry {
        &self.commands
    }

    pub fn jobs(&self) -> &JobQueue {
        &self.jobs
    }

    /// Whether a command exists and is enabled now (menus grey out the rest).
    pub fn is_enabled(&self, id: &str) -> bool {
        self.commands.get(id).is_some_and(|c| (c.enabled)(self))
    }

    /// Attach the User database and apply the preferences and Preferred order saved in it.
    pub fn attach_user(&mut self, user: Box<dyn UserData>) {
        if let Ok(Some(v)) = user.preference(VIEW_PREF)
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(&v)
        {
            crate::appearance::apply_json(&v, &mut self.appearance, &mut self.text);
        }
        if let Ok(order) = user.preferred_order() {
            self.library.set_preferred_order(order.clone());
            // Pin the initial order too: a later Import must not sort ahead of Resources
            // the user already had, even if they have never moved a row.
            let current = self.library.preferred_order();
            if current != order
                && let Err(e) = user.save_preferred_order(&current)
            {
                self.workspace
                    .push_effect(crate::workspace::Effect::Message(format!(
                        "Could not save the Preferred order: {e}"
                    )));
            }
        }
        self.user = Some(user);
        self.bible_search.forget_recent();
        self.history.clear();
        self.annotations_revision += 1;
        self.restore_import_reports();
        self.load_guide_arrangements();
    }

    /// Save the library's Preferred order.
    pub(crate) fn save_preferred_order(&self) -> Result<(), String> {
        let Some(user) = &self.user else {
            return Ok(());
        };
        user.save_preferred_order(&self.library.preferred_order())
    }

    /// How many Annotations have a Text range in a Resource (shown in its About Tab and its
    /// Remove confirmation). Passage anchors without a Text range show in every Bible and
    /// belong to none.
    pub fn annotations_referring(&self, resource_id: &str) -> u64 {
        self.user
            .as_ref()
            .and_then(|u| u.annotations_referring(resource_id).ok())
            .unwrap_or(0)
    }

    /// Save the View preferences.
    pub(crate) fn save_view_preferences(&self) -> Result<(), String> {
        let Some(user) = &self.user else {
            return Ok(());
        };
        let v = crate::appearance::to_json(&self.appearance, &self.text);
        user.set_preference(VIEW_PREF, &v.to_string())
    }

    /// Write the Last session if it changed since the last write.
    pub fn save_last_session(&mut self) -> Result<(), String> {
        let Some(user) = &self.user else {
            return Ok(());
        };
        let json = self.workspace.snapshot().to_json();
        if self.saved_session.as_deref() == Some(json.as_str()) {
            return Ok(());
        }
        user.save_last_session(&json)?;
        self.saved_session = Some(json);
        Ok(())
    }

    /// Restore the Last session, dropping Tabs whose Resource is gone. Returns whether there
    /// was one to restore.
    pub fn restore_last_session(&mut self) -> bool {
        let Some(json) = self
            .user
            .as_ref()
            .and_then(|u| u.last_session().ok().flatten())
        else {
            return false;
        };
        let Some(snap) = WorkspaceSnapshot::from_json(&json) else {
            return false;
        };
        let snap = snap.without_missing(|id| self.library.get(id).is_some());
        if snap.tabs.is_empty() {
            return false;
        }
        self.workspace.restore(snap);
        self.saved_session = Some(json);
        true
    }

    /// The id of the command running now (or run last).
    pub fn last_command(&self) -> Option<&'static str> {
        self.log.last().copied()
    }

    /// The most recent command ids run, oldest first.
    pub fn command_log(&self) -> &[&'static str] {
        &self.log
    }

    /// Run a command by id: look it up, check it is enabled, validate `params`, run it.
    /// The only way user actions reach the engine.
    pub fn run(&mut self, id: &str, params: &Value) -> Result<Value, CommandError> {
        let spec: CommandSpec = *self
            .commands
            .get(id)
            .ok_or_else(|| CommandError::UnknownCommand(id.to_string()))?;
        if !(spec.enabled)(self) {
            return Err(CommandError::Disabled(spec.id));
        }
        let params = spec.validate(params)?;
        if self.log.len() >= 256 {
            self.log.remove(0);
        }
        self.log.push(spec.id);
        (spec.run)(self, params)
    }
}
