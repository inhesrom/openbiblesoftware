//! The Workspace model: Panels, Tabs and Tab kinds, per-Tab history, and the placement rules
//! for opening a Tab (#18 rule 2). The UI shows it (egui_tiles in `ui-egui`) and reports back
//! what the user rearranged; every change made by a command is also queued as an [`Effect`]
//! for the UI to apply.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::resource_navigation::ResourceNavigation;
use obs_model::{Key, KeyScheme};
use obs_scripture::{ListedVersification, Verse, VerseMap};
use serde::{Deserialize, Serialize};

use crate::compare::{Comparison, in_range_chapters};
use crate::link::{ToStandard, to_native, to_standard};
use crate::persist::{LayoutNode, WorkspaceSnapshot};

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
}

/// The Guides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Guide {
    PassageGuide,
    WordStudy,
    /// A hub page for one person, people group or place of a People & places index (#149).
    Factbook,
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
        }
    }
}

impl Guide {
    pub fn title(self) -> &'static str {
        match self {
            Guide::PassageGuide => "Passage Guide",
            Guide::WordStudy => "Word Study",
            Guide::Factbook => "Factbook",
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
                | TabKind::Tool(
                    Tool::Notes
                        | Tool::WordInfo
                        | Tool::Favorites
                        | Tool::History
                        | Tool::ReadingPlans
                )
        )
    }

    /// Resources open in reading Panels; everything else in non-reading Panels.
    pub fn is_reading(&self) -> bool {
        matches!(self, TabKind::Resource(_))
    }

    pub fn header_box(&self) -> HeaderBox {
        match self {
            TabKind::Library | TabKind::Tool(Tool::Notes | Tool::Favorites | Tool::History) => {
                HeaderBox::Filter
            }
            TabKind::Tool(Tool::WordInfo) => HeaderBox::FollowsHover,
            TabKind::Guide(Guide::WordStudy) => HeaderBox::Word,
            TabKind::Guide(Guide::Factbook) => HeaderBox::Name,
            // The query, mode and scope fields are the Tab's body (#59).
            TabKind::Tool(Tool::Search) => HeaderBox::Keyless,
            TabKind::About(_)
            | TabKind::ImportReport(_)
            | TabKind::GetBibles
            | TabKind::StarterLibrary
            | TabKind::Credits
            | TabKind::Tool(Tool::ReadingPlans) => HeaderBox::Keyless,
            _ => HeaderBox::Reference,
        }
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
                | TabKind::Guide(Guide::Factbook)
                | TabKind::Tool(Tool::Notes | Tool::Favorites | Tool::History | Tool::ReadingPlans)
        )
    }

    /// Key schemes a Tab of this kind follows. A Resource's come from its manifest instead.
    pub fn default_key_schemes(&self) -> Vec<KeyScheme> {
        match self {
            TabKind::Guide(Guide::PassageGuide)
            | TabKind::Tool(Tool::TextComparison | Tool::CrossReferences)
            | TabKind::Resource(_) => vec![KeyScheme::Passage],
            TabKind::Guide(Guide::WordStudy) => vec![KeyScheme::StrongsNumber],
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

/// One Back/Forward entry: a location, and the Resource it is in once the Tab has switched
/// between parallel resources.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Stop<T> {
    at: T,
    resource: Option<String>,
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

    /// A jump: the old location goes on the Back stack and Forward is cleared.
    pub fn visit(&mut self, to: T) {
        if let Some(old) = self.current.clone()
            && old != to
        {
            let resource = self.resource.clone();
            self.push_back(Stop { at: old, resource });
            self.forward.clear();
        }
        self.current = Some(to);
    }

    /// The Tab switched from Resource `from` to the parallel resource `to` (#151): the old
    /// location goes on the Back stack (in `from`) and Forward is cleared. The new location is
    /// `at`, or unknown until the new Resource is loaded.
    pub fn switch_resource(&mut self, from: &str, to: String, at: Option<T>) {
        // Every location so far is in `from`, unless the Tab had switched before.
        for stop in self.back.iter_mut().chain(self.forward.iter_mut()) {
            stop.resource.get_or_insert_with(|| from.to_string());
        }
        let resource = Some(self.resource.take().unwrap_or_else(|| from.to_string()));
        if let Some(old) = self.current.take() {
            self.push_back(Stop { at: old, resource });
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

    /// Back one entry; [`Self::resource`] then tells which Resource it is in.
    pub fn back(&mut self) -> Option<T> {
        let to = self.back.pop()?;
        if let Some(cur) = self.current.take() {
            let resource = self.resource.clone();
            self.forward.push(Stop { at: cur, resource });
        }
        self.resource = to.resource;
        self.current = Some(to.at.clone());
        Some(to.at)
    }

    /// Forward one entry; [`Self::resource`] then tells which Resource it is in.
    pub fn forward(&mut self) -> Option<T> {
        let to = self.forward.pop()?;
        if let Some(cur) = self.current.take() {
            let resource = self.resource.clone();
            self.push_back(Stop { at: cur, resource });
        }
        self.resource = to.resource;
        self.current = Some(to.at.clone());
        Some(to.at)
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
}

impl Tab {
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

    fn follows_passages(&self) -> bool {
        self.key_schemes.contains(&KeyScheme::Passage)
    }

    /// Whether the Tab's location is an entry Key (Lexicon, Dictionary, Word Study, Factbook)
    /// kept in `entry_history`, rather than a Verse kept in `history`.
    pub fn keeps_entries(&self) -> bool {
        matches!(
            self.header_box(),
            HeaderBox::Lexicon | HeaderBox::Headword | HeaderBox::Word | HeaderBox::Name
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
        self.versification = None;
        self.verse_map = None;
        self.arriving_at = None;
    }

    /// A jump, Back/Forward or follow moved the Tab: a Text Comparison range is cleared once
    /// the Tab leaves the range's chapters (#18 rule 13).
    fn moved_to(&mut self, verse: Verse) {
        if let Some(r) = self.comparison.range
            && !in_range_chapters(r, verse)
        {
            self.comparison.range = None;
        }
    }
}

/// Where a new Tab goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Into an existing Panel.
    Panel(PanelId),
    /// Into a new Panel split off to the right of `beside` (or of the whole Workspace).
    NewPanel {
        panel: PanelId,
        beside: Option<PanelId>,
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
    /// Tabs that were opened or jumped since the last [`Workspace::take_visits`], oldest
    /// first, for History (#155).
    visits: Vec<PendingVisit>,
    /// Tabs closed while History still had Visits of theirs to record, kept until it has.
    closed_with_visits: BTreeMap<TabId, Tab>,
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
            return Placement::NewPanel { panel, beside };
        }
        let chosen = req
            .panel
            .filter(live)
            .or_else(|| self.remembered.get(&kind.slot()).copied().filter(live))
            .or_else(|| {
                if kind.is_reading() {
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
                Placement::NewPanel { panel, beside }
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
            if kind.is_reading() {
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
            t.history.visit(local);
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
                t.history.visit(start);
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
                t.entry_history.visit(k.clone());
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
        t.entry_history.visit(key.clone());
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
        let key = if back {
            t.entry_history.back()?
        } else {
            t.entry_history.forward()?
        };
        if t.follow_history_resource() {
            self.effects.push(Effect::Switched(tab));
            self.visited(tab);
            return Some(key);
        }
        self.effects.push(Effect::NavigateEntry {
            tab,
            key: key.clone(),
        });
        self.visited(tab);
        self.broadcast_entry(tab, &key);
        Some(key)
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
            t.history.visit(native);
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

    pub fn back(&mut self, tab: TabId) -> Option<Verse> {
        let v = self.tabs.get_mut(&tab)?.history.back()?;
        Some(self.stepped(tab, v))
    }

    pub fn forward(&mut self, tab: TabId) -> Option<Verse> {
        let v = self.tabs.get_mut(&tab)?.history.forward()?;
        Some(self.stepped(tab, v))
    }

    /// Back or Forward moved a Tab's history to `v`. Across a switch between parallel
    /// resources the Tab shows the other Resource again, at the same place: its Link set stays.
    fn stepped(&mut self, tab: TabId, v: Verse) -> Verse {
        let Some(t) = self.tabs.get_mut(&tab) else {
            return v;
        };
        if t.follow_history_resource() {
            self.effects.push(Effect::Switched(tab));
            self.visited(tab);
            return v;
        }
        t.moved_to(v);
        self.effects.push(Effect::Navigate { tab, verse: v });
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
            t.entry_history.switch_resource(&from, to.clone(), key);
        } else {
            // An Unmapped verse has no Standard place: the new Resource opens where it would.
            arriving_at =
                t.history
                    .current()
                    .and_then(|v| match to_standard(t.verse_map.as_deref(), v) {
                        ToStandard::Verse(s) => Some(s),
                        ToStandard::Unmapped => None,
                    });
            t.history.switch_resource(&from, to.clone(), None);
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
        for t in snap.tabs {
            let mut history = History::default();
            if let Some(v) = t.location {
                history.scrolled_to(v);
            }
            let key_schemes = t.kind.default_key_schemes();
            let id = TabId(t.id);
            self.next_tab = self.next_tab.max(t.id);
            self.tabs.insert(
                id,
                Tab {
                    id,
                    kind: t.kind,
                    title: t.title,
                    history,
                    entry_history: {
                        let mut h = History::default();
                        if let Some(k) = t.entry_location {
                            h.scrolled_to(k);
                        }
                        h
                    },
                    navigation: None,
                    versification: None,
                    verse_map: None,
                    key_schemes,
                    link: t.link,
                    comparison: Comparison::default(),
                    search: Default::default(),
                    arriving_at: None,
                },
            );
        }
        let placed: Vec<(u64, Vec<u64>, Option<u64>)> = snap
            .layout
            .as_ref()
            .map(LayoutNode::panels)
            .unwrap_or_default();
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
    }
}
