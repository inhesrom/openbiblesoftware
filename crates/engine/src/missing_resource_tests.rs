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
        .missing_resource(
            false,
            strongs.map(|s| n(s).language()),
            library,
            Some(&starter_manifest()),
        )
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
        (Summary, ResourceType::PeopleAndPlaces, Tagging::None, None),
        (
            KeyPassages,
            ResourceType::PeopleAndPlaces,
            Tagging::None,
            None,
        ),
        (
            DictionaryArticles,
            ResourceType::Dictionary,
            Tagging::None,
            Some("Get Dictionaries…"),
        ),
        (Related, ResourceType::PeopleAndPlaces, Tagging::None, None),
        (
            WordByWord,
            ResourceType::Bible,
            Tagging::Morphology,
            Some("Get Hebrew & Greek texts…"),
        ),
        (
            LexiconSummaries,
            ResourceType::Lexicon,
            Tagging::None,
            Some("Get Lexicons…"),
        ),
        (
            GrammaticalNotes,
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
    // The Exegetical Guide (#144): the original-language Bible of the Passage's testament.
    let passage = |section: GuideSection, key: &obs_model::Key| {
        section
            .missing_resource(
                false,
                crate::missing_resource::key_language(key),
                &empty,
                Some(&starter_manifest()),
            )
            .starter
    };
    let john = obs_model::Key::Passage(obs_scripture::Passage::verse(obs_scripture::Verse::new(
        obs_scripture::Book::John,
        3,
        16,
    )));
    let psalm = obs_model::Key::Passage(obs_scripture::Passage::verse(obs_scripture::Verse::new(
        obs_scripture::Book::Psalms,
        3,
        1,
    )));
    assert_eq!(passage(WordByWord, &john), ids(&["tagnt"]));
    assert_eq!(passage(WordByWord, &psalm), ids(&["tahot"]));
    assert_eq!(passage(GrammaticalNotes, &john), ids(&["tagnt"]));
    assert_eq!(offered(Forms, Some("H1254A"), &empty), ids(&["tahot"]));
    // Factbook (#149): TIPNR, and the Dictionaries for their articles (found through
    // TIPNR's names).
    for section in [Summary, KeyPassages, Related] {
        assert_eq!(
            offered(section, None, &empty),
            ids(&["tipnr"]),
            "{section:?}"
        );
    }
    assert_eq!(
        offered(DictionaryArticles, None, &empty),
        ids(&["tipnr", "easton", "smith"])
    );
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
        Some(n("G26").language()),
        &Library::new(),
        Some(&starter_manifest()),
    );
    assert_eq!(has.starter, StarterOffer::None);
    assert!(!has.offers_starter());
    assert_eq!(has.get_label().as_deref(), Some("Get Lexicons…"));
    // No bundle and the released list not fetched: the Starter library Tab will fetch it.
    let unlisted = GuideSection::LexiconEntries.missing_resource(
        false,
        Some(n("G26").language()),
        &Library::new(),
        None,
    );
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

fn settle(s: &mut Session, key: &Key, section: GuideSection) -> std::sync::Arc<SectionData> {
    for _ in 0..50 {
        match s.guide_section(key, section) {
            SectionStatus::Loading => {
                s.jobs().pump(4);
                s.poll_imports();
            }
            SectionStatus::Ready(d) => return d,
            SectionStatus::Failed(e) => panic!("{section:?} failed: {e}"),
        }
    }
    panic!("{section:?} never finished");
}

/// A Bible without Tagged words is a Bible, but it can never fill Key words or Forms: the
/// Starter Bibles that would are still offered. Once a Strong's-tagged Bible is in, an empty
/// section means nothing covers the Key, and only Get Resources is left.
#[test]
fn an_untagged_bible_does_not_hide_the_starter_bibles_from_key_words_and_forms() {
    let root = std::env::temp_dir().join(format!("obs-missing-untagged-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let bundle = root.join("starter");
    let mut set = testing::starter_set();
    set.push(testing::tagnt_john_3_16());
    testing::write_starter_bundle(&bundle, &set);
    let mut s = Session::new(JobMode::Inline);
    s.starter.set_bundle(&bundle).unwrap();
    let add = |s: &mut Session, r: &obs_model::Resource| {
        let path = root.join(format!("{}.obr", r.manifest.resource_id));
        obs_store::write_obr(r, &path).unwrap();
        s.library.add_file(&path).unwrap();
    };
    let john_3_16 = Verse::new(obs_scripture::Book::John, 3, 16);
    let text = [(john_3_16, "For God so loved the world")];
    add(
        &mut s,
        &testing::tagged_bible("plain", "Plain", "PLN", "en", &text),
    );
    let greek = [(john_3_16, "Οὕτως γὰρ ἠγάπησεν ὁ θεὸς τὸν κόσμον")];
    add(
        &mut s,
        &testing::tagged_bible("plain-grc", "Plain Greek", "PLG", "grc", &greek),
    );

    let john = Key::Passage(Passage::verse(john_3_16));
    assert!(settle(&mut s, &john, GuideSection::KeyWords).is_empty());
    let key_words = s.guide_empty_state(&john, GuideSection::KeyWords);
    assert_eq!(
        key_words.message,
        "None in your library: no Bible with Strong's numbers has this Passage."
    );
    assert_eq!(key_words.missing.starter, ids(&["kjv-strongs", "tagnt"]));
    let g26 = Key::StrongsNumber(n("G26"));
    assert!(settle(&mut s, &g26, GuideSection::Forms).is_empty());
    let forms = s.guide_empty_state(&g26, GuideSection::Forms);
    assert_eq!(
        forms.message,
        "None in your library: no Hebrew or Greek Bible with Morphology has this word."
    );
    assert_eq!(forms.missing.starter, ids(&["tagnt"]));

    // TAGNT (John 3:16 only) joins: Romans 1:1 and G26 are simply not in it.
    add(&mut s, &testing::tagnt_john_3_16());
    let romans = Key::Passage(Passage::verse(Verse::new(
        obs_scripture::Book::Romans,
        1,
        1,
    )));
    assert!(settle(&mut s, &romans, GuideSection::KeyWords).is_empty());
    let key_words = s.guide_empty_state(&romans, GuideSection::KeyWords);
    assert_eq!(key_words.missing.starter, StarterOffer::None);
    assert_eq!(
        key_words.missing.get_label().as_deref(),
        Some("Get Bibles…")
    );
    assert!(settle(&mut s, &g26, GuideSection::Forms).is_empty());
    let forms = s.guide_empty_state(&g26, GuideSection::Forms);
    assert_eq!(forms.missing.starter, StarterOffer::None);
    assert_eq!(
        forms.missing.get_label().as_deref(),
        Some("Get Hebrew & Greek texts…")
    );
    drop(s);
    let _ = std::fs::remove_dir_all(&root);
}

/// A Greek Bible with Strong's numbers but no Morphology tags G26, yet Forms stays empty: it
/// still offers TAGNT from the Starter library, as does a Hebrew Bible's Morphology (the
/// wrong half of the Strong's system for G26). Once a Greek Bible with Morphology is in,
/// only Get Resources is left.
#[test]
fn a_strongs_only_greek_bible_does_not_hide_tagnt_from_forms() {
    let root =
        std::env::temp_dir().join(format!("obs-missing-strongs-only-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let bundle = root.join("starter");
    let mut set = testing::starter_set();
    set.push(testing::tagnt_john_3_16());
    testing::write_starter_bundle(&bundle, &set);
    let mut s = Session::new(JobMode::Inline);
    s.starter.set_bundle(&bundle).unwrap();
    let add = |s: &mut Session, r: &obs_model::Resource| {
        let path = root.join(format!("{}.obr", r.manifest.resource_id));
        obs_store::write_obr(r, &path).unwrap();
        s.library.add_file(&path).unwrap();
    };
    let john_15_13 = Verse::new(obs_scripture::Book::John, 15, 13);
    add(
        &mut s,
        &testing::tagged_bible(
            "strongs-grc",
            "Greek with Strong's",
            "GRS",
            "grc",
            &[(
                john_15_13,
                "μείζονα{G3187} ταύτης{G3778} ἀγάπης{G26} οὐδεὶς{G3762} ἔχει{G2192}",
            )],
        ),
    );
    add(&mut s, &testing::hebrew_bara());

    // The Hebrew Bible's Forms (Morphology) are gathered first.
    let h1254 = Key::StrongsNumber(n("H1254"));
    assert!(!settle(&mut s, &h1254, GuideSection::Forms).is_empty());
    let g26 = Key::StrongsNumber(n("G26"));
    let SectionData::Forms(forms) = &*settle(&mut s, &g26, GuideSection::Forms) else {
        panic!("not Forms");
    };
    assert_eq!(forms.bible_id, "strongs-grc");
    assert!(forms.forms.is_empty());
    assert_eq!(forms.without_form, 1);
    let empty = s.guide_empty_state(&g26, GuideSection::Forms);
    assert_eq!(
        empty.message,
        "None in your library: no Hebrew or Greek Bible with Morphology has this word."
    );
    assert_eq!(empty.missing.tagging, Tagging::Morphology);
    assert_eq!(empty.missing.starter, ids(&["tagnt"]));

    // TAGNT (John 3:16 only, with Morphology) joins: G26 is simply not in it.
    add(&mut s, &testing::tagnt_john_3_16());
    assert!(settle(&mut s, &g26, GuideSection::Forms).is_empty());
    let empty = s.guide_empty_state(&g26, GuideSection::Forms);
    assert_eq!(empty.missing.starter, StarterOffer::None);
    assert_eq!(
        empty.missing.get_label().as_deref(),
        Some("Get Hebrew & Greek texts…")
    );

    // A TAGNT with G26 and its Morphology joins: Forms reads it, though the Strong's-only
    // Bible still comes first, and Occurrences counts in the same Bible so its form filter
    // matches.
    add(&mut s, &testing::tagnt_love());
    let SectionData::Forms(forms) = &*settle(&mut s, &g26, GuideSection::Forms) else {
        panic!("not Forms");
    };
    assert_eq!(forms.bible_id, "tagnt-love");
    let labels: Vec<&str> = forms.forms.iter().map(|f| f.label.as_str()).collect();
    assert_eq!(labels.len(), 2, "{labels:?}");
    assert_eq!(forms.without_form, 0);
    let SectionData::Occurrences(occurrences) = &*settle(&mut s, &g26, GuideSection::Occurrences)
    else {
        panic!("not Occurrences");
    };
    assert_eq!(occurrences.bible_id, "tagnt-love");
    let mut occurrence_forms: Vec<&str> = occurrences.forms.iter().map(String::as_str).collect();
    occurrence_forms.sort_unstable();
    let mut labels = labels;
    labels.sort_unstable();
    assert_eq!(occurrence_forms, labels);
    drop(s);
    let _ = std::fs::remove_dir_all(&root);
}

/// Dictionary articles are found through a People & places index: with a Dictionary but no
/// index the section names TIPNR (from the Starter library; the online repositories offer no
/// index), never more Dictionaries, and once TIPNR is in the same Factbook fills.
#[test]
fn a_dictionary_without_tipnr_asks_for_tipnr_for_dictionary_articles() {
    let root = std::env::temp_dir().join(format!("obs-missing-no-index-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let bundle = root.join("starter");
    let mut set = testing::starter_set();
    set.extend([
        testing::tipnr(),
        testing::dictionary("easton"),
        testing::dictionary("smith"),
    ]);
    testing::write_starter_bundle(&bundle, &set);
    let mut s = Session::new(JobMode::Inline);
    s.starter.set_bundle(&bundle).unwrap();
    let add = |s: &mut Session, r: &obs_model::Resource| {
        let path = root.join(format!("{}.obr", r.manifest.resource_id));
        obs_store::write_obr(r, &path).unwrap();
        s.library.add_file(&path).unwrap();
    };
    let abraham = Key::Headword("Abraham@Gen.11.26-1Pe".into());
    let articles = GuideSection::DictionaryArticles;

    // Nothing at all: TIPNR and the Dictionaries, and Get Dictionaries for the latter.
    let empty = s.guide_empty_state(&abraham, articles);
    assert_eq!(
        empty.message,
        "None in your library. Add a People & places index (TIPNR) and a Dictionary \
         (Easton's, Smith's) to read about them."
    );
    assert_eq!(empty.missing.starter, ids(&["tipnr", "easton", "smith"]));
    assert_eq!(
        empty.missing.get_label().as_deref(),
        Some("Get Dictionaries…")
    );

    // Easton's is in, TIPNR is not: only TIPNR would help.
    add(&mut s, &testing::dictionary("easton"));
    assert!(settle(&mut s, &abraham, articles).is_empty());
    let empty = s.guide_empty_state(&abraham, articles);
    assert_eq!(
        empty.message,
        "None in your library. Add a People & places index (TIPNR) to find the Dictionary \
         articles on them."
    );
    assert_eq!(empty.missing.resource_type, ResourceType::PeopleAndPlaces);
    assert_eq!(empty.missing.get_resources, None);
    assert_eq!(empty.missing.starter, ids(&["tipnr"]));

    // TIPNR joins: the section fills.
    add(&mut s, &testing::tipnr());
    let SectionData::DictionaryArticles(found) = &*settle(&mut s, &abraham, articles) else {
        panic!("not Dictionary articles");
    };
    assert!(
        found.iter().any(|a| a.article.abbreviation == "Easton"),
        "{found:?}"
    );
    drop(s);
    let _ = std::fs::remove_dir_all(&root);
}
