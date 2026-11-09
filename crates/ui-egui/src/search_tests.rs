//! The Search Tool (#59, #60) through the shell's real-event headless harness: typing a query
//! and Enter, the Bible and Resource pickers, the summary line, highlighted Verse rows and
//! Library rows grouped by Resource, ↑/↓/Enter and Ctrl/Cmd-click to open hits (a Library
//! hit at its paragraph), and how soon the first results page draws.

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
    harness_of(
        name,
        &[
            bibles::kjv(),
            bibles::web(),
            bibles::psalms(psalm_verses),
            bibles::tahot(),
        ],
    )
}

/// The Harness with a library of `resources` (in this Preferred order) written as real
/// `.obr` files, their Search indexes built.
fn harness_of(name: &str, resources: &[obs_model::Resource]) -> (Harness, Files) {
    let root = std::env::temp_dir().join(format!("obs-ui-search-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let files = Files(root.clone());
    bibles::write_library(&root.join("library"), resources);
    let mut h = Harness::new();
    let s = &mut h.shell.st.session;
    s.library = Library::new();
    assert!(s.library.scan(&root.join("library")).is_empty());
    s.library.set_preferred_order(
        resources
            .iter()
            .map(|r| r.manifest.resource_id.clone())
            .collect(),
    );
    s.set_search_cache_dir(root.join("cache"));
    s.refresh_search_indexes();
    settle(&mut h);
    (h, files)
}

/// KJV, two Commentaries (TCC ★, TC2), a Lexicon (TLX) and a Dictionary (TBD).
fn library_harness(name: &str) -> (Harness, Files) {
    use obs_engine::testing::study;
    harness_of(
        name,
        &[
            bibles::kjv(),
            study::tcc(),
            study::tc2(),
            study::tlx(),
            study::tbd(),
        ],
    )
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
    type_query_in(h, obs_engine::SearchMode::Bible, query);
}

/// Click a mode of the Search Tool's switch (the lowest text of that name: below the menus
/// and tab bars).
fn switch_mode(h: &mut Harness, label: &str) {
    let at = h
        .shapes
        .iter()
        .filter(|(_, t)| t == label)
        .map(|(r, _)| *r)
        .max_by(|a, b| a.top().total_cmp(&b.top()))
        .unwrap();
    h.click(at.center());
    h.frames(2);
}

/// [`type_query`] in a mode (switched to with the switch if the Tab is in another one).
fn type_query_in(h: &mut Harness, mode: obs_engine::SearchMode, query: &str) {
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
    let tab = search_tab(h);
    if h.shell.st.session.search_tab(tab).unwrap().mode != mode {
        switch_mode(h, mode.label());
    }
    let hint = super::super::search_tab::query_hint(mode);
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
    // Notes mode (#64) starts with its own prompt.
    switch_mode(&mut h, "Notes");
    assert!(
        h.find("Search the text of your Notes, then press Enter.")
            .is_some(),
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

#[test]
fn covenant_in_library_mode_draws_commentary_and_dictionary_hits_grouped_by_resource() {
    let (mut h, files) = library_harness("library-covenant");
    // Tall enough for all three groups' first rows at the default Text size (#241).
    h.screen = vec2(1400.0, 1100.0);
    type_query_in(&mut h, obs_engine::SearchMode::Library, "covenant");
    settle(&mut h);
    let tab = search_tab(&h);
    let st = h.shell.st.session.search_tab(tab).unwrap();
    assert_eq!(st.mode, obs_engine::SearchMode::Library);
    assert_eq!(st.query, "covenant");
    // The default scope: every Commentary, Lexicon and Dictionary, as chips.
    for text in [
        "Commentaries",
        "Lexicons",
        "Dictionaries",
        "9 paragraphs in 4 Resources (4 searched)",
        "TCC — Test Covenant Commentary · 3 hits",
        "TC2 — Second Test Commentary · 1 hit",
        "TLX — Test Lexicon · 2 hits",
    ] {
        assert!(
            h.find(text).is_some(),
            "{text:?} not drawn: {:?}",
            h.texts()
        );
    }
    // Group headers top to bottom in Preferred order, Commentaries first.
    let tcc = h.find("TCC — Test Covenant Commentary · 3 hits").unwrap();
    let tc2 = h.find("TC2 — Second Test Commentary · 1 hit").unwrap();
    let tlx = h.find("TLX — Test Lexicon · 2 hits").unwrap();
    assert!(tcc.top() < tc2.top() && tc2.top() < tlx.top());
    let view = h.shell.st.tabs[&tab].search.as_ref().unwrap();
    assert_eq!(view.last.placeholders, 0, "every row in view drawn");
    assert!(view.last.drawn >= 4, "{:?}", view.last);
    // A highlight under "covenant" in every row in view.
    let hits = rects_of(&h, h.shell.st.palette.search_hit);
    for (i, r) in &view.row_rects {
        assert!(
            hits.iter().any(|x| r.contains(x.center())),
            "row {i} has no highlight"
        );
    }
    // The Dictionary group is further down: ↓ moves through the rows (TCC 3, TC2 1, TLX 2)
    // to its first one, which is scrolled into view with its header.
    for _ in 0..6 {
        key(&mut h, egui::Key::ArrowDown, Modifiers::NONE);
    }
    settle(&mut h);
    assert_eq!(
        h.shell.st.session.search_tab(tab).unwrap().selected,
        Some(6)
    );
    assert!(
        h.find("TBD — Test Bible Dictionary · 3 hits").is_some(),
        "{:?}",
        h.texts()
    );
    // The picker leaves Lexicons out and the search runs again.
    h.click_text("Choose…");
    h.click_text("All Lexicons");
    settle(&mut h);
    let r = h
        .shell
        .st
        .session
        .search_tab(tab)
        .unwrap()
        .library_results
        .clone()
        .unwrap();
    let abbrs: Vec<&str> = r.groups.iter().map(|g| g.abbreviation.as_str()).collect();
    assert_eq!(abbrs, ["TCC", "TC2", "TBD"]);
    assert_eq!(
        h.shell
            .st
            .session
            .recent_searches(obs_engine::SearchMode::Library),
        ["covenant"]
    );
    drop(h);
    drop(files);
}

#[test]
fn clicking_a_library_hit_opens_its_unit_scrolled_to_the_paragraph() {
    let (mut h, files) = library_harness("library-open");
    type_query_in(&mut h, obs_engine::SearchMode::Library, "furnace");
    settle(&mut h);
    let tab = search_tab(&h);
    let hit = h
        .shell
        .st
        .session
        .search_tab(tab)
        .unwrap()
        .library_results
        .clone()
        .unwrap()
        .rows[0]
        .clone();
    let row = h.shell.st.tabs[&tab].search.as_ref().unwrap().row_rects[0].1;
    h.click(row.center());
    settle(&mut h);
    let tcc = h.tab_id("TCC");
    let gen15_1 = obs_scripture::Verse::new(obs_scripture::Book::Genesis, 15, 1);
    assert_eq!(
        h.shell
            .st
            .session
            .workspace
            .tab(tcc)
            .unwrap()
            .history
            .current(),
        Some(gen15_1)
    );
    let panel = h
        .panels()
        .into_iter()
        .find(|(_, titles)| titles.iter().any(|t| t == "TCC"))
        .unwrap()
        .0;
    let check = |h: &mut Harness| {
        let view = h
            .shell
            .st
            .tabs
            .get_mut(&tcc)
            .unwrap()
            .view
            .as_mut()
            .unwrap();
        let unit = view
            .doc
            .units
            .iter()
            .position(|u| u.id == hit.unit)
            .unwrap();
        // The opened Tab shows the hit's unit at the top…
        assert_eq!(view.top_unit(), Some(unit));
        // …with the paragraph scrolled to the top of the view, the unit's heading and its
        // first paragraphs above it.
        let top = view
            .text_top(unit, hit.range.start as usize, panel.height())
            .expect("the paragraph is in view");
        assert!((0.0..=4.0).contains(&top), "paragraph drawn at {top}");
        assert!(view.text_top(unit, 0, panel.height()).is_none());
        let flash = h.shell.st.tabs[&tcc].flash.clone();
        assert!(
            flash.is_none() || matches!(flash, Some((crate::pane::Flash::Text { .. }, _))),
            "{flash:?}"
        );
    };
    check(&mut h);
    // Again from the Search Tab: the same TCC Tab goes there (no second Tab).
    h.shell.st.session.workspace.focus(tab);
    h.shell.apply_effects();
    h.frames(2);
    let row = h.shell.st.tabs[&tab].search.as_ref().unwrap().row_rects[0].1;
    h.click(row.center());
    settle(&mut h);
    let tcc_tabs = h
        .shell
        .st
        .session
        .workspace
        .tabs()
        .filter(|t| t.kind == TabKind::Resource("tcc".into()))
        .count();
    assert_eq!(tcc_tabs, 1);
    check(&mut h);
    drop(h);
    drop(files);
}

/// The Library search preview (synthetic open-text fixtures only). Run with
/// `UPDATE_PREVIEWS=1 cargo test -p obs-ui-egui library_search_previews`; writes
/// `target/previews/search-library/*.ppm`.
#[test]
fn library_search_previews() {
    if std::env::var_os("UPDATE_PREVIEWS").is_none() {
        return;
    }
    let (mut h, files) = library_harness("library-previews");
    type_query_in(&mut h, obs_engine::SearchMode::Library, "covenant");
    settle(&mut h);
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/previews/search-library");
    std::fs::create_dir_all(&dir).unwrap();
    h.screenshot(&dir.join("results.ppm"));
    let tab = search_tab(&h);
    let row = h.shell.st.tabs[&tab].search.as_ref().unwrap().row_rects[1].1;
    h.click(row.center());
    settle(&mut h);
    h.screenshot(&dir.join("opened.ppm"));
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

/// Visual evidence for `docs/previews/<name>.png` when `UPDATE_PREVIEWS` is set.
fn preview(h: &Harness, name: &str) {
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
        std::fs::create_dir_all(&dir).unwrap();
        h.screenshot(&dir.join(format!("{name}.ppm")));
    }
}

/// KJV (★, Strong's numbers only) and TGNT (Lemma and Morphology), both with `strong:G26`
/// four times in John and 1 John, and the Test Lexicon; `strong:G26` searched in both.
fn agape_search(name: &str) -> (Harness, Files, TabId) {
    use obs_engine::testing::{greek_agape, kjv_agape, study};
    let (mut h, files) = harness_of(name, &[kjv_agape(), greek_agape(), study::tlx()]);
    h.shell.run("tool.search", &json!({}));
    h.frames(2);
    h.click_text("+ Bible");
    h.click_text("TGNT — Tagged Greek New Testament (test)");
    type_query(&mut h, "strong:G26");
    settle(&mut h);
    let tab = search_tab(&h);
    // The Search Tab alone in the window, for room to show the views.
    let others: Vec<TabId> = h
        .shell
        .st
        .session
        .workspace
        .tabs()
        .filter(|t| t.id != tab)
        .map(|t| t.id)
        .collect();
    for t in others {
        h.shell.run("tab.close", &json!({ "tab": t.0 }));
    }
    h.frames(3);
    assert!(
        h.find("10 verses · 13 hits · KJV 10 · TGNT 3").is_some(),
        "{:?}",
        h.texts()
    );
    (h, files, tab)
}

/// The lowest text of that name (below the menus, tab bars and Bible chips).
fn lowest(h: &Harness, text: &str) -> Rect {
    h.shapes
        .iter()
        .filter(|(_, t)| t == text)
        .map(|(r, _)| *r)
        .max_by(|a, b| a.top().total_cmp(&b.top()))
        .unwrap_or_else(|| panic!("{text:?} not on screen: {:?}", h.texts()))
}

/// Draw until the text pipeline's layouts have arrived.
fn draw_laid(h: &mut Harness) {
    for _ in 0..40 {
        h.frame();
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn strong_g26_grid_shows_john_and_1_john_hottest_and_a_cell_filters_the_verse_list() {
    let (mut h, files, tab) = agape_search("grid");
    for view in ["Verses", "Grid", "Chart", "Analysis"] {
        assert!(h.find(view).is_some(), "{view} switch: {:?}", h.texts());
    }
    switch_mode(&mut h, "Grid");
    let st = h.shell.st.session.search_tab(tab).unwrap();
    assert_eq!(st.view, obs_engine::ResultsView::Grid);
    for text in [
        "Bible book",
        "John",
        "Romans",
        "1 Corinthians",
        "1 John",
        "TGNT",
    ] {
        assert!(h.find(text).is_some(), "{text} not drawn: {:?}", h.texts());
    }
    // John × KJV and 1 John × KJV are the two hottest cells.
    let below = lowest(&h, "Bible book").top();
    let hottest: Vec<Rect> = rects_of(&h, super::super::search_views::heat_fill(1.0))
        .into_iter()
        .filter(|r| r.top() > below)
        .collect();
    assert_eq!(hottest.len(), 2, "{hottest:?}");
    let kjv = lowest(&h, "KJV");
    let john = lowest(&h, "John");
    let first_john = lowest(&h, "1 John");
    for (row, r) in [(john, hottest[0]), (first_john, hottest[1])] {
        assert!(
            (r.center().y - row.center().y).abs() < 4.0,
            "{r:?} not on {row:?}"
        );
        assert!(
            (r.center().x - kjv.center().x).abs() < 4.0,
            "{r:?} not under KJV"
        );
    }
    preview(&h, "search-grid-g26");

    // Click 1 John × KJV: the Verse list shows 1 John's KJV Verses only.
    h.click(egui::pos2(kjv.center().x, first_john.center().y));
    settle(&mut h);
    let st = h.shell.st.session.search_tab(tab).unwrap();
    assert_eq!(st.view, obs_engine::ResultsView::Verses);
    assert_eq!(st.row_count(), 4);
    for text in [
        "Only 1 John · KJV",
        "4 verses · 4 hits · KJV 4 · TGNT 0",
        "1 John 4:7",
        "1 John 4:8",
        "1 John 4:16",
        "1 John 4:18",
    ] {
        assert!(h.find(text).is_some(), "{text} not drawn: {:?}", h.texts());
    }
    assert!(h.find("John 13:35").is_none());
    let view = h.shell.st.tabs[&tab].search.as_ref().unwrap();
    assert_eq!(view.last.visible, 0..4);
    preview(&h, "search-grid-filtered");
    // × on the chip shows every result again.
    let chip = h.find("Only 1 John · KJV").unwrap();
    let x = h
        .shapes
        .iter()
        .filter(|(r, t)| {
            t == "[x]" && r.left() > chip.right() && (r.center().y - chip.center().y).abs() < 6.0
        })
        .map(|(r, _)| r.center())
        .next()
        .expect("the chip's ×");
    h.click(x);
    settle(&mut h);
    assert_eq!(h.shell.st.session.search_tab(tab).unwrap().row_count(), 10);
    assert!(h.find("John 13:35").is_some(), "{:?}", h.texts());
    drop(h);
    drop(files);
}

#[test]
fn chart_bars_by_book_and_by_chapter_and_a_bar_filters_the_verse_list() {
    let (mut h, files, tab) = agape_search("chart");
    switch_mode(&mut h, "Chart");
    for text in ["By book", "John", "Romans", "1 Corinthians", "1 John"] {
        assert!(h.find(text).is_some(), "{text} not drawn: {:?}", h.texts());
    }
    // The bars: John and 1 John (4 Verses each) twice as long as… four times Romans'.
    let lapis = crate::theme::c().lapis;
    let below = lowest(&h, "By book").bottom();
    let bars_of = |h: &Harness| -> Vec<Rect> {
        rects_of(h, lapis)
            .into_iter()
            .filter(|r| r.top() > below)
            .collect()
    };
    let mut bars = bars_of(&h);
    bars.sort_by(|a, b| a.top().total_cmp(&b.top()));
    assert_eq!(bars.len(), 4, "{bars:?}");
    let w: Vec<f32> = bars.iter().map(|r| r.width()).collect();
    assert!(
        (w[0] - w[3]).abs() < 0.5 && (w[0] - 4.0 * w[1]).abs() < 1.0,
        "{w:?}"
    );
    preview(&h, "search-chart-g26");
    // Chapters of John: 21 bars, three of them with Verses.
    h.shell
        .run("search.chart", &json!({ "tab": tab.0, "value": "John" }));
    h.frames(3);
    assert!(h.find("Chapters of John").is_some(), "{:?}", h.texts());
    assert!(h.find("John 21").is_some(), "{:?}", h.texts());
    assert_eq!(bars_of(&h).iter().filter(|r| r.width() > 0.5).count(), 3);
    h.click_text("John 15");
    settle(&mut h);
    let st = h.shell.st.session.search_tab(tab).unwrap();
    assert_eq!(st.view, obs_engine::ResultsView::Verses);
    assert!(h.find("Only John 15").is_some(), "{:?}", h.texts());
    assert!(h.find("John 15:9").is_some() && h.find("John 15:13").is_some());
    assert!(h.find("John 13:35").is_none());
    drop(h);
    drop(files);
}

#[test]
fn analysis_table_sorts_by_each_column_header_and_opens_a_row() {
    let (mut h, files, tab) = agape_search("analysis");
    switch_mode(&mut h, "Analysis");
    settle(&mut h);
    draw_laid(&mut h);
    for text in [
        "17 matched words",
        "Export CSV…",
        "Reference",
        "Bible",
        "Word",
        "Lemma",
        "Morphology",
        "Context",
        "John 13:35",
        "noun, acc. fem. sing.",
    ] {
        assert!(h.find(text).is_some(), "{text} not drawn: {:?}", h.texts());
    }
    // The words, Lemmas and Context are Resource text, drawn by the text pipeline.
    assert!(!h.glyphs.is_empty());
    preview(&h, "search-analysis-g26");
    let sort = |h: &Harness| h.shell.st.session.search_tab(tab).unwrap().analysis_sort;
    assert_eq!(sort(&h), obs_engine::AnalysisSort::default());
    for column in obs_engine::AnalysisColumn::ALL {
        // Click the header: ascending (Reference, already ascending, turns round) …
        let at = lowest(&h, column.label()).center();
        h.click(at);
        let reference = column == obs_engine::AnalysisColumn::Reference;
        assert_eq!(sort(&h).column, column);
        assert_eq!(sort(&h).descending, reference, "{column:?}");
        // … and again: the other way.
        h.click(at);
        assert_eq!(sort(&h).column, column);
        assert_eq!(sort(&h).descending, !reference, "{column:?}");
    }
    // Morphology in plain English, descending: nominative rows above dative above
    // accusative. No raw code is drawn in the table.
    let at = lowest(&h, "Morphology").center();
    h.click(at);
    h.click(at);
    assert!(sort(&h).descending);
    let (nom, dat, acc) = (
        "noun, nom. fem. sing.",
        "noun, dat. fem. sing.",
        "noun, acc. fem. sing.",
    );
    let top = |t: &str| h.find(t).map(|r| r.top()).unwrap();
    assert!(top(nom) < top(dat) && top(dat) < top(acc));
    for code in ["N-NSF", "N-DSF", "N-ASF"] {
        assert!(h.find(code).is_none(), "raw {code} drawn");
    }
    // Resting on the Morphology cell (a second of frames) shows the code as the Bible tags
    // it.
    let p = h.at(dat);
    h.events.push(egui::Event::PointerMoved(p));
    h.frames(60);
    assert!(
        h.texts()
            .iter()
            .any(|t| t.contains("Morphology code: N-DSF")),
        "no code tooltip: {:?}",
        h.texts()
    );
    // Clicking a row opens its Verse.
    h.click_text(dat);
    h.frames(3);
    let tgnt = h.tab_id("TGNT");
    assert_eq!(
        h.shell
            .st
            .session
            .workspace
            .tab(tgnt)
            .unwrap()
            .history
            .current(),
        Some(obs_scripture::Verse::new(
            obs_scripture::Book::FirstJohn,
            4,
            16
        ))
    );
    drop(h);
    drop(files);
}

/// Pick `value` in the Morphology chooser's `category` menu: its combo box is the first one
/// right of the category's label.
fn choose_morphology(h: &mut Harness, category: &str, value: &str) {
    let label = lowest(h, category);
    let combo = h
        .shapes
        .iter()
        .filter(|(r, _)| (r.center().y - label.center().y).abs() < 6.0 && r.left() > label.right())
        .map(|(r, _)| *r)
        .min_by(|a, b| a.left().total_cmp(&b.left()))
        .unwrap_or_else(|| panic!("no menu after {category:?}: {:?}", h.texts()));
    h.click(combo.center());
    h.click_text(value);
}

/// The Morphology chooser (#145) builds `@verb aorist imperative` from plain-English menus
/// and searches it, finding what the same criteria typed find; added after a Strong's number
/// it describes that word.
#[test]
fn the_morphology_chooser_builds_the_query_and_searches_it() {
    let (mut h, files) = harness_of("morph-chooser", &[obs_engine::testing::tagnt_romans()]);
    h.shell.run("tool.search", &json!({}));
    h.frames(2);
    let tab = search_tab(&h);
    let others: Vec<TabId> = h
        .shell
        .st
        .session
        .workspace
        .tabs()
        .filter(|t| t.id != tab)
        .map(|t| t.id)
        .collect();
    for t in others {
        h.shell.run("tab.close", &json!({ "tab": t.0 }));
    }
    h.frames(3);
    h.click_text("Morphology…");
    assert!(h.find("Choose a part of speech, tense, case …").is_some());
    for (category, value) in [
        ("Part of speech", "verb"),
        ("Tense", "aorist"),
        ("Mood", "imperative"),
    ] {
        choose_morphology(&mut h, category, value);
    }
    assert!(
        h.find("@verb aorist imperative").is_some(),
        "{:?}",
        h.texts()
    );
    h.click_text("Add and search");
    settle(&mut h);
    draw_laid(&mut h);
    preview(&h, "search-morphology-chooser-kittest");
    let st = h.shell.st.session.search_tab(tab).unwrap().clone();
    assert_eq!(st.query, "@verb aorist imperative");
    let refs = |st: &obs_engine::SearchTab| {
        let r = st.results.clone().expect("results");
        r.rows
            .iter()
            .map(|row| r.row_reference(row))
            .collect::<Vec<_>>()
    };
    let chosen = refs(&st);
    assert_eq!(
        chosen,
        ["John 15:9", "Romans 6:13", "Romans 13:14", "Romans 16:16"]
    );
    assert!(h.find("4 verses · TAGNT 4").is_some(), "{:?}", h.texts());
    // The same criteria typed another way.
    h.shell.run(
        "search.run",
        &json!({ "tab": tab.0, "text": "@imperative aorist verb" }),
    );
    settle(&mut h);
    assert_eq!(refs(h.shell.st.session.search_tab(tab).unwrap()), chosen);

    // After a Strong's number, the chooser's criteria describe that word.
    h.shell
        .run("search.run", &json!({ "tab": tab.0, "text": "G3936" }));
    settle(&mut h);
    choose_morphology(&mut h, "Tense", "present");
    h.click_text("Add to query");
    assert!(
        h.find("G3936 @verb present imperative").is_some(),
        "{:?}",
        h.texts()
    );
    h.click_text("Search");
    settle(&mut h);
    let st = h.shell.st.session.search_tab(tab).unwrap();
    assert_eq!(st.query, "G3936 @verb present imperative");
    assert_eq!(refs(st), ["Romans 6:13"]);
    drop(h);
    drop(files);
}

/// Word forms (#180): the toggle beside the query, off by default, reruns the search with
/// every form of its English words, highlighted ("work" → "worketh").
#[test]
fn the_word_forms_toggle_reruns_the_search_with_every_form() {
    let (mut h, files) = harness("word-forms", 3);
    type_query(&mut h, "work");
    settle(&mut h);
    let tab = search_tab(&h);
    assert!(!h.shell.st.session.search_word_forms());
    assert!(h.find("0 verses · KJV 0").is_some(), "{:?}", h.texts());
    h.click_text(super::super::search_tab::WORD_FORMS);
    h.frames(2);
    settle(&mut h);
    assert!(h.shell.st.session.search_word_forms());
    let r = h
        .shell
        .st
        .session
        .search_tab(tab)
        .unwrap()
        .results
        .clone()
        .unwrap();
    assert_eq!(r.rows.len(), 1);
    for text in ["1 verse · KJV 1", "Romans 13:10"] {
        assert!(
            h.find(text).is_some(),
            "{text:?} not drawn: {:?}",
            h.texts()
        );
    }
    let snippet = h
        .shell
        .st
        .session
        .search_tab(tab)
        .unwrap()
        .snippet(0, 0)
        .unwrap()
        .clone();
    let marked: Vec<&str> = snippet
        .highlights
        .iter()
        .map(|r| &snippet.text[r.clone()])
        .collect();
    assert_eq!(marked, ["worketh"]);
    assert!(!rects_of(&h, h.shell.st.palette.search_hit).is_empty());
    preview(&h, "search-word-forms");
    // A proximity search with it on: "worketh no ill to his neighbour".
    h.shell.run(
        "search.run",
        &json!({ "tab": tab.0, "text": "work NEAR/5 neighbour" }),
    );
    settle(&mut h);
    assert!(h.find("Romans 13:10").is_some(), "{:?}", h.texts());
    // Off again: back to the word as typed.
    h.shell
        .run("search.run", &json!({ "tab": tab.0, "text": "work" }));
    settle(&mut h);
    h.click_text(super::super::search_tab::WORD_FORMS);
    h.frames(2);
    settle(&mut h);
    assert!(!h.shell.st.session.search_word_forms());
    assert!(h.find("0 verses · KJV 0").is_some(), "{:?}", h.texts());
    drop(h);
    drop(files);
}
