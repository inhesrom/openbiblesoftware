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

use egui::{Rect, RichText, Sense, Stroke, Ui, vec2};
use obs_engine::bible_search::{ErrorField, InlineError, PAGE_ROWS, SearchMode, scope_presets};
use obs_engine::library_search::{LIBRARY_TYPES, type_id, type_plural};
use obs_engine::{MorphCategory, MorphCriteria, ResultsView, SearchTab, TabId};
use obs_layout::ColorRole;
use obs_model::ResourceType;
use serde_json::{Value, json};

use super::{State, search_sidebar};
use crate::search_view::{SearchView, search_pane};
use crate::theme;
use crate::tokens::{alpha, inset, radius, scale, size, space, stroke, text};

/// The query and Scope fields as typed, and what the engine last said they were (a search
/// handed over by another Tool, or a Saved search, Search history entry, Favorite or Visit
/// run here, replaces what is typed).
#[derive(Default)]
pub(super) struct Fields {
    query: String,
    scope: String,
    seen: Option<(String, String, u64)>,
    /// The Morphology chooser: shown, and what is chosen in it.
    morph_open: bool,
    morph: MorphCriteria,
    /// The sidebar of Saved searches and Search history (#179): `None` until toggled, when
    /// it shows if the Tab is wide enough.
    sidebar: Option<bool>,
    /// Save…'s name field while its menu is open.
    pub(super) save_name: Option<String>,
    /// The query field, for "Edit the search" when nothing matches (#223).
    query_field: Option<egui::Id>,
    /// The sidebar asks whether to clear the Search history.
    pub(super) confirm_clear_history: bool,
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
        .stroke(crate::components::rule_stroke())
        .corner_radius(radius::M)
        .inner_margin(inset::CARD)
        .show(ui, |ui| {
            ui.label(crate::components::hint_text(
                "Morphology: one word with all of these",
            ));
            // As many label and menu pairs to a row as fit.
            let per_row = ((ui.available_width() / (size::FIELD_NARROW + size::LABEL_COLUMN))
                as usize)
                .clamp(1, MorphCategory::ALL.len());
            egui::Grid::new(("search-morph-grid", tab))
                .num_columns(per_row * 2)
                .spacing(vec2(space::S, space::XS))
                .show(ui, |ui| {
                    for (i, &category) in MorphCategory::ALL.iter().enumerate() {
                        let current = morph.get(category);
                        ui.label(crate::components::hint_text(category.label()));
                        egui::ComboBox::from_id_salt(("search-morph", tab, category))
                            .selected_text(current.unwrap_or("Any"))
                            .width(size::FIELD_NARROW)
                            .height(size::LIST_TALL)
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
                    .add_enabled(chosen, crate::components::button("Add to query"))
                    .on_hover_text("Describes the Strong's number or Lemma typed just before it")
                    .clicked()
                {
                    add = Some(false);
                }
                if ui
                    .add_enabled(chosen, crate::components::button("Add and search"))
                    .clicked()
                {
                    add = Some(true);
                }
                if ui
                    .add_enabled(chosen, crate::components::button("Clear"))
                    .clicked()
                {
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

/// The query row's toggle for Word forms (#180).
pub(super) const WORD_FORMS: &str = "Word forms";

/// What the empty query box says.
pub(super) fn query_hint(mode: SearchMode) -> &'static str {
    match mode {
        SearchMode::Bible => {
            "Words, \"a phrase\", OR, -word, lov*, NEAR/5, strong:H1254, lemma:, @verb aorist"
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
        // The sidebar first, so the fields and results get the rest of the Tab.
        let sidebar = self
            .tabs
            .get(&tab)
            .and_then(|t| t.search_fields.sidebar)
            .unwrap_or(ui.available_width() >= search_sidebar::SIDEBAR_MIN_TAB_WIDTH);
        let mut run: Vec<(&'static str, Value)> = Vec::new();
        if sidebar {
            egui::Panel::right(("search-sidebar", tab))
                .resizable(true)
                .default_size(size::SIDE_LIST)
                .size_range(size::FIELD..=size::PROSE)
                .frame(
                    egui::Frame::new()
                        .fill(theme::c().field)
                        .inner_margin(inset::CARD),
                )
                .show(ui, |ui| self.search_sidebar(ui, tab, &mut run));
        }
        let context = self.session.search_context(tab);
        let Some(t) = self.tabs.get_mut(&tab) else {
            return;
        };
        let engine_fields = (st.query.clone(), st.scope.clone(), st.restored);
        if t.search_fields.seen.as_ref() != Some(&engine_fields) {
            t.search_fields.query = st.query.clone();
            t.search_fields.scope = st.scope.clone();
            t.search_fields.seen = Some(engine_fields);
        }
        let mut query = std::mem::take(&mut t.search_fields.query);
        let mut scope = std::mem::take(&mut t.search_fields.scope);
        let mut morph = std::mem::take(&mut t.search_fields.morph);
        let mut morph_open = t.search_fields.morph_open;
        let mut toggle_sidebar = false;
        let mut query_field = None;
        let recent = self.session.recent_searches(st.mode);
        let word_forms = self.session.search_word_forms();
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
            .inner_margin(inset::CARD)
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
                        ui.add_space(space::L);
                        if ui
                            .selectable_label(morph_open, "Morphology…")
                            .on_hover_text("Choose grammar in plain English: @verb aorist")
                            .clicked()
                        {
                            morph_open = !morph_open;
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .selectable_label(sidebar, "Saved & history")
                            .on_hover_text("Saved searches and the Search history")
                            .clicked()
                        {
                            toggle_sidebar = true;
                        }
                    });
                });
                // Word forms is for English words of Bibles and Library Resources (#180).
                let forms_toggle = st.mode != SearchMode::Notes;
                let buttons = if forms_toggle {
                    size::FIELD_WIDE + space::XXXL
                } else {
                    size::SIDE_LIST
                };
                // Wrapped, so a narrow Tab puts the toggle on a line of its own.
                ui.horizontal_wrapped(|ui| {
                    let r = ui
                        .add(
                            egui::TextEdit::singleline(&mut query)
                                .id_salt(("search-query", tab))
                                .hint_text(query_hint(st.mode))
                                .desired_width(
                                    (ui.available_width() - buttons).max(size::FIELD_NARROW),
                                )
                                .margin(inset::BAR),
                        )
                        .accessible_name("Search for");
                    query_field = Some(r.id);
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
                    crate::components::menu(ui, "Recent", |ui| {
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
                    self.save_search_button(ui, tab, &query, &scope, &mut run);
                    if forms_toggle {
                        let mut on = word_forms;
                        if crate::components::toggle(ui, &mut on, WORD_FORMS)
                            .on_hover_text(
                                "Also find other forms of English words: love finds loved, \
                                 loving and loveth. \"Quoted\" words stay exact.",
                            )
                            .changed()
                        {
                            run.push(("search.word_forms", json!({ "tab": tab.0, "value": on })));
                        }
                    }
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
                    return;
                }
                ui.horizontal(|ui| {
                    ui.label(crate::components::hint_text("Scope"));
                    let r = ui
                        .add(
                            egui::TextEdit::singleline(&mut scope)
                                .id_salt(("search-scope", tab))
                                .hint_text("Whole Bible, or a Passage list: Rom 1-8; Gal")
                                .desired_width(size::SIDE_LIST)
                                .margin(inset::BAR),
                        )
                        .accessible_name("Scope");
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                        run.push((
                            "search.scope",
                            json!({ "tab": tab.0, "value": scope.clone() }),
                        ));
                    }
                    let presets = crate::icons::image(
                        ui.ctx(),
                        crate::icons::Icon::ChevronDown,
                        crate::icons::SMALL,
                        theme::c().muted,
                    );
                    crate::components::menu(ui, presets, |ui| {
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
                        ui.label(crate::components::hint_text("Notebook"));
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
                            })
                            .response
                            .accessible_name("Notebook");
                    });
                    return;
                }
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = space::XS;
                    ui.label(crate::components::hint_text("Bibles"));
                    for (id, abbreviation) in &chosen {
                        if crate::components::removable_chip(
                            ui,
                            abbreviation,
                            "Do not search this Bible",
                        ) {
                            run.push(("search.bible", json!({ "tab": tab.0, "id": id })));
                        }
                    }
                    crate::components::menu(ui, crate::components::label_text("+ Bible"), |ui| {
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
            });
        if let Some(t) = self.tabs.get_mut(&tab) {
            t.search_fields.query = query;
            t.search_fields.scope = scope;
            t.search_fields.morph = morph;
            t.search_fields.morph_open = morph_open;
            t.search_fields.query_field = query_field;
            if toggle_sidebar {
                t.search_fields.sidebar = Some(!sidebar);
            }
        }
        if let Some(notice) = &st.notice {
            crate::components::band_frame().show(ui, |ui| {
                ui.label(RichText::new(notice).small().color(theme::c().warn));
            });
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
        let chip = crate::components::removable_chip;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = space::XS;
            ui.label(crate::components::hint_text("Resources"));
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
            crate::components::menu(ui, crate::components::label_text("Choose…"), |ui| {
                let mut any = false;
                // Collections (#153): chosen while every type is searched, one narrows the
                // search to just it.
                if !library.collections().is_empty() {
                    ui.label(crate::components::hint_text("Collections"));
                    for c in library.collections() {
                        let mut on = scope.collections.contains(&c.name);
                        if crate::components::toggle(ui, &mut on, c.name.as_str())
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
                    if crate::components::toggle(ui, &mut all, format!("All {}", type_plural(ty)))
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
                            if crate::components::toggle(ui, &mut on, label).clicked() {
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
        let any = st.results.is_some() || st.library_results.is_some();
        if self.results_state(ui, tab, st, any) {
            return;
        }
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
            crate::components::band_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(summary).strong().color(theme::c().ink));
                for name in indexing {
                    ui.label(crate::components::hint_text(format!(
                        "Indexing {name}… searched when ready"
                    )));
                }
                for name in stale {
                    ui.label(crate::components::hint_text(format!(
                        "{name}: searched through its older index while it is rebuilt"
                    )));
                }
                // A line of the summary: the plain message, the error itself on hover (#223).
                for (name, why) in failed {
                    ui.label(
                        RichText::new(format!("{name} could not be searched."))
                            .small()
                            .color(theme::c().warn),
                    )
                    .on_hover_text(why);
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
        if st.row_count() == 0 {
            // Bibles still being indexed are searched when ready: loading, not empty.
            let indexing = match (&st.results, &st.library_results) {
                (Some(r), _) => r.indexing.clone(),
                (None, Some(r)) => r.indexing.clone(),
                (None, None) => Vec::new(),
            };
            if !indexing.is_empty() {
                crate::components::band_frame().show(ui, |ui| {
                    ui.add_space(space::M);
                    let what = format!("Indexing {}…", indexing.join(", "));
                    crate::components::loading_state(ui, &what);
                });
                return;
            }
            let message = match (&st.results, st.query.trim()) {
                (Some(_), q) => format!("No verses match “{q}”."),
                (None, q) => format!("No paragraphs match “{q}”."),
            };
            self.no_matches(ui, tab, &message);
            return;
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

    /// The results area while there are no results to show (#223): the search failed (a plain
    /// message, Retry, and the error behind Details), is running (a skeleton), or has not been
    /// run (what to search for, and an example). A failure is shown over older results too.
    /// Returns whether there is nothing more to draw.
    fn results_state(&mut self, ui: &mut Ui, tab: TabId, st: &SearchTab, any: bool) -> bool {
        let failed = st
            .error
            .as_ref()
            .filter(|e| e.field == ErrorField::Search && !st.running);
        if any && failed.is_none() {
            return false;
        }
        let mut run: Option<Value> = None;
        crate::components::band_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add_space(space::M);
            if let Some(e) = failed {
                if crate::components::error_state(ui, "The search could not be run.", &e.message) {
                    run = Some(json!({ "tab": tab.0, "text": st.query, "scope": st.scope }));
                }
            } else if st.running {
                crate::components::loading_state(ui, "Searching…");
            } else {
                let (message, example) = match st.mode {
                    SearchMode::Bible => (
                        "Search the Bibles in your library for words, “a phrase”, a Strong's \
                         number (strong:G26) or grammar (@verb aorist), then press Enter.",
                        "love",
                    ),
                    SearchMode::Library => (
                        "Search your Commentaries, Lexicons and Dictionaries for words or \
                         “a phrase”, then press Enter.",
                        "grace",
                    ),
                    SearchMode::Notes => {
                        ("Search the text of your Notes, then press Enter.", "prayer")
                    }
                };
                if crate::components::empty_state(ui, message, &format!("Search for “{example}”"))
                    .clicked()
                {
                    run = Some(json!({ "tab": tab.0, "text": example }));
                }
            }
        });
        if let Some(params) = run {
            self.run("search.run", &params);
        }
        !any
    }

    /// A search that matched nothing: say so, and offer to edit the query (#223).
    fn no_matches(&mut self, ui: &mut Ui, tab: TabId, message: &str) {
        crate::components::band_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add_space(space::M);
            if crate::components::empty_state(ui, message, "Edit the search").clicked()
                && let Some(id) = self
                    .tabs
                    .get(&tab)
                    .and_then(|t| t.search_fields.query_field)
            {
                ui.memory_mut(|m| m.request_focus(id));
            }
        });
    }

    /// Notes mode's results (#16 rule 13): newest change first, each row the Note's anchor
    /// reference, its Notebook and a snippet with the hits marked, drawn by the text pipeline.
    /// A click opens the Note in the Notes Tool; Ctrl/Cmd-click goes to its text in a new Tab.
    /// ↑/↓ move through the rows and Enter opens one.
    fn notes_results(&mut self, ui: &mut Ui, tab: TabId, st: &SearchTab) {
        if self.results_state(ui, tab, st, st.notes.is_some()) {
            return;
        }
        let Some(results) = st.notes.clone() else {
            return;
        };
        crate::components::band_frame().show(ui, |ui| {
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
            let message = format!("No Notes match “{}”.", st.query.trim());
            self.no_matches(ui, tab, &message);
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
        let line = settings.size * settings.line_spacing * scale::ROW_LINE;
        let row_height = size::ROW + 2.0 * line;
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
                    // In a High contrast page (#224) the selected row is framed in ink.
                    if select == Some(i) && palette.high_contrast {
                        painter.rect_stroke(
                            rect,
                            0.0,
                            Stroke::new(stroke::MEDIUM, palette.ink),
                            egui::StrokeKind::Inside,
                        );
                    }
                    painter.text(
                        rect.min + vec2(space::L, space::XS),
                        egui::Align2::LEFT_TOP,
                        format!("{} · {}", hit.reference, hit.notebook),
                        egui::FontId::proportional(text::LABEL),
                        palette.verse_number,
                    );
                    let width = (rect.width() - 2.0 * space::L).max(size::TEXT_MIN);
                    let origin = rect.min + vec2(space::L, size::CONTROL_COMPACT);
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
                                    radius::XS,
                                    palette.search_hit,
                                );
                            }
                        }
                        self.atlas.paint(&painter, &laid, origin, &palette);
                    } else {
                        crate::components::text_placeholder(
                            &painter,
                            origin,
                            width,
                            line,
                            palette.verse_number.gamma_multiply(alpha::WASH),
                        );
                        ui.ctx().request_repaint();
                    }
                    painter.hline(
                        rect.x_range(),
                        rect.bottom() - stroke::HAIR / 2.0,
                        Stroke::new(
                            stroke::HAIR,
                            palette.verse_number.gamma_multiply(alpha::TINT),
                        ),
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
