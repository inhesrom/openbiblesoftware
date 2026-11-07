//! Drag and drop across the Workspace (#216), besides docking Tabs (egui_tiles) and Resources
//! dragged out of the Library launcher's pane (`toolbar.rs`, `library_drag`).
//!
//! - **From text:** a drag that starts on a Scripture reference carries its Passage, and the
//!   reference as written for a Search Tab; one that
//!   starts inside the selection carries the selected text (a word, or a reference written
//!   plainly). `pane.rs` tells the drag apart from selecting ([`crate::pane::DragOut`]).
//! - **A Library Tab row** carries its Resource: it opens in the Panel it is dropped on, or in a
//!   new Panel beside it (`toolbar.open_resource`, as from the Library launcher).
//! - **A Note card** is dragged by its Note icon (the grip) onto a Resource's text: on
//!   confirmation it is re-anchored there (`annotation.reattach`, one undo step), on the
//!   selection when it was dropped inside it, else on the Verse (in a Bible) or word under the
//!   pointer. Only text takes it, wherever it is (the Panel's right quarter too): not the
//!   pane's margins, the Tab's header or Contents.
//! - **Drop targets** (CONTEXT.md), highlighted while dragging: a Tab's text (it goes there:
//!   `workspace.drop` with `tab`), a Panel's tab bar (a new Tab in it), a Panel's right edge
//!   (a new Panel beside it). Dropped back on the Tab it came from, nothing happens; Esc
//!   cancels. A Note drops only on a Resource's text.
//!
//! Each window's dock is its own: a drag drops in the OS window it started in.

use egui::{Pos2, Rect, RichText, Ui, pos2, vec2};
use egui_tiles::Tree;
use obs_engine::{PanelId, TabId, TabKind, Tool};
use obs_model::ResourceType;
use obs_scripture::{RefStyle, Standard, format_passage};
use serde_json::{Value, json};

use super::annotate::selection_params;
use super::references::TargetKey;
use super::toolbar_look as look;
use super::{PanelTiles, State};
use crate::pane::{DragOut, has_text};
use crate::selection::{Granularity, Selection};
use crate::theme;

/// A Panel's tab bar (`Behavior::tab_bar_height`).
const TAB_BAR: f32 = 31.0;

/// What is being carried across the Workspace.
#[derive(Debug, Clone)]
pub(crate) enum Dragged {
    /// A Scripture reference in a Resource's text; its Passage comes from the target cache.
    Reference {
        key: TargetKey,
        label: String,
        from: TabId,
        bible: Option<String>,
    },
    /// Selected text, from Tab `from`.
    Text {
        text: String,
        from: TabId,
        bible: Option<String>,
    },
    /// A Note card, by the Note's id, with the reference it is anchored at.
    Note { id: String, reference: String },
    /// A Resource, by its id, dragged out of Library Tab `from`.
    Resource {
        id: String,
        abbreviation: String,
        from: TabId,
    },
}

impl Dragged {
    /// What the ghost following the pointer reads.
    fn label(&self) -> String {
        match self {
            Dragged::Reference { label, .. } => label.clone(),
            Dragged::Text { text, .. } => text.clone(),
            Dragged::Note { reference, .. } => format!("Note: {reference}"),
            Dragged::Resource { abbreviation, .. } => abbreviation.clone(),
        }
    }

    fn from(&self) -> Option<TabId> {
        match self {
            Dragged::Reference { from, .. }
            | Dragged::Text { from, .. }
            | Dragged::Resource { from, .. } => Some(*from),
            Dragged::Note { .. } => None,
        }
    }
}

/// Where a drop lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DropTarget {
    /// The Tab's text.
    Tab(TabId),
    /// The Panel's tab bar: a new Tab in it.
    Panel(PanelId),
    /// The Panel's right edge: a new Panel beside it.
    Beside(PanelId),
}

impl DropTarget {
    fn params(self) -> Value {
        match self {
            DropTarget::Tab(t) => json!({ "tab": t.0 }),
            DropTarget::Panel(p) => json!({ "panel": p.0 }),
            DropTarget::Beside(p) => json!({ "beside": p.0 }),
        }
    }
}

/// A Note dropped on text, waiting for the user to confirm the move.
#[derive(Debug, Clone)]
pub(crate) struct Reanchor {
    pub(crate) id: String,
    /// The Tab it was dropped on: its window shows the box.
    pub(crate) tab: TabId,
    params: Value,
    pub(crate) from: String,
    pub(crate) to: String,
}

/// The drag in progress, and what a drop left to finish.
#[derive(Default)]
pub(crate) struct DragDropUi {
    /// What is being dragged, and the OS window it started in.
    pub(crate) dragging: Option<(Dragged, egui::ViewportId)>,
    /// A dropped reference whose Passage the worker had not yet worked out.
    pending: Option<(TargetKey, DropTarget, Option<String>)>,
    /// The Move Note confirmation.
    pub(crate) confirm: Option<Reanchor>,
}

impl DragDropUi {
    /// A dropped reference is waiting for its Passage.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn has_pending(&self) -> bool {
        self.pending.is_some()
    }
}

/// One Panel of a window's dock as drawn this frame.
pub(super) struct DropPanel {
    panel: PanelId,
    rect: Rect,
    tab: Option<TabId>,
}

/// The Panels of `tree` (a window's dock), with their front Tabs.
pub(super) fn drop_panels(
    tree: &Tree<TabId>,
    panels: &PanelTiles,
    workspace: &obs_engine::Workspace,
) -> Vec<DropPanel> {
    panels
        .iter()
        .filter_map(|(tile, panel)| {
            Some(DropPanel {
                panel: *panel,
                rect: tree.tiles.rect(*tile)?,
                tab: workspace.active_in(*panel),
            })
        })
        .collect()
}

/// The Drop target under `p` and the area to highlight.
fn target_at(panels: &[DropPanel], p: Pos2) -> Option<(DropTarget, Rect)> {
    let d = panels.iter().find(|d| d.rect.contains(p))?;
    let r = d.rect;
    let edge = r.right() - r.width() * super::toolbar::DROP_EDGE;
    let bar = r.top() + TAB_BAR;
    Some(match d.tab {
        _ if p.x >= edge => (
            DropTarget::Beside(d.panel),
            Rect::from_min_max(pos2(edge, r.top()), r.max),
        ),
        Some(tab) if p.y >= bar => (
            DropTarget::Tab(tab),
            Rect::from_min_max(pos2(r.left(), bar), pos2(edge, r.bottom())),
        ),
        _ => (
            DropTarget::Panel(d.panel),
            Rect::from_min_max(r.min, pos2(edge, bar)),
        ),
    })
}

/// What a reference's ghost reads: the text it is written as, else its first target.
fn reference_label(doc: &crate::ReadingDoc, unit: usize, span: usize) -> String {
    let Some(u) = doc.units.get(unit) else {
        return String::new();
    };
    let Some(s) = u.spans.get(span) else {
        return String::new();
    };
    let written = u.text.get(s.range()).unwrap_or_default().trim();
    if !written.is_empty() {
        return written.to_string();
    }
    match &s.kind {
        obs_model::SpanKind::ScriptureRef { targets } | obs_model::SpanKind::Xref { targets } => {
            targets
                .passages()
                .first()
                .map(|p| format_passage(*p, &Standard, RefStyle::Short))
                .unwrap_or_default()
        }
        _ => String::new(),
    }
}

/// At most `n` characters of `text` on one line, with "…" when cut.
fn one_line(text: &str, n: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= n {
        return flat;
    }
    let mut cut: String = flat.chars().take(n).collect();
    cut.push('…');
    cut
}

impl State {
    /// A drag started on Tab `tab`'s text this frame and carries `out` out of its pane.
    pub(super) fn text_drag_started(&mut self, ui: &Ui, tab: TabId, out: DragOut) {
        let Some(view) = self.tabs.get(&tab).and_then(|t| t.view.as_ref()) else {
            return;
        };
        let doc = &view.doc;
        let bible = (doc.manifest.resource_type == ResourceType::Bible)
            .then(|| doc.manifest.resource_id.clone());
        let what = match out {
            DragOut::Reference { unit, span } => {
                let key = TargetKey::new(doc, unit, span);
                let label = reference_label(doc, unit, span);
                // Its Passage is worked out now, to be ready by the drop.
                let _ = self.references.targets.get(key.clone());
                Dragged::Reference {
                    key,
                    label,
                    from: tab,
                    bible,
                }
            }
            DragOut::Selection => {
                let Some(sel) = view.selection else {
                    return;
                };
                let text = crate::selection::selected_text(doc, &sel);
                let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                if text.is_empty() {
                    return;
                }
                Dragged::Text {
                    text,
                    from: tab,
                    bible,
                }
            }
        };
        self.drag.dragging = Some((what, ui.ctx().viewport_id()));
    }

    /// A row of Library Tab `tab` started dragging Resource `id` out of it.
    pub(super) fn resource_drag_started(
        &mut self,
        ui: &Ui,
        tab: TabId,
        id: String,
        abbreviation: String,
    ) {
        self.drag.dragging = Some((
            Dragged::Resource {
                id,
                abbreviation,
                from: tab,
            },
            ui.ctx().viewport_id(),
        ));
    }

    /// A Note card's grip started a drag.
    pub(super) fn note_drag_started(&mut self, ui: &Ui, id: &str, reference: &str) {
        self.drag.dragging = Some((
            Dragged::Note {
                id: id.to_string(),
                reference: reference.to_string(),
            },
            ui.ctx().viewport_id(),
        ));
    }

    /// After a window's dock (its Panels, `panels`) is drawn: what is dragged out of a reading
    /// pane or a Notes card follows the pointer, the Drop target under it is highlighted, and
    /// the release drops it there.
    pub(super) fn workspace_drag(&mut self, ui: &Ui, panels: &[DropPanel]) {
        self.pending_drop(ui);
        let Some((what, viewport)) = &self.drag.dragging else {
            return;
        };
        if *viewport != ui.ctx().viewport_id() {
            return;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.drag.dragging = None;
            return;
        }
        let pos = ui.input(|i| i.pointer.latest_pos());
        let released = ui.input(|i| i.pointer.any_released() || !i.pointer.any_down());
        let target = pos.and_then(|p| match what {
            Dragged::Note { .. } => self.note_target_at(panels, p),
            _ => target_at(panels, p).filter(|(t, _)| self.takes(what, *t)),
        });
        if released {
            if let (Some((what, _)), Some((target, _)), Some(p)) =
                (self.drag.dragging.take(), target, pos)
            {
                self.dropped(what, target, p);
            }
            self.drag.dragging = None;
            return;
        }
        if let Some(p) = pos {
            look::drag_ghost(ui.ctx(), p, &one_line(&what.label(), 40));
        }
        if let Some((_, rect)) = target {
            look::drop_target(ui.ctx(), rect);
        }
        ui.ctx().request_repaint();
    }

    /// Whether `what` (not a Note) can drop on `target`: never back on the Tab it came from.
    fn takes(&self, what: &Dragged, target: DropTarget) -> bool {
        match target {
            DropTarget::Tab(t) => what.from() != Some(t),
            _ => true,
        }
    }

    /// Where a Note can drop with the pointer at `p`: only on a Resource's text, wherever it
    /// is under the pointer (the Panel's right quarter included), and the Panel's body to
    /// highlight.
    fn note_target_at(&self, panels: &[DropPanel], p: Pos2) -> Option<(DropTarget, Rect)> {
        let d = panels.iter().find(|d| d.rect.contains(p))?;
        let tab = d.tab?;
        let resource = self
            .session
            .workspace
            .tab(tab)
            .is_some_and(|t| matches!(t.kind, TabKind::Resource(_)));
        (resource && self.text_under(tab, p).is_some()).then(|| {
            let body = Rect::from_min_max(pos2(d.rect.left(), d.rect.top() + TAB_BAR), d.rect.max);
            (DropTarget::Tab(tab), body)
        })
    }

    fn dropped(&mut self, what: Dragged, target: DropTarget, p: Pos2) {
        match what {
            Dragged::Reference {
                key, label, bible, ..
            } => {
                // A Search Tab searches for the reference as written, as typed in its box,
                // even one with no Standard equivalent: no need to wait for its Passage.
                if let DropTarget::Tab(t) = target
                    && !label.is_empty()
                    && self
                        .session
                        .workspace
                        .tab(t)
                        .is_some_and(|t| t.kind == TabKind::Tool(Tool::Search))
                {
                    self.run("workspace.drop", &json!({ "tab": t.0, "text": label }));
                    return;
                }
                self.drag.pending = Some((key, target, bible));
            }
            Dragged::Text { text, from, bible } => {
                let mut params = target.params();
                params["text"] = json!(text);
                // A Bible reference selected in a Bible is in its numbering.
                params["from"] = json!(from.0);
                if let Some(b) = bible {
                    params["bible"] = json!(b);
                }
                self.run("workspace.drop", &params);
            }
            Dragged::Note { id, reference } => {
                if let DropTarget::Tab(tab) = target {
                    self.note_dropped(id, reference, tab, p);
                }
            }
            Dragged::Resource { id, .. } => {
                // On a Tab's text it opens in that Tab's Panel, as on its tab bar.
                let params = match target {
                    DropTarget::Tab(t) => match self.session.workspace.panel_of(t) {
                        Some(panel) => json!({ "id": id, "panel": panel.0 }),
                        None => return,
                    },
                    DropTarget::Panel(panel) => json!({ "id": id, "panel": panel.0 }),
                    DropTarget::Beside(panel) => json!({ "id": id, "beside": panel.0 }),
                };
                self.run("toolbar.open_resource", &params);
            }
        }
    }

    /// A dropped reference goes where it was dropped once its Passage is known.
    fn pending_drop(&mut self, ui: &Ui) {
        let Some((key, target, bible)) = self.drag.pending.clone() else {
            return;
        };
        let Some(result) = self.references.targets.get(key) else {
            ui.ctx().request_repaint();
            return;
        };
        self.drag.pending = None;
        let passage = match result {
            Ok(targets) => targets.iter().flat_map(|p| p.iter()).next().copied(),
            Err(e) => {
                self.toast(e);
                return;
            }
        };
        let Some(passage) = passage else {
            self.toast("This reference has no Standard equivalent");
            return;
        };
        let mut params = target.params();
        params["passage"] = json!(format_passage(passage, &Standard, RefStyle::Long));
        if let Some(b) = bible {
            params["bible"] = json!(b);
        }
        self.run("workspace.drop", &params);
    }

    /// The text of Tab `tab`'s reading pane under the screen point `p`, if the Tab shows a
    /// Resource without an error.
    fn text_under(&self, tab: TabId, p: Pos2) -> Option<crate::selection::DocPos> {
        let t = self.tabs.get(&tab).filter(|t| t.error.is_none())?;
        t.view.as_ref()?.text_under(p.x, p.y)
    }

    /// A Note dropped at `p` on Tab `tab`'s text: where it would go, to confirm.
    fn note_dropped(&mut self, id: String, reference: String, tab: TabId, p: Pos2) {
        let Some(at) = self.text_under(tab, p) else {
            return;
        };
        let Some(view) = self.tabs.get(&tab).and_then(|t| t.view.as_ref()) else {
            return;
        };
        let doc = &view.doc;
        let bible = !doc.verses.is_empty();
        let sel = view
            .selection
            .filter(|s| has_text(view, s) && s.contains(doc, at))
            .unwrap_or(Selection {
                anchor: at,
                head: at,
                granularity: if bible {
                    Granularity::Verse
                } else {
                    Granularity::Word
                },
            });
        let Some(params) = selection_params(doc, &sel) else {
            return;
        };
        let to = if bible {
            crate::selection::copy_text(doc, &sel)
                .lines()
                .last()
                .unwrap_or_default()
                .to_string()
        } else {
            format!(
                "“{}” ({})",
                one_line(&crate::selection::selected_text(doc, &sel), 40),
                doc.manifest.abbreviation
            )
        };
        self.confirm_reanchor(id, reference, tab, params, to);
    }

    /// Ask before moving Note `id` (anchored at `from`) onto the text of Tab `tab` that
    /// `params` select (`to`): the Move Note box.
    pub(super) fn confirm_reanchor(
        &mut self,
        id: String,
        from: String,
        tab: TabId,
        params: Value,
        to: String,
    ) {
        self.drag.confirm = Some(Reanchor {
            id,
            tab,
            params,
            from,
            to,
        });
    }

    /// The Move Note confirmation: Move re-anchors it (Edit → Undo moves it back).
    pub(super) fn reanchor_prompt(&mut self, ctx: &egui::Context) {
        let Some(r) = self.drag.confirm.take() else {
            return;
        };
        let mut done: Option<bool> = None;
        egui::Window::new("Move Note")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_max_width(420.0);
                ui.label(format!("Move this Note from {} to {}?", r.from, r.to));
                ui.label(
                    RichText::new("It keeps its text and Notebook.")
                        .small()
                        .color(theme::c().muted),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Move").clicked() {
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
            None => self.drag.confirm = Some(r),
            Some(false) => {}
            Some(true) => {
                let mut params = r.params.clone();
                params["id"] = json!(r.id);
                if self.run("annotation.reattach", &params).is_some() {
                    if let Some(view) = self.tabs.get_mut(&r.tab).and_then(|t| t.view.as_mut()) {
                        view.selection = None;
                    }
                    let undo =
                        super::with_key(&self.session, "Edit → Undo moves it back", "edit.undo");
                    self.toast(format!("Moved the Note to {}. {undo}.", r.to));
                }
            }
        }
    }
}
