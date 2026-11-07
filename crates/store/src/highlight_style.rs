//! Highlight styles and Highlight palettes of the User database (#181, ADR 0012).
//!
//! A Highlight stores only its style's id. A style is a colour with a look and an optional
//! name, in one palette; the colour for each Page tone is derived at draw time unless the user
//! adjusted it. The five styles of v0.1 are the **Standard** palette, seeded by the migration
//! with their old ids, so Highlights made before keep their look.
//!
//! These types compile everywhere (wasm included); only the SQLite code is desktop-only.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Id of the Standard palette, seeded with the five built-in styles.
pub const STANDARD_PALETTE_ID: &str = "standard";
/// Its name when first made. The user may rename it.
pub const STANDARD_PALETTE_NAME: &str = "Standard";

/// Longest style or palette name, in characters.
pub const MAX_STYLE_NAME: usize = 60;

/// An opaque sRGB colour, written `#rrggbb`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb(pub [u8; 3]);

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self([r, g, b])
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [r, g, b] = self.0;
        write!(f, "#{r:02x}{g:02x}{b:02x}")
    }
}

impl FromStr for Rgb {
    type Err = String;

    /// `#rrggbb` (or `rrggbb`), either case.
    fn from_str(s: &str) -> Result<Self, String> {
        let hex = s.trim().trim_start_matches('#');
        let bad = || format!("“{s}” is not a colour (write it as #rrggbb)");
        if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(bad());
        }
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| bad());
        Ok(Self([byte(0)?, byte(2)?, byte(4)?]))
    }
}

impl Serialize for Rgb {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// How a Highlight style marks its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HighlightLook {
    /// A background behind the glyphs.
    Background,
    /// A line under the glyphs.
    Underline,
    /// An outline around the text.
    Box,
    /// The glyphs drawn in the style's colour.
    TextColor,
}

impl HighlightLook {
    pub const ALL: [HighlightLook; 4] = [
        HighlightLook::Background,
        HighlightLook::Underline,
        HighlightLook::Box,
        HighlightLook::TextColor,
    ];

    /// Stable id, as stored.
    pub fn id(self) -> &'static str {
        match self {
            HighlightLook::Background => "background",
            HighlightLook::Underline => "underline",
            HighlightLook::Box => "box",
            HighlightLook::TextColor => "text_color",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.id() == id)
    }

    /// As the UI names it.
    pub fn label(self) -> &'static str {
        match self {
            HighlightLook::Background => "Background",
            HighlightLook::Underline => "Underline",
            HighlightLook::Box => "Box",
            HighlightLook::TextColor => "Text colour",
        }
    }
}

/// What a Highlight style looks like: its colour and look, and the colour the user chose for
/// a Page tone (`None`: derived from `color` for that tone when drawn, ADR 0012).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StyleAppearance {
    pub look: HighlightLook,
    pub color: Rgb,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paper: Option<Rgb>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sepia: Option<Rgb>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dark: Option<Rgb>,
}

impl StyleAppearance {
    pub const fn new(look: HighlightLook, color: Rgb) -> Self {
        Self {
            look,
            color,
            paper: None,
            sepia: None,
            dark: None,
        }
    }
}

/// One look a Highlight is drawn in: a colour with a look and an optional name, in one
/// Highlight palette.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HighlightStyle {
    /// The five built-in styles keep their v0.1 ids (`yellow` …); others are UUIDv7.
    pub id: String,
    pub palette_id: String,
    /// Order within its palette (Alt+1 … Alt+9 follow it); gaps are allowed.
    pub position: i64,
    /// May be empty: the UI then calls it by its place in the palette.
    pub name: String,
    #[serde(flatten)]
    pub appearance: StyleAppearance,
    /// Milliseconds since the Unix epoch (0 for the seeded built-ins).
    pub created_at: i64,
    pub modified_at: i64,
}

/// A named, ordered group of Highlight styles; one is active at a time and the selection
/// toolbar, right-click menu and Alt+1…9 show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HighlightPalette {
    /// `standard` for the seeded one; others are UUIDv7.
    pub id: String,
    pub name: String,
    pub position: i64,
    pub created_at: i64,
    pub modified_at: i64,
}

/// The five built-in styles as the migration seeds them: their Paper colours, and the Sepia
/// and Dark colours v0.1 tuned by hand as adjustments (#21 decision 3).
pub fn built_in_styles() -> Vec<HighlightStyle> {
    let style = |id: &str, position: i64, name: &str, a: StyleAppearance| HighlightStyle {
        id: id.into(),
        palette_id: STANDARD_PALETTE_ID.into(),
        position,
        name: name.into(),
        appearance: a,
        created_at: 0,
        modified_at: 0,
    };
    let bg = |paper: Rgb, sepia: Rgb, dark: Rgb| StyleAppearance {
        sepia: Some(sepia),
        dark: Some(dark),
        ..StyleAppearance::new(HighlightLook::Background, paper)
    };
    vec![
        style(
            "yellow",
            0,
            "Yellow",
            bg(
                Rgb::new(255, 236, 140),
                Rgb::new(248, 224, 128),
                Rgb::new(70, 60, 16),
            ),
        ),
        style(
            "green",
            1,
            "Green",
            bg(
                Rgb::new(200, 238, 190),
                Rgb::new(204, 226, 176),
                Rgb::new(26, 56, 36),
            ),
        ),
        style(
            "blue",
            2,
            "Blue",
            bg(
                Rgb::new(196, 222, 250),
                Rgb::new(196, 214, 232),
                Rgb::new(24, 46, 76),
            ),
        ),
        style(
            "pink",
            3,
            "Pink",
            bg(
                Rgb::new(250, 206, 224),
                Rgb::new(240, 200, 208),
                Rgb::new(72, 32, 50),
            ),
        ),
        style(
            "red_underline",
            4,
            "Red underline",
            StyleAppearance {
                sepia: Some(Rgb::new(184, 40, 32)),
                dark: Some(Rgb::new(240, 120, 120)),
                ..StyleAppearance::new(HighlightLook::Underline, Rgb::new(200, 40, 40))
            },
        ),
    ]
}

/// The Standard palette as the migration seeds it.
pub fn standard_palette() -> HighlightPalette {
    HighlightPalette {
        id: STANDARD_PALETTE_ID.into(),
        name: STANDARD_PALETTE_NAME.into(),
        position: 0,
        created_at: 0,
        modified_at: 0,
    }
}

/// The look of a style id no row has (a database edited by hand): the first built-in's, so
/// the Highlight still shows.
pub fn fallback_appearance() -> StyleAppearance {
    built_in_styles().into_iter().next().map_or_else(
        || StyleAppearance::new(HighlightLook::Background, Rgb::new(255, 236, 140)),
        |s| s.appearance,
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn colours_read_and_write_as_hex() {
        let c: Rgb = "#C9a227".parse().unwrap();
        assert_eq!(c, Rgb::new(0xc9, 0xa2, 0x27));
        assert_eq!(c.to_string(), "#c9a227");
        assert_eq!("c9a227".parse::<Rgb>().unwrap(), c);
        for bad in ["", "#c9a22", "#c9a2278", "gold", "#c9a22g"] {
            assert!(bad.parse::<Rgb>().is_err(), "{bad}");
        }
        assert_eq!(serde_json::to_value(c).unwrap(), "#c9a227");
    }

    #[test]
    fn an_appearance_serializes_without_unadjusted_tones() {
        let a = StyleAppearance {
            dark: Some(Rgb::new(1, 2, 3)),
            ..StyleAppearance::new(HighlightLook::Box, Rgb::new(201, 162, 39))
        };
        let v = serde_json::to_value(a).unwrap();
        assert_eq!(
            v,
            serde_json::json!({ "look": "box", "color": "#c9a227", "dark": "#010203" })
        );
        assert_eq!(serde_json::from_value::<StyleAppearance>(v).unwrap(), a);
    }

    #[test]
    fn the_built_ins_keep_their_v0_1_ids_in_order() {
        let ids: Vec<String> = built_in_styles().into_iter().map(|s| s.id).collect();
        assert_eq!(ids, ["yellow", "green", "blue", "pink", "red_underline"]);
    }
}
