//! Drag and drop across the Workspace (#216), besides docking Tabs (egui_tiles) and Resources
//! dragged out of the Library launcher or the Library Tab (`toolbar.rs`, `library_drag`).
//!
//! - **From text:** a drag that starts on a Scripture reference carries its Passage; one that
//!   starts inside the selection carries the selected text (a word, or a reference written
//!   plainly). `pane.rs` tells the drag apart from selecting ([`crate::pane::DragOut`]).
//! - **A Note card** is dragged by its ✏ grip onto a Resource's text: on confirmation it is
//!   re-anchored there (`annotation.reattach`, one undo step), on the selection when it was
//!   dropped inside it, else on the Verse (in a Bible) or word under the pointer.
//! - **Drop targets** (CONTEXT.md), highlighted while dragging: a Tab's text (it goes there:
//!   `workspace.drop` with `tab`), a Panel's tab bar (a new Tab in it), a Panel's right edge
//!   (a new Panel beside it). Dropped back on the Tab it came from, nothing happens; Esc
//!   cancels. A Note drops only on a Resource's text.
//!
//! Each window's dock is its own: a drag drops in the OS window it started in.

use egui::{Pos2, Rect, RichText, Ui, pos2, vec2};
use egui_tiles::Tree;
use obs_engine::{PanelId, TabId, TabKind};
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
    /// A Note card, by the Note's id, labelled with its reference.
    Note { id: String, label: String },
}

impl Dragged {
    fn label(&self) -> &str {
        match self {
            Dragged::Reference { label, .. } | Dragged::Note { label, .. } => label,
            Dragged::Text { text, .. } => text,
        }
    }

    fn from(&self) -> Option<TabId> {
        match self {
            Dragged::Reference { from, .. } | Dragged::Text { from, .. } => Some(*from),
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
    tab: TabId,
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

    /// A Note card's grip started a drag.
    pub(super) fn note_drag_started(&mut self, ui: &Ui, id: &str, label: &str) {
        self.drag.dragging = Some((
            Dragged::Note {
                id: id.to_string(),
                label: format!("✏ {label}"),
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
        let target = pos
            .and_then(|p| target_at(panels, p))
            .filter(|(t, _)| self.takes(what, *t));
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
            look::drag_ghost(ui.ctx(), p, &one_line(what.label(), 40));
        }
        if let Some((_, rect)) = target {
            look::drop_target(ui.ctx(), rect);
        }
        ui.ctx().request_repaint();
    }

    /// Whether `what` can drop on `target`: never back on the Tab it came from; a Note only
    /// on a Resource's text.
    fn takes(&self, what: &Dragged, target: DropTarget) -> bool {
        match (what, target) {
            (Dragged::Note { .. }, DropTarget::Tab(t)) => {
                self.tabs
                    .get(&t)
                    .is_some_and(|t| t.view.is_some() && t.error.is_none())
                    && self
                        .session
                        .workspace
                        .tab(t)
                        .is_some_and(|t| matches!(t.kind, TabKind::Resource(_)))
            }
            (Dragged::Note { .. }, _) => false,
            (_, DropTarget::Tab(t)) => what.from() != Some(t),
            _ => true,
        }
    }

    fn dropped(&mut self, what: Dragged, target: DropTarget, p: Pos2) {
        match what {
            Dragged::Reference { key, bible, .. } => {
                self.drag.pending = Some((key, target, bible));
            }
            Dragged::Text { text, bible, .. } => {
                let mut params = target.params();
                params["text"] = json!(text);
                if let Some(b) = bible {
                    params["bible"] = json!(b);
                }
                self.run("workspace.drop", &params);
            }
            Dragged::Note { id, label } => {
                if let DropTarget::Tab(tab) = target {
                    self.note_dropped(id, label, tab, p);
                }
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

    /// A Note dropped at `p` on Tab `tab`'s text: where it would go, to confirm.
    fn note_dropped(&mut self, id: String, label: String, tab: TabId, p: Pos2) {
        let Some(view) = self.tabs.get_mut(&tab).and_then(|t| t.view.as_mut()) else {
            return;
        };
        let [ox, oy] = view.text_origin;
        let Some(at) = view.pos_at(p.x - ox, p.y - oy) else {
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
        let from = label.trim_start_matches("✏ ").to_string();
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
