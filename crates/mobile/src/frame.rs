//! What a reader's hosted text view shows (plan A4, option B; `docs/dev/mobile-text-view.md`).
//! The shell scrolls a native scroll view whose content coordinates are the reader's page in
//! points (x from the page's left edge, y from the document's top) and asks for a
//! [`ReaderFrame`] at its content offset: the tiles to keep and where they go, the page colour
//! and the Current location. It draws each tile's pixels ([`MobileApp::tile`]) and asks what
//! is under a tap ([`MobileApp::hit`]).
//!
//! **Scrolling.** The reader's [`ReadingView`] stays anchored to a block (#14 rule 7). The
//! shell reports its content offset; the reader scrolls by the difference from the position
//! it last reported, lays out what is in view and a viewport either side of it, and reports
//! its position again. When blocks above the view were laid out at heights other than their
//! estimates, that position differs from the offset the shell sent, and the shell moves its
//! offset there: the text in view never jumps. After a jump (or a new Text size, or another
//! Resource) the next frame takes no scroll from the shell's offset, and the shell takes the
//! reader's position.
//!
//! **Tiles.** A tile is a band of one block, at most [`BAND`] points tall, keyed by
//! [`TileKey`] (the epoch, the block and the band). Its cost is that block's layout and the
//! band's pixels wherever the block is in the document. The shell keeps the images of the keys
//! the last frame listed and drops the rest; a new width, scale, Page tone, Text size or
//! system appearance starts a new epoch, so every key changes.

use std::sync::Arc;

use obs_layout::{LaidBlock, UnitPos};
use obs_reading::TonePalette;
use obs_scripture::{Passage, RefStyle, format_passage};

use crate::readers::location_label;
use crate::{MobileApp, MobileError, ReaderId, Tile};

/// The tallest tile, in points: a block taller than this is drawn in several bands.
pub const BAND: f32 = 256.0;

/// How much of the page is kept laid out and drawn above and below the view, in viewport
/// heights, so a fling shows tiles drawn ahead.
pub const AROUND: f32 = 1.0;

/// The most device pixels a tile may have on a side. A tile is `width × height × 4` bytes
/// allocated at once, and a failed allocation aborts the app, so a shell's bad size is refused
/// instead. A tile is the page's width by at most a [`BAND`]: the widest page (a 13-inch iPad
/// in landscape, 1376 pt at 2×) is 2752 px, so 4096 leaves room and bounds a tile at 64 MiB.
pub const MAX_TILE_PX: usize = 4096;

/// The reader's viewport: its size in points and the device pixels per point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub width: f32,
    pub height: f32,
    pub scale: f32,
}

/// What the system's appearance is now, as the shell reads it: the Follow system Theme and
/// the Auto Page tone follow it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SystemAppearance {
    pub dark: bool,
}

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
    /// The Current location, as the bar shows it ("Genesis 1:3", or an entry's Key).
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

/// What is under a point of a reader's page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// The Verse there (in a Bible).
    pub verse: Option<HitVerse>,
    /// The word there, when it is a Tagged word.
    pub word: Option<HitWord>,
    /// The Footnote marker there.
    pub footnote: Option<FootnoteMarkerHit>,
}

/// A Verse under a point, as the Study sheet names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HitVerse {
    /// Its OSIS id, `John.3.16`.
    pub id: String,
    /// `John 3:16 (BSB)`.
    pub title: String,
    pub text: String,
    /// Its Cross-references across the library's Cross-reference sets.
    pub cross_reference_count: u32,
    /// `12 cross-references`.
    pub cross_reference_label: String,
}

/// A Tagged word under a point, as Word Info shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HitWord {
    pub written: String,
    pub strongs: String,
    pub lemma: Option<String>,
    pub gloss: Option<String>,
}

/// A Footnote marker under a point: [`MobileApp::footnote`] gives its Footnote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FootnoteMarkerHit {
    /// The note's id in its Resource.
    pub note: String,
    /// The marker as drawn: `a`, `aa`, or `↗` for a cross-reference note.
    pub label: String,
    pub cross_reference: bool,
}

/// A Footnote's text, plain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Footnote {
    pub note: String,
    pub text: String,
}

/// What the text view keeps for one reader besides its [`ReadingView`].
#[derive(Debug, Default)]
pub(crate) struct FrameState {
    /// The position last reported to the shell.
    pub(crate) last: f64,
    /// The reader moved by itself (a jump, a new Text size, another Resource) since the last
    /// frame: the next frame takes no scroll from the shell's offset.
    pub(crate) moved: bool,
    /// The page width and scale of the last frame.
    pub(crate) page: (f32, f32),
    /// The text column's left edge on the page.
    pub(crate) left: f32,
    /// The blocks laid out around the view: index, top in content points, layout.
    pub(crate) window: Vec<(usize, f64, Arc<LaidBlock>)>,
    /// Each unit's first text offset (units are joined by one offset, as a newline).
    pub(crate) unit_starts: Vec<u64>,
}

impl FrameState {
    /// A new reader's: its first frame takes the reader's position.
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

pub(crate) fn plural(n: u32, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// A viewport, or a tile scale, the shell may ask for: finite sizes above zero.
fn positive(x: f32) -> bool {
    x.is_finite() && x > 0.0
}

impl MobileApp {
    /// The Page tone's palette now, with the system appearance of the last frame.
    pub(crate) fn palette(&self) -> TonePalette {
        let appearance = self.session.appearance;
        let dark = crate::settings::chrome_is_dark(&self.session, self.dark);
        TonePalette::for_theme(
            appearance.page_tone.resolve(dark),
            appearance.theme.is_high_contrast(),
        )
    }

    /// Start a new epoch: every tile is drawn again.
    pub(crate) fn new_epoch(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
    }

    /// The frame of reader `reader` for the shell's content offset `offset` (clamped to the
    /// page by the shell) in `viewport`, with the system appearance `appearance`.
    pub fn frame(
        &mut self,
        reader: ReaderId,
        offset: f64,
        viewport: Viewport,
        appearance: SystemAppearance,
    ) -> Result<ReaderFrame, MobileError> {
        let Viewport {
            width,
            height,
            scale,
        } = viewport;
        if !(positive(width) && positive(height) && positive(scale) && offset.is_finite()) {
            return Err(MobileError::TileSize);
        }
        let started = std::time::Instant::now();
        let i = reader.0 as usize;
        if i >= self.readers.len() {
            return Err(MobileError::NoReader(reader));
        }
        self.refresh_note_room(i);
        let mut epoch = false;
        if appearance.dark != self.dark {
            self.dark = appearance.dark;
            epoch = true;
        }
        let page = self.palette().page.0;
        let r = &mut self.readers[i];
        let (view, state) = (&mut r.view, &mut r.frame);
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
        let (position, total) = view.position();
        state.last = position;
        state.window = (blocks.iter())
            .map(|b| (b.index, position + f64::from(b.top), Arc::clone(&b.laid)))
            .collect();
        // Tell the Workspace where the reader is: its Current location (no Visit) and, outside
        // Bibles, the unit at the top, which Back/Forward entries snapshot (#176).
        let current = view.current_location();
        let entry = view.current_entry();
        let top_unit = (view.doc.manifest.resource_type != obs_model::ResourceType::Bible)
            .then(|| view.top_unit())
            .flatten()
            .and_then(|u| view.doc.units.get(u))
            .map(|u| u.id.clone());
        let keyed = (self.session.workspace.tab(r.tab)).is_some_and(obs_engine::Tab::keeps_entries);
        let ws = &mut self.session.workspace;
        if keyed {
            if let Some(key) = entry.clone() {
                ws.user_scrolled_entry(r.tab, key);
            }
        } else if let Some(v) = current {
            ws.scrolled(r.tab, v);
        }
        ws.view_shows(r.tab, top_unit);
        // Over a heading (the top of a book) the bar names the first Verse below.
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
        let location = if keyed {
            location_label(view, None, entry.as_ref())
        } else {
            location_label(view, current.or_else(below), None)
        };
        if epoch {
            self.new_epoch();
        }
        let epoch = self.epoch;
        let state = &self.readers[i].frame;
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
    /// epoch is over (the shell drops it). Refused when `scale` is not above zero or the tile
    /// is over [`MAX_TILE_PX`] a side.
    pub fn tile(
        &mut self,
        reader: ReaderId,
        key: TileKey,
        scale: f32,
    ) -> Result<Option<TimedTile>, MobileError> {
        if !positive(scale) {
            return Err(MobileError::TileSize);
        }
        if key.epoch != self.epoch {
            return Ok(None);
        }
        let started = std::time::Instant::now();
        let palette = self.palette();
        let r = (self.readers.get_mut(reader.0 as usize)).ok_or(MobileError::NoReader(reader))?;
        let width = r.frame.page.0;
        let block = key.block as usize;
        let Some(height) = r.view.laid(block).map(|l| l.height) else {
            return Ok(None);
        };
        let from = key.band as f32 * BAND;
        let band = (height - from).min(BAND);
        if band.is_nan() || band <= 0.0 {
            return Ok(None);
        }
        let (w, h) = obs_render::tile_size([0.0, from, width, from + band], scale);
        if w > MAX_TILE_PX || h > MAX_TILE_PX {
            return Err(MobileError::TileSize);
        }
        let tile = obs_reading::compose_block_tile(
            &mut r.view,
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

    /// What is under content point (`x`, `y`) of reader `reader`'s page, among the text laid
    /// out around the view: the Verse with its text and Cross-reference count, the word when
    /// it is a Tagged word (as Word Info shows it), a Footnote marker. `None` off the text.
    pub fn hit(&mut self, reader: ReaderId, x: f32, y: f64) -> Result<Option<Hit>, MobileError> {
        let r = self.reader_at(reader)?;
        let state = &r.frame;
        let Some(i) = crate::text_input::block_under(state, y) else {
            return Ok(None);
        };
        let (index, top, laid) = &state.window[i];
        let Some(hit) = laid.hit(x - state.left, (y - top) as f32) else {
            return Ok(None);
        };
        let doc = Arc::clone(&r.view.doc);
        let unit = doc.blocks[*index].unit;
        let mut out = Hit {
            verse: None,
            word: None,
            footnote: None,
        };
        let byte = match &hit.source {
            UnitPos::Text(b) => *b,
            UnitPos::Object { anchor, object } => {
                if let obs_layout::InlineObject::NoteMarker { note, kind } = object {
                    out.footnote = Some(FootnoteMarkerHit {
                        note: note.to_string(),
                        label: marker_label(laid, *anchor, note),
                        cross_reference: *kind == obs_model::NoteKind::CrossReference,
                    });
                }
                *anchor
            }
        };
        if let Some(verse) = doc.verse_containing(unit, byte) {
            let abbreviation = &doc.manifest.abbreviation;
            let reference =
                format_passage(Passage::verse(verse), &doc.versification, RefStyle::Long);
            let title = format!("{reference} ({abbreviation})");
            if let Some((range, parts)) = doc.tagged_word_at(unit, byte) {
                let written = doc.units[unit].text.get(range).unwrap_or_default().trim();
                let rtl = doc.direction() == obs_model::Direction::Rtl;
                let library = &self.session.library;
                if let Some(shown) =
                    obs_engine::word_info::ShownWord::new(written, rtl, title.clone(), parts, |n| {
                        obs_engine::word_info::lexicon_gloss(library, n)
                    })
                {
                    out.word = Some(HitWord {
                        written: shown.written.clone(),
                        strongs: shown.strongs().to_string(),
                        lemma: shown.main.lemma.clone(),
                        gloss: shown.gloss.clone(),
                    });
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
            out.verse = Some(HitVerse {
                id: verse.osis(),
                title,
                text: crate::text_input::clean(doc.verse_text(verse).unwrap_or_default()),
                cross_reference_count: count,
                cross_reference_label: plural(count, "cross-reference", "cross-references"),
            });
        }
        let empty = out.verse.is_none() && out.word.is_none() && out.footnote.is_none();
        Ok((!empty).then_some(out))
    }

    /// The Footnote `note` (a [`FootnoteMarkerHit::note`]) of reader `reader`'s Resource.
    pub fn footnote(&self, reader: ReaderId, note: &str) -> Result<Footnote, MobileError> {
        let doc = &self.reader_at(reader)?.view.doc;
        let missing = || MobileError::Failed(format!("no Footnote {note}"));
        let id = obs_model::UnitId::new(note).map_err(|_| missing())?;
        let unit = doc.footnote(&id).ok_or_else(missing)?;
        Ok(Footnote {
            note: note.to_string(),
            text: crate::text_input::clean(&unit.text),
        })
    }
}

/// A Footnote marker's label in `laid`: every cluster of the marker for `note` at `anchor`
/// (`aa` is two).
fn marker_label(laid: &LaidBlock, anchor: usize, note: &obs_model::UnitId) -> String {
    let mut label = String::new();
    for line in &laid.lines {
        for item in &line.items {
            for (bytes, _, _) in item.clusters() {
                if let Some(UnitPos::Object {
                    anchor: a,
                    object: obs_layout::InlineObject::NoteMarker { note: n, .. },
                }) = laid.to_unit(bytes.start)
                    && a == anchor
                    && n == *note
                {
                    label.push_str(laid.text.get(bytes).unwrap_or_default());
                }
            }
        }
    }
    label.retain(|c| !c.is_control() && c != '\u{200F}');
    label
}

/// For tests: the reader's view.
#[cfg(test)]
pub(crate) fn view(app: &MobileApp, reader: ReaderId) -> &obs_reading::ReadingView {
    &app.readers[reader.0 as usize].view
}
