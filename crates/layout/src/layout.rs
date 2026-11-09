//! Paragraph → lines: UAX #9 bidi, UAX #14 line breaking, harfrust shaping through the shared
//! cache, greedy line filling, visual reordering, and hit-testing back to unit bytes.
//!
//! Coordinates are px relative to the block's top-left; y grows down. A line's height comes
//! from the tallest run on it (with Hebrew's metrics scaled by `hebrew_line_height`), so
//! stacked marks are not clipped (#14 rule 11).

use std::collections::HashSet;
use std::ops::Range;
use std::sync::Arc;

use obs_fonts::{Cluster, ShapeCache, ShapeStyle, ShapedRun, ink_top};
use unicode_bidi::{BidiInfo, Level};
use unicode_segmentation::UnicodeSegmentation;

use crate::paragraph::{Align, InlineObject, Paragraph, RunSource};
use crate::style::RunStyle;

/// Least space between neighbouring interlinear cells, as a share of the lines' text size.
const COLUMN_GAP: f32 = 0.6;

/// One shaped piece of a line: a single run, direction and line-break segment.
#[derive(Debug, Clone)]
pub struct LaidItem {
    /// Left edge in px from the block's left.
    pub x: f32,
    pub width: f32,
    /// Index of the paragraph run it came from.
    pub run: usize,
    pub rtl: bool,
    pub style: RunStyle,
    /// Glyphs and clusters; cluster byte ranges are relative to `bytes.start`.
    pub shaped: Arc<ShapedRun>,
    /// Paragraph-text bytes this item draws.
    pub bytes: Range<usize>,
    trailing_space: f32,
    breakable_after: bool,
    /// The interlinear column (index into the paragraph's interlinear words) it is part of:
    /// its word, or the punctuation against it.
    word: Option<usize>,
    /// Whether it is the column's word itself (not the punctuation against it).
    in_word: bool,
    /// Room a justified line adds to its space (#209): after the item in left-to-right
    /// text, before it in right-to-left text. Its space's cluster takes it.
    stretch: f32,
}

impl LaidItem {
    /// Its left and right edges, with the room a justified line added to its space.
    fn extent(&self) -> (f32, f32) {
        if self.rtl {
            (self.x - self.stretch, self.x + self.width)
        } else {
            (self.x, self.x + self.width + self.stretch)
        }
    }

    /// Clusters with paragraph-text byte ranges and block x positions, in visual order. In a
    /// justified line the space's cluster covers the room added to it.
    pub fn clusters(&self) -> impl Iterator<Item = (Range<usize>, f32, f32)> + '_ {
        let base = self.bytes.start;
        let (stretch, width) = (self.stretch, self.shaped.width);
        let rtl = self.rtl;
        self.shaped.clusters.iter().map(move |c| {
            let (mut x0, mut x1) = (c.x0, c.x1);
            if stretch > 0.0 {
                if rtl && x0 <= 0.01 {
                    x0 -= stretch;
                } else if !rtl && x1 >= width - 0.01 {
                    x1 += stretch;
                }
            }
            (
                base + c.bytes.start..base + c.bytes.end,
                self.x + x0,
                self.x + x1,
            )
        })
    }
}

/// One line of a laid block.
#[derive(Debug, Clone)]
pub struct LaidLine {
    pub top: f32,
    pub baseline: f32,
    pub bottom: f32,
    /// Items in visual (left-to-right) order.
    pub items: Vec<LaidItem>,
    /// Paragraph-text bytes on this line (logical).
    pub bytes: Range<usize>,
    /// Bottom of the line's text, above any interlinear lines under it (`bottom` without).
    pub text_bottom: f32,
}

/// One interlinear line under a Tagged word (#143), laid out on its own.
#[derive(Debug, Clone)]
pub struct LaidCell {
    /// Index of the block line whose word it sits under.
    pub line: usize,
    /// Its row: 0 is the line just under the word.
    pub row: usize,
    /// Unit bytes of the Tagged word.
    pub word: Range<usize>,
    /// Its text, in logical order.
    pub text: String,
    /// Its box, in px from the block's top-left.
    pub x: f32,
    pub width: f32,
    pub top: f32,
    pub bottom: f32,
    pub baseline: f32,
    /// Shaped pieces in visual order; their x is absolute in the block and their cluster
    /// bytes index `text`.
    pub items: Vec<LaidItem>,
}

/// Where a paragraph byte came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnitPos {
    /// Unit text at this byte.
    Text(usize),
    /// Inside a synthetic object anchored at a unit byte.
    Object { anchor: usize, object: InlineObject },
}

/// What is under a point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// Paragraph-text bytes of the cluster under the point.
    pub bytes: Range<usize>,
    /// The same cluster in unit bytes, or the object it belongs to.
    pub source: UnitPos,
}

/// A laid-out block.
#[derive(Debug, Clone)]
pub struct LaidBlock {
    pub width: f32,
    pub height: f32,
    /// Base direction of the paragraph.
    pub rtl: bool,
    /// Concatenated run text.
    pub text: String,
    run_starts: Vec<usize>,
    sources: Vec<RunSource>,
    pub lines: Vec<LaidLine>,
    /// Interlinear lines under its Tagged words (#143), line by line.
    pub cells: Vec<LaidCell>,
    /// The box a drop numeral (#209) is drawn in, beside the first lines; its items are the
    /// first line's.
    drop_numeral: Option<DropBox>,
}

/// Where a drop numeral is drawn, in px from the block's top-left.
#[derive(Debug, Clone, Copy)]
struct DropBox {
    /// The paragraph run it draws.
    run: usize,
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
}

impl DropBox {
    fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

/// A shaped piece of an interlinear cell: one face and one bidi level.
struct CellPiece {
    shaped: Arc<ShapedRun>,
    style: RunStyle,
    /// Its resolved bidi level (odd: right to left).
    level: u8,
    /// Its bytes of the cell's text.
    bytes: Range<usize>,
}

/// An interlinear cell's text, its shaped pieces in logical order, and its width.
type CellPieces = (String, Vec<CellPiece>, f32);

/// An interlinear word as laid out: its paragraph bytes and its lines, shaped.
///
/// Its column is the word with the punctuation right against it (`said,`, `“Let there be`),
/// which is never broken or spaced apart (#243). Its lines are centred on the word. Space is
/// added at the column's edges only where its widest line would come too close to the next
/// column's ([`COLUMN_GAP`]), reach under the text beside it (another column's or an untagged
/// word) or stick out past the line's edge, so lines narrower than their words leave the text
/// exactly as it is without them.
struct WordColumn {
    /// Paragraph-text bytes of the word.
    bytes: Range<usize>,
    /// Paragraph-text bytes of the column: the word and the punctuation against it.
    chunk: Range<usize>,
    /// Unit bytes of the word.
    unit: Range<usize>,
    /// Each row's text and shaped pieces (logical order), with its width.
    cells: Vec<CellPieces>,
    /// How far its widest line reaches past the column at its logical start and end
    /// (negative: it stops short of the edge).
    over: [f32; 2],
    /// Space added at its logical start and end.
    pad: [f32; 2],
    /// Whether it reads right to left (its logical start is at the right).
    rtl: bool,
}

/// The untagged text between two neighbouring interlinear columns (or a column and the
/// paragraph's start or end), in logical order.
#[derive(Default)]
struct Between {
    /// Its whole width.
    width: f32,
    /// Width of the space before its first visible text, if it has any.
    lead: Option<f32>,
    /// Width of the space after its last visible text (all of it, until some shows).
    trail: f32,
}

impl Between {
    /// Add an item `width` wide, `visible` unless it is only space, ending in
    /// `trailing_space` of space.
    fn add(&mut self, width: f32, visible: bool, trailing_space: f32) {
        self.width += width;
        if visible {
            self.lead.get_or_insert(self.trail);
            self.trail = trailing_space;
        } else {
            self.trail += width;
        }
    }
}

/// The paragraph bytes of unit bytes `unit`, if the runs show all of them as text.
fn para_range(
    run_starts: &[usize],
    sources: &[RunSource],
    n: usize,
    unit: &Range<usize>,
) -> Option<Range<usize>> {
    let mut start = None;
    let mut end = None;
    for (i, src) in sources.iter().enumerate() {
        if let RunSource::Text { unit_start } = *src {
            let len = run_starts.get(i + 1).copied().unwrap_or(n) - run_starts[i];
            if start.is_none() && unit.start >= unit_start && unit.start < unit_start + len {
                start = Some(run_starts[i] + (unit.start - unit_start));
            }
            if unit.end > unit_start && unit.end <= unit_start + len {
                end = Some(run_starts[i] + (unit.end - unit_start));
            }
        }
    }
    let (s, e) = (start?, end?);
    (s < e).then_some(s..e)
}

/// Shape an interlinear cell's runs (one per script and face) in logical order. A cell is a
/// bidi paragraph of its own (its first strong letter sets its direction), split where its
/// face or resolved level changes, so mixed Hebrew and Latin, digits and punctuation each
/// read their own way.
fn shape_cell(runs: &[crate::paragraph::Run], cache: &ShapeCache) -> CellPieces {
    let mut text = String::new();
    let mut run_starts = Vec::with_capacity(runs.len());
    for r in runs {
        run_starts.push(text.len());
        text.push_str(&r.text);
    }
    let n = text.len();
    let bidi = BidiInfo::new(&text, None);
    let levels = &bidi.levels;
    let mut bounds = run_starts.clone();
    bounds.push(n);
    for i in 1..n {
        if text.is_char_boundary(i) && levels[i] != levels[i - 1] {
            bounds.push(i);
        }
    }
    bounds.sort_unstable();
    bounds.dedup();
    let mut pieces = Vec::with_capacity(bounds.len());
    let mut width = 0.0;
    for w in bounds.windows(2) {
        let (a, b) = (w[0], w[1]);
        if a >= b {
            continue;
        }
        let style = runs[run_starts.partition_point(|&s| s <= a).saturating_sub(1)].style;
        let level = levels[a];
        if let Ok(shaped) = cache.shape(&text[a..b], style.shape_style(level.is_rtl())) {
            width += shaped.width;
            pieces.push(CellPiece {
                shaped,
                style,
                level: level.number(),
                bytes: a..b,
            });
        }
    }
    (text, pieces, width)
}

/// Lay `para` out at `width` px, shaping through `cache`. A drop numeral (#209) is sized to
/// the lines it stands beside, which [`layout_lines`] only knows once it has laid them: with
/// Interlinear lines (#143) or Hebrew metrics they are further apart than ordinary text.
pub fn layout(para: &Paragraph, width: f32, cache: &ShapeCache) -> LaidBlock {
    let mut laid = layout_lines(para, width, cache);
    let Some(run) = para.drop_numeral.filter(|&r| r < para.runs.len()) else {
        return laid;
    };
    let mut fitted: Option<Paragraph> = None;
    // A new size changes the numeral's width, which can move words between lines; the
    // lines rarely change height for it, so the size settles at once.
    for _ in 0..DROP_FITS {
        let current = fitted.as_ref().unwrap_or(para).runs[run].style.size;
        let Some(size) = laid.fitted_drop_size(run) else {
            break;
        };
        if (size - current).abs() < 0.25 {
            break;
        }
        let p = fitted.get_or_insert_with(|| para.clone());
        p.runs[run].style.size = size;
        laid = layout_lines(p, width, cache);
    }
    laid
}

/// Lay `para` out at `width` px, its drop numeral as large as it asks.
fn layout_lines(para: &Paragraph, width: f32, cache: &ShapeCache) -> LaidBlock {
    if para.hidden {
        return LaidBlock {
            width,
            height: 0.0,
            rtl: para.rtl,
            text: String::new(),
            run_starts: Vec::new(),
            sources: Vec::new(),
            lines: Vec::new(),
            cells: Vec::new(),
            drop_numeral: None,
        };
    }
    let mut text = String::new();
    let mut run_starts = Vec::with_capacity(para.runs.len());
    for r in &para.runs {
        run_starts.push(text.len());
        text.push_str(&r.text);
    }
    let sources: Vec<RunSource> = para.runs.iter().map(|r| r.source.clone()).collect();
    let n = text.len();
    let base = if para.rtl { Level::rtl() } else { Level::ltr() };
    let bidi = BidiInfo::new(&text, Some(base));
    let levels = &bidi.levels;
    let run_of = |p: usize| run_starts.partition_point(|&s| s <= p).saturating_sub(1);
    // A drop numeral (#209) stands beside the first lines, out of their flow (and out of
    // any Interlinear column, though no space parts it from the first word).
    let drop_run = para.drop_numeral.filter(|&r| r < para.runs.len());
    let drop_bytes = drop_run.map(|r| run_starts[r]..run_starts.get(r + 1).copied().unwrap_or(n));

    // Interlinear words (#143): their paragraph bytes and shaped lines.
    let rows = para.interlinear.as_ref().map_or(0, |il| il.rows);
    let mut columns: Vec<WordColumn> = para
        .interlinear
        .iter()
        .flat_map(|il| &il.words)
        .filter_map(|w| {
            let bytes = para_range(&run_starts, &sources, n, &w.word)?;
            Some(WordColumn {
                chunk: bytes.clone(),
                bytes,
                unit: w.word.clone(),
                cells: w.cells.iter().map(|c| shape_cell(c, cache)).collect(),
                over: [0.0; 2],
                pad: [0.0; 2],
                rtl: false,
            })
        })
        .collect();
    columns.sort_by_key(|c| c.bytes.start);
    // Each column takes the punctuation right against its word, up to a space or the next
    // column.
    for i in 0..columns.len() {
        let mut floor = i.checked_sub(1).map_or(0, |p| columns[p].chunk.end);
        let mut ceiling = columns.get(i + 1).map_or(n, |c| c.bytes.start);
        let c = &mut columns[i];
        if let Some(d) = &drop_bytes {
            if d.end <= c.bytes.start {
                floor = floor.max(d.end);
            } else if d.start >= c.bytes.end {
                ceiling = ceiling.min(d.start);
            }
        }
        let lead = text[floor..c.bytes.start]
            .char_indices()
            .rev()
            .take_while(|(_, ch)| !ch.is_whitespace())
            .last()
            .map_or(c.bytes.start, |(k, _)| floor + k);
        let trail = text[c.bytes.end..ceiling]
            .char_indices()
            .find(|(_, ch)| ch.is_whitespace())
            .map_or(ceiling, |(k, _)| c.bytes.end + k);
        c.chunk = lead..trail;
        c.rtl = levels.get(c.bytes.start).is_some_and(|l| l.is_rtl());
    }
    let column_at = |p: usize| {
        let i = columns.partition_point(|c| c.chunk.start <= p);
        i.checked_sub(1).filter(|&i| columns[i].chunk.contains(&p))
    };

    // Segment at run starts, break opportunities, direction changes and interlinear columns
    // and words.
    let mut bounds: Vec<usize> = vec![0, n];
    bounds.extend(run_starts.iter().copied());
    for c in &columns {
        bounds.extend([c.chunk.start, c.bytes.start, c.bytes.end, c.chunk.end]);
    }
    let mut breaks = HashSet::new();
    // Breaks the line must take (after a newline or a line or paragraph separator).
    let mut mandatory = HashSet::new();
    for (i, kind) in unicode_linebreak::linebreaks(&text) {
        // An interlinear column is never broken: its lines stand under all of its word.
        if i < n && column_at(i).is_none_or(|c| columns[c].chunk.start == i) {
            bounds.push(i);
            breaks.insert(i);
            if kind == unicode_linebreak::BreakOpportunity::Mandatory {
                mandatory.insert(i);
                // The separator before it is an item of its own, which draws nothing.
                bounds.push(text[..i].trim_end_matches(is_line_separator).len());
            }
        }
    }
    for i in 1..n {
        if text.is_char_boundary(i) && levels[i] != levels[i - 1] {
            bounds.push(i);
        }
    }
    bounds.sort_unstable();
    bounds.dedup();

    let mut items: Vec<LaidItem> = Vec::new();
    for w in bounds.windows(2) {
        let (a, b) = (w[0], w[1]);
        if a >= b {
            continue;
        }
        let run = run_of(a);
        let style = para.runs[run].style;
        let rtl = levels[a].is_rtl();
        let Some(shaped) = shape_segment(&text[a..b], style.shape_style(rtl), cache) else {
            continue;
        };
        items.push(LaidItem {
            x: 0.0,
            width: shaped.width,
            run,
            rtl,
            style,
            trailing_space: shaped.trailing_space,
            shaped,
            bytes: a..b,
            breakable_after: breaks.contains(&b) || b == n,
            word: column_at(a),
            in_word: column_at(a).is_some_and(|c| columns[c].bytes.contains(&a)),
            stretch: 0.0,
        });
    }

    let (drop_items, items): (Vec<LaidItem>, Vec<LaidItem>) =
        items.into_iter().partition(|it| Some(it.run) == drop_run);

    // How far each column's widest line reaches past it, then the space each pair of
    // neighbouring columns needs between them, shared between the two.
    if !columns.is_empty() {
        let gap = para
            .interlinear
            .as_ref()
            .map_or(0.0, |il| il.size * COLUMN_GAP);
        // Width of each column's word, its punctuation at the start and at the end.
        let mut widths = vec![[0.0f32; 3]; columns.len()];
        for it in &items {
            if let Some(w) = it.word {
                let c = &columns[w];
                let part = if it.in_word {
                    1
                } else if it.bytes.start < c.bytes.start {
                    0
                } else {
                    2
                };
                widths[w][part] += it.width;
            }
        }
        for (c, [lead, word, trail]) in columns.iter_mut().zip(widths) {
            let widest = c.cells.iter().map(|(_, _, w)| *w).fold(0.0, f32::max);
            if widest > 0.0 {
                let half = (widest - word) / 2.0;
                c.over = [half - lead, half - trail];
            } else {
                c.over = [f32::MIN, f32::MIN];
            }
        }
        // Text between neighbouring columns (logical order): the space, untagged words.
        let mut between = Between::default();
        let mut previous: Option<usize> = None;
        let mut i = 0;
        loop {
            let next = items.get(i).map(|it| it.word);
            if let Some(None) = next {
                let it = &items[i];
                let visible = !text[it.bytes.clone()].trim().is_empty();
                between.add(it.width, visible, it.trailing_space);
                i += 1;
                continue;
            }
            // The next column, if any.
            let w = next.flatten();
            // Lines never reach under untagged text beside them: no lines stand under it
            // to tell whose they are, and at a line's start or end that text is all that
            // keeps them inside the line (#243 review).
            if let Some(lead) = between.lead {
                if let Some(p) = previous {
                    let c = &mut columns[p];
                    c.pad[1] = c.pad[1].max(c.over[1] - lead);
                }
                if let Some(w) = w {
                    let c = &mut columns[w];
                    c.pad[0] = c.pad[0].max(c.over[0] - between.trail);
                }
            }
            if let (Some(p), Some(w)) = (previous, w) {
                // Their lines keep the gap between them, and neither reaches under the
                // other's text.
                let (end, start) = (columns[p].over[1], columns[w].over[0]);
                let short = (end + start + gap).max(end).max(start)
                    - between.width
                    - columns[p].pad[1]
                    - columns[w].pad[0];
                if short > 0.0 {
                    columns[p].pad[1] += short / 2.0;
                    columns[w].pad[0] += short / 2.0;
                }
            }
            let Some(w) = w else { break };
            previous = Some(w);
            between = Between::default();
            while i < items.len() && items[i].word == Some(w) {
                i += 1;
            }
        }
    }
    let drop_width: f32 = drop_items.iter().map(|i| i.width).sum();
    let drop_space = if drop_items.is_empty() {
        0.0
    } else {
        drop_width + drop_items[0].style.size * DROP_GAP
    };
    // Lines that must start at a run (One verse per line, #209), by paragraph byte.
    let forced: HashSet<usize> = para
        .line_starts
        .iter()
        .filter(|&&r| r < run_starts.len() && Some(r) != drop_run)
        .map(|&r| run_starts[r])
        .collect();

    // Greedy line filling over unbreakable chunks.
    let available = |line: usize| {
        let indent = if line == 0 { para.indent } else { 0.0 };
        let drop = if line < DROP_LINES { drop_space } else { 0.0 };
        (width - para.inset - indent - drop).max(1.0)
    };
    let mut lines_items: Vec<Vec<LaidItem>> = Vec::new();
    // Whether each line ends where a forced line starts the next (never justified).
    let mut forced_ends: Vec<bool> = Vec::new();
    let mut cur: Vec<LaidItem> = Vec::new();
    let mut cur_w = 0.0f32;
    let mut chunk: Vec<LaidItem> = Vec::new();
    let mut chunk_w = 0.0f32;
    for it in items {
        if forced.contains(&it.bytes.start) && !(cur.is_empty() && chunk.is_empty()) {
            // A piece of a word before the verse that does not fit goes on a line of its own.
            if !cur.is_empty()
                && !chunk.is_empty()
                && cur_w + chunk_w > available(lines_items.len())
            {
                lines_items.push(std::mem::take(&mut cur));
                forced_ends.push(false);
            }
            cur.append(&mut chunk);
            lines_items.push(std::mem::take(&mut cur));
            forced_ends.push(true);
            cur_w = 0.0;
            chunk_w = 0.0;
        }
        chunk_w += it.width;
        let br = it.breakable_after;
        let end = it.bytes.end;
        let style = it.style;
        chunk.push(it);
        // Place the chunk: on this line if it fits, else split by hyphenation or moved to the
        // next line, where it is tried again (a word wider than a whole line is hyphenated
        // there, and so is its tail if that is still too wide).
        if br {
            // Broken at a soft hyphen, the line ends in a hyphen: leave room for it.
            let soft_hyphen = if end < n && ends_in_soft_hyphen(&text, end) {
                hyphen_run(style, false, cache).map_or(0.0, |h| h.width)
            } else {
                0.0
            };
            loop {
                let avail = available(lines_items.len());
                let trail = chunk.last().map_or(0.0, |i| i.trailing_space);
                let fits = if columns.is_empty() {
                    cur_w + chunk_w - trail + soft_hyphen <= avail
                } else {
                    // Interlinear columns take space at their edges, more at the line's ends
                    // than between neighbours: measure the line as it would be drawn.
                    let line: Vec<LaidItem> = cur.iter().chain(&chunk).cloned().collect();
                    let (ordered, pads, hang) = arrange_line(line, levels, &columns, &text);
                    line_content(&ordered, &pads, hang) + soft_hyphen <= avail
                };
                if fits {
                    cur.append(&mut chunk);
                    cur_w += chunk_w;
                    break;
                }
                if let Some((head, hyphen, tail)) = (para.hyphenate && chunk.len() == 1)
                    .then(|| {
                        let run_start = run_starts[chunk[0].run];
                        hyphenate(&chunk[0], &text, run_start, avail - cur_w, cache)
                    })
                    .flatten()
                {
                    // The word's head and a hyphen end this line; its tail starts the next.
                    cur.push(head);
                    cur.push(hyphen);
                    lines_items.push(std::mem::take(&mut cur));
                    forced_ends.push(false);
                    cur_w = 0.0;
                    chunk_w = tail.width;
                    chunk = vec![tail];
                } else if cur.is_empty() {
                    // Alone on its line and still too wide (a long name in a narrow Panel):
                    // wrap it between grapheme clusters so all of it stays in the column. An
                    // interlinear column is never broken.
                    if chunk_w - trail > avail && chunk.iter().all(|i| i.word.is_none()) {
                        // Lines after the next are as wide or wider (a drop numeral narrows
                        // only the first two).
                        let mut pieces = split_overwide(
                            std::mem::take(&mut chunk),
                            avail,
                            available(lines_items.len() + 1),
                            &text,
                            cache,
                        );
                        chunk = pieces.pop().unwrap_or_default();
                        for piece in pieces {
                            lines_items.push(piece);
                            forced_ends.push(false);
                        }
                        chunk_w = chunk.iter().map(|i| i.width).sum();
                    }
                    cur.append(&mut chunk);
                    cur_w = chunk_w;
                    break;
                } else {
                    lines_items.push(std::mem::take(&mut cur));
                    forced_ends.push(false);
                    cur_w = 0.0;
                }
            }
            chunk_w = 0.0;
            if mandatory.contains(&end) {
                // A line that ends where the text breaks it is never justified.
                lines_items.push(std::mem::take(&mut cur));
                forced_ends.push(true);
                cur_w = 0.0;
            }
        }
    }
    cur.append(&mut chunk);
    if !cur.is_empty() || lines_items.is_empty() {
        lines_items.push(cur);
        forced_ends.push(false);
    }
    let last_line = lines_items.len() - 1;

    // The text's style (not a drop numeral's), for empty lines and the room around
    // interlinear lines.
    let fallback = (para.runs.iter().enumerate())
        .find(|&(r, _)| Some(r) != drop_run)
        .or(para.runs.first().map(|r| (0, r)))
        .map_or_else(|| RunStyle::body(16.0), |(_, r)| r.style);
    // Interlinear rows: each as tall as its tallest cell anywhere in the paragraph, so rows
    // line up from line to line.
    let row_extents: Vec<(f32, f32)> = (0..rows)
        .map(|row| {
            let size = para.interlinear.as_ref().map_or(12.0, |il| il.size);
            let mut ext = RunStyle::body(size).line_extent(para.hebrew_line_height);
            for c in &columns {
                if let Some((_, pieces, _)) = c.cells.get(row) {
                    for piece in pieces {
                        let (a, d) = piece.style.line_extent(para.hebrew_line_height);
                        ext = (ext.0.max(a), ext.1.max(d));
                    }
                }
            }
            ext
        })
        .collect();
    let mut lines = Vec::with_capacity(lines_items.len());
    let mut cells = Vec::new();
    let mut y = para.space_before;
    let line_count = lines_items.len();
    for (li, mut its) in lines_items.into_iter().enumerate() {
        let start = its.iter().map(|i| i.bytes.start).min().unwrap_or(n);
        let end = its.iter().map(|i| i.bytes.end).max().unwrap_or(n);
        // UAX #9 rule L1: whitespace at the line's end (and before a tab) takes the
        // paragraph's level before the line is reordered.
        let line_levels = bidi
            .has_rtl()
            .then(|| {
                let p = bidi
                    .paragraphs
                    .iter()
                    .find(|p| p.range.contains(&start))
                    .or(bidi.paragraphs.last())?;
                Some(bidi.reordered_levels(p, start..end))
            })
            .flatten();
        if let Some(line_levels) = &line_levels {
            its = split_at_levels(its, line_levels, &text, cache);
        }
        if li + 1 < line_count && ends_in_soft_hyphen(&text, end) {
            its = show_hyphen(its, end, &text, cache);
        }
        let line_levels = line_levels.as_deref().unwrap_or(levels);
        let (mut ordered, edge_pads, hang) = arrange_line(its, line_levels, &columns, &text);
        let (mut asc, mut desc) = (0.0f32, 0.0f32);
        for it in &ordered {
            let (a, d) = it.style.line_extent(para.hebrew_line_height);
            asc = asc.max(a);
            desc = desc.max(d);
        }
        if ordered.is_empty() {
            let (a, d) = fallback.line_extent(para.hebrew_line_height);
            asc = a;
            desc = d;
        }
        let natural = asc + desc;
        let line_height = natural * para.line_spacing;
        let content = line_content(&ordered, &edge_pads, hang);
        let first_indent = if li == 0 { para.indent } else { 0.0 };
        let drop = if li < DROP_LINES { drop_space } else { 0.0 };
        let mut x = match (para.align, para.rtl) {
            (Align::Center, _) => ((width - content) / 2.0).max(0.0),
            (Align::Start, false) => para.inset + first_indent + drop,
            (Align::Start, true) => width - para.inset - first_indent - drop - content,
        };
        // Justified text (#209): a full line's room goes into its spaces, all but the one at
        // its logical end.
        let stretches = |i: &LaidItem| {
            i.bytes.end != end && i.trailing_space > 0.0 && text[i.bytes.clone()].ends_with(' ')
        };
        let justify_gap =
            if para.justify && para.align == Align::Start && li < last_line && !forced_ends[li] {
                let gaps = ordered.iter().filter(|i| stretches(i)).count();
                let room = available(li) - content;
                if gaps > 0 && room > 0.0 {
                    room / gaps as f32
                } else {
                    0.0
                }
            } else {
                0.0
            };
        if para.rtl && para.align == Align::Start {
            // Hanging space sits at the logical end, which is the left edge in RTL; a
            // justified line's room (taken in its spaces) moves its start that much left.
            x -= hang;
            if justify_gap > 0.0 {
                x -= available(li) - content;
            }
        }
        for (item, [left, right]) in ordered.iter_mut().zip(&edge_pads) {
            // A space is after its word in left-to-right text, and before it (on its left)
            // in right-to-left text.
            let gap = if stretches(item) { justify_gap } else { 0.0 };
            if item.rtl {
                x += gap;
            }
            x += left;
            item.x = x;
            item.stretch = gap;
            x += item.width + right;
            if !item.rtl {
                x += gap;
            }
        }
        let top = y;
        let baseline = (top + (line_height - natural) / 2.0 + asc).round();
        y += line_height;
        let mut text_bottom = y;
        if rows > 0 {
            // The lines under each word on this line, centred on its column.
            let mut row_baselines = Vec::with_capacity(rows);
            let mut row_y = baseline + desc + fallback.size * 0.08;
            text_bottom = row_y;
            for &(a, d) in &row_extents {
                row_baselines.push(((row_y + a).round(), row_y, row_y + a + d));
                row_y += (a + d) * 1.08;
            }
            y = row_y + fallback.size * 0.45;
            let mut seen: Vec<usize> = Vec::new();
            for it in &ordered {
                let Some(w) = it.word else { continue };
                if seen.contains(&w) {
                    continue;
                }
                seen.push(w);
                let (left, right) = ordered
                    .iter()
                    .filter(|o| o.word == Some(w) && o.in_word)
                    .fold((f32::MAX, f32::MIN), |(l, r), o| {
                        (l.min(o.x), r.max(o.x + o.width))
                    });
                let centre = (left + right) / 2.0;
                let column = &columns[w];
                for (row, (cell_text, pieces, cell_width)) in column.cells.iter().enumerate() {
                    let Some(&(cell_baseline, cell_top, cell_bottom)) = row_baselines.get(row)
                    else {
                        continue;
                    };
                    if pieces.is_empty() {
                        continue;
                    }
                    let x0 = centre - cell_width / 2.0;
                    let mut px = x0;
                    let logical: Vec<LaidItem> = pieces
                        .iter()
                        .map(|p| LaidItem {
                            x: 0.0,
                            width: p.shaped.width,
                            run: 0,
                            rtl: p.level % 2 == 1,
                            style: p.style,
                            shaped: Arc::clone(&p.shaped),
                            bytes: p.bytes.clone(),
                            trailing_space: p.shaped.trailing_space,
                            breakable_after: false,
                            word: Some(w),
                            in_word: true,
                            stretch: 0.0,
                        })
                        .collect();
                    let cell_levels: Vec<u8> = pieces.iter().map(|p| p.level).collect();
                    let mut laid = reorder(logical, &cell_levels);
                    for item in &mut laid {
                        item.x = px;
                        px += item.width;
                    }
                    cells.push(LaidCell {
                        line: li,
                        row,
                        word: column.unit.clone(),
                        text: cell_text.clone(),
                        x: x0,
                        width: *cell_width,
                        top: cell_top,
                        bottom: cell_bottom,
                        baseline: cell_baseline,
                        items: laid,
                    });
                }
            }
        }
        lines.push(LaidLine {
            top,
            baseline,
            bottom: y,
            items: ordered,
            bytes: start..end,
            text_bottom,
        });
    }
    let drop_numeral = if drop_items.is_empty() {
        None
    } else {
        place_drop_numeral(&mut lines, drop_items, para, width)
    };
    if let Some(d) = drop_numeral {
        y = y.max(d.bottom);
    }
    LaidBlock {
        width,
        height: y + para.space_after,
        rtl: para.rtl,
        text,
        run_starts,
        sources,
        lines,
        cells,
        drop_numeral,
    }
}

/// Whether `c` ends a line wherever it is (UAX #14 classes BK, CR, LF and NL).
fn is_line_separator(c: char) -> bool {
    matches!(
        c,
        '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

/// One segment of paragraph text, shaped through `cache`. Line and paragraph separators draw
/// nothing: a segment of them is one empty cluster over its bytes.
fn shape_segment(text: &str, style: ShapeStyle, cache: &ShapeCache) -> Option<Arc<ShapedRun>> {
    if !text.is_empty() && text.chars().all(is_line_separator) {
        return Some(Arc::new(ShapedRun {
            glyphs: Vec::new(),
            clusters: vec![Cluster {
                bytes: 0..text.len(),
                x0: 0.0,
                x1: 0.0,
            }],
            width: 0.0,
            trailing_space: 0.0,
        }));
    }
    cache.shape(text, style).ok()
}

/// U+00AD SOFT HYPHEN: a break opportunity that draws nothing unless the line breaks at it.
const SOFT_HYPHEN: char = '\u{00AD}';

/// Whether the paragraph text before byte `end` ends in a soft hyphen.
fn ends_in_soft_hyphen(text: &str, end: usize) -> bool {
    text.get(..end).is_some_and(|t| t.ends_with(SOFT_HYPHEN))
}

/// The visible hyphen a line broken at a soft hyphen ends in.
fn hyphen_run(style: RunStyle, rtl: bool, cache: &ShapeCache) -> Option<Arc<ShapedRun>> {
    cache.shape("-", style.shape_style(rtl)).ok()
}

/// Bytes `bytes` of `it` shaped on their own, right to left or not.
fn reshape(
    it: &LaidItem,
    bytes: Range<usize>,
    rtl: bool,
    breakable_after: bool,
    text: &str,
    cache: &ShapeCache,
) -> Option<LaidItem> {
    let shaped = shape_segment(&text[bytes.clone()], it.style.shape_style(rtl), cache)?;
    Some(LaidItem {
        x: 0.0,
        width: shaped.width,
        run: it.run,
        rtl,
        style: it.style,
        trailing_space: shaped.trailing_space,
        shaped,
        bytes,
        breakable_after,
        word: it.word,
        in_word: it.in_word,
        stretch: 0.0,
    })
}

/// An unbreakable chunk (items in logical order) too wide for a line, cut between grapheme
/// clusters into pieces that fit: the first in `first` px, the rest in `rest`. A cluster wider
/// than the line is a piece of its own, and trailing space never starts a piece.
fn split_overwide(
    chunk: Vec<LaidItem>,
    first: f32,
    rest: f32,
    text: &str,
    cache: &ShapeCache,
) -> Vec<Vec<LaidItem>> {
    let (Some(a), Some(b)) = (
        chunk.first().map(|i| i.bytes.start),
        chunk.last().map(|i| i.bytes.end),
    ) else {
        return vec![chunk];
    };
    let graphemes: HashSet<usize> = text[a..b]
        .grapheme_indices(true)
        .map(|(i, _)| a + i)
        .collect();
    let mut cuts = Vec::new();
    let mut avail = first;
    let mut used = 0.0f32;
    let mut placed = false;
    for it in &chunk {
        let mut clusters: Vec<(Range<usize>, f32)> =
            it.clusters().map(|(r, x0, x1)| (r, x1 - x0)).collect();
        clusters.sort_by_key(|(r, _)| r.start);
        for (r, w) in clusters {
            let space = text[r.clone()].chars().all(char::is_whitespace);
            if placed && used + w > avail && !space && graphemes.contains(&r.start) {
                cuts.push(r.start);
                avail = rest;
                used = 0.0;
            }
            used += w;
            placed = true;
        }
    }
    if cuts.is_empty() {
        return vec![chunk];
    }
    let mut pieces: Vec<Vec<LaidItem>> = vec![Vec::new(); cuts.len() + 1];
    for it in chunk {
        let mut start = it.bytes.start;
        let inner = cuts
            .iter()
            .copied()
            .filter(|&c| c > it.bytes.start && c < it.bytes.end);
        for edge in inner.chain([it.bytes.end]) {
            let piece = if start == it.bytes.start && edge == it.bytes.end {
                Some(it.clone())
            } else {
                let breakable = edge != it.bytes.end || it.breakable_after;
                reshape(&it, start..edge, it.rtl, breakable, text, cache)
            };
            if let Some(p) = piece {
                pieces[cuts.partition_point(|&c| c <= start)].push(p);
            }
            start = edge;
        }
    }
    pieces.retain(|p| !p.is_empty());
    pieces
}

/// A line's items split where its levels (after rule L1) differ within one, or from the
/// direction it was shaped in, each piece shaped in its own direction.
fn split_at_levels(
    its: Vec<LaidItem>,
    levels: &[Level],
    text: &str,
    cache: &ShapeCache,
) -> Vec<LaidItem> {
    let mut out = Vec::with_capacity(its.len());
    for it in its {
        let r = it.bytes.clone();
        let mut cuts: Vec<usize> = text[r.clone()]
            .char_indices()
            .map(|(i, _)| r.start + i)
            .collect::<Vec<_>>()
            .windows(2)
            .filter(|w| levels[w[0]] != levels[w[1]])
            .map(|w| w[1])
            .collect();
        if cuts.is_empty() && levels[r.start].is_rtl() == it.rtl {
            out.push(it);
            continue;
        }
        cuts.push(r.end);
        let mut start = r.start;
        for edge in cuts {
            let breakable = edge == r.end && it.breakable_after;
            let rtl = levels[start].is_rtl();
            if let Some(p) = reshape(&it, start..edge, rtl, breakable, text, cache) {
                out.push(p);
            }
            start = edge;
        }
    }
    out
}

/// A line broken at a soft hyphen: the soft hyphen ending it becomes an item of its own that
/// draws a visible hyphen over its bytes.
fn show_hyphen(
    mut its: Vec<LaidItem>,
    end: usize,
    text: &str,
    cache: &ShapeCache,
) -> Vec<LaidItem> {
    let Some(k) = its.iter().position(|i| i.bytes.end == end) else {
        return its;
    };
    let shy = end - SOFT_HYPHEN.len_utf8();
    let Some(hyphen) = hyphen_run(its[k].style, its[k].rtl, cache) else {
        return its;
    };
    let it = its.remove(k);
    let mut run = (*hyphen).clone();
    for g in &mut run.glyphs {
        g.cluster = 0;
    }
    for c in &mut run.clusters {
        c.bytes = 0..SOFT_HYPHEN.len_utf8();
    }
    run.trailing_space = 0.0;
    let mut pieces = Vec::with_capacity(2);
    if shy > it.bytes.start {
        pieces.extend(reshape(
            &it,
            it.bytes.start..shy,
            it.rtl,
            false,
            text,
            cache,
        ));
    }
    pieces.push(LaidItem {
        width: run.width,
        trailing_space: 0.0,
        shaped: Arc::new(run),
        bytes: shy..end,
        ..it
    });
    its.splice(k..k, pieces);
    its
}

/// A line's items in visual order, the space each takes at its left and right, and the
/// trailing space that hangs past the margin at its logical end.
///
/// An interlinear column gets what its neighbours need between them, except at the line's
/// ends, where only what keeps its lines inside the line counts. Line filling measures
/// candidate lines with this too, so a line never runs wider than it is drawn (#243).
fn arrange_line(
    its: Vec<LaidItem>,
    levels: &[Level],
    columns: &[WordColumn],
    text: &str,
) -> (Vec<LaidItem>, Vec<[f32; 2]>, f32) {
    let end = its.iter().map(|i| i.bytes.end).max();
    let lv: Vec<u8> = its.iter().map(|i| levels[i.bytes.start].number()).collect();
    let ordered = reorder(its, &lv);
    let hang = ordered
        .iter()
        .find(|i| Some(i.bytes.end) == end)
        .map_or(0.0, |i| i.trailing_space);
    let line_pads = |w: usize, at_left: bool, at_right: bool| -> [f32; 2] {
        let c = &columns[w];
        // Logical start and end as left and right.
        let (pad, over) = if c.rtl {
            ([c.pad[1], c.pad[0]], [c.over[1], c.over[0]])
        } else {
            (c.pad, c.over)
        };
        [
            if at_left { over[0].max(0.0) } else { pad[0] },
            if at_right { over[1].max(0.0) } else { pad[1] },
        ]
    };
    let edge_pads: Vec<[f32; 2]> = (0..ordered.len())
        .map(|i| {
            let Some(w) = ordered[i].word else {
                return [0.0; 2];
            };
            let first = i == 0 || ordered[i - 1].word != Some(w);
            let last = i + 1 == ordered.len() || ordered[i + 1].word != Some(w);
            // Space (a trailing one) is not text beside it.
            let beside =
                |o: &LaidItem| o.word != Some(w) && !text[o.bytes.clone()].trim().is_empty();
            let at_left = !ordered[..i].iter().any(beside);
            let at_right = !ordered[i + 1..].iter().any(beside);
            let [l, r] = line_pads(w, at_left, at_right);
            [if first { l } else { 0.0 }, if last { r } else { 0.0 }]
        })
        .collect();
    (ordered, edge_pads, hang)
}

/// Width a line arranged by [`arrange_line`] takes: its items, their padding, less the
/// hanging trailing space.
fn line_content(ordered: &[LaidItem], edge_pads: &[[f32; 2]], hang: f32) -> f32 {
    let padding: f32 = edge_pads.iter().map(|[l, r]| l + r).sum();
    ordered.iter().map(|i| i.width).sum::<f32>() + padding - hang
}

/// How many lines a drop numeral stands beside.
const DROP_LINES: usize = 2;
/// How many times [`layout`] lays a paragraph out again to fit its drop numeral.
const DROP_FITS: usize = 3;
/// Space between a drop numeral and the text beside it, as a share of the numeral's size.
const DROP_GAP: f32 = 0.08;

/// Put a drop numeral's items beside the first lines (on the paragraph's start side), its
/// baseline on the second line's; its items join the first line's. Returns the box it is drawn
/// in, from the first line's top down to the block's bottom it needs.
fn place_drop_numeral(
    lines: &mut [LaidLine],
    mut numeral: Vec<LaidItem>,
    para: &Paragraph,
    width: f32,
) -> Option<DropBox> {
    let first = lines.first()?;
    let line_height = first.bottom - first.top;
    let baseline = lines
        .get(DROP_LINES - 1)
        .map_or(first.baseline + line_height, |l| l.baseline);
    let lowered = first.baseline - baseline;
    let numeral_width: f32 = numeral.iter().map(|i| i.width).sum();
    let left = if para.rtl {
        width - para.inset - numeral_width
    } else {
        para.inset
    };
    let mut x = left;
    let numeral_start = numeral.iter().map(|i| i.bytes.start).min().unwrap_or(0);
    for it in &mut numeral {
        it.x = x;
        x += it.width;
        it.style.raise = lowered;
    }
    let drawn = DropBox {
        run: numeral.first()?.run,
        left,
        right: left + numeral_width,
        top: first.top,
        bottom: baseline + (first.bottom - first.baseline),
    };
    let first = &mut lines[0];
    first.bytes.start = first.bytes.start.min(numeral_start);
    if para.rtl {
        first.items.extend(numeral);
    } else {
        numeral.append(&mut first.items);
        first.items = numeral;
    }
    Some(drawn)
}

/// Split a word that does not fit in `room` px at the last English hyphenation point whose
/// head and a hyphen fit (Justified text, #209): its head, the hyphen (drawn, not in the text:
/// it has no clusters) and its tail. Only a plain run of Latin letters, left to right and
/// outside any interlinear word, is hyphenated; never Greek or Hebrew. A tail left by an
/// earlier split is split at the whole word's hyphenation points.
fn hyphenate(
    item: &LaidItem,
    text: &str,
    run_start: usize,
    room: f32,
    cache: &ShapeCache,
) -> Option<(LaidItem, LaidItem, LaidItem)> {
    if item.rtl || item.word.is_some() || item.style.raise != 0.0 {
        return None;
    }
    let piece = text.get(item.bytes.clone())?;
    // The word: its letters after any opening punctuation, up to its first non-letter.
    let lead = piece.find(|c: char| c.is_ascii_alphabetic())?;
    let len = piece[lead..]
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(piece.len() - lead);
    // Another letter after or before the word (a non-ASCII one such as é) leaves it whole, and
    // so does a soft hyphen: the text's own break points are the word's.
    let joined = |c: char| c.is_alphabetic() || c == SOFT_HYPHEN;
    if piece[lead + len..].starts_with(joined) {
        return None;
    }
    let start = item.bytes.start;
    // The whole word: a tail's head is on the line before, in its run just before it.
    let before = if lead == 0 {
        text[run_start..start]
            .bytes()
            .rev()
            .take_while(u8::is_ascii_alphabetic)
            .count()
    } else {
        0
    };
    let word_start = start + lead - before;
    let word_end = start + lead + len;
    let word = &text[word_start..word_end];
    if !(MIN_HYPHENATED..=MAX_HYPHENATED).contains(&word.len())
        || text[run_start..word_start].ends_with(joined)
    {
        return None;
    }
    let hyphen = cache.shape("-", item.style.shape_style(false)).ok()?;
    let mut at = word_start;
    let mut points = Vec::new();
    for syllable in hypher::hyphenate(word, hypher::Lang::English) {
        at += syllable.len();
        if at > start && at < word_end {
            points.push(at);
        }
    }
    for &p in points.iter().rev() {
        let head = cache
            .shape(&text[start..p], item.style.shape_style(false))
            .ok()?;
        if head.width + hyphen.width > room {
            continue;
        }
        let tail = cache
            .shape(&text[p..item.bytes.end], item.style.shape_style(false))
            .ok()?;
        let piece_of = |shaped: Arc<ShapedRun>, bytes: Range<usize>, breakable_after| LaidItem {
            x: 0.0,
            width: shaped.width,
            run: item.run,
            rtl: false,
            style: item.style,
            trailing_space: shaped.trailing_space,
            shaped,
            bytes,
            breakable_after,
            word: None,
            in_word: false,
            stretch: 0.0,
        };
        let drawn_only = Arc::new(ShapedRun {
            clusters: Vec::new(),
            trailing_space: 0.0,
            ..(*hyphen).clone()
        });
        return Some((
            piece_of(head, start..p, true),
            piece_of(drawn_only, p..p, true),
            piece_of(tail, p..item.bytes.end, item.breakable_after),
        ));
    }
    None
}

/// Shortest word hyphenated: two letters before the hyphen and three after (the English
/// patterns' minimums).
const MIN_HYPHENATED: usize = 5;
/// Longest word looked up; longer runs of letters are not English words.
const MAX_HYPHENATED: usize = 40;

/// UAX #9 rule L2: from the highest level down to the lowest odd level, reverse every run of
/// items at that level or higher.
fn reorder(items: Vec<LaidItem>, levels: &[u8]) -> Vec<LaidItem> {
    let mut idx: Vec<usize> = (0..items.len()).collect();
    if let (Some(&max), Some(&min)) = (levels.iter().max(), levels.iter().min()) {
        let lowest_odd = if min % 2 == 1 { min } else { min + 1 };
        let mut level = max;
        while level >= lowest_odd && level > 0 {
            let mut i = 0;
            while i < idx.len() {
                if levels[idx[i]] >= level {
                    let s = i;
                    while i < idx.len() && levels[idx[i]] >= level {
                        i += 1;
                    }
                    idx[s..i].reverse();
                } else {
                    i += 1;
                }
            }
            level -= 1;
        }
    }
    let mut slots: Vec<Option<LaidItem>> = items.into_iter().map(Some).collect();
    idx.into_iter().filter_map(|i| slots[i].take()).collect()
}

impl LaidBlock {
    /// The size the drop numeral drawn from paragraph run `run` fits its lines at: the top of
    /// its figures level with the top of figures as large as the first line's text, its
    /// baseline on the second line's.
    fn fitted_drop_size(&self, run: usize) -> Option<f32> {
        let first = self.lines.first()?;
        let numeral: Vec<&LaidItem> = first.items.iter().filter(|i| i.run == run).collect();
        let size = numeral.first()?.style.size;
        // How far below the first line's baseline the numeral's baseline is.
        let lowered = -numeral[0].style.raise;
        let top = numeral
            .iter()
            .filter_map(|i| ink_top(&i.shaped, i.style.shape_style(i.rtl)))
            .reduce(f32::max)?;
        if top <= 0.0 || size <= 0.0 {
            return None;
        }
        let text = first
            .items
            .iter()
            .filter(|i| i.run != run)
            .map(|i| i.style.size)
            .reduce(f32::max)?;
        // Figures are `top / size` of their size tall.
        Some(text + lowered * size / top)
    }

    /// The band, top to bottom in px, a range over `item` of line `li` is marked in: the
    /// line's text, not its Interlinear lines (#143); a drop numeral's (#209) whole height.
    pub fn range_band(&self, li: usize, item: &LaidItem) -> (f32, f32) {
        if li == 0
            && let Some(d) = self.drop_numeral.filter(|d| d.run == item.run)
        {
            return (d.top, d.bottom);
        }
        self.lines
            .get(li)
            .map_or((0.0, 0.0), |l| (l.top, l.text_bottom))
    }

    fn run_of(&self, p: usize) -> usize {
        self.run_starts
            .partition_point(|&s| s <= p)
            .saturating_sub(1)
    }

    /// Where paragraph byte `p` came from.
    pub fn to_unit(&self, p: usize) -> Option<UnitPos> {
        let r = self.run_of(p);
        match self.sources.get(r)? {
            RunSource::Text { unit_start } => {
                Some(UnitPos::Text(unit_start + (p - self.run_starts[r])))
            }
            RunSource::Object { anchor, object } => Some(UnitPos::Object {
                anchor: *anchor,
                object: object.clone(),
            }),
        }
    }

    /// The paragraph byte of unit byte `u`, if this block shows it.
    pub fn from_unit(&self, u: usize) -> Option<usize> {
        for (i, src) in self.sources.iter().enumerate() {
            if let RunSource::Text { unit_start } = *src {
                let len = self
                    .run_starts
                    .get(i + 1)
                    .copied()
                    .unwrap_or(self.text.len())
                    - self.run_starts[i];
                if u >= unit_start && u <= unit_start + len {
                    return Some(self.run_starts[i] + (u - unit_start));
                }
            }
        }
        None
    }

    /// Index of the line at `y` (clamped to the first and last line).
    pub fn line_index_at(&self, y: f32) -> usize {
        let i = self.lines.partition_point(|l| l.bottom <= y);
        i.min(self.lines.len().saturating_sub(1))
    }

    /// The cluster exactly under a point, traced back to the unit. Over an interlinear line
    /// it is that line's cell, as an [`InlineObject::InterlinearCell`] anchored at its word's
    /// first byte, with the word's paragraph bytes.
    pub fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        if self.drop_numeral.is_some_and(|d| d.contains(x, y)) {
            return self.item_hit(&self.lines[0], x);
        }
        let li = self.line_index_at(y);
        let line = self.lines.get(li)?;
        if y < line.top || y >= line.bottom {
            return None;
        }
        if y >= line.text_bottom {
            return self.cell_at(li, x, y).map(|c| self.cell_hit(c));
        }
        self.item_hit(line, x)
    }

    /// The cluster of `line` at `x`.
    fn item_hit(&self, line: &LaidLine, x: f32) -> Option<Hit> {
        for it in &line.items {
            let (left, right) = it.extent();
            if x < left || x >= right {
                continue;
            }
            for (bytes, x0, x1) in it.clusters() {
                if x >= x0 && x < x1 {
                    let source = self.to_unit(bytes.start)?;
                    return Some(Hit { bytes, source });
                }
            }
        }
        None
    }

    /// Index of the line a point is on for hit-testing and selection: a drop numeral's
    /// whole box is the first line's.
    fn line_at(&self, x: f32, y: f32) -> usize {
        if self.drop_numeral.is_some_and(|d| d.contains(x, y)) {
            0
        } else {
            self.line_index_at(y)
        }
    }

    /// The interlinear cell of line `li` under a point: within its row, from its column's
    /// left to right edge.
    fn cell_at(&self, li: usize, x: f32, y: f32) -> Option<&LaidCell> {
        self.cells
            .iter()
            .find(|c| c.line == li && y >= c.top && y < c.bottom && x >= c.x && x < c.x + c.width)
    }

    fn cell_hit(&self, cell: &LaidCell) -> Hit {
        let bytes = self
            .from_unit(cell.word.start)
            .zip(self.from_unit(cell.word.end))
            .map_or(0..0, |(a, b)| a..b);
        Hit {
            bytes,
            source: UnitPos::Object {
                anchor: cell.word.start,
                object: InlineObject::InterlinearCell { row: cell.row },
            },
        }
    }

    /// The interlinear cells under the Tagged words that overlap unit bytes `unit` (a
    /// selection), for drawing it.
    pub fn cells_in(&self, unit: Range<usize>) -> impl Iterator<Item = &LaidCell> + '_ {
        self.cells
            .iter()
            .filter(move |c| c.word.start < unit.end && c.word.end > unit.start)
    }

    /// The logical caret position (paragraph byte, on a grapheme boundary) nearest a point:
    /// for selection, which works in document coordinates (#14 rule 9). Between items (the
    /// space an interlinear column adds around its word) it is the nearer item's edge; under
    /// the text, among interlinear lines, it goes by x alone.
    pub fn caret_at(&self, x: f32, y: f32) -> usize {
        let Some(line) = self.lines.get(self.line_at(x, y)) else {
            return 0;
        };
        if line.items.is_empty() {
            return line.bytes.start;
        }
        let left = line
            .items
            .iter()
            .map(|i| i.extent().0)
            .fold(f32::MAX, f32::min);
        let right = line
            .items
            .iter()
            .map(|i| i.extent().1)
            .fold(f32::MIN, f32::max);
        // Beyond the line: the boundary at its visual edge, as just inside it, so a drag across
        // the edge never jumps to the far end of a run in the other direction.
        if x < left || x >= right {
            let edge = if x < left {
                line.items
                    .iter()
                    .min_by(|a, b| a.extent().0.total_cmp(&b.extent().0))
            } else {
                line.items
                    .iter()
                    .max_by(|a, b| a.extent().1.total_cmp(&b.extent().1))
            };
            let Some(it) = edge else {
                return line.bytes.start;
            };
            let cluster = if x < left {
                it.clusters().next()
            } else {
                it.clusters().last()
            };
            let bytes = cluster.map_or(it.bytes.clone(), |(b, _, _)| b);
            return self.snap(if (x < left) ^ it.rtl {
                bytes.start
            } else {
                bytes.end
            });
        }
        for it in &line.items {
            let (left, right) = it.extent();
            if x < left || x >= right {
                continue;
            }
            for (bytes, x0, x1) in it.clusters() {
                if x >= x0 && x < x1 {
                    let left_half = x < (x0 + x1) / 2.0;
                    let p = if left_half ^ it.rtl {
                        bytes.start
                    } else {
                        bytes.end
                    };
                    return self.snap(p);
                }
            }
        }
        // In a gap: the nearest item's nearer edge.
        let nearest = line.items.iter().min_by(|a, b| {
            let d = |i: &LaidItem| {
                let (left, right) = i.extent();
                (left - x).abs().min((right - x).abs())
            };
            d(a).total_cmp(&d(b))
        });
        match nearest {
            Some(it) => {
                let (left, right) = it.extent();
                let at_left = (left - x).abs() <= (right - x).abs();
                self.snap(if at_left ^ it.rtl {
                    it.bytes.start
                } else {
                    it.bytes.end
                })
            }
            None => line.bytes.start,
        }
    }

    /// Round a paragraph byte to the nearest grapheme boundary at or before it.
    fn snap(&self, p: usize) -> usize {
        let p = p.min(self.text.len());
        let mut best = 0;
        for (i, _) in self.text.grapheme_indices(true) {
            if i > p {
                break;
            }
            best = i;
        }
        if p == self.text.len() { p } else { best }
    }

    /// Every glyph with its absolute position (pen x, baseline y) and the item it belongs to,
    /// for painting: the text's, then the interlinear lines'.
    pub fn glyphs(&self) -> impl Iterator<Item = (&LaidLine, &LaidItem, f32, f32, u32)> + '_ {
        let text = self.lines.iter().flat_map(|line| {
            line.items.iter().flat_map(move |it| {
                it.shaped.glyphs.iter().map(move |g| {
                    (
                        line,
                        it,
                        it.x + g.x,
                        line.baseline - it.style.raise - g.y,
                        g.id,
                    )
                })
            })
        });
        let cells = self
            .cells
            .iter()
            .filter_map(|c| Some((self.lines.get(c.line)?, c)));
        let cells = cells.flat_map(|(line, c)| {
            c.items.iter().flat_map(move |it| {
                it.shaped
                    .glyphs
                    .iter()
                    .map(move |g| (line, it, it.x + g.x, c.baseline - g.y, g.id))
            })
        });
        text.chain(cells)
    }
}
