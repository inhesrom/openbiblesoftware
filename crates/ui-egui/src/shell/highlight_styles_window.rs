//! View → Highlight styles (#181, ADR 0012): the user's own Highlight styles and palettes.
//!
//! - The palette row switches the active palette (the selection toolbar, right-click menu and
//!   Alt+1…9 follow it), makes, renames and deletes palettes, and exports or imports one as
//!   a small JSON file through the native file dialogs.
//! - Each style of the active palette is a row: its swatch and key, name, look, colour (a hex
//!   field or a preset), ⏶ ⏷ to reorder, and Delete. Deleting a style that Highlights use asks
//!   which style they move to; Ctrl+Z brings both back.
//! - The chosen style's colour in each Page tone, shown on that tone's page with its contrast:
//!   derived unless adjusted, and an adjustment that fails WCAG AA (#21) is not saved.
//!
//! Every change is an engine command, so each is one undo step.

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::{Receiver, TryRecvError};

use egui::{Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use obs_engine::{HighlightLook, HighlightStyle, Rgb, StyleAppearance, StyleTone};
use serde_json::{Value, json};

use super::State;
use super::annotate::{StyleBook, paint_swatch};
use crate::icons::{self, Icon};
use crate::palette::{Palette, to_color, to_rgb};
use crate::theme;

/// Colours offered beside the hex field: the built-ins' and a few strong ones for lines,
/// boxes and text.
const PRESETS: [(&str, Rgb); 12] = [
    ("Yellow", Rgb::new(255, 236, 140)),
    ("Green", Rgb::new(200, 238, 190)),
    ("Blue", Rgb::new(196, 222, 250)),
    ("Pink", Rgb::new(250, 206, 224)),
    ("Orange", Rgb::new(255, 214, 170)),
    ("Lavender", Rgb::new(222, 210, 250)),
    ("Gold", Rgb::new(201, 162, 39)),
    ("Red", Rgb::new(200, 40, 40)),
    ("Teal", Rgb::new(20, 130, 120)),
    ("Navy", Rgb::new(40, 70, 150)),
    ("Purple", Rgb::new(120, 60, 160)),
    ("Brown", Rgb::new(130, 80, 40)),
];

/// A file dialog's answer, on its own thread.
#[cfg(not(target_arch = "wasm32"))]
enum FileWait {
    Export(String, Receiver<Option<PathBuf>>),
    Import(Receiver<Option<PathBuf>>),
}

/// The Highlight styles window's state.
#[derive(Default)]
pub(crate) struct StylesUi {
    pub(super) open: bool,
    /// The style whose tone colours are shown.
    chosen: Option<String>,
    /// Text being typed into fields, by style id (committed on Enter or leaving the field).
    names: std::collections::HashMap<String, Draft>,
    colors: std::collections::HashMap<String, Draft>,
    tones: std::collections::HashMap<(String, StyleTone), Draft>,
    /// A palette name being typed: for a new palette (`None`) or a rename.
    palette_name: Option<(Option<String>, String)>,
    /// A deletion waiting for its replacement: the style (or palette) id, whether it is a
    /// palette, how many Highlights move, and the replacement chosen.
    deleting: Option<Deleting>,
    /// The last thing refused (a contrast failure, a bad colour), shown in the window.
    error: Option<String>,
    #[cfg(not(target_arch = "wasm32"))]
    file: Option<FileWait>,
}

/// A field's text and the stored value it showed when the user began typing. A draft still
/// equal to its start is clean: it follows the style as it changes (undo, redo, a new colour
/// clearing the tone adjustments), so leaving the field commits only what the user typed.
#[derive(Debug, Clone)]
struct Draft {
    start: String,
    text: String,
}

impl Draft {
    fn of(stored: &str) -> Self {
        Self {
            start: stored.to_string(),
            text: stored.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
struct Deleting {
    id: String,
    palette: bool,
    uses: u64,
    replacement: String,
}

/// What a row of the window asked for.
enum Act {
    Run(&'static str, Value),
    Delete {
        id: String,
        palette: bool,
    },
    /// Make a palette with this name and make it active.
    NewPalette(String),
    /// Ask where to export palette `id`, or which file to import.
    Export(String),
    Import,
    Error(String),
}

/// The text a hex field shows for a colour.
fn hex(c: Rgb) -> String {
    c.to_string()
}

/// A sample of a style on one tone's page: "Rom 8:28" marked as the style marks text there.
fn paint_sample(ui: &Ui, rect: Rect, a: &StyleAppearance, palette: &Palette) {
    let p = ui.painter();
    p.rect_filled(rect, 4.0, palette.page);
    p.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0, theme::c().rule),
        egui::StrokeKind::Inside,
    );
    let paint = palette.paint(a);
    let font = egui::FontId::proportional(14.0);
    let ink = if paint.look == HighlightLook::TextColor {
        paint.color
    } else {
        palette.ink
    };
    let galley = p.layout_no_wrap("Rom 8:28".into(), font, ink);
    let text = Rect::from_center_size(rect.center(), galley.size());
    let band = text.expand2(vec2(3.0, 1.0));
    match paint.look {
        HighlightLook::Background => {
            p.rect_filled(band, 1.5, paint.color);
        }
        HighlightLook::Box => {
            p.rect_stroke(
                band,
                2.0,
                Stroke::new(1.5, paint.color),
                egui::StrokeKind::Outside,
            );
        }
        HighlightLook::Underline => {
            let y = band.bottom() - 1.5;
            p.line_segment(
                [pos2(band.left(), y), pos2(band.right(), y)],
                Stroke::new(1.5, paint.color),
            );
        }
        HighlightLook::TextColor => {}
    }
    p.galley(text.min, galley, ink);
}

/// A text field over `stored`, the value saved now: shows the user's draft while it differs
/// from what it started as, else `stored`. Returns the text the user typed when they press
/// Enter or leave the field having changed it.
fn draft_field<K: std::hash::Hash + Eq + Clone>(
    ui: &mut Ui,
    drafts: &mut std::collections::HashMap<K, Draft>,
    key: &K,
    stored: &str,
    (width, name, hint): (f32, &str, &str),
) -> Option<String> {
    let draft = drafts
        .entry(key.clone())
        .or_insert_with(|| Draft::of(stored));
    if draft.text == draft.start && draft.start != stored {
        *draft = Draft::of(stored);
    }
    let r = ui
        .add(
            egui::TextEdit::singleline(&mut draft.text)
                .desired_width(width)
                .min_size(vec2(width, 0.0))
                .hint_text(hint),
        )
        .accessible_name(name);
    if !r.lost_focus() {
        return None;
    }
    let draft = drafts.remove(key)?;
    (draft.text != draft.start).then_some(draft.text)
}

impl State {
    /// The Highlight styles window, when open.
    pub(super) fn highlight_styles_window(&mut self, ctx: &egui::Context) {
        #[cfg(not(target_arch = "wasm32"))]
        self.poll_palette_file();
        if !self.styles_ui.open {
            return;
        }
        let book = self.marks.styles(&self.session).clone();
        let keys = super::annotate::SelectionKeyText::of(&self.session);
        let mut open = true;
        let escape = super::escape_closes_window(ctx);
        let mut acts: Vec<Act> = Vec::new();
        let max_height = ctx.content_rect().height() - 160.0;
        let window = egui::Window::new("Highlight styles")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(620.0)
            // Pinned at the top, so rows stay put while a question opens below them.
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 60.0))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(max_height)
                    .min_scrolled_height(max_height.clamp(120.0, 640.0))
                    .show(ui, |ui| {
                        self.palette_row(ui, &book, &mut acts);
                        ui.separator();
                        self.style_rows(ui, &book, &keys, &mut acts);
                        self.deleting_ui(ui, &book, &mut acts);
                        ui.separator();
                        self.tones_ui(ui, &book, &mut acts);
                        if let Some(e) = &self.styles_ui.error {
                            ui.label(RichText::new(e).color(theme::c().warn));
                        }
                    });
            });
        if !open || escape(window.map(|r| r.response.layer_id)) {
            self.styles_ui.open = false;
            self.styles_ui.deleting = None;
        }
        for act in acts {
            match act {
                Act::Run(cmd, params) => {
                    self.styles_ui.error = None;
                    if let Some(out) = self.run(cmd, &params)
                        && cmd == "highlight_style.create"
                    {
                        self.styles_ui.chosen = out["id"].as_str().map(String::from);
                    }
                }
                Act::Delete { id, palette } => self.ask_delete(&book, id, palette),
                Act::NewPalette(name) => {
                    if let Some(out) =
                        self.run("highlight_palette.create", &json!({ "name": name }))
                        && let Some(id) = out["id"].as_str()
                    {
                        self.run("highlight_palette.activate", &json!({ "id": id }));
                    }
                }
                Act::Export(id) => self.palette_file_dialog(ctx, Some(id)),
                Act::Import => self.palette_file_dialog(ctx, None),
                Act::Error(e) => self.styles_ui.error = Some(e),
            }
        }
    }

    /// Start deleting a style or palette: at once when no Highlight uses it, else ask where
    /// its Highlights go.
    fn ask_delete(&mut self, book: &StyleBook, id: String, palette: bool) {
        let ids: Vec<&str> = if palette {
            book.of_palette(&id).map(|s| s.id.as_str()).collect()
        } else {
            vec![id.as_str()]
        };
        let uses: u64 = ids.iter().filter_map(|s| book.uses.get(*s)).sum();
        let cmd = if palette {
            "highlight_palette.delete"
        } else {
            "highlight_style.delete"
        };
        if uses == 0 {
            self.run(cmd, &json!({ "id": id }));
            return;
        }
        let replacement = book
            .all
            .iter()
            .find(|s| !ids.contains(&s.id.as_str()))
            .map(|s| s.id.clone())
            .unwrap_or_default();
        self.styles_ui.deleting = Some(Deleting {
            id,
            palette,
            uses,
            replacement,
        });
    }

    fn palette_row(&mut self, ui: &mut Ui, book: &StyleBook, acts: &mut Vec<Act>) {
        let ui_state = &mut self.styles_ui;
        ui.horizontal_wrapped(|ui| {
            ui.label("Palette");
            let current = book
                .palettes
                .iter()
                .find(|p| p.id == book.active)
                .map_or_else(String::new, |p| p.name.clone());
            egui::ComboBox::from_id_salt("highlight-palette")
                .selected_text(&current)
                .show_ui(ui, |ui| {
                    for p in &book.palettes {
                        if ui.selectable_label(p.id == book.active, &p.name).clicked()
                            && p.id != book.active
                        {
                            acts.push(Act::Run(
                                "highlight_palette.activate",
                                json!({ "id": p.id }),
                            ));
                        }
                    }
                })
                .response
                .accessible_name("Palette");
            if ui.button("New palette…").clicked() {
                ui_state.palette_name = Some((None, String::new()));
            }
            if ui.button("Rename…").clicked() {
                ui_state.palette_name = Some((Some(book.active.clone()), current.clone()));
            }
            if ui
                .add_enabled(
                    book.palettes.len() > 1,
                    egui::Button::new("Delete palette…"),
                )
                .on_disabled_hover_text("The last palette cannot be deleted")
                .clicked()
            {
                acts.push(Act::Delete {
                    id: book.active.clone(),
                    palette: true,
                });
            }
            if ui.button("Export…").clicked() {
                acts.push(Act::Export(book.active.clone()));
            }
            if ui.button("Import…").clicked() {
                acts.push(Act::Import);
            }
        });
        if let Some((renaming, name)) = &mut ui_state.palette_name {
            let mut done = false;
            ui.horizontal(|ui| {
                ui.label(if renaming.is_some() {
                    "New name"
                } else {
                    "Name of the new palette"
                });
                let r = ui
                    .add(egui::TextEdit::singleline(name).desired_width(200.0))
                    .accessible_name("Palette name");
                r.request_focus();
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if enter || ui.button("OK").clicked() {
                    let act = match renaming {
                        Some(id) => Act::Run(
                            "highlight_palette.rename",
                            json!({ "id": id, "name": name }),
                        ),
                        None => Act::NewPalette(name.clone()),
                    };
                    acts.push(act);
                    done = true;
                }
                if ui.button("Cancel").clicked() {
                    done = true;
                }
            });
            if done {
                ui_state.palette_name = None;
            }
        }
    }

    fn style_rows(
        &mut self,
        ui: &mut Ui,
        book: &StyleBook,
        keys: &super::annotate::SelectionKeyText,
        acts: &mut Vec<Act>,
    ) {
        let palette = self.palette;
        let styles = book.active_styles();
        let n = styles.len();
        ui.label(
            RichText::new(
                "Alt+1…9 highlight with the first nine styles, in this order. A colour is \
                 adjusted for each page tone so text stays readable.",
            )
            .small()
            .color(theme::c().muted),
        );
        if styles.is_empty() {
            ui.label("This palette has no styles yet.");
        }
        let ui_state = &mut self.styles_ui;
        egui::Grid::new("highlight-styles")
            .num_columns(7)
            .spacing(vec2(8.0, 6.0))
            .show(ui, |ui| {
                for (i, style) in styles.iter().enumerate() {
                    let label = book.label(&style.id);
                    let chosen = ui_state.chosen.as_deref() == Some(style.id.as_str());
                    let (rect, r) = ui.allocate_exact_size(vec2(26.0, 22.0), Sense::click());
                    paint_swatch(ui, rect, &style.appearance, &palette, r.hovered() || chosen);
                    if r.accessible_name(format!("Show {label} in each page tone"))
                        .on_hover_text("Show its colour in each page tone")
                        .clicked()
                    {
                        ui_state.chosen = Some(style.id.clone());
                    }
                    ui.label(RichText::new(keys.slot(i)).small().color(theme::c().muted));
                    let field_name = format!("Name of {label}");
                    if let Some(name) = draft_field(
                        ui,
                        &mut ui_state.names,
                        &style.id,
                        &style.name,
                        (140.0, &field_name, &label),
                    ) && name != style.name
                    {
                        acts.push(Act::Run(
                            "highlight_style.edit",
                            json!({ "id": style.id, "name": name }),
                        ));
                    }
                    let mut look = style.appearance.look;
                    egui::ComboBox::from_id_salt(("highlight-look", &style.id))
                        .selected_text(look.label())
                        .width(110.0)
                        .show_ui(ui, |ui| {
                            for l in HighlightLook::ALL {
                                ui.selectable_value(&mut look, l, l.label());
                            }
                        })
                        .response
                        .accessible_name(format!("Look of {label}"));
                    if look != style.appearance.look {
                        acts.push(Act::Run(
                            "highlight_style.edit",
                            json!({ "id": style.id, "look": look.id() }),
                        ));
                    }
                    ui.horizontal(|ui| {
                        color_field(ui, ui_state, style, &label, acts);
                    });
                    ui.horizontal(|ui| {
                        let up = icons::image(ui.ctx(), Icon::MoveUp, icons::SMALL, theme::c().ink);
                        if ui
                            .add_enabled(i > 0, egui::Button::new(up).small())
                            .on_hover_text("Move up")
                            .accessible_name(format!("Move {label} up"))
                            .clicked()
                        {
                            acts.push(Act::Run(
                                "highlight_style.move",
                                json!({ "id": style.id, "position": i - 1 }),
                            ));
                        }
                        let down =
                            icons::image(ui.ctx(), Icon::MoveDown, icons::SMALL, theme::c().ink);
                        if ui
                            .add_enabled(i + 1 < n, egui::Button::new(down).small())
                            .on_hover_text("Move down")
                            .accessible_name(format!("Move {label} down"))
                            .clicked()
                        {
                            acts.push(Act::Run(
                                "highlight_style.move",
                                json!({ "id": style.id, "position": i + 1 }),
                            ));
                        }
                    });
                    let uses = book.uses.get(&style.id).copied().unwrap_or(0);
                    if ui
                        .button("Delete…")
                        .on_hover_text(format!("{uses} Highlight(s) use it"))
                        .accessible_name(format!("Delete {label}"))
                        .clicked()
                    {
                        acts.push(Act::Delete {
                            id: style.id.clone(),
                            palette: false,
                        });
                    }
                    ui.end_row();
                }
            });
        if ui.button("+ New style").clicked() {
            let color = PRESETS[n % PRESETS.len()].1;
            acts.push(Act::Run(
                "highlight_style.create",
                json!({ "palette": book.active, "color": hex(color) }),
            ));
        }
    }

    /// The question a deletion of a used style or palette asks: where its Highlights go.
    fn deleting_ui(&mut self, ui: &mut Ui, book: &StyleBook, acts: &mut Vec<Act>) {
        let Some(d) = self.styles_ui.deleting.as_mut() else {
            return;
        };
        let leaving: Vec<&str> = if d.palette {
            book.of_palette(&d.id).map(|s| s.id.as_str()).collect()
        } else {
            vec![d.id.as_str()]
        };
        let what = if d.palette {
            book.palettes
                .iter()
                .find(|p| p.id == d.id)
                .map_or_else(String::new, |p| format!("the palette “{}”", p.name))
        } else {
            format!("“{}”", book.label(&d.id))
        };
        let mut close = false;
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.label(format!("{} Highlight(s) use {what}. Move them to:", d.uses));
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("highlight-replacement")
                    .selected_text(book.label(&d.replacement))
                    .show_ui(ui, |ui| {
                        for p in &book.palettes {
                            for s in book.of_palette(&p.id) {
                                if leaving.contains(&s.id.as_str()) {
                                    continue;
                                }
                                ui.selectable_value(
                                    &mut d.replacement,
                                    s.id.clone(),
                                    format!("{} ({})", book.label(&s.id), p.name),
                                );
                            }
                        }
                    })
                    .response
                    .accessible_name("Move the Highlights to");
                if ui
                    .add_enabled(
                        !d.replacement.is_empty(),
                        egui::Button::new("Delete and move"),
                    )
                    .clicked()
                {
                    let cmd = if d.palette {
                        "highlight_palette.delete"
                    } else {
                        "highlight_style.delete"
                    };
                    acts.push(Act::Run(
                        cmd,
                        json!({ "id": d.id, "replacement": d.replacement }),
                    ));
                    close = true;
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
            ui.label(
                RichText::new(format!(
                    "{}.",
                    super::undo_hint(&self.session, "brings back the style and its Highlights")
                ))
                .small()
                .color(theme::c().muted),
            );
        });
        if close {
            self.styles_ui.deleting = None;
        }
    }

    /// The chosen style in each Page tone: a sample on the tone's page, whether its colour
    /// there is derived or adjusted, its contrast, and a field to adjust it.
    fn tones_ui(&mut self, ui: &mut Ui, book: &StyleBook, acts: &mut Vec<Act>) {
        let active = book.active_styles();
        let Some(style) = (self.styles_ui.chosen.as_deref())
            .and_then(|id| active.iter().find(|s| s.id == id))
            .or(active.first())
        else {
            return;
        };
        let label = book.label(&style.id);
        ui.label(RichText::new(format!("{label} in each page tone")).strong());
        let ui_state = &mut self.styles_ui;
        egui::Grid::new("highlight-tones")
            .num_columns(4)
            .spacing(vec2(8.0, 6.0))
            .show(ui, |ui| {
                for tone in StyleTone::ALL {
                    let palette = Palette::for_style_tone(tone);
                    let paint = palette.paint(&style.appearance);
                    ui.label(tone.label());
                    let (rect, _) = ui.allocate_exact_size(vec2(110.0, 26.0), Sense::hover());
                    paint_sample(ui, rect, &style.appearance, &palette);
                    let adjusted = tone.adjusted(&style.appearance);
                    let state = match adjusted {
                        Some(c) if palette.check(style.appearance.look, to_color(c)).is_ok() => {
                            "adjusted"
                        }
                        Some(_) => "adjusted colour too faint: derived",
                        None => "derived",
                    };
                    let ratio = contrast_text(&palette, style.appearance.look, paint.color);
                    ui.label(
                        RichText::new(format!("{} · {state} · {ratio}", hex(to_rgb(paint.color))))
                            .small(),
                    );
                    ui.horizontal(|ui| {
                        let key = (style.id.clone(), tone);
                        let stored = adjusted.map(hex).unwrap_or_default();
                        let name = format!("{} colour of {label}", tone.label());
                        let field = (80.0, name.as_str(), "#rrggbb");
                        if let Some(typed) =
                            draft_field(ui, &mut ui_state.tones, &key, &stored, field)
                        {
                            let typed = typed.trim().to_string();
                            if typed.is_empty() {
                                if adjusted.is_some() {
                                    acts.push(Act::Run(
                                        "highlight_style.edit",
                                        json!({ "id": style.id, tone.id(): "" }),
                                    ));
                                }
                            } else {
                                match adjust(&palette, style, &typed) {
                                    Ok(c) if Some(c) != adjusted => acts.push(Act::Run(
                                        "highlight_style.edit",
                                        json!({ "id": style.id, tone.id(): hex(c) }),
                                    )),
                                    Ok(_) => {}
                                    Err(e) => acts.push(Act::Error(format!(
                                        "{label} in {}: {e}",
                                        tone.label()
                                    ))),
                                }
                            }
                        }
                        if ui
                            .add_enabled(adjusted.is_some(), egui::Button::new("Derive"))
                            .on_hover_text("Go back to the colour derived for this tone")
                            .accessible_name(format!(
                                "Derive the {} colour of {label}",
                                tone.label()
                            ))
                            .clicked()
                        {
                            ui_state.tones.remove(&key);
                            acts.push(Act::Run(
                                "highlight_style.edit",
                                json!({ "id": style.id, tone.id(): "" }),
                            ));
                        }
                    });
                    ui.end_row();
                }
            });
    }

    /// Ask where to save the active palette, or which file to import, on the dialog's own
    /// thread; [`State::poll_palette_file`] picks up the answer.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn palette_file_dialog(&mut self, ctx: &egui::Context, export: Option<String>) {
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        let name = export.as_ref().map(|id| {
            let name = self
                .marks
                .styles
                .palettes
                .iter()
                .find(|p| p.id == *id)
                .map_or("palette", |p| p.name.as_str())
                .chars()
                .map(|c| if c.is_alphanumeric() { c } else { '-' })
                .collect::<String>();
            format!("{name}.highlights.json")
        });
        let spawned = std::thread::Builder::new()
            .name("obs-palette-file".into())
            .spawn(move || {
                let dialog = rfd::FileDialog::new().add_filter("Highlight palette", &["json"]);
                let chosen = match name {
                    Some(name) => dialog
                        .set_title("Export Highlight palette")
                        .set_file_name(name)
                        .save_file(),
                    None => dialog.set_title("Import Highlight palette").pick_file(),
                };
                let _ = tx.send(chosen);
                ctx.request_repaint();
            });
        match (spawned, export) {
            (Ok(_), Some(id)) => self.styles_ui.file = Some(FileWait::Export(id, rx)),
            (Ok(_), None) => self.styles_ui.file = Some(FileWait::Import(rx)),
            (Err(e), _) => self.toast(format!("Could not open the file dialog: {e}")),
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn palette_file_dialog(&mut self, _: &egui::Context, _: Option<String>) {
        self.toast("Exporting and importing palettes needs the desktop app");
    }

    /// Export or import once the file dialog has answered; never wait for it.
    #[cfg(not(target_arch = "wasm32"))]
    fn poll_palette_file(&mut self) {
        let answer = match &self.styles_ui.file {
            Some(FileWait::Export(_, rx) | FileWait::Import(rx)) => rx.try_recv(),
            None => return,
        };
        let path = match answer {
            Ok(path) => path,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => None,
        };
        let Some(wait) = self.styles_ui.file.take() else {
            return;
        };
        let Some(path) = path else {
            return;
        };
        self.palette_file_chosen(wait_export(&wait), &path);
    }

    /// Export palette `export` to `path`, or import the palette at `path`.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(super) fn palette_file_chosen(&mut self, export: Option<String>, path: &std::path::Path) {
        let path_text = path.to_string_lossy();
        match export {
            Some(id) => {
                if self
                    .run(
                        "highlight_palette.export",
                        &json!({ "id": id, "path": path_text }),
                    )
                    .is_some()
                {
                    self.toast(format!("Exported the palette to {}", path.display()));
                }
            }
            None => {
                if let Some(out) =
                    self.run("highlight_palette.import", &json!({ "path": path_text }))
                {
                    let name = out["palette"]["name"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    if let Some(id) = out["id"].as_str() {
                        self.run("highlight_palette.activate", &json!({ "id": id }));
                    }
                    self.toast(format!("Imported the palette “{name}”"));
                }
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn wait_export(w: &FileWait) -> Option<String> {
    match w {
        FileWait::Export(id, _) => Some(id.clone()),
        FileWait::Import(_) => None,
    }
}

/// A style's colour as typed for a tone, checked against that tone's AA rules.
fn adjust(palette: &Palette, style: &HighlightStyle, typed: &str) -> Result<Rgb, String> {
    let c: Rgb = typed.parse()?;
    palette
        .check(style.appearance.look, to_color(c))
        .map_err(|short| format!("does not meet WCAG AA here ({short}); not saved"))?;
    Ok(c)
}

/// "7.2:1" for the contrast that decides whether a colour passes in a tone, with ✔ when it
/// does (it always does once drawn: a failing one is derived instead).
fn contrast_text(palette: &Palette, look: HighlightLook, color: egui::Color32) -> String {
    use crate::palette::contrast;
    let ratio = match look {
        HighlightLook::Background => palette
            .inks()
            .iter()
            .map(|(_, ink)| contrast(*ink, color))
            .fold(f32::MAX, f32::min),
        HighlightLook::TextColor => palette
            .overlays()
            .iter()
            .map(|(_, bg)| contrast(color, *bg))
            .fold(f32::MAX, f32::min),
        HighlightLook::Underline | HighlightLook::Box => contrast(color, palette.page),
    };
    let ok = if palette.check(look, color).is_ok() {
        "meets AA"
    } else {
        "below AA"
    };
    format!("{ratio:.1}:1 {ok}")
}

/// A style's colour: its hex field (Enter or leaving it applies) and a menu of presets.
fn color_field(
    ui: &mut Ui,
    ui_state: &mut StylesUi,
    style: &HighlightStyle,
    label: &str,
    acts: &mut Vec<Act>,
) {
    let name = format!("Colour of {label}");
    let stored = hex(style.appearance.color);
    if let Some(typed) = draft_field(
        ui,
        &mut ui_state.colors,
        &style.id,
        &stored,
        (80.0, &name, "#rrggbb"),
    ) {
        match typed.trim().parse::<Rgb>() {
            Ok(c) if c != style.appearance.color => acts.push(Act::Run(
                "highlight_style.edit",
                json!({ "id": style.id, "color": hex(c) }),
            )),
            Ok(_) => {}
            Err(e) => acts.push(Act::Error(e)),
        }
    }
    let open = icons::image(ui.ctx(), Icon::ChevronDown, icons::SMALL, theme::c().muted);
    ui.menu_button(open, |ui| {
        for (name, c) in PRESETS {
            let r = ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                ui.painter().rect_filled(rect, 2.0, to_color(c));
                ui.button(name)
            });
            if r.inner.clicked() {
                ui_state.colors.remove(&style.id);
                acts.push(Act::Run(
                    "highlight_style.edit",
                    json!({ "id": style.id, "color": hex(c) }),
                ));
                ui.close();
            }
        }
    })
    .response
    .on_hover_text("Preset colours")
    .accessible_name(format!("Preset colours for {label}"));
}
