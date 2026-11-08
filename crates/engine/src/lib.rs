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
pub mod atlas;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod atlas_tests;
pub mod backups;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod backups_tests;
pub mod base_map;
pub mod bible_search;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod bible_search_tests;
mod builtin;
pub mod collections;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod collections_tests;
pub mod command;
pub mod command_box;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod command_box_tests;
pub mod compare;
pub mod contents;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod contents_tests;
pub mod copy_verses;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod copy_verses_tests;
pub mod credits;
pub mod cross_references;
pub mod diagnostics;
pub mod drag_drop;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod drag_drop_tests;
pub mod exegetical_guide;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod exegetical_guide_tests;
pub mod factbook;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod factbook_tests;
pub mod favorites;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod favorites_tests;
pub mod floating_windows;
#[cfg(test)]
mod floating_windows_tests;
pub mod get_bibles;
#[cfg(test)]
mod get_bibles_tests;
pub mod guide;
#[cfg(test)]
mod guide_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod guide_words_tests;
pub mod highlight_styles;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod highlight_styles_tests;
pub mod home;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod home_tests;
pub mod import;
pub mod interlinear;
#[cfg(test)]
mod interlinear_tests;
pub mod jobs;
pub mod key_bindings;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod key_bindings_tests;
pub mod key_words;
pub mod keyboard_navigation;
#[cfg(test)]
mod keyboard_navigation_tests;
pub mod language_names;
#[cfg(test)]
mod language_names_tests;
mod library;
mod library_commands;
pub mod library_search;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod library_search_tests;
pub mod licence;
pub mod link;
mod lookups;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod lookups_tests;
pub mod missing_resource;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod missing_resource_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod morph_search_tests;
pub mod notes;
pub mod notes_export;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod notes_export_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod notes_tests;
pub mod open_catalogue;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod open_catalogue_tests;
pub mod parallel_passages;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod parallel_passages_tests;
pub mod people_places;
pub mod persist;
pub mod reading_layout;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod reading_layout_tests;
pub mod reading_plans;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod reading_plans_tests;
pub mod reanchor;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod reanchor_tests;
pub mod resource_navigation;
pub mod saved_searches;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod saved_searches_tests;
pub mod search;
#[cfg(test)]
mod search_tests;
pub mod search_views;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod search_views_tests;
mod session;
pub mod settings;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod settings_tests;
pub mod starter;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod starter_tests;
pub mod strongs_index;
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub mod testing;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod text_size_tests;
pub mod timeline;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod timeline_tests;
pub mod toolbar;
pub mod toolbar_settings;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod toolbar_settings_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod toolbar_tests;
pub mod topic_guide;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod topic_guide_tests;
pub mod visual_filters;
#[cfg(test)]
mod visual_filters_tests;
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
    Anchor, Annotation, Change, Content, NoteTarget, Notebook, NotebookDeletion, Row, Selection,
    TextPos, TextRange, UndoHistory, UndoStep,
};
pub use appearance::{
    Appearance, FontChoice, FontChoices, FontSlot, InterfaceSize, PageTone, Theme,
};
pub use atlas::{Confidence, MapPlace, PlacePoint};
pub use backups::{DataExport, LastRestore, RestoreLine, RestorePreview};
pub use base_map::{BaseMap, GeoBounds, MapView};
pub use bible_search::{
    BibleOutline, BibleResults, ErrorField, InlineError, RowHit, Scope, ScopePreset, SearchMode,
    SearchRequestFields, SearchTab, SearchedBible, Snippet, VerseRow, parse_scope, scope_presets,
    verse_rows,
};
pub use collections::{Collection, CollectionRule, CollectionSet, parse_rules, rules_text};
pub use command::{CommandError, CommandRegistry, CommandSpec, ParamKind, ParamSpec, Params};
pub use command_box::{
    COMMANDS_PREFIX, CommandBoxAction, IN_PLACE_COMMANDS, SCRIPTING_COMMANDS, Suggestion, is_listed,
};
pub use compare::{
    Cell, ComparedBible, ComparedCell, Comparison, DEFAULT_BIBLES, Marks, VerseBlock, WordDiff,
    verse_blocks, word_diff,
};
pub use copy_verses::{
    BookNames, CopiedBible, CopiedBlock, CopiedText, CopiedVerse, CopyFormat, CopyOptions,
    CopyPreset, CopyVerses, ReferencePlacement, VerseLayout, copy_verses, html_document,
};
pub use diagnostics::{CrashReport, LogLevel, ZipEntry};
pub use exegetical_guide::{
    ExegeticalVerse, ExegeticalWord, GrammarNote, LexiconSummary, WordByWord,
};
pub use factbook::{
    DictionaryArticle, DirectoryEntry, EntityCard, EntityLink, FactbookChooser, FactbookSummary,
    FamilyFact, NameMeaning, PeopleDirectory, RelatedEntity,
};
pub use favorites::{Favorite, HistoryDay, SavedSearch, TabLocation, Visit, day_heading};
pub use floating_windows::{FloatingWindow, ScreenRect, WindowId, WindowPlacement, WindowSnapshot};
pub use guide::{
    CommentaryRow, GuideArrangement, GuideSection, Guides, LibrarySections, SectionData,
    SectionPlace, SectionSource, SectionStatus,
};
pub use highlight_styles::{
    ACTIVE_PALETTE_PREF, HighlightLook, HighlightPalette, HighlightStyle, PaletteFile,
    PaletteFileStyle, Rgb, STANDARD_PALETTE_ID, STYLE_SLOTS, StyleAppearance, StyleEdit, StyleTone,
    style_label,
};
pub use home::{
    CardPlace, HOME_PREF, HomeCard, HomePlan, HomeReading, HomeSettings, LibraryCount,
    QUICK_STARTS, QuickStart, StarterSummary, VERSES_OF_THE_DAY, VerseOfTheDay,
    verse_of_the_day_on,
};
pub use import::{
    DEFERRED_INDEX, ImportIndexStatus, ImportIndexer, ImportPhase, ImportRecord, ImportReport,
    ImportSource, Imports, OpenOrigin, RepoOrigin, UNAVAILABLE_INDEX,
};
pub use interlinear::{
    InterlinearKind, InterlinearLine, InterlinearLines, LexiconWord, cell_gloss, cell_texts,
};
pub use jobs::{
    Job, JobContext, JobEvent, JobEventKind, JobId, JobOutput, JobQueue, Step, sword_installer,
};
pub use key_bindings::{KeyBinding, KeyConflict, SELECTION_KEYS, SelectionAction, SelectionKey};
pub use key_words::{KeyWord, KeyWords};
pub use language_names::{LanguageName, language};
pub use library::{
    Library, LibraryEntry, LibraryFilter, STARTER_PREFERRED_ORDER, default_library_dir,
    default_user_db, language_name,
};
pub use library_commands::annotations_phrase;
pub use library_search::{
    LIBRARY_TYPES, LibraryGroup, LibraryResults, LibraryRow, LibraryScope, SearchedResource,
};
pub use licence::{LicenceClass, LicenceFilter, licence_class};
pub use link::LINK_SETS;
pub use missing_resource::{EmptyState, MissingResource, StarterOffer, Tagging};
pub use notes::{
    Card, CardOrder, NoteBodyLine, NoteHit, NotesFilter, NotesResults, NotesTool, body_references,
    body_references_in, link_passages, note_body,
};
pub use notes_export::{NotesExport, NotesFormat, NotesSource};
/// The Morphology chooser's menus and choices (#145).
pub use obs_search::{MorphCategory, MorphCriteria};
pub use parallel_passages::{ParallelGroup, ParallelPassages, ParallelRow};
pub use people_places::{ArticleLink, Fact, PersonOrPlace};
pub use persist::{
    LayoutNode, SNAPSHOT_VERSION, ScrollPosition, TabSnapshot, UserData, WorkspaceSnapshot,
};
pub use reading_layout::{COLUMN_WIDTHS, TEXT_FLOWS, TabReadingLayout};
pub use reading_plans::{
    PlanDate, PlanTemplate, Reading, ReadingPlan, ReadingPlans, WEEKDAY_NAMES, Weekdays,
};
pub use reanchor::{Outcome, Plan, Planned, ReanchorReport, Reanchoring};
pub use saved_searches::{NamedSearch, SEARCH_HISTORY_LIMIT, SearchHistoryEntry, TypedFields};
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
pub use settings::{
    SETTINGS, Setting, SettingKind, SettingsSection, setting_asking_first, setting_by_id,
    setting_matches, settings_matching,
};
pub use timeline::{Emphasis, Timeline, TimelineEntry, TimelineView};
pub use toolbar::{
    BUILTIN_LAYOUTS, Launcher, LayoutCard, LayoutThumbnail, LibraryLauncherRow,
    LibraryLauncherSection,
};
pub use toolbar_settings::{LauncherLabels, ToolbarPosition, ToolbarSettings};
pub use topic_guide::{
    CitedPassage, Subtopic, TopicChoice, TopicChooser, TopicDirectory, TopicGuides, TopicalEntry,
};
pub use visual_filters::{
    AlignedWords, DocumentSlice, FilterHit, FilterMatcher, MarkedText, TabFilters, VisualFilter,
    block_hits, built_in_filters, paragraph_slices,
};
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
    CameBackBeside, ClosedTabs, Effect, Guide, HeaderBox, History, OpenRequest, PanelId, Placement,
    RECENTLY_CLOSED_LIMIT, RecentlyClosed, Side, Tab, TabId, TabKind, Tool, Workspace,
};
pub use workspace_commands::{
    BACKUPS_TITLE, MENUS, Menu, MenuItem, choices, menu_of, parallel_resources,
};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod workspace_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod recently_closed_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod persist_tests;

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
