//! The Workspace shell (#35): the engine's Panels and Tabs shown as an egui_tiles dock, each
//! Tab with a variant-B header strip — [link chip] [‹ ›] [box] [title] [⋯] — and the launcher
//! menus. Every menu item, + entry and header button runs a command through [`Session::run`];
//! what the commands change comes back as [`Effect`]s, which the shell applies to the dock.

use std::collections::BTreeMap;
use std::sync::Arc;

use egui::{Color32, RichText, Stroke, Ui, pos2, vec2};
use egui_tiles::{Container, Linear, LinearDir, Tile, TileId, Tiles, Tree, UiResponse};
use obs_engine::{
    Effect, HeaderBox, JobMode, MENUS, MenuItem, PanelId, Placement, Session, TabId, TabKind, Tool,
};
use obs_model::ResourceType;
use obs_scripture::{Passage, RefStyle, Standard, format_passage};
use serde_json::{Value, json};

use crate::doc::ReadingDoc;
use crate::palette::Palette;
use crate::pane::{AtlasTexture, reading_pane};
use crate::theme;
use crate::view::ReadingView;

/// Per-Tab UI state the engine does not need.
#[derive(Default)]
struct TabUi {
    view: Option<ReadingView>,
    /// The header box's text while it is being edited.
    edit: Option<String>,
    filter: String,
    error: Option<String>,
}

/// Everything the dock's behaviour needs, apart from the tree itself.
pub(crate) struct State {
    pub(crate) session: Session,
    tabs: BTreeMap<TabId, TabUi>,
    docs: BTreeMap<String, Arc<ReadingDoc>>,
    atlas: AtlasTexture,
    toast: Option<(String, f64)>,
    window: Option<(String, String)>,
    search: String,
    now: f64,
    /// Ctrl/Cmd is held: opening something splits off a new Panel.
    split_held: bool,
    /// egui_tiles Tabs containers ↔ engine Panels.
    pub(crate) panels: std::collections::HashMap<TileId, PanelId>,
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
                toast: None,
                window: None,
                search: String::new(),
                now: 0.0,
                split_held: false,
                panels: std::collections::HashMap::new(),
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

    /// The default arrangement: the first Bible in one Panel; the Library and Word Info in a
    /// Panel to its right.
    pub fn reset_layout(&mut self) {
        self.st.session.workspace.clear();
        self.apply_effects();
        self.tree = Tree::empty(egui::Id::unique("workspace"));
        self.st.panels.clear();
        let first = self
            .st
            .session
            .library
            .of_type(ResourceType::Bible)
            .next()
            .map(|e| e.manifest.resource_id.clone());
        if let Some(id) = first {
            self.run("resource.open", &json!({ "id": id }));
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
                    if let Some(view) = self.st.tabs.get_mut(&tab).and_then(|t| t.view.as_mut())
                        && !view.jump_to(verse)
                    {
                        self.st.toast(format!("{verse} is not in this Bible"));
                    }
                }
                Effect::ResetLayout => self.reset_layout(),
                Effect::Message(m) => self.st.toast(m),
                Effect::Show { title, body } => self.st.window = Some((title, body)),
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
    }

    /// Draw the whole window.
    pub fn ui(&mut self, ui: &mut Ui) {
        self.st.now = ui.input(|i| i.time);
        self.st.split_held = ui.input(|i| i.modifiers.command);
        self.shortcuts(ui);
        egui::Panel::top("menus")
            .frame(
                egui::Frame::new()
                    .fill(theme::SURFACE)
                    .inner_margin(egui::Margin::symmetric(14, 6)),
            )
            .show(ui, |ui| self.menu_bar(ui));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::SHELL)
                    .inner_margin(egui::Margin::same(8)),
            )
            .show(ui, |ui| {
                if self.tree.is_empty() {
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            RichText::new(
                                "Nothing is open. Use the Library menu, or drop a .obr file here.",
                            )
                            .color(theme::MUTED),
                        );
                    });
                } else {
                    self.tree.ui(&mut self.st, ui);
                }
            });
        self.apply_effects();
        self.st.overlays(ui);
    }

    fn shortcuts(&mut self, ui: &mut Ui) {
        let focused = self.st.session.workspace.focused();
        let mut run: Vec<(&'static str, Value)> = Vec::new();
        for spec in self.st.session.commands().iter() {
            let Some(sc) = spec.shortcut.and_then(parse_shortcut) else {
                continue;
            };
            let needs_tab = spec.params.iter().any(|p| p.name == "tab" && p.required);
            let other_required = spec.params.iter().any(|p| p.required && p.name != "tab");
            if other_required || (needs_tab && focused.is_none()) {
                continue;
            }
            if ui.input_mut(|i| i.consume_shortcut(&sc)) {
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
    }

    fn menu_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Open Bible")
                    .font(theme::display(19.0))
                    .color(theme::INK),
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
                    "view.dark_page" if self.st.session.dark_page => "✓ Dark page".into(),
                    "view.verse_numbers" if self.st.session.text.verse_numbers => {
                        "✓ Verse numbers".into()
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
                    ui.label(RichText::new("No Resources yet").color(theme::MUTED));
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
        "library.import" => "Import arrives with #47",
        "layout.save" => "Saved Layouts arrive with #37",
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
        if let Some(TabKind::Resource(id)) = kind {
            match self.doc(&id) {
                Ok(doc) => {
                    let mut view = ReadingView::new(Arc::clone(&doc), self.session.text);
                    // Start where the focused reading Tab is, else at the beginning.
                    let at = self
                        .session
                        .workspace
                        .tabs()
                        .filter(|t| t.id != tab && t.kind.is_reading())
                        .find_map(|t| t.history.current());
                    if let Some(v) = at
                        && view.jump_to(v)
                    {
                        self.session.workspace.scrolled(tab, v);
                    }
                    if let Some(t) = self.session.workspace.tab_mut(tab) {
                        t.versification = Some(Arc::new(doc.versification.clone()));
                    }
                    ui_state.view = Some(view);
                }
                Err(e) => ui_state.error = Some(e),
            }
        }
        self.tabs.insert(tab, ui_state);
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
        // The body gets exactly the rest of the pane, whatever the header did (#13 finding).
        let rect = egui::Rect::from_min_max(pos2(pane.left(), ui.cursor().top()), pane.max);
        let mut body = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt(("body", tab)));
        body.set_clip_rect(rect.intersect(ui.clip_rect()));
        match kind {
            TabKind::Resource(_) => self.reading_body(&mut body, tab),
            TabKind::Library => self.library_body(&mut body, tab),
            TabKind::Guide(g) => placeholder(&mut body, g.title(), guide_ticket(g)),
            TabKind::Tool(t) => placeholder(&mut body, t.title(), tool_ticket(t)),
        }
    }

    fn reading_body(&mut self, ui: &mut Ui, tab: TabId) {
        let palette = if self.session.dark_page {
            Palette::DARK
        } else {
            Palette::PAPER
        };
        let text = self.session.text;
        let Some(t) = self.tabs.get_mut(&tab) else {
            return;
        };
        let Some(view) = t.view.as_mut() else {
            let msg = t.error.clone().unwrap_or_default();
            ui.centered_and_justified(|ui| ui.colored_label(theme::WARN, msg));
            return;
        };
        view.set_settings(text);
        reading_pane(ui, view, &mut self.atlas, &palette);
        if let Some(at) = view.current_location()
            && self
                .session
                .workspace
                .tab(tab)
                .is_some_and(|t| t.history.current() != Some(at))
        {
            self.session.workspace.scrolled(tab, at);
        }
    }

    fn library_body(&mut self, ui: &mut Ui, tab: TabId) {
        let filter = self
            .tabs
            .get(&tab)
            .map(|t| t.filter.to_lowercase())
            .unwrap_or_default();
        let mut open: Option<String> = None;
        egui::Frame::new()
            .fill(theme::FIELD)
            .inner_margin(egui::Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let lib = &self.session.library;
                        if lib.entries().is_empty() {
                            ui.label(
                                RichText::new(
                                    "Your library is empty. Drop a .obr file on the window, or \
                                     make one with `obs-cli import`.",
                                )
                                .color(theme::MUTED),
                            );
                        }
                        for ty in RESOURCE_TYPES {
                            let rows: Vec<_> = lib
                                .of_type(ty)
                                .filter(|e| {
                                    filter.is_empty()
                                        || e.manifest.title.to_lowercase().contains(&filter)
                                        || e.manifest.abbreviation.to_lowercase().contains(&filter)
                                })
                                .collect();
                            if rows.is_empty() {
                                continue;
                            }
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(type_name(ty).to_uppercase())
                                    .size(10.5)
                                    .strong()
                                    .color(theme::MUTED),
                            );
                            for e in rows {
                                let m = &e.manifest;
                                let r = ui.add(
                                    egui::Button::new(
                                        RichText::new(format!("{}  {}", m.abbreviation, m.title))
                                            .color(theme::INK),
                                    )
                                    .frame(false),
                                );
                                let r = r.on_hover_text(format!(
                                    "{} · {} · {}",
                                    m.language, m.license, m.resource_id
                                ));
                                if r.clicked() {
                                    open = Some(m.resource_id.clone());
                                }
                            }
                        }
                    });
            });
        if let Some(id) = open {
            let split = self.split_held;
            self.run("resource.open", &json!({ "id": id, "split": split }));
        }
    }

    fn header(&mut self, ui: &mut Ui, tab: TabId, kind: &TabKind) {
        let focused = self.session.workspace.focused() == Some(tab);
        let fill = if focused {
            theme::FIELD
        } else {
            theme::SURFACE
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
                        link_chip(ui);
                    }
                    let passage = kind.header_box() == HeaderBox::Reference;
                    if passage {
                        self.history_buttons(ui, tab);
                    }
                    self.header_box(
                        ui,
                        tab,
                        kind.header_box(),
                        if narrow { 110.0 } else { 140.0 },
                    );
                    let title = self.title(tab, kind);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.tab_menu(ui, tab);
                        if !narrow {
                            ui.add(
                                egui::Label::new(RichText::new(title).small().color(theme::MUTED))
                                    .truncate(),
                            );
                        }
                    });
                });
                if let Some(e) = self.tabs.get(&tab).and_then(|t| t.error.clone()) {
                    ui.label(RichText::new(e).small().color(theme::WARN));
                }
            });
        let r = frame.response.rect;
        ui.painter().rect_filled(
            egui::Rect::from_min_size(r.min, vec2(if focused { 4.0 } else { 2.0 }, r.height())),
            0.0,
            theme::UNLINKED,
        );
        ui.painter()
            .hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, theme::RULE));
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

    fn history_buttons(&mut self, ui: &mut Ui, tab: TabId) {
        let (back, fwd) = self.session.workspace.tab(tab).map_or((false, false), |t| {
            (t.history.can_go_back(), t.history.can_go_forward())
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
            HeaderBox::FollowsHover => {
                ui.label(RichText::new("Follows hover").small().color(theme::MUTED));
            }
            HeaderBox::Filter => {
                if let Some(t) = self.tabs.get_mut(&tab) {
                    ui.add(
                        egui::TextEdit::singleline(&mut t.filter)
                            .hint_text("Filter")
                            .desired_width(width)
                            .margin(egui::Margin::symmetric(8, 4)),
                    );
                }
            }
            HeaderBox::Word | HeaderBox::Reference => {
                let shown = self.location_text(tab);
                let hint = if kind == HeaderBox::Word {
                    "Strong's number or word"
                } else {
                    "Go to passage"
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
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) && kind == HeaderBox::Reference
                    {
                        self.session.workspace.user_focused(tab);
                        let out = self
                            .session
                            .run("tab.go", &json!({ "tab": tab.0, "text": text }));
                        if let Some(t) = self.tabs.get_mut(&tab) {
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
        ui.menu_button(RichText::new("…").color(theme::MUTED), |ui| {
            if ui
                .add(egui::Button::new("Close Tab").shortcut_text("Ctrl+W"))
                .clicked()
            {
                self.run("tab.close", &json!({ "tab": tab.0 }));
                ui.close();
            }
        });
    }

    /// Toasts and the About/Credits window.
    fn overlays(&mut self, ui: &Ui) {
        let ctx = ui.ctx().clone();
        if let Some((msg, t)) = &self.toast {
            if self.now - t < 4.0 {
                let screen = ctx.content_rect();
                egui::Area::new(egui::Id::unique("toast"))
                    .order(egui::Order::Foreground)
                    .anchor(egui::Align2::CENTER_BOTTOM, vec2(0.0, -48.0))
                    .show(&ctx, |ui| {
                        ui.set_max_width(screen.width() * 0.6);
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.label(RichText::new(msg).color(theme::LAPIS));
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

fn tool_ticket(t: Tool) -> &'static str {
    match t {
        Tool::Search => "#59",
        Tool::TextComparison => "#52",
        Tool::Notes => "#64",
        Tool::WordInfo => "#50",
        Tool::CrossReferences => "#51",
    }
}

fn guide_ticket(g: obs_engine::Guide) -> &'static str {
    match g {
        obs_engine::Guide::PassageGuide => "#53",
        obs_engine::Guide::WordStudy => "#55",
    }
}

fn placeholder(ui: &mut Ui, title: &str, ticket: &str) {
    egui::Frame::new().fill(theme::FIELD).show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        ui.centered_and_justified(|ui| {
            ui.label(RichText::new(format!("{title} arrives with {ticket}")).color(theme::MUTED));
        });
    });
}

/// The link-chip slot. Link sets arrive with #36; until then every Tab is unlinked.
fn link_chip(ui: &mut Ui) {
    let txt = RichText::new("–").strong().size(11.5).color(Color32::WHITE);
    ui.add(
        egui::Button::new(txt)
            .fill(theme::UNLINKED)
            .stroke(Stroke::NONE)
            .corner_radius(11.0)
            .min_size(vec2(24.0, 22.0)),
    )
    .on_hover_text("Link set: not linked. Link sets arrive with #36.");
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
            .color(if focused { theme::INK } else { theme::MUTED })
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
        if resp.clicked()
            && let Some(Tile::Pane(t)) = tiles.get(tile_id)
        {
            self.session.workspace.user_focused(*t);
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
                    .color(theme::MUTED),
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
        theme::SHELL
    }

    fn tab_bg_color(
        &self,
        _visuals: &egui::Visuals,
        _tiles: &Tiles<TabId>,
        _tile_id: TileId,
        state: &egui_tiles::TabState,
    ) -> Color32 {
        if state.active {
            theme::SURFACE
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
        Stroke::new(1.0, theme::RULE)
    }

    fn resize_stroke(&self, _style: &egui::Style, state: egui_tiles::ResizeState) -> Stroke {
        match state {
            egui_tiles::ResizeState::Idle => Stroke::new(1.0, theme::SHELL),
            _ => Stroke::new(2.0, theme::LAPIS),
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
