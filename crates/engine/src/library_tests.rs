#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use obs_model::{Manifest, ResourceType};
use serde_json::json;

use crate::*;

/// A fresh folder holding a library directory and a User database path.
fn temp(name: &str) -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("obs-library-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let lib = root.join("library");
    std::fs::create_dir_all(&lib).unwrap();
    (lib, root.join("user.db"))
}

/// Write a small Bible `.obr` with this id, abbreviation and language into `dir`.
fn write_bible(dir: &Path, id: &str, abbrev: &str, language: &str) -> PathBuf {
    let mut r = obs_store::sample_resource();
    r.manifest.resource_id = id.into();
    r.manifest.abbreviation = abbrev.into();
    r.manifest.title = format!("{abbrev} Bible");
    r.manifest.language = language.into();
    let path = dir.join(format!("{id}.obr"));
    obs_store::write_obr(&r, &path).unwrap();
    path
}

/// Launch as the app does: scan the library directory, then attach the User database.
fn launch(lib: &Path, db: &Path) -> Session {
    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(lib).is_empty());
    let (store, _) = obs_store::UserStore::open(db).unwrap();
    s.attach_user(Box::new(store));
    s
}

fn bibles(s: &Session) -> Vec<String> {
    s.library
        .of_type(ResourceType::Bible)
        .map(|e| e.manifest.resource_id.clone())
        .collect()
}

fn starred(s: &Session) -> String {
    s.library
        .preferred(ResourceType::Bible)
        .unwrap()
        .manifest
        .resource_id
        .clone()
}

#[test]
fn reordering_persists_across_restarts_and_changes_which_bible_is_starred() {
    let (lib, db) = temp("reorder");
    for (id, ab) in [("asv", "ASV"), ("kjv", "KJV"), ("web", "WEB")] {
        write_bible(&lib, id, ab, "en");
    }
    let mut s = launch(&lib, &db);
    // Never reordered: file-name order, and the first Bible is ★.
    assert_eq!(bibles(&s), ["asv", "kjv", "web"]);
    assert_eq!(starred(&s), "asv");

    // Drag WEB to the top: it becomes ★.
    let out = s
        .run("library.move", &json!({ "id": "web", "to": 0 }))
        .unwrap();
    assert_eq!(out["position"], 0);
    assert_eq!(bibles(&s), ["web", "asv", "kjv"]);
    assert_eq!(starred(&s), "web");
    // Move down / Move up step one place; past either end they stay put.
    s.run("library.move_down", &json!({ "id": "asv" })).unwrap();
    assert_eq!(bibles(&s), ["web", "kjv", "asv"]);
    s.run("library.move_down", &json!({ "id": "asv" })).unwrap();
    assert_eq!(bibles(&s), ["web", "kjv", "asv"]);
    s.run("library.move_up", &json!({ "id": "web" })).unwrap();
    assert_eq!(bibles(&s), ["web", "kjv", "asv"]);
    s.run("library.move_up", &json!({ "id": "kjv" })).unwrap();
    assert_eq!(bibles(&s), ["kjv", "web", "asv"]);
    assert_eq!(starred(&s), "kjv");
    drop(s);

    // Restart: the order and ★ come back from the User database.
    let s = launch(&lib, &db);
    assert_eq!(bibles(&s), ["kjv", "web", "asv"]);
    assert_eq!(starred(&s), "kjv");
    drop(s);
    std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
}

#[test]
fn new_imports_go_to_the_bottom_of_their_type() {
    let (lib, db) = temp("imports");
    write_bible(&lib, "kjv", "KJV", "en");
    write_bible(&lib, "web", "WEB", "en");
    let mut s = launch(&lib, &db);
    s.run("library.move", &json!({ "id": "web", "to": 0 }))
        .unwrap();
    drop(s);
    // A Bible imported later sorts first by file name, but goes to the bottom.
    write_bible(&lib, "asv", "ASV", "en");
    let s = launch(&lib, &db);
    assert_eq!(bibles(&s), ["web", "kjv", "asv"]);

    drop(s);

    // The same holds when the User database is attached before Resources are added.
    let mut s = Session::new(JobMode::Inline);
    let (store, _) = obs_store::UserStore::open(&db).unwrap();
    s.attach_user(Box::new(store));
    for id in ["asv", "kjv", "web"] {
        s.library.add_file(&lib.join(format!("{id}.obr"))).unwrap();
    }
    assert_eq!(bibles(&s), ["web", "kjv", "asv"]);
    drop(s);
    std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
}

#[test]
fn new_imports_stay_last_across_restarts_without_any_moves() {
    let (lib, db) = temp("imports-no-moves");
    write_bible(&lib, "kjv", "KJV", "en");
    write_bible(&lib, "web", "WEB", "en");
    let s = launch(&lib, &db);
    assert_eq!(bibles(&s), ["kjv", "web"]);
    assert_eq!(
        s.user.as_ref().unwrap().preferred_order().unwrap(),
        ["kjv", "web"]
    );
    drop(s);

    write_bible(&lib, "asv", "ASV", "en");
    let s = launch(&lib, &db);
    assert_eq!(bibles(&s), ["kjv", "web", "asv"]);
    assert_eq!(starred(&s), "kjv");
    drop(s);

    // Each launch also pins newly seen Resources, not just the first launch.
    write_bible(&lib, "bsb", "BSB", "en");
    let s = launch(&lib, &db);
    assert_eq!(bibles(&s), ["kjv", "web", "asv", "bsb"]);
    assert_eq!(starred(&s), "kjv");
    drop(s);
    std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
}

#[test]
fn moving_keeps_other_types_and_absent_resources_in_place() {
    let mut lib = Library::new();
    let entry = |id: &str, ty| LibraryEntry {
        manifest: Manifest::new(id, id, id, ty, "en"),
        path: format!("/library/{id}.obr").into(),
    };
    lib.set_preferred_order(vec!["gone".into(), "b".into(), "a".into()]);
    lib.add(entry("a", ResourceType::Bible));
    lib.add(entry("jfb", ResourceType::Commentary));
    lib.add(entry("b", ResourceType::Bible));
    lib.add(entry("c", ResourceType::Bible));
    let ids = |l: &Library| -> Vec<String> {
        l.entries()
            .iter()
            .map(|e| e.manifest.resource_id.clone())
            .collect()
    };
    assert_eq!(ids(&lib), ["b", "a", "c", "jfb"]);
    assert_eq!(lib.move_to("c", 99), Some(2));
    assert_eq!(lib.move_to("c", 1), Some(1));
    assert_eq!(ids(&lib), ["b", "c", "a", "jfb"]);
    assert_eq!(lib.move_to("jfb", 0), Some(0));
    assert_eq!(lib.position_in_type("a"), Some(2));
    assert_eq!(lib.move_to("nope", 0), None);
    // A Resource that is away keeps its rank in what is saved.
    assert_eq!(lib.preferred_order(), ["b", "c", "a", "jfb", "gone"]);
}

#[test]
fn remove_asks_first_then_deletes_the_obr_from_the_library() {
    let (lib, db) = temp("remove");
    let kjv = write_bible(&lib, "kjv", "KJV", "en");
    write_bible(&lib, "web", "WEB", "en");
    let mut s = launch(&lib, &db);
    s.run("library.move", &json!({ "id": "kjv", "to": 1 }))
        .unwrap();
    let reading = s.run("resource.open", &json!({ "id": "kjv" })).unwrap();
    let about = s.run("resource.about", &json!({ "id": "kjv" })).unwrap();
    s.workspace.take_effects();

    // Without confirmation nothing happens except the question, which states the count.
    let out = s.run("library.remove", &json!({ "id": "kjv" })).unwrap();
    assert!(kjv.is_file());
    assert!(s.library.get("kjv").is_some());
    let effects = s.workspace.take_effects();
    let [
        Effect::ConfirmRemove {
            resource_id,
            message,
            ..
        },
    ] = &effects[..]
    else {
        panic!("{effects:?}");
    };
    assert_eq!(resource_id, "kjv");
    assert!(message.contains("kjv.obr is deleted"), "{message}");
    assert!(message.contains("No Annotations refer"), "{message}");
    assert_eq!(out["annotations"], 0);

    // Confirmed: the file is gone, so are its Tabs, and the library no longer lists it.
    let out = s
        .run("library.remove", &json!({ "id": "kjv", "confirm": true }))
        .unwrap();
    assert_eq!(out["deleted_file"], true);
    assert!(!kjv.exists());
    assert!(s.library.get("kjv").is_none());
    for t in [&reading, &about] {
        assert!(s.workspace.tab(TabId(t["tab"].as_u64().unwrap())).is_none());
    }
    let effects = s.workspace.take_effects();
    assert!(
        effects.contains(&Effect::Removed("kjv".into())),
        "{effects:?}"
    );
    assert!(s.run("library.remove", &json!({ "id": "kjv" })).is_err());
    drop(s);
    let s = launch(&lib, &db);
    assert_eq!(bibles(&s), ["web"]);
    assert_eq!(s.library.preferred_order(), ["web"]);
    drop(s);
    std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
}

#[test]
fn remove_leaves_a_file_outside_the_library_folder_alone() {
    let (lib, db) = temp("remove-outside");
    let elsewhere = lib.parent().unwrap().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let file = write_bible(&elsewhere, "mine", "MINE", "en");
    let mut s = launch(&lib, &db);
    s.library.add_file(&file).unwrap();
    let out = s.run("library.remove", &json!({ "id": "mine" })).unwrap();
    assert!(
        out["confirm"]
            .as_str()
            .unwrap()
            .contains("stays where it is"),
        "{out}"
    );
    let out = s
        .run("library.remove", &json!({ "id": "mine", "confirm": true }))
        .unwrap();
    assert_eq!(out["deleted_file"], false);
    assert!(file.is_file());
    assert!(s.library.get("mine").is_none());
    drop(s);
    std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
}

#[test]
fn filtering_by_text_and_by_language_works_together() {
    let m = |id: &str, title: &str, lang: &str| {
        let mut m = Manifest::new(id, title, id.to_uppercase(), ResourceType::Bible, lang);
        m.title = title.into();
        m
    };
    let kjv = m("kjv", "King James Version", "en");
    let tahot = m("tahot", "Translators Amalgamated Hebrew OT", "hbo");
    let tagnt = m("tagnt", "Translators Amalgamated Greek NT", "grc");
    let all = [&kjv, &tahot, &tagnt];
    let shown = |f: &LibraryFilter| -> Vec<&str> {
        all.iter()
            .filter(|m| f.matches(m))
            .map(|m| m.resource_id.as_str())
            .collect()
    };
    let mut f = LibraryFilter::default();
    assert_eq!(shown(&f), ["kjv", "tahot", "tagnt"]);
    f.text = "translators".into();
    assert_eq!(shown(&f), ["tahot", "tagnt"]);
    f.toggle_language("Greek");
    assert_eq!(shown(&f), ["tagnt"]);
    f.toggle_language("English");
    assert_eq!(shown(&f), ["tagnt"], "text and language both apply");
    f.text = "KJV".into();
    assert_eq!(shown(&f), ["kjv"], "abbreviation matches, any case");
    f.text.clear();
    assert_eq!(shown(&f), ["kjv", "tagnt"], "chips combine with or");
    f.toggle_language("Greek");
    f.toggle_language("English");
    assert_eq!(f, LibraryFilter::default());
    assert_eq!(language_name("hbo"), "Hebrew");
    assert_eq!(language_name("en-GB"), "English");
    assert_eq!(language_name("xyz"), "xyz");

    // Chips are language names, alphabetical whatever the Preferred order; `he` and `hbo`
    // share the Hebrew chip, which shows both.
    let mut lib = Library::new();
    for (id, lang) in [
        ("wlc", "hbo"),
        ("kjv", "en"),
        ("modern", "he"),
        ("sblgnt", "grc"),
    ] {
        lib.add(LibraryEntry {
            manifest: m(id, id, lang),
            path: format!("/library/{id}.obr").into(),
        });
    }
    assert_eq!(lib.languages(), ["English", "Greek", "Hebrew"]);
    lib.move_to("modern", 0);
    assert_eq!(lib.languages(), ["English", "Greek", "Hebrew"]);
    let mut f = LibraryFilter::default();
    f.toggle_language("Hebrew");
    let hebrew: Vec<&str> = lib
        .entries()
        .iter()
        .filter(|e| f.matches(&e.manifest))
        .map(|e| e.manifest.resource_id.as_str())
        .collect();
    assert_eq!(hebrew, ["modern", "wlc"]);
}

#[test]
fn the_about_tab_is_keyless_and_one_per_resource() {
    let (lib, _) = temp("about");
    write_bible(&lib, "kjv", "KJV", "en");
    let mut s = Session::new(JobMode::Inline);
    s.library.scan(&lib);
    let a = s.run("resource.about", &json!({ "id": "kjv" })).unwrap();
    let b = s.run("resource.about", &json!({ "id": "kjv" })).unwrap();
    assert_eq!(a, b);
    let t = s.workspace.tab(TabId(a["tab"].as_u64().unwrap())).unwrap();
    assert_eq!(t.title, "About · KJV");
    assert_eq!(t.kind.header_box(), HeaderBox::Keyless);
    assert!(!t.kind.has_link_chip());
    assert!(t.key_schemes.is_empty());
    assert!(s.run("resource.about", &json!({ "id": "nope" })).is_err());
    // A saved About Tab of a Resource that has gone is dropped on restore.
    let snap = s.workspace.snapshot().without_missing(|_| false);
    assert!(snap.tabs.is_empty());
    assert_eq!(annotations_phrase(1), "1 Annotation refers");
    assert_eq!(annotations_phrase(3), "3 Annotations refer");
    drop(s);
    std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
}

#[test]
fn remove_succeeds_when_the_obr_was_deleted_outside_the_app() {
    let (lib, db) = temp("remove-missing");
    let file = write_bible(&lib, "kjv", "KJV", "en");
    let mut s = launch(&lib, &db);
    s.run("resource.open", &json!({ "id": "kjv" })).unwrap();
    s.run("resource.about", &json!({ "id": "kjv" })).unwrap();
    s.run("library.remove", &json!({ "id": "kjv" })).unwrap();
    std::fs::remove_file(file).unwrap();
    s.run("library.remove", &json!({ "id": "kjv", "confirm": true }))
        .unwrap();
    assert!(s.library.get("kjv").is_none());
    assert!(s.workspace.tabs().next().is_none());
    assert!(
        s.user
            .as_ref()
            .unwrap()
            .preferred_order()
            .unwrap()
            .is_empty()
    );
    drop(s);
    std::fs::remove_dir_all(lib.parent().unwrap()).unwrap();
}
