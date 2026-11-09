//! Keyboard navigation across Panels and Tabs (#215), the UI's side: the Tab switcher shown
//! while Ctrl+Tab's Ctrl is held, the keyboard put on a Panel's Tab (Ctrl+1…9, the switcher),
//! and F6 / Shift+F6 moving the keyboard between the areas of a window: the menu bar, the
//! Toolbar, then each Panel in screen order. The commands themselves are the engine's
//! (`obs_engine::keyboard_navigation`); every key is the user's to rebind (#177).

use egui::{Id, Modifiers, Rect, Ui};
use obs_engine::{PanelId, TabId, WindowId};
use serde_json::json;

use super::{Shell, State};
use crate::tokens::size;

/// The commands whose key opens the Tab switcher.
pub(super) const SWITCHER_COMMANDS: [&str; 2] = ["tab.next_in_panel", "tab.previous_in_panel"];

/// The Tab switcher (#215): a Panel's Tabs, the focused one first, then the others most
/// recently used first. Up while a modifier of the key that opened it is held; letting go
/// shows the Tab picked, Esc leaves things as they were.
pub(crate) struct TabSwitcher {
    window: Option<WindowId>,
    pub(crate) order: Vec<TabId>,
    /// Which of `order` is picked.
    pub(crate) at: usize,
    /// The Ctrl, Cmd or Alt of the key that opened it.
    hold: Modifiers,
}

impl TabSwitcher {
    fn held(&self, now: Modifiers) -> bool {
        (self.hold.ctrl && now.ctrl)
            || (self.hold.command && now.command)
            || (self.hold.mac_cmd && now.mac_cmd)
            || (self.hold.alt && now.alt)
    }
}

/// Whether a key with `modifiers` can be held: one with Ctrl, Cmd or Alt keeps the switcher
/// up; another (a function key alone) switches at once.
fn can_hold(modifiers: Modifiers) -> bool {
    modifiers.ctrl || modifiers.command || modifiers.mac_cmd || modifiers.alt
}

/// Where the keyboard goes in each area F6 visits, besides the Panels: the first widget of
/// the menu bar and of the Toolbar, and where each was drawn, as of last frame.
#[derive(Default)]
pub(crate) struct AreaMarks {
    pub(crate) menus: Option<(Id, Rect)>,
    pub(crate) toolbar: Option<(Id, Rect)>,
}

/// One stop of F6.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Stop {
    /// A widget that takes the keyboard (the menu bar's first menu, the first Launcher).
    Widget(Id),
    Panel(PanelId),
}

impl State {
    /// Put the keyboard on `tab`'s button in its Panel's tab bar next time it is drawn.
    pub(super) fn keyboard_to(&mut self, tab: TabId) {
        self.keyboard_to = Some(tab);
    }

    /// A Tab's button was drawn: it takes the keyboard when asked to.
    pub(super) fn tab_button_drawn(&mut self, tab: TabId, button: &egui::Response) {
        if self.keyboard_to == Some(tab) {
            self.keyboard_to = None;
            button.request_focus();
        }
    }
}

impl Shell {
    /// The keyboard was sent to the main window's Command box or Launchers: the main window is
    /// brought to the front, and the Tab focused in a floating OS window, the one they act on,
    /// stays focused when it gains the keyboard.
    pub(super) fn raise_main_window(&mut self) {
        self.os_focus_to = Some(None);
        self.main_raised_keeping =
            (self.st.session.workspace.focused()).filter(|t| self.keys_window(*t).is_some());
    }

    /// The keyboard was sent to a Tab (Ctrl+1…9, the Tab switcher), or to the main window's
    /// Command box or Launchers: when floating windows are OS windows of their own, the window
    /// showing it is brought to the front and takes the keyboard, so the keys typed next reach
    /// it. Asked once all of the frame's windows are drawn; the main window included, for the
    /// way back.
    pub(super) fn focus_os_window(&mut self, ctx: &egui::Context) {
        let Some(window) = self.os_focus_to.take() else {
            return;
        };
        // Only one OS window (the web, floating windows drawn inside the main one): it has the
        // keyboard already.
        if !self.floating.iter().any(|d| d.native) {
            return;
        }
        // The main window in front already gains no keyboard: nothing is kept for it.
        if window.is_none()
            && ctx.input_for(egui::ViewportId::ROOT, |i| i.viewport().focused) == Some(true)
        {
            self.main_raised_keeping = None;
        }
        let viewport = match window {
            None => egui::ViewportId::ROOT,
            Some(w) => match self.floating.iter().find(|d| d.id == w) {
                Some(d) => d.viewport_id(),
                None => return,
            },
        };
        ctx.send_viewport_cmd_to(viewport, egui::ViewportCommand::Focus);
    }

    /// The Tab switcher's part of the keys typed in `window`: let go of its key's modifier and
    /// it shows the Tab picked; Esc closes it. Before the window's other keys are read.
    pub(super) fn tab_switcher_keys(&mut self, ui: &Ui, window: Option<WindowId>) {
        let Some(sw) = self.switcher.as_ref().filter(|s| s.window == window) else {
            return;
        };
        // Esc with the switcher's Ctrl still held.
        if ui.input_mut(|i| {
            let held = i.modifiers;
            i.consume_key(held, egui::Key::Escape)
        }) {
            self.switcher = None;
            return;
        }
        if !sw.held(ui.input(|i| i.modifiers)) {
            self.tab_switcher_pick(None);
        }
    }

    /// A switcher key was pressed in `window` on `focused`: open the switcher (or move its pick
    /// down, or up without `forward`). A key that cannot be held switches at once.
    pub(super) fn tab_switcher_step(
        &mut self,
        window: Option<WindowId>,
        focused: TabId,
        forward: bool,
        modifiers: Modifiers,
    ) {
        let id = if forward {
            SWITCHER_COMMANDS[0]
        } else {
            SWITCHER_COMMANDS[1]
        };
        if !can_hold(modifiers) {
            self.run(id, &json!({ "tab": focused.0 }));
            return;
        }
        if self.switcher.as_ref().is_none_or(|s| s.window != window) {
            let order = self.st.session.workspace.tab_switcher_order(focused);
            if order.len() < 2 {
                return;
            }
            self.switcher = Some(TabSwitcher {
                window,
                order,
                at: 0,
                hold: modifiers,
            });
        }
        if let Some(sw) = &mut self.switcher {
            let n = sw.order.len();
            sw.at = if forward {
                (sw.at + 1) % n
            } else {
                (sw.at + n - 1) % n
            };
        }
    }

    /// Show the switcher's pick (or the Tab at `at` in its list) and close it.
    fn tab_switcher_pick(&mut self, at: Option<usize>) {
        let Some(sw) = self.switcher.take() else {
            return;
        };
        let at = at.unwrap_or(sw.at);
        let Some(&from) = sw.order.first() else {
            return;
        };
        // The list may have changed under it (a Tab closed): the pick is by Tab, not place.
        let Some(&to) = sw.order.get(at) else {
            return;
        };
        let now = self.st.session.workspace.tab_switcher_order(from);
        let Some(steps) = now.iter().position(|t| *t == to) else {
            return;
        };
        self.run(
            SWITCHER_COMMANDS[0],
            &json!({ "tab": from.0, "steps": steps }),
        );
    }

    /// Draw the Tab switcher, when it is up in `window`, over its Panel.
    pub(super) fn tab_switcher_ui(&mut self, ctx: &egui::Context, window: Option<WindowId>) {
        let Some(sw) = self.switcher.as_ref().filter(|s| s.window == window) else {
            return;
        };
        let ws = &self.st.session.workspace;
        let rows: Vec<String> = (sw.order.iter())
            .map(|t| ws.tab(*t).map(|t| t.title.clone()).unwrap_or_default())
            .collect();
        let at = sw.at;
        let over = (sw.order.first())
            .and_then(|t| ws.panel_of(*t))
            .and_then(|p| self.panel_rect(p))
            .map_or_else(|| ctx.content_rect().center(), |(_, r)| r.center());
        let mut clicked = None;
        egui::Area::new(Id::unique("tab-switcher").with(window.map(|w| w.0)))
            .order(egui::Order::Foreground)
            // At once: it is up only while Ctrl is held.
            .fade_in(false)
            .pivot(egui::Align2::CENTER_CENTER)
            .fixed_pos(over)
            .show(ctx, |ui| {
                crate::components::popover_frame().show(ui, |ui| {
                    ui.set_min_width(size::SIDE_LIST);
                    ui.label(crate::components::hint_text(
                        "Tabs in this Panel, last used first",
                    ));
                    for (i, title) in rows.iter().enumerate() {
                        let r = ui.selectable_label(i == at, title);
                        if i == at {
                            r.scroll_to_me(None);
                        }
                        if r.clicked() {
                            clicked = Some(i);
                        }
                    }
                });
            });
        if clicked.is_some() {
            self.tab_switcher_pick(clicked);
        }
    }

    /// The window `panel` is drawn in (none: the main one, or a floating window drawn inside
    /// it) and its rectangle there, as of last frame.
    fn panel_rect(&self, panel: PanelId) -> Option<(Option<WindowId>, Rect)> {
        self.panel_drawn(panel).map(|(w, _, r)| (w, r))
    }

    /// Like [`Self::panel_rect`], with the layer the Panel's widgets are on.
    fn panel_drawn(
        &self,
        panel: PanelId,
    ) -> Option<(Option<WindowId>, Option<egui::LayerId>, Rect)> {
        let tile = |panels: &super::PanelTiles| {
            (panels.iter()).find_map(|(tile, p)| (*p == panel).then_some(*tile))
        };
        if let Some(t) = tile(&self.st.panels) {
            return Some((None, self.dock_layer, self.tree.tiles.rect(t)?));
        }
        self.floating.iter().find_map(|d| {
            let rect = d.tree.tiles.rect(tile(&d.panels)?)?;
            Some((d.native.then_some(d.id), d.layer, rect))
        })
    }

    /// The keyboard moved onto another widget in `window` (none: the main one), by Tab, a
    /// click or a shortcut: when it is a Panel's (its tab bar or its active Tab's header or
    /// text), that Panel's active Tab becomes the focused one, so the keys typed next act on
    /// the Tab the keyboard is in (#256). A popup or window over a Panel is not in it. Read
    /// before the window's keys.
    pub(super) fn follow_keyboard(&mut self, ctx: &egui::Context, window: Option<WindowId>) {
        let now = ctx.memory(egui::Memory::focused);
        if self.keyboard_seen.insert(window, now) == Some(now) {
            return;
        }
        let Some(widget) = now.and_then(|id| ctx.read_response(id)) else {
            return;
        };
        let at = widget.rect.center();
        let ws = &self.st.session.workspace;
        let panel = (ws.panels_in_order().into_iter())
            .filter_map(|p| Some((p, self.panel_drawn(p)?)))
            .filter(|(_, (w, layer, rect))| {
                *w == window && *layer == Some(widget.layer_id) && rect.contains(at)
            })
            .min_by(|a, b| a.1.2.area().total_cmp(&b.1.2.area()))
            .map(|(p, _)| p);
        let ws = &mut self.st.session.workspace;
        if let Some(t) = panel.and_then(|p| ws.active_in(p))
            && ws.focused() != Some(t)
        {
            ws.user_focused(t);
        }
    }

    /// F6's stops in `window`, in order, each with where it was drawn: in the main window the
    /// menu bar and the Toolbar, then the window's Panels in screen order.
    fn area_stops(&self, window: Option<WindowId>) -> Vec<(Stop, Rect)> {
        let mut stops = Vec::new();
        if window.is_none() {
            let marks = &self.st.areas;
            for (id, rect) in [marks.menus, marks.toolbar].into_iter().flatten() {
                stops.push((Stop::Widget(id), rect));
            }
        }
        for p in self.st.session.workspace.panels_in_order() {
            if let Some((w, rect)) = self.panel_rect(p)
                && w == window
            {
                stops.push((Stop::Panel(p), rect));
            }
        }
        stops
    }

    /// Move the keyboard to the next area of `window` (F6), or the previous one: from the area
    /// the keyboard is in (else the focused Tab's Panel).
    pub(super) fn cycle_area(&mut self, ctx: &egui::Context, window: Option<WindowId>) {
        let Some(forward) = self.next_area.take() else {
            return;
        };
        let stops = self.area_stops(window);
        if stops.is_empty() {
            return;
        }
        let ws = &self.st.session.workspace;
        let in_panel = |p: PanelId| stops.iter().position(|(s, _)| *s == Stop::Panel(p));
        // The smallest area round the widget with the keyboard: the Toolbar at the Top is
        // inside the menu bar.
        let here = ctx
            .memory(egui::Memory::focused)
            .and_then(|id| ctx.read_response(id))
            .and_then(|r| {
                let at = r.rect.center();
                (stops.iter().enumerate())
                    .filter(|(_, (_, rect))| rect.contains(at))
                    .min_by(|a, b| a.1.1.area().total_cmp(&b.1.1.area()))
                    .map(|(i, _)| i)
            })
            .or_else(|| {
                let tab = ws.focused()?;
                in_panel(ws.panel_of(tab)?)
            });
        let n = stops.len();
        let next = match (here, forward) {
            (Some(i), true) => (i + 1) % n,
            (Some(i), false) => (i + n - 1) % n,
            (None, true) => 0,
            (None, false) => n - 1,
        };
        match stops[next].0 {
            Stop::Widget(id) => ctx.memory_mut(|m| m.request_focus(id)),
            Stop::Panel(p) => {
                if let Some(tab) = self.st.session.workspace.active_in(p) {
                    self.st.session.workspace.focus(tab);
                    self.st.keyboard_to(tab);
                    self.apply_effects();
                }
            }
        }
    }
}
