//! The Search Tool's Tab (#59, #60; #16 rules 9–12 and 14): the Bible / Library / Notes
//! switch, the query box (runs on Enter, recent queries per mode), inline errors, then the
//! mode's Search scope (Bible: the Scope field with its presets and the Bible picker;
//! Library: the Resource picker by type or Resource), the summary line and the results (the
//! Verse list, or paragraphs grouped by Resource). ↑/↓ move through the results and Enter
//! opens one; Ctrl/Cmd-click opens it in a new Tab.

use egui::{RichText, Stroke, Ui, vec2};
use obs_engine::bible_search::{ErrorField, InlineError, PAGE_ROWS, SearchMode, scope_presets};
use obs_engine::library_search::{LIBRARY_TYPES, type_id, type_plural};
use obs_engine::{SearchTab, TabId};
use obs_model::ResourceType;
use serde_json::{Value, json};

use super::State;
use crate::search_view::{SearchView, search_pane};
use crate::theme;

/// The query and Scope fields as typed, and what the engine last said they were (a search
/// handed over by another Tool replaces what is typed).
#[derive(Default)]
pub(super) struct Fields {
    query: String,
    scope: String,
    seen: Option<(String, String)>,
}

/// An inline error under its field, the offending part quoted.
fn error_line(ui: &mut Ui, error: Option<&InlineError>, field: ErrorField, text: &str) {
    let Some(e) = error.filter(|e| e.field == field) else {
        return;
    };
    let quoted = e
        .span
        .clone()
        .and_then(|s| text.get(s))
        .filter(|q| !q.trim().is_empty() && !e.message.contains(*q))
        .map(|q| format!(" (“{q}”)"))
        .unwrap_or_default();
    ui.label(
        RichText::new(format!("{}{quoted}", e.message))
            .small()
            .color(theme::c().warn),
    );
}

/// What the empty query box says.
pub(super) fn query_hint(mode: SearchMode) -> &'static str {
    match mode {
        SearchMode::Bible => "Words, \"a phrase\", OR, -word, lov*, strong:H1254, lemma:",
        SearchMode::Library => {
            "Commentaries, Lexicons, Dictionaries: words, \"a phrase\", OR, lov*"
        }
        SearchMode::Notes => "Search your notes",
    }
}

impl State {
    pub(super) fn search_body(&mut self, ui: &mut Ui, tab: TabId) {
        let Some(st) = self.session.search_tab(tab).cloned() else {
            return;
        };
        let context = self.session.search_context(tab);
        let Some(t) = self.tabs.get_mut(&tab) else {
            return;
        };
        let engine_fields = (st.query.clone(), st.scope.clone());
        if t.search_fields.seen.as_ref() != Some(&engine_fields) {
            t.search_fields.query = st.query.clone();
            t.search_fields.scope = st.scope.clone();
            t.search_fields.seen = Some(engine_fields);
        }
        let mut run: Vec<(&'static str, Value)> = Vec::new();
        let mut query = std::mem::take(&mut t.search_fields.query);
        let mut scope = std::mem::take(&mut t.search_fields.scope);
        let recent = self.session.recent_searches(st.mode);
        let bibles = st.bibles(&self.session.library);
        if st.mode == SearchMode::Bible {
            self.session.warm_bible_outlines(&bibles);
        }
        let abbrev = |id: &str| {
            self.session
                .library
                .get(id)
                .map_or_else(|| id.to_string(), |e| e.manifest.abbreviation.clone())
        };
        let chosen: Vec<(String, String)> =
            bibles.iter().map(|id| (id.clone(), abbrev(id))).collect();
        let others: Vec<(String, String)> = self
            .session
            .library
            .of_type(ResourceType::Bible)
            .filter(|e| !bibles.contains(&e.manifest.resource_id))
            .map(|e| {
                (
                    e.manifest.resource_id.clone(),
                    format!("{} — {}", e.manifest.abbreviation, e.manifest.title),
                )
            })
            .collect();
        let error = st.error.clone();
        egui::Frame::new()
            .fill(theme::c().surface)
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    for m in SearchMode::ALL {
                        if ui.selectable_label(st.mode == m, m.label()).clicked() && st.mode != m {
                            run.push(("search.mode", json!({ "tab": tab.0, "value": m.id() })));
                        }
                    }
                });
                ui.horizontal(|ui| {
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut query)
                            .id_salt(("search-query", tab))
                            .hint_text(query_hint(st.mode))
                            .desired_width((ui.available_width() - 120.0).max(120.0))
                            .margin(egui::Margin::symmetric(8, 4)),
                    );
                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if enter {
                        // The same Enter must not also open the selected result.
                        ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                    }
                    if enter || ui.button("Search").clicked() {
                        run.push((
                            "search.run",
                            json!({ "tab": tab.0, "text": query.clone(), "scope": scope.clone() }),
                        ));
                    }
                    ui.menu_button("Recent", |ui| {
                        if recent.is_empty() {
                            ui.label(RichText::new("No recent searches").color(theme::c().muted));
                        }
                        for q in &recent {
                            if ui.button(q).clicked() {
                                run.push(("search.run", json!({ "tab": tab.0, "text": q })));
                                ui.close();
                            }
                        }
                    });
                });
                error_line(ui, error.as_ref(), ErrorField::Query, &query);
                if st.mode == SearchMode::Library {
                    self.resource_picker(ui, tab, &st, &mut run);
                    error_line(ui, error.as_ref(), ErrorField::Resources, "");
                    error_line(ui, error.as_ref(), ErrorField::Search, "");
                }
                if st.mode != SearchMode::Bible {
                    return;
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Scope").small().color(theme::c().muted));
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut scope)
                            .id_salt(("search-scope", tab))
                            .hint_text("Whole Bible, or a Passage list: Rom 1-8; Gal")
                            .desired_width(220.0)
                            .margin(egui::Margin::symmetric(8, 3)),
                    );
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                        run.push((
                            "search.scope",
                            json!({ "tab": tab.0, "value": scope.clone() }),
                        ));
                    }
                    ui.menu_button("▾", |ui| {
                        for p in scope_presets(context) {
                            if ui.button(&p.label).clicked() {
                                run.push((
                                    "search.scope",
                                    json!({ "tab": tab.0, "value": p.value }),
                                ));
                                ui.close();
                            }
                        }
                    })
                    .response
                    .on_hover_text("Scope presets");
                });
                error_line(ui, error.as_ref(), ErrorField::Scope, &scope);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(RichText::new("Bibles").small().color(theme::c().muted));
                    for (id, abbreviation) in &chosen {
                        egui::Frame::new()
                            .fill(theme::c().field)
                            .stroke(Stroke::new(1.0, theme::c().rule))
                            .corner_radius(11.0)
                            .inner_margin(egui::Margin::symmetric(6, 0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 2.0;
                                    ui.label(RichText::new(abbreviation).size(12.5));
                                    let x = egui::Button::new(RichText::new("×").size(12.0))
                                        .frame(false)
                                        .min_size(vec2(16.0, 20.0));
                                    if ui
                                        .add(x)
                                        .on_hover_text("Do not search this Bible")
                                        .clicked()
                                    {
                                        run.push((
                                            "search.bible",
                                            json!({ "tab": tab.0, "id": id }),
                                        ));
                                    }
                                });
                            });
                    }
                    ui.menu_button(RichText::new("+ Bible").size(12.5), |ui| {
                        if others.is_empty() {
                            ui.label(
                                RichText::new("Every Bible is searched").color(theme::c().muted),
                            );
                        }
                        for (id, label) in &others {
                            if ui.button(label).clicked() {
                                run.push(("search.bible", json!({ "tab": tab.0, "id": id })));
                                ui.close();
                            }
                        }
                    });
                });
                error_line(ui, error.as_ref(), ErrorField::Bibles, "");
                error_line(ui, error.as_ref(), ErrorField::Search, "");
            });
        if let Some(t) = self.tabs.get_mut(&tab) {
            t.search_fields.query = query;
            t.search_fields.scope = scope;
        }
        if let Some(ticket) = st.mode.ticket() {
            super::placeholder(ui, &format!("{} search", st.mode.label()), ticket);
        } else {
            self.search_results(ui, tab, &st);
        }
        for (id, params) in run {
            self.run(id, &params);
        }
    }

    /// Library mode's Search scope: a chip for each Resource type and single Resource
    /// searched (× leaves it out), and a menu of checkboxes by type and Resource.
    fn resource_picker(
        &self,
        ui: &mut Ui,
        tab: TabId,
        st: &SearchTab,
        run: &mut Vec<(&'static str, Value)>,
    ) {
        let library = &self.session.library;
        let scope = &st.library;
        let chip = |ui: &mut Ui, label: &str, hover: &str| -> bool {
            let mut clicked = false;
            egui::Frame::new()
                .fill(theme::c().field)
                .stroke(Stroke::new(1.0, theme::c().rule))
                .corner_radius(11.0)
                .inner_margin(egui::Margin::symmetric(6, 0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        ui.label(RichText::new(label).size(12.5));
                        let x = egui::Button::new(RichText::new("×").size(12.0))
                            .frame(false)
                            .min_size(vec2(16.0, 20.0));
                        clicked = ui.add(x).on_hover_text(hover).clicked();
                    });
                });
            clicked
        };
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(RichText::new("Resources").small().color(theme::c().muted));
            for ty in &scope.types {
                if library.of_type(*ty).next().is_none() {
                    continue;
                }
                if chip(ui, type_plural(*ty), "Do not search these") {
                    run.push((
                        "search.resource_type",
                        json!({ "tab": tab.0, "value": type_id(*ty) }),
                    ));
                }
            }
            for id in &scope.resources {
                let Some(e) = library.get(id) else {
                    continue;
                };
                if chip(ui, &e.manifest.abbreviation, "Do not search this Resource") {
                    run.push(("search.resource", json!({ "tab": tab.0, "id": id })));
                }
            }
            ui.menu_button(RichText::new("Choose…").size(12.5), |ui| {
                let mut any = false;
                for ty in LIBRARY_TYPES {
                    let entries: Vec<_> = library.of_type(ty).collect();
                    if entries.is_empty() {
                        continue;
                    }
                    any = true;
                    let mut all = scope.types.contains(&ty);
                    if ui
                        .checkbox(&mut all, format!("All {}", type_plural(ty)))
                        .clicked()
                    {
                        run.push((
                            "search.resource_type",
                            json!({ "tab": tab.0, "value": type_id(ty) }),
                        ));
                    }
                    ui.indent(("search-resources", type_id(ty)), |ui| {
                        for e in entries {
                            let id = &e.manifest.resource_id;
                            let mut on = scope.includes(library, id);
                            let label =
                                format!("{} — {}", e.manifest.abbreviation, e.manifest.title);
                            if ui.checkbox(&mut on, label).clicked() {
                                run.push(("search.resource", json!({ "tab": tab.0, "id": id })));
                            }
                        }
                    });
                }
                if !any {
                    ui.label(
                        RichText::new("No Commentaries, Lexicons or Dictionaries in the library")
                            .color(theme::c().muted),
                    );
                }
            })
            .response
            .on_hover_text("Choose Resource types or single Resources to search");
        });
    }

    /// The summary line, notices about Resources not searched, and the results.
    fn search_results(&mut self, ui: &mut Ui, tab: TabId, st: &SearchTab) {
        let summary = match (&st.results, &st.library_results) {
            (Some(r), _) => Some((
                r.summary(),
                r.indexing.clone(),
                r.stale.clone(),
                r.failed.clone(),
            )),
            (None, Some(r)) => Some((
                r.summary(),
                r.indexing.clone(),
                r.stale.clone(),
                r.failed.clone(),
            )),
            (None, None) => None,
        };
        if let Some((summary, indexing, stale, failed)) = &summary {
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(12, 4))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(summary).strong().color(theme::c().ink));
                    for name in indexing {
                        ui.label(
                            RichText::new(format!("Indexing {name}… searched when ready"))
                                .small()
                                .color(theme::c().muted),
                        );
                    }
                    for name in stale {
                        ui.label(
                            RichText::new(format!(
                                "{name}: searched through its older index while it is rebuilt"
                            ))
                            .small()
                            .color(theme::c().muted),
                        );
                    }
                    for (name, why) in failed {
                        ui.label(
                            RichText::new(format!("{name} could not be searched: {why}"))
                                .small()
                                .color(theme::c().warn),
                        );
                    }
                });
        }
        let focused = self.session.workspace.focused() == Some(tab);
        let typing = ui.memory(|m| m.focused().is_some());
        let rows = st.row_count();
        let mut open: Option<(usize, usize, bool)> = None;
        let mut select = st.selected;
        if focused && !typing && rows > 0 {
            ui.input_mut(|i| {
                if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                    select = Some(select.map_or(0, |s| (s + 1).min(rows - 1)));
                }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                    select = Some(select.map_or(0, |s| s.saturating_sub(1)));
                }
                let split = i.modifiers.command;
                let enter = i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                    || i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter);
                if enter && let Some(s) = select {
                    open = Some((s, 0, split));
                }
            });
        }
        let palette = self.palette;
        let settings = self.session.text;
        let Some(t) = self.tabs.get_mut(&tab) else {
            return;
        };
        let view = t.search.get_or_insert_with(|| SearchView::new(settings));
        if select != st.selected
            && let Some(s) = select
        {
            view.reveal(s);
        }
        view.set_settings(settings);
        let mut shown = st.clone();
        shown.selected = select;
        let out = search_pane(ui, view, &shown, &mut self.atlas, &palette);
        if select != st.selected
            && let Some(ws) = self.session.workspace.tab_mut(tab)
        {
            ws.search.selected = select;
        }
        if let Some(c) = out.clicked {
            self.session.workspace.user_focused(tab);
            open = Some((c.row, c.hit, c.new_tab));
        }
        let visible = out.frame.visible;
        if !visible.is_empty() {
            // The rows in view, and the next stretch to keep scrolling smooth.
            self.session
                .request_search_snippets(tab, visible.start..visible.end + PAGE_ROWS / 3);
        }
        if let Some((row, hit, split)) = open {
            self.run(
                "search.open",
                &json!({ "tab": tab.0, "row": row, "hit": hit, "split": split }),
            );
        }
    }
}
