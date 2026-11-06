//! The Notes Tool's Tab (#64; #18 rules 7 and 10, #17 rules 7, 13, 18 and 19): a Notebook
//! column beside a card list.
//!
//! - **Column**: All, each Notebook (its 👁 Show-in-text toggle, its count; right-click or
//!   double-click to rename, right-click to delete) and + New notebook. Deleting asks what
//!   happens to the Notebook's Annotations: moved to the default Notebook (pre-selected) or
//!   deleted; either way Ctrl+Z brings everything back. **⚠ Needs attention** (#65, #17
//!   rule 12) lists the Orphaned annotations; it shows only when there are some (or while it
//!   is the chosen entry).
//! - **Orphaned cards** say their text was not found after their Resource changed, show the
//!   quote, and offer **Re-attach here** (anchor it on the text selected in the most recently
//!   used reading pane), **Keep without anchor** (Notes only) and **Delete**; each is one undo
//!   step. A card whose Resource left the library says so ("WEB is not in the library"): its
//!   Annotations are dormant until it is imported again.
//! - **Cards**, in Bible order or by last change, narrowed by the header's filter field.
//!   A Note card shows its reference, quote and body; a Highlight card its reference, quote
//!   and style swatch. Clicking a reference follows the anchor (Ctrl/Cmd: a new Tab). Clicking
//!   a Note's body edits it in place: it saves as you type (one undo step per editing pass),
//!   and when focus leaves it, on Esc or Ctrl+Enter. Bible references in a body are links
//!   with the reference popup (clicking one runs `notes.show_passage`); they are found once
//!   per body, not every frame. A save the database refuses (#129) keeps the editor open with
//!   the text and the error, tries again a moment later, and offers Discard changes; only a
//!   Note deleted underneath ends the editing pass. The list is virtualized (only cards in view are drawn and
//!   measured) and quotes and bodies are drawn by the text pipeline.
//! - **While a card is edited** the list keeps its order and members, so a save (which moves
//!   the card in Last modified order, or out of the filter) cannot pull the editor out of
//!   view mid-sentence; the list re-sorts when editing ends. The Shell checks the autosave
//!   every frame, and ends the editing pass (saving it) in any frame the Notes Tab is not
//!   drawn, so a Note is searchable, and safe, as soon as another Tab covers it.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::Arc;

use egui::{Mesh, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, UiBuilder, pos2, vec2};
use obs_engine::{
    Card, CardOrder, Content, HighlightStyle, Notebook, NotesFilter, NotesFormat, TabId,
    body_references,
};
use obs_layout::{ColorRole, LaidBlock};
use obs_scripture::{PassageList, RefStyle, Standard, format_passage};
use serde_json::{Value, json};

use super::State;
use super::annotate::{menu_label, paint_swatch, selection_params};
use crate::heights::HeightIndex;
use crate::theme;
use crate::workers::SnippetCache;

/// A card being edited saves this long after the last keystroke.
pub(crate) const AUTOSAVE_SECS: f64 = 1.0;

/// The Notebook column's width.
const COLUMN: f32 = 210.0;
/// Space between cards.
const GAP: f32 = 8.0;

/// A Note card being edited in place.
pub(super) struct CardEdit {
    pub(super) id: String,
    pub(super) text: String,
    /// The text as last saved.
    saved: String,
    /// Saved at least once in this editing pass: later saves join its undo step.
    saved_once: bool,
    changed_at: f64,
    focus: bool,
    /// Why the last save failed (the database refused the write). The editor stays open with
    /// the text and this message; the save is tried again a moment later.
    pub(super) error: Option<String>,
}

/// The Delete Notebook dialog.
pub(super) struct DeleteNotebook {
    id: String,
    name: String,
    count: u64,
    /// Delete the Annotations rather than moving them to the default Notebook.
    pub(super) delete: bool,
}

/// Where a card was drawn last frame (screen points), for tests.
#[derive(Debug, Clone)]
#[cfg_attr(not(test), allow(dead_code))]
pub(super) struct DrawnCard {
    pub(super) id: String,
    /// The whole card.
    pub(super) rect: Rect,
    /// A Note's body (or its editor).
    pub(super) body: Option<Rect>,
}

/// What the cards were loaded for.
#[derive(Debug, Clone, PartialEq)]
struct Loaded {
    revision: u64,
    filter: NotesFilter,
    order: CardOrder,
    text: String,
    /// [`library_key`]: which Resources are in the library, and their abbreviations.
    library: u64,
    /// Loaded while a card was edited: the order and members were kept.
    pinned: bool,
}

impl Loaded {
    /// The same list asked for (only the data under it may have moved).
    fn same_view(&self, other: &Loaded) -> bool {
        self.filter == other.filter && self.order == other.order && self.text == other.text
    }
}

/// A hash of what card labels and "not in the library" read from the library: each entry's
/// id, abbreviation and revision. Swapping one Resource for another changes it.
fn library_key(library: &obs_engine::Library) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for e in library.entries() {
        (
            &e.manifest.resource_id,
            &e.manifest.abbreviation,
            e.manifest.revision,
        )
            .hash(&mut h);
    }
    h.finish()
}

/// A Note body's Bible references, one list per line (byte ranges into the line).
type BodyRefs = Arc<Vec<Vec<(Range<usize>, PassageList)>>>;

/// The Notes Tab's UI state.
#[derive(Default)]
pub(super) struct NotesUi {
    loaded: Option<Loaded>,
    cards: Arc<Vec<Card>>,
    notebooks: Vec<Notebook>,
    counts: BTreeMap<String, u64>,
    hidden: BTreeSet<String>,
    /// How many Orphaned annotations there are (Needs attention).
    attention: usize,
    /// Re-attach clicked on a card: its id, the reading Tab with the selection, and the
    /// selection as command params. Run after the frame's cards.
    reattach: Option<(String, TabId, Value)>,
    heights: Option<HeightIndex>,
    pub(super) editing: Option<CardEdit>,
    /// A Notebook being renamed: id and text.
    renaming: Option<(String, String, bool)>,
    /// The new Notebook's name being typed.
    creating: Option<(String, bool)>,
    pub(super) deleting: Option<DeleteNotebook>,
    pub(super) drawn: Vec<DrawnCard>,
    snippets: SnippetCache,
    scroll_to: Option<f32>,
    /// Each Note's Bible references, by id, with the body they were found in.
    refs: HashMap<String, (String, BodyRefs)>,
    /// How many bodies were searched for references (tests check the cache).
    pub(super) ref_parses: usize,
    /// The Notes Tab was drawn this frame.
    pub(super) shown: bool,
}

impl NotesUi {
    /// Card text still being laid out.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn has_pending(&self) -> bool {
        self.snippets.has_pending()
    }

    #[cfg(test)]
    pub(super) fn cards_len(&self) -> usize {
        self.cards.len()
    }

    /// Where a card is in the list.
    #[cfg(test)]
    pub(super) fn card_index(&self, id: &str) -> Option<usize> {
        self.cards.iter().position(|c| c.annotation.id == id)
    }

    /// A Note body's Bible references, found once per (id, body).
    fn body_refs(&mut self, id: &str, body: &str) -> BodyRefs {
        if let Some((seen, refs)) = self.refs.get(id)
            && seen == body
        {
            return Arc::clone(refs);
        }
        self.ref_parses += 1;
        let refs: BodyRefs = Arc::new(body.split('\n').map(body_references).collect());
        self.refs
            .insert(id.to_string(), (body.to_string(), Arc::clone(&refs)));
        refs
    }
}

fn estimate(card: &Card, size: f32) -> f32 {
    let lines = 2.0 + card.annotation.body().len() as f32 / 60.0;
    40.0 + lines * size * 1.6 + GAP
}

impl State {
    pub(super) fn notes_body(&mut self, ui: &mut Ui, tab: TabId) {
        if self.session.user.is_none() {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new("Notes are kept in the User database, which is not open")
                        .color(theme::c().muted),
                );
            });
            return;
        }
        self.notes_ui.shown = true;
        self.refresh_notes(tab);
        self.reveal_card(tab);
        let rect = ui.max_rect();
        let column = Rect::from_min_max(rect.min, pos2(rect.left() + COLUMN, rect.bottom()));
        let cards = Rect::from_min_max(pos2(column.right() + 1.0, rect.top()), rect.max);
        ui.painter().rect_filled(column, 0.0, theme::c().surface);
        ui.painter().vline(
            column.right(),
            column.y_range(),
            Stroke::new(1.0, theme::c().rule),
        );
        let mut run: Vec<(&'static str, Value)> = Vec::new();
        let mut col = ui.new_child(
            UiBuilder::new()
                .max_rect(column.shrink2(vec2(8.0, 6.0)))
                .id_salt(("notes-column", tab)),
        );
        let export = self.notebook_column(&mut col, &mut run);
        let mut list = ui.new_child(
            UiBuilder::new()
                .max_rect(cards.shrink2(vec2(10.0, 6.0)))
                .id_salt(("notes-cards", tab)),
        );
        list.set_clip_rect(cards.intersect(ui.clip_rect()));
        self.card_list(&mut list, tab, &mut run);
        self.delete_notebook_dialog(ui.ctx(), &mut run);
        for (id, params) in run {
            self.run(id, &params);
        }
        if let Some(format) = export {
            // The Notebook shown; All and Needs attention export every Notebook.
            let notebook = match &self.session.notes.filter {
                NotesFilter::Notebook(id) => id.clone(),
                NotesFilter::All | NotesFilter::NeedsAttention => "all".to_string(),
            };
            self.export_or_print(ui.ctx(), json!({ "notebook": notebook }), format);
        }
        self.run_reattach();
    }

    /// The reading pane whose selection **Re-attach here** uses: the most recently used Tab
    /// with text selected, and the selection as command params, with where it is
    /// ("Genesis 1:3 (TB)").
    pub(super) fn reattach_target(&self) -> Option<(TabId, Value, String)> {
        let tab = self
            .session
            .workspace
            .most_recent(|t| self.has_selection(t.id))?;
        let view = self.tabs.get(&tab)?.view.as_ref()?;
        let sel = view.selection?;
        let params = selection_params(&view.doc, &sel)?;
        let label = crate::selection::copy_text(&view.doc, &sel)
            .lines()
            .last()
            .unwrap_or_default()
            .to_string();
        Some((tab, params, label))
    }

    /// Run a Re-attach clicked this frame; on success the selection goes, so the moved
    /// Highlight or Note marker shows at once.
    fn run_reattach(&mut self) {
        let Some((id, tab, mut params)) = self.notes_ui.reattach.take() else {
            return;
        };
        params["id"] = json!(id);
        if self.run("annotation.reattach", &params).is_some() {
            if let Some(view) = self.tabs.get_mut(&tab).and_then(|t| t.view.as_mut()) {
                view.selection = None;
            }
            self.toast("Re-attached. Ctrl+Z to undo.".to_string());
        }
    }

    /// Reload the cards and Notebooks when anything they show changed. While a card is edited
    /// the list keeps its order and members (fresh contents, Annotations gone dropped, new
    /// ones at the end), so its own saves do not move it; it re-sorts when editing ends.
    fn refresh_notes(&mut self, tab: TabId) {
        let want = Loaded {
            revision: self.session.annotations_revision(),
            filter: self.session.notes.filter.clone(),
            order: self.session.notes.order,
            text: self
                .tabs
                .get(&tab)
                .map(|t| t.filter.clone())
                .unwrap_or_default(),
            library: library_key(&self.session.library),
            pinned: self.notes_ui.editing.is_some(),
        };
        if self.notes_ui.loaded.as_ref() == Some(&want) {
            return;
        }
        let keep_order = want.pinned
            && self
                .notes_ui
                .loaded
                .as_ref()
                .is_some_and(|l| l.same_view(&want));
        let fresh = match self.session.note_cards(&want.text) {
            Ok(c) => c,
            Err(e) => {
                self.toast(e);
                Vec::new()
            }
        };
        let cards = if keep_order {
            // The kept cards' contents come unfiltered: the edited card may no longer match.
            let unfiltered = if want.text.trim().is_empty() {
                fresh.clone()
            } else {
                self.session.note_cards("").unwrap_or_default()
            };
            let mut all: HashMap<String, Card> = unfiltered
                .into_iter()
                .map(|c| (c.annotation.id.clone(), c))
                .collect();
            let old = &self.notes_ui.cards;
            let kept: BTreeSet<&str> = old.iter().map(|c| c.annotation.id.as_str()).collect();
            let mut cards: Vec<Card> = old
                .iter()
                .filter_map(|c| all.remove(&c.annotation.id))
                .collect();
            cards.extend(
                fresh
                    .into_iter()
                    .filter(|c| !kept.contains(c.annotation.id.as_str())),
            );
            cards
        } else {
            fresh
        };
        let size = self.session.text.size;
        let ui = &mut self.notes_ui;
        // The same cards in the same order keep their measured heights.
        let same = ui.cards.len() == cards.len()
            && ui
                .cards
                .iter()
                .zip(&cards)
                .all(|(a, b)| a.annotation.id == b.annotation.id);
        if !same {
            ui.heights = Some(HeightIndex::new(
                cards.iter().map(|c| estimate(c, size)).collect(),
            ));
        }
        let ids: BTreeSet<&str> = cards.iter().map(|c| c.annotation.id.as_str()).collect();
        ui.refs.retain(|id, _| ids.contains(id.as_str()));
        ui.cards = Arc::new(cards);
        ui.notebooks = self.session.notebooks().unwrap_or_default();
        ui.counts = self
            .session
            .notebook_counts()
            .unwrap_or_default()
            .into_iter()
            .collect();
        ui.hidden = self.session.hidden_in_text();
        ui.attention = self.session.orphaned_annotations().map_or(0, |a| a.len());
        ui.loaded = Some(want);
    }

    /// `notes.open` asked for a card: scroll to it and, for a Note, start editing it.
    fn reveal_card(&mut self, tab: TabId) {
        let Some((id, edit)) = self.session.notes.reveal.take() else {
            return;
        };
        let Some(i) = self
            .notes_ui
            .cards
            .iter()
            .position(|c| c.annotation.id == id)
        else {
            // Hidden by the filter field: clear it and look again next frame.
            if let Some(t) = self.tabs.get_mut(&tab)
                && !t.filter.is_empty()
            {
                t.filter.clear();
                self.session.notes.reveal = Some((id, edit));
            }
            return;
        };
        if let Some(h) = self.notes_ui.heights.as_mut() {
            self.notes_ui.scroll_to = Some(h.offset(i) as f32);
        }
        let card = &self.notes_ui.cards[i].annotation;
        if edit && card.is_note() {
            let (id, body) = (card.id.clone(), card.body().to_string());
            self.start_card_edit(&id, &body);
        }
    }

    fn start_card_edit(&mut self, id: &str, body: &str) {
        if self.notes_ui.editing.as_ref().is_some_and(|e| e.id == id) {
            return;
        }
        self.save_card_edit(true);
        // The card being edited did not save: keep it (and its error) rather than lose its text.
        if self.notes_ui.editing.is_some() {
            return;
        }
        self.notes_ui.editing = Some(CardEdit {
            id: id.to_string(),
            text: body.to_string(),
            saved: body.to_string(),
            saved_once: false,
            changed_at: self.now,
            focus: true,
            error: None,
        });
    }

    /// Save the card being edited if its text changed; `finished` ends the editing pass. A save
    /// the database refuses keeps the editor open, with its text and the error, whatever
    /// `finished` says.
    pub(super) fn save_card_edit(&mut self, finished: bool) {
        let Some(mut ed) = self.notes_ui.editing.take() else {
            return;
        };
        if ed.text != ed.saved {
            let params = json!({ "id": ed.id, "body": ed.text, "continue": ed.saved_once });
            match self.session.run("annotation.edit", &params) {
                Ok(_) => {
                    ed.saved = ed.text.clone();
                    ed.saved_once = true;
                    ed.error = None;
                }
                Err(e) if self.card_gone(&ed.id) => {
                    // Gone underneath (an undo removed it): nothing left to save into.
                    self.toast(e.to_string());
                    return;
                }
                Err(e) => {
                    // Toasted once, since the card (and its message) may be covered.
                    if ed.error.is_none() {
                        self.toast(format!("The Note was not saved: {e}"));
                    }
                    ed.error = Some(e.to_string());
                    // Try again a moment from now, not every frame.
                    ed.changed_at = self.now;
                    self.notes_ui.editing = Some(ed);
                    return;
                }
            }
        }
        if !finished {
            self.notes_ui.editing = Some(ed);
        }
    }

    /// After each frame (the Shell calls it): save the card being edited a moment after the
    /// last keystroke, or at once, ending the editing pass, if the Notes Tab was not drawn
    /// (another Tab covers it, or it closed), since its editor can no longer lose focus.
    pub(super) fn notes_after_frame(&mut self, ctx: &egui::Context) {
        let shown = std::mem::take(&mut self.notes_ui.shown);
        if self.notes_ui.editing.is_none() {
            return;
        }
        // A save that failed is retried on the autosave's clock even while the Tab is hidden.
        let failed = self
            .notes_ui
            .editing
            .as_ref()
            .is_some_and(|e| e.error.is_some());
        if shown || failed {
            self.autosave_card(ctx);
        }
        // Covered, with nothing left unsaved (or a retry just went through): end the pass.
        let unsaved = self
            .notes_ui
            .editing
            .as_ref()
            .is_some_and(|e| e.error.is_some());
        if !shown && !unsaved {
            self.save_card_edit(true);
        }
        // A try that just failed left the card unsaved: ask for the frame of the next one,
        // since a covered Tab and an idle window would otherwise never draw it.
        if let Some(ed) = &self.notes_ui.editing
            && ed.error.is_some()
            && ed.text != ed.saved
        {
            let wait = (AUTOSAVE_SECS - (self.now - ed.changed_at)).max(0.0);
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
        }
    }

    /// The Note being edited no longer exists (an undo removed it, or the User database
    /// closed), as opposed to the database refusing the write.
    fn card_gone(&self, id: &str) -> bool {
        self.session
            .user
            .as_deref()
            .is_none_or(|u| matches!(u.annotation(id), Ok(None)))
    }

    /// Save as the user types: a moment after the last keystroke.
    fn autosave_card(&mut self, ctx: &egui::Context) {
        let Some(ed) = &self.notes_ui.editing else {
            return;
        };
        if ed.text == ed.saved {
            return;
        }
        let wait = AUTOSAVE_SECS - (self.now - ed.changed_at);
        if wait <= 0.0 {
            self.save_card_edit(false);
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
        }
    }

    /// The Notebook column; returns the Export menu's choice, if made.
    fn notebook_column(
        &mut self,
        ui: &mut Ui,
        run: &mut Vec<(&'static str, Value)>,
    ) -> Option<NotesFormat> {
        let filter = self.session.notes.filter.clone();
        let total: u64 = self.notes_ui.counts.values().sum();
        let notebooks = self.notes_ui.notebooks.clone();
        let mut export = None;
        egui::ScrollArea::vertical()
            .id_salt("notes-notebooks")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.label(RichText::new("Notebooks").small().color(theme::c().muted));
                let row = |ui: &mut Ui, selected: bool, name: &str, count: u64| {
                    ui.horizontal(|ui| {
                        let r = ui.add(
                            egui::Button::selectable(selected, name)
                                .min_size(vec2(ui.available_width() - 34.0, 22.0)),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                RichText::new(count.to_string())
                                    .small()
                                    .color(theme::c().muted),
                            );
                        });
                        r
                    })
                    .inner
                };
                ui.horizontal(|ui| {
                    ui.add_space(26.0);
                    let r = row(ui, filter == NotesFilter::All, "All", total);
                    if r.clicked() {
                        run.push(("notes.notebook", json!({ "value": "all" })));
                    }
                });
                // Orphaned annotations (#17 rule 12): only when there are some.
                let attention = filter == NotesFilter::NeedsAttention;
                if self.notes_ui.attention > 0 || attention {
                    ui.horizontal(|ui| {
                        ui.add_space(4.0);
                        ui.label(RichText::new("⚠").color(theme::c().warn));
                        ui.add_space(4.0);
                        let r = row(ui, attention, "Needs attention", self.notes_ui.attention as u64)
                            .on_hover_text(
                                "Annotations whose text was not found after their Resource changed",
                            );
                        if r.clicked() {
                            run.push(("notes.notebook", json!({ "value": "attention" })));
                        }
                    });
                }
                for nb in &notebooks {
                    let count = self.notes_ui.counts.get(&nb.id).copied().unwrap_or(0);
                    let selected = filter == NotesFilter::Notebook(nb.id.clone());
                    ui.horizontal(|ui| {
                        let shown = !self.notes_ui.hidden.contains(&nb.id);
                        let eye = egui::Button::new(RichText::new("👁").color(if shown {
                            theme::c().ink
                        } else {
                            theme::c().rule
                        }))
                        .frame(false)
                        .min_size(vec2(22.0, 22.0));
                        let tip = if shown {
                            "Shown in the text. Click to hide this Notebook's Highlights and Notes."
                        } else {
                            "Hidden in the text. Click to show this Notebook's Highlights and Notes."
                        };
                        if ui.add(eye).on_hover_text(tip).clicked() {
                            run.push((
                                "notebook.show_in_text",
                                json!({ "id": nb.id, "show": !shown }),
                            ));
                        }
                        if let Some((id, text, focused)) = self.notes_ui.renaming.as_mut()
                            && *id == nb.id
                        {
                            let r = ui.add(
                                egui::TextEdit::singleline(text)
                                    .id_salt(("notebook-rename", id.as_str()))
                                    .desired_width(ui.available_width() - 8.0),
                            )
.accessible_name("Notebook name");
                            if !*focused {
                                r.request_focus();
                                *focused = true;
                            }
                            let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
                            if r.lost_focus() {
                                if !escape && text.trim() != nb.name {
                                    run.push((
                                        "notebook.rename",
                                        json!({ "id": nb.id, "name": text.trim() }),
                                    ));
                                }
                                self.notes_ui.renaming = None;
                            }
                            return;
                        }
                        let r = row(ui, selected, &nb.name, count);
                        let r = if nb.is_default {
                            r.on_hover_text("The default Notebook")
                        } else {
                            r
                        };
                        if r.clicked() {
                            run.push(("notes.notebook", json!({ "value": nb.id })));
                        }
                        if r.double_clicked() {
                            self.notes_ui.renaming = Some((nb.id.clone(), nb.name.clone(), false));
                        }
                        r.context_menu(|ui| {
                            if ui.button("Rename…").clicked() {
                                self.notes_ui.renaming =
                                    Some((nb.id.clone(), nb.name.clone(), false));
                                ui.close();
                            }
                            let delete = ui
                                .add_enabled(!nb.is_default, egui::Button::new("Delete…"))
                                .on_disabled_hover_text(
                                    "The default Notebook cannot be deleted; it can be renamed",
                                );
                            if delete.clicked() {
                                self.notes_ui.deleting = Some(DeleteNotebook {
                                    id: nb.id.clone(),
                                    name: nb.name.clone(),
                                    count,
                                    delete: false,
                                });
                                ui.close();
                            }
                        });
                    });
                }
                ui.add_space(4.0);
                match self.notes_ui.creating.as_mut() {
                    Some((text, focused)) => {
                        let r = ui.add(
                            egui::TextEdit::singleline(text)
                                .id_salt("notebook-new")
                                .hint_text("Notebook name")
                                .desired_width(ui.available_width()),
                        )
.accessible_name("Notebook name");
                        if !*focused {
                            r.request_focus();
                            *focused = true;
                        }
                        if r.lost_focus() {
                            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                            if enter && !text.trim().is_empty() {
                                run.push(("notebook.create", json!({ "name": text.trim() })));
                            }
                            self.notes_ui.creating = None;
                        }
                    }
                    None => {
                        if ui
                            .add(egui::Button::new("+ New notebook").frame(false))
                            .clicked()
                        {
                            self.notes_ui.creating = Some((String::new(), false));
                        }
                    }
                }
                ui.add_space(8.0);
                let shown = match &filter {
                    NotesFilter::Notebook(id) => self.notes_ui.counts.get(id).copied().unwrap_or(0),
                    NotesFilter::All | NotesFilter::NeedsAttention => total,
                };
                export = export_menu(ui, "Export…", shown > 0);
            });
        export
    }

    /// Ask what happens to a deleted Notebook's Annotations (#17 rule 19).
    fn delete_notebook_dialog(
        &mut self,
        ctx: &egui::Context,
        run: &mut Vec<(&'static str, Value)>,
    ) {
        let Some(mut d) = self.notes_ui.deleting.take() else {
            return;
        };
        let default = self
            .notes_ui
            .notebooks
            .iter()
            .find(|n| n.is_default)
            .map_or_else(String::new, |n| n.name.clone());
        let mut done: Option<bool> = None;
        egui::Window::new("Delete Notebook")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_max_width(380.0);
                let what = match d.count {
                    0 => "It is empty.".to_string(),
                    1 => "It holds 1 Annotation.".to_string(),
                    n => format!("It holds {n} Annotations."),
                };
                ui.label(format!("Delete the Notebook “{}”? {what}", d.name));
                if d.count > 0 {
                    ui.radio_value(&mut d.delete, false, format!("Move them to “{default}”"));
                    ui.radio_value(&mut d.delete, true, "Delete them");
                }
                ui.label(
                    RichText::new("Ctrl+Z (Edit → Undo) brings it all back.")
                        .small()
                        .color(theme::c().muted),
                );
                ui.horizontal(|ui| {
                    if ui.button("Delete").clicked() {
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
            Some(true) => {
                let annotations = if d.delete { "delete" } else { "move" };
                run.push((
                    "notebook.delete",
                    json!({ "id": d.id, "annotations": annotations }),
                ));
                self.toast(format!(
                    "Deleted the Notebook “{}”. Ctrl+Z to undo.",
                    d.name
                ));
            }
            Some(false) => {}
            None => self.notes_ui.deleting = Some(d),
        }
    }

    /// The order switch, then the virtualized cards.
    fn card_list(&mut self, ui: &mut Ui, tab: TabId, run: &mut Vec<(&'static str, Value)>) {
        let cards = Arc::clone(&self.notes_ui.cards);
        let order = self.session.notes.order;
        ui.horizontal(|ui| {
            for o in [CardOrder::Bible, CardOrder::Modified] {
                if ui.selectable_label(order == o, o.label()).clicked() && order != o {
                    run.push(("notes.order", json!({ "value": o.id() })));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let n = cards.len();
                let what = if n == 1 {
                    "1 Annotation".to_string()
                } else {
                    format!("{n} Annotations")
                };
                ui.label(RichText::new(what).small().color(theme::c().muted));
            });
        });
        ui.add_space(4.0);
        self.notes_ui.drawn.clear();
        if cards.is_empty() {
            let filtered = self
                .tabs
                .get(&tab)
                .is_some_and(|t| !t.filter.trim().is_empty());
            let message = if filtered {
                "Nothing matches the filter"
            } else if self.session.notes.filter == NotesFilter::NeedsAttention {
                "Nothing needs attention: every Annotation is anchored to its text."
            } else if self.session.notes.filter != NotesFilter::All {
                "This Notebook is empty"
            } else {
                "No Notes or Highlights yet. Select text in a Bible, then pick a colour or ✏ Note."
            };
            ui.add_space(30.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(message).color(theme::c().muted));
            });
            return;
        }
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt(("notes-card-scroll", tab))
            .auto_shrink([false, false]);
        if let Some(y) = self.notes_ui.scroll_to.take() {
            scroll = scroll.vertical_scroll_offset(y);
        }
        let Some(mut heights) = self.notes_ui.heights.take() else {
            return;
        };
        scroll.show_viewport(ui, |ui, viewport| {
            let width = ui.available_width();
            let origin = ui.max_rect().min;
            ui.set_min_height(heights.total() as f32);
            let (mut i, _) = heights.at(f64::from(viewport.top().max(0.0)));
            while i < cards.len() {
                let top = heights.offset(i) as f32;
                if top > viewport.bottom() {
                    break;
                }
                let rect = Rect::from_min_size(origin + vec2(0.0, top), vec2(width, 10_000.0));
                let used = ui
                    .scope_builder(
                        UiBuilder::new()
                            .max_rect(rect)
                            .id_salt(("card", &cards[i].annotation.id)),
                        |ui| self.card(ui, &cards[i], run),
                    )
                    .response
                    .rect;
                heights.set(i, used.height() + GAP);
                i += 1;
            }
            ui.set_min_height(heights.total() as f32);
        });
        self.notes_ui.heights = Some(heights);
    }

    fn card(&mut self, ui: &mut Ui, card: &Card, run: &mut Vec<(&'static str, Value)>) {
        let a = &card.annotation;
        let mut body_rect = None;
        let frame = egui::Frame::new()
            .fill(self.palette.page)
            .stroke(Stroke::new(1.0, theme::c().rule))
            .corner_radius(6.0)
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    match a.content {
                        Content::Highlight { style } => {
                            let (rect, _) =
                                ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                            paint_swatch(ui, rect, style, &self.palette, false);
                        }
                        Content::Note { .. } => {
                            ui.label(RichText::new("✏").color(theme::c().muted));
                        }
                    }
                    let reference = RichText::new(&card.reference).strong();
                    if card.followable() {
                        let r = ui
                            .link(reference)
                            .on_hover_text("Go to the text (Ctrl/Cmd-click: in a new Tab)");
                        if r.clicked() {
                            let split = ui.input(|i| i.modifiers.command);
                            run.push(("notes.follow", json!({ "id": a.id, "split": split })));
                        }
                    } else {
                        ui.label(reference);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.menu_button(RichText::new("…").color(theme::c().muted), |ui| {
                            self.card_menu(ui, card, run);
                        })
                        .response
                        .accessible_name("Note menu");
                        ui.label(
                            RichText::new(&card.notebook)
                                .small()
                                .color(theme::c().muted),
                        );
                    });
                });
                if let Some(m) = &card.missing {
                    ui.label(RichText::new(m).small().color(theme::c().warn))
                        .on_hover_text("Its Annotations come back when it is imported again");
                }
                if let Some(m) = &card.attention {
                    ui.label(RichText::new(m).small().color(theme::c().warn));
                }
                if let Some(q) = &card.quote {
                    let width = ui.available_width();
                    self.pipeline_text(ui, &format!("“{q}”"), ColorRole::VerseNumber, &[], width);
                }
                if card.needs_attention() {
                    self.attention_actions(ui, card, run);
                }
                if a.is_note() {
                    body_rect = Some(self.card_body(ui, card, run));
                }
            });
        frame
            .response
            .context_menu(|ui| self.card_menu(ui, card, run));
        self.notes_ui.drawn.push(DrawnCard {
            id: a.id.clone(),
            rect: frame.response.rect,
            body: body_rect,
        });
    }

    /// An Orphaned card's actions (#17 rule 12): Re-attach here, Keep without anchor (Notes
    /// only), Delete.
    fn attention_actions(
        &mut self,
        ui: &mut Ui,
        card: &Card,
        run: &mut Vec<(&'static str, Value)>,
    ) {
        let a = &card.annotation;
        let target = self.reattach_target();
        ui.horizontal(|ui| {
            let hint = match &target {
                Some((_, _, at)) => format!("Anchor it on the text selected in {at}"),
                None => "Select the text in a reading pane first".to_string(),
            };
            let r = ui
                .add_enabled(target.is_some(), egui::Button::new("Re-attach here"))
                .on_hover_text(&hint)
                .on_disabled_hover_text(&hint);
            if r.clicked()
                && let Some((tab, params, _)) = target
            {
                self.notes_ui.reattach = Some((a.id.clone(), tab, params));
            }
            if a.is_note()
                && ui
                    .button("Keep without anchor")
                    .on_hover_text("Keep the Note in its Notebook, not tied to any text")
                    .clicked()
            {
                run.push(("annotation.detach", json!({ "id": a.id })));
            }
            if ui.button("Delete").clicked() {
                if self.notes_ui.editing.as_ref().is_some_and(|e| e.id == a.id) {
                    self.notes_ui.editing = None;
                }
                run.push(("annotation.delete", json!({ "id": a.id })));
            }
        });
    }

    /// A Note card's body: its editor while editing, else its text by the pipeline (Bible
    /// references as links); a click on it starts editing.
    fn card_body(
        &mut self,
        ui: &mut Ui,
        card: &Card,
        run: &mut Vec<(&'static str, Value)>,
    ) -> Rect {
        let a = &card.annotation;
        let editing = self.notes_ui.editing.as_ref().is_some_and(|e| e.id == a.id);
        if editing {
            let now = self.now;
            let mut finished = false;
            let Some(ed) = self.notes_ui.editing.as_mut() else {
                return Rect::NOTHING;
            };
            let r = ui
                .add(
                    egui::TextEdit::multiline(&mut ed.text)
                        .id(egui::Id::unique(("notes-card-edit", a.id.as_str())))
                        .hint_text("Write a note…")
                        .desired_rows(3)
                        .desired_width(f32::INFINITY),
                )
                .accessible_name("Note");
            if ed.focus {
                r.request_focus();
                ed.focus = false;
            }
            if r.changed() {
                ed.changed_at = now;
            }
            if r.has_focus()
                && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter))
            {
                finished = true;
            }
            if r.lost_focus() {
                finished = true;
            }
            ui.label(
                RichText::new(
                    "Saves as you type, and when you click away, press Esc or Ctrl+Enter.",
                )
                .small()
                .color(theme::c().muted),
            );
            let mut discard = false;
            if let Some(e) = &ed.error {
                ui.label(
                    RichText::new(format!("Not saved: {e}. Trying again…"))
                        .small()
                        .color(theme::c().warn),
                );
                discard = ui
                    .button("Discard changes")
                    .on_hover_text("Put back the text last saved")
                    .clicked();
            }
            if discard {
                self.notes_ui.editing = None;
            } else if finished {
                self.save_card_edit(true);
            }
            return r.rect;
        }
        let width = ui.available_width();
        let top = ui.cursor().top();
        let mut link_hit = false;
        if a.body().trim().is_empty() {
            ui.label(
                RichText::new("Empty Note. Click to write.")
                    .italics()
                    .color(theme::c().muted),
            );
        } else {
            let refs = self.notes_ui.body_refs(&a.id, a.body());
            for (line, refs) in a.body().split('\n').zip(refs.iter()) {
                if line.trim().is_empty() {
                    ui.add_space(self.session.text.size * 0.8);
                    continue;
                }
                let styled: Vec<(Range<usize>, ColorRole)> = refs
                    .iter()
                    .map(|(r, _)| (r.clone(), ColorRole::Link))
                    .collect();
                let Some((rect, laid)) =
                    self.pipeline_text(ui, line, ColorRole::Ink, &styled, width)
                else {
                    continue;
                };
                link_hit |= self.body_links(ui, rect, &laid, refs, run);
            }
        }
        let rect = Rect::from_min_max(
            pos2(ui.min_rect().left(), top),
            pos2(ui.min_rect().right(), ui.cursor().top()),
        );
        let r = ui.interact(
            rect,
            ui.scope_id().with(("card-body", a.id.as_str())),
            Sense::click(),
        );
        let r = r.on_hover_cursor(egui::CursorIcon::Text);
        if r.clicked() && !link_hit {
            self.start_card_edit(&a.id, a.body());
        }
        rect
    }

    /// Underline a body line's Bible references; hovering one shows the reference popup and
    /// clicking it shows the Passage (`notes.show_passage`, #18 rule 10; Ctrl/Cmd: a new
    /// Tab). Returns whether the pointer is on a reference.
    fn body_links(
        &mut self,
        ui: &Ui,
        rect: Rect,
        laid: &LaidBlock,
        refs: &[(Range<usize>, PassageList)],
        run: &mut Vec<(&'static str, Value)>,
    ) -> bool {
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let clicked = ui.input(|i| i.pointer.primary_clicked());
        let split = ui.input(|i| i.modifiers.command);
        let mut on = false;
        for (range, list) in refs {
            let rects: Vec<Rect> = crate::compare_view::range_rects(laid, range)
                .into_iter()
                .map(|[x0, y0, x1, y1]| {
                    Rect::from_min_max(rect.min + vec2(x0, y0), rect.min + vec2(x1, y1))
                })
                .collect();
            for r in &rects {
                ui.painter().hline(
                    r.x_range(),
                    r.bottom() - 2.0,
                    Stroke::new(1.0, self.palette.link.gamma_multiply(0.6)),
                );
            }
            if pointer.is_some_and(|p| rects.iter().any(|r| r.contains(p)))
                && ui.rect_contains_pointer(rect)
            {
                on = true;
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                self.reference_popup(ui, std::slice::from_ref(list));
                if clicked && let Some(p) = list.passages().first() {
                    let passage = format_passage(*p, &Standard, RefStyle::Short);
                    run.push((
                        "notes.show_passage",
                        json!({ "passage": passage, "split": split }),
                    ));
                }
            }
        }
        on
    }

    /// Draw `text` with the text pipeline (glyph atlas), `width` wide; `None` (space kept)
    /// until a layout worker has it.
    fn pipeline_text(
        &mut self,
        ui: &mut Ui,
        text: &str,
        base: ColorRole,
        styled: &[(Range<usize>, ColorRole)],
        width: f32,
    ) -> Option<(Rect, Arc<LaidBlock>)> {
        let settings = self.session.text;
        let laid = self.notes_ui.snippets.get_styled(
            text,
            base,
            styled,
            false,
            width.max(40.0),
            &settings,
        );
        let Some(laid) = laid else {
            ui.allocate_space(vec2(width, settings.size * 1.6));
            ui.ctx().request_repaint();
            return None;
        };
        let (rect, response) = ui.allocate_exact_size(vec2(width, laid.height), Sense::hover());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text));
        if ui.is_rect_visible(rect) {
            let quads = self.atlas.atlas.quads(
                &laid,
                [rect.left(), rect.top()],
                ui.ctx().pixels_per_point(),
            );
            let mut mesh = Mesh::with_texture(self.atlas.sync(ui.ctx()));
            for q in quads {
                mesh.add_rect_with_uv(
                    Rect::from_min_max(Pos2::from(q.min), Pos2::from(q.max)),
                    Rect::from_min_max(Pos2::from(q.uv_min), Pos2::from(q.uv_max)),
                    self.palette.color(q.role),
                );
            }
            ui.painter().add(Shape::mesh(mesh));
        }
        Some((rect, laid))
    }

    /// A card's … and right-click menu: go to the text, move, restyle, delete.
    fn card_menu(&mut self, ui: &mut Ui, card: &Card, run: &mut Vec<(&'static str, Value)>) {
        let a = &card.annotation;
        if card.followable() {
            if ui.button("Go to text").clicked() {
                run.push(("notes.follow", json!({ "id": a.id })));
                ui.close();
            }
            if ui.button("Go to text in a new Tab").clicked() {
                run.push(("notes.follow", json!({ "id": a.id, "split": true })));
                ui.close();
            }
            ui.separator();
        }
        let notebooks = self.notes_ui.notebooks.clone();
        ui.menu_button("Move to Notebook", |ui| {
            for nb in notebooks.iter().filter(|n| n.id != a.notebook_id) {
                if ui.button(&nb.name).clicked() {
                    run.push(("annotation.move", json!({ "id": a.id, "notebook": nb.id })));
                    ui.close();
                }
            }
            if notebooks.len() < 2 {
                ui.label(RichText::new("No other Notebook").color(theme::c().muted));
            }
        });
        if let Content::Highlight { style } = a.content {
            let palette = self.palette;
            ui.menu_button("Highlight style", |ui| {
                for s in HighlightStyle::ALL {
                    if ui
                        .add_enabled(s != style, egui::Button::new(menu_label(ui, s, &palette)))
                        .clicked()
                    {
                        run.push(("annotation.style", json!({ "id": a.id, "style": s.id() })));
                        ui.close();
                    }
                }
            });
        }
        ui.separator();
        let label = if a.is_note() {
            "Delete Note"
        } else {
            "Delete Highlight"
        };
        if ui.button(label).clicked() {
            if self.notes_ui.editing.as_ref().is_some_and(|e| e.id == a.id) {
                self.notes_ui.editing = None;
            }
            run.push(("annotation.delete", json!({ "id": a.id })));
            ui.close();
        }
    }
}

/// The Export menu of the Notes Tool and of a Notes search (#154): Markdown, HTML, or
/// Print / Save as PDF. Returns the format chosen.
pub(super) fn export_menu(ui: &mut Ui, label: &str, enabled: bool) -> Option<NotesFormat> {
    let mut chosen = None;
    ui.add_enabled_ui(enabled, |ui| {
        ui.menu_button(label, |ui| {
            for (format, item) in [
                (NotesFormat::Markdown, "Export as Markdown…"),
                (NotesFormat::Html, "Export as HTML…"),
                (NotesFormat::Print, "Print / Save as PDF…"),
            ] {
                if ui.button(item).clicked() {
                    chosen = Some(format);
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text("Export these Annotations with their Passages and quoted text");
    });
    chosen
}

impl State {
    /// Export (asking where to save) or print the Notes `params` choose.
    pub(super) fn export_or_print(
        &mut self,
        ctx: &egui::Context,
        params: Value,
        format: NotesFormat,
    ) {
        match format {
            NotesFormat::Print => self.print_notes(ctx, params),
            NotesFormat::Markdown | NotesFormat::Html => self.export_notes(ctx, params, format),
        }
    }
}
