//! The Search Tool's Tab (#59, #60; #16 rules 9–12 and 14): the Bible / Library / Notes
//! switch, the query box (runs on Enter, recent queries per mode), inline errors, then the
//! mode's Search scope (Bible: the Scope field with its presets and the Bible picker;
//! Library: the Resource picker by type or Resource), the summary line and the results (the
//! Verse list, or paragraphs grouped by Resource). ↑/↓ move through the results and Enter
//! opens one; Ctrl/Cmd-click opens it in a new Tab. Notes mode (#64) has a Notebook filter
//! beside the Scope field and lists the Notes found, newest change first; opening one shows
//! it in the Notes Tool. Bible mode also has the Morphology chooser (#145): a menu for each
//! kind of grammar (part of speech, tense, voice, mood …) that writes `@verb aorist
//! imperative` into the query box.

use egui::{Mesh, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, pos2, vec2};
use obs_engine::bible_search::{ErrorField, InlineError, PAGE_ROWS, SearchMode, scope_presets};
use obs_engine::library_search::{LIBRARY_TYPES, type_id, type_plural};
use obs_engine::{MorphCategory, MorphCriteria, ResultsView, SearchTab, TabId};
use obs_layout::ColorRole;
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
    /// The Morphology chooser: shown, and what is chosen in it.
    morph_open: bool,
    morph: MorphCriteria,
}

/// `query` with the chooser's `@…` added at the end, so that it describes the word typed
/// last when that is a Strong's number or Lemma.
fn with_morphology(query: &str, morph: &str) -> String {
    let typed = query.trim_end();
    if typed.is_empty() {
        morph.to_string()
    } else {
        format!("{typed} {morph}")
    }
}

/// The Morphology chooser (#145): a menu of plain-English values for each kind of grammar,
/// the query text they make, and buttons to add it to the query (and search).
fn morphology_chooser(ui: &mut Ui, tab: TabId, morph: &mut MorphCriteria) -> Option<bool> {
    let mut add = None;
    egui::Frame::new()
        .fill(theme::c().field)
        .stroke(Stroke::new(1.0, theme::c().rule))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.label(
                RichText::new("Morphology: one word with all of these")
                    .small()
                    .color(theme::c().muted),
            );
            // As many label and menu pairs to a row as fit.
            let per_row =
                ((ui.available_width() / 190.0) as usize).clamp(1, MorphCategory::ALL.len());
            egui::Grid::new(("search-morph-grid", tab))
                .num_columns(per_row * 2)
                .spacing(vec2(6.0, 4.0))
                .show(ui, |ui| {
                    for (i, &category) in MorphCategory::ALL.iter().enumerate() {
                        let current = morph.get(category);
                        ui.label(
                            RichText::new(category.label())
                                .small()
                                .color(theme::c().muted),
                        );
                        egui::ComboBox::from_id_salt(("search-morph", tab, category))
                            .selected_text(current.unwrap_or("Any"))
                            .width(110.0)
                            .height(320.0)
                            .show_ui(ui, |ui| {
                                if ui.selectable_label(current.is_none(), "Any").clicked() {
                                    morph.set(category, None);
                                }
                                for &value in category.values() {
                                    if ui.selectable_label(current == Some(value), value).clicked()
                                    {
                                        morph.set(category, Some(value));
                                    }
                                }
                            });
                        if (i + 1) % per_row == 0 {
                            ui.end_row();
                        }
                    }
                });
            let text = morph.query_text();
            let chosen = !text.is_empty();
            if chosen {
                ui.label(RichText::new(&text).monospace());
            } else {
                ui.label(
                    RichText::new("Choose a part of speech, tense, case …").color(theme::c().muted),
                );
            }
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(chosen, egui::Button::new("Add to query"))
                    .on_hover_text("Describes the Strong's number or Lemma typed just before it")
                    .clicked()
                {
                    add = Some(false);
                }
                if ui
                    .add_enabled(chosen, egui::Button::new("Add and search"))
                    .clicked()
                {
                    add = Some(true);
                }
                if ui.add_enabled(chosen, egui::Button::new("Clear")).clicked() {
                    *morph = MorphCriteria::default();
                }
            });
        });
    add
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
        SearchMode::Bible => {
            "Words, \"a phrase\", OR, -word, lov*, strong:H1254, lemma:, @verb aorist"
        }
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
        let mut morph = std::mem::take(&mut t.search_fields.morph);
        let mut morph_open = t.search_fields.morph_open;
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
        let notebooks: Vec<(String, String)> = if st.mode == SearchMode::Notes {
            self.session
                .notebooks()
                .unwrap_or_default()
                .into_iter()
                .map(|n| (n.id, n.name))
                .collect()
        } else {
            Vec::new()
        };
        egui::Frame::new()
            .fill(theme::c().surface)
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let bible_mode = st.mode == SearchMode::Bible;
                ui.horizontal(|ui| {
                    for m in SearchMode::ALL {
                        if ui.selectable_label(st.mode == m, m.label()).clicked() && st.mode != m {
                            run.push(("search.mode", json!({ "tab": tab.0, "value": m.id() })));
                        }
                    }
                    if bible_mode {
                        ui.add_space(12.0);
                        if ui
                            .selectable_label(morph_open, "Morphology…")
                            .on_hover_text("Choose grammar in plain English: @verb aorist")
                            .clicked()
                        {
                            morph_open = !morph_open;
                        }
                    }
                });
                ui.horizontal(|ui| {
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut query)
                            .id_salt(("search-query", tab))
                            .hint_text(query_hint(st.mode))
                            .desired_width((ui.available_width() - 150.0).max(120.0))
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
                if bible_mode
                    && morph_open
                    && let Some(and_search) = morphology_chooser(ui, tab, &mut morph)
                {
                    query = with_morphology(&query, &morph.query_text());
                    if and_search {
                        run.push((
                            "search.run",
                            json!({ "tab": tab.0, "text": query.clone(), "scope": scope.clone() }),
                        ));
                    }
                }
                if st.mode == SearchMode::Library {
                    self.resource_picker(ui, tab, &st, &mut run);
                    error_line(ui, error.as_ref(), ErrorField::Resources, "");
                    error_line(ui, error.as_ref(), ErrorField::Search, "");
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
                if st.mode == SearchMode::Notes {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Notebook").small().color(theme::c().muted));
                        let current = st
                            .notebook
                            .as_ref()
                            .and_then(|id| notebooks.iter().find(|(n, _)| n == id))
                            .map_or("All Notebooks", |(_, name)| name.as_str());
                        egui::ComboBox::from_id_salt(("search-notebook", tab))
                            .selected_text(current)
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_label(st.notebook.is_none(), "All Notebooks")
                                    .clicked()
                                {
                                    run.push((
                                        "search.notebook",
                                        json!({ "tab": tab.0, "value": "all" }),
                                    ));
                                }
                                for (id, name) in &notebooks {
                                    let on = st.notebook.as_ref() == Some(id);
                                    if ui.selectable_label(on, name).clicked() {
                                        run.push((
                                            "search.notebook",
                                            json!({ "tab": tab.0, "value": id }),
                                        ));
                                    }
                                }
                            });
                    });
                    error_line(ui, error.as_ref(), ErrorField::Search, "");
                    return;
                }
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
            t.search_fields.morph = morph;
            t.search_fields.morph_open = morph_open;
        }
        if st.mode == SearchMode::Notes {
            self.notes_results(ui, tab, &st);
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
            for name in &scope.collections {
                if chip(
                    ui,
                    &format!("Collection: {name}"),
                    "Do not search this Collection",
                ) {
                    run.push(("search.collection", json!({ "tab": tab.0, "name": name })));
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
                // Collections (#153): chosen while every type is searched, one narrows the
                // search to just it.
                if !library.collections().is_empty() {
                    ui.label(RichText::new("Collections").small().color(theme::c().muted));
                    for c in library.collections() {
                        let mut on = scope.collections.contains(&c.name);
                        if ui
                            .checkbox(&mut on, c.name.as_str())
                            .on_hover_text(if c.rules.is_empty() {
                                "Chosen by hand".to_string()
                            } else {
                                obs_engine::rules_text(&c.rules)
                            })
                            .clicked()
                        {
                            run.push((
                                "search.collection",
                                json!({ "tab": tab.0, "name": c.name }),
                            ));
                        }
                    }
                    ui.separator();
                }
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
            .on_hover_text("Choose Resource types, Collections or single Resources to search");
        });
    }

    /// The summary line, notices about Resources not searched, and the results: in Bible
    /// mode, in the chosen results view (#146).
    fn search_results(&mut self, ui: &mut Ui, tab: TabId, st: &SearchTab) {
        // The Grid, Chart and Analysis count every result; the Verse list may be filtered.
        let shown = match st.view {
            ResultsView::Verses => st.results.as_ref(),
            _ => st.all_results(),
        };
        let summary = match (shown, &st.library_results) {
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
                    if st.all_results().is_some() {
                        self.results_view_switch(ui, tab, st);
                    }
                });
        }
        if st.all_results().is_some() {
            if st.view != ResultsView::Verses
                && let Some(view) = self.tabs.get_mut(&tab).and_then(|t| t.search.as_mut())
            {
                view.drain();
            }
            match st.view {
                ResultsView::Verses => {}
                ResultsView::Grid => return self.results_grid(ui, tab, st),
                ResultsView::Chart => return self.results_chart(ui, tab, st),
                ResultsView::Analysis => return self.results_analysis(ui, tab, st),
            }
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
        self.search_menu(ui, tab, out.response.as_ref(), out.context);
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

    /// Notes mode's results (#16 rule 13): newest change first, each row the Note's anchor
    /// reference, its Notebook and a snippet with the hits marked, drawn by the text pipeline.
    /// A click opens the Note in the Notes Tool; Ctrl/Cmd-click goes to its text in a new Tab.
    /// ↑/↓ move through the rows and Enter opens one.
    fn notes_results(&mut self, ui: &mut Ui, tab: TabId, st: &SearchTab) {
        let muted = theme::c().muted;
        let Some(results) = st.notes.clone() else {
            ui.add_space(30.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("Type a search above and press Enter").color(muted));
            });
            return;
        };
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(12, 4))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(results.summary())
                            .strong()
                            .color(theme::c().ink),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let any = !results.hits.is_empty();
                        if let Some(format) = super::notes_tab::export_menu(ui, "Export…", any) {
                            self.export_or_print(ui.ctx(), json!({ "tab": tab.0 }), format);
                        }
                    });
                });
            });
        let rows = results.hits.len();
        if rows == 0 {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("No notes match").color(muted));
            });
            return;
        }
        let focused = self.session.workspace.focused() == Some(tab);
        let typing = ui.memory(|m| m.focused().is_some());
        let mut select = st.selected;
        let mut open: Option<(usize, bool)> = None;
        if focused && !typing {
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
                    open = Some((s, split));
                }
            });
        }
        let settings = self.session.text;
        let palette = self.palette;
        let line = settings.size * settings.line_spacing * 1.2;
        let row_height = 26.0 + 2.0 * line;
        egui::ScrollArea::vertical()
            .id_salt(("notes-hits", tab))
            .auto_shrink([false, false])
            .show_rows(ui, row_height, rows, |ui, range| {
                for i in range {
                    let hit = &results.hits[i];
                    let (rect, r) = ui.allocate_exact_size(
                        vec2(ui.available_width(), row_height),
                        Sense::click(),
                    );
                    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
                    let fill = if select == Some(i) {
                        palette.selection
                    } else if r.hovered() {
                        palette.hover
                    } else {
                        palette.page
                    };
                    painter.rect_filled(rect, 0.0, fill);
                    painter.text(
                        rect.min + vec2(12.0, 4.0),
                        egui::Align2::LEFT_TOP,
                        format!("{} · {}", hit.reference, hit.notebook),
                        egui::FontId::proportional(12.5),
                        palette.verse_number,
                    );
                    let width = (rect.width() - 24.0).max(60.0);
                    let origin = rect.min + vec2(12.0, 22.0);
                    if let Some(laid) = self.snippets.get(
                        &hit.snippet.text,
                        ColorRole::Ink,
                        false,
                        width,
                        &settings,
                    ) {
                        for h in &hit.snippet.highlights {
                            for [x0, y0, x1, y1] in crate::compare_view::range_rects(&laid, h) {
                                painter.rect_filled(
                                    Rect::from_min_max(
                                        origin + vec2(x0, y0),
                                        origin + vec2(x1, y1),
                                    ),
                                    2.0,
                                    palette.search_hit,
                                );
                            }
                        }
                        let quads = self.atlas.atlas.quads(
                            &laid,
                            [origin.x, origin.y],
                            ui.ctx().pixels_per_point(),
                        );
                        let mut mesh = Mesh::with_texture(self.atlas.sync(ui.ctx()));
                        for q in quads {
                            mesh.add_rect_with_uv(
                                Rect::from_min_max(Pos2::from(q.min), Pos2::from(q.max)),
                                Rect::from_min_max(Pos2::from(q.uv_min), Pos2::from(q.uv_max)),
                                palette.color(q.role),
                            );
                        }
                        painter.add(Shape::mesh(mesh));
                    } else {
                        painter.rect_filled(
                            Rect::from_min_size(
                                pos2(origin.x, origin.y + line * 0.3),
                                vec2(width, line * 0.35),
                            ),
                            2.0,
                            palette.verse_number.gamma_multiply(0.12),
                        );
                        ui.ctx().request_repaint();
                    }
                    painter.hline(
                        rect.x_range(),
                        rect.bottom() - 0.5,
                        Stroke::new(1.0, palette.verse_number.gamma_multiply(0.2)),
                    );
                    r.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::Button, true, &hit.snippet.text)
                    });
                    if r.clicked() {
                        select = Some(i);
                        open = Some((i, ui.input(|i| i.modifiers.command)));
                    }
                }
            });
        if select != st.selected
            && let Some(ws) = self.session.workspace.tab_mut(tab)
        {
            ws.search.selected = select;
        }
        if let Some((row, split)) = open
            && let Some(hit) = results.hits.get(row)
        {
            self.session.workspace.user_focused(tab);
            if split {
                self.run("notes.follow", &json!({ "id": hit.id, "split": true }));
            } else {
                self.run("notes.open", &json!({ "id": hit.id }));
            }
        }
    }
}
