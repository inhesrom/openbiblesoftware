//! The bundled faces, faces the app loads at runtime, and how text picks one.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

use harfrust::Tag;
use skrifa::instance::{LocationRef, Size};
use skrifa::raw::TableProvider;
use skrifa::{FontRef, MetadataProvider};
use unicode_script::{Script, UnicodeScript};

/// One face Resource text can be drawn in: a bundled face (ADR 0004), or a font the app
/// loaded at runtime with [`load_user_face`] (a reading font the user chose, #138).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FaceId {
    /// Gentium Plus Regular: English, polytonic Greek, Latin.
    Regular,
    Italic,
    Bold,
    BoldItalic,
    /// Ezra SIL: Hebrew with niqqud and cantillation.
    Hebrew,
    /// A loaded user face. Numbers are never reused, so cached glyphs and shaped segments of
    /// an unloaded face can never be mistaken for another font's.
    User(u32),
}

impl FaceId {
    /// Every bundled face.
    pub const ALL: [FaceId; 5] = [
        FaceId::Regular,
        FaceId::Italic,
        FaceId::Bold,
        FaceId::BoldItalic,
        FaceId::Hebrew,
    ];

    /// A number unique to the face, stable for the run (part of layout cache keys).
    pub const fn code(self) -> u32 {
        match self {
            FaceId::Regular => 0,
            FaceId::Italic => 1,
            FaceId::Bold => 2,
            FaceId::BoldItalic => 3,
            FaceId::Hebrew => 4,
            FaceId::User(n) => 0x100 + n,
        }
    }

    pub const fn is_bundled(self) -> bool {
        !matches!(self, FaceId::User(_))
    }

    /// File name of the face, for diagnostics.
    pub const fn file(self) -> &'static str {
        match self {
            FaceId::Regular => "GentiumPlus-Regular.ttf",
            FaceId::Italic => "GentiumPlus-Italic.ttf",
            FaceId::Bold => "GentiumPlus-Bold.ttf",
            FaceId::BoldItalic => "GentiumPlus-BoldItalic.ttf",
            FaceId::Hebrew => "SILEOT.ttf",
            FaceId::User(_) => "a font loaded from this computer",
        }
    }

    /// The font file of a bundled face, compiled into the binary.
    pub const fn bundled_bytes(self) -> Option<&'static [u8]> {
        Some(match self {
            FaceId::Regular => include_bytes!("../assets/gentium/GentiumPlus-Regular.ttf"),
            FaceId::Italic => include_bytes!("../assets/gentium/GentiumPlus-Italic.ttf"),
            FaceId::Bold => include_bytes!("../assets/gentium/GentiumPlus-Bold.ttf"),
            FaceId::BoldItalic => include_bytes!("../assets/gentium/GentiumPlus-BoldItalic.ttf"),
            FaceId::Hebrew => include_bytes!("../assets/ezra/SILEOT.ttf"),
            FaceId::User(_) => return None,
        })
    }

    /// The bundled face for a script and style. Hebrew has one face; bold and italic Hebrew
    /// are drawn upright and regular weight.
    pub fn for_script(script: Script, bold: bool, italic: bool) -> FaceId {
        if script == Script::Hebrew {
            return FaceId::Hebrew;
        }
        match (bold, italic) {
            (false, false) => FaceId::Regular,
            (false, true) => FaceId::Italic,
            (true, false) => FaceId::Bold,
            (true, true) => FaceId::BoldItalic,
        }
    }
}

/// Vertical metrics in font units (y up: ascent positive, descent negative).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceMetrics {
    pub units_per_em: u16,
    pub ascent: f32,
    pub descent: f32,
    pub leading: f32,
}

impl FaceMetrics {
    /// Ascent, descent and leading at `size` px.
    pub fn scaled(&self, size: f32) -> (f32, f32, f32) {
        let s = size / f32::from(self.units_per_em.max(1));
        (self.ascent * s, self.descent * s, self.leading * s)
    }
}

/// The bytes of a face's font file.
#[derive(Clone)]
enum FaceData {
    Bundled(&'static [u8]),
    Loaded(Arc<Vec<u8>>),
}

impl FaceData {
    fn bytes(&self) -> &[u8] {
        match self {
            FaceData::Bundled(b) => b,
            FaceData::Loaded(b) => b,
        }
    }
}

/// A loaded face: shaping data (harfrust) and metrics/outlines (skrifa) over the same bytes.
pub struct Face {
    pub id: FaceId,
    pub(crate) shaping: harfrust::Font,
    data: FaceData,
    /// The face's index in its file (a `.ttc` collection holds several).
    index: u32,
    pub metrics: FaceMetrics,
    small_caps: bool,
}

impl std::fmt::Debug for Face {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Face")
            .field("id", &self.id)
            .field("metrics", &self.metrics)
            .field("small_caps", &self.small_caps)
            .finish_non_exhaustive()
    }
}

impl Face {
    /// skrifa view of the face, for metrics and outlines.
    pub fn font_ref(&self) -> Option<FontRef<'_>> {
        FontRef::from_index(self.data.bytes(), self.index).ok()
    }

    /// Whether the face has a glyph for `c` (its cmap maps it).
    pub fn covers(&self, c: char) -> bool {
        self.shaping.charmap().map_unicode(c).is_some()
    }

    /// Whether the face has real small capitals (an OpenType `smcp` feature). Without them
    /// the layout draws synthetic ones (#138).
    pub fn has_small_caps(&self) -> bool {
        self.small_caps
    }
}

/// Why a face failed to load. For a bundled face this means a broken build (tests load
/// every one); for a loaded face, a file that is not a usable font.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceError(pub FaceId);

impl std::fmt::Display for FaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            FaceId::User(_) => write!(f, "the font file could not be read"),
            id => write!(f, "bundled font {} could not be read", id.file()),
        }
    }
}

impl std::error::Error for FaceError {}

fn load(id: FaceId, data: FaceData, index: u32) -> Result<Face, FaceError> {
    let shaping = match &data {
        FaceData::Bundled(b) => harfrust::Font::new(*b, index),
        FaceData::Loaded(b) => {
            let shared: Arc<dyn AsRef<[u8]> + Send + Sync> = Arc::clone(b) as _;
            harfrust::Font::new(shared, index)
        }
    }
    .ok_or(FaceError(id))?;
    let font_ref = FontRef::from_index(data.bytes(), index).map_err(|_| FaceError(id))?;
    let m = font_ref.metrics(Size::unscaled(), LocationRef::default());
    if m.units_per_em == 0 || !outlined(&font_ref) {
        return Err(FaceError(id));
    }
    let smcp = Tag::new(b"smcp");
    let small_caps = font_ref
        .gsub()
        .ok()
        .and_then(|gsub| gsub.feature_list().ok())
        .is_some_and(|list| {
            list.feature_records()
                .iter()
                .any(|r| r.feature_tag() == smcp)
        });
    Ok(Face {
        id,
        shaping,
        data,
        index,
        metrics: FaceMetrics {
            units_per_em: m.units_per_em,
            ascent: m.ascent,
            descent: m.descent,
            leading: m.leading,
        },
        small_caps,
    })
}

/// Whether the face has outlines (TrueType `glyf`, CFF or CFF2). Glyphs are painted from
/// outlines only, so a bitmap-only face (`EBDT`, `CBDT` or `sbix` alone) would map letters in
/// its cmap and then draw nothing: it is refused instead, and the bundled face is used.
fn outlined(font: &FontRef<'_>) -> bool {
    font.outline_glyphs().format().is_some()
}

/// Whether face `index` of a font file has outlines the app can draw (see [`load_user_face`]).
/// For the interface font, which egui loads itself.
pub fn has_outlines(bytes: &[u8], index: u32) -> bool {
    FontRef::from_index(bytes, index).is_ok_and(|f| outlined(&f))
}

fn load_bundled(id: FaceId) -> Result<Arc<Face>, FaceError> {
    let bytes = id.bundled_bytes().ok_or(FaceError(id))?;
    load(id, FaceData::Bundled(bytes), 0).map(Arc::new)
}

/// Every bundled face, loaded once and shared by all threads.
fn bundled() -> Result<&'static [Arc<Face>; 5], FaceError> {
    static FACES: OnceLock<Result<[Arc<Face>; 5], FaceError>> = OnceLock::new();
    FACES
        .get_or_init(|| {
            Ok([
                load_bundled(FaceId::Regular)?,
                load_bundled(FaceId::Italic)?,
                load_bundled(FaceId::Bold)?,
                load_bundled(FaceId::BoldItalic)?,
                load_bundled(FaceId::Hebrew)?,
            ])
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Faces loaded at runtime, by number.
fn loaded() -> &'static RwLock<HashMap<u32, Arc<Face>>> {
    static LOADED: OnceLock<RwLock<HashMap<u32, Arc<Face>>>> = OnceLock::new();
    LOADED.get_or_init(|| RwLock::new(HashMap::new()))
}

/// One face, bundled or loaded. A loaded face that was unloaded is an error.
pub fn face(id: FaceId) -> Result<Arc<Face>, FaceError> {
    match id {
        FaceId::User(n) => loaded()
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .get(&n)
            .cloned()
            .ok_or(FaceError(id)),
        bundled_id => bundled().map(|all| Arc::clone(&all[bundled_id.code() as usize])),
    }
}

/// Load a font file the app read from this computer (#138): face `index` of `bytes`. Every
/// call gets a new [`FaceId::User`], usable from any thread until [`unload_user_face`]. A file
/// that is not a font, or a face without outlines ([`has_outlines`]), is refused.
pub fn load_user_face(bytes: Arc<Vec<u8>>, index: u32) -> Result<FaceId, FaceError> {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let id = FaceId::User(NEXT.fetch_add(1, Ordering::Relaxed));
    let face = load(id, FaceData::Loaded(bytes), index)?;
    if let FaceId::User(n) = id {
        loaded()
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .insert(n, Arc::new(face));
    }
    Ok(id)
}

/// Forget a loaded face. Text still laid out in it draws nothing; bundled faces stay.
pub fn unload_user_face(id: FaceId) {
    if let FaceId::User(n) = id {
        loaded()
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&n);
    }
}

/// The script of a run of text: the first character with a real script (marks, digits and
/// punctuation inherit), or `Common`.
pub fn script_of(text: &str) -> Script {
    text.chars()
        .map(|c| c.script())
        .find(|s| !matches!(s, Script::Common | Script::Inherited | Script::Unknown))
        .unwrap_or(Script::Common)
}
