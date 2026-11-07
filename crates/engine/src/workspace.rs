//! The Workspace model: Panels, Tabs and Tab kinds, per-Tab history, and the placement rules
//! for opening a Tab (#18 rule 2). The UI shows it (egui_tiles in `ui-egui`) and reports back
//! what the user rearranged; every change made by a command is also queued as an [`Effect`]
//! for the UI to apply.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::resource_navigation::ResourceNavigation;
use obs_model::{Key, KeyScheme, UnitId};
use obs_scripture::{ListedVersification, Passage, Verse, VerseMap};
use serde::{Deserialize, Serialize};

use crate::compare::{Comparison, in_range_chapters};
use crate::floating_windows::{FloatingWindow, WindowId, WindowPlacement};
use crate::link::{ToStandard, to_native, to_standard};
use crate::persist::{LayoutNode, ScrollPosition, TabSnapshot, WorkspaceSnapshot};

mod recently_closed;
pub use recently_closed::{ClosedTabs, RECENTLY_CLOSED_LIMIT};

/// A Tab, stable for the life of the Workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TabId(pub u64);

/// A Panel: a dockable region holding one or more Tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PanelId(pub u64);

/// The Tools of v0.1 plus Cross-references, Favorites and History (#155) and Reading plans
/// (#147).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Tool {
    Search,
    TextComparison,
    Notes,
    WordInfo,
    CrossReferences,
    Favorites,
    History,
    ReadingPlans,
    /// An offline map of the places a Passage names (#150).
    Atlas,
    /// Biblical history on a zoomable axis: Eras, Lifespans and Events (#185).
    Timeline,
}

/// The Guides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Guide {
    PassageGuide,
    WordStudy,
    /// A hub page for one person, people group or place of a People & places index (#149).
    Factbook,
    /// A Passage word by word in its original language (#144).
    ExegeticalGuide,
    /// One topic across the topical indexes and Dictionaries (#148).
    TopicGuide,
}

impl Tool {
    pub fn title(self) -> &'static str {
        match self {
            Tool::Search => "Search",
            Tool::TextComparison => "Text Comparison",
            Tool::Notes => "Notes",
            Tool::WordInfo => "Word Info",
            Tool::CrossReferences => "Cross-references",
            Tool::Favorites => "Favorites",
            Tool::History => "History",
            Tool::ReadingPlans => "Reading plans",
            Tool::Atlas => "Atlas",
            Tool::Timeline => "Timeline",
        }
    }
}

impl Guide {
    pub fn title(self) -> &'static str {
        match self {
            Guide::PassageGuide => "Passage Guide",
            Guide::WordStudy => "Word Study",
            Guide::Factbook => "Factbook",
            Guide::ExegeticalGuide => "Exegetical Guide",
            Guide::TopicGuide => "Topic Guide",
        }
    }
}

/// What a Tab shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TabKind {
    Library,
    /// A Resource from the library, by its resource id.
    Resource(String),
    /// A Resource's About Tab (keyless): manifest, licence and Attribution, by resource id.
    About(String),
    /// Live or latest Import report, keyed by the imported Resource id.
    ImportReport(String),
    /// Get Bibles (#91): download Bibles from SWORD repositories. Keyless, single instance.
    GetBibles,
    /// Library → Starter library… (#131): which Starter Resources are in the Library, with
    /// Add back and the download. Keyless, single instance.
    StarterLibrary,
    /// Help → Credits (#67): Starter Resources, data built into the app, fonts and
    /// third-party licences. Keyless, single instance.
    Credits,
    /// Home (#178): today's reading, where the user left off, History and Favorites, the
    /// Verse of the day, Quick start and the library summary. Keyless, single instance.
    Home,
    /// Help → Keyboard shortcuts (#177): every key binding, to see and change. Single
    /// instance, with a filter box.
    KeyboardShortcuts,
    /// Settings (#213, Ctrl+,): every preference in one place, with a search box. Single
    /// instance.
    Settings,
    Guide(Guide),
    Tool(Tool),
}

/// What a Tab's header box holds (#18 rule 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderBox {
    /// A reference box, for Passage Tabs.
    Reference,
    /// A Strong's number or English word (Word Study).
    Word,
    /// The name of a person or place (Factbook).
    Name,
    /// A topic (Topic Guide).
    Topic,
    /// A Lexicon Strong's number or Lemma.
    Lexicon,
    /// A Dictionary headword.
    Headword,
    /// A filter field, and no link chip (Library, Notes).
    Filter,
    /// "Follows hover" (Word Info).
    FollowsHover,
    /// No box: the Tab has no Key (About).
    Keyless,
}

/// Which remembered-Panel slot a Tab kind uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
enum Slot {
    Library,
    Resource,
    About,
    ImportReport,
    GetBibles,
    StarterLibrary,
    Credits,
    Home,
    KeyboardShortcuts,
    Settings,
    Guide(Guide),
    Tool(Tool),
}

impl TabKind {
    /// Library, Notes and Word Info have at most one Tab each, and each Resource at most
    /// one About Tab.
    pub fn single_instance(&self) -> bool {
        matches!(
            self,
            TabKind::Library
                | TabKind::About(_)
                | TabKind::ImportReport(_)
                | TabKind::GetBibles
                | TabKind::StarterLibrary
                | TabKind::Credits
                | TabKind::Home
                | TabKind::KeyboardShortcuts
                | TabKind::Settings
                | TabKind::Tool(
                    Tool::Notes
                        | Tool::WordInfo
                        | Tool::Favorites
                        | Tool::History
                        | Tool::ReadingPlans
                )
        )
    }

    /// The Resource a Tab of this kind shows or is about, by resource id.
    pub fn resource_id(&self) -> Option<&str> {
        match self {
            TabKind::Resource(id) | TabKind::About(id) | TabKind::ImportReport(id) => Some(id),
            _ => None,
        }
    }

    /// Resources open in reading Panels; everything else in non-reading Panels.
    pub fn is_reading(&self) -> bool {
        matches!(self, TabKind::Resource(_))
    }

    /// Whether Tabs of this kind belong with reading Panels: a new one goes to the last
    /// reading Panel when no Panel is remembered for it, and focusing one makes its Panel the
    /// last reading Panel. Resources, and Home, which wants the room (#178) but is not Bible
    /// content ([`TabKind::is_reading`]).
    pub(crate) fn opens_in_reading_panel(&self) -> bool {
        matches!(self, TabKind::Resource(_) | TabKind::Home)
    }

    pub fn header_box(&self) -> HeaderBox {
        match self {
            TabKind::Library
            | TabKind::KeyboardShortcuts
            | TabKind::Settings
            | TabKind::Tool(Tool::Notes | Tool::Favorites | Tool::History) => HeaderBox::Filter,
            TabKind::Tool(Tool::WordInfo) => HeaderBox::FollowsHover,
            TabKind::Guide(Guide::WordStudy) => HeaderBox::Word,
            TabKind::Guide(Guide::Factbook) => HeaderBox::Name,
            TabKind::Guide(Guide::TopicGuide) => HeaderBox::Topic,
            // The query, mode and scope fields are the Tab's body (#59).
            TabKind::Tool(Tool::Search) => HeaderBox::Keyless,
            TabKind::About(_)
            | TabKind::ImportReport(_)
            | TabKind::GetBibles
            | TabKind::StarterLibrary
            | TabKind::Credits
            | TabKind::Home
            | TabKind::Tool(Tool::ReadingPlans) => HeaderBox::Keyless,
            _ => HeaderBox::Reference,
        }
    }

    /// Whether it has a Find bar (Ctrl+F, #175): a Resource or Text Comparison Tab.
    pub fn has_find_bar(&self) -> bool {
        matches!(
            self,
            TabKind::Resource(_) | TabKind::Tool(Tool::TextComparison)
        )
    }

    /// Whether the header has a link chip.
    pub fn has_link_chip(&self) -> bool {
        !matches!(
            self,
            TabKind::Library
                | TabKind::About(_)
                | TabKind::ImportReport(_)
                | TabKind::GetBibles
                | TabKind::StarterLibrary
                | TabKind::Credits
                | TabKind::Home
                | TabKind::KeyboardShortcuts
                | TabKind::Settings
                | TabKind::Guide(Guide::Factbook)
                | TabKind::Tool(Tool::Notes | Tool::Favorites | Tool::History | Tool::ReadingPlans)
        )
    }

    /// Key schemes a Tab of this kind follows. A Resource's come from its manifest instead.
    pub fn default_key_schemes(&self) -> Vec<KeyScheme> {
        match self {
            TabKind::Guide(Guide::PassageGuide | Guide::ExegeticalGuide)
            | TabKind::Tool(
                Tool::TextComparison | Tool::CrossReferences | Tool::Atlas | Tool::Timeline,
            )
            | TabKind::Resource(_) => vec![KeyScheme::Passage],
            TabKind::Guide(Guide::WordStudy) => vec![KeyScheme::StrongsNumber],
            // A Topic Guide follows (and moves) its Link set's Dictionary headwords.
            TabKind::Guide(Guide::TopicGuide) => vec![KeyScheme::Headword],
            _ => Vec::new(),
        }
    }

    fn slot(&self) -> Slot {
        match self {
            TabKind::Library => Slot::Library,
            TabKind::Resource(_) => Slot::Resource,
            TabKind::About(_) => Slot::About,
            TabKind::ImportReport(_) => Slot::ImportReport,
            TabKind::GetBibles => Slot::GetBibles,
            TabKind::StarterLibrary => Slot::StarterLibrary,
            TabKind::Credits => Slot::Credits,
            TabKind::Home => Slot::Home,
            TabKind::KeyboardShortcuts => Slot::KeyboardShortcuts,
            TabKind::Settings => Slot::Settings,
            TabKind::Guide(g) => Slot::Guide(*g),
            TabKind::Tool(t) => Slot::Tool(*t),
        }
    }
}

/// A Tab's Back/Forward history of locations, in the Tab's own numbering.
///
/// A Resource Tab that has switched between parallel resources (#151) also remembers which
/// Resource each location is in, so Back and Forward switch it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct History<T = Verse> {
    back: Vec<Stop<T>>,
    current: Option<T>,
    /// The Resource `current` is in, once the Tab has switched Resources (else `None`).
    resource: Option<String>,
    forward: Vec<Stop<T>>,
}

/// One Back/Forward entry, a snapshot taken as it is pushed: a location, the Resource it is
/// in once the Tab has switched between parallel resources, and the text unit the view
/// showed there (#176), where that is known. A location alone may show another unit than the
/// one the Tab was at: a Commentary's book introduction starts at Gen 1:1, where the
/// narrower Gen 1:1-2 section wins.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Stop<T> {
    at: T,
    resource: Option<String>,
    unit: Option<UnitId>,
}

/// History keeps at most this many entries each way.
pub const HISTORY_LIMIT: usize = 100;

impl<T> Default for History<T> {
    fn default() -> Self {
        Self {
            back: Vec::new(),
            current: None,
            resource: None,
            forward: Vec::new(),
        }
    }
}

impl<T: Clone + PartialEq> History<T> {
    pub fn current(&self) -> Option<T> {
        self.current.clone()
    }

    /// The Resource the current location is in, once the Tab has switched between parallel
    /// resources (#151); `None` before.
    pub fn resource(&self) -> Option<&str> {
        self.resource.as_deref()
    }

    fn push_back(&mut self, stop: Stop<T>) {
        self.back.push(stop);
        if self.back.len() > HISTORY_LIMIT {
            self.back.remove(0);
        }
    }

    /// The current location as a Back/Forward entry, its view showing unit `shown`.
    fn leave(&mut self, shown: Option<UnitId>) -> Option<Stop<T>> {
        Some(Stop {
            at: self.current.take()?,
            resource: self.resource.clone(),
            unit: shown,
        })
    }

    /// A jump to `to`, which shows unit `to_unit` where known: the old location goes on the
    /// Back stack with `shown`, the unit the view showed there, and Forward is cleared. A
    /// jump to the same location is a step only when it shows another unit than `shown`.
    pub fn visit(&mut self, to: T, to_unit: Option<&UnitId>, shown: Option<UnitId>) {
        let moves = self.current.as_ref().is_some_and(|old| *old != to)
            || to_unit.is_some_and(|u| shown.as_ref().is_some_and(|s| s != u));
        if moves && let Some(stop) = self.leave(shown) {
            self.push_back(stop);
            self.forward.clear();
        }
        self.current = Some(to);
    }

    /// The Tab switched from Resource `from` to the parallel resource `to` (#151): the old
    /// location goes on the Back stack (in `from`, showing `shown`) and Forward is cleared.
    /// The new location is `at`, or unknown until the new Resource is loaded.
    pub fn switch_resource(
        &mut self,
        from: &str,
        to: String,
        at: Option<T>,
        shown: Option<UnitId>,
    ) {
        // Every location so far is in `from`, unless the Tab had switched before.
        for stop in self.back.iter_mut().chain(self.forward.iter_mut()) {
            stop.resource.get_or_insert_with(|| from.to_string());
        }
        self.resource.get_or_insert_with(|| from.to_string());
        if let Some(stop) = self.leave(shown) {
            self.push_back(stop);
        }
        self.forward.clear();
        self.current = at;
        self.resource = Some(to);
    }

    /// Scrolling moves the location without making a history entry.
    pub fn scrolled_to(&mut self, at: T) {
        self.current = Some(at);
    }

    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    /// Back one entry, leaving the current location showing unit `shown` for Forward: the
    /// location and the unit its view showed. [`Self::resource`] then tells which Resource it
    /// is in.
    pub fn back(&mut self, shown: Option<UnitId>) -> Option<(T, Option<UnitId>)> {
        let to = self.back.pop()?;
        if let Some(stop) = self.leave(shown) {
            self.forward.push(stop);
        }
        Some(self.arrive(to))
    }

    /// Forward one entry, leaving the current location showing unit `shown` for Back: the
    /// location and the unit its view showed. [`Self::resource`] then tells which Resource it
    /// is in.
    pub fn forward(&mut self, shown: Option<UnitId>) -> Option<(T, Option<UnitId>)> {
        let to = self.forward.pop()?;
        if let Some(stop) = self.leave(shown) {
            self.push_back(stop);
        }
        Some(self.arrive(to))
    }

    fn arrive(&mut self, to: Stop<T>) -> (T, Option<UnitId>) {
        self.resource = to.resource;
        self.current = Some(to.at.clone());
        (to.at, to.unit)
    }
}

/// One open Tab.
#[derive(Debug, Clone)]
pub struct Tab {
    pub id: TabId,
    pub kind: TabKind,
    pub title: String,
    pub history: History,
    pub entry_history: History<Key>,
    pub navigation: Option<Arc<ResourceNavigation>>,
    /// The Resource's own numbering, for reading typed references (ADR 0002 rule 10).
    pub versification: Option<Arc<ListedVersification>>,
    /// A Bible's Verse map to the Standard versification (none: it numbers like the Standard).
    pub verse_map: Option<Arc<VerseMap>>,
    /// The Key schemes this Tab follows in a Link set.
    pub key_schemes: Vec<KeyScheme>,
    /// The Link set this Tab belongs to, if any.
    pub link: Option<char>,
    /// What a Text Comparison Tab compares (unused by other kinds).
    pub comparison: Comparison,
    /// A Search Tab's mode, fields and results (unused by other kinds).
    pub search: crate::bible_search::SearchTab,
    /// Where a Resource Tab that has just switched to a parallel resource (#151) goes once
    /// that Resource is loaded: its old location, as a Standard Verse.
    pub arriving_at: Option<Verse>,
    /// A Bible Tab's interlinear lines (#143).
    pub interlinear: crate::interlinear::InterlinearLines,
    /// A Resource Tab shows its Contents pane (#176).
    pub contents: bool,
    /// The text unit at the top of the Tab's view, as the UI reports it from the live view
    /// each time it draws or moves it ([`Workspace::view_shows`]); `None` for a Bible, or
    /// until reported. Each history entry snapshots it as it is pushed (#176).
    pub view_unit: Option<UnitId>,
    /// The whole Passage a Passage Guide, Exegetical Guide or Cross-references Tab shows when
    /// a command or typed reference gave a chapter or range (Standard numbering). Kept while
    /// the Tab's location is the Passage's first Verse; otherwise the Tab shows that Verse.
    pub passage: Option<Passage>,
}

impl Tab {
    /// A Tab as saved in a snapshot, given id `id`: at its saved locations without history,
    /// with nothing of its Resource loaded yet.
    pub(crate) fn from_snapshot(id: TabId, t: TabSnapshot) -> Self {
        let mut history = History::default();
        if let Some(v) = t.location {
            history.scrolled_to(v);
        }
        let mut entry_history = History::default();
        if let Some(k) = t.entry_location {
            entry_history.scrolled_to(k);
        }
        Tab {
            id,
            key_schemes: t.kind.default_key_schemes(),
            kind: t.kind,
            title: t.title,
            history,
            entry_history,
            navigation: None,
            versification: None,
            verse_map: None,
            link: t.link,
            comparison: Comparison::default(),
            search: Default::default(),
            arriving_at: None,
            interlinear: t.interlinear,
            contents: t.contents,
            view_unit: None,
            passage: None,
        }
    }

    pub fn header_box(&self) -> HeaderBox {
        if matches!(self.kind, TabKind::Resource(_)) {
            if self.key_schemes.contains(&KeyScheme::Headword) {
                return HeaderBox::Headword;
            }
            if self.key_schemes.contains(&KeyScheme::StrongsNumber)
                || self.key_schemes.contains(&KeyScheme::Lemma)
            {
                return HeaderBox::Lexicon;
            }
        }
        self.kind.header_box()
    }

    /// The entry Key this Tab shows for a Link set's `key`: a Lexicon's or Dictionary's own
    /// entry Key; a Word Study takes any Strong's number as it is (#55).
    fn entry_key(&self, key: &Key) -> Option<Key> {
        if self.kind == TabKind::Guide(Guide::WordStudy) {
            return matches!(key, Key::StrongsNumber(_)).then(|| key.clone());
        }
        // A Topic Guide takes any headword as its topic (#148).
        if self.kind == TabKind::Guide(Guide::TopicGuide) {
            return matches!(key, Key::Headword(_)).then(|| key.clone());
        }
        if self.kind == TabKind::Guide(Guide::Factbook) {
            return None;
        }
        Some(self.navigation.as_ref()?.lookup(key)?.key.clone())
    }

    fn local_passage(&self, verse: Verse) -> Option<Verse> {
        if let Some(nav) = &self.navigation
            && self.follows_passages()
        {
            return match &nav.section(verse)?.key {
                Key::Passage(p) => Some(p.start()),
                _ => None,
            };
        }
        Some(verse)
    }

    /// A jump to Verse `to`, showing unit `unit` when it is reached by its unit (a Contents
    /// line, a search hit), else the unit the Verse alone shows: a step in the Tab's history.
    pub fn visit(&mut self, to: Verse, unit: Option<&UnitId>) {
        let reached = unit
            .cloned()
            .or_else(|| self.reaches(&Key::Passage(Passage::verse(to))));
        self.history
            .visit(to, reached.as_ref(), self.view_unit.clone());
    }

    /// A jump to entry Key `to`, like [`Self::visit`].
    pub fn visit_entry(&mut self, to: Key, unit: Option<&UnitId>) {
        let reached = unit.cloned().or_else(|| self.reaches(&to));
        self.entry_history
            .visit(to, reached.as_ref(), self.view_unit.clone());
    }

    /// The unit `key` alone shows in this Tab's Resource (a Commentary's narrowest section at
    /// a Verse, a Dictionary's first entry under a headword), once its navigation is loaded.
    fn reaches(&self, key: &Key) -> Option<UnitId> {
        let nav = self.navigation.as_ref()?;
        match key {
            Key::Passage(p) => nav.section(p.start()),
            k => nav.lookup(k),
        }
        .map(|e| e.unit.clone())
    }

    fn follows_passages(&self) -> bool {
        self.key_schemes.contains(&KeyScheme::Passage)
    }

    /// Whether the Tab's location is an entry Key (Lexicon, Dictionary, Word Study, Factbook,
    /// Topic Guide) kept in `entry_history`, rather than a Verse kept in `history`.
    pub fn keeps_entries(&self) -> bool {
        matches!(
            self.header_box(),
            HeaderBox::Lexicon
                | HeaderBox::Headword
                | HeaderBox::Word
                | HeaderBox::Name
                | HeaderBox::Topic
        )
    }

    /// A Back/Forward step left the history in another Resource than the Tab shows: show
    /// that one (#151). True when it did.
    fn follow_history_resource(&mut self) -> bool {
        let resource = if self.keeps_entries() {
            self.entry_history.resource()
        } else {
            self.history.resource()
        };
        let Some(resource) = resource.map(String::from) else {
            return false;
        };
        if self.kind == TabKind::Resource(resource.clone()) {
            return false;
        }
        self.show_resource(resource);
        true
    }

    /// Show another Resource: what was loaded from the old one is dropped until the UI loads
    /// the new one.
    fn show_resource(&mut self, resource: String) {
        self.kind = TabKind::Resource(resource);
        self.navigation = None;
        self.view_unit = None;
        self.versification = None;
        self.verse_map = None;
        self.arriving_at = None;
    }

    /// A jump, Back/Forward or follow moved the Tab: a Text Comparison range is cleared once
    /// the Tab leaves the range's chapters (#18 rule 13), a Guide's [`Self::passage`] once the
    /// Tab leaves its first Verse.
    fn moved_to(&mut self, verse: Verse) {
        if let Some(r) = self.comparison.range
            && !in_range_chapters(r, verse)
        {
            self.comparison.range = None;
        }
        if self.passage.is_some_and(|p| p.start() != verse) {
            self.passage = None;
        }
    }

    /// The Passage this Tab shows: its [`Self::passage`] while the Tab is at its first Verse,
    /// otherwise the Verse it is at.
    pub fn shown_passage(&self) -> Option<Passage> {
        let at = self.history.current()?;
        Some(
            self.passage
                .filter(|p| p.start() == at)
                .unwrap_or(Passage::verse(at)),
        )
    }
}

/// Where a new Tab goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Into an existing Panel.
    Panel(PanelId),
    /// Into a new Panel split off to the right of `beside` (or of the whole Workspace), or
    /// to its left when `before` (a reopened Panel that was first in its window, #214).
    NewPanel {
        panel: PanelId,
        beside: Option<PanelId>,
        before: bool,
    },
}

/// A change for the UI to apply to what it shows.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Opened {
        tab: TabId,
        placement: Placement,
    },
    Focus(TabId),
    Closed(TabId),
    /// Move a Tab's view to a location (a jump, Back or Forward).
    Navigate {
        tab: TabId,
        verse: Verse,
    },
    /// Move a Lexicon or Dictionary to its entry Key.
    NavigateEntry {
        tab: TabId,
        key: Key,
    },
    /// Move a Resource Tab to a Text range of its text (a Library search hit, #60): the
    /// paragraph `range` of `unit` goes to the top of the view and is flashed. `key` is the
    /// Key that covers it, to fall back on when the text is not shown.
    NavigateText {
        tab: TabId,
        unit: obs_model::UnitId,
        range: std::ops::Range<u32>,
        key: Option<Key>,
    },
    /// Back or Forward returned a Resource Tab to a history entry whose view showed text unit
    /// `unit` (#176): after the move to its location, the view goes to that unit where the
    /// location alone shows another (a book introduction starting where a section does) and
    /// the unit is still in the Resource.
    ShowUnit {
        tab: TabId,
        unit: UnitId,
    },
    /// A Link-set member follows another; `flash` marks the target after an explicit jump.
    Follow {
        tab: TabId,
        verse: Verse,
        flash: bool,
    },
    /// Rebuild the default arrangement.
    ResetLayout,
    /// The Tabs and arrangement were replaced (a Layout or the Last session was restored):
    /// rebuild the dock from [`Workspace::arrangement`].
    Rebuild,
    /// Ask the user for a Layout name: to save the current Workspace, or to rename `rename`.
    PromptLayoutName {
        rename: Option<String>,
    },
    /// A short message for the user (a toast).
    Message(String),
    /// A titled text window (About).
    Show {
        title: String,
        body: String,
    },
    /// Ask the user to confirm removing a Resource; `message` states how many Annotations
    /// refer to it. Confirming runs `library.remove` with `confirm`.
    ConfirmRemove {
        resource_id: String,
        title: String,
        message: String,
    },
    /// A Resource left the library: drop anything loaded from it.
    Removed(String),
    /// A Resource's file was replaced under the same id (a Resource revision, a re-Import):
    /// drop anything loaded from it and load its open Tabs again.
    Replaced(String),
    /// Go to a Standard Verse in a Tab opened just now, once its Verse map is loaded (the
    /// Notes Tool following a Passage anchor, #64): the Tab's Link set follows.
    NavigateStandard {
        tab: TabId,
        verse: Verse,
    },
    /// Go to a location, in the Tab's own numbering, in a Resource Tab opened just now at it
    /// (a Favorite or a History Visit), once its Verse map is loaded: a jump, so its Link set
    /// follows.
    NavigateOpened {
        tab: TabId,
        verse: Option<Verse>,
        key: Option<Key>,
    },
    /// Bring a position in a Tab's Resource into view: a text unit and a UTF-8 byte offset
    /// into it (the Notes Tool following a Text-range anchor, #64).
    RevealText {
        tab: TabId,
        unit: obs_model::UnitId,
        byte: usize,
    },
    /// Host opens a native picker off the UI thread, then runs `import.start`.
    PickImport,
    /// Open the Fonts window (View → Fonts…, #138).
    ShowFonts,
    /// Open the Highlight styles window (View → Highlight styles…, #181).
    ShowHighlightStyles,
    /// A Resource Tab now shows another Resource (a parallel resource, or Back/Forward across
    /// a switch, #151): load it, at the Tab's location.
    Switched(TabId),
    /// Put the keyboard in the Command box (Ctrl+L, #139).
    FocusCommandBox,
    /// Open the Copy verses window (Tools → Copy verses…, #154), with these Passages (a Bible
    /// reference) and Bibles (resource ids) when given; otherwise at the Current location of
    /// the most recent Bible Tab.
    ShowCopyVerses {
        passages: Option<String>,
        bibles: Vec<String>,
    },
    /// Drop down a toolbar Launcher (Alt+L, Alt+G, Alt+T, Alt+Y, #140).
    OpenLauncher(crate::toolbar::Launcher),
    /// Open the Customize toolbar window (View → Customize toolbar…, #141).
    CustomizeToolbar,
    /// Move a Tab out of its Panel into a new floating window (#152), as the one Tab of
    /// Panel `panel` there, at `placement` when given.
    Float {
        tab: TabId,
        window: WindowId,
        panel: PanelId,
        placement: Option<WindowPlacement>,
    },
    /// Move a Tab out of its floating window into a new Panel `panel` at the right of the
    /// main window (#152).
    Dock {
        tab: TabId,
        panel: PanelId,
    },
    /// Close a floating window, its Panels moving to the right of the main window (#152).
    DockWindow(WindowId),
    /// Open a Resource or Text Comparison Tab's Find bar and put the keyboard in it (Ctrl+F,
    /// #175), with `text` to find when given.
    Find {
        tab: TabId,
        text: Option<String>,
    },
    /// Move a Tab's Find bar to its next match (or previous, without `forward`).
    FindStep {
        tab: TabId,
        forward: bool,
    },
    /// Scroll a reopened Resource Tab (#214) back to where its view was when it was closed,
    /// once it is loaded at its Current location.
    RestoreScroll {
        tab: TabId,
        at: ScrollPosition,
    },
}

/// How the caller asked for a Tab to be opened.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OpenRequest {
    /// The Panel whose + was used.
    pub panel: Option<PanelId>,
    /// Ctrl/Cmd was held: always split off a new Panel.
    pub split: bool,
}

/// The live arrangement of Panels and Tabs.
#[derive(Debug, Default)]
pub struct Workspace {
    tabs: BTreeMap<TabId, Tab>,
    /// Each Panel's Tabs in order, and its active Tab.
    panels: BTreeMap<PanelId, (Vec<TabId>, Option<TabId>)>,
    next_tab: u64,
    next_panel: u64,
    focused: Option<TabId>,
    last_reading_panel: Option<PanelId>,
    last_other_panel: Option<PanelId>,
    remembered: BTreeMap<Slot, PanelId>,
    effects: Vec<Effect>,
    /// When each Tab was last active (a counter), for "the most recently active Tab".
    activity: BTreeMap<TabId, u64>,
    clock: u64,
    /// How the UI currently arranges the Panels (kept up to date by the UI, for saving).
    pub arrangement: Option<LayoutNode>,
    /// The floating windows (#152), oldest first: each one's Panels and place on screen,
    /// kept up to date by the UI like [`Self::arrangement`].
    pub floating: Vec<FloatingWindow>,
    next_window: u64,
    /// Tabs that were opened or jumped since the last [`Workspace::take_visits`], oldest
    /// first, for History (#155).
    visits: Vec<PendingVisit>,
    /// Tabs closed while History still had Visits of theirs to record, kept until it has.
    closed_with_visits: BTreeMap<TabId, Tab>,
    /// Recently closed Tabs and Panels, oldest first, at most [`RECENTLY_CLOSED_LIMIT`] (#214).
    closed: Vec<ClosedTabs>,
    next_closed: u64,
}

/// A Tab opened or jumped, for History to record (#155).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingVisit {
    pub tab: TabId,
    /// Where a jump went, just after it. `None` for a Tab just opened, which the UI places
    /// later; History reads where it is then.
    pub at: Option<VisitedAt>,
}

/// Where a Tab was just after a jump: its Passage location, entry Key and Search fields, read
/// when the jump is made (the Tab may move again before History records it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisitedAt {
    pub verse: Option<Verse>,
    pub key: Option<Key>,
    /// A Search Tab's fields (none for other Tabs, or with an empty query).
    pub search: Option<crate::favorites::SavedSearch>,
}

impl Workspace {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tab(&self, id: TabId) -> Option<&Tab> {
        self.tabs.get(&id)
    }

    pub fn tab_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        self.tabs.get_mut(&id)
    }

    pub fn tabs(&self) -> impl Iterator<Item = &Tab> {
        self.tabs.values()
    }

    pub fn panels(&self) -> impl Iterator<Item = (PanelId, &[TabId])> {
        self.panels.iter().map(|(p, (t, _))| (*p, t.as_slice()))
    }

    pub fn focused(&self) -> Option<TabId> {
        self.focused
    }

    pub fn panel_of(&self, tab: TabId) -> Option<PanelId> {
        self.panels
            .iter()
            .find(|(_, (tabs, _))| tabs.contains(&tab))
            .map(|(p, _)| *p)
    }

    /// A Panel's active Tab.
    pub fn active_in(&self, panel: PanelId) -> Option<TabId> {
        self.panels
            .get(&panel)
            .and_then(|(tabs, active)| active.or(tabs.first().copied()))
    }

    /// The id the next Tab opened will get.
    pub fn next_tab_id(&self) -> u64 {
        self.next_tab + 1
    }

    /// Every Tab, most recently active first.
    pub fn by_activity(&self) -> Vec<TabId> {
        let mut tabs: Vec<TabId> = self.tabs.keys().copied().collect();
        tabs.sort_by_key(|t| std::cmp::Reverse((self.activity.get(t).copied().unwrap_or(0), *t)));
        tabs
    }

    /// A fresh Panel id (for Panels the user makes by dragging a Tab out).
    pub fn new_panel_id(&mut self) -> PanelId {
        self.next_panel += 1;
        PanelId(self.next_panel)
    }

    /// Effects queued since the last call, for the UI to apply in order.
    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }

    pub fn push_effect(&mut self, e: Effect) {
        self.effects.push(e);
    }

    /// A Tab jumped (not scrolled, and not following its Link set): History records where
    /// it is now.
    pub(crate) fn visited(&mut self, tab: TabId) {
        let at = self.tabs.get(&tab).map(|t| VisitedAt {
            verse: t.history.current(),
            key: t.entry_history.current(),
            search: (t.kind == TabKind::Tool(Tool::Search))
                .then(|| crate::favorites::SavedSearch::of(&t.search))
                .flatten(),
        });
        self.visits.push(PendingVisit { tab, at });
    }

    /// Tabs opened or jumped since the last call, oldest first.
    pub fn take_visits(&mut self) -> Vec<PendingVisit> {
        std::mem::take(&mut self.visits)
    }

    /// Put back Visits History could not record yet (Tabs not yet placed, or a failed write),
    /// before any queued since they were taken. Closed Tabs no queued Visit needs are let go.
    pub(crate) fn keep_visits(&mut self, mut kept: Vec<PendingVisit>) {
        kept.append(&mut self.visits);
        self.visits = kept;
        let visits = &self.visits;
        self.closed_with_visits
            .retain(|id, _| visits.iter().any(|v| v.tab == *id));
    }

    /// Forget every queued Visit (History was cleared).
    pub(crate) fn forget_visits(&mut self) {
        self.visits.clear();
        self.closed_with_visits.clear();
    }

    /// The Tab a queued Visit is of: open, or closed since the Visit was queued.
    pub(crate) fn visited_tab(&self, id: TabId) -> Option<&Tab> {
        self.tabs
            .get(&id)
            .or_else(|| self.closed_with_visits.get(&id))
    }

    /// Take a Tab out, keeping it for History while a queued Visit is of it.
    fn remove_tab(&mut self, id: TabId) -> bool {
        let Some(t) = self.tabs.remove(&id) else {
            return false;
        };
        if self.visits.iter().any(|v| v.tab == id) {
            self.closed_with_visits.insert(id, t);
        }
        true
    }

    fn existing(&self, kind: &TabKind) -> Option<TabId> {
        self.tabs.values().find(|t| t.kind == *kind).map(|t| t.id)
    }

    /// Where a Tab of `kind` would go, per #18 rule 2.
    pub fn placement_for(&mut self, kind: &TabKind, req: OpenRequest) -> Placement {
        let live = |p: &PanelId| self.panels.contains_key(p);
        let beside = self.focused.and_then(|t| self.panel_of(t));
        if req.split {
            let panel = self.new_panel_id();
            return Placement::NewPanel {
                panel,
                beside,
                before: false,
            };
        }
        let chosen = req
            .panel
            .filter(live)
            .or_else(|| self.remembered.get(&kind.slot()).copied().filter(live))
            .or_else(|| {
                if kind.opens_in_reading_panel() {
                    self.last_reading_panel.filter(live)
                } else {
                    self.last_other_panel.filter(live)
                }
            });
        match chosen {
            Some(p) => Placement::Panel(p),
            None => {
                let panel = self.new_panel_id();
                let beside = beside.or_else(|| self.panels.keys().next_back().copied());
                Placement::NewPanel {
                    panel,
                    beside,
                    before: false,
                }
            }
        }
    }

    /// Open a Tab of `kind`, or focus the existing one for single-instance kinds.
    pub fn open(&mut self, kind: TabKind, title: String, req: OpenRequest) -> TabId {
        if kind.single_instance()
            && let Some(id) = self.existing(&kind)
        {
            self.focus(id);
            return id;
        }
        let placement = self.placement_for(&kind, req);
        // New linked Tabs join the opener's Link set.
        let key_schemes = kind.default_key_schemes();
        let link = if key_schemes.is_empty() || !kind.has_link_chip() {
            None
        } else {
            self.opener_link()
        };
        self.next_tab += 1;
        let id = TabId(self.next_tab);
        let panel = match placement {
            Placement::Panel(p) | Placement::NewPanel { panel: p, .. } => p,
        };
        let entry = self.panels.entry(panel).or_default();
        entry.0.push(id);
        entry.1 = Some(id);
        self.tabs.insert(
            id,
            Tab {
                id,
                kind,
                title,
                history: History::default(),
                entry_history: History::default(),
                navigation: None,
                versification: None,
                verse_map: None,
                key_schemes,
                link,
                comparison: Comparison::default(),
                search: Default::default(),
                arriving_at: None,
                interlinear: Default::default(),
                contents: false,
                view_unit: None,
                passage: None,
            },
        );
        // A linked Tool or Guide starts at its Link set's location. (Bibles wait until their
        // Verse map is known: the UI places them when it loads them.)
        if !self.tabs.get(&id).is_some_and(|t| t.kind.is_reading())
            && let Some(v) = self.set_location(id)
            && let Some(t) = self.tabs.get_mut(&id)
        {
            t.history.scrolled_to(v);
        }
        self.effects.push(Effect::Opened { tab: id, placement });
        self.visits.push(PendingVisit { tab: id, at: None });
        self.focus(id);
        id
    }

    /// The Link set a linked Tab opened now joins: the focused Tab's, when it follows a Key
    /// scheme (`None`: unlinked).
    pub fn opener_link(&self) -> Option<char> {
        self.focused
            .and_then(|t| self.tabs.get(&t))
            .filter(|t| !t.key_schemes.is_empty())
            .and_then(|t| t.link)
    }

    /// The most recently active Tab that `pred` accepts.
    pub fn most_recent(&self, pred: impl Fn(&Tab) -> bool) -> Option<TabId> {
        self.tabs
            .values()
            .filter(|t| pred(t))
            .max_by_key(|t| (self.activity.get(&t.id).copied().unwrap_or(0), t.id))
            .map(|t| t.id)
    }

    /// Make `tab` the focused Tab (and its Panel the last-active one of its kind).
    pub fn focus(&mut self, tab: TabId) {
        let Some(kind) = self.tabs.get(&tab).map(|t| t.kind.clone()) else {
            return;
        };
        self.note_active(tab, &kind);
        self.effects.push(Effect::Focus(tab));
        self.focused = Some(tab);
    }

    fn note_active(&mut self, tab: TabId, kind: &TabKind) {
        self.clock += 1;
        self.activity.insert(tab, self.clock);
        if let Some(p) = self.panel_of(tab) {
            self.remembered.insert(kind.slot(), p);
            if kind.opens_in_reading_panel() {
                self.last_reading_panel = Some(p);
            } else {
                self.last_other_panel = Some(p);
            }
            if let Some(entry) = self.panels.get_mut(&p) {
                entry.1 = Some(tab);
            }
        }
    }

    /// The user clicked into a Tab (no effect is queued: the UI already shows it).
    pub fn user_focused(&mut self, tab: TabId) {
        if let Some(kind) = self.tabs.get(&tab).map(|t| t.kind.clone()) {
            self.note_active(tab, &kind);
            self.focused = Some(tab);
        }
    }

    pub fn close(&mut self, tab: TabId) -> bool {
        if !self.remove_tab(tab) {
            return false;
        }
        self.activity.remove(&tab);
        for (tabs, active) in self.panels.values_mut() {
            tabs.retain(|t| *t != tab);
            if *active == Some(tab) {
                *active = tabs.last().copied();
            }
        }
        self.panels.retain(|_, (tabs, _)| !tabs.is_empty());
        if self.focused == Some(tab) {
            self.focused = None;
        }
        self.effects.push(Effect::Closed(tab));
        true
    }

    /// What the UI now shows after the user dragged, docked or closed Tabs: each Panel's
    /// Tabs in order. Tabs missing from every Panel are closed.
    pub fn sync_panels(&mut self, panels: Vec<(PanelId, Vec<TabId>)>) {
        let shown: Vec<TabId> = panels.iter().flat_map(|(_, t)| t.iter().copied()).collect();
        let gone: Vec<TabId> = self
            .tabs
            .keys()
            .filter(|t| !shown.contains(t))
            .copied()
            .collect();
        // The user closed them (× or a middle click): Reopen closed Tab brings them back (#214).
        self.remember_gone(&gone);
        for t in gone {
            self.remove_tab(t);
            self.activity.remove(&t);
            if self.focused == Some(t) {
                self.focused = None;
            }
        }
        let mut next = BTreeMap::new();
        for (p, tabs) in panels {
            let tabs: Vec<TabId> = tabs
                .into_iter()
                .filter(|t| self.tabs.contains_key(t))
                .collect();
            if tabs.is_empty() {
                continue;
            }
            let active = self
                .panels
                .get(&p)
                .and_then(|(_, a)| *a)
                .filter(|a| tabs.contains(a))
                .or_else(|| tabs.first().copied());
            self.next_panel = self.next_panel.max(p.0);
            next.insert(p, (tabs, active));
        }
        self.panels = next;
    }

    /// Record a jump in a Tab's history, move its view, and move its Link set (flashing the
    /// target in every member).
    pub fn navigate(&mut self, tab: TabId, verse: Verse) {
        if let Some(t) = self.tabs.get_mut(&tab)
            && let Some(local) = t.local_passage(verse)
        {
            t.visit(local, None);
            t.moved_to(local);
            self.effects.push(Effect::Navigate { tab, verse: local });
            self.visited(tab);
            self.broadcast(tab, verse, true);
        }
    }

    /// Jump to a Text range of a Resource Tab (a Library search hit) covered by `key`: the
    /// Key goes into the Tab's history (a Passage Key also moves its Link set, an entry Key
    /// the members that follow its scheme) and the view scrolls to the range.
    pub fn navigate_text(
        &mut self,
        tab: TabId,
        unit: obs_model::UnitId,
        range: std::ops::Range<u32>,
        key: Option<Key>,
    ) {
        let Some(t) = self.tabs.get_mut(&tab) else {
            return;
        };
        match &key {
            Some(Key::Passage(p)) if t.follows_passages() => {
                let start = p.start();
                t.visit(start, Some(&unit));
                t.moved_to(start);
                self.effects.push(Effect::NavigateText {
                    tab,
                    unit,
                    range,
                    key: key.clone(),
                });
                self.broadcast(tab, start, true);
            }
            Some(k) if !matches!(k, Key::Passage(_)) => {
                t.visit_entry(k.clone(), Some(&unit));
                self.effects.push(Effect::NavigateText {
                    tab,
                    unit,
                    range,
                    key: key.clone(),
                });
                self.broadcast_entry(tab, k);
            }
            _ => self.effects.push(Effect::NavigateText {
                tab,
                unit,
                range,
                key,
            }),
        }
        self.visited(tab);
    }

    /// Entry navigation broadcasts only to members supporting this Key scheme.
    pub fn navigate_entry(&mut self, tab: TabId, key: Key) {
        let Some(t) = self.tabs.get_mut(&tab) else {
            return;
        };
        t.visit_entry(key.clone(), None);
        self.effects.push(Effect::NavigateEntry {
            tab,
            key: key.clone(),
        });
        self.visited(tab);
        self.broadcast_entry(tab, &key);
    }

    pub fn user_scrolled_entry(&mut self, tab: TabId, key: Key) {
        let Some(t) = self.tabs.get_mut(&tab) else {
            return;
        };
        if t.entry_history.current().as_ref() == Some(&key) {
            return;
        }
        t.entry_history.scrolled_to(key.clone());
        self.broadcast_entry(tab, &key);
    }

    fn broadcast_entry(&mut self, tab: TabId, key: &Key) {
        let Some(link) = self.tabs.get(&tab).and_then(|t| t.link) else {
            return;
        };
        for t in self.tabs.values_mut().filter(|t| {
            t.id != tab && t.link == Some(link) && t.key_schemes.contains(&key.scheme())
        }) {
            if let Some(key) = t.entry_key(key) {
                t.entry_history.scrolled_to(key.clone());
                self.effects.push(Effect::NavigateEntry { tab: t.id, key });
            }
        }
    }

    pub fn entry_history(&mut self, tab: TabId, back: bool) -> Option<Key> {
        let t = self.tabs.get_mut(&tab)?;
        let shown = t.view_unit.clone();
        let (key, unit) = if back {
            t.entry_history.back(shown)?
        } else {
            t.entry_history.forward(shown)?
        };
        if t.follow_history_resource() {
            self.effects.push(Effect::Switched(tab));
            self.show_unit(tab, unit);
            self.visited(tab);
            return Some(key);
        }
        self.effects.push(Effect::NavigateEntry {
            tab,
            key: key.clone(),
        });
        self.show_unit(tab, unit);
        self.visited(tab);
        self.broadcast_entry(tab, &key);
        Some(key)
    }

    /// Back or Forward returned to a history entry whose view showed `unit` (#176).
    fn show_unit(&mut self, tab: TabId, unit: Option<UnitId>) {
        if let Some(unit) = unit {
            self.effects.push(Effect::ShowUnit { tab, unit });
        }
    }

    /// A compatible entry Key from another member, focused member first.
    pub fn set_entry_location(&self, tab: TabId) -> Option<Key> {
        let t = self.tabs.get(&tab)?;
        let link = t.link?;
        let mut members: Vec<_> = self
            .tabs
            .values()
            .filter(|m| m.id != tab && m.link == Some(link))
            .collect();
        members.sort_by_key(|m| Some(m.id) != self.focused);
        members.into_iter().find_map(|m| {
            let key = m.entry_history.current()?;
            if !t.key_schemes.contains(&key.scheme()) {
                return None;
            }
            t.entry_key(&key)
        })
    }

    /// Navigate to a Standard Verse, translating each Link-set member independently.
    /// Never round-trip through the opener: Absent and merged Verses lose information.
    pub fn navigate_standard(&mut self, tab: TabId, standard: Verse) {
        let Some(t) = self.tabs.get_mut(&tab) else {
            return;
        };
        if t.follows_passages()
            && let Some(native) =
                to_native(t.verse_map.as_deref(), t.versification.as_deref(), standard)
        {
            t.visit(native, None);
            t.moved_to(native);
            self.effects.push(Effect::Navigate { tab, verse: native });
            self.visited(tab);
        }
        // A Lexicon can open a Scripture reference without following Passages itself.
        // Even if the opener has no preceding Verse, other members may contain the target.
        self.broadcast_standard(tab, standard, true);
    }

    /// The view moved without the user scrolling it (a follow settling, a resize): update
    /// the location without a history entry or moving the Link set.
    pub fn scrolled(&mut self, tab: TabId, verse: Verse) {
        if let Some(t) = self.tabs.get_mut(&tab) {
            t.history.scrolled_to(verse);
        }
    }

    /// The user scrolled a Tab to a new location: its Link set follows continuously.
    pub fn user_scrolled(&mut self, tab: TabId, verse: Verse) {
        let Some(t) = self.tabs.get_mut(&tab) else {
            return;
        };
        if t.history.current() == Some(verse) {
            return;
        }
        t.history.scrolled_to(verse);
        self.broadcast(tab, verse, false);
    }

    /// The UI reports the text unit at the top of a Tab's view ([`Tab::view_unit`]).
    pub fn view_shows(&mut self, tab: TabId, unit: Option<UnitId>) {
        if let Some(t) = self.tabs.get_mut(&tab) {
            t.view_unit = unit;
        }
    }

    pub fn back(&mut self, tab: TabId) -> Option<Verse> {
        let t = self.tabs.get_mut(&tab)?;
        let (v, unit) = t.history.back(t.view_unit.clone())?;
        Some(self.stepped(tab, v, unit))
    }

    pub fn forward(&mut self, tab: TabId) -> Option<Verse> {
        let t = self.tabs.get_mut(&tab)?;
        let (v, unit) = t.history.forward(t.view_unit.clone())?;
        Some(self.stepped(tab, v, unit))
    }

    /// Back or Forward moved a Tab's history to `v`, where its view showed `unit`. Across a
    /// switch between parallel resources the Tab shows the other Resource again, at the same
    /// place: its Link set stays.
    fn stepped(&mut self, tab: TabId, v: Verse, unit: Option<UnitId>) -> Verse {
        let Some(t) = self.tabs.get_mut(&tab) else {
            return v;
        };
        if t.follow_history_resource() {
            self.effects.push(Effect::Switched(tab));
            self.show_unit(tab, unit);
            self.visited(tab);
            return v;
        }
        t.moved_to(v);
        self.effects.push(Effect::Navigate { tab, verse: v });
        self.show_unit(tab, unit);
        self.visited(tab);
        self.broadcast(tab, v, false);
        v
    }

    /// Switch a Resource Tab to the parallel resource `to` (#151), keeping its Link set and
    /// recording the switch in its history. A Bible or Commentary goes to the same Standard
    /// Verse once `to` is loaded ([`Self::arrival`]); a Lexicon or Dictionary to the same Key.
    /// False when the Tab is not a Resource Tab or already shows `to`.
    pub fn switch_resource(&mut self, tab: TabId, to: String) -> bool {
        let Some(t) = self.tabs.get_mut(&tab) else {
            return false;
        };
        let TabKind::Resource(from) = &t.kind else {
            return false;
        };
        if *from == to {
            return false;
        }
        let from = from.clone();
        let mut arriving_at = None;
        if t.keeps_entries() {
            let key = t.entry_history.current();
            let shown = t.view_unit.clone();
            t.entry_history
                .switch_resource(&from, to.clone(), key, shown);
        } else {
            // An Unmapped verse has no Standard place: the new Resource opens where it would.
            arriving_at =
                t.history
                    .current()
                    .and_then(|v| match to_standard(t.verse_map.as_deref(), v) {
                        ToStandard::Verse(s) => Some(s),
                        ToStandard::Unmapped => None,
                    });
            let shown = t.view_unit.clone();
            t.history.switch_resource(&from, to.clone(), None, shown);
        }
        t.show_resource(to);
        t.arriving_at = arriving_at;
        self.effects.push(Effect::Switched(tab));
        // History records the new Resource where the UI places it, as for a Tab just opened.
        self.visits.push(PendingVisit { tab, at: None });
        true
    }

    /// Where a Tab that switched to a parallel resource goes, now that the UI has loaded that
    /// Resource (its Verse map and sections): the Standard Verse it left, in the new
    /// Resource's numbering, or a Commentary's nearest earlier section. Taken once.
    pub fn arrival(&mut self, tab: TabId) -> Option<Verse> {
        let t = self.tabs.get_mut(&tab)?;
        let standard = t.arriving_at.take()?;
        t.local_passage(to_native(
            t.verse_map.as_deref(),
            t.versification.as_deref(),
            standard,
        )?)
    }

    /// Put a Tab in a Link set (or none). A Tab joining a set moves to the set's location.
    pub fn set_link(&mut self, tab: TabId, link: Option<char>) {
        let Some(t) = self.tabs.get_mut(&tab) else {
            return;
        };
        t.link = link;
        if let Some(v) = self.set_location(tab) {
            self.follow(tab, v, false);
        }
        if let Some(key) = self.set_entry_location(tab) {
            if let Some(t) = self.tabs.get_mut(&tab) {
                t.entry_history.scrolled_to(key.clone());
            }
            self.effects.push(Effect::NavigateEntry { tab, key });
        }
    }

    /// The Standard Verse a Link set is at, read from its members other than `except`
    /// (the focused member first).
    pub(crate) fn standard_location_of(&self, link: char, except: TabId) -> Option<Verse> {
        let mut members: Vec<&Tab> = self
            .tabs
            .values()
            .filter(|t| t.id != except && t.link == Some(link) && t.follows_passages())
            .collect();
        members.sort_by_key(|t| Some(t.id) != self.focused);
        members.into_iter().find_map(|t| {
            let at = t.history.current()?;
            match to_standard(t.verse_map.as_deref(), at) {
                ToStandard::Verse(v) => Some(v),
                ToStandard::Unmapped => None,
            }
        })
    }

    /// Where a member should be to match the rest of its Link set, in its own numbering.
    pub fn set_location(&self, tab: TabId) -> Option<Verse> {
        let t = self.tabs.get(&tab)?;
        if !t.follows_passages() {
            return None;
        }
        let std = self.standard_location_of(t.link?, tab)?;
        t.local_passage(to_native(
            t.verse_map.as_deref(),
            t.versification.as_deref(),
            std,
        )?)
    }

    fn follow(&mut self, tab: TabId, verse: Verse, flash: bool) {
        if let Some(t) = self.tabs.get_mut(&tab) {
            t.history.scrolled_to(verse);
            t.moved_to(verse);
            self.effects.push(Effect::Follow { tab, verse, flash });
        }
    }

    /// Move every other member of `from`'s Link set to `native` (in `from`'s numbering),
    /// translated through the Standard versification. An Unmapped verse moves nobody.
    fn broadcast(&mut self, from: TabId, native: Verse, flash: bool) {
        let Some(origin) = self.tabs.get(&from) else {
            return;
        };
        if !origin.follows_passages() {
            return;
        }
        let ToStandard::Verse(std) = to_standard(origin.verse_map.as_deref(), native) else {
            return;
        };
        self.broadcast_standard(from, std, flash);
    }

    fn broadcast_standard(&mut self, from: TabId, std: Verse, flash: bool) {
        let Some(link) = self.tabs.get(&from).and_then(|t| t.link) else {
            return;
        };
        let moves: Vec<(TabId, Verse)> = self
            .tabs
            .values()
            .filter(|t| t.id != from && t.link == Some(link) && t.follows_passages())
            .filter_map(|t| {
                let to = t.local_passage(to_native(
                    t.verse_map.as_deref(),
                    t.versification.as_deref(),
                    std,
                )?)?;
                (t.history.current() != Some(to) || flash).then_some((t.id, to))
            })
            .collect();
        for (tab, verse) in moves {
            self.follow(tab, verse, flash);
        }
    }

    /// Tabs in a Link set, by id.
    pub fn members(&self, link: char) -> Vec<TabId> {
        self.tabs
            .values()
            .filter(|t| t.link == Some(link))
            .map(|t| t.id)
            .collect()
    }

    pub(crate) fn remembered_slots(&self) -> Vec<(String, u64)> {
        self.remembered
            .iter()
            .filter_map(|(s, p)| Some((serde_json::to_string(s).ok()?, p.0)))
            .collect()
    }

    pub(crate) fn last_panels(&self) -> (Option<u64>, Option<u64>) {
        (
            self.last_reading_panel.map(|p| p.0),
            self.last_other_panel.map(|p| p.0),
        )
    }

    /// Replace everything with a saved snapshot. Locations are set without history; the UI
    /// rebuilds the dock on [`Effect::Rebuild`].
    pub fn restore(&mut self, snap: WorkspaceSnapshot) {
        // Tabs with Visits still queued are kept for History, unless a restored Tab takes
        // their id (its Visits would name the wrong Tab).
        let restored: Vec<TabId> = snap.tabs.iter().map(|t| TabId(t.id)).collect();
        self.visits.retain(|v| !restored.contains(&v.tab));
        for (id, t) in std::mem::take(&mut self.tabs) {
            if self.visits.iter().any(|v| v.tab == id) {
                self.closed_with_visits.insert(id, t);
            }
        }
        self.panels.clear();
        self.remembered.clear();
        self.activity.clear();
        self.focused = None;
        self.restore_closed(snap.recently_closed);
        for t in snap.tabs {
            let id = TabId(t.id);
            self.next_tab = self.next_tab.max(t.id);
            self.tabs.insert(id, Tab::from_snapshot(id, t));
        }
        let mut placed: Vec<(u64, Vec<u64>, Option<u64>)> = snap
            .layout
            .as_ref()
            .map(LayoutNode::panels)
            .unwrap_or_default();
        for w in &snap.windows {
            placed.extend(w.layout.panels());
        }
        self.floating = snap
            .windows
            .into_iter()
            .map(|w| FloatingWindow {
                id: WindowId(w.id),
                arrangement: Some(w.layout),
                placement: w.placement,
            })
            .collect();
        self.next_window = self
            .floating
            .iter()
            .map(|w| w.id.0)
            .fold(self.next_window, u64::max);
        let mut layout = snap.layout;
        // Tabs the layout does not place get a Panel of their own on the right.
        let unplaced: Vec<u64> = self
            .tabs
            .keys()
            .map(|t| t.0)
            .filter(|t| !placed.iter().any(|(_, tabs, _)| tabs.contains(t)))
            .collect();
        for (panel, tabs, active) in &placed {
            self.next_panel = self.next_panel.max(*panel);
            self.panels.insert(
                PanelId(*panel),
                (tabs.iter().map(|t| TabId(*t)).collect(), active.map(TabId)),
            );
        }
        if !unplaced.is_empty() {
            let panel = self.new_panel_id();
            self.panels.insert(
                panel,
                (
                    unplaced.iter().map(|t| TabId(*t)).collect(),
                    unplaced.first().map(|t| TabId(*t)),
                ),
            );
            let leaf = LayoutNode::Panel {
                panel: panel.0,
                tabs: unplaced.clone(),
                active: unplaced.first().copied(),
            };
            layout = Some(match layout {
                None => leaf,
                Some(LayoutNode::Row { mut children }) => {
                    children.push((leaf, 1.0));
                    LayoutNode::Row { children }
                }
                Some(other) => LayoutNode::Row {
                    children: vec![(other, 2.0), (leaf, 1.0)],
                },
            });
        }
        self.arrangement = layout;
        for (slot, panel) in snap.remembered {
            if let Ok(slot) = serde_json::from_str::<Slot>(&slot)
                && self.panels.contains_key(&PanelId(panel))
            {
                self.remembered.insert(slot, PanelId(panel));
            }
        }
        let live = |p: Option<u64>| p.map(PanelId).filter(|p| self.panels.contains_key(p));
        self.last_reading_panel = live(snap.last_reading_panel);
        self.last_other_panel = live(snap.last_other_panel);
        self.focused = snap
            .focused
            .map(TabId)
            .filter(|t| self.tabs.contains_key(t));
        self.effects.push(Effect::Rebuild);
    }

    /// Close everything (before a layout reset).
    pub fn clear(&mut self) {
        let ids: Vec<TabId> = self.tabs.keys().copied().collect();
        for t in ids {
            self.close(t);
        }
        self.remembered.clear();
        self.last_reading_panel = None;
        self.last_other_panel = None;
        self.arrangement = None;
        self.floating.clear();
        self.forget_closed_panels();
    }

    /// The floating window a Tab is in (none: the main window).
    pub fn window_of(&self, tab: TabId) -> Option<WindowId> {
        self.floating
            .iter()
            .find(|w| {
                w.arrangement
                    .as_ref()
                    .is_some_and(|a| a.panels().iter().any(|(_, tabs, _)| tabs.contains(&tab.0)))
            })
            .map(|w| w.id)
    }

    /// Float a Tab in a new window of its own (#152), at `placement` when given. A Tab that
    /// is already alone in a floating window stays there. Returns its window.
    pub fn float(&mut self, tab: TabId, placement: Option<WindowPlacement>) -> Option<WindowId> {
        if !self.tabs.contains_key(&tab) {
            return None;
        }
        if let Some(w) = self.window_of(tab)
            && self.window_tabs(w).len() == 1
        {
            return Some(w);
        }
        self.next_window += 1;
        let window = WindowId(self.next_window);
        let panel = self.move_to_new_panel(tab);
        self.floating.push(FloatingWindow {
            id: window,
            arrangement: Some(LayoutNode::Panel {
                panel: panel.0,
                tabs: vec![tab.0],
                active: Some(tab.0),
            }),
            placement,
        });
        self.effects.push(Effect::Float {
            tab,
            window,
            panel,
            placement,
        });
        self.focus(tab);
        Some(window)
    }

    /// Move a Tab from its floating window into a new Panel at the right of the main window
    /// (#152). Returns false for a Tab already in the main window.
    pub fn dock(&mut self, tab: TabId) -> bool {
        if self.window_of(tab).is_none() {
            return false;
        }
        let panel = self.move_to_new_panel(tab);
        self.arrangement = Some(beside_right(
            self.arrangement.take(),
            LayoutNode::Panel {
                panel: panel.0,
                tabs: vec![tab.0],
                active: Some(tab.0),
            },
        ));
        self.effects.push(Effect::Dock { tab, panel });
        self.focus(tab);
        true
    }

    /// Close a floating window: its Panels move, as they were, to the right of the main
    /// window (#152). Returns false for no such window.
    pub fn dock_window(&mut self, window: WindowId) -> bool {
        let Some(i) = self.floating.iter().position(|w| w.id == window) else {
            return false;
        };
        let w = self.floating.remove(i);
        if let Some(node) = w.arrangement {
            self.arrangement = Some(beside_right(self.arrangement.take(), node));
        }
        self.effects.push(Effect::DockWindow(window));
        true
    }

    /// The Tabs in a floating window.
    pub fn window_tabs(&self, window: WindowId) -> Vec<TabId> {
        self.floating
            .iter()
            .filter(|w| w.id == window)
            .filter_map(|w| w.arrangement.as_ref())
            .flat_map(LayoutNode::panels)
            .flat_map(|(_, tabs, _)| tabs)
            .map(TabId)
            .collect()
    }

    /// Take a Tab out of its Panel (and out of the saved arrangements, a floating window left
    /// empty closing) into a new Panel of its own, which is placed by the caller.
    fn move_to_new_panel(&mut self, tab: TabId) -> PanelId {
        for (tabs, active) in self.panels.values_mut() {
            tabs.retain(|t| *t != tab);
            if *active == Some(tab) {
                *active = tabs.last().copied();
            }
        }
        self.panels.retain(|_, (tabs, _)| !tabs.is_empty());
        let mut others: std::collections::BTreeSet<u64> = self.tabs.keys().map(|t| t.0).collect();
        others.remove(&tab.0);
        self.arrangement = self.arrangement.take().and_then(|a| a.retain_tabs(&others));
        for w in &mut self.floating {
            w.arrangement = w.arrangement.take().and_then(|a| a.retain_tabs(&others));
        }
        self.floating.retain(|w| w.arrangement.is_some());
        let panel = self.new_panel_id();
        self.panels.insert(panel, (vec![tab], Some(tab)));
        panel
    }
}

/// `node` added at the right of an arrangement.
fn beside_right(arrangement: Option<LayoutNode>, node: LayoutNode) -> LayoutNode {
    match arrangement {
        None => node,
        Some(LayoutNode::Row { mut children }) => {
            children.push((node, 1.0));
            LayoutNode::Row { children }
        }
        Some(other) => LayoutNode::Row {
            children: vec![(other, 2.0), (node, 1.0)],
        },
    }
}
