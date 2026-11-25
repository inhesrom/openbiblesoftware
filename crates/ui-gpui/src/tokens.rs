//! Design tokens (ADR 0015) for the GPUI UI: spacing, type sizes, corner radii, strokes,
//! elevations, insets, control sizes and the chrome's colour roles.
//!
//! The values are the egui app's (`obs_ui_egui::tokens` and `obs_ui_egui::Palette`). They are
//! copied, not shared, because those modules are written in egui's types and a GPUI-only build
//! compiles no egui; the one place they could both read from would be below the UI layer,
//! which is outside this crate. `tests/tokens.rs` checks every value here against the egui one,
//! so the two cannot drift: change a value in both places, or the test fails.

use gpui_kit::{BoxShadow, Hsla, Rgba, hsla, point, px};
use obs_engine::Rgb;

/// The spacing scale, in points.
pub mod space {
    pub const PX: f32 = 1.0;
    pub const XXS: f32 = 2.0;
    pub const XS: f32 = 4.0;
    pub const S: f32 = 6.0;
    pub const M: f32 = 8.0;
    pub const L: f32 = 12.0;
    pub const XL: f32 = 16.0;
    pub const XXL: f32 = 24.0;
    pub const XXXL: f32 = 32.0;
}

/// The type scale, in points (the chrome; Resource text has its own size setting).
pub mod text {
    pub const CAPTION: f32 = 11.0;
    pub const SMALL: f32 = 12.0;
    pub const LABEL: f32 = 13.0;
    pub const BODY: f32 = 14.0;
    pub const SUBTITLE: f32 = 16.0;
    pub const TITLE: f32 = 20.0;
    pub const DISPLAY: f32 = 24.0;
    pub const MONO: f32 = 12.0;
    pub const LABEL_LINE: f32 = 18.0;
    pub const CAPTION_LINE: f32 = 15.0;
}

/// Corner radii, in points.
pub mod radius {
    pub const XS: f32 = 2.0;
    pub const S: f32 = 4.0;
    pub const M: f32 = 6.0;
    pub const L: f32 = 8.0;
    pub const XL: f32 = 10.0;
    pub const PILL: f32 = super::size::CHIP / 2.0;
}

/// Stroke widths, in points.
pub mod stroke {
    pub const HAIR: f32 = 1.0;
    pub const MEDIUM: f32 = 1.5;
    pub const THICK: f32 = 2.0;
    /// The keyboard focus ring in a High contrast theme (#224).
    pub const HEAVY: f32 = 3.0;
}

/// Opacity factors for washes and tints.
pub mod alpha {
    pub const WASH: f32 = 0.12;
    pub const TINT: f32 = 0.2;
    pub const HALF: f32 = 0.5;
    pub const SOFT: f32 = 0.6;
    pub const STRONG: f32 = 0.75;
}

/// Control and row sizes, in points: the ones the shared components use.
pub mod size {
    pub const ICON_SMALL: f32 = 12.0;
    pub const ICON: f32 = 16.0;
    pub const ICON_LARGE: f32 = 18.0;
    pub const CONTROL: f32 = 28.0;
    pub const CONTROL_COMPACT: f32 = 22.0;
    pub const CHIP: f32 = 22.0;
    pub const ROW_COMPACT: f32 = 24.0;
    pub const ROW: f32 = 28.0;
    pub const TAB_BAR: f32 = 31.0;
    pub const SCROLLBAR: f32 = 10.0;
    pub const THUMB_MIN: f32 = 24.0;
    pub const FIELD_NARROW: f32 = 120.0;
    pub const FIELD: f32 = 180.0;
    pub const FIELD_WIDE: f32 = 280.0;
    pub const PROSE: f32 = 460.0;
    pub const POPOVER: f32 = 360.0;
    pub const TOAST_RISE: f32 = super::space::XXXL + super::space::XL;
    pub const DIALOG_WIDTH: f32 = 480.0;
    pub const MENU_COLUMN: f32 = 160.0;
    pub const PAGE_MAX: f32 = 720.0;
}

/// Insets of frames, as (horizontal, vertical) points.
pub mod inset {
    use super::space;
    pub const PAGE: (f32, f32) = (space::XL, space::L);
    pub const CARD: (f32, f32) = (space::L, space::L);
    pub const NOTICE: (f32, f32) = (space::L, space::S);
    pub const POPOVER: (f32, f32) = (space::M, space::M);
    pub const WINDOW: (f32, f32) = (space::L, space::L);
    pub const BAR: (f32, f32) = (space::M, space::XS);
    pub const CHIP: (f32, f32) = (space::S, 0.0);
    pub const BAND: (f32, f32) = (space::L, space::XS);
    pub const MENU_BAR: (f32, f32) = (space::L, space::S);
    pub const WORKSPACE: (f32, f32) = (space::M, space::M);
}

/// Elevation: the shadows of things that float over the Workspace, as egui's `Shadow`
/// (offset y, blur, black alpha out of 255 in a light and a dark theme).
pub mod elevation {
    /// A window: offset 8, blur 24, alpha 34 light / 90 dark.
    pub const WINDOW: (f32, f32, u8, u8) = (8.0, 24.0, 34, 90);
    /// A popover, menu or Launcher pane: offset 6, blur 18, alpha 30 light / 80 dark.
    pub const POPUP: (f32, f32, u8, u8) = (6.0, 18.0, 30, 80);
}

/// A shadow from an [`elevation`] token.
pub fn shadow(token: (f32, f32, u8, u8), dark: bool) -> Vec<BoxShadow> {
    let (y, blur, light_alpha, dark_alpha) = token;
    let a = if dark { dark_alpha } else { light_alpha };
    vec![BoxShadow {
        color: hsla(0.0, 0.0, 0.0, f32::from(a) / 255.0),
        offset: point(px(0.0), px(y)),
        blur_radius: px(blur),
        spread_radius: px(0.0),
        inset: false,
    }]
}

/// Motion (#225): how long each kind of movement takes, in seconds.
pub mod motion {
    pub const POPOVER: f64 = 0.12;
    pub const POPOVER_SCALE: f32 = 0.96;
    pub const PANEL: f64 = 0.18;
    pub const JUMP: f64 = 0.28;
    pub const SCROLL: f64 = 0.14;
    pub const FLASH: f64 = 1.2;
    pub const SHIMMER: f64 = 1.4;
}

/// A colour of a token, as GPUI draws it.
pub fn rgba(c: Rgb) -> Rgba {
    let [r, g, b] = c.0;
    Rgba {
        r: f32::from(r) / 255.0,
        g: f32::from(g) / 255.0,
        b: f32::from(b) / 255.0,
        a: 1.0,
    }
}

/// A colour of a token with an opacity factor (an [`alpha`] token).
pub fn with_alpha(c: Rgb, a: f32) -> Hsla {
    let mut h: Hsla = rgba(c).into();
    h.a = a;
    h
}

/// The chrome's colour roles in one theme (the egui app's `tokens::Chrome`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chrome {
    pub dark: bool,
    pub high_contrast: bool,
    pub shell: Rgb,
    pub surface: Rgb,
    pub field: Rgb,
    pub rule: Rgb,
    pub ink: Rgb,
    pub muted: Rgb,
    pub lapis: Rgb,
    pub verdigris: Rgb,
    pub violet: Rgb,
    pub gilt: Rgb,
    pub selection: Rgb,
    pub hover: Rgb,
    pub hover_rule: Rgb,
    pub warn: Rgb,
    pub unlinked: Rgb,
}

const fn c(r: u8, g: u8, b: u8) -> Rgb {
    Rgb::new(r, g, b)
}

/// Cool linen surfaces, iron ink, lapis and verdigris (the Study desk, #13).
pub const LIGHT: Chrome = Chrome {
    dark: false,
    high_contrast: false,
    shell: c(232, 236, 235),
    surface: c(244, 246, 244),
    field: c(255, 255, 253),
    rule: c(205, 213, 211),
    ink: c(32, 38, 41),
    muted: c(80, 91, 96),
    lapis: c(45, 78, 134),
    verdigris: c(36, 104, 94),
    violet: c(112, 79, 145),
    gilt: c(150, 112, 30),
    selection: c(211, 224, 244),
    hover: c(226, 235, 247),
    hover_rule: c(166, 184, 211),
    warn: c(150, 60, 30),
    unlinked: c(154, 162, 164),
};

/// The same desk at night.
pub const DARK: Chrome = Chrome {
    dark: true,
    high_contrast: false,
    shell: c(24, 27, 30),
    surface: c(33, 37, 41),
    field: c(40, 45, 50),
    rule: c(58, 64, 70),
    ink: c(222, 226, 228),
    muted: c(160, 170, 176),
    lapis: c(132, 168, 226),
    verdigris: c(96, 184, 168),
    violet: c(160, 128, 200),
    gilt: c(206, 168, 88),
    selection: c(28, 44, 74),
    hover: c(48, 56, 66),
    hover_rule: c(84, 104, 136),
    warn: c(232, 140, 100),
    unlinked: c(104, 112, 118),
};

/// High contrast on white (#224).
pub const HIGH_CONTRAST_LIGHT: Chrome = Chrome {
    dark: false,
    high_contrast: true,
    shell: c(242, 242, 242),
    surface: c(255, 255, 255),
    field: c(255, 255, 255),
    rule: c(64, 64, 64),
    ink: c(0, 0, 0),
    muted: c(51, 51, 51),
    lapis: c(0, 51, 153),
    verdigris: c(0, 70, 60),
    violet: c(80, 0, 130),
    gilt: c(110, 80, 0),
    selection: c(210, 228, 255),
    hover: c(230, 240, 255),
    hover_rule: c(0, 0, 0),
    warn: c(130, 20, 0),
    unlinked: c(70, 70, 70),
};

/// High contrast on black (#224).
pub const HIGH_CONTRAST_DARK: Chrome = Chrome {
    dark: true,
    high_contrast: true,
    shell: c(0, 0, 0),
    surface: c(18, 18, 18),
    field: c(28, 28, 28),
    rule: c(180, 180, 180),
    ink: c(255, 255, 255),
    muted: c(220, 220, 220),
    lapis: c(160, 200, 255),
    verdigris: c(110, 220, 190),
    violet: c(200, 170, 255),
    gilt: c(240, 200, 80),
    selection: c(20, 40, 100),
    hover: c(40, 48, 64),
    hover_rule: c(255, 255, 255),
    warn: c(255, 170, 140),
    unlinked: c(150, 150, 150),
};

/// The page's colours in one Page tone (the egui app's `Palette`: page and ink, which the
/// chrome's reading surfaces use; the Highlight tones stay with the reading pane, #380).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    pub page: Rgb,
    pub ink: Rgb,
    pub verse_number: Rgb,
    pub link: Rgb,
}

pub const PAPER: Page = Page {
    page: c(250, 251, 248),
    ink: c(32, 38, 41),
    verse_number: c(110, 82, 26),
    link: c(38, 68, 122),
};

pub const SEPIA: Page = Page {
    page: c(244, 236, 216),
    ink: c(56, 44, 30),
    verse_number: c(104, 72, 24),
    link: c(40, 66, 112),
};

pub const PAGE_DARK: Page = Page {
    page: c(22, 22, 24),
    ink: c(228, 226, 220),
    verse_number: c(214, 192, 142),
    link: c(164, 198, 248),
};

pub const PAPER_HIGH_CONTRAST: Page = Page {
    page: c(255, 255, 255),
    ink: c(0, 0, 0),
    verse_number: c(100, 60, 0),
    link: c(0, 51, 153),
};

pub const SEPIA_HIGH_CONTRAST: Page = Page {
    page: c(250, 240, 215),
    ink: c(30, 20, 0),
    verse_number: c(90, 50, 0),
    link: c(0, 45, 130),
};

pub const DARK_HIGH_CONTRAST: Page = Page {
    page: c(0, 0, 0),
    ink: c(255, 255, 255),
    verse_number: c(240, 210, 150),
    link: c(170, 210, 255),
};

/// The page of a resolved Page tone (`Auto` reads as Paper), High contrast or not (the egui
/// app's `Palette::for_theme`).
pub fn page_for(tone: obs_engine::PageTone, high_contrast: bool) -> Page {
    use obs_engine::PageTone;
    match (tone, high_contrast) {
        (PageTone::Sepia, false) => SEPIA,
        (PageTone::Dark, false) => PAGE_DARK,
        (PageTone::Paper | PageTone::Auto, false) => PAPER,
        (PageTone::Sepia, true) => SEPIA_HIGH_CONTRAST,
        (PageTone::Dark, true) => DARK_HIGH_CONTRAST,
        (PageTone::Paper | PageTone::Auto, true) => PAPER_HIGH_CONTRAST,
    }
}

/// The window the app opens at (points), as the egui app's `main_viewport`.
pub const WINDOW_WIDTH: f32 = 1280.0;
pub const WINDOW_HEIGHT: f32 = 800.0;

/// The title bar's height (points): `size::CONTROL` plus the menu bar's vertical inset.
pub const TITLE_BAR_HEIGHT: f32 = size::CONTROL + 2.0 * inset::MENU_BAR.1;
