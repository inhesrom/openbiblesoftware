//! Page-tone palettes in egui's colours: [`obs_reading::TonePalette`] (the tone constants and
//! how each tone draws a Highlight style, ADR 0012), with every colour a [`Color32`] for the
//! painter. Each method converts and asks the tone palette, so the colours are the same ones
//! the reading core works out.

use egui::Color32;
use obs_engine::{HighlightLook, PageTone, Rgb, StyleAppearance, StyleTone};
use obs_layout::ColorRole;
use obs_reading::palette::TonePalette;

pub use obs_reading::palette::{AAA_TEXT_CONTRAST, ContrastShort, MARK_CONTRAST, TEXT_CONTRAST};

/// A Highlight style as one Page tone draws it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StylePaint {
    pub look: HighlightLook,
    /// The background, line, box or text colour.
    pub color: Color32,
    /// Its margin mark beside a verse number (#63), and its swatch on the chrome.
    pub mark: Color32,
}

impl From<obs_reading::StylePaint> for StylePaint {
    fn from(p: obs_reading::StylePaint) -> Self {
        StylePaint {
            look: p.look,
            color: to_color(p.color),
            mark: to_color(p.mark),
        }
    }
}

pub const fn to_color(c: Rgb) -> Color32 {
    let [r, g, b] = c.0;
    Color32::from_rgb(r, g, b)
}

pub fn to_rgb(c: Color32) -> Rgb {
    Rgb::new(c.r(), c.g(), c.b())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// The tone this is the palette of (Auto resolved).
    pub tone: StyleTone,
    /// The tone as a High contrast theme draws it (#224): its text reads at WCAG AAA (7:1)
    /// on the page and every overlay, and Highlight colours are derived to keep it so.
    pub high_contrast: bool,
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

impl Palette {
    /// Cool linen paper with iron ink (the Study desk page).
    pub const PAPER: Palette = Palette::from_tone(TonePalette::PAPER);
    /// Warm sepia paper with brown ink.
    pub const SEPIA: Palette = Palette::from_tone(TonePalette::SEPIA);
    /// Near-black page with warm light ink.
    pub const DARK: Palette = Palette::from_tone(TonePalette::DARK);
    /// White paper with black ink: Paper in a High contrast theme (#224).
    pub const PAPER_HIGH_CONTRAST: Palette = Palette::from_tone(TonePalette::PAPER_HIGH_CONTRAST);
    /// Sepia paper with near-black ink: Sepia in a High contrast theme.
    pub const SEPIA_HIGH_CONTRAST: Palette = Palette::from_tone(TonePalette::SEPIA_HIGH_CONTRAST);
    /// Black page with white ink: Dark in a High contrast theme.
    pub const DARK_HIGH_CONTRAST: Palette = Palette::from_tone(TonePalette::DARK_HIGH_CONTRAST);

    /// `t` in egui's colours (`const`, for the tone constants above).
    const fn from_tone(t: TonePalette) -> Palette {
        Palette {
            tone: t.tone,
            high_contrast: t.high_contrast,
            page: to_color(t.page),
            ink: to_color(t.ink),
            verse_number: to_color(t.verse_number),
            link: to_color(t.link),
            note_marker: to_color(t.note_marker),
            accent: to_color(t.accent),
            words_of_jesus: to_color(t.words_of_jesus),
            underline: to_color(t.underline),
            selection: to_color(t.selection),
            hover: to_color(t.hover),
            search_hit: to_color(t.search_hit),
            find_match: to_color(t.find_match),
            find_current: to_color(t.find_current),
            flash: to_color(t.flash),
        }
    }

    /// This palette as the reading core's, whose methods every one here asks.
    fn tone_palette(&self) -> TonePalette {
        TonePalette::from(*self)
    }

    /// The palette of a resolved tone in a theme that is High contrast or not (#224).
    pub fn for_theme(tone: PageTone, high_contrast: bool) -> Palette {
        TonePalette::for_theme(tone, high_contrast).into()
    }

    /// What text needs against what it is drawn on in this palette: WCAG AA, or AAA in a High
    /// contrast theme.
    pub fn text_contrast(&self) -> f32 {
        self.tone_palette().text_contrast()
    }

    /// The palette of a resolved tone (`Auto` must be resolved first; it reads as Paper).
    pub fn for_tone(tone: PageTone) -> Palette {
        TonePalette::for_tone(tone).into()
    }

    /// The palette of a style tone in a theme that is High contrast or not (#224).
    pub fn for_style_tone(tone: StyleTone, high_contrast: bool) -> Palette {
        TonePalette::for_style_tone(tone, high_contrast).into()
    }

    /// Whether `color` can be a style's colour with `look` in this tone:
    /// [`TonePalette::check`].
    pub fn check(&self, look: HighlightLook, color: Color32) -> Result<(), ContrastShort> {
        self.tone_palette().check(look, to_rgb(color))
    }

    /// `base` made to pass [`Palette::check`] for `look` here: [`TonePalette::derive`].
    pub fn derive(&self, look: HighlightLook, base: Color32) -> Color32 {
        to_color(self.tone_palette().derive(look, to_rgb(base)))
    }

    /// A text-colour Highlight's `color` as drawn on `ground`: [`TonePalette::text_on`].
    pub fn text_on(&self, color: Color32, ground: Color32) -> Color32 {
        to_color(self.tone_palette().text_on(to_rgb(color), to_rgb(ground)))
    }

    /// A style's colours in this tone: [`TonePalette::paint`].
    pub fn paint(&self, a: &StyleAppearance) -> StylePaint {
        self.tone_palette().paint(a).into()
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
        to_color(self.tone_palette().color(role))
    }

    /// Every background text can sit on in this tone apart from Highlights: the page and
    /// each overlay.
    pub fn overlays(&self) -> [(&'static str, Color32); 6] {
        self.tone_palette()
            .overlays()
            .map(|(name, c)| (name, to_color(c)))
    }

    /// Every text colour of this tone, by role.
    pub fn inks(&self) -> [(ColorRole, Color32); 7] {
        self.tone_palette()
            .inks()
            .map(|(role, c)| (role, to_color(c)))
    }
}

impl From<TonePalette> for Palette {
    fn from(t: TonePalette) -> Self {
        Palette::from_tone(t)
    }
}

impl From<Palette> for TonePalette {
    fn from(p: Palette) -> Self {
        TonePalette {
            tone: p.tone,
            high_contrast: p.high_contrast,
            page: to_rgb(p.page),
            ink: to_rgb(p.ink),
            verse_number: to_rgb(p.verse_number),
            link: to_rgb(p.link),
            note_marker: to_rgb(p.note_marker),
            accent: to_rgb(p.accent),
            words_of_jesus: to_rgb(p.words_of_jesus),
            underline: to_rgb(p.underline),
            selection: to_rgb(p.selection),
            hover: to_rgb(p.hover),
            search_hit: to_rgb(p.search_hit),
            find_match: to_rgb(p.find_match),
            find_current: to_rgb(p.find_current),
            flash: to_rgb(p.flash),
        }
    }
}

/// WCAG 2 contrast ratio of two opaque colours (1–21): [`obs_reading::contrast`].
pub fn contrast(a: Color32, b: Color32) -> f32 {
    obs_reading::contrast(to_rgb(a), to_rgb(b))
}
