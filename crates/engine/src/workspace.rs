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

/// The Tools of v0.1 plus Cross-references.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Tool {
    Search,
    TextComparison,
    Notes,
    WordInfo,
    CrossReferences,
}

/// The Guides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Guide {
    PassageGuide,
    WordStudy,
}

impl Tool {
    pub fn title(self) -> &'static str {
        match self {
            Tool::Search => "Search",
            Tool::TextComparison => "Text Comparison",
            Tool::Notes => "Notes",
            Tool::WordInfo => "Word Info",
            Tool::CrossReferences => "Cross-references",
        }
    }
}

impl Guide {
    pub fn title(self) -> &'static str {
        match self {
            Guide::PassageGuide => "Passage Guide",
            Guide::WordStudy => "Word Study",
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
                | TabKind::Tool(Tool::Notes | Tool::WordInfo)
        )
    }

    /// Resources open in reading Panels; everything else in non-reading Panels.
    pub fn is_reading(&self) -> bool {
        matches!(self, TabKind::Resource(_))
    }

    pub fn header_box(&self) -> HeaderBox {
        match self {
            TabKind::Library | TabKind::Tool(Tool::Notes) => HeaderBox::Filter,
            TabKind::Tool(Tool::WordInfo) => HeaderBox::FollowsHover,
            TabKind::Guide(Guide::WordStudy) => HeaderBox::Word,
            // The query, mode and scope fields are the Tab's body (#59).
            TabKind::Tool(Tool::Search) => HeaderBox::Keyless,
            TabKind::About(_)
            | TabKind::ImportReport(_)
            | TabKind::GetBibles
            | TabKind::StarterLibrary
            | TabKind::Credits => HeaderBox::Keyless,
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
                | TabKind::Tool(Tool::Notes)
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct History<T = Verse> {
    back: Vec<T>,
    current: Option<T>,
    forward: Vec<T>,
}

/// History keeps at most this many entries each way.
pub const HISTORY_LIMIT: usize = 100;

impl<T> Default for History<T> {
    fn default() -> Self {
        Self {
            back: Vec::new(),
            current: None,
            forward: Vec::new(),
        }
    }
}

impl<T: Clone + PartialEq> History<T> {
    pub fn current(&self) -> Option<T> {
        self.current.clone()
    }

    /// A jump: the old location goes on the Back stack and Forward is cleared.
    pub fn visit(&mut self, to: T) {
        if let Some(old) = self.current.clone()
            && old != to
        {
            self.back.push(old);
            if self.back.len() > HISTORY_LIMIT {
                self.back.remove(0);
            }
            self.forward.clear();
        }
        self.current = Some(to);
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

    pub fn back(&mut self) -> Option<T> {
        let to = self.back.pop()?;
        if let Some(cur) = self.current.clone() {
            self.forward.push(cur);
        }
        self.current = Some(to.clone());
        Some(to)
    }

    pub fn forward(&mut self) -> Option<T> {
        let to = self.forward.pop()?;
        if let Some(cur) = self.current.clone() {
            self.back.push(cur);
        }
        self.current = Some(to.clone());
        Some(to)
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
    /// Go to a Standard Verse in a Tab opened just now, once its Verse map is loaded (the
    /// Notes Tool following a Passage anchor, #64): the Tab's Link set follows.
    NavigateStandard {
        tab: TabId,
        verse: Verse,
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
            self.focused
                .and_then(|t| self.tabs.get(&t))
                .filter(|t| !t.key_schemes.is_empty())
                .and_then(|t| t.link)
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
        self.focus(id);
        id
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
        if self.tabs.remove(&tab).is_none() {
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
            self.tabs.remove(&t);
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
        self.effects.push(Effect::NavigateEntry {
            tab,
            key: key.clone(),
        });
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
        let t = self.tabs.get_mut(&tab)?;
        let v = t.history.back()?;
        t.moved_to(v);
        self.effects.push(Effect::Navigate { tab, verse: v });
        self.broadcast(tab, v, false);
        Some(v)
    }

    pub fn forward(&mut self, tab: TabId) -> Option<Verse> {
        let t = self.tabs.get_mut(&tab)?;
        let v = t.history.forward()?;
        t.moved_to(v);
        self.effects.push(Effect::Navigate { tab, verse: v });
        self.broadcast(tab, v, false);
        Some(v)
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
        self.tabs.clear();
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
