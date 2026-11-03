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
}

impl Palette {
    pub const PAPER: Palette = Palette {
        page: Color32::from_rgb(0xFB, 0xF8, 0xF1),
        ink: Color32::from_rgb(0x1C, 0x1C, 0x1E),
        verse_number: Color32::from_rgb(0x6E, 0x6E, 0x78),
        link: Color32::from_rgb(0x1E, 0x50, 0xAA),
        note_marker: Color32::from_rgb(0x28, 0x78, 0x46),
        accent: Color32::from_rgb(0x96, 0x3C, 0x1E),
        words_of_jesus: Color32::from_rgb(0xA8, 0x1E, 0x1E),
    };

    pub const DARK: Palette = Palette {
        page: Color32::from_rgb(0x16, 0x16, 0x18),
        ink: Color32::from_rgb(0xE4, 0xE2, 0xDC),
        verse_number: Color32::from_rgb(0x9A, 0x9A, 0xA4),
        link: Color32::from_rgb(0x8A, 0xB4, 0xF0),
        note_marker: Color32::from_rgb(0x7C, 0xC4, 0x94),
        accent: Color32::from_rgb(0xE0, 0x9A, 0x6E),
        words_of_jesus: Color32::from_rgb(0xF0, 0x82, 0x78),
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
