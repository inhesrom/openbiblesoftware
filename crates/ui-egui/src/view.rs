//! Reading-pane state without egui: which block is at the top, heights, laid blocks, the
//! Current location. The scroll position is anchored to a block plus an offset into it, so
//! re-measuring other blocks never moves the text in view (#14 rule 7).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use obs_layout::{
    HeightEstimate, LaidBlock, Paragraph, TextSettings, UnitPos, block_paragraph_with_note_room,
};
use obs_scripture::Verse;

use crate::doc::ReadingDoc;
use crate::heights::HeightIndex;
use crate::interlinear::InterlinearSource;
use crate::selection::{DocPos, Selection};
use crate::tokens::{mark, motion, size, space};
use crate::workers::{LayoutClient, lay_out};

/// UI-thread layout per frame. #19 allows 2 ms; one block may overrun the check, so stop
/// starting new ones at 1.5 ms.
pub const SYNC_BUDGET: Duration = Duration::from_micros(1500);

/// Predicted UI-thread layout cost per byte of block text (measured cold on the KJV: Genesis,
/// 194 KB, lays out in about 60 ms with an empty shape cache). Blocks predicted not to fit
/// the remaining budget go to the workers instead, so one long paragraph cannot overrun it.
const LAYOUT_COST_PER_BYTE: Duration = Duration::from_nanos(320);

/// How many times dearer a block is to lay out with interlinear lines (#143): each Tagged
/// word adds its lines' lookups and shaping.
const INTERLINEAR_COST: u32 = 6;

/// How far beyond the viewport to lay out ahead of time, in viewport heights (#19 rule 3).
const PREFETCH: f32 = 1.5;

/// Horizontal margin inside the pane, in points.
pub const MARGIN: f32 = space::XXL;

/// A Footnote marker in view (#208): the note it stands for, where it sits in the unit, its
/// label as drawn and its box (`x` from the text's left edge, `y` from the viewport top).
#[derive(Debug, Clone, PartialEq)]
pub struct FootnoteMarker {
    pub unit: usize,
    pub anchor: usize,
    pub note: obs_model::UnitId,
    pub kind: obs_model::NoteKind,
    pub label: String,
    pub rect: [f32; 4],
}

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

/// Where a view is scrolled to, in its document's text rather than in points, so it survives a
/// relayout or a new view of the same document: the first visible line's start (`unit`, an
/// index into the document's units, and `byte` in it) and how far the viewport top is below
/// that line's top.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextAnchor {
    pub unit: usize,
    pub byte: usize,
    pub into: f32,
}

/// A jump or keyboard scroll easing into place (#225). The view is already at its new place
/// (its anchor and Current location); it is drawn `offset` points from there, the offset
/// easing to nothing over `duration` seconds of frame time from `start`.
#[derive(Debug, Clone, Copy)]
struct Glide {
    offset: f32,
    start: f64,
    duration: f64,
    ease: fn(f32) -> f32,
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
    /// After a jump to a Text range (a Library search hit, #60) or back to where a view was
    /// scrolled (#151, #214): that text, kept at the top across relayouts until the user
    /// scrolls or the view jumps elsewhere.
    sticky_text: Option<TextAnchor>,
    /// Blocks laid out so far, on any thread (for tests and the performance overlay).
    pub layouts: u64,
    /// Blocks styled for a height estimate so far (for tests).
    pub estimated: u64,
    /// Worker layouts that arrived after the text was laid out anew (a new size, say), and
    /// so were thrown away (for tests).
    pub stale_layouts: u64,
    client: LayoutClient,
    /// Bumped when width or settings change; worker results from older generations are dropped.
    generation: u64,
    /// A follow waiting for its target block's layout (`follow`).
    pending_follow: Option<Verse>,
    /// The current selection, in document coordinates.
    pub selection: Option<Selection>,
    /// Whether the pointer drag in progress is on the scrollbar rather than the text.
    pub(crate) dragging_scrollbar: bool,
    /// Whether the pointer drag in progress carries a Bible reference or the selection out of
    /// the pane (#216) rather than selecting.
    pub(crate) dragging_out: bool,
    /// Where the pane last drew the text's top-left corner, in screen points.
    pub(crate) text_origin: [f32; 2],
    /// The pane last drawn, in screen points: left, top, right, bottom.
    pub(crate) pane_rect: [f32; 4],
    /// The interlinear lines under its Tagged words, when the Tab shows them (#143).
    interlinear: Option<Arc<InterlinearSource>>,
    /// A jump or keyboard scroll still easing into place (#225).
    glide: Option<Glide>,
    /// Frame time (egui's input time, in seconds), which drives the glide.
    clock: f64,
    /// The height of the viewport last drawn.
    viewport: f32,
    /// Every block's height estimate at any width, for the settings and interlinear lines
    /// (their keys) it was measured with: a new width only does the arithmetic (#255), and so
    /// does a new Text size (#299).
    estimates: Vec<HeightEstimate>,
    estimates_key: Option<(u64, Option<u64>)>,
    /// The Text size the estimates were measured at: other sizes scale them (#299).
    estimates_size: f32,
    /// What the last frame since the text was last laid out anew cost (`None`: none yet).
    drawn: Option<FrameStats>,
    /// Where the Notes whose markers it draws end, in document order: the text makes room for
    /// each marker there (#305).
    note_ends: Arc<Vec<DocPos>>,
}

/// Block `i` of `doc` as a paragraph: with its interlinear lines when there are any (or, for
/// an `estimate`, room for them), and room for the markers of Notes ending at `note_ends`.
/// Justified text of an English Resource is hyphenated (#209).
fn paragraph(
    doc: &ReadingDoc,
    i: usize,
    settings: &TextSettings,
    interlinear: Option<&InterlinearSource>,
    note_ends: &[DocPos],
    estimate: bool,
) -> Option<Paragraph> {
    let b = doc.blocks.get(i)?;
    let ends = block_note_ends(note_ends, b.unit, &b.source.range);
    let mut p = match interlinear {
        Some(source) => source.paragraph(doc, i, settings, &ends, estimate)?,
        None => block_paragraph_with_note_room(
            doc.units.get(b.unit)?,
            &b.source,
            doc.direction(),
            settings,
            &ends,
        ),
    };
    p.hyphenate = p.justify && hyphenates(&doc.manifest.language);
    Some(p)
}

/// The unit bytes of `note_ends` in unit `unit` that end text of a block over `range`: past
/// its start, up to and including its end.
fn block_note_ends(
    note_ends: &[DocPos],
    unit: usize,
    range: &std::ops::Range<usize>,
) -> Vec<usize> {
    (note_ends.iter())
        .filter(|p| p.unit == unit && range.start < p.byte && p.byte <= range.end)
        .map(|p| p.byte)
        .collect()
}

/// Whether text in `language` (a BCP 47 tag or ISO 639 code) is hyphenated when justified:
/// only English has a hyphenation dictionary (#209).
pub fn hyphenates(language: &str) -> bool {
    let primary = language.split(['-', '_']).next().unwrap_or_default();
    primary.eq_ignore_ascii_case("en") || primary.eq_ignore_ascii_case("eng")
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
            sticky_text: None,
            layouts: 0,
            estimated: 0,
            stale_layouts: 0,
            client: LayoutClient::default(),
            generation: 0,
            pending_follow: None,
            selection: None,
            dragging_scrollbar: false,
            dragging_out: false,
            text_origin: [0.0, 0.0],
            pane_rect: [0.0; 4],
            interlinear: None,
            glide: None,
            clock: 0.0,
            viewport: 0.0,
            estimates: Vec::new(),
            estimates_key: None,
            estimates_size: TextSettings::DEFAULT_SIZE,
            drawn: None,
            note_ends: Arc::new(Vec::new()),
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
        let width = width.max(size::TEXT_MIN * 2.0);
        if (width - self.width).abs() < 0.5 && !self.heights.is_empty() {
            return;
        }
        self.width = width;
        self.reset_heights();
    }

    /// New settings lay the text out again only if they change its layout; a new Reading
    /// column width changes the width the pane gives [`Self::set_width`] instead.
    pub fn set_settings(&mut self, settings: TextSettings) {
        let relayout = settings.key() != self.settings.key();
        self.settings = settings;
        if relayout {
            self.reset_heights();
        }
    }

    /// The interlinear lines it shows (#143).
    pub fn interlinear(&self) -> Option<&Arc<InterlinearSource>> {
        self.interlinear.as_ref()
    }

    /// A selection as copied: [`crate::copy_text`], with the interlinear lines of its Tagged
    /// words when the Tab shows them (#143).
    pub fn copy_text(&self, sel: &Selection) -> String {
        let rows = self.interlinear.as_ref().map_or_else(Vec::new, |source| {
            let (a, b) = sel.range(&self.doc);
            source.copy_rows(&self.doc, a, b)
        });
        crate::selection::copy_text_with(&self.doc, sel, &rows)
    }

    /// Show these interlinear lines (none: no lines). The text is laid out again, and the
    /// Current location stays where it is.
    pub fn set_interlinear(&mut self, source: Option<Arc<InterlinearSource>>) {
        let key = |s: Option<&Arc<InterlinearSource>>| s.map(|s| s.key());
        if key(source.as_ref()) != key(self.interlinear.as_ref()) {
            self.interlinear = source;
            self.reset_heights();
        }
    }

    /// Make room in the text for the markers of Notes ending at `ends` (#305), in any order.
    /// Only the blocks where a Note's end came or went are laid out again, at once, so the
    /// text around them does not flicker; layouts still with the workers are dropped.
    pub fn set_note_ends(&mut self, mut ends: Vec<DocPos>) {
        ends.sort_unstable();
        ends.dedup();
        if ends == *self.note_ends {
            return;
        }
        let old = std::mem::replace(&mut self.note_ends, Arc::new(ends));
        let changed: std::collections::BTreeSet<usize> = (old.iter())
            .filter(|p| self.note_ends.binary_search(p).is_err())
            .chain(
                self.note_ends
                    .iter()
                    .filter(|p| old.binary_search(p).is_err()),
            )
            .filter_map(|p| self.doc.block_at(p.unit, p.byte.saturating_sub(1)))
            .collect();
        if changed.is_empty() {
            return;
        }
        self.generation += 1;
        self.client.forget_pending();
        for i in changed {
            if self.laid.remove(&i).is_some() {
                self.laid(i);
            }
        }
    }

    fn reset_heights(&mut self) {
        self.glide = None;
        // Keep the reader on the same verse across a relayout. A Commentary scrolled by hand
        // (its verse is its section's, which may start far above) and a Resource without
        // verses keep the same text at the top instead.
        let commentary = self.doc.manifest.resource_type == obs_model::ResourceType::Commentary;
        let by_verse = self.sticky.is_some() || !commentary;
        let keep = self.current_location_opt().filter(|_| by_verse);
        let keep_text = (keep.is_none() && self.sticky_text.is_none() && !self.heights.is_empty())
            .then(|| self.text_anchor())
            .flatten();
        let interlinear = self.interlinear.as_ref().map(|s| s.key());
        // A new Text size scales the estimates measured at another size (#299): styling every
        // block of a whole Bible again takes hundreds of milliseconds.
        let measured_at = TextSettings {
            size: self.estimates_size,
            ..self.settings
        };
        if self.estimates_key != Some((measured_at.key(), interlinear)) {
            let source = self.interlinear.as_deref();
            self.estimates = (0..self.doc.blocks.len())
                .map(|i| {
                    paragraph(&self.doc, i, &self.settings, source, &[], true)
                        .map_or_else(HeightEstimate::default, |p| HeightEstimate::of(&p))
                })
                .collect();
            self.estimates_key = Some((self.settings.key(), interlinear));
            self.estimates_size = self.settings.size;
            self.estimated += self.estimates.len() as u64;
        }
        let (width, k) = (self.width, self.settings.size / self.estimates_size);
        self.heights = HeightIndex::new(
            (self.estimates.iter())
                .map(|e| e.scaled(k).at(width))
                .collect(),
        );
        self.laid.clear();
        self.drawn = None;
        self.generation += 1;
        // Layouts queued at the old settings are not wanted: the workers skip them (#299).
        self.client.forget_pending();
        if let Some(at) = self.sticky_text {
            self.place_text(at);
        } else if let Some(v) = keep {
            self.jump_to(v);
        } else if let Some(at) = keep_text {
            self.place_text(TextAnchor { into: 0.0, ..at });
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
        let p = paragraph(
            &self.doc,
            i,
            &self.settings,
            self.interlinear.as_deref(),
            &self.note_ends,
            false,
        )?;
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
        // Scrolling during a glide starts from where the text is drawn.
        self.anchor.1 += self.glide_offset();
        self.glide = None;
        self.sticky = None;
        self.sticky_text = None;
        self.pending_follow = None;
        self.anchor.1 += dy;
        self.normalize();
    }

    /// Set the frame time (egui's input time, in seconds), which drives a glide.
    pub fn set_clock(&mut self, now: f64) {
        self.clock = now;
    }

    /// How far from its anchor the view is drawn now: a glide's offset, eased (0: none).
    fn glide_offset(&self) -> f32 {
        self.glide.map_or(0.0, |g| {
            let p = motion::progress(self.clock, g.start, g.duration);
            g.offset * (1.0 - (g.ease)(p))
        })
    }

    /// Whether a jump or scroll is still easing into place, so the pane draws again.
    pub fn is_gliding(&self) -> bool {
        self.glide
            .is_some_and(|g| !motion::is_done(self.clock, g.start, g.duration))
    }

    /// The document position of the viewport top as drawn now (during a glide, short of
    /// [`Self::position`]): where a new glide starts from.
    pub fn shown_position(&mut self) -> f64 {
        self.position().0 + f64::from(self.glide_offset())
    }

    /// Ease the view from document position `from` (where it was drawn, [`Self::shown_position`])
    /// to where it is now (#225): over `duration` seconds of frame time, eased by `ease`. A
    /// long way glides only its last [`motion::JUMP_REACH`] pane heights, and only through
    /// text already laid out, so a glide never draws a placeholder; what is left lands at
    /// once. The view's place (and Current location) is the new one from the start.
    pub fn glide_from(&mut self, from: f64, duration: f64, ease: fn(f32) -> f32) {
        let to = self.position().0;
        let reach = motion::JUMP_REACH * self.viewport;
        let offset = ((from - to) as f32).clamp(-reach, reach);
        let offset = if offset < 0.0 {
            offset.max(-self.laid_above())
        } else {
            offset.min(self.laid_below())
        };
        self.glide = (offset.abs() >= space::PX).then_some(Glide {
            offset,
            start: self.clock,
            duration,
            ease,
        });
    }

    /// How far above the viewport top the text is laid out without a gap.
    fn laid_above(&self) -> f32 {
        if !self.laid.contains_key(&self.anchor.0) {
            return 0.0;
        }
        let mut room = self.anchor.1;
        let mut i = self.anchor.0;
        while i > 0 && self.laid.contains_key(&(i - 1)) {
            i -= 1;
            room += self.height_of(i);
        }
        room
    }

    /// How far below the viewport bottom the text is laid out without a gap.
    fn laid_below(&self) -> f32 {
        let mut bottom = -self.anchor.1;
        let mut i = self.anchor.0;
        while self.laid.contains_key(&i) {
            bottom += self.height_of(i);
            i += 1;
        }
        (bottom - self.viewport).max(0.0)
    }

    /// The block at the top of the viewport as drawn now, and how far its top is above the
    /// viewport top: the anchor, moved by a glide's offset.
    fn shown_anchor(&self) -> (usize, f32) {
        let offset = self.glide_offset();
        if offset == 0.0 {
            return self.anchor;
        }
        let (mut i, mut y) = (self.anchor.0, self.anchor.1 + offset);
        while y < 0.0 && i > 0 {
            i -= 1;
            y += self.height_of(i);
        }
        let n = self.doc.blocks.len();
        while i + 1 < n && y >= self.height_of(i) {
            y -= self.height_of(i);
            i += 1;
        }
        (i, y.max(0.0))
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
        self.glide = None;
        self.sticky = None;
        self.sticky_text = None;
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
        let interlinear = self.interlinear.clone();
        let note_ends = Arc::clone(&self.note_ends);
        self.client.submit_with(key, move || {
            match paragraph(
                &doc,
                i,
                &settings,
                interlinear.as_deref(),
                &note_ends,
                false,
            ) {
                Some(p) => lay_out(&p, width),
                // Checked above: the block exists.
                None => lay_out(
                    &obs_layout::snippet_paragraph(
                        "",
                        obs_layout::ColorRole::Ink,
                        false,
                        &settings,
                    ),
                    width,
                ),
            }
        });
        true
    }

    /// Laid-out blocks in a viewport `height` tall, with their tops (no layout happens).
    pub fn laid_in_view(&self, height: f32) -> Vec<(usize, f32, Arc<LaidBlock>)> {
        let mut out = Vec::new();
        let (mut i, top) = self.shown_anchor();
        let mut top = -top;
        while top < height && i < self.doc.blocks.len() {
            if let Some(l) = self.laid.get(&i) {
                out.push((i, top, Arc::clone(l)));
            }
            top += self.height_of(i);
            i += 1;
        }
        out
    }

    /// The Footnote markers in the laid blocks of a viewport `height` tall, in text order.
    pub fn footnote_markers(&self, height: f32) -> Vec<FootnoteMarker> {
        let mut out: Vec<FootnoteMarker> = Vec::new();
        for (index, top, laid) in self.laid_in_view(height) {
            let unit = self.doc.blocks[index].unit;
            let first = out.len();
            for line in &laid.lines {
                for item in &line.items {
                    for (bytes, x0, x1) in item.clusters() {
                        let Some(UnitPos::Object {
                            anchor,
                            object: obs_layout::InlineObject::NoteMarker { note, kind },
                        }) = laid.to_unit(bytes.start)
                        else {
                            continue;
                        };
                        let text = laid.text.get(bytes).unwrap_or_default();
                        let (y0, y1) = (top + line.top, top + line.text_bottom);
                        // A label of several letters is one marker.
                        if let Some(m) = out[first..]
                            .iter_mut()
                            .find(|m| m.anchor == anchor && m.note == note)
                        {
                            m.label.push_str(text);
                            m.rect[0] = m.rect[0].min(x0);
                            m.rect[2] = m.rect[2].max(x1);
                            continue;
                        }
                        out.push(FootnoteMarker {
                            unit,
                            anchor,
                            note,
                            kind,
                            label: text.to_string(),
                            rect: [x0, y0, x1, y1],
                        });
                    }
                }
            }
        }
        for m in &mut out {
            m.label.retain(|c| !c.is_control() && c != '\u{200F}');
            // The glyph sits high in the line: its box is the line's upper half.
            m.rect[3] = m.rect[1] + (m.rect[3] - m.rect[1]) * mark::FOOTNOTE_BOX;
        }
        out
    }

    /// The document position under a point (`x` from the text's left edge, `y` from the
    /// viewport top), on a grapheme boundary. Points above or below the document clamp to it.
    pub fn pos_at(&mut self, x: f32, y: f32) -> Option<DocPos> {
        let n = self.doc.blocks.len();
        let (mut i, top) = self.shown_anchor();
        let mut top = -top;
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

    /// The text under a screen point in the pane as last drawn (a Note dropped on it, #216):
    /// `None` off the pane or off the text, where [`Self::pos_at`] would clamp to it.
    pub(crate) fn text_under(&self, x: f32, y: f32) -> Option<DocPos> {
        let [left, top, right, bottom] = self.pane_rect;
        if x < left || x >= right || y < top || y >= bottom {
            return None;
        }
        let [ox, oy] = self.text_origin;
        let (unit, pos) = self.hit_in_unit_at(x - ox, y - oy)?;
        let byte = match pos {
            UnitPos::Text(b) => b,
            UnitPos::Object { anchor, .. } => anchor,
        };
        Some(DocPos { unit, byte })
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

    /// Whether the block at the top of the view is laid out, so a frame draws its text (the
    /// scale bench's "first Bible text", #227).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn top_block_laid(&self) -> bool {
        self.laid.contains_key(&self.anchor.0)
    }

    /// Whether a frame has drawn every block in view since the text was last laid out anew (a
    /// new size, say), none as a placeholder: the Text size bench's "change shown" (#299).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn drew_all_text(&self) -> bool {
        self.drawn.is_some_and(|s| s.placeholders == 0)
    }

    /// Layouts the workers did for this view, stale ones included (tests).
    #[cfg(test)]
    pub(crate) fn worker_layouts(&self) -> u64 {
        self.client.ran()
    }

    /// Keep this view's layouts back from the workers until [`Self::release_layouts`], as a
    /// pool busy with other work would (tests).
    #[cfg(test)]
    pub(crate) fn hold_layouts(&mut self) {
        self.client.hold();
    }

    #[cfg(test)]
    pub(crate) fn release_layouts(&mut self) {
        self.client.release();
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
            } else {
                self.stale_layouts += 1;
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
        self.viewport = height;
        if !self.is_gliding() {
            self.glide = None;
        }
        let n = self.doc.blocks.len();
        let mut blocks = Vec::new();
        let (mut i, top) = self.shown_anchor();
        let mut top = -top;
        let cost_per_byte = if self.interlinear.is_some() {
            LAYOUT_COST_PER_BYTE * INTERLINEAR_COST
        } else {
            LAYOUT_COST_PER_BYTE
        };
        while top < height && i < n {
            let laid = match self.laid.get(&i) {
                Some(l) => Some(Arc::clone(l)),
                None if budget.is_none_or(|b| {
                    let bytes = self.doc.blocks[i].source.range.len() as u32;
                    stats.sync_layout + cost_per_byte * bytes <= b
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
        self.drawn = Some(stats);
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
        self.glide = None;
        self.anchor = (block, 0.0);
        self.sticky = Some(verse);
        self.sticky_text = None;
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
        self.glide = None;
        self.anchor = (block, top);
        self.sticky = Some(verse);
        self.sticky_text = None;
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
        let jumped = self.jump_to_text(unit, byte);
        self.sticky_text = None;
        jumped
    }

    /// Jump to byte `byte` of unit `unit` (an index into the document's units): its line goes
    /// to the top of the view and stays there across relayouts until the user scrolls. This
    /// is how a Library search hit scrolls to its paragraph (#60).
    pub fn jump_to_text(&mut self, unit: usize, byte: usize) -> bool {
        let at = TextAnchor {
            unit,
            byte,
            into: 0.0,
        };
        if !self.place_text(at) {
            return false;
        }
        self.sticky = None;
        self.sticky_text = Some(at);
        self.pending_follow = None;
        true
    }

    /// Put the line holding `at` at the top of the view, `at.into` above it.
    fn place_text(&mut self, at: TextAnchor) -> bool {
        let Some(block) = self.doc.block_at(at.unit, at.byte) else {
            return false;
        };
        let Some(laid) = self.laid(block) else {
            return false;
        };
        let top = laid
            .from_unit(at.byte)
            .and_then(|p| laid.lines.iter().find(|l| l.bytes.end > p))
            .map_or(0.0, |l| l.top);
        self.glide = None;
        self.anchor = (block, top);
        if at.into != 0.0 {
            self.anchor.1 += at.into;
            self.normalize();
        }
        true
    }

    /// Where the view is scrolled to, as a [`TextAnchor`].
    pub fn text_anchor(&mut self) -> Option<TextAnchor> {
        let (mut i, mut offset) = self.anchor;
        loop {
            let laid = self.laid(i)?;
            if let Some(line) = laid.lines.iter().find(|l| l.bottom > offset + 0.5) {
                let byte = (line.bytes.start..line.bytes.end)
                    .find_map(|p| match laid.to_unit(p) {
                        Some(UnitPos::Text(u)) => Some(u),
                        Some(UnitPos::Object { anchor, .. }) => Some(anchor),
                        None => None,
                    })
                    .unwrap_or(self.doc.blocks[i].source.range.start);
                return Some(TextAnchor {
                    unit: self.doc.blocks[i].unit,
                    byte,
                    into: (offset - line.top).max(0.0),
                });
            }
            i += 1;
            offset = 0.0;
            if i >= self.doc.blocks.len() {
                return None;
            }
        }
    }

    /// Scroll back to a [`TextAnchor`] taken from a view of the same document; it stays
    /// there, as far into its line, across relayouts (a narrower or wider pane) until the
    /// user scrolls or the view jumps elsewhere. With a `verse`, that stays the Current
    /// location meanwhile, as after [`Self::jump_to`].
    pub fn restore_text_anchor(&mut self, at: TextAnchor, verse: Option<Verse>) -> bool {
        if !self.place_text(at) {
            return false;
        }
        self.sticky = verse;
        self.sticky_text = Some(at);
        self.pending_follow = None;
        true
    }

    /// The unit (an index into the document's units) at the top of the view.
    pub fn top_unit(&self) -> Option<usize> {
        self.doc.blocks.get(self.anchor.0).map(|b| b.unit)
    }

    /// Where byte `byte` of unit `unit` is drawn relative to the viewport top, if it is laid
    /// out in a viewport `viewport` tall.
    pub fn text_top(&mut self, unit: usize, byte: usize, viewport: f32) -> Option<f32> {
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

    /// Bring a text position (unit index, byte) to the top of the view: the Notes Tool
    /// following a Text-range anchor (#64).
    pub fn jump_to_pos(&mut self, unit: usize, byte: usize) -> bool {
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
        self.sticky_text = None;
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
