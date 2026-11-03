//! Page-tone palettes: the only place colour roles become colours (#21 rule 4). Dark, Sepia
//! and theme following arrive with #66; switching a palette never touches layout or the atlas.

use egui::Color32;
use obs_layout::ColorRole;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub page: Color32,
    pub ink: Color32,
    pub verse_number: Color32,
    pub link: Color32,
    pub note_marker: Color32,
    pub accent: Color32,
    pub words_of_jesus: Color32,
    pub selection: Color32,
}

impl Palette {
    /// The Study desk page: cool linen, iron ink, lapis and verdigris accents.
    pub const PAPER: Palette = Palette {
        page: Color32::from_rgb(250, 251, 248),
        ink: Color32::from_rgb(32, 38, 41),
        verse_number: Color32::from_rgb(146, 112, 46),
        link: Color32::from_rgb(45, 78, 134),
        note_marker: Color32::from_rgb(45, 116, 105),
        accent: Color32::from_rgb(45, 78, 134),
        words_of_jesus: Color32::from_rgb(160, 40, 36),
        selection: Color32::from_rgb(211, 224, 244),
    };

    pub const DARK: Palette = Palette {
        page: Color32::from_rgb(0x16, 0x16, 0x18),
        ink: Color32::from_rgb(0xE4, 0xE2, 0xDC),
        verse_number: Color32::from_rgb(0x9A, 0x9A, 0xA4),
        link: Color32::from_rgb(0x8A, 0xB4, 0xF0),
        note_marker: Color32::from_rgb(0x7C, 0xC4, 0x94),
        accent: Color32::from_rgb(0xE0, 0x9A, 0x6E),
        words_of_jesus: Color32::from_rgb(0xF0, 0x82, 0x78),
        selection: Color32::from_rgb(0x2C, 0x48, 0x70),
    };

    pub fn color(&self, role: ColorRole) -> Color32 {
        match role {
            ColorRole::Ink => self.ink,
            ColorRole::VerseNumber => self.verse_number,
            ColorRole::Link => self.link,
            ColorRole::NoteMarker => self.note_marker,
            ColorRole::Accent => self.accent,
            ColorRole::WordsOfJesus => self.words_of_jesus,
        }
    }
}
