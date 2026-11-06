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
    let rules =
        parse_rules(r#"type:commentary lang:en tag:older licence:cc0 title:"Test Covenant""#)
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
    for bad in [
        "type:poems",
        "colour:red",
        "commentary",
        "title:\"open",
        "tag:",
    ] {
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
    assert!(
        c("title:ebd").includes(&english_dictionary, &[]),
        "abbreviation"
    );
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
    (r.groups.iter().map(|g| g.id.clone()).collect(), r.searched)
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
    assert!(
        err.to_string().contains("no Collection named “Nope”"),
        "{err}"
    );
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
        .run(
            "search.run",
            &json!({ "text": "covenant", "mode": "library" }),
        )
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
    let genesis = Key::Passage(
        Passage::new(
            Verse::new(Book::Genesis, 15, 1),
            Verse::new(Book::Genesis, 17, 2),
        )
        .unwrap(),
    );
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
    assert_eq!(
        commentary_ids(&mut s, &genesis),
        ["tcc", "tc2"],
        "not filtered again"
    );
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
    s.run(
        "resource.tags",
        &json!({ "id": "tc2", "tags": "older, Reformed, older" }),
    )
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

/// The Search Tab's Library scope as `search.resource` reports it, and the groups of its
/// search once run.
fn scope_and_groups(
    s: &mut Session,
    tab: u64,
    out: &serde_json::Value,
) -> (Vec<String>, Vec<String>) {
    settle(s);
    let ids = out["resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let groups = s
        .search_tab(TabId(tab))
        .unwrap()
        .library_results
        .as_ref()
        .map(|r| r.groups.iter().map(|g| g.id.clone()).collect())
        .unwrap_or_default();
    (ids, groups)
}

#[test]
fn leaving_out_a_resource_drops_every_part_of_the_scope_that_covered_it() {
    let (mut s, root) = fixture("overlap");
    s.run(
        "collection.save",
        &json!({ "name": "Notes", "rules": "type:commentary" }),
    )
    .unwrap();
    s.run(
        "collection.save",
        &json!({ "name": "Words", "resources": "tlx,tbd" }),
    )
    .unwrap();
    // A Resource type and a Collection both cover TCC.
    let out = s
        .run(
            "search.run",
            &json!({ "text": "covenant", "mode": "library", "resources": "collection:Notes" }),
        )
        .unwrap();
    let tab = out["tab"].as_u64().unwrap();
    s.run(
        "search.resource_type",
        &json!({ "tab": tab, "value": "commentary" }),
    )
    .unwrap();
    let out = s
        .run("search.resource", &json!({ "tab": tab, "id": "tcc" }))
        .unwrap();
    assert_eq!(out["types"], json!([]));
    assert_eq!(out["collections"], json!([]));
    let (ids, groups) = scope_and_groups(&mut s, tab, &out);
    assert_eq!(ids, ["tc2"]);
    assert_eq!(groups, ["tc2"]);
    assert!(
        !s.search_tab(TabId(tab))
            .unwrap()
            .library
            .includes(&s.library, "tcc")
    );

    // A Resource chosen one by one and a Collection both cover TLX; the unrelated TCC stays.
    let out = s
        .run(
            "search.run",
            &json!({ "tab": tab, "text": "covenant", "mode": "library",
                     "resources": "tcc,tlx,collection:Words" }),
        )
        .unwrap();
    assert!(out["error"].is_null(), "{out}");
    let out = s
        .run("search.resource", &json!({ "tab": tab, "id": "tlx" }))
        .unwrap();
    assert_eq!(out["collections"], json!([]));
    let (ids, groups) = scope_and_groups(&mut s, tab, &out);
    assert_eq!(ids, ["tcc", "tbd"]);
    assert_eq!(groups, ["tcc", "tbd"]);
    // Choosing it again adds it back on its own.
    let out = s
        .run("search.resource", &json!({ "tab": tab, "id": "tlx" }))
        .unwrap();
    let (ids, _) = scope_and_groups(&mut s, tab, &out);
    assert_eq!(ids, ["tcc", "tlx", "tbd"]);
    cleanup(s, root);
}

#[test]
fn a_collection_keeps_a_removed_resource_and_can_still_be_edited_after_a_restart() {
    let (mut s, root) = fixture("removed");
    s.run(
        "collection.save",
        &json!({ "name": "Pair", "resources": "tcc,tc2" }),
    )
    .unwrap();
    s.run("library.remove", &json!({ "id": "tcc", "confirm": true }))
        .unwrap();
    settle(&mut s);
    assert!(s.library.get("tcc").is_none());
    assert_eq!(members(&mut s, "Pair"), ["tc2"]);
    // The editor sends back what was chosen by hand, the removed Resource with it.
    let out = s
        .run(
            "collection.save",
            &json!({ "name": "Both", "previous": "Pair", "resources": "tcc,tc2",
                     "rules": "type:lexicon" }),
        )
        .unwrap();
    assert_eq!(out["resources"], json!(["tcc", "tc2"]));
    assert_eq!(out["members"], json!(["tc2", "tlx"]));
    // A Resource newly chosen must still be in the library.
    let err = s
        .run(
            "collection.save",
            &json!({ "name": "Both", "resources": "tcc,tc2,nope" }),
        )
        .unwrap_err();
    assert!(
        err.to_string().contains("nope is not in the library"),
        "{err}"
    );
    let err = s
        .run(
            "collection.save",
            &json!({ "name": "Other", "resources": "tcc" }),
        )
        .unwrap_err();
    assert!(
        err.to_string().contains("tcc is not in the library"),
        "{err}"
    );

    drop(s);
    let mut s = open(&root);
    assert_eq!(
        s.library.collection("Both").unwrap().resources,
        ["tcc", "tc2"]
    );
    s.run(
        "collection.save",
        &json!({ "name": "Both", "resources": "tcc,tc2" }),
    )
    .unwrap();
    // Taken out by hand, though it is not in the library.
    let out = s
        .run("collection.toggle", &json!({ "name": "Both", "id": "tcc" }))
        .unwrap();
    assert_eq!(out["resources"], json!(["tc2"]));
    assert!(
        s.run("collection.toggle", &json!({ "name": "Both", "id": "tcc" }))
            .is_err(),
        "not added back while it is not in the library"
    );
    cleanup(s, root);
}

/// The groups the Search Tab shows, without letting its jobs run.
fn shown_groups(s: &Session, tab: u64) -> Option<Vec<String>> {
    s.search_tab(TabId(tab))
        .unwrap()
        .library_results
        .as_ref()
        .map(|r| r.groups.iter().map(|g| g.id.clone()).collect())
}

#[test]
fn changing_or_deleting_a_searched_collection_runs_the_search_again() {
    let (mut s, root) = fixture("rerun");
    s.run(
        "collection.save",
        &json!({ "name": "Pair", "resources": "tcc,tc2" }),
    )
    .unwrap();
    let out = s
        .run(
            "search.run",
            &json!({ "text": "covenant", "mode": "library", "resources": "collection:Pair" }),
        )
        .unwrap();
    let tab = out["tab"].as_u64().unwrap();
    settle(&mut s);
    assert_eq!(shown_groups(&s, tab).unwrap(), ["tcc", "tc2"]);

    // Taking TC2 out runs the search again over TCC alone.
    s.run("collection.toggle", &json!({ "name": "Pair", "id": "tc2" }))
        .unwrap();
    settle(&mut s);
    assert_eq!(shown_groups(&s, tab).unwrap(), ["tcc"]);

    // A change while the search is still running: its results are dropped for the new run's.
    s.run("collection.toggle", &json!({ "name": "Pair", "id": "tc2" }))
        .unwrap();
    assert!(s.search_tab(TabId(tab)).unwrap().running);
    s.run(
        "collection.save",
        &json!({ "name": "Pair", "resources": "tc2" }),
    )
    .unwrap();
    settle(&mut s);
    assert_eq!(shown_groups(&s, tab).unwrap(), ["tc2"]);

    // A rule on a Resource tag: tagging TCC brings it in.
    s.run(
        "collection.save",
        &json!({ "name": "Pair", "rules": "tag:mine" }),
    )
    .unwrap();
    settle(&mut s);
    assert_eq!(
        shown_groups(&s, tab),
        None,
        "nothing in it yet: nothing searched"
    );
    s.run("resource.tags", &json!({ "id": "tcc", "tags": "mine" }))
        .unwrap();
    settle(&mut s);
    assert_eq!(shown_groups(&s, tab).unwrap(), ["tcc"]);

    // Deleting the Collection leaves nothing to search: no results, and the Resources error.
    s.run("collection.delete", &json!({ "name": "Pair" }))
        .unwrap();
    settle(&mut s);
    let st = s.search_tab(TabId(tab)).unwrap();
    assert!(st.library_results.is_none());
    assert_eq!(
        st.error.as_ref().map(|e| e.field),
        Some(ErrorField::Resources)
    );
    cleanup(s, root);
}

#[test]
fn a_search_brought_back_with_a_deleted_or_renamed_collection_can_stop_searching_it() {
    let (mut s, root) = fixture("stale");
    s.run(
        "collection.save",
        &json!({ "name": "Pair", "resources": "tcc,tc2" }),
    )
    .unwrap();
    let out = s
        .run(
            "search.run",
            &json!({ "text": "covenant", "mode": "library", "resources": "collection:Pair" }),
        )
        .unwrap();
    let tab = out["tab"].as_u64().unwrap();
    settle(&mut s);
    s.record_history();
    let favorite = s.run("favorite.add", &json!({ "tab": tab })).unwrap()["id"].clone();
    let visit = s
        .history()
        .iter()
        .find(|v| v.label.contains("covenant"))
        .unwrap()
        .id;
    let collections = |s: &Session, tab: u64| {
        s.search_tab(TabId(tab))
            .unwrap()
            .library
            .collections
            .clone()
    };

    // Renamed: the Favorite still names "Pair", and its chip still comes off.
    s.run(
        "collection.save",
        &json!({ "name": "Duo", "resources": "tcc,tc2", "previous": "Pair" }),
    )
    .unwrap();
    settle(&mut s);
    assert_eq!(collections(&s, tab), ["Duo"]);
    let t = s.run("favorite.open", &json!({ "id": favorite })).unwrap()["tab"]
        .as_u64()
        .unwrap();
    settle(&mut s);
    assert_eq!(collections(&s, t), ["Pair"]);
    let out = s
        .run("search.collection", &json!({ "tab": t, "name": "Pair" }))
        .unwrap();
    assert_eq!(out["collections"], json!([]));
    assert!(collections(&s, t).is_empty());

    // Deleted: the History Visit brings "Pair" back, and its chip comes off too.
    s.run("collection.delete", &json!({ "name": "Duo" }))
        .unwrap();
    settle(&mut s);
    let t = s.run("history.open", &json!({ "id": visit })).unwrap()["tab"]
        .as_u64()
        .unwrap();
    settle(&mut s);
    assert_eq!(collections(&s, t), ["Pair"]);
    s.run("search.collection", &json!({ "tab": t, "name": "pair" }))
        .unwrap();
    assert!(collections(&s, t).is_empty());

    // Choosing a Collection that does not exist (any more) still says so.
    let err = s
        .run("search.collection", &json!({ "tab": t, "name": "Pair" }))
        .unwrap_err();
    assert!(
        err.to_string().contains("no Collection named “Pair”"),
        "{err}"
    );
    cleanup(s, root);
}
