//! The Workspace shell (#35): the engine's Panels and Tabs shown as an egui_tiles dock, each
//! Tab with a variant-B header strip — [link chip] [‹ ›] [box] [title] [⋯] — and the launcher
//! menus. Every menu item, + entry and header button runs a command through [`Session::run`];
//! what the commands change comes back as [`Effect`]s, which the shell applies to the dock.

use std::collections::BTreeMap;
use std::sync::Arc;

use egui::{Color32, RichText, Stroke, Ui, pos2, vec2};
use egui_tiles::{Container, Linear, LinearDir, Tile, TileId, Tiles, Tree, UiResponse};
use obs_engine::{
    Effect, HeaderBox, JobMode, LayoutNode, MENUS, MenuItem, PanelId, Placement, Session, TabId,
    TabKind, Tool,
};
use obs_model::{Key, ResourceType};
use obs_scripture::{Passage, RefStyle, Standard, Verse, format_passage};
use serde_json::{Value, json};

use crate::compare_view::{ComparisonBible, ComparisonView, comparison_pane, paint_star};
use crate::doc::ReadingDoc;
use crate::palette::Palette;
use crate::pane::{AtlasTexture, WordHit, reading_pane};
use crate::theme;
use crate::view::ReadingView;
use crate::workers::SnippetCache;

mod annotate;
mod credits_tab;
mod get_bibles_tab;
mod guide_tab;
mod guide_words;
mod import_tab;
mod library_tab;
mod notes_tab;
#[path = "references.rs"]
pub(crate) mod references;
mod search_tab;
mod word_info_tab;
mod word_sections_view;
mod word_study_tab;

/// How long the pointer rests on a Tagged word before Word Info follows it (#18 rule 16).
pub(crate) const HOVER_DWELL_SECS: f64 = 0.15;

/// The pointer resting on a Tagged word: where, since when, and whether Word Info has it.
#[derive(Debug, Clone, PartialEq)]
struct Dwell {
    tab: TabId,
    word: WordHit,
    since: f64,
    shown: bool,
}

/// Per-Tab UI state the engine does not need.
#[derive(Default)]
struct TabUi {
    view: Option<ReadingView>,
    /// The header box's text while it is being edited.
    edit: Option<String>,
    filter: String,
    error: Option<String>,
    /// A verse or Text range marked after a jump, and when the mark started.
    flash: Option<(crate::pane::Flash, f64)>,
    /// Library Tab: the language chips that are on, the selected row and the row whose
    /// handle is being dragged (resource ids).
    languages: Vec<String>,
    selected: Option<String>,
    dragging: Option<String>,
    /// A Text Comparison Tab's view.
    comparison: Option<ComparisonView>,
    /// A Search Tab's results view and its fields being edited.
    search: Option<crate::search_view::SearchView>,
    search_fields: search_tab::Fields,
    /// Failed selected Bibles are retried only on request (or after removal/reselection).
    comparison_errors: BTreeMap<String, String>,
    reference_passage: Option<Passage>,
    reference_at: Option<Passage>,
    expanded_references: std::collections::BTreeSet<Verse>,
    /// A Passage Guide section whose header is being dragged.
    guide_drag: Option<obs_engine::GuideSection>,
    /// The selection toolbar is up (from a selection's release until the next press).
    toolbar: bool,
    /// Where the selection was drawn last frame, in screen points.
    selection_rect: Option<egui::Rect>,
    /// Passage Guide sections showing all their rows (Key words, People & places).
    guide_expanded: std::collections::BTreeSet<obs_engine::GuideSection>,
    /// Word Study: Lexicon entries expanded (true) or collapsed by the user, by entry id, for
    /// the number in `lexicon_entries_for` (otherwise only the first is expanded).
    lexicon_open: BTreeMap<String, bool>,
    lexicon_entries_for: Option<obs_scripture::StrongsNumber>,
    /// Word Study (#56): the form filtering Occurrences (a [`obs_engine::Form::label`]) and
    /// whether Translated as shows every rendering, for `lexicon_entries_for`.
    occurrence_form: Option<String>,
    renderings_all: bool,
}

/// How long a jump's flash lasts, in seconds.
const FLASH_SECS: f64 = 1.2;

/// Everything the dock's behaviour needs, apart from the tree itself.
pub(crate) struct State {
    pub(crate) session: Session,
    tabs: BTreeMap<TabId, TabUi>,
    docs: BTreeMap<String, Arc<ReadingDoc>>,
    atlas: AtlasTexture,
    references: references::References,
    toast: Option<(String, f64)>,
    window: Option<(String, String)>,
    /// The Layout-name prompt: the Layout being renamed (none: saving a new one) and the text.
    prompt: Option<(Option<String>, String)>,
    /// The Remove… confirmation: resource id, window title and message.
    confirm: Option<(String, String, String)>,
    pick_import: bool,
    /// The Get Bibles Tab's search and language filter.
    get_bibles_ui: get_bibles_tab::GetBiblesUi,
    picker: Option<std::sync::mpsc::Receiver<Option<std::path::PathBuf>>>,
    /// When the Last session was last checked for saving.
    last_save: f64,
    /// This frame's Page-tone palette.
    pub(crate) palette: Palette,
    search: String,
    now: f64,
    /// Ctrl/Cmd is held: opening something splits off a new Panel.
    split_held: bool,
    /// egui_tiles Tabs containers ↔ engine Panels.
    pub(crate) panels: std::collections::HashMap<TileId, PanelId>,
    /// The Tagged word the pointer is resting on, for Word Info's dwell.
    dwell: Option<Dwell>,
    /// Inline-snippet layouts for Word Info's word and Lemma.
    snippets: SnippetCache,
    /// Highlights and Notes to draw, per Resource (#62).
    marks: annotate::MarkCache,
    /// The popup Note editor, when open.
    note_editor: Option<annotate::NoteEditor>,
    /// The Notes Tool's column, cards and the card being edited (#64).
    notes_ui: notes_tab::NotesUi,
}

/// The app window's content: menus, Search field and the docked Workspace.
pub struct Shell {
    pub(crate) st: State,
    pub(crate) tree: Tree<TabId>,
    shown: Vec<(PanelId, Vec<TabId>)>,
}

impl Default for Shell {
    fn default() -> Self {
        Self::new(Session::new(JobMode::default_for_host()))
    }
}

impl Shell {
    pub fn new(session: Session) -> Self {
        Self {
            st: State {
                session,
                tabs: BTreeMap::new(),
                docs: BTreeMap::new(),
                atlas: AtlasTexture::default(),
                references: references::References::default(),
                toast: None,
                window: None,
                prompt: None,
                confirm: None,
                pick_import: false,
                get_bibles_ui: get_bibles_tab::GetBiblesUi::default(),
                picker: None,
                last_save: 0.0,
                palette: Palette::PAPER,
                search: String::new(),
                now: 0.0,
                split_held: false,
                panels: std::collections::HashMap::new(),
                dwell: None,
                snippets: SnippetCache::default(),
                marks: annotate::MarkCache::default(),
                note_editor: None,
                notes_ui: notes_tab::NotesUi::default(),
            },
            tree: Tree::empty(egui::Id::unique("workspace")),
            shown: Vec::new(),
        }
    }

    pub fn session(&self) -> &Session {
        &self.st.session
    }

    pub fn session_mut(&mut self) -> &mut Session {
        &mut self.st.session
    }

    /// Run a command and apply what it changed. Failures become a toast.
    pub fn run(&mut self, id: &str, params: &Value) -> Option<Value> {
        let out = self.st.run(id, params);
        self.apply_effects();
        out
    }

    /// The default arrangement: the ★ Bible (first in Preferred order) in one Panel; the
    /// Library and Word Info in a Panel to its right.
    pub fn reset_layout(&mut self) {
        self.st.session.workspace.clear();
        self.apply_effects();
        self.tree = Tree::empty(egui::Id::unique("workspace"));
        self.st.panels.clear();
        let first = self
            .st
            .session
            .library
            .preferred(ResourceType::Bible)
            .map(|e| e.manifest.resource_id.clone());
        if let Some(id) = first {
            // The first Bible starts in Link set A, so Bibles opened from it follow along.
            let bible = self.run("resource.open", &json!({ "id": id }));
            if let Some(t) = bible.and_then(|v| v["tab"].as_u64()) {
                self.run("tab.link", &json!({ "tab": t, "set": "A" }));
            }
        }
        let library = self.run("library.open", &json!({}));
        self.run("tool.word_info", &json!({}));
        // The Library is the useful Tab there until Word Info has content (#50).
        if let Some(t) = library.and_then(|v| v["tab"].as_u64()) {
            self.st.session.workspace.focus(TabId(t));
            self.apply_effects();
        }
        if let Some(Tile::Container(Container::Linear(l))) =
            self.tree.root().and_then(|r| self.tree.tiles.get_mut(r))
            && let [a, b] = l.children[..]
        {
            l.shares.set_share(a, 2.0);
            l.shares.set_share(b, 1.0);
        }
        // Focus the Bible.
        let bible = self
            .st
            .session
            .workspace
            .tabs()
            .find(|t| t.kind.is_reading())
            .map(|t| t.id);
        if let Some(t) = bible {
            self.st.session.workspace.focus(t);
            self.apply_effects();
        }
    }

    /// Apply queued engine effects to the dock.
    pub fn apply_effects(&mut self) {
        for e in self.st.session.workspace.take_effects() {
            match e {
                Effect::Opened { tab, placement } => {
                    self.st.opened(tab);
                    self.insert_tab(tab, placement);
                }
                Effect::Focus(tab) => {
                    self.tree
                        .make_active(|_, t| matches!(t, Tile::Pane(p) if *p == tab));
                }
                Effect::Closed(tab) => {
                    if let Some(id) = self.tree.tiles.find_pane(&tab) {
                        self.tree.remove_recursively(id);
                    }
                    self.st.tabs.remove(&tab);
                }
                Effect::Navigate { tab, verse } => {
                    let now = self.st.now;
                    if let Some(t) = self.st.tabs.get_mut(&tab)
                        && let Some(view) = t.view.as_mut()
                    {
                        if view.jump_to(verse) {
                            t.flash = Some((crate::pane::Flash::Verse(verse), now));
                        } else {
                            self.st
                                .toast(format!("No section or Verse for {verse} in this Resource"));
                        }
                    }
                }
                Effect::NavigateEntry { tab, key } => {
                    if let Some(view) = self.st.tabs.get_mut(&tab).and_then(|t| t.view.as_mut()) {
                        view.jump_to_entry(&key);
                    }
                }
                Effect::Follow { tab, verse, flash } => {
                    let now = self.st.now;
                    if let Some(t) = self.st.tabs.get_mut(&tab)
                        && let Some(view) = t.view.as_mut()
                        && view.follow(verse)
                        && flash
                    {
                        t.flash = Some((crate::pane::Flash::Verse(verse), now));
                    }
                }
                Effect::NavigateText {
                    tab,
                    unit,
                    range,
                    key,
                } => {
                    let now = self.st.now;
                    if let Some(t) = self.st.tabs.get_mut(&tab)
                        && let Some(view) = t.view.as_mut()
                    {
                        let index = view.doc.units.iter().position(|u| u.id == unit);
                        let (start, end) = (range.start as usize, range.end as usize);
                        if let Some(i) = index
                            && view.jump_to_text(i, start)
                        {
                            let flash = crate::pane::Flash::Text {
                                unit: i,
                                range: start..end,
                            };
                            t.flash = Some((flash, now));
                        } else {
                            // The text is not shown (a note): its Key, at least.
                            let shown = match &key {
                                Some(Key::Passage(p)) => view.jump_to(p.start()),
                                Some(k) => view.jump_to_entry(k),
                                None => false,
                            };
                            if !shown {
                                self.st
                                    .toast(format!("“{unit}” is not shown in this Resource"));
                            }
                        }
                    }
                }
                Effect::ResetLayout => self.reset_layout(),
                Effect::Rebuild => self.rebuild(),
                Effect::PromptLayoutName { rename } => {
                    let text = rename.clone().unwrap_or_default();
                    self.st.prompt = Some((rename, text));
                }
                Effect::Message(m) => self.st.toast(m),
                Effect::Show { title, body } => self.st.window = Some((title, body)),
                Effect::ConfirmRemove {
                    resource_id,
                    title,
                    message,
                } => self.st.confirm = Some((resource_id, title, message)),
                Effect::PickImport => self.st.pick_import = true,
                Effect::Removed(id) => {
                    self.st.docs.remove(&id);
                    self.st.marks.forget(&id);
                }
                Effect::NavigateStandard { tab, verse } => {
                    self.st.session.workspace.navigate_standard(tab, verse);
                }
                Effect::RevealText { tab, unit, byte } => {
                    let now = self.st.now;
                    let shown = self
                        .st
                        .tabs
                        .get_mut(&tab)
                        .and_then(|t| {
                            let view = t.view.as_mut()?;
                            let u = view.doc.units.iter().position(|x| x.id == unit)?;
                            if !view.jump_to_pos(u, byte) {
                                return None;
                            }
                            if let Some(v) = view.doc.verse_containing(u, byte) {
                                t.flash = Some((crate::pane::Flash::Verse(v), now));
                            }
                            Some(())
                        })
                        .is_some();
                    if !shown {
                        self.st
                            .toast("That text is no longer in this Resource".to_string());
                    }
                }
            }
        }
        self.record_layout();
    }

    fn container_of(&self, panel: PanelId) -> Option<TileId> {
        self.st
            .panels
            .iter()
            .find(|(t, p)| **p == panel && self.tree.tiles.get(**t).is_some())
            .map(|(t, _)| *t)
    }

    fn insert_tab(&mut self, tab: TabId, placement: Placement) {
        let pane = self.tree.tiles.insert_pane(tab);
        if let Placement::Panel(p) = placement
            && let Some(c) = self.container_of(p)
            && let Some(Tile::Container(Container::Tabs(tabs))) = self.tree.tiles.get_mut(c)
        {
            tabs.add_child(pane);
            tabs.set_active(pane);
            return;
        }
        let (panel, beside) = match placement {
            Placement::Panel(p) => (p, None),
            Placement::NewPanel { panel, beside } => (panel, beside),
        };
        let container = self.tree.tiles.insert_tab_tile(vec![pane]);
        self.st.panels.insert(container, panel);
        let Some(root) = self.tree.root() else {
            self.tree.root = Some(container);
            return;
        };
        // Split off to the right of `beside` (or of the whole Workspace).
        let anchor = beside.and_then(|b| self.container_of(b)).unwrap_or(root);
        let parent = self.tree.tiles.parent_of(anchor);
        if let Some(parent) = parent
            && let Some(Tile::Container(Container::Linear(l))) = self.tree.tiles.get_mut(parent)
            && l.dir == LinearDir::Horizontal
        {
            let at = l
                .children
                .iter()
                .position(|c| *c == anchor)
                .map_or(l.children.len(), |i| i + 1);
            l.children.insert(at, container);
            return;
        }
        let row = self
            .tree
            .tiles
            .insert_new(Tile::Container(Container::Linear(Linear::new(
                LinearDir::Horizontal,
                vec![anchor, container],
            ))));
        match parent {
            None => self.tree.root = Some(row),
            Some(parent) => {
                if let Some(Tile::Container(c)) = self.tree.tiles.get_mut(parent) {
                    replace_child(c, anchor, row);
                }
            }
        }
    }

    /// Tell the engine what the dock now shows (after drags, docks and × closes).
    fn record_layout(&mut self) {
        // Right after a drop, egui_tiles can leave a pane outside any Tabs container until its
        // next simplify; do it now so a moved Tab is never mistaken for a closed one.
        let options = egui_tiles::Behavior::simplification_options(&self.st);
        self.tree.simplify(&options);
        let mut shown = Vec::new();
        let ids: Vec<TileId> = self.tree.tiles.tile_ids().collect();
        for id in ids {
            let Some(Tile::Container(Container::Tabs(t))) = self.tree.tiles.get(id) else {
                continue;
            };
            let tabs: Vec<TabId> = t
                .children
                .iter()
                .filter_map(|c| match self.tree.tiles.get(*c) {
                    Some(Tile::Pane(p)) => Some(*p),
                    _ => None,
                })
                .collect();
            let panel = match self.st.panels.get(&id) {
                Some(p) => *p,
                None => {
                    let p = self.st.session.workspace.new_panel_id();
                    self.st.panels.insert(id, p);
                    p
                }
            };
            shown.push((panel, tabs));
        }
        shown.sort();
        let tiles = &self.tree.tiles;
        self.st.panels.retain(|t, _| tiles.get(*t).is_some());
        if shown != self.shown {
            self.st.session.workspace.sync_panels(shown.clone());
            self.shown = shown;
            let live: Vec<TabId> = self.st.session.workspace.tabs().map(|t| t.id).collect();
            self.st.tabs.retain(|t, _| live.contains(t));
        }
        let arrangement = self.tree.root().and_then(|r| self.node(r));
        self.st.session.workspace.arrangement = arrangement;
    }

    /// The dock below `tile` as a saved arrangement.
    fn node(&self, tile: TileId) -> Option<LayoutNode> {
        match self.tree.tiles.get(tile)? {
            Tile::Pane(t) => Some(LayoutNode::Panel {
                panel: 0,
                tabs: vec![t.0],
                active: Some(t.0),
            }),
            Tile::Container(Container::Tabs(t)) => {
                let tabs: Vec<u64> = t
                    .children
                    .iter()
                    .filter_map(|c| match self.tree.tiles.get(*c) {
                        Some(Tile::Pane(p)) => Some(p.0),
                        _ => None,
                    })
                    .collect();
                let active = t.active.and_then(|a| match self.tree.tiles.get(a) {
                    Some(Tile::Pane(p)) => Some(p.0),
                    _ => None,
                });
                let panel = self.st.panels.get(&tile).map_or(0, |p| p.0);
                (!tabs.is_empty()).then_some(LayoutNode::Panel {
                    panel,
                    tabs,
                    active,
                })
            }
            Tile::Container(Container::Linear(l)) => {
                let children: Vec<(LayoutNode, f32)> = l
                    .children
                    .iter()
                    .filter_map(|c| Some((self.node(*c)?, l.shares[*c])))
                    .collect();
                if children.is_empty() {
                    return None;
                }
                Some(match l.dir {
                    LinearDir::Horizontal => LayoutNode::Row { children },
                    LinearDir::Vertical => LayoutNode::Column { children },
                })
            }
            Tile::Container(Container::Grid(g)) => {
                let children: Vec<(LayoutNode, f32)> = g
                    .children()
                    .filter_map(|c| Some((self.node(*c)?, 1.0)))
                    .collect();
                (!children.is_empty()).then_some(LayoutNode::Row { children })
            }
        }
    }

    /// Insert a saved arrangement into the dock; returns its root tile.
    fn build(&mut self, node: &LayoutNode) -> Option<TileId> {
        match node {
            LayoutNode::Panel {
                panel,
                tabs,
                active,
            } => {
                let live: Vec<TabId> = tabs
                    .iter()
                    .map(|t| TabId(*t))
                    .filter(|t| self.st.session.workspace.tab(*t).is_some())
                    .collect();
                if live.is_empty() {
                    return None;
                }
                let panes: Vec<TileId> = live
                    .iter()
                    .map(|t| self.tree.tiles.insert_pane(*t))
                    .collect();
                let container = self.tree.tiles.insert_tab_tile(panes.clone());
                if let Some(Tile::Container(Container::Tabs(t))) =
                    self.tree.tiles.get_mut(container)
                    && let Some(i) = active.and_then(|a| live.iter().position(|t| t.0 == a))
                {
                    t.set_active(panes[i]);
                }
                let panel = if *panel == 0 {
                    self.st.session.workspace.new_panel_id()
                } else {
                    PanelId(*panel)
                };
                self.st.panels.insert(container, panel);
                Some(container)
            }
            LayoutNode::Row { children } | LayoutNode::Column { children } => {
                let built: Vec<(TileId, f32)> = children
                    .iter()
                    .filter_map(|(c, share)| Some((self.build(c)?, *share)))
                    .collect();
                if built.len() == 1 {
                    return built.first().map(|(t, _)| *t);
                }
                if built.is_empty() {
                    return None;
                }
                let dir = if matches!(node, LayoutNode::Row { .. }) {
                    LinearDir::Horizontal
                } else {
                    LinearDir::Vertical
                };
                let mut linear = Linear::new(dir, built.iter().map(|(t, _)| *t).collect());
                for (t, share) in &built {
                    linear.shares.set_share(*t, *share);
                }
                Some(
                    self.tree
                        .tiles
                        .insert_new(Tile::Container(Container::Linear(linear))),
                )
            }
        }
    }

    /// Rebuild the dock and every Tab's view from the engine's restored Workspace.
    fn rebuild(&mut self) {
        self.tree = Tree::empty(egui::Id::unique("workspace"));
        self.st.panels.clear();
        self.st.tabs.clear();
        self.shown.clear();
        let ids: Vec<TabId> = self.st.session.workspace.tabs().map(|t| t.id).collect();
        for t in ids {
            self.st.opened(t);
        }
        if let Some(node) = self.st.session.workspace.arrangement.clone() {
            self.tree.root = self.build(&node);
        }
        if let Some(f) = self.st.session.workspace.focused() {
            self.tree
                .make_active(|_, t| matches!(t, Tile::Pane(p) if *p == f));
        }
    }

    /// Follow the Theme choice (Follow system tracks the OS live) and pick this frame's
    /// chrome and page palettes. Nothing is laid out or rasterized again: colour roles are
    /// resolved when quads are drawn.
    fn apply_appearance(&mut self, ctx: &egui::Context) {
        let want = match self.st.session.appearance.theme {
            obs_engine::Theme::Light => egui::ThemePreference::Light,
            obs_engine::Theme::Dark => egui::ThemePreference::Dark,
            obs_engine::Theme::System => egui::ThemePreference::System,
        };
        if ctx.options(|o| o.theme_preference) != want {
            ctx.set_theme(want);
        }
        theme::set_for(ctx);
        let tone = self
            .st
            .session
            .appearance
            .page_tone
            .resolve(theme::c().dark);
        self.st.palette = Palette::for_tone(tone);
    }

    /// Save the Last session now (on quit), or at most once a second while running.
    pub fn save_session(&mut self, now: bool) {
        if !now && self.st.now - self.st.last_save < 1.0 {
            return;
        }
        self.st.last_save = self.st.now;
        if let Err(e) = self.st.session.save_last_session() {
            self.st.toast(format!("Could not save the session: {e}"));
        }
    }

    /// Restore the Last session if there is one, else open the default layout.
    pub fn restore_or_default(&mut self) {
        if self.st.session.restore_last_session() {
            self.apply_effects();
        } else {
            self.reset_layout();
        }
    }

    /// How soon to draw again without input: job events are applied only when a frame
    /// polls them, so running work keeps frames coming while the app is idle. Background
    /// Search index builds (#58) need only a slow poll.
    fn background_repaint(&self) -> Option<std::time::Duration> {
        // Bible searches and their snippet pages (#59) are applied by a frame too.
        if self.st.session.imports.is_running()
            || self.st.picker.is_some()
            || self.st.session.guides.is_busy()
            || self.st.session.bible_search_running()
            || self.st.session.word_study.is_busy()
            || self.st.session.reanchoring.is_busy()
        {
            Some(std::time::Duration::from_millis(16))
        } else if self.st.session.search.is_building() {
            Some(std::time::Duration::from_millis(250))
        } else {
            None
        }
    }

    /// Draw the whole window.
    pub fn ui(&mut self, ui: &mut Ui) {
        self.apply_appearance(ui.ctx());
        self.st.references.refresh(&self.st.session.library);
        self.st.now = ui.input(|i| i.time);
        self.st.split_held = ui.input(|i| i.modifiers.command);
        // Poll, never wait: parsing, Resource writes, Search-index hooks and Guide sections
        // live on workers. Without worker threads (wasm), Guide sections step here, a job a
        // frame.
        if !self.st.session.jobs().is_threaded()
            && (self.st.session.guides.is_busy()
                || self.st.session.word_study.is_busy()
                || self.st.session.reanchoring.is_busy())
        {
            self.st.session.jobs().pump(1);
        }
        let _other_jobs = self.st.session.poll_imports();
        self.st.poll_picker();
        self.apply_effects();
        if let Some(after) = self.background_repaint() {
            ui.ctx().request_repaint_after(after);
        }
        self.shortcuts(ui);
        egui::Panel::top("menus")
            .frame(
                egui::Frame::new()
                    .fill(theme::c().surface)
                    .inner_margin(egui::Margin::symmetric(14, 6)),
            )
            .show(ui, |ui| self.menu_bar(ui));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::c().shell)
                    .inner_margin(egui::Margin::same(8)),
            )
            .show(ui, |ui| {
                if self.tree.is_empty() {
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            RichText::new(
                                "Nothing is open. Use the Library menu, or drop a .obr file here.",
                            )
                            .color(theme::c().muted),
                        );
                    });
                } else {
                    self.tree.ui(&mut self.st, ui);
                }
            });
        self.apply_effects();
        self.st.overlays(ui);
        // A Notes card being edited saves as the user types, and at once once its Tab is not
        // drawn (#64): checked here, since the Notes Tab's own code only runs while drawn.
        self.st.notes_after_frame(ui.ctx());
        self.apply_effects();
        let closing = ui.input(|i| i.viewport().close_requested());
        if closing {
            // A Note being written is saved, as if focus had left it. One that cannot be saved
            // keeps the window open, showing why, until it is saved or discarded.
            self.st.save_note();
            self.st.save_card_edit(true);
            if self.st.note_editor.is_some() {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        }
        self.save_session(closing);
    }

    fn shortcuts(&mut self, ui: &mut Ui) {
        let focused = self.st.session.workspace.focused();
        // Keys a text field uses itself (Ctrl+Z, Esc …) are left to it while it has focus.
        let typing = ui.ctx().text_edit_focused();
        let all = bindings(&self.st.session);
        let bound: Vec<(&obs_engine::CommandSpec, egui::KeyboardShortcut)> = self
            .st
            .session
            .commands()
            .iter()
            .filter(|spec| !(typing && spec.shortcut.is_some_and(annotate::yields_to_text_fields)))
            .filter_map(|spec| Some((spec, spec.shortcut.and_then(parse_shortcut)?)))
            .collect();
        let mut run: Vec<(&'static str, Value)> = Vec::new();
        for (spec, sc) in bound {
            let needs_tab = spec.params.iter().any(|p| p.name == "tab" && p.required);
            let other_required = spec.params.iter().any(|p| p.required && p.name != "tab");
            if other_required || (needs_tab && focused.is_none()) {
                continue;
            }
            // A disabled command leaves its key to whoever else wants it (Esc, say).
            if !self.st.session.is_enabled(spec.id) {
                continue;
            }
            if ui.input_mut(|i| consume_key(i, sc.modifiers, sc.logical_key, extra_held(sc, &all)))
            {
                let params = match (needs_tab, focused) {
                    (true, Some(t)) => json!({ "tab": t.0 }),
                    _ => json!({}),
                };
                run.push((spec.id, params));
            }
        }
        for (id, p) in run {
            self.run(id, &p);
        }
        self.st.annotation_keys(ui);
        self.apply_effects();
    }

    fn menu_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Open Bible")
                    .font(theme::display(19.0))
                    .color(theme::c().ink),
            );
            ui.add_space(6.0);
            ui.scope(|ui| {
                let w = &mut ui.visuals_mut().widgets;
                w.inactive.bg_stroke = Stroke::NONE;
                w.inactive.weak_bg_fill = Color32::TRANSPARENT;
                w.hovered.bg_stroke = Stroke::NONE;
                for menu in MENUS {
                    ui.menu_button(menu.title, |ui| {
                        for item in menu.items {
                            self.menu_item(ui, *item);
                        }
                    });
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut self.st.search)
                        .hint_text("Search")
                        .desired_width(220.0)
                        .margin(egui::Margin::symmetric(8, 4)),
                );
                if r.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && !self.st.search.trim().is_empty()
                {
                    let text = self.st.search.trim().to_string();
                    self.run("search.run", &json!({ "text": text }));
                }
            });
        });
    }

    fn menu_item(&mut self, ui: &mut Ui, item: MenuItem) {
        match item {
            MenuItem::Separator => {
                ui.separator();
            }
            MenuItem::Command(id) => {
                let Some(spec) = self.st.session.commands().get(id).copied() else {
                    return;
                };
                let label = match id {
                    "view.red_letter" if self.st.session.text.red_letter => "✓ Red letter".into(),
                    "view.highlights" if self.st.session.appearance.show_highlights => {
                        format!("✓ {}", spec.label)
                    }
                    _ => spec.label.to_string(),
                };
                let mut b = egui::Button::new(label);
                if let Some(sc) = spec.shortcut {
                    b = b.shortcut_text(sc);
                }
                let enabled = self.st.session.is_enabled(id);
                let r = ui.add_enabled(enabled, b);
                let r = if enabled {
                    r
                } else {
                    r.on_disabled_hover_text(not_yet(id))
                };
                if r.clicked() {
                    self.run(id, &json!({}));
                    ui.close();
                }
            }
            MenuItem::Choice { title, command } => {
                let (options, current) = obs_engine::choices(&self.st.session, command);
                let mut picked = None;
                ui.menu_button(title, |ui| {
                    for (id, label) in options {
                        if ui.selectable_label(id == current, label).clicked() {
                            picked = Some(id);
                            ui.close();
                        }
                    }
                });
                if let Some(id) = picked {
                    self.run(command, &json!({ "value": id }));
                    ui.close();
                }
            }
            MenuItem::Notebooks => {
                // Each Notebook's Show in text (#17 rule 18); the Notes Tool (#64) shows the
                // same toggles beside its Notebooks.
                let notebooks = if self.st.session.user.is_some() {
                    self.st.session.notebooks().unwrap_or_default()
                } else {
                    Vec::new()
                };
                let mut toggled = None;
                let enabled = self.st.session.is_enabled("notebook.show_in_text");
                ui.add_enabled_ui(enabled, |ui| {
                    ui.menu_button("Notebooks in text", |ui| {
                        for n in &notebooks {
                            let mut on = n.show_in_text;
                            if ui.checkbox(&mut on, &n.name).clicked() {
                                toggled = Some((n.id.clone(), on));
                            }
                        }
                    })
                    .response
                    .on_disabled_hover_text("Notebooks are kept in the User database");
                });
                if let Some((id, show)) = toggled {
                    self.run("notebook.show_in_text", &json!({ "id": id, "show": show }));
                }
            }
            MenuItem::Layouts => {
                let names = self
                    .st
                    .session
                    .user
                    .as_ref()
                    .and_then(|u| u.layouts().ok())
                    .unwrap_or_default();
                if names.is_empty() {
                    ui.label(RichText::new("No saved layouts").color(theme::c().muted));
                }
                for name in names {
                    let mut run: Option<&'static str> = None;
                    ui.menu_button(&name, |ui| {
                        for id in ["layout.restore", "layout.rename", "layout.delete"] {
                            let label = self.st.session.commands().get(id).map_or(id, |c| c.label);
                            let label = label.replace(" layout", "");
                            if ui.button(label).clicked() {
                                run = Some(id);
                                ui.close();
                            }
                        }
                    });
                    if let Some(id) = run {
                        self.run(id, &json!({ "name": name }));
                        ui.close();
                    }
                }
            }
            MenuItem::Resources => {
                let entries: Vec<(String, String, ResourceType)> = self
                    .st
                    .session
                    .library
                    .entries()
                    .iter()
                    .map(|e| {
                        (
                            e.manifest.resource_id.clone(),
                            format!("{} — {}", e.manifest.abbreviation, e.manifest.title),
                            e.manifest.resource_type,
                        )
                    })
                    .collect();
                if entries.is_empty() {
                    ui.label(RichText::new("No Resources yet").color(theme::c().muted));
                }
                for (id, label, _) in entries {
                    if ui.button(label).clicked() {
                        let split = ui.input(|i| i.modifiers.command);
                        self.run("resource.open", &json!({ "id": id, "split": split }));
                        ui.close();
                    }
                }
            }
        }
    }
}

/// Which ticket a not-yet-built menu item arrives with.
fn not_yet(id: &str) -> &'static str {
    match id {
        "library.import" => "Import a local USFM file or folder",
        "layout.save" => "Saved Layouts arrive with #37",
        "word_info.word_study" => "Rest the pointer on a word with a Strong's number first",
        _ => "Not available yet",
    }
}

fn replace_child(c: &mut Container, old: TileId, new: TileId) {
    let children = match c {
        Container::Tabs(t) => &mut t.children,
        Container::Linear(l) => &mut l.children,
        Container::Grid(_) => {
            c.add_child(new);
            return;
        }
    };
    if let Some(slot) = children.iter_mut().find(|c| **c == old) {
        *slot = new;
    }
}

/// Every key binding in the app: the commands' and the annotation keys.
pub(crate) fn bindings(session: &obs_engine::Session) -> Vec<egui::KeyboardShortcut> {
    (session.commands().iter())
        .filter_map(|spec| spec.shortcut.and_then(parse_shortcut))
        .chain(annotate::shortcuts())
        .collect()
}

/// Which of Shift and Alt may be held on top of `sc`'s own modifiers and still press it.
///
/// Usually both, as egui matches: some keys need them on some layouts (Ctrl+Plus is
/// Ctrl+Shift+= on a US keyboard). None when another binding in `all` is the same key with other
/// modifiers, so the two are told apart exactly: Ctrl+Shift+Z is never Ctrl+Z, even when there
/// is nothing to redo.
pub(crate) fn extra_held(
    sc: egui::KeyboardShortcut,
    all: &[egui::KeyboardShortcut],
) -> egui::Modifiers {
    if (all.iter()).any(|o| o.logical_key == sc.logical_key && o.modifiers != sc.modifiers) {
        egui::Modifiers::NONE
    } else {
        egui::Modifiers::SHIFT | egui::Modifiers::ALT
    }
}

/// Consume presses of `key` with `modifiers`, plus any of the Shift and Alt in `extra`.
/// Shift or Alt held but in neither does not match.
pub(crate) fn consume_key(
    i: &mut egui::InputState,
    modifiers: egui::Modifiers,
    key: egui::Key,
    extra: egui::Modifiers,
) -> bool {
    let before = i.events.len();
    i.events.retain(|e| {
        let egui::Event::Key {
            key: k,
            modifiers: m,
            pressed: true,
            ..
        } = e
        else {
            return true;
        };
        let held = egui::Modifiers {
            shift: m.shift && !(extra.shift && !modifiers.shift),
            alt: m.alt && !(extra.alt && !modifiers.alt),
            ..*m
        };
        !(*k == key && held.matches_exact(modifiers))
    });
    i.events.len() != before
}

/// `Ctrl+Shift+L` → an egui shortcut. Ctrl means Cmd on macOS.
pub fn parse_shortcut(s: &str) -> Option<egui::KeyboardShortcut> {
    let mut mods = egui::Modifiers::NONE;
    let mut key = None;
    for part in s.split('+') {
        match part {
            "Ctrl" => mods |= egui::Modifiers::COMMAND,
            "Shift" => mods |= egui::Modifiers::SHIFT,
            "Alt" => mods |= egui::Modifiers::ALT,
            k => key = egui::Key::from_name(k),
        }
    }
    key.map(|k| egui::KeyboardShortcut::new(mods, k))
}

impl State {
    fn toast(&mut self, m: impl Into<String>) {
        self.toast = Some((m.into(), self.now));
    }

    fn run(&mut self, id: &str, params: &Value) -> Option<Value> {
        match self.session.run(id, params) {
            Ok(v) => Some(v),
            Err(e) => {
                self.toast(e.to_string());
                None
            }
        }
    }

    /// A Tab was opened: load its Resource if it has one.
    fn opened(&mut self, tab: TabId) {
        let mut ui_state = TabUi::default();
        let kind = self.session.workspace.tab(tab).map(|t| t.kind.clone());
        if kind == Some(TabKind::Tool(Tool::TextComparison)) {
            ui_state.comparison = Some(ComparisonView::new(self.session.text));
        }
        if kind == Some(TabKind::Tool(Tool::Search)) {
            ui_state.search = Some(crate::search_view::SearchView::new(self.session.text));
        }
        if let Some(TabKind::Resource(id)) = &kind {
            match self.doc(id) {
                Ok(doc) => {
                    let mut view = ReadingView::new(Arc::clone(&doc), self.session.text);
                    if let Some(t) = self.session.workspace.tab_mut(tab) {
                        t.versification = (doc.manifest.resource_type == ResourceType::Bible)
                            .then(|| Arc::new(doc.versification.clone()));
                        if doc.manifest.resource_type != ResourceType::Bible {
                            t.navigation = Some(Arc::clone(&doc.navigation));
                        }
                        t.verse_map = doc.verse_map.clone();
                        if !doc.manifest.key_schemes.is_empty() {
                            t.key_schemes = doc.manifest.key_schemes.clone();
                        }
                    }
                    // Start where the Link set is; unlinked, where another Bible is.
                    let ws = &self.session.workspace;
                    let own = ws.tab(tab).and_then(|t| t.history.current());
                    let at = own.or_else(|| ws.set_location(tab)).or_else(|| {
                        ws.tabs()
                            .filter(|t| t.id != tab && t.kind.is_reading() && t.verse_map.is_none())
                            .find_map(|t| t.history.current())
                    });
                    if let Some(v) = at {
                        let local = if doc.manifest.resource_type == ResourceType::Commentary {
                            doc.navigation.section(v).and_then(|e| match e.key {
                                Key::Passage(p) => Some(p.start()),
                                _ => None,
                            })
                        } else {
                            Some(v)
                        };
                        if let Some(v) = local
                            && view.jump_to(v)
                        {
                            self.session.workspace.scrolled(tab, v);
                        }
                    }
                    if matches!(
                        doc.manifest.resource_type,
                        ResourceType::Lexicon | ResourceType::Dictionary
                    ) {
                        let ws = &self.session.workspace;
                        let key = ws
                            .tab(tab)
                            .and_then(|t| t.entry_history.current())
                            .or_else(|| ws.set_entry_location(tab))
                            .or_else(|| doc.navigation.entries().next().map(|e| e.key.clone()));
                        if let Some(key) = key
                            && view.jump_to_entry(&key)
                            && let Some(t) = self.session.workspace.tab_mut(tab)
                        {
                            t.entry_history.scrolled_to(key);
                        }
                    }
                    ui_state.view = Some(view);
                }
                Err(e) => ui_state.error = Some(e),
            }
        }
        if let Some(TabKind::About(id)) = &kind
            && let Some(e) = self.session.library.get(id)
        {
            let doc = ReadingDoc::from_text(e.manifest.clone(), &e.manifest.attribution);
            ui_state.view = Some(ReadingView::new(Arc::new(doc), self.session.text));
        }
        self.tabs.insert(tab, ui_state);
        if let Some(standard) = self.references.pending_navigation.remove(&tab) {
            self.session.workspace.navigate_standard(tab, standard);
        }
    }

    fn doc(&mut self, id: &str) -> Result<Arc<ReadingDoc>, String> {
        if let Some(d) = self.docs.get(id) {
            return Ok(Arc::clone(d));
        }
        let entry = self
            .session
            .library
            .get(id)
            .ok_or_else(|| format!("{id} is not in the library"))?;
        let doc = load(&entry.path)?;
        self.docs.insert(id.to_string(), Arc::clone(&doc));
        Ok(doc)
    }

    /// The location shown in a Tab's reference box, in the Tab's own numbering.
    fn location_text(&self, tab: TabId) -> String {
        let Some(t) = self.session.workspace.tab(tab) else {
            return String::new();
        };
        if t.header_box() == HeaderBox::Word
            && let Some(words) = self.session.word_study.choosing(tab)
        {
            return words.to_string();
        }
        if matches!(
            t.header_box(),
            HeaderBox::Lexicon | HeaderBox::Headword | HeaderBox::Word
        ) {
            return t
                .entry_history
                .current()
                .map(|k| k.to_string())
                .unwrap_or_default();
        }
        let Some(v) = t.history.current() else {
            return String::new();
        };
        let p = Passage::verse(v);
        match &t.versification {
            Some(own) => format_passage(p, own.as_ref(), RefStyle::Long),
            None => format_passage(p, &Standard, RefStyle::Long),
        }
    }

    fn tab_body(&mut self, ui: &mut Ui, tab: TabId) {
        let pane = ui.max_rect();
        if ui.input(|i| i.pointer.any_pressed()) && ui.rect_contains_pointer(pane) {
            self.session.workspace.user_focused(tab);
        }
        let Some(kind) = self.session.workspace.tab(tab).map(|t| t.kind.clone()) else {
            return;
        };
        self.header(ui, tab, &kind);
        if self
            .session
            .workspace
            .tab(tab)
            .is_some_and(|t| matches!(t.header_box(), HeaderBox::Lexicon | HeaderBox::Headword))
        {
            self.entry_buttons(ui, tab);
        }
        // The body gets exactly the rest of the pane, whatever the header did (#13 finding).
        let rect = egui::Rect::from_min_max(pos2(pane.left(), ui.cursor().top()), pane.max);
        let mut body = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt(("body", tab)));
        body.set_clip_rect(rect.intersect(ui.clip_rect()));
        match kind {
            TabKind::Resource(_) => self.reading_body(&mut body, tab),
            TabKind::Library => self.library_body(&mut body, tab),
            TabKind::About(id) => self.about_body(&mut body, tab, &id),
            TabKind::ImportReport(id) => self.import_body(&mut body, &id),
            TabKind::GetBibles => self.get_bibles_body(&mut body),
            TabKind::Credits => self.credits_body(&mut body, tab),
            TabKind::Guide(obs_engine::Guide::PassageGuide) => {
                self.passage_guide_body(&mut body, tab)
            }
            TabKind::Guide(obs_engine::Guide::WordStudy) => self.word_study_body(&mut body, tab),
            TabKind::Tool(Tool::TextComparison) => self.comparison_body(&mut body, tab),
            TabKind::Tool(Tool::CrossReferences) => self.cross_references_body(&mut body, tab),
            TabKind::Tool(Tool::WordInfo) => self.word_info_body(&mut body, tab),
            TabKind::Tool(Tool::Search) => self.search_body(&mut body, tab),
            TabKind::Tool(Tool::Notes) => self.notes_body(&mut body, tab),
        }
    }

    fn reading_body(&mut self, ui: &mut Ui, tab: TabId) {
        let palette = self.palette;
        let text = self.session.text;
        let Some(t) = self.tabs.get_mut(&tab) else {
            return;
        };
        let Some(view) = t.view.as_mut() else {
            let msg = t.error.clone().unwrap_or_default();
            ui.centered_and_justified(|ui| ui.colored_label(theme::c().warn, msg));
            return;
        };
        view.set_settings(text);
        let flash = t.flash.clone().and_then(|(v, start)| {
            let age = self.now - start;
            (age < FLASH_SECS).then(|| {
                let a = (1.0 - age / FLASH_SECS).clamp(0.0, 1.0) as f32;
                (v, palette.flash.gamma_multiply(a))
            })
        });
        if flash.is_some() {
            ui.ctx().request_repaint();
        } else {
            t.flash = None;
        }
        let marks = self.marks.marks(&self.session, &view.doc);
        let out = reading_pane(ui, view, &mut self.atlas, &palette, flash, &marks);
        let following = view.is_following();
        let at = view.current_location();
        let reference = out
            .hovered_reference
            .map(|(unit, span)| references::TargetKey::new(&view.doc, unit, span));
        if let Some(n) = out.hovered_verse_number {
            verse_number_tooltip(ui, &view.doc, n);
        }
        let entry = out.user_moved.then(|| view.current_entry()).flatten();
        self.word_hovered(ui, tab, out.hovered_word.clone(), out.clicked_word.clone());
        if let Some(key) = entry {
            self.session.workspace.user_scrolled_entry(tab, key);
        }
        if let Some(at) = at {
            let ws = &mut self.session.workspace;
            if out.user_moved {
                ws.user_scrolled(tab, at);
            } else if !following && ws.tab(tab).is_some_and(|t| t.history.current() != Some(at)) {
                ws.scrolled(tab, at);
            }
        }
        if let Some(key) = reference {
            if out.reference_clicked {
                self.references.pending_clicks.insert(
                    tab,
                    (
                        key.clone(),
                        ui.input(|i| i.modifiers.command || i.modifiers.ctrl),
                    ),
                );
            }
            match self.references.targets.get(key) {
                Some(Ok(targets)) => self.reference_popup(ui, &targets),
                Some(Err(error)) => self.reference_message_popup(ui, &error),
                None => {
                    self.reference_message_popup(ui, "Loading Scripture reference…");
                    ui.ctx().request_repaint();
                }
            }
        }
        // A click on a cold cache must not disappear when the pointer leaves the span.
        if let Some((key, new_tab)) = self.references.pending_clicks.get(&tab).cloned() {
            match self.references.targets.get(key) {
                None => ui.ctx().request_repaint(),
                Some(result) => {
                    self.references.pending_clicks.remove(&tab);
                    match result {
                        Ok(targets) => {
                            if let Some(passage) = targets.iter().flat_map(|p| p.iter()).next() {
                                self.reference_navigate(tab, *passage, new_tab);
                            }
                        }
                        Err(error) => self.toast(error),
                    }
                }
            }
        }
        self.after_reading_pane(ui, tab, &out);
    }

    /// Text Comparison: the Bible chips, then the Verse blocks. The view follows the Tab's
    /// location (moved by its Link set or reference box) and scrolling it moves the Link set.
    fn comparison_body(&mut self, ui: &mut Ui, tab: TabId) {
        let Some(t) = self.session.workspace.tab(tab) else {
            return;
        };
        let comparison = t.comparison.clone();
        let location = t.history.current();
        let bibles = comparison.bibles(&self.session.library);
        let base = comparison.base_index(&bibles);
        let Some(tab_ui) = self.tabs.get_mut(&tab) else {
            return;
        };
        let mut errors = std::mem::take(&mut tab_ui.comparison_errors);
        errors.retain(|id, _| bibles.contains(id));
        let mut docs = Vec::new();
        for id in &bibles {
            let result = match errors.get(id) {
                Some(error) => Err(error.clone()),
                None => self.doc(id),
            };
            if let Err(error) = &result {
                errors.insert(id.clone(), error.clone());
            }
            if let Some(entry) = self.session.library.get(id) {
                docs.push(ComparisonBible {
                    id: id.clone(),
                    abbreviation: entry.manifest.abbreviation.clone(),
                    language: entry.manifest.language.clone(),
                    doc: result,
                });
            }
        }
        let extent = location.map(|v| comparison.extent(v)).unwrap_or_default();
        self.comparison_chips(ui, tab, &comparison, &bibles, base);
        if !errors.is_empty() && ui.button("Retry unavailable Bibles").clicked() {
            errors.clear();
        }
        let palette = self.palette;
        let Some(tab_ui) = self.tabs.get_mut(&tab) else {
            return;
        };
        tab_ui.comparison_errors = errors;
        let Some(view) = tab_ui.comparison.as_mut() else {
            return;
        };
        view.set_settings(self.session.text);
        view.set(docs, base, comparison.differences, extent);
        if location != view.synced {
            if let Some(v) = location {
                view.jump_to(v);
            }
            view.synced = location;
        }
        let out = comparison_pane(ui, view, &mut self.atlas, &palette);
        if out.user_moved
            && let Some(v) = view.current_location()
            && Some(v) != view.synced
        {
            view.synced = Some(v);
            self.session.workspace.user_scrolled(tab, v);
        }
    }

    /// The chips under a Text Comparison header: one per Bible (☆ makes it the ★ base, ×
    /// removes it), + Bible, Show differences, and the typed range's removable chip.
    fn comparison_chips(
        &mut self,
        ui: &mut Ui,
        tab: TabId,
        comparison: &obs_engine::Comparison,
        bibles: &[String],
        base: Option<usize>,
    ) {
        let mut run: Option<(&'static str, Value)> = None;
        let abbrev = |id: &str| {
            self.session
                .library
                .get(id)
                .map_or_else(|| id.to_string(), |e| e.manifest.abbreviation.clone())
        };
        let others: Vec<(String, String)> = self
            .session
            .library
            .of_type(ResourceType::Bible)
            .filter(|e| !bibles.contains(&e.manifest.resource_id))
            .map(|e| {
                (
                    e.manifest.resource_id.clone(),
                    format!("{} — {}", e.manifest.abbreviation, e.manifest.title),
                )
            })
            .collect();
        egui::Frame::new()
            .fill(theme::c().surface)
            .inner_margin(egui::Margin::symmetric(10, 5))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let small = |s: &str| {
                        egui::Button::new(RichText::new(s).size(12.0))
                            .frame(false)
                            .min_size(vec2(16.0, 20.0))
                    };
                    for (i, id) in bibles.iter().enumerate() {
                        egui::Frame::new()
                            .fill(theme::c().field)
                            .stroke(Stroke::new(1.0, theme::c().rule))
                            .corner_radius(11.0)
                            .inner_margin(egui::Margin::symmetric(6, 0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 2.0;
                                    let (rect, r) = ui.allocate_exact_size(
                                        vec2(16.0, 20.0),
                                        egui::Sense::click(),
                                    );
                                    let color = if r.hovered() || Some(i) == base {
                                        theme::c().lapis
                                    } else {
                                        theme::c().muted
                                    };
                                    paint_star(
                                        ui.painter(),
                                        rect.center(),
                                        6.0,
                                        Some(i) == base,
                                        color,
                                    );
                                    if r.on_hover_text("Compare the others with this Bible")
                                        .clicked()
                                    {
                                        run = Some((
                                            "compare.base",
                                            json!({ "tab": tab.0, "id": id }),
                                        ));
                                    }
                                    ui.label(RichText::new(abbrev(id)).size(12.5));
                                    if ui.add(small("×")).on_hover_text("Remove").clicked() {
                                        run = Some((
                                            "compare.bible",
                                            json!({ "tab": tab.0, "id": id }),
                                        ));
                                    }
                                });
                            });
                    }
                    ui.menu_button(RichText::new("+ Bible").size(12.5), |ui| {
                        if others.is_empty() {
                            ui.label(RichText::new("Every Bible is shown").color(theme::c().muted));
                        }
                        for (id, label) in &others {
                            if ui.button(label).clicked() {
                                run = Some(("compare.bible", json!({ "tab": tab.0, "id": id })));
                                ui.close();
                            }
                        }
                    });
                    let mut differences = comparison.differences;
                    if ui
                        .checkbox(&mut differences, RichText::new("Differences").size(12.5))
                        .clicked()
                    {
                        run = Some(("compare.differences", json!({ "tab": tab.0 })));
                    }
                    if let Some(range) = comparison.range {
                        let text = format_passage(range, &Standard, RefStyle::Short);
                        if ui
                            .add(
                                egui::Button::new(RichText::new(format!("{text}  ×")).size(12.5))
                                    .corner_radius(11.0),
                            )
                            .on_hover_text("Show the whole chapter")
                            .clicked()
                        {
                            run = Some(("compare.clear_range", json!({ "tab": tab.0 })));
                        }
                    }
                });
            });
        if let Some((id, params)) = run {
            self.run(id, &params);
        }
    }

    fn header(&mut self, ui: &mut Ui, tab: TabId, kind: &TabKind) {
        let focused = self.session.workspace.focused() == Some(tab);
        let fill = if focused {
            theme::c().field
        } else {
            theme::c().surface
        };
        let frame = egui::Frame::new()
            .fill(fill)
            .inner_margin(egui::Margin::symmetric(9, 4))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 5.0;
                    let narrow = ui.available_width() < 430.0;
                    ui.add_space(4.0);
                    if kind.has_link_chip() {
                        self.link_chip(ui, tab);
                    }
                    let header = self
                        .session
                        .workspace
                        .tab(tab)
                        .map_or(kind.header_box(), |t| t.header_box());
                    if matches!(
                        header,
                        HeaderBox::Reference
                            | HeaderBox::Lexicon
                            | HeaderBox::Headword
                            | HeaderBox::Word
                    ) {
                        self.history_buttons(ui, tab);
                    }
                    self.header_box(ui, tab, header, if narrow { 110.0 } else { 140.0 });
                    let title = self.title(tab, kind);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.tab_menu(ui, tab);
                        if !narrow {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(title).small().color(theme::c().muted),
                                )
                                .truncate(),
                            );
                        }
                    });
                });
                if let Some(e) = self.tabs.get(&tab).and_then(|t| t.error.clone()) {
                    ui.label(RichText::new(e).small().color(theme::c().warn));
                }
            });
        let r = frame.response.rect;
        ui.painter().rect_filled(
            egui::Rect::from_min_size(r.min, vec2(if focused { 4.0 } else { 2.0 }, r.height())),
            0.0,
            theme::link_color(self.session.workspace.tab(tab).and_then(|t| t.link)),
        );
        ui.painter().hline(
            r.x_range(),
            r.bottom() - 0.5,
            Stroke::new(1.0, theme::c().rule),
        );
    }

    fn title(&self, tab: TabId, kind: &TabKind) -> String {
        match kind {
            TabKind::Resource(id) => self
                .session
                .library
                .get(id)
                .map(|e| e.manifest.title.clone())
                .unwrap_or_default(),
            _ => self
                .session
                .workspace
                .tab(tab)
                .map(|t| t.title.clone())
                .unwrap_or_default(),
        }
    }

    fn entry_buttons(&mut self, ui: &mut Ui, tab: TabId) {
        let enabled = |forward| {
            self.session.workspace.tab(tab).is_some_and(|t| {
                t.entry_history.current().is_some_and(|key| {
                    t.navigation
                        .as_ref()
                        .and_then(|n| n.adjacent(&key, forward))
                        .is_some()
                })
            })
        };
        let previous = enabled(false);
        let next = enabled(true);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(previous, egui::Button::new("Previous entry"))
                .clicked()
            {
                self.run("tab.previous_entry", &json!({ "tab": tab.0 }));
            }
            if ui
                .add_enabled(next, egui::Button::new("Next entry"))
                .clicked()
            {
                self.run("tab.next_entry", &json!({ "tab": tab.0 }));
            }
        });
    }

    fn history_buttons(&mut self, ui: &mut Ui, tab: TabId) {
        let (back, fwd) = self.session.workspace.tab(tab).map_or((false, false), |t| {
            if matches!(
                t.header_box(),
                HeaderBox::Lexicon | HeaderBox::Headword | HeaderBox::Word
            ) {
                (
                    t.entry_history.can_go_back(),
                    t.entry_history.can_go_forward(),
                )
            } else {
                (t.history.can_go_back(), t.history.can_go_forward())
            }
        });
        let arrow = |s: &str| {
            egui::Button::new(RichText::new(s).size(18.0))
                .frame(false)
                .min_size(vec2(22.0, 24.0))
        };
        if ui
            .add_enabled(back, arrow("‹"))
            .on_hover_text("Back (Alt+←)")
            .clicked()
        {
            self.run("tab.back", &json!({ "tab": tab.0 }));
        }
        if ui
            .add_enabled(fwd, arrow("›"))
            .on_hover_text("Forward (Alt+→)")
            .clicked()
        {
            self.run("tab.forward", &json!({ "tab": tab.0 }));
        }
    }

    fn header_box(&mut self, ui: &mut Ui, tab: TabId, kind: HeaderBox, width: f32) {
        match kind {
            HeaderBox::Keyless => {}
            HeaderBox::FollowsHover => {
                if self.session.word_info.pinned {
                    let r = ui.add(
                        egui::Button::new(RichText::new("📌 Pinned").small()).corner_radius(11.0),
                    );
                    if r.on_hover_text(
                        "Pinned to the clicked word. Click (or press Esc) to follow hover again.",
                    )
                    .clicked()
                    {
                        self.run("word_info.unpin", &json!({}));
                    }
                } else {
                    ui.label(
                        RichText::new("Follows hover")
                            .small()
                            .color(theme::c().muted),
                    );
                }
            }
            HeaderBox::Filter => {
                if let Some(t) = self.tabs.get_mut(&tab) {
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut t.filter)
                            .hint_text("Filter")
                            .desired_width(width)
                            .margin(egui::Margin::symmetric(8, 4)),
                    );
                    if r.lost_focus() {
                        // TextEdit releases focus on Enter; don't let that same key open a row.
                        ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                    }
                }
            }
            HeaderBox::Word | HeaderBox::Reference | HeaderBox::Lexicon | HeaderBox::Headword => {
                let shown = self.location_text(tab);
                let hint = match kind {
                    HeaderBox::Word => "Strong's number or word",
                    HeaderBox::Lexicon => "Strong's number or Lemma",
                    HeaderBox::Headword => "Headword",
                    _ => "Go to passage",
                };
                let Some(t) = self.tabs.get_mut(&tab) else {
                    return;
                };
                let mut text = t.edit.clone().unwrap_or(shown);
                let r = ui.add(
                    egui::TextEdit::singleline(&mut text)
                        .id_salt(("refbox", tab))
                        .hint_text(hint)
                        .desired_width(width)
                        .margin(egui::Margin::symmetric(8, 4)),
                );
                if r.has_focus() {
                    t.edit = Some(text.clone());
                }
                if r.lost_focus() {
                    t.edit = None;
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        self.session.workspace.user_focused(tab);
                        let out = self
                            .session
                            .run("tab.go", &json!({ "tab": tab.0, "text": text }));
                        if let Some(t) = self.tabs.get_mut(&tab) {
                            if let Ok(result) = &out {
                                t.reference_passage = result["text"].as_str().and_then(|text| {
                                    obs_scripture::parse_reference(text, &Standard, None)
                                        .ok()
                                        .and_then(|p| p.passages().first().copied())
                                });
                            }
                            t.error = out.err().map(|e| match e {
                                obs_engine::CommandError::Failed { message, .. } => message,
                                e => e.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    fn tab_menu(&mut self, ui: &mut Ui, tab: TabId) {
        let resource = match self.session.workspace.tab(tab).map(|t| &t.kind) {
            Some(TabKind::Resource(id)) => Some(id.clone()),
            _ => None,
        };
        let library = self
            .session
            .workspace
            .tab(tab)
            .is_some_and(|t| t.kind == TabKind::Library);
        let guide = match self.session.workspace.tab(tab).map(|t| &t.kind) {
            Some(TabKind::Guide(g)) if !g.sections().is_empty() => Some(*g),
            _ => None,
        };
        ui.menu_button(RichText::new("…").color(theme::c().muted), |ui| {
            if library && ui.button("Import…").clicked() {
                self.run("library.import", &json!({}));
                ui.close();
            }
            if let Some(g) = guide {
                self.guide_menu(ui, g);
            }
            if let Some(id) = &resource
                && ui.button("About").clicked()
            {
                self.run("resource.about", &json!({ "id": id }));
                ui.close();
            }
            if ui
                .add(egui::Button::new("Close Tab").shortcut_text("Ctrl+W"))
                .clicked()
            {
                self.run("tab.close", &json!({ "tab": tab.0 }));
                ui.close();
            }
        });
    }

    /// Ask for a Layout name (Save layout as…, Rename…).
    fn layout_prompt(&mut self, ctx: &egui::Context) {
        let Some((rename, mut text)) = self.prompt.take() else {
            return;
        };
        let title = if rename.is_some() {
            "Rename layout"
        } else {
            "Save layout as"
        };
        let mut done: Option<bool> = None;
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut text)
                        .id_salt("layout-name")
                        .hint_text("Layout name")
                        .desired_width(260.0),
                );
                r.request_focus();
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = Some(true);
                }
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        done = Some(true);
                    }
                    if ui.button("Cancel").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        done = Some(false);
                    }
                });
            });
        match done {
            None => self.prompt = Some((rename, text)),
            Some(false) => {}
            Some(true) if text.trim().is_empty() => self.prompt = Some((rename, text)),
            Some(true) => match rename {
                None => {
                    self.run("layout.save", &json!({ "name": text.trim() }));
                }
                Some(from) => {
                    self.run("layout.rename", &json!({ "name": from, "to": text.trim() }));
                }
            },
        }
    }

    /// Toasts and the About/Credits window.
    fn overlays(&mut self, ui: &Ui) {
        let ctx = ui.ctx().clone();
        self.note_editor_ui(&ctx);
        self.layout_prompt(&ctx);
        self.remove_prompt(&ctx);
        self.import_chooser(&ctx);
        if let Some((msg, t)) = &self.toast {
            if self.now - t < 4.0 {
                let screen = ctx.content_rect();
                egui::Area::new(egui::Id::unique("toast"))
                    .order(egui::Order::Foreground)
                    .anchor(egui::Align2::CENTER_BOTTOM, vec2(0.0, -48.0))
                    .show(&ctx, |ui| {
                        ui.set_max_width(screen.width() * 0.6);
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.label(RichText::new(msg).color(theme::c().lapis));
                        });
                    });
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            } else {
                self.toast = None;
            }
        }
        if let Some((title, body)) = &self.window {
            let mut open = true;
            egui::Window::new(title.as_str())
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
                .show(&ctx, |ui| {
                    ui.set_max_width(460.0);
                    ui.label(body.as_str());
                });
            if !open {
                self.window = None;
            }
        }
    }
}

const RESOURCE_TYPES: [ResourceType; 6] = [
    ResourceType::Bible,
    ResourceType::Commentary,
    ResourceType::Lexicon,
    ResourceType::Dictionary,
    ResourceType::CrossReferenceSet,
    ResourceType::PeopleAndPlaces,
];

fn type_name(t: ResourceType) -> &'static str {
    match t {
        ResourceType::Bible => "Bibles",
        ResourceType::Commentary => "Commentaries",
        ResourceType::Lexicon => "Lexicons",
        ResourceType::Dictionary => "Dictionaries",
        ResourceType::CrossReferenceSet => "Cross-references",
        ResourceType::PeopleAndPlaces => "People and places",
    }
}

/// "Standard: Genesis 31:55" beside a verse number whose Standard equivalent differs.
fn verse_number_tooltip(ui: &Ui, doc: &ReadingDoc, native: Verse) {
    let Some(map) = doc.verse_map.as_deref() else {
        return;
    };
    let text = match obs_engine::link::to_standard(Some(map), native) {
        obs_engine::link::ToStandard::Verse(std) if std == native => return,
        obs_engine::link::ToStandard::Verse(std) => {
            format!(
                "Standard numbering: {}",
                format_passage(Passage::verse(std), &Standard, RefStyle::Long)
            )
        }
        obs_engine::link::ToStandard::Unmapped => {
            "No Standard equivalent: Link sets do not follow this verse".to_string()
        }
    };
    egui::Tooltip::always_open(
        ui.ctx().clone(),
        ui.layer_id(),
        ui.scope_id().with("verse-number"),
        egui::PopupAnchor::Pointer,
    )
    .show(|ui| ui.label(text));
}

impl State {
    /// The link chip: the Tab's Link set letter in its colour; click to change it.
    fn link_chip(&mut self, ui: &mut Ui, tab: TabId) {
        let link = self.session.workspace.tab(tab).and_then(|t| t.link);
        let txt = RichText::new(link.map_or_else(|| "–".to_string(), String::from))
            .strong()
            .size(11.5)
            .color(Color32::WHITE);
        let resp = ui
            .add(
                egui::Button::new(txt)
                    .fill(theme::link_color(link))
                    .stroke(Stroke::NONE)
                    .corner_radius(11.0)
                    .min_size(vec2(24.0, 22.0)),
            )
            .on_hover_text(
                "Link set: Tabs with the same letter follow one location. Click to change.",
            );
        egui::Popup::menu(&resp).show(|ui| self.link_menu(ui, tab));
    }

    fn link_menu(&mut self, ui: &mut Ui, tab: TabId) {
        let current = self.session.workspace.tab(tab).and_then(|t| t.link);
        let choices = std::iter::once(None).chain(obs_engine::LINK_SETS.iter().copied().map(Some));
        for l in choices {
            let label = l.map_or_else(|| "Not linked".to_string(), |c| format!("Link set {c}"));
            if ui.selectable_label(current == l, label).clicked() {
                let set = l.map(String::from).unwrap_or_default();
                self.run("tab.link", &json!({ "tab": tab.0, "set": set }));
                ui.close();
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn load(path: &std::path::Path) -> Result<Arc<ReadingDoc>, String> {
    ReadingDoc::open(path)
        .map(Arc::new)
        .map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(target_arch = "wasm32")]
fn load(path: &std::path::Path) -> Result<Arc<ReadingDoc>, String> {
    Err(format!(
        "{}: opening files is not supported here",
        path.display()
    ))
}

impl egui_tiles::Behavior<TabId> for State {
    fn pane_ui(&mut self, ui: &mut Ui, _tile: TileId, tab: &mut TabId) -> UiResponse {
        self.tab_body(ui, *tab);
        UiResponse::None
    }

    fn tab_title_for_pane(&mut self, tab: &TabId) -> egui::WidgetText {
        let focused = self.session.workspace.focused() == Some(*tab);
        let title = self
            .session
            .workspace
            .tab(*tab)
            .map(|t| t.title.clone())
            .unwrap_or_default();
        RichText::new(title)
            .size(13.5)
            .color(if focused {
                theme::c().ink
            } else {
                theme::c().muted
            })
            .into()
    }

    fn is_tab_closable(&self, _tiles: &Tiles<TabId>, _tile: TileId) -> bool {
        true
    }

    fn on_tab_button(
        &mut self,
        tiles: &mut Tiles<TabId>,
        tile_id: TileId,
        resp: egui::Response,
    ) -> egui::Response {
        if let Some(Tile::Pane(t)) = tiles.get(tile_id) {
            let t = *t;
            if resp.clicked() {
                self.session.workspace.user_focused(t);
            }
            let linkable = self
                .session
                .workspace
                .tab(t)
                .is_some_and(|tab| tab.kind.has_link_chip());
            if linkable {
                resp.context_menu(|ui| self.link_menu(ui, t));
            }
        }
        resp
    }

    fn top_bar_right_ui(
        &mut self,
        _tiles: &Tiles<TabId>,
        ui: &mut Ui,
        tile_id: TileId,
        _tabs: &egui_tiles::Tabs,
        _scroll: &mut f32,
    ) {
        let panel = self.panel_for_tile(tile_id);
        ui.menu_button(" + ", |ui| {
            let split = ui.input(|i| i.modifiers.command);
            let mut picked: Option<(&'static str, Value)> = None;
            let bibles: Vec<(String, String)> = self
                .session
                .library
                .entries()
                .iter()
                .map(|e| {
                    (
                        e.manifest.resource_id.clone(),
                        format!("{} — {}", e.manifest.abbreviation, e.manifest.title),
                    )
                })
                .collect();
            for (id, label) in bibles {
                if ui.button(label).clicked() {
                    picked = Some(("resource.open", json!({ "id": id })));
                }
            }
            ui.separator();
            for id in [
                "library.open",
                "guide.passage_guide",
                "guide.word_study",
                "tool.search",
                "tool.text_comparison",
                "tool.notes",
                "tool.word_info",
                "tool.cross_references",
            ] {
                let label = self.session.commands().get(id).map_or(id, |c| c.label);
                if ui.button(label).clicked() {
                    picked = Some((id, json!({})));
                }
            }
            ui.label(
                RichText::new("Ctrl/Cmd-click opens a new split")
                    .small()
                    .color(theme::c().muted),
            );
            if let Some((id, mut params)) = picked {
                if let Some(obj) = params.as_object_mut() {
                    obj.insert("split".into(), json!(split));
                    if let Some(p) = panel {
                        obj.insert("panel".into(), json!(p.0));
                    }
                }
                self.run(id, &params);
                ui.close();
            }
        });
    }

    fn tab_bar_height(&self, _style: &egui::Style) -> f32 {
        31.0
    }

    fn gap_width(&self, _style: &egui::Style) -> f32 {
        6.0
    }

    fn tab_title_spacing(&self, _visuals: &egui::Visuals) -> f32 {
        12.0
    }

    fn tab_bar_color(&self, _visuals: &egui::Visuals) -> Color32 {
        theme::c().shell
    }

    fn tab_bg_color(
        &self,
        _visuals: &egui::Visuals,
        _tiles: &Tiles<TabId>,
        _tile_id: TileId,
        state: &egui_tiles::TabState,
    ) -> Color32 {
        if state.active {
            theme::c().surface
        } else {
            Color32::TRANSPARENT
        }
    }

    fn tab_outline_stroke(
        &self,
        _visuals: &egui::Visuals,
        _tiles: &Tiles<TabId>,
        _tile_id: TileId,
        _state: &egui_tiles::TabState,
    ) -> Stroke {
        Stroke::NONE
    }

    fn tab_bar_hline_stroke(&self, _visuals: &egui::Visuals) -> Stroke {
        Stroke::new(1.0, theme::c().rule)
    }

    fn resize_stroke(&self, _style: &egui::Style, state: egui_tiles::ResizeState) -> Stroke {
        match state {
            egui_tiles::ResizeState::Idle => Stroke::new(1.0, theme::c().shell),
            _ => Stroke::new(2.0, theme::c().lapis),
        }
    }

    fn simplification_options(&self) -> egui_tiles::SimplificationOptions {
        egui_tiles::SimplificationOptions {
            all_panes_must_have_tabs: true,
            ..Default::default()
        }
    }
}

impl State {
    /// The Panel a Tabs container shows.
    fn panel_for_tile(&self, tile: TileId) -> Option<PanelId> {
        self.panels.get(&tile).copied()
    }
}

#[cfg(test)]
#[path = "shell_tests.rs"]
mod tests;
