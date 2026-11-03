//! Reading-pane state without egui: which block is at the top, heights, laid blocks, the
//! Current location. The scroll position is anchored to a block plus an offset into it, so
//! re-measuring other blocks never moves the text in view (#14 rule 7).

use std::collections::HashMap;
use std::sync::Arc;

use obs_fonts::ShapeCache;
use obs_layout::{LaidBlock, TextSettings, UnitPos, block_paragraph, estimate_height, layout};
use obs_scripture::Verse;

use crate::doc::ReadingDoc;
use crate::heights::HeightIndex;

/// Horizontal margin inside the pane, in points.
pub const MARGIN: f32 = 24.0;

/// One block in view: its index, its top relative to the viewport top, and its layout.
#[derive(Debug, Clone)]
pub struct VisibleBlock {
    pub index: usize,
    pub top: f32,
    pub laid: Arc<LaidBlock>,
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
    /// Blocks laid out so far (for tests and the performance overlay).
    pub layouts: u64,
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
        let l = Arc::new(layout(&p, self.width, ShapeCache::shared()));
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
            let h = self.laid(self.anchor.0).map_or(0.0, |l| l.height);
            self.anchor.1 += h;
        }
        loop {
            let laid = self.laid(self.anchor.0);
            let h = laid.as_ref().map_or(0.0, |l| l.height);
            if self.anchor.0 >= last {
                // At the end, keep the last line in view.
                let last_line = laid
                    .as_ref()
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

    /// Blocks that fill a viewport `height` tall, from the anchor down.
    pub fn visible(&mut self, height: f32) -> Vec<VisibleBlock> {
        let mut out = Vec::new();
        let mut i = self.anchor.0;
        let mut top = -self.anchor.1;
        while top < height && i < self.doc.blocks.len() {
            let Some(laid) = self.laid(i) else { break };
            let h = laid.height;
            out.push(VisibleBlock {
                index: i,
                top,
                laid,
            });
            top += h;
            i += 1;
        }
        // Drop layouts far from view (keep about ±1.5 viewports' worth of blocks).
        if self.laid.len() > 400 {
            let keep_from = self.anchor.0.saturating_sub(60);
            let keep_to = i + 60;
            self.laid.retain(|k, _| (keep_from..keep_to).contains(k));
        }
        out
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
