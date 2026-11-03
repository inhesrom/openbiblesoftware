//! The Workspace model: Panels, Tabs and Tab kinds, per-Tab history, and the placement rules
//! for opening a Tab (#18 rule 2). The UI shows it (egui_tiles in `ui-egui`) and reports back
//! what the user rearranged; every change made by a command is also queued as an [`Effect`]
//! for the UI to apply.

use std::collections::BTreeMap;
use std::sync::Arc;

use obs_model::KeyScheme;
use obs_scripture::{ListedVersification, Verse, VerseMap};

use crate::link::{ToStandard, to_native, to_standard};

/// A Tab, stable for the life of the Workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TabId(pub u64);

/// A Panel: a dockable region holding one or more Tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PanelId(pub u64);

/// The Tools of v0.1 plus Cross-references.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tool {
    Search,
    TextComparison,
    Notes,
    WordInfo,
    CrossReferences,
}

/// The Guides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabKind {
    Library,
    /// A Resource from the library, by its resource id.
    Resource(String),
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
    /// A filter field, and no link chip (Library, Notes).
    Filter,
    /// "Follows hover" (Word Info).
    FollowsHover,
}

/// Which remembered-Panel slot a Tab kind uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Slot {
    Library,
    Resource,
    Guide(Guide),
    Tool(Tool),
}

impl TabKind {
    /// Library, Notes and Word Info have at most one Tab each.
    pub fn single_instance(&self) -> bool {
        matches!(
            self,
            TabKind::Library | TabKind::Tool(Tool::Notes | Tool::WordInfo)
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
            TabKind::Tool(Tool::Search) => HeaderBox::Filter,
            _ => HeaderBox::Reference,
        }
    }

    /// Whether the header has a link chip.
    pub fn has_link_chip(&self) -> bool {
        !matches!(self, TabKind::Library | TabKind::Tool(Tool::Notes))
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
            TabKind::Guide(g) => Slot::Guide(*g),
            TabKind::Tool(t) => Slot::Tool(*t),
        }
    }
}

/// A Tab's Back/Forward history of locations, in the Tab's own numbering.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct History {
    back: Vec<Verse>,
    current: Option<Verse>,
    forward: Vec<Verse>,
}

/// History keeps at most this many entries each way.
pub const HISTORY_LIMIT: usize = 100;

impl History {
    pub fn current(&self) -> Option<Verse> {
        self.current
    }

    /// A jump: the old location goes on the Back stack and Forward is cleared.
    pub fn visit(&mut self, to: Verse) {
        if let Some(old) = self.current
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
    pub fn scrolled_to(&mut self, at: Verse) {
        self.current = Some(at);
    }

    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    pub fn back(&mut self) -> Option<Verse> {
        let to = self.back.pop()?;
        if let Some(cur) = self.current {
            self.forward.push(cur);
        }
        self.current = Some(to);
        Some(to)
    }

    pub fn forward(&mut self) -> Option<Verse> {
        let to = self.forward.pop()?;
        if let Some(cur) = self.current {
            self.back.push(cur);
        }
        self.current = Some(to);
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
    /// The Resource's own numbering, for reading typed references (ADR 0002 rule 10).
    pub versification: Option<Arc<ListedVersification>>,
    /// A Bible's Verse map to the Standard versification (none: it numbers like the Standard).
    pub verse_map: Option<Arc<VerseMap>>,
    /// The Key schemes this Tab follows in a Link set.
    pub key_schemes: Vec<KeyScheme>,
    /// The Link set this Tab belongs to, if any.
    pub link: Option<char>,
}

impl Tab {
    fn follows_passages(&self) -> bool {
        self.key_schemes.contains(&KeyScheme::Passage)
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
    /// A Link-set member follows another; `flash` marks the target after an explicit jump.
    Follow {
        tab: TabId,
        verse: Verse,
        flash: bool,
    },
    /// Rebuild the default arrangement.
    ResetLayout,
    /// A short message for the user (a toast).
    Message(String),
    /// A titled text window (About, Credits).
    Show {
        title: String,
        body: String,
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
                versification: None,
                verse_map: None,
                key_schemes,
                link,
            },
        );
        self.effects.push(Effect::Opened { tab: id, placement });
        self.focus(id);
        id
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
        if let Some(t) = self.tabs.get_mut(&tab) {
            t.history.visit(verse);
            self.effects.push(Effect::Navigate { tab, verse });
            self.broadcast(tab, verse, true);
        }
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
        self.effects.push(Effect::Navigate { tab, verse: v });
        self.broadcast(tab, v, false);
        Some(v)
    }

    pub fn forward(&mut self, tab: TabId) -> Option<Verse> {
        let v = self.tabs.get_mut(&tab)?.history.forward()?;
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
    }

    /// The Standard Verse a Link set is at, read from its members other than `except`
    /// (the focused member first).
    fn standard_location_of(&self, link: char, except: TabId) -> Option<Verse> {
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
        to_native(t.verse_map.as_deref(), t.versification.as_deref(), std)
    }

    fn follow(&mut self, tab: TabId, verse: Verse, flash: bool) {
        if let Some(t) = self.tabs.get_mut(&tab) {
            t.history.scrolled_to(verse);
            self.effects.push(Effect::Follow { tab, verse, flash });
        }
    }

    /// Move every other member of `from`'s Link set to `native` (in `from`'s numbering),
    /// translated through the Standard versification. An Unmapped verse moves nobody.
    fn broadcast(&mut self, from: TabId, native: Verse, flash: bool) {
        let Some(origin) = self.tabs.get(&from) else {
            return;
        };
        let Some(link) = origin.link else {
            return;
        };
        if !origin.follows_passages() {
            return;
        }
        let ToStandard::Verse(std) = to_standard(origin.verse_map.as_deref(), native) else {
            return;
        };
        let moves: Vec<(TabId, Verse)> = self
            .tabs
            .values()
            .filter(|t| t.id != from && t.link == Some(link) && t.follows_passages())
            .filter_map(|t| {
                let to = to_native(t.verse_map.as_deref(), t.versification.as_deref(), std)?;
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
