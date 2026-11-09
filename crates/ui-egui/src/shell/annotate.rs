//! Making and showing Highlights and Notes in the origin Bible (#62, #18 rule 8, #17 rules
//! 15 and 17). The engine stores them and keeps undo (#61); this is the UI around a reading
//! pane's selection:
//!
//! - releasing a selection shows a floating **selection toolbar** (the active Highlight
//!   palette's swatches, Note, Copy, Search this); the right-click menu's Selection target
//!   offers the same actions (`context_menu.rs`, #142); Alt+1…9 (the palette's first nine
//!   styles, #181) and Ctrl+Shift+N are
//!   their keys by default (the Selection keys, rebindable in Help → Keyboard shortcuts, #177),
//!   and Ctrl+Z / Ctrl+Shift+Z undo and redo. None of these keys fire while a text field has
//!   focus: there they belong to the field.
//! - Note opens a **popup editor** anchored at the selection with a Notebook picker; it saves
//!   when focus leaves it (a click elsewhere, Esc) or on Ctrl+Enter. New Annotations go into
//!   the Notebook last used, which the engine remembers in the User database.
//! - Highlights and Note markers are drawn by the reading pane from [`Mark`]s, reloaded
//!   whenever the engine's annotation revision changes; hovering a marker previews its Note
//!   and clicking it opens the Note in the Notes Tool (#64). Notebooks hidden with their
//!   Show-in-text toggle are not drawn.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use egui::{Key, Modifiers, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use obs_engine::{
    Annotation, HighlightLook, HighlightPalette, HighlightStyle, SELECTION_KEYS, STYLE_SLOTS,
    SelectionAction, SelectionKey, Session, StyleAppearance, TabId, style_label,
};
use obs_model::ResourceType;
use serde_json::{Value, json};

use super::{State, bindings, consume_key, extra_held, parse_shortcut, pressed_as};
use crate::doc::ReadingDoc;
use crate::icons::{Icon, with_icon};
use crate::marks::{HighlightLooks, Mark, marks_for};
use crate::palette::Palette;
use crate::pane::{PaneOutput, has_text};
use crate::selection::{Selection, copy_text, selected_text};
use crate::theme;
use crate::tokens::{inset, mark, radius, size, space, stroke};

/// The Selection keys as the user has them (Ctrl+Shift+N and Alt+1…9 by default), with what
/// each does: each as every key report that presses it ([`pressed_as`]), so one bound to
/// Ctrl+Plus runs on Ctrl+= too, as a command's does.
fn selection_keys(
    session: &Session,
) -> impl Iterator<Item = (&'static SelectionKey, egui::KeyboardShortcut)> + '_ {
    (SELECTION_KEYS.iter())
        .filter_map(|k| Some((k, session.shortcut(k.id).and_then(parse_shortcut)?)))
        .flat_map(|(k, sc)| pressed_as(sc).map(move |sc| (k, sc)))
}

/// The Selection keys' shortcuts.
pub(super) fn shortcuts(session: &Session) -> impl Iterator<Item = egui::KeyboardShortcut> + '_ {
    selection_keys(session).map(|(_, sc)| sc)
}

/// The Selection keys as text, for tooltips and menus (empty: no key).
#[derive(Debug, Clone, Default)]
pub(super) struct SelectionKeyText {
    note: String,
    /// By place in the active palette.
    slots: [String; STYLE_SLOTS],
}

impl SelectionKeyText {
    pub(super) fn of(session: &Session) -> Self {
        let key = |action: SelectionAction| {
            (SELECTION_KEYS.iter())
                .find(|k| k.action == action)
                .and_then(|k| session.shortcut(k.id))
                .map(crate::key_labels::label)
                .unwrap_or_default()
        };
        Self {
            note: key(SelectionAction::Note),
            slots: std::array::from_fn(|i| key(SelectionAction::Highlight(i))),
        }
    }

    pub(super) fn note(&self) -> &str {
        &self.note
    }

    /// The key of the active palette's style at place `i` (0 first); none past the ninth.
    pub(super) fn slot(&self, i: usize) -> &str {
        self.slots.get(i).map_or("", String::as_str)
    }
}

/// `text (key)`, or `text` when `key` is empty.
fn with_key_text(text: &str, key: &str) -> String {
    if key.is_empty() {
        text.to_string()
    } else {
        format!("{text} ({key})")
    }
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Annotate {
    /// Highlight with the style of this id.
    Highlight(String),
    Note,
    Copy,
    /// Search for the selected text (#59, #142).
    Search,
    /// Open the Highlight styles window (#181).
    Styles,
}

/// The Highlight styles and palettes as loaded from the User database (#181).
#[derive(Debug, Clone, Default)]
pub(super) struct StyleBook {
    /// Every style, by palette and then in order.
    pub(super) all: Vec<HighlightStyle>,
    pub(super) palettes: Vec<HighlightPalette>,
    /// The active palette's id.
    pub(super) active: String,
    /// How many Highlights use each style.
    pub(super) uses: HashMap<String, u64>,
}

impl StyleBook {
    fn load(session: &Session) -> Self {
        if session.user.is_none() {
            return Self::default();
        }
        Self {
            all: session.highlight_styles().unwrap_or_default(),
            palettes: session.highlight_palettes().unwrap_or_default(),
            active: session
                .active_highlight_palette()
                .map(|p| p.id)
                .unwrap_or_default(),
            uses: session.highlight_style_uses().unwrap_or_default(),
        }
    }

    /// The styles of palette `id`, in order.
    pub(super) fn of_palette<'a>(
        &'a self,
        id: &'a str,
    ) -> impl Iterator<Item = &'a HighlightStyle> {
        self.all.iter().filter(move |s| s.palette_id == id)
    }

    /// The active palette's styles, in order (Alt+1 … Alt+9 for the first nine).
    pub(super) fn active_styles(&self) -> Vec<&HighlightStyle> {
        self.of_palette(&self.active).collect()
    }

    pub(super) fn get(&self, id: &str) -> Option<&HighlightStyle> {
        self.all.iter().find(|s| s.id == id)
    }

    /// What the UI calls style `id`: its name, or its place ("Style 3").
    pub(super) fn label(&self, id: &str) -> String {
        self.get(id)
            .map_or_else(|| id.to_string(), |s| style_label(s, &self.all))
    }

    /// Style `id`'s look (a missing one shows as the first built-in).
    pub(super) fn appearance(&self, id: &str) -> StyleAppearance {
        self.get(id)
            .map_or_else(obs_store::fallback_appearance, |s| s.appearance)
    }
}

/// Annotations shown in text as loaded from the User database, and each Resource's [`Mark`]s,
/// reloaded when the engine's annotation revision moves (any new, edited, undone or redone
/// Annotation, a Notebook's Show in text, View → Show Highlights, a Highlight style or
/// palette edited or switched).
#[derive(Default)]
pub(super) struct MarkCache {
    revision: Option<u64>,
    annotations: Vec<Annotation>,
    pub(super) styles: StyleBook,
    looks: HighlightLooks,
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
        self.styles = StyleBook::load(session);
        self.looks = HighlightLooks::new(&self.styles.all);
        self.by_resource.clear();
    }

    /// The Highlight styles as of the engine's annotation revision now.
    pub(super) fn styles(&mut self, session: &Session) -> &StyleBook {
        self.refresh(session);
        &self.styles
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
        let (annotations, looks) = (&self.annotations, &self.looks);
        Arc::clone(
            self.by_resource
                .entry(doc.manifest.resource_id.clone())
                .or_insert_with(|| Arc::new(marks_for(doc, annotations, looks))),
        )
    }

    fn annotation(&self, id: &str) -> Option<&Annotation> {
        self.annotations.iter().find(|a| a.id == id)
    }
}

/// The popup Note editor, anchored at the selection it was opened on.
pub(super) struct NoteEditor {
    pub(super) tab: TabId,
    /// The selection, as `annotation.note` params (resource, start_unit, start, end_unit, end,
    /// and the revision it was made in).
    target: Value,
    /// "Genesis 30:3 (TB)": where the Note goes.
    label: String,
    quote: String,
    /// The text selected, whole, as [`selected_text`] read it: what a new revision of the
    /// Resource must still have at `target` for the Note to be made in it (#259).
    selected: String,
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

/// The selection as command params: resource, start_unit, start, end_unit, end, and the
/// revision (content hash) its offsets belong to, so the engine refuses them once the
/// Resource has been replaced (#259).
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
        "revision": doc.manifest.content_hash,
    }))
}

/// The text selection params `target` cover in `doc`, as [`selected_text`] reads it; `None`
/// when `doc` has no such units or offsets.
fn text_at(doc: &ReadingDoc, target: &Value) -> Option<String> {
    let pos = |unit: &str, byte: &str| {
        let id = target[unit].as_str()?;
        let unit = doc.units.iter().position(|u| u.id.as_str() == id)?;
        let byte = usize::try_from(target[byte].as_u64()?).ok()?;
        doc.units[unit]
            .text
            .is_char_boundary(byte)
            .then_some(crate::selection::DocPos { unit, byte })
    };
    let sel = Selection {
        anchor: pos("start_unit", "start")?,
        head: pos("end_unit", "end")?,
        granularity: crate::selection::Granularity::Grapheme,
    };
    Some(selected_text(doc, &sel))
}

/// Paint one style's swatch into `rect` as it shows on the page of `palette`'s tone: a
/// background, a line under a sample stroke, a box, or a letter in its colour.
pub(super) fn paint_swatch(
    ui: &Ui,
    rect: Rect,
    style: &StyleAppearance,
    palette: &Palette,
    hovered: bool,
) {
    let p = ui.painter();
    let paint = palette.paint(style);
    let inner = rect.shrink(rect.height() * mark::SWATCH_INSET);
    match paint.look {
        HighlightLook::Background => {
            p.rect_filled(rect, radius::S, paint.color);
        }
        HighlightLook::Underline => {
            p.rect_filled(rect, radius::S, palette.page);
            let y = rect.bottom() - rect.height() * mark::SWATCH_UNDERLINE;
            let pad = mark::SAMPLE_PAD.x;
            p.line_segment(
                [pos2(rect.left() + pad, y), pos2(rect.right() - pad, y)],
                Stroke::new(stroke::THICK, paint.color),
            );
        }
        HighlightLook::Box => {
            p.rect_filled(rect, radius::S, palette.page);
            p.rect_stroke(
                inner,
                mark::RADIUS,
                Stroke::new(stroke::THICK, paint.color),
                egui::StrokeKind::Inside,
            );
        }
        HighlightLook::TextColor => {
            p.rect_filled(rect, radius::S, palette.page);
            p.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "A",
                egui::FontId::proportional(rect.height() * mark::SWATCH_LETTER),
                paint.color,
            );
        }
    }
    let stroke = if hovered {
        Stroke::new(stroke::MEDIUM, theme::c().lapis)
    } else {
        crate::components::rule_stroke()
    };
    p.rect_stroke(rect, radius::S, stroke, egui::StrokeKind::Inside);
}

/// The "Highlight styles" button that opens the styles window from the toolbar.
const STYLES_BUTTON: &str = "…";

/// The selection toolbar's buttons in a row; returns the one clicked. Without a User database
/// (`can_annotate` false) only Copy works.
fn toolbar_row(
    ui: &mut Ui,
    palette: &Palette,
    can_annotate: bool,
    keys: &SelectionKeyText,
    styles: &StyleBook,
) -> Option<Annotate> {
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::XS;
        let sense = if can_annotate {
            Sense::click()
        } else {
            Sense::hover()
        };
        for (i, style) in styles.active_styles().into_iter().enumerate() {
            let (rect, r) = ui.allocate_exact_size(egui::Vec2::splat(size::CONTROL_COMPACT), sense);
            paint_swatch(
                ui,
                rect.shrink(space::XXS),
                &style.appearance,
                palette,
                r.hovered(),
            );
            let name = format!("Highlight: {}", style_label(style, &styles.all));
            let r = r.accessible_name(&name);
            if r.on_hover_text(with_key_text(&name, keys.slot(i)))
                .clicked()
            {
                picked = Some(Annotate::Highlight(style.id.clone()));
            }
        }
        if ui
            .add_enabled(
                can_annotate,
                crate::components::button(STYLES_BUTTON).small(),
            )
            .on_hover_text("Highlight styles")
            .accessible_name("Highlight styles")
            .clicked()
        {
            picked = Some(Annotate::Styles);
        }
        ui.separator();
        if ui
            .add_enabled(
                can_annotate,
                crate::components::button(with_icon(ui, Icon::Note, "Note")),
            )
            .on_hover_text(with_key_text("Write a Note on the selection", keys.note()))
            .clicked()
        {
            picked = Some(Annotate::Note);
        }
        if ui
            .add(crate::components::button(with_icon(ui, Icon::Copy, "Copy")))
            .on_hover_text(format!(
                "Copy the text with its reference ({})",
                crate::key_labels::label("Ctrl+C")
            ))
            .clicked()
        {
            picked = Some(Annotate::Copy);
        }
        if ui
            .add(crate::components::button(with_icon(
                ui,
                Icon::Search,
                "Search this",
            )))
            .on_hover_text("Search for the selected words")
            .clicked()
        {
            picked = Some(Annotate::Search);
        }
    });
    picked
}

/// A swatch of the style's colour, then its name; an underline style's name is underlined in
/// its colour.
pub(super) fn menu_label(
    ui: &Ui,
    name: &str,
    style: &StyleAppearance,
) -> (egui::Image<'static>, egui::text::LayoutJob) {
    use egui::text::TextFormat;
    let font = egui::TextStyle::Button.resolve(ui.style());
    let ink = ui.visuals().text_color();
    let mut job = egui::text::LayoutJob::default();
    // The swatch sits on the menu, not the page: the style's margin-mark colour for the
    // chrome's theme, strong enough to see against it (#182).
    let c = crate::theme::c();
    let tone = if c.dark {
        obs_engine::PageTone::Dark
    } else {
        obs_engine::PageTone::Paper
    };
    let chrome = Palette::for_theme(tone, c.high_contrast);
    let paint = chrome.paint(style);
    let swatch = crate::icons::image(ui.ctx(), Icon::Swatch, crate::icons::SMALL, paint.mark);
    let mut text = TextFormat::simple(font, ink);
    if paint.look == HighlightLook::Underline {
        text.underline = Stroke::new(stroke::THICK, paint.mark);
    }
    job.append(name, 0.0, text);
    (swatch, job)
}

impl State {
    /// Whether `tab` is a reading pane with some text selected.
    pub(super) fn has_selection(&self, tab: TabId) -> bool {
        self.tabs
            .get(&tab)
            .and_then(|t| t.view.as_ref())
            .is_some_and(|v| v.selection.is_some_and(|s| has_text(v, &s)))
    }

    /// Alt+1…9 and Ctrl+Shift+N on the selection of `tab`, the Tab focused in the window
    /// the keys were typed in: whether one ran. Not while a text field has focus, and the
    /// keys stay unconsumed when there is no selection.
    pub(super) fn annotation_keys(&mut self, ui: &Ui, tab: Option<TabId>) -> bool {
        if ui.ctx().text_edit_focused() {
            return false;
        }
        let Some(tab) = tab else {
            return false;
        };
        if !self.has_selection(tab) {
            return false;
        }
        let all = bindings(&self.session);
        // An extra Shift is allowed unless another binding needs it (digits need Shift on
        // AZERTY), never an extra Alt: Ctrl+Alt is AltGr on Windows.
        let extra = |sc| Modifiers {
            alt: false,
            ..extra_held(sc, &all)
        };
        let mut action = None;
        // Alt+1…9 follow the active palette's order; a place past its last style is no key.
        let active: Vec<String> = (self.marks.styles(&self.session).active_styles().iter())
            .map(|s| s.id.clone())
            .collect();
        let keys: Vec<_> = selection_keys(&self.session)
            .filter(|(k, _)| match k.action {
                SelectionAction::Highlight(slot) => slot < active.len(),
                SelectionAction::Note => true,
            })
            .collect();
        ui.input_mut(|i| {
            for (k, sc) in keys {
                if consume_key(i, sc.modifiers, sc.logical_key, extra(sc)) {
                    action = Some(match k.action {
                        SelectionAction::Note => Annotate::Note,
                        SelectionAction::Highlight(slot) => {
                            Annotate::Highlight(active[slot].clone())
                        }
                    });
                }
            }
        });
        let Some(a) = action else {
            return false;
        };
        self.annotate(ui.ctx(), tab, a, None);
        true
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
        // Copy takes the Tab's interlinear lines along (#143).
        let copied = matches!(action, Annotate::Copy).then(|| view.copy_text(&sel));
        let Some(target) = selection_params(&doc, &sel) else {
            return;
        };
        let at = at.or(t.selection_rect).unwrap_or_else(|| {
            let o = view.text_origin;
            Rect::from_min_size(
                pos2(o[0], o[1] + space::XXXL),
                vec2(1.0, size::CONTROL_COMPACT),
            )
        });
        match action {
            Annotate::Copy => ctx.copy_text(copied.unwrap_or_default()),
            Annotate::Styles => self.styles_ui.open = true,
            Annotate::Highlight(style) => {
                let mut params = target;
                params["style"] = json!(style);
                if self.run("annotation.highlight", &params).is_some()
                    && let Some(view) = self.tabs.get_mut(&tab).and_then(|t| t.view.as_mut())
                {
                    // The Highlight shows at once; a selection on top would hide its colour.
                    view.selection = None;
                }
            }
            Annotate::Search => {
                let text = selected_text(&doc, &sel);
                let new_tab = ctx.input(|i| i.modifiers.command);
                let (id, ty) = (&doc.manifest.resource_id, doc.manifest.resource_type);
                self.search_text(&text, id, ty, new_tab);
            }
            Annotate::Note => {
                let copied = copy_text(&doc, &sel);
                let label = copied.lines().last().unwrap_or_default().to_string();
                let selected = selected_text(&doc, &sel);
                let mut quote: String = selected.split_whitespace().collect::<Vec<_>>().join(" ");
                if quote.chars().count() > 90 {
                    quote = quote.chars().take(89).collect::<String>() + "…";
                }
                self.open_note_editor(ctx, tab, (target, selected), label, quote, at);
            }
        }
    }

    /// Open the popup Note editor at `at` for a Note on `target` (`annotation.note` params: a
    /// selection, with the text selected whole, or a `passage` and no text). `label` says
    /// where the Note goes, `quote` what it is on.
    pub(super) fn open_note_editor(
        &mut self,
        ctx: &egui::Context,
        tab: TabId,
        (target, selected): (Value, String),
        label: String,
        quote: String,
        at: Rect,
    ) {
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
        // Focus now, so keys typed on the very next frame land in the editor; the key that
        // opened it (Enter or Space on a menu item) is not typed into it this frame.
        ctx.memory_mut(|m| m.request_focus(editor_body_id()));
        ctx.input_mut(|i| {
            i.events.retain(|e| match e {
                egui::Event::Key { key, pressed, .. } => {
                    !(*pressed && matches!(key, egui::Key::Enter | egui::Key::Space))
                }
                egui::Event::Text(_) => false,
                _ => true,
            });
        });
        self.note_editor = Some(NoteEditor {
            tab,
            target,
            label,
            quote,
            selected,
            body: String::new(),
            notebook,
            notebooks,
            at,
            rect: None,
            focused_once: false,
            error: None,
        });
    }

    /// Search for selected text (#142), always quoted so it is searched literally: a
    /// selected `OR`, `AND`, `NOT`, `G25`, `-word` or `word*` is the text, not query syntax.
    /// In a Bible, Bible search in that Bible; elsewhere, Library search in that Resource.
    /// In the most recent Search Tab, or a new one with `new_tab` (Ctrl/Cmd held).
    pub(super) fn search_text(
        &mut self,
        text: &str,
        resource: &str,
        ty: ResourceType,
        new_tab: bool,
    ) {
        let words: Vec<&str> = text.split_whitespace().collect();
        if words.is_empty() {
            return;
        }
        let query = format!("\"{}\"", words.join(" ").replace('"', ""));
        let params = if ty == ResourceType::Bible {
            json!({ "text": query, "mode": "bible", "bibles": resource, "split": new_tab })
        } else {
            json!({ "text": query, "mode": "library", "resources": resource, "split": new_tab })
        };
        self.run("search.run", &params);
    }

    /// A new revision of Resource `id` replaced the one the open Note editor's selection was
    /// made in (#259). Where the new one has the same text there, the Note will be made in it;
    /// otherwise the editor says so, keeping the text, and saving is refused (the selection's
    /// `revision` is the old one), so the Note never quotes other words.
    pub(super) fn note_editor_replaced(&mut self, id: &str) {
        let Some(ed) = self.note_editor.as_ref() else {
            return;
        };
        if ed.target["resource"].as_str() != Some(id) {
            return;
        }
        let doc = self.doc(id).ok();
        let Some(ed) = self.note_editor.as_mut() else {
            return;
        };
        match doc {
            Some(doc) if text_at(&doc, &ed.target).as_deref() == Some(ed.selected.as_str()) => {
                ed.target["revision"] = json!(doc.manifest.content_hash);
            }
            doc => {
                let abbreviation =
                    doc.map_or_else(|| id.to_string(), |d| d.manifest.abbreviation.clone());
                ed.error = Some(format!(
                    "{abbreviation} changed while this Note was open: “{}” is no longer where it \
                     was. Copy your text, then Discard and select the text again.",
                    ed.quote
                ));
            }
        }
    }

    /// Save the open Note editor's Note (nothing is saved when it is only white space) and
    /// close it, returning the new Note's id. The text is saved as typed: its white space is
    /// CommonMark (four spaces start a code block), as in the Notes Tool's editor. If the save
    /// fails, the editor stays open with the error, keeping the text.
    pub(super) fn save_note(&mut self) -> Option<String> {
        let mut ed = self.note_editor.take()?;
        let body = ed.body.as_str();
        if body.trim().is_empty() {
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

    /// After a reading pane drew: the selection toolbar, the Note preview, where the Note
    /// editor anchors, and the right-click menu (#142).
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
        let keys = SelectionKeyText::of(&self.session);
        let styles = self.marks.styles(&self.session).clone();
        let mut picked: Option<(Annotate, Option<Rect>)> = None;
        // The floating toolbar, just above the selection (below it at the top of the pane).
        if let (true, true, false, Some(sel)) = (
            self.tabs.get(&tab).is_some_and(|t| t.toolbar),
            focused,
            editing_here,
            out.selection_rect,
        ) {
            let pane = out.response.as_ref().map_or(sel, |r| r.rect);
            let above = sel.top() - pane.top() > size::CONTROL + space::XL;
            let (anchor, pivot) = if above {
                (
                    pos2(sel.left(), sel.top() - space::S),
                    egui::Align2::LEFT_BOTTOM,
                )
            } else {
                (
                    pos2(sel.left(), sel.bottom() + space::S),
                    egui::Align2::LEFT_TOP,
                )
            };
            egui::Area::new(egui::Id::unique(("selection-toolbar", tab)))
                .order(egui::Order::Foreground)
                .fixed_pos(anchor)
                .pivot(pivot)
                .constrain_to(pane)
                .fade_in(false)
                .show(ui.ctx(), |ui| {
                    crate::components::popover_frame()
                        .inner_margin(inset::BAR)
                        .show(ui, |ui| {
                            if let Some(a) = toolbar_row(ui, &palette, can_annotate, &keys, &styles)
                            {
                                picked = Some((a, Some(sel)));
                            }
                        });
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
        self.reading_menu(ui, tab, out, focused);
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
        egui::Tooltip::always_open(
            ui.ctx().clone(),
            ui.layer_id(),
            egui::Id::unique(("note-preview", id)),
            egui::PopupAnchor::ParentRect(marker),
        )
        .show(|ui| {
            ui.set_max_width(size::POPOVER);
            ui.label(crate::components::hint_text(format!("Note · {notebook}")));
            note_preview_lines(ui, a.body());
            ui.label(crate::components::hint_text("Click to open it in Notes"));
        });
    }

    /// The popup Note editor, if open. Saves on a click outside it, Esc, or Ctrl+Enter. The
    /// Esc it saves on is its own: no window behind it closes too (#182).
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
            let modifiers = i.modifiers;
            let escape = !popup_open && i.consume_key(modifiers, Key::Escape);
            (outside, escape, submit)
        });
        if submit || (!popup_open && (pressed_outside || escape)) {
            self.save_note();
            return;
        }
        let screen = ctx.content_rect();
        let below = screen.bottom() - ed.at.bottom() > size::LIST_MAX;
        let (anchor, pivot) = if below {
            (
                pos2(ed.at.left(), ed.at.bottom() + space::S),
                egui::Align2::LEFT_TOP,
            )
        } else {
            (
                pos2(ed.at.left(), ed.at.top() - space::S),
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
                crate::components::popover_frame().show(ui, |ui| {
                    ui.set_width(size::POPOVER);
                    crate::icons::label(
                        ui,
                        Icon::Note,
                        theme::c().muted,
                        RichText::new(format!("Note · {}", ed.label)).strong(),
                    );
                    if !ed.quote.is_empty() {
                        ui.label(
                            RichText::new(format!("“{}”", ed.quote))
                                .italics()
                                .small()
                                .color(theme::c().muted),
                        );
                    }
                    let r = super::note_editor::note_editor(ui, editor_body_id(), &mut ed.body, 5)
                        .response;
                    if !ed.focused_once {
                        r.request_focus();
                        ed.focused_once = r.has_focus();
                    }
                    ui.horizontal(|ui| {
                        ui.label(crate::components::hint_text("Notebook"));
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
                            })
                            .response
                            .accessible_name("Notebook");
                    });
                    ui.horizontal(|ui| {
                        if ui
                            .add(
                                crate::components::button("Save")
                                    .shortcut_text(crate::key_labels::label("Ctrl+Enter")),
                            )
                            .clicked()
                        {
                            save = true;
                        }
                        if ui
                            .add_enabled(
                                !ed.body.trim().is_empty(),
                                crate::components::button("Open in Notes"),
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
                    ui.label(crate::components::hint_text(format!(
                        "Saves when you click away, press Esc or {}.",
                        crate::key_labels::label("Ctrl+Enter")
                    )));
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

/// A Note's text in its marker's preview: its first 600 characters as shown, a heading
/// strong, a list item after its bullet or number, a quote line in quotation marks (#186).
fn note_preview_lines(ui: &mut Ui, body: &str) {
    let mut left = 600usize;
    for line in obs_model::parse_note_text(body) {
        if left == 0 {
            ui.label("…");
            break;
        }
        let mut text: String = line.text.chars().take(left).collect();
        left = left.saturating_sub(line.text.chars().count());
        if left == 0 && text.chars().count() < line.text.chars().count() {
            text.push('…');
        }
        let indent = |level: u8| "    ".repeat(usize::from(level));
        match line.kind {
            obs_model::NoteLineKind::Blank => ui.add_space(space::XS),
            obs_model::NoteLineKind::Heading(_) => {
                ui.label(RichText::new(text).strong());
            }
            obs_model::NoteLineKind::Bullet { level } => {
                ui.label(format!("{}• {text}", indent(level)));
            }
            obs_model::NoteLineKind::Numbered { level, number } => {
                ui.label(format!("{}{number}. {text}", indent(level)));
            }
            obs_model::NoteLineKind::Quote => {
                ui.label(format!("“{text}”"));
            }
            obs_model::NoteLineKind::Text => {
                ui.label(text);
            }
        }
    }
}
