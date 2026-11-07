//! The Study desk visual system for the chrome (chosen in #13's polish follow-up): cool linen
//! surfaces, iron ink, lapis navigation, verdigris study accents and Gentium display headings.
//! The colours and measurements themselves are design tokens ([`crate::tokens`], #221);
//! this module installs them as egui's style and says which theme is current. Resource text
//! colours live in [`crate::Palette`].

use std::sync::Arc;

use egui::{Color32, FontFamily, FontId, Stroke, TextStyle};
use obs_fonts::FaceId;

use crate::tokens::{self, corner, elevation, inset, radius, size, stroke, text};

pub use crate::tokens::{Chrome, DARK, LIGHT};

thread_local! {
    static CURRENT: std::cell::Cell<Chrome> = const { std::cell::Cell::new(LIGHT) };
}

/// The chrome palette of the frame being drawn.
pub fn c() -> Chrome {
    CURRENT.with(std::cell::Cell::get)
}

/// Set the chrome palette for this frame (from the context's current theme).
pub fn set_for(ctx: &egui::Context) {
    let chrome = if ctx.theme() == egui::Theme::Dark {
        DARK
    } else {
        LIGHT
    };
    CURRENT.with(|cur| cur.set(chrome));
}

/// The text colour for a label on a solid `fill` (a Link set chip, a badge, a heat cell):
/// white or near-black, whichever reads better, and black when neither meets WCAG AA (#182).
pub fn text_on(fill: Color32) -> Color32 {
    let contrast = |c| crate::palette::contrast(c, fill);
    let best = if contrast(tokens::INK_WHITE) >= contrast(LIGHT.ink) {
        tokens::INK_WHITE
    } else {
        LIGHT.ink
    };
    if contrast(best) < crate::palette::TEXT_CONTRAST
        && contrast(tokens::INK_BLACK) > contrast(best)
    {
        tokens::INK_BLACK
    } else {
        best
    }
}

/// Use `chrome` for the frame being drawn on this thread, as [`set_for`] does from a context.
#[cfg(test)]
pub(crate) fn set_for_test(chrome: Chrome) {
    CURRENT.with(|cur| cur.set(chrome));
}

/// Each Link set's colour.
pub fn link_color(link: Option<char>) -> Color32 {
    let ch = c();
    match link {
        Some('A') => ch.lapis,
        Some('B') => ch.verdigris,
        Some('C') => ch.violet,
        Some(_) => ch.gilt,
        None => ch.unlinked,
    }
}

const DISPLAY: &str = "display";

/// Gentium Plus Bold, for headings and the app name.
pub fn display(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(DISPLAY.into()))
}

/// Install the fonts and the Study desk style on a context.
pub fn install(ctx: &egui::Context) {
    install_fonts(ctx, None);
    ctx.style_mut_of(egui::Theme::Light, |s| style(s, &LIGHT));
    ctx.style_mut_of(egui::Theme::Dark, |s| style(s, &DARK));
}

const INTERFACE: &str = "interface";

const FALLBACK: &str = "gentium-regular";

/// Install the chrome's fonts: egui's defaults, Gentium Plus Bold for display headings and,
/// when one is chosen (View → Fonts, #138), the interface font ahead of egui's proportional
/// fonts. Those stay behind it as fallbacks, so emoji and symbols still draw. Gentium Plus
/// Regular comes last in the interface and monospace families, for what egui's fonts lack:
/// arrows (→ in "Edit → Undo", #262), polytonic Greek, IPA. The modifier symbols of shortcuts
/// (⌘ ⌥ ⇧ ⌃) are drawn by [`crate::key_labels::modifier_rasterizer`].
pub fn install_fonts(ctx: &egui::Context, interface: Option<egui::FontData>) {
    let mut fonts = egui::FontDefinitions::default();
    if let Some(bold) = FaceId::Bold.bundled_bytes() {
        fonts.font_data.insert(
            "gentium-bold".into(),
            Arc::new(egui::FontData::from_static(bold)),
        );
    }
    if let Some(regular) = FaceId::Regular.bundled_bytes() {
        fonts.font_data.insert(
            FALLBACK.into(),
            Arc::new(egui::FontData::from_static(regular)),
        );
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .push(FALLBACK.into());
        }
    }
    if let Some(data) = interface {
        fonts.font_data.insert(INTERFACE.into(), Arc::new(data));
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, INTERFACE.into());
    }
    let mut family = vec!["gentium-bold".to_string()];
    if let Some(p) = fonts.families.get(&FontFamily::Proportional) {
        family.extend(p.iter().filter(|f| *f != INTERFACE).cloned());
    }
    fonts
        .families
        .insert(FontFamily::Name(DISPLAY.into()), family);
    ctx.set_fonts(fonts);
    ctx.add_glyph_rasterizer(crate::key_labels::modifier_rasterizer());
}

fn style(style: &mut egui::Style, ch: &Chrome) {
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(text::BODY));
    style
        .text_styles
        .insert(TextStyle::Button, FontId::proportional(text::BODY));
    style
        .text_styles
        .insert(TextStyle::Small, FontId::proportional(text::SMALL));
    style
        .text_styles
        .insert(TextStyle::Heading, display(text::TITLE));
    style
        .text_styles
        .insert(TextStyle::Monospace, FontId::monospace(text::MONO));
    style.spacing.item_spacing = tokens::ITEM_SPACING;
    style.spacing.button_padding = tokens::BUTTON_PADDING;
    style.spacing.interact_size.y = size::CONTROL;
    style.spacing.menu_margin = inset::POPOVER;
    style.spacing.window_margin = inset::WINDOW;

    let mut v = if ch.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.panel_fill = ch.shell;
    v.window_fill = ch.field;
    v.extreme_bg_color = ch.field;
    v.faint_bg_color = ch.surface;
    v.code_bg_color = ch.surface;
    v.hyperlink_color = ch.lapis;
    // Hints and shortcuts: egui's default fades the ink, below AA on the fields (#182).
    v.weak_text_color = Some(ch.muted);
    v.selection.bg_fill = ch.selection;
    v.selection.stroke = Stroke::new(stroke::HAIR, ch.lapis);
    v.window_stroke = Stroke::new(stroke::HAIR, ch.rule);
    v.window_corner_radius = corner(radius::XL);
    v.menu_corner_radius = corner(radius::L);
    v.window_shadow = elevation::window(ch.dark);
    v.popup_shadow = elevation::popup(ch.dark);
    let control = corner(radius::M);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = ch.surface;
    w.noninteractive.weak_bg_fill = ch.surface;
    w.noninteractive.bg_stroke = Stroke::new(stroke::HAIR, ch.rule);
    w.noninteractive.fg_stroke = Stroke::new(stroke::HAIR, ch.ink);
    w.noninteractive.corner_radius = control;
    w.inactive.bg_fill = ch.field;
    w.inactive.weak_bg_fill = Color32::TRANSPARENT;
    w.inactive.bg_stroke = Stroke::new(stroke::HAIR, ch.rule);
    w.inactive.fg_stroke = Stroke::new(stroke::HAIR, ch.ink);
    w.inactive.corner_radius = control;
    w.hovered.bg_fill = ch.hover;
    w.hovered.weak_bg_fill = ch.hover;
    w.hovered.bg_stroke = Stroke::new(stroke::HAIR, ch.hover_rule);
    w.hovered.fg_stroke = Stroke::new(stroke::HAIR, ch.ink);
    w.hovered.corner_radius = control;
    w.hovered.expansion = 0.0;
    w.active.bg_fill = ch.selection;
    w.active.weak_bg_fill = ch.selection;
    w.active.bg_stroke = Stroke::new(stroke::HAIR, ch.lapis);
    w.active.fg_stroke = Stroke::new(stroke::MEDIUM, ch.lapis);
    w.active.corner_radius = control;
    w.active.expansion = 0.0;
    w.open = w.active;
    style.visuals = v;
}
