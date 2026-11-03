//! The Session: everything the engine knows for one running app, and the single entry point
//! for commands.

use serde_json::Value;

use crate::builtin;
use crate::command::{CommandError, CommandRegistry, CommandSpec};
use crate::jobs::JobQueue;
use crate::library::Library;
use crate::workspace::Workspace;
use obs_layout::TextSettings;
use serde_json::json;

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
    pub workspace: Workspace,
    /// Text settings for Resource text (the View menu).
    pub text: TextSettings,
    /// Dark page for Resource text.
    pub dark_page: bool,
    /// Command ids run so far, oldest first (bounded); for diagnostics and tests.
    log: Vec<&'static str>,
    /// The User database, when there is one.
    pub user: Option<Box<dyn UserData>>,
    /// The Last session as last written, to skip writes that change nothing.
    saved_session: Option<String>,
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
            workspace: Workspace::new(),
            text: TextSettings::default(),
            dark_page: false,
            log: Vec::new(),
            user: None,
            saved_session: None,
        }
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

    /// Attach the User database and apply the preferences saved in it.
    pub fn attach_user(&mut self, user: Box<dyn UserData>) {
        if let Ok(Some(v)) = user.preference(VIEW_PREF)
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(&v)
        {
            if let Some(size) = v["text_size"].as_f64() {
                self.text.size = (size as f32).clamp(10.0, 40.0);
            }
            if let Some(b) = v["verse_numbers"].as_bool() {
                self.text.verse_numbers = b;
            }
            if let Some(b) = v["dark_page"].as_bool() {
                self.dark_page = b;
            }
        }
        self.user = Some(user);
    }

    /// Save the View preferences.
    pub(crate) fn save_view_preferences(&self) -> Result<(), String> {
        let Some(user) = &self.user else {
            return Ok(());
        };
        let v = json!({
            "text_size": self.text.size,
            "verse_numbers": self.text.verse_numbers,
            "dark_page": self.dark_page,
        });
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
