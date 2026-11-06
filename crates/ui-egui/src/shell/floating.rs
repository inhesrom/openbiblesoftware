//! Floating windows (#152): a Tab floated out of the main window (from its right-click menu,
//! or dragged out of the window) opens in an OS window of its own, with its own egui_tiles
//! dock that can hold more Panels and Tabs. Each is an immediate egui viewport drawn by the
//! one [`Shell`] (ADR 0008), so its Tabs share the Session: Link sets, focus and commands
//! work across windows. Closing one docks its Panels back into the main window.

use egui::{Pos2, Rect, Ui, pos2, vec2};
use egui_tiles::{Tile, Tree};
use obs_engine::floating_windows::MIN_WINDOW;
use obs_engine::{PanelId, Placement, ScreenRect, TabId, WindowId, WindowPlacement};

use super::{PanelTiles, Shell, attach_beside, build_node, dock_node, insert_pane};
use crate::theme;

/// A new floating window's size when its Tab's Panel size is not known, in points.
const DEFAULT_SIZE: (f32, f32) = (640.0, 720.0);
/// How far a floating window opened from a menu sits from the main window's corner (and
/// from the one opened before it), in points.
const CASCADE: f32 = 48.0;
/// How often the app is asked for the connected screens while floating windows are open.
const SCREENS_EVERY_SECS: f64 = 2.0;
/// How many times a new window that opened somewhere other than asked is moved there.
const PLACE_TRIES: u8 = 3;

/// What the operating system measures window places in: the same units across all its
/// monitors, which egui points are not when the monitors' scale factors differ (a window's
/// points are its own pixels over its own scale). Pixels on Windows and Linux (X11), points
/// on macOS, as winit reports them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopUnits {
    Pixels,
    Points,
}

impl DesktopUnits {
    /// This platform's.
    pub const NATIVE: Self = if cfg!(target_os = "macos") {
        Self::Points
    } else {
        Self::Pixels
    };

    /// Desktop units per egui point in a window drawn at `pixels_per_point` (its scale factor
    /// times egui's zoom) with egui's `zoom`.
    pub fn per_point(self, pixels_per_point: f32, zoom: f32) -> f32 {
        match self {
            Self::Pixels => pixels_per_point,
            Self::Points => zoom,
        }
    }

    /// A monitor as winit lists it (its corner and size in physical pixels, its scale factor)
    /// in desktop units, with egui's `zoom`.
    pub fn screen(self, at: (i32, i32), size: (u32, u32), scale: f32, zoom: f32) -> ScreenRect {
        let per_pixel = match self {
            Self::Pixels => 1.0,
            Self::Points => 1.0 / scale,
        };
        ScreenRect {
            x: at.0 as f32 * per_pixel,
            y: at.1 as f32 * per_pixel,
            width: size.0 as f32 * per_pixel,
            height: size.1 as f32 * per_pixel,
            scale: self.per_point(scale * zoom, zoom),
        }
    }
}

/// One floating window: its dock, and where it is on screen.
pub(crate) struct FloatingDock {
    pub(crate) id: WindowId,
    pub(crate) tree: Tree<TabId>,
    pub(crate) panels: PanelTiles,
    /// Where the window opens: where it was made or restored, put on a connected screen
    /// before it is first shown. The user moves it from there.
    open_at: Option<WindowPlacement>,
    /// Where it is now, as its window last reported (or where it opened).
    pub(crate) placement: Option<WindowPlacement>,
    /// Shown at least once (its opening place was checked against the screens).
    shown: bool,
    /// Times it was moved to where it should open, after opening elsewhere (as when the OS
    /// placed it at another screen's scale). [`PLACE_TRIES`] once it is in place.
    place_tries: u8,
    /// An OS window of its own (else an egui window embedded in the main one, whose keys are
    /// the main window's).
    pub(crate) native: bool,
    /// The user closed its window.
    pub(crate) close: bool,
}

impl FloatingDock {
    pub(crate) fn new(id: WindowId, placement: Option<WindowPlacement>) -> Self {
        Self {
            id,
            tree: Tree::empty(egui::Id::unique(("floating-window", id.0))),
            panels: PanelTiles::new(),
            open_at: placement,
            placement,
            shown: false,
            place_tries: 0,
            native: false,
            close: false,
        }
    }

    pub(crate) fn viewport_id(&self) -> egui::ViewportId {
        egui::ViewportId::from_hash_of(("floating-window", self.id.0))
    }

    fn tab_count(&self) -> usize {
        self.tree
            .tiles
            .tiles()
            .filter(|t| matches!(t, Tile::Pane(_)))
            .count()
    }
}

/// The screens (monitors) connected, as the app last told, and the main window's place.
pub(crate) struct Screens {
    list: Vec<ScreenRect>,
    told_at: Option<f64>,
    /// The main window's outer rectangle in desktop units, and its desktop units per point.
    main: Option<(Rect, f32)>,
    /// What window places are measured in.
    pub(crate) units: DesktopUnits,
    /// Where on the desktop the pointer last was while dragging a Tab, and the desktop units
    /// per point of the window it was in: a pointer that leaves its window mid-drag is gone
    /// from that window's input.
    drag_at: Option<(TabId, Pos2, f32)>,
}

impl Default for Screens {
    fn default() -> Self {
        Self {
            list: Vec::new(),
            told_at: None,
            main: None,
            units: DesktopUnits::NATIVE,
            drag_at: None,
        }
    }
}

impl Screens {
    /// The screen a point in desktop units is on.
    fn at(&self, p: Pos2) -> Option<ScreenRect> {
        self.list.iter().copied().find(|s| s.contains(p.x, p.y))
    }

    /// Desktop units per point in the viewport being drawn.
    fn per_point(&self, ctx: &egui::Context) -> f32 {
        self.units
            .per_point(ctx.pixels_per_point(), ctx.zoom_factor())
    }

    /// Desktop units per point where a window is placed: its screen's, or the main window's
    /// when its screen is not known.
    fn per_point_at(&self, p: &WindowPlacement) -> f32 {
        p.screen
            .map(|s| s.units_per_point())
            .or(self.main.map(|(_, k)| k))
            .unwrap_or(1.0)
    }
}

/// A Tab dragged out of its window: the Tab, its pane's size, and where on the desktop the
/// pointer left it with the desktop units per point there (none: not known).
struct DraggedOut {
    tab: TabId,
    size: Option<egui::Vec2>,
    at: Option<(Pos2, f32)>,
}

/// A Tab of `tree` whose drag ended outside the window (released there, or the pointer left
/// the window while dragging it). While it is dragged, `screens` keeps where the pointer is
/// on the desktop, for when the pointer leaves the window and its position goes with it.
fn dragged_out(tree: &Tree<TabId>, ui: &Ui, screens: &mut Screens) -> Option<DraggedOut> {
    let tile = tree.dragged_id(ui.ctx())?;
    let Some(Tile::Pane(tab)) = tree.tiles.get(tile) else {
        return None;
    };
    let (released, has_pointer, pos, inner) = ui.input(|i| {
        (
            i.pointer.any_released(),
            i.pointer.has_pointer(),
            i.pointer.latest_pos(),
            i.viewport().inner_rect,
        )
    });
    let k = screens.per_point(ui.ctx());
    if let Some(at) = pos
        .zip(inner)
        .map(|(p, inner)| (inner.min + p.to_vec2()) * k)
    {
        screens.drag_at = Some((*tab, at, k));
    }
    let outside = pos.is_none_or(|p| !ui.ctx().content_rect().contains(p));
    if !(outside && (released || !has_pointer)) {
        return None;
    }
    let at = screens
        .drag_at
        .take()
        .filter(|(t, _, _)| t == tab)
        .map(|(_, at, k)| (at, k));
    Some(DraggedOut {
        tab: *tab,
        size: tree.tiles.rect(tile).map(|r| r.size()),
        at,
    })
}

impl Shell {
    /// The app tells the screens connected now, the main window's first (#152), in desktop
    /// units (see [`DesktopUnits::screen`]). A floating window restored onto a screen that is
    /// gone is moved onto one of these.
    pub fn set_screens(&mut self, screens: Vec<ScreenRect>) {
        self.screens.list = screens;
        self.screens.told_at = Some(self.st.now);
    }

    /// Whether the app should tell the screens ([`Self::set_screens`]): floating windows are
    /// open and the list is not fresh.
    pub fn wants_screens(&self) -> bool {
        !self.floating.is_empty()
            && self
                .screens
                .told_at
                .is_none_or(|t| self.st.now - t >= SCREENS_EVERY_SECS)
    }

    /// Where a new floating window opens when no place is given: cascaded from the main
    /// window's corner, at the size its Tab had.
    fn new_window_place(&self, size: Option<egui::Vec2>) -> WindowPlacement {
        let size = size.unwrap_or(vec2(DEFAULT_SIZE.0, DEFAULT_SIZE.1));
        let (corner, k) = self
            .screens
            .main
            .map_or((pos2(0.0, 0.0), 1.0), |(r, k)| (r.min, k));
        let step = CASCADE * (1 + self.floating.len()) as f32 * k;
        let at = corner + vec2(step, step);
        WindowPlacement {
            x: at.x,
            y: at.y,
            width: size.x.max(MIN_WINDOW.0),
            height: size.y.max(MIN_WINDOW.1),
            screen: self.screens.at(at),
        }
    }

    /// The pane's size where a Tab is shown now, in whichever window.
    fn pane_size(&self, tab: TabId) -> Option<egui::Vec2> {
        let trees = std::iter::once(&self.tree).chain(self.floating.iter().map(|d| &d.tree));
        for tree in trees {
            if let Some(tile) = tree.tiles.find_pane(&tab) {
                return tree.tiles.rect(tile).map(|r| r.size());
            }
        }
        None
    }

    /// [`obs_engine::Effect::Float`]: move a Tab's pane into a new floating window.
    pub(super) fn float_pane(
        &mut self,
        tab: TabId,
        window: WindowId,
        panel: PanelId,
        placement: Option<WindowPlacement>,
    ) {
        let place = placement.unwrap_or_else(|| self.new_window_place(self.pane_size(tab)));
        self.remove_pane(tab);
        let mut d = FloatingDock::new(window, Some(place));
        insert_pane(
            &mut d.tree,
            &mut d.panels,
            tab,
            Placement::NewPanel {
                panel,
                beside: None,
            },
        );
        self.floating.push(d);
    }

    /// [`obs_engine::Effect::DockWindow`]: a floating window closes, its Panels moving, as
    /// they were, to the right of the main window.
    pub(super) fn dock_floating(&mut self, window: WindowId) {
        let Some(i) = self.floating.iter().position(|d| d.id == window) else {
            return;
        };
        let d = self.floating.remove(i);
        let Some(node) = d.tree.root().and_then(|r| dock_node(&d.tree, &d.panels, r)) else {
            return;
        };
        let ws = &mut self.st.session.workspace;
        if let Some(tile) = build_node(&mut self.tree, &mut self.st.panels, ws, &node) {
            attach_beside(&mut self.tree, tile, None);
        }
    }

    /// Float a Tab dragged out of a window, where the pointer left it.
    fn float_dragged(&mut self, ctx: &egui::Context, out: DraggedOut) {
        ctx.stop_dragging();
        let size = out.size.unwrap_or(vec2(DEFAULT_SIZE.0, DEFAULT_SIZE.1));
        let place = out.at.map(|(at, from)| {
            let screen = self.screens.at(at);
            let k = screen.map_or(from, |s| s.units_per_point());
            WindowPlacement {
                // The pointer lands on the new window's title bar.
                x: at.x - size.x.min(160.0) / 2.0 * k,
                y: at.y - 12.0 * k,
                width: size.x.max(MIN_WINDOW.0),
                height: size.y.max(MIN_WINDOW.1),
                screen,
            }
        });
        let place = place.or_else(|| Some(self.new_window_place(out.size)));
        self.st.session.workspace.float(out.tab, place);
        self.apply_effects();
    }

    /// Each frame, first: where the main window is on the desktop.
    pub(super) fn note_main_window(&mut self, ctx: &egui::Context) {
        let k = self.screens.per_point(ctx);
        self.screens.main = ctx.input(|i| i.viewport().outer_rect).map(|r| (r * k, k));
    }

    /// Before the main window's dock is drawn: a Tab dragged out of it floats.
    pub(super) fn main_drag_out(&mut self, ui: &Ui) {
        if let Some(out) = dragged_out(&self.tree, ui, &mut self.screens) {
            self.float_dragged(ui.ctx(), out);
        }
    }

    /// Draw every floating window, each in its own viewport: an OS window where the backend
    /// has them, an egui window inside the main one otherwise (the web, headless tests).
    pub(super) fn show_floating_windows(&mut self, ctx: &egui::Context) {
        let ids: Vec<WindowId> = self.floating.iter().map(|d| d.id).collect();
        for id in ids {
            let Some(d) = self.floating.iter_mut().find(|d| d.id == id) else {
                continue;
            };
            if !d.shown {
                d.open_at = d.open_at.map(|p| p.on_screen(&self.screens.list));
                d.placement = d.open_at;
                d.shown = true;
            }
            let mut builder = egui::ViewportBuilder::default()
                .with_title(self.floating_title(id))
                .with_min_inner_size(vec2(MIN_WINDOW.0, MIN_WINDOW.1));
            let Some(d) = self.floating.iter().find(|d| d.id == id) else {
                continue;
            };
            if let Some(p) = d.open_at {
                // In points at the scale of the screen it opens on; moved there by its desktop
                // place once it reports one, should it open at another scale.
                let k = self.screens.per_point_at(&p);
                builder = builder
                    .with_position(pos2(p.x / k, p.y / k))
                    .with_inner_size(vec2(p.width, p.height));
            }
            let viewport = d.viewport_id();
            ctx.show_viewport_immediate(viewport, builder, |ui, class| {
                self.floating_ui(ui, id, class);
            });
        }
        let closed: Vec<WindowId> = self
            .floating
            .iter()
            .filter(|d| d.close)
            .map(|d| d.id)
            .collect();
        for w in closed {
            self.st.session.workspace.dock_window(w);
        }
        self.apply_effects();
    }

    /// A floating window's title: its Panels' active Tabs.
    fn floating_title(&self, id: WindowId) -> String {
        let ws = &self.st.session.workspace;
        let titles: Vec<String> = self
            .floating
            .iter()
            .filter(|d| d.id == id)
            .flat_map(|d| d.panels.values())
            .filter_map(|p| ws.active_in(*p))
            .filter_map(|t| ws.tab(t).map(|t| t.title.clone()))
            .collect();
        if titles.is_empty() {
            crate::APP_TITLE.to_string()
        } else {
            format!("{} — {}", titles.join(" · "), crate::APP_TITLE)
        }
    }

    /// The floating window that is an OS window of its own and shows `tab` (none: the main
    /// window, which also holds the floating windows embedded in it): the window whose keys
    /// reach `tab`.
    pub(super) fn keys_window(&self, tab: TabId) -> Option<WindowId> {
        self.floating
            .iter()
            .find(|d| d.native && d.tree.tiles.find_pane(&tab).is_some())
            .map(|d| d.id)
    }

    /// The Tab keys typed in `window` (none: the main one) act on: the focused Tab when it is
    /// in that window, else the Tab last focused there, else (when a Tab in another window is
    /// focused) the active Tab of one of its Panels.
    pub(super) fn focus_in(&self, window: Option<WindowId>) -> Option<TabId> {
        let ws = &self.st.session.workspace;
        let here = |t: &TabId| ws.tab(*t).is_some() && self.keys_window(*t) == window;
        let focused = ws.focused();
        if let Some(t) = focused.filter(here) {
            return Some(t);
        }
        if let Some(t) = self.last_focused.get(&window).copied().filter(here) {
            return Some(t);
        }
        // Nothing focused anywhere: nothing here either.
        focused?;
        let panels = match window {
            None => &self.st.panels,
            Some(w) => &self.floating.iter().find(|d| d.id == w)?.panels,
        };
        panels.values().filter_map(|p| ws.active_in(*p)).find(here)
    }

    /// One floating window's frame: its dock, the keys typed in it, its Note editor, and where
    /// it is now.
    fn floating_ui(&mut self, ui: &mut Ui, id: WindowId, class: egui::ViewportClass) {
        let ctx = ui.ctx().clone();
        // An egui window inside the main one (no OS windows) reads the main window's input:
        // its close, place and keys are not this window's.
        let native = class != egui::ViewportClass::EmbeddedWindow;
        let (closing, outer, inner) = if native {
            ctx.input(|i| {
                let v = i.viewport();
                (v.close_requested(), v.outer_rect, v.inner_rect)
            })
        } else {
            (false, None, None)
        };
        let k = self.screens.per_point(&ctx);
        let screens = &self.screens;
        let Some(d) = self.floating.iter_mut().find(|d| d.id == id) else {
            return;
        };
        d.native = native;
        if closing {
            d.close = true;
        }
        if let (Some(outer), Some(inner)) = (outer, inner) {
            let at = outer.min * k;
            // Opened elsewhere than asked (at another screen's scale than the one it opened
            // on): moved there, in this window's points now that its scale is known.
            if let Some(want) = d.open_at
                && d.place_tries < PLACE_TRIES
                && (want.x - at.x).abs().max((want.y - at.y).abs()) > 1.0
            {
                d.place_tries += 1;
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                    pos2(want.x, want.y) / k,
                ));
            } else {
                d.place_tries = PLACE_TRIES;
            }
            d.placement = Some(WindowPlacement {
                x: at.x,
                y: at.y,
                width: inner.width(),
                height: inner.height(),
                screen: screens.at(outer.center() * k),
            });
        }
        // Ctrl/Cmd held in this window, not the main one, opens what is picked here in a new
        // Panel.
        let split_held = self.st.split_held;
        self.st.split_held = ctx.input(|i| i.modifiers.command);
        // Keys typed in this window run like keys typed in the main one, on its Tabs.
        if native {
            self.shortcuts(ui, Some(id));
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::c().shell)
                    .inner_margin(egui::Margin::same(8)),
            )
            .show(ui, |ui| {
                let Some(d) = self.floating.iter().find(|d| d.id == id) else {
                    return;
                };
                // Dragging the only Tab out of its window would make the same window again.
                if d.tab_count() > 1
                    && let Some(out) = dragged_out(&d.tree, ui, &mut self.screens)
                {
                    self.float_dragged(&ctx, out);
                }
                let Some(d) = self.floating.iter_mut().find(|d| d.id == id) else {
                    return;
                };
                // The dock's behaviour finds Panels by tile in `st.panels`: this window's,
                // while it is drawn.
                std::mem::swap(&mut self.st.panels, &mut d.panels);
                d.tree.ui(&mut self.st, ui);
                std::mem::swap(&mut self.st.panels, &mut d.panels);
            });
        // A Note being written on one of this window's Tabs is written here.
        if native
            && let Some(ed) = &self.st.note_editor
            && self.keys_window(ed.tab) == Some(id)
        {
            self.st.note_editor_ui(&ctx);
        }
        self.st.split_held = split_held;
        self.apply_effects();
    }
}
