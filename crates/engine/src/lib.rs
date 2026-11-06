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
pub mod command_box;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod command_box_tests;
pub mod compare;
pub mod copy_verses;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod copy_verses_tests;
pub mod credits;
pub mod cross_references;
pub mod factbook;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod factbook_tests;
pub mod favorites;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod favorites_tests;
pub mod get_bibles;
#[cfg(test)]
mod get_bibles_tests;
pub mod guide;
#[cfg(test)]
mod guide_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod guide_words_tests;
pub mod import;
pub mod interlinear;
#[cfg(test)]
mod interlinear_tests;
pub mod jobs;
pub mod key_words;
mod library;
mod library_commands;
pub mod library_search;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod library_search_tests;
pub mod link;
mod lookups;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod lookups_tests;
pub mod missing_resource;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod missing_resource_tests;
pub mod notes;
pub mod notes_export;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod notes_export_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod notes_tests;
pub mod people_places;
pub mod persist;
pub mod reading_plans;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod reading_plans_tests;
pub mod reanchor;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod reanchor_tests;
pub mod resource_navigation;
pub mod search;
#[cfg(test)]
mod search_tests;
pub mod search_views;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod search_views_tests;
mod session;
pub mod starter;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod starter_tests;
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub mod testing;
pub mod toolbar;
pub mod toolbar_settings;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod toolbar_settings_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod toolbar_tests;
pub mod word_info;
pub mod word_sections;
#[cfg(all(test, not(target_arch = "wasm32")))]
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
pub use appearance::{
    Appearance, FontChoice, FontChoices, FontSlot, InterfaceSize, PageTone, Theme,
};
pub use bible_search::{
    BibleOutline, BibleResults, ErrorField, InlineError, RowHit, Scope, ScopePreset, SearchMode,
    SearchRequestFields, SearchTab, SearchedBible, Snippet, VerseRow, parse_scope, scope_presets,
    verse_rows,
};
pub use command::{CommandError, CommandRegistry, CommandSpec, ParamKind, ParamSpec, Params};
pub use command_box::{CommandBoxAction, Suggestion};
pub use compare::{
    Cell, ComparedBible, ComparedCell, Comparison, DEFAULT_BIBLES, Marks, VerseBlock, WordDiff,
    verse_blocks, word_diff,
};
pub use copy_verses::{
    BookNames, CopiedBible, CopiedBlock, CopiedText, CopiedVerse, CopyFormat, CopyOptions,
    CopyPreset, CopyVerses, ReferencePlacement, VerseLayout, copy_verses, html_document,
};
pub use factbook::{
    DictionaryArticle, DirectoryEntry, EntityCard, EntityLink, FactbookChooser, FactbookSummary,
    FamilyFact, NameMeaning, PeopleDirectory, RelatedEntity,
};
pub use favorites::{Favorite, HistoryDay, SavedSearch, TabLocation, Visit, day_heading};
pub use guide::{
    CommentaryRow, GuideArrangement, GuideSection, Guides, LibrarySections, SectionData,
    SectionPlace, SectionSource, SectionStatus,
};
pub use import::{
    DEFERRED_INDEX, ImportIndexStatus, ImportIndexer, ImportPhase, ImportRecord, ImportReport,
    ImportSource, Imports, RepoOrigin, UNAVAILABLE_INDEX,
};
pub use interlinear::{
    InterlinearKind, InterlinearLine, InterlinearLines, LexiconWord, cell_gloss, cell_texts,
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
pub use missing_resource::{EmptyState, MissingResource, StarterOffer, Tagging};
pub use notes::{Card, CardOrder, NoteHit, NotesFilter, NotesResults, NotesTool, body_references};
pub use notes_export::{NotesExport, NotesFormat, NotesSource};
pub use people_places::{ArticleLink, Fact, PersonOrPlace};
pub use persist::{LayoutNode, SNAPSHOT_VERSION, TabSnapshot, UserData, WorkspaceSnapshot};
pub use reading_plans::{
    PlanDate, PlanTemplate, Reading, ReadingPlan, ReadingPlans, WEEKDAY_NAMES, Weekdays,
};
pub use reanchor::{Outcome, Plan, Planned, ReanchorReport, Reanchoring};
#[cfg(not(target_arch = "wasm32"))]
pub use search::CacheIndexer;
pub use search::{
    INDEX_VERSION, IndexStatus, ResourceHits, SearchIndexes, SearchRequest, SearchResults,
    default_search_cache_dir,
};
pub use search_views::{
    AnalysisColumn, AnalysisRow, AnalysisSort, ChartBar, ChartBy, HitGrid, ResultsFilter,
    ResultsView, SearchAnalysis, chart_bars,
};
pub use session::{JobMode, Session};
pub use toolbar::{
    BUILTIN_LAYOUTS, Launcher, LayoutCard, LayoutThumbnail, LibraryLauncherRow,
    LibraryLauncherSection,
};
pub use toolbar_settings::{LauncherLabels, ToolbarPosition, ToolbarSettings};
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
pub use workspace_commands::{MENUS, Menu, MenuItem, choices, parallel_resources};

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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod parallel_resources_tests;
