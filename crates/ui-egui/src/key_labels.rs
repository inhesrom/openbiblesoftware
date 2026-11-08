//! Shortcuts as people read them (#262). Keys are kept in the engine's portable notation
//! (`Ctrl+Comma`, Ctrl meaning Cmd on macOS); everywhere the interface shows one (menus,
//! Launchers, the Command box, tooltips, hints, Keyboard shortcuts) it goes through [`label`],
//! which writes it the way the platform does: `Ctrl+,` on Windows and Linux, `⌘,` on macOS
//! ([`obs_engine::key_bindings::key_label`]).
//!
//! The platform is the context's [`egui::os::OperatingSystem`] (so the web build on a Mac
//! shows ⌘ too), taken once a frame by [`set_for`], as the chrome palette is.
//!
//! None of the interface fonts has all of Apple's modifier symbols (⌥ is in none), so ⌘ ⌥ ⇧ ⌃
//! are drawn from the icon set (Lucide's command, option, arrow-big-up and chevron-up) by a
//! glyph rasterizer, in every font family, in the text's own colour and size.

use std::cell::Cell;

use egui::{FontPriority, GlyphBitmap, GlyphRasterizer, GlyphRasterizerRequest, RasterizedGlyph};
use obs_engine::key_bindings::{KeyStyle, key_label};

use crate::icons::{self, Icon};

thread_local! {
    static STYLE: Cell<KeyStyle> = const { Cell::new(KeyStyle::Pc) };
}

/// Write keys for the platform of `ctx` this frame.
pub fn set_for(ctx: &egui::Context) {
    let style = if ctx.os().is_mac() {
        KeyStyle::Mac
    } else {
        KeyStyle::Pc
    };
    STYLE.with(|s| s.set(style));
}

/// How keys are written this frame.
pub fn style() -> KeyStyle {
    STYLE.with(Cell::get)
}

/// `key` (engine notation, `Ctrl+Shift+N`) as the platform writes it (`⇧⌘N` on macOS).
pub fn label(key: &str) -> String {
    key_label(key, style())
}

/// The icon that draws a modifier symbol, if `cluster` is one.
fn modifier_icon(cluster: &str) -> Option<Icon> {
    let mut chars = cluster.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return None;
    };
    Some(match c {
        '⌘' => Icon::CommandKey,
        '⌥' => Icon::OptionKey,
        '⇧' => Icon::ShiftKey,
        '⌃' => Icon::ChevronUp,
        _ => return None,
    })
}

/// A modifier symbol at `font_size_px`: the icon's 24-unit square at 0.92 em, so a full-height
/// drawing (3 to 21) stands on the baseline about as tall as a capital letter.
fn modifier_glyph(icon: Icon, font_size_px: f32) -> RasterizedGlyph {
    let side = (font_size_px * 0.92).round().max(4.0);
    let unit = side / 24.0;
    let bearing = (font_size_px * 0.06).round();
    RasterizedGlyph {
        bitmap: GlyphBitmap {
            image: icons::rasterize(icon, side as u32),
            // The pen is on the baseline; the drawing's left edge (3 units in) is one bearing
            // after it.
            offset_px: egui::vec2(bearing - (3.0 * unit).round(), -(21.0 * unit).round()),
            is_color: false,
        },
        advance_px: (18.0 * unit).round() + 2.0 * bearing,
    }
}

/// The rasterizer that draws ⌘ ⌥ ⇧ ⌃ in every family, ahead of any font.
pub fn modifier_rasterizer() -> GlyphRasterizer {
    GlyphRasterizer::new(
        "obs::key_labels::modifiers",
        |r: &GlyphRasterizerRequest<'_>| {
            modifier_icon(r.cluster).map(|icon| modifier_glyph(icon, r.font_size_px))
        },
    )
    .with_priority(FontPriority::Highest)
}
