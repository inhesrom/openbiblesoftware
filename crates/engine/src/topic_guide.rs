//! The Topic Guide (#148): a Guide whose Key is a Headword, a topic such as "Forgiveness" or
//! "Prayer", gathered from the library's topical indexes and Dictionaries.
//!
//! - **Topical index** (ADR 0009): a Dictionary whose entries are Passage lists, as Nave's
//!   Topical Bible and Torrey's Topical Textbook are. It is not a seventh Resource type; the
//!   Topic Guide knows one by its resource id ([`TOPICAL_INDEXES`]), as SWORD Import names
//!   CrossWire's modules.
//! - **Key:** the topic as a Headword Key in the Tab's entry history (`Forgiveness`). Lookups
//!   compare normalized headwords, so `FORGIVENESS` and `Forgiveness` are one topic. A Topic
//!   Guide follows and moves its Link set's Headword Key (Dictionary Tabs).
//! - **Sections:** Topical Bible entries (each topical index's subtopics with their
//!   Passages), Dictionary articles (every other Dictionary's article on the topic), Key
//!   Passages (the Passages cited most often across the topical entries) and Search (a Library
//!   search for the topic).
//! - **Chooser:** text typed into the header box lists the topics it names: the headword
//!   itself, longer headwords that start with it (Torrey's "Forgiveness of Injuries"), and its
//!   synonyms from the topical indexes: entries that only send the reader to it ("Pardon: see
//!   Sin, forgiveness of").

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use obs_model::{Direction, Key, ParaStyle, ResourceType, Span, SpanKind, normalize};
use obs_scripture::{Passage, Standard};
use serde_json::{Value, json};

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
#[cfg(not(target_arch = "wasm32"))]
use crate::factbook::DictionaryArticle;
use crate::people_places::ArticleLink;
use crate::workspace::{Guide, OpenRequest, PanelId, TabId, TabKind};
use crate::{JobEvent, JobEventKind, JobId, Library, LibraryEntry, Session, WordStatus};

/// Dictionaries that are topical indexes, by resource id: Nave's Topical Bible (CrossWire's
/// `Nave`, and its older name `Naves`) and Torrey's New Topical Textbook (`Torrey`), as SWORD
/// Import and Get Resources name them.
pub const TOPICAL_INDEXES: &[&str] = &["nave", "naves", "torrey"];

/// Most Key Passages shown.
pub const KEY_PASSAGES: usize = 12;

/// Most articles from one Dictionary that has none under the topic's own headword.
pub const ARTICLES_PER_DICTIONARY: usize = 3;

/// Most chooser rows of each kind (longer headwords, synonyms, prefix matches).
pub const CHOICES: usize = 30;

/// Whether a Dictionary is a topical index ([`TOPICAL_INDEXES`]).
pub fn is_topical_index(resource_id: &str) -> bool {
    TOPICAL_INDEXES.contains(&resource_id)
}

/// One line of a topical index entry: a subtopic (level 1: Nave's `→ OF ENEMIES`, Torrey's
/// `- Commanded`) or an item under one (level 2: Nave's list items, Torrey's `. Joseph`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subtopic {
    pub level: u8,
    /// The label as the source writes it (`OF ENEMIES`, `Esau forgives Jacob`); empty when
    /// the entry starts with Passages.
    pub label: String,
    /// The Passages it cites, in the source's order (Standard versification).
    pub passages: Vec<Passage>,
    /// Topics it sends the reader to (`See ENEMY`, `See SIN, FORGIVENESS OF`).
    pub see: Vec<String>,
    /// When it cites no Passage itself but sends the reader to another entry's subtopic
    /// (`SIN, FORGIVENESS OF`), that subtopic: its Passages are in [`Self::passages`].
    pub via: Option<String>,
}

/// One topical index's entry on the topic, split into subtopics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicalEntry {
    pub article: ArticleLink,
    pub subtopics: Vec<Subtopic>,
    pub direction: Direction,
}

impl TopicalEntry {
    /// Every Passage the entry cites, subtopic by subtopic.
    pub fn passages(&self) -> impl Iterator<Item = Passage> + '_ {
        self.subtopics
            .iter()
            .flat_map(|s| s.passages.iter().copied())
    }
}

/// One Key Passage: a Passage the topical entries cite, and how many of their citations it
/// shares Verses with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CitedPassage {
    pub passage: Passage,
    pub citations: u32,
}

/// One row of the topic chooser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicChoice {
    /// The topic it opens (the Topic Guide's Key).
    pub topic: String,
    /// The Dictionaries with an entry on it, by abbreviation, in Preferred order.
    pub sources: Vec<String>,
    /// For a synonym, the topics its topical index sends the reader to.
    pub see: Vec<String>,
}

/// What a typed text names, best first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicChooser {
    pub text: String,
    pub choices: Vec<TopicChoice>,
}

/// One Dictionary's entry under one headword.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Source {
    resource_id: String,
    abbreviation: String,
    headword: String,
}

/// A topical index entry that cites nothing and only sends the reader elsewhere: a synonym of
/// the topics it names.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SeeEntry {
    /// Its normalized headword.
    topic: String,
    see: Vec<String>,
}

/// Every Dictionary headword of the library, grouped by normalized headword, and the topical
/// indexes' see-only entries (synonyms). Loaded once per library on the job queue.
#[derive(Debug, Default)]
pub struct TopicDirectory {
    /// Normalized headword → its display form and its entries, in Preferred order.
    topics: BTreeMap<String, (String, Vec<Source>)>,
    see_only: Vec<SeeEntry>,
}

impl TopicDirectory {
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    fn add(&mut self, entry: &LibraryEntry, headword: &str) {
        let norm = topic_norm(headword);
        if norm.is_empty() {
            return;
        }
        let (title, sources) = self
            .topics
            .entry(norm)
            .or_insert_with(|| (display_topic(headword), Vec::new()));
        // The first spelling with lower case letters wins: Easton's `Forgiveness` over Nave's
        // `FORGIVENESS`.
        if !has_lower(title) && has_lower(headword) {
            *title = headword.trim().to_string();
        }
        if !sources
            .iter()
            .any(|s| s.resource_id == entry.manifest.resource_id)
        {
            sources.push(Source {
                resource_id: entry.manifest.resource_id.clone(),
                abbreviation: entry.manifest.abbreviation.clone(),
                headword: headword.to_string(),
            });
        }
    }

    pub fn len(&self) -> usize {
        self.topics.len()
    }

    pub fn is_empty(&self) -> bool {
        self.topics.is_empty()
    }

    /// The topic's display form (`Forgiveness`), if any Dictionary has it.
    pub fn title(&self, topic: &str) -> Option<&str> {
        self.topics.get(&topic_norm(topic)).map(|(t, _)| t.as_str())
    }

    /// The headwords one Dictionary files under `topic`: the headword itself, then longer
    /// headwords that start with it as whole words (`PRAYER, ANSWERS TO`, `FORGIVENESS OF
    /// INJURIES`), in headword order.
    pub fn headwords_in(&self, resource_id: &str, topic: &str) -> Vec<String> {
        let norm = topic_norm(topic);
        if norm.is_empty() {
            return Vec::new();
        }
        let of = |sources: &Vec<Source>| {
            sources
                .iter()
                .find(|s| s.resource_id == resource_id)
                .map(|s| s.headword.clone())
        };
        let mut out: Vec<String> = self
            .topics
            .get(&norm)
            .and_then(|(_, s)| of(s))
            .into_iter()
            .collect();
        out.extend(
            self.topics
                .range(norm.clone()..)
                .skip_while(|(k, _)| **k == norm)
                .take_while(|(k, _)| k.starts_with(&norm))
                .filter(|(k, _)| extends(k, &norm))
                .filter_map(|(_, (_, s))| of(s)),
        );
        out
    }

    /// The topics `text` names, best first: the headword itself; longer headwords starting
    /// with it; synonyms (see-only topical index entries naming it); with none of those,
    /// headwords that start with the text.
    pub fn choose(&self, text: &str) -> TopicChooser {
        let norm = topic_norm(text);
        let mut choices: Vec<TopicChoice> = Vec::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut push = |key: &str, see: Vec<String>, choices: &mut Vec<TopicChoice>| {
            if norm.is_empty() || !seen.insert(key.to_string()) {
                return;
            }
            if let Some((title, sources)) = self.topics.get(key) {
                choices.push(TopicChoice {
                    topic: title.clone(),
                    sources: sources.iter().map(|s| s.abbreviation.clone()).collect(),
                    see,
                });
            }
        };
        push(&norm, Vec::new(), &mut choices);
        let longer: Vec<&String> = self
            .topics
            .range(norm.clone()..)
            .take_while(|(k, _)| k.starts_with(&norm))
            .filter(|(k, _)| extends(k, &norm))
            .map(|(k, _)| k)
            .take(CHOICES)
            .collect();
        for k in longer {
            push(k, Vec::new(), &mut choices);
        }
        for s in self
            .see_only
            .iter()
            .filter(|s| s.see.iter().any(|t| names(t, &norm)))
            .take(CHOICES)
        {
            let see = s.see.iter().map(|t| display_topic(t)).collect();
            push(&s.topic, see, &mut choices);
        }
        if choices.is_empty() {
            let prefixed: Vec<&String> = self
                .topics
                .range(norm.clone()..)
                .take_while(|(k, _)| k.starts_with(&norm))
                .map(|(k, _)| k)
                .take(CHOICES)
                .collect();
            for k in prefixed {
                push(k, Vec::new(), &mut choices);
            }
        }
        TopicChooser {
            text: text.trim().to_string(),
            choices,
        }
    }
}

/// A headword compared as a topic: normalized, whitespace collapsed.
fn topic_norm(s: &str) -> String {
    normalize(s)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether normalized headword `k` is `topic` followed by more words (`prayer, answers to`,
/// `forgiveness of injuries`), not a longer word (`prayerfulness`).
fn extends(k: &str, topic: &str) -> bool {
    k.strip_prefix(topic)
        .and_then(|rest| rest.chars().next())
        .is_some_and(|c| c == ' ' || c == ',')
}

/// Whether see target `target` names normalized `topic`: as a whole (`PRAYER`), as the topic
/// before its comma (`PRAYER, ANSWERED`) or as the subtopic after it (`SIN, FORGIVENESS OF`).
fn names(target: &str, topic: &str) -> bool {
    let t = topic_norm(target);
    if t == topic || extends(&t, topic) {
        return true;
    }
    t.split_once(',').is_some_and(|(_, sub)| {
        let sub = sub.trim();
        sub == topic || extends(sub, topic)
    })
}

fn has_lower(s: &str) -> bool {
    s.chars().any(char::is_lowercase)
}

/// Words an all-capitals headword keeps in lower case after its first word.
const MINOR_WORDS: [&str; 18] = [
    "a", "an", "and", "as", "at", "by", "for", "from", "in", "into", "of", "on", "or", "the",
    "to", "unto", "upon", "with",
];

/// A headword for display: as written when it has lower case letters, else in title case
/// with minor words in lower case (`SIN, FORGIVENESS OF` → `Sin, Forgiveness of`, `FAITH IN
/// CHRIST` → `Faith in Christ`). All capitals say nothing about which words are proper names,
/// and sentence case lower-cased them (`Faith in christ`, #261).
pub fn display_topic(headword: &str) -> String {
    let headword = headword.split_whitespace().collect::<Vec<_>>().join(" ");
    if has_lower(&headword) {
        return headword;
    }
    headword
        .split(' ')
        .enumerate()
        .map(|(i, word)| {
            let lower = word.to_lowercase();
            let bare = lower.trim_end_matches(|c: char| !c.is_alphanumeric());
            if i > 0 && MINOR_WORDS.contains(&bare) {
                return lower;
            }
            let mut chars = lower.chars();
            chars.next().map_or_else(String::new, |c| {
                c.to_uppercase().chain(chars).collect::<String>()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ---- Reading a topical index entry ---------------------------------------------------------

/// Split one topical index entry into subtopics. Each line (paragraph) is one: its label is
/// the text before its first Bible reference, its Passages the references' targets, and `See
/// X` sends the reader to topic X. A line with no label of its own (Torrey's reference lines,
/// Nave's `See ENEMY` item under a subtopic) adds to the line before it. A heading line (the
/// entry's headword) is skipped.
pub fn subtopics(text: &str, spans: &[Span]) -> Vec<Subtopic> {
    let mut out: Vec<Subtopic> = Vec::new();
    let mut at = 0usize;
    for line in text.split('\n') {
        let start = at;
        let end = at + line.len();
        at = end + 1;
        let in_line = |s: &&Span| (s.start as usize) >= start && (s.end as usize) <= end;
        // The entry's headword, which an imported entry opens with (#261), is no subtopic.
        if spans
            .iter()
            .filter(in_line)
            .any(|s| s.start as usize == start && matches!(s.kind, SpanKind::Heading { .. }))
        {
            continue;
        }
        let mut refs: Vec<&Span> = spans
            .iter()
            .filter(in_line)
            .filter(|s| matches!(s.kind, SpanKind::ScriptureRef { .. }))
            .collect();
        refs.sort_by_key(|s| s.start);
        let list_item = spans.iter().filter(in_line).any(|s| {
            s.start as usize == start
                && matches!(
                    s.kind,
                    SpanKind::Para {
                        style: ParaStyle::List
                    }
                )
        });
        let passages: Vec<Passage> = refs
            .iter()
            .flat_map(|s| match &s.kind {
                SpanKind::ScriptureRef { targets } => targets.passages().to_vec(),
                _ => Vec::new(),
            })
            .collect();
        let head_end = refs.first().map_or(end, |s| s.start as usize);
        let tail_start = refs.last().map_or(end, |s| s.end as usize);
        let head = text.get(start..head_end).unwrap_or_default().trim();
        let tail = if refs.is_empty() {
            ""
        } else {
            text.get(tail_start..end).unwrap_or_default()
        };
        let (level, head) = match marker(head) {
            Some((level, rest)) => (level, rest),
            None => (if list_item { 2 } else { 1 }, head),
        };
        let (label, mut see) = split_see(head);
        see.extend(split_see(tail).1);
        let label = trim_label(label);
        if label.is_empty() && passages.is_empty() && see.is_empty() {
            continue;
        }
        if label.is_empty()
            && let Some(last) = out.last_mut()
        {
            last.passages.extend(passages);
            last.see.extend(see);
            continue;
        }
        out.push(Subtopic {
            level,
            label: label.to_string(),
            passages,
            see,
            via: None,
        });
    }
    out
}

/// A subtopic marker at the start of a line: `→` (Nave's) or `- ` (Torrey's) is a subtopic,
/// `. ` (Torrey's) an item under one.
fn marker(line: &str) -> Option<(u8, &str)> {
    if let Some(rest) = line.strip_prefix('→') {
        return Some((1, rest.trim_start()));
    }
    if let Some(rest) = line.strip_prefix("- ") {
        return Some((1, rest.trim_start()));
    }
    line.strip_prefix(". ").map(|rest| (2, rest.trim_start()))
}

/// Text before the first `See`, and the topics after each `See`.
fn split_see(text: &str) -> (&str, Vec<String>) {
    let find = |t: &str| {
        t.match_indices("See ")
            .find(|(i, _)| *i == 0 || t[..*i].ends_with(char::is_whitespace))
            .map(|(i, _)| i)
    };
    let Some(first) = find(text) else {
        return (text, Vec::new());
    };
    let mut targets = Vec::new();
    let mut rest = &text[first + 4..];
    loop {
        match find(rest) {
            Some(i) => {
                targets.push(rest[..i].to_string());
                rest = &rest[i + 4..];
            }
            None => {
                targets.push(rest.to_string());
                break;
            }
        }
    }
    let targets = targets
        .into_iter()
        .map(|t| trim_label(&t).to_string())
        .filter(|t| !t.is_empty())
        .collect();
    (&text[..first], targets)
}

fn trim_label(s: &str) -> &str {
    s.trim()
        .trim_end_matches(|c: char| {
            matches!(c, ',' | ';' | ':' | '.' | '-' | '–') || c.is_whitespace()
        })
        .trim()
}

// ---- Sections --------------------------------------------------------------------------------

/// The Topic Guide's Key Passages: every distinct Passage the entries cite, ranked by how many
/// citations share Verses with it (narrower first, then Bible order); a Passage overlapping
/// one already chosen is passed over. At most [`KEY_PASSAGES`].
pub fn key_passages(entries: &[TopicalEntry]) -> Vec<CitedPassage> {
    let cited: Vec<Passage> = entries.iter().flat_map(TopicalEntry::passages).collect();
    let distinct: BTreeSet<Passage> = cited.iter().copied().collect();
    let mut ranked: Vec<CitedPassage> = distinct
        .into_iter()
        .map(|p| CitedPassage {
            passage: p,
            citations: u32::try_from(cited.iter().filter(|c| c.overlaps(p)).count())
                .unwrap_or(u32::MAX),
        })
        .collect();
    // Standard Verses spanned, so a cross-chapter Passage compares by its length; a Verse
    // outside the Standard ranks last.
    let width = |p: Passage| match (Standard.ordinal(p.start()), Standard.ordinal(p.end())) {
        (Some(a), Some(b)) => b.saturating_sub(a),
        _ => u32::MAX,
    };
    ranked.sort_by(|a, b| {
        b.citations
            .cmp(&a.citations)
            .then(width(a.passage).cmp(&width(b.passage)))
            .then(a.passage.cmp(&b.passage))
    });
    let mut out: Vec<CitedPassage> = Vec::new();
    for c in ranked {
        if out.len() == KEY_PASSAGES {
            break;
        }
        if out.iter().all(|o| !o.passage.overlaps(c.passage)) {
            out.push(c);
        }
    }
    out
}

/// The section data of a Topic Guide at `topic`.
#[cfg(not(target_arch = "wasm32"))]
pub fn section(
    library: &Library,
    topics: &TopicCache,
    topic: &str,
    section: crate::GuideSection,
) -> Result<crate::SectionData, String> {
    use crate::{GuideSection, SectionData};
    match section {
        GuideSection::TopicSearch => Ok(SectionData::TopicSearch(topic.trim().to_string())),
        GuideSection::TopicalEntries => Ok(SectionData::TopicalEntries(topical_entries(
            library, topics, topic,
        )?)),
        GuideSection::TopicPassages => Ok(SectionData::TopicPassages(key_passages(
            &topical_entries(library, topics, topic)?,
        ))),
        GuideSection::TopicArticles => Ok(SectionData::DictionaryArticles(articles(
            library, topics, topic,
        )?)),
        other => Err(format!(
            "{} needs a Passage or a Strong's number",
            other.title()
        )),
    }
}

#[cfg(target_arch = "wasm32")]
pub fn section(
    _: &Library,
    _: &TopicCache,
    _: &str,
    _: crate::GuideSection,
) -> Result<crate::SectionData, String> {
    Err("Opening Resources is not supported here".into())
}

/// Each topical index's entries on `topic`, in Preferred order: the topic's own headword,
/// then longer headwords under it. An index that cannot be read is skipped.
#[cfg(not(target_arch = "wasm32"))]
pub fn topical_entries(
    library: &Library,
    topics: &TopicCache,
    topic: &str,
) -> Result<Vec<TopicalEntry>, String> {
    use obs_store::ResourceStore;
    let indexes: Vec<&LibraryEntry> = library
        .of_type(ResourceType::Dictionary)
        .filter(|d| is_topical_index(&d.manifest.resource_id))
        .collect();
    if indexes.is_empty() {
        return Ok(Vec::new());
    }
    let directory = topics.get(library)?;
    let mut out = Vec::new();
    for index in indexes {
        let Ok(reader) = obs_store::ObrReader::open(&index.path) else {
            continue;
        };
        for headword in directory.headwords_in(&index.manifest.resource_id, topic) {
            let Some(mut subtopics) = entry_subtopics(&reader, &headword) else {
                continue;
            };
            follow_see(&reader, &mut subtopics);
            out.push(TopicalEntry {
                article: article_link(index, headword),
                subtopics,
                direction: reader.manifest().direction,
            });
        }
    }
    Ok(out)
}

/// One entry's subtopics, by headword.
#[cfg(not(target_arch = "wasm32"))]
fn entry_subtopics(reader: &obs_store::ObrReader, headword: &str) -> Option<Vec<Subtopic>> {
    use obs_store::ResourceStore;
    let entries = reader.lookup_key(&Key::Headword(headword.into())).ok()?;
    let entry = entries
        .iter()
        .find(|e| !e.alias)
        .or_else(|| entries.first())?;
    let unit = reader.unit(&entry.unit).ok()??;
    let (start, end) = entry
        .range
        .map_or((0, unit.text.len()), |(s, e)| (s as usize, e as usize));
    let end = end.min(unit.text.len());
    let text = unit.text.get(start..end)?;
    let offset = u32::try_from(start).ok()?;
    let spans: Vec<Span> = unit
        .spans
        .iter()
        .filter(|s| s.start as usize >= start && s.end as usize <= end)
        .map(|s| Span {
            start: s.start - offset,
            end: s.end - offset,
            kind: s.kind.clone(),
        })
        .collect();
    Some(subtopics(text, &spans))
}

/// A subtopic that cites nothing but sends the reader to another entry's subtopic (`See SIN,
/// FORGIVENESS OF`) takes that subtopic's Passages.
#[cfg(not(target_arch = "wasm32"))]
fn follow_see(reader: &obs_store::ObrReader, subtopics: &mut [Subtopic]) {
    for s in subtopics.iter_mut().filter(|s| s.passages.is_empty()) {
        for target in s.see.clone() {
            let Some((head, sub)) = target.split_once(',') else {
                continue;
            };
            let sub = topic_norm(sub);
            let Some(theirs) = entry_subtopics(reader, head.trim()) else {
                continue;
            };
            let Some(i) = theirs
                .iter()
                .position(|t| t.level == 1 && topic_norm(&t.label) == sub)
            else {
                continue;
            };
            // Its own Passages, else its items'.
            let mut passages = theirs[i].passages.clone();
            if passages.is_empty() {
                passages = theirs[i + 1..]
                    .iter()
                    .take_while(|t| t.level > 1)
                    .flat_map(|t| t.passages.iter().copied())
                    .collect();
            }
            if !passages.is_empty() {
                s.passages = passages;
                s.via = Some(display_topic(&target));
                break;
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn article_link(dictionary: &LibraryEntry, headword: String) -> ArticleLink {
    ArticleLink {
        resource_id: dictionary.manifest.resource_id.clone(),
        abbreviation: dictionary.manifest.abbreviation.clone(),
        title: dictionary.manifest.title.clone(),
        headword,
    }
}

/// Every other Dictionary's articles on `topic`, in Preferred order: its article under the
/// topic's own headword, or when it has none, up to [`ARTICLES_PER_DICTIONARY`] under longer
/// headwords (Easton files forgiveness under "Forgiveness of sin").
#[cfg(not(target_arch = "wasm32"))]
fn articles(
    library: &Library,
    topics: &TopicCache,
    topic: &str,
) -> Result<Vec<DictionaryArticle>, String> {
    use obs_store::ResourceStore;
    let dictionaries: Vec<&LibraryEntry> = library
        .of_type(ResourceType::Dictionary)
        .filter(|d| !is_topical_index(&d.manifest.resource_id))
        .collect();
    if dictionaries.is_empty() {
        return Ok(Vec::new());
    }
    let directory = topics.get(library)?;
    let norm = topic_norm(topic);
    let mut out = Vec::new();
    for d in dictionaries {
        let headwords = directory.headwords_in(&d.manifest.resource_id, topic);
        let exact = headwords.first().is_some_and(|h| topic_norm(h) == norm);
        let take = if exact { 1 } else { ARTICLES_PER_DICTIONARY };
        let Ok(reader) = obs_store::ObrReader::open(&d.path) else {
            continue;
        };
        for headword in headwords.into_iter().take(take) {
            let Ok(entries) = reader.lookup_key(&Key::Headword(headword.clone())) else {
                continue;
            };
            let Some(entry) = entries
                .iter()
                .find(|e| !e.alias)
                .or_else(|| entries.first())
            else {
                continue;
            };
            let Ok(Some(unit)) = reader.unit(&entry.unit) else {
                continue;
            };
            let range = entry.range.map_or(0..unit.text.len(), |(s, e)| {
                (s as usize).min(unit.text.len())..(e as usize).min(unit.text.len())
            });
            let (_, excerpt) = crate::guide::heading_and_excerpt(&unit, range);
            out.push(DictionaryArticle {
                article: article_link(d, headword),
                excerpt: crate::guide::cut(&excerpt, crate::factbook::ARTICLE_CHARS),
                direction: reader.manifest().direction,
            });
        }
    }
    Ok(out)
}

// ---- The directory cache ----------------------------------------------------------------------

type Fingerprint = Vec<(String, u32, String, std::path::PathBuf)>;

fn fingerprint(library: &Library) -> Fingerprint {
    library
        .of_type(ResourceType::Dictionary)
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

/// The library's [`TopicDirectory`], kept until its Dictionaries change. Shared by the Session
/// (chooser) and the section jobs.
#[derive(Debug, Default)]
pub struct TopicCache {
    cached: Mutex<Option<(Fingerprint, Arc<TopicDirectory>)>>,
    /// Held while one job builds, so jobs asking at once share one build.
    building: Mutex<()>,
}

impl TopicCache {
    /// The directory for `library` if it is loaded and current; never reads a file.
    pub fn current(&self, library: &Library) -> Option<Arc<TopicDirectory>> {
        let cached = self.cached.lock().unwrap_or_else(|p| p.into_inner());
        cached
            .as_ref()
            .filter(|(f, _)| *f == fingerprint(library))
            .map(|(_, d)| Arc::clone(d))
    }

    /// The directory for `library`, read from its Dictionaries when not current (on a worker).
    pub fn get(&self, library: &Library) -> Result<Arc<TopicDirectory>, String> {
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

/// Every Dictionary's headwords; for a topical index also its see-only entries.
#[cfg(not(target_arch = "wasm32"))]
fn read_directory(library: &Library) -> Result<TopicDirectory, String> {
    use obs_store::ResourceStore;
    let mut directory = TopicDirectory::default();
    let mut errors = Vec::new();
    let mut read = 0usize;
    for d in library.of_type(ResourceType::Dictionary) {
        let reader = match obs_store::ObrReader::open(&d.path) {
            Ok(r) => r,
            Err(e) => {
                errors.push(format!("{}: {e}", d.manifest.abbreviation));
                continue;
            }
        };
        let keys = match reader.keys(obs_model::KeyScheme::Headword) {
            Ok(k) => k,
            Err(e) => {
                errors.push(format!("{}: {e}", d.manifest.abbreviation));
                continue;
            }
        };
        read += 1;
        for k in &keys {
            if let Key::Headword(h) = &k.key {
                directory.add(d, h);
            }
        }
        if !is_topical_index(&d.manifest.resource_id) {
            continue;
        }
        for k in keys.iter().filter(|k| !k.alias) {
            let Key::Headword(h) = &k.key else {
                continue;
            };
            let Some(subtopics) = entry_subtopics(&reader, h) else {
                continue;
            };
            if subtopics.iter().all(|s| s.passages.is_empty()) {
                let see: Vec<String> = subtopics.into_iter().flat_map(|s| s.see).collect();
                if !see.is_empty() {
                    directory.see_only.push(SeeEntry {
                        topic: topic_norm(h),
                        see,
                    });
                }
            }
        }
    }
    if read == 0 && !errors.is_empty() {
        return Err(errors.join("; "));
    }
    Ok(directory)
}

#[cfg(target_arch = "wasm32")]
fn read_directory(_: &Library) -> Result<TopicDirectory, String> {
    Err("Opening Resources is not supported here".into())
}

// ---- Session state and commands ----------------------------------------------------------------

/// Topic Guide state for a Session: the directory load and the Tabs showing a chooser.
#[derive(Debug, Default)]
pub struct TopicGuides {
    job: Option<JobId>,
    failed: Option<(Fingerprint, String)>,
    /// Topic Guide Tabs showing the chooser, with the text typed.
    choosing: BTreeMap<TabId, String>,
}

impl TopicGuides {
    /// The text a Topic Guide Tab's chooser shows, if it shows one.
    pub fn choosing(&self, tab: TabId) -> Option<&str> {
        self.choosing.get(&tab).map(String::as_str)
    }

    pub fn is_busy(&self) -> bool {
        self.job.is_some()
    }
}

impl Session {
    /// The topic directory. Never waits: when it is not loaded for this library it is loaded
    /// on the job queue and reported as Loading; ask again on a later frame.
    pub fn topic_directory(&mut self) -> WordStatus<TopicDirectory> {
        let cache = self.guides.topics();
        if let Some(d) = cache.current(&self.library) {
            return WordStatus::Ready(d);
        }
        let print = fingerprint(&self.library);
        if let Some((f, e)) = &self.topic_guides.failed
            && *f == print
        {
            return WordStatus::Failed(e.clone());
        }
        if self.topic_guides.job.is_none() {
            let library = self.library.clone();
            let job = self
                .jobs()
                .submit_fn("Topics", move |_| cache.get(&library).map(|_| ()));
            self.topic_guides.job = Some(job);
        }
        WordStatus::Loading
    }

    /// The chooser for `text`, once the directory is loaded.
    pub fn topic_chooser(&mut self, text: &str) -> WordStatus<TopicChooser> {
        match self.topic_directory() {
            WordStatus::Ready(d) => WordStatus::Ready(Arc::new(d.choose(text))),
            WordStatus::Loading => WordStatus::Loading,
            WordStatus::Failed(e) => WordStatus::Failed(e),
        }
    }

    /// Close a Topic Guide Tab's chooser.
    pub fn stop_choosing_topic(&mut self, tab: TabId) {
        self.topic_guides.choosing.remove(&tab);
    }

    /// Show the chooser for `text` in a Topic Guide Tab; when the directory is loaded and the
    /// text names only one topic, show it instead.
    fn choose_topic(&mut self, tab: TabId, text: &str) {
        self.topic_guides
            .choosing
            .insert(tab, text.trim().to_string());
        self.resolve_topic_choosers();
    }

    /// Choosers whose text names exactly one topic move their Tab to it.
    fn resolve_topic_choosers(&mut self) {
        let Some(dir) = self.guides.topics().current(&self.library) else {
            return;
        };
        let single: Vec<(TabId, String)> = self
            .topic_guides
            .choosing
            .iter()
            .filter_map(|(tab, text)| match dir.choose(text).choices.as_slice() {
                [one] => Some((*tab, one.topic.clone())),
                _ => None,
            })
            .collect();
        for (tab, topic) in single {
            self.topic_guides.choosing.remove(&tab);
            self.workspace.navigate_entry(tab, Key::Headword(topic));
        }
    }

    /// Finish the directory load; return the other events.
    pub(crate) fn poll_topic_guides(&mut self, events: Vec<JobEvent>) -> Vec<JobEvent> {
        let mut other = Vec::new();
        for event in events {
            if Some(event.id) != self.topic_guides.job {
                other.push(event);
                continue;
            }
            if !event.kind.is_terminal() {
                continue;
            }
            self.topic_guides.job = None;
            if let JobEventKind::Failed(e) = &event.kind {
                self.topic_guides.failed = Some((fingerprint(&self.library), e.clone()));
            }
            self.resolve_topic_choosers();
        }
        self.topic_guides
            .choosing
            .retain(|tab, _| self.workspace.tab(*tab).is_some());
        other
    }

    /// The Topic Guide Tab to show something in: the most recent one (focused), or a new one
    /// with `new_tab` or a split request, or when there is none.
    fn topic_guide_tab(&mut self, request: OpenRequest, new_tab: bool) -> TabId {
        let kind = TabKind::Guide(Guide::TopicGuide);
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
                .open(kind, Guide::TopicGuide.title().into(), request),
        }
    }

    /// A blank Topic Guide starts at its Link set's current Headword (a linked Dictionary's
    /// entry, say), as Word Study adopts its Link set's Strong's number.
    fn adopt_link_headword(&mut self, tab: TabId) {
        let blank = self
            .workspace
            .tab(tab)
            .is_some_and(|t| t.entry_history.current().is_none());
        if blank
            && let Some(key) = self.workspace.set_entry_location(tab)
            && let Some(t) = self.workspace.tab_mut(tab)
        {
            t.entry_history.scrolled_to(key);
        }
    }

    /// Open a Topic Guide at `topic` (in the most recent Topic Guide Tab unless `new_tab`).
    pub(crate) fn open_topic_guide_at(
        &mut self,
        topic: &str,
        request: OpenRequest,
        new_tab: bool,
    ) -> TabId {
        let tab = self.topic_guide_tab(request, new_tab);
        self.stop_choosing_topic(tab);
        self.workspace
            .navigate_entry(tab, Key::Headword(topic.trim().to_string()));
        tab
    }
}

/// `tab.go` for a Topic Guide: the text opens the chooser (or the only topic it names).
pub(crate) fn go(s: &mut Session, tab: TabId, text: &str) -> Result<Value, CommandError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(failed(
            "tab.go",
            "Type a topic, such as Forgiveness or Prayer",
        ));
    }
    s.choose_topic(tab, text);
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
    id: "guide.topic_guide",
    label: "Topic Guide",
    shortcut: None,
    params: &[
        ParamSpec {
            name: "topic",
            kind: ParamKind::String,
            required: false,
            help: "The topic to show, e.g. `Forgiveness`",
        },
        ParamSpec {
            name: "text",
            kind: ParamKind::String,
            required: false,
            help: "Text to choose a topic from, e.g. `forgive` (the only match opens directly)",
        },
        ParamSpec {
            name: "new_tab",
            kind: ParamKind::Boolean,
            required: false,
            help: "Open a new Topic Guide Tab even if one is open (Ctrl/Cmd-click)",
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
        let new_tab = p.bool("new_tab").unwrap_or(false);
        let req = request(&p);
        if let Some(topic) = p.str("topic").filter(|t| !t.trim().is_empty()) {
            let tab = s.open_topic_guide_at(topic, req, new_tab);
            return Ok(json!({ "tab": tab.0, "key": topic.trim() }));
        }
        let tab = s.topic_guide_tab(req, new_tab);
        s.adopt_link_headword(tab);
        match p.str("text").filter(|t| !t.trim().is_empty()) {
            Some(text) => {
                s.choose_topic(tab, text);
                Ok(json!({ "tab": tab.0, "choose": text.trim() }))
            }
            None => Ok(json!({ "tab": tab.0 })),
        }
    },
}];
