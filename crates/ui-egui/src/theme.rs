//! The Study desk visual system for the chrome (chosen in #13's polish follow-up): cool linen
//! surfaces, iron ink, lapis navigation, verdigris study accents and Gentium display headings.
//! Resource text colours live in [`crate::Palette`]; dark chrome arrives with #66.

use std::sync::Arc;

use egui::{Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle};
use obs_fonts::FaceId;

/// The chrome's colours in one theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chrome {
    pub dark: bool,
    pub shell: Color32,
    pub surface: Color32,
    pub field: Color32,
    pub rule: Color32,
    pub ink: Color32,
    pub muted: Color32,
    pub lapis: Color32,
    pub verdigris: Color32,
    pub violet: Color32,
    pub gilt: Color32,
    pub selection: Color32,
    pub hover: Color32,
    pub hover_rule: Color32,
    pub warn: Color32,
    /// The link chip of a Tab with no Link set.
    pub unlinked: Color32,
}

/// Cool linen surfaces, iron ink, lapis and verdigris (the Study desk, #13).
pub const LIGHT: Chrome = Chrome {
    dark: false,
    shell: Color32::from_rgb(232, 236, 235),
    surface: Color32::from_rgb(244, 246, 244),
    field: Color32::from_rgb(255, 255, 253),
    rule: Color32::from_rgb(205, 213, 211),
    ink: Color32::from_rgb(32, 38, 41),
    // Dark enough for AA on the selection as well as on the surfaces (#182).
    muted: Color32::from_rgb(80, 91, 96),
    lapis: Color32::from_rgb(45, 78, 134),
    // AA as text on the selection too (#182).
    verdigris: Color32::from_rgb(36, 104, 94),
    violet: Color32::from_rgb(112, 79, 145),
    // Deep enough for the ★ to show on every surface, 3:1 (#182).
    gilt: Color32::from_rgb(150, 112, 30),
    selection: Color32::from_rgb(211, 224, 244),
    hover: Color32::from_rgb(226, 235, 247),
    hover_rule: Color32::from_rgb(166, 184, 211),
    warn: Color32::from_rgb(150, 60, 30),
    unlinked: Color32::from_rgb(154, 162, 164),
};

/// The same desk at night: slate surfaces, light ink, brighter accents.
pub const DARK: Chrome = Chrome {
    dark: true,
    shell: Color32::from_rgb(24, 27, 30),
    surface: Color32::from_rgb(33, 37, 41),
    field: Color32::from_rgb(40, 45, 50),
    rule: Color32::from_rgb(58, 64, 70),
    ink: Color32::from_rgb(222, 226, 228),
    // AA on the hover and selection fills too (#182).
    muted: Color32::from_rgb(160, 170, 176),
    // AA as text on the hover and selection fills too (#182).
    lapis: Color32::from_rgb(132, 168, 226),
    // AA as text on the hover and selection fills too (#182).
    verdigris: Color32::from_rgb(96, 184, 168),
    violet: Color32::from_rgb(160, 128, 200),
    gilt: Color32::from_rgb(206, 168, 88),
    // Deep enough that lapis and muted text on it meet AA (#182).
    selection: Color32::from_rgb(28, 44, 74),
    hover: Color32::from_rgb(48, 56, 66),
    hover_rule: Color32::from_rgb(84, 104, 136),
    warn: Color32::from_rgb(232, 140, 100),
    unlinked: Color32::from_rgb(104, 112, 118),
};

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
    let best = if contrast(Color32::WHITE) >= contrast(LIGHT.ink) {
        Color32::WHITE
    } else {
        LIGHT.ink
    };
    if contrast(best) < 4.5 && contrast(Color32::BLACK) > contrast(best) {
        Color32::BLACK
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

/// Install the chrome's fonts: egui's defaults, Gentium Plus Bold for display headings and,
/// when one is chosen (View → Fonts, #138), the interface font ahead of egui's proportional
/// fonts. Those stay behind it as fallbacks, so emoji and symbols still draw.
pub fn install_fonts(ctx: &egui::Context, interface: Option<egui::FontData>) {
    let mut fonts = egui::FontDefinitions::default();
    if let Some(bold) = FaceId::Bold.bundled_bytes() {
        fonts.font_data.insert(
            "gentium-bold".into(),
            Arc::new(egui::FontData::from_static(bold)),
        );
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
}

fn style(style: &mut egui::Style, ch: &Chrome) {
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(14.0));
    style
        .text_styles
        .insert(TextStyle::Button, FontId::proportional(14.0));
    style
        .text_styles
        .insert(TextStyle::Small, FontId::proportional(12.0));
    style.text_styles.insert(TextStyle::Heading, display(21.0));
    style
        .text_styles
        .insert(TextStyle::Monospace, FontId::monospace(12.0));
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.interact_size.y = 28.0;
    style.spacing.menu_margin = Margin::same(8);
    style.spacing.window_margin = Margin::same(12);

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
    v.selection.stroke = Stroke::new(1.0, ch.lapis);
    v.window_stroke = Stroke::new(1.0, ch.rule);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_shadow = Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(if ch.dark { 90 } else { 34 }),
    };
    v.popup_shadow = Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(if ch.dark { 80 } else { 30 }),
    };
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = ch.surface;
    w.noninteractive.weak_bg_fill = ch.surface;
    w.noninteractive.bg_stroke = Stroke::new(1.0, ch.rule);
    w.noninteractive.fg_stroke = Stroke::new(1.0, ch.ink);
    w.noninteractive.corner_radius = CornerRadius::same(6);
    w.inactive.bg_fill = ch.field;
    w.inactive.weak_bg_fill = Color32::TRANSPARENT;
    w.inactive.bg_stroke = Stroke::new(1.0, ch.rule);
    w.inactive.fg_stroke = Stroke::new(1.0, ch.ink);
    w.inactive.corner_radius = CornerRadius::same(6);
    w.hovered.bg_fill = ch.hover;
    w.hovered.weak_bg_fill = ch.hover;
    w.hovered.bg_stroke = Stroke::new(1.0, ch.hover_rule);
    w.hovered.fg_stroke = Stroke::new(1.2, ch.ink);
    w.hovered.corner_radius = CornerRadius::same(6);
    w.hovered.expansion = 0.0;
    w.active.bg_fill = ch.selection;
    w.active.weak_bg_fill = ch.selection;
    w.active.bg_stroke = Stroke::new(1.0, ch.lapis);
    w.active.fg_stroke = Stroke::new(1.4, ch.lapis);
    w.active.corner_radius = CornerRadius::same(6);
    w.active.expansion = 0.0;
    w.open = w.active;
    style.visuals = v;
}
