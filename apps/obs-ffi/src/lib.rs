//! The FFI surface of the Mobile app (ADR 0021, `docs/dev/mobile-core.md`): UniFFI exports over
//! `obs-mobile`. Rules stay in `obs-mobile`; this crate only converts and forwards.
//!
//! Everything but [`engine_version`] is the S03 spike of `docs/plans/mobile-ios.md` ("Hello
//! engine"): boot, open a reader, lay out a tile. S13 replaces it with the designed surface.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

uniffi::setup_scaffolding!();

/// The engine's version.
#[uniffi::export]
pub fn engine_version() -> String {
    obs_mobile::MobileApp::engine_version().to_string()
}

// S03 spike: replaced in S13.
/// Where the app keeps its files: `obs_mobile::Paths`, as absolute paths.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Paths {
    pub library: String,
    pub user_db: String,
    pub search_cache: String,
    pub alias_cache: String,
    pub logs: String,
    pub downloads: String,
    /// S07 prototype, debug builds: a Starter library folder whose Study set is read in place.
    pub study_set: Option<String>,
}

impl From<Paths> for obs_mobile::Paths {
    fn from(p: Paths) -> Self {
        Self {
            library: PathBuf::from(p.library),
            user_db: PathBuf::from(p.user_db),
            search_cache: PathBuf::from(p.search_cache),
            alias_cache: PathBuf::from(p.alias_cache),
            logs: PathBuf::from(p.logs),
            downloads: PathBuf::from(p.downloads),
            study_set: p.study_set.map(PathBuf::from),
        }
    }
}

// S03 spike: replaced in S13.
/// A tile's pixels: `width × height`, row-major RGBA8, premultiplied alpha.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Tile {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl From<obs_mobile::Tile> for Tile {
    fn from(t: obs_mobile::Tile) -> Self {
        Self {
            width: t.width,
            height: t.height,
            rgba: t.rgba,
        }
    }
}

// S03 spike: replaced in S13.
/// Why a call failed, in words.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
pub enum MobileError {
    Failed { message: String },
}

impl std::fmt::Display for MobileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MobileError::Failed { message } => f.write_str(message),
        }
    }
}

impl std::error::Error for MobileError {}

impl From<obs_mobile::MobileError> for MobileError {
    fn from(e: obs_mobile::MobileError) -> Self {
        MobileError::Failed {
            message: e.to_string(),
        }
    }
}

// S03 spike: replaced in S13.
/// The running Mobile app (`obs_mobile::MobileApp`) behind one lock (plan A3): every call
/// locks, acts and returns.
#[doc(hidden)]
#[derive(uniffi::Object)]
pub struct MobileApp {
    app: Mutex<obs_mobile::MobileApp>,
}

impl MobileApp {
    fn with<T>(
        &self,
        f: impl FnOnce(&mut obs_mobile::MobileApp) -> Result<T, obs_mobile::MobileError>,
    ) -> Result<T, MobileError> {
        let mut app = self.app.lock().map_err(|_| MobileError::Failed {
            message: "the app stopped after an internal error".into(),
        })?;
        f(&mut app).map_err(MobileError::from)
    }
}

#[uniffi::export]
impl MobileApp {
    /// Start the app over `paths`, adding the Starter library from `bundle` (the Bundled set
    /// folder) on the first run, with the first Bible open as reader 0.
    #[uniffi::constructor]
    pub fn boot(paths: Paths, bundle: Option<String>) -> Result<Arc<Self>, MobileError> {
        let app = obs_mobile::MobileApp::boot(paths.into(), bundle.as_deref().map(Path::new))?;
        Ok(Arc::new(Self {
            app: Mutex::new(app),
        }))
    }

    /// What went wrong at boot without stopping it.
    pub fn problems(&self) -> Vec<String> {
        self.with(|a| Ok(a.problems().to_vec())).unwrap_or_default()
    }

    /// The ids of the library's Resources, in Preferred order.
    pub fn resource_ids(&self) -> Vec<String> {
        self.with(|a| Ok(a.resource_ids())).unwrap_or_default()
    }

    /// The Resource reader `reader` shows, if there is such a reader.
    pub fn reader_resource(&self, reader: u32) -> Option<String> {
        (self.with(|a| {
            Ok(a.reader_resource(obs_mobile::ReaderId(reader))
                .map(String::from))
        }))
        .ok()
        .flatten()
    }

    /// A reader of `resource_id` at `reference` (or its start); returns its id. A Resource
    /// already open gives back its reader, where it was unless `reference` is given.
    pub fn open_reader(
        &self,
        resource_id: String,
        reference: Option<String>,
    ) -> Result<u32, MobileError> {
        self.with(|a| a.open_reader(&resource_id, reference.as_deref()))
            .map(|r| r.0)
    }

    /// The tile of `reader`'s page, `width` points wide, `height` points tall from `top`
    /// points below its Current location, at `scale` pixels per point.
    pub fn layout_tile(
        &self,
        reader: u32,
        top: f32,
        width: f32,
        height: f32,
        scale: f32,
    ) -> Result<Tile, MobileError> {
        self.with(|a| a.layout_tile(obs_mobile::ReaderId(reader), top, width, height, scale))
            .map(Tile::from)
    }
}

// S07 prototype (`obs_mobile::text_view`): replaced in S13.
/// A tile of a reader's page: block `block`'s band `band`, drawn in epoch `epoch`.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct TileKey {
    pub epoch: u32,
    pub block: u32,
    pub band: u32,
}

impl From<obs_mobile::TileKey> for TileKey {
    fn from(k: obs_mobile::TileKey) -> Self {
        Self {
            epoch: k.epoch,
            block: k.block,
            band: k.band,
        }
    }
}

impl From<TileKey> for obs_mobile::TileKey {
    fn from(k: TileKey) -> Self {
        Self {
            epoch: k.epoch,
            block: k.block,
            band: k.band,
        }
    }
}

/// Where a tile goes: its top in content points and its height.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TilePlace {
    pub key: TileKey,
    pub y: f64,
    pub height: f32,
}

/// What the text view shows for one content offset (`obs_mobile::ReaderFrame`).
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ReaderFrame {
    pub position: f64,
    pub total: f64,
    pub page_red: u8,
    pub page_green: u8,
    pub page_blue: u8,
    pub tiles: Vec<TilePlace>,
    pub location: String,
    pub micros: u32,
}

impl From<obs_mobile::ReaderFrame> for ReaderFrame {
    fn from(f: obs_mobile::ReaderFrame) -> Self {
        let [page_red, page_green, page_blue] = f.page;
        Self {
            position: f.position,
            total: f.total,
            page_red,
            page_green,
            page_blue,
            tiles: (f.tiles.into_iter())
                .map(|t| TilePlace {
                    key: t.key.into(),
                    y: t.y,
                    height: t.height,
                })
                .collect(),
            location: f.location,
            micros: f.micros,
        }
    }
}

/// A tile's pixels and how long they took, in microseconds.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TimedTile {
    pub tile: Tile,
    pub micros: u32,
}

/// A rectangle in content points.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct ContentRect {
    pub x: f32,
    pub y: f64,
    pub width: f32,
    pub height: f32,
}

impl From<obs_mobile::ContentRect> for ContentRect {
    fn from(r: obs_mobile::ContentRect) -> Self {
        Self {
            x: r.x,
            y: r.y,
            width: r.width,
            height: r.height,
        }
    }
}

/// The text units UIKit's text input asks about; the core decides what each covers.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TextUnitKind {
    Character,
    Word,
    Sentence,
    Paragraph,
    Line,
    Document,
}

impl From<TextUnitKind> for obs_mobile::TextUnitKind {
    fn from(k: TextUnitKind) -> Self {
        match k {
            TextUnitKind::Character => Self::Character,
            TextUnitKind::Word => Self::Word,
            TextUnitKind::Sentence => Self::Sentence,
            TextUnitKind::Paragraph => Self::Paragraph,
            TextUnitKind::Line => Self::Line,
            TextUnitKind::Document => Self::Document,
        }
    }
}

/// A range of text offsets.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct TextSpan {
    pub start: u64,
    pub end: u64,
}

impl From<TextSpan> for obs_mobile::TextSpan {
    fn from(s: TextSpan) -> Self {
        Self {
            start: s.start,
            end: s.end,
        }
    }
}

impl From<obs_mobile::TextSpan> for TextSpan {
    fn from(s: obs_mobile::TextSpan) -> Self {
        Self {
            start: s.start,
            end: s.end,
        }
    }
}

/// One accessibility element: a Verse, or a heading.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct AccessibilityItem {
    pub id: String,
    pub label: String,
    pub frame: ContentRect,
    pub heading: bool,
}

/// What a tap opens: the Study sheet stub.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct StudySheetStub {
    pub title: String,
    pub verse_text: String,
    pub word: Option<String>,
    pub strongs: Option<String>,
    pub lemma: Option<String>,
    pub gloss: Option<String>,
    pub cross_reference_count: u32,
    pub cross_reference_label: String,
}

/// A choice to list (a Page tone, a Bible).
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Choice {
    pub id: String,
    pub label: String,
}

/// The Page tones and the one in use.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Choices {
    pub choices: Vec<Choice>,
    pub current: String,
}

/// A Bible book and its chapter count, for the chapter grid.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BookChapters {
    pub name: String,
    pub chapters: u16,
}

fn choice(c: obs_mobile::Choice) -> Choice {
    Choice {
        id: c.id,
        label: c.label,
    }
}

// S07 prototype: replaced in S13.
#[uniffi::export]
impl MobileApp {
    /// The frame for content offset `offset` in a `width` × `height` viewport at `scale`.
    pub fn reader_frame(
        &self,
        reader: u32,
        offset: f64,
        width: f32,
        height: f32,
        scale: f32,
        dark: bool,
    ) -> Result<ReaderFrame, MobileError> {
        let r = obs_mobile::ReaderId(reader);
        self.with(|a| a.reader_frame(r, offset, width, height, scale, dark))
            .map(ReaderFrame::from)
    }

    /// A tile's pixels, or nothing when its epoch is over.
    pub fn reader_tile(
        &self,
        reader: u32,
        key: TileKey,
        scale: f32,
    ) -> Result<Option<TimedTile>, MobileError> {
        let r = obs_mobile::ReaderId(reader);
        let tile = self.with(|a| a.reader_tile(r, key.into(), scale))?;
        Ok(tile.map(|t| TimedTile {
            tile: t.tile.into(),
            micros: t.micros,
        }))
    }

    /// Go to a typed reference; returns the new position.
    pub fn go_to(&self, reader: u32, text: String) -> Result<f64, MobileError> {
        self.with(|a| a.go_to(obs_mobile::ReaderId(reader), &text))
    }

    /// The reader's Bible books with their chapter counts.
    pub fn books(&self, reader: u32) -> Vec<BookChapters> {
        (self.with(|a| Ok(a.books(obs_mobile::ReaderId(reader)))))
            .unwrap_or_default()
            .into_iter()
            .map(|b| BookChapters {
                name: b.name,
                chapters: b.chapters,
            })
            .collect()
    }

    /// Go to chapter `chapter` of the `book`th book; returns the new position.
    pub fn go_to_chapter(&self, reader: u32, book: u32, chapter: u16) -> Result<f64, MobileError> {
        self.with(|a| a.go_to_chapter(obs_mobile::ReaderId(reader), book, chapter))
    }

    /// The Page tones and the one in use.
    pub fn page_tones(&self) -> Choices {
        let (choices, current) = (self.with(|a| Ok(a.page_tones()))).unwrap_or_default();
        Choices {
            choices: choices.into_iter().map(choice).collect(),
            current,
        }
    }

    pub fn set_page_tone(&self, id: String) {
        let _ = self.with(|a| {
            a.set_page_tone(&id);
            Ok(())
        });
    }

    /// Follow UIKit's content size category; returns the Text size.
    pub fn set_content_size(&self, category: String) -> f32 {
        (self.with(|a| Ok(a.set_content_size(&category)))).unwrap_or_default()
    }

    /// The library's Bibles, in Preferred order.
    pub fn bibles(&self) -> Vec<Choice> {
        (self.with(|a| Ok(a.bibles())))
            .unwrap_or_default()
            .into_iter()
            .map(choice)
            .collect()
    }

    pub fn text_end(&self, reader: u32) -> Result<u64, MobileError> {
        self.with(|a| a.text_end(obs_mobile::ReaderId(reader)))
    }

    pub fn move_offset(&self, reader: u32, offset: u64, by: i64) -> Result<u64, MobileError> {
        self.with(|a| a.move_offset(obs_mobile::ReaderId(reader), offset, by))
    }

    pub fn closest_offset(&self, reader: u32, x: f32, y: f64) -> Result<u64, MobileError> {
        self.with(|a| a.closest_offset(obs_mobile::ReaderId(reader), x, y))
    }

    pub fn enclosing_span(
        &self,
        reader: u32,
        offset: u64,
        kind: TextUnitKind,
    ) -> Result<Option<TextSpan>, MobileError> {
        let r = obs_mobile::ReaderId(reader);
        Ok(self
            .with(|a| a.enclosing_span(r, offset, kind.into()))?
            .map(TextSpan::from))
    }

    pub fn text_in(&self, reader: u32, span: TextSpan) -> Result<String, MobileError> {
        self.with(|a| a.text_in(obs_mobile::ReaderId(reader), span.into()))
    }

    pub fn copy_text(&self, reader: u32, span: TextSpan) -> Result<String, MobileError> {
        self.with(|a| a.copy_text(obs_mobile::ReaderId(reader), span.into()))
    }

    pub fn span_rects(&self, reader: u32, span: TextSpan) -> Result<Vec<ContentRect>, MobileError> {
        let r = obs_mobile::ReaderId(reader);
        let rects = self.with(|a| a.span_rects(r, span.into()))?;
        Ok(rects.into_iter().map(ContentRect::from).collect())
    }

    pub fn caret_rect(&self, reader: u32, offset: u64) -> Result<ContentRect, MobileError> {
        self.with(|a| a.caret_rect(obs_mobile::ReaderId(reader), offset))
            .map(ContentRect::from)
    }

    pub fn accessibility_items(&self, reader: u32) -> Result<Vec<AccessibilityItem>, MobileError> {
        let items = self.with(|a| a.accessibility_items(obs_mobile::ReaderId(reader)))?;
        Ok((items.into_iter())
            .map(|i| AccessibilityItem {
                id: i.id,
                label: i.label,
                frame: i.frame.into(),
                heading: i.heading,
            })
            .collect())
    }

    pub fn tap(&self, reader: u32, x: f32, y: f64) -> Result<Option<StudySheetStub>, MobileError> {
        let stub = self.with(|a| a.tap(obs_mobile::ReaderId(reader), x, y))?;
        Ok(stub.map(|s| StudySheetStub {
            title: s.title,
            verse_text: s.verse_text,
            word: s.word,
            strongs: s.strongs,
            lemma: s.lemma,
            gloss: s.gloss,
            cross_reference_count: s.cross_reference_count,
            cross_reference_label: s.cross_reference_label,
        }))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn engine_version_is_the_engines() {
        assert_eq!(
            super::engine_version(),
            obs_mobile::MobileApp::engine_version()
        );
    }
}
