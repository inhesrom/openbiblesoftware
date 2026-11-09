//! Panels settling into a new arrangement (#225). When the dock's Panels change (a Tab docked,
//! split off, floated or closed, a Panel dropped elsewhere), every Panel takes its new size at
//! once, so its contents are laid out once, at their final width; the change is then shown
//! over [`motion::PANEL`]: an outline eases from each resized Panel's old bounds to its new
//! ones, and a new Panel fades in. Nothing is drawn with motion
//! reduced. Resizing a Panel by dragging its divider, or the window, follows the pointer and
//! is not animated.

use std::collections::HashMap;

use egui::{Rect, Stroke, StrokeKind, Ui};
use egui_tiles::{Container, Tile, TileId, Tree};
use obs_engine::{PanelId, TabId};

use crate::theme;
use crate::tokens::{corner, motion, radius, stroke};

/// A Panel settling: its outline's start and end (a new Panel has none: it fades in), and when
/// it started: at the frame after the change (`None` until then), since the frame that makes
/// a change lays its Panels out again and may itself take a while.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Settling {
    pub(crate) from: Rect,
    pub(crate) to: Rect,
    pub(crate) new: bool,
    pub(crate) start: Option<f64>,
}

#[derive(Default)]
pub(crate) struct PanelMotion {
    /// Every Panel's bounds as last drawn.
    last: HashMap<PanelId, Rect>,
    /// Panels settling into a change.
    pub(crate) settling: HashMap<PanelId, Settling>,
}

/// Every Panel (a Tabs container) of `tree` and where it was drawn.
fn panel_rects(tree: &Tree<TabId>, panels: &HashMap<TileId, PanelId>) -> HashMap<PanelId, Rect> {
    tree.tiles
        .iter()
        .filter(|(_, t)| matches!(t, Tile::Container(Container::Tabs(_))))
        .filter_map(|(id, _)| Some((*panels.get(id)?, tree.tiles.rect(*id)?)))
        .collect()
}

/// What changes when the Panels drawn as `last` are drawn as `now`: nothing unless the set of
/// Panels changed; then each Panel whose bounds moved settles from its old bounds, and each
/// new Panel fades in.
fn changes(
    last: &HashMap<PanelId, Rect>,
    now: &HashMap<PanelId, Rect>,
) -> Vec<(PanelId, Settling)> {
    let same_panels = last.len() == now.len() && now.keys().all(|p| last.contains_key(p));
    if last.is_empty() || same_panels {
        return Vec::new();
    }
    now.iter()
        .filter_map(|(p, to)| {
            let (from, new) = match last.get(p) {
                Some(old) if old == to => return None,
                Some(old) => (*old, false),
                None => (*to, true),
            };
            Some((
                *p,
                Settling {
                    from,
                    to: *to,
                    new,
                    start: None,
                },
            ))
        })
        .collect()
}

impl PanelMotion {
    /// After the dock is drawn in `ui` at frame time `now`: start settling the Panels a change
    /// moved, and draw those settling over them. Returns whether any is (draw again).
    pub(crate) fn after_tree(
        &mut self,
        ui: &Ui,
        tree: &Tree<TabId>,
        panels: &HashMap<TileId, PanelId>,
        now: f64,
        reduce: bool,
    ) -> bool {
        let rects = panel_rects(tree, panels);
        for s in self.settling.values_mut() {
            s.start.get_or_insert(now);
        }
        if reduce {
            self.settling.clear();
        } else {
            self.settling.extend(changes(&self.last, &rects));
        }
        self.last = rects;
        self.settling.retain(|p, s| {
            let start = s.start.unwrap_or(now);
            self.last.contains_key(p) && !motion::is_done(now, start, motion::PANEL)
        });
        let c = theme::c();
        let painter = ui.painter();
        for s in self.settling.values() {
            let start = s.start.unwrap_or(now);
            let e = motion::ease_out(motion::progress(now, start, motion::PANEL));
            let fade = motion::remaining(now, start, motion::PANEL);
            if s.new {
                painter.rect_filled(s.to, corner(radius::S), c.shell.gamma_multiply(fade));
                continue;
            }
            let outline =
                Rect::from_min_max(s.from.min.lerp(s.to.min, e), s.from.max.lerp(s.to.max, e));
            painter.rect_stroke(
                outline,
                corner(radius::S),
                Stroke::new(stroke::MEDIUM, c.lapis.gamma_multiply(fade)),
                StrokeKind::Inside,
            );
        }
        !self.settling.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2};

    fn r(x: f32, w: f32) -> Rect {
        Rect::from_min_size(pos2(x, 0.0), vec2(w, 500.0))
    }

    #[test]
    fn a_panel_split_off_fades_in_and_its_neighbour_settles_from_its_old_size() {
        let (a, b) = (PanelId(1), PanelId(2));
        let last = HashMap::from([(a, r(0.0, 800.0))]);
        let now = HashMap::from([(a, r(0.0, 400.0)), (b, r(400.0, 400.0))]);
        let mut got: Vec<(PanelId, Settling)> = changes(&last, &now);
        got.sort_by_key(|(p, _)| p.0);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].1.from, r(0.0, 800.0));
        assert_eq!(got[0].1.to, r(0.0, 400.0));
        assert!(!got[0].1.new);
        assert!(got[1].1.new);
        assert_eq!(got[1].1.start, None, "it starts with the next frame");
    }

    #[test]
    fn only_a_change_of_panels_settles_not_a_resize() {
        let a = PanelId(1);
        let last = HashMap::from([(a, r(0.0, 800.0))]);
        // The window or a divider resized it: it follows at once.
        assert!(changes(&last, &HashMap::from([(a, r(0.0, 600.0))])).is_empty());
        // The first frame, a whole Layout: nothing to settle from.
        assert!(changes(&HashMap::new(), &last).is_empty());
    }
}
