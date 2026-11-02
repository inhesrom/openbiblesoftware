//! PROTOTYPE visual system for the polished app-shell variants.
//!
//! Palette: cool linen pages, iron ink, lapis navigation, verdigris study tools and a narrow
//! gilt navigation flash. Link sets are shown with a vertical "link thread" on each panel.

use egui::{Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle};
use std::sync::Arc;

pub const SHELL: Color32 = Color32::from_rgb(232, 236, 235);
pub const SURFACE: Color32 = Color32::from_rgb(244, 246, 244);
pub const PAGE: Color32 = Color32::from_rgb(250, 251, 248);
pub const FIELD: Color32 = Color32::from_rgb(255, 255, 253);
pub const RULE: Color32 = Color32::from_rgb(205, 213, 211);
pub const INK: Color32 = Color32::from_rgb(32, 38, 41);
pub const MUTED: Color32 = Color32::from_rgb(91, 103, 108);
pub const LAPIS: Color32 = Color32::from_rgb(45, 78, 134);
pub const VERDIGRIS: Color32 = Color32::from_rgb(45, 116, 105);
pub const GILT: Color32 = Color32::from_rgb(180, 138, 53);
pub const SELECTION: Color32 = Color32::from_rgb(211, 224, 244);
pub const HOVER: Color32 = Color32::from_rgb(226, 235, 247);

pub fn link_color(c: Option<char>) -> Color32 {
    match c {
        Some('A') => LAPIS,
        Some('B') => VERDIGRIS,
        Some('C') => Color32::from_rgb(112, 79, 145),
        _ => Color32::from_rgb(154, 162, 164),
    }
}

pub fn display(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("display".into()))
}

pub fn install(ctx: &egui::Context, font_dir: &std::path::Path) {
    let mut fonts = egui::FontDefinitions::default();
    if let Ok(bytes) = std::fs::read(font_dir.join("GentiumPlus-Bold.ttf")) {
        fonts.font_data.insert("gentium-display".into(), Arc::new(egui::FontData::from_owned(bytes)));
        fonts.families.insert(FontFamily::Name("display".into()), vec!["gentium-display".into()]);
    } else {
        fonts.families.insert(FontFamily::Name("display".into()), fonts.families[&FontFamily::Proportional].clone());
    }
    ctx.set_fonts(fonts);

    ctx.all_styles_mut(|style| {
        style.text_styles.insert(TextStyle::Body, FontId::proportional(14.0));
        style.text_styles.insert(TextStyle::Button, FontId::proportional(14.0));
        style.text_styles.insert(TextStyle::Small, FontId::proportional(12.0));
        style.text_styles.insert(TextStyle::Heading, display(21.0));
        style.text_styles.insert(TextStyle::Monospace, FontId::monospace(12.0));
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
        v.window_shadow = Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(34) };
        v.popup_shadow = Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(30) };

        v.widgets.noninteractive.bg_fill = SURFACE;
        v.widgets.noninteractive.weak_bg_fill = SURFACE;
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, RULE);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, INK);
        v.widgets.noninteractive.corner_radius = CornerRadius::same(6);

        v.widgets.inactive.bg_fill = FIELD;
        v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, RULE);
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, INK);
        v.widgets.inactive.corner_radius = CornerRadius::same(6);

        v.widgets.hovered.bg_fill = HOVER;
        v.widgets.hovered.weak_bg_fill = HOVER;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(166, 184, 211));
        v.widgets.hovered.fg_stroke = Stroke::new(1.2, INK);
        v.widgets.hovered.corner_radius = CornerRadius::same(6);
        v.widgets.hovered.expansion = 0.0;

        v.widgets.active.bg_fill = SELECTION;
        v.widgets.active.weak_bg_fill = SELECTION;
        v.widgets.active.bg_stroke = Stroke::new(1.0, LAPIS);
        v.widgets.active.fg_stroke = Stroke::new(1.4, LAPIS);
        v.widgets.active.corner_radius = CornerRadius::same(6);
        v.widgets.active.expansion = 0.0;

        v.widgets.open = v.widgets.active;
        style.visuals = v;
    });
}
