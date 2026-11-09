//! The Library Tab and the About Tab (#46). The Library lists Resources grouped by Resource
//! type; within a type the rows are the Preferred order (★ marks the first), reordered by
//! dragging a row's handle or with Move up/down. Language chips and the header filter narrow
//! the list together. The About Tab shows a Resource's manifest, licence, Attribution (drawn by
//! the text pipeline, verbatim) and changes, and offers Remove…

use egui::{Align2, FontId, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use obs_engine::{LibraryFilter, TabId, annotations_phrase, language_name};
use obs_model::{Manifest, Origin, ResourceType};
use serde_json::json;

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
                ui.label(
                    RichText::new(
                        "Your library is empty. Use Library → Import… to add a USFM Resource, \
                             or drop a .obr file on the window.",
                    )
                    .color(theme::c().muted),
                );
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
                        ui.label(
                            RichText::new("Nothing matches the filter.").color(theme::c().muted),
                        );
                        return;
                    }
                    ui.set_height(total - gap);
                    let origin = ui.max_rect().min;
                    let width = ui.available_width();
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

/// Draw one row: [handle] [★] abbreviation, title, language, licence badge.
fn library_row(
    ui: &mut Ui,
    tab: TabId,
    m: &Manifest,
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
        painter.rect_filled(rect, radius::S, c.selection);
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
    let x = rect.left() + HANDLE_W + space::XL;
    let abbrev = painter.text(
        pos2(x, y),
        Align2::LEFT_CENTER,
        &m.abbreviation,
        FontId::proportional(text::BODY),
        c.ink,
    );
    // Right side: licence badge, then language.
    let badge_font = FontId::proportional(text::CAPTION);
    let galley = painter.layout_no_wrap(m.license.clone(), badge_font.clone(), c.muted);
    let pad = space::S;
    let badge = Rect::from_min_size(
        pos2(
            rect.right() - galley.size().x - 2.0 * pad - space::XS,
            y - size::BADGE / 2.0,
        ),
        vec2(galley.size().x + 2.0 * pad, size::BADGE),
    );
    painter.rect_stroke(
        badge,
        size::BADGE / 2.0,
        Stroke::new(stroke::HAIR, c.rule),
        egui::StrokeKind::Inside,
    );
    painter.text(
        badge.center(),
        Align2::CENTER_CENTER,
        &m.license,
        badge_font,
        c.muted,
    );
    let lang = painter.text(
        pos2(badge.left() - space::M, y),
        Align2::RIGHT_CENTER,
        &m.language,
        FontId::proportional(text::SMALL),
        c.muted,
    );
    let title_left = abbrev.right().max(x + size::FIELD_NUMBER) + space::M;
    let title_clip = Rect::from_x_y_ranges(title_left..=lang.left() - space::M, rect.y_range());
    if title_clip.width() > space::XXL {
        ui.painter_at(title_clip).text(
            pos2(title_left, y),
            Align2::LEFT_CENTER,
            &m.title,
            FontId::proportional(text::LABEL),
            c.ink,
        );
    }
    let body = body.on_hover_text(format!(
        "{} · {} · {} — double-click or Enter to open, or drag it onto a Panel; right-click \
         for more",
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
}
