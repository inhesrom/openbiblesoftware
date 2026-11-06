//! Collections (#153): rule-based and by-hand sets of Resources kept in the User database, a
//! newly downloaded Commentary joining the rule-based Collection "type:commentary", and that
//! Collection as a Library Search scope, a Passage Guide filter and a Library filter.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::Arc;

use obs_model::{Key, Manifest, ResourceType};
use obs_scripture::{Book, Passage, Verse};
use serde_json::json;

use crate::collections::{Collection, CollectionRule, parse_rules, rules_text};
use crate::testing::MockRepos;
use crate::testing::bibles::{kjv, write_library};
use crate::testing::study;
use crate::*;

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("obs-collections-{name}-{}", std::process::id()))
}

/// A Session with a User database, a library of the KJV, two Commentaries (TCC, TC2), a
/// Lexicon (TLX) and a Dictionary (TBD) with their Search indexes, and mock SWORD
/// repositories offering the Commentary `ObsNotes`.
fn fixture(name: &str) -> (Session, PathBuf) {
    let root = temp(name);
    let _ = std::fs::remove_dir_all(&root);
    write_library(
        &root.join("library"),
        &[
            kjv(),
            study::tcc(),
            study::tc2(),
            study::tlx(),
            study::tbd(),
        ],
    );
    let s = open(&root);
    (s, root)
}

/// (Re)open the Session over `root`, as at a launch.
fn open(root: &std::path::Path) -> Session {
    let repos = Arc::new(MockRepos::new(&root.join("repos")));
    let mut s = Session::new(JobMode::Inline);
    s.set_fetch(repos);
    s.get_bibles
        .set_cache_path(root.join("cache/sword-catalog.json"));
    s.get_bibles.set_sword_dir(root.join("sword"));
    assert!(s.library.scan(&root.join("library")).is_empty());
    s.library.set_preferred_order(
        ["kjv", "tcc", "tc2", "tlx", "tbd"]
            .map(String::from)
            .to_vec(),
    );
    let (store, _) = obs_store::UserStore::open(&root.join("user.db")).unwrap();
    s.attach_user(Box::new(store));
    s.set_search_cache_dir(root.join("search-cache"));
    s.refresh_search_indexes();
    settle(&mut s);
    s
}

fn settle(s: &mut Session) {
    for _ in 0..1000 {
        s.jobs().pump(16);
        let _ = s.poll_imports();
        if !s.imports.is_running()
            && !s.search.is_building()
            && !s.bible_search_running()
            && !s.bible_search.is_reading_ahead()
            && !s.get_bibles.is_refreshing()
            && !s.guides.is_busy()
        {
            return;
        }
    }
    panic!("jobs never settled");
}

/// Download a SWORD module from the mock repositories, import it and index it.
fn download(s: &mut Session, module: &str) -> String {
    s.run("getbibles.refresh", &json!({})).unwrap();
    settle(s);
    let out = s
        .run("getbibles.download", &json!({ "module": module }))
        .unwrap();
    let id = out["id"].as_str().unwrap().to_string();
    settle(s);
    let record = s.imports.report(&id).unwrap();
    assert_eq!(record.phase, ImportPhase::Complete, "{:?}", record.error);
    s.refresh_search_indexes();
    settle(s);
    id
}

fn members(s: &mut Session, name: &str) -> Vec<String> {
    let list = s.run("collection.list", &json!({})).unwrap();
    let c = list
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("no Collection {name}: {list}"));
    c["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m.as_str().unwrap().to_string())
        .collect()
}

fn cleanup(s: Session, root: PathBuf) {
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn rules_parse_from_text_and_print_back() {
    let rules = parse_rules(r#"type:commentary lang:en tag:older licence:cc0 title:"Test Covenant""#)
        .unwrap();
    assert_eq!(
        rules,
        [
            CollectionRule::ResourceType(ResourceType::Commentary),
            CollectionRule::Language("en".into()),
            CollectionRule::Tag("older".into()),
            CollectionRule::Licence("cc0".into()),
            CollectionRule::TitleContains("Test Covenant".into()),
        ]
    );
    assert_eq!(
        rules_text(&rules),
        r#"type:commentary language:en tag:older licence:cc0 title:"Test Covenant""#
    );
    assert_eq!(
        parse_rules("type:Commentaries").unwrap(),
        [CollectionRule::ResourceType(ResourceType::Commentary)]
    );
    assert!(parse_rules("").unwrap().is_empty());
    for bad in ["type:poems", "colour:red", "commentary", "title:\"open", "tag:"] {
        assert!(parse_rules(bad).is_err(), "{bad}");
    }
}

#[test]
fn rules_of_one_kind_match_any_and_kinds_must_all_match() {
    let mut greek_lexicon = Manifest::new(
        "abbott-smith",
        "Abbott-Smith Manual Greek Lexicon",
        "AS",
        ResourceType::Lexicon,
        "grc",
    );
    greek_lexicon.license = "CC-BY-SA-4.0".into();
    let english_dictionary =
        Manifest::new("easton", "Easton's", "EBD", ResourceType::Dictionary, "en");
    let c = |rules: &str| Collection {
        name: "c".into(),
        rules: parse_rules(rules).unwrap(),
        resources: Vec::new(),
    };
    let reference = c("type:lexicon type:dictionary");
    assert!(reference.includes(&greek_lexicon, &[]));
    assert!(reference.includes(&english_dictionary, &[]));
    let greek = c("type:lexicon type:dictionary language:greek");
    assert!(greek.includes(&greek_lexicon, &[]));
    assert!(!greek.includes(&english_dictionary, &[]));
    assert!(c("language:grc").includes(&greek_lexicon, &[]));
    assert!(c("licence:cc-by").includes(&greek_lexicon, &[]));
    assert!(!c("licence:cc-by").includes(&english_dictionary, &[]));
    assert!(c("title:greek").includes(&greek_lexicon, &[]));
    assert!(c("title:ebd").includes(&english_dictionary, &[]), "abbreviation");
    let tags = ["Older".to_string()];
    assert!(c("tag:older").includes(&english_dictionary, &tags));
    assert!(!c("tag:older").includes(&greek_lexicon, &[]));
    // No rules: only what was chosen by hand.
    let by_hand = Collection {
        name: "by hand".into(),
        rules: Vec::new(),
        resources: vec!["easton".into()],
    };
    assert!(by_hand.includes(&english_dictionary, &[]));
    assert!(!by_hand.includes(&greek_lexicon, &[]));
}

#[test]
fn the_rule_based_collection_type_commentary_takes_in_a_newly_downloaded_commentary() {
    let (mut s, root) = fixture("download");
    let saved = s
        .run(
            "collection.save",
            &json!({ "name": "Commentaries", "rules": "type = commentary" }),
        )
        .unwrap_err();
    assert!(saved.to_string().contains("not a rule"), "{saved}");
    let saved = s
        .run(
            "collection.save",
            &json!({ "name": "Commentaries", "rules": "type:commentary" }),
        )
        .unwrap();
    assert_eq!(saved["rules"], "type:commentary");
    assert_eq!(saved["members"], json!(["tcc", "tc2"]));

    let id = download(&mut s, "ObsNotes");
    assert_eq!(
        s.library.get(&id).unwrap().manifest.resource_type,
        ResourceType::Commentary
    );
    // Nothing was done to the Collection: its rule takes the new Commentary in.
    assert_eq!(members(&mut s, "Commentaries"), ["tcc", "tc2", id.as_str()]);
    assert!(s.library.in_collection("commentaries", &id), "any case");

    // Kept in the User database: after a restart the rule still covers it.
    drop(s);
    let mut s = open(&root);
    assert_eq!(members(&mut s, "Commentaries"), ["tcc", "tc2", id.as_str()]);
    cleanup(s, root);
}

/// The groups of a Library search with `resources`, and how many Resources it searched.
fn library_search(s: &mut Session, text: &str, resources: &str) -> (Vec<String>, usize) {
    let out = s
        .run(
            "search.run",
            &json!({ "text": text, "mode": "library", "resources": resources }),
        )
        .unwrap();
    assert!(out["error"].is_null(), "{out}");
    settle(s);
    let tab = TabId(out["tab"].as_u64().unwrap());
    let r = s.search_tab(tab).unwrap().library_results.clone().unwrap();
    (
        r.groups.iter().map(|g| g.id.clone()).collect(),
        r.searched,
    )
}

#[test]
fn searching_with_a_collection_as_the_scope_searches_only_its_resources() {
    let (mut s, root) = fixture("search");
    s.run(
        "collection.save",
        &json!({ "name": "Commentaries", "rules": "type:commentary" }),
    )
    .unwrap();
    let notes = download(&mut s, "ObsNotes");
    // Every type: the Lexicon and the Dictionary mention the covenant too.
    let (groups, searched) = library_search(&mut s, "covenant", "commentary,lexicon,dictionary");
    assert_eq!(groups, ["tcc", "tc2", "tlx", "tbd"]);
    assert_eq!(searched, 5);
    // The Collection: only its three Commentaries, the downloaded one among them.
    let (groups, searched) = library_search(&mut s, "covenant", "collection:Commentaries");
    assert_eq!(groups, ["tcc", "tc2"]);
    assert_eq!(searched, 3);
    let (groups, _) = library_search(&mut s, "word", "collection:Commentaries");
    assert_eq!(groups, ["tcc", notes.as_str()]);
    let err = s
        .run(
            "search.run",
            &json!({ "text": "covenant", "mode": "library", "resources": "collection:Nope" }),
        )
        .unwrap_err();
    assert!(err.to_string().contains("no Collection named “Nope”"), "{err}");
    cleanup(s, root);
}

#[test]
fn the_search_tools_collection_choice_narrows_and_leaves_resources_out_one_by_one() {
    let (mut s, root) = fixture("scope");
    s.run(
        "collection.save",
        &json!({ "name": "Notes", "rules": "type:commentary" }),
    )
    .unwrap();
    s.run(
        "collection.save",
        &json!({ "name": "Words", "resources": "tlx,kjv" }),
    )
    .unwrap();
    let out = s
        .run("search.run", &json!({ "text": "covenant", "mode": "library" }))
        .unwrap();
    settle(&mut s);
    let tab = out["tab"].as_u64().unwrap();
    // Chosen while every type is searched: just the Collection.
    let out = s
        .run("search.collection", &json!({ "tab": tab, "name": "notes" }))
        .unwrap();
    assert_eq!(out["types"], json!([]));
    assert_eq!(out["collections"], json!(["Notes"]));
    assert_eq!(out["resources"], json!(["tcc", "tc2"]));
    // Another Collection adds to it; its Bible is Bible mode's.
    let out = s
        .run("search.collection", &json!({ "tab": tab, "name": "Words" }))
        .unwrap();
    assert_eq!(out["resources"], json!(["tcc", "tc2", "tlx"]));
    // Leaving one Resource of a Collection out keeps its others, one by one.
    let out = s
        .run("search.resource", &json!({ "tab": tab, "id": "tc2" }))
        .unwrap();
    assert_eq!(out["collections"], json!(["Words"]));
    assert_eq!(out["resources"], json!(["tcc", "tlx"]));
    settle(&mut s);
    // The Search Tab follows a rename and forgets a deleted Collection.
    s.run(
        "collection.save",
        &json!({ "name": "Word books", "previous": "Words", "resources": "tlx" }),
    )
    .unwrap();
    let st = s.search_tab(TabId(tab)).unwrap();
    assert_eq!(st.library.collections, ["Word books"]);
    s.run("collection.delete", &json!({ "name": "Word books" }))
        .unwrap();
    let st = s.search_tab(TabId(tab)).unwrap();
    assert!(st.library.collections.is_empty());
    assert_eq!(st.library.resources, ["tcc"]);
    cleanup(s, root);
}

fn commentary_ids(s: &mut Session, key: &Key) -> Vec<String> {
    for _ in 0..100 {
        match s.guide_section(key, GuideSection::Commentaries) {
            SectionStatus::Ready(d) => {
                let SectionData::Commentaries(rows) = &*d else {
                    panic!("{d:?}")
                };
                return rows.iter().map(|r| r.resource_id.clone()).collect();
            }
            SectionStatus::Failed(e) => panic!("{e}"),
            _ => {
                s.jobs().pump(4);
                let _ = s.poll_imports();
            }
        }
    }
    panic!("Commentaries never finished");
}

#[test]
fn the_passage_guides_commentaries_filter_to_a_collection_and_it_is_remembered() {
    let (mut s, root) = fixture("guide");
    s.run(
        "collection.save",
        &json!({ "name": "Second", "rules": "title:second" }),
    )
    .unwrap();
    let genesis = Key::Passage(Passage::new(Verse::new(Book::Genesis, 15, 1), Verse::new(Book::Genesis, 17, 2)).unwrap());
    assert_eq!(commentary_ids(&mut s, &genesis), ["tcc", "tc2"]);
    let out = s
        .run(
            "guide.filter_section",
            &json!({ "guide": "passage_guide", "section": "commentaries", "collection": "second" }),
        )
        .unwrap();
    assert_eq!(out[0]["collection"], "Second");
    assert_eq!(commentary_ids(&mut s, &genesis), ["tc2"]);
    assert_eq!(
        s.guide_filter(Guide::PassageGuide, GuideSection::Commentaries)
            .as_deref(),
        Some("Second")
    );
    // Only Commentaries filter; an unknown Collection is refused.
    assert!(
        s.run(
            "guide.filter_section",
            &json!({ "guide": "passage_guide", "section": "cross_references", "collection": "Second" }),
        )
        .is_err()
    );
    assert!(
        s.run(
            "guide.filter_section",
            &json!({ "guide": "passage_guide", "section": "commentaries", "collection": "Nope" }),
        )
        .is_err()
    );
    // Remembered with the Guide arrangement.
    drop(s);
    let mut s = open(&root);
    assert_eq!(commentary_ids(&mut s, &genesis), ["tc2"]);
    // Deleting the Collection lifts the filter.
    s.run("collection.delete", &json!({ "name": "Second" }))
        .unwrap();
    assert_eq!(
        s.guide_filter(Guide::PassageGuide, GuideSection::Commentaries),
        None
    );
    assert_eq!(commentary_ids(&mut s, &genesis), ["tcc", "tc2"]);
    s.run(
        "collection.save",
        &json!({ "name": "Second", "rules": "title:second" }),
    )
    .unwrap();
    assert_eq!(commentary_ids(&mut s, &genesis), ["tcc", "tc2"], "not filtered again");
    cleanup(s, root);
}

#[test]
fn collections_by_hand_and_by_tag_filter_the_library_and_are_kept() {
    let (mut s, root) = fixture("by-hand");
    // By hand: Resources added and taken out one at a time.
    s.run("collection.save", &json!({ "name": "Study set" }))
        .unwrap();
    s.run(
        "collection.toggle",
        &json!({ "name": "Study set", "id": "tbd" }),
    )
    .unwrap();
    let out = s
        .run(
            "collection.toggle",
            &json!({ "name": "Study set", "id": "kjv" }),
        )
        .unwrap();
    assert_eq!(out["members"], json!(["kjv", "tbd"]));
    let out = s
        .run(
            "collection.toggle",
            &json!({ "name": "Study set", "id": "tbd" }),
        )
        .unwrap();
    assert_eq!(out["members"], json!(["kjv"]));
    // By Resource tag.
    s.run("resource.tags", &json!({ "id": "tc2", "tags": "older, Reformed, older" }))
        .unwrap();
    assert_eq!(s.library.tags_of("tc2"), ["older", "Reformed"]);
    s.run(
        "collection.save",
        &json!({ "name": "Older", "rules": "tag:older" }),
    )
    .unwrap();
    assert_eq!(members(&mut s, "Older"), ["tc2"]);
    // As a Library filter, with the header text and language chips.
    let filter = LibraryFilter {
        collection: Some("Study set".into()),
        ..LibraryFilter::default()
    };
    let shown: Vec<&str> = s
        .library
        .entries()
        .iter()
        .filter(|e| filter.matches_in(&s.library, &e.manifest))
        .map(|e| e.manifest.resource_id.as_str())
        .collect();
    assert_eq!(shown, ["kjv"]);
    // Names are unique (any case) and have no commas. Saving under a name edits that
    // Collection.
    for bad in [
        json!({ "name": "older", "previous": "Study set" }),
        json!({ "name": "a, b" }),
        json!({ "name": "  " }),
        json!({ "name": "x", "resources": "nope" }),
    ] {
        assert!(s.run("collection.save", &bad).is_err(), "{bad}");
    }
    // Kept in the User database.
    drop(s);
    let mut s = open(&root);
    assert_eq!(members(&mut s, "Study set"), ["kjv"]);
    assert_eq!(members(&mut s, "Older"), ["tc2"]);
    assert_eq!(s.library.tags_of("tc2"), ["older", "Reformed"]);
    s.run("resource.tags", &json!({ "id": "tc2", "tags": "" }))
        .unwrap();
    assert!(members(&mut s, "Older").is_empty());
    cleanup(s, root);
}
