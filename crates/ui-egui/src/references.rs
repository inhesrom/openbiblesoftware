//! Cross-references and reference popups share asynchronous data and snippet caches.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use egui::{Mesh, Pos2, Rect, Sense, Shape, Ui, vec2};
use obs_engine::cross_references::{self, BiblePreview, ReferenceGroup, TOP_REFERENCES};
use obs_engine::{JobEventKind, JobId, JobQueue, Library};
use obs_layout::ColorRole;
use obs_model::{Direction, SpanKind};
use obs_scripture::{Passage, RefStyle, Standard, Verse, format_passage};

use super::{State, TabId};
use crate::{ReadingDoc, SnippetCache};

struct Queries<T> {
    jobs: JobQueue,
    pending: BTreeMap<JobId, Passage>,
    ready: HashMap<Passage, Result<Arc<T>, String>>,
}

impl<T: Send + 'static> Default for Queries<T> {
    fn default() -> Self {
        Self {
            jobs: if cfg!(target_arch = "wasm32") {
                JobQueue::inline()
            } else {
                JobQueue::threaded(1)
            },
            pending: BTreeMap::new(),
            ready: HashMap::new(),
        }
    }
}

impl<T: Send + 'static> Queries<T> {
    fn clear(&mut self) {
        for id in self.pending.keys() {
            self.jobs.cancel(*id);
        }
        self.pending.clear();
        self.ready.clear();
    }

    fn get(
        &mut self,
        library: &Library,
        passage: Passage,
        query: impl FnOnce(&Library, Passage) -> Result<T, String> + Send + 'static,
    ) -> Option<Result<Arc<T>, String>> {
        if !self.jobs.is_threaded() {
            self.jobs.pump(1);
        }
        for event in self.jobs.drain_events() {
            if !event.kind.is_terminal() {
                continue;
            }
            let Some(key) = self.pending.remove(&event.id) else {
                continue;
            };
            let result = match event.kind {
                JobEventKind::Finished(value) => value
                    .downcast::<T>()
                    .map(|v| Arc::new(*v))
                    .map_err(|_| "Unexpected reference result".into()),
                JobEventKind::Failed(e) => Err(e),
                _ => continue,
            };
            if self.ready.len() >= 256 {
                self.ready.clear();
            }
            self.ready.insert(key, result);
        }
        if let Some(result) = self.ready.get(&passage) {
            return Some(result.clone());
        }
        if self.pending.len() < 32 && !self.pending.values().any(|p| *p == passage) {
            let library = library.clone();
            let id = self
                .jobs
                .submit_fn("Scripture references", move |_| query(&library, passage));
            self.pending.insert(id, passage);
        }
        None
    }
}

#[derive(Default)]
pub(super) struct References {
    groups: Queries<Vec<ReferenceGroup>>,
    previews: Queries<BiblePreview>,
    snippets: SnippetCache,
    preview_text: Arc<std::sync::Mutex<cross_references::BiblePreviewCache>>,
    library_key: Vec<(String, u32, String, std::path::PathBuf)>,
    pub pending_navigation: BTreeMap<TabId, Verse>,
}

impl References {
    pub(super) fn refresh(&mut self, library: &Library) {
        let key = library
            .entries()
            .iter()
            .map(|e| {
                (
                    e.manifest.resource_id.clone(),
                    e.manifest.revision,
                    e.manifest.content_hash.clone(),
                    e.path.clone(),
                )
            })
            .collect::<Vec<_>>();
        if key != self.library_key {
            self.library_key = key;
            self.groups.clear();
            self.previews.clear();
        }
    }
}

/// Scripture-reference spans in Bibles use native numbering; Cross-references always use
/// Standard. Keep every mapped Passage in written order, including split/merged Verses,
/// and omit Unmapped ones. Sorting or merging would change the first click destination.
pub(super) fn targets(doc: &ReadingDoc, unit: usize, span: usize) -> Vec<Passage> {
    let Some(span) = doc.units.get(unit).and_then(|u| u.spans.get(span)) else {
        return vec![];
    };
    match &span.kind {
        SpanKind::Xref { targets } => targets.passages().to_vec(),
        SpanKind::ScriptureRef { targets } => match &doc.verse_map {
            Some(map) => targets
                .passages()
                .iter()
                .flat_map(|p| p.verses(&doc.versification))
                .filter_map(|v| map.to_standard(v))
                .collect(),
            None => targets.passages().to_vec(),
        },
        _ => vec![],
    }
}

impl State {
    pub(super) fn cross_references_body(&mut self, ui: &mut Ui, tab: TabId) {
        let at = self
            .session
            .workspace
            .tab(tab)
            .and_then(|t| t.history.current());
        let Some(at) = at else {
            ui.label("Enter a Bible reference or join a Link set");
            return;
        };
        let Some(t) = self.tabs.get_mut(&tab) else {
            return;
        };
        if t.reference_passage.is_some_and(|p| p.start() != at) {
            t.reference_passage = None;
        }
        let passage = t.reference_passage.unwrap_or(Passage::verse(at));
        if t.reference_at != Some(passage) {
            t.reference_at = Some(passage);
            t.expanded_references.clear();
        }
        let result = self.references.groups.get(
            &self.session.library,
            passage,
            cross_references::library_references,
        );
        let groups = match result {
            None => {
                ui.label("Loading Cross-references…");
                ui.ctx().request_repaint();
                return;
            }
            Some(Err(e)) => {
                ui.label(format!("Could not load Cross-references: {e}"));
                return;
            }
            Some(Ok(groups)) => groups,
        };
        if groups.is_empty() {
            ui.label("None in your library — add a Cross-reference set or try another Passage");
            return;
        }
        let mut navigate = None;
        egui::ScrollArea::vertical()
            .id_salt(("cross-references", tab, passage))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for group in groups.iter() {
                    ui.strong(format_passage(
                        Passage::verse(group.verse),
                        &Standard,
                        RefStyle::Short,
                    ));
                    let expanded = self
                        .tabs
                        .get(&tab)
                        .is_some_and(|t| t.expanded_references.contains(&group.verse));
                    let count = if expanded {
                        group.rows.len()
                    } else {
                        group.rows.len().min(TOP_REFERENCES)
                    };
                    for row in &group.rows[..count] {
                        let label = format_passage(row.target, &Standard, RefStyle::Short);
                        let r = ui
                            .link(label)
                            .on_hover_text(format!("{} · {} votes", row.source_title, row.votes));
                        if r.clicked() {
                            navigate = Some(row.target);
                        }
                        if r.hovered() {
                            self.reference_popup(ui, &[row.target]);
                        }
                        self.preview(ui, Passage::verse(row.target.start()), true);
                    }
                    if group.rows.len() > TOP_REFERENCES
                        && ui
                            .button(if expanded { "Show less" } else { "Show all" })
                            .clicked()
                        && let Some(t) = self.tabs.get_mut(&tab)
                    {
                        if expanded {
                            t.expanded_references.remove(&group.verse);
                        } else {
                            t.expanded_references.insert(group.verse);
                        }
                    }
                    ui.separator();
                }
            });
        if let Some(passage) = navigate {
            self.reference_navigate(
                tab,
                passage,
                ui.input(|i| i.modifiers.command || i.modifiers.ctrl),
            );
        }
    }

    /// Resource text is always drawn with the glyph atlas. A cache miss only submits work;
    /// it never shapes, lays out or reads SQLite on the UI thread.
    fn preview(&mut self, ui: &mut Ui, passage: Passage, one_line: bool) {
        let width = ui.available_width().max(40.0);
        let height = self.session.text.size * 1.8;
        if one_line
            && !ui.is_rect_visible(Rect::from_min_size(ui.cursor().min, vec2(width, height)))
        {
            ui.allocate_space(vec2(width, height));
            return;
        }
        let cache = Arc::clone(&self.references.preview_text);
        let preview = match self.references.previews.get(
            &self.session.library,
            passage,
            move |library, passage| {
                let mut cache = cache.lock().unwrap_or_else(|p| p.into_inner());
                cross_references::preferred_preview_cached(library, passage, &mut cache)
            },
        ) {
            None => {
                ui.allocate_space(vec2(width, height));
                ui.ctx().request_repaint();
                return;
            }
            Some(Err(e)) => {
                ui.label(e);
                return;
            }
            Some(Ok(preview)) => preview,
        };
        if !one_line {
            ui.label(format!("★ {}", preview.abbreviation));
        }
        let laid = self.references.snippets.get(
            &preview.text,
            ColorRole::Ink,
            preview.direction == Direction::Rtl,
            width,
            &self.session.text,
        );
        let Some(laid) = laid else {
            ui.allocate_space(vec2(width, height));
            ui.ctx().request_repaint();
            return;
        };
        let height = if one_line {
            laid.lines.first().map_or(height, |line| line.bottom)
        } else {
            laid.height.min(360.0)
        };
        let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &preview.text));
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
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        painter.rect_filled(rect, 0.0, self.palette.page);
        painter.add(Shape::mesh(mesh));
        if !one_line && (preview.truncated || laid.height > height) {
            ui.small("Preview shortened — click the reference to read");
        }
    }

    pub(super) fn reference_popup(&mut self, ui: &Ui, passages: &[Passage]) {
        egui::Tooltip::always_open(
            ui.ctx().clone(),
            ui.layer_id(),
            ui.scope_id().with("scripture-preview"),
            egui::PopupAnchor::Pointer,
        )
        .show(|ui| {
            ui.set_width(360.0);
            // A bounded popup; each target stays independently reachable in the Resource.
            for passage in passages.iter().take(3) {
                ui.strong(format_passage(*passage, &Standard, RefStyle::Short));
                self.preview(ui, *passage, false);
            }
            if passages.len() > 3 {
                ui.small("More Passages in this reference");
            }
            if passages.is_empty() {
                ui.label("No Standard equivalent");
            }
        });
    }

    pub(super) fn reference_navigate(&mut self, opener: TabId, passage: Passage, new_tab: bool) {
        let linked = self
            .session
            .workspace
            .tab(opener)
            .is_some_and(|t| t.link.is_some());
        if linked && !new_tab {
            self.session
                .workspace
                .navigate_standard(opener, passage.start());
            return;
        }
        let id = self
            .session
            .library
            .of_type(obs_model::ResourceType::Bible)
            .next()
            .map(|e| e.manifest.resource_id.clone());
        let Some(id) = id else {
            self.toast("Add a Bible to your library to open this Passage");
            return;
        };
        self.session.workspace.focus(opener);
        if let Some(result) = self.run("resource.open", &serde_json::json!({"id": id}))
            && let Some(id) = result["tab"].as_u64()
        {
            self.references
                .pending_navigation
                .insert(TabId(id), passage.start());
        }
    }
}

#[cfg(test)]
#[path = "reference_data_tests.rs"]
mod tests;
