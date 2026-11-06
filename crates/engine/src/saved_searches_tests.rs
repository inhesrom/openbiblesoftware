//! Saved searches and the Search history (#179): saving a search under a name and running it
//! after a restart with its mode and Search scope, the last 50 searches kept and re-run, the
//! Command box's suggestions, and a Search scope naming something that is gone. A real `.obr`
//! library and a real User database in a temp folder.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use obs_scripture::Book;
use serde_json::json;

use crate::testing::bibles::{bible, v, write_library};
use crate::testing::{study, tagged_bible};
use crate::*;

/// A library of `grk`, a Strong's-tagged Bible with G26 (ἀγάπη) in two Old Testament Verses
/// (as the Septuagint has it) and three New Testament ones; WEB; and the Commentary TCC.
fn folder(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("obs-saved-search-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let grk = tagged_bible(
        "grk",
        "Greek-tagged test Bible",
        "GRK",
        "en",
        &[
            (
                v(Book::SongOfSongs, 2, 4),
                "and his banner over me was love{G26}.",
            ),
            (
                v(Book::Jeremiah, 31, 3),
                "I have loved thee with an everlasting love{G26}.",
            ),
            (
                v(Book::John, 15, 13),
                "Greater love{G26} hath no man than this.",
            ),
            (
                v(Book::Romans, 5, 5),
                "because the love{G26} of God is shed abroad in our hearts.",
            ),
            (v(Book::FirstJohn, 4, 8), "for God is love{G26}."),
        ],
    );
    let web = bible(
        "web",
        "WEB",
        &[
            (v(Book::John, 15, 13), "Greater love has no one than this."),
            (v(Book::FirstJohn, 4, 8), "for God is love."),
        ],
        None,
        &[],
    );
    write_library(&root.join("library"), &[grk, web, study::tcc()]);
    root
}

/// Launch as the app does: scan the library, attach the User database, build the Search
/// indexes.
fn launch(root: &Path) -> Session {
    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(&root.join("library")).is_empty());
    let (store, _) = obs_store::UserStore::open(&root.join("user.db")).unwrap();
    s.attach_user(Box::new(store));
    s.library
        .set_preferred_order(["grk", "web", "tcc"].map(String::from).to_vec());
    s.set_search_cache_dir(root.join("cache"));
    s.refresh_search_indexes();
    settle(&mut s);
    s
}

fn settle(s: &mut Session) {
    for _ in 0..1000 {
        s.jobs().pump(16);
        assert!(s.poll_imports().is_empty(), "no stray events");
        if !s.search.is_building()
            && !s.bible_search_running()
            && !s.bible_search.is_reading_ahead()
        {
            return;
        }
    }
    panic!("jobs never settled");
}

fn run(s: &mut Session, id: &str, params: serde_json::Value) -> serde_json::Value {
    let out = s.run(id, &params).unwrap();
    settle(s);
    out
}

fn tab(out: &serde_json::Value) -> TabId {
    TabId(out["tab"].as_u64().unwrap())
}

/// The Verse list's references and Bibles.
fn rows(s: &Session, t: TabId) -> Vec<String> {
    let r = s.search_tab(t).unwrap().results.clone().expect("results");
    r.rows
        .iter()
        .map(|row| {
            let bibles: Vec<&str> = row
                .hits
                .iter()
                .map(|h| r.bibles[h.bible].abbreviation.as_str())
                .collect();
            format!("{} {}", r.row_reference(row), bibles.join(","))
        })
        .collect()
}

#[test]
fn agape_in_nt_saved_then_run_after_a_restart_gives_the_same_results() {
    let root = folder("agape");
    let mut s = launch(&root);
    let t = tab(&run(
        &mut s,
        "search.run",
        json!({ "text": "strong:G26", "scope": "New Testament", "mode": "bible" }),
    ));
    let before = rows(&s, t);
    assert_eq!(
        before,
        ["John 15:13 GRK", "Romans 5:5 GRK", "1 John 4:8 GRK"],
        "the Old Testament's G26 is out of scope"
    );
    let out = run(
        &mut s,
        "search.save",
        json!({ "tab": t.0, "name": "agape in NT" }),
    );
    assert_eq!(out["name"], "agape in NT");
    // The Tab moves on to something else before the restart.
    run(
        &mut s,
        "search.run",
        json!({ "tab": t.0, "text": "love", "scope": "" }),
    );
    drop(s);

    let mut s = launch(&root);
    let names: Vec<&str> = s.saved_searches().iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, ["agape in NT"]);
    assert_eq!(
        s.saved_searches()[0].search.summary(),
        "strong:G26 · Bible · New Testament"
    );
    let t = tab(&run(
        &mut s,
        "search.open_saved",
        json!({ "name": "AGAPE in nt" }),
    ));
    let st = s.search_tab(t).unwrap();
    assert_eq!(st.mode, SearchMode::Bible);
    assert_eq!(st.query, "strong:G26");
    assert_eq!(st.scope, "New Testament");
    assert!(st.notice.is_none() && st.error.is_none());
    assert_eq!(rows(&s, t), before, "same results after the restart");

    // The scope is what made the difference: the Whole Bible has the Old Testament's too.
    run(&mut s, "search.scope", json!({ "tab": t.0, "value": "" }));
    assert_eq!(rows(&s, t).len(), 5);
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn search_history_lists_the_last_50_searches_and_each_reruns() {
    let root = folder("history");
    let mut s = launch(&root);
    let t = tab(&run(
        &mut s,
        "search.run",
        json!({ "text": "love", "scope": "John", "mode": "bible" }),
    ));
    run(
        &mut s,
        "search.run",
        json!({ "tab": t.0, "text": "covenant", "mode": "library" }),
    );
    // Back to the Bible search: restored with its mode and Search scope.
    let out = run(&mut s, "search.open_history", json!({ "index": 1 }));
    let st = s.search_tab(tab(&out)).unwrap();
    assert_eq!(
        (st.mode, st.query.as_str(), st.scope.as_str()),
        (SearchMode::Bible, "love", "John")
    );
    assert_eq!(rows(&s, tab(&out)), ["John 15:13 GRK"]);
    assert_eq!(
        s.search_history()[0].search.summary(),
        "love · Bible · John",
        "run again, it is the newest"
    );
    assert_eq!(s.search_history().len(), 2, "not listed twice");

    for i in 0..55 {
        s.run(
            "search.run",
            &json!({ "tab": t.0, "text": format!("word{i}"), "mode": "bible", "scope": "" }),
        )
        .unwrap();
    }
    settle(&mut s);
    let queries: Vec<String> = s
        .search_history()
        .iter()
        .map(|e| e.search.query.clone())
        .collect();
    assert_eq!(queries.len(), SEARCH_HISTORY_LIMIT);
    assert_eq!(queries[0], "word54");
    assert_eq!(queries[49], "word5");
    drop(s);

    let mut s = launch(&root);
    assert_eq!(s.search_history().len(), 50, "kept across a restart");
    let listed = s.run("search.list_saved", &json!({})).unwrap();
    assert_eq!(listed["history"].as_array().unwrap().len(), 50);
    assert_eq!(
        listed["history"][49]["summary"],
        "word5 · Bible · Whole Bible"
    );
    // Every one of the 50 re-runs.
    for i in 0..50 {
        let want = s.search_history()[i].search.query.clone();
        let out = s
            .run("search.open_history", &json!({ "index": i }))
            .unwrap();
        assert!(out["error"].is_null(), "{out}");
        assert_eq!(s.search_tab(tab(&out)).unwrap().query, want);
        // Re-running moves it to the top; those above it only change places, so index
        // `i + 1` is still the next one listed before.
        assert_eq!(s.search_history()[0].search.query, want);
    }
    settle(&mut s);
    assert!(
        s.run("search.open_history", &json!({ "index": 50 }))
            .is_err()
    );

    s.run("search.clear_history", &json!({})).unwrap();
    assert!(s.search_history().is_empty());
    assert!(!s.is_enabled("search.clear_history"));
    drop(s);
    let s = launch(&root);
    assert!(s.search_history().is_empty(), "cleared for good");
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn saving_under_a_used_name_replaces_it_and_saved_searches_can_be_deleted() {
    let root = folder("names");
    let mut s = launch(&root);
    let t = tab(&run(&mut s, "search.run", json!({ "text": "love" })));
    run(&mut s, "search.save", json!({ "tab": t.0, "name": "Love" }));
    run(
        &mut s,
        "search.run",
        json!({ "tab": t.0, "text": "banner" }),
    );
    run(
        &mut s,
        "search.save",
        json!({ "tab": t.0, "name": "love " }),
    );
    run(
        &mut s,
        "search.save",
        json!({ "tab": t.0, "name": "Banner" }),
    );
    let saved: Vec<(String, String)> = s
        .saved_searches()
        .iter()
        .map(|n| (n.name.clone(), n.search.query.clone()))
        .collect();
    assert_eq!(
        saved,
        [
            ("Banner".to_string(), "banner".to_string()),
            ("love".to_string(), "banner".to_string())
        ]
    );
    let err = s
        .run("search.save", &json!({ "tab": t.0, "name": "  " }))
        .unwrap_err();
    assert!(err.to_string().contains("name"), "{err}");
    let empty = tab(&s.run("tool.search", &json!({ "split": true })).unwrap());
    let err = s
        .run("search.save", &json!({ "tab": empty.0, "name": "x" }))
        .unwrap_err();
    assert!(err.to_string().contains("Type a search"), "{err}");

    s.run("search.delete_saved", &json!({ "name": "BANNER" }))
        .unwrap();
    assert!(
        s.run("search.delete_saved", &json!({ "name": "Banner" }))
            .is_err()
    );
    drop(s);
    let s = launch(&root);
    let names: Vec<&str> = s.saved_searches().iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, ["love"]);
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn saved_searches_are_command_box_suggestions() {
    let root = folder("command-box");
    let mut s = launch(&root);
    let t = tab(&run(
        &mut s,
        "search.run",
        json!({ "text": "strong:G26", "scope": "NT" }),
    ));
    run(
        &mut s,
        "search.save",
        json!({ "tab": t.0, "name": "agape in NT" }),
    );
    run(
        &mut s,
        "search.run",
        json!({ "tab": t.0, "text": "love", "scope": "" }),
    );
    let found = s.command_box_suggestions("agape");
    let saved = found
        .iter()
        .position(|x| x.action == CommandBoxAction::OpenSavedSearch("agape in NT".into()))
        .expect("a Saved search suggestion");
    assert_eq!(found[saved].detail, "Saved search");
    assert_eq!(found[saved].label, "agape in NT — strong:G26 · Bible · NT");
    run(
        &mut s,
        "command_box.run",
        json!({ "text": "agape", "choice": saved }),
    );
    let st = s.search_tab(t).unwrap();
    assert_eq!((st.query.as_str(), st.scope.as_str()), ("strong:G26", "NT"));
    assert_eq!(rows(&s, t).len(), 3);
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_search_scope_naming_a_removed_resource_or_collection_still_opens_with_a_notice() {
    let root = folder("gone");
    let mut s = launch(&root);
    let t = tab(&run(
        &mut s,
        "search.run",
        json!({ "text": "love", "bibles": "grk,web", "scope": "1 John" }),
    ));
    assert_eq!(rows(&s, t), ["1 John 4:8 GRK,WEB"]);
    run(
        &mut s,
        "search.save",
        json!({ "tab": t.0, "name": "two Bibles" }),
    );
    run(
        &mut s,
        "collection.save",
        json!({ "name": "Mine", "resources": "tcc" }),
    );
    run(
        &mut s,
        "search.run",
        json!({ "tab": t.0, "text": "covenant", "mode": "library", "resources": "collection:Mine" }),
    );
    assert!(s.search_tab(t).unwrap().library_results.is_some());
    run(
        &mut s,
        "search.save",
        json!({ "tab": t.0, "name": "my covenant" }),
    );

    // WEB leaves the library and the Collection is deleted.
    s.run("library.remove", &json!({ "id": "web", "confirm": true }))
        .unwrap();
    s.run("collection.delete", &json!({ "name": "Mine" }))
        .unwrap();
    settle(&mut s);

    let out = run(&mut s, "search.open_saved", json!({ "name": "two Bibles" }));
    let t = tab(&out);
    assert_eq!(
        out["notice"],
        "Searched without what is gone: WEB is no longer in your library."
    );
    assert_eq!(rows(&s, t), ["1 John 4:8 GRK"], "the rest is searched");
    assert_eq!(
        s.search_tab(t).unwrap().notice.as_deref(),
        out["notice"].as_str()
    );

    let out = run(
        &mut s,
        "search.open_saved",
        json!({ "name": "my covenant" }),
    );
    assert_eq!(
        out["notice"],
        "Searched without what is gone: the Collection “Mine” no longer exists."
    );
    let st = s.search_tab(tab(&out)).unwrap();
    assert_eq!(st.mode, SearchMode::Library);
    assert_eq!(st.library.collections, ["Mine"], "kept as saved");
    assert_eq!(out["error"]["field"], "resources", "nothing left to search");

    // A new search clears the notice.
    run(
        &mut s,
        "search.run",
        json!({ "tab": t.0, "text": "love", "mode": "bible" }),
    );
    assert!(s.search_tab(t).unwrap().notice.is_none());
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn search_save_keeps_the_fields_as_typed_and_a_rerun_marks_the_tab_restored() {
    let root = folder("typed");
    let mut s = launch(&root);
    let t = tab(&run(
        &mut s,
        "search.run",
        json!({ "text": "love", "scope": "John" }),
    ));
    // Typed but not run: what is saved, though the last search was "love" in John.
    run(
        &mut s,
        "search.save",
        json!({ "tab": t.0, "name": "agape", "text": "strong:G26", "scope": "NT" }),
    );
    let saved = &s.saved_searches()[0].search;
    assert_eq!(
        (saved.query.as_str(), saved.scope.as_str()),
        ("strong:G26", "NT")
    );
    let st = s.search_tab(t).unwrap();
    assert_eq!((st.query.as_str(), st.scope.as_str()), ("love", "John"));
    // Only a query typed: the Scope as last run.
    run(
        &mut s,
        "search.save",
        json!({ "tab": t.0, "name": "banner", "text": "banner" }),
    );
    assert_eq!(s.saved_searches()[1].search.scope, "John");
    let err = s
        .run(
            "search.save",
            &json!({ "tab": t.0, "name": "x", "text": "  " }),
        )
        .unwrap_err();
    assert!(err.to_string().contains("Type a search"), "{err}");

    // Run again, even into the same fields, the Tab says it was restored.
    run(&mut s, "search.save", json!({ "tab": t.0, "name": "love" }));
    let before = s.search_tab(t).unwrap().restored;
    run(
        &mut s,
        "search.open_saved",
        json!({ "name": "love", "tab": t.0 }),
    );
    assert_eq!(s.search_tab(t).unwrap().restored, before + 1);
    run(
        &mut s,
        "search.open_history",
        json!({ "index": 0, "tab": t.0 }),
    );
    assert_eq!(s.search_tab(t).unwrap().restored, before + 2);
    run(&mut s, "search.run", json!({ "tab": t.0, "text": "love" }));
    assert_eq!(
        s.search_tab(t).unwrap().restored,
        before + 2,
        "not by a search"
    );
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn search_scope_details_name_the_bibles_resources_and_notebook() {
    let root = folder("details");
    let mut s = launch(&root);
    let t = tab(&run(&mut s, "search.run", json!({ "text": "love" })));
    let of = |s: &Session| {
        let st = s.search_tab(t).unwrap();
        s.search_scope_details(&SavedSearch::of(st).unwrap())
    };
    assert_eq!(of(&s), "Bibles: the ★ Bible");
    run(
        &mut s,
        "search.run",
        json!({ "tab": t.0, "text": "love", "bibles": "grk,web" }),
    );
    assert_eq!(of(&s), "Bibles: GRK, WEB");
    run(
        &mut s,
        "collection.save",
        json!({ "name": "Mine", "resources": "tcc" }),
    );
    run(
        &mut s,
        "search.run",
        json!({ "tab": t.0, "text": "covenant", "mode": "library" }),
    );
    assert_eq!(of(&s), "Resources: Commentaries, Lexicons, Dictionaries");
    run(
        &mut s,
        "search.run",
        json!({ "tab": t.0, "text": "covenant", "resources": "collection:Mine" }),
    );
    assert_eq!(of(&s), "Resources: Collection “Mine”");
    run(
        &mut s,
        "search.run",
        json!({ "tab": t.0, "text": "covenant", "resources": "tcc" }),
    );
    assert_eq!(of(&s), "Resources: TCC");
    run(
        &mut s,
        "search.run",
        json!({ "tab": t.0, "text": "covenant", "mode": "notes" }),
    );
    assert_eq!(of(&s), "All Notebooks");
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}
