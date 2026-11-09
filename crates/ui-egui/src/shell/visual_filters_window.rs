//! View → Visual filters (#211): the user's Visual filters, each a Search query (and/or text
//! the Resource marks) with the style its matches are painted in.
//!
//! - Each filter is a row: its swatch, name, what it finds, whether it is on in all Tabs, and
//!   Edit and Delete. **New filter** opens an empty form.
//! - The form: name, query (the Search Tool's grammar, with its inline errors), marked text
//!   (Words of Christ, Divine names), the Passages it is limited to, look and colour (a hex
//!   field or a preset), and "On in all Tabs". Save runs `filter.save`; an error stays under
//!   the form until the next try.
//! - The Tab header's Filters menu (in a narrow Tab, its ⋯ menu's Visual filters) switches
//!   each filter for one Tab (`tab.visual_filter`).

use egui::{RichText, Sense, Ui, vec2};
use obs_engine::{HighlightLook, MarkedText, Rgb, StyleAppearance, VisualFilter};
use serde_json::{Value, json};

use super::State;
use super::annotate::paint_swatch;
use crate::theme;
use crate::tokens::{size, space};

/// Colours the form offers beside its hex field.
const PRESETS: [(&str, Rgb); 8] = crate::tokens::FILTER_PRESETS;

/// The filter being made or edited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FilterForm {
    /// The filter edited; none: a new one.
    pub(crate) id: Option<String>,
    pub(crate) name: String,
    pub(crate) query: String,
    pub(crate) marked: Option<MarkedText>,
    pub(crate) scope: String,
    pub(crate) look: HighlightLook,
    pub(crate) color: String,
    pub(crate) all_tabs: bool,
}

impl FilterForm {
    fn new_filter() -> Self {
        Self {
            id: None,
            name: String::new(),
            query: String::new(),
            marked: None,
            scope: String::new(),
            look: HighlightLook::Background,
            color: crate::tokens::FILTER_DEFAULT.to_string(),
            all_tabs: false,
        }
    }

    fn of(f: &VisualFilter) -> Self {
        Self {
            id: Some(f.id.clone()),
            name: f.name.clone(),
            query: f.query.clone(),
            marked: f.marked,
            scope: f.scope.clone(),
            look: f.look,
            color: f.color.to_string(),
            all_tabs: f.all_tabs,
        }
    }

    fn params(&self) -> Value {
        let mut p = json!({
            "name": self.name,
            "query": self.query,
            "marked": self.marked.map_or("none", MarkedText::id),
            "scope": self.scope,
            "look": self.look.id(),
            "color": self.color,
            "all_tabs": self.all_tabs,
        });
        if let Some(id) = &self.id {
            p["id"] = json!(id);
        }
        p
    }
}

/// The Visual filters window's state.
#[derive(Default)]
pub(crate) struct FiltersUi {
    pub(crate) open: bool,
    pub(crate) form: Option<FilterForm>,
    /// Why the last Save or Delete was refused.
    pub(crate) error: Option<String>,
}

/// What the window asked for this frame.
enum Act {
    Edit(FilterForm),
    Save(FilterForm),
    Run(&'static str, Value),
    Cancel,
}

/// What a filter finds, in a few words: its query and marked text, and its Passages.
pub(super) fn finds(f: &VisualFilter) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !f.query.trim().is_empty() {
        parts.push(f.query.trim().to_string());
    }
    if let Some(m) = f.marked {
        parts.push(format!("text marked as {}", m.label()));
    }
    let mut out = parts.join(" or ");
    if !f.scope.trim().is_empty() {
        out.push_str(&format!(" · in {}", f.scope.trim()));
    }
    out
}

impl State {
    /// The Visual filters window, when open.
    pub(super) fn visual_filters_window(&mut self, ctx: &egui::Context) {
        if !self.filters_ui.open {
            return;
        }
        let filters = self.session.visual_filters().to_vec();
        let palette = self.palette;
        let mut open = true;
        let escape = super::escape_closes_window(ctx);
        let mut acts: Vec<Act> = Vec::new();
        let ui_state = &mut self.filters_ui;
        let max_height = ctx.content_rect().height() - size::DIALOG_ROOM;
        let window = egui::Window::new("Visual filters")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(size::DIALOG.x)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, size::DIALOG_TOP))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(max_height)
                    .min_scrolled_height(
                        max_height.clamp(size::LABEL_COLUMN_WIDE, size::DIALOG_LIST),
                    )
                    .show(ui, |ui| {
                        ui.label(crate::components::hint_text(
                            "Colour the words that match a search as you read. Switch a \
                                 filter on or off in a Tab with its header's Filters menu.",
                        ));
                        ui.add_space(space::XS);
                        filter_rows(ui, &filters, &palette, &mut acts);
                        if ui.button("New filter…").clicked() {
                            acts.push(Act::Edit(FilterForm::new_filter()));
                        }
                        if let Some(form) = &mut ui_state.form {
                            ui.separator();
                            form_ui(ui, form, &palette, &mut acts);
                        }
                        if let Some(e) = &ui_state.error {
                            ui.label(RichText::new(e).color(theme::c().warn));
                        }
                    });
            });
        if !open || escape(window.map(|r| r.response.layer_id)) {
            self.filters_ui.open = false;
            self.filters_ui.form = None;
            self.filters_ui.error = None;
        }
        for act in acts {
            match act {
                Act::Edit(form) => {
                    self.filters_ui.form = Some(form);
                    self.filters_ui.error = None;
                }
                Act::Cancel => {
                    self.filters_ui.form = None;
                    self.filters_ui.error = None;
                }
                Act::Save(form) => match self.session.run("filter.save", &form.params()) {
                    Ok(_) => {
                        self.filters_ui.form = None;
                        self.filters_ui.error = None;
                    }
                    Err(e) => self.filters_ui.error = Some(command_message(&e)),
                },
                Act::Run(cmd, params) => {
                    if let Err(e) = self.session.run(cmd, &params) {
                        self.filters_ui.error = Some(command_message(&e));
                    } else {
                        self.filters_ui.error = None;
                    }
                }
            }
        }
    }
}

/// A refused command's reason, without the command's name.
fn command_message(e: &obs_engine::CommandError) -> String {
    match e {
        obs_engine::CommandError::Failed { message, .. } => message.clone(),
        other => other.to_string(),
    }
}

fn filter_rows(
    ui: &mut Ui,
    filters: &[VisualFilter],
    palette: &crate::palette::Palette,
    acts: &mut Vec<Act>,
) {
    if filters.is_empty() {
        ui.label("You have no Visual filters.");
        return;
    }
    egui::Grid::new("visual-filters")
        .num_columns(5)
        .spacing(vec2(space::M, space::S))
        .show(ui, |ui| {
            for f in filters {
                let (rect, _) = ui.allocate_exact_size(
                    vec2(size::CONTROL, size::CONTROL_COMPACT),
                    Sense::hover(),
                );
                paint_swatch(ui, rect, &f.appearance(), palette, false);
                ui.vertical(|ui| {
                    ui.set_max_width(size::COLUMN);
                    ui.label(&f.name);
                    let what = finds(f);
                    ui.add(egui::Label::new(crate::components::hint_text(&what)).truncate())
                        .on_hover_text(what);
                });
                let mut all = f.all_tabs;
                if ui
                    .checkbox(&mut all, "All Tabs")
                    .on_hover_text("On in every Tab, unless a Tab switches it off")
                    .accessible_name(format!("{} on in all Tabs", f.name))
                    .changed()
                {
                    acts.push(Act::Run(
                        "filter.save",
                        json!({ "id": f.id, "all_tabs": all }),
                    ));
                }
                if ui
                    .button("Edit…")
                    .accessible_name(format!("Edit {}", f.name))
                    .clicked()
                {
                    acts.push(Act::Edit(FilterForm::of(f)));
                }
                if ui
                    .button("Delete")
                    .accessible_name(format!("Delete {}", f.name))
                    .clicked()
                {
                    acts.push(Act::Run("filter.delete", json!({ "id": f.id })));
                }
                ui.end_row();
            }
        });
}

fn form_ui(
    ui: &mut Ui,
    form: &mut FilterForm,
    palette: &crate::palette::Palette,
    acts: &mut Vec<Act>,
) {
    ui.label(
        RichText::new(if form.id.is_some() {
            "Edit filter"
        } else {
            "New filter"
        })
        .strong(),
    );
    egui::Grid::new("visual-filter-form")
        .num_columns(2)
        .spacing(vec2(space::M, space::S))
        .show(ui, |ui| {
            ui.label("Name");
            ui.add(
                egui::TextEdit::singleline(&mut form.name)
                    .desired_width(size::FIELD_WIDE)
                    .hint_text("Imperatives in Romans 12"),
            )
            .accessible_name("Filter name");
            ui.end_row();
            ui.label("Search query");
            ui.add(
                egui::TextEdit::singleline(&mut form.query)
                    .desired_width(size::FIELD_WIDE)
                    .hint_text("@imperative · strong:G26 · \"love one another\""),
            )
            .accessible_name("Filter search query");
            ui.end_row();
            ui.label("Also colour");
            egui::ComboBox::from_id_salt("visual-filter-marked")
                .selected_text(form.marked.map_or("Nothing else", MarkedText::label))
                .width(size::FIELD)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut form.marked, None, "Nothing else");
                    for m in MarkedText::ALL {
                        ui.selectable_value(&mut form.marked, Some(m), m.label());
                    }
                })
                .response
                .on_hover_text("Text the Resource itself marks, coloured whatever the query")
                .accessible_name("Also colour marked text");
            ui.end_row();
            ui.label("Passages");
            ui.add(
                egui::TextEdit::singleline(&mut form.scope)
                    .desired_width(size::FIELD_WIDE)
                    .hint_text("Whole Bible"),
            )
            .on_hover_text("Limit it to these Passages, as a Search scope: Rom 12, NT, Gen-Deut")
            .accessible_name("Filter Passages");
            ui.end_row();
            ui.label("Look");
            egui::ComboBox::from_id_salt("visual-filter-look")
                .selected_text(form.look.label())
                .width(size::FIELD_NARROW)
                .show_ui(ui, |ui| {
                    for l in HighlightLook::ALL {
                        ui.selectable_value(&mut form.look, l, l.label());
                    }
                })
                .response
                .accessible_name("Filter look");
            ui.end_row();
            ui.label("Colour");
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut form.color).desired_width(size::FIELD_HEX))
                    .accessible_name("Filter colour");
                for (name, rgb) in PRESETS {
                    let (rect, r) =
                        ui.allocate_exact_size(egui::Vec2::splat(size::ICON_LARGE), Sense::click());
                    let shown = StyleAppearance::new(form.look, rgb);
                    paint_swatch(ui, rect, &shown, palette, r.hovered());
                    if r.on_hover_text(name)
                        .accessible_name(format!("Colour {name}"))
                        .clicked()
                    {
                        form.color = rgb.to_string();
                    }
                }
            });
            ui.end_row();
            ui.label("");
            ui.checkbox(&mut form.all_tabs, "On in all Tabs")
                .on_hover_text("Otherwise it shows in the Tabs whose Filters menu switches it on");
            ui.end_row();
        });
    ui.horizontal(|ui| {
        if ui.button("Save").clicked() {
            acts.push(Act::Save(form.clone()));
        }
        if ui.button("Cancel").clicked() {
            acts.push(Act::Cancel);
        }
    });
}

impl State {
    /// A wide Resource Tab's Filters button (#211): its menu is [`Self::filters_items`]. The
    /// button reads how many filters the Tab shows. A narrow Tab has them in its ⋯ menu
    /// instead ([`Self::filters_submenu`]), leaving the header's room to the reference box.
    pub(super) fn filters_menu(&mut self, ui: &mut Ui, tab: obs_engine::TabId) {
        let shown = self.session.tab_visual_filters(tab).len();
        let label = match shown {
            0 => "Filters".to_string(),
            n => format!("Filters {n}"),
        };
        let text = if shown == 0 {
            RichText::new(label).small()
        } else {
            RichText::new(label).small().strong()
        };
        ui.menu_button(text, |ui| self.filters_items(ui, tab))
            .response
            .on_hover_text("Visual filters: colour the words that match a search")
            .accessible_name("Visual filters in this Tab");
    }

    /// The ⋯ menu's Visual filters submenu, in a Tab too narrow for the Filters button.
    pub(super) fn filters_submenu(&mut self, ui: &mut Ui, tab: obs_engine::TabId) {
        let label = match self.session.tab_visual_filters(tab).len() {
            0 => "Visual filters".to_string(),
            n => format!("Visual filters ({n})"),
        };
        ui.menu_button(label, |ui| self.filters_items(ui, tab));
    }

    /// Each Visual filter with a checkbox that shows or hides it in Tab `tab`, then Visual
    /// filters… to make and edit them.
    fn filters_items(&mut self, ui: &mut Ui, tab: obs_engine::TabId) {
        let filters = self.session.visual_filters().to_vec();
        let shown: Vec<String> = self
            .session
            .tab_visual_filters(tab)
            .into_iter()
            .map(|f| f.id)
            .collect();
        let palette = self.palette;
        let mut run: Option<(&'static str, Value)> = None;
        ui.label(crate::components::hint_text("Visual filters in this Tab"));
        for f in &filters {
            ui.horizontal(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(vec2(size::ICON_LARGE, size::ICON), Sense::hover());
                paint_swatch(ui, rect, &f.appearance(), &palette, false);
                let mut on = shown.contains(&f.id);
                if ui
                    .checkbox(&mut on, &f.name)
                    .on_hover_text(finds(f))
                    .changed()
                {
                    run = Some((
                        "tab.visual_filter",
                        json!({ "tab": tab.0, "id": f.id, "on": on }),
                    ));
                }
            });
        }
        if filters.is_empty() {
            ui.label("No Visual filters yet.");
        }
        ui.separator();
        if ui.button("Visual filters").clicked() {
            run = Some(("view.visual_filters", json!({})));
            ui.close();
        }
        if let Some((cmd, params)) = run {
            self.session.workspace.user_focused(tab);
            self.run(cmd, &params);
        }
    }
}
