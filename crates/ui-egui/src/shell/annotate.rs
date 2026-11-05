//! Making and showing Highlights and Notes in the origin Bible (#62, #18 rule 8, #17 rules
//! 15 and 17). The engine stores them and keeps undo (#61); this is the UI around a reading
//! pane's selection:
//!
//! - releasing a selection shows a floating **selection toolbar** (5 style swatches, Note,
//!   Copy, Search this); right-click offers the same actions; Ctrl+1…5 and Ctrl+Shift+N are
//!   their keys, and Ctrl+Z / Ctrl+Shift+Z undo and redo. None of these keys fire while a
//!   text field has focus: there they belong to the field.
//! - Note opens a **popup editor** anchored at the selection with a Notebook picker; it saves
//!   when focus leaves it (a click elsewhere, Esc) or on Ctrl+Enter. New Annotations go into
//!   the Notebook last used, which the engine remembers in the User database.
//! - Highlights and Note markers are drawn by the reading pane from [`Mark`]s, reloaded
//!   whenever the engine's annotation revision changes; hovering a marker previews its Note
//!   and clicking it opens the Note in the Notes Tool (#64). Notebooks hidden with their
//!   Show-in-text toggle are not drawn.

use std::collections::BTreeMap;
use std::sync::Arc;

use egui::{Key, Modifiers, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use obs_engine::{Annotation, HighlightStyle, Session, TabId};
use serde_json::{Value, json};

use super::{State, bindings, consume_key, extra_held};
use crate::doc::ReadingDoc;
use crate::marks::{Mark, marks_for};
use crate::palette::{HIGHLIGHT_STYLES, Palette};
use crate::pane::{PaneOutput, has_text};
use crate::selection::{Selection, copy_text, selected_text};
use crate::theme;

/// The keys of the 5 Highlight styles, in [`HighlightStyle::ALL`] order.
const STYLE_KEYS: [Key; 5] = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5];

/// Ctrl+Shift+N, the Note key.
const NOTE_KEY: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::N);

/// The annotation keys: Ctrl+Shift+N and Ctrl+1…5.
pub(super) fn shortcuts() -> impl Iterator<Item = egui::KeyboardShortcut> {
    let styles = STYLE_KEYS.map(|k| egui::KeyboardShortcut::new(Modifiers::COMMAND, k));
    std::iter::once(NOTE_KEY).chain(styles)
}

/// Shortcuts that text fields use themselves (undo, redo, leaving the field, moving by word on
/// macOS). While a text field has focus, the commands bound to them do not run.
const TEXT_FIELD_KEYS: [&str; 5] = ["Ctrl+Z", "Ctrl+Shift+Z", "Escape", "Alt+Left", "Alt+Right"];

/// The Note editor's text field.
fn editor_body_id() -> egui::Id {
    egui::Id::unique("note-editor-body")
}

/// Whether a command shortcut is left to a focused text field.
pub(super) fn yields_to_text_fields(shortcut: &str) -> bool {
    TEXT_FIELD_KEYS.contains(&shortcut)
}

/// Something done to a reading pane's selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Annotate {
    Highlight(HighlightStyle),
    Note,
    Copy,
}

/// Annotations shown in text as loaded from the User database, and each Resource's [`Mark`]s,
/// reloaded when the engine's annotation revision moves (any new, edited, undone or redone
/// Annotation, a Notebook's Show in text, View → Show Highlights).
#[derive(Default)]
pub(super) struct MarkCache {
    revision: Option<u64>,
    annotations: Vec<Annotation>,
    /// Notebook names by id (for Note previews).
    notebooks: BTreeMap<String, String>,
    by_resource: BTreeMap<String, Arc<Vec<Mark>>>,
}

impl MarkCache {
    fn refresh(&mut self, session: &Session) {
        let revision = session.annotations_revision();
        if self.revision == Some(revision) {
            return;
        }
        self.revision = Some(revision);
        // Only what shows in text: Notebooks with Show in text on, Highlights unless View →
        // Show Highlights is off (#63).
        self.annotations = if session.user.is_some() {
            session.annotations_in_text().unwrap_or_default()
        } else {
            Vec::new()
        };
        self.notebooks = if session.user.is_some() {
            session.notebooks().unwrap_or_default()
        } else {
            Vec::new()
        }
        .into_iter()
        .map(|n| (n.id, n.name))
        .collect();
        self.by_resource.clear();
    }

    /// Forget a Resource's Marks (it was removed; a re-import may change its text units).
    pub(super) fn forget(&mut self, resource_id: &str) {
        self.by_resource.remove(resource_id);
    }

    /// The Marks to draw in `doc`. None while its Resource's Annotations are being
    /// re-anchored (#65): they may point at text that moved; they show once the job is
    /// applied (which moves the annotation revision).
    pub(super) fn marks(&mut self, session: &Session, doc: &ReadingDoc) -> Arc<Vec<Mark>> {
        if session.is_reanchoring(&doc.manifest.resource_id) {
            return Arc::new(Vec::new());
        }
        self.refresh(session);
        let annotations = &self.annotations;
        Arc::clone(
            self.by_resource
                .entry(doc.manifest.resource_id.clone())
                .or_insert_with(|| Arc::new(marks_for(doc, annotations))),
        )
    }

    fn annotation(&self, id: &str) -> Option<&Annotation> {
        self.annotations.iter().find(|a| a.id == id)
    }
}

/// The popup Note editor, anchored at the selection it was opened on.
pub(super) struct NoteEditor {
    pub(super) tab: TabId,
    /// The selection, as `annotation.note` params (resource, start_unit, start, end_unit, end).
    target: Value,
    /// "Genesis 30:3 (TB)": where the Note goes.
    label: String,
    quote: String,
    pub(super) body: String,
    /// The Notebook chosen in the picker (it becomes the last used one when the Note saves).
    pub(super) notebook: String,
    notebooks: Vec<(String, String)>,
    /// The selection's rect on screen, followed while it is in view.
    pub(super) at: Rect,
    /// The editor's own rect last frame (a press outside it is a blur).
    rect: Option<Rect>,
    focused_once: bool,
    error: Option<String>,
}

/// The selection as command params: resource, start_unit, start, end_unit, end.
pub(super) fn selection_params(doc: &ReadingDoc, sel: &Selection) -> Option<Value> {
    let (a, b) = sel.range(doc);
    if a >= b {
        return None;
    }
    Some(json!({
        "resource": doc.manifest.resource_id,
        "start_unit": doc.units.get(a.unit)?.id.as_str(),
        "start": a.byte,
        "end_unit": doc.units.get(b.unit)?.id.as_str(),
        "end": b.byte,
    }))
}

fn style_name(style: HighlightStyle) -> &'static str {
    HighlightStyle::ALL
        .iter()
        .position(|s| *s == style)
        .map_or("", |i| HIGHLIGHT_STYLES[i])
}

fn style_shortcut(style: HighlightStyle) -> String {
    let n = HighlightStyle::ALL
        .iter()
        .position(|s| *s == style)
        .map_or(0, |i| i + 1);
    format!("Ctrl+{n}")
}

/// Paint one style's swatch into `rect`: its background, or the red underline on the page.
pub(super) fn paint_swatch(
    ui: &Ui,
    rect: Rect,
    style: HighlightStyle,
    palette: &Palette,
    hovered: bool,
) {
    let p = ui.painter();
    let fill = palette.highlight(style).unwrap_or(palette.page);
    p.rect_filled(rect, 3.0, fill);
    if palette.highlight(style).is_none() {
        let y = rect.bottom() - rect.height() * 0.28;
        p.line_segment(
            [pos2(rect.left() + 3.0, y), pos2(rect.right() - 3.0, y)],
            Stroke::new(2.0, palette.underline),
        );
    }
    let stroke = if hovered {
        Stroke::new(1.5, theme::c().lapis)
    } else {
        Stroke::new(1.0, theme::c().rule)
    };
    p.rect_stroke(rect, 3.0, stroke, egui::StrokeKind::Inside);
}

/// The selection toolbar's buttons in a row; returns the one clicked. Without a User database
/// (`can_annotate` false) only Copy works.
fn toolbar_row(ui: &mut Ui, palette: &Palette, can_annotate: bool) -> Option<Annotate> {
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let sense = if can_annotate {
            Sense::click()
        } else {
            Sense::hover()
        };
        for style in HighlightStyle::ALL {
            let (rect, r) = ui.allocate_exact_size(vec2(22.0, 22.0), sense);
            paint_swatch(ui, rect.shrink(2.0), style, palette, r.hovered());
            if r.on_hover_text(format!(
                "Highlight {} ({})",
                style_name(style).to_lowercase(),
                style_shortcut(style)
            ))
            .clicked()
            {
                picked = Some(Annotate::Highlight(style));
            }
        }
        ui.separator();
        if ui
            .add_enabled(can_annotate, egui::Button::new("✏ Note"))
            .on_hover_text("Write a Note on the selection (Ctrl+Shift+N)")
            .clicked()
        {
            picked = Some(Annotate::Note);
        }
        if ui
            .button("Copy")
            .on_hover_text("Copy the text with its reference (Ctrl+C)")
            .clicked()
        {
            picked = Some(Annotate::Copy);
        }
        ui.add_enabled(false, egui::Button::new("🔎 Search this"))
            .on_disabled_hover_text(super::not_yet("word_info.search"));
    });
    picked
}

/// "■ Yellow": the style's name after a square of its colour; the red underline's name is
/// underlined in its colour.
pub(super) fn menu_label(
    ui: &Ui,
    style: HighlightStyle,
    palette: &Palette,
) -> egui::text::LayoutJob {
    use egui::text::TextFormat;
    let font = egui::TextStyle::Button.resolve(ui.style());
    let ink = ui.visuals().text_color();
    let mut job = egui::text::LayoutJob::default();
    let square = palette.highlight(style).unwrap_or(palette.page);
    job.append("■", 0.0, TextFormat::simple(font.clone(), square));
    let mut name = TextFormat::simple(font, ink);
    if palette.highlight(style).is_none() {
        name.underline = Stroke::new(2.0, palette.underline);
    }
    job.append(style_name(style), 6.0, name);
    job
}

/// The right-click menu: the toolbar's actions, one per line with their keys. `enabled`: there
/// is a selection; `can_annotate`: there is a User database to keep Annotations in.
fn context_menu_items(
    ui: &mut Ui,
    palette: &Palette,
    enabled: bool,
    can_annotate: bool,
) -> Option<Annotate> {
    let mut picked = None;
    for style in HighlightStyle::ALL {
        let r = ui.add_enabled(
            enabled && can_annotate,
            egui::Button::new(menu_label(ui, style, palette)).shortcut_text(style_shortcut(style)),
        );
        if r.clicked() {
            picked = Some(Annotate::Highlight(style));
        }
    }
    ui.separator();
    if ui
        .add_enabled(
            enabled && can_annotate,
            egui::Button::new("✏ Note…").shortcut_text("Ctrl+Shift+N"),
        )
        .clicked()
    {
        picked = Some(Annotate::Note);
    }
    if ui
        .add_enabled(enabled, egui::Button::new("Copy").shortcut_text("Ctrl+C"))
        .clicked()
    {
        picked = Some(Annotate::Copy);
    }
    ui.add_enabled(false, egui::Button::new("🔎 Search this"))
        .on_disabled_hover_text(super::not_yet("word_info.search"));
    if picked.is_some() {
        ui.close();
    }
    picked
}

impl State {
    /// Whether `tab` is a reading pane with some text selected.
    pub(super) fn has_selection(&self, tab: TabId) -> bool {
        self.tabs
            .get(&tab)
            .and_then(|t| t.view.as_ref())
            .is_some_and(|v| v.selection.is_some_and(|s| has_text(v, &s)))
    }

    /// Ctrl+1…5 and Ctrl+Shift+N on the focused Tab's selection. Not while a text field has
    /// focus, and the keys stay unconsumed when there is no selection.
    pub(super) fn annotation_keys(&mut self, ui: &Ui) {
        if ui.ctx().text_edit_focused() {
            return;
        }
        let Some(tab) = self.session.workspace.focused() else {
            return;
        };
        if !self.has_selection(tab) {
            return;
        }
        let all = bindings(&self.session);
        // An extra Shift is allowed unless another binding needs it (digits need Shift on
        // AZERTY), never an extra Alt: Ctrl+Alt is AltGr on Windows.
        let extra = |sc| Modifiers {
            alt: false,
            ..extra_held(sc, &all)
        };
        let mut action = None;
        ui.input_mut(|i| {
            if consume_key(i, NOTE_KEY.modifiers, NOTE_KEY.logical_key, extra(NOTE_KEY)) {
                action = Some(Annotate::Note);
            }
            for (style, sc) in HighlightStyle::ALL.into_iter().zip(shortcuts().skip(1)) {
                if consume_key(i, sc.modifiers, sc.logical_key, extra(sc)) {
                    action = Some(Annotate::Highlight(style));
                }
            }
        });
        if let Some(a) = action {
            self.annotate(ui.ctx(), tab, a, None);
        }
    }

    /// Act on `tab`'s selection. `at` is where the selection is drawn (for the Note editor).
    pub(super) fn annotate(
        &mut self,
        ctx: &egui::Context,
        tab: TabId,
        action: Annotate,
        at: Option<Rect>,
    ) {
        let Some(t) = self.tabs.get_mut(&tab) else {
            return;
        };
        t.toolbar = false;
        let Some(view) = t.view.as_mut() else {
            return;
        };
        let Some(sel) = view.selection.filter(|s| has_text(view, s)) else {
            return;
        };
        let doc = Arc::clone(&view.doc);
        let Some(target) = selection_params(&doc, &sel) else {
            return;
        };
        let at = at.or(t.selection_rect).unwrap_or_else(|| {
            let o = view.text_origin;
            Rect::from_min_size(pos2(o[0], o[1] + 40.0), vec2(1.0, 20.0))
        });
        match action {
            Annotate::Copy => ctx.copy_text(copy_text(&doc, &sel)),
            Annotate::Highlight(style) => {
                let mut params = target;
                params["style"] = json!(style.id());
                if self.run("annotation.highlight", &params).is_some()
                    && let Some(view) = self.tabs.get_mut(&tab).and_then(|t| t.view.as_mut())
                {
                    // The Highlight shows at once; a selection on top would hide its colour.
                    view.selection = None;
                }
            }
            Annotate::Note => {
                self.save_note();
                // The open Note did not save: keep it (and its error) rather than lose its text.
                if self.note_editor.is_some() {
                    return;
                }
                let notebooks: Vec<(String, String)> = self
                    .session
                    .notebooks()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|n| (n.id, n.name))
                    .collect();
                let notebook = match self.session.last_notebook() {
                    Ok(n) => n.id,
                    Err(e) => {
                        self.toast(e);
                        return;
                    }
                };
                let copied = copy_text(&doc, &sel);
                let label = copied.lines().last().unwrap_or_default().to_string();
                let mut quote: String = selected_text(&doc, &sel)
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                if quote.chars().count() > 90 {
                    quote = quote.chars().take(89).collect::<String>() + "…";
                }
                // Focus now, so keys typed on the very next frame land in the editor.
                ctx.memory_mut(|m| m.request_focus(editor_body_id()));
                self.note_editor = Some(NoteEditor {
                    tab,
                    target,
                    label,
                    quote,
                    body: String::new(),
                    notebook,
                    notebooks,
                    at,
                    rect: None,
                    focused_once: false,
                    error: None,
                });
            }
        }
    }

    /// Save the open Note editor's Note (nothing is saved when it is empty) and close it,
    /// returning the new Note's id. If the save fails, the editor stays open with the error,
    /// keeping the text.
    pub(super) fn save_note(&mut self) -> Option<String> {
        let mut ed = self.note_editor.take()?;
        let body = ed.body.trim();
        if body.is_empty() {
            return None;
        }
        let mut params = ed.target.clone();
        params["body"] = json!(body);
        params["notebook"] = json!(ed.notebook);
        match self.session.run("annotation.note", &params) {
            Ok(out) => {
                if let Some(view) = self.tabs.get_mut(&ed.tab).and_then(|t| t.view.as_mut()) {
                    view.selection = None;
                }
                out["id"].as_str().map(String::from)
            }
            Err(e) => {
                ed.error = Some(e.to_string());
                self.note_editor = Some(ed);
                None
            }
        }
    }

    /// After a reading pane drew: the selection toolbar, the right-click menu, the Note
    /// preview, and where the Note editor anchors.
    pub(super) fn after_reading_pane(&mut self, ui: &mut Ui, tab: TabId, out: &PaneOutput) {
        let focused = self.session.workspace.focused() == Some(tab);
        let selected = self.has_selection(tab);
        let escape = ui.input(|i| i.key_pressed(Key::Escape));
        let Some(t) = self.tabs.get_mut(&tab) else {
            return;
        };
        t.selection_rect = out.selection_rect;
        if out.pressed || out.context_clicked || escape || !selected {
            t.toolbar = false;
        }
        if out.selection_made && selected {
            t.toolbar = true;
        }
        let editing_here = self.note_editor.as_ref().is_some_and(|e| e.tab == tab);
        if let (Some(ed), Some(r)) = (self.note_editor.as_mut(), out.selection_rect)
            && ed.tab == tab
        {
            ed.at = r;
        }
        let palette = self.palette;
        let can_annotate = self.session.is_enabled("annotation.highlight");
        let mut picked: Option<(Annotate, Option<Rect>)> = None;
        // The floating toolbar, just above the selection (below it at the top of the pane).
        if let (true, true, false, Some(sel)) = (
            self.tabs.get(&tab).is_some_and(|t| t.toolbar),
            focused,
            editing_here,
            out.selection_rect,
        ) {
            let pane = out.response.as_ref().map_or(sel, |r| r.rect);
            let above = sel.top() - pane.top() > 44.0;
            let (anchor, pivot) = if above {
                (pos2(sel.left(), sel.top() - 6.0), egui::Align2::LEFT_BOTTOM)
            } else {
                (pos2(sel.left(), sel.bottom() + 6.0), egui::Align2::LEFT_TOP)
            };
            egui::Area::new(egui::Id::unique(("selection-toolbar", tab)))
                .order(egui::Order::Foreground)
                .fixed_pos(anchor)
                .pivot(pivot)
                .constrain_to(pane)
                .fade_in(false)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style())
                        .inner_margin(egui::Margin::symmetric(6, 4))
                        .show(ui, |ui| {
                            if let Some(a) = toolbar_row(ui, &palette, can_annotate) {
                                picked = Some((a, Some(sel)));
                            }
                        });
                });
        }
        if let Some(r) = &out.response {
            r.context_menu(|ui| {
                if let Some(a) = context_menu_items(ui, &palette, selected, can_annotate) {
                    picked = Some((a, out.selection_rect));
                }
            });
        }
        if let Some((id, marker)) = &out.hovered_note {
            self.note_preview(ui, id, *marker);
        }
        if let Some(id) = &out.clicked_note {
            let split = ui.input(|i| i.modifiers.command);
            self.run("notes.open", &json!({ "id": id, "split": split }));
        }
        if let Some((action, at)) = picked {
            self.annotate(ui.ctx(), tab, action, at);
        }
    }

    /// Hovering a Note marker previews the Note.
    fn note_preview(&self, ui: &Ui, id: &str, marker: Rect) {
        let Some(a) = self.marks.annotation(id) else {
            return;
        };
        let notebook = self
            .marks
            .notebooks
            .get(&a.notebook_id)
            .cloned()
            .unwrap_or_default();
        let mut body: String = a.body().chars().take(600).collect();
        if a.body().chars().count() > 600 {
            body.push('…');
        }
        egui::Tooltip::always_open(
            ui.ctx().clone(),
            ui.layer_id(),
            egui::Id::unique(("note-preview", id)),
            egui::PopupAnchor::ParentRect(marker),
        )
        .show(|ui| {
            ui.set_max_width(320.0);
            ui.label(
                RichText::new(format!("Note · {notebook}"))
                    .small()
                    .color(theme::c().muted),
            );
            ui.label(body);
            ui.label(
                RichText::new("Click to open it in Notes")
                    .small()
                    .color(theme::c().muted),
            );
        });
    }

    /// The popup Note editor, if open. Saves on a click outside it, Esc, or Ctrl+Enter.
    pub(super) fn note_editor_ui(&mut self, ctx: &egui::Context) {
        let Some(ed) = self.note_editor.as_mut() else {
            return;
        };
        let popup_open = egui::Popup::is_any_open(ctx);
        let (pressed_outside, escape, submit) = ctx.input_mut(|i| {
            let outside = i.pointer.any_pressed()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|p| ed.rect.is_some_and(|r| !r.contains(p)));
            let submit = i.consume_key(Modifiers::COMMAND, Key::Enter);
            (outside, i.key_pressed(Key::Escape), submit)
        });
        if submit || (!popup_open && (pressed_outside || escape)) {
            self.save_note();
            return;
        }
        let screen = ctx.content_rect();
        let below = screen.bottom() - ed.at.bottom() > 240.0;
        let (anchor, pivot) = if below {
            (
                pos2(ed.at.left(), ed.at.bottom() + 6.0),
                egui::Align2::LEFT_TOP,
            )
        } else {
            (
                pos2(ed.at.left(), ed.at.top() - 6.0),
                egui::Align2::LEFT_BOTTOM,
            )
        };
        let mut save = false;
        let mut discard = false;
        let mut open_in_notes = false;
        let shown = egui::Area::new(egui::Id::unique("note-editor"))
            .order(egui::Order::Foreground)
            .fixed_pos(anchor)
            .pivot(pivot)
            .fade_in(false)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(340.0);
                    ui.label(RichText::new(format!("✏ Note · {}", ed.label)).strong());
                    if !ed.quote.is_empty() {
                        ui.label(
                            RichText::new(format!("“{}”", ed.quote))
                                .italics()
                                .small()
                                .color(theme::c().muted),
                        );
                    }
                    let r = ui.add(
                        egui::TextEdit::multiline(&mut ed.body)
                            .id(editor_body_id())
                            .hint_text("Write a note…")
                            .desired_rows(5)
                            .desired_width(f32::INFINITY),
                    );
                    if !ed.focused_once {
                        r.request_focus();
                        ed.focused_once = r.has_focus();
                    }
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Notebook").small().color(theme::c().muted));
                        let current = ed
                            .notebooks
                            .iter()
                            .find(|(id, _)| *id == ed.notebook)
                            .map_or_else(String::new, |(_, n)| n.clone());
                        egui::ComboBox::from_id_salt("note-editor-notebook")
                            .selected_text(current)
                            .show_ui(ui, |ui| {
                                for (id, name) in &ed.notebooks {
                                    ui.selectable_value(&mut ed.notebook, id.clone(), name);
                                }
                            });
                    });
                    ui.horizontal(|ui| {
                        if ui
                            .add(egui::Button::new("Save").shortcut_text("Ctrl+Enter"))
                            .clicked()
                        {
                            save = true;
                        }
                        if ui
                            .add_enabled(
                                !ed.body.trim().is_empty(),
                                egui::Button::new("Open in Notes"),
                            )
                            .on_hover_text("Save, and go on writing in the Notes Tool")
                            .clicked()
                        {
                            open_in_notes = true;
                        }
                        if ui
                            .button("Discard")
                            .on_hover_text("Close without saving")
                            .clicked()
                        {
                            discard = true;
                        }
                    });
                    ui.label(
                        RichText::new("Saves when you click away, press Esc or Ctrl+Enter.")
                            .small()
                            .color(theme::c().muted),
                    );
                    if let Some(e) = &ed.error {
                        ui.label(RichText::new(e).small().color(theme::c().warn));
                    }
                });
            });
        ed.rect = Some(shown.response.rect);
        if discard {
            self.note_editor = None;
        } else if open_in_notes {
            if let Some(id) = self.save_note() {
                self.run("notes.open", &json!({ "id": id }));
            }
        } else if save {
            self.save_note();
        }
    }
}
