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

pub mod aliases;
pub mod annotations;
pub mod appearance;
pub mod bible_search;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod bible_search_tests;
mod builtin;
pub mod command;
pub mod compare;
pub mod credits;
pub mod cross_references;
pub mod get_bibles;
#[cfg(test)]
mod get_bibles_tests;
pub mod guide;
#[cfg(test)]
mod guide_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod guide_words_tests;
pub mod import;
pub mod jobs;
pub mod key_words;
mod library;
mod library_commands;
pub mod library_search;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod library_search_tests;
pub mod link;
pub mod notes;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod notes_tests;
pub mod people_places;
pub mod persist;
pub mod resource_navigation;
pub mod search;
#[cfg(test)]
mod search_tests;
mod session;
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub mod testing;
pub mod word_info;
pub mod word_sections;
#[cfg(test)]
mod word_sections_tests;
pub mod word_study;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod word_study_tests;
pub mod workspace;

/// SWORD repository transport types (Get Bibles, #91).
pub use obs_formats::sword_repo;
mod workspace_commands;

pub use annotations::{
    Anchor, Annotation, Change, Content, HighlightStyle, NoteTarget, Notebook, NotebookDeletion,
    Row, Selection, TextPos, TextRange, UndoHistory, UndoStep,
};
pub use appearance::{Appearance, PageTone, Theme};
pub use bible_search::{
    BibleOutline, BibleResults, ErrorField, InlineError, RowHit, Scope, ScopePreset, SearchMode,
    SearchRequestFields, SearchTab, SearchedBible, Snippet, VerseRow, parse_scope, scope_presets,
    verse_rows,
};
pub use command::{CommandError, CommandRegistry, CommandSpec, ParamKind, ParamSpec, Params};
pub use compare::{
    Cell, ComparedBible, ComparedCell, Comparison, DEFAULT_BIBLES, Marks, VerseBlock, WordDiff,
    verse_blocks, word_diff,
};
pub use guide::{
    CommentaryRow, GuideArrangement, GuideSection, Guides, LibrarySections, SectionData,
    SectionPlace, SectionSource, SectionStatus,
};
pub use import::{
    DEFERRED_INDEX, ImportIndexStatus, ImportIndexer, ImportPhase, ImportRecord, ImportReport,
    ImportSource, Imports, UNAVAILABLE_INDEX,
};
pub use jobs::{
    Job, JobContext, JobEvent, JobEventKind, JobId, JobOutput, JobQueue, Step, sword_installer,
};
pub use key_words::{KeyWord, KeyWords};
pub use library::{
    Library, LibraryEntry, LibraryFilter, STARTER_PREFERRED_ORDER, default_library_dir,
    default_user_db, language_name,
};
pub use library_commands::annotations_phrase;
pub use library_search::{
    LIBRARY_TYPES, LibraryGroup, LibraryResults, LibraryRow, LibraryScope, SearchedResource,
};
pub use link::LINK_SETS;
pub use notes::{Card, CardOrder, NoteHit, NotesFilter, NotesResults, NotesTool, body_references};
pub use people_places::{ArticleLink, Fact, PersonOrPlace};
pub use persist::{LayoutNode, SNAPSHOT_VERSION, TabSnapshot, UserData, WorkspaceSnapshot};
#[cfg(not(target_arch = "wasm32"))]
pub use search::CacheIndexer;
pub use search::{
    INDEX_VERSION, IndexStatus, ResourceHits, SearchIndexes, SearchRequest, SearchResults,
    default_search_cache_dir,
};
pub use session::{JobMode, Session};
pub use word_info::{LemmaCard, LexiconGloss, PartRole, ShownWord, WordInfo, WordPart};
pub use word_sections::{
    FilteredOccurrences, Form, Forms, OCCURRENCE_ROWS, OccurrenceList, OccurrencePreview,
    OccurrenceVerse, PreviewBible, RenderingRow, TranslatedAs,
};
pub use word_study::{
    Choice, Chooser, LexiconEntry, Occurrences, WordStatus, WordStudies, WordStudyHeader,
    WordStudyInput,
};
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod annotations_tests;
