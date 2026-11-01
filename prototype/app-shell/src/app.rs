//! PROTOTYPE app shell for issue #13: "What should the main window feel like?"
//!
//! Three structurally different chrome variants over the same Workspace model, switchable
//! from the floating bar at the bottom (or ← / → when no text field has focus):
//!   A — Toolbar: one global reference box + history + link chip for the focused Tab.
//!   B — Panel headers: every Tab carries its own reference box / history / link chip (Logos-like).
//!   C — Focus + palette: reading Panels only; study Tools in a right drawer; Ctrl+L palette.

use crate::data::{HebWordInfo, Library, ResId};
use crate::doc::{DocView, Hit, Snippets, Stats};
use crate::present::{TextSettings, INK, LINK};
use crate::refs::{self, Parsed, VRef};
use crate::text::Fonts;
use egui::{pos2, vec2, Color32, RichText, Ui};
use egui_tiles::{Container, Tile, TileId, Tiles, Tree, UiResponse};
use std::collections::BTreeMap;

pub type TabId = usize;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pane(pub TabId);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TabKind {
    Doc(ResId),
    CrossRefs,
    WordInfo,
}

pub struct Tab {
    pub kind: TabKind,
    pub link: Option<char>,
    pub doc: DocView,
    pub loc: Option<VRef>,
    refbox: String,
    refbox_editing: bool,
    back: Vec<VRef>,
    fwd: Vec<VRef>,
}

#[derive(Clone, Debug)]
pub enum WordSel {
    Strong { strong: String, word: String },
    Heb(usize),
}

const VARIANTS: [&str; 3] = ["A — Toolbar", "B — Panel headers", "C — Focus + palette"];
const LINKS: [char; 3] = ['A', 'B', 'C'];

fn link_color(c: Option<char>) -> Color32 {
    match c {
        Some('A') => Color32::from_rgb(214, 120, 40),
        Some('B') => Color32::from_rgb(30, 140, 140),
        Some('C') => Color32::from_rgb(130, 80, 180),
        _ => Color32::from_gray(160),
    }
}

struct SavedLayout {
    name: String,
    variant: usize,
    tree: Tree<Pane>,
    links: BTreeMap<TabId, Option<char>>,
}

pub struct State {
    lib: Library,
    fonts: Fonts,
    settings: TextSettings,
    tabs: BTreeMap<TabId, Tab>,
    next_id: TabId,
    focused: Option<TabId>,
    word: Option<WordSel>,
    snippets: Snippets,
    toast: Option<(String, f64)>,
    stats: Stats,
    last_stats: Stats,
    raster_budget: u32,
    variant: usize,
    add_requests: Vec<(TileId, TabKind)>,
    nav: Vec<Nav>,
    hover_popup: Option<(Hit, egui::Pos2)>,
    palette: Option<String>,
    global_ref: String,
    global_ref_editing: bool,
    drawer: [TabId; 2],
    drawer_comm: ResId,
    frame_ms: f32,
}

enum Nav {
    /// user scrolled a Tab: followers follow, no history, no flash
    Follow(TabId, VRef),
    /// explicit jump (reference box, click): whole link set incl. self, with history + flash
    Jump(TabId, VRef),
}

pub struct App {
    shot: Option<(usize, String, String, u32)>,
    st: State,
    trees: Vec<Tree<Pane>>,
    layouts: Vec<SavedLayout>,
}

impl State {
    fn new_tab(&mut self, kind: TabKind, link: Option<char>) -> TabId {
        let id = self.next_id;
        self.next_id += 1;
        self.tabs.insert(
            id,
            Tab { kind, link, doc: DocView::new(), loc: None, refbox: String::new(), refbox_editing: false, back: vec![], fwd: vec![] },
        );
        id
    }

    fn title(&self, id: TabId) -> String {
        match self.tabs[&id].kind {
            TabKind::Doc(r) => self.lib.get(r).abbrev.clone(),
            TabKind::CrossRefs => "Cross-references".into(),
            TabKind::WordInfo => "Word Info".into(),
        }
    }

    fn display_loc(&self, id: TabId) -> String {
        let t = &self.tabs[&id];
        let Some(l) = t.loc else { return String::new() };
        match t.kind {
            TabKind::Doc(ResId::Tahot) => self.lib.tahot.std_to_native.get(&l).copied().unwrap_or(l).display(),
            _ => l.display(),
        }
    }

    fn toast(&mut self, s: impl Into<String>, now: f64) {
        self.toast = Some((s.into(), now));
    }

    /// Parse a typed reference in the context of a Tab (its own numbering for Bibles).
    fn submit_ref(&mut self, id: TabId, text: &str, now: f64) {
        let t = &self.tabs[&id];
        let ctx = t.loc.map(|l| match t.kind {
            TabKind::Doc(ResId::Tahot) => self.lib.tahot.std_to_native.get(&l).copied().unwrap_or(l),
            _ => l,
        });
        match refs::parse(text, ctx) {
            Some(Parsed::Verse(r)) | Some(Parsed::Chapter(r)) => {
                if r.book != 0 {
                    self.toast(format!("{} — the prototype library only has Genesis", r.display()), now);
                    return;
                }
                let std = match t.kind {
                    TabKind::Doc(ResId::Tahot) => self.lib.tahot.native_to_std.get(&r).copied().unwrap_or(r),
                    _ => r,
                };
                self.nav.push(Nav::Jump(id, std));
            }
            None => self.toast(format!("Can't read “{text}” as a Bible reference"), now),
        }
    }

    fn goto_tab(&mut self, id: TabId, r: VRef, flash: bool, now: f64) {
        let lib = &self.lib;
        let t = self.tabs.get_mut(&id).unwrap();
        match t.kind {
            TabKind::Doc(res) => {
                t.doc.goto(lib.get(res), r, now, flash);
                t.loc = t.doc.location;
            }
            TabKind::CrossRefs => t.loc = Some(r),
            TabKind::WordInfo => {}
        }
    }

    fn process_nav(&mut self, now: f64) {
        let navs = std::mem::take(&mut self.nav);
        for n in navs {
            let (from, r, jump) = match n {
                Nav::Follow(f, r) => (f, r, false),
                Nav::Jump(f, r) => (f, r, true),
            };
            let link = self.tabs[&from].link;
            if jump {
                let t = self.tabs.get_mut(&from).unwrap();
                if let Some(old) = t.loc {
                    if old != r {
                        t.back.push(old);
                        t.fwd.clear();
                    }
                }
                self.goto_tab(from, r, true, now);
            }
            let members: Vec<TabId> = self.tabs.iter().filter(|(k, t)| **k != from && link.is_some() && t.link == link).map(|(k, _)| *k).collect();
            for m in members {
                self.goto_tab(m, r, jump, now);
            }
        }
    }

    fn history(&mut self, id: TabId, back: bool, now: f64) {
        let t = self.tabs.get_mut(&id).unwrap();
        let (from, to) = if back { (&mut t.back, &mut t.fwd) } else { (&mut t.fwd, &mut t.back) };
        if let Some(r) = from.pop() {
            if let Some(cur) = t.loc {
                to.push(cur);
            }
            self.goto_tab(id, r, true, now);
            self.nav.push(Nav::Follow(id, r));
        }
    }

    // ------------------------------------------------------------------ chips + boxes

    fn link_chip(&mut self, ui: &mut Ui, id: TabId) {
        let link = self.tabs[&id].link;
        let txt = RichText::new(format!(" {} ", link.map(|c| c.to_string()).unwrap_or("–".into()))).strong().color(Color32::WHITE);
        let resp = ui.add(egui::Button::new(txt).fill(link_color(link)).corner_radius(9.0)).on_hover_text("Link set — Tabs with the same letter share a location. Click to change.");
        egui::Popup::menu(&resp).show(|ui| {
            for l in [None, Some('A'), Some('B'), Some('C')] {
                let label = l.map(|c| format!("Link set {c}")).unwrap_or("Not linked".into());
                if ui.selectable_label(self.tabs[&id].link == l, label).clicked() {
                    self.tabs.get_mut(&id).unwrap().link = l;
                    ui.close();
                }
            }
        });
    }

    fn nav_buttons(&mut self, ui: &mut Ui, id: TabId, now: f64) {
        let (b, f) = (self.tabs[&id].back.len(), self.tabs[&id].fwd.len());
        if ui.add_enabled(b > 0, egui::Button::new("‹").small()).on_hover_text("Back").clicked() {
            self.history(id, true, now);
        }
        if ui.add_enabled(f > 0, egui::Button::new("›").small()).on_hover_text("Forward").clicked() {
            self.history(id, false, now);
        }
    }

    fn tab_refbox(&mut self, ui: &mut Ui, id: TabId, width: f32, now: f64) {
        let shown = self.display_loc(id);
        let t = self.tabs.get_mut(&id).unwrap();
        if !t.refbox_editing {
            t.refbox = shown;
        }
        let r = ui.add(egui::TextEdit::singleline(&mut t.refbox).desired_width(width).hint_text("Go to reference"));
        if r.gained_focus() {
            t.refbox_editing = true;
            self.focused = Some(id);
        }
        if r.lost_focus() {
            t.refbox_editing = false;
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let s = t.refbox.clone();
                self.submit_ref(id, &s, now);
            }
        }
    }

    // ------------------------------------------------------------------ tab bodies

    fn tab_body(&mut self, ui: &mut Ui, id: TabId, now: f64) {
        let kind = self.tabs[&id].kind;
        let pane_rect = ui.max_rect();
        if self.variant == 1 {
            // B: per-Tab header
            egui::Frame::new().fill(Color32::from_gray(244)).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    self.link_chip(ui, id);
                    if let TabKind::Doc(r) = kind {
                        self.nav_buttons(ui, id, now);
                        self.tab_refbox(ui, id, 130.0, now);
                        ui.add(egui::Label::new(RichText::new(&self.lib.get(r).title).weak().small()).truncate());
                    } else {
                        ui.label(RichText::new(self.title(id)).weak());
                        if kind == TabKind::CrossRefs {
                            ui.label(RichText::new(self.display_loc(id)).strong());
                        }
                    }
                });
            });
        }
        // body gets exactly the rest of the pane, whatever the header did to max_rect
        let rect = egui::Rect::from_min_max(pos2(pane_rect.left(), ui.cursor().top()), pane_rect.right_bottom());
        let mut body = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt(("body", id)));
        body.set_clip_rect(rect.intersect(ui.clip_rect()));
        let ui = &mut body;
        match kind {
            TabKind::Doc(r) => self.doc_body(ui, id, r),
            TabKind::CrossRefs => self.xref_body(ui, id, now),
            TabKind::WordInfo => {
                egui::ScrollArea::vertical().id_salt(("wi", id)).auto_shrink([false, false]).show(ui, |ui| self.word_body(ui));
            }
        }
    }

    fn doc_body(&mut self, ui: &mut Ui, id: TabId, r: ResId) {
        let focused = self.focused == Some(id);
        let res = self.lib.get(r);
        let t = self.tabs.get_mut(&id).unwrap();
        let before = t.doc.location;
        let out = t.doc.show(ui, res, &self.fonts, &self.settings, focused, &mut self.stats, &mut self.raster_budget);
        t.loc = t.doc.location;
        if out.interacted {
            self.focused = Some(id);
        }
        if out.user_moved && t.doc.location != before {
            if let Some(l) = t.doc.location {
                self.nav.push(Nav::Follow(id, l));
            }
        }
        if let Some(c) = out.copy {
            ui.ctx().copy_text(c);
            self.toast = Some(("Copied with reference".into(), ui.input(|i| i.time)));
        }
        match &out.hover {
            Some(Hit::Strong { strong, word }) => self.word = Some(WordSel::Strong { strong: strong.clone(), word: word.clone() }),
            Some(Hit::Heb(i)) => self.word = Some(WordSel::Heb(*i)),
            _ => {}
        }
        if let Some(h @ (Hit::Ref(_) | Hit::Note(_))) = &out.hover {
            if let Some(p) = ui.input(|i| i.pointer.hover_pos()) {
                self.hover_popup = Some((h.clone(), p));
            }
        }
        if let Some(Hit::Ref(osis)) = &out.clicked {
            if let Some(v) = VRef::from_osis(osis.split('-').next().unwrap_or("")) {
                if v.book == 0 {
                    self.nav.push(Nav::Jump(id, v));
                } else {
                    let now = ui.input(|i| i.time);
                    self.toast(format!("{} is outside the prototype library (preview on hover)", v.display()), now);
                }
            }
        }
    }

    fn xref_body(&mut self, ui: &mut Ui, id: TabId, now: f64) {
        let loc = self.tabs[&id].loc;
        egui::Frame::new().inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
            let Some(loc) = loc else {
                ui.label("Not linked to a location.");
                return;
            };
            if self.variant != 1 {
                ui.label(RichText::new(format!("Cross-references · {}", loc.display())).strong());
            }
            let list = self.lib.xrefs.refs.get(&loc).cloned().unwrap_or_default();
            if list.is_empty() {
                ui.label(RichText::new("None for this verse.").weak());
            }
            egui::ScrollArea::vertical().id_salt(("xr", id)).auto_shrink([false, false]).show(ui, |ui| {
                let w = ui.available_width() - 8.0;
                for (target, votes) in list {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let r = ui.add(egui::Label::new(RichText::new(refs::osis_range_short(&target)).color(Color32::from_rgb(30, 80, 170)).strong()).sense(egui::Sense::click()));
                        if r.clicked() {
                            match VRef::from_osis(target.split('-').next().unwrap_or("")) {
                                Some(v) if v.book == 0 => self.nav.push(Nav::Jump(id, v)),
                                Some(v) => self.toast(format!("{} is outside the prototype library", v.display()), now),
                                None => {}
                            }
                        }
                        r.on_hover_cursor(egui::CursorIcon::PointingHand);
                        ui.label(RichText::new(format!("{votes} votes")).weak().small());
                    });
                    if let Some(t) = self.lib.kjv_plain(&target) {
                        self.snippets.show(ui, &self.fonts, &self.settings, &t, self.settings.size * 0.82, INK, false, w);
                    }
                }
                ui.add_space(8.0);
                ui.label(RichText::new(&self.lib.xrefs.attribution).weak().small());
            });
        });
    }

    fn word_body(&mut self, ui: &mut Ui) {
        egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
            let w = ui.available_width();
            let s = self.settings;
            match self.word.clone() {
                None => {
                    ui.label(RichText::new("Hover a word in the KJV or the Hebrew text.").weak());
                }
                Some(WordSel::Heb(i)) => {
                    let wd: HebWordInfo = self.lib.tahot.words[i].clone();
                    self.snippets.show(ui, &self.fonts, &s, &wd.heb, s.size * 1.9, INK, true, w);
                    ui.label(RichText::new(wd.translit.replace(['.', '/'], "")).italics());
                    ui.label(RichText::new(&wd.gloss).strong());
                    ui.separator();
                    field(ui, "Lemma");
                    self.snippets.show(ui, &self.fonts, &s, &wd.lemma, s.size * 1.4, INK, true, w);
                    ui.label(format!("{} — {}", wd.lemma_strong, wd.lemma_gloss));
                    field(ui, "Morphology");
                    ui.label(&wd.morph_en);
                    ui.label(RichText::new(&wd.morph).weak().small().monospace());
                    field(ui, "Strong's (all parts)");
                    ui.label(RichText::new(&wd.strongs).monospace());
                    field(ui, "Reference");
                    ui.label(VRef::from_osis(&wd.reference).map(|r| r.display()).unwrap_or_default());
                }
                Some(WordSel::Strong { strong, word }) => {
                    ui.label(RichText::new(&word).size(22.0).strong());
                    ui.label(RichText::new(format!("KJV · Strong's {strong}")).weak());
                    ui.separator();
                    match self.lib.lexicon.get(&strong).cloned() {
                        Some(lx) => {
                            field(ui, "Lemma");
                            self.snippets.show(ui, &self.fonts, &s, &lx.lemma, s.size * 1.6, INK, true, w);
                            ui.label(format!("{} — {}", lx.strong, lx.gloss));
                            field(ui, "Occurrences");
                            ui.label(format!("{} in Genesis (TAHOT)", lx.count));
                        }
                        None => {
                            ui.label(RichText::new("No lexicon entry in the prototype data.").weak());
                        }
                    }
                }
            }
            ui.add_space(10.0);
            ui.add_enabled(false, egui::Button::new("Word Study ▸"));
        });
    }

    fn hover_popup_ui(&mut self, ctx: &egui::Context) {
        let Some((hit, p)) = self.hover_popup.take() else { return };
        egui::Area::new(egui::Id::new("hover-popup")).order(egui::Order::Tooltip).fixed_pos(p + vec2(14.0, 16.0)).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(380.0);
                let s = self.settings;
                match hit {
                    Hit::Ref(osis) => {
                        ui.label(RichText::new(refs::osis_range_short(&osis)).strong());
                        if let Some(t) = self.lib.kjv_plain(&osis) {
                            self.snippets.show(ui, &self.fonts, &s, &t, s.size * 0.85, INK, false, 360.0);
                        }
                        ui.label(RichText::new("KJV · click to go there (Genesis only)").weak().small());
                    }
                    Hit::Note(t) => {
                        ui.label(RichText::new("Translator's note").weak().small());
                        self.snippets.show(ui, &self.fonts, &s, &t, s.size * 0.85, INK, false, 360.0);
                    }
                    _ => {}
                }
            });
        });
    }

    fn settings_menu(&mut self, ui: &mut Ui) {
        ui.menu_button("Text", |ui| {
            ui.add(egui::Slider::new(&mut self.settings.size, 12.0..=30.0).text("Font size"));
            ui.add(egui::Slider::new(&mut self.settings.hebrew_scale, 1.0..=2.0).text("Hebrew scale"));
            ui.add(egui::Slider::new(&mut self.settings.line_spacing, 1.0..=2.0).text("Line spacing"));
            ui.checkbox(&mut self.settings.verse_numbers, "Verse numbers");
        });
    }

    fn status_bar(&mut self, ui: &mut Ui, visible: &[TabId]) {
        ui.horizontal(|ui| {
            for l in LINKS {
                let members: Vec<String> = self
                    .tabs
                    .iter()
                    .filter(|(k, t)| t.link == Some(l) && visible.contains(k))
                    .map(|(k, _)| self.title(*k))
                    .collect();
                if members.is_empty() {
                    continue;
                }
                let loc = self.tabs.iter().find(|(_, t)| t.link == Some(l) && t.loc.is_some()).and_then(|(_, t)| t.loc);
                ui.label(RichText::new(format!(" {l} ")).color(Color32::WHITE).background_color(link_color(Some(l))));
                ui.label(format!("{} — {}", loc.map(|r| r.short()).unwrap_or_default(), members.join(", ")));
                ui.separator();
            }
        });
        ui.horizontal(|ui| {
            {
                let st = &self.last_stats;
                ui.label(
                    RichText::new(format!(
                        "frame {:.1} ms · layout {}× {:.1} ms · raster {}× {:.1} ms · {} block textures {:.1} MB",
                        self.frame_ms,
                        st.layouts,
                        st.layout_ms,
                        st.rasters,
                        st.raster_ms,
                        st.textures,
                        st.texture_bytes as f32 / 1e6
                    ))
                    .weak()
                    .small()
                    .monospace(),
                );
            }
        });
    }
}

fn field(ui: &mut Ui, name: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(name).small().weak());
}

// ---------------------------------------------------------------------- egui_tiles behavior

struct Shell<'a> {
    st: &'a mut State,
    now: f64,
}

impl egui_tiles::Behavior<Pane> for Shell<'_> {
    fn pane_ui(&mut self, ui: &mut Ui, _tile: TileId, pane: &mut Pane) -> UiResponse {
        self.st.tab_body(ui, pane.0, self.now);
        UiResponse::None
    }

    fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
        let t = &self.st.tabs[&pane.0];
        let mut job = egui::text::LayoutJob::default();
        if let Some(l) = t.link {
            job.append(
                &format!(" {l} "),
                0.0,
                egui::TextFormat { color: Color32::WHITE, background: link_color(Some(l)), font_id: egui::FontId::proportional(11.0), ..Default::default() },
            );
        }
        let focused = self.st.focused == Some(pane.0);
        job.append(
            &self.st.title(pane.0),
            6.0,
            egui::TextFormat {
                color: if focused { Color32::BLACK } else { Color32::from_gray(80) },
                font_id: egui::FontId::proportional(13.5),
                ..Default::default()
            },
        );
        job.into()
    }

    fn is_tab_closable(&self, _tiles: &Tiles<Pane>, _tile: TileId) -> bool {
        true
    }

    fn on_tab_button(&mut self, tiles: &mut Tiles<Pane>, tile_id: TileId, resp: egui::Response) -> egui::Response {
        if let Some(Tile::Pane(p)) = tiles.get(tile_id) {
            let id = p.0;
            if resp.clicked() {
                self.st.focused = Some(id);
            }
            resp.context_menu(|ui| {
                ui.label(RichText::new("Link set").weak());
                for l in [None, Some('A'), Some('B'), Some('C')] {
                    let label = l.map(|c| format!("Link set {c}")).unwrap_or("Not linked".into());
                    if ui.selectable_label(self.st.tabs[&id].link == l, label).clicked() {
                        self.st.tabs.get_mut(&id).unwrap().link = l;
                        ui.close();
                    }
                }
            });
        }
        resp
    }

    fn top_bar_right_ui(&mut self, _tiles: &Tiles<Pane>, ui: &mut Ui, tile_id: TileId, _tabs: &egui_tiles::Tabs, _scroll: &mut f32) {
        ui.menu_button(" + ", |ui| {
            for (label, kind) in [
                ("KJV with Strong's", TabKind::Doc(ResId::Kjv)),
                ("Hebrew OT (TAHOT)", TabKind::Doc(ResId::Tahot)),
                ("JFB commentary", TabKind::Doc(ResId::Jfb)),
                ("Matthew Henry Concise", TabKind::Doc(ResId::Mhcc)),
                ("Cross-references", TabKind::CrossRefs),
                ("Word Info", TabKind::WordInfo),
            ] {
                if ui.button(label).clicked() {
                    self.st.add_requests.push((tile_id, kind));
                    ui.close();
                }
            }
        });
    }

    fn tab_bar_height(&self, _style: &egui::Style) -> f32 {
        28.0
    }

    fn gap_width(&self, _style: &egui::Style) -> f32 {
        3.0
    }

    fn simplification_options(&self) -> egui_tiles::SimplificationOptions {
        egui_tiles::SimplificationOptions { all_panes_must_have_tabs: true, ..Default::default() }
    }
}

// ---------------------------------------------------------------------- trees

fn tree_main(ids: &BTreeMap<&str, TabId>, name: &str) -> Tree<Pane> {
    let mut tiles = Tiles::default();
    let p = |tiles: &mut Tiles<Pane>, k: &str| tiles.insert_pane(Pane(ids[k]));
    let kjv = p(&mut tiles, "kjv");
    let heb = p(&mut tiles, "tahot");
    let jfb = p(&mut tiles, "jfb");
    let mhcc = p(&mut tiles, "mhcc");
    let xr = p(&mut tiles, "xref");
    let wi = p(&mut tiles, "word");
    let t_kjv = tiles.insert_tab_tile(vec![kjv]);
    let t_heb = tiles.insert_tab_tile(vec![heb]);
    let t_comm = tiles.insert_tab_tile(vec![jfb, mhcc]);
    let t_tools = tiles.insert_tab_tile(vec![xr, wi]);
    let right = tiles.insert_vertical_tile(vec![t_comm, t_tools]);
    let root = tiles.insert_horizontal_tile(vec![t_kjv, t_heb, right]);
    if let Some(Tile::Container(Container::Linear(l))) = tiles.get_mut(root) {
        l.shares.set_share(t_kjv, 1.15);
        l.shares.set_share(t_heb, 1.0);
        l.shares.set_share(right, 1.05);
    }
    Tree::new(egui::Id::new(name), root, tiles)
}

fn tree_focus(ids: &BTreeMap<&str, TabId>) -> Tree<Pane> {
    let mut tiles = Tiles::default();
    let kjv = tiles.insert_pane(Pane(ids["kjv"]));
    let heb = tiles.insert_pane(Pane(ids["tahot"]));
    let t_kjv = tiles.insert_tab_tile(vec![kjv]);
    let t_heb = tiles.insert_tab_tile(vec![heb]);
    let root = tiles.insert_horizontal_tile(vec![t_kjv, t_heb]);
    Tree::new(egui::Id::new("focus"), root, tiles)
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> App {
        cc.egui_ctx.set_theme(egui::Theme::Light);
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let lib = Library::load(&base.join("data"));
        let fonts = Fonts::load(&base.join("assets/fonts"));
        let mut st = State {
            lib,
            fonts,
            settings: TextSettings::default(),
            tabs: BTreeMap::new(),
            next_id: 1,
            focused: None,
            word: None,
            snippets: Snippets::new(),
            toast: None,
            stats: Stats::default(),
            last_stats: Stats::default(),
            raster_budget: 0,
            variant: 0,
            add_requests: vec![],
            nav: vec![],
            hover_popup: None,
            palette: None,
            global_ref: String::new(),
            global_ref_editing: false,
            drawer: [0, 0],
            drawer_comm: ResId::Jfb,
            frame_ms: 0.0,
        };
        let mut ids = BTreeMap::new();
        ids.insert("kjv", st.new_tab(TabKind::Doc(ResId::Kjv), Some('A')));
        ids.insert("tahot", st.new_tab(TabKind::Doc(ResId::Tahot), Some('A')));
        ids.insert("jfb", st.new_tab(TabKind::Doc(ResId::Jfb), Some('A')));
        ids.insert("mhcc", st.new_tab(TabKind::Doc(ResId::Mhcc), Some('A')));
        ids.insert("xref", st.new_tab(TabKind::CrossRefs, Some('A')));
        ids.insert("word", st.new_tab(TabKind::WordInfo, None));
        // variant C drawer: its own commentary + cross-ref Tabs, in link set A
        let dc = st.new_tab(TabKind::Doc(ResId::Jfb), Some('A'));
        let dx = st.new_tab(TabKind::CrossRefs, Some('A'));
        st.drawer = [dc, dx];
        st.focused = Some(ids["kjv"]);
        let start = VRef::new(0, 1, 1);
        let all: Vec<TabId> = st.tabs.keys().copied().collect();
        for id in all {
            st.goto_tab(id, start, false, 0.0);
        }
        let trees = vec![tree_main(&ids, "main-a"), tree_main(&ids, "main-b"), tree_focus(&ids)];
        // OBS_SHOT="variant;reference;out.ppm" — self-screenshot for smoke checks, then exit
        let shot = std::env::var("OBS_SHOT").ok().map(|v| {
            let mut it = v.split(';');
            (it.next().unwrap_or("0").parse().unwrap_or(0), it.next().unwrap_or("").to_string(), it.next().unwrap_or("/tmp/obs-shot.ppm").to_string(), 0)
        });
        if let Some((v, r, _, _)) = &shot {
            st.variant = *v;
            if !r.is_empty() {
                let f = st.focused.unwrap();
                st.submit_ref(f, r, 0.0);
            }
            st.word = Some(WordSel::Heb(3));
        }
        App { shot, st, trees, layouts: vec![] }
    }

    fn layouts_menu(&mut self, ui: &mut Ui) {
        ui.menu_button("Layouts", |ui| {
            if ui.button("Save current layout").clicked() {
                let n = self.layouts.len() + 1;
                self.layouts.push(SavedLayout {
                    name: format!("Layout {n} ({})", &VARIANTS[self.st.variant][..1]),
                    variant: self.st.variant,
                    tree: self.trees[self.st.variant].clone(),
                    links: self.st.tabs.iter().map(|(k, t)| (*k, t.link)).collect(),
                });
                ui.close();
            }
            let mut restore = None;
            for (i, l) in self.layouts.iter().enumerate() {
                if ui.button(&l.name).clicked() {
                    restore = Some(i);
                    ui.close();
                }
            }
            if let Some(i) = restore {
                let l = &self.layouts[i];
                self.trees[l.variant] = l.tree.clone();
                for (k, link) in &l.links {
                    if let Some(t) = self.st.tabs.get_mut(k) {
                        t.link = *link;
                    }
                }
                self.st.variant = l.variant;
            }
            ui.separator();
            if ui.button("Reset this variant's layout").clicked() {
                let ids: BTreeMap<&str, TabId> = [("kjv", 1), ("tahot", 2), ("jfb", 3), ("mhcc", 4), ("xref", 5), ("word", 6)].into_iter().collect();
                self.trees[self.st.variant] = if self.st.variant == 2 { tree_focus(&ids) } else { tree_main(&ids, if self.st.variant == 0 { "main-a" } else { "main-b" }) };
                ui.close();
            }
            ui.label(RichText::new("In memory only (prototype)").weak().small());
        });
    }

    fn toolbar_a(&mut self, ui: &mut Ui, now: f64) {
        let Some(fid) = self.st.focused.filter(|f| self.st.tabs.contains_key(f)) else { return };
        ui.horizontal(|ui| {
            ui.label(RichText::new("Open Bible").strong());
            ui.separator();
            self.st.nav_buttons(ui, fid, now);
            let shown = self.st.display_loc(fid);
            if !self.st.global_ref_editing {
                self.st.global_ref = shown;
            }
            let r = ui.add(egui::TextEdit::singleline(&mut self.st.global_ref).desired_width(200.0).hint_text("Go to reference (Ctrl+L)").id(egui::Id::new("global-ref")));
            if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::L)) {
                r.request_focus();
            }
            if r.gained_focus() {
                self.st.global_ref_editing = true;
            }
            if r.lost_focus() {
                self.st.global_ref_editing = false;
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let s = self.st.global_ref.clone();
                    self.st.submit_ref(fid, &s, now);
                }
            }
            ui.label(RichText::new(format!("in {}", self.st.title(fid))).weak());
            self.st.link_chip(ui, fid);
            ui.separator();
            self.layouts_menu(ui);
            self.st.settings_menu(ui);
        });
    }

    fn menubar_b(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Open Bible").strong());
            ui.separator();
            self.layouts_menu(ui);
            self.st.settings_menu(ui);
            ui.label(RichText::new("Each Tab has its own reference box, history and link chip.").weak().small());
        });
    }

    fn crumb_c(&mut self, ui: &mut Ui) {
        let fid = self.st.focused.filter(|f| self.st.tabs.contains_key(f)).unwrap_or(1);
        ui.horizontal(|ui| {
            self.layouts_menu(ui);
            self.st.settings_menu(ui);
            let label = format!("{}  ·  {}", self.st.display_loc(fid), self.st.title(fid));
            let avail = ui.available_width();
            ui.add_space((avail / 2.0 - 140.0).max(0.0));
            let r = ui.add(egui::Button::new(RichText::new(label).size(15.0)).frame(true).min_size(vec2(280.0, 24.0))).on_hover_text("Go to… (Ctrl+L)");
            if r.clicked() {
                self.st.palette = Some(String::new());
            }
            self.st.link_chip(ui, fid);
        });
        if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::L)) {
            self.st.palette = Some(String::new());
        }
    }

    fn drawer_c(&mut self, ui: &mut Ui, now: f64) {
        let [dc, dx] = self.st.drawer;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Study").strong());
            self.st.link_chip(ui, dc);
            ui.label(RichText::new("follows the link set").weak().small());
        });
        // keep drawer Tabs in the same link set
        let l = self.st.tabs[&dc].link;
        self.st.tabs.get_mut(&dx).unwrap().link = l;
        let h = ui.available_height();
        egui::CollapsingHeader::new("Commentary").default_open(true).show(ui, |ui| {
            ui.horizontal(|ui| {
                for (r, name) in [(ResId::Jfb, "JFB"), (ResId::Mhcc, "Matthew Henry")] {
                    if ui.selectable_label(self.st.drawer_comm == r, name).clicked() && self.st.drawer_comm != r {
                        self.st.drawer_comm = r;
                        let t = self.st.tabs.get_mut(&dc).unwrap();
                        t.kind = TabKind::Doc(r);
                        t.doc = DocView::new();
                        if let Some(loc) = self.st.tabs.iter().find(|(k, t)| **k != dc && t.link == l && t.loc.is_some()).and_then(|(_, t)| t.loc) {
                            self.st.goto_tab(dc, loc, false, now);
                        }
                    }
                }
            });
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), (h * 0.45).max(160.0)), egui::Sense::hover());
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt("drawer-comm"));
            let r = self.st.drawer_comm;
            self.st.doc_body(&mut child, dc, r);
        });
        egui::CollapsingHeader::new("Cross-references").default_open(true).show(ui, |ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), (h * 0.3).max(120.0)), egui::Sense::hover());
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt("drawer-xr"));
            self.st.xref_body(&mut child, dx, now);
        });
        egui::CollapsingHeader::new("Word Info").default_open(true).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("drawer-wi").auto_shrink([false, false]).show(ui, |ui| self.st.word_body(ui));
        });
    }

    fn palette_ui(&mut self, ctx: &egui::Context, now: f64) {
        let Some(mut text) = self.st.palette.take() else { return };
        let fid = self.st.focused.filter(|f| self.st.tabs.contains_key(f)).unwrap_or(1);
        let mut keep = true;
        let screen = ctx.content_rect();
        egui::Area::new(egui::Id::new("palette")).order(egui::Order::Foreground).fixed_pos(pos2(screen.center().x - 240.0, screen.top() + 60.0)).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).inner_margin(10.0).show(ui, |ui| {
                ui.set_width(480.0);
                let r = ui.add(egui::TextEdit::singleline(&mut text).hint_text("Reference (Gen 3:15, 2:4, gn 22) or command…").desired_width(460.0).id(egui::Id::new("palette-edit")));
                r.request_focus();
                let parsed = refs::parse(&text, self.st.tabs[&fid].loc);
                let mut first: Option<Box<dyn FnOnce(&mut State)>> = None;
                if let Some(Parsed::Verse(v) | Parsed::Chapter(v)) = parsed {
                    let label = format!("Go to {} in {} (and its link set)", v.display(), self.st.title(fid));
                    let txt = text.clone();
                    if ui.button(label).clicked() {
                        self.st.submit_ref(fid, &txt, now);
                        keep = false;
                    }
                    first = Some(Box::new(move |st: &mut State| st.submit_ref(fid, &txt, now)));
                }
                for (label, kind) in [("Open KJV with Strong's", ResId::Kjv), ("Open Hebrew OT", ResId::Tahot)] {
                    if text.is_empty() || label.to_lowercase().contains(&text.to_lowercase()) {
                        if ui.button(label).clicked() {
                            let tile = self.trees[2].root();
                            if let Some(t) = tile {
                                self.st.add_requests.push((t, TabKind::Doc(kind)));
                            }
                            keep = false;
                        }
                    }
                }
                ui.label(RichText::new("Enter = first item · Esc = close").weak().small());
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    if let Some(f) = first {
                        f(&mut self.st);
                    }
                    keep = false;
                }
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    keep = false;
                }
            });
        });
        if keep {
            self.st.palette = Some(text);
        }
    }

    fn switcher(&mut self, ctx: &egui::Context) {
        let screen = ctx.content_rect();
        egui::Area::new(egui::Id::new("variant-switcher")).order(egui::Order::Foreground).fixed_pos(pos2(screen.center().x - 150.0, screen.bottom() - 70.0)).show(ctx, |ui| {
            egui::Frame::new().fill(Color32::from_rgb(25, 25, 30)).corner_radius(18.0).inner_margin(egui::Margin::symmetric(12, 6)).shadow(egui::Shadow { offset: [0, 3], blur: 12, spread: 0, color: Color32::from_black_alpha(80) }).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let wt = |s: &str| RichText::new(s).color(Color32::WHITE).strong().size(16.0);
                    if ui.add(egui::Button::new(wt("‹")).frame(false)).clicked() {
                        self.st.variant = (self.st.variant + 2) % 3;
                    }
                    ui.add_sized(vec2(210.0, 20.0), egui::Label::new(RichText::new(format!("PROTOTYPE  {}", VARIANTS[self.st.variant])).color(Color32::WHITE).strong()));
                    if ui.add(egui::Button::new(wt("›")).frame(false)).clicked() {
                        self.st.variant = (self.st.variant + 1) % 3;
                    }
                });
            });
        });
        if !ctx.egui_wants_keyboard_input() && self.st.palette.is_none() {
            let (l, r) = ctx.input(|i| (i.key_pressed(egui::Key::ArrowLeft), i.key_pressed(egui::Key::ArrowRight)));
            if l {
                self.st.variant = (self.st.variant + 2) % 3;
            }
            if r {
                self.st.variant = (self.st.variant + 1) % 3;
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let t0 = std::time::Instant::now();
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        self.st.last_stats = std::mem::take(&mut self.st.stats);
        self.st.raster_budget = 10;
        self.st.snippets.next_frame();
        let v = self.st.variant;

        egui::Panel::top("chrome").show_inside(ui, |ui| {
            ui.add_space(3.0);
            match v {
                0 => self.toolbar_a(ui, now),
                1 => self.menubar_b(ui),
                _ => self.crumb_c(ui),
            }
            ui.add_space(3.0);
        });
        let mut visible: Vec<TabId> = self.trees[v].tiles.tiles().filter_map(|t| if let Tile::Pane(p) = t { Some(p.0) } else { None }).collect();
        if v == 2 {
            visible.extend(self.st.drawer);
        }
        egui::Panel::bottom("status").show_inside(ui, |ui| self.st.status_bar(ui, &visible));
        if v == 2 {
            egui::Panel::right("drawer").resizable(true).default_size(420.0).show_inside(ui, |ui| self.drawer_c(ui, now));
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_gray(225))).show_inside(ui, |ui| {
            let mut tree = std::mem::replace(&mut self.trees[v], Tree::empty(egui::Id::new("tmp")));
            tree.ui(&mut Shell { st: &mut self.st, now }, ui);
            for (container, kind) in std::mem::take(&mut self.st.add_requests) {
                let link = if matches!(kind, TabKind::WordInfo) { None } else { Some('A') };
                let id = self.st.new_tab(kind, link);
                let loc = self.st.tabs.values().find(|t| t.link == Some('A') && t.loc.is_some()).and_then(|t| t.loc).unwrap_or(VRef::new(0, 1, 1));
                self.st.goto_tab(id, loc, false, now);
                let tid = tree.tiles.insert_pane(Pane(id));
                match tree.tiles.get_mut(container) {
                    Some(Tile::Container(Container::Tabs(t))) => {
                        t.add_child(tid);
                        t.set_active(tid);
                    }
                    Some(Tile::Container(c)) => c.add_child(tid),
                    _ => {}
                }
                self.st.focused = Some(id);
            }
            self.trees[v] = tree;
        });

        self.st.process_nav(now);
        self.st.hover_popup_ui(&ctx);
        self.palette_ui(&ctx, now);
        if let Some((msg, t)) = &self.st.toast {
            if now - t < 3.0 {
                let screen = ctx.content_rect();
                egui::Area::new(egui::Id::new("toast")).order(egui::Order::Foreground).fixed_pos(pos2(screen.center().x - 200.0, screen.bottom() - 120.0)).show(&ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.label(RichText::new(msg).color(Color32::from_rgb(LINK[0], LINK[1], LINK[2])));
                    });
                });
                ctx.request_repaint();
            } else {
                self.st.toast = None;
            }
        }
        self.switcher(&ctx);
        self.st.frame_ms = t0.elapsed().as_secs_f32() * 1000.0;
        if let Some((_, _, path, n)) = &mut self.shot {
            *n += 1;
            if let Ok(px) = std::env::var("OBS_SCROLL").map(|v| v.parse::<f32>().unwrap_or(0.0)) {
                if (5..25).contains(n) {
                    let f = self.st.focused.unwrap();
                    self.st.tabs.get_mut(&f).unwrap().doc.inject_scroll = px / 20.0;
                }
            }
            ctx.request_repaint();
            if *n == 40 {
                let path = path.clone();
                let c2 = ctx.clone();
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |img| {
                    let [w, h] = img.size;
                    let mut f = format!("P6 {w} {h} 255\n").into_bytes();
                    for p in &img.pixels {
                        f.extend([p.r(), p.g(), p.b()]);
                    }
                    std::fs::write(&path, f).unwrap();
                    c2.send_viewport_cmd(egui::ViewportCommand::Close);
                })));
            }
            if *n > 400 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}
