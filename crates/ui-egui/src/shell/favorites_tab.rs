//! The Favorites and History Tools (#155), and the ☆ in each Tab header that adds the Tab's
//! Current location to Favorites. Opening a Favorite or a Visit runs `favorite.open` or
//! `history.open`: the most recent Tab of that kind moves there, else a new one opens.

use egui::{RichText, Ui, vec2};
use obs_engine::TabId;
use serde_json::{Value, json};

use super::State;
use crate::compare_view::paint_star;
use crate::theme;

/// Height of one History row (Visits and day headings alike).
const HISTORY_ROW_H: f32 = 24.0;

/// One line of the History list.
enum HistoryLine {
    Day(String),
    Visit {
        id: i64,
        time: String,
        label: String,
    },
}

impl State {
    /// The header's ☆ (★ when the Tab's Current location is a Favorite). Nothing for Tabs
    /// without a location.
    pub(super) fn favorite_star(&mut self, ui: &mut Ui, tab: TabId) {
        if self.session.user.is_none() || self.session.location_of(tab).is_none() {
            if let Some(t) = self.tabs.get_mut(&tab) {
                t.favorite_star = None;
            }
            return;
        }
        let favorite = self.session.favorite_of(tab).map(|f| f.id);
        let (rect, r) = ui.allocate_exact_size(vec2(20.0, 22.0), egui::Sense::click());
        let color = match (favorite.is_some(), r.hovered()) {
            (true, _) => theme::c().gilt,
            (false, true) => theme::c().lapis,
            (false, false) => theme::c().muted,
        };
        paint_star(ui.painter(), rect.center(), 7.0, favorite.is_some(), color);
        let label = if favorite.is_some() {
            "Remove from Favorites"
        } else {
            "Add to Favorites (Ctrl+D)"
        };
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
        if let Some(t) = self.tabs.get_mut(&tab) {
            t.favorite_star = Some(rect);
        }
        if r.on_hover_text(label).clicked() {
            match favorite {
                Some(id) => self.run("favorite.remove", &json!({ "id": id })),
                None => self.run("favorite.add", &json!({ "tab": tab.0 })),
            };
        }
    }

    /// The ⋯ menu's "Add to Favorites folder": each folder, and a new one.
    pub(super) fn favorite_folder_menu(&mut self, ui: &mut Ui, tab: TabId) {
        if self.session.user.is_none() || self.session.location_of(tab).is_none() {
            return;
        }
        let folders = self.session.favorite_folders();
        let mut picked: Option<String> = None;
        ui.menu_button("Add to Favorites folder", |ui| {
            for f in &folders {
                if ui.button(f).clicked() {
                    picked = Some(f.clone());
                }
            }
            if !folders.is_empty() {
                ui.separator();
            }
            if let Some(name) = new_folder_field(ui, ("new-folder", tab.0)) {
                picked = Some(name);
            }
        });
        if let Some(folder) = picked {
            self.run("favorite.add", &json!({ "tab": tab.0, "folder": folder }));
            ui.close();
        }
    }

    pub(super) fn favorites_body(&mut self, ui: &mut Ui, tab: TabId) {
        let filter = self
            .tabs
            .get(&tab)
            .map(|t| t.filter.clone())
            .unwrap_or_default();
        let split = self.split_held;
        let mut run: Option<(&'static str, Value)> = None;
        egui::Frame::new()
            .fill(theme::c().field)
            .inner_margin(egui::Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                if self.session.user.is_none() {
                    muted(
                        ui,
                        "Favorites are kept in the User database, which is not open.",
                    );
                    return;
                }
                if self.session.favorites().is_empty() {
                    muted(
                        ui,
                        "No Favorites yet. Click the ☆ in a Tab's header (or press Ctrl+D) to \
                         add the place it shows.",
                    );
                    return;
                }
                let shown: Vec<(i64, String, String)> = self
                    .session
                    .favorites_matching(&filter)
                    .into_iter()
                    .map(|f| (f.id, f.folder.clone(), f.label.clone()))
                    .collect();
                if shown.is_empty() {
                    muted(ui, "No Favorites match the filter.");
                    return;
                }
                let folders = self.session.favorite_folders();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let mut folder: Option<&str> = None;
                        for (id, f, label) in &shown {
                            if folder != Some(f.as_str()) {
                                folder = Some(f.as_str());
                                if !f.is_empty() {
                                    ui.add_space(8.0);
                                    heading(ui, f);
                                }
                            }
                            let r = ui.add(
                                egui::Button::new(RichText::new(label).size(14.0))
                                    .frame(false)
                                    .min_size(vec2(ui.available_width(), 24.0)),
                            );
                            let r = r.on_hover_text("Open (Ctrl/Cmd-click: in a new split)");
                            if r.clicked() {
                                run = Some(("favorite.open", json!({ "id": id, "split": split })));
                            }
                            r.context_menu(|ui| {
                                if ui.button("Open in a new split").clicked() {
                                    run =
                                        Some(("favorite.open", json!({ "id": id, "split": true })));
                                    ui.close();
                                }
                                ui.menu_button("Move to folder", |ui| {
                                    if !f.is_empty() && ui.button("No folder").clicked() {
                                        run = Some((
                                            "favorite.move",
                                            json!({ "id": id, "folder": "" }),
                                        ));
                                        ui.close();
                                    }
                                    for other in folders.iter().filter(|o| *o != f) {
                                        if ui.button(other).clicked() {
                                            run = Some((
                                                "favorite.move",
                                                json!({ "id": id, "folder": other }),
                                            ));
                                            ui.close();
                                        }
                                    }
                                    if let Some(name) = new_folder_field(ui, ("move", *id)) {
                                        run = Some((
                                            "favorite.move",
                                            json!({ "id": id, "folder": name }),
                                        ));
                                        ui.close();
                                    }
                                });
                                if ui.button("Remove from Favorites").clicked() {
                                    run = Some(("favorite.remove", json!({ "id": id })));
                                    ui.close();
                                }
                            });
                        }
                    });
            });
        if let Some((id, params)) = run {
            self.run(id, &params);
        }
    }

    pub(super) fn history_body(&mut self, ui: &mut Ui, tab: TabId) {
        let filter = self
            .tabs
            .get(&tab)
            .map(|t| t.filter.clone())
            .unwrap_or_default();
        let confirming = self.tabs.get(&tab).is_some_and(|t| t.confirm_clear);
        let split = self.split_held;
        let mut run: Option<(&'static str, Value)> = None;
        let mut confirm: Option<bool> = None;
        egui::Frame::new()
            .fill(theme::c().field)
            .inner_margin(egui::Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                if self.session.user.is_none() {
                    muted(
                        ui,
                        "History is kept in the User database, which is not open.",
                    );
                    return;
                }
                if self.session.history().is_empty() {
                    muted(
                        ui,
                        "History is empty. The places you go in any Tab are listed here, \
                         newest first.",
                    );
                    return;
                }
                ui.horizontal(|ui| {
                    if confirming {
                        ui.label("Clear all of History? This cannot be undone.");
                        if ui.button("Clear").clicked() {
                            run = Some(("history.clear", Value::Null));
                            confirm = Some(false);
                        }
                        if ui.button("Cancel").clicked() {
                            confirm = Some(false);
                        }
                    } else if ui.button("Clear History…").clicked() {
                        confirm = Some(true);
                    }
                });
                ui.add_space(4.0);
                let mut lines = Vec::new();
                for day in self.session.history_days(&filter) {
                    lines.push(HistoryLine::Day(day.heading));
                    for v in day.visits {
                        lines.push(HistoryLine::Visit {
                            id: v.id,
                            time: v.time,
                            label: v.label,
                        });
                    }
                }
                if lines.is_empty() {
                    muted(ui, "Nothing in History matches the filter.");
                    return;
                }
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show_rows(ui, HISTORY_ROW_H, lines.len(), |ui, range| {
                        for line in &lines[range] {
                            match line {
                                HistoryLine::Day(d) => {
                                    ui.allocate_ui(
                                        vec2(ui.available_width(), HISTORY_ROW_H),
                                        |ui| {
                                            ui.add_space(6.0);
                                            ui.label(
                                                RichText::new(d).strong().color(theme::c().ink),
                                            );
                                        },
                                    );
                                }
                                HistoryLine::Visit { id, time, label } => {
                                    ui.allocate_ui(
                                        vec2(ui.available_width(), HISTORY_ROW_H),
                                        |ui| {
                                            ui.horizontal(|ui| {
                                                ui.label(
                                                    RichText::new(time)
                                                        .size(12.0)
                                                        .color(theme::c().muted),
                                                );
                                                let r = ui.add(
                                                    egui::Button::new(
                                                        RichText::new(label).size(14.0),
                                                    )
                                                    .frame(false),
                                                );
                                                if r.on_hover_text(
                                                "Go back there (Ctrl/Cmd-click: in a new split)",
                                            )
                                            .clicked()
                                            {
                                                run = Some((
                                                    "history.open",
                                                    json!({ "id": id, "split": split }),
                                                ));
                                            }
                                            });
                                        },
                                    );
                                }
                            }
                        }
                    });
            });
        if let Some(on) = confirm
            && let Some(t) = self.tabs.get_mut(&tab)
        {
            t.confirm_clear = on;
        }
        if let Some((id, params)) = run {
            self.run(id, &params);
        }
    }
}

/// A "New folder" text field and its Add button; returns the name once given.
fn new_folder_field(ui: &mut Ui, salt: impl std::hash::Hash + std::fmt::Debug) -> Option<String> {
    let id = egui::Id::unique("favorites-new-folder").with(salt);
    let mut text: String = ui.data_mut(|d| d.get_temp(id)).unwrap_or_default();
    let mut done = false;
    ui.horizontal(|ui| {
        let r = ui.add(
            egui::TextEdit::singleline(&mut text)
                .id_salt(id)
                .hint_text("New folder")
                .desired_width(140.0),
        );
        done |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        done |= ui
            .add_enabled(!text.trim().is_empty(), egui::Button::new("Add"))
            .clicked();
    });
    let name = text.trim().to_string();
    if done && !name.is_empty() {
        ui.data_mut(|d| d.remove::<String>(id));
        return Some(name);
    }
    ui.data_mut(|d| d.insert_temp(id, text));
    None
}

fn muted(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).color(theme::c().muted));
}

fn heading(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text.to_uppercase())
            .size(10.5)
            .strong()
            .color(theme::c().muted),
    );
}
