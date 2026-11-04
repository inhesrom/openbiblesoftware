//! Reading-pane state without egui: which block is at the top, heights, laid blocks, the
//! Current location. The scroll position is anchored to a block plus an offset into it, so
//! re-measuring other blocks never moves the text in view (#14 rule 7).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use obs_layout::{LaidBlock, TextSettings, UnitPos, block_paragraph, estimate_height};
use obs_scripture::Verse;

use crate::doc::ReadingDoc;
use crate::heights::HeightIndex;
use crate::selection::{DocPos, Selection};
use crate::workers::{LayoutClient, lay_out};

/// UI-thread layout per frame. #19 allows 2 ms; one block may overrun the check, so stop
/// starting new ones at 1.5 ms.
pub const SYNC_BUDGET: Duration = Duration::from_micros(1500);

/// Predicted UI-thread layout cost per byte of block text (measured cold on the KJV: Genesis,
/// 194 KB, lays out in about 60 ms with an empty shape cache). Blocks predicted not to fit
/// the remaining budget go to the workers instead, so one long paragraph cannot overrun it.
const LAYOUT_COST_PER_BYTE: Duration = Duration::from_nanos(320);

/// How far beyond the viewport to lay out ahead of time, in viewport heights (#19 rule 3).
const PREFETCH: f32 = 1.5;

/// Horizontal margin inside the pane, in points.
pub const MARGIN: f32 = 24.0;

/// One block in view: its index, its top relative to the viewport top, and its layout.
#[derive(Debug, Clone)]
pub struct VisibleBlock {
    pub index: usize,
    pub top: f32,
    pub laid: Arc<LaidBlock>,
}

/// One block in a frame: laid out, or a placeholder of its estimated height while a worker
/// lays it out.
#[derive(Debug, Clone)]
pub struct FrameBlock {
    pub index: usize,
    pub top: f32,
    pub height: f32,
    pub laid: Option<Arc<LaidBlock>>,
}

/// What one frame cost the UI thread.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FrameStats {
    /// Time spent laying out on the UI thread.
    pub sync_layout: Duration,
    pub sync_blocks: usize,
    /// Visible blocks drawn as placeholders.
    pub placeholders: usize,
    /// Layouts sent to workers this frame.
    pub submitted: usize,
    /// Worker layouts applied this frame.
    pub applied: usize,
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub blocks: Vec<FrameBlock>,
    pub stats: FrameStats,
}

#[derive(Debug)]
pub struct ReadingView {
    pub doc: Arc<ReadingDoc>,
    settings: TextSettings,
    width: f32,
    heights: HeightIndex,
    laid: HashMap<usize, Arc<LaidBlock>>,
    /// Block at the top of the viewport, and how far its top is above the viewport top.
    anchor: (usize, f32),
    /// After a jump, the Current location stays on the target until the user scrolls (#13).
    sticky: Option<Verse>,
    /// Blocks laid out so far, on any thread (for tests and the performance overlay).
    pub layouts: u64,
    client: LayoutClient,
    /// Bumped when width or settings change; worker results from older generations are dropped.
    generation: u64,
    /// A follow waiting for its target block's layout (`follow`).
    pending_follow: Option<Verse>,
    /// The current selection, in document coordinates.
    pub selection: Option<Selection>,
    /// Whether the pointer drag in progress is on the scrollbar rather than the text.
    pub(crate) dragging_scrollbar: bool,
    /// Where the pane last drew the text's top-left corner, in screen points.
    pub(crate) text_origin: [f32; 2],
}

impl ReadingView {
    pub fn new(doc: Arc<ReadingDoc>, settings: TextSettings) -> Self {
        let mut view = Self {
            doc,
            settings,
            width: 0.0,
            heights: HeightIndex::new(Vec::new()),
            laid: HashMap::new(),
            anchor: (0, 0.0),
            sticky: None,
            layouts: 0,
            client: LayoutClient::default(),
            generation: 0,
            pending_follow: None,
            selection: None,
            dragging_scrollbar: false,
            text_origin: [0.0, 0.0],
        };
        view.set_width(600.0);
        view
    }

    pub fn settings(&self) -> &TextSettings {
        &self.settings
    }

    /// Text width (pane width less margins). A new width or settings means new layouts; the
    /// height index is keyed by both, so it restarts from estimates.
    pub fn set_width(&mut self, width: f32) {
        let width = width.max(80.0);
        if (width - self.width).abs() < 0.5 && !self.heights.is_empty() {
            return;
        }
        self.width = width;
        self.reset_heights();
    }

    pub fn set_settings(&mut self, settings: TextSettings) {
        if settings.key() != self.settings.key() {
            self.settings = settings;
            self.reset_heights();
        }
    }

    fn reset_heights(&mut self) {
        // Keep the reader on the same verse across a relayout.
        let keep = self.current_location_opt();
        let doc = Arc::clone(&self.doc);
        let estimates = doc
            .blocks
            .iter()
            .map(|b| {
                let p = block_paragraph(
                    &doc.units[b.unit],
                    &b.source,
                    doc.direction(),
                    &self.settings,
                );
                estimate_height(&p, self.width)
            })
            .collect();
        self.heights = HeightIndex::new(estimates);
        self.laid.clear();
        self.generation += 1;
        self.client.forget_pending();
        if let Some(v) = keep {
            self.jump_to(v);
        }
    }

    pub fn width(&self) -> f32 {
        self.width
    }

    /// Lay a block out (synchronously; off-thread layout is #33) and record its height.
    pub fn laid(&mut self, i: usize) -> Option<Arc<LaidBlock>> {
        if let Some(l) = self.laid.get(&i) {
            return Some(Arc::clone(l));
        }
        let doc = Arc::clone(&self.doc);
        let b = doc.blocks.get(i)?;
        let p = block_paragraph(
            &doc.units[b.unit],
            &b.source,
            doc.direction(),
            &self.settings,
        );
        let l = Arc::new(lay_out(&p, self.width));
        self.layouts += 1;
        self.heights.set(i, l.height);
        self.laid.insert(i, Arc::clone(&l));
        Some(l)
    }

    /// Scroll by `dy` points (positive = down the document).
    pub fn scroll_by(&mut self, dy: f32) {
        if dy == 0.0 {
            return;
        }
        self.sticky = None;
        self.pending_follow = None;
        self.anchor.1 += dy;
        self.normalize();
    }

    /// Move the anchor so its offset lies within its block, laying out blocks it passes so
    /// their real heights are used.
    fn normalize(&mut self) {
        let last = self.doc.blocks.len().saturating_sub(1);
        while self.anchor.1 < 0.0 {
            if self.anchor.0 == 0 {
                self.anchor.1 = 0.0;
                break;
            }
            self.anchor.0 -= 1;
            self.anchor.1 += self.height_of(self.anchor.0);
        }
        loop {
            let h = self.height_of(self.anchor.0);
            if self.anchor.0 >= last {
                // At the end, keep the last line in view.
                let last_line = self
                    .laid(self.anchor.0)
                    .and_then(|l| l.lines.last().map(|line| line.top))
                    .unwrap_or(0.0);
                self.anchor.1 = self.anchor.1.min(last_line).max(0.0);
                break;
            }
            if self.anchor.1 < h {
                break;
            }
            self.anchor.1 -= h;
            self.anchor.0 += 1;
        }
    }

    /// Document position of the viewport top, and the document height (for the scrollbar).
    pub fn position(&mut self) -> (f64, f64) {
        let y = self.heights.offset(self.anchor.0) + f64::from(self.anchor.1);
        (y, self.heights.total())
    }

    /// Jump to a document position (scrollbar drag).
    pub fn scroll_to_position(&mut self, y: f64) {
        self.sticky = None;
        self.anchor = self.heights.at(y.max(0.0));
        self.normalize();
    }

    /// A block's height: real if laid out, else estimated. Never lays out.
    fn height_of(&self, i: usize) -> f32 {
        self.laid
            .get(&i)
            .map_or_else(|| self.heights.height(i), |l| l.height)
    }

    fn key(&self, i: usize) -> u64 {
        (self.generation << 32) | i as u64
    }

    /// Queue a block for a worker unless it is laid out or already queued.
    fn prefetch(&mut self, i: usize) -> bool {
        if self.laid.contains_key(&i) || self.client.is_pending(self.key(i)) {
            return false;
        }
        if i >= self.doc.blocks.len() {
            return false;
        }
        let key = self.key(i);
        let (doc, settings, width) = (Arc::clone(&self.doc), self.settings, self.width);
        self.client.submit_with(key, move || {
            let b = &doc.blocks[i];
            let p = block_paragraph(&doc.units[b.unit], &b.source, doc.direction(), &settings);
            lay_out(&p, width)
        });
        true
    }

    /// Laid-out blocks in a viewport `height` tall, with their tops (no layout happens).
    pub fn laid_in_view(&self, height: f32) -> Vec<(usize, f32, Arc<LaidBlock>)> {
        let mut out = Vec::new();
        let mut i = self.anchor.0;
        let mut top = -self.anchor.1;
        while top < height && i < self.doc.blocks.len() {
            if let Some(l) = self.laid.get(&i) {
                out.push((i, top, Arc::clone(l)));
            }
            top += self.height_of(i);
            i += 1;
        }
        out
    }

    /// The document position under a point (`x` from the text's left edge, `y` from the
    /// viewport top), on a grapheme boundary. Points above or below the document clamp to it.
    pub fn pos_at(&mut self, x: f32, y: f32) -> Option<DocPos> {
        let n = self.doc.blocks.len();
        let mut i = self.anchor.0;
        let mut top = -self.anchor.1;
        loop {
            if i >= n {
                return None;
            }
            let h = self.height_of(i);
            if y < top + h || i + 1 == n {
                let laid = self.laid(i)?;
                let p = laid.caret_at(x, (y - top).max(0.0));
                let unit = self.doc.blocks[i].unit;
                let byte = match laid.to_unit(p)? {
                    UnitPos::Text(b) => b,
                    UnitPos::Object { anchor, .. } => anchor,
                };
                return Some(DocPos { unit, byte });
            }
            top += h;
            i += 1;
        }
    }

    /// What is under a point (`x` from the text's left edge, `y` from the viewport top), if
    /// its block is laid out.
    pub fn hit_at(&self, x: f32, y: f32) -> Option<UnitPos> {
        self.hit_in_unit_at(x, y).map(|(_, pos)| pos)
    }

    /// [`Self::hit_at`] with the index of the unit the hit is in.
    pub fn hit_in_unit_at(&self, x: f32, y: f32) -> Option<(usize, UnitPos)> {
        for (index, top, laid) in self.laid_in_view(y + 1.0) {
            if y >= top && y < top + laid.height {
                let unit = self.doc.blocks[index].unit;
                return laid.hit(x, y - top).map(|h| (unit, h.source));
            }
        }
        None
    }

    /// A Scripture-reference or Cross-reference span under the pointer. Uses retained layout
    /// only: hover never lays out a block, and whitespace outside glyphs is not a reference.
    pub fn reference_at(&self, x: f32, y: f32) -> Option<(usize, usize)> {
        for (block, top, laid) in self.laid_in_view(y + 1.0) {
            if y < top || y >= top + laid.height {
                continue;
            }
            let hit = laid.hit(x, y - top)?;
            let unit = self.doc.blocks[block].unit;
            let byte = match hit.source {
                UnitPos::Text(byte) => byte,
                UnitPos::Object {
                    object: obs_layout::InlineObject::ScriptureReference { span },
                    ..
                } => return Some((unit, span)),
                _ => return None,
            };
            let span = self.doc.units[unit].spans.iter().position(|s| {
                matches!(
                    s.kind,
                    obs_model::SpanKind::ScriptureRef { .. } | obs_model::SpanKind::Xref { .. }
                ) && s.start as usize <= byte
                    && byte < s.end as usize
            })?;
            return Some((unit, span));
        }
        None
    }

    /// Whether a `follow` is still waiting for its block's layout.
    pub fn is_following(&self) -> bool {
        self.pending_follow.is_some()
    }

    /// Whether workers still have layouts for this view in flight.
    pub fn has_pending(&self) -> bool {
        self.client.pending() > 0
    }

    /// One frame: apply finished worker layouts, then fill a viewport `height` tall from the
    /// anchor down, laying out on this thread only within `budget` (`None`: no limit) and
    /// drawing placeholders otherwise; finally queue ±1.5 viewports around it for the workers.
    pub fn frame(&mut self, height: f32, budget: Option<Duration>) -> Frame {
        let mut stats = FrameStats::default();
        for (key, laid) in self.client.drain() {
            if key >> 32 == self.generation {
                let i = (key & 0xFFFF_FFFF) as usize;
                self.heights.set(i, laid.height);
                self.laid.insert(i, laid);
                self.layouts += 1;
                stats.applied += 1;
            }
        }
        // Finish a follow once its block has arrived.
        if let Some(v) = self.pending_follow
            && let Some((unit, byte)) = self.doc.verse_position(v)
            && let Some(block) = self.doc.block_at(unit, byte)
            && self.laid.contains_key(&block)
        {
            self.pending_follow = None;
            if self.anchor == (block, 0.0) {
                self.jump_to(v);
            }
        }
        let n = self.doc.blocks.len();
        let mut blocks = Vec::new();
        let mut i = self.anchor.0;
        let mut top = -self.anchor.1;
        while top < height && i < n {
            let laid = match self.laid.get(&i) {
                Some(l) => Some(Arc::clone(l)),
                None if budget.is_none_or(|b| {
                    let bytes = self.doc.blocks[i].source.range.len() as u32;
                    stats.sync_layout + LAYOUT_COST_PER_BYTE * bytes <= b
                }) =>
                {
                    let start = Instant::now();
                    let l = self.laid(i);
                    stats.sync_layout += start.elapsed();
                    stats.sync_blocks += 1;
                    l
                }
                None => {
                    if self.prefetch(i) {
                        stats.submitted += 1;
                    }
                    None
                }
            };
            let h = laid
                .as_ref()
                .map_or_else(|| self.heights.height(i), |l| l.height);
            if laid.is_none() {
                stats.placeholders += 1;
            }
            blocks.push(FrameBlock {
                index: i,
                top,
                height: h,
                laid,
            });
            top += h;
            i += 1;
        }
        // Prefetch below and above the view.
        let mut below = i;
        while top < height * (1.0 + PREFETCH) && below < n {
            if self.prefetch(below) {
                stats.submitted += 1;
            }
            top += self.height_of(below);
            below += 1;
        }
        let mut above_index = self.anchor.0;
        let mut above = self.anchor.1;
        while above < height * PREFETCH && above_index > 0 {
            above_index -= 1;
            if self.prefetch(above_index) {
                stats.submitted += 1;
            }
            above += self.height_of(above_index);
        }
        // Keep layouts around the prefetch window, drop the rest.
        if self.laid.len() > 2 * (below - above_index) + 200 {
            let from = above_index.saturating_sub(100);
            let to = below + 100;
            self.laid.retain(|k, _| (from..to).contains(k));
        }
        Frame { blocks, stats }
    }

    /// Blocks that fill a viewport `height` tall, from the anchor down, all laid out now.
    pub fn visible(&mut self, height: f32) -> Vec<VisibleBlock> {
        self.frame(height, None)
            .blocks
            .into_iter()
            .filter_map(|b| {
                Some(VisibleBlock {
                    index: b.index,
                    top: b.top,
                    laid: b.laid?,
                })
            })
            .collect()
    }

    /// Follow a verse without laying out on this thread: if its block is not laid out yet,
    /// show that block (as a placeholder) and finish the jump when a worker delivers it. This
    /// is how linked followers move, settling a few frames after the origin (#19 rule 3).
    pub fn follow(&mut self, verse: Verse) -> bool {
        let Some((unit, byte)) = self.doc.verse_position(verse) else {
            return false;
        };
        let Some(block) = self.doc.block_at(unit, byte) else {
            return false;
        };
        if self.laid.contains_key(&block) {
            return self.jump_to(verse);
        }
        self.prefetch(block);
        self.anchor = (block, 0.0);
        self.sticky = Some(verse);
        self.pending_follow = Some(verse);
        true
    }

    /// Jump to a verse (own numbering): its line goes to the top of the view and stays the
    /// Current location until the user scrolls.
    pub fn jump_to(&mut self, verse: Verse) -> bool {
        let Some((unit, byte)) = self.doc.verse_position(verse) else {
            return false;
        };
        let Some(block) = self.doc.block_at(unit, byte) else {
            return false;
        };
        let Some(laid) = self.laid(block) else {
            return false;
        };
        let top = laid
            .from_unit(byte)
            .and_then(|p| laid.lines.iter().find(|l| l.bytes.end > p))
            .map_or(0.0, |l| l.top);
        self.anchor = (block, top);
        self.sticky = Some(verse);
        true
    }

    /// Entry Tabs use exactly the same blocks, selection, layout workers and atlas as Bibles.
    pub fn jump_to_entry(&mut self, key: &obs_model::Key) -> bool {
        let Some(entry) = self.doc.navigation.lookup(key) else {
            return false;
        };
        let Some((unit, byte)) = self.doc.entry_position(entry) else {
            return false;
        };
        let Some(block) = self.doc.block_at(unit, byte) else {
            return false;
        };
        let Some(laid) = self.laid(block) else {
            return false;
        };
        let top = laid
            .from_unit(byte)
            .and_then(|p| laid.lines.iter().find(|l| l.bytes.end > p))
            .map_or(0.0, |l| l.top);
        self.anchor = (block, top);
        self.sticky = None;
        self.pending_follow = None;
        true
    }

    pub fn current_entry(&self) -> Option<obs_model::Key> {
        let unit = &self
            .doc
            .units
            .get(self.doc.blocks.get(self.anchor.0)?.unit)?
            .id;
        self.doc
            .navigation
            .entry_for_unit(unit)
            .map(|e| e.key.clone())
    }

    fn current_location_opt(&mut self) -> Option<Verse> {
        if let Some(v) = self.sticky {
            return Some(v);
        }
        if self.doc.blocks.is_empty() || self.heights.is_empty() {
            return None;
        }
        // The top visible line: first line of the anchor block whose bottom is below the top.
        let (mut i, mut offset) = self.anchor;
        loop {
            let laid = self.laid(i)?;
            if let Some(line) = laid.lines.iter().find(|l| l.bottom > offset + 0.5) {
                let unit = self.doc.blocks[i].unit;
                let texts: Vec<usize> = (line.bytes.start..line.bytes.end)
                    .filter_map(|p| match laid.to_unit(p) {
                        Some(UnitPos::Text(u)) => Some(u),
                        Some(UnitPos::Object { anchor, .. }) => Some(anchor),
                        None => None,
                    })
                    .collect();
                let (Some(&start), Some(&end)) = (texts.iter().min(), texts.iter().max()) else {
                    return self
                        .doc
                        .verse_containing(unit, self.doc.blocks[i].source.range.start);
                };
                // A verse starting on the top line wins; else the verse holding its start.
                return self
                    .doc
                    .verses_starting_in(unit, start..end + 1)
                    .first()
                    .copied()
                    .or_else(|| self.doc.verse_containing(unit, start));
            }
            i += 1;
            offset = 0.0;
            if i >= self.doc.blocks.len() {
                return None;
            }
        }
    }

    /// The Current location: the verse starting on the top visible line, else the verse
    /// containing the start of that line (#13); after a jump, the jump target.
    pub fn current_location(&mut self) -> Option<Verse> {
        self.current_location_opt()
    }

    /// Where `verse` is drawn relative to the viewport top, if it is laid out in view.
    pub fn verse_top(&mut self, verse: Verse, viewport: f32) -> Option<f32> {
        let (unit, byte) = self.doc.verse_position(verse)?;
        for vb in self.visible(viewport) {
            if self.doc.blocks[vb.index].unit != unit {
                continue;
            }
            if let Some(p) = vb.laid.from_unit(byte)
                && let Some(line) = vb.laid.lines.iter().find(|l| l.bytes.end > p)
            {
                return Some(vb.top + line.top);
            }
        }
        None
    }
}
