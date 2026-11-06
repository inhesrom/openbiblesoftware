//! What an empty Guide section lacks (#135), as data the UI turns into buttons: the Resource
//! type and tagging it needs, the Get Resources filter that lists such Resources, and the
//! Starter library Resources that would fill it. Every "None in your library" state gets one,
//! so the app always says how to get the missing Resource, not only that it is missing.

use obs_model::{Key, ResourceType};
use obs_scripture::{StrongsLanguage, Testament};

use crate::get_bibles::ResourceFilter;
use crate::guide::GuideSection;
use crate::key_words::is_original_language;
use crate::starter::{StarterItem, StarterManifest};
use crate::{Library, STARTER_PREFERRED_ORDER, Session};

/// The original-language tagging a Bible needs for a section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tagging {
    /// No tagging (and not a Bible).
    None,
    /// Tagged words with Strong's numbers (Key words, Translated as, Occurrences).
    StrongsNumbers,
    /// Strong's numbers and Morphology in Hebrew or Greek (Forms).
    Morphology,
}

/// What the Starter library can do for an empty section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StarterOffer {
    /// Nothing: the Library has the kind of Resource already but none covers the Key, or
    /// every Starter Resource that would fill the section is in the Library.
    None,
    /// These Starter Resources (resource ids, in manifest order) would fill the section and
    /// are not in the Library.
    Resources(Vec<String>),
    /// No Starter library manifest is known yet (no bundle, and the released one not fetched).
    /// The Starter library is made to fill every Guide section, so it is offered; its Tab
    /// fetches the list.
    Unlisted,
}

/// What a "None in your library" state lacks, and where to get it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingResource {
    pub resource_type: ResourceType,
    pub tagging: Tagging,
    /// The Get Resources filter that lists Resources that would fill it; `None` when the
    /// online repositories do not offer the Resource type (Cross-reference sets, People &
    /// places indexes).
    pub get_resources: Option<ResourceFilter>,
    pub starter: StarterOffer,
}

impl MissingResource {
    /// The Get Resources button's label: "Get Lexicons…", "Get Hebrew & Greek texts…".
    pub fn get_label(&self) -> Option<String> {
        self.get_resources.map(|f| match f {
            ResourceFilter::HebrewAndGreek => "Get Hebrew & Greek texts…".to_string(),
            ResourceFilter::All => "Get Resources…".to_string(),
            ResourceFilter::Type(_) => format!("Get {}…", f.label()),
        })
    }

    /// Whether the Starter library is offered.
    pub fn offers_starter(&self) -> bool {
        self.starter != StarterOffer::None
    }
}

/// A Guide section's empty state: what it says and what would fill it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmptyState {
    pub message: &'static str,
    pub missing: MissingResource,
}

impl GuideSection {
    /// The tagging a Bible needs for the section ([`Tagging::None`] for other types).
    pub fn tagging(self) -> Tagging {
        match self {
            GuideSection::KeyWords | GuideSection::TranslatedAs | GuideSection::Occurrences => {
                Tagging::StrongsNumbers
            }
            GuideSection::Forms | GuideSection::WordByWord | GuideSection::GrammaticalNotes => {
                Tagging::Morphology
            }
            GuideSection::Commentaries
            | GuideSection::CrossReferences
            | GuideSection::PeopleAndPlaces
            | GuideSection::LexiconEntries
            | GuideSection::Summary
            | GuideSection::KeyPassages
            | GuideSection::DictionaryArticles
            | GuideSection::Related
            | GuideSection::LexiconSummaries
            | GuideSection::TopicalEntries
            | GuideSection::TopicArticles
            | GuideSection::TopicPassages
            | GuideSection::TopicSearch => Tagging::None,
        }
    }

    /// The Get Resources filter that lists what the section reads: Bibles (whose rows show
    /// the Strong's badge) for the Strong's-number sections, Hebrew & Greek for Forms and the
    /// Exegetical Guide's Bible sections; `None` for a Resource type the online repositories
    /// do not offer (Cross-reference sets, People & places indexes).
    pub fn get_resources_filter(self) -> Option<ResourceFilter> {
        match self {
            GuideSection::Forms | GuideSection::WordByWord | GuideSection::GrammaticalNotes => {
                Some(ResourceFilter::HebrewAndGreek)
            }
            s => Some(ResourceFilter::Type(s.resource_type())).filter(|f| f.id().is_some()),
        }
    }

    /// Whether a Starter Resource would fill the section for a Key in the original
    /// `language` ([`key_language`]; `None`: either). The tagged English Bible is the first of
    /// the Starter Preferred order (the KJV with Strong's); Hebrew and Greek Bibles (TAHOT,
    /// TAGNT) carry Strong's numbers and Morphology.
    pub fn filled_by(self, item: &StarterItem, language: Option<StrongsLanguage>) -> bool {
        let Some(ty) = item.resource_type else {
            return false;
        };
        let original = |language: Option<StrongsLanguage>| {
            ty == ResourceType::Bible
                && [StrongsLanguage::Hebrew, StrongsLanguage::Greek]
                    .into_iter()
                    .filter(|l| language.is_none_or(|want| want == *l))
                    .any(|l| is_original_language(&item.language, l))
        };
        let tagged_english = ty == ResourceType::Bible
            && STARTER_PREFERRED_ORDER.first() == Some(&&*item.resource_id);
        match self {
            GuideSection::KeyWords => tagged_english || original(None),
            GuideSection::TranslatedAs => tagged_english,
            GuideSection::Occurrences => tagged_english || original(language),
            GuideSection::Forms | GuideSection::WordByWord | GuideSection::GrammaticalNotes => {
                original(language)
            }
            // Articles are found through the People & places index (TIPNR).
            GuideSection::DictionaryArticles => {
                ty == ResourceType::Dictionary || ty == ResourceType::PeopleAndPlaces
            }
            // The Starter library has no topical index (ADR 0009): Get Resources has them.
            GuideSection::TopicalEntries | GuideSection::TopicPassages => {
                crate::topic_guide::is_topical_index(&item.resource_id)
            }
            GuideSection::TopicArticles => {
                ty == ResourceType::Dictionary
                    && !crate::topic_guide::is_topical_index(&item.resource_id)
            }
            s => ty == s.resource_type(),
        }
    }

    /// Whether the Library has the kind of Resource the section reads with the tagging it
    /// needs ([`Self::tagging`]), so an empty section means nothing covers the Key. A Bible
    /// without Tagged words has the type Key words and Forms read ([`Self::has_needed`]) but
    /// can never fill them, nor can a Hebrew or Greek Bible with Strong's numbers but no
    /// Morphology fill Forms. Forms, Word by word and Grammatical notes need Morphology in
    /// the original `language` of the Key ([`key_language`]; `None`: Hebrew or Greek). Tagging is read from what the section's job loaded (never
    /// read here): a Bible whose rendering index (Strong's numbers) or concordance
    /// (Morphology) is not loaded counts as lacking it.
    pub fn has_needed_tagging(
        self,
        library: &Library,
        indexes: &crate::word_study::RenderingIndexes,
        concordances: &crate::word_sections::Concordances,
        language: Option<StrongsLanguage>,
    ) -> bool {
        let tagged = |e: &crate::LibraryEntry| {
            indexes
                .loaded(e)
                .is_some_and(|index| !index.totals.is_empty())
        };
        let mut bibles = library.of_type(ResourceType::Bible);
        match self {
            GuideSection::KeyWords => bibles.any(tagged),
            // Morphology in a Hebrew or Greek Bible (TAHOT, TAGNT) of the Key's language.
            GuideSection::Forms | GuideSection::WordByWord | GuideSection::GrammaticalNotes => {
                bibles.any(|e| {
                    [StrongsLanguage::Hebrew, StrongsLanguage::Greek]
                        .into_iter()
                        .filter(|l| language.is_none_or(|want| want == *l))
                        .any(|l| is_original_language(&e.manifest.language, l))
                        && concordances.loaded(e).is_some_and(|c| c.has_morphology())
                })
            }
            GuideSection::DictionaryArticles => {
                self.has_needed(library, indexes) && !self.lacks_people_index(library)
            }
            // Translated as and Occurrences already ask for Strong's numbers.
            _ => self.has_needed(library, indexes),
        }
    }

    /// Whether the section is empty for want of a People & places index whatever else the
    /// Library has: Dictionary articles are found through the index's names (#149), so a
    /// Dictionary alone never fills them.
    pub fn lacks_people_index(self, library: &Library) -> bool {
        self == GuideSection::DictionaryArticles
            && library
                .of_type(ResourceType::PeopleAndPlaces)
                .next()
                .is_none()
    }

    /// What would fill the empty section. `has` is [`Self::has_needed_tagging`]: when the
    /// Library already has the kind of Resource with its tagging, only Get Resources is
    /// offered, never the Starter library (#135: "no entry for this Strong's number" offers
    /// Get Lexicons only). Dictionary articles without a People & places index name the index
    /// once the Library has a Dictionary, and offer only the kinds the Library lacks.
    pub fn missing_resource(
        self,
        has: bool,
        language: Option<StrongsLanguage>,
        library: &Library,
        starter: Option<&StarterManifest>,
    ) -> MissingResource {
        // The Starter library is made without a topical index; never offer it for one.
        let never_starter = matches!(
            self,
            GuideSection::TopicalEntries | GuideSection::TopicPassages
        );
        let starter = match (has, starter) {
            (true, _) => StarterOffer::None,
            (false, _) if never_starter => StarterOffer::None,
            (false, None) => StarterOffer::Unlisted,
            (false, Some(manifest)) => {
                let ids: Vec<String> = manifest
                    .resources
                    .iter()
                    .filter(|i| {
                        self.filled_by(i, language)
                            && library.get(&i.resource_id).is_none()
                            && !(self == GuideSection::DictionaryArticles
                                && i.resource_type
                                    .is_some_and(|t| library.of_type(t).next().is_some()))
                    })
                    .map(|i| i.resource_id.clone())
                    .collect();
                if ids.is_empty() {
                    StarterOffer::None
                } else {
                    StarterOffer::Resources(ids)
                }
            }
        };
        // A Dictionary without the index: the index is what is missing, and the online
        // repositories offer none.
        let index_only = self.lacks_people_index(library)
            && library.of_type(ResourceType::Dictionary).next().is_some();
        MissingResource {
            resource_type: if index_only {
                ResourceType::PeopleAndPlaces
            } else {
                self.resource_type()
            },
            tagging: self.tagging(),
            get_resources: self.get_resources_filter().filter(|_| !index_only),
            starter,
        }
    }
}

/// The original language a Key is read in: a Strong's number's own; for a Passage, Hebrew in
/// the Old Testament and Greek in the New (by its first Verse); otherwise `None`.
pub fn key_language(key: &Key) -> Option<StrongsLanguage> {
    match key {
        Key::StrongsNumber(n) => Some(n.language()),
        Key::Passage(p) => Some(match p.start().book.testament() {
            Testament::Old => StrongsLanguage::Hebrew,
            Testament::New => StrongsLanguage::Greek,
        }),
        _ => None,
    }
}

impl Session {
    /// The empty state of `section` for `key`: its "None in your library" text and what would
    /// fill it. Asked for by the UI each frame, so it follows Resources being added.
    pub fn guide_empty_state(&self, key: &Key, section: GuideSection) -> EmptyState {
        let indexes = self.guides.indexes();
        let concordances = self.guides.concordances();
        let language = key_language(key);
        let has = match (section, language) {
            // The Exegetical Guide reads the Passage's own original language.
            (GuideSection::WordByWord | GuideSection::GrammaticalNotes, Some(l)) => self
                .library
                .of_type(ResourceType::Bible)
                .any(|e| is_original_language(&e.manifest.language, l)),
            _ => section.has_needed(&self.library, &indexes),
        };
        let tagged = section.has_needed_tagging(&self.library, &indexes, &concordances, language);
        let message = match (section.lacks_people_index(&self.library), has) {
            (true, true) => {
                "None in your library. Add a People & places index (TIPNR) to find the \
                 Dictionary articles on them."
            }
            (true, false) => {
                "None in your library. Add a People & places index (TIPNR) and a Dictionary \
                 (Easton's, Smith's) to read about them."
            }
            (false, _) => section.empty_message(has),
        };
        EmptyState {
            message,
            missing: section.missing_resource(
                tagged,
                language,
                &self.library,
                self.starter.manifest(),
            ),
        }
    }
}
