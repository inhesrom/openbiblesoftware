//! The Command box's Search preview (#295): as the user types, the first Verses of the ★ Bible
//! (first in Preferred order) that match the text, shown under the dropdown's Search row.
//!
//! The UI says each frame what the box would search ([`Session::update_search_preview`],
//! with its clock). The search waits until the typing has stopped for [`PREVIEW_DEBOUNCE`]
//! seconds, then runs as a background job over the ★ Bible's Search index, as the Search
//! Tool's searches do, never on the UI thread. Until the new hits arrive the last ones stay,
//! so the list does not jump while the user types. Without a Bible the preview says so; while
//! the ★ Bible's index is being built it says that, and searches again once it is ready.

use std::collections::BTreeSet;
use std::sync::Arc;

use obs_model::ResourceType;
use obs_scripture::Verse;

use crate::bible_search::Snippet;
use crate::jobs::{JobEvent, JobEventKind, JobId};
use crate::session::Session;

/// How many Verses the Search preview shows.
pub const PREVIEW_HITS: usize = 5;

/// How long, in seconds, the typing must stop before the Search preview searches: short
/// enough that hits show within about 150 ms of the last key.
pub const PREVIEW_DEBOUNCE: f64 = 0.1;

/// What the Search preview shows.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum SearchPreview {
    /// Nothing: no text to search, a query the Search Tool would refuse, or a ★ Bible that
    /// cannot be searched here.
    #[default]
    Off,
    /// The library has no Bible: the preview offers Get Bibles instead.
    NoBible,
    /// The ★ Bible's Search index is being built: its abbreviation.
    Indexing(String),
    /// What the ★ Bible has for the text.
    Hits(Arc<PreviewHits>),
}

/// The ★ Bible's hits for one text.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewHits {
    /// The text searched.
    pub query: String,
    /// The ★ Bible's abbreviation.
    pub bible: String,
    /// Whether the ★ Bible is written right to left: its Verses are laid out from the right,
    /// as the Search Tool lays them out.
    pub rtl: bool,
    /// How many Verses match in all.
    pub total: usize,
    /// The first [`PREVIEW_HITS`] of them, in canonical order.
    pub hits: Vec<PreviewHit>,
}

/// One Verse of the Search preview.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewHit {
    /// `Psalms 119:3`: the Standard Verse, or the Bible's own for an Unmapped verse.
    pub reference: String,
    /// The Verse's text with the hits highlighted.
    pub snippet: Snippet,
    /// The Standard Verse, if the Verse has one.
    pub(crate) standard: Option<Verse>,
    /// The Verse in its Bible's own numbering.
    pub(crate) native: Verse,
    /// The Bible's resource id.
    pub(crate) bible: String,
}

impl PreviewHit {
    /// The `command_box.open_hit` parameters that open this Verse, whatever the Search
    /// preview shows by the time they run.
    pub fn open_params(&self, new_tab: bool) -> serde_json::Value {
        let mut params = serde_json::json!({
            "bible": self.bible,
            "verse": self.native.osis(),
            "new_tab": new_tab,
        });
        if let Some(standard) = self.standard {
            params["standard"] = standard.osis().into();
        }
        params
    }
}

/// The Search preview's state.
#[derive(Debug, Default)]
pub(crate) struct Preview {
    /// The text to preview, and when (the UI's clock, seconds) it last changed.
    typed: Option<(String, f64)>,
    /// The text last searched: its hits are shown, or its job is running.
    searched: Option<String>,
    /// The job of the newest search.
    job: Option<JobId>,
    /// Every preview job not finished yet (older ones cancelled), so their events are not
    /// passed on.
    jobs: BTreeSet<JobId>,
    shown: SearchPreview,
    /// The ★ Bible whose index was being built when `searched` was looked for.
    waiting_for: Option<String>,
}

impl Session {
    /// The Command box's text to preview a search of (`None`: the dropdown is closed or offers
    /// no search) at time `now` (seconds, the UI's clock). Call it every frame the dropdown
    /// shows: the search starts once the text has not changed for [`PREVIEW_DEBOUNCE`], and
    /// again when the ★ Bible's Search index it waited for is ready.
    pub fn update_search_preview(&mut self, text: Option<&str>, now: f64) {
        let Some(text) = text.map(str::trim).filter(|t| !t.is_empty()) else {
            self.stop_search_preview();
            return;
        };
        let p = &mut self.command_box.preview;
        if p.typed.as_ref().is_none_or(|(t, _)| t != text) {
            p.typed = Some((text.to_string(), now));
            if let Some(job) = p.job.take() {
                // Its hits never come: that text must be searched again if it is typed again.
                p.searched = None;
                self.jobs().cancel(job);
            }
        }
        let p = &self.command_box.preview;
        let since = p.typed.as_ref().map_or(0.0, |(_, at)| now - at);
        let due = p.searched.as_deref() != Some(text) && since >= PREVIEW_DEBOUNCE;
        let index_ready = (p.waiting_for.as_deref()).is_some_and(|id| self.search.is_ready(id));
        if due || index_ready {
            self.start_search_preview(text.to_string());
        }
    }

    /// What the Search preview shows.
    pub fn search_preview(&self) -> &SearchPreview {
        &self.command_box.preview.shown
    }

    /// The text the Search preview last searched (its hits shown, or being searched).
    pub fn search_preview_query(&self) -> Option<&str> {
        self.command_box.preview.searched.as_deref()
    }

    /// Whether a Search preview search is running.
    pub fn search_preview_running(&self) -> bool {
        self.command_box.preview.job.is_some()
    }

    /// How long after `now` the Search preview searches the text typed (`None`: it is
    /// searched already), for the UI to draw again then.
    pub fn search_preview_due_in(&self, now: f64) -> Option<f64> {
        let p = &self.command_box.preview;
        let (text, at) = p.typed.as_ref()?;
        (p.searched.as_ref() != Some(text)).then(|| (PREVIEW_DEBOUNCE - (now - at)).max(0.0))
    }

    /// Forget the Search preview (the dropdown closed), cancelling its search.
    fn stop_search_preview(&mut self) {
        let p = std::mem::take(&mut self.command_box.preview);
        if let Some(job) = p.job {
            self.jobs().cancel(job);
        }
        // Their events are still to come: swallowed, not passed on.
        self.command_box.preview.jobs = p.jobs;
    }

    /// Search the ★ Bible for `query` in the background.
    fn start_search_preview(&mut self, query: String) {
        let p = &mut self.command_box.preview;
        p.searched = Some(query.clone());
        p.waiting_for = None;
        if let Some(job) = p.job.take() {
            self.jobs().cancel(job);
        }
        let shown = self.submit_search_preview(query);
        if let Some(shown) = shown {
            self.command_box.preview.shown = shown;
        }
    }

    /// Submit the preview search, or say why there is none (`Some`); `None` while it runs.
    #[cfg(not(target_arch = "wasm32"))]
    fn submit_search_preview(&mut self, query: String) -> Option<SearchPreview> {
        use crate::bible_search::{Scope, WHOLE_BIBLE, check_query, desktop, outline_key};
        use crate::search::{NotReady, Opened};
        let Some(entry) = self.library.preferred(ResourceType::Bible) else {
            return Some(SearchPreview::NoBible);
        };
        let Ok(query) = check_query(&query) else {
            return Some(SearchPreview::Off);
        };
        let id = entry.manifest.resource_id.clone();
        let bible = desktop::JobBible {
            outline_key: outline_key(entry),
            abbreviation: entry.manifest.abbreviation.clone(),
            rtl: entry.manifest.direction == obs_model::Direction::Rtl,
            obr: entry.path.clone(),
            id: id.clone(),
        };
        let index = match self.open_search_index(&id) {
            Ok(Opened::Current(index) | Opened::Stale(index)) => index,
            Err(NotReady::Indexing) => {
                self.command_box.preview.waiting_for = Some(id);
                return Some(SearchPreview::Indexing(bible.abbreviation));
            }
            Err(NotReady::Failed(_) | NotReady::NotInLibrary) => return Some(SearchPreview::Off),
        };
        let input = desktop::RunInput {
            query,
            word_forms: self.search_word_forms(),
            scope_text: WHOLE_BIBLE.to_string(),
            scope: Scope::WholeBible,
            bibles: vec![(bible, index)],
            indexing: Vec::new(),
            failed: Vec::new(),
            stale: Vec::new(),
            lexicons: (self.library.of_type(ResourceType::Lexicon))
                .map(|e| e.path.clone())
                .collect(),
            outlines: Arc::clone(&self.bible_search.outlines),
            snippet_rows: PREVIEW_HITS,
        };
        let job = self
            .jobs()
            .submit_fn("Search preview", move |ctx| desktop::run(input, ctx));
        let p = &mut self.command_box.preview;
        p.job = Some(job);
        p.jobs.insert(job);
        None
    }

    /// Searching needs the desktop app: no preview.
    #[cfg(target_arch = "wasm32")]
    fn submit_search_preview(&mut self, _: String) -> Option<SearchPreview> {
        Some(SearchPreview::Off)
    }

    /// Apply the Search preview's job events; return the others.
    pub(crate) fn poll_search_preview(&mut self, events: Vec<JobEvent>) -> Vec<JobEvent> {
        let mut other = Vec::new();
        for event in events {
            if !self.command_box.preview.jobs.contains(&event.id) {
                other.push(event);
                continue;
            }
            if !event.kind.is_terminal() {
                continue;
            }
            self.command_box.preview.jobs.remove(&event.id);
            if self.command_box.preview.job != Some(event.id) {
                continue;
            }
            self.command_box.preview.job = None;
            let mut kind = event.kind;
            if let JobEventKind::Finished(out) = &mut kind {
                self.repair_damaged_indexes(out);
            }
            let shown = match kind {
                JobEventKind::Finished(out) => preview_of(out),
                _ => SearchPreview::Off,
            };
            if let SearchPreview::Indexing(_) = shown {
                self.command_box.preview.waiting_for =
                    (self.library.preferred(ResourceType::Bible))
                        .map(|e| e.manifest.resource_id.clone());
            }
            self.command_box.preview.shown = shown;
        }
        other
    }
}

/// What a finished preview search shows.
fn preview_of(out: crate::jobs::JobOutput) -> SearchPreview {
    let Ok(out) = out.downcast::<crate::bible_search::RunOutput>() else {
        return SearchPreview::Off;
    };
    let results = &out.results;
    // Its index failed the search and is being built again.
    if let Some(abbreviation) = results.indexing.first() {
        return SearchPreview::Indexing(abbreviation.clone());
    }
    let Some(bible) = results.bibles.first() else {
        return SearchPreview::Off;
    };
    let hits = (results.rows.iter().zip(&out.first_page))
        .take(PREVIEW_HITS)
        .filter_map(|(row, snippets)| {
            let hit = row.hits.first()?;
            Some(PreviewHit {
                reference: results.row_reference(row),
                snippet: snippets.first().cloned().unwrap_or_default(),
                standard: row.standard,
                native: hit.native,
                bible: bible.id.clone(),
            })
        })
        .collect();
    SearchPreview::Hits(Arc::new(PreviewHits {
        query: results.query.clone(),
        bible: bible.abbreviation.clone(),
        rtl: bible.rtl,
        total: results.rows.len(),
        hits,
    }))
}
