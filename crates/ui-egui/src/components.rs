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

/// **Link**: text that does something when clicked. In a High contrast theme (#224) it is
/// underlined at rest as well, so it shows as a link without its colour.
pub fn link(ui: &mut Ui, text: impl Into<egui::WidgetText>) -> Response {
    let galley = laid_out(ui, text);
    let r = ui.link(galley.clone());
    underlined(ui, r, &galley)
}

/// A [`link`] that is greyed out unless `enabled`.
pub fn link_enabled(ui: &mut Ui, enabled: bool, text: impl Into<egui::WidgetText>) -> Response {
    let galley = laid_out(ui, text);
    let r = ui.add_enabled(enabled, egui::Link::new(galley.clone()));
    underlined(ui, r, &galley)
}

/// A [`link`] that opens `url` in the browser.
pub fn hyperlink_to(
    ui: &mut Ui,
    text: impl Into<egui::WidgetText>,
    url: impl ToString,
) -> Response {
    let galley = laid_out(ui, text);
    let r = ui.hyperlink_to(galley.clone(), url);
    underlined(ui, r, &galley)
}

/// A link's text laid out as egui's link lays it out, so its rows can be underlined.
fn laid_out(ui: &Ui, text: impl Into<egui::WidgetText>) -> std::sync::Arc<egui::Galley> {
    text.into()
        .into_galley(ui, None, ui.available_width(), egui::FontSelection::Default)
}

/// Underline each row of a link at rest in High contrast; egui underlines it only when
/// hovered or focused.
fn underlined(ui: &Ui, r: Response, galley: &egui::Galley) -> Response {
    if theme::c().high_contrast && ui.is_rect_visible(r.rect) {
        let color = if r.enabled() {
            ui.visuals().hyperlink_color
        } else {
            ui.visuals().widgets.noninteractive.fg_stroke.color
        };
        for row in &galley.rows {
            let row = row.rect().translate(r.rect.min.to_vec2());
            ui.painter().hline(
                row.x_range(),
                row.bottom(),
                Stroke::new(stroke::HAIR, color),
            );
        }
    }
    r
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

// ---------------------------------------------------------------------------------------
// Pane states (#223): every Tab and pane says the same way that it has nothing to show,
// that it is loading, or that something failed.

/// The Button of every [`error_state`].
pub const RETRY: &str = "Retry";
/// The link that shows an [`error_state`]'s details.
pub const DETAILS: &str = "Details";
/// The link that hides them again.
pub const HIDE_DETAILS: &str = "Hide details";

/// **EmptyState**: why there is nothing to show, then the one Button that would fill it
/// (#135's pattern: "None in your library" with a way to get it). Returns the Button.
pub fn empty_state(ui: &mut Ui, message: &str, action: &str) -> Response {
    ui.add(egui::Label::new(RichText::new(message).color(theme::c().muted)).wrap());
    ui.add_space(space::XS);
    ui.add(button(action))
}

/// **Loading**: what is loading, in a hint line, over a skeleton of the text to come (faint
/// bars with the loading shimmer, [`text_placeholder`]). Asks for the next frame, so the pane
/// shows the result as soon as it is there.
pub fn loading_state(ui: &mut Ui, what: &str) {
    ui.label(hint_text(what));
    let width = ui.available_width().min(size::PROSE);
    let line = text::LABEL_LINE;
    for share in crate::tokens::share::SKELETON {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, line), egui::Sense::hover());
        text_placeholder(ui.painter(), rect.min, width * share, line, theme::c().rule);
    }
    ui.ctx().request_repaint();
}

/// **Progress**: how far a long task (a download, an import) is, with what it is doing.
pub fn progress_bar(fraction: f32, what: impl Into<egui::WidgetText>) -> egui::ProgressBar {
    egui::ProgressBar::new(fraction)
        .desired_width(size::FIELD_WIDE)
        .text(what)
}

/// **ErrorState**: what failed, in plain words, a [`RETRY`] Button, and the error itself
/// (`details`, as the code that failed reported it) only after [`DETAILS`] is clicked.
/// Returns whether Retry was clicked.
pub fn error_state(ui: &mut Ui, message: &str, details: &str) -> bool {
    let id = ui.scope_id().with(("error-details", message));
    let mut open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    let mut retry = false;
    let warn = theme::c().warn;
    notice_frame(warn).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = space::XS;
            ui.add(icons::image(ui.ctx(), Icon::Warning, size::ICON, warn));
            ui.add(egui::Label::new(RichText::new(message).color(theme::c().ink)).wrap());
        });
        ui.horizontal(|ui| {
            retry = ui.add(button(RETRY)).clicked();
            if !details.is_empty() {
                let label = if open { HIDE_DETAILS } else { DETAILS };
                if link(ui, label).clicked() {
                    open = !open;
                }
            }
        });
        if open {
            ui.add(
                egui::Label::new(caption_text(details).color(theme::c().muted))
                    .wrap()
                    .selectable(true),
            );
        }
    });
    ui.data_mut(|d| d.insert_temp(id, open));
    retry
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

/// A selected row or open button the app paints itself (the Command box's current suggestion,
/// a selected Library row, an open Launcher): filled in the Selection colour and, in a High
/// contrast theme (#224), framed in the Borders colour, so it shows without its fill.
pub fn selected_row(painter: &egui::Painter, rect: egui::Rect, radius: f32) {
    let c = theme::c();
    painter.rect_filled(rect, corner(radius), c.selection);
    if c.high_contrast {
        painter.rect_stroke(
            rect,
            corner(radius),
            Stroke::new(stroke::MEDIUM, c.rule),
            egui::StrokeKind::Inside,
        );
    }
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
