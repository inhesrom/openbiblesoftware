//! Prototype (plan S07, #331): what the iOS text view prototype asks of a reader. The shell
//! scrolls a `UIScrollView` whose content coordinates are the reader's page in points (x from
//! the page's left edge, y from the document's top) and asks this module for everything it
//! shows or decides there: which tiles to keep and where they go, their pixels, the text
//! under a point, word and Verse ranges, selection rectangles, the accessibility elements and
//! what a tap opens. Nothing here is the designed surface; S13 replaces it.
//!
//! **Scrolling.** The reader's [`ReadingView`] stays anchored to a block (#14 rule 7). The
//! shell reports its content offset; the reader scrolls by the difference from the position
//! it last reported, lays out what is in view and a viewport either side of it, and reports
//! its position again. When blocks above the view were laid out at heights other than their
//! estimates, that position differs from the offset the shell sent, and the shell moves its
//! offset there: the text in view never jumps.
//!
//! **Tiles.** A tile is a band of one block, at most [`BAND`] points tall, keyed by
//! [`TileKey`] (the epoch, the block and the band). Its cost is that block's layout and the
//! band's pixels wherever the block is in the document, so it does not grow with how far the
//! reader has scrolled (S03's `layout_tile` laid out everything from the Current location
//! down). The shell keeps the images of the keys the last frame listed and drops the rest; a
//! new width, scale, Page tone or Text size starts a new epoch, so every key changes.

use std::sync::Arc;

use obs_engine::appearance::{PageTone, TEXT_SIZE_MAX, TEXT_SIZE_MIN};
use obs_layout::{LaidBlock, UnitPos};
use obs_reading::{DocPos, ReadingView, Selection, TonePalette};
use obs_scripture::{Passage, RefStyle, Verse, Versification, format_passage};

use crate::{MobileApp, MobileError, ReaderId, Tile};

/// The tallest tile, in points: a block taller than this is drawn in several bands.
pub const BAND: f32 = 256.0;

/// How much of the page is kept laid out and drawn above and below the view, in viewport
/// heights, so a fling shows tiles drawn ahead.
pub const AROUND: f32 = 1.0;

/// A tile of a reader's page: block `block`'s band `band` (from [`BAND`] × `band` points
/// below the block's top), drawn in epoch `epoch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileKey {
    pub epoch: u32,
    pub block: u32,
    pub band: u32,
}

/// Where a tile goes: its top in content points (rounded to device pixels) and its height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TilePlace {
    pub key: TileKey,
    pub y: f64,
    pub height: f32,
}

/// What the shell shows for one content offset.
#[derive(Debug, Clone, PartialEq)]
pub struct ReaderFrame {
    /// Where the view's top is now, in content points: move the content offset here.
    pub position: f64,
    /// The page's height so far (estimated below what is laid out), in points.
    pub total: f64,
    /// The page colour, behind and between the tiles.
    pub page: [u8; 3],
    /// The tiles to keep, top first.
    pub tiles: Vec<TilePlace>,
    /// The Current location, as the header shows it ("Genesis 1:3").
    pub location: String,
    /// Time spent laying out and placing, in microseconds.
    pub micros: u32,
}

/// A tile's pixels and how long they took to draw, in microseconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimedTile {
    pub tile: Tile,
    pub micros: u32,
}

/// A rectangle in content points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContentRect {
    pub x: f32,
    pub y: f64,
    pub width: f32,
    pub height: f32,
}

/// The text units the shell's text input asks about (UIKit's `UITextGranularity`); what each
/// covers is decided here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextUnitKind {
    Character,
    Word,
    Sentence,
    Paragraph,
    Line,
    Document,
}

/// A range of text, as text offsets (see [`MobileApp::text_offset`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextSpan {
    pub start: u64,
    pub end: u64,
}

/// One accessibility element: a Verse in a Bible, a block (a heading) otherwise.
#[derive(Debug, Clone, PartialEq)]
pub struct AccessibilityItem {
    /// Stable for the same text: `Gen.1.1` for a Verse, `block.12` for a block.
    pub id: String,
    /// What VoiceOver reads: the Verse's reference, then its text.
    pub label: String,
    /// The box around its text, in content points.
    pub frame: ContentRect,
    /// Whether it is a heading rather than reading text.
    pub heading: bool,
}

/// What a tap opens (a stub of the Study sheet, S16): the Verse, and the word when it is a
/// Tagged word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudySheetStub {
    /// `Genesis 1:1 (BSB)`.
    pub title: String,
    pub verse_text: String,
    /// The word tapped, when it carries original-language data.
    pub word: Option<String>,
    pub strongs: Option<String>,
    pub lemma: Option<String>,
    pub gloss: Option<String>,
    pub cross_reference_count: u32,
    /// `12 cross-references`.
    pub cross_reference_label: String,
}

/// A choice the shell lists (a Page tone, a Bible).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub id: String,
    pub label: String,
}

/// A Bible book of a reader's Resource, for the book and chapter grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookChapters {
    pub name: String,
    pub chapters: u16,
}

/// What the text view keeps for one reader besides its [`ReadingView`].
#[derive(Debug, Default)]
pub(crate) struct ReaderState {
    /// The position last reported to the shell.
    last: f64,
    /// The reader moved by itself (a jump, a new Text size) since the last frame: the next
    /// frame takes no scroll from the shell's offset, and the shell takes its position.
    moved: bool,
    /// The page width and scale of the last frame.
    page: (f32, f32),
    /// The text column's left edge on the page.
    left: f32,
    /// The blocks laid out around the view: index, top in content points, layout.
    window: Vec<(usize, f64, Arc<LaidBlock>)>,
    /// Each unit's first text offset (units are joined by one offset, as a newline).
    unit_starts: Vec<u64>,
}

/// Text Size for UIKit's content size categories (`UIContentSizeCategory` raw values),
/// within the app's 12–40 pt: Large is the desktop's default, 20 pt. The same table serves
/// Android's font scale later (plan A4).
pub fn text_size_for_content_size(category: &str) -> f32 {
    let size: f32 = match category {
        "UICTContentSizeCategoryXS" => 15.0,
        "UICTContentSizeCategoryS" => 17.0,
        "UICTContentSizeCategoryM" => 18.0,
        "UICTContentSizeCategoryXL" => 22.0,
        "UICTContentSizeCategoryXXL" => 24.0,
        "UICTContentSizeCategoryXXXL" => 26.0,
        "UICTContentSizeCategoryAccessibilityM" => 29.0,
        "UICTContentSizeCategoryAccessibilityL" => 32.0,
        "UICTContentSizeCategoryAccessibilityXL" => 35.0,
        "UICTContentSizeCategoryAccessibilityXXL" => 38.0,
        "UICTContentSizeCategoryAccessibilityXXXL" => 40.0,
        _ => obs_engine::appearance::TEXT_SIZE_DEFAULT,
    };
    size.clamp(TEXT_SIZE_MIN, TEXT_SIZE_MAX)
}

impl ReaderState {
    /// A new reader's: its first frame takes the reader's position.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) fn new() -> Self {
        Self {
            moved: true,
            ..Self::default()
        }
    }
}

fn micros(since: std::time::Instant) -> u32 {
    u32::try_from(since.elapsed().as_micros()).unwrap_or(u32::MAX)
}

fn plural(n: u32, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

impl MobileApp {
    fn reader_parts(
        &mut self,
        reader: ReaderId,
    ) -> Result<(&mut ReadingView, &mut ReaderState), MobileError> {
        let i = reader.0 as usize;
        match (self.readers.get_mut(i), self.states.get_mut(i)) {
            (Some(v), Some(s)) => Ok((v, s)),
            _ => Err(MobileError::NoReader(reader)),
        }
    }

    fn palette(&self) -> TonePalette {
        let appearance = self.session.appearance;
        TonePalette::for_theme(
            appearance.page_tone.resolve(self.dark),
            appearance.theme.is_high_contrast(),
        )
    }

    /// Start a new epoch: every tile is drawn again.
    fn new_epoch(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
    }

    /// The frame for the shell's content offset `offset` (clamped to the page by the shell)
    /// in a viewport `width` × `height` points at `scale` px per point, with the system
    /// appearance dark or not (for the Auto Page tone). Prototype.
    pub fn reader_frame(
        &mut self,
        reader: ReaderId,
        offset: f64,
        width: f32,
        height: f32,
        scale: f32,
        dark: bool,
    ) -> Result<ReaderFrame, MobileError> {
        let positive = |x: f32| x.is_finite() && x > 0.0;
        if !(positive(width) && positive(height) && positive(scale) && offset.is_finite()) {
            return Err(MobileError::TileSize);
        }
        let started = std::time::Instant::now();
        let mut epoch = false;
        if dark != self.dark {
            self.dark = dark;
            epoch = true;
        }
        let page = self.palette().page.0;
        let (view, state) = self.reader_parts(reader)?;
        if state.page != (width, scale) {
            state.page = (width, scale);
            epoch = true;
        }
        let (left, measure) = obs_reading::text_column(view, width);
        let before = view.width();
        view.set_width(measure);
        if (view.width() - before).abs() < 0.5 && !state.moved {
            view.scroll_by((offset - state.last) as f32);
        }
        state.moved = false;
        state.left = left;
        let blocks = view.visible_around(height * AROUND, height * (1.0 + AROUND));
        let position = view.position().0;
        let total = view.position().1;
        state.last = position;
        state.window = (blocks.iter())
            .map(|b| (b.index, position + f64::from(b.top), Arc::clone(&b.laid)))
            .collect();
        // Over a heading (the top of a book) the Current location is the first Verse below.
        let current = view.current_location();
        let doc = Arc::clone(&view.doc);
        let below = || {
            (blocks.iter())
                .filter(|b| b.top + b.laid.height > 0.0)
                .find_map(|b| {
                    let block = &doc.blocks[b.index];
                    (doc.verses_starting_in(block.unit, block.source.range.clone()))
                        .first()
                        .copied()
                })
        };
        let location = (current.or_else(below))
            .map(|v| format_passage(Passage::verse(v), &doc.versification, RefStyle::Long))
            .unwrap_or_default();
        if epoch {
            self.new_epoch();
        }
        let epoch = self.epoch;
        let state = &self.states[reader.0 as usize];
        let snap = |y: f64| (y * f64::from(scale)).round() / f64::from(scale);
        let mut tiles = Vec::new();
        for (index, top, laid) in &state.window {
            let mut band = 0u32;
            while (band as f32) * BAND < laid.height {
                let from = band as f32 * BAND;
                tiles.push(TilePlace {
                    key: TileKey {
                        epoch,
                        block: *index as u32,
                        band,
                    },
                    y: snap(top + f64::from(from)),
                    height: (laid.height - from).min(BAND),
                });
                band += 1;
            }
        }
        Ok(ReaderFrame {
            position,
            total,
            page,
            tiles,
            location,
            micros: micros(started),
        })
    }

    /// The pixels of a tile the last frame listed, at `scale` px per point; `None` when its
    /// epoch is over (the shell drops it). Prototype.
    pub fn reader_tile(
        &mut self,
        reader: ReaderId,
        key: TileKey,
        scale: f32,
    ) -> Result<Option<TimedTile>, MobileError> {
        if key.epoch != self.epoch {
            return Ok(None);
        }
        let started = std::time::Instant::now();
        let palette = self.palette();
        let i = reader.0 as usize;
        let (Some(view), Some(state)) = (self.readers.get_mut(i), self.states.get(i)) else {
            return Err(MobileError::NoReader(reader));
        };
        let width = state.page.0;
        let block = key.block as usize;
        let Some(height) = view.laid(block).map(|l| l.height) else {
            return Ok(None);
        };
        let from = key.band as f32 * BAND;
        let band = (height - from).min(BAND);
        if band.is_nan() || band <= 0.0 {
            return Ok(None);
        }
        let (w, h) = obs_render::tile_size([0.0, from, width, from + band], scale);
        if w > crate::MAX_TILE_PX || h > crate::MAX_TILE_PX {
            return Err(MobileError::TileSize);
        }
        let tile = obs_reading::compose_block_tile(
            view,
            &mut self.atlas,
            &palette,
            width,
            block,
            from,
            band,
            scale,
        );
        Ok(tile.map(|tile| TimedTile {
            tile,
            micros: micros(started),
        }))
    }

    /// Go to a typed Bible reference, read in the reader's own numbering (`Jn 3:16`): its
    /// first Verse goes to the top. Returns the new position for the content offset.
    /// Prototype.
    pub fn go_to(&mut self, reader: ReaderId, text: &str) -> Result<f64, MobileError> {
        let (view, state) = self.reader_parts(reader)?;
        let refused = |error: String| MobileError::Reference {
            reference: text.to_string(),
            error,
        };
        let context = view.current_location();
        let list = obs_scripture::parse_reference(text, &view.doc.versification, context)
            .map_err(|e| refused(e.to_string()))?;
        let verse = (list.passages().first())
            .map(|p| p.start())
            .ok_or_else(|| refused("no Passage".into()))?;
        if !view.jump_to(verse) {
            let abbreviation = view.doc.manifest.abbreviation.clone();
            return Err(refused(format!("not in {abbreviation}")));
        }
        state.moved = true;
        Ok(view.position().0)
    }

    /// The Bible books of the reader's Resource, in its order, with their chapter counts.
    /// Prototype.
    pub fn books(&self, reader: ReaderId) -> Vec<BookChapters> {
        let Some(view) = self.readers.get(reader.0 as usize) else {
            return Vec::new();
        };
        let mut books: Vec<obs_scripture::Book> = Vec::new();
        for v in &view.doc.verses {
            if books.last() != Some(&v.verse.book) && !books.contains(&v.verse.book) {
                books.push(v.verse.book);
            }
        }
        (books.into_iter())
            .map(|b| BookChapters {
                name: b.name().to_string(),
                chapters: view.doc.versification.chapter_count(b),
            })
            .collect()
    }

    /// Go to chapter `chapter` of the `book`th book [`Self::books`] lists. Prototype.
    pub fn go_to_chapter(
        &mut self,
        reader: ReaderId,
        book: u32,
        chapter: u16,
    ) -> Result<f64, MobileError> {
        let books = self.books(reader);
        let name = (books.get(book as usize))
            .map(|b| b.name.clone())
            .ok_or(MobileError::NoReader(reader))?;
        let (view, state) = self.reader_parts(reader)?;
        let refused = |error: &str| MobileError::Reference {
            reference: format!("{name} {chapter}"),
            error: error.to_string(),
        };
        let b = (view.doc.verses.iter())
            .map(|v| v.verse.book)
            .find(|b| b.name() == name)
            .ok_or_else(|| refused("no such book"))?;
        let verse = (view.doc.versification.first_verse(b, chapter))
            .ok_or_else(|| refused("no such chapter"))?;
        if !view.jump_to(verse) && !view.jump_to(Verse::new(b, chapter, 1)) {
            return Err(refused("not in this Bible"));
        }
        state.moved = true;
        Ok(view.position().0)
    }

    /// The Page tones, as the Page tone switch lists them, and the one in use.
    pub fn page_tones(&self) -> (Vec<Choice>, String) {
        let choices = (PageTone::ALL.iter())
            .map(|(_, id, label)| Choice {
                id: (*id).to_string(),
                label: (*label).to_string(),
            })
            .collect();
        let now = self.session.appearance.page_tone;
        let current = (PageTone::ALL.iter())
            .find(|(t, _, _)| *t == now)
            .map(|(_, id, _)| (*id).to_string())
            .unwrap_or_default();
        (choices, current)
    }

    /// Use the Page tone `id` (one of [`Self::page_tones`]). Kept for this run only.
    pub fn set_page_tone(&mut self, id: &str) {
        if let Some((tone, _, _)) = PageTone::ALL.iter().find(|(_, i, _)| *i == id)
            && *tone != self.session.appearance.page_tone
        {
            self.session.appearance.page_tone = *tone;
            self.new_epoch();
        }
    }

    /// Follow the system's text size: every reader's Text size becomes the one for UIKit's
    /// content size category `category` ([`text_size_for_content_size`]). Returns it.
    pub fn set_content_size(&mut self, category: &str) -> f32 {
        let size = text_size_for_content_size(category);
        let mut changed = false;
        for view in &mut self.readers {
            if view.settings().size != size {
                let settings = obs_layout::TextSettings {
                    size,
                    ..*view.settings()
                };
                view.set_settings(settings);
                changed = true;
            }
        }
        if changed {
            self.new_epoch();
            for state in &mut self.states {
                state.moved = true;
            }
        }
        size
    }

    /// The Bibles in the library, in Preferred order, by abbreviation.
    pub fn bibles(&self) -> Vec<Choice> {
        let library = &self.session.library;
        (library.preferred_order().iter())
            .filter_map(|id| library.get(id))
            .filter(|e| e.manifest.resource_type == obs_model::ResourceType::Bible)
            .map(|e| Choice {
                id: e.manifest.resource_id.clone(),
                label: e.manifest.abbreviation.clone(),
            })
            .collect()
    }

    fn unit_starts(view: &ReadingView, state: &mut ReaderState) {
        if state.unit_starts.len() == view.doc.units.len() + 1 {
            return;
        }
        let mut at = 0u64;
        state.unit_starts = Vec::with_capacity(view.doc.units.len() + 1);
        for u in &view.doc.units {
            state.unit_starts.push(at);
            at += u.text.len() as u64 + 1;
        }
        state.unit_starts.push(at);
    }

    /// The text offset of a document position: the shell's text input counts positions this
    /// way (each unit's bytes, then one between units).
    pub fn text_offset(&mut self, reader: ReaderId, pos: DocPos) -> Result<u64, MobileError> {
        let (view, state) = self.reader_parts(reader)?;
        Self::unit_starts(view, state);
        Ok(state.unit_starts.get(pos.unit).copied().unwrap_or(0) + pos.byte as u64)
    }

    /// The document position of a text offset, on a character boundary at or before it.
    fn doc_pos(view: &ReadingView, state: &mut ReaderState, offset: u64) -> DocPos {
        Self::unit_starts(view, state);
        let units = view.doc.units.len();
        if units == 0 {
            return DocPos { unit: 0, byte: 0 };
        }
        let unit = (state.unit_starts.partition_point(|&s| s <= offset))
            .saturating_sub(1)
            .min(units - 1);
        let text = &view.doc.units[unit].text;
        let mut byte = ((offset - state.unit_starts[unit]) as usize).min(text.len());
        while !text.is_char_boundary(byte) {
            byte -= 1;
        }
        DocPos { unit, byte }
    }

    fn offset_of(state: &ReaderState, pos: DocPos) -> u64 {
        state.unit_starts.get(pos.unit).copied().unwrap_or(0) + pos.byte as u64
    }

    /// The last text offset (the end of the document).
    pub fn text_end(&mut self, reader: ReaderId) -> Result<u64, MobileError> {
        let (view, state) = self.reader_parts(reader)?;
        Self::unit_starts(view, state);
        Ok(state
            .unit_starts
            .last()
            .copied()
            .unwrap_or(1)
            .saturating_sub(1))
    }

    /// `offset` moved by `by` characters, within the document.
    pub fn move_offset(
        &mut self,
        reader: ReaderId,
        offset: u64,
        by: i64,
    ) -> Result<u64, MobileError> {
        let end = self.text_end(reader)?;
        let (view, state) = self.reader_parts(reader)?;
        let mut pos = Self::doc_pos(view, state, offset.min(end));
        let mut at = Self::offset_of(state, pos);
        for _ in 0..by.unsigned_abs().min(1 << 20) {
            let text = &view.doc.units[pos.unit].text;
            let next = if by > 0 {
                text[pos.byte..]
                    .chars()
                    .next()
                    .map(|c| pos.byte + c.len_utf8())
            } else {
                text[..pos.byte]
                    .chars()
                    .next_back()
                    .map(|c| pos.byte - c.len_utf8())
            };
            match next {
                Some(b) => pos.byte = b,
                None if by > 0 && pos.unit + 1 < view.doc.units.len() => {
                    pos = DocPos {
                        unit: pos.unit + 1,
                        byte: 0,
                    }
                }
                None if by < 0 && pos.unit > 0 => {
                    let unit = pos.unit - 1;
                    pos = DocPos {
                        unit,
                        byte: view.doc.units[unit].text.len(),
                    }
                }
                None => break,
            }
            at = Self::offset_of(state, pos);
        }
        Ok(at)
    }

    /// The text offset nearest a content point, among the text laid out around the view.
    pub fn closest_offset(&mut self, reader: ReaderId, x: f32, y: f64) -> Result<u64, MobileError> {
        let (view, state) = self.reader_parts(reader)?;
        Self::unit_starts(view, state);
        let Some(hit) = Self::block_under(state, y) else {
            return Ok(0);
        };
        let (index, top, laid) = &state.window[hit];
        let p = laid.caret_at(x - state.left, (y - top) as f32);
        let unit = view.doc.blocks[*index].unit;
        let byte = match laid.to_unit(p) {
            Some(UnitPos::Text(b)) => b,
            Some(UnitPos::Object { anchor, .. }) => anchor,
            None => view.doc.blocks[*index].source.range.start,
        };
        Ok(Self::offset_of(state, DocPos { unit, byte }))
    }

    /// The window block under content y, else the nearest.
    fn block_under(state: &ReaderState, y: f64) -> Option<usize> {
        if state.window.is_empty() {
            return None;
        }
        let i = (state.window.iter())
            .position(|(_, top, laid)| y < top + f64::from(laid.height))
            .unwrap_or(state.window.len() - 1);
        Some(i)
    }

    /// The span of text of kind `kind` around `offset`: a word is the word (or space) there;
    /// a sentence, line or paragraph is the Verse in a Bible (the block elsewhere).
    pub fn enclosing_span(
        &mut self,
        reader: ReaderId,
        offset: u64,
        kind: TextUnitKind,
    ) -> Result<Option<TextSpan>, MobileError> {
        let (view, state) = self.reader_parts(reader)?;
        let pos = Self::doc_pos(view, state, offset);
        let doc = &view.doc;
        let Some(unit) = doc.units.get(pos.unit) else {
            return Ok(None);
        };
        let range = match kind {
            TextUnitKind::Character => {
                let len = unit.text[pos.byte..]
                    .chars()
                    .next()
                    .map_or(0, char::len_utf8);
                pos.byte..pos.byte + len
            }
            TextUnitKind::Word => {
                let w = obs_reading::word_at(doc, pos);
                if unit.text[w.clone()].trim().is_empty() {
                    return Ok(None);
                }
                w
            }
            TextUnitKind::Sentence | TextUnitKind::Line | TextUnitKind::Paragraph => {
                match doc.verse_range_at(pos.unit, pos.byte) {
                    Some(r) if r.contains(&pos.byte) => r,
                    _ => match doc.block_at(pos.unit, pos.byte) {
                        Some(b) => doc.blocks[b].source.range.clone(),
                        None => 0..unit.text.len(),
                    },
                }
            }
            TextUnitKind::Document => {
                let end = state
                    .unit_starts
                    .last()
                    .copied()
                    .unwrap_or(1)
                    .saturating_sub(1);
                return Ok(Some(TextSpan { start: 0, end }));
            }
        };
        let start = Self::offset_of(
            state,
            DocPos {
                unit: pos.unit,
                byte: range.start,
            },
        );
        Ok(Some(TextSpan {
            start,
            end: start + (range.end - range.start) as u64,
        }))
    }

    fn selection(view: &ReadingView, state: &mut ReaderState, span: TextSpan) -> Selection {
        let a = Self::doc_pos(view, state, span.start.min(span.end));
        let b = Self::doc_pos(view, state, span.start.max(span.end));
        Selection {
            anchor: a,
            head: b,
            granularity: obs_reading::Granularity::Grapheme,
        }
    }

    /// The text of a span, as written (units joined by a newline).
    pub fn text_in(&mut self, reader: ReaderId, span: TextSpan) -> Result<String, MobileError> {
        let (view, state) = self.reader_parts(reader)?;
        let sel = Self::selection(view, state, span);
        Ok(obs_reading::selected_text(&view.doc, &sel))
    }

    /// A span as Copy puts it on the pasteboard: the text, then its reference.
    pub fn copy_text(&mut self, reader: ReaderId, span: TextSpan) -> Result<String, MobileError> {
        let (view, state) = self.reader_parts(reader)?;
        let sel = Self::selection(view, state, span);
        Ok(view.copy_text(&sel))
    }

    /// The rectangles covering a span's text among the text laid out around the view.
    pub fn span_rects(
        &mut self,
        reader: ReaderId,
        span: TextSpan,
    ) -> Result<Vec<ContentRect>, MobileError> {
        let (view, state) = self.reader_parts(reader)?;
        let sel = Self::selection(view, state, span);
        let range = (sel.anchor, sel.head);
        let mut out = Vec::new();
        for (index, top, laid) in &state.window {
            let rects =
                obs_reading::selection::block_range_rects(&view.doc, *index, laid, &[range]);
            for [x0, y0, x1, y1] in rects.into_iter().flatten() {
                out.push(ContentRect {
                    x: state.left + x0,
                    y: top + f64::from(y0),
                    width: x1 - x0,
                    height: y1 - y0,
                });
            }
        }
        Ok(out)
    }

    /// The caret's rectangle at `offset`: a thin box at the leading edge of the character
    /// there (the trailing edge of the one before, at the end of a unit).
    pub fn caret_rect(
        &mut self,
        reader: ReaderId,
        offset: u64,
    ) -> Result<ContentRect, MobileError> {
        let next = self.move_offset(reader, offset, 1)?;
        let rtl = (self.readers.get(reader.0 as usize))
            .is_some_and(|v| v.doc.direction() == obs_model::Direction::Rtl);
        let rects = self.span_rects(
            reader,
            TextSpan {
                start: offset,
                end: next,
            },
        )?;
        let caret = |r: &ContentRect, leading: bool| ContentRect {
            x: if leading != rtl { r.x } else { r.x + r.width },
            width: 2.0,
            ..*r
        };
        if let Some(r) = rects.first() {
            return Ok(caret(r, true));
        }
        let before = self.move_offset(reader, offset, -1)?;
        let rects = self.span_rects(
            reader,
            TextSpan {
                start: before,
                end: offset,
            },
        )?;
        Ok(rects.last().map_or(
            ContentRect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
            |r| caret(r, false),
        ))
    }

    /// The accessibility elements of the text laid out around the view, in reading order:
    /// one per Verse in a Bible (a Verse split across paragraphs is one element, framed
    /// around all of it), one per block without Verses (a heading).
    pub fn accessibility_items(
        &mut self,
        reader: ReaderId,
    ) -> Result<Vec<AccessibilityItem>, MobileError> {
        let (view, state) = self.reader_parts(reader)?;
        let doc = &view.doc;
        let mut out: Vec<AccessibilityItem> = Vec::new();
        let union = |a: ContentRect, b: ContentRect| {
            let (x0, y0) = (a.x.min(b.x), a.y.min(b.y));
            let x1 = (a.x + a.width).max(b.x + b.width);
            let y1 = (a.y + f64::from(a.height)).max(b.y + f64::from(b.height));
            ContentRect {
                x: x0,
                y: y0,
                width: x1 - x0,
                height: (y1 - y0) as f32,
            }
        };
        for (index, top, laid) in &state.window {
            let block = &doc.blocks[*index];
            let unit = block.unit;
            let range = block.source.range.clone();
            let n = doc.verses.len();
            let first = partition(n, |i| doc.verse_unit(i) < Some(unit));
            let mut verses = Vec::new();
            for i in first..n {
                if doc.verse_unit(i) != Some(unit) {
                    break;
                }
                let v = &doc.verses[i];
                let (s, e) = (v.start as usize, v.end as usize);
                if s < range.end && e > range.start {
                    verses.push((v.verse, s.max(range.start), e.min(range.end)));
                }
            }
            let frame_of = |a: usize, b: usize| {
                let rects = obs_reading::selection::block_range_rects(
                    doc,
                    *index,
                    laid,
                    &[(DocPos { unit, byte: a }, DocPos { unit, byte: b })],
                );
                (rects.into_iter().flatten())
                    .map(|[x0, y0, x1, y1]| ContentRect {
                        x: state.left + x0,
                        y: top + f64::from(y0),
                        width: x1 - x0,
                        height: y1 - y0,
                    })
                    .reduce(union)
            };
            if verses.is_empty() {
                let text = doc.units[unit].text.get(range.clone()).unwrap_or_default();
                let label = clean(text);
                if let Some(frame) = frame_of(range.start, range.end).filter(|_| !label.is_empty())
                {
                    out.push(AccessibilityItem {
                        id: format!("block.{index}"),
                        label,
                        frame,
                        heading: true,
                    });
                }
                continue;
            }
            for (verse, a, b) in verses {
                let Some(frame) = frame_of(a, b) else {
                    continue;
                };
                let id = verse.osis();
                if let Some(last) = out.last_mut()
                    && last.id == id
                {
                    last.frame = union(last.frame, frame);
                    continue;
                }
                let reference =
                    format_passage(Passage::verse(verse), &doc.versification, RefStyle::Long);
                let text = clean(doc.verse_text(verse).unwrap_or_default());
                out.push(AccessibilityItem {
                    id,
                    label: format!("{reference}. {text}"),
                    frame,
                    heading: false,
                });
            }
        }
        Ok(out)
    }

    /// What a tap at a content point opens: the Study sheet stub for the Verse there, with
    /// the word when it is a Tagged word. `None` off the text.
    pub fn tap(
        &mut self,
        reader: ReaderId,
        x: f32,
        y: f64,
    ) -> Result<Option<StudySheetStub>, MobileError> {
        let (view, state) = self.reader_parts(reader)?;
        let Some(i) = Self::block_under(state, y) else {
            return Ok(None);
        };
        let (index, top, laid) = &state.window[i];
        let Some(hit) = laid.hit(x - state.left, (y - top) as f32) else {
            return Ok(None);
        };
        let doc = Arc::clone(&view.doc);
        let unit = doc.blocks[*index].unit;
        let byte = match hit.source {
            UnitPos::Text(b) => b,
            UnitPos::Object { anchor, .. } => anchor,
        };
        let Some(verse) = doc.verse_containing(unit, byte) else {
            return Ok(None);
        };
        let abbreviation = &doc.manifest.abbreviation;
        let reference = format_passage(Passage::verse(verse), &doc.versification, RefStyle::Long);
        let mut stub = StudySheetStub {
            title: format!("{reference} ({abbreviation})"),
            verse_text: clean(doc.verse_text(verse).unwrap_or_default()),
            word: None,
            strongs: None,
            lemma: None,
            gloss: None,
            cross_reference_count: 0,
            cross_reference_label: String::new(),
        };
        if let Some((range, parts)) = doc.tagged_word_at(unit, byte) {
            let written = doc.units[unit].text.get(range).unwrap_or_default().trim();
            let rtl = doc.direction() == obs_model::Direction::Rtl;
            let library = &self.session.library;
            if let Some(shown) = obs_engine::word_info::ShownWord::new(
                written,
                rtl,
                stub.title.clone(),
                parts,
                |n| obs_engine::word_info::lexicon_gloss(library, n),
            ) {
                stub.word = Some(shown.written.clone());
                stub.strongs = Some(shown.strongs().to_string());
                stub.lemma = shown.main.lemma.clone();
                stub.gloss = shown.gloss.clone();
            }
        }
        let standard = match &doc.verse_map {
            Some(map) => map.to_standard(verse),
            None => Some(Passage::verse(verse)),
        };
        let count = standard
            .and_then(|p| {
                obs_engine::cross_references::library_references(&self.session.library, p).ok()
            })
            .map_or(0, |groups| groups.iter().map(|g| g.rows.len() as u32).sum());
        stub.cross_reference_count = count;
        stub.cross_reference_label = plural(count, "cross-reference", "cross-references");
        Ok(Some(stub))
    }
}

/// The first `i` in `0..n` where `pred` is false (`pred` true then false).
fn partition(n: usize, pred: impl Fn(usize) -> bool) -> usize {
    let (mut lo, mut hi) = (0, n);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if pred(mid) {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

/// Text as VoiceOver reads it: whitespace runs as one space, no control characters.
fn clean(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for word in text.split(|c: char| c.is_whitespace() || c.is_control()) {
        if word.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use obs_engine::testing::bibles::{bsb_john_1, tahot};
    use obs_engine::testing::write_starter_bundle;

    use super::*;
    use crate::Paths;

    fn app(name: &str) -> MobileApp {
        let root =
            std::env::temp_dir().join(format!("obs-mobile-tv-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let at = |p: &str| root.join(p);
        let bundle = at("bundle");
        let mut set = vec![bsb_john_1(), tahot()];
        for r in &mut set {
            r.manifest.origin = obs_model::Origin::Starter;
        }
        write_starter_bundle(&bundle, &set);
        let paths = Paths {
            library: at("library"),
            user_db: at("db/user.db"),
            search_cache: at("search"),
            alias_cache: at("aliases"),
            logs: at("logs"),
            downloads: at("downloads"),
            study_set: None,
        };
        MobileApp::boot(paths, Some(Path::new(&bundle))).unwrap()
    }

    fn bsb(app: &mut MobileApp) -> ReaderId {
        app.open_reader("bsb", Some("John 1:1")).unwrap()
    }

    const PHONE: (f32, f32, f32) = (390.0, 700.0, 3.0);

    fn frame(app: &mut MobileApp, r: ReaderId, offset: f64) -> ReaderFrame {
        let (w, h, s) = PHONE;
        app.reader_frame(r, offset, w, h, s, false).unwrap()
    }

    #[test]
    fn a_frame_lists_tiles_over_the_view_and_a_viewport_either_side() {
        let mut app = app("frame");
        let r = bsb(&mut app);
        let f = frame(&mut app, r, 0.0);
        assert_eq!(f.location, "John 1:1");
        assert_eq!(f.page, TonePalette::PAPER.page.0);
        assert!(f.total > f.position + 700.0, "{f:?}");
        let first = f.tiles.first().unwrap();
        let last = f.tiles.last().unwrap();
        assert!(first.y <= f.position, "tiles start at or above the view");
        assert!(last.y + f64::from(last.height) >= f.position + 1400.0);
        assert!(f.tiles.iter().all(|t| t.height > 0.0 && t.height <= BAND));
        // At the very top, over the book's first heading, the location is its first Verse.
        let top = app.open_reader("bsb", None).unwrap();
        assert_eq!(frame(&mut app, top, 0.0).location, "John 1:1");
        for pair in f.tiles.windows(2) {
            assert!(pair[0].y <= pair[1].y, "top first");
        }
        // Each tile's pixels: the band's height at the scale, the page's width.
        let t = app.reader_tile(r, first.key, 3.0).unwrap().unwrap();
        assert_eq!(t.tile.width, 1170);
        assert_eq!(t.tile.height, (first.height * 3.0).round() as u32);
    }

    #[test]
    fn scrolling_moves_by_the_offset_and_a_tile_costs_one_block_wherever_it_is() {
        let mut app = app("scroll");
        let r = bsb(&mut app);
        let f = frame(&mut app, r, 0.0);
        let g = frame(&mut app, r, f.position + 500.0);
        assert!(
            (g.position - (f.position + 500.0)).abs() < 1.0,
            "{} {}",
            f.position,
            g.position
        );
        assert_ne!(g.location, f.location);
        // Far down the page: one tile lays out at most its own block.
        let far = app.go_to(r, "John 1:23").unwrap();
        let h = frame(&mut app, r, 0.0);
        // The shell's offset is ignored after a jump; laying out the blocks above it can move
        // the position, never the text in view.
        assert_eq!(h.location, "John 1:23");
        assert!((h.position - far).abs() < 50.0, "{far} {}", h.position);
        let key = h.tiles.last().unwrap().key;
        let before = app.readers[0]
            .layouts
            .max(app.readers[r.0 as usize].layouts);
        app.reader_tile(r, key, 3.0).unwrap().unwrap();
        let after = app.readers[r.0 as usize].layouts;
        assert!(after - before <= 1, "{before} → {after}");
    }

    #[test]
    fn a_new_page_tone_text_size_or_width_starts_a_new_epoch() {
        let mut app = app("epoch");
        let r = bsb(&mut app);
        let f = frame(&mut app, r, 0.0);
        let key = f.tiles[0].key;
        let (tones, now) = app.page_tones();
        assert_eq!(now, "auto");
        assert_eq!(tones.len(), 4);
        app.set_page_tone("sepia");
        assert_eq!(app.reader_tile(r, key, 3.0).unwrap(), None, "stale");
        let g = frame(&mut app, r, f.position);
        assert_eq!(g.page, TonePalette::SEPIA.page.0);
        assert_ne!(g.tiles[0].key.epoch, key.epoch);
        assert_eq!(
            app.set_content_size("UICTContentSizeCategoryAccessibilityXXXL"),
            40.0
        );
        let h = frame(&mut app, r, 12345.0);
        assert_eq!(
            h.location, g.location,
            "a new Text size keeps the Current location"
        );
        assert_ne!(h.tiles[0].key.epoch, g.tiles[0].key.epoch);
        app.set_page_tone("auto");
        let (w, height, s) = PHONE;
        let i = app
            .reader_frame(r, h.position, w + 100.0, height, s, true)
            .unwrap();
        assert_eq!(
            i.page,
            TonePalette::DARK.page.0,
            "Auto follows a dark system"
        );
        assert_ne!(i.tiles[0].key.epoch, h.tiles[0].key.epoch);
    }

    #[test]
    fn typed_references_and_the_chapter_grid_go_there() {
        let mut app = app("goto");
        let r = bsb(&mut app);
        frame(&mut app, r, 0.0);
        let at = app.go_to(r, "Jn 1:14").unwrap();
        let f = frame(&mut app, r, 0.0);
        assert_eq!((f.position, f.location.as_str()), (at, "John 1:14"));
        assert!(matches!(
            app.go_to(r, "Hezekiah 4"),
            Err(MobileError::Reference { .. })
        ));
        assert!(matches!(
            app.go_to(r, "Gen 1"),
            Err(MobileError::Reference { .. })
        ));
        let books = app.books(r);
        let john = books.iter().position(|b| b.name == "John").unwrap();
        // The fixture's own numbering has John 1 only.
        assert_eq!(books[john].chapters, 1);
        app.go_to_chapter(r, john as u32, 1).unwrap();
        assert_eq!(frame(&mut app, r, 0.0).location, "John 1:1");
        assert!(app.go_to_chapter(r, john as u32, 2).is_err());
    }

    #[test]
    fn words_verses_rects_and_copy_come_from_the_text_under_a_point() {
        let mut app = app("select");
        let r = bsb(&mut app);
        frame(&mut app, r, 0.0);
        // A point well inside the first line of text in view.
        let items = app.accessibility_items(r).unwrap();
        let v = items.iter().find(|i| i.id == "John.1.1").unwrap();
        let (x, y) = (v.frame.x + 40.0, v.frame.y + 5.0);
        let at = app.closest_offset(r, x, y).unwrap();
        let word = app
            .enclosing_span(r, at, TextUnitKind::Word)
            .unwrap()
            .unwrap();
        let text = app.text_in(r, word).unwrap();
        assert!(!text.trim().is_empty() && !text.contains(' '), "{text:?}");
        let verse = app
            .enclosing_span(r, at, TextUnitKind::Sentence)
            .unwrap()
            .unwrap();
        let verse_text = app.text_in(r, verse).unwrap();
        assert!(
            verse_text.starts_with("In the beginning was the Word"),
            "{verse_text}"
        );
        let rects = app.span_rects(r, verse).unwrap();
        assert!(!rects.is_empty());
        assert!(rects.iter().all(|r| r.width > 0.0 && r.height > 0.0));
        let copied = app.copy_text(r, verse).unwrap();
        assert!(copied.ends_with("John 1:1 (BSB)"), "{copied}");
        let caret = app.caret_rect(r, word.start).unwrap();
        assert!(caret.height > 0.0);
        // Moving by characters stays in the document.
        let end = app.text_end(r).unwrap();
        assert_eq!(app.move_offset(r, end, 5).unwrap(), end);
        assert_eq!(app.move_offset(r, 0, -5).unwrap(), 0);
        assert_eq!(app.move_offset(r, word.start, 1).unwrap(), word.start + 1);
    }

    #[test]
    fn one_accessibility_element_per_verse_in_reading_order() {
        let mut app = app("a11y");
        let r = bsb(&mut app);
        frame(&mut app, r, 0.0);
        let items = app.accessibility_items(r).unwrap();
        let verses: Vec<&AccessibilityItem> = items.iter().filter(|i| !i.heading).collect();
        assert!(verses.len() > 3, "{items:?}");
        let mut ids: Vec<&str> = verses.iter().map(|i| i.id.as_str()).collect();
        let n = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), n, "one element per Verse");
        let one = verses.iter().find(|i| i.id == "John.1.1").unwrap();
        assert!(
            one.label
                .starts_with("John 1:1. In the beginning was the Word"),
            "{}",
            one.label
        );
        for pair in verses.windows(2) {
            let (a, b) = (
                Verse::from_osis(&pair[0].id).unwrap(),
                Verse::from_osis(&pair[1].id).unwrap(),
            );
            assert!(a < b, "reading order: {} then {}", pair[0].id, pair[1].id);
        }
    }

    #[test]
    fn a_tap_opens_the_verse_and_a_tagged_word() {
        let mut app = app("tap");
        let r = bsb(&mut app);
        frame(&mut app, r, 0.0);
        let items = app.accessibility_items(r).unwrap();
        let v = items.iter().find(|i| i.id == "John.1.1").unwrap();
        let stub = app
            .tap(r, v.frame.x + 40.0, v.frame.y + 5.0)
            .unwrap()
            .unwrap();
        assert_eq!(stub.title, "John 1:1 (BSB)");
        assert_eq!(stub.word, None, "the BSB has no Tagged words");
        assert_eq!(stub.cross_reference_label, "0 cross-references");
        assert_eq!(app.tap(r, 1.0, -5000.0).unwrap(), None);
        // The Hebrew Bible's words carry Strong's numbers, and it reads right to left.
        let h = app.open_reader("tahot", None).unwrap();
        frame(&mut app, h, 0.0);
        let items = app.accessibility_items(h).unwrap();
        let v = items.iter().find(|i| !i.heading).unwrap();
        let y = v.frame.y + 8.0;
        let stub = (0..40)
            .filter_map(|k| {
                app.tap(h, v.frame.x + v.frame.width - 4.0 * k as f32, y)
                    .unwrap()
            })
            .find(|s| s.strongs.is_some())
            .expect("a Tagged word on the first line");
        assert!(stub.title.ends_with("(TAHOT)"), "{}", stub.title);
        assert!(
            stub.strongs.as_deref().is_some_and(|s| s.starts_with('H')),
            "{stub:?}"
        );
        assert!(stub.word.is_some());
    }

    #[test]
    fn the_content_size_table_stays_in_the_text_size_range() {
        assert_eq!(text_size_for_content_size("UICTContentSizeCategoryL"), 20.0);
        assert_eq!(text_size_for_content_size("unknown"), 20.0);
        let all = [
            "UICTContentSizeCategoryXS",
            "UICTContentSizeCategoryS",
            "UICTContentSizeCategoryM",
            "UICTContentSizeCategoryL",
            "UICTContentSizeCategoryXL",
            "UICTContentSizeCategoryXXL",
            "UICTContentSizeCategoryXXXL",
            "UICTContentSizeCategoryAccessibilityM",
            "UICTContentSizeCategoryAccessibilityL",
            "UICTContentSizeCategoryAccessibilityXL",
            "UICTContentSizeCategoryAccessibilityXXL",
            "UICTContentSizeCategoryAccessibilityXXXL",
        ];
        let sizes: Vec<f32> = all.iter().map(|c| text_size_for_content_size(c)).collect();
        assert!(sizes.windows(2).all(|p| p[0] < p[1]), "{sizes:?}");
        assert!(
            sizes
                .iter()
                .all(|s| (TEXT_SIZE_MIN..=TEXT_SIZE_MAX).contains(s))
        );
    }
}
