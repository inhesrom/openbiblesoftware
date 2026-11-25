//! The colours and sizes the scaffold draws with. PLACEHOLDER until #378: these are values of
//! the egui app's tokens (`obs_ui_egui::tokens`: `LIGHT`, `space`, `text`, `size`, ADR 0015),
//! copied so a GPUI-only build compiles no egui. #378 moves the tokens below the UI layer, and
//! both UIs then read one set.

use gpui_kit::{Rgba, rgb};

/// `Chrome::shell`: the ground behind the Panels.
pub fn shell() -> Rgba {
    rgb(0xe8eceb)
}

/// `Chrome::surface`: bars and Tab pages.
pub fn surface() -> Rgba {
    rgb(0xf4f6f4)
}

/// `Chrome::rule`: dividers and outlines.
pub fn rule() -> Rgba {
    rgb(0xcdd5d3)
}

/// `Chrome::ink`: text.
pub fn ink() -> Rgba {
    rgb(0x202629)
}

/// `Chrome::muted`: secondary text.
pub fn muted() -> Rgba {
    rgb(0x505b60)
}

/// `Chrome::hover`: a control under the pointer.
pub fn hover() -> Rgba {
    rgb(0xe2ebf7)
}

/// The chrome's colour roles in one theme: the egui app's `Chrome` (`obs_ui_egui::tokens`),
/// the roles the scaffold draws with. PLACEHOLDER until #378, which brings every role, the
/// High contrast pair and Custom themes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Chrome {
    pub dark: bool,
    /// The ground behind the Panels.
    pub shell: Rgba,
    /// Bars and Tab pages.
    pub surface: Rgba,
    /// Dividers and outlines.
    pub rule: Rgba,
    /// Text.
    pub ink: Rgba,
    /// Secondary text.
    pub muted: Rgba,
    /// A control under the pointer.
    pub hover: Rgba,
}

/// `tokens::LIGHT`: cool linen surfaces and iron ink.
pub fn light() -> Chrome {
    Chrome {
        dark: false,
        shell: shell(),
        surface: surface(),
        rule: rule(),
        ink: ink(),
        muted: muted(),
        hover: hover(),
    }
}

/// `tokens::DARK`: slate surfaces and light ink.
pub fn dark() -> Chrome {
    Chrome {
        dark: true,
        shell: rgb(0x181b1e),
        surface: rgb(0x212529),
        rule: rgb(0x3a4046),
        ink: rgb(0xdee2e4),
        muted: rgb(0xa0aab0),
        hover: rgb(0x303842),
    }
}

/// `space::S`, `space::M`, `space::L`, `space::XL` (points).
pub const SPACE_S: f32 = 6.0;
pub const SPACE_M: f32 = 8.0;
pub const SPACE_L: f32 = 12.0;
pub const SPACE_XL: f32 = 16.0;

/// `text::BODY` and the title bar's wordmark size (points).
pub const TEXT_BODY: f32 = 14.0;
pub const TEXT_WORDMARK: f32 = 20.0;

/// The title bar's height (points).
pub const TITLE_BAR_HEIGHT: f32 = 40.0;

/// `radius::S` (points).
pub const RADIUS_CONTROL: f32 = 4.0;

/// The window the app opens at (points), as the egui app's `main_viewport`.
pub const WINDOW_WIDTH: f32 = 1280.0;
pub const WINDOW_HEIGHT: f32 = 800.0;
