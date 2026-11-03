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
use crate::pane::reading_pane;
use crate::theme;

/// Height of one Library row.
const ROW_H: f32 = 28.0;
/// Width of the drag handle at the left of a row.
const HANDLE_W: f32 = 22.0;

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
}

impl State {
    pub(super) fn library_body(&mut self, ui: &mut Ui, tab: TabId) {
        let Some(t) = self.tabs.get(&tab) else {
            return;
        };
        let filter = LibraryFilter {
            text: t.filter.clone(),
            languages: t.languages.clone(),
        };
        let selected = t.selected.clone();
        let mut dragging = t.dragging.clone();
        let mut action: Option<Action> = None;
        let mut select: Option<String> = None;
        let mut toggle: Option<String> = None;
        let mut drag_start: Option<String> = None;
        let mut drag_end = false;
        let mut rows: Vec<Row> = Vec::new();
        egui::Frame::new()
            .fill(theme::c().field)
            .inner_margin(egui::Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                let lib = &self.session.library;
                if lib.entries().is_empty() {
                    ui.label(
                        RichText::new(
                            "Your library is empty. Drop a .obr file on the window, or make one \
                             with `obs-cli import`.",
                        )
                        .color(theme::c().muted),
                    );
                    return;
                }
                // Language chips.
                ui.horizontal_wrapped(|ui| {
                    for lang in lib.languages() {
                        let on = filter.languages.iter().any(|l| l == lang);
                        let chip =
                            egui::Button::selectable(on, language_name(lang)).corner_radius(11.0);
                        if ui
                            .add(chip)
                            .on_hover_text(format!("Show only {lang} Resources"))
                            .clicked()
                        {
                            toggle = Some(lang.to_string());
                        }
                    }
                });
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let mut any = false;
                        for ty in RESOURCE_TYPES {
                            let shown: Vec<(usize, &Manifest)> = lib
                                .of_type(ty)
                                .enumerate()
                                .filter(|(_, e)| filter.matches(&e.manifest))
                                .map(|(i, e)| (i, &e.manifest))
                                .collect();
                            if shown.is_empty() {
                                continue;
                            }
                            any = true;
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(type_name(ty).to_uppercase())
                                    .size(10.5)
                                    .strong()
                                    .color(theme::c().muted),
                            );
                            for (position, m) in shown {
                                let id = &m.resource_id;
                                let row = library_row(
                                    ui,
                                    tab,
                                    m,
                                    position == 0,
                                    selected.as_deref() == Some(id.as_str()),
                                    dragging.as_deref() == Some(id.as_str()),
                                );
                                if row.handle.drag_started() {
                                    drag_start = Some(id.clone());
                                }
                                if row.handle.drag_stopped() {
                                    drag_end = true;
                                }
                                if row.body.clicked() || row.body.secondary_clicked() {
                                    select = Some(id.clone());
                                }
                                if row.body.double_clicked() {
                                    action = Some(Action::Open(id.clone()));
                                }
                                row.body.context_menu(|ui| {
                                    if let Some(a) = row_menu(ui, id) {
                                        action = Some(a);
                                    }
                                });
                                rows.push(Row {
                                    id: id.clone(),
                                    ty,
                                    position,
                                    rect: row.body.rect,
                                });
                            }
                        }
                        if !any {
                            ui.label(
                                RichText::new("Nothing matches the filter.")
                                    .color(theme::c().muted),
                            );
                        }
                    });
            });

        // Dragging a handle: mark where the row would land; dropping moves it there. A drag
        // whose row stopped being drawn (filtered out mid-drag) ends with the button.
        if !drag_end && drag_start.is_none() && !ui.input(|i| i.pointer.any_down()) {
            drag_end = dragging.is_some();
            dragging = None;
        }
        let pointer = ui.ctx().pointer_interact_pos();
        let dragged = drag_start.clone().or(dragging);
        if let (Some(id), Some(p)) = (dragged, pointer)
            && let Some(target) = drop_target(&rows, &id, p.y)
        {
            if drag_end {
                action = Some(Action::Move(id, target.position));
            } else {
                let y = if p.y > target.rect.center().y && target.id != id {
                    target.rect.bottom()
                } else {
                    target.rect.top()
                };
                ui.painter()
                    .hline(target.rect.x_range(), y, Stroke::new(2.0, theme::c().lapis));
            }
        }
        // Enter opens the selected row while the Library is the focused Tab.
        let focused = self.session.workspace.focused() == Some(tab);
        if focused
            && action.is_none()
            && ui.memory(|m| m.focused().is_none())
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
            && let Some(id) = select.clone().or(selected)
        {
            action = Some(Action::Open(id));
        }

        if let Some(t) = self.tabs.get_mut(&tab) {
            if let Some(lang) = toggle {
                let mut f = LibraryFilter {
                    text: String::new(),
                    languages: std::mem::take(&mut t.languages),
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
        if drag_end || action.is_some() {
            self.session.workspace.user_focused(tab);
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
        let Some(m) = self.session.library.get(id).map(|e| e.manifest.clone()) else {
            ui.centered_and_justified(|ui| {
                ui.colored_label(theme::c().warn, format!("{id} is not in the library"))
            });
            return;
        };
        let annotations = self.session.annotations_referring(id);
        let mut action: Option<Action> = None;
        egui::Frame::new()
            .fill(theme::c().field)
            .inner_margin(egui::Margin::symmetric(18, 12))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(&m.title)
                                .font(theme::display(20.0))
                                .color(theme::c().ink),
                        );
                        ui.label(
                            RichText::new(format!(
                                "{} · {} · {}",
                                m.abbreviation,
                                type_singular(m.resource_type),
                                language_name(&m.language)
                            ))
                            .color(theme::c().muted),
                        );
                        ui.add_space(8.0);
                        egui::Grid::new(("about-fields", tab))
                            .num_columns(2)
                            .spacing(vec2(14.0, 4.0))
                            .show(ui, |ui| {
                                for (label, value) in about_fields(&m) {
                                    ui.label(RichText::new(label).color(theme::c().muted));
                                    ui.label(value);
                                    ui.end_row();
                                }
                            });
                        ui.add_space(10.0);
                        section(ui, "Attribution");
                        // Verbatim, through the text pipeline (fonts for every script).
                        let width = ui.available_width();
                        let palette = self.palette;
                        if let Some(view) = self.tabs.get_mut(&tab).and_then(|t| t.view.as_mut()) {
                            ui.allocate_ui(vec2(width, 120.0), |ui| {
                                reading_pane(ui, view, &mut self.atlas, &palette, None);
                            });
                        }
                        ui.add_space(10.0);
                        section(ui, "Changes we made");
                        ui.label(m.changes.as_deref().unwrap_or("None recorded."));
                        ui.add_space(10.0);
                        section(ui, "Source file");
                        ui.label(source_file(&m));
                        ui.add_space(10.0);
                        section(ui, "Annotations");
                        ui.label(format!(
                            "{} to this Resource.",
                            annotations_phrase(annotations)
                        ));
                        ui.add_space(14.0);
                        ui.horizontal(|ui| {
                            if ui.button("Open").clicked() {
                                action = Some(Action::Open(id.to_string()));
                            }
                            if ui.button("Remove…").clicked() {
                                action = Some(Action::Remove(id.to_string()));
                            }
                        });
                    });
            });
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
                ui.set_max_width(420.0);
                ui.label(message.as_str());
                ui.add_space(8.0);
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
    let (rect, body) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::click());
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
        painter.rect_filled(rect, 4.0, c.selection);
    } else if body.hovered() {
        painter.rect_filled(rect, 4.0, c.hover);
    }
    let y = rect.center().y;
    let font = FontId::proportional(14.0);
    painter.text(
        pos2(rect.left() + 6.0, y),
        Align2::LEFT_CENTER,
        "≡",
        font.clone(),
        if handle.hovered() { c.ink } else { c.muted },
    );
    if first {
        painter.text(
            pos2(rect.left() + HANDLE_W, y),
            Align2::LEFT_CENTER,
            "★",
            font.clone(),
            c.gilt,
        );
    }
    let x = rect.left() + HANDLE_W + 18.0;
    let abbrev = painter.text(
        pos2(x, y),
        Align2::LEFT_CENTER,
        &m.abbreviation,
        FontId::proportional(14.0),
        c.ink,
    );
    // Right side: licence badge, then language.
    let badge_font = FontId::proportional(11.0);
    let galley = painter.layout_no_wrap(m.license.clone(), badge_font.clone(), c.muted);
    let badge = Rect::from_min_size(
        pos2(rect.right() - galley.size().x - 14.0, y - 9.0),
        vec2(galley.size().x + 10.0, 18.0),
    );
    painter.rect_stroke(
        badge,
        9.0,
        Stroke::new(1.0, c.rule),
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
        pos2(badge.left() - 10.0, y),
        Align2::RIGHT_CENTER,
        &m.language,
        FontId::proportional(12.0),
        c.muted,
    );
    let title_left = abbrev.right().max(x + 48.0) + 10.0;
    let title_clip = Rect::from_x_y_ranges(title_left..=lang.left() - 10.0, rect.y_range());
    if title_clip.width() > 20.0 {
        ui.painter_at(title_clip).text(
            pos2(title_left, y),
            Align2::LEFT_CENTER,
            &m.title,
            FontId::proportional(13.5),
            c.ink,
        );
    }
    let body = body.on_hover_text(format!(
        "{} · {} · {} — double-click or Enter to open; right-click for more",
        m.title, m.language, m.license
    ));
    RowResponse { body, handle }
}

/// The right-click menu of a Library row.
fn row_menu(ui: &mut Ui, id: &str) -> Option<Action> {
    let mut action = None;
    let items: [(&str, Action); 5] = [
        ("Open", Action::Open(id.to_string())),
        ("About", Action::About(id.to_string())),
        ("Move up", Action::Step(id.to_string(), true)),
        ("Move down", Action::Step(id.to_string(), false)),
        ("Remove…", Action::Remove(id.to_string())),
    ];
    for (i, (label, a)) in items.into_iter().enumerate() {
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

/// The row a dragged row would land on: the row of its type under `y`, else the nearest end.
fn drop_target<'a>(rows: &'a [Row], dragged: &str, y: f32) -> Option<&'a Row> {
    let ty = rows.iter().find(|r| r.id == dragged)?.ty;
    let same: Vec<&Row> = rows.iter().filter(|r| r.ty == ty).collect();
    same.iter()
        .find(|r| r.rect.y_range().contains(y))
        .or_else(|| {
            if y < same.first()?.rect.top() {
                same.first()
            } else {
                same.last()
            }
        })
        .copied()
}

fn section(ui: &mut Ui, title: &str) {
    ui.label(
        RichText::new(title.to_uppercase())
            .size(10.5)
            .strong()
            .color(theme::c().muted),
    );
}

fn type_singular(t: ResourceType) -> &'static str {
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
