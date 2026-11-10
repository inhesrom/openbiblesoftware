//! The Text Comparison pane (#18 rules 11–14): the engine's Verse blocks, one row per Bible,
//! drawn through the text pipeline (Hebrew rows are RTL) and virtualized like a reading pane.
//! Words that differ from the ★ base are tinted, and ‸ marks base words missing from a row.

use std::ops::Range;
use std::sync::Arc;

use egui::{Align2, FontId, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use obs_engine::{Cell, ComparedBible, CopiedBible, Marks, VerseBlock, verse_blocks};
use obs_layout::{ColorRole, LaidBlock, TextSettings, UnitPos};
use obs_model::Direction;
use obs_scripture::{Verse, VerseMap};

use crate::doc::ReadingDoc;
use crate::find::Found;
use crate::heights::HeightIndex;
use crate::palette::Palette;
use crate::pane::AtlasTexture;
use crate::tokens::{alpha, mark, radius, scale, size, space, stroke, text};
use crate::workers::SnippetCache;

/// The abbreviation column's width.
pub const LABEL_WIDTH: f32 = size::LABEL_COLUMN + space::XL;
/// A Verse block's reference line.
const BLOCK_HEAD: f32 = size::ROW_COMPACT;
/// Space below each row and below each block.
const ROW_GAP: f32 = space::S;
const BLOCK_GAP: f32 = space::L;
const MARGIN: f32 = space::L;
const SCROLLBAR: f32 = size::SCROLLBAR;
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

impl CopiedBible for ReadingDoc {
    fn manifest(&self) -> &obs_model::Manifest {
        &self.manifest
    }

    fn versification(&self) -> &dyn obs_scripture::Versification {
        &self.versification
    }

    fn starts_paragraph(&self, native: Verse) -> bool {
        ReadingDoc::starts_paragraph(self, native)
    }
}

/// A selected Bible keeps its row even when its Resource cannot be loaded.
pub struct ComparisonBible {
    pub id: String,
    pub abbreviation: String,
    pub language: String,
    pub doc: Result<Arc<ReadingDoc>, String>,
}

impl ComparedBible for ComparisonBible {
    fn language(&self) -> &str {
        &self.language
    }

    fn verse_map(&self) -> Option<&VerseMap> {
        self.doc.as_ref().ok().and_then(|d| d.verse_map())
    }

    fn verse_at(&self, native: Verse) -> Option<(Verse, u16)> {
        self.doc.as_ref().ok().and_then(|d| d.verse_at(native))
    }

    fn verse_text(&self, native: Verse) -> Option<String> {
        self.doc
            .as_ref()
            .ok()
            .and_then(|d| ComparedBible::verse_text(d.as_ref(), native))
    }
}

impl From<Arc<ReadingDoc>> for ComparisonBible {
    fn from(doc: Arc<ReadingDoc>) -> Self {
        Self {
            id: doc.manifest.resource_id.clone(),
            abbreviation: doc.manifest.abbreviation.clone(),
            language: doc.manifest.language.clone(),
            doc: Ok(doc),
        }
    }
}

/// What the view compares; a change rebuilds the Verse blocks.
#[derive(Debug, Clone, PartialEq)]
struct Key {
    bibles: Vec<(String, Option<String>)>,
    base: Option<usize>,
    differences: bool,
    extent: Vec<Verse>,
}

/// One Text Comparison Tab's view: its Verse blocks, their heights and where it is scrolled.
pub struct ComparisonView {
    docs: Vec<ComparisonBible>,
    key: Option<Key>,
    pub blocks: Vec<VerseBlock>,
    heights: HeightIndex,
    width: f32,
    /// Where the pane was drawn last frame.
    viewport: Rect,
    settings: TextSettings,
    /// The top of the view: a block, and how far into it.
    anchor: (usize, f32),
    /// The location last taken from or reported to the engine.
    pub synced: Option<Verse>,
    snippets: SnippetCache,
    /// Bumped whenever the Verse blocks are made again (a Find bar searches them anew, #175).
    generation: u64,
    /// The Find bar's current match is to be scrolled into view once drawn (#175).
    revealing: bool,
    /// Rows in view the last frame drew as placeholders, waiting for their layout (`None`:
    /// not drawn since its text settings changed).
    placeholders: Option<usize>,
}

impl ComparisonView {
    pub fn new(settings: TextSettings) -> Self {
        Self {
            docs: Vec::new(),
            key: None,
            blocks: Vec::new(),
            heights: HeightIndex::new(Vec::new()),
            width: 400.0,
            viewport: Rect::from_min_size(Pos2::ZERO, vec2(400.0, 400.0)),
            settings,
            anchor: (0, 0.0),
            synced: None,
            snippets: SnippetCache::default(),
            generation: 0,
            revealing: false,
            placeholders: None,
        }
    }

    /// Compare `docs` (row order) over `extent`, marked against the row `base`. Rebuilds only
    /// when something changed; the view stays on the same Standard Verse.
    pub fn set(
        &mut self,
        docs: Vec<ComparisonBible>,
        base: Option<usize>,
        differences: bool,
        extent: Vec<Verse>,
    ) {
        let key = Key {
            bibles: docs
                .iter()
                .map(|d| (d.id.clone(), d.doc.as_ref().err().cloned()))
                .collect(),
            base,
            differences,
            extent,
        };
        if self.key.as_ref() == Some(&key) {
            return;
        }
        let at = self.current_location();
        let bibles: Vec<&dyn ComparedBible> =
            docs.iter().map(|d| d as &dyn ComparedBible).collect();
        let base_available = base
            .and_then(|i| docs.get(i))
            .is_some_and(|d| d.doc.is_ok());
        self.blocks = verse_blocks(&key.extent, &bibles, base, differences && base_available);
        self.generation += 1;
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
            // Rows queued for layout at the old settings are not wanted any more (#299).
            self.snippets.forget_pending();
            self.placeholders = None;
            self.reset_heights();
        }
    }

    pub fn settings(&self) -> &TextSettings {
        &self.settings
    }

    /// Whether a frame has drawn every row in view since the text settings last changed,
    /// none as a placeholder (the Text size bench's "change shown", #299).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn drew_all_text(&self) -> bool {
        self.placeholders == Some(0)
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
        self.settings.size * self.settings.line_spacing * scale::ROW_LINE
    }

    fn text_width(&self) -> f32 {
        (self.width - LABEL_WIDTH).max(size::TEXT_MIN)
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

    /// Which version of its Verse blocks it shows: it changes whenever they are made again.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The text of every row with text, as (Verse block, row, text), for its Find bar.
    pub(crate) fn find_rows(&self) -> Vec<(usize, usize, String)> {
        let mut rows = Vec::new();
        for (b, block) in self.blocks.iter().enumerate() {
            for (r, c) in block.cells.iter().enumerate() {
                if let Cell::Text { text, .. } = &c.cell
                    && !text.is_empty()
                {
                    rows.push((b, r, text.clone()));
                }
            }
        }
        rows
    }

    /// Where the pane was drawn last frame.
    #[cfg(test)]
    pub(crate) fn viewport(&self) -> Rect {
        self.viewport
    }

    /// Verse block `i`'s height (measured once drawn, else estimated).
    #[cfg(test)]
    pub(crate) fn block_height(&self, i: usize) -> f32 {
        self.heights.height(i)
    }

    /// The Verse block at the top of the view.
    pub fn top_block(&self) -> usize {
        self.anchor.0
    }

    /// Show the Find bar's current match, in Verse block `i` (#175): bring the block to the
    /// top unless the pane draws it from where the view is, then let the pane scroll within
    /// the block until the match's line is in view (a block can be taller than the view).
    /// True when the view moved.
    pub(crate) fn reveal_match(&mut self, i: usize) -> bool {
        if i >= self.blocks.len() {
            return false;
        }
        self.revealing = true;
        let top = self.heights.offset(i)
            - (self.heights.offset(self.anchor.0) + f64::from(self.anchor.1));
        if i == self.anchor.0 || (i > self.anchor.0 && top < f64::from(self.viewport.height())) {
            return false;
        }
        self.anchor = (i, 0.0);
        true
    }

    /// After a frame drew the current Find match of Verse block `block` at `at` (the block's
    /// top, the match's top and bottom, in screen points), or did not draw it (`pending`: its
    /// row is still being laid out): scroll so it is in view. True when the view moved.
    fn reveal_drawn(&mut self, block: usize, at: Option<(f32, f32, f32)>, pending: bool) -> bool {
        let to = match at {
            Some((_, top, bottom))
                if top >= self.viewport.top() && bottom <= self.viewport.bottom() =>
            {
                None
            }
            // Its line at the top, below a block heading's worth of context.
            Some((block_top, top, _)) => Some((block, (top - block_top - BLOCK_HEAD).max(0.0))),
            // Not laid out yet: try again next frame.
            None if pending => return false,
            None => Some((block, 0.0)),
        };
        match to {
            Some(to) if to != self.anchor => {
                self.anchor = to;
                true
            }
            _ => {
                self.revealing = false;
                false
            }
        }
    }

    /// The Standard Verse whose block is at the top of the view.
    pub fn current_location(&self) -> Option<Verse> {
        self.blocks.get(self.anchor.0).map(|b| b.verse)
    }

    /// Scroll by `dy` px; the last block can reach the top, then expose its overflow.
    pub fn scroll_by(&mut self, dy: f32) {
        if self.blocks.is_empty() {
            return;
        }
        let max = self.max_scroll();
        let y = (self.heights.offset(self.anchor.0) + f64::from(self.anchor.1) + f64::from(dy))
            .clamp(0.0, max);
        self.anchor = self.heights.at(y);
    }

    fn max_scroll(&mut self) -> f64 {
        self.heights
            .offset(self.blocks.len().saturating_sub(1))
            .max(self.heights.total() - f64::from(self.viewport.height()))
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
            .and_then(|d| d.doc.as_ref().ok())
            .is_some_and(|d| d.direction() == Direction::Rtl)
    }

    fn abbreviation(&self, row: usize) -> &str {
        self.docs.get(row).map_or("", |d| d.abbreviation.as_str())
    }

    pub fn has_pending(&self) -> bool {
        self.snippets.has_pending()
    }
}

/// What happened in the comparison pane this frame.
#[derive(Debug, Clone, Default)]
pub struct ComparisonOutput {
    /// The user scrolled the view.
    pub user_moved: bool,
    /// The view scrolled to bring the current Find match into view (#175).
    pub find_moved: bool,
    /// The pane's own response (for its right-click menu).
    pub response: Option<egui::Response>,
    /// The Verse block right-clicked this frame (#142).
    pub context: Option<ComparisonContext>,
}

/// A right-click on a Verse block: its Standard Verse and, on a row with text, that row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComparisonContext {
    pub verse: Verse,
    pub row: Option<ComparisonRow>,
}

/// One Bible's text in a Verse block, as Copy with reference copies it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComparisonRow {
    /// The Bible's resource id and abbreviation.
    pub bible: String,
    pub abbreviation: String,
    /// Its own Verses in the block (its numbering may differ from the block's Standard
    /// Verse), with the last verse number each covers.
    pub natives: Vec<(Verse, u16)>,
    pub text: String,
}

/// Highlight rectangles `[x0, y0, x1, y1]` of the clusters of `laid` whose text bytes reach
/// into `range` (all of a ligature when the range starts or ends inside it), merged along
/// each line: one rectangle per stretch of a line, not per word, so a band has no notches
/// where words meet (#305).
pub fn range_rects(laid: &LaidBlock, range: &Range<usize>) -> Vec<[f32; 4]> {
    let mut out = Vec::new();
    if range.is_empty() {
        return out;
    }
    for line in &laid.lines {
        let mut run: Option<[f32; 4]> = None;
        for item in &line.items {
            for (bytes, x0, x1) in item.clusters() {
                let inside = matches!(laid.to_unit(bytes.start),
                    Some(UnitPos::Text(b)) if b < range.end && range.start < b + bytes.len());
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
        }
        out.extend(run);
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
    found: Found<'_>,
) -> ComparisonOutput {
    // Navigation can leave only Absent rows (or no Bibles), so `get` may never run.
    view.snippets.drain();
    let mut out = ComparisonOutput::default();
    let rect = ui.available_rect_before_wrap();
    if !rect.is_positive() {
        return out;
    }
    view.viewport = rect;
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    let context_at = response
        .secondary_clicked()
        .then(|| response.interact_pointer_pos())
        .flatten();
    let painter = ui.painter_at(rect);
    let accessible = ui
        .ctx()
        .accesskit_node_builder(response.id, |n| {
            n.set_role(egui::accesskit::Role::Document);
            n.set_label("Text Comparison");
        })
        .is_some();
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
        && track.expand(space::XS).contains(p)
    {
        let (y, _) = view.position();
        let max = view.max_scroll();
        let frac = f64::from(((p.y - track.top()) / track.height()).clamp(0.0, 1.0));
        view.scroll_by((frac * max - y) as f32);
    }
    out.user_moved = (view.position().0 - before).abs() > 0.01;
    if out.user_moved {
        view.revealing = false;
    }
    // The current Find match, while it is being scrolled into view: its (Verse block, row).
    let reveal = found
        .current
        .and_then(|k| found.matches.get(k))
        .filter(|_| view.revealing)
        .map(|m| (m.unit, m.row));
    // Where it was drawn (block top, its top and bottom), or whether its row awaits layout.
    let mut revealed: Option<(f32, f32, f32)> = None;
    let mut reveal_pending = false;

    if view.blocks.is_empty() {
        out.response = Some(response);
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Go to a passage, or link this Tab to a Bible",
            FontId::proportional(text::BODY),
            palette.verse_number,
        );
        return out;
    }

    let settings = view.settings;
    let text_left = left + LABEL_WIDTH;
    let text_width = view.text_width();
    let label_font = FontId::proportional(text::LABEL);
    let small = FontId::proportional(text::SMALL);
    let tint = palette.search_hit.gamma_multiply(alpha::STRONG);
    let mut runs = Vec::new();
    let mut tints: Vec<Rect> = Vec::new();
    // Find matches (#175): their rectangles, and whether each is the current one.
    let mut matches: Vec<(Rect, bool)> = Vec::new();
    let mut carets: Vec<[f32; 3]> = Vec::new();
    let (first, offset) = view.anchor;
    let mut y = rect.top() - offset;
    let mut i = first;
    let mut placeholders = 0;
    while i < view.blocks.len() && y < rect.bottom() {
        let block = view.blocks[i].clone();
        let mut row_y = y + BLOCK_HEAD;
        let reference = obs_scripture::format_passage(
            obs_scripture::Passage::verse(block.verse),
            &obs_scripture::Standard,
            obs_scripture::RefStyle::Long,
        );
        painter.text(
            pos2(left, y + space::XS),
            Align2::LEFT_TOP,
            &reference,
            FontId::proportional(text::LABEL),
            palette.verse_number,
        );
        let mut measured = true;
        let mut row_context = None;
        for (row, c) in block.cells.iter().enumerate() {
            let rtl = view.rtl(row);
            let mut height = view.line_height();
            let mut accessible_laid = None;
            let mut accessible_text = String::new();
            let mut accessible_label = format!("{} — {reference}", view.abbreviation(row));
            // The label column: abbreviation (a star for the base), the Bible's own number where it
            // differs, and "not compared".
            let label = painter.text(
                pos2(left, row_y + space::XXS),
                Align2::LEFT_TOP,
                view.abbreviation(row),
                label_font.clone(),
                palette.ink,
            );
            let is_base = view.key.as_ref().is_some_and(|k| k.base == Some(row));
            if is_base {
                accessible_label.push_str(" — base Bible");
                let at = Rect::from_center_size(
                    pos2(label.right() + space::L, label.center().y),
                    egui::Vec2::splat(crate::icons::SMALL),
                );
                crate::icons::paint(
                    &painter,
                    crate::icons::Icon::StarFilled,
                    at,
                    crate::icons::SMALL,
                    crate::icons::page_muted(palette),
                );
            }
            let mut label_h = text::LABEL_LINE;
            let notes = [
                c.cell.native_label(block.verse),
                (c.marks == Marks::NotCompared).then(|| NOT_COMPARED.to_string()),
            ];
            for note in notes.into_iter().flatten() {
                accessible_label.push_str(&format!(" — {note}"));
                painter.text(
                    pos2(left, row_y + label_h + space::PX),
                    Align2::LEFT_TOP,
                    note,
                    small.clone(),
                    palette.verse_number,
                );
                label_h += text::CAPTION_LINE;
            }
            if let Some(error) = view.docs[row].doc.as_ref().err() {
                // The plain message on the page (#223); screen readers get the error too.
                let message = if is_base {
                    "Base Bible unavailable: it could not be opened, so differences are not shown."
                } else {
                    "Bible unavailable: it could not be opened."
                };
                let galley =
                    painter.layout(message.into(), label_font.clone(), palette.ink, text_width);
                height = galley.size().y;
                painter.galley(pos2(text_left, row_y + space::XS), galley, palette.ink);
                accessible_text = format!("{message} {error}");
            } else {
                match &c.cell {
                    Cell::Text { text, .. } if !text.is_empty() => {
                        let laid =
                            view.snippets
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
                                for k in found.within(i, row, 0..text.len()) {
                                    let current = found.current == Some(k);
                                    for [x0, y0, x1, y1] in
                                        range_rects(&laid, &found.matches[k].range)
                                    {
                                        if current && reveal.is_some() {
                                            let (t, b) = revealed
                                                .map_or((row_y + y0, row_y + y1), |(_, t, b)| {
                                                    (t.min(row_y + y0), b.max(row_y + y1))
                                                });
                                            revealed = Some((y, t, b));
                                        }
                                        matches.push((
                                            Rect::from_min_max(
                                                pos2(text_left + x0, row_y + y0),
                                                pos2(text_left + x1, row_y + y1),
                                            ),
                                            current,
                                        ));
                                    }
                                }
                                runs.extend(atlas.runs(
                                    ui.ctx(),
                                    &laid,
                                    pos2(text_left, row_y),
                                    rect,
                                ));
                                height = laid.height;
                                accessible_laid = Some(laid);
                            }
                            None => {
                                measured = false;
                                placeholders += 1;
                                reveal_pending |= reveal == Some((i, row));
                                crate::components::text_placeholder(
                                    &painter,
                                    pos2(text_left, row_y),
                                    text_width,
                                    height,
                                    palette.verse_number.gamma_multiply(alpha::WASH),
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
                            pos2(text_left, row_y + space::XS),
                            Align2::LEFT_TOP,
                            &label,
                            label_font.clone(),
                            palette.verse_number,
                        );
                        accessible_text = label;
                    }
                    Cell::Absent => {
                        painter.text(
                            pos2(text_left, row_y + space::XS),
                            Align2::LEFT_TOP,
                            ABSENT,
                            label_font.clone(),
                            palette.verse_number,
                        );
                        accessible_text = ABSENT.to_string();
                    }
                }
            }
            height = height.max(label_h);
            if let Some(p) = context_at
                && (row_y..row_y + height + ROW_GAP).contains(&p.y)
                && let Cell::Text { natives, text } = &c.cell
                && !text.is_empty()
                && let Some(bible) = view.docs.get(row)
            {
                row_context = Some(ComparisonRow {
                    bible: bible.id.clone(),
                    abbreviation: bible.abbreviation.clone(),
                    natives: natives.clone(),
                    text: text.clone(),
                });
            }
            if accessible && (accessible_laid.is_some() || !accessible_text.is_empty()) {
                crate::a11y::publish_comparison_row(
                    ui,
                    response.id,
                    response.id.with(("comparison-row", block.verse, row)),
                    accessible_label,
                    Rect::from_min_size(pos2(text_left, row_y), vec2(text_width, height)),
                    &accessible_text,
                    accessible_laid.as_deref(),
                );
            }
            row_y += height + ROW_GAP;
        }
        let height = row_y - y + BLOCK_GAP;
        if let Some(p) = context_at
            && (y..y + height).contains(&p.y)
        {
            out.context = Some(ComparisonContext {
                verse: block.verse,
                row: row_context,
            });
        }
        if measured {
            view.heights.set(i, height);
        }
        painter.hline(
            left..=rect.right() - MARGIN - SCROLLBAR,
            y + height - BLOCK_GAP / 2.0,
            Stroke::new(
                stroke::HAIR,
                palette.verse_number.gamma_multiply(alpha::TINT),
            ),
        );
        y += height;
        i += 1;
    }
    view.placeholders = Some(placeholders);
    if let Some((block, _)) = reveal
        && view.reveal_drawn(block, revealed, reveal_pending)
    {
        out.find_moved = true;
        ui.ctx().request_repaint();
    }
    if view.has_pending() {
        ui.ctx().request_repaint();
    }
    for r in tints {
        painter.rect_filled(r, radius::XS, tint);
    }
    for (r, current) in matches {
        crate::pane::paint_find_match(&painter, r, current, palette);
    }
    crate::pane::paint_runs(&painter, runs, |q| palette.color(q.role));
    for [x, _, b] in carets {
        let s = settings.size * mark::CARET_EMS;
        painter.line(
            vec![
                pos2(x - s, b),
                pos2(x, b - s * mark::CARET_RISE),
                pos2(x + s, b),
            ],
            Stroke::new(stroke::MEDIUM, palette.underline),
        );
    }

    let (pos, total) = view.position();
    if total > f64::from(rect.height()) {
        let frac = (pos / view.max_scroll()) as f32;
        crate::components::scroll_thumb(
            &painter,
            track,
            frac,
            rect.height() / total as f32,
            palette.verse_number.gamma_multiply(alpha::HALF),
        );
    }
    out.response = Some(response);
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
    fn chapter_layout_is_virtualized_and_resize_keeps_the_current_location() {
        let doc = crate::tests::numbered_bible("kjv-num", "KJV-numbered", "KJV", false);
        let extent: Vec<_> =
            obs_scripture::Passage::chapter(&obs_scripture::Standard, Book::Genesis, 31)
                .unwrap()
                .verses(&obs_scripture::Standard)
                .collect();
        let mut view = ComparisonView::new(TextSettings::default());
        view.set(vec![doc.into()], Some(0), true, extent);
        let ctx = egui::Context::default();
        let mut atlas = AtlasTexture::default();
        let palette = Palette::for_tone(obs_engine::PageTone::Paper);
        for _ in 0..100 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0))),
                    ..Default::default()
                },
                |ui| {
                    comparison_pane(ui, &mut view, &mut atlas, &palette, Found::default());
                },
            );
            output.textures_delta.clear();
            if !view.has_pending() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(!view.has_pending(), "visible rows settle off-thread");
        let measured = (0..view.blocks.len())
            .filter(|&i| view.heights.is_measured(i))
            .count();
        assert!(
            measured > 0 && measured < 10,
            "only viewport laid out: {measured}"
        );
        assert_eq!(view.blocks.len(), 55, "whole chapter available");
        let verse = Verse::new(Book::Genesis, 31, 30);
        view.jump_to(verse);
        view.set_width(300.0);
        view.set_settings(TextSettings {
            size: 24.0,
            ..TextSettings::default()
        });
        assert_eq!(view.current_location(), Some(verse));
    }

    fn render(
        ctx: &egui::Context,
        view: &mut ComparisonView,
        atlas: &mut AtlasTexture,
        height: f32,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0))),
                events,
                ..Default::default()
            },
            |ui| {
                let rect = Rect::from_min_size(pos2(10.0, 10.0), vec2(550.0, height));
                let mut body = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                body.set_clip_rect(rect);
                comparison_pane(&mut body, view, atlas, &Palette::PAPER, Found::default());
            },
        );
        output.textures_delta.clear();
        output
    }

    fn pending_layouts_settle_after_text_disappears(remove_bibles: bool) {
        let doc = crate::tests::bible();
        let mut view = ComparisonView::new(TextSettings::default());
        view.set(
            vec![Arc::clone(&doc).into()],
            Some(0),
            true,
            vec![Verse::new(Book::Genesis, 31, 55)],
        );
        let Cell::Text { text, .. } = &view.blocks[0].cells[0].cell else {
            panic!("expected text to queue");
        };
        // Queue real work, then navigate before any frame can collect its result.
        assert!(
            view.snippets
                .get(
                    text,
                    ColorRole::Ink,
                    false,
                    view.text_width(),
                    &view.settings
                )
                .is_none()
        );
        assert!(view.has_pending());
        let absent_chapter: Vec<_> =
            obs_scripture::Passage::chapter(&obs_scripture::Standard, Book::Genesis, 34)
                .unwrap()
                .verses(&obs_scripture::Standard)
                .collect();
        view.set(
            if remove_bibles {
                vec![]
            } else {
                vec![doc.into()]
            },
            (!remove_bibles).then_some(0),
            true,
            absent_chapter,
        );
        assert!(!view.blocks.is_empty());
        assert!(view.blocks.iter().all(|b| {
            if remove_bibles {
                b.cells.is_empty()
            } else {
                b.cells.len() == 1 && matches!(b.cells[0].cell, Cell::Absent)
            }
        }));

        let ctx = egui::Context::default();
        let mut atlas = AtlasTexture::default();
        for _ in 0..2000 {
            render(&ctx, &mut view, &mut atlas, 400.0, vec![]);
            if !view.has_pending() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(!view.has_pending(), "completed text layouts must drain");
        // Consume egui's startup/previous-frame repaint requests before checking idle.
        for _ in 0..3 {
            render(&ctx, &mut view, &mut atlas, 400.0, vec![]);
        }
        let output = render(&ctx, &mut view, &mut atlas, 400.0, vec![]);
        assert!(
            output.viewport_output[&egui::ViewportId::ROOT].repaint_delay
                > std::time::Duration::ZERO,
            "textless comparison must stop requesting immediate repaints"
        );
    }

    #[test]
    fn pending_layouts_drain_after_navigating_to_an_all_absent_chapter() {
        pending_layouts_settle_after_text_disappears(false);
    }

    #[test]
    fn pending_layouts_drain_after_removing_all_selected_bibles() {
        pending_layouts_settle_after_text_disappears(true);
    }

    #[test]
    fn wheel_exposes_the_bottom_of_a_final_block_taller_than_the_viewport() {
        let verse = Verse::new(Book::Genesis, 31, 55);
        let mut view = ComparisonView::new(TextSettings {
            size: 28.0,
            ..Default::default()
        });
        view.set(
            (0..6)
                .map(|i| {
                    crate::tests::numbered_bible(
                        &format!("bible-{i}"),
                        "Numbered Bible",
                        &format!("B{i}"),
                        false,
                    )
                    .into()
                })
                .collect(),
            Some(0),
            true,
            vec![verse],
        );
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut atlas = AtlasTexture::default();
        for _ in 0..100 {
            render(&ctx, &mut view, &mut atlas, 120.0, vec![]);
            if !view.has_pending() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(!view.has_pending());
        assert!(view.heights.height(0) > 120.0);
        let mut output = render(
            &ctx,
            &mut view,
            &mut atlas,
            120.0,
            vec![
                egui::Event::PointerMoved(pos2(250.0, 60.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: vec2(0.0, -10000.0),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Wheel,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        for _ in 0..60 {
            output = render(&ctx, &mut view, &mut atlas, 120.0, vec![]);
        }
        assert_eq!(view.current_location(), Some(verse));
        assert!((view.anchor.1 - (view.heights.height(0) - 120.0)).abs() < 0.1);
        let tree = output.platform_output.accesskit_update.unwrap();
        let paragraph = tree
            .nodes
            .iter()
            .find(|(_, n)| {
                n.role() == egui::accesskit::Role::Paragraph
                    && n.label()
                        .is_some_and(|s| s.starts_with("B5 — Genesis 31:55"))
            })
            .expect("last Bible row accessible after wheel scrolling");
        let last = paragraph.1.children().last().unwrap();
        let run = &tree.nodes.iter().find(|(id, _)| id == last).unwrap().1;
        assert!(
            run.value().unwrap().ends_with('.'),
            "final text line exposed"
        );
        let bounds = run.bounds().unwrap();
        assert!(bounds.y0 >= 10.0 && bounds.y1 <= 130.0, "{bounds:?}");
    }

    /// A Find match (#175) or a changed range starting or ending inside a ligature marks the
    /// whole ligature: the shaper gives "fi" one cluster, so testing only its first byte
    /// would draw nothing for an "i" counted in the Find bar.
    #[test]
    fn a_range_inside_a_ligature_marks_the_ligature() {
        let text = "the first fish";
        let laid = laid(text, false, 400.0);
        let fi = text.find("fi").unwrap();
        let ligature = laid
            .lines
            .iter()
            .flat_map(|l| &l.items)
            .flat_map(obs_layout::LaidItem::clusters)
            .find(|(bytes, _, _)| bytes.start == fi)
            .unwrap();
        assert_eq!(ligature.0, fi..fi + 2, "the body font forms an fi ligature");
        let [[x0, _, x1, _]] = range_rects(&laid, &(fi + 1..fi + 2))[..] else {
            panic!("the \"i\" of \"first\" is marked");
        };
        assert!((x0 - ligature.1).abs() < 0.5 && (x1 - ligature.2).abs() < 0.5);
        // "irst": the ligature and the letters after it, one rectangle.
        let [[x0, _, x1, _]] = range_rects(&laid, &(fi + 1..fi + 5))[..] else {
            panic!("one rectangle over \"irst\"");
        };
        assert!((x0 - ligature.1).abs() < 0.5 && x1 > ligature.2);
        assert!(range_rects(&laid, &(fi + 1..fi + 1)).is_empty());
    }

    #[test]
    fn tiny_and_nonpositive_viewports_render_without_panicking() {
        let mut view = ComparisonView::new(TextSettings::default());
        view.set(
            vec![crate::tests::bible().into()],
            Some(0),
            true,
            vec![Verse::new(Book::Genesis, 31, 55)],
        );
        let ctx = egui::Context::default();
        let mut atlas = AtlasTexture::default();
        for height in [23.0, 1.0, 0.0, -5.0] {
            render(&ctx, &mut view, &mut atlas, height, vec![]);
        }
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
