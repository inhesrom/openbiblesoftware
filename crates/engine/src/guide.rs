//! Guides (#53; decisions #15 and #18). A Guide Tab takes one Key and shows ordered Guide
//! sections. Each section is an engine type that takes the Guide's Key and the library and
//! returns **pure data** ([`SectionData`]); `ui-egui` only draws it. Sections are computed
//! independently on the engine job queue and fill in as they finish, so a slow section never
//! holds back the others or a frame.
//!
//! The Guide arrangement — section order, collapsed and hidden sections — is remembered for
//! each kind of Guide (not each Tab) in the User database, so a change applies to every open
//! Guide of that kind and survives a restart.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::sync::{Arc, Mutex};

use obs_model::{Direction, Key, KeyEntry, ResourceType, SpanKind, TextUnit};
use obs_scripture::Passage;
use obs_store::{ResourceStore, StoreError};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::aliases::AliasCache;
use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::cross_references::ReferenceGroup;
use crate::key_words::{BibleCache, KeyWords};
use crate::people_places::PersonOrPlace;
use crate::resource_navigation::ResourceNavigation;
use crate::workspace::Guide;
use crate::{Effect, JobContext, JobEvent, JobEventKind, JobId, Library, Session};

/// One kind of Guide section. The Word Study's arrived with #55 and #56. Ids are stored in
/// the User database and never renamed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GuideSection {
    Commentaries,
    CrossReferences,
    /// #54: [`crate::key_words`].
    KeyWords,
    /// #54: [`crate::people_places`].
    PeopleAndPlaces,
    /// Word Study (#55): every Lexicon's entry for the Strong's number.
    LexiconEntries,
    /// Word Study (#56): the English renderings, grouped and counted.
    TranslatedAs,
    /// Word Study (#56): occurrences per Bible book and the first Verses.
    Occurrences,
    /// Word Study (#56): the distinct grammatical forms, collapsed by default.
    Forms,
    /// Factbook (#149): other names, the name's meaning and family ([`crate::factbook`]).
    Summary,
    /// Factbook: every Passage that mentions the person or place.
    KeyPassages,
    /// Factbook: each Dictionary's article on them.
    DictionaryArticles,
    /// Factbook: people and places linked to them or named with them.
    Related,
}

impl GuideSection {
    pub const ALL: [GuideSection; 12] = [
        GuideSection::Commentaries,
        GuideSection::CrossReferences,
        GuideSection::KeyWords,
        GuideSection::PeopleAndPlaces,
        GuideSection::LexiconEntries,
        GuideSection::TranslatedAs,
        GuideSection::Occurrences,
        GuideSection::Forms,
        GuideSection::Summary,
        GuideSection::KeyPassages,
        GuideSection::DictionaryArticles,
        GuideSection::Related,
    ];

    /// Stable id, as stored and as passed to commands.
    pub fn id(self) -> &'static str {
        match self {
            GuideSection::Commentaries => "commentaries",
            GuideSection::CrossReferences => "cross_references",
            GuideSection::KeyWords => "key_words",
            GuideSection::PeopleAndPlaces => "people_and_places",
            GuideSection::LexiconEntries => "lexicon_entries",
            GuideSection::TranslatedAs => "translated_as",
            GuideSection::Occurrences => "occurrences",
            GuideSection::Forms => "forms",
            GuideSection::Summary => "summary",
            GuideSection::KeyPassages => "key_passages",
            GuideSection::DictionaryArticles => "dictionary_articles",
            GuideSection::Related => "related",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }

    pub fn title(self) -> &'static str {
        match self {
            GuideSection::Commentaries => "Commentaries",
            GuideSection::CrossReferences => "Cross-references",
            GuideSection::KeyWords => "Key words",
            GuideSection::PeopleAndPlaces => "People & places",
            GuideSection::LexiconEntries => "Lexicon entries",
            GuideSection::TranslatedAs => "Translated as",
            GuideSection::Occurrences => "Occurrences",
            GuideSection::Forms => "Forms",
            GuideSection::Summary => "Summary",
            GuideSection::KeyPassages => "Key Passages",
            GuideSection::DictionaryArticles => "Dictionary articles",
            GuideSection::Related => "Related people and places",
        }
    }

    /// The Resource type that feeds the section.
    pub fn resource_type(self) -> ResourceType {
        match self {
            GuideSection::Commentaries => ResourceType::Commentary,
            GuideSection::CrossReferences => ResourceType::CrossReferenceSet,
            GuideSection::KeyWords => ResourceType::Bible,
            GuideSection::PeopleAndPlaces => ResourceType::PeopleAndPlaces,
            GuideSection::LexiconEntries => ResourceType::Lexicon,
            GuideSection::TranslatedAs | GuideSection::Occurrences | GuideSection::Forms => {
                ResourceType::Bible
            }
            GuideSection::Summary | GuideSection::KeyPassages | GuideSection::Related => {
                ResourceType::PeopleAndPlaces
            }
            GuideSection::DictionaryArticles => ResourceType::Dictionary,
        }
    }

    /// Whether the section header shows how many rows it has (the Summary has none to count).
    pub fn counted(self) -> bool {
        self != GuideSection::Summary
    }

    /// Whether the section starts collapsed in a new (or reset) Guide arrangement (#15:
    /// Forms).
    pub fn collapsed_by_default(self) -> bool {
        self == GuideSection::Forms
    }

    /// What an empty section says (#15 rule 1): with no Resource of the kind it needs, a
    /// hint naming that kind; otherwise that nothing in the library covers the Key. Translated
    /// as and Occurrences need a Bible with Strong's numbers (English for Translated as):
    /// that is read from the rendering `indexes` the section's job loaded (never read here).
    pub fn empty_hint(
        self,
        library: &Library,
        indexes: &crate::word_study::RenderingIndexes,
    ) -> &'static str {
        let mut needed = library.of_type(self.resource_type());
        let has = match self {
            // A Bible whose index has English renderings, as the chooser counts them.
            GuideSection::TranslatedAs => {
                needed.any(|e| indexes.loaded(e).is_some_and(|i| !i.renderings.is_empty()))
            }
            // Any Bible with Tagged words.
            GuideSection::Occurrences => {
                needed.any(|e| indexes.loaded(e).is_some_and(|i| !i.totals.is_empty()))
            }
            // Forms need a Hebrew or Greek Bible (TAHOT, TAGNT).
            GuideSection::Forms => needed.any(|e| {
                [
                    obs_scripture::StrongsLanguage::Hebrew,
                    obs_scripture::StrongsLanguage::Greek,
                ]
                .into_iter()
                .any(|l| crate::key_words::is_original_language(&e.manifest.language, l))
            }),
            _ => needed.next().is_some(),
        };
        match (self, has) {
            (GuideSection::Commentaries, false) => {
                "None in your library. Add a Commentary to see what it says about this Passage."
            }
            (GuideSection::CrossReferences, false) => {
                "None in your library. Add a Cross-reference set to see related Passages."
            }
            (GuideSection::Commentaries, true) => {
                "None in your library: no Commentary has a section on this Passage."
            }
            (GuideSection::CrossReferences, true) => {
                "None in your library: no Cross-references start in this Passage."
            }
            (GuideSection::KeyWords, false) => {
                "None in your library. Add a Bible with Strong's numbers to see the Passage's key words."
            }
            (GuideSection::KeyWords, true) => {
                "None in your library: no Bible with Strong's numbers has this Passage."
            }
            (GuideSection::PeopleAndPlaces, false) => {
                "None in your library. Add a People & places index to see who and where this Passage mentions."
            }
            (GuideSection::PeopleAndPlaces, true) => {
                "None in your library: no person or place is listed for this Passage."
            }
            (GuideSection::LexiconEntries, false) => {
                "None in your library. Add a Lexicon to read its entry for this word."
            }
            (GuideSection::LexiconEntries, true) => {
                "None in your library: no Lexicon has an entry for this Strong's number."
            }
            (GuideSection::TranslatedAs, false) => {
                "None in your library. Add an English Bible with Strong's numbers (the KJV with \
                 Strong's) to see how this word is translated."
            }
            (GuideSection::TranslatedAs, true) => {
                "None in your library: no English Bible with Strong's numbers translates this word."
            }
            (GuideSection::Occurrences, false) => {
                "None in your library. Add a Bible with Strong's numbers to see where this word \
                 occurs."
            }
            (GuideSection::Occurrences, true) => {
                "None in your library: no Bible with Strong's numbers has this word."
            }
            (GuideSection::Forms, false) => {
                "None in your library. Add a Hebrew or Greek Bible with Morphology to see this \
                 word's forms: TAHOT and TAGNT come with the Starter library, and more are in \
                 Get Resources › Hebrew & Greek."
            }
            (GuideSection::Forms, true) => {
                "None in your library: no Hebrew or Greek Bible with Morphology has this word."
            }
            (GuideSection::Summary | GuideSection::KeyPassages | GuideSection::Related, false) => {
                "None in your library. Add a People & places index (TIPNR) to see this."
            }
            (GuideSection::Summary, true) => {
                "Nothing more is known: no other names, name meaning or family."
            }
            (GuideSection::KeyPassages, true) => {
                "None in your library: no Passage is listed for this person or place."
            }
            (GuideSection::Related, true) => {
                "None in your library: no person or place is linked or named with them."
            }
            (GuideSection::DictionaryArticles, false) => {
                "None in your library. Add a Dictionary (Easton's, Smith's) to read about them."
            }
            (GuideSection::DictionaryArticles, true) => {
                "None in your library: no Dictionary has an article on them."
            }
        }
    }
}

impl Guide {
    /// Stable id, as stored and as passed to commands.
    pub fn id(self) -> &'static str {
        match self {
            Guide::PassageGuide => "passage_guide",
            Guide::WordStudy => "word_study",
            Guide::Factbook => "factbook",
        }
    }

    pub const ALL: [Guide; 3] = [Guide::PassageGuide, Guide::WordStudy, Guide::Factbook];

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|g| g.id() == id)
    }

    /// The Guide's sections in their default order.
    pub fn sections(self) -> &'static [GuideSection] {
        match self {
            Guide::PassageGuide => &[
                GuideSection::Commentaries,
                GuideSection::CrossReferences,
                GuideSection::KeyWords,
                GuideSection::PeopleAndPlaces,
            ],
            Guide::WordStudy => &[
                GuideSection::LexiconEntries,
                GuideSection::TranslatedAs,
                GuideSection::Occurrences,
                GuideSection::Forms,
            ],
            // The Atlas (#150) adds Map for places.
            Guide::Factbook => &[
                GuideSection::Summary,
                GuideSection::KeyPassages,
                GuideSection::DictionaryArticles,
                GuideSection::Related,
            ],
        }
    }
}

/// One section's place in a Guide arrangement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectionPlace {
    pub section: GuideSection,
    pub collapsed: bool,
    pub hidden: bool,
}

impl SectionPlace {
    fn new(section: GuideSection) -> Self {
        Self {
            section,
            collapsed: section.collapsed_by_default(),
            hidden: false,
        }
    }
}

/// The order of a Guide's sections and which are collapsed or hidden. Always holds each of
/// the Guide's sections exactly once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuideArrangement {
    pub sections: Vec<SectionPlace>,
}

/// The stored form. Section ids stay strings so an arrangement saved by a later version with
/// more sections still loads (unknown ids are dropped).
#[derive(Serialize, Deserialize)]
struct Stored {
    sections: Vec<StoredPlace>,
}

#[derive(Serialize, Deserialize)]
struct StoredPlace {
    id: String,
    #[serde(default)]
    collapsed: bool,
    #[serde(default)]
    hidden: bool,
}

impl GuideArrangement {
    pub fn default_for(guide: Guide) -> Self {
        Self {
            sections: guide
                .sections()
                .iter()
                .copied()
                .map(SectionPlace::new)
                .collect(),
        }
    }

    /// Read a saved arrangement. Unknown or repeated sections are dropped, and sections the
    /// save does not mention (added by a later ticket) go to their default place.
    pub fn from_json(guide: Guide, json: &str) -> Self {
        let Ok(stored) = serde_json::from_str::<Stored>(json) else {
            return Self::default_for(guide);
        };
        let defaults = guide.sections();
        let mut sections: Vec<SectionPlace> = Vec::new();
        for p in stored.sections {
            if let Some(section) = GuideSection::from_id(&p.id)
                && defaults.contains(&section)
                && !sections.iter().any(|s| s.section == section)
            {
                sections.push(SectionPlace {
                    section,
                    collapsed: p.collapsed,
                    hidden: p.hidden,
                });
            }
        }
        for (i, s) in defaults.iter().enumerate() {
            if sections.iter().any(|p| p.section == *s) {
                continue;
            }
            let at = defaults[..i]
                .iter()
                .rev()
                .find_map(|before| sections.iter().position(|p| p.section == *before))
                .map_or(0, |p| p + 1);
            sections.insert(at, SectionPlace::new(*s));
        }
        Self { sections }
    }

    pub fn to_json(&self) -> String {
        let stored = Stored {
            sections: self
                .sections
                .iter()
                .map(|p| StoredPlace {
                    id: p.section.id().into(),
                    collapsed: p.collapsed,
                    hidden: p.hidden,
                })
                .collect(),
        };
        serde_json::to_string(&stored).unwrap_or_default()
    }

    pub fn place(&self, section: GuideSection) -> Option<&SectionPlace> {
        self.sections.iter().find(|p| p.section == section)
    }

    fn place_mut(&mut self, section: GuideSection) -> Option<&mut SectionPlace> {
        self.sections.iter_mut().find(|p| p.section == section)
    }

    /// The sections shown, in order.
    pub fn visible(&self) -> impl Iterator<Item = &SectionPlace> {
        self.sections.iter().filter(|p| !p.hidden)
    }

    /// Move a section to index `to` of the list without it (clamped to the end). Returns
    /// whether anything changed.
    pub fn move_to(&mut self, section: GuideSection, to: usize) -> bool {
        let Some(from) = self.sections.iter().position(|p| p.section == section) else {
            return false;
        };
        let place = self.sections.remove(from);
        let to = to.min(self.sections.len());
        self.sections.insert(to, place);
        from != to
    }

    pub fn set_collapsed(&mut self, section: GuideSection, collapsed: bool) -> bool {
        self.place_mut(section)
            .is_some_and(|p| std::mem::replace(&mut p.collapsed, collapsed) != collapsed)
    }

    pub fn set_hidden(&mut self, section: GuideSection, hidden: bool) -> bool {
        self.place_mut(section)
            .is_some_and(|p| std::mem::replace(&mut p.hidden, hidden) != hidden)
    }
}

/// One Commentary's row in a Passage Guide: the section of that Commentary that best covers
/// the Passage, with its heading and the start of its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentaryRow {
    pub resource_id: String,
    pub title: String,
    pub abbreviation: String,
    /// The section's Passage Key (Standard versification).
    pub section: Passage,
    /// The section's own heading, when its text starts with one.
    pub heading: Option<String>,
    /// The section's text after the heading, whitespace collapsed, at most
    /// [`EXCERPT_CHARS`] characters (the row shows its first ~3 lines; hovering shows more).
    pub excerpt: String,
    pub direction: Direction,
}

/// Longest excerpt kept for a Commentary row.
pub const EXCERPT_CHARS: usize = 900;

/// What one section found for a Key. Pure data: the UI draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SectionData {
    /// One row per Commentary that has a section on the Passage, in Preferred order.
    Commentaries(Vec<CommentaryRow>),
    /// Cross-references grouped by source Verse, ranked by votes (see
    /// [`crate::cross_references::group_references`]).
    CrossReferences(Vec<ReferenceGroup>),
    /// Distinct Strong's numbers of the Passage, rarest first (#54).
    KeyWords(KeyWords),
    /// People and places mentioned in the Passage, in reading order (#54).
    PeopleAndPlaces(Vec<PersonOrPlace>),
    /// Every Lexicon's entry for a Strong's number, in Preferred order (#55).
    LexiconEntries(Vec<crate::word_study::LexiconEntry>),
    /// English renderings of a Strong's number, most frequent first (#56).
    TranslatedAs(crate::word_sections::TranslatedAs),
    /// Where a Strong's number occurs, Verse by Verse (#56).
    Occurrences(crate::word_sections::OccurrenceList),
    /// A Strong's number's grammatical forms, most frequent first (#56).
    Forms(crate::word_sections::Forms),
    /// A Factbook's Summary (#149).
    FactbookSummary(crate::factbook::FactbookSummary),
    /// Every Passage that mentions a person or place, in Bible order (#149).
    KeyPassages(Vec<Passage>),
    /// Each Dictionary's article on a person or place, in Preferred order (#149).
    DictionaryArticles(Vec<crate::factbook::DictionaryArticle>),
    /// People and places related to one (#149).
    Related(Vec<crate::factbook::RelatedEntity>),
}

impl SectionData {
    /// Rows (Commentaries) or Cross-references, for the section header's count.
    pub fn count(&self) -> usize {
        match self {
            SectionData::Commentaries(rows) => rows.len(),
            SectionData::CrossReferences(groups) => groups.iter().map(|g| g.rows.len()).sum(),
            SectionData::KeyWords(k) => k.words.len(),
            SectionData::PeopleAndPlaces(rows) => rows.len(),
            SectionData::LexiconEntries(entries) => entries.len(),
            SectionData::TranslatedAs(t) => t.renderings.len(),
            SectionData::Occurrences(o) => o.count() as usize,
            SectionData::Forms(f) => f.forms.len(),
            SectionData::FactbookSummary(s) => usize::from(!s.is_empty()),
            SectionData::KeyPassages(p) => p.len(),
            SectionData::DictionaryArticles(a) => a.len(),
            SectionData::Related(r) => r.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.count() == 0
    }
}

/// Where a section is for one Key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SectionStatus {
    /// Queued or running on the job queue.
    Loading,
    Ready(Arc<SectionData>),
    Failed(String),
}

/// Computes sections on a worker. The library-backed [`LibrarySections`] is the default;
/// tests install their own to hold a section back.
pub trait SectionSource: std::fmt::Debug + Send + Sync {
    fn compute(
        &self,
        library: &Library,
        key: &Key,
        section: GuideSection,
        ctx: &JobContext,
    ) -> Result<SectionData, String>;
}

/// Reads sections from the library's `.obr` files. Each Commentary's Passage Keys are loaded
/// once per Resource revision and shared by later jobs, as are Bibles' Verse maps and
/// Strong's totals and the Dictionary alias tables.
#[derive(Debug, Default)]
pub struct LibrarySections {
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    navigation: Mutex<BTreeMap<(String, u32, String), Arc<ResourceNavigation>>>,
    bibles: BibleCache,
    aliases: Arc<AliasCache>,
    /// Bibles' rendering indexes, shared with the Word Study header and chooser.
    indexes: Arc<crate::word_study::RenderingIndexes>,
    concordances: crate::word_sections::Concordances,
    /// The People & places directory, shared with the Session's Factbooks (#149).
    directory: Arc<crate::factbook::DirectoryCache>,
}

impl SectionSource for LibrarySections {
    fn compute(
        &self,
        library: &Library,
        key: &Key,
        section: GuideSection,
        _: &JobContext,
    ) -> Result<SectionData, String> {
        if let Key::Headword(entity) = key {
            return crate::factbook::section(
                library,
                &self.directory,
                &self.aliases,
                entity,
                section,
            );
        }
        if let Key::StrongsNumber(n) = key {
            use crate::word_sections as ws;
            let n = *n;
            return match section {
                GuideSection::LexiconEntries => {
                    crate::word_study::lexicon_entries(library, n).map(SectionData::LexiconEntries)
                }
                GuideSection::TranslatedAs => Ok(SectionData::TranslatedAs(ws::translated_as(
                    library,
                    &self.indexes,
                    n,
                ))),
                GuideSection::Occurrences => {
                    ws::occurrences(library, &self.indexes, &self.concordances, n)
                        .map(SectionData::Occurrences)
                }
                GuideSection::Forms => {
                    ws::forms(library, &self.indexes, &self.concordances, n).map(SectionData::Forms)
                }
                _ => Err(format!("{} needs a Passage", section.title())),
            };
        }
        let Key::Passage(passage) = key else {
            return Err(format!("{} needs a Passage", section.title()));
        };
        match section {
            GuideSection::Commentaries => self
                .commentary_rows(library, *passage)
                .map(SectionData::Commentaries),
            GuideSection::CrossReferences => {
                crate::cross_references::library_references(library, *passage)
                    .map(SectionData::CrossReferences)
            }
            GuideSection::KeyWords => {
                crate::key_words::library_key_words(library, *passage, &self.bibles)
                    .map(SectionData::KeyWords)
            }
            GuideSection::PeopleAndPlaces => crate::people_places::library_people_and_places(
                library,
                *passage,
                &self.bibles,
                &self.aliases,
            )
            .map(SectionData::PeopleAndPlaces),
            GuideSection::LexiconEntries
            | GuideSection::TranslatedAs
            | GuideSection::Occurrences
            | GuideSection::Forms => Err(format!("{} needs a Strong's number", section.title())),
            GuideSection::Summary
            | GuideSection::KeyPassages
            | GuideSection::DictionaryArticles
            | GuideSection::Related => Err(format!("{} needs a person or place", section.title())),
        }
    }
}

impl LibrarySections {
    /// Sections that share `aliases` with the Session (which keeps them current).
    pub fn with_aliases(aliases: Arc<AliasCache>) -> Self {
        Self {
            aliases,
            ..Self::default()
        }
    }

    /// Sections that share `aliases`, the Bibles' rendering `indexes` and the People &
    /// places `directory` with the Session.
    pub fn with_shared(
        aliases: Arc<AliasCache>,
        indexes: Arc<crate::word_study::RenderingIndexes>,
        directory: Arc<crate::factbook::DirectoryCache>,
    ) -> Self {
        Self {
            aliases,
            indexes,
            directory,
            ..Self::default()
        }
    }

    /// One row per Commentary in Preferred order that has a section on `passage`. A
    /// Commentary that cannot be read is skipped so the others still show; the section only
    /// fails when every Commentary failed.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn commentary_rows(
        &self,
        library: &Library,
        passage: Passage,
    ) -> Result<Vec<CommentaryRow>, String> {
        let mut rows = Vec::new();
        let mut errors = Vec::new();
        for entry in library.of_type(ResourceType::Commentary) {
            match self.commentary_entry_row(entry, passage) {
                Ok(Some(row)) => rows.push(row),
                Ok(None) => {}
                Err(e) => errors.push(format!("{}: {e}", entry.manifest.abbreviation)),
            }
        }
        if rows.is_empty() && !errors.is_empty() {
            return Err(errors.join("; "));
        }
        Ok(rows)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn commentary_entry_row(
        &self,
        entry: &crate::library::LibraryEntry,
        passage: Passage,
    ) -> Result<Option<CommentaryRow>, StoreError> {
        let m = &entry.manifest;
        let reader = obs_store::ObrReader::open(&entry.path)?;
        let identity = (m.resource_id.clone(), m.revision, m.content_hash.clone());
        let cached = self
            .navigation
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&identity)
            .cloned();
        let nav = match cached {
            Some(nav) => nav,
            None => {
                let keys = reader.keys(obs_model::KeyScheme::Passage)?;
                let nav = Arc::new(ResourceNavigation::new(keys));
                let mut navs = self.navigation.lock().unwrap_or_else(|p| p.into_inner());
                navs.retain(|(id, _, _), _| *id != identity.0);
                navs.insert(identity, Arc::clone(&nav));
                nav
            }
        };
        commentary_row(&reader, &nav, passage)
    }

    #[cfg(target_arch = "wasm32")]
    pub fn commentary_rows(&self, _: &Library, _: Passage) -> Result<Vec<CommentaryRow>, String> {
        Err("Opening Resources is not supported here".into())
    }
}

/// The section of one Commentary that best covers `passage`, as a row; `None` when no
/// section's Passage overlaps it. The section that the Commentary's own Tab would show for
/// the Passage's first Verse (the nearest earlier section, narrowest first) wins when it
/// overlaps; otherwise the first section starting inside the Passage; otherwise the latest,
/// narrowest section that still covers part of it (a chapter or book introduction).
pub fn commentary_row(
    store: &dyn ResourceStore,
    nav: &ResourceNavigation,
    passage: Passage,
) -> Result<Option<CommentaryRow>, StoreError> {
    let Some((section, entry)) = section_for(nav, passage) else {
        return Ok(None);
    };
    let Some(unit) = store.unit(&entry.unit)? else {
        return Ok(None);
    };
    let range = entry.range.map_or(0..unit.text.len(), |(s, e)| {
        (s as usize).min(unit.text.len())..(e as usize).min(unit.text.len())
    });
    let (heading, excerpt) = heading_and_excerpt(&unit, range);
    let m = store.manifest();
    Ok(Some(CommentaryRow {
        resource_id: m.resource_id.clone(),
        title: m.title.clone(),
        abbreviation: m.abbreviation.clone(),
        section,
        heading,
        excerpt,
        direction: m.direction,
    }))
}

fn section_for(nav: &ResourceNavigation, passage: Passage) -> Option<(Passage, &KeyEntry)> {
    let as_passage = |e: &KeyEntry| match e.key {
        Key::Passage(p) => Some(p),
        _ => None,
    };
    let overlapping = |e: &&KeyEntry| as_passage(e).is_some_and(|p| p.overlaps(passage));
    let entry = nav
        .section(passage.start())
        .filter(overlapping)
        .or_else(|| {
            nav.keys
                .iter()
                .filter(|e| !e.alias)
                .find(|e| as_passage(e).is_some_and(|p| passage.contains_verse(p.start())))
        })
        .or_else(|| {
            nav.keys
                .iter()
                .filter(|e| !e.alias)
                .filter(overlapping)
                .max_by(|a, b| {
                    let (a, b) = (as_passage(a), as_passage(b));
                    // Latest start, then the narrowest.
                    a.map(|p| p.start())
                        .cmp(&b.map(|p| p.start()))
                        .then(b.map(|p| p.end()).cmp(&a.map(|p| p.end())))
                })
        })?;
    Some((as_passage(entry)?, entry))
}

/// The heading a section's text starts with, and the text after it (whitespace collapsed,
/// cut at a word boundary to [`EXCERPT_CHARS`]).
pub(crate) fn heading_and_excerpt(
    unit: &TextUnit,
    range: Range<usize>,
) -> (Option<String>, String) {
    let text = &unit.text;
    let heading = unit.spans.iter().find(|s| {
        matches!(s.kind, SpanKind::Heading { .. })
            && s.start as usize >= range.start
            && s.end as usize <= range.end
            && text
                .get(range.start..s.start as usize)
                .is_some_and(|before| before.trim().is_empty())
    });
    let (heading, from) = match heading {
        Some(s) => (
            text.get(s.range()).map(collapse).filter(|h| !h.is_empty()),
            s.end as usize,
        ),
        None => (None, range.start),
    };
    let body = text.get(from..range.end).map(collapse).unwrap_or_default();
    (heading, cut(&body, EXCERPT_CHARS))
}

#[cfg(test)]
#[allow(clippy::expect_used)]
pub(crate) fn heading_and_excerpt_for_tests(
    text: &str,
    heading_end: u32,
) -> (Option<String>, String) {
    let spans = (heading_end > 0)
        .then_some(obs_model::Span {
            start: 0,
            end: heading_end,
            kind: SpanKind::Heading { level: 2 },
        })
        .into_iter()
        .collect();
    let unit = TextUnit {
        id: obs_model::UnitId::new("test").expect("unit id"),
        kind: obs_model::UnitKind::Section,
        title: None,
        text: text.into(),
        spans,
    };
    heading_and_excerpt(&unit, 0..text.len())
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// At most `max` characters, ending at a word boundary with "…" when shortened.
pub(crate) fn cut(s: &str, max: usize) -> String {
    let Some((end, _)) = s.char_indices().nth(max) else {
        return s.to_string();
    };
    let head = &s[..end];
    let head = head.rfind(' ').map_or(head, |space| &head[..space]);
    format!("{}…", head.trim_end())
}

/// Guide state for a Session: the arrangements, section results and the jobs computing them.
#[derive(Debug)]
pub struct Guides {
    arrangements: BTreeMap<Guide, GuideArrangement>,
    results: BTreeMap<(Key, GuideSection), SectionStatus>,
    pending: BTreeMap<JobId, (Key, GuideSection)>,
    /// Sections asked for since the last poll; pending jobs nobody asks for are cancelled.
    wanted: BTreeSet<(Key, GuideSection)>,
    library: Vec<(String, u32, String, std::path::PathBuf)>,
    source: Arc<dyn SectionSource>,
    /// The People & places ↔ Dictionary alias caches (#54), shared with the default source.
    aliases: Arc<AliasCache>,
    /// The job refreshing them at library load.
    alias_job: Option<JobId>,
    /// Bibles' rendering indexes, shared with the default source and the Word Study.
    indexes: Arc<crate::word_study::RenderingIndexes>,
    /// The People & places directory, shared with the default source and the Factbook.
    directory: Arc<crate::factbook::DirectoryCache>,
}

impl Default for Guides {
    fn default() -> Self {
        let aliases = Arc::new(AliasCache::default());
        let indexes = Arc::new(crate::word_study::RenderingIndexes::default());
        let directory = Arc::new(crate::factbook::DirectoryCache::default());
        Self {
            arrangements: BTreeMap::new(),
            results: BTreeMap::new(),
            pending: BTreeMap::new(),
            wanted: BTreeSet::new(),
            library: Vec::new(),
            source: Arc::new(LibrarySections::with_shared(
                Arc::clone(&aliases),
                Arc::clone(&indexes),
                Arc::clone(&directory),
            )),
            aliases,
            alias_job: None,
            indexes,
            directory,
        }
    }
}

/// Section results kept at most (older Keys are recomputed when asked for again).
const RESULTS_LIMIT: usize = 256;

impl Guides {
    /// How sections are computed (tests hold one back).
    pub fn set_source(&mut self, source: Arc<dyn SectionSource>) {
        self.source = source;
        self.results.clear();
    }

    /// Whether a section job (or the alias refresh) is queued or running.
    pub fn is_busy(&self) -> bool {
        !self.pending.is_empty() || self.alias_job.is_some()
    }

    /// The People & places ↔ Dictionary alias caches.
    pub fn aliases(&self) -> &Arc<AliasCache> {
        &self.aliases
    }

    /// The People & places directory the default source and the Factbook read.
    pub fn directory(&self) -> Arc<crate::factbook::DirectoryCache> {
        Arc::clone(&self.directory)
    }

    /// The Bibles' rendering indexes the default source reads.
    pub fn indexes(&self) -> Arc<crate::word_study::RenderingIndexes> {
        Arc::clone(&self.indexes)
    }

    /// The arrangement for a kind of Guide.
    pub fn arrangement(&self, guide: Guide) -> GuideArrangement {
        self.arrangements
            .get(&guide)
            .cloned()
            .unwrap_or_else(|| GuideArrangement::default_for(guide))
    }
}

fn pref(guide: Guide) -> String {
    format!("guide-arrangement/{}", guide.id())
}

impl Session {
    /// Where a section is for `key`. Never waits: a section not computed yet is submitted to
    /// the job queue and reported as [`SectionStatus::Loading`]; ask again on a later frame.
    pub fn guide_section(&mut self, key: &Key, section: GuideSection) -> SectionStatus {
        let fingerprint: Vec<_> = self
            .library
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
            .collect();
        if fingerprint != self.guides.library {
            // The library changed: every result may be stale.
            self.guides.library = fingerprint;
            self.guides.results.clear();
            for id in self.guides.pending.keys() {
                self.jobs().cancel(*id);
            }
            self.guides.pending.clear();
        }
        let wanted = (key.clone(), section);
        if let Some(status) = self.guides.results.get(&wanted) {
            return status.clone();
        }
        self.guides.wanted.insert(wanted.clone());
        if !self.guides.pending.values().any(|w| *w == wanted) {
            let source = Arc::clone(&self.guides.source);
            let library = self.library.clone();
            let job_key = key.clone();
            let job = self
                .jobs()
                .submit_fn(format!("Guide: {}", section.title()), move |ctx| {
                    source.compute(&library, &job_key, section, ctx)
                });
            self.guides.pending.insert(job, wanted);
        }
        SectionStatus::Loading
    }

    /// Apply finished section jobs; return the other events. Pending sections nobody asked
    /// for since the last poll (the Guide moved on, or closed) are cancelled.
    pub(crate) fn poll_guides(&mut self, events: Vec<JobEvent>) -> Vec<JobEvent> {
        let wanted = std::mem::take(&mut self.guides.wanted);
        for (id, w) in &self.guides.pending {
            if !wanted.contains(w) {
                self.jobs().cancel(*id);
            }
        }
        let mut other = Vec::new();
        for event in events {
            if Some(event.id) == self.guides.alias_job {
                if let JobEventKind::Failed(e) = &event.kind {
                    self.workspace.push_effect(Effect::Message(format!(
                        "Could not match People & places to Dictionaries: {e}"
                    )));
                }
                if event.kind.is_terminal() {
                    self.guides.alias_job = None;
                    // Sections computed before the refresh may have missed an article.
                    self.guides.results.retain(|(_, s), _| {
                        !matches!(
                            s,
                            GuideSection::PeopleAndPlaces
                                | GuideSection::Summary
                                | GuideSection::DictionaryArticles
                        )
                    });
                }
                continue;
            }
            if !self.guides.pending.contains_key(&event.id) {
                other.push(event);
                continue;
            }
            if !event.kind.is_terminal() {
                continue;
            }
            let Some(w) = self.guides.pending.remove(&event.id) else {
                continue;
            };
            let status = match event.kind {
                JobEventKind::Finished(out) => match out.downcast::<SectionData>() {
                    Ok(data) => SectionStatus::Ready(Arc::new(*data)),
                    Err(_) => SectionStatus::Failed("Unexpected Guide section result".into()),
                },
                JobEventKind::Failed(e) => SectionStatus::Failed(e),
                _ => continue,
            };
            if self.guides.results.len() >= RESULTS_LIMIT {
                self.guides.results.clear();
            }
            self.guides.results.insert(w, status);
        }
        other
    }

    /// Where alias caches are written ([`crate::aliases::default_alias_cache_dir`] in the app).
    pub fn set_alias_cache_dir(&mut self, dir: std::path::PathBuf) {
        self.guides.aliases.set_dir(dir);
    }

    /// Make the People & places ↔ Dictionary alias cache of every pair in the library current,
    /// in the background: at library load, a table whose Resources changed is rebuilt (#20).
    pub fn refresh_alias_caches(&mut self) {
        if let Some(job) = self.guides.alias_job.take() {
            self.jobs().cancel(job);
        }
        let aliases = Arc::clone(&self.guides.aliases);
        let library = self.library.clone();
        let job = self
            .jobs()
            .submit_fn("Dictionary aliases", move |_| aliases.refresh(&library));
        self.guides.alias_job = Some(job);
    }

    /// Load each kind of Guide's saved arrangement from the User database.
    pub(crate) fn load_guide_arrangements(&mut self) {
        let Some(user) = &self.user else {
            return;
        };
        for guide in Guide::ALL {
            if let Ok(Some(json)) = user.preference(&pref(guide)) {
                self.guides
                    .arrangements
                    .insert(guide, GuideArrangement::from_json(guide, &json));
            }
        }
    }

    /// Change a kind of Guide's arrangement and save it.
    fn arrange(
        &mut self,
        guide: Guide,
        change: impl FnOnce(&mut GuideArrangement),
    ) -> Result<Value, CommandError> {
        let mut arrangement = self.guides.arrangement(guide);
        change(&mut arrangement);
        let json = arrangement.to_json();
        self.guides.arrangements.insert(guide, arrangement);
        if let Some(user) = &self.user
            && let Err(e) = user.set_preference(&pref(guide), &json)
        {
            self.workspace.push_effect(Effect::Message(format!(
                "Could not save the Guide arrangement: {e}"
            )));
        }
        Ok(arrangement_json(&self.guides.arrangement(guide)))
    }
}

fn arrangement_json(a: &GuideArrangement) -> Value {
    Value::Array(
        a.sections
            .iter()
            .map(|p| json!({ "section": p.section.id(), "collapsed": p.collapsed, "hidden": p.hidden }))
            .collect(),
    )
}

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

const GUIDE: ParamSpec = ParamSpec {
    name: "guide",
    kind: ParamKind::String,
    required: true,
    help: "Kind of Guide: `passage_guide`, `word_study` or `factbook`",
};
const SECTION: ParamSpec = ParamSpec {
    name: "section",
    kind: ParamKind::String,
    required: true,
    help: "Section id, e.g. `commentaries` or `cross_references`",
};

fn guide_param(command: &'static str, p: &Params<'_>) -> Result<Guide, CommandError> {
    let id = p.str("guide").unwrap_or_default();
    Guide::from_id(id).ok_or_else(|| failed(command, format!("no Guide `{id}`")))
}

fn section_param(
    command: &'static str,
    guide: Guide,
    p: &Params<'_>,
) -> Result<GuideSection, CommandError> {
    let id = p.str("section").unwrap_or_default();
    GuideSection::from_id(id)
        .filter(|s| guide.sections().contains(s))
        .ok_or_else(|| failed(command, format!("{} has no section `{id}`", guide.title())))
}

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "guide.move_section",
        label: "Move Guide section",
        shortcut: None,
        params: &[
            GUIDE,
            SECTION,
            ParamSpec {
                name: "to",
                kind: ParamKind::Integer,
                required: true,
                help: "New position in the Guide; 0 is the top",
            },
        ],
        enabled: always,
        run: |s, p| {
            const ID: &str = "guide.move_section";
            let guide = guide_param(ID, &p)?;
            let section = section_param(ID, guide, &p)?;
            let to = p
                .i64("to")
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(|| failed(ID, "`to` must be 0 or more"))?;
            s.arrange(guide, |a| {
                a.move_to(section, to);
            })
        },
    },
    CommandSpec {
        id: "guide.collapse_section",
        label: "Collapse or expand Guide section",
        shortcut: None,
        params: &[
            GUIDE,
            SECTION,
            ParamSpec {
                name: "collapsed",
                kind: ParamKind::Boolean,
                required: false,
                help: "Collapse (true) or expand (false); toggles when missing",
            },
        ],
        enabled: always,
        run: |s, p| {
            const ID: &str = "guide.collapse_section";
            let guide = guide_param(ID, &p)?;
            let section = section_param(ID, guide, &p)?;
            let now = s
                .guides
                .arrangement(guide)
                .place(section)
                .is_some_and(|p| p.collapsed);
            let collapsed = p.bool("collapsed").unwrap_or(!now);
            s.arrange(guide, |a| {
                a.set_collapsed(section, collapsed);
            })
        },
    },
    CommandSpec {
        id: "guide.hide_section",
        label: "Hide or show Guide section",
        shortcut: None,
        params: &[
            GUIDE,
            SECTION,
            ParamSpec {
                name: "hidden",
                kind: ParamKind::Boolean,
                required: false,
                help: "Hide (true, the default) or show again (false)",
            },
        ],
        enabled: always,
        run: |s, p| {
            const ID: &str = "guide.hide_section";
            let guide = guide_param(ID, &p)?;
            let section = section_param(ID, guide, &p)?;
            let hidden = p.bool("hidden").unwrap_or(true);
            s.arrange(guide, |a| {
                a.set_hidden(section, hidden);
            })
        },
    },
    CommandSpec {
        id: "guide.reset_arrangement",
        label: "Reset arrangement",
        shortcut: None,
        params: &[GUIDE],
        enabled: always,
        run: |s, p| {
            const ID: &str = "guide.reset_arrangement";
            let guide = guide_param(ID, &p)?;
            s.arrange(guide, |a| *a = GuideArrangement::default_for(guide))
        },
    },
];
