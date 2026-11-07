//! The toolbar's Launchers (#140): Library, Guides, Tools and Layouts, each an icon-and-label
//! button that drops down what it launches. The engine decides what each drop-down lists and
//! what choosing an item does; the UI (`crates/ui-egui/src/shell/toolbar.rs`) draws it.
//!
//! - **Library:** a search over title, abbreviation, Resource type and language
//!   ([`Session::library_launcher`]); with nothing typed, the Recent Resources and the top of
//!   each type's Preferred order. Choosing one runs `toolbar.open_resource`.
//! - **Guides** and **Tools:** the commands of the menu of the same title ([`Launcher::menu`]).
//! - **Layouts:** the saved Layouts and the Built-in Layouts as cards, each with a thumbnail
//!   of its Panel tiling read from the stored arrangement ([`Session::layout_cards`]).
//!   A Built-in Layout is made from the library's Preferred order when it is loaded
//!   (`layout.builtin`).
//!
//! Each Launcher opens from the keyboard too: `toolbar.library` (Alt+L), `toolbar.guides`
//! (Alt+G), `toolbar.tools` (Alt+T) and `toolbar.layouts` (Alt+Y).

use obs_model::{KeyScheme, ResourceType};
use obs_scripture::Verse;
use serde_json::{Value, json};

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::persist::{LayoutNode, SNAPSHOT_VERSION, TabSnapshot, WorkspaceSnapshot};
use crate::session::Session;
use crate::workspace::{Effect, Guide, OpenRequest, PanelId, TabId, TabKind, Tool};
use crate::workspace_commands::{MENUS, Menu};

/// A toolbar button that drops down what it launches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Launcher {
    Library,
    Guides,
    Tools,
    Layouts,
}

impl Launcher {
    /// Left to right on the toolbar.
    pub const ALL: [Launcher; 4] = [
        Launcher::Library,
        Launcher::Guides,
        Launcher::Tools,
        Launcher::Layouts,
    ];

    /// Its label, which is also its accessible name and its menu's title.
    pub fn title(self) -> &'static str {
        match self {
            Launcher::Library => "Library",
            Launcher::Guides => "Guides",
            Launcher::Tools => "Tools",
            Launcher::Layouts => "Layouts",
        }
    }

    /// Its id in commands and the saved toolbar preferences: "library", "guides" …
    pub fn id(self) -> &'static str {
        match self {
            Launcher::Library => "library",
            Launcher::Guides => "guides",
            Launcher::Tools => "tools",
            Launcher::Layouts => "layouts",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.id() == id)
    }

    /// The command that opens it (its shortcut is that command's).
    pub fn command(self) -> &'static str {
        match self {
            Launcher::Library => "toolbar.library",
            Launcher::Guides => "toolbar.guides",
            Launcher::Tools => "toolbar.tools",
            Launcher::Layouts => "toolbar.layouts",
        }
    }

    /// The menu whose items it offers.
    pub fn menu(self) -> &'static Menu {
        let title = self.title();
        MENUS.iter().find(|m| m.title == title).unwrap_or(&MENUS[0])
    }
}

/// How many Recent Resources the Library drop-down lists.
pub const RECENT_ROWS: usize = 4;
/// How many Resources of each type the Library drop-down lists with nothing typed.
pub const PREFERRED_ROWS: usize = 3;
/// How many matches the Library drop-down lists for a search.
pub const MATCH_ROWS: usize = 12;

/// One Resource in the Library drop-down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryLauncherRow {
    pub resource_id: String,
    pub abbreviation: String,
    pub title: String,
    pub resource_type: ResourceType,
    /// The language's English name ("Greek").
    pub language: String,
}

impl LibraryLauncherRow {
    /// Its type and language: "Bible · Greek".
    pub fn detail(&self) -> String {
        format!(
            "{} · {}",
            crate::command_box::type_name(self.resource_type),
            self.language
        )
    }
}

/// A group of rows in the Library drop-down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryLauncherSection {
    Recent,
    /// The top of one type's Preferred order.
    Preferred(ResourceType),
    /// What the search found, best first.
    Matches,
}

impl LibraryLauncherSection {
    pub fn heading(&self) -> &'static str {
        match self {
            LibraryLauncherSection::Recent => "Recent",
            LibraryLauncherSection::Preferred(ty) => crate::library_search::type_plural(*ty),
            LibraryLauncherSection::Matches => "Best matches",
        }
    }
}

/// The Resource types in the order the Library lists them.
pub(crate) const TYPE_ORDER: [ResourceType; 6] = [
    ResourceType::Bible,
    ResourceType::Commentary,
    ResourceType::Lexicon,
    ResourceType::Dictionary,
    ResourceType::CrossReferenceSet,
    ResourceType::PeopleAndPlaces,
];

/// A Resource type as one word or phrase a search can name: "bible", "commentary" …
fn type_words(ty: ResourceType) -> &'static str {
    match ty {
        ResourceType::Bible => "bible bibles",
        ResourceType::Commentary => "commentary commentaries",
        ResourceType::Lexicon => "lexicon lexicons",
        ResourceType::Dictionary => "dictionary dictionaries",
        ResourceType::CrossReferenceSet => "cross-reference cross reference set sets",
        ResourceType::PeopleAndPlaces => "people places index",
    }
}

/// A saved or Built-in Layout as the Layouts drop-down shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutCard {
    pub name: String,
    pub builtin: bool,
    /// Its Panel tiling (none for a Layout this version cannot read).
    pub thumbnail: Option<LayoutThumbnail>,
    /// Why a Built-in Layout cannot be loaded with this library ("Needs a Commentary").
    pub unavailable: Option<&'static str>,
}

/// A Layout's Panel tiling, for its thumbnail: rows and columns with relative shares, and
/// for each Panel the title of its active Tab and that Tab's Link set.
#[derive(Debug, Clone, PartialEq)]
pub enum LayoutThumbnail {
    Panel {
        title: String,
        link: Option<char>,
        tabs: usize,
    },
    Row(Vec<(LayoutThumbnail, f32)>),
    Column(Vec<(LayoutThumbnail, f32)>),
}

impl LayoutThumbnail {
    /// The thumbnail of a saved Workspace's arrangement.
    pub fn of(snap: &WorkspaceSnapshot) -> Option<Self> {
        snap.layout.as_ref().map(|l| Self::of_node(snap, l))
    }

    fn of_node(snap: &WorkspaceSnapshot, node: &LayoutNode) -> Self {
        match node {
            LayoutNode::Panel { tabs, active, .. } => {
                let shown = active.or(tabs.first().copied());
                let tab = snap.tabs.iter().find(|t| Some(t.id) == shown);
                LayoutThumbnail::Panel {
                    title: tab.map(|t| t.title.clone()).unwrap_or_default(),
                    link: tab.and_then(|t| t.link),
                    tabs: tabs.len(),
                }
            }
            LayoutNode::Row { children } => LayoutThumbnail::Row(
                children
                    .iter()
                    .map(|(c, s)| (Self::of_node(snap, c), *s))
                    .collect(),
            ),
            LayoutNode::Column { children } => LayoutThumbnail::Column(
                children
                    .iter()
                    .map(|(c, s)| (Self::of_node(snap, c), *s))
                    .collect(),
            ),
        }
    }

    /// Every Panel's title, left to right and top to bottom.
    pub fn titles(&self) -> Vec<&str> {
        match self {
            LayoutThumbnail::Panel { title, .. } => vec![title.as_str()],
            LayoutThumbnail::Row(c) | LayoutThumbnail::Column(c) => {
                c.iter().flat_map(|(t, _)| t.titles()).collect()
            }
        }
    }
}

/// The Built-in Layouts, in the order the Layouts drop-down shows them.
pub const BUILTIN_LAYOUTS: [&str; 4] = [
    "Bible + Commentary",
    "Word study desk",
    "Parallel Bibles",
    "Reading only",
];

/// One Tab of a Built-in Layout before it has an id.
struct Planned {
    kind: TabKind,
    link: Option<char>,
}

/// A Built-in Layout's arrangement, with Panels as lists of [`Planned`] Tabs.
enum Plan {
    Panel(Vec<Planned>),
    Row(Vec<(Plan, f32)>),
    Column(Vec<(Plan, f32)>),
}

fn resource(id: &str) -> Planned {
    Planned {
        kind: TabKind::Resource(id.to_string()),
        link: Some('A'),
    }
}

impl Session {
    /// The Library drop-down's rows for `query`. With nothing typed: the Recent Resources,
    /// then the top of each type's Preferred order. Otherwise every Resource whose title,
    /// abbreviation, type or language each word of `query` starts (or whose abbreviation or
    /// title contains it), best first: an exact abbreviation, then abbreviations and titles
    /// it starts, then the rest, each in type and Preferred order.
    pub fn library_launcher(
        &self,
        query: &str,
    ) -> Vec<(LibraryLauncherSection, Vec<LibraryLauncherRow>)> {
        let row = |e: &crate::LibraryEntry| {
            let m = &e.manifest;
            LibraryLauncherRow {
                resource_id: m.resource_id.clone(),
                abbreviation: m.abbreviation.clone(),
                title: m.title.clone(),
                resource_type: m.resource_type,
                language: crate::language_name(&m.language).to_string(),
            }
        };
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            let mut out = Vec::new();
            let recent: Vec<LibraryLauncherRow> = self
                .recent_resources()
                .iter()
                .filter_map(|id| self.library.get(id))
                .take(RECENT_ROWS)
                .map(row)
                .collect();
            if !recent.is_empty() {
                out.push((LibraryLauncherSection::Recent, recent));
            }
            for ty in TYPE_ORDER {
                let rows: Vec<LibraryLauncherRow> = self
                    .library
                    .of_type(ty)
                    .take(PREFERRED_ROWS)
                    .map(row)
                    .collect();
                if !rows.is_empty() {
                    out.push((LibraryLauncherSection::Preferred(ty), rows));
                }
            }
            return out;
        }
        let words: Vec<&str> = needle.split_whitespace().collect();
        let mut found: Vec<(u8, usize, usize, LibraryLauncherRow)> = Vec::new();
        for ty in TYPE_ORDER {
            for (rank, e) in self.library.of_type(ty).enumerate() {
                let m = &e.manifest;
                let abbreviation = m.abbreviation.to_lowercase();
                let title = m.title.to_lowercase();
                let language = format!(
                    "{} {}",
                    crate::language_name(&m.language).to_lowercase(),
                    m.language.to_lowercase()
                );
                let kind = type_words(ty);
                let matches = |w: &str| {
                    abbreviation.contains(w)
                        || title.contains(w)
                        || starts_a_word(&language, w)
                        || starts_a_word(kind, w)
                };
                if !words.iter().all(|w| matches(w)) {
                    continue;
                }
                let score = if abbreviation == needle || title == needle {
                    3
                } else if abbreviation.starts_with(&needle) || starts_a_word(&title, &needle) {
                    2
                } else if words
                    .iter()
                    .any(|w| abbreviation.contains(w) || title.contains(w))
                {
                    1
                } else {
                    0
                };
                let type_rank = TYPE_ORDER.iter().position(|t| *t == ty).unwrap_or(0);
                found.push((score, type_rank, rank, row(e)));
            }
        }
        found.sort_by_key(|(score, ty, rank, _)| (std::cmp::Reverse(*score), *ty, *rank));
        let rows: Vec<LibraryLauncherRow> = found
            .into_iter()
            .take(MATCH_ROWS)
            .map(|(.., r)| r)
            .collect();
        if rows.is_empty() {
            Vec::new()
        } else {
            vec![(LibraryLauncherSection::Matches, rows)]
        }
    }

    /// Resources opened lately, newest first: those of History's Visits, then those of open
    /// Tabs (most recently active first) that History does not list.
    pub fn recent_resources(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for v in self.history() {
            if let TabKind::Resource(id) = &v.location.kind
                && !out.contains(id)
            {
                out.push(id.clone());
            }
        }
        for t in self.workspace.by_activity() {
            if let Some(TabKind::Resource(id)) = self.workspace.tab(t).map(|t| &t.kind)
                && !out.contains(id)
            {
                out.push(id.clone());
            }
        }
        out.retain(|id| self.library.get(id).is_some());
        out
    }

    /// The Layouts drop-down's cards: the saved Layouts (by name), then the Built-in ones.
    pub fn layout_cards(&self) -> Vec<LayoutCard> {
        let mut out = Vec::new();
        if let Some(user) = &self.user {
            for name in user.layouts().unwrap_or_default() {
                let thumbnail = user
                    .layout(&name)
                    .ok()
                    .flatten()
                    .and_then(|json| WorkspaceSnapshot::from_json(&json))
                    .and_then(|snap| LayoutThumbnail::of(&snap));
                out.push(LayoutCard {
                    name,
                    builtin: false,
                    thumbnail,
                    unavailable: None,
                });
            }
        }
        for name in BUILTIN_LAYOUTS {
            let (thumbnail, unavailable) = match self.builtin_snapshot(name) {
                Ok(snap) => (LayoutThumbnail::of(&snap), None),
                Err(why) => (None, Some(why)),
            };
            out.push(LayoutCard {
                name: name.to_string(),
                builtin: true,
                thumbnail,
                unavailable,
            });
        }
        out
    }

    /// What a Built-in Layout holds with this library, or why it cannot be made.
    fn builtin_plan(&self, name: &str) -> Result<Plan, &'static str> {
        let bibles: Vec<&str> = self
            .library
            .of_type(ResourceType::Bible)
            .map(|e| e.manifest.resource_id.as_str())
            .collect();
        let Some(first) = bibles.first() else {
            return Err("Needs a Bible in the library");
        };
        match name {
            "Bible + Commentary" => {
                let commentary = self
                    .library
                    .preferred(ResourceType::Commentary)
                    .ok_or("Needs a Commentary in the library")?;
                Ok(Plan::Row(vec![
                    (Plan::Panel(vec![resource(first)]), 1.0),
                    (
                        Plan::Panel(vec![resource(&commentary.manifest.resource_id)]),
                        1.0,
                    ),
                ]))
            }
            "Word study desk" => Ok(Plan::Row(vec![
                (Plan::Panel(vec![resource(first)]), 3.0),
                (
                    Plan::Column(vec![
                        (
                            Plan::Panel(vec![Planned {
                                kind: TabKind::Guide(Guide::WordStudy),
                                link: None,
                            }]),
                            3.0,
                        ),
                        (
                            Plan::Panel(vec![Planned {
                                kind: TabKind::Tool(Tool::WordInfo),
                                link: None,
                            }]),
                            2.0,
                        ),
                    ]),
                    2.0,
                ),
            ])),
            "Parallel Bibles" => {
                let [a, b, ..] = bibles.as_slice() else {
                    return Err("Needs two Bibles in the library");
                };
                Ok(Plan::Row(vec![
                    (Plan::Panel(vec![resource(a)]), 1.0),
                    (Plan::Panel(vec![resource(b)]), 1.0),
                ]))
            }
            "Reading only" => Ok(Plan::Panel(vec![resource(first)])),
            _ => Err("No such Built-in Layout"),
        }
    }

    /// A Built-in Layout as a Workspace snapshot, its Tab ids following the Workspace's.
    fn builtin_snapshot(&self, name: &str) -> Result<WorkspaceSnapshot, &'static str> {
        let plan = self.builtin_plan(name)?;
        let mut snap = WorkspaceSnapshot {
            version: SNAPSHOT_VERSION,
            tabs: Vec::new(),
            layout: None,
            focused: None,
            remembered: Vec::new(),
            last_reading_panel: None,
            last_other_panel: None,
            windows: Vec::new(),
            recently_closed: Default::default(),
        };
        let mut next = (self.workspace.next_tab_id(), 0u64);
        snap.layout = Some(self.place(&plan, &mut snap, &mut next));
        Ok(snap)
    }

    fn place(
        &self,
        plan: &Plan,
        snap: &mut WorkspaceSnapshot,
        next: &mut (u64, u64),
    ) -> LayoutNode {
        match plan {
            Plan::Panel(tabs) => {
                next.1 += 1;
                let panel = next.1;
                let mut ids = Vec::new();
                for t in tabs {
                    let id = next.0;
                    next.0 += 1;
                    ids.push(id);
                    snap.tabs.push(TabSnapshot {
                        id,
                        kind: t.kind.clone(),
                        title: crate::workspace_commands::title_of(self, &t.kind),
                        location: None,
                        entry_location: None,
                        link: t.link,
                        interlinear: Default::default(),
                        visual_filters: Default::default(),
                        contents: false,
                        scroll: None,
                        footnote_markers_hidden: false,
                        search: None,
                        comparison: Default::default(),
                    });
                    if t.kind.is_reading() {
                        snap.focused = snap.focused.or(Some(id));
                    }
                    if t.kind.opens_in_reading_panel() {
                        snap.last_reading_panel = snap.last_reading_panel.or(Some(panel));
                    } else {
                        snap.last_other_panel = snap.last_other_panel.or(Some(panel));
                    }
                }
                LayoutNode::Panel {
                    panel,
                    active: ids.first().copied(),
                    tabs: ids,
                }
            }
            Plan::Row(children) => LayoutNode::Row {
                children: children
                    .iter()
                    .map(|(c, s)| (self.place(c, snap, next), *s))
                    .collect(),
            },
            Plan::Column(children) => LayoutNode::Column {
                children: children
                    .iter()
                    .map(|(c, s)| (self.place(c, snap, next), *s))
                    .collect(),
            },
        }
    }

    /// The Standard Verse the focused Tab (else the most recent Bible or Passage-keyed Tab)
    /// shows, which a Built-in Layout opens at.
    fn current_standard_verse(&self) -> Option<Verse> {
        let ws = &self.workspace;
        let passage_keyed = |t: &crate::Tab| t.key_schemes.contains(&KeyScheme::Passage);
        ws.focused()
            .and_then(|f| ws.tab(f))
            .filter(|t| passage_keyed(t))
            .or_else(|| ws.most_recent(passage_keyed).and_then(|t| ws.tab(t)))
            .and_then(|t| {
                match crate::link::to_standard(t.verse_map.as_deref(), t.history.current()?) {
                    crate::link::ToStandard::Verse(v) => Some(v),
                    crate::link::ToStandard::Unmapped => None,
                }
            })
    }
}

/// Whether some word of `hay` starts with `word` (`hay` lowercase).
fn starts_a_word(hay: &str, word: &str) -> bool {
    hay.starts_with(word)
        || hay
            .split(|c: char| !c.is_alphanumeric())
            .any(|w| w.starts_with(word))
}

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

/// `toolbar.open_resource`: the Library drop-down's click, Enter, Ctrl/Cmd-click and drop.
fn open_resource(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "toolbar.open_resource";
    let id = p.str("id").unwrap_or_default().to_string();
    if s.library.get(&id).is_none() {
        return Err(failed(ID, format!("{id} is not in the library")));
    }
    let kind = TabKind::Resource(id.clone());
    let panel = p
        .i64("panel")
        .and_then(|n| u64::try_from(n).ok())
        .map(PanelId);
    let beside = p
        .i64("beside")
        .and_then(|n| u64::try_from(n).ok())
        .map(PanelId);
    if let Some(beside) = beside {
        // Split off beside the Panel it was dropped on: that Panel's Tab is the opener.
        if let Some(t) = s.workspace.active_in(beside) {
            s.workspace.focus(t);
        }
        let tab = crate::workspace_commands::open_tab(
            s,
            kind,
            OpenRequest {
                panel: None,
                split: true,
            },
        );
        return Ok(json!({ "tab": tab.0 }));
    }
    if p.bool("new_tab").unwrap_or(false) {
        let tab = crate::workspace_commands::open_tab(
            s,
            kind,
            OpenRequest {
                panel: None,
                split: true,
            },
        );
        return Ok(json!({ "tab": tab.0 }));
    }
    let panel = panel.or_else(|| s.workspace.focused().and_then(|f| s.workspace.panel_of(f)));
    // A Tab of it already in that Panel comes to the front instead of a second one.
    let existing = panel.and_then(|panel| {
        s.workspace
            .panels()
            .find(|(p, _)| *p == panel)
            .and_then(|(_, tabs)| {
                tabs.iter()
                    .copied()
                    .find(|t| s.workspace.tab(*t).is_some_and(|t| t.kind == kind))
            })
    });
    if let Some(tab) = existing {
        s.workspace.focus(tab);
        return Ok(json!({ "tab": tab.0 }));
    }
    let tab = crate::workspace_commands::open_tab(
        s,
        kind,
        OpenRequest {
            panel,
            split: false,
        },
    );
    Ok(json!({ "tab": tab.0 }))
}

/// `layout.builtin`: replace the Workspace with a Built-in Layout, at the Passage shown now.
fn builtin(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "layout.builtin";
    let name = p.str("name").unwrap_or_default();
    let snap = s.builtin_snapshot(name).map_err(|why| failed(ID, why))?;
    let at = s.current_standard_verse();
    let first = snap.focused.map(TabId);
    // Where the closing Tabs went is History before they are replaced.
    s.record_history();
    s.workspace.restore(snap);
    if let (Some(tab), Some(verse)) = (first, at) {
        // Its Link set follows once the Bible's Verse map is loaded.
        s.workspace
            .push_effect(Effect::NavigateStandard { tab, verse });
    }
    Ok(json!({ "name": name }))
}

const fn launcher(
    id: &'static str,
    label: &'static str,
    shortcut: &'static str,
    run: fn(&mut Session, Params<'_>) -> Result<Value, CommandError>,
) -> CommandSpec {
    CommandSpec {
        id,
        label,
        shortcut: Some(shortcut),
        params: &[],
        enabled: always,
        run,
    }
}

fn show(s: &mut Session, which: Launcher) -> Result<Value, CommandError> {
    s.workspace.push_effect(Effect::OpenLauncher(which));
    Ok(Value::Null)
}

pub(crate) const COMMANDS: &[CommandSpec] = &[
    launcher("toolbar.library", "Library launcher", "Alt+L", |s, _| {
        show(s, Launcher::Library)
    }),
    launcher("toolbar.guides", "Guides launcher", "Alt+G", |s, _| {
        show(s, Launcher::Guides)
    }),
    launcher("toolbar.tools", "Tools launcher", "Alt+T", |s, _| {
        show(s, Launcher::Tools)
    }),
    launcher("toolbar.layouts", "Layouts launcher", "Alt+Y", |s, _| {
        show(s, Launcher::Layouts)
    }),
    CommandSpec {
        id: "toolbar.open_resource",
        label: "Open from the Library launcher",
        shortcut: None,
        params: &[
            ParamSpec {
                name: "id",
                kind: ParamKind::String,
                required: true,
                help: "Resource id from the library",
            },
            ParamSpec {
                name: "new_tab",
                kind: ParamKind::Boolean,
                required: false,
                help: "Always open a new Tab, split off beside the focused Panel (Ctrl/Cmd-click)",
            },
            ParamSpec {
                name: "panel",
                kind: ParamKind::Integer,
                required: false,
                help: "Open in this Panel (dropped on it) instead of the focused one",
            },
            ParamSpec {
                name: "beside",
                kind: ParamKind::Integer,
                required: false,
                help: "Open in a new Panel split off beside this one (dropped on its edge)",
            },
        ],
        enabled: always,
        run: open_resource,
    },
    CommandSpec {
        id: "layout.builtin",
        label: "Load a Built-in Layout",
        shortcut: None,
        params: &[ParamSpec {
            name: "name",
            kind: ParamKind::String,
            required: true,
            help: "Built-in Layout name, e.g. `Parallel Bibles`",
        }],
        enabled: always,
        run: builtin,
    },
];
