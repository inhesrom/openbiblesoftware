//! The Study desk visual system for the chrome (chosen in #13's polish follow-up): cool linen
//! surfaces, iron ink, lapis navigation, verdigris study accents and Gentium display headings.
//! Resource text colours live in [`crate::Palette`]; dark chrome arrives with #66.

use std::sync::Arc;

use egui::{Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle};
use obs_fonts::FaceId;

pub const SHELL: Color32 = Color32::from_rgb(232, 236, 235);
pub const SURFACE: Color32 = Color32::from_rgb(244, 246, 244);
pub const FIELD: Color32 = Color32::from_rgb(255, 255, 253);
pub const RULE: Color32 = Color32::from_rgb(205, 213, 211);
pub const INK: Color32 = Color32::from_rgb(32, 38, 41);
pub const MUTED: Color32 = Color32::from_rgb(91, 103, 108);
pub const LAPIS: Color32 = Color32::from_rgb(45, 78, 134);
pub const VERDIGRIS: Color32 = Color32::from_rgb(45, 116, 105);
pub const SELECTION: Color32 = Color32::from_rgb(211, 224, 244);
pub const HOVER: Color32 = Color32::from_rgb(226, 235, 247);
pub const WARN: Color32 = Color32::from_rgb(150, 60, 30);
/// The link chip of a Tab with no Link set.
pub const UNLINKED: Color32 = Color32::from_rgb(154, 162, 164);
pub const GILT: Color32 = Color32::from_rgb(180, 138, 53);
/// The mark on a jump's target verse (a translucent gilt).
pub const FLASH: Color32 = Color32::from_rgba_premultiplied(108, 83, 32, 96);

/// Each Link set's colour.
pub fn link_color(link: Option<char>) -> Color32 {
    match link {
        Some('A') => LAPIS,
        Some('B') => VERDIGRIS,
        Some('C') => Color32::from_rgb(112, 79, 145),
        Some(_) => GILT,
        None => UNLINKED,
    }
}

const DISPLAY: &str = "display";

/// Gentium Plus Bold, for headings and the app name.
pub fn display(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(DISPLAY.into()))
}

/// Install the fonts and the Study desk style on a context.
pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "gentium-bold".into(),
        Arc::new(egui::FontData::from_static(FaceId::Bold.bytes())),
    );
    let mut family = vec!["gentium-bold".to_string()];
    if let Some(p) = fonts.families.get(&FontFamily::Proportional) {
        family.extend(p.iter().cloned());
    }
    fonts
        .families
        .insert(FontFamily::Name(DISPLAY.into()), family);
    ctx.set_fonts(fonts);
    ctx.set_theme(egui::Theme::Light);
    ctx.all_styles_mut(style);
}

fn style(style: &mut egui::Style) {
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

    let mut v = egui::Visuals::light();
    v.panel_fill = SHELL;
    v.window_fill = FIELD;
    v.extreme_bg_color = FIELD;
    v.faint_bg_color = SURFACE;
    v.code_bg_color = SURFACE;
    v.hyperlink_color = LAPIS;
    v.selection.bg_fill = SELECTION;
    v.selection.stroke = Stroke::new(1.0, LAPIS);
    v.window_stroke = Stroke::new(1.0, RULE);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_shadow = Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(34),
    };
    v.popup_shadow = Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(30),
    };
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = SURFACE;
    w.noninteractive.weak_bg_fill = SURFACE;
    w.noninteractive.bg_stroke = Stroke::new(1.0, RULE);
    w.noninteractive.fg_stroke = Stroke::new(1.0, INK);
    w.noninteractive.corner_radius = CornerRadius::same(6);
    w.inactive.bg_fill = FIELD;
    w.inactive.weak_bg_fill = Color32::TRANSPARENT;
    w.inactive.bg_stroke = Stroke::new(1.0, RULE);
    w.inactive.fg_stroke = Stroke::new(1.0, INK);
    w.inactive.corner_radius = CornerRadius::same(6);
    w.hovered.bg_fill = HOVER;
    w.hovered.weak_bg_fill = HOVER;
    w.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(166, 184, 211));
    w.hovered.fg_stroke = Stroke::new(1.2, INK);
    w.hovered.corner_radius = CornerRadius::same(6);
    w.hovered.expansion = 0.0;
    w.active.bg_fill = SELECTION;
    w.active.weak_bg_fill = SELECTION;
    w.active.bg_stroke = Stroke::new(1.0, LAPIS);
    w.active.fg_stroke = Stroke::new(1.4, LAPIS);
    w.active.corner_radius = CornerRadius::same(6);
    w.active.expansion = 0.0;
    w.open = w.active;
    style.visuals = v;
}
