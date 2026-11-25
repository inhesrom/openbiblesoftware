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
