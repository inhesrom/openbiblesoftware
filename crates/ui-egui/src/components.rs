//! Shared components (#221): the Buttons, fields, frames and headers every surface is built
//! from, each drawn from the design tokens ([`crate::tokens`]) and the chrome's colour roles
//! ([`crate::theme::c`]). A surface that needs one of these uses it rather than restyling
//! an egui widget itself, so the same thing looks the same in every Tab, pane and window.

use egui::{Frame, Response, RichText, Stroke, TextEdit, Ui, vec2};

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

/// **Toggle**: a check box with its label, its box outlined so that it shows unchecked
/// ([`with_control_outline`]).
pub fn toggle<'a>(ui: &mut Ui, on: &'a mut bool, label: impl egui::IntoAtoms<'a>) -> Response {
    with_control_outline(ui, |ui| ui.checkbox(on, label))
}

/// A [`toggle`] that is greyed out and does nothing unless `enabled`.
pub fn toggle_enabled<'a>(
    ui: &mut Ui,
    enabled: bool,
    on: &'a mut bool,
    label: impl egui::IntoAtoms<'a>,
) -> Response {
    with_control_outline(ui, |ui| {
        ui.add_enabled(enabled, egui::Checkbox::new(on, label))
    })
}

/// **Radio**: one choice of several, with its label, selected when `selected`; its circle
/// outlined as a [`toggle`]'s box is.
pub fn radio<'a>(ui: &mut Ui, selected: bool, label: impl egui::IntoAtoms<'a>) -> Response {
    with_control_outline(ui, |ui| ui.radio(selected, label))
}

/// A [`radio`] that sets `current` to `value` when clicked.
pub fn radio_value<'a, V: PartialEq>(
    ui: &mut Ui,
    current: &mut V,
    value: V,
    label: impl egui::IntoAtoms<'a>,
) -> Response {
    with_control_outline(ui, |ui| ui.radio_value(current, value, label))
}

/// Draw check boxes and radio buttons with `add`, their edge in Muted and their check mark
/// as thick as the focus ring (#306). egui fills the box with the Field colour and borders it
/// in Rule, which nearly vanishes on a Field-coloured dialog or page; Muted reads at WCAG AA
/// on every background of the chrome ([`theme::contrast_shorts`]), more than the 3:1 a
/// control's edge needs. Hovered, the edge is Lapis.
fn with_control_outline<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let saved = ui.visuals().widgets.clone();
    let c = theme::c();
    // Menus take the border off their items; a check box keeps its edge there too.
    let width = theme::border_width(&c);
    let w = &mut ui.visuals_mut().widgets;
    w.inactive.bg_stroke = Stroke::new(width, c.muted);
    w.hovered.bg_stroke = Stroke::new(width, c.lapis);
    w.inactive.fg_stroke.width = stroke::THICK;
    w.hovered.fg_stroke.width = stroke::THICK;
    let response = add(ui);
    ui.visuals_mut().widgets = saved;
    response
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

/// The page of a Guide Tab: the Page tone `page`, which the Guide's Resource text is drawn on,
/// so its snippets sit on the page and not on slabs of their own (#306). When the chrome's
/// text would not read on it (Paper chosen in a dark Theme), the field, as [`page_frame`];
/// each snippet then keeps its own page.
pub fn guide_page_frame(page: egui::Color32) -> Frame {
    let c = theme::c();
    let fill = if theme::text_reads_on(&c, page) {
        page
    } else {
        c.field
    };
    page_frame().fill(fill)
}

/// A bar across a Tab under its header (a view switcher, a chart's controls).
pub fn band_frame() -> Frame {
    Frame::new().inner_margin(inset::BAND)
}

// ---------------------------------------------------------------------------------------
// Narrow Panels (#301): a Tab's page scrolls only up and down, so a row wider than its Panel
// widens the whole page (egui grows a layout to what it holds) and everything after it that
// fills the width is laid out past the Panel's edge, where it is cut off. Rows of text wrap,
// and what fills the width takes no more than the Panel shows.

/// The width a full-width block (Resource text, a reading pane) takes at the cursor: what the
/// layout has left, never past the visible right edge (the Panel's).
pub fn fill_width(ui: &Ui) -> f32 {
    let visible = ui.clip_rect().right() - ui.cursor().left();
    ui.available_width().min(visible).max(0.0)
}

/// A title and a muted detail on one line (a Resource and the section it shows): in a narrow
/// Panel the title wraps and the detail goes under it whole.
pub fn title_and_detail(ui: &mut Ui, title: RichText, detail: RichText) {
    ui.horizontal_wrapped(|ui| {
        ui.label(title);
        ui.add(egui::Label::new(detail).extend());
    });
}

/// A Tool's summary after its controls ("Acts 13 · 15 places"), inside a
/// `horizontal_wrapped` row: where it fits, else on a row of its own under them, and cut
/// short (the whole text on hover) only when even that row is too narrow.
pub fn toolbar_summary(ui: &mut Ui, summary: String) -> Response {
    let text = RichText::new(summary).color(theme::c().muted);
    let width = egui::WidgetText::from(text.clone())
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Body,
        )
        .size()
        .x;
    if width > ui.available_width() {
        ui.end_row();
    }
    ui.add(egui::Label::new(text).truncate())
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
    chip(ui, ("removable-chip", label), |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::XXS;
            ui.label(label_text(label));
            clicked = icons::chip_close(ui, what).clicked();
        });
    });
    clicked
}

/// A chip ([`chip_frame`]) round `add_contents`, kept whole in a `horizontal_wrapped` row:
/// one that does not fit after the chips before it starts the next row, rather than running
/// past the Panel's edge (#301). egui sizes a frame only once it is drawn, so the chip is
/// placed by its size last frame (`id_salt` names it among its row's chips).
pub fn chip<R>(
    ui: &mut Ui,
    id_salt: impl egui::AsIdSalt,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<R> {
    let id = ui.scope_id().with(id_salt);
    let last: Option<egui::Vec2> = ui.data(|d| d.get_temp(id));
    let shown = ui.allocate_ui(last.unwrap_or(egui::Vec2::ZERO), |ui| {
        chip_frame().show(ui, add_contents).inner
    });
    let size = shown.response.rect.size();
    if last != Some(size) {
        ui.data_mut(|d| d.insert_temp(id, size));
        if last.is_none() {
            ui.ctx().request_discard("a chip's size");
        }
    }
    shown
}

/// **SectionHeader**: a small, muted, upper-case heading over a group of rows.
pub fn section_header(ui: &mut Ui, title: &str) -> Response {
    ui.label(
        caption_text(title.to_uppercase())
            .strong()
            .color(theme::c().muted),
    )
}

// ---------------------------------------------------------------------------------------
// Rows that fit their Panel (#303): in a narrow side Panel a row's controls go onto more lines
// rather than being drawn over each other or past the Panel's edge. Their widths are measured
// as they are drawn, so a row settles a frame after its width changes.

/// Last frame's widths of a row's parts, and asks for another frame when they changed.
fn measured_widths<const N: usize>(ui: &Ui, id: egui::Id, now: Option<[f32; N]>) -> [f32; N] {
    let before = ui.data(|d| d.get_temp::<[f32; N]>(id));
    if let Some(now) = now
        && before.is_none_or(|b| b.iter().zip(&now).any(|(a, b)| (a - b).abs() > 0.5))
    {
        ui.data_mut(|d| d.insert_temp(id, now));
        ui.ctx().request_repaint();
    }
    before.unwrap_or([0.0; N])
}

/// The width a wrapped group of controls needs on one line: its width, or the whole row when
/// it took more than one line (so it is not tried beside anything again).
fn wrapped_width(ui: &Ui, rect: egui::Rect, row: f32) -> f32 {
    let line = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
    if rect.height() > line {
        row
    } else {
        rect.width()
    }
}

/// **RowWithEnd**: controls from the left (`start`) and one group against the row's right
/// edge (`end`, laid out right to left). When the row is too narrow for both, `start` wraps
/// onto more lines and `end` takes a line of its own, still at the right, rather than being
/// drawn over `start`. Each control wraps whole: a Button never wraps its own label onto two
/// lines. `cx` is what both parts change (each gets it in turn).
pub fn row_with_end<C, R>(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    cx: &mut C,
    start: impl FnOnce(&mut Ui, &mut C),
    end: impl FnOnce(&mut Ui, &mut C) -> R,
) -> R {
    let id = ui.make_persistent_id(("row-with-end", id_salt));
    let [start_w, end_w] = measured_widths(ui, id, None);
    let gap = ui.spacing().item_spacing.x;
    let fits = start_w + gap + end_w <= ui.available_width();
    let (widths, out) = if fits {
        ui.horizontal(|ui| {
            let start_w = ui.horizontal(|ui| start(ui, cx)).response.rect.width();
            let (end_w, out) = ui
                .with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let r = ui.horizontal(|ui| end(ui, cx));
                    (r.response.rect.width(), r.inner)
                })
                .inner;
            ([start_w, end_w], out)
        })
        .inner
    } else {
        let row = ui.available_width();
        let rect = ui
            .horizontal_wrapped(|ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                start(ui, cx);
            })
            .response
            .rect;
        let start_w = wrapped_width(ui, rect, row);
        // A line of its own, as tall as a control.
        let line = vec2(ui.available_width(), ui.spacing().interact_size.y);
        let (end_w, out) = ui
            .allocate_ui_with_layout(
                line,
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    let r = ui.horizontal(|ui| end(ui, cx));
                    (r.response.rect.width(), r.inner)
                },
            )
            .inner;
        ([start_w, end_w], out)
    };
    measured_widths(ui, id, Some(widths));
    out
}

/// **FieldRow**: a field that takes the row's spare width (`field` gets the width to give
/// it), then `controls`. When that would leave the field less than `min` wide, the field takes
/// the whole row and the controls go under it, wrapping whole: a Button never wraps its own
/// label onto two lines.
pub fn field_row<C>(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    min: f32,
    cx: &mut C,
    field: impl FnOnce(&mut Ui, &mut C, f32),
    controls: impl FnOnce(&mut Ui, &mut C),
) {
    let id = ui.make_persistent_id(("field-row", id_salt));
    let [controls_w] = measured_widths(ui, id, None);
    let gap = ui.spacing().item_spacing.x;
    let room = ui.available_width();
    let beside = room - controls_w - gap;
    let controls_w = if beside >= min {
        ui.horizontal(|ui| {
            field(ui, cx, beside);
            ui.horizontal(|ui| controls(ui, cx)).response.rect.width()
        })
        .inner
    } else {
        ui.horizontal(|ui| field(ui, cx, room));
        let rect = ui
            .horizontal_wrapped(|ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                controls(ui, cx);
            })
            .response
            .rect;
        wrapped_width(ui, rect, room)
    };
    measured_widths(ui, id, Some([controls_w]));
}

/// **Scroll bars that always show** in `ui` (egui's float out of sight until hovered): for a
/// table wider than its Tab, so it shows that it scrolls sideways (#303).
pub fn shown_scroll_bars(ui: &mut Ui) {
    ui.spacing_mut().scroll = egui::style::ScrollStyle {
        floating_width: space::XS,
        dormant_handle_opacity: crate::tokens::alpha::HALF,
        ..egui::style::ScrollStyle::floating()
    };
}

/// `text` on one line at most `width` wide, cut short with "…" when it does not fit.
pub fn elided(
    painter: &egui::Painter,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(0.0));
    painter.layout_job(job)
}

/// **Menu**: a button that opens a drop-down menu (a submenu inside a menu).
pub fn menu<'a, R>(
    ui: &mut Ui,
    title: impl egui::IntoAtoms<'a>,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    ui.menu_button(title, add_contents)
}

/// A [`menu`] that a click inside leaves open (one outside, or Esc, closes it): for menus
/// with controls used in place, such as a slider. Its items close it themselves
/// (`ui.close()`).
pub fn menu_staying_open<'a, R>(
    ui: &mut Ui,
    title: impl egui::IntoAtoms<'a>,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    let config = egui::containers::menu::MenuConfig::new()
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    let (response, inner) = egui::containers::menu::MenuButton::new(title)
        .config(config)
        .ui(ui, add_contents);
    egui::InnerResponse::new(inner.map(|i| i.inner), response)
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
    error_frame(ui, message, details, true)
}

/// An [`error_state`] without Retry, for a failure the same attempt cannot fix (a file that
/// is not there); the surface offers its own way on.
pub fn error_notice(ui: &mut Ui, message: &str, details: &str) {
    error_frame(ui, message, details, false);
}

fn error_frame(ui: &mut Ui, message: &str, details: &str, with_retry: bool) -> bool {
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
        if with_retry || !details.is_empty() {
            ui.horizontal(|ui| {
                retry = with_retry && ui.add(button(RETRY)).clicked();
                if !details.is_empty() {
                    let label = if open { HIDE_DETAILS } else { DETAILS };
                    if link(ui, label).clicked() {
                        open = !open;
                    }
                }
            });
        }
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

/// The next scroll area's content keeps clear of its scroll bar, so a row's Buttons at the right
/// edge never sit under the bar's thumb (LIBRARY-14). egui's floating bar is drawn over the
/// content and takes no room of its own.
pub fn keep_clear_of_scroll_bar(ui: &mut Ui) {
    let scroll = &mut ui.spacing_mut().scroll;
    scroll.floating_allocated_width =
        scroll.bar_inner_margin + scroll.bar_width + scroll.bar_outer_margin;
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

/// `text` laid out on one line at most `width` wide, ending in "…" when it does not fit (a
/// Library row's title, a Layout card's chip, a Timeline label): never cut mid-letter.
pub fn elided(
    painter: &egui::Painter,
    text: impl Into<String>,
    font: egui::FontId,
    color: egui::Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.into(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(1.0));
    painter.layout_job(job)
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
