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

pub mod appearance;
mod builtin;
pub mod command;
pub mod compare;
pub mod cross_references;
pub mod import;
pub mod jobs;
mod library;
mod library_commands;
pub mod link;
pub mod persist;
pub mod resource_navigation;
mod session;
pub mod word_info;
pub mod workspace;
mod workspace_commands;

pub use appearance::{Appearance, PageTone, Theme};
pub use command::{CommandError, CommandRegistry, CommandSpec, ParamKind, ParamSpec, Params};
pub use compare::{
    Cell, ComparedBible, ComparedCell, Comparison, DEFAULT_BIBLES, Marks, VerseBlock, WordDiff,
    verse_blocks, word_diff,
};
pub use import::{
    DEFERRED_INDEX, ImportIndexStatus, ImportIndexer, ImportPhase, ImportRecord, ImportReport,
    Imports,
};
pub use jobs::{Job, JobContext, JobEvent, JobEventKind, JobId, JobOutput, JobQueue, Step};
pub use library::{
    Library, LibraryEntry, LibraryFilter, default_library_dir, default_user_db, language_name,
};
pub use library_commands::annotations_phrase;
pub use link::LINK_SETS;
pub use persist::{LayoutNode, SNAPSHOT_VERSION, TabSnapshot, UserData, WorkspaceSnapshot};
pub use session::{JobMode, Session};
pub use word_info::{LemmaCard, LexiconGloss, PartRole, ShownWord, WordInfo, WordPart};
pub use workspace::{
    Effect, Guide, HeaderBox, History, OpenRequest, PanelId, Placement, Tab, TabId, TabKind, Tool,
    Workspace,
};
pub use workspace_commands::{MENUS, Menu, MenuItem, choices};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod workspace_tests;

#[cfg(test)]
mod link_tests;

#[cfg(test)]
mod library_tests;

#[cfg(test)]
mod compare_tests;

#[cfg(test)]
mod resource_navigation_tests;

#[cfg(test)]
mod import_tests;
