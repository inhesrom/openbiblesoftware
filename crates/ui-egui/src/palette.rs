//! Page-tone palettes: the only place colour roles become colours (#21 rule 4). Each tone has
//! its own ink, verse-number, link, note-marker, accent and red-letter colours and overlays
//! (selection, hover, search hit, jump flash), and turns a Highlight style into the colours it
//! is drawn in for that tone ([`Palette::paint`], ADR 0012). Switching tone never touches
//! layout or the glyph atlas: glyphs take their colour per vertex.

use egui::Color32;
use obs_engine::{HighlightLook, PageTone, Rgb, StyleAppearance, StyleTone};
use obs_layout::ColorRole;

/// WCAG AA for text (#21): ink on a Highlight background, a text-colour style on the page.
pub const TEXT_CONTRAST: f32 = 4.5;
/// WCAG AA for graphics (#21): an underline, a box or a margin mark against the page.
pub const MARK_CONTRAST: f32 = 3.0;

/// A Highlight style as one Page tone draws it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StylePaint {
    pub look: HighlightLook,
    /// The background, line, box or text colour.
    pub color: Color32,
    /// Its margin mark beside a verse number (#63), and its swatch on the chrome.
    pub mark: Color32,
}

/// Why a colour cannot be a style's colour in a tone: what it is read against, the contrast
/// it has and what it needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContrastShort {
    pub against: &'static str,
    pub ratio: f32,
    pub needed: f32,
}

impl std::fmt::Display for ContrastShort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:.1}:1 against the {}, needs {:.1}:1",
            self.ratio, self.against, self.needed
        )
    }
}

pub fn to_color(c: Rgb) -> Color32 {
    let [r, g, b] = c.0;
    Color32::from_rgb(r, g, b)
}

pub fn to_rgb(c: Color32) -> Rgb {
    Rgb::new(c.r(), c.g(), c.b())
}

/// `a` moved `t` (0–1) of the way to `b`, per channel.
fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let ch = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(ch(a.r(), b.r()), ch(a.g(), b.g()), ch(a.b(), b.b()))
}

/// Steps a derived colour is searched in, from the colour itself to the target.
const DERIVE_STEPS: u16 = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// The tone this is the palette of (Auto resolved).
    pub tone: StyleTone,
    pub page: Color32,
    pub ink: Color32,
    pub verse_number: Color32,
    pub link: Color32,
    pub note_marker: Color32,
    pub accent: Color32,
    pub words_of_jesus: Color32,
    /// The underline of the Tagged word under the pointer (a mark: 3:1 against the page).
    pub underline: Color32,
    pub selection: Color32,
    pub hover: Color32,
    pub search_hit: Color32,
    /// Find in Resource (#175): every match of the Find bar; the current one is drawn in
    /// `search_hit` with a `find_current` outline.
    pub find_match: Color32,
    pub find_current: Color32,
    /// The jump flash at full strength (it fades out).
    pub flash: Color32,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

impl Palette {
    /// Cool linen paper with iron ink (the Study desk page).
    pub const PAPER: Palette = Palette {
        tone: StyleTone::Paper,
        page: rgb(250, 251, 248),
        ink: rgb(32, 38, 41),
        verse_number: rgb(110, 82, 26),
        link: rgb(38, 68, 122),
        note_marker: rgb(30, 98, 88),
        accent: rgb(38, 68, 122),
        words_of_jesus: rgb(150, 32, 30),
        underline: rgb(200, 40, 40),
        selection: rgb(211, 224, 244),
        hover: rgb(226, 235, 247),
        search_hit: rgb(255, 214, 120),
        find_match: rgb(255, 234, 186),
        find_current: rgb(176, 98, 0),
        flash: rgb(240, 214, 150),
    };

    /// Warm sepia paper with brown ink.
    pub const SEPIA: Palette = Palette {
        tone: StyleTone::Sepia,
        page: rgb(244, 236, 216),
        ink: rgb(56, 44, 30),
        verse_number: rgb(104, 72, 24),
        link: rgb(40, 66, 112),
        note_marker: rgb(34, 92, 80),
        accent: rgb(118, 58, 26),
        words_of_jesus: rgb(146, 34, 26),
        underline: rgb(184, 40, 32),
        selection: rgb(214, 208, 222),
        hover: rgb(232, 222, 200),
        search_hit: rgb(250, 204, 110),
        find_match: rgb(248, 224, 172),
        find_current: rgb(160, 88, 8),
        flash: rgb(232, 206, 144),
    };

    /// Near-black page with warm light ink; Highlight backgrounds derive deeper and darker,
    /// lines lighter (#21 decision 3).
    pub const DARK: Palette = Palette {
        tone: StyleTone::Dark,
        page: rgb(22, 22, 24),
        ink: rgb(228, 226, 220),
        verse_number: rgb(214, 192, 142),
        link: rgb(164, 198, 248),
        note_marker: rgb(148, 214, 170),
        accent: rgb(238, 182, 138),
        words_of_jesus: rgb(250, 166, 154),
        underline: rgb(240, 120, 120),
        selection: rgb(36, 56, 88),
        hover: rgb(36, 42, 52),
        search_hit: rgb(80, 62, 16),
        find_match: rgb(56, 46, 18),
        find_current: rgb(240, 182, 84),
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

    /// The palette of a style tone.
    pub fn for_style_tone(tone: StyleTone) -> Palette {
        match tone {
            StyleTone::Paper => Self::PAPER,
            StyleTone::Sepia => Self::SEPIA,
            StyleTone::Dark => Self::DARK,
        }
    }

    /// Whether `color` can be a style's colour with `look` in this tone (#21): every ink
    /// reads on it as a background (4.5:1); as a text colour it reads on the page and every
    /// overlay (4.5:1); as a line or box it shows against the page (3:1).
    pub fn check(&self, look: HighlightLook, color: Color32) -> Result<(), ContrastShort> {
        let short = |against: &'static str, fg: Color32, bg: Color32, needed: f32| {
            let ratio = contrast(fg, bg);
            (ratio < needed).then_some(ContrastShort {
                against,
                ratio,
                needed,
            })
        };
        let worst = match look {
            HighlightLook::Background => self
                .inks()
                .into_iter()
                .filter_map(|(role, ink)| short(role_name(role), ink, color, TEXT_CONTRAST))
                .min_by(|a, b| a.ratio.total_cmp(&b.ratio)),
            HighlightLook::TextColor => self
                .overlays()
                .into_iter()
                .filter_map(|(name, bg)| short(name, color, bg, TEXT_CONTRAST))
                .min_by(|a, b| a.ratio.total_cmp(&b.ratio)),
            HighlightLook::Underline | HighlightLook::Box => {
                short("page", color, self.page, MARK_CONTRAST)
            }
        };
        worst.map_or(Ok(()), Err)
    }

    /// `base` made to pass [`Palette::check`] for `look` here, changing it as little as
    /// possible: a background fades toward the page, a line, box or text colour deepens
    /// toward the ink (ADR 0012). Both ends pass, so this always finds one.
    pub fn derive(&self, look: HighlightLook, base: Color32) -> Color32 {
        let toward = match look {
            HighlightLook::Background => self.page,
            _ => self.ink,
        };
        (0..=DERIVE_STEPS)
            .map(|k| mix(base, toward, f32::from(k) / f32::from(DERIVE_STEPS)))
            .find(|c| self.check(look, *c).is_ok())
            .unwrap_or(toward)
    }

    /// A style's colours in this tone: the user's adjustment for the tone when it passes the
    /// check, else derived from the style's colour; and its margin mark, the colour itself
    /// for a line, box or text colour, a deepened one for a background.
    pub fn paint(&self, a: &StyleAppearance) -> StylePaint {
        let color = self
            .tone
            .adjusted(a)
            .map(to_color)
            .filter(|c| self.check(a.look, *c).is_ok())
            .unwrap_or_else(|| self.derive(a.look, to_color(a.color)));
        let mark = match a.look {
            HighlightLook::Background => self.derive(HighlightLook::Underline, to_color(a.color)),
            _ => color,
        };
        StylePaint {
            look: a.look,
            color,
            mark,
        }
    }

    /// The colour a built-in Highlight style (`yellow` … `red_underline`) is drawn in here.
    #[cfg(test)]
    pub(crate) fn built_in_color(&self, id: &str) -> Color32 {
        self.paint(&crate::marks::HighlightLooks::built_in().get(id))
            .color
    }

    /// The margin mark of a built-in Highlight style here.
    #[cfg(test)]
    pub(crate) fn built_in_mark(&self, id: &str) -> Color32 {
        self.paint(&crate::marks::HighlightLooks::built_in().get(id))
            .mark
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

    /// Every background text can sit on in this tone apart from Highlights: the page and
    /// each overlay.
    pub fn overlays(&self) -> [(&'static str, Color32); 6] {
        [
            ("page", self.page),
            ("selection", self.selection),
            ("hover", self.hover),
            ("search hit", self.search_hit),
            ("find match", self.find_match),
            ("jump flash", self.flash),
        ]
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

/// How the styles window names a text colour that does not read on a background.
fn role_name(role: ColorRole) -> &'static str {
    match role {
        ColorRole::Ink => "text",
        ColorRole::VerseNumber => "verse numbers",
        ColorRole::Link => "links",
        ColorRole::NoteMarker => "Note markers",
        ColorRole::Accent => "accents",
        ColorRole::WordsOfJesus => "red letters",
        ColorRole::Interlinear => "interlinear lines",
    }
}
