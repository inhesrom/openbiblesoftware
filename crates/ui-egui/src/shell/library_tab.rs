//! The Library Tab and the About Tab (#46). The Library lists Resources grouped by Resource
//! type; within a type the rows are the Preferred order (★ marks the first), reordered by
//! dragging a row's handle or with Move up/down. Language chips and the header filter narrow
//! the list together. The About Tab shows a Resource's manifest, licence, Attribution (drawn by
//! the text pipeline, verbatim) and changes, and offers Remove…

use egui::{Align2, FontId, Rangef, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use obs_engine::{LibraryFilter, TabId, annotations_phrase, language_name};
use obs_model::{Manifest, Origin, ResourceType};
use serde_json::json;

use super::get_bibles_tab::spdx_badge;
use super::{RESOURCE_TYPES, State, type_name};
use crate::icons::{self, Icon};
use crate::pane::reading_pane;
use crate::theme;
use crate::tokens::{radius, size, space, stroke, text};

/// Height of one Library row.
pub(crate) const ROW_H: f32 = size::ROW;
/// Width of the drag handle at the left of a row.
const HANDLE_W: f32 = size::HANDLE;
/// A Resource type's heading in the list: its text size, and the space above it.
const HEADER_SIZE: f32 = text::CAPTION;
const HEADER_GAP: f32 = space::S;

/// One line of the list: a Resource type's heading, or a row (its type, its position within
/// the type in the full Preferred order, its manifest).
enum Item<'a> {
    Header(ResourceType),
    Row(ResourceType, usize, &'a Manifest),
}

/// One row as drawn this frame, for working out where a dragged row lands.
struct Row {
    id: String,
    ty: ResourceType,
    /// Position within its type in the full (unfiltered) Preferred order.
    position: usize,
    rect: Rect,
}

/// What the user asked for this frame.
enum Action {
    Open(String),
    About(String),
    Remove(String),
    Move(String, usize),
    Step(String, bool),
    /// Add a Resource to a Collection by hand, or take it out: (Collection, resource id).
    Collect(String, String),
    /// The empty Library's Button (#223).
    Starter,
    /// Nothing matches the filter: show every Resource again.
    ClearFilter,
}

impl State {
    pub(super) fn library_body(&mut self, ui: &mut Ui, tab: TabId) {
        let Some(t) = self.tabs.get_mut(&tab) else {
            return;
        };
        let languages = self.session.library.languages();
        // Removal can retire a chip; never leave its filter selected invisibly.
        t.languages
            .retain(|lang| languages.contains(&lang.as_str()));
        // A deleted Collection never stays chosen invisibly.
        if t.collection
            .as_ref()
            .is_some_and(|c| self.session.library.collection(c).is_none())
        {
            t.collection = None;
        }
        let filter = LibraryFilter {
            text: t.filter.clone(),
            languages: t.languages.clone(),
            collection: t.collection.clone(),
        };
        let selected = t.selected.clone();
        let mut dragging = t.dragging.clone();
        let mut action: Option<Action> = None;
        let mut select: Option<String> = None;
        let mut toggle: Option<String> = None;
        let mut bar = None;
        let mut drag_start: Option<String> = None;
        let mut drag_end = false;
        let mut resource_drag: Option<(String, String)> = None;
        let mut rows: Vec<Row> = Vec::new();
        let mut drop_region = Rect::NOTHING;
        let enter_pressed =
            ui.input(|i| i.key_pressed(egui::Key::Enter)) && !egui::Popup::is_any_open(ui.ctx());
        crate::components::page_frame().show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            let lib = &self.session.library;
            if lib.entries().is_empty() {
                if crate::components::empty_state(
                    ui,
                    "Your library is empty. Add the Starter library's open Bibles, lexicons and \
                     reference books, import a file with Library → Import…, or drop a .obr file \
                     on the window.",
                    super::missing_resource::ADD_STARTER,
                )
                .clicked()
                {
                    action = Some(Action::Starter);
                }
                return;
            }
            // Language chips.
            ui.horizontal_wrapped(|ui| {
                for lang in languages {
                    let on = filter.languages.iter().any(|l| l == lang);
                    let chip = egui::Button::selectable(on, lang).corner_radius(radius::PILL);
                    if ui
                        .add(chip)
                        .on_hover_text(format!(
                            "Show {lang} Resources (chips add to each other; with none on, \
                                 every language shows)"
                        ))
                        .clicked()
                    {
                        toggle = Some(lang.to_string());
                    }
                }
            });
            bar = super::collections_ui::collections_bar(ui, lib, filter.collection.as_deref());
            ui.add_space(space::XS);
            // Only the rows in view are drawn (#227): a library of thousands of Resources
            // lays out a screenful a frame. Every row still has its place, for dragging.
            let mut items: Vec<Item> = Vec::new();
            for ty in RESOURCE_TYPES {
                let start = items.len();
                items.extend(
                    lib.of_type(ty)
                        .enumerate()
                        .filter(|(_, e)| filter.matches_in(lib, &e.manifest))
                        .map(|(position, e)| Item::Row(ty, position, &e.manifest)),
                );
                if items.len() > start {
                    items.insert(start, Item::Header(ty));
                }
            }
            let gap = ui.spacing().item_spacing.y;
            let header_h = HEADER_GAP
                + ui.fonts_mut(|f| f.row_height(&FontId::proportional(HEADER_SIZE)))
                + gap;
            let row_h = ROW_H + gap;
            let height = |item: &Item| match item {
                Item::Header(_) => header_h,
                Item::Row(..) => row_h,
            };
            let total: f32 = items.iter().map(height).sum();
            let scroll = egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show_viewport(ui, |ui, viewport| {
                    if items.is_empty() {
                        if crate::components::empty_state(
                            ui,
                            "Nothing matches the filter.",
                            "Clear the filter",
                        )
                        .clicked()
                        {
                            action = Some(Action::ClearFilter);
                        }
                        return;
                    }
                    ui.set_height(total - gap);
                    let origin = ui.max_rect().min;
                    let width = ui.available_width();
                    // The rows in view (and a dragged one) share their columns.
                    let mut top = 0.0;
                    let mut in_view = Vec::new();
                    for item in &items {
                        let h = height(item);
                        let shown = top + h > viewport.top() && top < viewport.bottom();
                        top += h;
                        if let Item::Row(_, _, m) = item
                            && (shown || dragging.as_deref() == Some(m.resource_id.as_str()))
                        {
                            in_view.push(*m);
                        }
                    }
                    let columns = RowColumns::fit(Widest::of(ui, in_view), width);
                    let needle = filter.text.trim().to_lowercase();
                    let mut top = 0.0;
                    for item in &items {
                        let h = height(item);
                        let shown = top + h > viewport.top() && top < viewport.bottom();
                        let at = origin + vec2(0.0, top);
                        top += h;
                        let Item::Row(ty, position, m) = *item else {
                            if shown && let Item::Header(ty) = item {
                                let rect = Rect::from_min_size(at, vec2(width, h));
                                let header = egui::UiBuilder::new().max_rect(rect).scope_id(
                                    ui.scope_id().with(("library-header", tab, type_name(*ty))),
                                );
                                ui.scope_builder(header, |ui| {
                                    ui.add_space(HEADER_GAP);
                                    ui.label(
                                        RichText::new(type_name(*ty).to_uppercase())
                                            .size(HEADER_SIZE)
                                            .strong()
                                            .color(theme::c().muted),
                                    );
                                });
                            }
                            continue;
                        };
                        let id = &m.resource_id;
                        let rect = Rect::from_min_size(at, vec2(width, ROW_H));
                        rows.push(Row {
                            id: id.clone(),
                            ty,
                            position,
                            rect,
                        });
                        // The dragged row is laid out out of view too, so its drag ends
                        // with a drop when the list scrolled under it.
                        let dragged_row = dragging.as_deref() == Some(id.as_str());
                        if !shown && !dragged_row {
                            continue;
                        }
                        // Ids of the row's own, not of its place among the rows in view:
                        // keyboard focus stays on the row as the list scrolls.
                        let builder = egui::UiBuilder::new()
                            .max_rect(rect)
                            .scope_id(ui.scope_id().with(("library-row", tab, id.as_str())));
                        let row = ui
                            .scope_builder(builder, |ui| {
                                library_row(
                                    ui,
                                    tab,
                                    m,
                                    &columns,
                                    &needle,
                                    position == 0,
                                    selected.as_deref() == Some(id.as_str()),
                                    dragged_row,
                                )
                            })
                            .inner;
                        if row.handle.drag_started() {
                            drag_start = Some(id.clone());
                        }
                        if row.handle.drag_stopped() {
                            drag_end = true;
                        }
                        // The row itself drags the Resource into the Workspace (#216).
                        if row.body.drag_started() {
                            resource_drag = Some((id.clone(), m.abbreviation.clone()));
                        }
                        if row.body.clicked() || row.body.secondary_clicked() {
                            select = Some(id.clone());
                        }
                        if row.body.double_clicked() || (row.body.has_focus() && enter_pressed) {
                            action = Some(Action::Open(id.clone()));
                        }
                        row.body.context_menu(|ui| {
                            if let Some(a) = row_menu(ui, lib, id) {
                                action = Some(a);
                            }
                        });
                    }
                });
            drop_region = scroll.inner_rect.intersect(ui.clip_rect());
        });

        // Dragging a handle: a line marks the gap the row would drop into, and dropping moves
        // it there. Escape, or dropping well away from the rows of its type, cancels. A drag
        // whose row stopped being drawn (filtered out mid-drag) ends with the button.
        if !drag_end && drag_start.is_none() && !ui.input(|i| i.pointer.any_down()) {
            drag_end = dragging.is_some();
            dragging = None;
        }
        if dragging.is_some() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            drag_end = true;
            dragging = None;
        }
        let pointer = ui.ctx().pointer_interact_pos();
        let dragged = drag_start.clone().or(dragging);
        if let (Some(id), Some(p)) = (dragged, pointer)
            && let Some(slot) = drop_slot(&rows, &id, p, drop_region)
        {
            if drag_end {
                action = Some(Action::Move(id, slot.to));
            } else {
                ui.painter()
                    .hline(slot.x, slot.y, Stroke::new(stroke::THICK, theme::c().lapis));
            }
        }
        // Enter opens the selected row while the Library is the focused Tab.
        let focused = self.session.workspace.focused() == Some(tab);
        if focused
            && action.is_none()
            && ui.memory(|m| m.focused().is_none())
            && enter_pressed
            && let Some(id) = select.clone().or(selected)
        {
            action = Some(Action::Open(id));
        }

        if let Some(t) = self.tabs.get_mut(&tab) {
            if let Some(lang) = toggle {
                let mut f = LibraryFilter {
                    languages: std::mem::take(&mut t.languages),
                    ..LibraryFilter::default()
                };
                f.toggle_language(&lang);
                t.languages = f.languages;
            }
            if let Some(id) = select {
                t.selected = Some(id);
            }
            if drag_end {
                t.dragging = None;
            } else if let Some(id) = drag_start {
                t.selected = Some(id.clone());
                t.dragging = Some(id);
            }
        }
        // Dropped in the window this Library Tab is drawn in (`workspace_drag`).
        if let Some((id, abbreviation)) = resource_drag {
            self.resource_drag_started(ui, tab, id, abbreviation);
        }
        if drag_end || action.is_some() || bar.is_some() {
            self.session.workspace.user_focused(tab);
        }
        if let Some(bar) = bar {
            self.collections_bar_action(tab, bar);
        }
        match action {
            None => {}
            Some(Action::Starter) => {
                self.run("library.starter", &json!({}));
            }
            Some(Action::ClearFilter) => {
                if let Some(t) = self.tabs.get_mut(&tab) {
                    t.filter.clear();
                    t.languages.clear();
                    t.collection = None;
                }
            }
            Some(Action::Open(id)) => {
                let split = self.split_held;
                self.run("resource.open", &json!({ "id": id, "split": split }));
            }
            Some(Action::About(id)) => {
                self.run("resource.about", &json!({ "id": id }));
            }
            Some(Action::Remove(id)) => {
                self.run("library.remove", &json!({ "id": id }));
            }
            Some(Action::Move(id, to)) => {
                self.run("library.move", &json!({ "id": id, "to": to }));
            }
            Some(Action::Collect(name, id)) => {
                self.run("collection.toggle", &json!({ "name": name, "id": id }));
            }
            Some(Action::Step(id, up)) => {
                let cmd = if up {
                    "library.move_up"
                } else {
                    "library.move_down"
                };
                self.run(cmd, &json!({ "id": id }));
            }
        }
    }

    pub(super) fn about_body(&mut self, ui: &mut Ui, tab: TabId, id: &str) {
        let changes_record = self
            .session
            .library
            .get(id)
            .and_then(obs_engine::credits::changes_record_url);
        let Some(m) = self.session.library.get(id).map(|e| e.manifest.clone()) else {
            ui.centered_and_justified(|ui| {
                ui.colored_label(theme::c().warn, format!("{id} is not in the library"))
            });
            return;
        };
        let annotations = self.session.annotations_referring(id);
        let current_tags = self.session.library.tags_of(id).join(", ");
        let mut set_tags: Option<String> = None;
        let mut action: Option<Action> = None;
        crate::components::page_frame().show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.label(crate::components::title_text(&m.title));
                    ui.label(
                        RichText::new(format!(
                            "{} · {} · {}",
                            m.abbreviation,
                            type_singular(m.resource_type),
                            language_name(&m.language)
                        ))
                        .color(theme::c().muted),
                    );
                    ui.add_space(space::M);
                    egui::Grid::new(("about-fields", tab))
                        .num_columns(2)
                        .spacing(vec2(space::L, space::XS))
                        .show(ui, |ui| {
                            for (label, value) in about_fields(&m) {
                                ui.label(RichText::new(label).color(theme::c().muted));
                                ui.label(value);
                                ui.end_row();
                            }
                            // Resource tags (#153): the user's own labels, for a
                            // Collection's tag: rules. Saved when the field loses focus.
                            ui.label(RichText::new("Tags").color(theme::c().muted));
                            if let Some(t) = self.tabs.get_mut(&tab) {
                                let text = t.tags_edit.get_or_insert_with(|| current_tags.clone());
                                let r = ui
                                    .add(crate::components::search_field(
                                        text,
                                        "older, reformed",
                                        size::FIELD_WIDE,
                                    ))
                                    .on_hover_text(
                                        "Your own labels for this Resource, comma-separated; \
                                             a Collection's tag: rule matches them",
                                    );
                                if r.lost_focus() && *text != current_tags {
                                    set_tags = Some(text.clone());
                                }
                                if !r.has_focus() {
                                    t.tags_edit = None;
                                }
                            }
                            ui.end_row();
                        });
                    ui.add_space(space::L);
                    section(ui, "Attribution");
                    // Verbatim, through the text pipeline (fonts for every script).
                    let width = ui.available_width();
                    let palette = self.palette;
                    if m.attribution.is_empty() {
                        ui.label("None recorded.");
                    } else if let Some(view) = self.tabs.get_mut(&tab).and_then(|t| t.view.as_mut())
                    {
                        view.set_settings(self.session.text);
                        // As tall as the text (once laid out), so the page scrolls rather
                        // than a box inside it; only a very long Attribution scrolls itself.
                        #[allow(clippy::cast_possible_truncation)]
                        let height = (view.position().1 as f32 + space::XL)
                            .clamp(size::FIELD_NUMBER, size::DIALOG.x);
                        ui.allocate_ui(vec2(width, height), |ui| {
                            reading_pane(
                                ui,
                                view,
                                &mut self.atlas,
                                &palette,
                                None,
                                crate::pane::PaneMarks::default(),
                                crate::Found::default(),
                            );
                        });
                    }
                    ui.add_space(space::L);
                    section(ui, "Changes we made");
                    ui.label(m.changes.as_deref().unwrap_or("None recorded."));
                    // CC BY Starter Resources: the record published with each release
                    // (#20 decision 4, #67).
                    if let Some(url) = &changes_record {
                        // One row: a wrapped link would start mid-line, off its own text.
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Published change record:").color(theme::c().muted),
                            );
                            crate::components::hyperlink_to(
                                ui,
                                obs_model::changes_record_file(&m.resource_id),
                                url,
                            )
                            .on_hover_text(url);
                        });
                    }
                    ui.add_space(space::L);
                    section(ui, "Source file");
                    ui.label(source_file(&m));
                    ui.add_space(space::L);
                    section(ui, "Annotations");
                    ui.label(format!(
                        "{} to this Resource.",
                        annotations_phrase(annotations)
                    ));
                    ui.add_space(space::XL);
                    ui.horizontal(|ui| {
                        if ui.button("Open").clicked() {
                            action = Some(Action::Open(id.to_string()));
                        }
                        if self.session.imports.report(id).is_some()
                            && ui.button("Latest Import report").clicked()
                        {
                            self.run("import.report", &json!({ "id": id }));
                        }
                        if ui.button("Remove…").clicked() {
                            action = Some(Action::Remove(id.to_string()));
                        }
                    });
                });
        });
        if let Some(tags) = set_tags {
            self.run("resource.tags", &json!({ "id": id, "tags": tags }));
        }
        match action {
            Some(Action::Open(id)) => {
                self.run("resource.open", &json!({ "id": id }));
            }
            Some(Action::Remove(id)) => {
                self.run("library.remove", &json!({ "id": id }));
            }
            _ => {}
        }
    }

    /// The Remove… confirmation, stating how many Annotations refer to the Resource.
    pub(super) fn remove_prompt(&mut self, ctx: &egui::Context) {
        let Some((id, title, message)) = self.confirm.take() else {
            return;
        };
        let mut done: Option<bool> = None;
        egui::Window::new(title.as_str())
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_max_width(size::PROSE);
                ui.label(message.as_str());
                ui.add_space(space::M);
                ui.horizontal(|ui| {
                    if ui.button("Remove").clicked() {
                        done = Some(true);
                    }
                    if ui.button("Cancel").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        done = Some(false);
                    }
                });
            });
        match done {
            None => self.confirm = Some((id, title, message)),
            Some(false) => {}
            Some(true) => {
                self.run("library.remove", &json!({ "id": id, "confirm": true }));
            }
        }
    }
}

/// A Library row's two interactive parts.
struct RowResponse {
    body: egui::Response,
    handle: egui::Response,
}

/// The widest abbreviation, language and licence badge among some Library rows.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Widest {
    abbreviation: f32,
    language: f32,
    badge: f32,
}

impl Widest {
    fn of<'a>(ui: &Ui, rows: impl IntoIterator<Item = &'a Manifest>) -> Self {
        let painter = ui.painter();
        let width = |text: &str, size: f32| {
            painter
                .layout_no_wrap(
                    text.to_string(),
                    FontId::proportional(size),
                    egui::Color32::PLACEHOLDER,
                )
                .size()
                .x
        };
        let mut widest = Widest::default();
        for m in rows {
            widest.abbreviation = widest.abbreviation.max(width(&m.abbreviation, text::BODY));
            widest.language = widest.language.max(width(&m.language, text::SMALL));
            let (badge, _) = spdx_badge(&m.license);
            widest.badge = widest
                .badge
                .max(width(badge, text::CAPTION) + 2.0 * BADGE_PAD);
        }
        widest
    }
}

/// Where the parts of the Library rows in view go, as x ranges from a row's left edge (#302):
/// one abbreviation, title, language and licence badge column for every row, so they line
/// up. A narrow Panel (or a Larger Interface size) drops the badge, then the language, then
/// narrows the abbreviation (to [`size::FIELD_NUMBER`]), before the title gets narrower than
/// [`size::TITLE_MIN`]; text that still does not fit ends in "…".
#[derive(Clone, Copy, Debug, PartialEq)]
struct RowColumns {
    abbreviation: Rangef,
    title: Rangef,
    language: Option<Rangef>,
    badge: Option<Rangef>,
}

impl RowColumns {
    fn fit(widest: Widest, width: f32) -> Self {
        let left = HANDLE_W + space::XL;
        let right = (width - space::XS).max(left);
        // The abbreviation yields to the title, down to a short abbreviation's width.
        let room = right - left;
        let abbreviation = Rangef::new(
            left,
            left + widest
                .abbreviation
                .min(size::FIELD_NARROW)
                .min((room - space::M - size::TITLE_MIN).max(size::FIELD_NUMBER)),
        );
        let title_left = abbreviation.max + space::M;
        // A column's width with the gap before it; an empty column takes none.
        let with_gap = |w: f32| if w > 0.0 { w + space::M } else { 0.0 };
        let fits = |taken: f32| right - taken - title_left >= size::TITLE_MIN;
        let language_w = with_gap(widest.language);
        let badge_w = with_gap(widest.badge);
        let (language, badge) = if fits(language_w + badge_w) {
            (language_w > 0.0, badge_w > 0.0)
        } else {
            (language_w > 0.0 && fits(language_w), false)
        };
        let mut end = right;
        let badge = badge.then(|| {
            let r = Rangef::new(end - widest.badge, end);
            end = r.min - space::M;
            r
        });
        let language = language.then(|| {
            let r = Rangef::new(end - widest.language, end);
            end = r.min - space::M;
            r
        });
        RowColumns {
            abbreviation,
            title: Rangef::new(title_left, end.max(title_left)),
            language,
            badge,
        }
    }
}

/// The space between a licence badge's edge and its text.
const BADGE_PAD: f32 = space::S;

/// Where the Filter's `needle` (lower case) first matches in `title`, ignoring case: the
/// match's end, in chars, and the title from the start of the word it is in.
fn filter_match<'t>(title: &'t str, needle: &str) -> Option<(usize, &'t str)> {
    if needle.is_empty() {
        return None;
    }
    let (chars_before, (at, _)) = title
        .char_indices()
        .enumerate()
        .find(|(_, (i, _))| title[*i..].to_lowercase().starts_with(needle))?;
    let word = title[..at].rfind(char::is_whitespace).map_or(0, |w| {
        w + title[w..].chars().next().map_or(0, char::len_utf8)
    });
    Some((chars_before + needle.chars().count(), &title[word..]))
}

/// Draw one row: [handle] [★] abbreviation, title, language, licence badge, in `columns`.
/// A title cut short before the part the Filter's `needle` matched shows from that word on.
#[allow(clippy::too_many_arguments)]
fn library_row(
    ui: &mut Ui,
    tab: TabId,
    m: &Manifest,
    columns: &RowColumns,
    needle: &str,
    first: bool,
    selected: bool,
    dragging: bool,
) -> RowResponse {
    let width = ui.available_width();
    let (rect, body) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::click_and_drag());
    let handle_rect = Rect::from_min_size(rect.min, vec2(HANDLE_W, ROW_H));
    let handle = ui
        .interact(
            handle_rect,
            ui.scope_id().with(("library-handle", tab, &m.resource_id)),
            Sense::drag(),
        )
        .on_hover_cursor(egui::CursorIcon::Grab)
        .on_hover_text("Drag to change the Preferred order");
    let c = theme::c();
    let painter = ui.painter_at(rect);
    if selected || dragging {
        crate::components::selected_row(&painter, rect, radius::S);
    } else if body.hovered() {
        painter.rect_filled(rect, radius::S, c.hover);
    }
    let y = rect.center().y;
    let at =
        |x: f32| Rect::from_center_size(pos2(rect.left() + x, y), egui::Vec2::splat(icons::BODY));
    icons::paint(
        &painter,
        Icon::Grip,
        at(space::L),
        icons::SMALL,
        if handle.hovered() { c.ink } else { c.muted },
    );
    if first {
        icons::paint(
            &painter,
            Icon::StarFilled,
            at(HANDLE_W + space::S),
            icons::SMALL,
            c.gilt,
        );
    }
    // Each part on one line in its column, left-aligned and centred on the row.
    let layout = |column: Rangef, text: &str, size: f32, color: egui::Color32| {
        crate::components::elided(
            &painter,
            text,
            FontId::proportional(size),
            color,
            column.span(),
        )
    };
    let paint = |column: Rangef, galley: std::sync::Arc<egui::Galley>| {
        let at = pos2(rect.left() + column.min, y - galley.size().y / 2.0);
        painter.galley(at, galley, c.ink);
    };
    paint(
        columns.abbreviation,
        layout(columns.abbreviation, &m.abbreviation, text::BODY, c.ink),
    );
    let mut title = layout(columns.title, &m.title, text::LABEL, c.ink);
    if title.elided
        && let Some((end, from_word)) = filter_match(&m.title, needle)
        // What shows, less the "…".
        && end + 1 > title.rows.iter().map(|r| r.glyphs.len()).sum::<usize>()
    {
        title = layout(columns.title, &format!("…{from_word}"), text::LABEL, c.ink);
    }
    paint(columns.title, title);
    if let Some(column) = columns.language {
        paint(column, layout(column, &m.language, text::SMALL, c.muted));
    }
    let (licence, _) = spdx_badge(&m.license);
    if let Some(column) = columns.badge {
        let galley = painter.layout_no_wrap(
            licence.to_string(),
            FontId::proportional(text::CAPTION),
            c.muted,
        );
        let badge = Rect::from_min_size(
            pos2(rect.left() + column.min, y - size::BADGE / 2.0),
            vec2(galley.size().x + 2.0 * BADGE_PAD, size::BADGE),
        );
        painter.rect_stroke(
            badge,
            size::BADGE / 2.0,
            Stroke::new(stroke::HAIR, c.rule),
            egui::StrokeKind::Inside,
        );
        painter.galley(badge.center() - galley.size() / 2.0, galley, c.muted);
    }
    let body = body.on_hover_text(format!(
        "{} · {} · {licence} ({}) — double-click or Enter to open, or drag it onto a Panel; \
         right-click for more",
        m.title, m.language, m.license
    ));
    RowResponse { body, handle }
}

/// The right-click menu of a Library row.
fn row_menu(ui: &mut Ui, library: &obs_engine::Library, id: &str) -> Option<Action> {
    let mut action = None;
    let items: [(&str, Action); 5] = [
        ("Open", Action::Open(id.to_string())),
        ("About", Action::About(id.to_string())),
        ("Move up", Action::Step(id.to_string(), true)),
        ("Move down", Action::Step(id.to_string(), false)),
        ("Remove…", Action::Remove(id.to_string())),
    ];
    for (i, (label, a)) in items.into_iter().enumerate() {
        if i == 2
            && let Some(name) = super::collections_ui::collections_menu(ui, library, id)
        {
            action = Some(Action::Collect(name, id.to_string()));
        }
        if i == 2 || i == 4 {
            ui.separator();
        }
        if ui.button(label).clicked() {
            action = Some(a);
            ui.close();
        }
    }
    action
}

/// Where a dragged row would drop.
#[derive(Debug, PartialEq)]
struct DropSlot {
    /// The row's position within its type once moved (the `to` of `library.move`).
    to: usize,
    /// Where the drop line is drawn: across the row, in the gap it would drop into.
    x: egui::Rangef,
    y: f32,
}

/// The gap a dragged row drops into: after every other row of its type whose centre is above
/// the pointer. `None` outside the Library's visible list, when that leaves the row where it
/// is, or when the pointer is more than a row away from the rows of its type.
fn drop_slot(rows: &[Row], dragged: &str, pointer: egui::Pos2, region: Rect) -> Option<DropSlot> {
    if !region.contains(pointer) {
        return None;
    }
    let y = pointer.y;
    let me = rows.iter().find(|r| r.id == dragged)?;
    let others: Vec<&Row> = rows
        .iter()
        .filter(|r| r.ty == me.ty && r.id != dragged)
        .collect();
    let (first, last) = (others.first()?, others.last()?);
    let top = first.rect.top().min(me.rect.top());
    let bottom = last.rect.bottom().max(me.rect.bottom());
    if y < top - ROW_H || y > bottom + ROW_H {
        return None;
    }
    let slot = others.iter().filter(|r| r.rect.center().y < y).count();
    let (before, after) = (slot.checked_sub(1).map(|i| others[i]), others.get(slot));
    // A position in the full Preferred order of the type; rows hidden by the filter between
    // `before` and `after` stay above the dropped row.
    let at = after.map_or(last.position + 1, |r| r.position);
    let to = if me.position < at { at - 1 } else { at };
    if to == me.position {
        return None;
    }
    let line = match (before, after) {
        (Some(a), Some(b)) => (a.rect.bottom() + b.rect.top()) / 2.0,
        (None, Some(b)) => b.rect.top(),
        (Some(a), None) => a.rect.bottom(),
        (None, None) => return None,
    };
    Some(DropSlot {
        to,
        x: me.rect.x_range(),
        y: line,
    })
}

pub(super) fn section(ui: &mut Ui, title: &str) {
    crate::components::section_header(ui, title);
}

pub(super) fn type_singular(t: ResourceType) -> &'static str {
    match t {
        ResourceType::Bible => "Bible",
        ResourceType::Commentary => "Commentary",
        ResourceType::Lexicon => "Lexicon",
        ResourceType::Dictionary => "Dictionary",
        ResourceType::CrossReferenceSet => "Cross-reference set",
        ResourceType::PeopleAndPlaces => "People & places index",
    }
}

/// The manifest fields shown at the top of the About Tab.
fn about_fields(m: &Manifest) -> Vec<(&'static str, String)> {
    let mut out = vec![("Licence", m.license.clone())];
    if let Some(p) = &m.publisher {
        out.push(("Publisher", p.clone()));
    }
    if let Some(e) = &m.edition {
        out.push(("Edition", e.clone()));
    }
    out.push(("Revision", m.revision.to_string()));
    if let Some(u) = &m.source_url {
        out.push(("Source", u.clone()));
    }
    out.push((
        "Origin",
        match m.origin {
            Origin::Starter => "Starter library".to_string(),
            Origin::Imported => "Imported".to_string(),
        },
    ));
    out.push(("Resource id", m.resource_id.clone()));
    out
}

fn source_file(m: &Manifest) -> String {
    match (&m.source_file, m.source_format.is_empty()) {
        (Some(f), false) => format!("{f} ({})", m.source_format),
        (Some(f), true) => f.clone(),
        (None, false) => m.source_format.clone(),
        (None, true) => "Not recorded.".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bible rows A, B, C, `ROW_H` tall with 6-point gaps, from y = 0.
    fn rows() -> Vec<Row> {
        ["A", "B", "C"]
            .iter()
            .enumerate()
            .map(|(i, id)| {
                #[allow(clippy::cast_precision_loss)]
                let top = i as f32 * (ROW_H + 6.0);
                Row {
                    id: (*id).to_string(),
                    ty: ResourceType::Bible,
                    position: i,
                    rect: Rect::from_min_size(pos2(0.0, top), vec2(200.0, ROW_H)),
                }
            })
            .collect()
    }

    fn drop_at(dragged: &str, y: f32) -> Option<(usize, f32)> {
        drop_slot(&rows(), dragged, pos2(100.0, y), drop_region()).map(|s| (s.to, s.y))
    }

    fn drop_region() -> Rect {
        Rect::from_min_max(pos2(0.0, -ROW_H), pos2(200.0, 96.0 + ROW_H))
    }

    #[test]
    fn drops_outside_the_visible_library_list_cancel() {
        let region = drop_region();
        for p in [
            pos2(region.left() - 1.0, 8.0),
            pos2(region.right() + 1.0, 8.0),
            pos2(100.0, region.top() - 1.0),
            pos2(100.0, region.bottom() + 1.0),
        ] {
            assert_eq!(drop_slot(&rows(), "C", p, region), None);
        }
        // Even a row still partly laid out above the viewport is not a drop target there.
        let clipped = Rect::from_min_max(pos2(0.0, 10.0), region.max);
        assert_eq!(drop_slot(&rows(), "C", pos2(100.0, 8.0), clipped), None);
    }

    #[test]
    fn the_drop_line_marks_where_the_row_lands() {
        // A 0..28, B 34..62, C 68..96: gaps centred on 31 and 65.
        // C on the lower half of A: between A and B, not above A.
        assert_eq!(drop_at("C", 20.0), Some((1, 31.0)));
        // C in the gap between A and B: the same.
        assert_eq!(drop_at("C", 31.0), Some((1, 31.0)));
        // C on the upper half of A: above A, so C becomes ★.
        assert_eq!(drop_at("C", 8.0), Some((0, 0.0)));
        // A on the upper half of B, or in the gap above it: A stays put.
        assert_eq!(drop_at("A", 40.0), None);
        assert_eq!(drop_at("A", 31.0), None);
        // A on the lower half of B: below B.
        assert_eq!(drop_at("A", 55.0), Some((1, 65.0)));
        // A below C: last.
        assert_eq!(drop_at("A", 100.0), Some((2, 96.0)));
        // More than a row away from the rows: cancelled.
        assert_eq!(drop_at("A", 96.0 + ROW_H + 1.0), None);
        assert_eq!(drop_at("C", -ROW_H - 1.0), None);
    }

    #[test]
    fn filtered_rows_keep_their_place_above_a_dropped_row() {
        // B is hidden by the filter: the shown rows are A (position 0) and C (position 2).
        let mut rows = rows();
        rows.remove(1);
        rows[1].rect = rows[1].rect.translate(vec2(0.0, -(ROW_H + 6.0)));
        // C onto the upper half of A: first.
        assert_eq!(
            drop_slot(&rows, "C", pos2(100.0, 5.0), drop_region()).map(|s| s.to),
            Some(0)
        );
        // A onto the lower half of C: after C, so last of all three.
        assert_eq!(
            drop_slot(&rows, "A", pos2(100.0, 60.0), drop_region()).map(|s| s.to),
            Some(2)
        );
        // A onto the upper half of C: before C, after the hidden B.
        assert_eq!(
            drop_slot(&rows, "A", pos2(100.0, 36.0), drop_region()).map(|s| s.to),
            Some(1)
        );
    }

    /// Widths like the Starter library's: "Abbott-Smith", "grc", "Public domain".
    const WIDEST: Widest = Widest {
        abbreviation: 84.0,
        language: 20.0,
        badge: 80.0,
    };

    fn in_order(c: &RowColumns) -> Vec<Rangef> {
        let mut out = vec![c.abbreviation, c.title];
        out.extend(c.language);
        out.extend(c.badge);
        out
    }

    fn assert_apart(c: &RowColumns, width: f32) {
        let parts = in_order(c);
        for pair in parts.windows(2) {
            assert!(pair[0].max + space::M <= pair[1].min + 0.01, "{c:?}");
        }
        assert!(parts.last().unwrap().max <= width - space::XS, "{c:?}");
    }

    #[test]
    fn a_wide_row_has_every_column_and_a_title_of_its_own() {
        let c = RowColumns::fit(WIDEST, 460.0);
        assert!(c.language.is_some() && c.badge.is_some(), "{c:?}");
        assert_eq!(c.abbreviation.span(), WIDEST.abbreviation);
        assert!(c.title.span() >= size::TITLE_MIN, "{c:?}");
        assert_eq!(c.badge.unwrap().max, 460.0 - space::XS);
        assert_apart(&c, 460.0);
    }

    #[test]
    fn a_narrowing_row_drops_the_badge_then_the_language_before_the_title() {
        let mut dropped_badge_at = None;
        let mut dropped_language_at = None;
        for width in (100..=600).rev().map(|w| w as f32) {
            let c = RowColumns::fit(WIDEST, width);
            assert_apart(&c, width);
            assert!(
                c.language.is_some() || c.badge.is_none(),
                "the language went before the badge: {c:?}"
            );
            if c.badge.is_some() || c.language.is_some() {
                assert!(c.title.span() >= size::TITLE_MIN, "{width}: {c:?}");
            }
            if c.badge.is_none() && dropped_badge_at.is_none() {
                dropped_badge_at = Some(width);
            }
            if c.language.is_none() && dropped_language_at.is_none() {
                dropped_language_at = Some(width);
            }
        }
        assert!(
            dropped_badge_at > dropped_language_at,
            "{dropped_badge_at:?}"
        );
    }

    #[test]
    fn a_long_abbreviation_yields_to_the_title() {
        let long = Widest {
            abbreviation: 300.0,
            ..WIDEST
        };
        assert_eq!(
            RowColumns::fit(long, 600.0).abbreviation.span(),
            size::FIELD_NARROW
        );
        let c = RowColumns::fit(long, 260.0);
        assert_eq!(c.title.span(), size::TITLE_MIN, "{c:?}");
        assert_apart(&c, 260.0);
        let c = RowColumns::fit(long, 180.0);
        assert_eq!(c.abbreviation.span(), size::FIELD_NUMBER);
        assert!(c.title.span() > size::FIELD_NUMBER, "{c:?}");
        assert_apart(&c, 180.0);
    }

    #[test]
    fn a_filter_match_starts_at_its_word() {
        let kjv = "King James Version with Strong's Numbers";
        assert_eq!(filter_match(kjv, "strong"), Some((30, "Strong's Numbers")));
        assert_eq!(filter_match(kjv, "king"), Some((4, kjv)));
        assert_eq!(
            filter_match(kjv, "ames"),
            Some((10, "James Version with Strong's Numbers"))
        );
        assert_eq!(
            filter_match(kjv, "s ver"),
            Some((14, "James Version with Strong's Numbers"))
        );
        assert_eq!(filter_match(kjv, "hebrew"), None);
        assert_eq!(filter_match(kjv, ""), None);
        assert_eq!(filter_match("Ἀρχὴ Λόγος", "λόγ"), Some((8, "Λόγος")));
    }
}
