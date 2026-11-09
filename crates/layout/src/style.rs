//! Styles carried by laid-out text. Colours are **roles**, never RGB: the painter resolves
//! them from the active Page tone, so a theme change never re-lays-out text (#21 rule 4).

use obs_fonts::{FaceId, ReadingFaces, ShapeStyle, face};
use serde::{Deserialize, Serialize};

/// What a piece of text is, for colouring. The painter maps each role to a colour per tone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorRole {
    /// Body text.
    Ink,
    /// Verse numbers.
    VerseNumber,
    /// Scripture references and cross-references.
    Link,
    /// Note markers.
    NoteMarker,
    /// Headings, chapter numbers.
    Accent,
    /// Words of Jesus (red letter, when enabled).
    WordsOfJesus,
    /// Interlinear lines under Tagged words (#143).
    Interlinear,
}

/// How verse numbers are shown (Text settings, #66).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum VerseNumbers {
    /// Small and raised (the default).
    #[default]
    Superscript,
    /// Bold, slightly smaller than the text, on the baseline.
    Inline,
    /// Not shown (chapter numbers still are).
    Hidden,
}

/// How a Bible's verses flow on the page (Reading layout, #209).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextFlow {
    /// Paragraphs where the Bible starts them (the default).
    #[default]
    Paragraphs,
    /// Every verse starts a line of its own.
    VersePerLine,
}

/// How wide the reading column is drawn (Reading layout, #209). The pane centres it; a pane
/// narrower than the column gets all its width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColumnWidth {
    Narrow,
    /// About 70 characters a line (the default).
    #[default]
    Medium,
    Wide,
    /// The whole pane.
    Full,
}

impl ColumnWidth {
    /// The column's width in ems of the Text size; `None` for the whole pane.
    pub fn ems(self) -> Option<f32> {
        match self {
            ColumnWidth::Narrow => Some(30.0),
            ColumnWidth::Medium => Some(38.0),
            ColumnWidth::Wide => Some(50.0),
            ColumnWidth::Full => None,
        }
    }
}

/// How Resource text is laid out on the page (#209): set per Tab, with defaults in Settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct ReadingLayout {
    pub flow: TextFlow,
    /// A drop numeral, two lines tall, at the start of each chapter.
    pub large_chapter_numbers: bool,
    /// A Bible's section headings are shown.
    pub section_headings: bool,
    pub column: ColumnWidth,
    /// Prose is justified, and words of an English Resource are hyphenated to fit.
    pub justified: bool,
}

impl Default for ReadingLayout {
    fn default() -> Self {
        Self {
            flow: TextFlow::Paragraphs,
            large_chapter_numbers: false,
            section_headings: true,
            column: ColumnWidth::Medium,
            justified: false,
        }
    }
}

/// The user's text settings in v0.1 (#14 rule 12), plus per-script tuning.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextSettings {
    /// Body font size in px.
    pub size: f32,
    /// Hebrew is drawn this much larger than Latin and Greek, to match its visual size.
    pub hebrew_scale: f32,
    /// Line height as a multiple of the tallest font on the line.
    pub line_spacing: f32,
    /// Share of Ezra SIL's ascent and descent counted for line height. Its metrics are loose
    /// once scaled up, so full metrics leave Hebrew lines visibly further apart (#13).
    pub hebrew_line_height: f32,
    pub red_letter: bool,
    pub verse_numbers: VerseNumbers,
    /// Footnote markers (#208): a raised letter where each Footnote of the Resource belongs,
    /// and the marker of each cross-reference note. Off: neither is laid out.
    pub footnote_markers: bool,
    /// The reading font of each script (View → Fonts, #138). A character the chosen face
    /// lacks is drawn in the bundled face for its script.
    pub fonts: ReadingFaces,
    /// How the text flows on the page (#209).
    pub layout: ReadingLayout,
}

impl Default for TextSettings {
    fn default() -> Self {
        Self {
            size: Self::DEFAULT_SIZE,
            hebrew_scale: 1.3,
            line_spacing: 1.25,
            hebrew_line_height: 0.82,
            red_letter: true,
            verse_numbers: VerseNumbers::Superscript,
            footnote_markers: true,
            fonts: ReadingFaces::default(),
            layout: ReadingLayout::default(),
        }
    }
}

impl TextSettings {
    /// The reading size of a fresh install, in points (#241): comfortably readable at 1× and
    /// on 2× displays (glyphs are rastered at `size × pixels per point`).
    pub const DEFAULT_SIZE: f32 = 20.0;

    /// A key that changes whenever layout output would (part of the height-index key).
    pub fn key(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for bits in [
            self.size.to_bits(),
            self.hebrew_scale.to_bits(),
            self.line_spacing.to_bits(),
            self.hebrew_line_height.to_bits(),
            u32::from(self.red_letter),
            self.verse_numbers as u32,
            u32::from(self.footnote_markers),
            // The column width only changes the width, which the height index keys apart.
            self.layout.flow as u32,
            u32::from(self.layout.large_chapter_numbers),
            u32::from(self.layout.section_headings),
            u32::from(self.layout.justified),
        ]
        .into_iter()
        .chain(
            [self.fonts.latin, self.fonts.greek, self.fonts.hebrew]
                .iter()
                .flat_map(|set| set.faces().map(FaceId::code)),
        ) {
            h ^= u64::from(bits);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h
    }
}

/// How one run is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunStyle {
    pub face: FaceId,
    /// Size in px.
    pub size: f32,
    pub small_caps: bool,
    /// Baseline shift in px, positive up (superscript verse numbers).
    pub raise: f32,
    pub role: ColorRole,
}

impl RunStyle {
    pub fn body(size: f32) -> Self {
        Self {
            face: FaceId::Regular,
            size,
            small_caps: false,
            raise: 0.0,
            role: ColorRole::Ink,
        }
    }

    pub(crate) fn shape_style(&self, rtl: bool) -> ShapeStyle {
        ShapeStyle::new(self.face, self.size, self.small_caps, rtl)
    }

    /// Ascent and descent (both positive, px) this run adds to its line, after the raise.
    pub(crate) fn line_extent(&self, settings_hebrew_line: f32) -> (f32, f32) {
        let Ok(f) = face(self.face) else {
            return (self.size * 0.8, self.size * 0.25);
        };
        let (asc, desc, _) = f.metrics.scaled(self.size);
        let k = if self.face == FaceId::Hebrew {
            settings_hebrew_line
        } else {
            1.0
        };
        (
            (asc * k + self.raise).max(0.0),
            (-desc * k - self.raise).max(0.0),
        )
    }
}
