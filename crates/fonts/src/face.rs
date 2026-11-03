//! The bundled faces and how text picks one.

use std::sync::OnceLock;

use skrifa::instance::{LocationRef, Size};
use skrifa::{FontRef, MetadataProvider};
use unicode_script::{Script, UnicodeScript};

/// One bundled face. Resource text uses only these (ADR 0004; user font families are later).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FaceId {
    /// Gentium Plus Regular: English, polytonic Greek, Latin.
    Regular,
    Italic,
    Bold,
    BoldItalic,
    /// Ezra SIL: Hebrew with niqqud and cantillation.
    Hebrew,
}

impl FaceId {
    pub const ALL: [FaceId; 5] = [
        FaceId::Regular,
        FaceId::Italic,
        FaceId::Bold,
        FaceId::BoldItalic,
        FaceId::Hebrew,
    ];

    pub const fn index(self) -> usize {
        self as usize
    }

    /// File name of the face, for diagnostics.
    pub const fn file(self) -> &'static str {
        match self {
            FaceId::Regular => "GentiumPlus-Regular.ttf",
            FaceId::Italic => "GentiumPlus-Italic.ttf",
            FaceId::Bold => "GentiumPlus-Bold.ttf",
            FaceId::BoldItalic => "GentiumPlus-BoldItalic.ttf",
            FaceId::Hebrew => "SILEOT.ttf",
        }
    }

    /// The font file, compiled into the binary.
    pub const fn bytes(self) -> &'static [u8] {
        match self {
            FaceId::Regular => include_bytes!("../assets/gentium/GentiumPlus-Regular.ttf"),
            FaceId::Italic => include_bytes!("../assets/gentium/GentiumPlus-Italic.ttf"),
            FaceId::Bold => include_bytes!("../assets/gentium/GentiumPlus-Bold.ttf"),
            FaceId::BoldItalic => include_bytes!("../assets/gentium/GentiumPlus-BoldItalic.ttf"),
            FaceId::Hebrew => include_bytes!("../assets/ezra/SILEOT.ttf"),
        }
    }

    /// The face for a script and style. Hebrew has one face; bold and italic Hebrew are drawn
    /// upright and regular weight.
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

/// A loaded face: shaping data (harfrust) and metrics/outlines (skrifa) over the same bytes.
pub struct Face {
    pub id: FaceId,
    pub(crate) shaping: harfrust::Font,
    pub metrics: FaceMetrics,
}

impl std::fmt::Debug for Face {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Face")
            .field("id", &self.id)
            .field("metrics", &self.metrics)
            .finish_non_exhaustive()
    }
}

impl Face {
    /// skrifa view of the face, for metrics and outlines.
    pub fn font_ref(&self) -> Option<FontRef<'static>> {
        FontRef::new(self.id.bytes()).ok()
    }
}

/// Why a bundled face failed to load. Bundled data is fixed at build time, so this means a
/// broken build; tests load every face.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceError(pub FaceId);

impl std::fmt::Display for FaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "bundled font {} could not be read", self.0.file())
    }
}

impl std::error::Error for FaceError {}

fn load(id: FaceId) -> Result<Face, FaceError> {
    let bytes = id.bytes();
    let shaping = harfrust::Font::new(bytes, 0).ok_or(FaceError(id))?;
    let font_ref = FontRef::new(bytes).map_err(|_| FaceError(id))?;
    let m = font_ref.metrics(Size::unscaled(), LocationRef::default());
    Ok(Face {
        id,
        shaping,
        metrics: FaceMetrics {
            units_per_em: m.units_per_em,
            ascent: m.ascent,
            descent: m.descent,
            leading: m.leading,
        },
    })
}

/// Every bundled face, loaded once and shared by all threads.
pub fn faces() -> Result<&'static [Face; 5], FaceError> {
    static FACES: OnceLock<Result<[Face; 5], FaceError>> = OnceLock::new();
    FACES
        .get_or_init(|| {
            Ok([
                load(FaceId::Regular)?,
                load(FaceId::Italic)?,
                load(FaceId::Bold)?,
                load(FaceId::BoldItalic)?,
                load(FaceId::Hebrew)?,
            ])
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// One bundled face.
pub fn face(id: FaceId) -> Result<&'static Face, FaceError> {
    faces().map(|all| &all[id.index()])
}

/// The script of a run of text: the first character with a real script (marks, digits and
/// punctuation inherit), or `Common`.
pub fn script_of(text: &str) -> Script {
    text.chars()
        .map(|c| c.script())
        .find(|s| !matches!(s, Script::Common | Script::Inherited | Script::Unknown))
        .unwrap_or(Script::Common)
}
