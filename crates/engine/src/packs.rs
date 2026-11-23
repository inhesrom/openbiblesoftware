//! Book packs (#415): named groups of related Books in the Open catalogue, such as "The
//! Puritans" or "Confessions & Catechisms", each downloaded as one job. A pack is a rule over
//! the catalogue's Books, written in `data/packs.toml` (its header explains the rule), so it
//! grows when a source collection grows (ADR 0024). Every Book of a pack is still an ordinary
//! Open catalogue entry: it downloads from its publisher, checked by its pin (ADR 0008).

use std::sync::OnceLock;

use obs_model::ResourceType;
use serde::Deserialize;

use crate::open_catalogue::OpenSource;

const PACKS_TOML: &str = include_str!("../data/packs.toml");

/// The pack of every Book.
pub const ALL_BOOKS: &str = "all-books";

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

#[cfg(test)]
#[path = "packs_tests.rs"]
mod tests;
