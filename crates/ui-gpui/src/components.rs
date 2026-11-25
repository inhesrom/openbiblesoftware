//! The shared components every GPUI surface is built from (ADR 0015): the GPUI versions of the
//! egui app's `components.rs`. A surface uses one of these instead of styling a control
//! itself, so Tabs, panes, menus and dialogs look like one app.
//!
//! Each component takes the current [`Look`] and draws with its chrome. Controls are built on
//! `gpui-base`'s primitives, which give them their behaviour (activation by pointer and
//! keyboard, focus, IME and undo in text fields) and their AccessKit role. Every control has an
//! accessible name and a visible keyboard focus ring.

use gpui_kit::base::input::{InputAccessibility, InputEditorStyle, InputState};
use gpui_kit::base::{Button, InputBase, Radio, Toggle};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Div, ElementId, Entity, Focusable, FontWeight, Hsla, Role,
    SharedString, Stateful, Window, div, px,
};

use crate::icons::Icon;
use crate::theme::{DISPLAY_FAMILY, Look};
use crate::tokens::{self, inset, radius, rgba, size, space, stroke, text, with_alpha};

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

fn hsla(c: obs_engine::Rgb) -> Hsla {
    rgba(c).into()
}

/// A plain element with an accessible role and name. AccessKit nodes need an element id: it is
/// taken from the role and the name, so give elements of one role distinct names in a scope.
fn semantic(d: Div, role: Role, name: &SharedString) -> Stateful<Div> {
    d.id(ElementId::Name(format!("{role:?}:{name}").into()))
        .role(role)
        .aria_label(name.clone())
}

/// A control's frame: the chrome's field and rule, rounded, with the hover and keyboard focus
/// states. The focus ring is the lapis role, heavy in High contrast (#224).
fn control_frame<E: Styled + InteractiveElement>(e: E, look: &Look) -> E {
    let ch = look.chrome;
    let ring = hsla(ch.lapis);
    let ring_width = look.focus_width();
    e.h(px(look.pt(size::CONTROL)))
        .px(px(look.pt(space::M)))
        .rounded(px(look.pt(radius::M)))
        .border(px(stroke::HAIR))
        .border_color(hsla(ch.rule))
        .bg(hsla(ch.field))
        .text_color(hsla(ch.ink))
        .text_size(px(look.pt(text::BODY)))
        .hover(move |s| s.bg(hsla(ch.hover)).border_color(hsla(ch.hover_rule)))
        .focus_visible(move |s| s.border(px(ring_width)).border_color(ring))
}

/// A button with a text label.
pub fn button(id: impl Into<ElementId>, label: impl Into<SharedString>, look: &Look) -> Button {
    let label = label.into();
    let b = Button::new(id).accessibility_label(label.clone());
    control_frame(b, look).child(label)
}

/// A primary button: the lapis fill, for the one action a dialog or page is for.
pub fn primary_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    look: &Look,
) -> Button {
    let ch = look.chrome;
    button(id, label, look)
        .bg(hsla(ch.lapis))
        .border_color(hsla(ch.lapis))
        .text_color(hsla(if ch.dark { ch.shell } else { ch.field }))
}

/// A button showing an icon only; `name` is what a screen reader says and the tooltip shows.
pub fn icon_button(
    id: impl Into<ElementId>,
    icon: Icon,
    name: impl Into<SharedString>,
    look: &Look,
) -> Button {
    let ch = look.chrome;
    let b = Button::new(id).accessibility_label(name.into());
    control_frame(b, look)
        .w(px(look.pt(size::CONTROL)))
        .px(px(0.0))
        .bg(hsla(ch.surface))
        .border_color(hsla(ch.surface))
        .child(icon.element(look.pt(size::ICON), hsla(ch.ink)))
}

/// Project the Look onto a text field's editing engine: its colours and accessible name.
pub fn style_text_state(
    state: &Entity<InputState>,
    label: impl Into<SharedString>,
    look: &Look,
    cx: &mut App,
) {
    let ch = look.chrome;
    let label = label.into();
    state.update(cx, |s, _| {
        s.set_editor_style(InputEditorStyle {
            foreground: hsla(ch.ink),
            muted_foreground: hsla(ch.muted),
            background: hsla(ch.field),
            border: hsla(ch.rule),
            selection: hsla(ch.selection),
            caret: hsla(ch.ink),
            ..InputEditorStyle::default()
        });
        s.set_accessibility(
            InputAccessibility::new(Some(Role::TextInput))
                .with_label(Some(label))
                .with_value_exposed(true),
        );
    });
}

/// A single-line text field over `state` (IME, selection, undo, clipboard: `gpui-base`'s
/// editing engine). `label` is its accessible name.
pub fn text_field(
    id: impl Into<ElementId>,
    state: &Entity<InputState>,
    label: impl Into<SharedString>,
    look: &Look,
    window: &Window,
    cx: &mut App,
) -> AnyElement {
    style_text_state(state, label, look, cx);
    let focused = state.read(cx).focus_handle(cx).is_focused(window);
    let ch = look.chrome;
    let frame = InputBase::new(id)
        .semantic_owner(state.entity_id())
        .focused(focused)
        .role(gpui_kit::base::RoleOverride::Presentational);
    control_frame(frame, look)
        .w(px(look.pt(size::FIELD)))
        .flex()
        .items_center()
        .when(focused, |f| {
            f.border(px(look.focus_width()))
                .border_color(hsla(ch.lapis))
        })
        .child(state.clone())
        .into_any_element()
}

/// A search field: a text field with the search icon before it.
pub fn search_field(
    id: impl Into<ElementId>,
    state: &Entity<InputState>,
    label: impl Into<SharedString>,
    look: &Look,
    window: &Window,
    cx: &mut App,
) -> AnyElement {
    let ch = look.chrome;
    div()
        .flex()
        .items_center()
        .gap(px(look.pt(space::XS)))
        .child(Icon::Search.element(look.pt(size::ICON), hsla(ch.muted)))
        .child(text_field(id, state, label, look, window, cx))
        .into_any_element()
}

/// A switch with its label after it.
pub fn toggle(
    id: impl Into<ElementId>,
    on: bool,
    label: impl Into<SharedString>,
    look: &Look,
    on_change: impl Fn(bool, &ClickEvent, &mut Window, &mut App) + 'static,
) -> Toggle {
    let ch = look.chrome;
    let label = label.into();
    let ring = hsla(ch.lapis);
    let ring_width = look.focus_width();
    let track = look.pt(size::CHIP);
    Toggle::new(id)
        .pressed(on)
        .accessibility_label(label.clone())
        .on_change(on_change)
        .flex()
        .items_center()
        .justify_start()
        .gap(px(look.pt(space::S)))
        .rounded(px(look.pt(radius::S)))
        .text_color(hsla(ch.ink))
        .text_size(px(look.pt(text::BODY)))
        .focus_visible(move |s| s.border(px(ring_width)).border_color(ring))
        .child(
            div()
                .w(px(track * 1.8))
                .h(px(track * 0.9))
                .rounded_full()
                .border(px(stroke::HAIR))
                .border_color(hsla(if on { ch.lapis } else { ch.rule }))
                .bg(hsla(if on { ch.lapis } else { ch.field }))
                .flex()
                .items_center()
                .when(on, |t| t.justify_end())
                .px(px(2.0))
                .child(
                    div()
                        .size(px(track * 0.9 - 6.0))
                        .rounded_full()
                        .bg(hsla(if on { ch.field } else { ch.muted })),
                ),
        )
        .child(label)
}

/// A radio button with its label after it.
pub fn radio(
    id: impl Into<ElementId>,
    selected: bool,
    label: impl Into<SharedString>,
    look: &Look,
    on_change: impl Fn(bool, &ClickEvent, &mut Window, &mut App) + 'static,
) -> Radio {
    let ch = look.chrome;
    let label = label.into();
    let ring = hsla(ch.lapis);
    let ring_width = look.focus_width();
    let dot = look.pt(size::ICON);
    Radio::new(id)
        .checked(selected)
        .accessibility_label(label.clone())
        .on_change(on_change)
        .flex()
        .items_center()
        .justify_start()
        .gap(px(look.pt(space::S)))
        .text_color(hsla(ch.ink))
        .text_size(px(look.pt(text::BODY)))
        .focus_visible(move |s| s.border(px(ring_width)).border_color(ring))
        .child(
            div()
                .size(px(dot))
                .rounded_full()
                .border(px(stroke::MEDIUM))
                .border_color(hsla(if selected { ch.lapis } else { ch.rule }))
                .bg(hsla(ch.field))
                .flex()
                .items_center()
                .justify_center()
                .when(selected, |d| {
                    d.child(div().size(px(dot / 2.0)).rounded_full().bg(hsla(ch.lapis)))
                }),
        )
        .child(label)
}

/// A chip: a short label in a pill (a Bible in a list, a filter). With `on_close`, it has a
/// Close button, named "Remove <label>".
pub fn chip(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    look: &Look,
    on_close: Option<ClickHandler>,
) -> Stateful<Div> {
    let ch = look.chrome;
    let label: SharedString = label.into();
    let id: ElementId = id.into();
    let close = on_close.map(|handler| {
        Button::new(ElementId::Name(format!("close-{label}").into()))
            .accessibility_label(format!("Remove {label}"))
            .size(px(look.pt(size::ICON)))
            .rounded_full()
            .hover(move |s| s.bg(hsla(ch.hover)))
            .on_click(move |e, w, cx| handler(e, w, cx))
            .child(Icon::Close.element(look.pt(size::ICON_SMALL), hsla(ch.muted)))
    });
    div()
        .id(id)
        .role(Role::Group)
        .aria_label(label.clone())
        .flex()
        .items_center()
        .gap(px(look.pt(space::XXS)))
        .h(px(look.pt(size::CHIP)))
        .px(px(look.pt(inset::CHIP.0)))
        .rounded(px(look.pt(radius::PILL)))
        .border(px(stroke::HAIR))
        .border_color(hsla(ch.rule))
        .bg(hsla(ch.field))
        .text_color(hsla(ch.ink))
        .text_size(px(look.pt(text::LABEL)))
        .child(label)
        .children(close)
}

/// A section's heading on a page or in a Guide.
pub fn section_header(title: impl Into<SharedString>, look: &Look) -> Stateful<Div> {
    let ch = look.chrome;
    let title = title.into();
    semantic(div(), Role::Heading, &title)
        .pb(px(look.pt(space::XS)))
        .mb(px(look.pt(space::S)))
        .border_b(px(stroke::HAIR))
        .border_color(hsla(ch.rule))
        .text_color(hsla(ch.ink))
        .text_size(px(look.pt(text::SUBTITLE)))
        .font_weight(FontWeight::SEMIBOLD)
        .child(title)
}

/// A Tab's heading in the display face.
pub fn title(text_: impl Into<SharedString>, look: &Look) -> Stateful<Div> {
    let t = text_.into();
    semantic(div(), Role::Heading, &t)
        .font_family(DISPLAY_FAMILY)
        .font_weight(FontWeight::BOLD)
        .text_size(px(look.pt(text::TITLE)))
        .text_color(hsla(look.chrome.ink))
        .child(t)
}

/// Secondary text: a hint or a caption.
pub fn hint(text_: impl Into<SharedString>, look: &Look) -> Div {
    div()
        .text_color(hsla(look.chrome.muted))
        .text_size(px(look.pt(text::SMALL)))
        .child(text_.into())
}

/// A Tab's page: the surface, inset from the Panel's edges.
pub fn page(look: &Look) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(look.pt(space::L)))
        .px(px(look.pt(inset::PAGE.0)))
        .py(px(look.pt(inset::PAGE.1)))
        .bg(hsla(look.chrome.surface))
        .text_color(hsla(look.chrome.ink))
        .text_size(px(look.pt(text::BODY)))
}

/// A card: a group on a page.
pub fn card(look: &Look) -> Div {
    let ch = look.chrome;
    div()
        .flex()
        .flex_col()
        .gap(px(look.pt(space::M)))
        .p(px(look.pt(inset::CARD.0)))
        .rounded(px(look.pt(radius::M)))
        .border(px(stroke::HAIR))
        .border_color(hsla(ch.rule))
        .bg(hsla(ch.field))
}

/// A notice: a one-line band washed in a colour role (an update, offline, a warning).
pub fn notice(
    message: impl Into<SharedString>,
    role_color: obs_engine::Rgb,
    look: &Look,
) -> Stateful<Div> {
    let m = message.into();
    semantic(div(), Role::Status, &m)
        .px(px(look.pt(inset::NOTICE.0)))
        .py(px(look.pt(inset::NOTICE.1)))
        .rounded(px(look.pt(radius::M)))
        .bg(with_alpha(role_color, tokens::alpha::WASH))
        .border_l(px(stroke::THICK))
        .border_color(hsla(role_color))
        .text_color(hsla(look.chrome.ink))
        .child(m)
}

/// A floating frame: a popover, a menu, a tooltip or a toast, with the popup elevation.
fn floating(look: &Look) -> Div {
    let ch = look.chrome;
    div()
        .flex()
        .flex_col()
        .p(px(look.pt(inset::POPOVER.0)))
        .rounded(px(look.pt(radius::L)))
        .border(px(stroke::HAIR))
        .border_color(hsla(ch.rule))
        .bg(hsla(ch.surface))
        .text_color(hsla(ch.ink))
        .shadow(tokens::shadow(tokens::elevation::POPUP, ch.dark))
}

/// A popover's frame, named for a screen reader.
pub fn popover_frame(name: impl Into<SharedString>, look: &Look) -> Stateful<Div> {
    semantic(floating(look), Role::Dialog, &name.into())
        .max_w(px(look.pt(size::POPOVER)))
        .gap(px(look.pt(space::S)))
}

/// A menu: a column of items.
pub fn menu(name: impl Into<SharedString>, look: &Look) -> Stateful<Div> {
    semantic(floating(look), Role::Menu, &name.into())
        .min_w(px(look.pt(size::MENU_COLUMN)))
        .p(px(look.pt(space::XS)))
}

/// One item of a menu, with its keyboard shortcut after it.
pub fn menu_item(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    shortcut: Option<&str>,
    look: &Look,
) -> Button {
    let ch = look.chrome;
    let label = label.into();
    let ring = hsla(ch.lapis);
    Button::new(id)
        .role(Role::MenuItem)
        .accessibility_label(label.clone())
        .justify_between()
        .gap(px(look.pt(space::XL)))
        .h(px(look.pt(size::ROW)))
        .px(px(look.pt(space::M)))
        .rounded(px(look.pt(radius::S)))
        .text_size(px(look.pt(text::BODY)))
        .text_color(hsla(ch.ink))
        .hover(move |s| s.bg(hsla(ch.hover)))
        .focus_visible(move |s| s.bg(hsla(ch.selection)).border_color(ring))
        .child(label)
        .children(shortcut.map(|s| {
            div()
                .text_color(hsla(ch.muted))
                .text_size(px(look.pt(text::SMALL)))
                .child(SharedString::from(s.to_string()))
        }))
}

/// A tooltip's frame.
pub fn tooltip(text_: impl Into<SharedString>, look: &Look) -> Stateful<Div> {
    let t = text_.into();
    semantic(floating(look), Role::Tooltip, &t)
        .px(px(look.pt(space::S)))
        .py(px(look.pt(space::XS)))
        .rounded(px(look.pt(radius::S)))
        .text_size(px(look.pt(text::SMALL)))
        .child(t)
}

/// A toast: a short message floating near the window's bottom.
pub fn toast(message: impl Into<SharedString>, look: &Look) -> Stateful<Div> {
    let m = message.into();
    semantic(floating(look), Role::Status, &m)
        .px(px(look.pt(space::L)))
        .child(m)
}

/// A dialog's window: a title, its content and its buttons, with the window elevation.
pub fn dialog(title_: impl Into<SharedString>, look: &Look) -> Stateful<Div> {
    let ch = look.chrome;
    let t: SharedString = title_.into();
    semantic(div(), Role::Dialog, &t)
        .flex()
        .flex_col()
        .gap(px(look.pt(space::L)))
        .w(px(look.pt(size::DIALOG_WIDTH)))
        .p(px(look.pt(inset::WINDOW.0)))
        .rounded(px(look.pt(radius::XL)))
        .border(px(stroke::HAIR))
        .border_color(hsla(ch.rule))
        .bg(hsla(ch.surface))
        .text_color(hsla(ch.ink))
        .shadow(tokens::shadow(tokens::elevation::WINDOW, ch.dark))
        .child(
            div()
                .text_size(px(look.pt(text::SUBTITLE)))
                .font_weight(FontWeight::SEMIBOLD)
                .child(t),
        )
}

/// The empty state (#223): what is not here yet, and the action that fills it.
pub fn empty_state(message: impl Into<SharedString>, action: Button, look: &Look) -> Div {
    let m = message.into();
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(look.pt(space::M)))
        .py(px(look.pt(space::XXXL)))
        .text_color(hsla(look.chrome.muted))
        .child(semantic(div(), Role::Label, &m).child(m.clone()))
        .child(action)
}

/// The loading state (#223): what is being loaded, with a progress bar when the fraction is
/// known.
pub fn loading_state(
    what: impl Into<SharedString>,
    fraction: Option<f32>,
    look: &Look,
) -> Stateful<Div> {
    let ch = look.chrome;
    let w = what.into();
    semantic(div(), Role::ProgressIndicator, &w)
        .flex()
        .flex_col()
        .gap(px(look.pt(space::S)))
        .text_color(hsla(ch.muted))
        .child(w)
        .children(fraction.map(|f| {
            div()
                .w(px(look.pt(size::FIELD_WIDE)))
                .h(px(look.pt(space::XS)))
                .rounded_full()
                .bg(hsla(ch.rule))
                .child(
                    div()
                        .h_full()
                        .w(px(look.pt(size::FIELD_WIDE) * f.clamp(0.0, 1.0)))
                        .rounded_full()
                        .bg(hsla(ch.lapis)),
                )
        }))
}

/// The error state (#223): what went wrong in plain words, with the details after it.
pub fn error_state(
    message: impl Into<SharedString>,
    details: impl Into<SharedString>,
    look: &Look,
) -> Stateful<Div> {
    let ch = look.chrome;
    let m = message.into();
    semantic(div(), Role::Alert, &m)
        .flex()
        .flex_col()
        .gap(px(look.pt(space::XS)))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(look.pt(space::S)))
                .text_color(hsla(ch.warn))
                .child(Icon::Warning.element(look.pt(size::ICON), hsla(ch.warn)))
                .child(m),
        )
        .child(hint(details, look))
}

/// A scroll bar's thumb beside custom-drawn content: `top` and `height` are fractions of the
/// track (the egui app's `scroll_thumb`).
pub fn scroll_bar(
    name: impl Into<SharedString>,
    top: f32,
    height: f32,
    look: &Look,
) -> Stateful<Div> {
    let ch = look.chrome;
    let track = look.pt(size::SCROLLBAR);
    semantic(div(), Role::ScrollBar, &name.into())
        .relative()
        .w(px(track))
        .h_full()
        .child(
            div()
                .absolute()
                .left(px(2.0))
                .w(px(track - 4.0))
                .top(gpui_kit::relative(top.clamp(0.0, 1.0)))
                .h(gpui_kit::relative(height.clamp(0.0, 1.0)))
                .min_h(px(look.pt(size::THUMB_MIN)))
                .rounded_full()
                .bg(hsla(ch.rule))
                .hover(move |s| s.bg(hsla(ch.muted))),
        )
}
