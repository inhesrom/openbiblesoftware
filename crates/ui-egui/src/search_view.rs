//! The Search Tool's results (#59, #16 rule 11): a virtualized Verse list in Standard
//! canonical order. Each row is a reference and the matching text of each searched Bible,
//! drawn by the inline-snippet mode (Hebrew rows right to left) with the engine's highlights
//! painted under the glyphs. Only the rows in view are laid out (off the UI thread) and only
//! their pages of snippets are fetched.

use std::ops::Range;

use egui::{Align2, FontId, Mesh, Pos2, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};
use obs_engine::SearchTab;
use obs_layout::{ColorRole, TextSettings};

use crate::compare_view::range_rects;
use crate::heights::HeightIndex;
use crate::palette::Palette;
use crate::pane::AtlasTexture;
use crate::workers::SnippetCache;

/// The Bible label column's width.
pub const LABEL_WIDTH: f32 = 70.0;
/// A row's reference line.
const ROW_HEAD: f32 = 22.0;
const HIT_GAP: f32 = 4.0;
const ROW_GAP: f32 = 10.0;
const MARGIN: f32 = 12.0;
const SCROLLBAR: f32 = 10.0;

/// What the last frame drew.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchFrame {
    /// Rows in view.
    pub visible: Range<usize>,
    /// Hits in view still waiting for their snippet or its layout.
    pub placeholders: usize,
    /// Hits in view drawn with their text.
    pub drawn: usize,
}

/// A hit the user clicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchClick {
    pub row: usize,
    pub hit: usize,
    /// Ctrl/Cmd was held: open in a new Tab.
    pub new_tab: bool,
}

/// What happened in the results pane this frame.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchOutput {
    pub clicked: Option<SearchClick>,
    pub frame: SearchFrame,
}

/// One Search Tab's results view: row heights and where it is scrolled.
pub struct SearchView {
    /// The search shown (the Tab's generation) and its row count.
    shown: Option<(u64, usize)>,
    heights: HeightIndex,
    width: f32,
    viewport_height: f32,
    settings: TextSettings,
    /// The top of the view: a row, and how far into it.
    anchor: (usize, f32),
    /// Bring this row into view at the next frame.
    reveal: Option<usize>,
    snippets: SnippetCache,
    /// What the last frame drew.
    pub last: SearchFrame,
}

impl SearchView {
    pub fn new(settings: TextSettings) -> Self {
        Self {
            shown: None,
            heights: HeightIndex::new(Vec::new()),
            width: 400.0,
            viewport_height: 400.0,
            settings,
            anchor: (0, 0.0),
            reveal: None,
            snippets: SnippetCache::default(),
            last: SearchFrame::default(),
        }
    }

    pub fn set_settings(&mut self, settings: TextSettings) {
        if settings != self.settings {
            self.settings = settings;
            self.reset_heights();
        }
    }

    fn line_height(&self) -> f32 {
        self.settings.size * self.settings.line_spacing * 1.2
    }

    fn text_width(&self) -> f32 {
        (self.width - LABEL_WIDTH).max(60.0)
    }

    /// Rows are estimated at two lines per hit until laid out.
    fn estimate(&self, hits: usize) -> f32 {
        ROW_HEAD + hits as f32 * (2.0 * self.line_height() + HIT_GAP) + ROW_GAP
    }

    fn reset_heights(&mut self) {
        let rows = self.shown.map_or(0, |(_, n)| n);
        let per = self.estimate(1);
        self.heights = HeightIndex::new(vec![per; rows]);
        self.anchor.1 = 0.0;
    }

    /// Follow the Tab's latest search: a new one starts at the top.
    fn sync(&mut self, tab: &SearchTab) {
        let rows = tab.results.as_ref().map_or(0, |r| r.rows.len());
        let key = (tab.generation(), rows);
        if self.shown == Some(key) {
            return;
        }
        self.shown = Some(key);
        let estimates = tab.results.as_ref().map_or_else(Vec::new, |r| {
            r.rows
                .iter()
                .map(|row| self.estimate(row.hits.len()))
                .collect()
        });
        self.heights = HeightIndex::new(estimates);
        self.anchor = (0, 0.0);
        self.reveal = None;
    }

    fn set_width(&mut self, width: f32) {
        if (width - self.width).abs() > 0.5 {
            self.width = width;
            self.reset_heights();
        }
    }

    /// Scroll so `row` is in view at the next frame (↑/↓).
    pub fn reveal(&mut self, row: usize) {
        self.reveal = Some(row);
    }

    /// The row at the top of the view.
    pub fn top_row(&self) -> usize {
        self.anchor.0
    }

    pub fn scroll_by(&mut self, dy: f32) {
        if self.heights.is_empty() {
            return;
        }
        let max = self.max_scroll();
        let y = (self.heights.offset(self.anchor.0) + f64::from(self.anchor.1) + f64::from(dy))
            .clamp(0.0, max);
        self.anchor = self.heights.at(y);
    }

    fn max_scroll(&mut self) -> f64 {
        let last = self.heights.len().saturating_sub(1);
        self.heights
            .offset(last)
            .max(self.heights.total() - f64::from(self.viewport_height))
            .max(0.0)
    }

    fn position(&mut self) -> (f64, f64) {
        (
            self.heights.offset(self.anchor.0) + f64::from(self.anchor.1),
            self.heights.total(),
        )
    }

    fn apply_reveal(&mut self) {
        let Some(row) = self.reveal.take() else {
            return;
        };
        if row >= self.heights.len() {
            return;
        }
        let top = self.heights.offset(row);
        let bottom = top + f64::from(self.heights.height(row));
        let (y, _) = self.position();
        let view = f64::from(self.viewport_height);
        if top < y {
            self.anchor = (row, 0.0);
        } else if bottom > y + view {
            self.anchor = self.heights.at((bottom - view).min(top));
        }
    }

    /// Layouts still being made.
    pub fn has_pending(&self) -> bool {
        self.snippets.has_pending()
    }
}

/// Draw the results of `tab` into all the space `ui` has; handle scrolling and clicks.
pub fn search_pane(
    ui: &mut Ui,
    view: &mut SearchView,
    tab: &SearchTab,
    atlas: &mut AtlasTexture,
    palette: &Palette,
) -> SearchOutput {
    view.snippets.drain();
    view.sync(tab);
    let mut out = SearchOutput::default();
    let rect = ui.available_rect_before_wrap();
    if !rect.is_positive() {
        return out;
    }
    view.viewport_height = rect.height();
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let accessible = ui
        .ctx()
        .accesskit_node_builder(response.id, |n| {
            n.set_role(egui::accesskit::Role::List);
            n.set_label("Search results");
        })
        .is_some();
    painter.rect_filled(rect, 0.0, palette.page);
    let Some(results) = tab.results.clone() else {
        let message = if tab.running {
            "Searching…"
        } else {
            "Type a search above and press Enter"
        };
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            message,
            FontId::proportional(14.0),
            palette.verse_number,
        );
        view.last = SearchFrame::default();
        return out;
    };
    if results.rows.is_empty() {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "No verses match",
            FontId::proportional(14.0),
            palette.verse_number,
        );
        view.last = SearchFrame::default();
        return out;
    }
    let left = rect.left() + MARGIN;
    view.set_width(rect.width() - 2.0 * MARGIN - SCROLLBAR);
    if response.hovered() {
        let dy = ui.input(|i| i.smooth_scroll_delta.y);
        view.scroll_by(-dy);
        ui.input(|i| {
            if i.key_pressed(egui::Key::PageDown) {
                view.scroll_by(rect.height() * 0.9);
            }
            if i.key_pressed(egui::Key::PageUp) {
                view.scroll_by(-rect.height() * 0.9);
            }
        });
    }
    let track = Rect::from_min_max(pos2(rect.right() - SCROLLBAR, rect.top()), rect.max);
    if response.dragged()
        && let Some(p) = response.interact_pointer_pos()
        && track.expand(4.0).contains(p)
    {
        let (y, _) = view.position();
        let max = view.max_scroll();
        let frac = f64::from(((p.y - track.top()) / track.height()).clamp(0.0, 1.0));
        view.scroll_by((frac * max - y) as f32);
    }
    view.apply_reveal();

    let ppp = ui.ctx().pixels_per_point();
    let settings = view.settings;
    let text_left = left + LABEL_WIDTH;
    let text_width = view.text_width();
    let label_font = FontId::proportional(13.0);
    let small = FontId::proportional(11.0);
    let hit_tint = palette.search_hit;
    let hover = ui
        .input(|i| i.pointer.hover_pos())
        .filter(|p| rect.contains(*p));
    let mut quads = Vec::new();
    let mut tints: Vec<Rect> = Vec::new();
    let mut targets: Vec<(Rect, usize, usize)> = Vec::new();
    let mut row_rects: Vec<(Rect, usize)> = Vec::new();
    let mut frame = SearchFrame::default();
    let (first, offset) = view.anchor;
    let mut y = rect.top() - offset;
    let mut i = first;
    while i < results.rows.len() && y < rect.bottom() {
        let row = &results.rows[i];
        let background = painter.add(Shape::Noop);
        let reference = results.row_reference(row);
        painter.text(
            pos2(left, y + 4.0),
            Align2::LEFT_TOP,
            &reference,
            FontId::proportional(12.5),
            palette.verse_number,
        );
        let mut row_y = y + ROW_HEAD;
        let mut measured = true;
        for (j, h) in row.hits.iter().enumerate() {
            let bible = results.bibles.get(h.bible);
            let abbreviation = bible.map_or("", |b| b.abbreviation.as_str());
            let rtl = bible.is_some_and(|b| b.rtl);
            painter.text(
                pos2(left, row_y + 2.0),
                Align2::LEFT_TOP,
                abbreviation,
                label_font.clone(),
                palette.ink,
            );
            let mut label_h = 18.0;
            // The Bible's own number where it differs from the row's Standard Verse.
            if row.standard.is_some_and(|s| s != h.native) {
                painter.text(
                    pos2(left, row_y + label_h),
                    Align2::LEFT_TOP,
                    format!("{}:{}", h.native.chapter, h.native.verse),
                    small.clone(),
                    palette.verse_number,
                );
                label_h += 14.0;
            }
            let mut height = view.line_height();
            let mut laid_for_a11y = None;
            let snippet = tab.snippet(i, j);
            let laid = snippet.and_then(|s| {
                view.snippets
                    .get(&s.text, ColorRole::Ink, rtl, text_width, &settings)
            });
            match (snippet, laid) {
                (Some(s), Some(laid)) => {
                    for r in &s.highlights {
                        for [x0, y0, x1, y1] in range_rects(&laid, r) {
                            tints.push(Rect::from_min_max(
                                pos2(text_left + x0, row_y + y0),
                                pos2(text_left + x1, row_y + y1),
                            ));
                        }
                    }
                    quads.extend(atlas.atlas.quads(&laid, [text_left, row_y], ppp));
                    height = laid.height;
                    laid_for_a11y = Some((laid, s.text.clone()));
                    frame.drawn += 1;
                }
                _ => {
                    measured = false;
                    frame.placeholders += 1;
                    painter.rect_filled(
                        Rect::from_min_size(
                            pos2(text_left, row_y + height * 0.3),
                            vec2(text_width, height * 0.35),
                        ),
                        2.0,
                        palette.verse_number.gamma_multiply(0.12),
                    );
                }
            }
            height = height.max(label_h);
            let hit_rect = Rect::from_min_max(
                pos2(left, row_y),
                pos2(text_left + text_width, row_y + height),
            );
            targets.push((hit_rect, i, j));
            if accessible && let Some((laid, text)) = &laid_for_a11y {
                crate::a11y::publish_comparison_row(
                    ui,
                    response.id,
                    response.id.with(("search-hit", i, j)),
                    format!("{abbreviation} — {reference}"),
                    Rect::from_min_size(pos2(text_left, row_y), vec2(text_width, height)),
                    text,
                    Some(laid),
                );
            }
            row_y += height + HIT_GAP;
        }
        let height = row_y - y + ROW_GAP;
        if measured {
            view.heights.set(i, height);
        }
        let row_rect = Rect::from_min_max(
            pos2(rect.left(), y),
            pos2(rect.right() - SCROLLBAR, y + height - ROW_GAP / 2.0),
        );
        row_rects.push((row_rect, i));
        let fill = if tab.selected == Some(i) {
            Some(palette.selection)
        } else if hover.is_some_and(|p| row_rect.contains(p)) {
            Some(palette.hover)
        } else {
            None
        };
        if let Some(fill) = fill {
            painter.set(background, Shape::rect_filled(row_rect, 0.0, fill));
        }
        painter.hline(
            left..=rect.right() - MARGIN - SCROLLBAR,
            y + height - ROW_GAP / 2.0,
            Stroke::new(1.0, palette.verse_number.gamma_multiply(0.2)),
        );
        y += height;
        i += 1;
    }
    frame.visible = first..i;
    if view.has_pending() || frame.placeholders > 0 {
        ui.ctx().request_repaint();
    }
    for r in tints {
        painter.rect_filled(r, 2.0, hit_tint);
    }
    let texture = atlas.sync(ui.ctx());
    let mut mesh = Mesh::with_texture(texture);
    for q in quads {
        mesh.add_rect_with_uv(
            Rect::from_min_max(Pos2::from(q.min), Pos2::from(q.max)),
            Rect::from_min_max(Pos2::from(q.uv_min), Pos2::from(q.uv_max)),
            palette.color(q.role),
        );
    }
    painter.add(Shape::mesh(mesh));

    let (pos, total) = view.position();
    if total > f64::from(rect.height()) {
        let max = view.max_scroll().max(1.0);
        let frac = (pos / max) as f32;
        let thumb_h = (rect.height() * rect.height() / total as f32)
            .clamp(24.0_f32.min(rect.height()), rect.height());
        let thumb_top = track.top() + frac.clamp(0.0, 1.0) * (track.height() - thumb_h);
        painter.rect_filled(
            Rect::from_min_size(
                pos2(track.left() + 2.0, thumb_top),
                vec2(SCROLLBAR - 4.0, thumb_h),
            ),
            3.0,
            palette.verse_number.gamma_multiply(0.5),
        );
    }
    if response.clicked()
        && let Some(p) = response.interact_pointer_pos()
        && !track.contains(p)
    {
        let new_tab = ui.input(|i| i.modifiers.command);
        out.clicked = targets
            .iter()
            .find(|(r, _, _)| r.contains(p))
            .map(|&(_, row, hit)| SearchClick { row, hit, new_tab });
        // A click on a row's reference line opens its first Bible.
        if out.clicked.is_none() {
            out.clicked = row_rects
                .iter()
                .find(|(r, _)| r.contains(p))
                .map(|&(_, row)| SearchClick {
                    row,
                    hit: 0,
                    new_tab,
                });
        }
    }
    view.last = frame.clone();
    out.frame = frame;
    out
}
