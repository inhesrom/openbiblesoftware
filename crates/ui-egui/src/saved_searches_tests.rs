//! Saved searches and the Search history (#179) through the headless harness: a real `.obr`
//! library and User database in a temp folder, a search saved with the query row's Save…,
//! the app quit and relaunched, and the search run again from the Search Tool's sidebar;
//! the history's rows re-run and Clear empties it; a Saved search whose Bible was removed
//! opens with a notice.

use std::path::{Path, PathBuf};
use std::time::Duration;

use obs_engine::TabId;
use obs_engine::testing::bibles::{bible, v, write_library};
use obs_scripture::Book;

use super::*;

/// Removes the fixture folder (after the Harness, which may hold files open, is dropped).
struct Files(PathBuf);

impl Drop for Files {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A library of `grk`, a Strong's-tagged Bible with G26 (ἀγάπη) in two Old Testament Verses
/// and three New Testament ones, and WEB.
fn folder(name: &str) -> Files {
    let root =
        std::env::temp_dir().join(format!("obs-ui-saved-search-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let grk = obs_engine::testing::tagged_bible(
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
    write_library(&root.join("library"), &[grk, web]);
    Files(root)
}

/// Run queued engine jobs and draw until nothing is pending.
fn settle(h: &mut Harness) {
    for _ in 0..2000 {
        h.shell.st.session.jobs().pump(16);
        h.frame();
        let s = &h.shell.st.session;
        let layouts = h
            .shell
            .st
            .tabs
            .values()
            .any(|t| t.search.as_ref().is_some_and(|v| v.has_pending()));
        if !s.search.is_building()
            && !s.bible_search_running()
            && !s.bible_search.is_reading_ahead()
            && !layouts
        {
            h.frames(2);
            return;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    panic!("never settled");
}

/// Launch on the folder's User database (restoring the Last session) with its library, and
/// the Search Tool alone in the window.
fn launch(root: &Path) -> Harness {
    let mut h = Harness::launch(Some(&root.join("user.db")));
    let s = &mut h.shell.st.session;
    s.library = obs_engine::Library::new();
    assert!(s.library.scan(&root.join("library")).is_empty());
    s.library
        .set_preferred_order(["grk", "web"].map(String::from).to_vec());
    // As the app does once the library is read.
    s.remember_resource_names();
    s.set_search_cache_dir(root.join("cache"));
    s.refresh_search_indexes();
    let others: Vec<TabId> = s
        .workspace
        .tabs()
        .filter(|t| t.kind != TabKind::Tool(Tool::Search))
        .map(|t| t.id)
        .collect();
    for t in others {
        h.shell.run("tab.close", &json!({ "tab": t.0 }));
    }
    if search_tab(&h).is_none() {
        h.shell.run("tool.search", &json!({}));
    }
    settle(&mut h);
    h
}

fn search_tab(h: &Harness) -> Option<TabId> {
    h.shell
        .st
        .session
        .workspace
        .tabs()
        .find(|t| t.kind == TabKind::Tool(Tool::Search))
        .map(|t| t.id)
}

fn key(h: &mut Harness, key: egui::Key, modifiers: Modifiers) {
    h.events.push(Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    });
    h.frames(2);
}

/// Click a text field showing `at` (its text or hint) and replace what it holds with `text`,
/// without pressing Enter.
fn type_over(h: &mut Harness, at: &str, text: &str) {
    h.click_text(at);
    key(h, egui::Key::A, Modifiers::COMMAND);
    h.events.push(Event::Text(text.into()));
    h.frames(2);
}

/// [`type_over`], then press Enter.
fn type_into(h: &mut Harness, at: &str, text: &str) {
    type_over(h, at, text);
    key(h, egui::Key::Enter, Modifiers::NONE);
}

const SCOPE_HINT: &str = "Whole Bible, or a Passage list: Rom 1-8; Gal";

/// The Verse list's references.
fn rows(h: &Harness, tab: TabId) -> Vec<String> {
    let r = h.shell.st.session.search_tab(tab).unwrap().results.clone();
    let r = r.expect("results");
    r.rows.iter().map(|row| r.row_reference(row)).collect()
}

/// Visual evidence for `docs/previews/<name>.png` when `UPDATE_PREVIEWS` is set.
fn preview(h: &Harness, name: &str) {
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
        std::fs::create_dir_all(&dir).unwrap();
        h.screenshot(&dir.join(format!("{name}.ppm")));
    }
}

#[test]
fn agape_in_nt_saved_then_run_from_the_sidebar_after_a_restart_gives_the_same_results() {
    let files = folder("agape");
    let mut h = launch(&files.0);
    let tab = search_tab(&h).unwrap();
    // A wide Search Tab shows its sidebar by itself.
    assert!(h.find("SAVED SEARCHES").is_some(), "{:?}", h.texts());
    assert!(h.find("Searches you run are listed here.").is_some());
    type_into(&mut h, SCOPE_HINT, "New Testament");
    let hint = super::super::search_tab::query_hint(obs_engine::SearchMode::Bible);
    type_into(&mut h, hint, "strong:G26");
    settle(&mut h);
    let before = rows(&h, tab);
    assert_eq!(before, ["John 15:13", "Romans 5:5", "1 John 4:8"]);
    // The history row is the search with its scope.
    assert!(
        h.find("strong:G26 · Bible · New Testament").is_some(),
        "{:?}",
        h.texts()
    );
    // Save… offers the query as the name; it is typed over.
    h.click_text("Save…");
    type_into(&mut h, "strong:G26", "agape in NT");
    settle(&mut h);
    let names: Vec<String> = (h.shell.st.session.saved_searches().iter())
        .map(|n| n.name.clone())
        .collect();
    assert_eq!(names, ["agape in NT"]);
    assert!(h.find("agape in NT").is_some(), "{:?}", h.texts());
    preview(&h, "search-saved-sidebar-kittest");
    // Something else, then quit.
    h.shell.run(
        "search.run",
        &json!({ "tab": tab.0, "text": "banner", "scope": "" }),
    );
    settle(&mut h);
    h.shell.save_session(true);
    drop(h);

    let mut h = launch(&files.0);
    let tab = search_tab(&h).unwrap();
    h.click_text("agape in NT");
    settle(&mut h);
    let st = h.shell.st.session.search_tab(tab).unwrap();
    assert_eq!(
        (st.mode, st.query.as_str(), st.scope.as_str()),
        (obs_engine::SearchMode::Bible, "strong:G26", "New Testament")
    );
    assert_eq!(rows(&h, tab), before, "the same results after the restart");
    assert!(h.find("3 verses · GRK 3").is_some(), "{:?}", h.texts());
    drop(h);
    drop(files);
}

#[test]
fn history_rows_rerun_their_search_and_clear_empties_the_history() {
    let files = folder("history");
    let mut h = launch(&files.0);
    let tab = search_tab(&h).unwrap();
    for (text, scope) in [("love", "John"), ("banner", ""), ("strong:G26", "OT")] {
        h.shell.run(
            "search.run",
            &json!({ "tab": tab.0, "text": text, "scope": scope }),
        );
        settle(&mut h);
    }
    let listed = [
        "strong:G26 · Bible · OT",
        "banner · Bible · Whole Bible",
        "love · Bible · John",
    ];
    let tops: Vec<f32> = listed.iter().map(|t| h.at(t).y).collect();
    assert!(tops.is_sorted(), "newest first: {tops:?}");
    h.click_text("love · Bible · John");
    settle(&mut h);
    let st = h.shell.st.session.search_tab(tab).unwrap();
    assert_eq!((st.query.as_str(), st.scope.as_str()), ("love", "John"));
    assert_eq!(rows(&h, tab), ["John 15:13"]);
    assert!(
        h.at("love · Bible · John").y < h.at("strong:G26 · Bible · OT").y,
        "run again, it moves to the top"
    );

    // The sidebar hides and shows again.
    h.click_text("Saved & history");
    assert!(h.find("SAVED SEARCHES").is_none());
    h.click_text("Saved & history");
    assert!(h.find("SAVED SEARCHES").is_some());

    // Clear asks first.
    h.click_text("Clear");
    assert!(h.find("Clear the Search history?").is_some());
    h.click_text("Cancel");
    assert_eq!(h.shell.st.session.search_history().len(), 3);
    h.click_text("Clear");
    h.click_text("Clear");
    assert!(h.shell.st.session.search_history().is_empty());
    assert!(h.find("Searches you run are listed here.").is_some());
    drop(h);
    drop(files);
}

#[test]
fn a_saved_search_whose_bible_was_removed_opens_with_a_notice() {
    let files = folder("gone");
    let mut h = launch(&files.0);
    let tab = search_tab(&h).unwrap();
    h.shell.run(
        "search.run",
        &json!({ "tab": tab.0, "text": "love", "bibles": "grk,web", "scope": "1 John" }),
    );
    settle(&mut h);
    h.shell.run(
        "search.save",
        &json!({ "tab": tab.0, "name": "two Bibles" }),
    );
    h.shell
        .run("library.remove", &json!({ "id": "web", "confirm": true }));
    settle(&mut h);
    h.click_text("two Bibles");
    settle(&mut h);
    let notice = "Searched without what is gone: WEB is no longer in your library.";
    assert!(h.find(notice).is_some(), "{:?}", h.texts());
    assert_eq!(rows(&h, tab), ["1 John 4:8"]);
    preview(&h, "search-saved-notice-kittest");
    drop(h);
    drop(files);
}

#[test]
fn save_keeps_the_query_and_scope_as_typed_without_enter() {
    let files = folder("typed");
    let mut h = launch(&files.0);
    let tab = search_tab(&h).unwrap();
    h.shell.run(
        "search.run",
        &json!({ "tab": tab.0, "text": "love", "scope": "" }),
    );
    settle(&mut h);
    // Typed over, neither field run.
    type_over(&mut h, "love", "strong:G26");
    type_over(&mut h, SCOPE_HINT, "New Testament");
    h.click_text("Save…");
    type_into(&mut h, "strong:G26", "agape in NT");
    settle(&mut h);
    let saved: Vec<(&str, &str, &str)> = (h.shell.st.session.saved_searches().iter())
        .map(|n| {
            (
                n.name.as_str(),
                n.search.query.as_str(),
                n.search.scope.as_str(),
            )
        })
        .collect();
    assert_eq!(saved, [("agape in NT", "strong:G26", "New Testament")]);
    // Saving runs nothing: the fields still show what is typed.
    let st = h.shell.st.session.search_tab(tab).unwrap();
    assert_eq!((st.query.as_str(), st.scope.as_str()), ("love", ""));
    assert!(h.find("strong:G26").is_some(), "{:?}", h.texts());
    assert!(h.find("New Testament").is_some(), "{:?}", h.texts());
    drop(h);
    drop(files);
}

#[test]
fn running_a_saved_or_history_search_replaces_what_is_typed() {
    let files = folder("replay");
    let mut h = launch(&files.0);
    let tab = search_tab(&h).unwrap();
    h.shell.run(
        "search.run",
        &json!({ "tab": tab.0, "text": "love", "scope": "" }),
    );
    settle(&mut h);
    h.shell.run(
        "search.save",
        &json!({ "tab": tab.0, "name": "plain love" }),
    );
    settle(&mut h);
    let shows = |h: &Harness| (h.find("love").is_some(), h.find(SCOPE_HINT).is_some());

    // Typed over without Enter, then the Saved search (the same query and Scope) runs.
    type_over(&mut h, "love", "banner");
    type_over(&mut h, SCOPE_HINT, "John");
    assert_eq!(shows(&h), (false, false), "{:?}", h.texts());
    h.click_text("plain love");
    settle(&mut h);
    assert_eq!(shows(&h), (true, true), "{:?}", h.texts());
    assert!(h.find("banner").is_none() && h.find("John").is_none());

    // The same from the Search history.
    type_over(&mut h, "love", "banner");
    type_over(&mut h, SCOPE_HINT, "John");
    assert_eq!(shows(&h), (false, false), "{:?}", h.texts());
    h.click_text("love · Bible · Whole Bible");
    settle(&mut h);
    assert_eq!(shows(&h), (true, true), "{:?}", h.texts());
    assert!(h.find("banner").is_none() && h.find("John").is_none());
    let st = h.shell.st.session.search_tab(tab).unwrap();
    assert_eq!((st.query.as_str(), st.scope.as_str()), ("love", ""));
    drop(h);
    drop(files);
}
