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

    // Segment at run starts, break opportunities and direction changes.
    let mut bounds: Vec<usize> = vec![0, n];
    bounds.extend(run_starts.iter().copied());
    let mut breaks = HashSet::new();
    for (i, _) in unicode_linebreak::linebreaks(&text) {
        if i < n {
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
        });
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
    let mut lines = Vec::with_capacity(lines_items.len());
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
        let content: f32 = ordered.iter().map(|i| i.width).sum::<f32>() - hang;
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
        for it in &mut ordered {
            it.x = x;
            x += it.width;
        }
        let top = y;
        let baseline = (top + (line_height - natural) / 2.0 + asc).round();
        y += line_height;
        lines.push(LaidLine {
            top,
            baseline,
            bottom: y,
            items: ordered,
            bytes: start..end,
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

    /// The cluster exactly under a point, traced back to the unit.
    pub fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        let line = self.lines.get(self.line_index_at(y))?;
        if y < line.top || y >= line.bottom {
            return None;
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

    /// The logical caret position (paragraph byte, on a grapheme boundary) nearest a point:
    /// for selection, which works in document coordinates (#14 rule 9).
    pub fn caret_at(&self, x: f32, y: f32) -> usize {
        let Some(line) = self.lines.get(self.line_index_at(y)) else {
            return 0;
        };
        let (Some(first), Some(last)) = (line.items.first(), line.items.last()) else {
            return line.bytes.start;
        };
        // Beyond the line: its logical start on the paragraph's start side, else its end.
        if x < first.x {
            return if self.rtl {
                line.bytes.end
            } else {
                line.bytes.start
            };
        }
        if x >= last.x + last.width {
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
        line.bytes.start
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
    /// for painting.
    pub fn glyphs(&self) -> impl Iterator<Item = (&LaidLine, &LaidItem, f32, f32, u32)> + '_ {
        self.lines.iter().flat_map(|line| {
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
        })
    }
}
