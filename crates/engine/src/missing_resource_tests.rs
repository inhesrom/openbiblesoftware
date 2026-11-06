//! Empty Guide sections say how to get the missing Resource (#135): the Resource type and
//! tagging each needs, the Get Resources filter that lists it, and the Starter Resources that
//! would fill it (none once the Library has the kind but nothing covers the Key).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use obs_model::{Key, ResourceType};
use obs_scripture::{Passage, StrongsNumber, Verse};

use crate::get_bibles::{ResourceFilter, type_filter_from_id};
use crate::starter::{StarterItem, StarterManifest};
use crate::*;

fn n(text: &str) -> StrongsNumber {
    text.parse().unwrap()
}

fn item(id: &str, ty: ResourceType, language: &str) -> StarterItem {
    StarterItem {
        resource_id: id.into(),
        revision: 1,
        content_hash: String::new(),
        file_sha256: String::new(),
        bytes: 1,
        title: id.into(),
        abbreviation: id.to_uppercase(),
        resource_type: Some(ty),
        language: language.into(),
        license: String::new(),
    }
}

/// Every Resource `starter/sources.toml` ships, with its type and language.
fn starter_manifest() -> StarterManifest {
    use ResourceType::*;
    let items = [
        ("kjv-strongs", Bible, "en"),
        ("bsb", Bible, "en"),
        ("web", Bible, "en"),
        ("asv", Bible, "en"),
        ("openbible-xrefs", CrossReferenceSet, "en"),
        ("tipnr", PeopleAndPlaces, "en"),
        ("tbesh", Lexicon, "en"),
        ("tbesg", Lexicon, "en"),
        ("strongs-hebrew", Lexicon, "en"),
        ("strongs-greek", Lexicon, "en"),
        ("bdb", Lexicon, "en"),
        ("abbott-smith", Lexicon, "en"),
        ("tahot", Bible, "hbo"),
        ("tagnt", Bible, "grc"),
        ("mhc", Commentary, "en"),
        ("jfb", Commentary, "en"),
        ("easton", Dictionary, "en"),
        ("smith", Dictionary, "en"),
    ];
    StarterManifest {
        format: starter::MANIFEST_FORMAT,
        resources: items.iter().map(|(i, t, l)| item(i, *t, l)).collect(),
    }
}

fn offered(section: GuideSection, strongs: Option<&str>, library: &Library) -> StarterOffer {
    section
        .missing_resource(false, strongs.map(n), library, Some(&starter_manifest()))
        .starter
}

fn ids(list: &[&str]) -> StarterOffer {
    StarterOffer::Resources(list.iter().map(|s| (*s).to_string()).collect())
}

#[test]
fn each_section_names_its_resource_type_tagging_and_get_resources_filter() {
    use GuideSection::*;
    let cases = [
        (
            Commentaries,
            ResourceType::Commentary,
            Tagging::None,
            Some("Get Commentaries…"),
        ),
        (
            CrossReferences,
            ResourceType::CrossReferenceSet,
            Tagging::None,
            None,
        ),
        (
            KeyWords,
            ResourceType::Bible,
            Tagging::StrongsNumbers,
            Some("Get Bibles…"),
        ),
        (
            PeopleAndPlaces,
            ResourceType::PeopleAndPlaces,
            Tagging::None,
            None,
        ),
        (
            LexiconEntries,
            ResourceType::Lexicon,
            Tagging::None,
            Some("Get Lexicons…"),
        ),
        (
            TranslatedAs,
            ResourceType::Bible,
            Tagging::StrongsNumbers,
            Some("Get Bibles…"),
        ),
        (
            Occurrences,
            ResourceType::Bible,
            Tagging::StrongsNumbers,
            Some("Get Bibles…"),
        ),
        (
            Forms,
            ResourceType::Bible,
            Tagging::Morphology,
            Some("Get Hebrew & Greek texts…"),
        ),
    ];
    assert_eq!(cases.len(), GuideSection::ALL.len());
    for (section, ty, tagging, label) in cases {
        let m = section.missing_resource(false, None, &Library::new(), None);
        assert_eq!(m.resource_type, ty, "{section:?}");
        assert_eq!(m.tagging, tagging, "{section:?}");
        assert_eq!(m.get_label().as_deref(), label, "{section:?}");
        // The Get button's filter goes through `library.get_resources` and back unchanged.
        if let Some(filter) = m.get_resources {
            let id = filter.id().expect("an id Get Resources reads");
            assert_eq!(type_filter_from_id(id), Ok(filter), "{section:?}");
        }
    }
    assert_eq!(
        GuideSection::Forms.get_resources_filter(),
        Some(ResourceFilter::HebrewAndGreek)
    );
    assert_eq!(
        ResourceFilter::Type(ResourceType::CrossReferenceSet).id(),
        None,
        "SWORD repositories offer no Cross-reference sets"
    );
}

#[test]
fn an_empty_library_is_offered_the_starter_resources_that_fill_each_section() {
    use GuideSection::*;
    let empty = Library::new();
    assert_eq!(offered(Commentaries, None, &empty), ids(&["mhc", "jfb"]));
    assert_eq!(
        offered(CrossReferences, None, &empty),
        ids(&["openbible-xrefs"])
    );
    assert_eq!(
        offered(KeyWords, None, &empty),
        ids(&["kjv-strongs", "tahot", "tagnt"])
    );
    assert_eq!(offered(PeopleAndPlaces, None, &empty), ids(&["tipnr"]));
    // G26: every Starter Lexicon (their manifests do not say which numbers they cover).
    assert_eq!(
        offered(LexiconEntries, Some("G26"), &empty),
        ids(&[
            "tbesh",
            "tbesg",
            "strongs-hebrew",
            "strongs-greek",
            "bdb",
            "abbott-smith"
        ])
    );
    assert_eq!(
        offered(TranslatedAs, Some("G26"), &empty),
        ids(&["kjv-strongs"])
    );
    // Occurrences and Forms: the original-language Bible of the number's language.
    assert_eq!(
        offered(Occurrences, Some("G26"), &empty),
        ids(&["kjv-strongs", "tagnt"])
    );
    assert_eq!(offered(Forms, Some("G26"), &empty), ids(&["tagnt"]));
    assert_eq!(offered(Forms, Some("H1254A"), &empty), ids(&["tahot"]));
}

#[test]
fn starter_resources_already_in_the_library_are_not_offered_again() {
    let t = std::env::temp_dir().join(format!("obs-missing-in-library-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&t);
    std::fs::create_dir_all(&t).unwrap();
    let mut library = Library::new();
    for r in testing::starter_set() {
        let path = t.join(format!("{}.obr", r.manifest.resource_id));
        obs_store::write_obr(&r, &path).unwrap();
        library.add_file(&path).unwrap();
    }
    // starter_set has the KJV with Strong's: Translated as has nothing left to offer.
    assert_eq!(
        offered(GuideSection::TranslatedAs, Some("G26"), &library),
        StarterOffer::None
    );
    assert_eq!(
        offered(GuideSection::Occurrences, Some("G26"), &library),
        ids(&["tagnt"])
    );
    assert_eq!(
        offered(GuideSection::LexiconEntries, Some("G26"), &library),
        ids(&["strongs-greek", "abbott-smith"])
    );
    drop(library);
    let _ = std::fs::remove_dir_all(&t);
}

#[test]
fn no_entry_for_the_number_offers_get_lexicons_only_and_no_manifest_still_offers_the_starter() {
    // The Library has a Lexicon, but none has this entry: Get Lexicons, not the Starter.
    let has = GuideSection::LexiconEntries.missing_resource(
        true,
        Some(n("G26")),
        &Library::new(),
        Some(&starter_manifest()),
    );
    assert_eq!(has.starter, StarterOffer::None);
    assert!(!has.offers_starter());
    assert_eq!(has.get_label().as_deref(), Some("Get Lexicons…"));
    // No bundle and the released list not fetched: the Starter library Tab will fetch it.
    let unlisted =
        GuideSection::LexiconEntries.missing_resource(false, Some(n("G26")), &Library::new(), None);
    assert_eq!(unlisted.starter, StarterOffer::Unlisted);
    assert!(unlisted.offers_starter());
}

#[test]
fn the_session_empty_state_follows_the_library_and_the_starter_manifest() {
    let root = std::env::temp_dir().join(format!("obs-missing-session-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let bundle = root.join("starter");
    testing::write_starter_bundle(&bundle, &testing::starter_set());
    let mut s = Session::new(JobMode::Inline);
    let g26 = Key::StrongsNumber(n("G26"));
    let empty = s.guide_empty_state(&g26, GuideSection::LexiconEntries);
    assert_eq!(
        empty.message,
        "None in your library. Add a Lexicon to read its entry for this word."
    );
    assert_eq!(empty.missing.starter, StarterOffer::Unlisted);
    s.starter.set_bundle(&bundle).unwrap();
    let listed = s.guide_empty_state(&g26, GuideSection::LexiconEntries);
    assert_eq!(
        listed.missing.starter,
        ids(&["tbesh", "tbesg", "strongs-hebrew", "bdb"])
    );
    // A Lexicon without G26 joins the Library: the state turns into "no entry", Get only.
    let lexicon = testing::lexicon("only-h", "OH", &[("H1", "אָב", "father")]);
    let path = root.join("only-h.obr");
    obs_store::write_obr(&lexicon, &path).unwrap();
    s.library.add_file(&path).unwrap();
    let no_entry = s.guide_empty_state(&g26, GuideSection::LexiconEntries);
    assert_eq!(
        no_entry.message,
        "None in your library: no Lexicon has an entry for this Strong's number."
    );
    assert_eq!(no_entry.missing.starter, StarterOffer::None);
    assert_eq!(
        no_entry.missing.get_label().as_deref(),
        Some("Get Lexicons…")
    );
    // A Passage Guide section reads the Passage Key the same way.
    let passage = Key::Passage(Passage::verse(Verse::new(obs_scripture::Book::John, 3, 16)));
    let xrefs = s.guide_empty_state(&passage, GuideSection::CrossReferences);
    assert!(xrefs.message.contains("Add a Cross-reference set"));
    assert_eq!(xrefs.missing.get_resources, None);
    drop(s);
    let _ = std::fs::remove_dir_all(&root);
}
