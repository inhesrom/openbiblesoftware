//! The FFI surface of the Mobile app (ADR 0021, `docs/dev/mobile-core.md`): UniFFI exports over
//! `obs-mobile`. Rules stay in `obs-mobile`; this crate only converts and forwards.
//!
//! One [`MobileApp`] Object per process holds `obs_mobile::MobileApp` behind one lock (plan
//! A3): every call locks, acts and returns a snapshot (a Record), and a lock poisoned by a
//! panic makes every later call fail with [`MobileError::Stopped`] instead of answering
//! nothing. [`MobileApp::set_observer`] starts the pump thread, which calls
//! `obs_mobile::MobileApp::pump` on a timer and hands each [`Changed`] to the shell's
//! [`Observer`]. Readers are `u32`s (0, then 1); Tab and Panel ids never cross.
//!
//! Tile pixels cross as one bulk copy ([`TilePixels`]), the one `unsafe impl` of the crate.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use obs_mobile::ReaderId;

uniffi::setup_scaffolding!();

/// The engine's version.
#[uniffi::export]
pub fn engine_version() -> String {
    obs_mobile::MobileApp::engine_version().to_string()
}

/// The Text size for a UIKit content size category (Dynamic Type), as the core maps it.
#[uniffi::export]
pub fn text_size_for_content_size(category: String) -> f32 {
    obs_mobile::text_size_for_content_size(&category)
}

// ---- Errors -------------------------------------------------------------------------------

/// Why a call failed, in words for the user.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
pub enum MobileError {
    /// A typed reference or Key could not be read, or the Resource does not have it: the
    /// reference field shows the message.
    Reference {
        message: String,
    },
    /// The core stopped after an internal error; nothing more can be done in this run.
    Stopped {
        message: String,
    },
    Failed {
        message: String,
    },
}

impl std::fmt::Display for MobileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MobileError::Reference { message }
            | MobileError::Stopped { message }
            | MobileError::Failed { message } => f.write_str(message),
        }
    }
}

impl std::error::Error for MobileError {}

impl From<obs_mobile::MobileError> for MobileError {
    fn from(e: obs_mobile::MobileError) -> Self {
        let message = e.to_string();
        match e {
            obs_mobile::MobileError::Reference { error, .. } => {
                MobileError::Reference { message: error }
            }
            _ => MobileError::Failed { message },
        }
    }
}

// ---- Tile pixels: the bulk byte path --------------------------------------------------------

/// A tile's pixels, crossing the FFI as Swift `Data` in one copy. UniFFI 0.32 lowers a
/// `Vec<u8>` a byte at a time (`uniffi_core`'s `Lower for Vec<T>` writes each item in a
/// loop: about 1.4 ms for a 3.6 MB 3× tile on the owner's Mac, against 0.15 ms to draw it);
/// this type writes the same bytes with one `extend_from_slice`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TilePixels(pub Vec<u8>);

// SAFETY (the one exception to `unsafe_code`, ADR 0021 and plan S13): `FfiConverter` is an
// `unsafe trait` because its implementor promises that `FfiType`, the lowered value, the
// serialized form and `TYPE_ID_META` agree with what the foreign bindings expect. This impl
// performs no unsafe operation. It declares itself as `Vec<u8>` (the same `TYPE_ID_META`, so
// the bindings see the builtin bytes type and Swift gets `Data`), and its wire format is
// exactly `Vec<u8>`'s: a big-endian `i32` count, then the bytes, in a `RustBuffer`. Lifting
// (Swift to Rust, which no call does) is `Vec<u8>`'s own. If a UniFFI upgrade changes the
// bytes type's wire format, `tests::tile_pixels_cross_as_vec_u8_does` fails.
#[allow(unsafe_code)]
unsafe impl<UT> uniffi::FfiConverter<UT> for TilePixels {
    type FfiType = uniffi::RustBuffer;

    fn lower(obj: Self) -> uniffi::RustBuffer {
        let mut buf = Vec::with_capacity(obj.0.len() + 4);
        <Self as uniffi::FfiConverter<UT>>::write(obj, &mut buf);
        uniffi::RustBuffer::from_vec(buf)
    }

    fn try_lift(v: uniffi::RustBuffer) -> uniffi::Result<Self> {
        <Vec<u8> as uniffi::Lift<UT>>::try_lift(v).map(TilePixels)
    }

    fn write(obj: Self, buf: &mut Vec<u8>) {
        // A tile is at most obs_mobile::MAX_TILE_PX² × 4 bytes (64 MiB), well under the i32
        // count; anything longer crosses as no pixels rather than as a wrong count.
        let Ok(count) = i32::try_from(obj.0.len()) else {
            buf.extend_from_slice(&0i32.to_be_bytes());
            return;
        };
        buf.extend_from_slice(&count.to_be_bytes());
        buf.extend_from_slice(&obj.0);
    }

    fn try_read(buf: &mut &[u8]) -> uniffi::Result<Self> {
        <Vec<u8> as uniffi::Lift<UT>>::try_read(buf).map(TilePixels)
    }

    const TYPE_ID_META: uniffi::MetadataBuffer = <Vec<u8> as uniffi::TypeId<UT>>::TYPE_ID_META;
}

uniffi::derive_ffi_traits!(blanket TilePixels);

// ---- Records and Enums --------------------------------------------------------------------

/// What kind of Resource it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ResourceType {
    Bible,
    Commentary,
    Lexicon,
    Dictionary,
    CrossReferenceSet,
    PeopleAndPlaces,
}

impl From<obs_mobile::ResourceType> for ResourceType {
    fn from(t: obs_mobile::ResourceType) -> Self {
        use obs_mobile::ResourceType as T;
        match t {
            T::Bible => Self::Bible,
            T::Commentary => Self::Commentary,
            T::Lexicon => Self::Lexicon,
            T::Dictionary => Self::Dictionary,
            T::CrossReferenceSet => Self::CrossReferenceSet,
            T::PeopleAndPlaces => Self::PeopleAndPlaces,
        }
    }
}

/// One Resource of the library.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LibraryItem {
    pub resource_id: String,
    pub title: String,
    pub abbreviation: String,
    pub resource_type: ResourceType,
    pub language: String,
    pub right_to_left: bool,
}

impl From<obs_mobile::LibraryItem> for LibraryItem {
    fn from(r: obs_mobile::LibraryItem) -> Self {
        Self {
            resource_id: r.resource_id,
            title: r.title,
            abbreviation: r.abbreviation,
            resource_type: r.resource_type.into(),
            language: r.language,
            right_to_left: r.right_to_left,
        }
    }
}

/// What a reader shows.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ReaderSnapshot {
    pub reader: u32,
    pub resource_id: String,
    pub abbreviation: String,
    pub title: String,
    pub resource_type: ResourceType,
    pub right_to_left: bool,
    pub keyed: bool,
    pub location: String,
    pub can_go_back: bool,
    pub can_go_forward: bool,
}

impl From<obs_mobile::ReaderSnapshot> for ReaderSnapshot {
    fn from(r: obs_mobile::ReaderSnapshot) -> Self {
        Self {
            reader: r.reader.0,
            resource_id: r.resource_id,
            abbreviation: r.abbreviation,
            title: r.title,
            resource_type: r.resource_type.into(),
            right_to_left: r.right_to_left,
            keyed: r.keyed,
            location: r.location,
            can_go_back: r.can_go_back,
            can_go_forward: r.can_go_forward,
        }
    }
}

/// A typed reference as the reader reads it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ParsedReference {
    pub label: String,
    pub id: String,
}

/// A Bible book of a reader's Resource.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BookChapters {
    pub id: String,
    pub name: String,
    pub chapters: u16,
}

/// One day of History.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct HistoryDay {
    pub heading: String,
    pub visits: Vec<HistoryVisit>,
}

/// One Visit.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct HistoryVisit {
    pub id: i64,
    pub label: String,
    pub time: String,
}

/// The reader's viewport in points, and device pixels per point.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct Viewport {
    pub width: f32,
    pub height: f32,
    pub scale: f32,
}

impl From<Viewport> for obs_mobile::Viewport {
    fn from(v: Viewport) -> Self {
        Self {
            width: v.width,
            height: v.height,
            scale: v.scale,
        }
    }
}

/// The system's appearance now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct SystemAppearance {
    pub dark: bool,
}

/// A colour, 0–255 a channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct Rgb {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

/// A tile of a reader's page.
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

/// Where a tile goes.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TilePlace {
    pub key: TileKey,
    pub y: f64,
    pub height: f32,
}

/// What the text view shows for one content offset.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ReaderFrame {
    pub position: f64,
    pub total: f64,
    pub page: Rgb,
    pub tiles: Vec<TilePlace>,
    pub location: String,
    pub micros: u32,
}

impl From<obs_mobile::ReaderFrame> for ReaderFrame {
    fn from(f: obs_mobile::ReaderFrame) -> Self {
        let [red, green, blue] = f.page;
        Self {
            position: f.position,
            total: f.total,
            page: Rgb { red, green, blue },
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

/// A tile's pixels: `width × height`, row-major RGBA8, premultiplied alpha.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Tile {
    pub width: u32,
    pub height: u32,
    pub rgba: TilePixels,
}

impl From<obs_mobile::Tile> for Tile {
    fn from(t: obs_mobile::Tile) -> Self {
        Self {
            width: t.width,
            height: t.height,
            rgba: TilePixels(t.rgba),
        }
    }
}

/// A tile's pixels and how long the core took to draw them, in microseconds.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TimedTile {
    pub tile: Tile,
    pub micros: u32,
}

/// What is under a point.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Hit {
    pub verse: Option<HitVerse>,
    pub word: Option<HitWord>,
    pub footnote: Option<FootnoteMarkerHit>,
}

/// A Verse under a point.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct HitVerse {
    pub id: String,
    pub title: String,
    pub text: String,
    pub cross_reference_count: u32,
    pub cross_reference_label: String,
}

/// A Tagged word under a point.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct HitWord {
    pub written: String,
    pub strongs: String,
    pub lemma: Option<String>,
    pub gloss: Option<String>,
}

/// A Footnote marker under a point.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FootnoteMarkerHit {
    pub note: String,
    pub label: String,
    pub cross_reference: bool,
}

/// A Footnote's text.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Footnote {
    pub note: String,
    pub text: String,
}

impl From<obs_mobile::Hit> for Hit {
    fn from(h: obs_mobile::Hit) -> Self {
        Self {
            verse: h.verse.map(|v| HitVerse {
                id: v.id,
                title: v.title,
                text: v.text,
                cross_reference_count: v.cross_reference_count,
                cross_reference_label: v.cross_reference_label,
            }),
            word: h.word.map(|w| HitWord {
                written: w.written,
                strongs: w.strongs,
                lemma: w.lemma,
                gloss: w.gloss,
            }),
            footnote: h.footnote.map(|f| FootnoteMarkerHit {
                note: f.note,
                label: f.label,
                cross_reference: f.cross_reference,
            }),
        }
    }
}

/// A rectangle in content points.
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
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct AccessibilityItem {
    pub id: String,
    pub label: String,
    pub frame: ContentRect,
    pub heading: bool,
}

/// One choice of a setting.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Choice {
    pub id: String,
    pub label: String,
}

/// A setting's choices and the one in use.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Choices {
    pub choices: Vec<Choice>,
    pub current: String,
}

impl From<obs_mobile::Choices> for Choices {
    fn from(c: obs_mobile::Choices) -> Self {
        Self {
            choices: (c.choices.into_iter())
                .map(|c| Choice {
                    id: c.id,
                    label: c.label,
                })
                .collect(),
            current: c.current,
        }
    }
}

/// The Reading font of one script.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ReadingFont {
    pub slot: String,
    pub label: String,
    pub family: Option<String>,
    pub bundled_family: String,
}

/// Every setting.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Settings {
    pub theme: Choices,
    pub page_tone: Choices,
    pub text_size: f32,
    pub text_size_min: f32,
    pub text_size_max: f32,
    pub reduce_motion: bool,
    pub reading_fonts: Vec<ReadingFont>,
}

impl From<obs_mobile::Settings> for Settings {
    fn from(s: obs_mobile::Settings) -> Self {
        Self {
            theme: s.theme.into(),
            page_tone: s.page_tone.into(),
            text_size: s.text_size,
            text_size_min: s.text_size_min,
            text_size_max: s.text_size_max,
            reduce_motion: s.reduce_motion,
            reading_fonts: (s.reading_fonts.into_iter())
                .map(|f| ReadingFont {
                    slot: f.slot,
                    label: f.label,
                    family: f.family,
                    bundled_family: f.bundled_family,
                })
                .collect(),
        }
    }
}

/// One change for the shell to show (`obs_mobile::Changed`).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum Changed {
    Library,
    Reader { reader: u32 },
    History,
    Message { text: String },
}

impl From<obs_mobile::Changed> for Changed {
    fn from(c: obs_mobile::Changed) -> Self {
        match c {
            obs_mobile::Changed::Library => Self::Library,
            obs_mobile::Changed::Reader(r) => Self::Reader { reader: r.0 },
            obs_mobile::Changed::History => Self::History,
            obs_mobile::Changed::Message(text) => Self::Message { text },
        }
    }
}

/// What the shell implements to hear of changes: called on the pump thread, never with the
/// core locked, so it may call back into the core (after hopping to its own main thread).
#[uniffi::export(with_foreign)]
pub trait Observer: Send + Sync {
    fn changed(&self, change: Changed);
}

// ---- The Object -----------------------------------------------------------------------------

/// How long the pump waits between polls while jobs are running, and otherwise (a call that
/// changes something wakes it at once).
const PUMP_BUSY: Duration = Duration::from_millis(50);
const PUMP_IDLE: Duration = Duration::from_secs(1);

struct Inner {
    app: Mutex<obs_mobile::MobileApp>,
    /// Wakes the pump thread; `None` until [`MobileApp::set_observer`].
    wake: Mutex<Option<Sender<()>>>,
}

/// The running Mobile app (`obs_mobile::MobileApp`). One per process, shared by every scene.
#[derive(uniffi::Object)]
pub struct MobileApp {
    inner: Arc<Inner>,
}

fn stopped() -> MobileError {
    MobileError::Stopped {
        message: "the app stopped after an internal error".into(),
    }
}

impl MobileApp {
    fn lock(&self) -> Result<MutexGuard<'_, obs_mobile::MobileApp>, MobileError> {
        self.inner.app.lock().map_err(|_| stopped())
    }

    /// Run `f` on the app, then wake the pump: what `f` changed may have effects to report.
    fn with<T>(
        &self,
        f: impl FnOnce(&mut obs_mobile::MobileApp) -> Result<T, obs_mobile::MobileError>,
    ) -> Result<T, MobileError> {
        let out = f(&mut *self.lock()?).map_err(MobileError::from);
        if let Ok(wake) = self.inner.wake.lock()
            && let Some(tx) = wake.as_ref()
        {
            let _ = tx.send(());
        }
        out
    }

    /// Read from the app without waking the pump.
    fn read<T>(&self, f: impl FnOnce(&obs_mobile::MobileApp) -> T) -> Result<T, MobileError> {
        Ok(f(&*self.lock()?))
    }
}

/// The pump thread: poll the app until it is gone (or the observer replaced), handing each
/// change to `observer` with the app unlocked.
fn pump(app: &Weak<Inner>, wake: &Receiver<()>, observer: &dyn Observer) {
    let mut wait = PUMP_BUSY;
    loop {
        match wake.recv_timeout(wait) {
            Ok(()) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        let Some(inner) = app.upgrade() else {
            return;
        };
        let Ok(mut core) = inner.app.lock() else {
            return;
        };
        let changed = core.pump();
        wait = if core.busy() { PUMP_BUSY } else { PUMP_IDLE };
        drop(core);
        drop(inner);
        for change in changed {
            observer.changed(change.into());
        }
    }
}

fn paths(strings: Vec<PathBuf>) -> Vec<String> {
    strings
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

#[uniffi::export]
impl MobileApp {
    /// Start the app in `root` (on iOS `Application Support/OpenBible`; the core names
    /// everything in it), adding the Starter library from `bundle` (the Bundled set folder)
    /// on the first run. `study_set` is the debug Study set override (`None` in a release
    /// build).
    #[uniffi::constructor]
    pub fn boot(
        root: String,
        bundle: Option<String>,
        study_set: Option<String>,
    ) -> Result<Arc<Self>, MobileError> {
        let paths =
            obs_mobile::Paths::under(Path::new(&root)).with_study_set(study_set.map(PathBuf::from));
        let app = obs_mobile::MobileApp::boot(paths, bundle.as_deref().map(Path::new))?;
        Ok(Arc::new(Self {
            inner: Arc::new(Inner {
                app: Mutex::new(app),
                wake: Mutex::new(None),
            }),
        }))
    }

    /// Hear of changes through `observer`, from the pump thread this starts. A second call
    /// replaces the observer (and its thread).
    pub fn set_observer(&self, observer: Arc<dyn Observer>) -> Result<(), MobileError> {
        let (tx, rx) = std::sync::mpsc::channel();
        let app = Arc::downgrade(&self.inner);
        std::thread::Builder::new()
            .name("obs-pump".into())
            .spawn(move || pump(&app, &rx, observer.as_ref()))
            .map_err(|e| MobileError::Failed {
                message: format!("cannot start the pump: {e}"),
            })?;
        // The old thread's sender goes: it stops.
        *self.inner.wake.lock().map_err(|_| stopped())? = Some(tx);
        Ok(())
    }

    /// What changed since the last poll (what the pump thread reports to the observer).
    pub fn poll_changed(&self) -> Result<Vec<Changed>, MobileError> {
        let changed = self.lock()?.pump();
        Ok(changed.into_iter().map(Changed::from).collect())
    }

    /// The folders a device backup leaves out (`isExcludedFromBackup`).
    pub fn backup_excluded(&self) -> Result<Vec<String>, MobileError> {
        self.read(|a| paths(a.paths().backup_excluded()))
    }

    /// What went wrong at boot without stopping it.
    pub fn problems(&self) -> Result<Vec<String>, MobileError> {
        self.read(|a| a.problems().to_vec())
    }

    /// The library's Resources, in Preferred order.
    pub fn library(&self) -> Result<Vec<LibraryItem>, MobileError> {
        self.read(|a| a.library().into_iter().map(LibraryItem::from).collect())
    }

    /// Every reader, first one first.
    pub fn readers(&self) -> Result<Vec<ReaderSnapshot>, MobileError> {
        self.read(|a| a.readers().into_iter().map(ReaderSnapshot::from).collect())
    }

    pub fn reader(&self, reader: u32) -> Result<ReaderSnapshot, MobileError> {
        self.read(|a| a.reader(ReaderId(reader)))?
            .map(ReaderSnapshot::from)
            .map_err(MobileError::from)
    }

    /// Show `resource_id` in `reader` (0, then 1), at `at` when given.
    pub fn open(
        &self,
        reader: u32,
        resource_id: String,
        at: Option<String>,
    ) -> Result<ReaderSnapshot, MobileError> {
        self.with(|a| a.open(ReaderId(reader), &resource_id, at.as_deref()))
            .map(ReaderSnapshot::from)
    }

    /// Go to a typed reference or entry Key.
    pub fn navigate(&self, reader: u32, text: String) -> Result<ReaderSnapshot, MobileError> {
        self.with(|a| a.navigate(ReaderId(reader), &text))
            .map(ReaderSnapshot::from)
    }

    /// Read a typed reference without going there.
    pub fn parse_reference(
        &self,
        reader: u32,
        text: String,
    ) -> Result<ParsedReference, MobileError> {
        let parsed = self.read(|a| a.parse_reference(ReaderId(reader), &text))??;
        Ok(ParsedReference {
            label: parsed.label,
            id: parsed.id,
        })
    }

    pub fn back(&self, reader: u32) -> Result<ReaderSnapshot, MobileError> {
        self.with(|a| a.back(ReaderId(reader)))
            .map(ReaderSnapshot::from)
    }

    pub fn forward(&self, reader: u32) -> Result<ReaderSnapshot, MobileError> {
        self.with(|a| a.forward(ReaderId(reader)))
            .map(ReaderSnapshot::from)
    }

    /// The reader's Bible books, for the grid.
    pub fn books(&self, reader: u32) -> Result<Vec<BookChapters>, MobileError> {
        let books = self.read(|a| a.books(ReaderId(reader)))??;
        Ok((books.into_iter())
            .map(|b| BookChapters {
                id: b.id,
                name: b.name,
                chapters: b.chapters,
            })
            .collect())
    }

    pub fn chapters(&self, reader: u32, book: String) -> Result<Vec<u16>, MobileError> {
        Ok(self.read(|a| a.chapters(ReaderId(reader), &book))??)
    }

    pub fn verses(&self, reader: u32, book: String, chapter: u16) -> Result<Vec<u16>, MobileError> {
        Ok(self.read(|a| a.verses(ReaderId(reader), &book, chapter))??)
    }

    /// Go to a verse (its chapter's first without one) from the grid.
    pub fn go_to_verse(
        &self,
        reader: u32,
        book: String,
        chapter: u16,
        verse: Option<u16>,
    ) -> Result<ReaderSnapshot, MobileError> {
        self.with(|a| a.go_to_verse(ReaderId(reader), &book, chapter, verse))
            .map(ReaderSnapshot::from)
    }

    /// History, newest day first.
    pub fn history(&self) -> Result<Vec<HistoryDay>, MobileError> {
        self.read(|a| {
            (a.history().into_iter())
                .map(|d| HistoryDay {
                    heading: d.heading,
                    visits: (d.visits.into_iter())
                        .map(|v| HistoryVisit {
                            id: v.id,
                            label: v.label,
                            time: v.time,
                        })
                        .collect(),
                })
                .collect()
        })
    }

    pub fn open_visit(&self, reader: u32, visit: i64) -> Result<ReaderSnapshot, MobileError> {
        self.with(|a| a.open_visit(ReaderId(reader), visit))
            .map(ReaderSnapshot::from)
    }

    /// The Resources the reader can switch to: its Resource type's, in Preferred order.
    pub fn parallel_resources(&self, reader: u32) -> Result<Vec<LibraryItem>, MobileError> {
        let items = self.read(|a| a.parallel_resources(ReaderId(reader)))??;
        Ok(items.into_iter().map(LibraryItem::from).collect())
    }

    // The hosted text view.

    pub fn frame(
        &self,
        reader: u32,
        offset: f64,
        viewport: Viewport,
        appearance: SystemAppearance,
    ) -> Result<ReaderFrame, MobileError> {
        let appearance = obs_mobile::SystemAppearance {
            dark: appearance.dark,
        };
        let frame = self
            .lock()?
            .frame(ReaderId(reader), offset, viewport.into(), appearance)?;
        Ok(frame.into())
    }

    /// A tile's pixels, or nothing when its epoch is over.
    pub fn tile(
        &self,
        reader: u32,
        key: TileKey,
        scale: f32,
    ) -> Result<Option<TimedTile>, MobileError> {
        let tile = self.lock()?.tile(ReaderId(reader), key.into(), scale)?;
        Ok(tile.map(|t| TimedTile {
            tile: t.tile.into(),
            micros: t.micros,
        }))
    }

    pub fn hit(&self, reader: u32, x: f32, y: f64) -> Result<Option<Hit>, MobileError> {
        Ok(self.lock()?.hit(ReaderId(reader), x, y)?.map(Hit::from))
    }

    pub fn footnote(&self, reader: u32, note: String) -> Result<Footnote, MobileError> {
        let f = self.read(|a| a.footnote(ReaderId(reader), &note))??;
        Ok(Footnote {
            note: f.note,
            text: f.text,
        })
    }

    pub fn text_end(&self, reader: u32) -> Result<u64, MobileError> {
        Ok(self.lock()?.text_end(ReaderId(reader))?)
    }

    pub fn move_offset(&self, reader: u32, offset: u64, by: i64) -> Result<u64, MobileError> {
        Ok(self.lock()?.move_offset(ReaderId(reader), offset, by)?)
    }

    pub fn closest_offset(&self, reader: u32, x: f32, y: f64) -> Result<u64, MobileError> {
        Ok(self.lock()?.closest_offset(ReaderId(reader), x, y)?)
    }

    pub fn enclosing_span(
        &self,
        reader: u32,
        offset: u64,
        kind: TextUnitKind,
    ) -> Result<Option<TextSpan>, MobileError> {
        let span = (self.lock()?).enclosing_span(ReaderId(reader), offset, kind.into())?;
        Ok(span.map(TextSpan::from))
    }

    pub fn text_in(&self, reader: u32, span: TextSpan) -> Result<String, MobileError> {
        Ok(self.lock()?.text_in(ReaderId(reader), span.into())?)
    }

    pub fn copy_text(&self, reader: u32, span: TextSpan) -> Result<String, MobileError> {
        Ok(self.lock()?.copy_text(ReaderId(reader), span.into())?)
    }

    pub fn span_rects(&self, reader: u32, span: TextSpan) -> Result<Vec<ContentRect>, MobileError> {
        let rects = self.lock()?.span_rects(ReaderId(reader), span.into())?;
        Ok(rects.into_iter().map(ContentRect::from).collect())
    }

    pub fn caret_rect(&self, reader: u32, offset: u64) -> Result<ContentRect, MobileError> {
        Ok(self.lock()?.caret_rect(ReaderId(reader), offset)?.into())
    }

    pub fn accessibility_items(&self, reader: u32) -> Result<Vec<AccessibilityItem>, MobileError> {
        let items = self.lock()?.accessibility_items(ReaderId(reader))?;
        Ok((items.into_iter())
            .map(|i| AccessibilityItem {
                id: i.id,
                label: i.label,
                frame: i.frame.into(),
                heading: i.heading,
            })
            .collect())
    }

    // Settings.

    pub fn settings(&self) -> Result<Settings, MobileError> {
        self.read(|a| a.settings().into())
    }

    pub fn set_theme(&self, id: String) -> Result<Settings, MobileError> {
        self.with(|a| a.set_theme(&id)).map(Settings::from)
    }

    pub fn set_page_tone(&self, id: String) -> Result<Settings, MobileError> {
        self.with(|a| a.set_page_tone(&id)).map(Settings::from)
    }

    pub fn set_text_size(&self, size: f32) -> Result<Settings, MobileError> {
        self.with(|a| a.set_text_size(size)).map(Settings::from)
    }

    pub fn set_reading_font(
        &self,
        slot: String,
        family: Option<String>,
    ) -> Result<Settings, MobileError> {
        self.with(|a| a.set_reading_font(&slot, family.as_deref()))
            .map(Settings::from)
    }

    pub fn set_reduce_motion(&self, on: bool) -> Result<Settings, MobileError> {
        self.with(|a| a.set_reduce_motion(on)).map(Settings::from)
    }

    /// Follow Dynamic Type: the Text size for UIKit's content size category. Returns it.
    pub fn dynamic_type(&self, category: String) -> Result<f32, MobileError> {
        self.with(|a| Ok(a.dynamic_type(&category)))
    }

    // The app's life.

    /// Save the Mobile Last session (and History's new Visits).
    pub fn save_session(&self) -> Result<(), MobileError> {
        self.with(obs_mobile::MobileApp::save_session)
    }

    /// The app went to the background.
    pub fn on_background(&self) -> Result<(), MobileError> {
        self.with(obs_mobile::MobileApp::on_background)
    }

    /// The app came back to the foreground.
    pub fn on_resume(&self) -> Result<(), MobileError> {
        self.with(|a| {
            a.on_resume();
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_version_is_the_engines() {
        assert_eq!(engine_version(), obs_mobile::MobileApp::engine_version());
    }

    /// The bytes `Vec<u8>`'s own (per-item) lowering makes, and `TilePixels`'.
    fn lowered(bytes: Vec<u8>) -> (Vec<u8>, Vec<u8>) {
        let vec = <Vec<u8> as uniffi::Lower<crate::UniFfiTag>>::lower(bytes.clone());
        let ours = <TilePixels as uniffi::Lower<crate::UniFfiTag>>::lower(TilePixels(bytes));
        (vec.destroy_into_vec(), ours.destroy_into_vec())
    }

    #[test]
    fn tile_pixels_cross_as_vec_u8_does() {
        for bytes in [
            Vec::new(),
            vec![7],
            (0..=255).cycle().take(1170 * 768 * 4).collect(),
        ] {
            let (vec, ours) = lowered(bytes.clone());
            assert_eq!(ours, vec, "{} bytes", bytes.len());
            // And in a Record (a field is written, not lowered).
            let mut a = Vec::new();
            let mut b = Vec::new();
            <Vec<u8> as uniffi::Lower<crate::UniFfiTag>>::write(bytes.clone(), &mut a);
            <TilePixels as uniffi::Lower<crate::UniFfiTag>>::write(TilePixels(bytes), &mut b);
            assert_eq!(a, b);
        }
        assert_eq!(
            <TilePixels as uniffi::TypeId<crate::UniFfiTag>>::TYPE_ID_META.as_ref(),
            <Vec<u8> as uniffi::TypeId<crate::UniFfiTag>>::TYPE_ID_META.as_ref(),
            "the bindings see bytes"
        );
        let back = <TilePixels as uniffi::Lift<crate::UniFfiTag>>::try_lift(
            <TilePixels as uniffi::Lower<crate::UniFfiTag>>::lower(TilePixels(vec![1, 2, 3])),
        )
        .unwrap();
        assert_eq!(back, TilePixels(vec![1, 2, 3]));
    }
}
