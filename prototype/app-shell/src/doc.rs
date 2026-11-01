//! PROTOTYPE: the "scrolling document" mode of the rich-text widget (ADR 0004 §5, §7–§9).
//! One block = one text unit. Unlaid blocks use an estimated height. Scroll position is
//! anchored to (block, offset) so re-measuring never moves the view.

use crate::data::{slice_units, ResKind, Resource, SpanKind, UnitKind};
use crate::present::{unit_para, Obj, TextSettings};
use crate::refs::VRef;
use crate::text::{layout, raster, Fonts, Laid};
use egui::{pos2, vec2, Color32, Rect, Sense, TextureHandle, Ui};
use std::collections::HashMap;
use std::time::Instant;

pub const MARGIN: f32 = 22.0;
const BAR: f32 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DocPos {
    pub unit: usize,
    pub byte: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Hit {
    Strong { strong: String, word: String },
    Heb(usize),
    Note(String),
    Ref(String),
    Verse(VRef),
}

struct Block {
    laid: Laid,
    objs: Vec<Obj>,
    tex: Option<(TextureHandle, [usize; 2], f32)>,
}

#[derive(Default)]
pub struct Stats {
    pub layouts: u32,
    pub rasters: u32,
    pub layout_ms: f32,
    pub raster_ms: f32,
    pub textures: usize,
    pub texture_bytes: usize,
}

pub struct DocView {
    anchor: usize,
    anchor_off: f32,
    heights: Vec<f32>,
    measured: Vec<bool>,
    key: (i32, u64),
    blocks: HashMap<usize, Block>,
    pub sel: Option<(DocPos, DocPos)>,
    selecting: bool,
    pending: Option<DocPos>,
    pub location: Option<VRef>,
    flash: Option<(DocPos, DocPos, f64)>,
    pub hover_word: Option<(usize, usize, usize)>,
    /// test hook: synthetic wheel input
    pub inject_scroll: f32,
}

pub struct DocOut {
    pub user_moved: bool,
    pub hover: Option<Hit>,
    pub clicked: Option<Hit>,
    pub interacted: bool,
    pub copy: Option<String>,
}

impl DocView {
    pub fn new() -> Self {
        DocView {
            anchor: 0,
            anchor_off: 0.0,
            heights: Vec::new(),
            measured: Vec::new(),
            key: (0, 0),
            blocks: HashMap::new(),
            sel: None,
            selecting: false,
            pending: None,
            location: None,
            flash: None,
            hover_word: None,
            inject_scroll: 0.0,
        }
    }

    /// Navigate to a Standard-versification location (link set follow / reference box).
    pub fn goto(&mut self, res: &Resource, r: VRef, now: f64, flash: bool) {
        if let Some((u, b)) = res.locate(r) {
            self.pending = Some(DocPos { unit: u, byte: b });
            self.location = Some(r);
            if !flash {
                return;
            }
            if res.kind == ResKind::Bible {
                if let Some((a, e)) = res.verse_range(r) {
                    self.flash = Some((DocPos { unit: a.0, byte: a.1 }, DocPos { unit: e.0, byte: e.1 }, now));
                }
            } else {
                let t = res.units[u].text.len();
                self.flash = Some((DocPos { unit: u, byte: 0 }, DocPos { unit: u, byte: t }, now));
            }
            self.location = Some(r);
        }
    }

    fn estimate(res: &Resource, ui: usize, w: f32, s: &TextSettings) -> f32 {
        let u = &res.units[ui];
        let (size, adv) = match u.kind {
            UnitKind::Chapter => (s.size * 1.6, 0.5),
            UnitKind::H2 => (s.size * 1.12, 0.5),
            _ if res.rtl => (s.size * s.hebrew_scale, 0.42),
            _ => (s.size, 0.45),
        };
        let chars = u.text.chars().count() as f32 + if res.kind == ResKind::Bible { 4.0 } else { 0.0 };
        let lines = ((chars * size * adv) / w.max(50.0)).ceil().max(1.0);
        let lh = size * if res.rtl { 1.9 } else { 1.3 } * s.line_spacing;
        let extra = match u.kind {
            UnitKind::Chapter => s.size * 1.7,
            UnitKind::H2 => s.size * 1.2,
            _ => s.size * 0.55,
        };
        lines * lh + extra
    }

    fn ensure_laid(&mut self, res: &Resource, fonts: &Fonts, s: &TextSettings, i: usize, w: f32, stats: &mut Stats) {
        if self.blocks.contains_key(&i) {
            return;
        }
        let t = Instant::now();
        let mut objs = Vec::new();
        let para = unit_para(res, i, s, &mut objs);
        let laid = layout(fonts, &para, w);
        stats.layouts += 1;
        stats.layout_ms += t.elapsed().as_secs_f32() * 1000.0;
        self.heights[i] = laid.height;
        self.measured[i] = true;
        self.blocks.insert(i, Block { laid, objs, tex: None });
    }

    fn h(&self, i: usize) -> f32 {
        self.heights[i]
    }

    /// If a verse starts on the top visible line, that verse is the Current location.
    fn top_line_verse(&self, res: &Resource) -> Option<VRef> {
        let b = self.blocks.get(&self.anchor)?;
        let line = b.laid.lines.get(b.laid.line_index_at(self.anchor_off + 1.0))?;
        let (a, _) = b.laid.to_unit(line.start);
        let (e, _) = b.laid.to_unit(line.end.saturating_sub(1));
        res.verse_pos.iter().find(|(u, by, _)| *u == self.anchor && *by >= a && *by <= e).map(|x| x.2)
    }

    fn top_doc_pos(&self) -> Option<DocPos> {
        let b = self.blocks.get(&self.anchor)?;
        let li = b.laid.line_index_at(self.anchor_off + 1.0);
        let line = b.laid.lines.get(li)?;
        let (ub, _) = b.laid.to_unit(line.start);
        Some(DocPos { unit: self.anchor, byte: ub })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ui: &mut Ui,
        res: &Resource,
        fonts: &Fonts,
        s: &TextSettings,
        focused: bool,
        stats: &mut Stats,
        raster_budget: &mut u32,
    ) -> DocOut {
        let mut out = DocOut { user_moved: false, hover: None, clicked: None, interacted: false, copy: None };
        let n = res.units.len();
        let rect = ui.available_rect_before_wrap();
        let id = ui.id().with("doc");
        let resp = ui.interact(rect, id, Sense::click_and_drag());
        ui.allocate_rect(rect, Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_rgb(253, 252, 248));
        let bar_rect = Rect::from_min_max(pos2(rect.right() - BAR, rect.top()), rect.right_bottom());
        let inner = Rect::from_min_max(pos2(rect.left() + MARGIN, rect.top()), pos2(bar_rect.left() - MARGIN * 0.6, rect.bottom()));
        let w = inner.width().max(60.0);
        let view_h = inner.height();
        let ppp = ui.ctx().pixels_per_point();
        let now = ui.input(|i| i.time);

        // width / settings change: re-estimate, keep the anchor on the same text
        let key = (w.round() as i32, s.key());
        if key != self.key || self.heights.len() != n {
            let keep = if self.heights.len() == n { self.top_doc_pos() } else { None };
            self.key = key;
            self.blocks.clear();
            self.heights = (0..n).map(|i| Self::estimate(res, i, w, s)).collect();
            self.measured = vec![false; n];
            if let (Some(p), None) = (keep, self.pending) {
                self.pending = Some(p);
            }
        }

        // pending navigation: put the target line at the top
        if let Some(p) = self.pending.take() {
            self.ensure_laid(res, fonts, s, p.unit, w, stats);
            let b = &self.blocks[&p.unit];
            let pb = b.laid.from_unit(p.byte);
            let line = b.laid.lines.iter().find(|l| pb >= l.start && pb < l.end.max(l.start + 1)).or(b.laid.lines.first());
            self.anchor = p.unit;
            self.anchor_off = line.map(|l| (l.top - 4.0).max(0.0)).unwrap_or(0.0);
        }

        // input: wheel
        let hovered = ui.rect_contains_pointer(rect);
        if hovered {
            let dy = ui.input(|i| i.smooth_scroll_delta.y);
            if dy != 0.0 {
                self.anchor_off -= dy;
                out.user_moved = true;
            }
        }
        if self.inject_scroll != 0.0 {
            self.anchor_off += std::mem::take(&mut self.inject_scroll);
            out.user_moved = true;
        }
        // keyboard paging for the focused doc
        if focused {
            let (pd, pu) = ui.input(|i| (i.key_pressed(egui::Key::PageDown), i.key_pressed(egui::Key::PageUp)));
            if pd {
                self.anchor_off += view_h * 0.9;
                out.user_moved = true;
            }
            if pu {
                self.anchor_off -= view_h * 0.9;
                out.user_moved = true;
            }
        }

        // drag-select auto-scroll
        let pointer = ui.input(|i| i.pointer.interact_pos());
        if self.selecting {
            if let Some(p) = pointer {
                if p.y < inner.top() + 10.0 {
                    self.anchor_off -= ((inner.top() + 10.0 - p.y) * 0.4).min(40.0);
                    ui.ctx().request_repaint();
                } else if p.y > inner.bottom() - 10.0 {
                    self.anchor_off += ((p.y - inner.bottom() + 10.0) * 0.4).min(40.0);
                    ui.ctx().request_repaint();
                }
            }
        }

        // scrollbar drag
        let total: f32 = self.heights.iter().sum();
        let bar_resp = ui.interact(bar_rect, id.with("bar"), Sense::click_and_drag());
        if bar_resp.dragged() || bar_resp.clicked() {
            if let Some(p) = bar_resp.interact_pointer_pos() {
                let frac = ((p.y - bar_rect.top()) / bar_rect.height()).clamp(0.0, 1.0);
                let mut target = frac * (total - view_h).max(0.0);
                let mut i = 0;
                while i + 1 < n && target >= self.heights[i] {
                    target -= self.heights[i];
                    i += 1;
                }
                self.anchor = i;
                self.anchor_off = target;
                out.user_moved = true;
            }
        }

        // normalize anchor
        while self.anchor_off < 0.0 && self.anchor > 0 {
            self.anchor -= 1;
            self.ensure_laid(res, fonts, s, self.anchor, w, stats);
            self.anchor_off += self.h(self.anchor);
        }
        if self.anchor_off < 0.0 {
            self.anchor_off = 0.0;
        }
        self.ensure_laid(res, fonts, s, self.anchor, w, stats);
        while self.anchor + 1 < n && self.anchor_off >= self.h(self.anchor) {
            self.anchor_off -= self.h(self.anchor);
            self.anchor += 1;
            self.ensure_laid(res, fonts, s, self.anchor, w, stats);
        }
        // don't scroll past the end
        {
            let mut acc = -self.anchor_off;
            let mut i = self.anchor;
            while i < n && acc < view_h {
                self.ensure_laid(res, fonts, s, i, w, stats);
                acc += self.h(i);
                i += 1;
            }
            if i >= n && acc < view_h - 40.0 {
                self.anchor_off -= view_h - 40.0 - acc;
                while self.anchor_off < 0.0 && self.anchor > 0 {
                    self.anchor -= 1;
                    self.ensure_laid(res, fonts, s, self.anchor, w, stats);
                    self.anchor_off += self.h(self.anchor);
                }
                self.anchor_off = self.anchor_off.max(0.0);
            }
        }

        // visible blocks
        let mut visible: Vec<(usize, f32)> = Vec::new();
        let mut y = inner.top() - self.anchor_off;
        let mut i = self.anchor;
        while i < n && y < inner.bottom() {
            self.ensure_laid(res, fonts, s, i, w, stats);
            visible.push((i, y));
            y += self.h(i);
            i += 1;
        }
        // pre-raster one block below the fold so slow scrolling stays smooth
        let lookahead = if i < n { Some(i) } else { None };

        // current location: kept as the jump target until the user scrolls this Tab
        if out.user_moved || self.location.is_none() {
            if let Some(p) = self.top_doc_pos() {
                self.location = match res.kind {
                    ResKind::Bible => self.top_line_verse(res).or(res.verse_at(p.unit, p.byte)),
                    ResKind::Commentary => res.key_at(p.unit),
                };
            }
        }

        // hit-testing
        let pos_to_doc = |vis: &[(usize, f32)], blocks: &HashMap<usize, Block>, p: egui::Pos2| -> Option<(DocPos, Option<crate::text::Cl>, usize)> {
            let mut cand = None;
            for &(bi, top) in vis {
                if p.y >= top {
                    cand = Some((bi, top));
                }
            }
            let (bi, top) = cand.or(vis.first().copied())?;
            let b = blocks.get(&bi)?;
            let (lx, ly) = (p.x - inner.left(), p.y - top);
            let caret = b.laid.caret_at(lx, ly.clamp(0.0, b.laid.height - 0.1));
            let (ub, _) = b.laid.to_unit(caret);
            Some((DocPos { unit: bi, byte: ub }, b.laid.hit(lx, ly), bi))
        };

        let mut hover_hit: Option<Hit> = None;
        let mut hover_range: Option<(usize, usize, usize)> = None; // unit, start, end bytes
        if let Some(p) = ui.input(|i| i.pointer.hover_pos()).filter(|p| inner.contains(*p)) {
            if let Some((_, Some(cl), bi)) = pos_to_doc(&visible, &self.blocks, p) {
                let b = &self.blocks[&bi];
                let (ub, obj) = b.laid.to_unit(cl.start);
                if let Some(id) = obj {
                    match &b.objs[id as usize] {
                        Obj::Note(t) => hover_hit = Some(Hit::Note(t.clone())),
                        Obj::Verse(r) => hover_hit = Some(Hit::Verse(*r)),
                    }
                } else {
                    let u = &res.units[bi];
                    for sp in u.spans.iter().filter(|sp| sp.start <= ub && ub < sp.end) {
                        let h = match &sp.kind {
                            SpanKind::Strong(st) => Some(Hit::Strong { strong: st.clone(), word: u.text[sp.start..sp.end].to_string() }),
                            SpanKind::HebWord(i) => Some(Hit::Heb(*i)),
                            SpanKind::Ref(r) => Some(Hit::Ref(r.clone())),
                            _ => None,
                        };
                        if h.is_some() {
                            hover_hit = h;
                            hover_range = Some((bi, sp.start, sp.end));
                        }
                    }
                }
            }
        }
        self.hover_word = hover_range;
        match &hover_hit {
            Some(Hit::Ref(_)) | Some(Hit::Note(_)) => ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand),
            Some(_) | None if hovered && inner.contains(pointer.unwrap_or_default()) => ui.ctx().set_cursor_icon(egui::CursorIcon::Text),
            _ => {}
        }

        // selection
        if resp.drag_started() {
            if let Some(p) = resp.interact_pointer_pos().filter(|p| inner.contains(*p)) {
                if let Some((dp, _, _)) = pos_to_doc(&visible, &self.blocks, p) {
                    self.sel = Some((dp, dp));
                    self.selecting = true;
                }
            }
        }
        if self.selecting {
            if let Some(p) = pointer {
                if let Some((dp, _, _)) = pos_to_doc(&visible, &self.blocks, p) {
                    if let Some(sel) = &mut self.sel {
                        sel.1 = dp;
                    }
                }
            }
            if !ui.input(|i| i.pointer.primary_down()) {
                self.selecting = false;
            }
        }
        if resp.triple_clicked() || resp.double_clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                if let Some((dp, _, _)) = pos_to_doc(&visible, &self.blocks, p) {
                    let u = &res.units[dp.unit];
                    if resp.triple_clicked() {
                        // whole verse
                        if let Some(r) = res.verse_at(dp.unit, dp.byte) {
                            if let Some((a, e)) = res.verse_range(r) {
                                self.sel = Some((DocPos { unit: a.0, byte: a.1 }, DocPos { unit: e.0, byte: e.1 }));
                            }
                        }
                    } else {
                        use unicode_segmentation::UnicodeSegmentation;
                        for (wi, wd) in u.text.split_word_bound_indices() {
                            if dp.byte >= wi && dp.byte < wi + wd.len() {
                                self.sel = Some((DocPos { unit: dp.unit, byte: wi }, DocPos { unit: dp.unit, byte: wi + wd.len() }));
                            }
                        }
                    }
                }
            }
        } else if resp.clicked() {
            out.clicked = hover_hit.clone();
            if !matches!(hover_hit, Some(Hit::Ref(_)) | Some(Hit::Note(_))) {
                self.sel = None;
            }
        }
        if resp.clicked() || resp.drag_started() || out.user_moved {
            out.interacted = true;
        }
        out.hover = hover_hit;

        // copy from the model, never from painted text
        if focused {
            if let Some((a, b)) = self.sel {
                let (a, b) = if a <= b { (a, b) } else { (b, a) };
                if a != b && ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy))) {
                    let txt = slice_units(res, (a.unit, a.byte), (b.unit, b.byte));
                    let label = match (res.verse_at(a.unit, a.byte), res.verse_at(b.unit, b.byte.saturating_sub(1))) {
                        (Some(x), Some(y)) if x == y => format!("{} ({})", x.display(), res.abbrev),
                        (Some(x), Some(y)) if x.ch == y.ch => format!("{}–{} ({})", x.display(), y.v, res.abbrev),
                        (Some(x), Some(y)) => format!("{}–{}:{} ({})", x.display(), y.ch, y.v, res.abbrev),
                        _ => res.abbrev.clone(),
                    };
                    let txt = if res.kind == ResKind::Bible { format!("{txt}\n— {label}") } else { txt };
                    out.copy = Some(txt);
                }
            }
        }

        // paint
        let snap = |v: f32| (v * ppp).round() / ppp;
        let sel = self.sel.map(|(a, b)| if a <= b { (a, b) } else { (b, a) });
        let flash_alpha = self.flash.map(|(_, _, t)| (1.0 - ((now - t) as f32 / 1.6)).clamp(0.0, 1.0)).unwrap_or(0.0);
        if flash_alpha > 0.0 {
            ui.ctx().request_repaint();
        }
        let ranges_for = |bi: usize, laid: &Laid, a: DocPos, b: DocPos| -> Option<(usize, usize)> {
            if bi < a.unit || bi > b.unit {
                return None;
            }
            let p0 = if bi == a.unit { laid.from_unit(a.byte) } else { 0 };
            let p1 = if bi == b.unit { laid.from_unit(b.byte) } else { laid.text.len() };
            Some((p0, p1))
        };
        for &(bi, top) in &visible {
            let top = snap(top);
            let left = snap(inner.left());
            let block = self.blocks.get_mut(&bi).unwrap();
            // flash (navigation target), hover, selection: egui shapes under the texture
            if let Some((a, b, _)) = self.flash.filter(|_| flash_alpha > 0.0) {
                if let Some((p0, p1)) = ranges_for(bi, &block.laid, a, b) {
                    for r in block.laid.range_rects(p0, p1) {
                        painter.rect_filled(
                            Rect::from_min_max(pos2(left + r[0], top + r[2]), pos2(left + r[1], top + r[3])),
                            2.0,
                            Color32::from_rgba_unmultiplied(255, 200, 60, (110.0 * flash_alpha) as u8),
                        );
                    }
                }
            }
            if let Some((hu, hs, he)) = hover_range {
                if hu == bi {
                    let (p0, p1) = (block.laid.from_unit(hs), block.laid.from_unit(he));
                    for r in block.laid.range_rects(p0, p1) {
                        painter.rect_filled(
                            Rect::from_min_max(pos2(left + r[0], top + r[2] + 2.0), pos2(left + r[1], top + r[3] - 2.0)),
                            3.0,
                            Color32::from_rgb(232, 238, 252),
                        );
                    }
                }
            }
            if let Some((a, b)) = sel {
                if let Some((p0, p1)) = ranges_for(bi, &block.laid, a, b) {
                    for r in block.laid.range_rects(p0, p1) {
                        painter.rect_filled(
                            Rect::from_min_max(pos2(left + r[0], top + r[2]), pos2(left + r[1], top + r[3])),
                            0.0,
                            Color32::from_rgb(180, 210, 250),
                        );
                    }
                }
            }
            // texture
            let need = match &block.tex {
                Some((_, _, tp)) => *tp != ppp,
                None => true,
            };
            if need && *raster_budget > 0 {
                *raster_budget -= 1;
                let t = Instant::now();
                let (px, size) = raster(fonts, &block.laid, ppp);
                stats.rasters += 1;
                stats.raster_ms += t.elapsed().as_secs_f32() * 1000.0;
                let img = egui::ColorImage::from_rgba_premultiplied(size, &px);
                let tex = ui.ctx().load_texture(format!("{}-{bi}", res.id), img, egui::TextureOptions::LINEAR);
                block.tex = Some((tex, size, ppp));
            } else if need {
                ui.ctx().request_repaint();
            }
            if let Some((tex, size, tp)) = &block.tex {
                let r = Rect::from_min_size(pos2(left, top), vec2(size[0] as f32 / tp, size[1] as f32 / tp));
                painter.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            } else {
                // placeholder lines until rastered
                for l in &block.laid.lines {
                    let lw: f32 = l.items.iter().map(|i| i.w).sum();
                    let x0 = l.items.first().map(|i| i.x).unwrap_or(0.0);
                    painter.rect_filled(
                        Rect::from_min_size(pos2(left + x0, top + l.top + 6.0), vec2(lw, (l.bottom - l.top - 12.0).max(2.0))),
                        2.0,
                        Color32::from_gray(235),
                    );
                }
            }
        }
        if let Some(i) = lookahead {
            if *raster_budget > 0 {
                if let Some(block) = self.blocks.get_mut(&i) {
                    if block.tex.is_none() {
                        *raster_budget -= 1;
                        let t = Instant::now();
                        let (px, size) = raster(fonts, &block.laid, ppp);
                        stats.rasters += 1;
                        stats.raster_ms += t.elapsed().as_secs_f32() * 1000.0;
                        let img = egui::ColorImage::from_rgba_premultiplied(size, &px);
                        block.tex = Some((ui.ctx().load_texture(format!("{}-{i}", res.id), img, egui::TextureOptions::LINEAR), size, ppp));
                    }
                }
            } else {
                self.ensure_laid(res, fonts, s, i, w, stats);
            }
        }

        // evict blocks far from the viewport (~3 viewport heights either side)
        let first = visible.first().map(|v| v.0).unwrap_or(0);
        let last = visible.last().map(|v| v.0).unwrap_or(0);
        let (mut lo, mut hi) = (first, last);
        let mut acc = 0.0;
        while lo > 0 && acc < view_h * 3.0 {
            lo -= 1;
            acc += self.heights[lo];
        }
        acc = 0.0;
        while hi + 1 < n && acc < view_h * 3.0 {
            hi += 1;
            acc += self.heights[hi];
        }
        self.blocks.retain(|k, _| *k >= lo && *k <= hi);
        for b in self.blocks.values() {
            if let Some((_, size, _)) = &b.tex {
                stats.textures += 1;
                stats.texture_bytes += size[0] * size[1] * 4;
            }
        }

        // scrollbar
        let total: f32 = self.heights.iter().sum();
        let pos: f32 = self.heights[..self.anchor].iter().sum::<f32>() + self.anchor_off;
        painter.rect_filled(bar_rect, 0.0, Color32::from_gray(240));
        if total > view_h {
            let th = (view_h / total * bar_rect.height()).max(24.0);
            let ty = bar_rect.top() + (pos / (total - view_h)).clamp(0.0, 1.0) * (bar_rect.height() - th);
            let col = if bar_resp.hovered() || bar_resp.dragged() { Color32::from_gray(150) } else { Color32::from_gray(190) };
            painter.rect_filled(Rect::from_min_size(pos2(bar_rect.left() + 2.0, ty), vec2(BAR - 4.0, th)), 3.0, col);
        }
        let measured = self.measured.iter().filter(|m| **m).count();
        let _ = measured;
        out
    }

    pub fn measured_fraction(&self) -> f32 {
        if self.measured.is_empty() {
            return 0.0;
        }
        self.measured.iter().filter(|m| **m).count() as f32 / self.measured.len() as f32
    }
}

/// Inline-snippet mode: lay out + raster one paragraph, cached by key.
pub struct Snippets {
    cache: HashMap<(String, i32, u64), (TextureHandle, [usize; 2], f32, f32)>,
    used: HashMap<(String, i32, u64), u64>,
    frame: u64,
}

impl Snippets {
    pub fn new() -> Self {
        Snippets { cache: HashMap::new(), used: HashMap::new(), frame: 0 }
    }

    pub fn next_frame(&mut self) {
        self.frame += 1;
        let f = self.frame;
        let used = &self.used;
        self.cache.retain(|k, _| used.get(k).map(|u| f - *u < 120).unwrap_or(false));
    }

    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ui: &mut Ui,
        fonts: &Fonts,
        s: &TextSettings,
        text: &str,
        size: f32,
        color: [u8; 4],
        rtl: bool,
        width: f32,
    ) -> egui::Response {
        let ppp = ui.ctx().pixels_per_point();
        let key = (format!("{text}\u{0}{size}{color:?}{rtl}"), width.round() as i32, s.key() ^ (ppp * 100.0) as u64);
        self.used.insert(key.clone(), self.frame);
        if !self.cache.contains_key(&key) {
            let para = crate::present::snippet_para(text, size, color, rtl, s);
            let laid = layout(fonts, &para, width);
            let (px, sz) = raster(fonts, &laid, ppp);
            let img = egui::ColorImage::from_rgba_premultiplied(sz, &px);
            let tex = ui.ctx().load_texture("snippet", img, egui::TextureOptions::LINEAR);
            let used_w = laid.lines.iter().map(|l| l.items.iter().map(|i| i.w).sum::<f32>()).fold(0.0, f32::max);
            self.cache.insert(key.clone(), (tex, sz, ppp, used_w));
        }
        let (tex, sz, tp, _) = &self.cache[&key];
        let size = vec2(sz[0] as f32 / tp, sz[1] as f32 / tp);
        let (rect, resp) = ui.allocate_exact_size(vec2(width, size.y), Sense::click());
        ui.painter().image(tex.id(), Rect::from_min_size(rect.min, size), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        resp
    }
}
