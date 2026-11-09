//! The Search Tool's results: a virtualized list whose rows are laid out only when in view
//! (off the UI thread), with only their pages of snippets fetched. Text is drawn by the
//! inline-snippet mode with the engine's highlights painted under the glyphs.
//!
//! - **Bible mode** (#59, #16 rule 11): the Verse list in Standard canonical order. Each row
//!   is a reference and the matching text of each searched Bible (Hebrew rows right to left).
//! - **Library mode** (#60, #16 rule 12): paragraph rows grouped by Resource, a header above
//!   each group's first row. Each row is its unit's title or Key and an excerpt of the
//!   paragraph.

use std::ops::Range;

use egui::{Align2, FontId, Pos2, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};
use obs_engine::library_search::row_label;
use obs_engine::{BibleResults, LibraryResults, SearchTab};
use obs_layout::{ColorRole, TextSettings};

use crate::compare_view::range_rects;
use crate::heights::HeightIndex;
use crate::palette::Palette;
use crate::pane::{AtlasTexture, GlyphRun};
use crate::tokens::{alpha, radius, scale, share, size, space, stroke, text};
use crate::workers::SnippetCache;

/// The Bible label column's width.
pub const LABEL_WIDTH: f32 = size::LABEL_COLUMN;
/// A row's reference line.
const ROW_HEAD: f32 = size::CONTROL_COMPACT;
const HIT_GAP: f32 = space::XS;
const ROW_GAP: f32 = space::L;
/// A Library group's header line.
const GROUP_HEAD: f32 = size::ROW;
/// How far Library rows are indented under their group's header.
const INDENT: f32 = space::L;
/// Library row labels are drawn this much smaller than the text.
const LABEL_SCALE: f32 = scale::LABEL;
const MARGIN: f32 = space::L;
const SCROLLBAR: f32 = size::SCROLLBAR;

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
#[derive(Debug, Clone, Default)]
pub struct SearchOutput {
    pub clicked: Option<SearchClick>,
    pub frame: SearchFrame,
    /// The pane's own response (for its right-click menu).
    pub response: Option<egui::Response>,
    /// Right-clicked this frame (#142): the row, and the hit under the pointer (else 0).
    pub context: Option<(usize, usize)>,
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
    /// Where each row in view was drawn last frame (for tests and hit-testing).
    pub row_rects: Vec<(usize, Rect)>,
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
            row_rects: Vec::new(),
        }
    }

    pub fn set_settings(&mut self, settings: TextSettings) {
        if settings != self.settings {
            self.settings = settings;
            self.reset_heights();
        }
    }

    fn line_height(&self) -> f32 {
        self.settings.size * self.settings.line_spacing * scale::ROW_LINE
    }

    fn text_width(&self) -> f32 {
        (self.width - LABEL_WIDTH).max(size::TEXT_MIN)
    }

    /// Rows are estimated at two lines per hit until laid out.
    fn estimate(&self, hits: usize) -> f32 {
        ROW_HEAD + hits as f32 * (2.0 * self.line_height() + HIT_GAP) + ROW_GAP
    }

    /// A Library row: its label and three lines of excerpt (and its group's header).
    fn library_estimate(&self, starts_group: bool) -> f32 {
        let head = if starts_group { GROUP_HEAD } else { 0.0 };
        head + self.line_height() * (LABEL_SCALE + 3.0) + HIT_GAP + ROW_GAP
    }

    /// Estimated heights of the Tab's result rows.
    fn estimates(&self, tab: &SearchTab) -> Vec<f32> {
        match (&tab.results, &tab.library_results) {
            (Some(r), _) => r
                .rows
                .iter()
                .map(|row| self.estimate(row.hits.len()))
                .collect(),
            (None, Some(r)) => (0..r.rows.len())
                .map(|i| self.library_estimate(r.starts_group(i)))
                .collect(),
            (None, None) => Vec::new(),
        }
    }

    fn reset_heights(&mut self) {
        let rows = self.shown.map_or(0, |(_, n)| n);
        let per = self.estimate(1);
        self.heights = HeightIndex::new(vec![per; rows]);
        self.anchor.1 = 0.0;
    }

    /// Follow the Tab's latest search: a new one starts at the top.
    fn sync(&mut self, tab: &SearchTab) {
        let key = (tab.generation(), tab.row_count());
        if self.shown == Some(key) {
            return;
        }
        self.shown = Some(key);
        self.heights = HeightIndex::new(self.estimates(tab));
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

    /// Collect finished layouts while the list is not drawn (another results view is shown).
    pub fn drain(&mut self) {
        self.snippets.drain();
    }
}

/// What one frame paints, gathered row by row.
struct Paint<'a> {
    painter: &'a egui::Painter,
    palette: &'a Palette,
    accessible: bool,
    pane: egui::Id,
    runs: Vec<GlyphRun>,
    tints: Vec<Rect>,
    /// Hit areas: rect, row and hit.
    targets: Vec<(Rect, usize, usize)>,
    frame: SearchFrame,
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
    let center_message = |message: &str| {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            message,
            FontId::proportional(text::BODY),
            palette.verse_number,
        );
    };
    let bible = tab.results.clone();
    let library = tab.library_results.clone();
    if bible.is_none() && library.is_none() {
        center_message(if tab.running {
            "Searching…"
        } else {
            "Type a search above and press Enter"
        });
        view.last = SearchFrame::default();
        view.row_rects.clear();
        return out;
    }
    let rows = tab.row_count();
    if rows == 0 {
        center_message(if bible.is_some() {
            "No verses match"
        } else {
            "No paragraphs match"
        });
        view.last = SearchFrame::default();
        view.row_rects.clear();
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
        && track.expand(space::XS).contains(p)
    {
        let (y, _) = view.position();
        let max = view.max_scroll();
        let frac = f64::from(((p.y - track.top()) / track.height()).clamp(0.0, 1.0));
        view.scroll_by((frac * max - y) as f32);
    }
    view.apply_reveal();

    let hover = ui
        .input(|i| i.pointer.hover_pos())
        .filter(|p| rect.contains(*p));
    let mut paint = Paint {
        painter: &painter,
        palette,
        accessible,
        pane: response.id,
        runs: Vec::new(),
        tints: Vec::new(),
        targets: Vec::new(),
        frame: SearchFrame::default(),
    };
    let mut row_rects: Vec<(usize, Rect)> = Vec::new();
    let (first, offset) = view.anchor;
    let mut y = rect.top() - offset;
    let mut i = first;
    while i < rows && y < rect.bottom() {
        let background = painter.add(Shape::Noop);
        let (height, measured, top) = match (&bible, &library) {
            (Some(r), _) => bible_row(ui, view, tab, r, i, left, y, atlas, &mut paint),
            (None, Some(r)) => library_row(ui, view, tab, r, i, left, y, atlas, &mut paint),
            (None, None) => break,
        };
        if measured {
            view.heights.set(i, height);
        }
        let row_rect = Rect::from_min_max(
            pos2(rect.left(), top),
            pos2(rect.right() - SCROLLBAR, y + height - ROW_GAP / 2.0),
        );
        row_rects.push((i, row_rect));
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
        // In a High contrast page (#224) the selected row is framed in ink, so it shows
        // without its fill.
        if tab.selected == Some(i) && palette.high_contrast {
            painter.rect_stroke(
                row_rect,
                0.0,
                Stroke::new(stroke::MEDIUM, palette.ink),
                egui::StrokeKind::Inside,
            );
        }
        painter.hline(
            left..=rect.right() - MARGIN - SCROLLBAR,
            y + height - ROW_GAP / 2.0,
            Stroke::new(
                stroke::HAIR,
                palette.verse_number.gamma_multiply(alpha::TINT),
            ),
        );
        y += height;
        i += 1;
    }
    let Paint {
        runs,
        tints,
        targets,
        mut frame,
        ..
    } = paint;
    frame.visible = first..i;
    if view.has_pending() || frame.placeholders > 0 {
        ui.ctx().request_repaint();
    }
    for r in tints {
        painter.rect_filled(r, radius::XS, palette.search_hit);
    }
    crate::pane::paint_runs(&painter, runs, |q| palette.color(q.role));

    let (pos, total) = view.position();
    if total > f64::from(rect.height()) {
        let max = view.max_scroll().max(1.0);
        let frac = (pos / max) as f32;
        crate::components::scroll_thumb(
            &painter,
            track,
            frac,
            rect.height() / total as f32,
            palette.verse_number.gamma_multiply(alpha::HALF),
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
        // A click elsewhere on a row (a Bible row's reference line) opens its first hit.
        if out.clicked.is_none() {
            out.clicked = row_rects
                .iter()
                .find(|(_, r)| r.contains(p))
                .map(|&(row, _)| SearchClick {
                    row,
                    hit: 0,
                    new_tab,
                });
        }
    }
    if response.secondary_clicked()
        && let Some(p) = response.interact_pointer_pos()
    {
        out.context = targets
            .iter()
            .find(|(r, _, _)| r.contains(p))
            .map(|&(_, row, hit)| (row, hit))
            .or_else(|| {
                row_rects
                    .iter()
                    .find(|(_, r)| r.contains(p))
                    .map(|&(row, _)| (row, 0))
            });
    }
    view.last = frame.clone();
    view.row_rects = row_rects;
    out.frame = frame;
    out.response = Some(response);
    out
}

/// Paint a laid snippet at `origin` with its highlights; returns its height.
fn paint_laid(
    paint: &mut Paint<'_>,
    atlas: &mut AtlasTexture,
    laid: &obs_layout::LaidBlock,
    highlights: &[Range<usize>],
    origin: Pos2,
) -> f32 {
    for r in highlights {
        for [x0, y0, x1, y1] in range_rects(laid, r) {
            paint.tints.push(Rect::from_min_max(
                pos2(origin.x + x0, origin.y + y0),
                pos2(origin.x + x1, origin.y + y1),
            ));
        }
    }
    let painter = paint.painter;
    (paint.runs).extend(atlas.runs(painter.ctx(), laid, origin, painter.clip_rect()));
    laid.height
}

/// A grey bar where text will be once its snippet or layout arrives.
fn placeholder(paint: &mut Paint<'_>, origin: Pos2, width: f32, height: f32) {
    paint.frame.placeholders += 1;
    crate::components::text_placeholder(
        paint.painter,
        origin,
        width,
        height,
        paint.palette.verse_number.gamma_multiply(alpha::WASH),
    );
}

/// One Verse row: the reference, then each Bible's label and text. Returns its height,
/// whether it is fully laid out, and where its highlight area starts.
#[allow(clippy::too_many_arguments)]
fn bible_row(
    ui: &mut Ui,
    view: &mut SearchView,
    tab: &SearchTab,
    results: &BibleResults,
    i: usize,
    left: f32,
    y: f32,
    atlas: &mut AtlasTexture,
    paint: &mut Paint<'_>,
) -> (f32, bool, f32) {
    let palette = paint.palette;
    let settings = view.settings;
    let text_left = left + LABEL_WIDTH;
    let text_width = view.text_width();
    let row = &results.rows[i];
    let reference = results.row_reference(row);
    paint.painter.text(
        pos2(left, y + space::XS),
        Align2::LEFT_TOP,
        &reference,
        FontId::proportional(text::LABEL),
        palette.verse_number,
    );
    let mut row_y = y + ROW_HEAD;
    let mut measured = true;
    for (j, h) in row.hits.iter().enumerate() {
        let bible = results.bibles.get(h.bible);
        let abbreviation = bible.map_or("", |b| b.abbreviation.as_str());
        let rtl = bible.is_some_and(|b| b.rtl);
        paint.painter.text(
            pos2(left, row_y + space::XXS),
            Align2::LEFT_TOP,
            abbreviation,
            FontId::proportional(text::LABEL),
            palette.ink,
        );
        let mut label_h = text::LABEL_LINE;
        // The Bible's own number where it differs from the row's Standard Verse.
        if row.standard.is_some_and(|s| s != h.native) {
            paint.painter.text(
                pos2(left, row_y + label_h),
                Align2::LEFT_TOP,
                format!("{}:{}", h.native.chapter, h.native.verse),
                FontId::proportional(text::CAPTION),
                palette.verse_number,
            );
            label_h += text::CAPTION_LINE;
        }
        let mut height = view.line_height();
        let origin = pos2(text_left, row_y);
        let snippet = tab.snippet(i, j);
        let laid = snippet.and_then(|s| {
            view.snippets
                .get(&s.text, ColorRole::Ink, rtl, text_width, &settings)
        });
        match (snippet, laid) {
            (Some(s), Some(laid)) => {
                height = paint_laid(paint, atlas, &laid, &s.highlights, origin);
                paint.frame.drawn += 1;
                if paint.accessible {
                    crate::a11y::publish_comparison_row(
                        ui,
                        paint.pane,
                        paint.pane.with(("search-hit", i, j)),
                        format!("{abbreviation} — {reference}"),
                        Rect::from_min_size(origin, vec2(text_width, height)),
                        &s.text,
                        Some(&laid),
                    );
                }
            }
            _ => {
                measured = false;
                placeholder(paint, origin, text_width, height);
            }
        }
        height = height.max(label_h);
        let hit_rect = Rect::from_min_max(
            pos2(left, row_y),
            pos2(text_left + text_width, row_y + height),
        );
        paint.targets.push((hit_rect, i, j));
        row_y += height + HIT_GAP;
    }
    (row_y - y + ROW_GAP, measured, y)
}

/// One Library row: its group's header if it is the group's first, then the unit's title or
/// Key and the paragraph's excerpt. Returns its height, whether it is fully laid out, and
/// where its highlight area starts (below the header).
#[allow(clippy::too_many_arguments)]
fn library_row(
    ui: &mut Ui,
    view: &mut SearchView,
    tab: &SearchTab,
    results: &LibraryResults,
    i: usize,
    left: f32,
    y: f32,
    atlas: &mut AtlasTexture,
    paint: &mut Paint<'_>,
) -> (f32, bool, f32) {
    let palette = paint.palette;
    let settings = view.settings;
    let mut label_settings = settings;
    label_settings.size *= LABEL_SCALE;
    let row = &results.rows[i];
    let group = results.group_of(i);
    let mut row_y = y;
    if results.starts_group(i)
        && let Some(g) = group
    {
        paint.painter.text(
            pos2(left, y + space::M),
            Align2::LEFT_TOP,
            results.group_header(g),
            FontId::proportional(text::LABEL),
            palette.ink,
        );
        row_y += GROUP_HEAD;
    }
    let top = row_y;
    let text_left = left + INDENT;
    let text_width = (view.width - INDENT).max(size::TEXT_MIN);
    let rtl = group.is_some_and(|g| g.rtl);
    let mut measured = true;
    // The unit's title or Key, through the text pipeline (Lexicon titles are Greek or Hebrew).
    let label = row_label(row);
    let origin = pos2(text_left, row_y + space::XXS);
    let label_h = match view.snippets.get(
        &label,
        ColorRole::Accent,
        false,
        text_width,
        &label_settings,
    ) {
        Some(laid) => paint_laid(paint, atlas, &laid, &[], origin),
        None => {
            measured = false;
            let h = view.line_height() * LABEL_SCALE;
            placeholder(paint, origin, text_width * share::LABEL, h);
            h
        }
    };
    row_y += space::XXS + label_h + space::XXS;
    let mut height = view.line_height();
    let origin = pos2(text_left, row_y);
    let snippet = tab.snippet(i, 0);
    let laid = snippet.and_then(|s| {
        view.snippets
            .get(&s.text, ColorRole::Ink, rtl, text_width, &settings)
    });
    match (snippet, laid) {
        (Some(s), Some(laid)) => {
            height = paint_laid(paint, atlas, &laid, &s.highlights, origin);
            paint.frame.drawn += 1;
            if paint.accessible {
                let abbreviation = group.map_or("", |g| g.abbreviation.as_str());
                crate::a11y::publish_comparison_row(
                    ui,
                    paint.pane,
                    paint.pane.with(("search-hit", i, 0)),
                    format!("{abbreviation} — {label}"),
                    Rect::from_min_size(origin, vec2(text_width, height)),
                    &s.text,
                    Some(&laid),
                );
            }
        }
        _ => {
            measured = false;
            placeholder(paint, origin, text_width, height);
        }
    }
    row_y += height + HIT_GAP;
    let hit_rect = Rect::from_min_max(pos2(left, top), pos2(text_left + text_width, row_y));
    paint.targets.push((hit_rect, i, 0));
    (row_y - y + ROW_GAP, measured, top)
}
