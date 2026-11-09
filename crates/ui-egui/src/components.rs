//! Shared components (#221): the Buttons, fields, frames and headers every surface is built
//! from, each drawn from the design tokens ([`crate::tokens`]) and the chrome's colour roles
//! ([`crate::theme::c`]). A surface that needs one of these uses it rather than restyling
//! an egui widget itself, so the same thing looks the same in every Tab, pane and window.

use egui::{Frame, Response, RichText, Stroke, TextEdit, Ui};

use crate::icons::{self, Icon};
use crate::theme::{self, display};
use crate::tokens::{corner, inset, radius, scale, size, space, stroke, text};

/// Text in the compact control size: chips, header controls, list cells.
pub fn label_text(s: impl Into<String>) -> RichText {
    RichText::new(s).size(text::LABEL)
}

/// Text in the small size, muted: hints, counts, secondary lines.
pub fn hint_text(s: impl Into<String>) -> RichText {
    RichText::new(s).size(text::SMALL).color(theme::c().muted)
}

/// Text in the caption size: licence chips, badges, axis ticks.
pub fn caption_text(s: impl Into<String>) -> RichText {
    RichText::new(s).size(text::CAPTION)
}

/// A Tab's heading, in the display face.
pub fn title_text(s: impl Into<String>) -> RichText {
    RichText::new(s)
        .font(display(text::TITLE))
        .color(theme::c().ink)
}

/// A card's or a window's title.
pub fn subtitle_text(s: impl Into<String>) -> RichText {
    RichText::new(s)
        .strong()
        .size(text::SUBTITLE)
        .color(theme::c().ink)
}

/// **Button**: a push button in the chrome's style (text, an icon, or both).
pub fn button<'a>(atoms: impl egui::IntoAtoms<'a>) -> egui::Button<'a> {
    egui::Button::new(atoms)
}

/// **IconButton**: an icon alone, named for screen readers and tooltips, in the body size.
pub fn icon_button(ui: &mut Ui, icon: Icon, name: &str) -> Response {
    icons::button(ui, icon, size::ICON, theme::c().muted, name).on_hover_text(name)
}

/// **TextField**: a one-line field of the standard width.
pub fn text_field(text: &mut String) -> TextEdit<'_> {
    TextEdit::singleline(text).desired_width(size::FIELD)
}

/// **SearchField**: a one-line field with a hint, as wide as `width` (a size token).
pub fn search_field<'t>(text: &'t mut String, hint: &str, width: f32) -> TextEdit<'t> {
    TextEdit::singleline(text)
        .hint_text(hint)
        .desired_width(width)
}

/// **Toggle**: a check box with its label.
pub fn toggle<'a>(ui: &mut Ui, on: &'a mut bool, label: impl egui::IntoAtoms<'a>) -> Response {
    ui.checkbox(on, label)
}

/// **Popover**: the frame of a pop-up over the Workspace (a Launcher pane, a hover card).
pub fn popover_frame() -> Frame {
    let c = theme::c();
    Frame::new()
        .fill(c.field)
        .stroke(Stroke::new(stroke::HAIR, c.rule))
        .corner_radius(corner(radius::L))
        .inner_margin(inset::POPOVER)
        .shadow(crate::tokens::elevation::popup(c.dark))
}

/// **Card**: a group of related content on a page (a Home card, a Note, a notice).
pub fn card_frame() -> Frame {
    let c = theme::c();
    Frame::new()
        .fill(c.surface)
        .stroke(Stroke::new(stroke::HAIR, c.rule))
        .corner_radius(corner(radius::M))
        .inner_margin(inset::CARD)
}

/// A Card washed in `color`, for a notice (an error, an update available).
pub fn notice_frame(color: egui::Color32) -> Frame {
    Frame::new()
        .fill(color.gamma_multiply(crate::tokens::alpha::WASH))
        .corner_radius(corner(radius::S))
        .inner_margin(inset::NOTICE)
}

/// The page of a Tab: the field colour, inset from the Panel's edge.
pub fn page_frame() -> Frame {
    Frame::new()
        .fill(theme::c().field)
        .inner_margin(inset::PAGE)
}

/// A bar across a Tab under its header (a view switcher, a chart's controls).
pub fn band_frame() -> Frame {
    Frame::new().inner_margin(inset::BAND)
}

/// A chip: a rounded outline round a short label and its controls.
pub fn chip_frame() -> Frame {
    let c = theme::c();
    Frame::new()
        .fill(c.field)
        .stroke(Stroke::new(stroke::HAIR, c.rule))
        .corner_radius(corner(radius::PILL))
        .inner_margin(inset::CHIP)
}

/// A chip with a label and a Close button named `what` ("Do not search this Bible");
/// returns whether Close was clicked.
pub fn removable_chip(ui: &mut Ui, label: &str, what: &str) -> bool {
    let mut clicked = false;
    chip_frame().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::XXS;
            ui.label(label_text(label));
            clicked = icons::chip_close(ui, what).clicked();
        });
    });
    clicked
}

/// **SectionHeader**: a small, muted, upper-case heading over a group of rows.
pub fn section_header(ui: &mut Ui, title: &str) -> Response {
    ui.label(
        caption_text(title.to_uppercase())
            .strong()
            .color(theme::c().muted),
    )
}

/// **Menu**: a button that opens a drop-down menu (a submenu inside a menu).
pub fn menu<'a, R>(
    ui: &mut Ui,
    title: impl egui::IntoAtoms<'a>,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    ui.menu_button(title, add_contents)
}

/// A hairline Stroke in the rule colour.
pub fn rule_stroke() -> Stroke {
    Stroke::new(stroke::HAIR, theme::c().rule)
}

/// The thumb of a custom-drawn view's scroll bar in `track` (the view's right edge,
/// `size::SCROLLBAR` wide): `frac` of the way down, `shown` the share of the content in view.
pub fn scroll_thumb(
    painter: &egui::Painter,
    track: egui::Rect,
    frac: f32,
    shown: f32,
    color: egui::Color32,
) {
    let h = (track.height() * shown).clamp(size::THUMB_MIN.min(track.height()), track.height());
    let top = track.top() + frac.clamp(0.0, 1.0) * (track.height() - h);
    let w = track.width() - 2.0 * space::XXS;
    painter.rect_filled(
        egui::Rect::from_min_size(egui::pos2(track.left() + space::XXS, top), egui::vec2(w, h)),
        w / 2.0,
        color,
    );
}

/// Where a line of text will be once its layout arrives: a faint bar across the line's
/// x-height, `width` long, at `origin` (the line's top left), `line` tall, with the loading
/// shimmer passing over it ([`crate::motion::shimmer`]).
pub fn text_placeholder(
    painter: &egui::Painter,
    origin: egui::Pos2,
    width: f32,
    line: f32,
    color: egui::Color32,
) {
    let bar = egui::Rect::from_min_size(
        origin + egui::vec2(0.0, line * scale::PLACEHOLDER_TOP),
        egui::vec2(width, line * scale::PLACEHOLDER_HEIGHT),
    );
    // The skeleton shimmer (#225) sweeps down the area the placeholders are drawn in.
    let color = crate::motion::shimmer(painter.ctx(), painter.clip_rect(), bar.center().y, color);
    painter.rect_filled(bar, corner(radius::XS), color);
}

/// The outline round a hovered card-like link (a Factbook card, a Topic Guide entry).
pub fn hover_outline(ui: &Ui, rect: egui::Rect) {
    ui.painter().rect_stroke(
        rect.expand(space::XXS),
        corner(radius::S),
        Stroke::new(stroke::HAIR, theme::c().hover_rule),
        egui::StrokeKind::Outside,
    );
}
