//! The Search Tool's sidebar (#179): the Saved searches, by name, and the Search history,
//! newest first. A click runs one in this Search Tab with its mode and Search scope as kept
//! (Ctrl/Cmd-click: in a new split); a Saved search's right-click menu deletes it, and Clear
//! (after a confirm) empties the history. Saving is the query row's **Save…** button
//! ([`State::save_search_button`]).

use egui::{RichText, Ui, vec2};
use obs_engine::{SavedSearch, Session, TabId};
use serde_json::{Value, json};

use super::State;
use crate::theme;

/// The sidebar shows by itself in a Search Tab at least this wide (until toggled).
pub(super) const SIDEBAR_MIN_TAB_WIDTH: f32 = 640.0;

fn heading(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text.to_uppercase())
            .size(10.5)
            .strong()
            .color(theme::c().muted),
    );
}

fn muted(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).small().color(theme::c().muted));
}

/// One clickable row: the label, with a tooltip of the search's summary (when the label is
/// a name), its Search scope's details ([`Session::search_scope_details`], made only while
/// hovered) and `action`.
fn row(
    ui: &mut Ui,
    session: &Session,
    label: &str,
    summary: Option<&str>,
    search: &SavedSearch,
    action: &str,
) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label).size(13.0))
            .frame(false)
            .truncate()
            .min_size(vec2(ui.available_width(), 22.0)),
    )
    .on_hover_ui(|ui| {
        if let Some(summary) = summary {
            ui.label(summary);
        }
        ui.label(session.search_scope_details(search));
        ui.label(RichText::new(action).small().color(theme::c().muted));
    })
}

impl State {
    /// The sidebar's lists; what is clicked goes into `run`.
    pub(super) fn search_sidebar(
        &mut self,
        ui: &mut Ui,
        tab: TabId,
        run: &mut Vec<(&'static str, Value)>,
    ) {
        let split = self.split_held;
        let session = &self.session;
        let saved: Vec<(String, String, SavedSearch)> = session
            .saved_searches()
            .iter()
            .map(|n| (n.name.clone(), n.search.summary(), n.search.clone()))
            .collect();
        let history: Vec<(String, SavedSearch)> = session
            .search_history()
            .iter()
            .map(|e| (e.search.summary(), e.search.clone()))
            .collect();
        let confirming = self
            .tabs
            .get(&tab)
            .is_some_and(|t| t.search_fields.confirm_clear_history);
        let mut confirm: Option<bool> = None;
        egui::ScrollArea::vertical()
            .id_salt(("search-sidebar-scroll", tab))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                heading(ui, "Saved searches");
                if saved.is_empty() {
                    muted(ui, "None yet. Save… beside Search keeps a search by name.");
                }
                for (name, summary, search) in &saved {
                    let action = "Run (Ctrl/Cmd-click: in a new split)";
                    let r = row(ui, session, name, Some(summary), search, action);
                    if r.clicked() {
                        run.push((
                            "search.open_saved",
                            json!({ "name": name, "tab": (!split).then_some(tab.0), "split": split }),
                        ));
                    }
                    r.context_menu(|ui| {
                        if ui.button("Run in a new split").clicked() {
                            run.push(("search.open_saved", json!({ "name": name, "split": true })));
                            ui.close();
                        }
                        if ui.button("Delete Saved search").clicked() {
                            run.push(("search.delete_saved", json!({ "name": name })));
                            ui.close();
                        }
                    });
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    heading(ui, "History");
                    if !history.is_empty() && !confirming {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Clear").clicked() {
                                confirm = Some(true);
                            }
                        });
                    }
                });
                if confirming {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Clear the Search history?").small());
                        if ui.small_button("Clear").clicked() {
                            run.push(("search.clear_history", json!({})));
                            confirm = Some(false);
                        }
                        if ui.small_button("Cancel").clicked() {
                            confirm = Some(false);
                        }
                    });
                }
                if history.is_empty() {
                    muted(ui, "Searches you run are listed here.");
                }
                for (index, (summary, search)) in history.iter().enumerate() {
                    let action = "Run again (Ctrl/Cmd-click: in a new split)";
                    if row(ui, session, summary, None, search, action).clicked() {
                        run.push((
                            "search.open_history",
                            json!({ "index": index, "tab": (!split).then_some(tab.0), "split": split }),
                        ));
                    }
                }
            });
        if let Some(on) = confirm
            && let Some(t) = self.tabs.get_mut(&tab)
        {
            t.search_fields.confirm_clear_history = on;
        }
    }

    /// The query row's **Save…** button: a name field (the query's text to start with) and
    /// Save, which runs `search.save` for this Tab with the query and Scope fields as typed
    /// (`query`, `scope`), run or not.
    pub(super) fn save_search_button(
        &mut self,
        ui: &mut Ui,
        tab: TabId,
        query: &str,
        scope: &str,
        run: &mut Vec<(&'static str, Value)>,
    ) {
        let Some(t) = self.tabs.get_mut(&tab) else {
            return;
        };
        let save_name = &mut t.search_fields.save_name;
        // Clicking into the name field must not close the menu.
        let config = egui::containers::menu::MenuConfig::new()
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
        let r = ui.add_enabled_ui(!query.trim().is_empty(), |ui| {
            egui::containers::menu::MenuButton::new("Save…")
                .config(config)
                .ui(ui, |ui| {
                    // The query's text to start with, each time the menu opens.
                    let name = save_name.get_or_insert_with(|| query.trim().to_string());
                    ui.label(RichText::new("Save this search as").small());
                    let field = ui.add(
                        egui::TextEdit::singleline(name)
                            .id_salt(("search-save-name", tab))
                            .desired_width(200.0),
                    );
                    let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if enter {
                        ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                    }
                    let named = !name.trim().is_empty();
                    if (ui.add_enabled(named, egui::Button::new("Save")).clicked() || enter)
                        && named
                    {
                        run.push((
                            "search.save",
                            json!({
                                "tab": tab.0,
                                "name": name.trim(),
                                "text": query,
                                "scope": scope,
                            }),
                        ));
                        ui.close();
                    }
                })
        });
        let (button, menu) = r.inner;
        if menu.is_none() {
            *save_name = None;
        }
        button
            .on_hover_text("Keep this search, with its mode and Search scope, by name")
            .on_disabled_hover_text("Type a search to save it");
    }
}
