//! The macOS title bar merged into the app's own top row (#297): the main window's menu bar,
//! and a Floating window's top tab bars, are the title bar, with the window's traffic lights
//! at their left, as in Safari or Xcode.
//!
//! The desktop app turns it on for macOS only ([`unified_title_bar`] on the main window's
//! viewport, [`crate::App::with_unified_title_bar`] for the rest). Then the window's content
//! fills it to the top edge and the system draws only the traffic lights over it; the window
//! keeps its title (for the Window menu, Mission Control, the Dock and VoiceOver) without
//! showing it. The top row leaves the traffic lights their room ([`traffic_lights`]),
//! collapsed in native full screen (and Split View), where the system hides them; dragging
//! empty space in it moves the window and double-clicking it zooms or minimizes it, as the
//! system's "Double-click a window's title bar to" setting says ([`SystemTitleBar`]).
//! Elsewhere (Linux, Windows, the web, tests that do not ask for it) windows keep the
//! system's own title bar and the app draws exactly what it drew before.

use egui::{Context, Rect, Response, Sense, ViewportBuilder, ViewportCommand, pos2};

use crate::tokens::size;

/// What double-clicking a window's title bar does: System Settings → Desktop & Dock →
/// "Double-click a window's title bar to".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TitleBarDoubleClick {
    /// Zoom (fills the screen, or puts back the size it had); Fill is taken as Zoom.
    #[default]
    Zoom,
    Minimize,
    /// Do nothing.
    Nothing,
}

/// The operating system's title bar setting, as the desktop app reads it.
pub trait SystemTitleBar: Send + Sync {
    /// What a double-click on the title bar does, as last read.
    fn double_click(&self) -> TitleBarDoubleClick;
    /// Read it again: the window came back to the front, so the user may have changed it.
    fn refresh(&self) {}
}

/// `builder` with its title bar merged into the window's content: the system draws the
/// traffic lights over the app's top row, and the title is kept but not shown. macOS only;
/// other systems ignore these settings.
pub fn unified_title_bar(builder: ViewportBuilder) -> ViewportBuilder {
    builder
        .with_fullsize_content_view(true)
        .with_titlebar_shown(false)
        .with_title_shown(false)
}

/// The traffic lights' room at the top left of the window `ctx` draws, in its points (they
/// keep their size at every Interface size, so the room is wider in points the smaller the
/// zoom). None in full screen, where the system hides them.
pub(crate) fn traffic_lights(ctx: &Context) -> Option<Rect> {
    if ctx.input(|i| i.viewport().fullscreen.unwrap_or(false)) {
        return None;
    }
    let zoom = ctx.zoom_factor();
    Some(Rect::from_min_max(
        pos2(0.0, 0.0),
        pos2(size::TRAFFIC_LIGHTS / zoom, size::TITLE_BAR / zoom),
    ))
}

/// What a title bar senses: clicks and drags, without taking the keyboard (it is no control
/// of its own; the window's are in the Window menu).
pub(crate) fn sense() -> Sense {
    Sense::CLICK | Sense::DRAG
}

/// Make `response`, a top row's own background (or the room beside the traffic lights), act
/// as the title bar: a drag that starts on it, not on a widget drawn over it, moves the
/// window, and a double-click on it does what the system setting says.
pub(crate) fn act_as_title_bar(ctx: &Context, response: &Response, system: &dyn SystemTitleBar) {
    let pressed_here = response.id.with("obs-title-bar-pressed");
    if ctx.input(|i| i.pointer.primary_pressed()) && response.contains_pointer() {
        // The press is on empty space when no widget drawn over the row takes its click.
        let empty = ctx.viewport(|v| v.hits.click.map(|w| w.id)) == Some(response.id);
        ctx.data_mut(|d| d.insert_temp(pressed_here, empty));
    }
    let on_empty = ctx.data(|d| d.get_temp::<bool>(pressed_here)) == Some(true);
    if response.drag_started_by(egui::PointerButton::Primary) && on_empty {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if response.double_clicked() {
        match system.double_click() {
            TitleBarDoubleClick::Zoom => {
                let zoomed = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                ctx.send_viewport_cmd(ViewportCommand::Maximized(!zoomed));
            }
            TitleBarDoubleClick::Minimize => {
                ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
            }
            TitleBarDoubleClick::Nothing => {}
        }
    }
}
