//! The Search Tool's Library mode (#60; #16 rules 10, 12 and 14; #18): full-text search of
//! Commentaries, Lexicons and Dictionaries, on the Search Tab's state, jobs and commands that
//! Bible mode (#59) uses.
//!
//! - **Search scope:** Resource types and single Resources ([`LibraryScope`]; by default
//!   every Commentary, Lexicon and Dictionary). Bibles are Bible mode's.
//! - **Documents:** the paragraphs of each Resource's Search index (#58), each carrying its
//!   unit's id, title and the Key that covers it.
//! - **Results:** grouped by Resource (Commentaries, then Lexicons, then Dictionaries, each in
//!   Preferred order), best first (`bm25`) inside each group. A row is the unit title or Key
//!   and an excerpt of the paragraph around its first hit, highlighted in Rust like Bible
//!   mode's.
//! - **Opening a hit:** a Tab of that Resource in the Search Tab's Link set, else the most
//!   recently active Tab of that Resource, else a new Tab (Ctrl/Cmd: always a new Tab) goes
//!   to the unit's Key and scrolls the paragraph to the top ([`crate::Effect::NavigateText`]).

use std::ops::Range;
use std::sync::Arc;

use obs_model::{Key, ResourceType, Span, SpanKind, UnitId};
use obs_scripture::{RefStyle, Standard, format_passage};
use obs_search::{Highlighter, SearchHit};
use serde_json::{Value, json};

#[cfg(not(target_arch = "wasm32"))]
use crate::bible_search::{BibleJob, JobKind};
use crate::bible_search::{
    ErrorField, InlineError, PAGE_ROWS, SearchMode, SearchTab, Snippet, TAB, check_query,
    error_json, failed, rerun, snippet, tab_param,
};
use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::library::Library;
use crate::session::Session;
use crate::workspace::{TabId, TabKind, Tool};

/// The Resource types Library mode searches, in result order.
pub const LIBRARY_TYPES: [ResourceType; 3] = [
    ResourceType::Commentary,
    ResourceType::Lexicon,
    ResourceType::Dictionary,
];

/// How much of a paragraph a result row shows, in bytes (about three lines).
pub const EXCERPT_BYTES: usize = 320;

/// A Library type's id in commands (`commentary`, `lexicon`, `dictionary`).
pub fn type_id(ty: ResourceType) -> &'static str {
    match ty {
        ResourceType::Bible => "bible",
        ResourceType::Commentary => "commentary",
        ResourceType::Lexicon => "lexicon",
        ResourceType::Dictionary => "dictionary",
        ResourceType::CrossReferenceSet => "cross_reference_set",
        ResourceType::PeopleAndPlaces => "people_and_places",
    }
}

/// A Library type from its id (singular or plural, any case).
pub fn type_from_id(id: &str) -> Option<ResourceType> {
    match id.trim().to_lowercase().as_str() {
        "commentary" | "commentaries" => Some(ResourceType::Commentary),
        "lexicon" | "lexicons" => Some(ResourceType::Lexicon),
        "dictionary" | "dictionaries" => Some(ResourceType::Dictionary),
        _ => None,
    }
}

/// A Library type as the picker names it: "Commentaries".
pub fn type_plural(ty: ResourceType) -> &'static str {
    match ty {
        ResourceType::Bible => "Bibles",
        ResourceType::Commentary => "Commentaries",
        ResourceType::Lexicon => "Lexicons",
        ResourceType::Dictionary => "Dictionaries",
        ResourceType::CrossReferenceSet => "Cross-reference sets",
        ResourceType::PeopleAndPlaces => "People & places indexes",
    }
}

fn type_rank(ty: ResourceType) -> usize {
    LIBRARY_TYPES
        .iter()
        .position(|t| *t == ty)
        .unwrap_or(LIBRARY_TYPES.len())
}

// ----------------------------------------------------------------------------------- scope

/// Library mode's Search scope (#16 rule 10): whole Resource types, Collections (#153), plus
/// single Resources of the other types. A type in `types` covers every Resource of it, and a
/// Collection every Resource in it, including ones added to the library later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryScope {
    /// In [`LIBRARY_TYPES`] order.
    pub types: Vec<ResourceType>,
    /// Collection names, in the order chosen. Their Bibles and other types Library mode does
    /// not search are left out.
    pub collections: Vec<String>,
    /// Resource ids chosen one by one (none of a type in `types`).
    pub resources: Vec<String>,
}

impl Default for LibraryScope {
    /// Every Commentary, Lexicon and Dictionary.
    fn default() -> Self {
        Self {
            types: LIBRARY_TYPES.to_vec(),
            collections: Vec::new(),
            resources: Vec::new(),
        }
    }
}

impl LibraryScope {
    /// Nothing chosen.
    pub fn empty() -> Self {
        Self {
            types: Vec::new(),
            collections: Vec::new(),
            resources: Vec::new(),
        }
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Whether a library Resource is searched.
    pub fn includes(&self, library: &Library, id: &str) -> bool {
        library.get(id).is_some_and(|e| {
            let ty = e.manifest.resource_type;
            LIBRARY_TYPES.contains(&ty)
                && (self.types.contains(&ty)
                    || self.resources.iter().any(|r| r == id)
                    || self.in_collection(library, id))
        })
    }

    /// Whether a library Resource is in one of the scope's Collections.
    fn in_collection(&self, library: &Library, id: &str) -> bool {
        self.collections
            .iter()
            .any(|c| library.in_collection(c, id))
    }

    /// The library Resources searched, in result order: Commentaries, Lexicons, then
    /// Dictionaries, each in Preferred order.
    pub fn resources(&self, library: &Library) -> Vec<String> {
        LIBRARY_TYPES
            .iter()
            .flat_map(|ty| library.of_type(*ty))
            .filter(|e| self.includes(library, &e.manifest.resource_id))
            .map(|e| e.manifest.resource_id.clone())
            .collect()
    }

    fn sort_types(&mut self) {
        self.types.sort_by_key(|t| type_rank(*t));
        self.types.dedup();
    }

    /// Search a whole type, or stop searching it (and any of its Resources chosen one by one).
    pub fn toggle_type(&mut self, library: &Library, ty: ResourceType) {
        self.resources.retain(|r| {
            library
                .get(r)
                .is_none_or(|e| e.manifest.resource_type != ty)
        });
        if let Some(i) = self.types.iter().position(|t| *t == ty) {
            self.types.remove(i);
        } else {
            self.types.push(ty);
            self.sort_types();
        }
    }

    /// Search a Collection, or stop searching it. Choosing one while the scope is the
    /// default (every Commentary, Lexicon and Dictionary) searches just that Collection.
    /// One already searched stops being searched even if it has since been deleted or
    /// renamed (a search brought back from Favorites or History keeps the saved name).
    pub fn toggle_collection(&mut self, library: &Library, name: &str) -> Result<(), String> {
        let name = name.trim();
        if let Some(i) = self
            .collections
            .iter()
            .position(|c| c.eq_ignore_ascii_case(name))
        {
            self.collections.remove(i);
            return Ok(());
        }
        let name = library
            .collection(name)
            .map(|c| c.name.clone())
            .ok_or_else(|| format!("no Collection named “{name}”"))?;
        if self.is_default() {
            *self = Self::empty();
        }
        self.collections.push(name);
        Ok(())
    }

    /// Follow a Collection renamed (`to: Some`) or deleted.
    pub fn rename_collection(&mut self, from: &str, to: Option<&str>) {
        match to {
            Some(to) => {
                for c in &mut self.collections {
                    if c.eq_ignore_ascii_case(from) {
                        *c = to.to_string();
                    }
                }
            }
            None => self.collections.retain(|c| !c.eq_ignore_ascii_case(from)),
        }
    }

    /// Search a Resource, or stop searching it. Leaving out one Resource drops everything
    /// that covered it (its whole type, the Collections it is in, its own choice) and keeps
    /// the others those covered, one by one; choosing every Resource of a type one by one
    /// makes it the whole type again.
    pub fn toggle_resource(&mut self, library: &Library, id: &str) -> Result<(), String> {
        let ty = library
            .get(id)
            .map(|e| e.manifest.resource_type)
            .filter(|t| LIBRARY_TYPES.contains(t))
            .ok_or_else(|| {
                format!("{id} is not a Commentary, Lexicon or Dictionary in the library")
            })?;
        if self.includes(library, id) {
            let searched = self.resources(library);
            self.types.retain(|t| *t != ty);
            self.collections.retain(|c| !library.in_collection(c, id));
            self.resources.retain(|r| r != id);
            for other in searched {
                if other != id && !self.includes(library, &other) {
                    self.resources.push(other);
                }
            }
        } else {
            self.resources.push(id.to_string());
            let all: Vec<String> = library
                .of_type(ty)
                .map(|e| e.manifest.resource_id.clone())
                .collect();
            if all.iter().all(|r| self.resources.contains(r)) {
                self.resources.retain(|r| !all.contains(r));
                self.types.push(ty);
                self.sort_types();
            }
        }
        Ok(())
    }

    /// Read `search.run`'s `resources`: comma-separated type ids (`commentary`, `lexicon`,
    /// `dictionary`, singular or plural), `collection:<name>` and resource ids.
    pub fn parse(text: &str, library: &Library) -> Result<Self, String> {
        let mut out = Self::empty();
        for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            if let Some(name) = part.strip_prefix("collection:") {
                let c = library
                    .collection(name)
                    .ok_or_else(|| format!("no Collection named “{}”", name.trim()))?;
                if !out.collections.contains(&c.name) {
                    out.collections.push(c.name.clone());
                }
            } else if let Some(ty) = type_from_id(part) {
                out.types.push(ty);
            } else if library
                .get(part)
                .is_some_and(|e| LIBRARY_TYPES.contains(&e.manifest.resource_type))
            {
                if !out.resources.iter().any(|r| r == part) {
                    out.resources.push(part.to_string());
                }
            } else {
                return Err(format!(
                    "“{part}” is neither commentary, lexicon or dictionary nor a Commentary, \
                     Lexicon or Dictionary in the library"
                ));
            }
        }
        out.sort_types();
        let types = out.types.clone();
        out.resources.retain(|r| {
            library
                .get(r)
                .is_none_or(|e| !types.contains(&e.manifest.resource_type))
        });
        Ok(out)
    }
}

// ---------------------------------------------------------------------------------- results

/// One Resource's group of hits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryGroup {
    pub id: String,
    pub abbreviation: String,
    pub title: String,
    pub resource_type: ResourceType,
    pub rtl: bool,
    /// Its rows in [`LibraryResults::rows`].
    pub rows: Range<usize>,
}

/// One matching paragraph.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryRow {
    /// Index into [`LibraryResults::groups`].
    pub group: usize,
    /// Where the paragraph is (the Text range space).
    pub unit: UnitId,
    pub range: Range<u32>,
    /// The Key that covers it, and its unit's title.
    pub key: Option<Key>,
    pub title: Option<String>,
    /// Its `bm25` score: lower is better.
    pub score: f64,
}

/// What a Library search found.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryResults {
    pub query: String,
    /// How many Resources were searched (with or without hits).
    pub searched: usize,
    /// Resources with hits, in result order.
    pub groups: Vec<LibraryGroup>,
    /// Every hit, group by group, best first within its group.
    pub rows: Vec<LibraryRow>,
    pub highlighter: Highlighter,
    /// Resources not searched: still indexing (abbreviations), or failed (with why).
    pub indexing: Vec<String>,
    pub failed: Vec<(String, String)>,
    /// Resources searched through an older index while theirs is rebuilt.
    pub stale: Vec<String>,
    pub elapsed_ms: u64,
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

impl LibraryResults {
    /// The summary line: `12 paragraphs in 3 Resources (5 searched)`.
    pub fn summary(&self) -> String {
        format!(
            "{} in {} ({} searched)",
            plural(self.rows.len(), "paragraph", "paragraphs"),
            plural(self.groups.len(), "Resource", "Resources"),
            self.searched
        )
    }

    /// A group's header: `TCC — Test Covenant Commentary · 3 hits`.
    pub fn group_header(&self, group: &LibraryGroup) -> String {
        let hits = plural(group.rows.len(), "hit", "hits");
        if group.abbreviation == group.title || group.title.is_empty() {
            format!("{} · {hits}", group.abbreviation)
        } else {
            format!("{} — {} · {hits}", group.abbreviation, group.title)
        }
    }

    /// Whether row `i` starts its group (its header is drawn above it).
    pub fn starts_group(&self, i: usize) -> bool {
        self.rows
            .get(i)
            .and_then(|r| self.groups.get(r.group))
            .is_some_and(|g| g.rows.start == i)
    }

    pub fn group_of(&self, i: usize) -> Option<&LibraryGroup> {
        self.groups.get(self.rows.get(i)?.group)
    }
}

/// A row's label: the unit's title, or the Key (a Strong's number goes first when the title
/// is the Lemma: `H1285 · בְּרִית`), or the unit id.
pub fn row_label(row: &LibraryRow) -> String {
    let key = row.key.as_ref().map(|k| match k {
        Key::Passage(p) => format_passage(*p, &Standard, RefStyle::Short),
        k => k.to_string(),
    });
    match (row.title.as_deref().filter(|t| !t.trim().is_empty()), key) {
        (Some(t), Some(k)) if matches!(row.key, Some(Key::StrongsNumber(_))) && !t.contains(&k) => {
            format!("{k} · {t}")
        }
        (Some(t), _) => t.to_string(),
        (None, Some(k)) => k,
        (None, None) => row.unit.to_string(),
    }
}

/// A searched Resource, as results show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchedResource {
    pub id: String,
    pub abbreviation: String,
    pub title: String,
    pub resource_type: ResourceType,
    pub rtl: bool,
}

/// Each Resource's hits (best first) → groups in the given order (Resources without hits
/// left out) and their rows.
pub fn library_rows(
    found: &[(SearchedResource, Vec<SearchHit>)],
) -> (Vec<LibraryGroup>, Vec<LibraryRow>) {
    let mut groups = Vec::new();
    let mut rows = Vec::new();
    for (resource, hits) in found {
        if hits.is_empty() {
            continue;
        }
        let group = groups.len();
        let start = rows.len();
        rows.extend(hits.iter().map(|h| LibraryRow {
            group,
            unit: h.unit.clone(),
            range: h.range.clone(),
            key: h.key.clone(),
            title: h.title.clone(),
            score: h.score,
        }));
        groups.push(LibraryGroup {
            id: resource.id.clone(),
            abbreviation: resource.abbreviation.clone(),
            title: resource.title.clone(),
            resource_type: resource.resource_type,
            rtl: resource.rtl,
            rows: start..rows.len(),
        });
    }
    (groups, rows)
}

/// The largest char boundary of `s` at or before `i`.
fn floor_boundary(s: &str, mut i: usize) -> usize {
    i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// At most about `max` bytes of a snippet around its first highlight, cut at spaces, with
/// `…` where text was left out; highlights move with the text.
pub fn excerpt(s: Snippet, max: usize) -> Snippet {
    let len = s.text.len();
    if len <= max {
        return s;
    }
    let focus = s.highlights.first().map_or(0, |h| h.start.min(len));
    let focus_end = s.highlights.first().map_or(0, |h| h.end.min(len));
    // Some context before the first hit, starting at a word.
    let mut start = floor_boundary(&s.text, focus.saturating_sub(max / 3));
    if start > 0
        && let Some(sp) = s.text.get(start..focus).and_then(|t| t.find(' '))
    {
        start += sp + 1;
    }
    let mut end = floor_boundary(&s.text, (start + max).min(len));
    if end < len
        && let Some(sp) = s.text.get(start..end).and_then(|t| t.rfind(' '))
        && start + sp >= focus_end
    {
        end = start + sp;
    }
    let prefix = if start > 0 { "… " } else { "" };
    let suffix = if end < len { " …" } else { "" };
    let text = format!(
        "{prefix}{}{suffix}",
        s.text.get(start..end).unwrap_or_default()
    );
    let shift = |b: usize| b - start + prefix.len();
    let highlights = s
        .highlights
        .iter()
        .filter_map(|h| {
            let (a, b) = (h.start.max(start), h.end.min(end));
            (a < b).then(|| shift(a)..shift(b))
        })
        .collect();
    Snippet { text, highlights }
}

/// The row text of a paragraph hit: the paragraph with white space collapsed and its hits
/// highlighted ([`snippet`]; headings are kept, since a heading can be a document of its own),
/// cut to an [`excerpt`] around the first hit.
pub fn paragraph_snippet(
    unit_text: &str,
    range: Range<usize>,
    spans: &[Span],
    highlighter: &Highlighter,
) -> Snippet {
    let tagged: Vec<Span> = spans
        .iter()
        .filter(|s| matches!(s.kind, SpanKind::TaggedWord { .. }))
        .cloned()
        .collect();
    excerpt(
        snippet(unit_text, range, &tagged, highlighter),
        EXCERPT_BYTES,
    )
}

/// What a Library search job hands back.
pub(crate) struct LibraryRunOutput {
    pub(crate) results: Arc<LibraryResults>,
    pub(crate) first_page: Vec<Vec<Snippet>>,
}

// ---------------------------------------------------------------------------------- session

impl Session {
    /// Start a Library search: check the query and the Resources (errors go inline), remember
    /// the query, and search the ready indexes in the background.
    pub(crate) fn start_library_search(
        &mut self,
        tab: TabId,
        st: &SearchTab,
    ) -> Option<InlineError> {
        let query = match check_query(&st.query) {
            Ok(q) => q,
            Err(e) => return Some(e),
        };
        let ids = st.library.resources(&self.library);
        if ids.is_empty() {
            let any = LIBRARY_TYPES
                .iter()
                .any(|t| self.library.of_type(*t).next().is_some());
            let message = if any {
                "Choose a Resource to search"
            } else {
                "Add a Commentary, Lexicon or Dictionary to your library to search it"
            };
            return Some(InlineError::new(ErrorField::Resources, message, None));
        }
        self.remember_search(SearchMode::Library, &query);
        self.submit_library_search(tab, query, ids)
    }

    #[cfg(target_arch = "wasm32")]
    fn submit_library_search(
        &mut self,
        _: TabId,
        _: String,
        _: Vec<String>,
    ) -> Option<InlineError> {
        Some(InlineError::new(
            ErrorField::Search,
            "Searching needs the desktop app",
            None,
        ))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn submit_library_search(
        &mut self,
        tab: TabId,
        query: String,
        ids: Vec<String>,
    ) -> Option<InlineError> {
        use crate::search::{NotReady, Opened};
        let searched_count = ids.len();
        let mut searched = Vec::new();
        let mut indexing = Vec::new();
        let mut failed = Vec::new();
        let mut stale = Vec::new();
        for id in ids {
            let Some(entry) = self.library.get(&id) else {
                continue;
            };
            let resource = SearchedResource {
                id: id.clone(),
                abbreviation: entry.manifest.abbreviation.clone(),
                title: entry.manifest.title.clone(),
                resource_type: entry.manifest.resource_type,
                rtl: entry.manifest.direction == obs_model::Direction::Rtl,
            };
            let obr = entry.path.clone();
            let abbreviation = resource.abbreviation.clone();
            match self.open_search_index(&id) {
                Ok(Opened::Current(index)) => searched.push((resource, obr, index)),
                Ok(Opened::Stale(index)) => {
                    stale.push(abbreviation);
                    searched.push((resource, obr, index));
                }
                Err(NotReady::Indexing) => indexing.push(abbreviation),
                Err(NotReady::Failed(e)) => failed.push((abbreviation, e)),
                Err(NotReady::NotInLibrary) => {}
            }
        }
        let lexicons: Vec<std::path::PathBuf> = self
            .library
            .of_type(ResourceType::Lexicon)
            .map(|e| e.path.clone())
            .collect();
        let word_forms = self.search_word_forms();
        let input = desktop::RunInput {
            query,
            word_forms,
            searched_count,
            resources: searched,
            indexing,
            failed,
            stale,
            lexicons,
        };
        let generation = self.search_tab(tab).map_or(0, SearchTab::generation);
        let job = self
            .jobs()
            .submit_fn("Library search", move |ctx| desktop::run(input, ctx));
        self.bible_search.jobs.insert(
            job,
            BibleJob {
                tab,
                generation,
                kind: JobKind::Run,
            },
        );
        if let Some(st) = self.search_tab_mut(tab) {
            st.running = true;
            st.loading.insert(0);
        }
        None
    }

    /// Fetch the snippets of Library result `rows` (the rows a view shows) if they are not
    /// here yet.
    pub(crate) fn request_library_snippets(&mut self, tab: TabId, rows: Range<usize>) {
        let Some(st) = self.search_tab(tab) else {
            return;
        };
        let Some(results) = st.library_results.clone() else {
            return;
        };
        let last = rows.end.min(results.rows.len());
        if rows.start >= last {
            return;
        }
        let pages: Vec<usize> = (rows.start / PAGE_ROWS..=(last - 1) / PAGE_ROWS)
            .filter(|p| !st.pages.contains_key(p) && !st.loading.contains(p))
            .collect();
        let generation = st.generation();
        for page in pages {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let obrs: Vec<Option<std::path::PathBuf>> = results
                    .groups
                    .iter()
                    .map(|g| self.library.get(&g.id).map(|e| e.path.clone()))
                    .collect();
                let results = Arc::clone(&results);
                let job = self.jobs().submit_fn("Search snippets", move |ctx| {
                    if ctx.is_cancelled() {
                        return Err("cancelled".to_string());
                    }
                    Ok(crate::bible_search::PageOutput(desktop::page_snippets(
                        &results, &obrs, page,
                    )))
                });
                self.bible_search.jobs.insert(
                    job,
                    BibleJob {
                        tab,
                        generation,
                        kind: JobKind::Page(page),
                    },
                );
                if let Some(st) = self.search_tab_mut(tab) {
                    st.loading.insert(page);
                }
            }
            #[cfg(target_arch = "wasm32")]
            let _ = (page, generation);
        }
    }

    /// Open a Library hit (#16 rule 14): a Tab of its Resource in the Search Tab's Link set,
    /// else the most recently active Tab of the Resource, else a new Tab goes to the unit's
    /// Key and scrolls to the paragraph. `new_tab` (Ctrl/Cmd) always opens a new Tab.
    /// Returns how it was opened (`link`, `tab` or `new`) and the Tab that shows it.
    pub(crate) fn open_library_hit(
        &mut self,
        tab: TabId,
        row: usize,
        new_tab: bool,
    ) -> Result<(&'static str, Option<TabId>), String> {
        let st = self.search_tab(tab).ok_or("not a Search Tab")?;
        let results = st.library_results.clone().ok_or("no results")?;
        let r = results.rows.get(row).ok_or("no such row")?;
        let id = results
            .groups
            .get(r.group)
            .map(|g| g.id.clone())
            .ok_or("no such Resource")?;
        if let Some(st) = self.search_tab_mut(tab) {
            st.selected = Some(row);
        }
        let kind = TabKind::Resource(id.clone());
        if !new_tab {
            let link = self.workspace.tab(tab).and_then(|t| t.link);
            let linked = link.and_then(|l| {
                self.workspace
                    .most_recent(|t| t.kind == kind && t.link == Some(l))
            });
            let found = match linked {
                Some(t) => Some(("link", t)),
                None => self
                    .workspace
                    .most_recent(|t| t.kind == kind)
                    .map(|t| ("tab", t)),
            };
            if let Some((via, t)) = found {
                self.workspace.focus(t);
                self.workspace
                    .navigate_text(t, r.unit.clone(), r.range.clone(), r.key.clone());
                return Ok((via, Some(t)));
            }
        }
        if self.library.get(&id).is_none() {
            return Err(format!("{id} is no longer in the library"));
        }
        let out = self
            .run("resource.open", &json!({ "id": id, "split": new_tab }))
            .map_err(|e| e.to_string())?;
        let t = out["tab"].as_u64().map(TabId).ok_or("no Tab opened")?;
        self.workspace
            .navigate_text(t, r.unit.clone(), r.range.clone(), r.key.clone());
        Ok(("new", Some(t)))
    }
}

// --------------------------------------------------------------------------------- desktop

#[cfg(not(target_arch = "wasm32"))]
mod desktop {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use obs_search::{HitOrder, SearchIndex, SearchOptions};
    use obs_store::{ObrReader, ResourceStore, SqliteSearchIndex};

    use super::*;
    use crate::jobs::JobContext;

    pub(super) struct RunInput {
        pub query: String,
        /// Word forms on (#180).
        pub word_forms: bool,
        pub searched_count: usize,
        pub resources: Vec<(SearchedResource, PathBuf, SqliteSearchIndex)>,
        pub indexing: Vec<String>,
        pub failed: Vec<(String, String)>,
        pub stale: Vec<String>,
        pub lexicons: Vec<PathBuf>,
    }

    /// The search job: compile the query, search each index best first, group the hits by
    /// Resource and read the first page of snippets.
    pub(super) fn run(input: RunInput, ctx: &JobContext) -> Result<LibraryRunOutput, String> {
        let started = std::time::Instant::now();
        let lookup = crate::bible_search::desktop::lemma_lookup(input.lexicons.clone());
        let parsed = crate::bible_search::parse_query(&input.query, input.word_forms)?;
        let compiled = obs_search::compile(&parsed, &lookup).map_err(|e| e.to_string())?;
        let highlighter = Highlighter::new(&parsed, &lookup);
        let options = SearchOptions {
            documents: Vec::new(),
            order: HitOrder::Relevance,
            limit: None,
        };
        let mut failed = input.failed.clone();
        let mut found = Vec::new();
        let mut obrs = Vec::new();
        for (resource, obr, index) in &input.resources {
            if ctx.is_cancelled() {
                return Err("cancelled".into());
            }
            match index.search(&compiled, &options) {
                Ok(hits) => {
                    if !hits.is_empty() {
                        obrs.push(Some(obr.clone()));
                    }
                    found.push((resource.clone(), hits));
                }
                Err(e) => failed.push((resource.abbreviation.clone(), e.to_string())),
            }
        }
        let (groups, rows) = library_rows(&found);
        let results = Arc::new(LibraryResults {
            query: input.query.clone(),
            searched: input.searched_count,
            groups,
            rows,
            highlighter,
            indexing: input.indexing.clone(),
            failed,
            stale: input.stale.clone(),
            elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        });
        let first_page = page_snippets(&results, &obrs, 0);
        Ok(LibraryRunOutput {
            results,
            first_page,
        })
    }

    /// The snippets of one page of rows (one each), reading each paragraph's unit text and
    /// Tagged words from its Resource (`obrs`: by group).
    pub(super) fn page_snippets(
        results: &LibraryResults,
        obrs: &[Option<PathBuf>],
        page: usize,
    ) -> Vec<Vec<Snippet>> {
        let mut readers: HashMap<usize, Option<ObrReader>> = HashMap::new();
        let mut texts: HashMap<(usize, UnitId), Option<String>> = HashMap::new();
        let start = page * PAGE_ROWS;
        let end = (start + PAGE_ROWS).min(results.rows.len());
        let mut out = Vec::with_capacity(end.saturating_sub(start));
        for row in results.rows.get(start..end).unwrap_or_default() {
            let reader = readers.entry(row.group).or_insert_with(|| {
                obrs.get(row.group)
                    .and_then(Option::as_ref)
                    .and_then(|p| ObrReader::open(p).ok())
            });
            let Some(reader) = reader.as_ref() else {
                out.push(vec![Snippet::default()]);
                continue;
            };
            let text = texts
                .entry((row.group, row.unit.clone()))
                .or_insert_with(|| reader.unit_text(&row.unit).ok().flatten().map(|t| t.1));
            let Some(text) = text.as_deref() else {
                out.push(vec![Snippet::default()]);
                continue;
            };
            let range = row.range.start as usize..row.range.end as usize;
            let spans = reader
                .spans_starting_in(&row.unit, range.clone())
                .unwrap_or_default();
            out.push(vec![paragraph_snippet(
                text,
                range,
                &spans,
                &results.highlighter,
            )]);
        }
        out
    }
}

// -------------------------------------------------------------------------------- commands

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "search.resource",
        label: "Search this Resource",
        shortcut: None,
        params: &[
            TAB,
            ParamSpec {
                name: "id",
                kind: ParamKind::String,
                required: true,
                help: "A Commentary, Lexicon or Dictionary: added to the Library search, or removed if it is in it",
            },
        ],
        enabled: always,
        run: search_resource,
    },
    CommandSpec {
        id: "search.resource_type",
        label: "Search this Resource type",
        shortcut: None,
        params: &[
            TAB,
            ParamSpec {
                name: "value",
                kind: ParamKind::String,
                required: true,
                help: "commentary, lexicon or dictionary: every Resource of the type added to the Library search, or removed",
            },
        ],
        enabled: always,
        run: search_resource_type,
    },
    CommandSpec {
        id: "search.collection",
        label: "Search this Collection",
        shortcut: None,
        params: &[
            TAB,
            ParamSpec {
                name: "name",
                kind: ParamKind::String,
                required: true,
                help: "A Collection: added to the Library search (alone, if every type was searched), or removed",
            },
        ],
        enabled: always,
        run: search_collection,
    },
];

/// The Tab's Library scope after `change`, then the search run again.
fn change_scope(
    command: &'static str,
    s: &mut Session,
    tab: TabId,
    change: impl FnOnce(&mut LibraryScope, &Library) -> Result<(), String>,
) -> Result<Value, CommandError> {
    let library = &s.library;
    let Some(st) = s
        .workspace
        .tab_mut(tab)
        .filter(|t| t.kind == TabKind::Tool(Tool::Search))
        .map(|t| &mut t.search)
    else {
        return Err(failed(command, "not a Search Tab"));
    };
    change(&mut st.library, library).map_err(|e| failed(command, e))?;
    let resources = st.library.resources(library);
    let types: Vec<&str> = st.library.types.iter().map(|t| type_id(*t)).collect();
    let collections = st.library.collections.clone();
    if st
        .error
        .as_ref()
        .is_some_and(|e| e.field == ErrorField::Resources)
    {
        st.error = None;
    }
    let error = if s
        .search_tab(tab)
        .is_some_and(|st| st.mode == SearchMode::Library)
    {
        rerun(s, tab)
    } else {
        None
    };
    Ok(json!({
        "tab": tab.0,
        "types": types,
        "collections": collections,
        "resources": resources,
        "error": error_json(error.as_ref()),
    }))
}

fn search_resource(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "search.resource";
    let tab = tab_param(ID, s, &p)?;
    let id = p.str("id").unwrap_or_default().to_string();
    change_scope(ID, s, tab, |scope, library| {
        scope.toggle_resource(library, &id)
    })
}

fn search_resource_type(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "search.resource_type";
    let tab = tab_param(ID, s, &p)?;
    let v = p.str("value").unwrap_or_default();
    let ty = type_from_id(v).ok_or_else(|| {
        failed(
            ID,
            format!("“{v}” is not commentary, lexicon or dictionary"),
        )
    })?;
    change_scope(ID, s, tab, |scope, library| {
        scope.toggle_type(library, ty);
        Ok(())
    })
}

fn search_collection(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "search.collection";
    let tab = tab_param(ID, s, &p)?;
    let name = p.str("name").unwrap_or_default().to_string();
    change_scope(ID, s, tab, |scope, library| {
        scope.toggle_collection(library, &name)
    })
}
