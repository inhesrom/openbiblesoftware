//! Paragraph → lines: UAX #9 bidi, UAX #14 line breaking, harfrust shaping through the shared
//! cache, greedy line filling, visual reordering, and hit-testing back to unit bytes.
//!
//! Coordinates are px relative to the block's top-left; y grows down. A line's height comes
//! from the tallest run on it (with Hebrew's metrics scaled by `hebrew_line_height`), so
//! stacked marks are not clipped (#14 rule 11).

use std::collections::HashSet;
use std::ops::Range;
use std::sync::Arc;

use obs_fonts::{Cluster, ShapeCache, ShapeStyle, ShapedRun};
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
}

impl LaidItem {
    /// Clusters with paragraph-text byte ranges and block x positions, in visual order.
    pub fn clusters(&self) -> impl Iterator<Item = (Range<usize>, f32, f32)> + '_ {
        let base = self.bytes.start;
        self.shaped.clusters.iter().map(move |c| {
            (
                base + c.bytes.start..base + c.bytes.end,
                self.x + c.x0,
                self.x + c.x1,
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

/// Lay `para` out at `width` px, shaping through `cache`.
pub fn layout(para: &Paragraph, width: f32, cache: &ShapeCache) -> LaidBlock {
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
        let floor = i.checked_sub(1).map_or(0, |p| columns[p].chunk.end);
        let ceiling = columns.get(i + 1).map_or(n, |c| c.bytes.start);
        let c = &mut columns[i];
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
        });
    }

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
    // Greedy line filling over unbreakable chunks.
    let available =
        |first: bool| (width - para.inset - if first { para.indent } else { 0.0 }).max(1.0);
    let mut lines_items: Vec<Vec<LaidItem>> = Vec::new();
    let mut cur: Vec<LaidItem> = Vec::new();
    let mut cur_w = 0.0f32;
    let mut chunk: Vec<LaidItem> = Vec::new();
    let mut chunk_w = 0.0f32;
    for it in items {
        chunk_w += it.width;
        let br = it.breakable_after;
        let trail = it.trailing_space;
        let end = it.bytes.end;
        let style = it.style;
        chunk.push(it);
        if br {
            let avail = available(lines_items.is_empty());
            // Broken at a soft hyphen, the line ends in a hyphen: leave room for it.
            let hyphen = if end < n && ends_in_soft_hyphen(&text, end) {
                hyphen_run(style, false, cache).map_or(0.0, |h| h.width)
            } else {
                0.0
            };
            let fits = cur.is_empty()
                || if columns.is_empty() {
                    cur_w + chunk_w - trail + hyphen <= avail
                } else {
                    // Interlinear columns take space at their edges, more at the line's
                    // ends than between neighbours: measure the line as it would be drawn.
                    let line: Vec<LaidItem> = cur.iter().chain(&chunk).cloned().collect();
                    let (ordered, pads, hang) = arrange_line(line, levels, &columns, &text);
                    line_content(&ordered, &pads, hang) + hyphen <= avail
                };
            if fits && !cur.is_empty() {
                cur.append(&mut chunk);
                cur_w += chunk_w;
            } else {
                if !cur.is_empty() {
                    lines_items.push(std::mem::take(&mut cur));
                }
                // Alone on its line and still too wide (a long name in a narrow Panel):
                // wrap it between grapheme clusters so all of it stays in the column. An
                // interlinear column is never broken.
                let avail = available(lines_items.is_empty());
                if chunk_w - trail > avail && chunk.iter().all(|i| i.word.is_none()) {
                    let mut pieces = split_overwide(
                        std::mem::take(&mut chunk),
                        avail,
                        available(false),
                        &text,
                        cache,
                    );
                    chunk = pieces.pop().unwrap_or_default();
                    lines_items.extend(pieces);
                    chunk_w = chunk.iter().map(|i| i.width).sum();
                }
                cur.append(&mut chunk);
                cur_w = chunk_w;
            }
            chunk_w = 0.0;
            if mandatory.contains(&end) {
                lines_items.push(std::mem::take(&mut cur));
                cur_w = 0.0;
            }
        }
    }
    cur.append(&mut chunk);
    if !cur.is_empty() || lines_items.is_empty() {
        lines_items.push(cur);
    }

    let fallback = para
        .runs
        .first()
        .map_or_else(|| RunStyle::body(16.0), |r| r.style);
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
        let mut x = match (para.align, para.rtl) {
            (Align::Center, _) => ((width - content) / 2.0).max(0.0),
            (Align::Start, false) => para.inset + first_indent,
            (Align::Start, true) => width - para.inset - first_indent - content,
        };
        if para.rtl && para.align == Align::Start {
            // Hanging space sits at the logical end, which is the left edge in RTL.
            x -= hang;
        }
        for (item, [left, right]) in ordered.iter_mut().zip(&edge_pads) {
            x += left;
            item.x = x;
            x += item.width + right;
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
    LaidBlock {
        width,
        height: y + para.space_after,
        rtl: para.rtl,
        text,
        run_starts,
        sources,
        lines,
        cells,
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
        let li = self.line_index_at(y);
        let line = self.lines.get(li)?;
        if y < line.top || y >= line.bottom {
            return None;
        }
        if y >= line.text_bottom {
            return self.cell_at(li, x, y).map(|c| self.cell_hit(c));
        }
        for it in &line.items {
            if x < it.x || x >= it.x + it.width {
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
        let Some(line) = self.lines.get(self.line_index_at(y)) else {
            return 0;
        };
        if line.items.is_empty() {
            return line.bytes.start;
        }
        let left = line.items.iter().map(|i| i.x).fold(f32::MAX, f32::min);
        let right = line
            .items
            .iter()
            .map(|i| i.x + i.width)
            .fold(f32::MIN, f32::max);
        // Beyond the line: the boundary at its visual edge, as just inside it, so a drag across
        // the edge never jumps to the far end of a run in the other direction.
        if x < left || x >= right {
            let edge = if x < left {
                line.items.iter().min_by(|a, b| a.x.total_cmp(&b.x))
            } else {
                line.items
                    .iter()
                    .max_by(|a, b| (a.x + a.width).total_cmp(&(b.x + b.width)))
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
            if x < it.x || x >= it.x + it.width {
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
            let d = |i: &LaidItem| (i.x - x).abs().min((i.x + i.width - x).abs());
            d(a).total_cmp(&d(b))
        });
        match nearest {
            Some(it) => {
                let at_left = (it.x - x).abs() <= (it.x + it.width - x).abs();
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
