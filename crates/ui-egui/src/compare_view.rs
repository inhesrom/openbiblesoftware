//! The Text Comparison pane (#18 rules 11–14): the engine's Verse blocks, one row per Bible,
//! drawn through the text pipeline (Hebrew rows are RTL) and virtualized like a reading pane.
//! Words that differ from the ★ base are tinted, and ‸ marks base words missing from a row.

use std::ops::Range;
use std::sync::Arc;

use egui::{Align2, Color32, FontId, Mesh, Pos2, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};
use obs_engine::{Cell, ComparedBible, Marks, VerseBlock, verse_blocks};
use obs_layout::{ColorRole, LaidBlock, TextSettings, UnitPos};
use obs_model::Direction;
use obs_scripture::{Verse, VerseMap};

use crate::doc::ReadingDoc;
use crate::heights::HeightIndex;
use crate::palette::Palette;
use crate::pane::AtlasTexture;
use crate::theme;
use crate::workers::SnippetCache;

/// The abbreviation column's width.
pub const LABEL_WIDTH: f32 = 86.0;
/// A Verse block's reference line.
const BLOCK_HEAD: f32 = 24.0;
/// Space below each row and below each block.
const ROW_GAP: f32 = 6.0;
const BLOCK_GAP: f32 = 10.0;
const MARGIN: f32 = 14.0;
const SCROLLBAR: f32 = 10.0;
/// What an Absent verse's row says (#10 rule 8).
pub const ABSENT: &str = "— not in this Bible —";
/// What a row in another language than the base says (#18 rule 14).
pub const NOT_COMPARED: &str = "not compared";

impl ComparedBible for ReadingDoc {
    fn language(&self) -> &str {
        &self.manifest.language
    }

    fn verse_map(&self) -> Option<&VerseMap> {
        self.verse_map.as_deref()
    }

    fn verse_at(&self, native: Verse) -> Option<(Verse, u16)> {
        self.bible_verse_at(native).map(|v| (v.verse, v.last))
    }

    fn verse_text(&self, native: Verse) -> Option<String> {
        ReadingDoc::verse_text(self, native)
            .map(|t| t.split_whitespace().collect::<Vec<_>>().join(" "))
    }
}

/// What the view compares; a change rebuilds the Verse blocks.
#[derive(Debug, Clone, PartialEq)]
struct Key {
    bibles: Vec<String>,
    base: Option<usize>,
    differences: bool,
    extent: Vec<Verse>,
}

/// One Text Comparison Tab's view: its Verse blocks, their heights and where it is scrolled.
pub struct ComparisonView {
    docs: Vec<Arc<ReadingDoc>>,
    key: Option<Key>,
    pub blocks: Vec<VerseBlock>,
    heights: HeightIndex,
    width: f32,
    settings: TextSettings,
    /// The top of the view: a block, and how far into it.
    anchor: (usize, f32),
    /// The location last taken from or reported to the engine.
    pub synced: Option<Verse>,
    snippets: SnippetCache,
}

impl ComparisonView {
    pub fn new(settings: TextSettings) -> Self {
        Self {
            docs: Vec::new(),
            key: None,
            blocks: Vec::new(),
            heights: HeightIndex::new(Vec::new()),
            width: 400.0,
            settings,
            anchor: (0, 0.0),
            synced: None,
            snippets: SnippetCache::default(),
        }
    }

    /// Compare `docs` (row order) over `extent`, marked against the row `base`. Rebuilds only
    /// when something changed; the view stays on the same Standard Verse.
    pub fn set(
        &mut self,
        docs: Vec<Arc<ReadingDoc>>,
        base: Option<usize>,
        differences: bool,
        extent: Vec<Verse>,
    ) {
        let key = Key {
            bibles: docs
                .iter()
                .map(|d| d.manifest.resource_id.clone())
                .collect(),
            base,
            differences,
            extent,
        };
        if self.key.as_ref() == Some(&key) {
            return;
        }
        let at = self.current_location();
        let bibles: Vec<&dyn ComparedBible> = docs
            .iter()
            .map(|d| d.as_ref() as &dyn ComparedBible)
            .collect();
        self.blocks = verse_blocks(&key.extent, &bibles, base, differences);
        self.docs = docs;
        self.key = Some(key);
        self.reset_heights();
        self.anchor = (0, 0.0);
        if let Some(v) = at {
            self.jump_to(v);
        }
    }

    pub fn set_settings(&mut self, settings: TextSettings) {
        if settings != self.settings {
            self.settings = settings;
            self.reset_heights();
        }
    }

    fn set_width(&mut self, width: f32) {
        if (width - self.width).abs() > 0.5 {
            self.width = width;
            self.reset_heights();
        }
    }

    fn reset_heights(&mut self) {
        let estimates = (0..self.blocks.len()).map(|i| self.estimate(i)).collect();
        self.heights = HeightIndex::new(estimates);
        self.anchor.1 = 0.0;
    }

    fn line_height(&self) -> f32 {
        self.settings.size * self.settings.line_spacing * 1.2
    }

    fn text_width(&self) -> f32 {
        (self.width - LABEL_WIDTH).max(60.0)
    }

    fn estimate(&self, i: usize) -> f32 {
        let Some(b) = self.blocks.get(i) else {
            return 0.0;
        };
        let per_line = (self.text_width() / (self.settings.size * 0.5)).max(10.0);
        let rows: f32 = b
            .cells
            .iter()
            .map(|c| {
                let lines = match &c.cell {
                    Cell::Text { text, .. } => (text.chars().count() as f32 / per_line).ceil(),
                    _ => 1.0,
                };
                lines.max(1.0) * self.line_height() + ROW_GAP
            })
            .sum();
        BLOCK_HEAD + rows + BLOCK_GAP
    }

    /// Move so `verse`'s block is at the top (or the nearest block before it).
    pub fn jump_to(&mut self, verse: Verse) {
        let i = self.blocks.partition_point(|b| b.verse <= verse);
        self.anchor = (i.saturating_sub(1), 0.0);
    }

    /// The Standard Verse whose block is at the top of the view.
    pub fn current_location(&self) -> Option<Verse> {
        self.blocks.get(self.anchor.0).map(|b| b.verse)
    }

    /// Scroll by `dy` px; the last block can reach the top.
    pub fn scroll_by(&mut self, dy: f32) {
        if self.blocks.is_empty() {
            return;
        }
        let last = self.blocks.len() - 1;
        let max = self.heights.offset(last);
        let y = (self.heights.offset(self.anchor.0) + f64::from(self.anchor.1) + f64::from(dy))
            .clamp(0.0, max);
        self.anchor = self.heights.at(y);
    }

    /// The scroll position and total height, for the scrollbar.
    fn position(&mut self) -> (f64, f64) {
        (
            self.heights.offset(self.anchor.0) + f64::from(self.anchor.1),
            self.heights.total(),
        )
    }

    fn rtl(&self, row: usize) -> bool {
        self.docs
            .get(row)
            .is_some_and(|d| d.direction() == Direction::Rtl)
    }

    fn abbreviation(&self, row: usize) -> &str {
        self.docs
            .get(row)
            .map_or("", |d| d.manifest.abbreviation.as_str())
    }

    pub fn has_pending(&self) -> bool {
        self.snippets.has_pending()
    }
}

/// A five-pointed star (the chrome fonts have no ★): filled for the base Bible, outlined
/// elsewhere.
pub fn paint_star(
    painter: &egui::Painter,
    center: Pos2,
    radius: f32,
    filled: bool,
    color: Color32,
) {
    let points: Vec<Pos2> = (0..10)
        .map(|i| {
            let r = if i % 2 == 0 { radius } else { radius * 0.45 };
            let a = std::f32::consts::PI * (i as f32 / 5.0 - 0.5);
            center + vec2(a.cos(), a.sin()) * r
        })
        .collect();
    let stroke = Stroke::new(1.2, color);
    if filled {
        // Convex fan pieces: the star is not convex, so fill each point's triangle and the core.
        let inner: Vec<Pos2> = points.iter().skip(1).step_by(2).copied().collect();
        painter.add(Shape::convex_polygon(inner, color, Stroke::NONE));
        for i in (0..10).step_by(2) {
            let tri = vec![points[(i + 9) % 10], points[i], points[i + 1]];
            painter.add(Shape::convex_polygon(tri, color, Stroke::NONE));
        }
    } else {
        painter.add(Shape::closed_line(points, stroke));
    }
}

/// What happened in the comparison pane this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ComparisonOutput {
    /// The user scrolled the view.
    pub user_moved: bool,
}

/// Highlight rectangles `[x0, y0, x1, y1]` of the clusters of `laid` whose text bytes fall in
/// `range`, merged along each line.
pub fn range_rects(laid: &LaidBlock, range: &Range<usize>) -> Vec<[f32; 4]> {
    let mut out = Vec::new();
    for line in &laid.lines {
        for item in &line.items {
            let mut run: Option<[f32; 4]> = None;
            for (bytes, x0, x1) in item.clusters() {
                let inside = matches!(laid.to_unit(bytes.start), Some(UnitPos::Text(b)) if range.contains(&b));
                match (&mut run, inside) {
                    (Some(r), true) if (r[2] - x0).abs() < 0.5 || (r[0] - x1).abs() < 0.5 => {
                        r[0] = r[0].min(x0);
                        r[2] = r[2].max(x1);
                    }
                    (_, true) => {
                        out.extend(run.take());
                        run = Some([x0, line.top, x1, line.bottom]);
                    }
                    (_, false) => out.extend(run.take()),
                }
            }
            out.extend(run);
        }
    }
    out
}

/// Where a ‸ goes for text byte `at`: before the word starting there, or after the last
/// word. Returns `[x, top, bottom]`.
pub fn caret_at(laid: &LaidBlock, at: usize) -> Option<[f32; 3]> {
    let mut before: Option<(usize, [f32; 3])> = None;
    let mut last: Option<(usize, [f32; 3])> = None;
    for line in &laid.lines {
        for item in &line.items {
            for (bytes, x0, x1) in item.clusters() {
                let Some(UnitPos::Text(b)) = laid.to_unit(bytes.start) else {
                    continue;
                };
                let (start, end) = if item.rtl { (x1, x0) } else { (x0, x1) };
                if b >= at && before.is_none_or(|(p, _)| b < p) {
                    before = Some((b, [start, line.top, line.bottom]));
                }
                if last.is_none_or(|(p, _)| b > p) {
                    last = Some((b, [end, line.top, line.bottom]));
                }
            }
        }
    }
    before.or(last).map(|(_, c)| c)
}

/// Draw the comparison into all the space `ui` has, and handle scrolling.
pub fn comparison_pane(
    ui: &mut Ui,
    view: &mut ComparisonView,
    atlas: &mut AtlasTexture,
    palette: &Palette,
) -> ComparisonOutput {
    let mut out = ComparisonOutput::default();
    let rect = ui.available_rect_before_wrap();
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, palette.page);
    let left = rect.left() + MARGIN;
    view.set_width(rect.width() - 2.0 * MARGIN - SCROLLBAR);

    let before = view.position().0;
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
        let (y, total) = view.position();
        let frac = f64::from(((p.y - track.top()) / track.height()).clamp(0.0, 1.0));
        view.scroll_by((frac * total - y) as f32);
    }
    out.user_moved = (view.position().0 - before).abs() > 0.01;

    if view.blocks.is_empty() {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Go to a passage, or link this Tab to a Bible",
            FontId::proportional(14.0),
            theme::c().muted,
        );
        return out;
    }

    let ppp = ui.ctx().pixels_per_point();
    let settings = view.settings;
    let text_left = left + LABEL_WIDTH;
    let text_width = view.text_width();
    let label_font = FontId::proportional(13.0);
    let small = FontId::proportional(11.5);
    let tint = palette.search_hit.gamma_multiply(0.75);
    let mut quads = Vec::new();
    let mut tints: Vec<Rect> = Vec::new();
    let mut carets: Vec<[f32; 3]> = Vec::new();
    let (first, offset) = view.anchor;
    let mut y = rect.top() - offset;
    let mut i = first;
    while i < view.blocks.len() && y < rect.bottom() {
        let block = view.blocks[i].clone();
        let mut row_y = y + BLOCK_HEAD;
        painter.text(
            pos2(left, y + 4.0),
            Align2::LEFT_TOP,
            obs_scripture::format_passage(
                obs_scripture::Passage::verse(block.verse),
                &obs_scripture::Standard,
                obs_scripture::RefStyle::Long,
            ),
            FontId::proportional(12.5),
            palette.verse_number,
        );
        let mut measured = true;
        for (row, c) in block.cells.iter().enumerate() {
            let rtl = view.rtl(row);
            let mut height = view.line_height();
            // The label column: abbreviation (★ for the base), the Bible's own number where it
            // differs, and "not compared".
            let label = painter.text(
                pos2(left, row_y + 2.0),
                Align2::LEFT_TOP,
                view.abbreviation(row),
                label_font.clone(),
                theme::c().ink,
            );
            if c.marks == Marks::Base {
                paint_star(
                    &painter,
                    pos2(label.right() + 9.0, label.center().y),
                    5.5,
                    true,
                    palette.verse_number,
                );
            }
            let mut label_h = 18.0;
            let notes = [
                c.cell.native_label(block.verse),
                (c.marks == Marks::NotCompared).then(|| NOT_COMPARED.to_string()),
            ];
            for note in notes.into_iter().flatten() {
                painter.text(
                    pos2(left, row_y + label_h + 1.0),
                    Align2::LEFT_TOP,
                    note,
                    small.clone(),
                    theme::c().muted,
                );
                label_h += 15.0;
            }
            match &c.cell {
                Cell::Text { text, .. } if !text.is_empty() => {
                    let laid = view
                        .snippets
                        .get(text, ColorRole::Ink, rtl, text_width, &settings);
                    match laid {
                        Some(laid) => {
                            if let Marks::Diff(d) = &c.marks {
                                for r in &d.changed {
                                    for [x0, y0, x1, y1] in range_rects(&laid, r) {
                                        tints.push(Rect::from_min_max(
                                            pos2(text_left + x0, row_y + y0),
                                            pos2(text_left + x1, row_y + y1),
                                        ));
                                    }
                                }
                                for &m in &d.missing {
                                    if let Some([x, t, b]) = caret_at(&laid, m) {
                                        carets.push([text_left + x, row_y + t, row_y + b]);
                                    }
                                }
                            }
                            quads.extend(atlas.atlas.quads(&laid, [text_left, row_y], ppp));
                            height = laid.height;
                        }
                        None => {
                            measured = false;
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
                }
                Cell::Text { .. } => {}
                Cell::Merged { into, native } => {
                    let label = format!(
                        "{}:{} is shown at {}:{}",
                        native.chapter, native.verse, into.chapter, into.verse
                    );
                    painter.text(
                        pos2(text_left, row_y + 3.0),
                        Align2::LEFT_TOP,
                        label,
                        label_font.clone(),
                        theme::c().muted,
                    );
                }
                Cell::Absent => {
                    painter.text(
                        pos2(text_left, row_y + 3.0),
                        Align2::LEFT_TOP,
                        ABSENT,
                        label_font.clone(),
                        theme::c().muted,
                    );
                }
            }
            row_y += height.max(label_h) + ROW_GAP;
        }
        let height = row_y - y + BLOCK_GAP;
        if measured {
            view.heights.set(i, height);
        }
        painter.hline(
            left..=rect.right() - MARGIN - SCROLLBAR,
            y + height - BLOCK_GAP / 2.0,
            Stroke::new(1.0, theme::c().rule),
        );
        y += height;
        i += 1;
    }
    if view.has_pending() {
        ui.ctx().request_repaint();
    }
    for r in tints {
        painter.rect_filled(r, 2.0, tint);
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
    for [x, _, b] in carets {
        let s = settings.size * 0.3;
        painter.line(
            vec![pos2(x - s, b), pos2(x, b - s * 1.4), pos2(x + s, b)],
            Stroke::new(1.6, palette.underline),
        );
    }

    let (pos, total) = view.position();
    if total > f64::from(rect.height()) {
        let frac = (pos / total) as f32;
        let thumb_h = (rect.height() * rect.height() / total as f32).clamp(24.0, rect.height());
        let thumb_top = track.top() + frac * (track.height() - thumb_h);
        painter.rect_filled(
            Rect::from_min_size(
                pos2(track.left() + 2.0, thumb_top),
                vec2(SCROLLBAR - 4.0, thumb_h),
            ),
            3.0,
            palette.verse_number.gamma_multiply(0.5),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use obs_engine::word_diff;
    use obs_layout::snippet_paragraph;
    use obs_scripture::Book;

    use super::*;
    use crate::workers::lay_out;

    const KJV: &str = "For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.";
    const WEB: &str = "For God so loved the world, that he gave his one and only Son, that whoever believes in him should not perish, but have eternal life.";

    fn laid(text: &str, rtl: bool, width: f32) -> LaidBlock {
        let para = snippet_paragraph(text, ColorRole::Ink, rtl, &TextSettings::default());
        lay_out(&para, width)
    }

    /// The x where `word` starts on the first line it is on.
    fn word_x(laid: &LaidBlock, text: &str, word: &str) -> f32 {
        let at = text.find(word).unwrap();
        range_rects(laid, &(at..at + word.len()))[0][0]
    }

    #[test]
    fn jn_3_16_web_differences_are_tinted_word_by_word_with_a_caret_before_son() {
        let d = word_diff(KJV, WEB);
        let laid = laid(WEB, false, 260.0);
        assert!(laid.lines.len() > 2, "wraps over several lines");
        // One tint per changed word ("one and" are adjacent but separate tokens).
        let tints: Vec<_> = d.changed.iter().map(|r| range_rects(&laid, r)).collect();
        assert_eq!(tints.len(), 5);
        for (r, rects) in d.changed.iter().zip(&tints) {
            assert_eq!(rects.len(), 1, "{:?}", &WEB[r.clone()]);
            let [x0, y0, x1, y1] = rects[0];
            assert!(x1 > x0 && y1 > y0);
        }
        // ‸ for the missing "begotten" sits at the left edge of "Son".
        let [caret] = d.missing[..] else {
            panic!("{:?}", d.missing)
        };
        let [x, top, bottom] = caret_at(&laid, caret).unwrap();
        assert!((x - word_x(&laid, WEB, "Son")).abs() < 0.5);
        assert!(bottom > top);
        // Missing words at the very end go after the last word.
        let [end_x, ..] = caret_at(&laid, WEB.len()).unwrap();
        let last = laid.lines.last().unwrap();
        let right = last.items.iter().map(|i| i.x + i.width).fold(0.0, f32::max);
        assert!(end_x <= right + 0.5 && end_x > right - 20.0);
    }

    #[test]
    fn hebrew_rows_are_laid_out_right_to_left() {
        let text = "בְּרֵאשִׁית בָּרָא אֱלֹהִים";
        let laid = laid(text, true, 400.0);
        let first = word_x(&laid, text, "בְּרֵאשִׁית");
        let second = word_x(&laid, text, "בָּרָא");
        assert!(first > second, "the first word is on the right");
        // A ‸ before the second word sits at its right edge.
        let at = text.find("בָּרָא").unwrap();
        let [x, ..] = caret_at(&laid, at).unwrap();
        assert!(x > second);
    }

    #[test]
    fn a_reading_doc_answers_as_a_compared_bible() {
        let doc = crate::tests::numbered_bible("heb-num", "Hebrew-numbered", "HNUM", true);
        let g = |c, v| Verse::new(Book::Genesis, c, v);
        let b: &dyn ComparedBible = doc.as_ref();
        assert_eq!(b.language(), "en");
        assert!(b.verse_map().is_some());
        assert_eq!(b.verse_at(g(32, 1)), Some((g(32, 1), 1)));
        assert_eq!(b.verse_at(g(31, 55)), None, "it ends chapter 31 at 54");
        let text = b.verse_text(g(32, 1)).unwrap();
        assert!(!text.is_empty() && !text.contains("  ") && text.ends_with('.'));
        assert_eq!(text.trim(), text);
    }
}
