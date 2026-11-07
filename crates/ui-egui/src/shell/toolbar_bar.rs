//! Where the toolbar's Launchers are drawn (#141): at the Top inside the menu bar (the
//! default), or docked on the Bottom, Left or Right edge as a panel of its own. Left and Right
//! show icons only, with the labels in the tooltips. What the toolbar shows and where are
//! app preferences ([`obs_engine::ToolbarSettings`]); this file only draws them and runs the
//! commands that change them.
//!
//! - **Grip:** the dots at the toolbar's start. Dragging it marks the window edge nearest the
//!   pointer, and letting go moves the toolbar there (`view.toolbar_position`).
//! - **Right-click** on the toolbar: Position ▸, Auto-hide, Customize….
//! - **Auto-hide:** the toolbar is drawn over the Workspace, and only while the pointer is at
//!   its window edge (or on the toolbar), Alt is held, or one of its panes or menus is open.
//! - **Overflow:** Launchers a small window has no room for go into a » button's menu.
//! - **Customize toolbar…:** shows or hides each Launcher, orders them, and picks icons and
//!   labels, icons only or labels only.

use egui::{Align, Align2, Layout, Pos2, Rect, Sense, Ui, Vec2, pos2, vec2};
use obs_engine::{Launcher, LauncherLabels, ToolbarPosition, ToolbarSettings};
use serde_json::{Value, json};

use super::Shell;
use super::toolbar_look as look;

/// Space between the toolbar's buttons.
const GAP: f32 = 4.0;
/// How near the window edge the pointer reveals an Auto-hide toolbar.
pub(crate) const REVEAL: f32 = 6.0;
/// How far around the toolbar the pointer keeps an Auto-hide toolbar showing.
const KEEP: f32 = 12.0;

/// A command a toolbar menu or the Customize window asked for.
type Act = Option<(&'static str, Value)>;

impl Shell {
    /// The toolbar's grip, Launchers and » button, along `ui`'s toolbar, taking at most `room`
    /// along it (`fill`: all of it, so a right-click anywhere on a docked toolbar finds it).
    pub(super) fn launcher_strip(&mut self, ui: &mut Ui, room: f32, fill: bool) {
        let settings = self.st.session.toolbar.clone();
        let vertical = settings.position.is_vertical();
        let shows = look::ButtonShows {
            icon: settings.shows_icons(),
            label: settings.shows_labels(),
        };
        let updates = self.st.session.resource_updates().updates.len();
        let badge = |w: Launcher| if w == Launcher::Library { updates } else { 0 };
        let along = |v: Vec2| if vertical { v.y } else { v.x };
        let sizes: Vec<(Launcher, f32)> = (settings.shown().into_iter())
            .map(|w| (w, along(look::launcher_size(ui, w, shows, badge(w)))))
            .collect();
        let lengths: Vec<f32> = sizes.iter().map(|(_, l)| *l).collect();
        let (fitting, needed) = fit(
            &lengths,
            along(look::grip_size(vertical)),
            along(look::overflow_size()),
            room,
        );
        let length = if fill { room.max(needed) } else { needed };
        let size = if vertical {
            vec2(look::grip_size(true).x, length)
        } else {
            vec2(length, look::BUTTON_HEIGHT)
        };
        let layout = if vertical {
            Layout::top_down(Align::Center)
        } else {
            Layout::left_to_right(Align::Center)
        };
        let work = self.work_rect(ui);
        let mut act: Act = None;
        let mut menu_open = false;
        let mut toggle = None;
        let strip = ui.allocate_ui_with_layout(size, layout, |ui| {
            ui.set_min_size(size);
            ui.spacing_mut().item_spacing = vec2(GAP, GAP);
            // Under the buttons, so they keep their clicks: a right-click between them. Not a
            // Tab stop.
            let background = ui.interact(
                ui.max_rect(),
                ui.scope_id().with("toolbar-background"),
                Sense::CLICK,
            );
            background.context_menu(|ui| {
                menu_open = true;
                act = toolbar_menu(ui, &settings).or(act.take());
            });

            let grip = look::grip(ui, vertical);
            grip.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::Button, true, "Move the toolbar")
            });
            let grip = grip.on_hover_text(
                "Drag to another edge of the window to move the toolbar; right-click for more",
            );
            grip.context_menu(|ui| {
                menu_open = true;
                act = toolbar_menu(ui, &settings).or(act.take());
            });
            if grip.dragged()
                && let Some(p) = ui.ctx().pointer_interact_pos()
            {
                let edge = nearest_edge(work, p);
                self.st.toolbar.grip_target = Some(edge);
                look::drop_edge(ui.ctx(), edge_rect(work, edge));
            }
            if grip.drag_stopped()
                && let Some(edge) = self.st.toolbar.grip_target.take()
            {
                act = Some(("view.toolbar_position", json!({ "value": edge.id() })));
            }

            for (which, _) in &sizes[..fitting] {
                let which = *which;
                let open = self.st.toolbar.open == Some(which);
                let resp = look::launcher_button(ui, which, open, badge(which), shows);
                let name = which.title();
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
                let tip = self.launcher_tip(which, shows.label);
                let resp = resp.on_hover_text(tip);
                self.st.toolbar.buttons.insert(which, resp.rect);
                if resp.clicked() {
                    toggle = Some(which);
                }
                resp.context_menu(|ui| {
                    menu_open = true;
                    act = toolbar_menu(ui, &settings).or(act.take());
                });
            }

            if fitting < sizes.len() {
                let rest: Vec<Launcher> = sizes[fitting..].iter().map(|(w, _)| *w).collect();
                let open = rest.iter().any(|w| self.st.toolbar.open == Some(*w));
                let more = look::overflow_button(ui, open);
                more.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::Button, true, "More launchers")
                });
                let more = more.on_hover_text("More launchers");
                for w in &rest {
                    self.st.toolbar.buttons.insert(*w, more.rect);
                }
                self.st.toolbar.overflow = Some(more.rect);
                egui::Popup::menu(&more).show(|ui| {
                    menu_open = true;
                    for w in &rest {
                        if ui.button(w.title()).clicked() {
                            toggle = Some(*w);
                        }
                    }
                });
            }
        });
        self.st.toolbar.bar = Some(strip.response.rect);
        self.st.toolbar.menu_open |= menu_open;
        if let Some(which) = toggle {
            self.st.toolbar.toggle(which);
        }
        if let Some((id, params)) = act {
            self.run(id, &params);
        }
    }

    /// A Launcher's tooltip: what it offers and its shortcut, after its name when the button
    /// does not show it.
    fn launcher_tip(&self, which: Launcher, labelled: bool) -> String {
        let hint = super::toolbar::hint(which);
        let text = if labelled {
            hint.to_string()
        } else {
            format!("{}: {hint}", which.title())
        };
        super::with_key(&self.st.session, &text, which.command())
    }

    /// The window under the menu bar, where the Workspace and the docked toolbars are.
    fn work_rect(&self, ui: &Ui) -> Rect {
        let c = ui.ctx().content_rect();
        Rect::from_min_max(pos2(c.left(), self.st.toolbar.menus_bottom), c.max)
    }

    /// The toolbar outside the menu bar: docked on the Bottom, Left or Right, or (Auto-hide)
    /// over the Workspace while it is revealed. Drawn after the menu bar, before the Workspace.
    pub(super) fn toolbar_outside_menus(&mut self, ui: &mut Ui) {
        let settings = self.st.session.toolbar.clone();
        let position = settings.position;
        let vertical = position.is_vertical();
        if !settings.auto_hide {
            let panel = match position {
                ToolbarPosition::Top => return,
                ToolbarPosition::Bottom => egui::Panel::bottom("toolbar"),
                ToolbarPosition::Left => egui::Panel::left("toolbar"),
                ToolbarPosition::Right => egui::Panel::right("toolbar"),
            };
            panel
                .resizable(false)
                .show_separator_line(false)
                .exact_size(look::bar_thickness(vertical))
                .frame(look::bar_frame())
                .show(ui, |ui| {
                    let room = if vertical {
                        ui.available_height()
                    } else {
                        ui.available_width()
                    };
                    self.launcher_strip(ui, room, true);
                });
            return;
        }
        let c = ui.ctx().content_rect();
        let top = self.st.toolbar.menus_bottom;
        let tb = &self.st.toolbar;
        let zone = reveal_zone(c, position);
        let keep = tb.last_bar.map_or(zone, |b| b.union(zone)).expand(KEEP);
        let (pointer, alt) = ui.input(|i| (i.pointer.hover_pos(), i.modifiers.alt));
        let revealed = alt
            || tb.open.is_some()
            || tb.grip_target.is_some()
            || tb.menu_was_open
            || pointer.is_some_and(|p| zone.contains(p) || (tb.revealed && keep.contains(p)));
        self.st.toolbar.revealed = revealed;
        if !revealed {
            return;
        }
        let (at, pivot) = match position {
            ToolbarPosition::Top => (pos2(c.left(), top), Align2::LEFT_TOP),
            ToolbarPosition::Bottom => (c.left_bottom(), Align2::LEFT_BOTTOM),
            ToolbarPosition::Left => (pos2(c.left(), top), Align2::LEFT_TOP),
            ToolbarPosition::Right => (pos2(c.right(), top), Align2::RIGHT_TOP),
        };
        let margins = look::bar_thickness(false) - look::BUTTON_HEIGHT;
        let room = if vertical {
            c.bottom() - top - margins
        } else {
            c.width() - margins
        };
        egui::Area::new(egui::Id::unique("toolbar-auto-hide"))
            .order(egui::Order::Middle)
            .pivot(pivot)
            .fixed_pos(at)
            .fade_in(false)
            .show(ui.ctx(), |ui| {
                look::bar_frame().show(ui, |ui| self.launcher_strip(ui, room, false));
            });
    }

    /// Where the open pane drops down from: its Launcher's button (or the » button holding it),
    /// else, for a Launcher hidden from the toolbar, the toolbar's start.
    pub(super) fn pane_anchor(&self, ui: &Ui, which: Launcher) -> Rect {
        let tb = &self.st.toolbar;
        if let Some(r) = tb.buttons.get(&which) {
            return *r;
        }
        if let Some(bar) = tb.bar {
            return Rect::from_min_size(bar.min, look::overflow_size());
        }
        let work = self.work_rect(ui);
        let start = match self.st.session.toolbar.position {
            ToolbarPosition::Top | ToolbarPosition::Left => work.left_top() + vec2(8.0, 0.0),
            ToolbarPosition::Bottom => work.left_bottom() + vec2(8.0, 0.0),
            ToolbarPosition::Right => work.right_top(),
        };
        Rect::from_min_size(start, Vec2::ZERO)
    }

    /// Place the open pane by `anchor`: under it at the Top, over it at the Bottom, beside it
    /// at Left and Right; and how tall the pane may grow there.
    pub(super) fn place_pane(&mut self, ui: &Ui, anchor: Rect) {
        let c = ui.ctx().content_rect();
        let gap = 6.0;
        let (at, pivot) = match self.st.session.toolbar.position {
            ToolbarPosition::Top => (anchor.left_bottom() + vec2(0.0, gap), Align2::LEFT_TOP),
            ToolbarPosition::Bottom => (anchor.left_top() - vec2(0.0, gap), Align2::LEFT_BOTTOM),
            ToolbarPosition::Left => (anchor.right_top() + vec2(gap, 0.0), Align2::LEFT_TOP),
            ToolbarPosition::Right => (anchor.left_top() - vec2(gap, 0.0), Align2::RIGHT_TOP),
        };
        let span = if pivot == Align2::LEFT_BOTTOM {
            at.y - c.top()
        } else {
            c.bottom() - at.y
        };
        self.st.toolbar.pane_at = Some((at, pivot));
        self.st.toolbar.pane_span = span;
    }

    /// The Customize toolbar window (View → Customize toolbar…, or right-click the toolbar).
    pub(super) fn customize_window(&mut self, ctx: &egui::Context) {
        if !self.st.toolbar.customizing {
            return;
        }
        let settings = self.st.session.toolbar.clone();
        let mut open = true;
        let escape = super::escape_closes_window(ctx);
        let mut acts: Vec<(&'static str, Value)> = Vec::new();
        let mut done = false;
        let window = egui::Window::new("Customize toolbar")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(egui::RichText::new("Launchers").strong());
                let list = settings.launchers();
                let last = list.len().saturating_sub(1);
                for (i, (which, shown)) in list.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let mut on = *shown;
                        if ui.checkbox(&mut on, which.title()).changed() {
                            acts.push((
                                "toolbar.show_launcher",
                                json!({ "launcher": which.id(), "shown": on }),
                            ));
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let down = ui.add_enabled(i < last, egui::Button::new("Move down"));
                            let down = down.on_hover_text(format!("Move {} later", which.title()));
                            if down.clicked() {
                                acts.push((
                                    "toolbar.move_launcher",
                                    json!({ "launcher": which.id(), "to": i + 1 }),
                                ));
                            }
                            let up = ui.add_enabled(i > 0, egui::Button::new("Move up"));
                            let up = up.on_hover_text(format!("Move {} earlier", which.title()));
                            if up.clicked() {
                                acts.push((
                                    "toolbar.move_launcher",
                                    json!({ "launcher": which.id(), "to": i - 1 }),
                                ));
                            }
                        });
                    });
                }
                ui.label(
                    egui::RichText::new(
                        "A hidden Launcher still opens from its shortcut and the Command box.",
                    )
                    .small()
                    .color(crate::theme::c().muted),
                );
                ui.add_space(6.0);
                ui.label(egui::RichText::new("Buttons show").strong());
                ui.horizontal(|ui| {
                    for (labels, id, label) in LauncherLabels::ALL {
                        if ui.radio(settings.labels == labels, label).clicked() {
                            acts.push(("toolbar.labels", json!({ "value": id })));
                        }
                    }
                });
                if settings.position.is_vertical() {
                    ui.label(
                        egui::RichText::new(
                            "At the left or right, the toolbar shows icons only; labels are in the tooltips.",
                        )
                        .small()
                        .color(crate::theme::c().muted),
                    );
                }
                ui.add_space(6.0);
                ui.label(egui::RichText::new("Position").strong());
                ui.horizontal(|ui| {
                    for (position, id, label) in ToolbarPosition::ALL {
                        if ui.radio(settings.position == position, label).clicked() {
                            acts.push(("view.toolbar_position", json!({ "value": id })));
                        }
                    }
                });
                let mut auto_hide = settings.auto_hide;
                if ui
                    .checkbox(
                        &mut auto_hide,
                        "Auto-hide: show the toolbar when the pointer reaches its edge or Alt is held",
                    )
                    .changed()
                {
                    acts.push(("view.toolbar_auto_hide", json!({})));
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Reset").clicked() {
                        acts.push(("toolbar.reset", json!({})));
                    }
                    if ui.button("Done").clicked() {
                        done = true;
                    }
                });
            });
        if !open || done || escape(window.map(|r| r.response.layer_id)) {
            self.st.toolbar.customizing = false;
        }
        for (id, params) in acts {
            self.run(id, &params);
        }
    }
}

/// How many of the Launchers, `lengths` long along the toolbar, fit in `room` after the grip,
/// keeping room for the » button when not all do; and the length the toolbar then takes.
pub(crate) fn fit(lengths: &[f32], grip: f32, more: f32, room: f32) -> (usize, f32) {
    let all = grip + lengths.iter().map(|l| GAP + l).sum::<f32>();
    if all <= room {
        return (lengths.len(), all);
    }
    let mut used = grip + GAP + more;
    let mut n = 0;
    for l in lengths {
        if used + l + GAP > room {
            break;
        }
        used += l + GAP;
        n += 1;
    }
    (n, used)
}

/// The edge of `work` (the window under the menu bar) nearest `p`.
pub(crate) fn nearest_edge(work: Rect, p: Pos2) -> ToolbarPosition {
    let distances = [
        (ToolbarPosition::Top, p.y - work.top()),
        (ToolbarPosition::Bottom, work.bottom() - p.y),
        (ToolbarPosition::Left, p.x - work.left()),
        (ToolbarPosition::Right, work.right() - p.x),
    ];
    distances
        .into_iter()
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(ToolbarPosition::Top, |d| d.0)
}

/// Where a toolbar at `edge` of `work` sits, for the drop preview.
fn edge_rect(work: Rect, edge: ToolbarPosition) -> Rect {
    let t = look::bar_thickness(edge.is_vertical());
    match edge {
        ToolbarPosition::Top => Rect::from_min_size(work.min, vec2(work.width(), t)),
        ToolbarPosition::Bottom => {
            Rect::from_min_max(pos2(work.left(), work.bottom() - t), work.max)
        }
        ToolbarPosition::Left => Rect::from_min_size(work.min, vec2(t, work.height())),
        ToolbarPosition::Right => Rect::from_min_max(pos2(work.right() - t, work.top()), work.max),
    }
}

/// The strip along the window's `edge` where the pointer reveals an Auto-hide toolbar.
fn reveal_zone(window: Rect, edge: ToolbarPosition) -> Rect {
    match edge {
        ToolbarPosition::Top => Rect::from_min_size(window.min, vec2(window.width(), REVEAL)),
        ToolbarPosition::Bottom => {
            Rect::from_min_max(pos2(window.left(), window.bottom() - REVEAL), window.max)
        }
        ToolbarPosition::Left => Rect::from_min_size(window.min, vec2(REVEAL, window.height())),
        ToolbarPosition::Right => {
            Rect::from_min_max(pos2(window.right() - REVEAL, window.top()), window.max)
        }
    }
}

/// The toolbar's right-click menu: Position ▸, Auto-hide, Customize….
fn toolbar_menu(ui: &mut Ui, settings: &ToolbarSettings) -> Act {
    let mut act: Act = None;
    ui.menu_button((crate::icons::blank(), "Position"), |ui| {
        for (position, id, label) in ToolbarPosition::ALL {
            if ui
                .selectable_label(settings.position == position, label)
                .clicked()
            {
                act = Some(("view.toolbar_position", json!({ "value": id })));
                ui.close();
            }
        }
    });
    let check = if settings.auto_hide {
        egui::Atom::from(crate::icons::image(
            ui.ctx(),
            crate::icons::Icon::Check,
            crate::icons::BODY,
            crate::theme::c().muted,
        ))
    } else {
        crate::icons::blank()
    };
    if ui.add(egui::Button::new((check, "Auto-hide"))).clicked() {
        act = Some(("view.toolbar_auto_hide", json!({})));
    }
    if ui
        .add(egui::Button::new((crate::icons::blank(), "Customize…")))
        .clicked()
    {
        act = Some(("view.toolbar_customize", json!({})));
    }
    if act.is_some() {
        ui.close();
    }
    act
}
