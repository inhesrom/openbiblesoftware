//! Collections in the Library Tab (#153): the Collection chips that filter the Library, the
//! New Collection… / Edit… window (rules and Resources chosen by hand), and a Library row's
//! Collections submenu. Every change runs a `collection.*` command.

use egui::{RichText, Ui};
use obs_engine::{Collection, TabId, parse_rules};
use serde_json::json;

use super::{RESOURCE_TYPES, State, type_name};
use crate::theme;

/// The New Collection… / Edit… window's fields.
#[derive(Debug, Clone, Default)]
pub(crate) struct CollectionEditor {
    /// The Collection being edited (none: a new one).
    pub(crate) previous: Option<String>,
    pub(crate) name: String,
    pub(crate) rules: String,
    /// Resource ids chosen by hand.
    pub(crate) resources: Vec<String>,
}

impl CollectionEditor {
    fn of(c: &Collection) -> Self {
        Self {
            previous: Some(c.name.clone()),
            name: c.name.clone(),
            rules: obs_engine::rules_text(&c.rules),
            resources: c.resources.clone(),
        }
    }
}

/// What the Collection chips asked for this frame.
pub(super) enum BarAction {
    Filter(Option<String>),
    New,
    Edit(String),
    Delete(String),
}

/// The Library's Collection chips: each filters the list to its Resources (one at a time; All
/// shows every Resource), then New Collection…, and Edit… / Delete for the `chosen` one.
pub(super) fn collections_bar(
    ui: &mut Ui,
    library: &obs_engine::Library,
    chosen: Option<&str>,
) -> Option<BarAction> {
    let mut action = None;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Collections").small().color(theme::c().muted));
        if !library.collections().is_empty() {
            let all = egui::Button::selectable(chosen.is_none(), "All").corner_radius(11.0);
            if ui.add(all).on_hover_text("Show every Resource").clicked() {
                action = Some(BarAction::Filter(None));
            }
        }
        for c in library.collections() {
            let name = c.name.as_str();
            let on = chosen == Some(name);
            let chip = egui::Button::selectable(on, name).corner_radius(11.0);
            if ui
                .add(chip)
                .on_hover_text(format!("Show only the Resources in {name}"))
                .clicked()
            {
                action = Some(BarAction::Filter((!on).then(|| name.to_string())));
            }
        }
        if ui
            .small_button("New Collection…")
            .on_hover_text(
                "Save a set of Resources, chosen by hand or by rule, to search, to filter the \
                 Passage Guide's Commentaries, or to filter the Library",
            )
            .clicked()
        {
            action = Some(BarAction::New);
        }
        if let Some(name) = chosen {
            if ui.small_button("Edit…").clicked() {
                action = Some(BarAction::Edit(name.to_string()));
            }
            if ui
                .small_button("Delete")
                .on_hover_text("Delete this Collection (its Resources stay in the Library)")
                .clicked()
            {
                action = Some(BarAction::Delete(name.to_string()));
            }
        }
    });
    action
}

impl State {
    /// Do what the Library Tab's Collection chips asked.
    pub(super) fn collections_bar_action(&mut self, tab: TabId, action: BarAction) {
        match action {
            BarAction::Filter(c) => {
                if let Some(t) = self.tabs.get_mut(&tab) {
                    t.collection = c;
                }
            }
            BarAction::New => self.collection_editor = Some(CollectionEditor::default()),
            BarAction::Edit(name) => {
                self.collection_editor = self
                    .session
                    .library
                    .collection(&name)
                    .map(CollectionEditor::of);
            }
            BarAction::Delete(name) => {
                if self
                    .run("collection.delete", &json!({ "name": name }))
                    .is_some()
                {
                    self.toast(format!("Deleted the Collection “{name}”"));
                }
            }
        }
    }

    /// The New Collection… / Edit… window.
    pub(super) fn collection_editor_window(&mut self, ctx: &egui::Context) {
        let Some(mut ed) = self.collection_editor.take() else {
            return;
        };
        let library = &self.session.library;
        // What the rules match now, as typed.
        let parsed = parse_rules(&ed.rules);
        let preview = parsed.as_ref().ok().map(|rules| {
            let c = Collection {
                name: String::new(),
                rules: rules.clone(),
                resources: ed.resources.clone(),
            };
            library
                .entries()
                .iter()
                .filter(|e| c.includes(&e.manifest, library.tags_of(&e.manifest.resource_id)))
                .count()
        });
        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        let title = if ed.previous.is_some() {
            "Edit Collection"
        } else {
            "New Collection"
        };
        egui::Window::new(title)
            .id(egui::Id::unique("collection-editor"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(480.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                egui::Grid::new("collection-fields")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Name");
                        ui.add(
                            egui::TextEdit::singleline(&mut ed.name)
                                .hint_text("Older commentaries")
                                .desired_width(320.0),
                        )
                        .accessible_name("Name");
                        ui.end_row();
                        ui.label("Rules");
                        ui.add(
                            egui::TextEdit::singleline(&mut ed.rules)
                                .hint_text("type:commentary language:en")
                                .desired_width(320.0),
                        )
                        .accessible_name("Rules")
                        .on_hover_text(
                            "type: (bible, commentary, lexicon, dictionary), language:, tag:, \
                             licence:, title: — each kind must match; several of one kind match \
                             any. A rule-based Collection takes in Resources added later.",
                        );
                        ui.end_row();
                    });
                match (&parsed, preview) {
                    (Err(e), _) => {
                        ui.label(RichText::new(e).small().color(theme::c().warn));
                    }
                    (Ok(_), Some(n)) => {
                        ui.label(
                            RichText::new(format!(
                                "{n} Resource{} in the Library now",
                                if n == 1 { "" } else { "s" }
                            ))
                            .small()
                            .color(theme::c().muted),
                        );
                    }
                    (Ok(_), None) => {}
                }
                ui.add_space(6.0);
                ui.label(
                    RichText::new("Chosen by hand")
                        .small()
                        .color(theme::c().muted),
                );
                egui::ScrollArea::vertical()
                    .max_height(260.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        // Chosen by hand before they left the library: kept unless unticked.
                        let absent: Vec<String> = ed
                            .resources
                            .iter()
                            .filter(|id| library.get(id).is_none())
                            .cloned()
                            .collect();
                        if !absent.is_empty() {
                            ui.label(RichText::new("Not in the Library now").strong());
                            for id in absent {
                                let mut on = true;
                                if ui.checkbox(&mut on, id.as_str()).changed() && !on {
                                    ed.resources.retain(|r| *r != id);
                                }
                            }
                        }
                        for ty in RESOURCE_TYPES {
                            let entries: Vec<_> = library.of_type(ty).collect();
                            if entries.is_empty() {
                                continue;
                            }
                            ui.label(RichText::new(type_name(ty)).strong());
                            for e in entries {
                                let id = &e.manifest.resource_id;
                                let mut on = ed.resources.iter().any(|r| r == id);
                                let label =
                                    format!("{} — {}", e.manifest.abbreviation, e.manifest.title);
                                if ui.checkbox(&mut on, label).changed() {
                                    if on {
                                        ed.resources.push(id.clone());
                                    } else {
                                        ed.resources.retain(|r| r != id);
                                    }
                                }
                            }
                        }
                    });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let ready = parsed.is_ok() && !ed.name.trim().is_empty();
                    if ui.add_enabled(ready, egui::Button::new("Save")).clicked() {
                        save = true;
                    }
                    if ui.button("Cancel").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        cancel = true;
                    }
                });
            });
        if save {
            let params = json!({
                "name": ed.name.trim(),
                "rules": ed.rules,
                "resources": ed.resources.join(","),
                "previous": ed.previous.clone().unwrap_or_default(),
            });
            if self.run("collection.save", &params).is_none() {
                self.collection_editor = Some(ed);
            }
            return;
        }
        if open && !cancel {
            self.collection_editor = Some(ed);
        }
    }
}

/// A Library row's Collections submenu: a checkbox for each Collection adds the Resource by
/// hand or takes it out. A Resource its rules take in is ticked and cannot be taken out here.
pub(super) fn collections_menu(
    ui: &mut Ui,
    library: &obs_engine::Library,
    id: &str,
) -> Option<String> {
    let mut toggled = None;
    ui.menu_button("Collections", |ui| {
        for c in library.collections() {
            let by_hand = c.resources.iter().any(|r| r == id);
            let mut on = library.in_collection(&c.name, id);
            let by_rule = on && !by_hand;
            let response = ui.add_enabled(!by_rule, egui::Checkbox::new(&mut on, &c.name));
            if by_rule {
                response.on_disabled_hover_text("In this Collection by its rules");
            } else if response.clicked() {
                toggled = Some(c.name.clone());
                ui.close();
            }
        }
        if library.collections().is_empty() {
            ui.label(
                RichText::new("No Collections yet: make one with New Collection…")
                    .color(theme::c().muted),
            );
        }
    });
    toggled
}
