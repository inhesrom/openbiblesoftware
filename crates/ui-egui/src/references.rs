//! Cross-references and reference popups share asynchronous data and snippet caches.

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Weak};

use egui::{Mesh, Pos2, Rect, Sense, Shape, Ui, vec2};
use obs_engine::cross_references::{self, BiblePreview, ReferenceGroup, TOP_REFERENCES};
use obs_engine::{JobEventKind, JobId, JobQueue, Library};
use obs_layout::ColorRole;
use obs_model::{Direction, SpanKind};
use obs_scripture::{Passage, PassageList, RefStyle, Standard, Verse, format_passage};

use super::{State, TabId};
use crate::{ReadingDoc, SnippetCache};

struct Queries<T, K = Passage> {
    jobs: JobQueue,
    pending: BTreeMap<JobId, K>,
    ready: HashMap<K, Result<Arc<T>, String>>,
    /// An inline queue runs nothing while held (tests: a query still pending).
    held: bool,
}

impl<T: Send + 'static, K> Default for Queries<T, K> {
    fn default() -> Self {
        Self {
            jobs: if cfg!(target_arch = "wasm32") {
                JobQueue::inline()
            } else {
                JobQueue::threaded(1)
            },
            pending: BTreeMap::new(),
            ready: HashMap::new(),
            held: false,
        }
    }
}

impl<T: Send + 'static, K: Clone + Eq + Hash + Send + 'static> Queries<T, K> {
    fn clear(&mut self) {
        for id in self.pending.keys() {
            self.jobs.cancel(*id);
        }
        self.pending.clear();
        self.ready.clear();
    }

    fn get_key(
        &mut self,
        passage: K,
        query: impl FnOnce(K) -> Result<T, String> + Send + 'static,
    ) -> Option<Result<Arc<T>, String>> {
        if !self.jobs.is_threaded() && !self.held {
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
            let key = passage.clone();
            let id = self
                .jobs
                .submit_fn("Scripture references", move |_| query(key));
            self.pending.insert(id, passage);
        }
        None
    }
}

impl<T: Send + 'static> Queries<T> {
    fn get(
        &mut self,
        library: &Library,
        passage: Passage,
        query: impl FnOnce(&Library, Passage) -> Result<T, String> + Send + 'static,
    ) -> Option<Result<Arc<T>, String>> {
        let library = library.clone();
        self.get_key(passage, move |p| query(&library, p))
    }
}

/// A Weak handle keeps the allocation's address reserved without retaining a closed
/// document's text: pointer reuse cannot return another revision's targets, and neither
/// hashing nor equality walks Resource text or Verse maps.
#[derive(Clone)]
pub(crate) struct TargetKey {
    doc: Weak<ReadingDoc>,
    unit: usize,
    span: usize,
}

impl TargetKey {
    pub(crate) fn new(doc: &Arc<ReadingDoc>, unit: usize, span: usize) -> Self {
        Self {
            doc: Arc::downgrade(doc),
            unit,
            span,
        }
    }
}

impl std::fmt::Debug for TargetKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TargetKey")
            .field("doc", &Weak::as_ptr(&self.doc))
            .field("unit", &self.unit)
            .field("span", &self.span)
            .finish()
    }
}

impl PartialEq for TargetKey {
    fn eq(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.doc, &other.doc) && self.unit == other.unit && self.span == other.span
    }
}
impl Eq for TargetKey {}
impl Hash for TargetKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Weak::as_ptr(&self.doc).hash(state);
        self.unit.hash(state);
        self.span.hash(state);
    }
}

/// One group per written target, even when its map has gaps. Popup limits count groups,
/// not individual mapped Verses; separate written targets are never sorted or merged.
pub(crate) type Targets = Vec<PassageList>;

#[derive(Default)]
pub(crate) struct TargetCache {
    queries: Queries<Targets, TargetKey>,
}

impl TargetCache {
    /// Tests: a cache whose worker runs nothing until [`TargetCache::release`].
    #[cfg(test)]
    pub(crate) fn held() -> Self {
        let mut cache = Self::default();
        cache.queries.jobs = JobQueue::inline();
        cache.queries.held = true;
        cache
    }

    #[cfg(test)]
    pub(crate) fn release(&mut self) {
        self.queries.held = false;
    }

    pub(crate) fn get(&mut self, key: TargetKey) -> Option<Result<Arc<Targets>, String>> {
        self.queries.get_key(key, |key| {
            let doc = key.doc.upgrade().ok_or("The Resource was closed")?;
            Ok(targets(&doc, key.unit, key.span))
        })
    }
}

#[derive(Default)]
pub(super) struct References {
    groups: Queries<Vec<ReferenceGroup>>,
    pub targets: TargetCache,
    pub pending_clicks: BTreeMap<TabId, (TargetKey, bool)>,
    previews: Queries<BiblePreview>,
    snippets: SnippetCache,
    preview_text: Arc<std::sync::Mutex<cross_references::BiblePreviewCache>>,
    library_key: Vec<(String, u32, String, std::path::PathBuf)>,
    pub pending_navigation: BTreeMap<TabId, Verse>,
    /// The Atlas's places, per Passage (#150).
    places: Queries<Vec<obs_engine::MapPlace>>,
    /// The Timeline's Eras, Lifespans and Events (#185): one for the library.
    timeline: Queries<obs_engine::Timeline, ()>,
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
            self.places.clear();
            self.timeline.clear();
        }
    }

    /// The Timeline of the library's People & places indexes (#185); `None` while it is read
    /// on a worker.
    pub(super) fn timeline(
        &mut self,
        library: &Library,
    ) -> Option<Result<Arc<obs_engine::Timeline>, String>> {
        let library = library.clone();
        self.timeline
            .get_key((), move |()| obs_engine::timeline::read_timeline(&library))
    }

    /// The places the library's People & places indexes list in `passage`, with their
    /// locations (the Atlas, #150); `None` while the query runs on a worker.
    pub(super) fn places(
        &mut self,
        library: &Library,
        directory: Arc<obs_engine::factbook::DirectoryCache>,
        passage: Passage,
    ) -> Option<Result<Arc<Vec<obs_engine::MapPlace>>, String>> {
        self.places.get(library, passage, move |library, passage| {
            obs_engine::atlas::passage_places(library, &directory, passage)
        })
    }
}

/// Native translation runs only on the worker. Scan map entries once per written target,
/// rather than linearly searching the entire map for every Verse of a large Passage.
fn targets(doc: &ReadingDoc, unit: usize, span: usize) -> Targets {
    let Some(span) = doc.units.get(unit).and_then(|u| u.spans.get(span)) else {
        return vec![];
    };
    match &span.kind {
        SpanKind::Xref { targets } => standard_targets(doc, targets, false),
        SpanKind::ScriptureRef { targets } => standard_targets(doc, targets, true),
        _ => vec![],
    }
}

/// A reference's written targets in the Standard versification: `native` ones are read in
/// the Bible's own numbering and go through its Verse map (a Scripture reference in its
/// text, ADR 0002); others already are Standard (a Cross-reference).
pub(crate) fn standard_targets(doc: &ReadingDoc, written: &PassageList, native: bool) -> Targets {
    let map = if native { doc.verse_map.as_ref() } else { None };
    written
        .iter()
        .map(|p| {
            let Some(map) = map else {
                return vec![*p].into();
            };
            map.entries()
                .iter()
                .filter_map(|entry| {
                    let end = Verse::new(entry.verse.book, entry.verse.chapter, entry.last);
                    (entry.verse <= p.end() && p.start() <= end)
                        .then_some(entry.standard)
                        .flatten()
                })
                .collect::<PassageList>()
                .merged(&Standard)
        })
        .collect()
}

impl State {
    pub(super) fn cross_references_body(&mut self, ui: &mut Ui, tab: TabId) {
        let passage = self
            .session
            .workspace
            .tab(tab)
            .and_then(|t| t.shown_passage());
        let Some(passage) = passage else {
            ui.label("Enter a Bible reference or join a Link set");
            return;
        };
        let Some(t) = self.tabs.get_mut(&tab) else {
            return;
        };
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
            // #135: say what is missing and offer the way to get it.
            let empty = self.session.guide_empty_state(
                &obs_model::Key::Passage(passage),
                obs_engine::GuideSection::CrossReferences,
            );
            if let Some((id, params)) = self.missing_resource_ui(ui, empty.message, &empty.missing)
            {
                self.run(id, &params);
            }
            return;
        }
        let mut navigate = None;
        egui::ScrollArea::vertical()
            .id_salt(("cross-references", tab, passage))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                navigate = self.reference_groups(ui, tab, &groups);
            });
        if let Some(passage) = navigate {
            self.reference_navigate(
                tab,
                passage,
                ui.input(|i| i.modifiers.command || i.modifiers.ctrl),
            );
        }
    }

    /// Cross-references grouped by source Verse: the top [`TOP_REFERENCES`] targets of each
    /// (Show all / Show less per Verse), each with its hover popup and a one-line preview
    /// from the ★ Bible. Shared by the Cross-references Tool and the Passage Guide; returns
    /// the target clicked this frame.
    pub(super) fn reference_groups(
        &mut self,
        ui: &mut Ui,
        tab: TabId,
        groups: &[ReferenceGroup],
    ) -> Option<Passage> {
        let mut navigate = None;
        for group in groups {
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
                    self.reference_popup(ui, &[vec![row.target].into()]);
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
        navigate
    }

    /// Resource text is always drawn with the glyph atlas. A cache miss only submits work;
    /// it never shapes, lays out or reads SQLite on the UI thread.
    pub(super) fn preview(&mut self, ui: &mut Ui, passage: Passage, one_line: bool) {
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

    pub(super) fn reference_message_popup(&self, ui: &Ui, message: &str) {
        egui::Tooltip::always_open(
            ui.ctx().clone(),
            ui.layer_id(),
            ui.scope_id().with("scripture-preview"),
            egui::PopupAnchor::Pointer,
        )
        .show(|ui| {
            ui.label(message);
        });
    }

    pub(super) fn reference_popup(&mut self, ui: &Ui, passages: &[PassageList]) {
        egui::Tooltip::always_open(
            ui.ctx().clone(),
            ui.layer_id(),
            ui.scope_id().with("scripture-preview"),
            egui::PopupAnchor::Pointer,
        )
        .show(|ui| {
            ui.set_width(360.0);
            // Bound work even when one written target maps to many disjoint Passages.
            for target in passages.iter().take(3) {
                let mut remaining = 20;
                for passage in target {
                    if remaining == 0 {
                        ui.small("Preview shortened — click the reference to read");
                        break;
                    }
                    let verses: Vec<_> = passage.verses(&Standard).take(remaining + 1).collect();
                    let count = verses.len().min(remaining);
                    let last = count.checked_sub(1).and_then(|i| verses.get(i));
                    let Some(Ok(shown)) =
                        verses.first().zip(last).map(|(s, e)| Passage::new(*s, *e))
                    else {
                        continue;
                    };
                    ui.strong(format_passage(*passage, &Standard, RefStyle::Short));
                    self.preview(ui, shown, false);
                    remaining -= count;
                    if verses.len() > count {
                        ui.small("Preview shortened — click the reference to read");
                    }
                }
                if target.is_empty() {
                    ui.label("No Standard equivalent");
                }
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
