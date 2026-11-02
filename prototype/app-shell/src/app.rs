//! PROTOTYPE: polished Study desk after issue #13's panel-header verdict.
//!
//! Docked Bibles, commentary and Tools. Every reading Tab keeps its own link chip, history and
//! reference box. Reader's table and Research ledger were tried earlier and rejected.
use crate::data::{HebWordInfo, Library, ResId};
use crate::doc::{DocView, Gfx, Hit, Snippets, Stats};
use crate::gfx::{Atlas, Mode, Pool};
#[allow(unused_imports)]
use crate::text::{SHAPE_CACHE_ON, SHAPE_HITS, SHAPE_MISSES};
use std::sync::Arc;
use crate::present::{TextSettings, INK};
use crate::refs::{self, Parsed, VRef};
use crate::text::Fonts;
use crate::theme::{self, link_color};
use egui::{pos2, vec2, Color32, Rect, RichText, Stroke, Ui};
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

const LINKS: [char; 3] = ['A', 'B', 'C'];

struct SavedLayout {
    name: String,
    tree: Tree<Pane>,
    links: BTreeMap<TabId, Option<char>>,
}

pub struct State {
    lib: Library,
    fonts: Arc<Fonts>,
    settings: TextSettings,
    tabs: BTreeMap<TabId, Tab>,
    next_id: TabId,
    focused: Option<TabId>,
    word: Option<WordSel>,
    snippets: Snippets,
    toast: Option<(String, f64)>,
    stats: Stats,
    last_stats: Stats,
    gfx: Gfx,
    add_requests: Vec<(TileId, TabKind)>,
    nav: Vec<Nav>,
    hover_popup: Option<(Hit, egui::Pos2)>,
    frame_ms: f32,
    show_diagnostics: bool,
}

enum Nav {
    /// user scrolled a Tab: followers follow, no history, no flash
    Follow(TabId, VRef),
    /// explicit jump (reference box, click): whole link set incl. self, with history + flash
    Jump(TabId, VRef),
}

struct Sample {
    scenario: usize,
    interval_ms: f32,
    ui_ms: f32,
    text_ms: f32,
    placeholders: u32,
    tex_bytes: usize,
    glyphs: u32,
    doc_ms: f32,
    snip_ms: f32,
    nav_ms: f32,
    /// previous frame's whole CPU time (ui + tessellation + paint submit), from eframe
    cpu_prev_ms: f32,
    snip_ph: u32,
}

/// OBS_BENCH=out.txt — scripted scroll/jump benchmark for #19, then exit.
struct Bench {
    out: String,
    frame: u32,
    last: Option<std::time::Instant>,
    samples: Vec<Sample>,
}

const SCENARIOS: [(&str, u32); 5] = [("warmup", 90), ("wheel KJV 25pt/f", 600), ("fling KJV 120pt/f", 240), ("jumps every 30f", 600), ("wheel TAHOT 25pt/f", 600)];

pub struct App {
    last_cpu_ms: f32,
    bench: Option<Bench>,
    shot: Option<(usize, String, String, u32)>,
    st: State,
    tree: Tree<Pane>,
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
        self.goto_tab_ext(id, r, flash, now, true)
    }

    fn goto_tab_ext(&mut self, id: TabId, r: VRef, flash: bool, now: f64, sync: bool) {
        let lib = &self.lib;
        let t = self.tabs.get_mut(&id).unwrap();
        match t.kind {
            TabKind::Doc(res) => {
                t.doc.goto(lib.get(res), r, now, flash, sync);
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
                self.goto_tab_ext(m, r, jump, now, false);
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
        let txt = RichText::new(link.map(|c| c.to_string()).unwrap_or("–".into())).strong().size(11.5).color(Color32::WHITE);
        let resp = ui
            .add(egui::Button::new(txt).fill(link_color(link)).stroke(Stroke::NONE).corner_radius(11.0).min_size(vec2(24.0, 22.0)))
            .on_hover_text("Link set: panels with the same letter follow one location. Click to change.");
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
        let arrow = |s: &str| egui::Button::new(RichText::new(s).size(18.0)).frame(false).min_size(vec2(24.0, 24.0));
        if ui.add_enabled(b > 0, arrow("‹")).on_hover_text("Back").clicked() {
            self.history(id, true, now);
        }
        if ui.add_enabled(f > 0, arrow("›")).on_hover_text("Forward").clicked() {
            self.history(id, false, now);
        }
    }

    fn tab_refbox(&mut self, ui: &mut Ui, id: TabId, width: f32, now: f64) {
        let shown = self.display_loc(id);
        let t = self.tabs.get_mut(&id).unwrap();
        if !t.refbox_editing {
            t.refbox = shown;
        }
        let r = ui.add(egui::TextEdit::singleline(&mut t.refbox).desired_width(width).hint_text("Go to passage").margin(egui::Margin::symmetric(8, 4)));
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
        self.panel_header(ui, id, kind, now);
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

    fn panel_header(&mut self, ui: &mut Ui, id: TabId, kind: TabKind, now: f64) {
        let link = self.tabs[&id].link;
        let focused = self.focused == Some(id);
        let fill = if focused { theme::FIELD } else { theme::SURFACE };
        let frame = egui::Frame::new().fill(fill).inner_margin(egui::Margin::symmetric(9, 4)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let narrow = ui.available_width() < 430.0;
                ui.spacing_mut().item_spacing.x = 5.0;
                ui.add_space(4.0);
                self.link_chip(ui, id);
                match kind {
                    TabKind::Doc(r) => {
                        self.nav_buttons(ui, id, now);
                        self.tab_refbox(ui, id, if narrow { 104.0 } else { 124.0 }, now);
                        if !narrow {
                            let title = self.lib.get(r).title.clone();
                            ui.add(egui::Label::new(RichText::new(title).small().color(theme::MUTED)).truncate());
                        }
                    }
                    TabKind::CrossRefs => {
                        ui.label(RichText::new("Cross-references").strong());
                        ui.label(RichText::new(self.display_loc(id)).color(theme::MUTED));
                    }
                    TabKind::WordInfo => {
                        ui.label(RichText::new("Word Info").strong());
                        ui.label(RichText::new("Follows your pointer").small().color(theme::MUTED));
                    }
                }
            });
        });
        let r = frame.response.rect;
        ui.painter().rect_filled(Rect::from_min_size(r.min, vec2(if focused { 4.0 } else { 2.0 }, r.height())), 0.0, link_color(link));
        ui.painter().hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, theme::RULE));
    }

    fn doc_body(&mut self, ui: &mut Ui, id: TabId, r: ResId) {
        let focused = self.focused == Some(id);
        let res = self.lib.get(r);
        let t = self.tabs.get_mut(&id).unwrap();
        let before = t.doc.location;
        let td = std::time::Instant::now();
        let out = t.doc.show(ui, res, &self.fonts, &self.settings, focused, &mut self.stats, &mut self.gfx);
        self.stats.doc_ms += td.elapsed().as_secs_f32() * 1000.0;
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
        egui::Frame::new().fill(theme::PAGE).inner_margin(egui::Margin::symmetric(16, 12)).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            let Some(loc) = loc else {
                ui.label(RichText::new("Choose a passage to see related verses.").color(theme::MUTED));
                return;
            };
            let list = self.lib.xrefs.refs.get(&loc).cloned().unwrap_or_default();
            if list.is_empty() {
                ui.label(RichText::new("No cross-references for this verse.").color(theme::MUTED));
            }
            egui::ScrollArea::vertical().id_salt(("xr", id)).auto_shrink([false, false]).show(ui, |ui| {
                let w = ui.available_width() - 8.0;
                for (i, (target, votes)) in list.into_iter().enumerate() {
                    if i > 0 {
                        ui.add_space(5.0);
                        ui.separator();
                    }
                    ui.add_space(5.0);
                    ui.horizontal(|ui| {
                        let r = ui.add(egui::Label::new(RichText::new(refs::osis_range_short(&target)).color(theme::LAPIS).strong()).sense(egui::Sense::click()));
                        if r.clicked() {
                            match VRef::from_osis(target.split('-').next().unwrap_or("")) {
                                Some(v) if v.book == 0 => self.nav.push(Nav::Jump(id, v)),
                                Some(v) => self.toast(format!("{} is outside the prototype library", v.display()), now),
                                None => {}
                            }
                        }
                        r.on_hover_cursor(egui::CursorIcon::PointingHand);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(format!("{votes} votes")).small().color(theme::MUTED));
                        });
                    });
                    if let Some(t) = self.lib.kjv_plain(&target) {
                        self.snippets.show(ui, &self.fonts, &mut self.gfx, &mut self.stats, &self.settings, &t, self.settings.size * 0.82, INK, false, w);
                    }
                }
                ui.add_space(14.0);
                ui.label(RichText::new(&self.lib.xrefs.attribution).small().color(theme::MUTED));
            });
        });
    }

    fn word_body(&mut self, ui: &mut Ui) {
        egui::Frame::new().fill(theme::PAGE).inner_margin(egui::Margin::symmetric(16, 12)).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            let w = ui.available_width();
            let s = self.settings;
            match self.word.clone() {
                None => {
                    ui.label(RichText::new("Point to a tagged word in the KJV or Hebrew text.").color(theme::MUTED));
                }
                Some(WordSel::Heb(i)) => {
                    let wd: HebWordInfo = self.lib.tahot.words[i].clone();
                    self.snippets.show(ui, &self.fonts, &mut self.gfx, &mut self.stats, &s, &wd.heb, s.size * 1.9, INK, true, w);
                    ui.label(RichText::new(wd.translit.replace(['.', '/'], "")).italics());
                    ui.label(RichText::new(&wd.gloss).strong());
                    ui.separator();
                    field(ui, "Lemma");
                    self.snippets.show(ui, &self.fonts, &mut self.gfx, &mut self.stats, &s, &wd.lemma, s.size * 1.4, INK, true, w);
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
                            self.snippets.show(ui, &self.fonts, &mut self.gfx, &mut self.stats, &s, &lx.lemma, s.size * 1.6, INK, true, w);
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
                            self.snippets.show(ui, &self.fonts, &mut self.gfx, &mut self.stats, &s, &t, s.size * 0.85, INK, false, 360.0);
                        }
                        ui.label(RichText::new("KJV · click to go there (Genesis only)").weak().small());
                    }
                    Hit::Note(t) => {
                        ui.label(RichText::new("Translator's note").weak().small());
                        self.snippets.show(ui, &self.fonts, &mut self.gfx, &mut self.stats, &s, &t, s.size * 0.85, INK, false, 360.0);
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
            ui.separator();
            ui.label("Renderer (#19)");
            ui.radio_value(&mut self.gfx.mode, Mode::Atlas, "Glyph atlas");
            ui.radio_value(&mut self.gfx.mode, Mode::Tiles, "Block tiles + workers");
        });
    }

    fn status_bar(&mut self, ui: &mut Ui, visible: &[TabId]) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Linked locations").small().strong().color(theme::MUTED));
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
                ui.label(RichText::new(format!(" {l} ")).small().strong().color(Color32::WHITE).background_color(link_color(Some(l))));
                ui.label(RichText::new(loc.map(|r| r.short()).unwrap_or_default()).strong());
                ui.add(egui::Label::new(RichText::new(members.join(", ")).small().color(theme::MUTED)).truncate());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new("Prototype library: Genesis").small().color(theme::MUTED));
            });
        });
        if self.show_diagnostics {
            ui.horizontal(|ui| {
                let st = &self.last_stats;
                ui.label(
                    RichText::new(format!(
                        "{} · frame {:.1} ms · text {:.1} ms · placeholders {} · layout {}× {:.1} ms · raster {}× {:.1} ms · {} tiles {:.1} MB · atlas {} glyphs {:.1} MB",
                        if self.gfx.mode == Mode::Atlas { "ATLAS" } else { "TILES" },
                        self.frame_ms,
                        st.text_ms,
                        st.placeholders,
                        st.layouts,
                        st.layout_ms,
                        st.rasters,
                        st.raster_ms,
                        st.textures,
                        st.texture_bytes as f32 / 1e6,
                        self.gfx.atlas.glyphs,
                        if self.gfx.atlas.glyphs > 0 { self.gfx.atlas.bytes() as f32 / 1e6 } else { 0.0 }
                    ))
                    .small()
                    .monospace()
                    .color(theme::MUTED),
                );
            });
        }
    }
}

fn field(ui: &mut Ui, name: &str) {
    ui.add_space(9.0);
    ui.label(RichText::new(name.to_uppercase()).size(10.5).strong().color(theme::MUTED));
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
        let link = t.link.map(|c| c.to_string()).unwrap_or_else(|| "-".into());
        job.append(&link, 0.0, egui::TextFormat { color: link_color(t.link), font_id: egui::FontId::proportional(12.0), ..Default::default() });
        let focused = self.st.focused == Some(pane.0);
        job.append(
            &self.st.title(pane.0),
            7.0,
            egui::TextFormat {
                color: if focused { theme::INK } else { theme::MUTED },
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

    fn tab_bg_color(&self, _visuals: &egui::Visuals, _tiles: &Tiles<Pane>, _tile_id: TileId, state: &egui_tiles::TabState) -> Color32 {
        if state.active { theme::SURFACE } else { Color32::TRANSPARENT }
    }

    fn tab_outline_stroke(&self, _visuals: &egui::Visuals, _tiles: &Tiles<Pane>, _tile_id: TileId, state: &egui_tiles::TabState) -> Stroke {
        let _ = state;
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

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> App {
        cc.egui_ctx.set_theme(egui::Theme::Light);
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        theme::install(&cc.egui_ctx, &base.join("assets/fonts"));
        let lib = Library::load(&base.join("data"));
        let fonts = Arc::new(Fonts::load(&base.join("assets/fonts")));
        let mut st = State {
            lib,
            fonts: fonts.clone(),
            settings: TextSettings::default(),
            tabs: BTreeMap::new(),
            next_id: 1,
            focused: None,
            word: None,
            snippets: Snippets::new(),
            toast: None,
            stats: Stats::default(),
            last_stats: Stats::default(),
            gfx: Gfx {
                mode: if std::env::var("OBS_RENDER").as_deref() == Ok("tiles") { Mode::Tiles } else { Mode::Atlas },
                atlas: Atlas::new(),
                pool: Pool::new(fonts.clone()),
                upload_budget: 0,
                retain: 1.5,
                layout_pool: crate::gfx::LayoutPool::new(fonts.clone(), 3),
                layout_budget_ms: 0.0,
                async_layout: std::env::var("OBS_ASYNC").as_deref() != Ok("0"),
            },
            add_requests: vec![],
            nav: vec![],
            hover_popup: None,
            frame_ms: 0.0,
            show_diagnostics: std::env::var("OBS_DIAGNOSTICS").as_deref() == Ok("1"),
        };
        let mut ids = BTreeMap::new();
        ids.insert("kjv", st.new_tab(TabKind::Doc(ResId::Kjv), Some('A')));
        ids.insert("tahot", st.new_tab(TabKind::Doc(ResId::Tahot), Some('A')));
        ids.insert("jfb", st.new_tab(TabKind::Doc(ResId::Jfb), Some('A')));
        ids.insert("mhcc", st.new_tab(TabKind::Doc(ResId::Mhcc), Some('A')));
        ids.insert("xref", st.new_tab(TabKind::CrossRefs, Some('A')));
        ids.insert("word", st.new_tab(TabKind::WordInfo, None));
        st.focused = Some(ids["kjv"]);
        let start = VRef::new(0, 1, 1);
        let all: Vec<TabId> = st.tabs.keys().copied().collect();
        for id in all {
            st.goto_tab(id, start, false, 0.0);
        }
        let tree = tree_main(&ids, "desk");
        // OBS_SHOT="ignored;reference;out.ppm" — self-screenshot for smoke checks, then exit
        let shot = std::env::var("OBS_SHOT").ok().map(|v| {
            let mut it = v.split(';');
            (it.next().unwrap_or("0").parse().unwrap_or(0), it.next().unwrap_or("").to_string(), it.next().unwrap_or("/tmp/obs-shot.ppm").to_string(), 0)
        });
        if let Some((_, r, _, _)) = &shot {
            if !r.is_empty() {
                let f = st.focused.unwrap();
                st.submit_ref(f, r, 0.0);
            }
            st.word = Some(WordSel::Heb(3));
        }
        if std::env::var("OBS_SHAPECACHE").as_deref() == Ok("0") {
            SHAPE_CACHE_ON.store(false, std::sync::atomic::Ordering::Relaxed);
        }
        let bench = std::env::var("OBS_BENCH").ok().map(|out| Bench { out, frame: 0, last: None, samples: vec![] });
        App { last_cpu_ms: 0.0, bench, shot, st, tree, layouts: vec![] }
    }

    fn layouts_menu(&mut self, ui: &mut Ui) {
        ui.menu_button("Layouts", |ui| {
            if ui.button("Save current layout").clicked() {
                let n = self.layouts.len() + 1;
                self.layouts.push(SavedLayout {
                    name: format!("Layout {n}"),
                    tree: self.tree.clone(),
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
                self.tree = l.tree.clone();
                for (k, link) in &l.links {
                    if let Some(t) = self.st.tabs.get_mut(k) {
                        t.link = *link;
                    }
                }
            }
            ui.separator();
            if ui.button("Reset layout").clicked() {
                let ids: BTreeMap<&str, TabId> = [("kjv", 1), ("tahot", 2), ("jfb", 3), ("mhcc", 4), ("xref", 5), ("word", 6)].into_iter().collect();
                self.tree = tree_main(&ids, "desk");
                ui.close();
            }
            ui.label(RichText::new("Saved for this session only").small().color(theme::MUTED));
        });
    }

    fn view_menu(&mut self, ui: &mut Ui) {
        ui.menu_button("View", |ui| {
            ui.checkbox(&mut self.st.show_diagnostics, "Show performance details");
        });
    }

    fn app_bar(&mut self, ui: &mut Ui) {
        let fid = self.st.focused.filter(|f| self.st.tabs.contains_key(f)).unwrap_or(1);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Open Bible").font(theme::display(20.0)).color(theme::INK));
            ui.label(RichText::new("Study desk").small().color(theme::MUTED));
            ui.separator();
            ui.scope(|ui| {
                let w = &mut ui.visuals_mut().widgets;
                w.inactive.bg_stroke = Stroke::NONE;
                w.inactive.weak_bg_fill = Color32::TRANSPARENT;
                w.hovered.bg_stroke = Stroke::NONE;
                self.layouts_menu(ui);
                self.st.settings_menu(ui);
                self.view_menu(ui);
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let loc = self.st.display_loc(fid);
                ui.label(RichText::new(format!("{loc} / {}", self.st.title(fid))).color(theme::MUTED));
            });
        });
    }

}

impl App {
    fn bench_step(&mut self, ctx: &egui::Context, t0: std::time::Instant) {
        let Some(b) = &mut self.bench else { return };
        ctx.request_repaint();
        let now = std::time::Instant::now();
        let interval = b.last.map(|l| (t0 - l).as_secs_f32() * 1000.0).unwrap_or(0.0);
        b.last = Some(t0);
        let _ = now;
        // which scenario is this frame in
        let mut f = b.frame;
        let mut sc = SCENARIOS.len();
        for (i, (_, len)) in SCENARIOS.iter().enumerate() {
            if f < *len {
                sc = i;
                break;
            }
            f -= len;
        }
        if b.frame > 0 && sc <= SCENARIOS.len() {
            let st = &self.st.stats;
            b.samples.push(Sample {
                scenario: sc.min(SCENARIOS.len() - 1),
                interval_ms: interval,
                ui_ms: self.st.frame_ms,
                text_ms: st.text_ms,
                placeholders: st.placeholders,
                tex_bytes: st.texture_bytes + if self.st.gfx.atlas.glyphs > 0 { self.st.gfx.atlas.bytes() } else { 0 },
                glyphs: st.glyphs_rastered,
                doc_ms: st.doc_ms,
                snip_ms: st.snip_ms,
                nav_ms: st.nav_ms,
                cpu_prev_ms: self.last_cpu_ms,
                snip_ph: st.snip_placeholders,
            });
        }
        let kjv = 1usize;
        let tahot = 2usize;
        match sc {
            1 => {
                self.st.focused = Some(kjv);
                self.st.tabs.get_mut(&kjv).unwrap().doc.inject_scroll = 25.0;
            }
            2 => {
                self.st.focused = Some(kjv);
                self.st.tabs.get_mut(&kjv).unwrap().doc.inject_scroll = 120.0;
            }
            3 => {
                if f % 30 == 0 {
                    let ch = [37, 3, 48, 12, 25, 1, 44, 19, 31, 7, 50, 22, 15, 40, 9, 28, 2, 46, 33, 18][(f / 30) as usize % 20];
                    self.st.focused = Some(kjv);
                    self.st.submit_ref(kjv, &format!("Gen {ch}:1"), 0.0);
                }
            }
            4 => {
                if f == 0 {
                    self.st.submit_ref(kjv, "Gen 1:1", 0.0);
                }
                self.st.focused = Some(tahot);
                self.st.tabs.get_mut(&tahot).unwrap().doc.inject_scroll = 25.0;
            }
            _ => {}
        }
        b.frame += 1;
        if b.frame % 100 == 0 {
            eprintln!("bench frame {} scenario {sc} interval {interval:.1}", b.frame);
        }
        if sc == SCENARIOS.len() {
            let mut rep = String::new();
            let mode = if self.st.gfx.mode == Mode::Atlas { "atlas" } else { "tiles" };
            let warm: Vec<f32> = b.samples.iter().filter(|s| s.scenario == 0).map(|s| s.interval_ms).collect();
            let refresh = pct(&warm, 0.5);
            use std::sync::atomic::Ordering::Relaxed;
            let (h, m) = (SHAPE_HITS.load(Relaxed), SHAPE_MISSES.load(Relaxed));
            rep += &format!(
                "shape cache {} hit rate {:.1}% ({} hits / {} misses) · async layout {}\n",
                if SHAPE_CACHE_ON.load(Relaxed) { "on" } else { "off" },
                100.0 * h as f64 / (h + m).max(1) as f64,
                h,
                m,
                self.st.gfx.async_layout
            );
            rep += &format!("mode={mode} workers={} refresh≈{refresh:.2} ms ({:.0} Hz) ppp={}\n", self.st.gfx.pool.workers, 1000.0 / refresh, ctx.pixels_per_point());
            for (i, (name, _)) in SCENARIOS.iter().enumerate().skip(1) {
                let ss: Vec<&Sample> = b.samples.iter().filter(|s| s.scenario == i).collect();
                let iv: Vec<f32> = ss.iter().map(|s| s.interval_ms).collect();
                let ui: Vec<f32> = ss.iter().map(|s| s.ui_ms).collect();
                let tx: Vec<f32> = ss.iter().map(|s| s.text_ms).collect();
                let missed = iv.iter().filter(|v| **v > refresh * 1.5).count();
                let ph = ss.iter().filter(|s| s.placeholders > 0).count();
                let mem = ss.iter().map(|s| s.tex_bytes).max().unwrap_or(0);
                let gl: u32 = ss.iter().map(|s| s.glyphs).sum();
                let dm: Vec<f32> = ss.iter().map(|s| s.doc_ms).collect();
                let sm: Vec<f32> = ss.iter().map(|s| s.snip_ms).collect();
                let nm: Vec<f32> = ss.iter().map(|s| s.nav_ms).collect();
                // worst 1% of frames: where did the time go?
                let mut worst: Vec<&&Sample> = ss.iter().collect();
                worst.sort_by(|a, b| b.ui_ms.partial_cmp(&a.ui_ms).unwrap());
                let k = (worst.len() / 100).max(3).min(worst.len());
                let avg = |f: &dyn Fn(&Sample) -> f32| worst[..k].iter().map(|s| f(s)).sum::<f32>() / k.max(1) as f32;
                let breakdown = format!(
                    "    worst {k} frames avg: ui {:.1} = docs {:.1} (text {:.1}) + xref/wordinfo snippets {:.1} + nav {:.1} + other {:.1}   | p99 docs {:.1} snippets {:.1}\n",
                    avg(&|s| s.ui_ms), avg(&|s| s.doc_ms), avg(&|s| s.text_ms), avg(&|s| s.snip_ms), avg(&|s| s.nav_ms),
                    avg(&|s| s.ui_ms - s.doc_ms - s.snip_ms - s.nav_ms), pct(&dm, 0.99), pct(&sm, 0.99)
                );
                let _ = &nm;
                let cpu: Vec<f32> = ss.iter().map(|s| s.cpu_prev_ms).collect();
                let post: Vec<f32> = ss.windows(2).map(|w| (w[1].cpu_prev_ms - w[0].ui_ms).max(0.0)).collect();
                let breakdown = format!("{breakdown}    whole-frame CPU p50 {:.2} p99 {:.2} max {:.2} | after-ui (tessellate+paint) p50 {:.2} p99 {:.2}\n", pct(&cpu, 0.5), pct(&cpu, 0.99), pct(&cpu, 1.0), pct(&post, 0.5), pct(&post, 0.99));
                let sph = ss.iter().filter(|s| s.snip_ph > 0).count();
                let breakdown = format!("{breakdown}    preview placeholder frames {sph} ({:.1}%)\n", 100.0 * sph as f32 / ss.len().max(1) as f32);
                rep += &format!(
                    "{name:<22} frames {:>4} | interval p50 {:>5.2} p99 {:>6.2} max {:>6.2} | missed {:>4} ({:>4.1}%) | ui p50 {:>5.2} p99 {:>6.2} | text p99 {:>6.2} max {:>6.2} | placeholder frames {:>4} ({:>4.1}%) | peak tex {:>6.1} MB | new glyphs {}\n",
                    iv.len(), pct(&iv, 0.5), pct(&iv, 0.99), pct(&iv, 1.0), missed, 100.0 * missed as f32 / iv.len().max(1) as f32,
                    pct(&ui, 0.5), pct(&ui, 0.99), pct(&tx, 0.99), pct(&tx, 1.0), ph, 100.0 * ph as f32 / ss.len().max(1) as f32, mem as f32 / 1e6, gl
                );
                rep += &breakdown;
            }
            std::fs::write(&b.out, &rep).unwrap();
            print!("{rep}");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn pct(v: &[f32], p: f32) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    s[((s.len() - 1) as f32 * p).round() as usize]
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        self.last_cpu_ms = frame.info().cpu_usage.unwrap_or(0.0) * 1000.0;
        let t0 = std::time::Instant::now();
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        self.st.last_stats = std::mem::take(&mut self.st.stats);
        self.st.gfx.upload_budget = 6;
        self.st.gfx.layout_budget_ms = 2.0;
        self.st.gfx.pool.gc();
        self.st.snippets.next_frame();
        egui::Panel::top("chrome")
            .frame(egui::Frame::new().fill(theme::SURFACE).inner_margin(egui::Margin::symmetric(14, 8)))
            .show_inside(ui, |ui| self.app_bar(ui));
        let visible: Vec<TabId> = self.tree.tiles.tiles().filter_map(|t| if let Tile::Pane(p) = t { Some(p.0) } else { None }).collect();
        egui::Panel::bottom("status")
            .frame(egui::Frame::new().fill(theme::SURFACE).inner_margin(egui::Margin::symmetric(14, 6)))
            .show_inside(ui, |ui| self.st.status_bar(ui, &visible));
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::SHELL).inner_margin(egui::Margin::same(10))).show_inside(ui, |ui| {
            let mut tree = std::mem::replace(&mut self.tree, Tree::empty(egui::Id::new("tmp")));
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
            self.tree = tree;
        });

        let tn = std::time::Instant::now();
        self.st.process_nav(now);
        self.st.stats.nav_ms += tn.elapsed().as_secs_f32() * 1000.0;
        self.st.stats.snip_ms = std::mem::take(&mut self.st.snippets.ms);
        self.st.hover_popup_ui(&ctx);
        if let Some((msg, t)) = &self.st.toast {
            if now - t < 3.0 {
                let screen = ctx.content_rect();
                egui::Area::new(egui::Id::new("toast")).order(egui::Order::Foreground).fixed_pos(pos2(screen.center().x - 200.0, screen.bottom() - 120.0)).show(&ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.label(RichText::new(msg).color(theme::LAPIS));
                    });
                });
                ctx.request_repaint();
            } else {
                self.st.toast = None;
            }
        }
        self.st.frame_ms = t0.elapsed().as_secs_f32() * 1000.0;
        self.bench_step(&ctx, t0);
        if let Some((_, _, path, n)) = &mut self.shot {
            *n += 1;
            if let Ok(px) = std::env::var("OBS_SCROLL").map(|v| v.parse::<f32>().unwrap_or(0.0)) {
                if (5..25).contains(n) {
                    let f = self.st.focused.unwrap();
                    self.st.tabs.get_mut(&f).unwrap().doc.inject_scroll = px / 20.0;
                }
            }
            ctx.request_repaint();
            let shot_frame = std::env::var("OBS_SHOT_FRAME").ok().and_then(|v| v.parse().ok()).unwrap_or(40);
            if *n == shot_frame {
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
            if *n > shot_frame + 360 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}
