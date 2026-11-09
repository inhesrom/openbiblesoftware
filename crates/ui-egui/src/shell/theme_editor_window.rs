//! View → Theme editor, and Settings → Theme and Page tone (#224): the user's Custom theme.
//!
//! - **Start from** a built-in theme (Light, Dark, High contrast light or dark): the Custom
//!   theme becomes that theme's colours, with none changed (asked first when some are).
//! - Each colour role is a row: a colour button with egui's picker, a hex field, and Reset
//!   when it differs from the base. Every change runs `theme.set_color`, which makes the
//!   Custom theme the app's Theme, so the whole window is the live preview; the preview card
//!   shows it too, whatever the Theme. A picker being dragged saves when let go.
//! - **Contrast**: [`theme::contrast_shorts`] of the theme as it is now, each pair that does
//!   not read named with its ratio and what it needs (AAA when the base is High contrast).
//! - **Export…** and **Import…** write and read the theme as a small JSON file through the
//!   native file dialogs (`theme.export`, `theme.import`).

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::{Receiver, TryRecvError};

use egui::{Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use obs_engine::{CustomTheme, Rgb, Theme, ThemeRole};
use serde_json::{Value, json};

use super::State;
use crate::palette::to_rgb;
use crate::theme::{self, Chrome};
use crate::tokens::{radius, size, space, stroke, text};

/// A file dialog's answer, on its own thread.
#[cfg(not(target_arch = "wasm32"))]
enum FileWait {
    Export(Receiver<Option<PathBuf>>),
    Import(Receiver<Option<PathBuf>>),
}

/// The Theme editor's state.
#[derive(Default)]
pub(crate) struct ThemeEditorUi {
    pub(crate) open: bool,
    /// Hex fields being typed, by role (committed on Enter or leaving the field; Escape
    /// drops it).
    hex: std::collections::HashMap<ThemeRole, String>,
    /// The role whose colour picker is being dragged: shown at once, saved when let go.
    dragging: Option<ThemeRole>,
    /// The built-in theme Start from was clicked for while colours are changed, until the
    /// user says to replace them or keep them.
    confirm_start: Option<Theme>,
    #[cfg(not(target_arch = "wasm32"))]
    file: Option<FileWait>,
}

/// What the window asked for this frame.
enum Act {
    Run(&'static str, Value),
    Export,
    Import,
}

impl State {
    /// The Theme editor, when open.
    pub(super) fn theme_editor_window(&mut self, ctx: &egui::Context) {
        #[cfg(not(target_arch = "wasm32"))]
        self.poll_theme_file();
        self.save_dragged_color(ctx);
        if !self.theme_editor.open {
            return;
        }
        let custom = self.session.custom_theme.clone();
        let in_use = self.session.appearance.theme == Theme::Custom;
        let mut open = true;
        let escape = super::escape_closes_window(ctx);
        let mut acts: Vec<Act> = Vec::new();
        let editor = &mut self.theme_editor;
        let max_height = ctx.content_rect().height() - size::DIALOG_ROOM;
        let window = egui::Window::new("Theme editor")
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
                        editor_ui(ui, &custom, in_use, editor, &mut acts);
                    });
            });
        if !open || escape(window.map(|r| r.response.layer_id)) {
            self.theme_editor.open = false;
            self.theme_editor.hex.clear();
            self.theme_editor.confirm_start = None;
        }
        for act in acts {
            match act {
                Act::Run(cmd, params) => {
                    self.run(cmd, &params);
                }
                Act::Export => self.theme_file_dialog(ctx, true),
                Act::Import => self.theme_file_dialog(ctx, false),
            }
        }
    }

    /// Save the colour a picker was dragged to once the pointer is let go (dragging changes
    /// it in memory only, so the User database is not written every frame).
    fn save_dragged_color(&mut self, ctx: &egui::Context) {
        let Some(role) = self.theme_editor.dragging else {
            return;
        };
        if ctx.input(|i| i.pointer.any_down()) {
            return;
        }
        self.theme_editor.dragging = None;
        let color = self
            .session
            .custom_theme
            .colors
            .get(&role)
            .map(Rgb::to_string);
        self.run(
            "theme.set_color",
            &json!({ "role": role.id(), "color": color }),
        );
    }

    /// Ask where to export the Custom theme, or which file to import, on the dialog's own
    /// thread; [`State::poll_theme_file`] picks up the answer.
    #[cfg(not(target_arch = "wasm32"))]
    fn theme_file_dialog(&mut self, ctx: &egui::Context, export: bool) {
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("obs-theme-file".into())
            .spawn(move || {
                let dialog = rfd::FileDialog::new().add_filter("Theme", &["json"]);
                let chosen = if export {
                    dialog
                        .set_title("Export the Custom theme")
                        .set_file_name("custom.theme.json")
                        .save_file()
                } else {
                    dialog.set_title("Import a theme").pick_file()
                };
                let _ = tx.send(chosen);
                ctx.request_repaint();
            });
        match spawned {
            Ok(_) if export => self.theme_editor.file = Some(FileWait::Export(rx)),
            Ok(_) => self.theme_editor.file = Some(FileWait::Import(rx)),
            Err(e) => self.toast(format!("Could not open the file dialog: {e}")),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn theme_file_dialog(&mut self, _: &egui::Context, _: bool) {
        self.toast("Exporting and importing themes needs the desktop app");
    }

    /// Export or import once the file dialog has answered; never wait for it.
    #[cfg(not(target_arch = "wasm32"))]
    fn poll_theme_file(&mut self) {
        let answer = match &self.theme_editor.file {
            Some(FileWait::Export(rx) | FileWait::Import(rx)) => rx.try_recv(),
            None => return,
        };
        let path = match answer {
            Ok(path) => path,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => None,
        };
        let Some(wait) = self.theme_editor.file.take() else {
            return;
        };
        if let Some(path) = path {
            self.theme_file_chosen(matches!(wait, FileWait::Export(_)), &path);
        }
    }

    /// Export the Custom theme to `path`, or import the theme at `path` and use it.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(super) fn theme_file_chosen(&mut self, export: bool, path: &std::path::Path) {
        let path_text = path.to_string_lossy();
        if export {
            if self
                .run("theme.export", &json!({ "path": path_text }))
                .is_some()
            {
                self.toast(format!("Exported the Custom theme to {}", path.display()));
            }
        } else if self
            .run("theme.import", &json!({ "path": path_text }))
            .is_some()
        {
            self.theme_editor.hex.clear();
            self.toast(format!("Imported the theme in {}", path.display()));
        }
    }
}

fn editor_ui(
    ui: &mut Ui,
    custom: &CustomTheme,
    in_use: bool,
    editor: &mut ThemeEditorUi,
    acts: &mut Vec<Act>,
) {
    let ch = theme::custom_chrome(custom);
    ui.label(crate::components::hint_text(
        "Start from a built-in theme and change its colours. Each change makes this your \
         Theme at once, so the whole window shows it; warnings name colours that do not read.",
    ));
    ui.add_space(space::S);
    ui.horizontal_wrapped(|ui| {
        ui.label("Start from");
        for t in Theme::BUILT_IN {
            let r = ui
                .add(egui::Button::selectable(custom.base == t, t.label()))
                .accessible_name(format!("Start from {}", t.label()))
                .on_hover_text("The Custom theme becomes this theme's colours, none changed");
            if r.clicked() {
                // Changed colours are kept unless the user says to replace them; the base
                // already chosen with none changed is left as it is.
                if !custom.colors.is_empty() {
                    editor.confirm_start = Some(t);
                } else if custom.base != t {
                    acts.push(Act::Run("theme.start_from", json!({ "base": t.id() })));
                }
            }
        }
    });
    if let Some(t) = editor.confirm_start {
        ui.horizontal_wrapped(|ui| {
            let n = custom.colors.len();
            let colours = if n == 1 { "colour" } else { "colours" };
            ui.label(format!(
                "Start from {}? Your {n} changed {colours} will be lost.",
                t.label()
            ));
            if ui.button("Replace").clicked() {
                editor.hex.clear();
                editor.confirm_start = None;
                acts.push(Act::Run("theme.start_from", json!({ "base": t.id() })));
            }
            if ui.button("Keep my colours").clicked() {
                editor.confirm_start = None;
            }
        });
    }
    if !in_use {
        ui.horizontal_wrapped(|ui| {
            ui.label(crate::components::hint_text(
                "Your Theme is not the Custom theme now.",
            ));
            if ui.button("Use the Custom theme").clicked() {
                acts.push(Act::Run("view.theme", json!({ "value": "custom" })));
            }
        });
    }
    ui.add_space(space::S);
    preview(ui, &ch);
    ui.add_space(space::S);
    contrast_ui(ui, &ch);
    ui.add_space(space::S);
    egui::Grid::new("theme-roles")
        .num_columns(4)
        .spacing(vec2(space::M, space::S))
        .show(ui, |ui| {
            for (role, id, label) in ThemeRole::ALL {
                let now = theme::role_color(&ch, role);
                ui.label(label);
                let mut rgb = [now.r(), now.g(), now.b()];
                let r = ui
                    .color_edit_button_srgb(&mut rgb)
                    .accessible_name(format!("{label} colour"));
                if r.changed() {
                    editor.hex.remove(&role);
                    let [r, g, b] = rgb;
                    let c = Rgb::new(r, g, b);
                    // While the picker is dragged the colour shows at once and is saved
                    // when let go.
                    let held = ui.input(|i| i.pointer.any_down());
                    if held {
                        editor.dragging = Some(role);
                    }
                    acts.push(Act::Run(
                        "theme.set_color",
                        json!({ "role": id, "color": c.to_string(), "save": !held }),
                    ));
                }
                let stored = to_rgb(now).to_string();
                let text = editor.hex.entry(role).or_insert_with(|| stored.clone());
                let field = ui
                    .add(
                        egui::TextEdit::singleline(text)
                            .desired_width(size::FIELD_HEX)
                            .min_size(vec2(size::FIELD_HEX, 0.0))
                            .font(egui::TextStyle::Monospace),
                    )
                    .accessible_name(format!("{label} hex colour"));
                if field.lost_focus() {
                    let typed = editor.hex.remove(&role).unwrap_or_default();
                    // Escape cancels what was typed (and closes the window) rather than
                    // applying it.
                    let cancelled = ui.input(|i| i.key_pressed(egui::Key::Escape));
                    if !cancelled && typed.trim() != stored {
                        acts.push(Act::Run(
                            "theme.set_color",
                            json!({ "role": id, "color": typed.trim() }),
                        ));
                    }
                } else if !field.has_focus() && *text != stored {
                    *text = stored;
                }
                let changed = custom.colors.contains_key(&role);
                if crate::components::link_enabled(ui, changed, "Reset")
                    .accessible_name(format!("Reset {label}"))
                    .on_hover_text(format!("Back to {}'s colour", custom.base.label()))
                    .on_disabled_hover_text("The base theme's colour")
                    .clicked()
                {
                    editor.hex.remove(&role);
                    acts.push(Act::Run("theme.set_color", json!({ "role": id })));
                }
                ui.end_row();
            }
        });
    ui.add_space(space::S);
    ui.horizontal(|ui| {
        if ui
            .button("Export…")
            .on_hover_text("Save the Custom theme as a JSON file")
            .clicked()
        {
            acts.push(Act::Export);
        }
        if ui
            .button("Import…")
            .on_hover_text("Use a theme from a JSON file as your Custom theme")
            .clicked()
        {
            acts.push(Act::Import);
        }
    });
}

/// The contrast warnings of the theme as it is now, or that every pair reads.
fn contrast_ui(ui: &mut Ui, ch: &Chrome) {
    let shorts = theme::contrast_shorts(ch);
    let level = if ch.high_contrast { "AAA" } else { "AA" };
    if shorts.is_empty() {
        ui.label(crate::components::hint_text(format!(
            "Contrast: every text colour reads at WCAG {level} on every background."
        )));
        return;
    }
    ui.label(
        RichText::new(format!(
            "Contrast warnings ({}): these do not reach WCAG {level}",
            shorts.len()
        ))
        .strong()
        .color(theme::c().warn),
    );
    for s in shorts {
        ui.label(RichText::new(format!("Low contrast: {s}")).color(theme::c().ink));
    }
}

/// A picture of the theme: a panel with text, secondary text, a link and a warning, a field, a selected and a hovered row, the Link set chips and a focused button. Its text is
/// a sample of the theme's colours, which may be any; the contrast list says what reads.
fn preview(ui: &mut Ui, ch: &Chrome) {
    let width = ui.available_width().min(size::DIALOG.x);
    let (rect, r) = ui.allocate_exact_size(vec2(width, size::THEME_PREVIEW), Sense::hover());
    r.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Image, true, "Preview of the Custom theme")
    });
    let p = ui.painter_at(rect);
    let font = |size: f32| egui::FontId::proportional(size);
    p.rect_filled(rect, radius::M, ch.shell);
    p.rect_stroke(
        rect,
        radius::M,
        Stroke::new(stroke::HAIR, ch.rule),
        egui::StrokeKind::Inside,
    );
    let inner = rect.shrink(space::M);
    let panel = Rect::from_min_size(inner.min, vec2(inner.width() / 2.0, inner.height()));
    p.rect_filled(panel, radius::S, ch.surface);
    p.rect_stroke(
        panel,
        radius::S,
        Stroke::new(stroke::HAIR, ch.rule),
        egui::StrokeKind::Inside,
    );
    let mut y = panel.top() + space::S;
    let x = panel.left() + space::M;
    let mut line = |text: &str, color: egui::Color32, size: f32, underline: bool| {
        let g = p.layout_no_wrap(text.into(), font(size), color);
        let h = g.size().y;
        let w = g.size().x;
        p.galley(pos2(x, y), g, color);
        if underline {
            p.hline(x..=x + w, y + h, Stroke::new(stroke::HAIR, color));
        }
        y += h + space::XXS;
    };
    line("Panel heading", ch.ink, text::SUBTITLE, false);
    line("Text in a panel", ch.ink, text::BODY, false);
    line("Secondary text", ch.muted, text::SMALL, false);
    line("A link", ch.lapis, text::BODY, true);
    line("A warning", ch.warn, text::SMALL, false);
    let right = Rect::from_min_max(pos2(panel.right() + space::M, inner.top()), inner.max);
    let row = |top: f32| {
        Rect::from_min_size(
            pos2(right.left(), top),
            vec2(right.width(), size::ROW_COMPACT),
        )
    };
    let field = row(right.top());
    p.rect_filled(field, radius::S, ch.field);
    p.rect_stroke(
        field,
        radius::S,
        Stroke::new(stroke::HAIR, ch.rule),
        egui::StrokeKind::Inside,
    );
    let label = |r: Rect, text: &str, color: egui::Color32| {
        p.text(
            pos2(r.left() + space::M, r.center().y),
            egui::Align2::LEFT_CENTER,
            text,
            font(text::BODY),
            color,
        );
    };
    label(field, "Text in a field", ch.ink);
    let selected = row(field.bottom() + space::XS);
    p.rect_filled(selected, radius::S, ch.selection);
    if ch.high_contrast {
        p.rect_stroke(
            selected,
            radius::S,
            Stroke::new(stroke::MEDIUM, ch.rule),
            egui::StrokeKind::Inside,
        );
    }
    label(selected, "A selected row", ch.ink);
    let hovered = row(selected.bottom() + space::XS);
    p.rect_filled(hovered, radius::S, ch.hover);
    p.rect_stroke(
        hovered,
        radius::S,
        Stroke::new(stroke::HAIR, ch.hover_rule),
        egui::StrokeKind::Inside,
    );
    label(hovered, "A hovered row", ch.ink);
    let mut chip_x = right.left();
    let chip_y = hovered.bottom() + space::S;
    for (letter, fill) in [
        ("A", ch.lapis),
        ("B", ch.verdigris),
        ("C", ch.violet),
        ("D", ch.gilt),
        ("–", ch.unlinked),
    ] {
        let chip = Rect::from_min_size(pos2(chip_x, chip_y), vec2(size::CHIP, size::CHIP));
        p.rect_filled(chip, radius::PILL, fill);
        p.text(
            chip.center(),
            egui::Align2::CENTER_CENTER,
            letter,
            font(text::LABEL),
            theme::text_on_in(ch, fill),
        );
        chip_x = chip.right() + space::XS;
    }
    let button = Rect::from_min_size(
        pos2(chip_x + space::M, chip_y),
        vec2(size::FIELD_NUMBER * 2.0, size::CHIP),
    );
    let ring = if ch.high_contrast {
        stroke::HEAVY
    } else {
        stroke::THICK
    };
    p.rect_stroke(
        button,
        radius::M,
        Stroke::new(ring, ch.lapis),
        egui::StrokeKind::Outside,
    );
    label(button, "Focused", ch.ink);
}
