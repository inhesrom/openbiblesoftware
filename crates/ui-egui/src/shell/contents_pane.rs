//! The Contents pane (#176): a Resource Tab's outline beside its text. A Bible's books and
//! chapters, a Commentary's volumes, books, chapters and sections, a Lexicon's or
//! Dictionary's entries A–Z with a filter box. The line of the Current location is
//! highlighted and kept in view as the text scrolls; clicking a line jumps the Tab (and its
//! Link set) there.
//!
//! The outline ([`obs_engine::contents::Contents`]) is built once per Resource on a worker,
//! the first time a pane of it opens, and shared by every Tab showing that Resource.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Range;
use std::sync::{Arc, Weak};

use egui::{Sense, Ui, vec2};
use obs_engine::contents::{Contents, ContentsTarget};
use obs_engine::{JobEventKind, JobId, JobQueue, TabId};
use obs_model::ResourceType;

use super::State;
use crate::doc::ReadingDoc;
use crate::icons::{self, Icon};
use crate::theme;
use crate::tokens::{radius, share, size, space, stroke};

/// Height of one line of the outline.
const ROW_H: f32 = size::ROW_COMPACT;
/// Indent per level of the outline.
const INDENT: f32 = space::L;

/// How wide the pane is beside text `width` wide: about a third, within limits, never over
/// half.
pub(super) fn pane_width(width: f32) -> f32 {
    (width * share::SIDE_PANE)
        .clamp(size::FIELD, size::FIELD_WIDE)
        .min(width * 0.5)
}

/// Each Resource's Contents, by resource id, with the document it was built from (a new
/// revision is a new document, so its Contents is built again).
#[derive(Default)]
pub(crate) struct ContentsCache {
    jobs: Option<JobQueue>,
    pending: BTreeMap<JobId, (String, Weak<ReadingDoc>)>,
    ready: HashMap<String, (Weak<ReadingDoc>, Arc<Contents>)>,
    /// Tests: builds wait, so the Contents pane stays loading (#223's loading state).
    #[cfg(test)]
    pub(crate) hold: bool,
}

impl ContentsCache {
    /// `doc`'s Contents, or `None` while a worker builds it (the build starts on first ask).
    /// `threaded`: build on a worker thread; else the build runs here, a step a frame (wasm,
    /// tests without threads).
    pub(crate) fn get(&mut self, doc: &Arc<ReadingDoc>, threaded: bool) -> Option<Arc<Contents>> {
        let id = &doc.manifest.resource_id;
        if let Some((built, contents)) = self.ready.get(id)
            && built.as_ptr() == Arc::as_ptr(doc)
        {
            return Some(Arc::clone(contents));
        }
        let jobs = self.jobs.get_or_insert_with(|| {
            if threaded && !cfg!(target_arch = "wasm32") {
                JobQueue::threaded(1)
            } else {
                JobQueue::inline()
            }
        });
        #[cfg(test)]
        let held = self.hold;
        #[cfg(not(test))]
        let held = false;
        if !jobs.is_threaded() && !held {
            jobs.pump(1);
        }
        for event in jobs.drain_events() {
            if !event.kind.is_terminal() {
                continue;
            }
            let Some((resource, built)) = self.pending.remove(&event.id) else {
                continue;
            };
            if let JobEventKind::Finished(value) = event.kind
                && let Ok(contents) = value.downcast::<Contents>()
            {
                self.ready.insert(resource, (built, Arc::new(*contents)));
            }
        }
        // Resources closed or replaced since are dropped.
        self.ready.retain(|_, (built, _)| built.strong_count() > 0);
        if let Some((built, contents)) = self.ready.get(id)
            && built.as_ptr() == Arc::as_ptr(doc)
        {
            return Some(Arc::clone(contents));
        }
        let building = self
            .pending
            .values()
            .any(|(_, built)| built.as_ptr() == Arc::as_ptr(doc));
        if !building {
            let source = Arc::clone(doc);
            let job = jobs.submit_fn("Contents", move |_| {
                Ok::<_, String>(Contents::build(
                    source.manifest.resource_type,
                    &source.verses,
                    &source.units,
                    &source.navigation,
                    &source.volumes,
                ))
            });
            self.pending.insert(job, (id.clone(), Arc::downgrade(doc)));
        }
        None
    }
}

/// One Tab's Contents pane state.
#[derive(Default)]
pub(crate) struct ContentsUi {
    /// The filter box's text (Lexicons and Dictionaries).
    pub(crate) filter: String,
    /// Lines whose children show.
    pub(crate) expanded: BTreeSet<usize>,
    /// The highlighted line: the Current location's, as of last frame.
    pub(crate) current: Option<usize>,
    /// Bring the highlighted line into view this frame.
    scroll_to: bool,
    /// Which rows of the list were in view last frame.
    in_view: Range<usize>,
    /// Where the pane was drawn last frame.
    pub(crate) rect: Option<egui::Rect>,
    /// Where the header's Contents button was drawn last frame.
    pub(crate) button: Option<egui::Rect>,
}

/// The lines shown: the top-level ones, and the children of every expanded line.
fn visible_rows(contents: &Contents, expanded: &BTreeSet<usize>) -> Vec<usize> {
    let mut out = Vec::new();
    let mut stack: Vec<usize> = contents.roots().iter().rev().copied().collect();
    while let Some(n) = stack.pop() {
        out.push(n);
        if expanded.contains(&n) {
            stack.extend(contents.node(n).children.iter().rev());
        }
    }
    out
}

impl State {
    /// The header's Contents button, at its left like a sidebar's: an outline icon that
    /// shows or hides the pane, filled while the pane shows.
    pub(super) fn contents_button(&mut self, ui: &mut Ui, tab: TabId) {
        let on = self.session.workspace.tab(tab).is_some_and(|t| t.contents);
        let (rect, r) =
            ui.allocate_exact_size(egui::Vec2::splat(size::CONTROL_COMPACT), Sense::click());
        let visuals = ui.visuals();
        if on {
            ui.painter()
                .rect_filled(rect, radius::S, visuals.selection.bg_fill);
        } else if r.hovered() {
            ui.painter()
                .rect_filled(rect, radius::S, visuals.widgets.hovered.weak_bg_fill);
        }
        let color = if on || r.hovered() {
            theme::c().lapis
        } else {
            theme::c().muted
        };
        icons::paint(ui.painter(), Icon::Contents, rect, icons::BODY, color);
        r.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, "Contents"));
        if let Some(t) = self.tabs.get_mut(&tab) {
            t.contents.button = Some(rect);
        }
        if r.on_hover_text("Contents: the table of contents beside the text")
            .clicked()
        {
            self.session.workspace.user_focused(tab);
            self.run("tab.contents", &serde_json::json!({ "tab": tab.0 }));
        }
    }

    pub(super) fn contents_pane(&mut self, ui: &mut Ui, tab: TabId) {
        let rect = ui.max_rect();
        if let Some(t) = self.tabs.get_mut(&tab) {
            t.contents.rect = Some(rect);
        }
        ui.painter().rect_filled(rect, 0.0, theme::c().surface);
        ui.painter().vline(
            rect.right() - stroke::HAIR / 2.0,
            rect.y_range(),
            crate::components::rule_stroke(),
        );
        let inner = rect.shrink2(vec2(space::S, space::XS));
        // Its own scope, so the list keeps where it is scrolled when the Tab moves Panels.
        let mut ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .scope_id(egui::Id::unique(("contents-pane", tab))),
        );
        let ui = &mut ui;
        let Some(doc) = self
            .tabs
            .get(&tab)
            .and_then(|t| t.view.as_ref())
            .map(|v| Arc::clone(&v.doc))
        else {
            return;
        };
        let threaded = self.session.jobs().is_threaded();
        let Some(contents) = self.contents.get(&doc, threaded) else {
            crate::components::loading_state(ui, "Loading contents…");
            return;
        };
        if contents.is_empty() {
            if crate::components::empty_state(
                ui,
                "This Resource has no contents to list.",
                "Close Contents",
            )
            .clicked()
            {
                self.run("tab.contents", &serde_json::json!({ "tab": tab.0 }));
            }
            return;
        }
        let Some(t) = self.tabs.get_mut(&tab) else {
            return;
        };
        // The Current location's line, as the view stands after this frame's scrolling.
        let current = t.view.as_mut().and_then(|view| {
            if doc.manifest.resource_type == ResourceType::Bible {
                view.current_location().and_then(|v| contents.at_verse(v))
            } else {
                let unit = view.top_unit()?;
                contents.at_unit(&doc.units.get(unit)?.id)
            }
        });
        let state = &mut t.contents;
        if current != state.current {
            state.current = current;
            if let Some(line) = current {
                state.expanded.extend(contents.ancestors(line));
                state.scroll_to = true;
            }
        }
        let mut go: Option<ContentsTarget> = None;
        if contents.is_filterable() {
            let r = ui.add(
                egui::TextEdit::singleline(&mut state.filter)
                    .id_salt(("contents-filter", tab))
                    .hint_text("Filter A–Z")
                    .desired_width(f32::INFINITY),
            );
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                // Enter opens the first match.
                go = contents
                    .filter(&state.filter)
                    .first()
                    .and_then(|&n| contents.node(n).target.clone());
            }
            ui.add_space(space::XXS);
        }
        let filtering = !state.filter.trim().is_empty();
        let rows = if filtering {
            contents.filter(&state.filter)
        } else {
            visible_rows(&contents, &state.expanded)
        };
        if filtering
            && rows.is_empty()
            && crate::components::empty_state(
                ui,
                "No entry matches the filter.",
                "Clear the filter",
            )
            .clicked()
        {
            state.filter.clear();
        }
        ui.spacing_mut().item_spacing.y = space::XXS;
        let area = egui::ScrollArea::vertical()
            .id_salt(("contents-rows", tab))
            .auto_shrink(false)
            .animated(false);
        // Bring the highlighted line into view, only when it is out of view, so the list does
        // not jump while the line shows.
        let reveal = std::mem::take(&mut state.scroll_to)
            .then(|| rows.iter().position(|&n| Some(n) == current))
            .flatten()
            .filter(|i| {
                state.in_view.is_empty()
                    || !(state.in_view.start + 1..state.in_view.end.saturating_sub(1)).contains(i)
            });
        let mut toggle: Option<usize> = None;
        let mut in_view = 0..0;
        area.show_rows(ui, ROW_H, rows.len(), |ui, range| {
            in_view = range.clone();
            if let Some(i) = reveal {
                let row = ROW_H + ui.spacing().item_spacing.y;
                let top = ui.max_rect().top() + (i as f32 - range.start as f32) * row;
                let line = egui::Rect::from_min_size(
                    egui::pos2(ui.max_rect().left(), top),
                    vec2(ui.max_rect().width(), ROW_H),
                );
                ui.scroll_to_rect(line, Some(egui::Align::Center));
            }
            for &n in &rows[range] {
                let node = contents.node(n);
                let depth = if filtering { 0 } else { node.depth };
                let open = state.expanded.contains(&n);
                // Each line exactly ROW_H tall, so the rows `show_rows` places line up.
                let (row, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
                let left = row.left() + depth as f32 * INDENT;
                let icon =
                    egui::Rect::from_min_size(egui::pos2(left, row.top()), vec2(INDENT, ROW_H));
                if !filtering && !node.children.is_empty() {
                    let response = ui
                        .interact(
                            icon,
                            ui.scope_id().with(("contents-toggle", n)),
                            Sense::click(),
                        )
                        .on_hover_text(if open { "Collapse" } else { "Expand" });
                    let mut tri = response.clone();
                    tri.rect = egui::Rect::from_center_size(
                        icon.center(),
                        egui::Vec2::splat(size::DISCLOSURE),
                    );
                    egui::collapsing_header::paint_default_icon(
                        ui,
                        if open { 1.0 } else { 0.0 },
                        &tri,
                    );
                    if response.clicked() {
                        toggle = Some(n);
                    }
                }
                let text = egui::Rect::from_min_max(
                    egui::pos2((icon.right() + space::XXS).min(row.right()), row.top()),
                    row.max,
                );
                let label = egui::Button::selectable(Some(n) == current, &node.label).truncate();
                let mut label_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(text)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                if label_ui.add(label).clicked() {
                    go = node.target.clone();
                    if !node.children.is_empty() && !open {
                        toggle = Some(n);
                    }
                }
            }
        });
        state.in_view = in_view;
        if let Some(n) = toggle
            && !state.expanded.remove(&n)
        {
            state.expanded.insert(n);
        }
        if let Some(target) = go {
            self.session.workspace.user_focused(tab);
            obs_engine::contents::go(&mut self.session.workspace, tab, &target);
        }
    }
}
