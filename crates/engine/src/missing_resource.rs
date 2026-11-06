//! What an empty Guide section lacks (#135), as data the UI turns into buttons: the Resource
//! type and tagging it needs, the Get Resources filter that lists such Resources, and the
//! Starter library Resources that would fill it. Every "None in your library" state gets one,
//! so the app always says how to get the missing Resource, not only that it is missing.

use obs_model::{Key, ResourceType};
use obs_scripture::{StrongsLanguage, StrongsNumber};

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
            GuideSection::Forms => Tagging::Morphology,
            GuideSection::Commentaries
            | GuideSection::CrossReferences
            | GuideSection::PeopleAndPlaces
            | GuideSection::LexiconEntries => Tagging::None,
        }
    }

    /// The Get Resources filter that lists what the section reads: Bibles (whose rows show
    /// the Strong's badge) for the Strong's-number sections, Hebrew & Greek for Forms.
    pub fn get_resources_filter(self) -> Option<ResourceFilter> {
        match self {
            GuideSection::Forms => Some(ResourceFilter::HebrewAndGreek),
            GuideSection::CrossReferences | GuideSection::PeopleAndPlaces => None,
            s => Some(ResourceFilter::Type(s.resource_type())),
        }
    }

    /// Whether a Starter Resource would fill the section for a Word Study on `strongs`
    /// (`None`: any number). The tagged English Bible is the first of the Starter Preferred
    /// order (the KJV with Strong's); Hebrew and Greek Bibles (TAHOT, TAGNT) carry Strong's
    /// numbers and Morphology.
    pub fn filled_by(self, item: &StarterItem, strongs: Option<StrongsNumber>) -> bool {
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
        let language = strongs.map(StrongsNumber::language);
        match self {
            GuideSection::KeyWords => tagged_english || original(None),
            GuideSection::TranslatedAs => tagged_english,
            GuideSection::Occurrences => tagged_english || original(language),
            GuideSection::Forms => original(language),
            s => ty == s.resource_type(),
        }
    }

    /// Whether the Library has the kind of Resource the section reads with the tagging it
    /// needs ([`Self::tagging`]), so an empty section means nothing covers the Key. A Bible
    /// without Tagged words has the type Key words and Forms read ([`Self::has_needed`]) but
    /// can never fill them. Tagging is read from the rendering indexes the section's job
    /// loaded (never read here): a Bible whose index is not loaded counts as untagged.
    pub fn has_needed_tagging(
        self,
        library: &Library,
        indexes: &crate::word_study::RenderingIndexes,
    ) -> bool {
        let tagged = |e: &crate::LibraryEntry| {
            indexes
                .loaded(e)
                .is_some_and(|index| !index.totals.is_empty())
        };
        let mut bibles = library.of_type(ResourceType::Bible);
        match self {
            GuideSection::KeyWords => bibles.any(tagged),
            // Strong's numbers in a Hebrew or Greek Bible (TAHOT, TAGNT).
            GuideSection::Forms => bibles.any(|e| {
                [StrongsLanguage::Hebrew, StrongsLanguage::Greek]
                    .into_iter()
                    .any(|l| is_original_language(&e.manifest.language, l))
                    && tagged(e)
            }),
            // Translated as and Occurrences already ask for Strong's numbers.
            _ => self.has_needed(library, indexes),
        }
    }

    /// What would fill the empty section. `has` is [`Self::has_needed_tagging`]: when the
    /// Library already has the kind of Resource with its tagging, only Get Resources is
    /// offered, never the Starter library (#135: "no entry for this Strong's number" offers
    /// Get Lexicons only).
    pub fn missing_resource(
        self,
        has: bool,
        strongs: Option<StrongsNumber>,
        library: &Library,
        starter: Option<&StarterManifest>,
    ) -> MissingResource {
        let starter = match (has, starter) {
            (true, _) => StarterOffer::None,
            (false, None) => StarterOffer::Unlisted,
            (false, Some(manifest)) => {
                let ids: Vec<String> = manifest
                    .resources
                    .iter()
                    .filter(|i| self.filled_by(i, strongs) && library.get(&i.resource_id).is_none())
                    .map(|i| i.resource_id.clone())
                    .collect();
                if ids.is_empty() {
                    StarterOffer::None
                } else {
                    StarterOffer::Resources(ids)
                }
            }
        };
        MissingResource {
            resource_type: self.resource_type(),
            tagging: self.tagging(),
            get_resources: self.get_resources_filter(),
            starter,
        }
    }
}

impl Session {
    /// The empty state of `section` for `key`: its "None in your library" text and what would
    /// fill it. Asked for by the UI each frame, so it follows Resources being added.
    pub fn guide_empty_state(&self, key: &Key, section: GuideSection) -> EmptyState {
        let indexes = self.guides.indexes();
        let has = section.has_needed(&self.library, &indexes);
        let tagged = section.has_needed_tagging(&self.library, &indexes);
        let strongs = match key {
            Key::StrongsNumber(n) => Some(*n),
            _ => None,
        };
        EmptyState {
            message: section.empty_message(has),
            missing: section.missing_resource(
                tagged,
                strongs,
                &self.library,
                self.starter.manifest(),
            ),
        }
    }
}
