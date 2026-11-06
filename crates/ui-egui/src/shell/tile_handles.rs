//! egui_tiles' own handles, for screen readers and the keyboard (#182). The separator between
//! two Panels (drag to resize them) and the background of a Panel's tab bar (drag to move the
//! Panel) are bare `Ui::interact`s: the Tab key stops on them, yet they have no AccessKit role
//! or name. After the tree is drawn each one gets a role and a name, and a focused separator
//! moves with the arrow keys (the shell's focus ring shows which one has the keyboard).

use egui::accesskit::{Orientation, Role};
use egui::{Id, Key, Rect, Ui};
use egui_tiles::{Container, LinearDir, Tile, TileId, Tree};
use obs_engine::TabId;

/// How far one arrow-key press moves a separator, in points.
const STEP: f32 = 24.0;
/// The narrowest the arrow keys make a Panel, in points.
const MIN_PANEL: f32 = 120.0;

/// The accessible name of a separator between Panels.
pub(crate) const SEPARATOR: &str = "Resize Panels";
/// The accessible name of a tab bar's background.
pub(crate) const TAB_BAR: &str = "Panel tab bar: drag to move the Panel";

/// How many widgets `ui`'s layer has so far: the tree's are the ones added after.
pub(super) fn widgets_so_far(ui: &Ui) -> usize {
    let layer = ui.layer_id();
    ui.ctx()
        .viewport(|v| v.this_pass.widgets.get_layer(layer).count())
}

enum Handle {
    /// Between `first` and `second`, children of a Linear container.
    Separator {
        container: TileId,
        first: TileId,
        second: TileId,
        dir: LinearDir,
    },
    TabBar,
}

/// Name, and (separators) move, the tree's handles among the widgets `ui`'s layer gained
/// after the first `before`.
pub(super) fn after_tree(ui: &Ui, tree: &mut Tree<TabId>, before: usize) {
    let ctx = ui.ctx().clone();
    let layer = ui.layer_id();
    let grab = 2.0 * ui.style().interaction.resize_grab_radius_side + 0.5;
    let widgets: Vec<(Id, Rect)> = ctx.viewport(|v| {
        (v.this_pass.widgets.get_layer(layer).skip(before))
            .filter(|w| w.sense.is_focusable() && w.sense.interactive())
            .map(|w| (w.id, w.rect))
            .collect()
    });
    let focused = ctx.memory(egui::Memory::focused);
    for (id, rect) in widgets {
        let Some(handle) = classify(tree, id, rect, grab) else {
            continue;
        };
        if ctx.has_accesskit_node(id) {
            ctx.accesskit_node_builder(id, |n| {
                if n.label().is_some() || !n.labelled_by().is_empty() {
                    return;
                }
                match &handle {
                    Handle::Separator { dir, .. } => {
                        n.set_role(Role::Splitter);
                        n.set_label(SEPARATOR);
                        n.set_orientation(match dir {
                            LinearDir::Horizontal => Orientation::Vertical,
                            LinearDir::Vertical => Orientation::Horizontal,
                        });
                    }
                    Handle::TabBar => {
                        n.set_role(Role::Group);
                        n.set_label(TAB_BAR);
                    }
                }
            });
        }
        if focused != Some(id) {
            continue;
        }
        if let Handle::Separator {
            container,
            first,
            second,
            dir,
        } = handle
        {
            // The arrow keys are the separator's, not egui's for moving focus.
            ctx.memory_mut(|m| {
                m.set_focus_lock_filter(
                    id,
                    egui::EventFilter {
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        ..Default::default()
                    },
                );
            });
            let (back, forward) = match dir {
                LinearDir::Horizontal => (Key::ArrowLeft, Key::ArrowRight),
                LinearDir::Vertical => (Key::ArrowUp, Key::ArrowDown),
            };
            let by = ui.input_mut(|i| {
                let back = i.consume_key(egui::Modifiers::NONE, back);
                let forward = i.consume_key(egui::Modifiers::NONE, forward);
                STEP * (f32::from(u8::from(forward)) - f32::from(u8::from(back)))
            });
            if by != 0.0 {
                resize(tree, container, first, second, dir, by);
            }
        }
    }
}

/// What the widget `id` at `rect` is, if it is one of the tree's handles: a separator is a
/// thin strip midway between two neighbours of a Linear container; a tab bar's background
/// starts at its Tabs container's corner and is no tab.
fn classify(tree: &Tree<TabId>, id: Id, rect: Rect, grab: f32) -> Option<Handle> {
    let tiles = &tree.tiles;
    for tile in tiles.tile_ids() {
        match tiles.get(tile) {
            Some(Tile::Container(Container::Linear(linear))) => {
                let shown: Vec<TileId> = (linear.children.iter().copied())
                    .filter(|c| tiles.is_visible(*c))
                    .collect();
                for pair in shown.windows(2) {
                    let (Some(a), Some(b)) = (tiles.rect(pair[0]), tiles.rect(pair[1])) else {
                        continue;
                    };
                    let between = match linear.dir {
                        LinearDir::Horizontal => {
                            rect.width() <= grab
                                && (rect.center().x - (a.right() + b.left()) / 2.0).abs() < 1.0
                        }
                        LinearDir::Vertical => {
                            rect.height() <= grab
                                && (rect.center().y - (a.bottom() + b.top()) / 2.0).abs() < 1.0
                        }
                    };
                    if between && a.union(b).intersects(rect) {
                        return Some(Handle::Separator {
                            container: tile,
                            first: pair[0],
                            second: pair[1],
                            dir: linear.dir,
                        });
                    }
                }
            }
            Some(Tile::Container(Container::Tabs(tabs))) => {
                let Some(r) = tiles.rect(tile) else {
                    continue;
                };
                let is_tab = (tabs.children.iter()).any(|c| c.egui_id(tree.id()) == id);
                if !is_tab && (rect.min - r.min).length() < 1.0 && rect.height() < r.height() {
                    return Some(Handle::TabBar);
                }
            }
            _ => {}
        }
    }
    None
}

/// Move the separator between `first` and `second` by `by` points, keeping both at least
/// [`MIN_PANEL`] long.
fn resize(
    tree: &mut Tree<TabId>,
    container: TileId,
    first: TileId,
    second: TileId,
    dir: LinearDir,
    by: f32,
) {
    let length = |r: Rect| match dir {
        LinearDir::Horizontal => r.width(),
        LinearDir::Vertical => r.height(),
    };
    let (Some(a), Some(b)) = (tree.tiles.rect(first), tree.tiles.rect(second)) else {
        return;
    };
    let (a, b) = (length(a), length(b));
    let total = a + b;
    if total <= 2.0 * MIN_PANEL {
        return;
    }
    let new_a = (a + by).clamp(MIN_PANEL, total - MIN_PANEL);
    if let Some(Tile::Container(Container::Linear(linear))) = tree.tiles.get_mut(container) {
        let shares = linear.shares[first] + linear.shares[second];
        linear.shares.set_share(first, shares * new_a / total);
        linear
            .shares
            .set_share(second, shares * (total - new_a) / total);
    }
}
