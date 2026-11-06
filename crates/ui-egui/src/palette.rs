//! Page-tone palettes: the only place colour roles become colours (#21 rule 4). Each tone has
//! its own ink, verse-number, link, note-marker, accent and red-letter colours, and its own
//! Highlight styles and overlays (selection, hover, search hit, jump flash). Switching tone
//! never touches layout or the glyph atlas: glyphs take their colour per vertex.

use egui::Color32;
use obs_engine::{HighlightStyle, PageTone};
use obs_layout::ColorRole;

/// The 5 Highlight styles (#18 rule 9): yellow, green, blue and pink backgrounds, and a red
/// underline, named in [`HighlightStyle::ALL`] order (Ctrl+1 … Ctrl+5). Annotations store only
/// the style's id, so the theme never touches the User database.
pub const HIGHLIGHT_STYLES: [&str; 5] = ["Yellow", "Green", "Blue", "Pink", "Red underline"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub page: Color32,
    pub ink: Color32,
    pub verse_number: Color32,
    pub link: Color32,
    pub note_marker: Color32,
    pub accent: Color32,
    pub words_of_jesus: Color32,
    /// Highlight backgrounds (styles 1–4).
    pub highlights: [Color32; 4],
    /// The red-underline Highlight style (style 5).
    pub underline: Color32,
    /// Margin marks of the 5 styles (#63): the style's colour, strong enough to read as a
    /// thin bar beside a verse number (WCAG's 3:1 for graphics against the page).
    pub marks: [Color32; 5],
    pub selection: Color32,
    pub hover: Color32,
    pub search_hit: Color32,
    /// The jump flash at full strength (it fades out).
    pub flash: Color32,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

impl Palette {
    /// Cool linen paper with iron ink (the Study desk page).
    pub const PAPER: Palette = Palette {
        page: rgb(250, 251, 248),
        ink: rgb(32, 38, 41),
        verse_number: rgb(110, 82, 26),
        link: rgb(38, 68, 122),
        note_marker: rgb(30, 98, 88),
        accent: rgb(38, 68, 122),
        words_of_jesus: rgb(150, 32, 30),
        highlights: [
            rgb(255, 236, 140),
            rgb(200, 238, 190),
            rgb(196, 222, 250),
            rgb(250, 206, 224),
        ],
        underline: rgb(200, 40, 40),
        marks: [
            rgb(184, 134, 0),
            rgb(46, 140, 60),
            rgb(40, 110, 200),
            rgb(200, 60, 130),
            rgb(200, 40, 40),
        ],
        selection: rgb(211, 224, 244),
        hover: rgb(226, 235, 247),
        search_hit: rgb(255, 214, 120),
        flash: rgb(240, 214, 150),
    };

    /// Warm sepia paper with brown ink.
    pub const SEPIA: Palette = Palette {
        page: rgb(244, 236, 216),
        ink: rgb(56, 44, 30),
        verse_number: rgb(104, 72, 24),
        link: rgb(40, 66, 112),
        note_marker: rgb(34, 92, 80),
        accent: rgb(118, 58, 26),
        words_of_jesus: rgb(146, 34, 26),
        highlights: [
            rgb(248, 224, 128),
            rgb(204, 226, 176),
            rgb(196, 214, 232),
            rgb(240, 200, 208),
        ],
        underline: rgb(184, 40, 32),
        marks: [
            rgb(168, 120, 0),
            rgb(52, 128, 58),
            rgb(40, 100, 180),
            rgb(186, 56, 120),
            rgb(184, 40, 32),
        ],
        selection: rgb(214, 208, 222),
        hover: rgb(232, 222, 200),
        search_hit: rgb(250, 204, 110),
        flash: rgb(232, 206, 144),
    };

    /// Near-black page with warm light ink; highlights are deeper and darker, the red
    /// underline lighter (#21 decision 3).
    pub const DARK: Palette = Palette {
        page: rgb(22, 22, 24),
        ink: rgb(228, 226, 220),
        verse_number: rgb(214, 192, 142),
        link: rgb(164, 198, 248),
        note_marker: rgb(148, 214, 170),
        accent: rgb(238, 182, 138),
        words_of_jesus: rgb(250, 166, 154),
        highlights: [
            rgb(70, 60, 16),
            rgb(26, 56, 36),
            rgb(24, 46, 76),
            rgb(72, 32, 50),
        ],
        underline: rgb(240, 120, 120),
        marks: [
            rgb(232, 196, 80),
            rgb(110, 200, 130),
            rgb(120, 166, 240),
            rgb(232, 130, 184),
            rgb(240, 120, 120),
        ],
        selection: rgb(36, 56, 88),
        hover: rgb(36, 42, 52),
        search_hit: rgb(80, 62, 16),
        flash: rgb(72, 60, 26),
    };

    /// The palette of a resolved tone (`Auto` must be resolved first; it reads as Paper).
    pub fn for_tone(tone: PageTone) -> Palette {
        match tone {
            PageTone::Sepia => Self::SEPIA,
            PageTone::Dark => Self::DARK,
            PageTone::Paper | PageTone::Auto => Self::PAPER,
        }
    }

    /// A Highlight style's background, or `None` for the red underline (drawn over the
    /// glyphs in [`Palette::underline`]).
    pub fn highlight(&self, style: HighlightStyle) -> Option<Color32> {
        match style {
            HighlightStyle::Yellow => Some(self.highlights[0]),
            HighlightStyle::Green => Some(self.highlights[1]),
            HighlightStyle::Blue => Some(self.highlights[2]),
            HighlightStyle::Pink => Some(self.highlights[3]),
            HighlightStyle::RedUnderline => None,
        }
    }

    /// A Highlight style's margin mark colour (#63).
    pub fn mark(&self, style: HighlightStyle) -> Color32 {
        let i = HighlightStyle::ALL
            .iter()
            .position(|s| *s == style)
            .unwrap_or(0);
        self.marks[i]
    }

    pub fn color(&self, role: ColorRole) -> Color32 {
        match role {
            ColorRole::Ink => self.ink,
            ColorRole::VerseNumber => self.verse_number,
            ColorRole::Link => self.link,
            ColorRole::NoteMarker => self.note_marker,
            ColorRole::Accent => self.accent,
            ColorRole::WordsOfJesus => self.words_of_jesus,
            // Interlinear lines (#143) read as the text's companions, apart from the ink.
            ColorRole::Interlinear => self.note_marker,
        }
    }

    /// Every background text can sit on in this tone: the page, each Highlight style and
    /// each overlay.
    pub fn backgrounds(&self) -> Vec<(&'static str, Color32)> {
        let mut v = vec![("page", self.page)];
        for (i, h) in self.highlights.iter().enumerate() {
            v.push((HIGHLIGHT_STYLES[i], *h));
        }
        v.extend([
            ("selection", self.selection),
            ("hover", self.hover),
            ("search hit", self.search_hit),
            ("jump flash", self.flash),
        ]);
        v
    }

    /// Every text colour of this tone, by role.
    pub fn inks(&self) -> [(ColorRole, Color32); 7] {
        [
            ColorRole::Ink,
            ColorRole::VerseNumber,
            ColorRole::Link,
            ColorRole::NoteMarker,
            ColorRole::Accent,
            ColorRole::WordsOfJesus,
            ColorRole::Interlinear,
        ]
        .map(|r| (r, self.color(r)))
    }
}

/// WCAG 2 contrast ratio of two opaque colours (1–21).
pub fn contrast(a: Color32, b: Color32) -> f32 {
    fn channel(c: u8) -> f32 {
        let c = f32::from(c) / 255.0;
        if c <= 0.040_45 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    let lum =
        |c: Color32| 0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b());
    let (la, lb) = (lum(a), lum(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}
