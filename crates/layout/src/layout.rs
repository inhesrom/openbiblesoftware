//! Paragraph → lines: UAX #9 bidi, UAX #14 line breaking, harfrust shaping through the shared
//! cache, greedy line filling, visual reordering, and hit-testing back to unit bytes.
//!
//! Coordinates are px relative to the block's top-left; y grows down. A line's height comes
//! from the tallest run on it (with Hebrew's metrics scaled by `hebrew_line_height`), so
//! stacked marks are not clipped (#14 rule 11).

use std::collections::HashSet;
use std::ops::Range;
use std::sync::Arc;

use obs_fonts::{ShapeCache, ShapedRun};
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
/// column's ([`COLUMN_GAP`]) or stick out past the line's edge, so lines narrower than their
/// words leave the text exactly as it is without them.
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
    for (i, _) in unicode_linebreak::linebreaks(&text) {
        // An interlinear column is never broken: its lines stand under all of its word.
        if i < n && column_at(i).is_none_or(|c| columns[c].chunk.start == i) {
            bounds.push(i);
            breaks.insert(i);
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
        let shaped = match cache.shape(&text[a..b], style.shape_style(rtl)) {
            Ok(s) => s,
            Err(_) => continue,
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
        let mut between = 0.0f32;
        let mut previous: Option<usize> = None;
        let mut i = 0;
        while i < items.len() {
            match items[i].word {
                Some(w) => {
                    if let Some(p) = previous
                        && p != w
                    {
                        let short = columns[p].over[1] + columns[w].over[0] + gap
                            - between
                            - columns[p].pad[1]
                            - columns[w].pad[0];
                        if short > 0.0 {
                            columns[p].pad[1] += short / 2.0;
                            columns[w].pad[0] += short / 2.0;
                        }
                    }
                    previous = Some(w);
                    between = 0.0;
                    while i < items.len() && items[i].word == Some(w) {
                        i += 1;
                    }
                }
                None => {
                    between += items[i].width;
                    i += 1;
                }
            }
        }
    }
    // Extra width an item takes in line filling: its column's padding at each end it is at.
    let pads = |items: &[LaidItem], i: usize| {
        let Some(w) = items[i].word else {
            return 0.0;
        };
        let first = i == 0 || items[i - 1].word != Some(w);
        let last = i + 1 == items.len() || items[i + 1].word != Some(w);
        let [start, end] = columns[w].pad;
        (if first { start } else { 0.0 }) + (if last { end } else { 0.0 })
    };
    let extra: Vec<f32> = (0..items.len()).map(|i| pads(&items, i)).collect();

    // Greedy line filling over unbreakable chunks.
    let available =
        |first: bool| (width - para.inset - if first { para.indent } else { 0.0 }).max(1.0);
    let mut lines_items: Vec<Vec<LaidItem>> = Vec::new();
    let mut cur: Vec<LaidItem> = Vec::new();
    let mut cur_w = 0.0f32;
    let mut chunk: Vec<LaidItem> = Vec::new();
    let mut chunk_w = 0.0f32;
    for (it, extra) in items.into_iter().zip(extra) {
        chunk_w += it.width + extra;
        let br = it.breakable_after;
        let trail = it.trailing_space;
        chunk.push(it);
        if br {
            let avail = available(lines_items.is_empty());
            if cur.is_empty() || cur_w + chunk_w - trail <= avail {
                cur.append(&mut chunk);
                cur_w += chunk_w;
            } else {
                lines_items.push(std::mem::take(&mut cur));
                cur.append(&mut chunk);
                cur_w = chunk_w;
            }
            chunk_w = 0.0;
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
    for (li, its) in lines_items.into_iter().enumerate() {
        let start = its.iter().map(|i| i.bytes.start).min().unwrap_or(n);
        let end = its.iter().map(|i| i.bytes.end).max().unwrap_or(n);
        let lv: Vec<u8> = its.iter().map(|i| levels[i.bytes.start].number()).collect();
        let mut ordered = reorder(its, &lv);
        // Trailing whitespace at the logical end of the line hangs past the margin.
        let hang = ordered
            .iter()
            .find(|i| i.bytes.end == end)
            .map_or(0.0, |i| i.trailing_space);
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
        // Each column's space on its left and right as it is drawn on this line: what its
        // neighbours need, except at the line's ends, where only what keeps its lines inside
        // the line counts.
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
                let at_left = !ordered[..i].iter().any(|o| o.word != Some(w));
                let at_right = !ordered[i + 1..].iter().any(|o| o.word != Some(w));
                let [l, r] = line_pads(w, at_left, at_right);
                [if first { l } else { 0.0 }, if last { r } else { 0.0 }]
            })
            .collect();
        let padding: f32 = edge_pads.iter().map(|[l, r]| l + r).sum();
        let content: f32 = ordered.iter().map(|i| i.width).sum::<f32>() + padding - hang;
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
        // Beyond the line: its logical start on the paragraph's start side, else its end.
        if x < left {
            return if self.rtl {
                line.bytes.end
            } else {
                line.bytes.start
            };
        }
        if x >= right {
            return if self.rtl {
                line.bytes.start
            } else {
                line.bytes.end
            };
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
