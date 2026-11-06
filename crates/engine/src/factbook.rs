//! The Factbook (#149): a Guide whose Key is one person, people group or place of a People &
//! places index (TIPNR), shown as a hub page.
//!
//! - **Key:** the entity's id in its index (TIPNR's unique name, `Abraham@Gen.11.26-1Pe`), as
//!   a Headword Key in the Tab's entry history. A Factbook is never in a Link set.
//! - **Header:** the usual name, what tells it from others of the same name ("son of
//!   Zebedee"), its kind and region, and where the Bible first and last names it.
//! - **Sections:** Summary (other names, the meaning of the name from a names Dictionary such
//!   as Hitchcock's, and family), Key Passages (every Passage that mentions it), Dictionary
//!   articles, Related people and places, and Map (#150: a place's locations, or the places a
//!   person lived in, from [`crate::atlas`]).
//! - **Chooser:** a name typed into the header box (or one name shared by several entities)
//!   lists every entity with that name, each told apart by its structured facts.
//!
//! Only TIPNR's structured fields are read: its descriptions, partly AI-written, are never
//! stored (#15), so nothing here can show them. Every lookup reads the [`PeopleDirectory`]:
//! all entities of the library's People & places indexes with their mention counts, loaded
//! once per library on the job queue and shared by the Session and the section jobs.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use obs_model::{Direction, Entity, EntityKind, Key, Relation};
use obs_scripture::{Passage, RefStyle, Standard, StrongsNumber, Verse, format_passage};
use serde_json::{Value, json};

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::people_places::{ArticleLink, display_name, kind_name, relation_label, usual_name};
use crate::workspace::{Guide, OpenRequest, PanelId, TabId, TabKind};
use crate::{JobEvent, JobEventKind, JobId, Library, Session, WordStatus};

/// Dictionaries whose article on a name gives its meaning, by resource id: Hitchcock's Bible
/// Names Dictionary, as the Starter library ships it and as SWORD Import names the CrossWire
/// module.
pub const NAME_MEANING_DICTIONARIES: &[&str] = &["hitchcock"];

/// Longest Dictionary article excerpt kept in the Dictionary articles section.
pub const ARTICLE_CHARS: usize = 600;

/// Longest name meaning kept.
pub const MEANING_CHARS: usize = 240;

/// Most rows in Related people and places that come from being named together.
pub const NAMED_TOGETHER: usize = 10;

/// One entity of a People & places index with how often and where the Bible names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryEntry {
    /// The People & places index it comes from.
    pub index_id: String,
    pub entity: Entity,
    /// Mentions (one per Verse).
    pub passages: u32,
    /// The first and last Verses that mention it.
    pub first: Option<Verse>,
    pub last: Option<Verse>,
}

/// Every entity of the library's People & places indexes, in Preferred order (an entity two
/// indexes list is kept from the first, with the Place locations of the first that has some).
#[derive(Debug, Default)]
pub struct PeopleDirectory {
    entries: Vec<DirectoryEntry>,
    by_id: HashMap<String, usize>,
    /// Normalized name → entries with that usual name or other name.
    by_name: HashMap<String, Vec<usize>>,
    /// Entity id → entities whose relations name it, with the relation they give.
    listed_by: HashMap<String, Vec<(usize, Relation)>>,
}

/// One entity as a line: a chooser row, the Factbook's header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityCard {
    pub entity: String,
    pub name: String,
    /// What tells it from others of the same name, from structured facts only: `son of
    /// Zebedee`, `wife of Joseph`, or a place's region.
    pub distinction: Option<String>,
    pub kind: EntityKind,
    pub region: Option<String>,
    pub passages: u32,
    pub first: Option<Verse>,
    pub last: Option<Verse>,
    /// Set when the entity matched a chooser query by another of its names (`Mark`, also
    /// called `John`).
    pub also_called: Option<String>,
}

impl EntityCard {
    /// `John (son of Zebedee)`.
    pub fn title(&self) -> String {
        match &self.distinction {
            Some(d) => format!("{} ({d})", self.name),
            None => self.name.clone(),
        }
    }

    /// `Man · Tribe of Judah · Mt 4:21 – Rev 1:9 · 35 Verses`.
    pub fn details(&self) -> String {
        let mut parts = vec![kind_name(self.kind).to_string()];
        if let Some(r) = &self.region
            && self.distinction.as_ref() != Some(r)
        {
            parts.push(r.clone());
        }
        let short = |v: Verse| format_passage(Passage::verse(v), &Standard, RefStyle::Short);
        match (self.first, self.last) {
            (Some(a), Some(b)) if a != b => parts.push(format!("{} – {}", short(a), short(b))),
            (Some(a), _) => parts.push(short(a)),
            _ => {}
        }
        parts.push(match self.passages {
            1 => "1 Verse".into(),
            n => format!("{n} Verses"),
        });
        parts.join(" · ")
    }
}

/// The entities a name typed into a Factbook stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactbookChooser {
    pub query: String,
    /// Entities called exactly that first (most often named first), then those that only
    /// start with it.
    pub choices: Vec<EntityCard>,
}

/// A person or place a Factbook links to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityLink {
    pub entity: String,
    pub name: String,
    /// The source marks the link as a decision on an ambiguous text.
    pub uncertain: bool,
    /// Whether the directory has a Factbook for it (unnamed relatives may have none).
    pub known: bool,
}

/// One line of family in the Summary: `Father: Terah`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyFact {
    pub label: &'static str,
    pub people: Vec<EntityLink>,
}

/// What a name means, from a names Dictionary (Hitchcock's).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameMeaning {
    pub article: ArticleLink,
    /// The article without its headword: `father of a great multitude`.
    pub text: String,
}

/// The Factbook's Summary section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactbookSummary {
    /// Other names and spellings (`Abram`).
    pub other_names: Vec<String>,
    pub meaning: Option<NameMeaning>,
    /// Parents, siblings, spouses, children (a place: its founders), in that order.
    pub family: Vec<FamilyFact>,
}

impl FactbookSummary {
    pub fn is_empty(&self) -> bool {
        self.other_names.is_empty() && self.meaning.is_none() && self.family.is_empty()
    }
}

/// One Dictionary's article on the entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictionaryArticle {
    pub article: ArticleLink,
    /// The start of the article, whitespace collapsed, at most [`ARTICLE_CHARS`].
    pub excerpt: String,
    pub direction: Direction,
}

/// One row of Related people and places.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelatedEntity {
    pub card: EntityCard,
    /// How it relates: `Lived here`, `Lived in`, `Named together in 12 Verses`.
    pub why: String,
}

/// Relations shown in the Summary's family, in this order.
const FAMILY: [Relation; 7] = [
    Relation::Father,
    Relation::Mother,
    Relation::Ancestor,
    Relation::Sibling,
    Relation::Partner,
    Relation::Offspring,
    Relation::Founder,
];

/// What `other` is to an entity whose record lists it with `relation` — read from `other`'s
/// side: an entity listed by its child as Father is the child's parent.
fn reverse_label(relation: Relation) -> &'static str {
    match relation {
        Relation::Father | Relation::Mother => "Child",
        Relation::Ancestor => "Descendant",
        Relation::Sibling => "Sibling",
        Relation::Partner => "Married to",
        Relation::Offspring => "Parent",
        Relation::Founded => "Founded by",
        Relation::Founder => "Founded",
        Relation::Inhabitant => "Lived in",
    }
}

impl PeopleDirectory {
    pub fn new(entries: Vec<DirectoryEntry>) -> Self {
        let mut d = Self::default();
        for e in entries {
            let mut names: BTreeSet<String> = crate::aliases::entity_names(&e.entity)
                .iter()
                .map(|n| obs_model::normalize(n))
                .collect();
            names.insert(obs_model::normalize(&usual_name(&e.entity)));
            names.retain(|n| !n.is_empty());
            if let Some(&kept) = d.by_id.get(&e.entity.id) {
                // A place a later index locates (the geocoding, #150) takes its locations, and
                // its names find the first record.
                for n in names {
                    let found = d.by_name.entry(n).or_default();
                    if !found.contains(&kept) {
                        found.push(kept);
                    }
                }
                let kept = &mut d.entries[kept].entity;
                if kept.locations.is_empty() {
                    kept.locations = e.entity.locations;
                }
                continue;
            }
            let i = d.entries.len();
            d.by_id.insert(e.entity.id.clone(), i);
            for n in names {
                d.by_name.entry(n).or_default().push(i);
            }
            for r in &e.entity.relations {
                d.listed_by
                    .entry(r.entity.clone())
                    .or_default()
                    .push((i, r.relation));
            }
            d.entries.push(e);
        }
        d
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, id: &str) -> Option<&DirectoryEntry> {
        self.by_id.get(id).map(|&i| &self.entries[i])
    }

    /// The usual name of entity `id` (`Abraham`), or the name its id stands for.
    pub fn name_of(&self, id: &str) -> String {
        self.get(id)
            .map_or_else(|| display_name(id), |e| usual_name(&e.entity))
    }

    /// A name worth telling an entity apart by: a named relative's usual name. TIPNR writes
    /// unnamed relatives in lower case (`father_of_Lazarus`).
    fn proper_name(&self, id: &str) -> Option<String> {
        let name = self.name_of(id);
        name.chars()
            .next()
            .is_some_and(char::is_uppercase)
            .then_some(name)
    }

    /// What tells `e` from others of the same name, from its structured facts: a parent,
    /// else a child, spouse, sibling or forebear, else its region.
    pub fn distinction(&self, e: &Entity) -> Option<String> {
        if e.kind == EntityKind::Place {
            return e.region.clone();
        }
        let first = |relation: Relation| {
            e.relations
                .iter()
                .filter(|r| r.relation == relation && !r.uncertain)
                .find_map(|r| self.proper_name(&r.entity))
        };
        let (child, parent, spouse, sibling) = match e.kind {
            EntityKind::Male => ("son", "father", "husband", "brother"),
            EntityKind::Female => ("daughter", "mother", "wife", "sister"),
            _ => ("descended from", "forebear", "related", "related"),
        };
        if let Some(p) = first(Relation::Father).or_else(|| first(Relation::Mother)) {
            return Some(if e.kind == EntityKind::Group {
                format!("descended from {p}")
            } else {
                format!("{child} of {p}")
            });
        }
        if let Some(c) = first(Relation::Offspring) {
            return Some(format!("{parent} of {c}"));
        }
        if let Some(p) = first(Relation::Partner) {
            return Some(format!("{spouse} of {p}"));
        }
        if let Some(s) = first(Relation::Sibling) {
            return Some(format!("{sibling} of {s}"));
        }
        if let Some(a) = first(Relation::Ancestor) {
            return Some(format!("descended from {a}"));
        }
        e.region.clone()
    }

    /// The places whose records list `id` as someone who lived there, most often named
    /// first.
    pub fn places_lived_in(&self, id: &str) -> Vec<String> {
        let mut places: Vec<&DirectoryEntry> = self
            .listed_by
            .get(id)
            .into_iter()
            .flatten()
            .filter(|(_, r)| *r == Relation::Inhabitant)
            .map(|(i, _)| &self.entries[*i])
            .filter(|e| e.entity.kind == EntityKind::Place)
            .collect();
        places.sort_by(|a, b| {
            b.passages
                .cmp(&a.passages)
                .then_with(|| a.entity.id.cmp(&b.entity.id))
        });
        places.dedup_by(|a, b| a.entity.id == b.entity.id);
        places.into_iter().map(|e| e.entity.id.clone()).collect()
    }

    pub fn card(&self, id: &str) -> Option<EntityCard> {
        let e = self.get(id)?;
        Some(EntityCard {
            entity: e.entity.id.clone(),
            name: usual_name(&e.entity),
            distinction: self.distinction(&e.entity),
            kind: e.entity.kind,
            region: e.entity.region.clone(),
            passages: e.passages,
            first: e.first,
            last: e.last,
            also_called: None,
        })
    }

    /// The entities called `query`: by usual name first, then by another name, each group
    /// most often named first. With none called exactly that, the names that start with it.
    pub fn choose(&self, query: &str) -> FactbookChooser {
        let query = query.trim();
        let q = obs_model::normalize(&query.replace('_', " "));
        let mut choices = Vec::new();
        if !q.is_empty() {
            let mut found: Vec<(usize, bool)> = Vec::new();
            if let Some(list) = self.by_name.get(&q) {
                found.extend(list.iter().map(|&i| (i, true)));
            } else {
                let mut starts: Vec<&String> =
                    self.by_name.keys().filter(|n| n.starts_with(&q)).collect();
                starts.sort();
                for n in starts {
                    for &i in &self.by_name[n] {
                        if !found.iter().any(|(j, _)| *j == i) {
                            found.push((i, false));
                        }
                    }
                }
            }
            for (i, _) in &found {
                let e = &self.entries[*i];
                let Some(mut card) = self.card(&e.entity.id) else {
                    continue;
                };
                let usual = obs_model::normalize(&card.name);
                if usual != q && !usual.starts_with(&q) {
                    card.also_called = crate::aliases::entity_names(&e.entity)
                        .into_iter()
                        .find(|n| obs_model::normalize(n).starts_with(&q));
                }
                choices.push(card);
            }
            choices.sort_by(|a, b| {
                a.also_called
                    .is_some()
                    .cmp(&b.also_called.is_some())
                    .then(b.passages.cmp(&a.passages))
                    .then(a.first.cmp(&b.first))
                    .then(a.entity.cmp(&b.entity))
            });
        }
        FactbookChooser {
            query: query.to_string(),
            choices,
        }
    }

    /// The entities with Strong's number `n`, as their unified number or the number of another
    /// of their names (Abram `H87`, Greek Abraham `G11`): the exact (extended) number when one
    /// has it, those whose unified number it is first; else every entity under the same base
    /// number.
    pub fn with_strongs(&self, n: StrongsNumber) -> Vec<&DirectoryEntry> {
        let numbers = |e: &DirectoryEntry| {
            let other = e.entity.other_strongs.iter().copied();
            e.entity
                .strongs
                .into_iter()
                .chain(other)
                .collect::<Vec<_>>()
        };
        let mut exact: Vec<&DirectoryEntry> = self
            .entries
            .iter()
            .filter(|e| numbers(e).contains(&n))
            .collect();
        if !exact.is_empty() {
            exact.sort_by_key(|e| e.entity.strongs != Some(n));
            return exact;
        }
        let mut base: Vec<&DirectoryEntry> = self
            .entries
            .iter()
            .filter(|e| numbers(e).iter().any(|s| s.base() == n.base()))
            .collect();
        base.sort_by_key(|a| std::cmp::Reverse(a.passages));
        base
    }

    /// The Summary: other names, the name's meaning (from `meaning`) and family.
    pub fn summary(&self, id: &str, meaning: Option<NameMeaning>) -> Option<FactbookSummary> {
        let e = &self.get(id)?.entity;
        let usual = usual_name(e);
        let other_names = crate::aliases::entity_names(e)
            .into_iter()
            .filter(|n| *n != usual && obs_model::normalize(n) != obs_model::normalize(&usual))
            .collect();
        let mut family: Vec<FamilyFact> = Vec::new();
        for relation in FAMILY {
            let mut people: Vec<EntityLink> = Vec::new();
            for r in e.relations.iter().filter(|r| r.relation == relation) {
                if people.iter().any(|p| p.entity == r.entity) {
                    continue;
                }
                people.push(self.link(&r.entity, r.uncertain));
            }
            if !people.is_empty() {
                family.push(FamilyFact {
                    label: relation_label(relation),
                    people,
                });
            }
        }
        Some(FactbookSummary {
            other_names,
            meaning,
            family,
        })
    }

    fn link(&self, id: &str, uncertain: bool) -> EntityLink {
        EntityLink {
            entity: id.to_string(),
            name: self.name_of(id),
            uncertain,
            known: self.get(id).is_some(),
        }
    }

    /// Related people and places: the entity's own links that are not family (people who
    /// lived in a place, places a person founded), the entities whose records link to it
    /// (and are not already in its family), then up to [`NAMED_TOGETHER`] most often named in
    /// the same Verses (`together`: entity id → shared Verses).
    pub fn related(&self, id: &str, together: &BTreeMap<String, u32>) -> Vec<RelatedEntity> {
        let Some(entry) = self.get(id) else {
            return Vec::new();
        };
        let e = &entry.entity;
        let mut seen: BTreeSet<String> = e
            .relations
            .iter()
            .filter(|r| FAMILY.contains(&r.relation))
            .map(|r| r.entity.clone())
            .collect();
        seen.insert(id.to_string());
        let mut out: Vec<RelatedEntity> = Vec::new();
        let mut push = |other: &str, why: String, seen: &mut BTreeSet<String>| {
            if seen.insert(other.to_string())
                && let Some(card) = self.card(other)
            {
                out.push(RelatedEntity { card, why });
            }
        };
        for r in e.relations.iter().filter(|r| !FAMILY.contains(&r.relation)) {
            push(&r.entity, relation_label(r.relation).to_string(), &mut seen);
        }
        let mut listing: Vec<&(usize, Relation)> = self
            .listed_by
            .get(id)
            .map(|l| l.iter().collect())
            .unwrap_or_default();
        listing.sort_by_key(|(i, r)| (*r, std::cmp::Reverse(self.entries[*i].passages)));
        for (i, relation) in listing {
            let other = &self.entries[*i].entity.id;
            push(other, reverse_label(*relation).to_string(), &mut seen);
        }
        let mut named: Vec<(&String, &u32)> = together.iter().collect();
        named.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        let mut added = 0;
        for (other, n) in named {
            if added == NAMED_TOGETHER {
                break;
            }
            let why = match n {
                1 => "Named together in 1 Verse".to_string(),
                n => format!("Named together in {n} Verses"),
            };
            let before = seen.len();
            push(other, why, &mut seen);
            if seen.len() > before {
                added += 1;
            }
        }
        out
    }
}

/// The text of a names-Dictionary article without its headword: `ABRAHAM, father of a great
/// multitude` → `father of a great multitude`. A name the Dictionary explains twice (Noah,
/// Zelophehad's daughter and the patriarch) keeps both, as `first · second`.
pub fn meaning_text(headword: &str, article: &str) -> String {
    let senses: Vec<String> = article
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect();
    let mut text = senses.join(" · ");
    if let Some(head) = text.get(..headword.len())
        && obs_model::normalize(head) == obs_model::normalize(headword)
    {
        text = text[headword.len()..].to_string();
    }
    let rest = text.trim_start_matches(|c: char| {
        matches!(c, ',' | ':' | '.' | '—' | '-') || c.is_whitespace()
    });
    crate::guide::cut(rest.trim(), MEANING_CHARS)
}

/// The library's People & places indexes as a [`PeopleDirectory`], kept until the indexes
/// change. Shared by the Session (chooser, header, Word Info) and the section jobs.
#[derive(Debug, Default)]
pub struct DirectoryCache {
    cached: Mutex<Option<(Fingerprint, Arc<PeopleDirectory>)>>,
    /// Held while one job builds, so jobs asking at once share one build.
    building: Mutex<()>,
}

type Fingerprint = Vec<(String, u32, String, std::path::PathBuf)>;

fn fingerprint(library: &Library) -> Fingerprint {
    library
        .of_type(obs_model::ResourceType::PeopleAndPlaces)
        .map(|e| {
            (
                e.manifest.resource_id.clone(),
                e.manifest.revision,
                e.manifest.content_hash.clone(),
                e.path.clone(),
            )
        })
        .collect()
}

impl DirectoryCache {
    /// The directory for `library` if it is loaded and current; never reads a file.
    pub fn current(&self, library: &Library) -> Option<Arc<PeopleDirectory>> {
        let cached = self.cached.lock().unwrap_or_else(|p| p.into_inner());
        cached
            .as_ref()
            .filter(|(f, _)| *f == fingerprint(library))
            .map(|(_, d)| Arc::clone(d))
    }

    /// The directory for `library`, read from its indexes when not current (on a worker).
    pub fn get(&self, library: &Library) -> Result<Arc<PeopleDirectory>, String> {
        if let Some(d) = self.current(library) {
            return Ok(d);
        }
        let _building = self.building.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(d) = self.current(library) {
            return Ok(d);
        }
        let d = Arc::new(read_directory(library)?);
        *self.cached.lock().unwrap_or_else(|p| p.into_inner()) =
            Some((fingerprint(library), Arc::clone(&d)));
        Ok(d)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read_directory(library: &Library) -> Result<PeopleDirectory, String> {
    use obs_store::ResourceStore;
    let mut entries = Vec::new();
    let mut errors = Vec::new();
    for index in library.of_type(obs_model::ResourceType::PeopleAndPlaces) {
        let read = obs_store::ObrReader::open(&index.path)
            .and_then(|r| Ok((r.entities()?, r.mention_stats()?)));
        let (entities, stats) = match read {
            Ok(x) => x,
            Err(e) => {
                errors.push(format!("{}: {e}", index.manifest.abbreviation));
                continue;
            }
        };
        let stats: HashMap<String, obs_store::MentionStats> =
            stats.into_iter().map(|s| (s.entity.clone(), s)).collect();
        for entity in entities {
            let s = stats.get(&entity.id);
            entries.push(DirectoryEntry {
                index_id: index.manifest.resource_id.clone(),
                passages: s.map_or(0, |s| s.count),
                first: s.map(|s| s.first),
                last: s.map(|s| s.last),
                entity,
            });
        }
    }
    if entries.is_empty() && !errors.is_empty() {
        return Err(errors.join("; "));
    }
    Ok(PeopleDirectory::new(entries))
}

#[cfg(target_arch = "wasm32")]
fn read_directory(_: &Library) -> Result<PeopleDirectory, String> {
    Err("Opening Resources is not supported here".into())
}

/// The section data of a Factbook at entity `id`.
#[cfg(not(target_arch = "wasm32"))]
pub fn section(
    library: &Library,
    directory: &DirectoryCache,
    aliases: &crate::aliases::AliasCache,
    id: &str,
    section: crate::GuideSection,
) -> Result<crate::SectionData, String> {
    use crate::{GuideSection, SectionData};
    use obs_store::ResourceStore;
    if library
        .of_type(obs_model::ResourceType::PeopleAndPlaces)
        .next()
        .is_none()
    {
        // No index at all: the section is empty, and its empty state says how to get one
        // (#135) instead of failing.
        return match section {
            GuideSection::Summary => Ok(SectionData::FactbookSummary(FactbookSummary {
                other_names: Vec::new(),
                meaning: None,
                family: Vec::new(),
            })),
            GuideSection::KeyPassages => Ok(SectionData::KeyPassages(Vec::new())),
            GuideSection::DictionaryArticles => Ok(SectionData::DictionaryArticles(Vec::new())),
            GuideSection::Related => Ok(SectionData::Related(Vec::new())),
            GuideSection::Map => Ok(SectionData::Map(Vec::new())),
            other => Err(format!(
                "{} needs a Passage or a Strong's number",
                other.title()
            )),
        };
    }
    let dir = directory.get(library)?;
    let entry = dir
        .get(id)
        .ok_or_else(|| format!("{} is in no People & places index", display_name(id)))?;
    let index = library
        .get(&entry.index_id)
        .ok_or_else(|| format!("{} is not in the library", entry.index_id))?;
    match section {
        GuideSection::Summary => {
            let meaning = name_meaning(library, aliases, index, id);
            dir.summary(id, meaning)
                .map(SectionData::FactbookSummary)
                .ok_or_else(|| "no such entity".into())
        }
        GuideSection::KeyPassages => {
            let reader = obs_store::ObrReader::open(&index.path).map_err(|e| e.to_string())?;
            let mentions = reader.mentions_of(id).map_err(|e| e.to_string())?;
            Ok(SectionData::KeyPassages(key_passages(
                mentions.into_iter().map(|m| m.passage),
            )))
        }
        GuideSection::DictionaryArticles => Ok(SectionData::DictionaryArticles(articles(
            library, aliases, index, id,
        ))),
        GuideSection::Related => {
            let reader = obs_store::ObrReader::open(&index.path).map_err(|e| e.to_string())?;
            let mut together: BTreeMap<String, BTreeSet<Verse>> = BTreeMap::new();
            for m in reader.mentions_of(id).map_err(|e| e.to_string())? {
                for other in reader.mentions_in(m.passage).map_err(|e| e.to_string())? {
                    if other.entity == id {
                        continue;
                    }
                    let shared = together.entry(other.entity).or_default();
                    for v in m.passage.verses(&Standard) {
                        if other.passage.contains_verse(v) {
                            shared.insert(v);
                        }
                    }
                }
            }
            let together = together
                .into_iter()
                .map(|(k, v)| (k, u32::try_from(v.len()).unwrap_or(u32::MAX)))
                .collect();
            Ok(SectionData::Related(dir.related(id, &together)))
        }
        GuideSection::Map => Ok(SectionData::Map(crate::atlas::factbook_map(&dir, id))),
        other => Err(format!(
            "{} needs a Passage or a Strong's number",
            other.title()
        )),
    }
}

#[cfg(target_arch = "wasm32")]
pub fn section(
    _: &Library,
    _: &DirectoryCache,
    _: &crate::aliases::AliasCache,
    _: &str,
    _: crate::GuideSection,
) -> Result<crate::SectionData, String> {
    Err("Opening Resources is not supported here".into())
}

/// Verses as Passages: sorted, with neighbouring Verses joined (`Gen 11:26-27`).
pub fn key_passages(passages: impl IntoIterator<Item = Passage>) -> Vec<Passage> {
    let mut list = obs_scripture::PassageList::new();
    for p in passages {
        list.push(p);
    }
    list.merged(&Standard).passages().to_vec()
}

/// The article headword one Dictionary gives entity `id`, from the alias cache.
#[cfg(not(target_arch = "wasm32"))]
fn headword(
    aliases: &crate::aliases::AliasCache,
    index: &crate::LibraryEntry,
    dictionary: &crate::LibraryEntry,
    id: &str,
) -> Option<String> {
    let table = aliases.table(index, dictionary).ok()?;
    table.headword(id).map(str::to_string)
}

/// One Dictionary article: its unit and the Key's range of it, and the text direction.
#[cfg(not(target_arch = "wasm32"))]
fn article_unit(
    dictionary: &crate::LibraryEntry,
    headword: &str,
) -> Option<(obs_model::TextUnit, std::ops::Range<usize>, Direction)> {
    use obs_store::ResourceStore;
    let reader = obs_store::ObrReader::open(&dictionary.path).ok()?;
    let entries = reader.lookup_key(&Key::Headword(headword.into())).ok()?;
    let entry = entries
        .iter()
        .find(|e| !e.alias)
        .or_else(|| entries.first())?;
    let unit = reader.unit(&entry.unit).ok()??;
    let range = entry.range.map_or(0..unit.text.len(), |(s, e)| {
        (s as usize).min(unit.text.len())..(e as usize).min(unit.text.len())
    });
    Some((unit, range, reader.manifest().direction))
}

/// An article's heading (when its text starts with one) and the text after it, whitespace
/// collapsed.
#[cfg(not(target_arch = "wasm32"))]
fn article_text(
    dictionary: &crate::LibraryEntry,
    headword: &str,
) -> Option<(Option<String>, String, Direction)> {
    let (unit, range, direction) = article_unit(dictionary, headword)?;
    let (heading, excerpt) = crate::guide::heading_and_excerpt(&unit, range);
    Some((heading, excerpt, direction))
}

/// An article's text as written, one paragraph per line.
#[cfg(not(target_arch = "wasm32"))]
fn entry_text(dictionary: &crate::LibraryEntry, headword: &str) -> Option<String> {
    let (unit, range, _) = article_unit(dictionary, headword)?;
    unit.text.get(range).map(str::to_string)
}

#[cfg(not(target_arch = "wasm32"))]
fn article_link(dictionary: &crate::LibraryEntry, headword: String) -> ArticleLink {
    ArticleLink {
        resource_id: dictionary.manifest.resource_id.clone(),
        abbreviation: dictionary.manifest.abbreviation.clone(),
        title: dictionary.manifest.title.clone(),
        headword,
    }
}

/// The meaning of the entity's name: the first names Dictionary in Preferred order that has
/// an article on it.
#[cfg(not(target_arch = "wasm32"))]
fn name_meaning(
    library: &Library,
    aliases: &crate::aliases::AliasCache,
    index: &crate::LibraryEntry,
    id: &str,
) -> Option<NameMeaning> {
    library
        .of_type(obs_model::ResourceType::Dictionary)
        .filter(|d| is_names_dictionary(&d.manifest.resource_id))
        .find_map(|d| {
            let headword = headword(aliases, index, d, id)?;
            let text = meaning_text(&headword, &entry_text(d, &headword)?);
            (!text.is_empty()).then(|| NameMeaning {
                article: article_link(d, headword),
                text,
            })
        })
}

/// Whether a Dictionary gives the meanings of names ([`NAME_MEANING_DICTIONARIES`]).
pub fn is_names_dictionary(resource_id: &str) -> bool {
    NAME_MEANING_DICTIONARIES.contains(&resource_id)
}

/// Every other Dictionary's article on the entity, in Preferred order (the names Dictionary
/// gives the Summary's meaning instead).
#[cfg(not(target_arch = "wasm32"))]
fn articles(
    library: &Library,
    aliases: &crate::aliases::AliasCache,
    index: &crate::LibraryEntry,
    id: &str,
) -> Vec<DictionaryArticle> {
    library
        .of_type(obs_model::ResourceType::Dictionary)
        .filter(|d| !is_names_dictionary(&d.manifest.resource_id))
        .filter_map(|d| {
            let headword = headword(aliases, index, d, id)?;
            let (_, excerpt, direction) = article_text(d, &headword)?;
            Some(DictionaryArticle {
                article: article_link(d, headword),
                excerpt: crate::guide::cut(&excerpt, ARTICLE_CHARS),
                direction,
            })
        })
        .collect()
}

/// Factbook state for a Session: the directory load and the Tabs showing a chooser.
#[derive(Debug, Default)]
pub struct Factbooks {
    /// The job loading the directory.
    job: Option<JobId>,
    /// Why the last load failed, for the library it was tried on.
    failed: Option<(Fingerprint, String)>,
    /// Factbook Tabs showing the chooser, with the name typed.
    choosing: BTreeMap<TabId, String>,
}

impl Factbooks {
    /// The name a Factbook Tab's chooser shows, if it shows one.
    pub fn choosing(&self, tab: TabId) -> Option<&str> {
        self.choosing.get(&tab).map(String::as_str)
    }

    pub fn is_busy(&self) -> bool {
        self.job.is_some()
    }
}

impl Session {
    /// The People & places directory. Never waits: when it is not loaded for this library it
    /// is loaded on the job queue and reported as Loading; ask again on a later frame.
    pub fn people_directory(&mut self) -> WordStatus<PeopleDirectory> {
        let cache = self.guides.directory();
        if let Some(d) = cache.current(&self.library) {
            return WordStatus::Ready(d);
        }
        let print = fingerprint(&self.library);
        if let Some((f, e)) = &self.factbooks.failed
            && *f == print
        {
            return WordStatus::Failed(e.clone());
        }
        if self.factbooks.job.is_none() {
            let library = self.library.clone();
            let job = self
                .jobs()
                .submit_fn("People & places", move |_| cache.get(&library).map(|_| ()));
            self.factbooks.job = Some(job);
        }
        WordStatus::Loading
    }

    /// The chooser for `name`, once the directory is loaded.
    pub fn factbook_chooser(&mut self, name: &str) -> WordStatus<FactbookChooser> {
        match self.people_directory() {
            WordStatus::Ready(d) => WordStatus::Ready(Arc::new(d.choose(name))),
            WordStatus::Loading => WordStatus::Loading,
            WordStatus::Failed(e) => WordStatus::Failed(e),
        }
    }

    /// The header of a Factbook at entity `id`, once the directory is loaded.
    pub fn factbook_card(&mut self, id: &str) -> WordStatus<EntityCard> {
        match self.people_directory() {
            WordStatus::Ready(d) => match d.card(id) {
                Some(c) => WordStatus::Ready(Arc::new(c)),
                None => WordStatus::Failed(format!(
                    "{} is in no People & places index in your library",
                    display_name(id)
                )),
            },
            WordStatus::Loading => WordStatus::Loading,
            WordStatus::Failed(e) => WordStatus::Failed(e),
        }
    }

    /// The people and places Strong's number `n` names (Word Info's Factbook link): empty
    /// until the directory is loaded.
    pub fn factbook_entities_for(&mut self, n: StrongsNumber) -> Vec<EntityCard> {
        match self.people_directory() {
            WordStatus::Ready(d) => d
                .with_strongs(n)
                .into_iter()
                .filter_map(|e| d.card(&e.entity.id))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Close a Factbook Tab's chooser.
    pub fn stop_choosing_factbook(&mut self, tab: TabId) {
        self.factbooks.choosing.remove(&tab);
    }

    /// Show the chooser for `name` in a Factbook Tab; when the directory is loaded and only
    /// one entity has that name, show it instead.
    fn choose_factbook(&mut self, tab: TabId, name: &str) {
        self.factbooks.choosing.insert(tab, name.trim().to_string());
        self.resolve_factbook_choosers();
    }

    /// Choosers whose name matches exactly one entity move their Tab to it.
    fn resolve_factbook_choosers(&mut self) {
        let Some(dir) = self.guides.directory().current(&self.library) else {
            return;
        };
        let single: Vec<(TabId, String)> = self
            .factbooks
            .choosing
            .iter()
            .filter_map(|(tab, name)| match dir.choose(name).choices.as_slice() {
                [one] => Some((*tab, one.entity.clone())),
                _ => None,
            })
            .collect();
        for (tab, id) in single {
            self.factbooks.choosing.remove(&tab);
            self.workspace.navigate_entry(tab, Key::Headword(id));
        }
    }

    /// Finish the directory load; return the other events.
    pub(crate) fn poll_factbooks(&mut self, events: Vec<JobEvent>) -> Vec<JobEvent> {
        let mut other = Vec::new();
        for event in events {
            if Some(event.id) != self.factbooks.job {
                other.push(event);
                continue;
            }
            if !event.kind.is_terminal() {
                continue;
            }
            self.factbooks.job = None;
            if let JobEventKind::Failed(e) = &event.kind {
                self.factbooks.failed = Some((fingerprint(&self.library), e.clone()));
            }
            self.resolve_factbook_choosers();
        }
        self.factbooks
            .choosing
            .retain(|tab, _| self.workspace.tab(*tab).is_some());
        other
    }

    /// The Factbook Tab to show something in: the focused or most recent one (focused), or a
    /// new one with `new_tab` or a split request, or when there is none.
    fn factbook_tab(&mut self, request: OpenRequest, new_tab: bool) -> TabId {
        let kind = TabKind::Guide(Guide::Factbook);
        let existing = (!new_tab && !request.split)
            .then(|| self.workspace.most_recent(|t| t.kind == kind))
            .flatten();
        match existing {
            Some(tab) => {
                self.workspace.focus(tab);
                tab
            }
            None => self
                .workspace
                .open(kind, Guide::Factbook.title().into(), request),
        }
    }

    /// Open a Factbook at entity `id` (in the most recent Factbook Tab unless `new_tab`).
    pub(crate) fn open_factbook_at(
        &mut self,
        id: &str,
        request: OpenRequest,
        new_tab: bool,
    ) -> TabId {
        let tab = self.factbook_tab(request, new_tab);
        self.stop_choosing_factbook(tab);
        self.workspace
            .navigate_entry(tab, Key::Headword(id.to_string()));
        tab
    }
}

/// The label of a Factbook at entity `id` (Favorites, History): `John (son of Zebedee)` once
/// the directory is loaded, else the name its id stands for.
pub(crate) fn label(s: &Session, id: &str) -> String {
    match s.guides.directory().current(&s.library) {
        Some(d) => d.card(id).map_or_else(|| display_name(id), |c| c.title()),
        None => display_name(id),
    }
}

/// `tab.go` for a Factbook: an entity id moves the Tab there; a name opens the chooser (or the
/// only entity with that name).
pub(crate) fn go(s: &mut Session, tab: TabId, text: &str) -> Result<Value, CommandError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(failed("tab.go", "Type the name of a person or place"));
    }
    if text.contains('@') {
        s.stop_choosing_factbook(tab);
        s.workspace
            .navigate_entry(tab, Key::Headword(text.to_string()));
        return Ok(json!({ "tab": tab.0, "key": text }));
    }
    s.choose_factbook(tab, text);
    Ok(json!({ "tab": tab.0, "choose": text }))
}

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

fn request(p: &Params<'_>) -> OpenRequest {
    OpenRequest {
        panel: p
            .i64("panel")
            .and_then(|n| u64::try_from(n).ok())
            .map(PanelId),
        split: p.bool("split").unwrap_or(false),
    }
}

pub(crate) const COMMANDS: &[CommandSpec] = &[CommandSpec {
    id: "guide.factbook",
    label: "Factbook",
    shortcut: None,
    params: &[
        ParamSpec {
            name: "entity",
            kind: ParamKind::String,
            required: false,
            help: "People & places id to show, e.g. `Abraham@Gen.11.26-1Pe`",
        },
        ParamSpec {
            name: "name",
            kind: ParamKind::String,
            required: false,
            help: "A name to choose from, e.g. `John` (the only match opens directly)",
        },
        ParamSpec {
            name: "strongs",
            kind: ParamKind::String,
            required: false,
            help: "A Strong's number naming a person or place, e.g. `H85` (Word Info)",
        },
        ParamSpec {
            name: "new_tab",
            kind: ParamKind::Boolean,
            required: false,
            help: "Open a new Factbook Tab even if one is open (Ctrl/Cmd-click)",
        },
        ParamSpec {
            name: "panel",
            kind: ParamKind::Integer,
            required: false,
            help: "Panel whose + was used",
        },
        ParamSpec {
            name: "split",
            kind: ParamKind::Boolean,
            required: false,
            help: "Open in a new Panel split off to the right (Ctrl/Cmd held)",
        },
    ],
    enabled: always,
    run: |s, p| {
        const ID: &str = "guide.factbook";
        let new_tab = p.bool("new_tab").unwrap_or(false);
        let req = request(&p);
        if let Some(id) = p.str("entity") {
            let tab = s.open_factbook_at(id, req, new_tab);
            return Ok(json!({ "tab": tab.0, "key": id }));
        }
        let name = match p.str("strongs") {
            Some(text) => {
                let n: StrongsNumber = text.parse().map_err(|e| failed(ID, e))?;
                let WordStatus::Ready(dir) = s.people_directory() else {
                    return Err(failed(ID, "People & places are still loading"));
                };
                let found = dir.with_strongs(n);
                match found.as_slice() {
                    [] => return Err(failed(ID, format!("No person or place has {n}"))),
                    [one] => {
                        let id = one.entity.id.clone();
                        let tab = s.open_factbook_at(&id, req, new_tab);
                        return Ok(json!({ "tab": tab.0, "key": id }));
                    }
                    [first, ..] => Some(usual_name(&first.entity)),
                }
            }
            None => p.str("name").map(str::to_string),
        };
        let tab = s.factbook_tab(req, new_tab);
        match name {
            Some(name) => {
                s.choose_factbook(tab, &name);
                Ok(json!({ "tab": tab.0, "choose": name }))
            }
            None => Ok(json!({ "tab": tab.0 })),
        }
    },
}];
