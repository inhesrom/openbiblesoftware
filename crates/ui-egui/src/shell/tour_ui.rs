//! The First-run tour's Coach marks (#218): one card at a time beside the part of the window
//! it explains, which is ringed. The engine says which Coach mark shows
//! ([`obs_engine::Tour`]); the shell notes where each target was drawn this frame
//! ([`TourUi::target`]). A target not on screen (the toolbar Auto-hidden, no Bible open)
//! puts its Coach mark in the middle of the window. Next (Done on the last), Back and Skip tour
//! step through it; Esc skips it. The card does not block the window: everything it points at
//! can be tried while it shows.

use std::collections::HashMap;

use egui::{Align2, Rect, RichText, Stroke, pos2, vec2};
use obs_engine::{Launcher, TOUR_STEPS, TourTarget};
use serde_json::json;

use super::State;
use crate::theme;
use crate::tokens::{radius, size, space, stroke};

/// Where each part of the window a Coach mark can point at was drawn this frame, and where the
/// Coach mark itself was (for tests).
#[derive(Debug, Default)]
pub(crate) struct TourUi {
    targets: HashMap<TourTarget, Rect>,
    /// The Coach mark drawn last frame: what it pointed at (none: the middle of the window)
    /// and where the card was.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) drawn: Option<(Option<TourTarget>, Rect)>,
}

impl TourUi {
    /// Forget where the targets were: each frame draws them again.
    pub(crate) fn begin_frame(&mut self) {
        self.targets.clear();
    }

    /// `target` was drawn at `rect` this frame. `preferred`: this one is the best of several
    /// (the focused Tab's), so it replaces one noted before.
    pub(crate) fn target(&mut self, target: TourTarget, rect: Rect, preferred: bool) {
        if preferred || !self.targets.contains_key(&target) {
            self.targets.insert(target, rect);
        }
    }
}

/// Where a Coach mark's card goes beside `target` in `screen`: under it when there is room,
/// else above it, else beside it; its pivot corner and that point.
fn place(target: Rect, screen: Rect) -> (Align2, egui::Pos2) {
    let room = size::POPOVER_ROOM + size::POPOVER_ROOM;
    let x = target.center().x.clamp(
        screen.left() + size::POPOVER / 2.0,
        screen.right() - size::POPOVER / 2.0,
    );
    if screen.bottom() - target.bottom() >= room {
        (Align2::CENTER_TOP, pos2(x, target.bottom() + space::L))
    } else if target.top() - screen.top() >= room {
        (Align2::CENTER_BOTTOM, pos2(x, target.top() - space::L))
    } else if screen.right() - target.right() >= size::POPOVER + space::L {
        (
            Align2::LEFT_CENTER,
            pos2(target.right() + space::L, target.center().y),
        )
    } else {
        (Align2::CENTER_CENTER, screen.center())
    }
}

impl State {
    /// Where `target` was drawn this frame: the Launchers' buttons as the toolbar noted them,
    /// the toolbar as the menu bar and the Launchers' bar, the rest as the shell noted them.
    pub(crate) fn target_rect(&self, target: TourTarget) -> Option<Rect> {
        let noted = self.tour_ui.targets.get(&target).copied();
        let button = |l: Launcher| self.toolbar.buttons.get(&l).copied();
        match target {
            TourTarget::Library => button(Launcher::Library),
            TourTarget::PassageGuide => button(Launcher::Guides),
            TourTarget::Toolbar => match (noted, self.toolbar.bar) {
                (Some(a), Some(b)) => Some(a.union(b)),
                (a, b) => a.or(b),
            },
            _ => noted,
        }
    }

    /// Esc skips the tour (before any command bound to Esc, Unpin Word Info say), unless a menu
    /// or pop-up is open: Esc closes that first.
    pub(super) fn tour_keys(&mut self, ctx: &egui::Context) {
        if !self.session.tour().is_running() || egui::Popup::is_any_open(ctx) {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.run("tour.skip", &json!({}));
        }
    }

    /// The Coach mark showing, if the tour is running: its target ringed and its card beside it.
    pub(super) fn coach_mark(&mut self, ctx: &egui::Context) {
        let Some((i, step)) = self.session.tour().step() else {
            self.tour_ui.drawn = None;
            return;
        };
        let screen = ctx.content_rect();
        let target = self
            .target_rect(step.target)
            .filter(|r| r.is_positive() && screen.intersects(*r));
        let c = theme::c();
        if let Some(t) = target {
            let layer =
                egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("coach-mark-ring"));
            ctx.layer_painter(layer).rect_stroke(
                t.expand(space::XS),
                radius::M,
                Stroke::new(stroke::HEAVY, c.lapis),
                egui::StrokeKind::Outside,
            );
        }
        let last = i + 1 == TOUR_STEPS.len();
        let mut act = None;
        let area = egui::Area::new(egui::Id::unique("coach-mark"))
            .order(egui::Order::Foreground)
            .constrain(true);
        let area = match target {
            Some(t) => {
                let (pivot, at) = place(t, screen);
                area.pivot(pivot).fixed_pos(at)
            }
            None => area.anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)),
        };
        let shown = area.show(ctx, |ui| {
            crate::components::popover_frame()
                .stroke(Stroke::new(theme::focus_ring_width(), c.lapis))
                .show(ui, |ui| {
                    ui.set_max_width(size::POPOVER);
                    ui.label(crate::components::caption_text(format!(
                        "Tour · {} of {}",
                        i + 1,
                        TOUR_STEPS.len()
                    )));
                    ui.label(crate::components::subtitle_text(step.title));
                    ui.label(RichText::new(step.text).color(c.ink));
                    ui.add_space(space::S);
                    ui.horizontal(|ui| {
                        if crate::components::link(ui, "Skip tour").clicked() {
                            act = Some("tour.skip");
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let next = ui.add(
                                crate::components::button(if last { "Done" } else { "Next" })
                                    .fill(c.lapis.gamma_multiply(crate::tokens::alpha::TINT)),
                            );
                            if next.clicked() {
                                act = Some("tour.next");
                            }
                            if i > 0 && ui.add(crate::components::button("Back")).clicked() {
                                act = Some("tour.back");
                            }
                        });
                    });
                });
        });
        self.tour_ui.drawn = Some((target.map(|_| step.target), shown.response.rect));
        if let Some(id) = act {
            self.run(id, &json!({}));
        }
    }
}
