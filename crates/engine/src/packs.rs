//! Book packs (#415): named groups of related Books in the Open catalogue, such as "The
//! Puritans" or "Confessions & Catechisms", each downloaded as one job. A pack is a rule over
//! the catalogue's Books, written in `data/packs.toml` (its header explains the rule), so it
//! grows when a source collection grows (ADR 0024). Every Book of a pack is still an ordinary
//! Open catalogue entry: it downloads from its publisher, checked by its pin (ADR 0008).

use std::sync::OnceLock;

use obs_model::ResourceType;
use serde::Deserialize;

use crate::open_catalogue::OpenSource;
use crate::session::Session;

const PACKS_TOML: &str = include_str!("../data/packs.toml");

/// The pack of every Book.
pub const ALL_BOOKS: &str = "all-books";

/// A pack with at least this much left to download (or All Books) asks before it starts.
pub const CONFIRM_BYTES: u64 = 250_000_000;

/// A count with thousands separated: "2,818".
pub fn count_words(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A size in words: "24 MB", "1.9 GB".
pub fn size_words(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let mb = bytes as f64 / 1e6;
    if mb >= 1000.0 {
        format!("{:.1} GB", mb / 1000.0)
    } else if mb >= 10.0 {
        format!("{mb:.0} MB")
    } else {
        format!("{mb:.1} MB")
    }
}

/// The collections a pack may be restricted to ([`OpenSource::collection_name`]).
pub const COLLECTIONS: &[&str] = &["EEBO-TCP", "Evans-TCP", "ECCO-TCP", "Gutenberg"];

/// One pack, as written in `data/packs.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pack {
    /// Fixed once listed.
    pub id: String,
    pub title: String,
    pub description: String,
    /// Every Book (then restricted).
    #[serde(default)]
    pub all: bool,
    /// A subject heading contains one, case aside.
    #[serde(default)]
    pub subjects: Vec<String>,
    /// Its author as shown ("John Owen") is one, case aside.
    #[serde(default)]
    pub authors: Vec<String>,
    /// Its title contains one, case aside.
    #[serde(default)]
    pub titles: Vec<String>,
    /// Its Open catalogue entry id is one.
    #[serde(default)]
    pub ids: Vec<String>,
    /// Restriction: its collection is one of these.
    #[serde(default)]
    pub collections: Vec<String>,
    /// Restriction: its language is one of these.
    #[serde(default)]
    pub languages: Vec<String>,
    /// Restriction: its edition was printed in these years, both included.
    #[serde(default)]
    pub printed: Option<[u16; 2]>,
    /// Restriction: its last author, translator or editor died in these years.
    #[serde(default)]
    pub died: Option<[u16; 2]>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PacksFile {
    #[serde(rename = "pack", default)]
    packs: Vec<Pack>,
}

/// Parse a packs file.
pub fn parse_packs(text: &str) -> Result<Vec<Pack>, String> {
    toml::from_str::<PacksFile>(text)
        .map(|f| f.packs)
        .map_err(|e| format!("packs.toml: {e}"))
}

/// The compiled-in packs (empty if they do not parse; a test checks they do).
pub fn packs() -> &'static [Pack] {
    static PACKS: OnceLock<Vec<Pack>> = OnceLock::new();
    PACKS.get_or_init(|| parse_packs(PACKS_TOML).unwrap_or_default())
}

/// The pack `id`.
pub fn pack(id: &str) -> Option<&'static Pack> {
    packs().iter().find(|p| p.id == id)
}

fn within(year: Option<u16>, range: Option<[u16; 2]>) -> bool {
    range.is_none_or(|[from, to]| year.is_some_and(|y| from <= y && y <= to))
}

impl Pack {
    /// Whether Book `s` is in the pack: it matches any inclusion and every restriction.
    pub fn contains(&self, s: &OpenSource) -> bool {
        if s.resource_type() != Some(ResourceType::Book) {
            return false;
        }
        let has = |haystack: &str, needles: &[String]| {
            let haystack = haystack.to_lowercase();
            needles.iter().any(|n| haystack.contains(&n.to_lowercase()))
        };
        let included = self.all
            || s.subjects
                .iter()
                .any(|subject| has(subject, &self.subjects))
            || self
                .authors
                .iter()
                .any(|a| a.to_lowercase() == s.author.to_lowercase())
            || has(&s.title, &self.titles)
            || self.ids.contains(&s.id);
        included
            && (self.collections.is_empty()
                || self.collections.iter().any(|c| c == s.collection_name()))
            && (self.languages.is_empty() || self.languages.contains(&s.language))
            && within(s.printed_year(), self.printed)
            && within(s.died, self.died)
    }

    /// The Books of `sources` in the pack, in catalogue order.
    pub fn books<'a>(&self, sources: &'a [OpenSource]) -> Vec<&'a OpenSource> {
        sources.iter().filter(|s| self.contains(s)).collect()
    }

    /// What is wrong with the pack, for the tests: a malformed id, no title or description,
    /// no inclusion, an unknown collection, a backward range.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut bad = |why: String| out.push(format!("{}: {why}", self.id));
        if !obs_model::is_valid_resource_id(&self.id) {
            bad("the id must be lowercase letters, digits and hyphens".into());
        }
        if self.title.trim().is_empty() || self.description.trim().is_empty() {
            bad("a title and a description are required".into());
        }
        if !self.all
            && self.subjects.is_empty()
            && self.authors.is_empty()
            && self.titles.is_empty()
            && self.ids.is_empty()
        {
            bad("no inclusion: give all, subjects, authors, titles or ids".into());
        }
        for c in &self.collections {
            if !COLLECTIONS.contains(&c.as_str()) {
                bad(format!("unknown collection {c:?}"));
            }
        }
        for (name, range) in [("printed", self.printed), ("died", self.died)] {
            if range.is_some_and(|[from, to]| from > to) {
                bad(format!("{name} runs backwards"));
            }
        }
        out
    }
}

// ------------------------------------------------------------------------------- session

/// A pack as Get Resources shows it: how many of its Books are in the Library, what is left to
/// download, and its download while one runs.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PackSummary {
    pub id: String,
    pub title: String,
    pub description: String,
    /// Its Books in the Open catalogue.
    pub books: usize,
    /// How many of them are in the Library.
    pub installed: usize,
    /// The download size of those not in the Library (bytes; Books of unknown size count 0).
    pub remaining_bytes: u64,
    /// Its download, while it runs: the batch's Import report id, and Books done and in all.
    pub running: Option<PackProgress>,
}

/// What Remove pack would do ([`Session::pack_removal`]): Resource ids.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackRemoval {
    /// Taken out of the Library.
    pub remove: Vec<String>,
    /// Kept: they have Notes or Highlights.
    pub annotated: Vec<String>,
    /// Kept: they are in another pack's Collection too.
    pub in_other_pack: Vec<String>,
}

/// A pack's running download.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PackProgress {
    /// The batch's Import report id, for Cancel (`import.cancel`) and its report Tab.
    pub report: String,
    pub done: u64,
    pub total: u64,
}

impl Session {
    /// The Open catalogue entries downloaded and in the Library, by entry id.
    fn open_installed(&self) -> std::collections::HashSet<&str> {
        self.imports
            .records()
            .filter(|r| {
                r.phase == crate::ImportPhase::Complete
                    && self.library.get(&r.resource_id).is_some()
            })
            .filter_map(|r| r.open_origin.as_ref().map(|o| o.id.as_str()))
            .collect()
    }

    /// Every pack, in `packs.toml`'s order, as Get Resources shows it.
    pub fn pack_summaries(&self) -> Vec<PackSummary> {
        let installed = self.open_installed();
        let sources = self.get_bibles.open_sources();
        let members = self.get_bibles.pack_members();
        packs()
            .iter()
            .zip(members.iter())
            .map(|(p, indexes)| {
                let books: Vec<&OpenSource> =
                    indexes.iter().filter_map(|&i| sources.get(i)).collect();
                let have = books
                    .iter()
                    .filter(|s| installed.contains(s.id.as_str()))
                    .count();
                let remaining_bytes = books
                    .iter()
                    .filter(|s| !installed.contains(s.id.as_str()))
                    .filter_map(|s| s.size)
                    .sum();
                let running = self.imports.packs.get(&p.id).and_then(|report| {
                    self.imports.job_of(report)?;
                    let r = self.imports.report(report)?;
                    Some(PackProgress {
                        report: report.clone(),
                        done: r.done,
                        total: r.total,
                    })
                });
                PackSummary {
                    id: p.id.clone(),
                    title: p.title.clone(),
                    description: p.description.clone(),
                    books: books.len(),
                    installed: have,
                    remaining_bytes,
                    running,
                }
            })
            .collect()
    }

    /// Download every Book of pack `id` not in the Library yet, as one batch with one Import
    /// report; each Book joins the Library, and the pack's Collection, as it comes in. Those
    /// of its Books already in the Library join the Collection at once.
    /// Returns the batch's report id, its Tab and the job.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn download_pack(&mut self, id: &str) -> Result<(String, u64, crate::JobId), String> {
        let pack = pack(id).ok_or_else(|| format!("There is no Book pack {id:?}"))?;
        if let Some(report) = self.imports.packs.get(id)
            && self.imports.job_of(report).is_some()
        {
            return Err(format!("{} is already downloading", pack.title));
        }
        // The pack's Books already in the Library go in its Collection now; the rest as they
        // come in.
        let members: std::collections::HashSet<&str> = pack
            .books(self.get_bibles.open_sources())
            .into_iter()
            .map(|s| s.id.as_str())
            .collect();
        let already: Vec<String> = self
            .imports
            .records()
            .filter(|r| {
                r.phase == crate::ImportPhase::Complete
                    && self.library.get(&r.resource_id).is_some()
                    && r.open_origin
                        .as_ref()
                        .is_some_and(|o| members.contains(o.id.as_str()))
            })
            .map(|r| r.resource_id.clone())
            .collect();
        self.add_to_collection(&pack.title, &already);
        let installed = self.open_installed();
        let busy: std::collections::HashSet<String> = self
            .imports
            .records()
            .filter(|r| self.imports.job_of(&r.resource_id).is_some())
            .filter_map(|r| r.open_origin.as_ref().map(|o| o.id.clone()))
            .collect();
        let sources: Vec<OpenSource> = pack
            .books(self.get_bibles.open_sources())
            .into_iter()
            .filter(|s| !installed.contains(s.id.as_str()) && !busy.contains(&s.id))
            .cloned()
            .collect();
        if sources.is_empty() {
            return Err(format!(
                "Every Book of {} is in your Library already",
                pack.title
            ));
        }
        let stream = crate::books::PackStream::new(sources, self.fetch(), self.pack_pause);
        let (report, tab, job) = self.start_book_batch(
            Box::new(stream),
            format!(
                "Book pack \u{201c}{}\u{201d} from the Open catalogue",
                pack.title
            ),
            &pack.title,
        )?;
        self.imports.packs.insert(pack.id.clone(), report.clone());
        Ok((report, tab, job))
    }

    /// Whether downloading pack `id` asks first: All Books, or [`CONFIRM_BYTES`] or more left
    /// to download. When it does, the question is pushed as [`crate::Effect::ConfirmPack`].
    pub fn ask_before_pack(&mut self, id: &str) -> bool {
        let Some(p) = self.pack_summaries().into_iter().find(|p| p.id == id) else {
            return false;
        };
        let left = p.books - p.installed;
        if left == 0 || (id != ALL_BOOKS && p.remaining_bytes < CONFIRM_BYTES) {
            return false;
        }
        let books = if left == 1 { "Book" } else { "Books" };
        self.workspace.push_effect(crate::Effect::ConfirmPack {
            pack: p.id.clone(),
            title: format!("Download {}?", p.title),
            message: format!(
                "{} {books}, about {}, each from its publisher and checked before it is \
                 added. They come in one at a time while you keep working, and go in your \
                 Library and in a Collection named \u{201c}{}\u{201d}. Cancel stops between \
                 Books; those already in stay.",
                count_words(left),
                size_words(p.remaining_bytes),
                p.title
            ),
            remove: false,
        });
        true
    }

    /// Pack `id`'s Books in the Library (Resource ids), split into those Remove pack takes out
    /// and those it keeps: a Book with Notes or Highlights, or in another pack's Collection.
    pub fn pack_removal(&self, id: &str) -> PackRemoval {
        let mut out = PackRemoval::default();
        let Some(this) = pack(id) else {
            return out;
        };
        let members: std::collections::HashSet<&str> = this
            .books(self.get_bibles.open_sources())
            .into_iter()
            .map(|s| s.id.as_str())
            .collect();
        let others: Vec<&Pack> = packs()
            .iter()
            .filter(|p| p.id != this.id && self.library.collection(&p.title).is_some())
            .collect();
        for r in self.imports.records() {
            let Some(origin) = &r.open_origin else {
                continue;
            };
            if r.phase != crate::ImportPhase::Complete
                || self.library.get(&r.resource_id).is_none()
                || !members.contains(origin.id.as_str())
            {
                continue;
            }
            let rid = r.resource_id.clone();
            if self.annotations_referring(&rid) > 0 {
                out.annotated.push(rid);
            } else if others
                .iter()
                .any(|p| self.library.in_collection(&p.title, &rid))
            {
                out.in_other_pack.push(rid);
            } else {
                out.remove.push(rid);
            }
        }
        out
    }

    /// Remove pack `id`'s Books from the Library, but those [`Self::pack_removal`] keeps, and
    /// take them out of the pack's Collection (deleted once it holds nothing). Returns how
    /// many were removed.
    pub(crate) fn remove_pack(&mut self, id: &str) -> Result<usize, String> {
        let this = pack(id).ok_or_else(|| format!("There is no Book pack {id:?}"))?;
        if let Some(report) = self.imports.packs.get(id)
            && self.imports.job_of(report).is_some()
        {
            return Err(format!(
                "{} is downloading. Cancel it first, then remove it.",
                this.title
            ));
        }
        let removal = self.pack_removal(id);
        let mut removed = Vec::new();
        for rid in &removal.remove {
            match self.run(
                "library.remove",
                &serde_json::json!({"id": rid, "confirm": true}),
            ) {
                Ok(_) => removed.push(rid.clone()),
                Err(e) => self
                    .workspace
                    .push_effect(crate::Effect::Message(e.to_string())),
            }
        }
        self.remove_from_collection(&this.title, &removed);
        Ok(removed.len())
    }

    /// The pack whose download batch is `report`.
    pub(crate) fn pack_of_batch(&self, report: &str) -> Option<&'static Pack> {
        let id = self
            .imports
            .packs
            .iter()
            .find_map(|(pack, r)| (r == report).then_some(pack))?;
        pack(id)
    }
}

#[cfg(test)]
#[path = "packs_tests.rs"]
mod tests;
