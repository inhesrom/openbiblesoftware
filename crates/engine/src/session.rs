//! The Session: everything the engine knows for one running app, and the single entry point
//! for commands.

use serde_json::Value;

use crate::builtin;
use crate::command::{CommandError, CommandRegistry, CommandSpec};
use crate::jobs::JobQueue;

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

/// One running engine: library, services, the command registry and the job queue.
#[derive(Debug)]
pub struct Session {
    commands: CommandRegistry,
    jobs: JobQueue,
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
        }
    }

    pub fn commands(&self) -> &CommandRegistry {
        &self.commands
    }

    pub fn jobs(&self) -> &JobQueue {
        &self.jobs
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
        (spec.run)(self, params)
    }
}
