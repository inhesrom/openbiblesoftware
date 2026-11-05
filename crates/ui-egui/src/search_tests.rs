//! The Search Tool (#59) through the shell's real-event headless harness: typing a query and
//! Enter, the Bible picker, the summary line and highlighted Verse rows, ↑/↓/Enter and
//! Ctrl/Cmd-click to open hits, and how soon the first results page draws.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use obs_engine::testing::bibles;
use obs_engine::{Library, TabId};

use super::*;

/// Removes the fixture folder (after the Harness, which may hold files open, is dropped).
struct Files(PathBuf);

impl Drop for Files {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Run queued engine jobs (Inline mode) and draw until nothing is pending.
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

/// The Harness with a library of real `.obr` files (KJV, WEB, a long Psalm and TAHOT) whose
/// Search indexes are built.
fn harness(name: &str, psalm_verses: u16) -> (Harness, Files) {
    let root = std::env::temp_dir().join(format!("obs-ui-search-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let files = Files(root.clone());
    bibles::write_library(
        &root.join("library"),
        &[
            bibles::kjv(),
            bibles::web(),
            bibles::psalms(psalm_verses),
            bibles::tahot(),
        ],
    );
    let mut h = Harness::new();
    let s = &mut h.shell.st.session;
    s.library = Library::new();
    assert!(s.library.scan(&root.join("library")).is_empty());
    s.library
        .set_preferred_order(["kjv", "web", "psalms", "tahot"].map(String::from).to_vec());
    s.set_search_cache_dir(root.join("cache"));
    s.refresh_search_indexes();
    settle(&mut h);
    (h, files)
}

fn search_tab(h: &Harness) -> TabId {
    h.shell
        .st
        .session
        .workspace
        .tabs()
        .find(|t| t.kind == TabKind::Tool(Tool::Search))
        .map(|t| t.id)
        .expect("a Search Tab")
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

/// Filled rectangles of exactly `color` painted last frame.
fn rects_of(h: &Harness, color: egui::Color32) -> Vec<Rect> {
    fn walk(shape: &Shape, color: egui::Color32, out: &mut Vec<Rect>) {
        match shape {
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, color, out)),
            Shape::Rect(r) if r.fill == color => out.push(r.rect),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for c in &h.last_shapes {
        walk(&c.shape, color, &mut out);
    }
    out
}

/// Open the Search Tool, type `query` into its box and press Enter.
fn type_query(h: &mut Harness, query: &str) {
    if !h
        .shell
        .st
        .session
        .workspace
        .tabs()
        .any(|t| t.kind == TabKind::Tool(Tool::Search))
    {
        h.shell.run("tool.search", &json!({}));
        h.frames(2);
    }
    let hint = super::super::search_tab::query_hint(obs_engine::SearchMode::Bible);
    h.click_text(hint);
    h.events.push(Event::Text(query.into()));
    h.frame();
    key(h, egui::Key::Enter, Modifiers::NONE);
}

#[test]
fn love_thy_neighbour_in_kjv_and_web_draws_highlighted_verse_rows() {
    let (mut h, files) = harness("phrase", 3);
    h.shell.run("tool.search", &json!({}));
    h.frames(2);
    // The ★ Bible is chosen; add WEB with the picker.
    assert!(h.find("KJV").is_some(), "{:?}", h.texts());
    h.click_text("+ Bible");
    h.click_text("WEB — WEB (test)");
    type_query(&mut h, "\"love thy neighbour\"");
    settle(&mut h);
    let tab = search_tab(&h);
    let st = h.shell.st.session.search_tab(tab).unwrap();
    assert_eq!(st.query, "\"love thy neighbour\"");
    assert_eq!(st.bibles(&h.shell.st.session.library), ["kjv", "web"]);
    for text in [
        "3 verses · 3 hits · KJV 3 · WEB 0",
        "Leviticus 19:18",
        "Matthew 5:43",
        "Matthew 22:39",
    ] {
        assert!(
            h.find(text).is_some(),
            "{text:?} not drawn: {:?}",
            h.texts()
        );
    }
    let view = h.shell.st.tabs[&tab].search.as_ref().unwrap();
    assert_eq!(view.last.visible, 0..3);
    assert_eq!(view.last.placeholders, 0);
    assert_eq!(view.last.drawn, 3);
    // One highlight per row, under "love thy neighbour", on one line each.
    // Highlights under "love thy neighbour" in each of the three rows (a phrase that wraps
    // in this narrow Panel is marked word by word on each line).
    let hits = rects_of(&h, h.shell.st.palette.search_hit);
    let row_tops: Vec<f32> = ["Leviticus 19:18", "Matthew 5:43", "Matthew 22:39"]
        .iter()
        .map(|r| h.find(r).unwrap().top())
        .collect();
    for (i, top) in row_tops.iter().enumerate() {
        let bottom = row_tops.get(i + 1).copied().unwrap_or(f32::MAX);
        let width: f32 = hits
            .iter()
            .filter(|r| r.top() > *top && r.bottom() < bottom)
            .map(|r| r.width())
            .sum();
        assert!(width > 80.0, "row {i}: {width} px of highlight in {hits:?}");
    }
    // The recent queries remember it.
    assert_eq!(
        h.shell
            .st
            .session
            .recent_searches(obs_engine::SearchMode::Bible),
        ["\"love thy neighbour\""]
    );
    drop(h);
    drop(files);
}

#[test]
fn arrows_and_enter_open_hits_and_ctrl_click_opens_a_new_tab() {
    let (mut h, files) = harness("open", 3);
    type_query(&mut h, "neighbour");
    settle(&mut h);
    let tab = search_tab(&h);
    assert_eq!(
        h.shell.st.session.search_tab(tab).unwrap().selected,
        Some(0),
        "the first row is chosen"
    );
    // ↓ moves to Matthew 5:43; Enter opens it in a new KJV Tab (no Link set, no KJV Tab).
    key(&mut h, egui::Key::ArrowDown, Modifiers::NONE);
    assert_eq!(
        h.shell.st.session.search_tab(tab).unwrap().selected,
        Some(1)
    );
    key(&mut h, egui::Key::Enter, Modifiers::NONE);
    h.frames(3);
    let kjv = h.tab_id("KJV");
    let at = |h: &mut Harness, t: TabId| {
        h.shell
            .st
            .session
            .workspace
            .tab(t)
            .unwrap()
            .history
            .current()
    };
    let mt5_43 = obs_scripture::Verse::new(obs_scripture::Book::Matthew, 5, 43);
    assert_eq!(at(&mut h, kjv), Some(mt5_43));
    // A click on the Romans row moves that same KJV Tab (the most recently active one).
    h.shell.st.session.workspace.focus(tab);
    h.shell.apply_effects();
    h.frames(2);
    h.click_text("Romans 13:10");
    let rom = obs_scripture::Verse::new(obs_scripture::Book::Romans, 13, 10);
    assert_eq!(at(&mut h, kjv), Some(rom));
    let kjv_tabs = |h: &Harness| {
        h.shell
            .st
            .session
            .workspace
            .tabs()
            .filter(|t| t.kind == TabKind::Resource("kjv".into()))
            .count()
    };
    assert_eq!(kjv_tabs(&h), 1);
    // Ctrl/Cmd-click always opens a new Tab, split off to the right.
    let panels = h.panels().len();
    h.shell.st.session.workspace.focus(tab);
    h.shell.apply_effects();
    h.frames(2);
    h.modifiers = Modifiers::COMMAND;
    h.click_text("Leviticus 19:18");
    h.modifiers = Modifiers::NONE;
    h.frames(3);
    assert_eq!(kjv_tabs(&h), 2);
    assert_eq!(h.panels().len(), panels + 1);
    drop(h);
    drop(files);
}

#[test]
fn query_errors_show_inline_under_the_box() {
    let (mut h, files) = harness("errors", 3);
    type_query(&mut h, "strong:X1");
    h.frames(2);
    let message = "“X1” is not a Strong's number (try G5485 or H1254A)";
    assert!(h.find(message).is_some(), "{:?}", h.texts());
    // Library mode is not built yet.
    // The mode switch's "Library" is the lowest one (below the menus and tab bars).
    let library = h
        .shapes
        .iter()
        .filter(|(_, t)| t == "Library")
        .map(|(r, _)| *r)
        .max_by(|a, b| a.top().total_cmp(&b.top()))
        .unwrap();
    h.click(library.center());
    h.frames(2);
    assert!(
        h.find("Library search arrives with #60").is_some(),
        "{:?}",
        h.texts()
    );
    drop(h);
    drop(files);
}

#[test]
fn word_info_search_this_word_searches_its_bible() {
    let (mut h, files) = harness("word-info", 3);
    let mut part = obs_model::MorphemePart::new("H1254A".parse().unwrap());
    part.lemma = Some("בָּרָא".into());
    let mut word =
        obs_engine::ShownWord::new("בָּרָ֣א", true, "Genesis 1:1 (TAHOT)".into(), &[part], |_| None)
            .unwrap();
    word.bible = Some("tahot".into());
    h.shell.st.session.word_info.pin(word);
    let p = h.tab_at("Word Info");
    h.click(p);
    // The card's word and Lemma lay out off-thread; click once the buttons have settled.
    for _ in 0..500 {
        h.frame();
        if !h.shell.st.snippets.has_pending() {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    h.frames(2);
    h.click_text("Search this word");
    settle(&mut h);
    for text in ["2 verses · TAHOT 2", "Genesis 1:1", "Genesis 1:27"] {
        assert!(
            h.find(text).is_some(),
            "{text:?} not drawn: {:?}",
            h.texts()
        );
    }
    // One highlight each: בָּרָא in 1:1, and only the root of וַיִּבְרָא in 1:27.
    assert_eq!(rects_of(&h, h.shell.st.palette.search_hit).len(), 2);
    drop(h);
    drop(files);
}

/// #19: the first results page is drawn ≤ 150 ms after the results arrive. The search job
/// runs first; the clock starts at the frame that receives its results and stops when every
/// row in view is drawn with its text (no placeholders).
#[test]
fn the_first_results_page_draws_within_150_ms_of_arriving() {
    let (mut h, files) = harness("timing", 400);
    h.shell.run("tool.search", &json!({}));
    h.frames(2);
    let tab = search_tab(&h);
    h.shell.run(
        "search.run",
        &json!({ "tab": tab.0, "text": "law", "bibles": "psalms" }),
    );
    // The search itself (Inline mode: it runs here, now).
    while h.shell.st.session.jobs().pump(16) > 0 {}
    let start = Instant::now();
    let mut frames = 0;
    let elapsed = loop {
        h.frame();
        frames += 1;
        let view = h.shell.st.tabs[&tab].search.as_ref().unwrap();
        if view.last.drawn > 0 && view.last.placeholders == 0 {
            break start.elapsed();
        }
        assert!(start.elapsed() < Duration::from_secs(10), "never drew");
        std::thread::sleep(Duration::from_micros(500));
    };
    let view = h.shell.st.tabs[&tab].search.as_ref().unwrap();
    let rows = view.last.visible.len();
    eprintln!("first results page: {rows} rows drawn {elapsed:?} after arriving ({frames} frames)");
    assert!(rows >= 5, "a page of rows: {rows}");
    assert_eq!(
        h.shell
            .st
            .session
            .search_tab(tab)
            .unwrap()
            .results
            .as_ref()
            .unwrap()
            .rows
            .len(),
        400
    );
    assert!(
        elapsed <= Duration::from_millis(150),
        "first page took {elapsed:?}"
    );
    drop(h);
    drop(files);
}

/// The Starter library's folder, if this machine has one.
fn starter_library() -> Option<PathBuf> {
    let dir = obs_engine::default_library_dir()?;
    dir.join("kjv-strongs.obr").is_file().then_some(dir)
}

/// The #19 measurement against the real Starter KJV and WEB (and TAHOT for Strong's): search
/// time and first-page draw time. Search index caches are built once into a temp folder
/// that is kept between runs. Run with
/// `cargo test --release -p obs-ui-egui real_library_search_timing -- --ignored --nocapture`.
#[test]
#[ignore = "needs the Starter library in the platform data folder"]
fn real_library_search_timing() {
    let Some(dir) = starter_library() else {
        eprintln!("no Starter library: skipped");
        return;
    };
    let mut h = Harness::new();
    let s = &mut h.shell.st.session;
    s.library = Library::new();
    for f in ["kjv-strongs.obr", "web.obr", "tahot.obr"] {
        s.library.add_file(&dir.join(f)).unwrap();
    }
    let cache: &Path = &std::env::temp_dir().join("obs-real-search-cache");
    s.set_search_cache_dir(cache.to_path_buf());
    let ready = Instant::now();
    s.refresh_search_indexes();
    // The Bibles' outlines are read ahead as soon as their indexes are Ready.
    settle(&mut h);
    eprintln!(
        "indexes ready and outlines of KJV, WEB and TAHOT read ahead in {:?}",
        ready.elapsed()
    );
    h.shell.run("tool.search", &json!({}));
    h.frames(2);
    let tab = search_tab(&h);
    let ids: Vec<String> = ["kjv-strongs", "web", "tahot"]
        .iter()
        .map(|f| {
            h.shell
                .st
                .session
                .library
                .entries()
                .iter()
                .find(|e| e.path.file_stem().is_some_and(|s| s == *f))
                .map(|e| e.manifest.resource_id.clone())
                .unwrap()
        })
        .collect();
    let bibles = format!("{},{}", ids[0], ids[1]);
    for (query, bibles) in [
        ("\"love thy neighbour\"", bibles.as_str()),
        ("love", bibles.as_str()),
        ("strong:H1254", ids[2].as_str()),
    ] {
        // Twice: the first run of the query, and again.
        for run in ["first", "again"] {
            h.shell.run(
                "search.run",
                &json!({ "tab": tab.0, "text": query, "bibles": bibles }),
            );
            let searched = Instant::now();
            while h.shell.st.session.jobs().pump(16) > 0 {}
            let search_ms = searched.elapsed();
            let start = Instant::now();
            loop {
                h.frame();
                let view = h.shell.st.tabs[&tab].search.as_ref().unwrap();
                if view.last.drawn > 0 && view.last.placeholders == 0 {
                    break;
                }
                assert!(start.elapsed() < Duration::from_secs(10), "never drew");
                std::thread::sleep(Duration::from_micros(500));
            }
            let st = h.shell.st.session.search_tab(tab).unwrap();
            let r = st.results.as_ref().unwrap();
            eprintln!(
                "{query} ({run}): {} — search job {search_ms:?} (search {} ms, the rest is the first page of snippets), first page drawn {:?} after arriving",
                r.summary(),
                r.elapsed_ms,
                start.elapsed()
            );
            assert!(!r.rows.is_empty());
        }
    }
    drop(h);
}
