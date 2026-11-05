//! Favorites and History (#155): starring Tabs' Current locations into Favorites folders and
//! opening them after a restart; History recording opened and jumped Tabs across all Tabs and
//! bringing them back. A real `.obr` library and a real User database in a temp folder.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use obs_scripture::{Book, Verse};
use serde_json::json;

use crate::testing::bibles::{bible, v, write_library};
use crate::*;

/// A library of BSB (Gen 1:1, Jn 3:16, Rom 8:28) and WEB (the same Verses), and where the
/// User database goes.
fn folder(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("obs-favorites-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let verses = [
        (
            v(Book::Genesis, 1, 1),
            "In the beginning God created the heavens and the earth.",
        ),
        (
            v(Book::John, 3, 16),
            "For God so loved the world that He gave His one and only Son.",
        ),
        (
            v(Book::Romans, 8, 28),
            "And we know that God works all things together for the good.",
        ),
    ];
    write_library(
        &root.join("library"),
        &[
            bible("bsb", "BSB", &verses, None, &[]),
            bible("web", "WEB", &verses, None, &[]),
        ],
    );
    root
}

/// Launch as the app does: scan the library, then attach the User database.
fn launch(root: &Path) -> Session {
    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(&root.join("library")).is_empty());
    let (store, _) = obs_store::UserStore::open(&root.join("user.db")).unwrap();
    s.attach_user(Box::new(store));
    s
}

fn tab(out: &serde_json::Value) -> TabId {
    TabId(out["tab"].as_u64().unwrap())
}

fn open(s: &mut Session, id: &str) -> TabId {
    tab(&s.run("resource.open", &json!({ "id": id })).unwrap())
}

fn go(s: &mut Session, t: TabId, text: &str) {
    s.run("tab.go", &json!({ "tab": t.0, "text": text }))
        .unwrap();
}

fn at(s: &Session, t: TabId) -> Option<Verse> {
    s.workspace.tab(t).unwrap().history.current()
}

fn labels(s: &Session) -> Vec<String> {
    s.history().iter().map(|v| v.label.clone()).collect()
}

#[test]
fn starring_rom_8_28_in_bsb_and_opening_it_after_a_restart_lands_there() {
    let root = folder("restart");
    let mut s = launch(&root);
    let bsb = open(&mut s, "bsb");
    go(&mut s, bsb, "Rom 8:28");
    let out = s.run("favorite.add", &json!({ "tab": bsb.0 })).unwrap();
    assert_eq!(out["label"], "Romans 8:28 · BSB");
    assert_eq!(out["added"], true);
    assert_eq!(s.favorite_of(bsb).map(|f| f.id), out["id"].as_i64());
    // Starring the same place again keeps one Favorite.
    let again = s.run("favorite.add", &json!({ "tab": bsb.0 })).unwrap();
    assert_eq!(again["added"], false);
    assert_eq!(s.favorites().len(), 1);
    drop(s);

    // Restart, with nothing open: the Favorite opens BSB at Rom 8:28.
    let mut s = launch(&root);
    assert_eq!(s.workspace.tabs().count(), 0);
    let favorites = s.favorites().to_vec();
    assert_eq!(favorites.len(), 1);
    assert_eq!(favorites[0].label, "Romans 8:28 · BSB");
    let out = s
        .run("favorite.open", &json!({ "id": favorites[0].id }))
        .unwrap();
    let t = tab(&out);
    assert_eq!(
        s.workspace.tab(t).unwrap().kind,
        TabKind::Resource("bsb".into())
    );
    assert_eq!(at(&s, t), Some(v(Book::Romans, 8, 28)));
    assert_eq!(s.workspace.focused(), Some(t));
    assert!(s.favorite_of(t).is_some());

    // With BSB open elsewhere, that Tab moves there instead of a new one opening.
    go(&mut s, t, "Gen 1:1");
    s.workspace.take_effects();
    let out = s
        .run("favorite.open", &json!({ "id": favorites[0].id }))
        .unwrap();
    assert_eq!(tab(&out), t);
    assert_eq!(at(&s, t), Some(v(Book::Romans, 8, 28)));
    assert!(s.workspace.take_effects().contains(&Effect::Navigate {
        tab: t,
        verse: v(Book::Romans, 8, 28)
    }));
    // Split: a new Tab even so.
    let out = s
        .run(
            "favorite.open",
            &json!({ "id": favorites[0].id, "split": true }),
        )
        .unwrap();
    assert_ne!(tab(&out), t);
    assert_eq!(at(&s, tab(&out)), Some(v(Book::Romans, 8, 28)));
    // Windows cannot remove the User database while it is open.
    drop(s);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn favorites_go_into_folders_move_between_them_and_are_removed() {
    let root = folder("folders");
    let mut s = launch(&root);
    let bsb = open(&mut s, "bsb");
    go(&mut s, bsb, "Jn 3:16");
    let a = s
        .run("favorite.add", &json!({ "tab": bsb.0, "folder": "Sermon" }))
        .unwrap();
    go(&mut s, bsb, "Rom 8:28");
    let b = s.run("favorite.add", &json!({ "tab": bsb.0 })).unwrap();
    // A Word Study and a Search are Favorites too.
    let ws = tab(&s
        .run("guide.word_study", &json!({ "strongs": "G26" }))
        .unwrap());
    let w = s
        .run("favorite.add", &json!({ "tab": ws.0, "folder": "sermon" }))
        .unwrap();
    assert_eq!(w["label"], "G26 · Word Study");
    assert_eq!(s.favorite_folders(), ["Sermon"]);
    let shown = |s: &Session| {
        s.favorites()
            .iter()
            .map(|f| format!("{}/{}", f.folder, f.label))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        shown(&s),
        [
            "/Romans 8:28 · BSB",
            "Sermon/John 3:16 · BSB",
            "Sermon/G26 · Word Study"
        ]
    );
    s.run(
        "favorite.move",
        &json!({ "id": b["id"], "folder": "Advent" }),
    )
    .unwrap();
    s.run("favorite.remove", &json!({ "id": a["id"] })).unwrap();
    assert_eq!(
        shown(&s),
        ["Advent/Romans 8:28 · BSB", "Sermon/G26 · Word Study"]
    );
    // The Command box's suggestions (#139) match labels and folders.
    let matching = |q: &str| {
        s.favorites_matching(q)
            .iter()
            .map(|f| f.label.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(matching("rom"), ["Romans 8:28 · BSB"]);
    assert_eq!(matching("SERMON"), ["G26 · Word Study"]);
    assert!(s.run("favorite.remove", &json!({ "id": a["id"] })).is_err());

    // Keyless Tabs have nothing to star.
    let library = tab(&s.run("library.open", &json!({})).unwrap());
    let err = s
        .run("favorite.add", &json!({ "tab": library.0 }))
        .unwrap_err();
    assert!(err.to_string().contains("no location"), "{err}");

    // A Word Study Favorite opens the Word Study at its Strong's number.
    s.run("tab.close", &json!({ "tab": ws.0 })).unwrap();
    let out = s.run("favorite.open", &json!({ "id": w["id"] })).unwrap();
    let t = s.workspace.tab(tab(&out)).unwrap();
    assert_eq!(t.kind, TabKind::Guide(Guide::WordStudy));
    assert_eq!(t.entry_history.current().unwrap().to_string(), "G26");
    drop(s);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn the_history_tool_shows_the_last_sessions_locations_in_order() {
    let root = folder("history");
    let mut s = launch(&root);
    let bsb = open(&mut s, "bsb");
    // Linked WEB follows BSB without Visits of its own.
    let web = open(&mut s, "web");
    s.run("tab.link", &json!({ "tab": bsb.0, "set": "A" }))
        .unwrap();
    s.run("tab.link", &json!({ "tab": web.0, "set": "A" }))
        .unwrap();
    // The UI records History after each frame's effects, once new Tabs are placed: these
    // have no location yet (no UI places them here), so they make no Visits.
    s.record_history();
    go(&mut s, bsb, "Gen 1:1");
    go(&mut s, bsb, "Rom 8:28");
    assert_eq!(at(&s, web), Some(v(Book::Romans, 8, 28)));
    go(&mut s, bsb, "Rom 8:28");
    go(&mut s, bsb, "Jn 3:16");
    s.run("tab.back", &json!({ "tab": bsb.0 })).unwrap();
    s.run("guide.word_study", &json!({ "strongs": "G26" }))
        .unwrap();
    s.run("search.run", &json!({ "text": "loved" })).unwrap();
    // The Last session is saved on quit, with History's Visits.
    s.save_last_session().unwrap();
    drop(s);

    let mut s = launch(&root);
    let expected = [
        "“loved” · Bible Search",
        "G26 · Word Study",
        "Romans 8:28 · BSB",
        "John 3:16 · BSB",
        "Romans 8:28 · BSB",
        "Genesis 1:1 · BSB",
    ];
    assert_eq!(labels(&s), expected);
    let days = s.history_days("");
    assert_eq!(days.len(), 1);
    assert_eq!(days[0].heading, "Today");
    assert_eq!(days[0].visits.len(), expected.len());
    // Searchable.
    let found: Vec<String> = s
        .history_days("rom")
        .into_iter()
        .flat_map(|d| d.visits)
        .map(|v| v.label)
        .collect();
    assert_eq!(found, ["Romans 8:28 · BSB", "Romans 8:28 · BSB"]);

    // Opening a Visit brings that Tab kind back at that location.
    let john = s.history()[3].id;
    let out = s.run("history.open", &json!({ "id": john })).unwrap();
    let t = tab(&out);
    assert_eq!(
        s.workspace.tab(t).unwrap().kind,
        TabKind::Resource("bsb".into())
    );
    assert_eq!(at(&s, t), Some(v(Book::John, 3, 16)));
    let search = s.history()[0].id;
    let t = tab(&s.run("history.open", &json!({ "id": search })).unwrap());
    let st = s.search_tab(t).unwrap();
    assert_eq!((st.mode, st.query.as_str()), (SearchMode::Bible, "loved"));

    s.run("history.clear", &json!({})).unwrap();
    assert!(s.history().is_empty());
    drop(s);
    let s = launch(&root);
    assert!(s.history().is_empty());
    drop(s);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_favorite_of_a_removed_resource_says_so() {
    let root = folder("removed");
    let mut s = launch(&root);
    let web = open(&mut s, "web");
    go(&mut s, web, "Jn 3:16");
    let id = s.run("favorite.add", &json!({ "tab": web.0 })).unwrap()["id"].clone();
    drop(s);
    std::fs::remove_file(root.join("library/web.obr")).unwrap();
    let mut s = launch(&root);
    let err = s.run("favorite.open", &json!({ "id": id })).unwrap_err();
    assert!(
        err.to_string().contains("no longer in your library"),
        "{err}"
    );
    drop(s);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn history_headings_name_today_yesterday_and_older_days() {
    assert_eq!(day_heading("2026-11-05", 0), "Today");
    assert_eq!(day_heading("2026-11-04", 1), "Yesterday");
    assert_eq!(day_heading("2026-11-05", 2), "Thursday 5 November 2026");
    assert_eq!(day_heading("2024-02-29", 600), "Thursday 29 February 2024");
    assert_eq!(day_heading("1970-01-01", 9_000), "Thursday 1 January 1970");
    assert_eq!(day_heading("2000-03-01", 9_000), "Wednesday 1 March 2000");
}

#[test]
fn locations_round_trip_through_json() {
    let loc = TabLocation {
        kind: TabKind::Tool(Tool::Search),
        verse: None,
        key: None,
        search: Some(SavedSearch {
            mode: SearchMode::Library,
            query: "grace".into(),
            scope: String::new(),
            bibles: None,
            types: vec![obs_model::ResourceType::Lexicon],
            resources: vec!["tcc".into()],
            notebook: None,
        }),
    };
    assert_eq!(TabLocation::from_json(&loc.to_json()), Some(loc));
}
