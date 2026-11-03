//! Session, library, command registry, undo, Link-set routing, Guide sections, annotation
//! service, search orchestration and the job queue.
//!
//! Layer L4. No UI dependencies; compiles for `wasm32-unknown-unknown`.
//!
//! ```
//! use obs_engine::{JobMode, Session};
//! use serde_json::json;
//!
//! let mut session = Session::new(JobMode::Inline);
//! let out = session.run("reference.parse", &json!({ "text": "jn 3:16-18" })).unwrap();
//! assert_eq!(out["text"], "John 3:16-18");
//! ```

mod builtin;
pub mod command;
pub mod jobs;
mod library;
mod session;
pub mod workspace;
mod workspace_commands;

pub use command::{CommandError, CommandRegistry, CommandSpec, ParamKind, ParamSpec, Params};
pub use jobs::{Job, JobContext, JobEvent, JobEventKind, JobId, JobOutput, JobQueue, Step};
pub use library::{Library, LibraryEntry, default_library_dir};
pub use session::{JobMode, Session};
pub use workspace::{
    Effect, Guide, HeaderBox, History, OpenRequest, PanelId, Placement, Tab, TabId, TabKind, Tool,
    Workspace,
};
pub use workspace_commands::{MENUS, Menu, MenuItem};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod workspace_tests;
