//! Floating windows (#152): a Tab floated out of the main window (from its right-click menu,
//! or dragged out of the window) opens in an OS window of its own, with its own egui_tiles
//! dock that can hold more Panels and Tabs. Each is an immediate egui viewport drawn by the
//! one [`Shell`] (ADR 0008), so its Tabs share the Session: Link sets, focus and commands
//! work across windows. Closing one docks its Panels back into the main window.

use egui::{Rect, Ui, pos2, vec2};
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
#[derive(Default)]
pub(crate) struct Screens {
    list: Vec<ScreenRect>,
    told_at: Option<f64>,
    main: Option<Rect>,
}

/// A Tab dragged out of its window: the Tab, its pane's size, and where on the desktop the
/// pointer left it (none: not known).
struct DraggedOut {
    tab: TabId,
    size: Option<egui::Vec2>,
    at: Option<egui::Pos2>,
}

/// A Tab of `tree` whose drag ended outside the window (released there, or the pointer left
/// the window while dragging it).
fn dragged_out(tree: &Tree<TabId>, ui: &Ui) -> Option<DraggedOut> {
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
    let outside = pos.is_none_or(|p| !ui.ctx().content_rect().contains(p));
    if !(outside && (released || !has_pointer)) {
        return None;
    }
    Some(DraggedOut {
        tab: *tab,
        size: tree.tiles.rect(tile).map(|r| r.size()),
        at: pos.zip(inner).map(|(p, inner)| inner.min + p.to_vec2()),
    })
}

impl Shell {
    /// The app tells the screens connected now, the main window's first (#152), in points
    /// on the desktop. A floating window restored onto a screen that is gone is moved onto
    /// one of these.
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
        let corner = self.screens.main.map_or(pos2(0.0, 0.0), |r| r.min);
        let step = CASCADE * (1 + self.floating.len()) as f32;
        WindowPlacement {
            x: corner.x + step,
            y: corner.y + step,
            width: size.x.max(MIN_WINDOW.0),
            height: size.y.max(MIN_WINDOW.1),
            screen: None,
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
        let place = out.at.map(|at| WindowPlacement {
            // The pointer lands on the new window's title bar.
            x: at.x - size.x.min(160.0) / 2.0,
            y: at.y - 12.0,
            width: size.x.max(MIN_WINDOW.0),
            height: size.y.max(MIN_WINDOW.1),
            screen: None,
        });
        let place = place.or_else(|| Some(self.new_window_place(out.size)));
        self.st.session.workspace.float(out.tab, place);
        self.apply_effects();
    }

    /// Before the main window's dock is drawn: a Tab dragged out of it floats.
    pub(super) fn main_drag_out(&mut self, ui: &Ui) {
        self.screens.main = ui.input(|i| i.viewport().outer_rect);
        if let Some(out) = dragged_out(&self.tree, ui) {
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
                builder = builder
                    .with_position(pos2(p.x, p.y))
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

    /// One floating window's frame: its dock, the keys typed in it, and where it is now.
    fn floating_ui(&mut self, ui: &mut Ui, id: WindowId, class: egui::ViewportClass) {
        let ctx = ui.ctx().clone();
        // An egui window inside the main one (no OS windows) reads the main window's input:
        // its close and place are not this window's.
        let (closing, outer, inner) = if class == egui::ViewportClass::EmbeddedWindow {
            (false, None, None)
        } else {
            ctx.input(|i| {
                let v = i.viewport();
                (v.close_requested(), v.outer_rect, v.inner_rect)
            })
        };
        let screens = &self.screens.list;
        let Some(d) = self.floating.iter_mut().find(|d| d.id == id) else {
            return;
        };
        if closing {
            d.close = true;
        }
        if let (Some(outer), Some(inner)) = (outer, inner) {
            let centre = outer.center();
            d.placement = Some(WindowPlacement {
                x: outer.min.x,
                y: outer.min.y,
                width: inner.width(),
                height: inner.height(),
                screen: screens.iter().copied().find(|s| {
                    Rect::from_min_size(pos2(s.x, s.y), vec2(s.width, s.height)).contains(centre)
                }),
            });
        }
        // Keys typed in this window run like keys typed in the main one.
        self.shortcuts(ui);
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
                    && let Some(out) = dragged_out(&d.tree, ui)
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
        self.apply_effects();
    }
}
