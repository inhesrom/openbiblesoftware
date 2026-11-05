//! The right-click menu (#142) through the shell's real-event headless harness: each target
//! type (Selection, Verse, Lemma, Strong's number, Morphology, an untagged English word), the
//! keyboard (Shift+F10, the Menu key), and the Text Comparison and Search menus.

use obs_engine::{Guide, Library, TabId};
use obs_model::Key;
use obs_scripture::{Book, Verse};

use super::*;

struct Files(std::path::PathBuf);
impl Drop for Files {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A library of real `.obr` files: KJV John with Strong's numbers, TAGNT John 3:16 and a
/// Greek Lexicon.
fn setup(h: &mut Harness, name: &str) -> Files {
    setup_with(h, name, &[])
}

/// [`setup`]'s library and `more`.
fn setup_with(h: &mut Harness, name: &str, more: &[obs_model::Resource]) -> Files {
    let dir = std::env::temp_dir().join(format!(
        "obs-shell-context-menu-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let files = Files(dir);
    h.shell.st.session.library = Library::new();
    let base = [
        obs_engine::testing::kjv_john(),
        obs_engine::testing::tagnt_john_3_16(),
        obs_engine::testing::greek_lexicon(),
    ];
    for r in base.iter().chain(more) {
        let path = files.0.join(format!("{}.obr", r.manifest.resource_id));
        obs_store::write_obr(r, &path).unwrap();
        h.shell.st.session.library.add_file(&path).unwrap();
    }
    h.shell
        .st
        .session
        .set_search_cache_dir(files.0.join("cache"));
    h.shell.st.session.refresh_search_indexes();
    h.shell.reset_layout();
    settle_search(h);
    files
}

/// Run queued engine jobs and draw until no Search index is building and no search running.
fn settle_search(h: &mut Harness) {
    for _ in 0..2000 {
        h.shell.st.session.jobs().pump(16);
        h.frame();
        let s = &h.shell.st.session;
        if !s.search.is_building() && !s.bible_search_running() {
            h.frames(2);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("never settled");
}

/// Open a Bible at John 3:16 in its own Panel and let its text lay out.
fn open_at_john_3_16(h: &mut Harness, id: &str) -> TabId {
    let tab = h
        .shell
        .run("resource.open", &json!({ "id": id, "split": true }))
        .unwrap()["tab"]
        .as_u64()
        .map(TabId)
        .unwrap();
    h.frames(2);
    h.shell
        .run("tab.go", &json!({ "tab": tab.0, "text": "Jn 3:16" }));
    settle_all(h);
    tab
}

fn right_click(h: &mut Harness, p: Pos2) {
    h.events.push(Event::PointerMoved(p));
    h.frame();
    for pressed in [true, false] {
        h.events.push(Event::PointerButton {
            pos: p,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.frame();
    }
    h.frame();
}

fn press(h: &mut Harness, key: egui::Key, modifiers: Modifiers) {
    h.events.push(Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    });
    h.frames(2);
}

/// Rest the pointer on a menu row (hovering a target chooses it).
fn hover(h: &mut Harness, text: &str) {
    let p = h.at(text);
    h.events.push(Event::PointerMoved(p));
    h.frames(2);
}

fn menu_open(h: &Harness) -> bool {
    egui::Popup::is_any_open(&h.ctx)
}

fn focused_kind(h: &Harness) -> TabKind {
    let ws = &h.shell.session().workspace;
    ws.tab(ws.focused().unwrap()).unwrap().kind.clone()
}

fn focused_key(h: &Harness) -> Option<Key> {
    let ws = &h.shell.session().workspace;
    ws.tab(ws.focused().unwrap())
        .unwrap()
        .entry_history
        .current()
}

fn search_query(h: &Harness) -> (String, Option<Vec<String>>) {
    let ws = &h.shell.session().workspace;
    let search = ws
        .tabs()
        .find(|t| t.kind == TabKind::Tool(Tool::Search))
        .map(|t| t.id)
        .expect("a Search Tab");
    let st = h.shell.session().search_tab(search).unwrap();
    (st.query.clone(), st.bibles.clone())
}

#[test]
fn right_click_loved_in_john_3_16_kjv_opens_word_study_on_g25_in_one_click() {
    let mut h = Harness::new();
    let _files = setup(&mut h, "kjv-loved");
    let kjv = open_at_john_3_16(&mut h, "kjv");
    let loved = super::word_info::word_spot(&h, kjv, "loved");
    right_click(&mut h, loved);
    assert!(menu_open(&h));
    // The left column: the selection, the Verse, the Lemma (from the Lexicon: KJV+Strong's
    // tags only numbers) and the Strong's number; no Morphology.
    for t in [
        "Selection",
        "“loved”",
        "Verse",
        "John 3:16",
        "Lemma",
        "ἀγαπάω",
        "Strong's number",
        "G25",
    ] {
        assert!(h.find(t).is_some(), "no {t}: {:?}", h.texts());
    }
    assert!(h.find("Morphology").is_none());
    // The word is chosen: Word Study is one click away.
    h.click_text("Word Study");
    assert!(!menu_open(&h));
    assert_eq!(focused_kind(&h), TabKind::Guide(Guide::WordStudy));
    assert_eq!(
        focused_key(&h),
        Some(Key::StrongsNumber("G25".parse().unwrap()))
    );
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        right_click(&mut h, loved);
        h.frames(10);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/previews/context-menu");
        h.screenshot(&dir.join("context-menu-kjv-loved.ppm"));
        press(&mut h, egui::Key::Escape, Modifiers::NONE);
        assert!(!menu_open(&h));
    }

    // Strong's: Search Bible for G25 searches this Bible.
    right_click(&mut h, loved);
    hover(&mut h, "G25");
    h.click_text("🔎 Search Bible for G25");
    let (query, bibles) = search_query(&h);
    assert_eq!(query, "strong:G25");
    assert_eq!(bibles.as_deref(), Some(&["kjv".to_string()][..]));

    // Open in preferred Lexicon: the Lexicon at G25.
    right_click(&mut h, loved);
    h.click_text("Open in preferred Lexicon");
    assert_eq!(focused_kind(&h), TabKind::Resource("lexicon".into()));
    assert_eq!(
        focused_key(&h),
        Some(Key::StrongsNumber("G25".parse().unwrap()))
    );
    drop(h);
}

#[test]
fn right_click_in_a_greek_nt_lists_lemma_strongs_and_morphology() {
    let mut h = Harness::new();
    let _files = setup(&mut h, "tagnt");
    let tagnt = open_at_john_3_16(&mut h, "tagnt");
    let word = super::word_info::word_spot(&h, tagnt, "ἠγάπησεν");
    right_click(&mut h, word);
    for t in ["Lemma", "ἀγαπάω", "Strong's number", "G25", "Morphology"] {
        assert!(h.find(t).is_some(), "no {t}: {:?}", h.texts());
    }
    let morphology = obs_engine::word_info::ShownWord::new(
        "ἠγάπησεν",
        false,
        String::new(),
        &[obs_model::MorphemePart {
            strongs: "G25".parse().unwrap(),
            lemma: Some("ἀγαπάω".into()),
            morph: Some("V-AAI-3S".into()),
            gloss: None,
        }],
        |_| None,
    )
    .unwrap()
    .main
    .morphology
    .unwrap();
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        h.frames(10);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/previews/context-menu");
        h.screenshot(&dir.join("context-menu-tagnt.ppm"));
    }
    assert!(
        h.find(&morphology).is_some(),
        "no {morphology}: {:?}",
        h.texts()
    );

    // Lemma: the right column searches by Lemma.
    hover(&mut h, "ἀγαπάω");
    h.click_text("🔎 Search Bible for ἀγαπάω");
    let (query, bibles) = search_query(&h);
    assert_eq!(query, "lemma:ἀγαπάω");
    assert_eq!(bibles.as_deref(), Some(&["tagnt".to_string()][..]));

    // Morphology: Pin in Word Info shows the word there, pinned.
    right_click(&mut h, word);
    hover(&mut h, &morphology);
    assert!(h.find("Copy").is_some(), "{:?}", h.texts());
    h.click_text("📌 Pin in Word Info");
    let info = &h.shell.session().word_info;
    assert!(info.pinned);
    assert_eq!(info.shown.as_ref().unwrap().written, "ἠγάπησεν");
    assert_eq!(focused_kind(&h), TabKind::Tool(Tool::WordInfo));
    drop(h);
}

#[test]
fn right_click_on_an_untagged_word_offers_the_english_word_chooser() {
    let mut h = Harness::new();
    let _files = setup(&mut h, "untagged");
    let kjv = open_at_john_3_16(&mut h, "kjv");
    // "should" carries no Strong's number in KJV+Strong's.
    let should = super::word_info::word_spot(&h, kjv, "should");
    right_click(&mut h, should);
    assert!(h.find("“should”").is_some(), "{:?}", h.texts());
    assert!(h.find("Strong's number").is_none(), "{:?}", h.texts());
    // The selection is chosen; Word Study… opens the English-word chooser.
    h.click_text("Word Study…");
    assert_eq!(focused_kind(&h), TabKind::Guide(Guide::WordStudy));
    let ws = &h.shell.session().workspace;
    let study = ws.focused().unwrap();
    assert_eq!(h.shell.session().word_study.choosing(study), Some("should"));

    // Search this searches the selected word in this Bible.
    right_click(&mut h, should);
    h.click_text("🔎 Search this");
    let (query, bibles) = search_query(&h);
    assert_eq!(query, "should");
    assert_eq!(bibles.as_deref(), Some(&["kjv".to_string()][..]));
    drop(h);
}

#[test]
fn the_verse_target_opens_passage_keyed_tools_at_the_verse() {
    let mut h = Harness::new();
    let _files = setup(&mut h, "verse");
    let kjv = open_at_john_3_16(&mut h, "kjv");
    let loved = super::word_info::word_spot(&h, kjv, "loved");
    for (action, kind) in [
        ("Passage Guide", TabKind::Guide(Guide::PassageGuide)),
        ("Cross-references", TabKind::Tool(Tool::CrossReferences)),
        ("Text Comparison", TabKind::Tool(Tool::TextComparison)),
    ] {
        right_click(&mut h, loved);
        hover(&mut h, "John 3:16");
        assert!(h.find("Copy with reference").is_some(), "{:?}", h.texts());
        let p = h
            .shapes
            .iter()
            .rev()
            .find(|(_, t)| t == action)
            .map(|(r, _)| r.center())
            .unwrap();
        h.click(p);
        assert_eq!(focused_kind(&h), kind, "{action}");
        let ws = &h.shell.session().workspace;
        let t = ws.tab(ws.focused().unwrap()).unwrap();
        assert_eq!(
            t.history.current(),
            Some(Verse::new(Book::John, 3, 16)),
            "{action}"
        );
        h.shell.session_mut().workspace.focus(kjv);
        h.frames(2);
    }
    drop(h);
}

#[test]
fn shift_f10_and_the_menu_key_open_the_menu_at_the_hovered_word_or_the_selection() {
    let mut h = Harness::new();
    let _files = setup(&mut h, "keys");
    let kjv = open_at_john_3_16(&mut h, "kjv");
    // Pointer on "loved", the KJV focused: Shift+F10 opens the menu on it.
    let loved = super::word_info::word_spot(&h, kjv, "loved");
    h.click(loved);
    assert_eq!(h.shell.session().workspace.focused(), Some(kjv));
    press(&mut h, egui::Key::F10, Modifiers::SHIFT);
    assert!(menu_open(&h));
    assert!(h.find("G25").is_some(), "{:?}", h.texts());
    assert!(h.shell.st.has_selection(kjv));
    press(&mut h, egui::Key::Escape, Modifiers::NONE);
    h.click(h.at("Word Info"));
    h.shell.session_mut().workspace.focus(kjv);
    h.frames(2);
    assert!(!menu_open(&h));

    // A selection and the pointer elsewhere: the Menu key opens it on the selection.
    let world = super::word_info::word_spot(&h, kjv, "world");
    h.time += 1.0;
    h.events.push(Event::PointerMoved(world));
    h.frame();
    for _ in 0..2 {
        h.button(world, true);
        h.frame();
        h.button(world, false);
        h.frame();
    }
    h.frames(2);
    assert!(h.shell.st.has_selection(kjv));
    h.events.push(Event::PointerMoved(pos2(5.0, 890.0)));
    h.frame();
    press(&mut h, egui::Key::Menu, Modifiers::NONE);
    assert!(menu_open(&h), "{:?}", h.texts());
    for t in ["“world”", "G2889", "John 3:16"] {
        assert!(h.find(t).is_some(), "no {t}: {:?}", h.texts());
    }
    h.click_text("Word Study");
    assert_eq!(
        focused_key(&h),
        Some(Key::StrongsNumber("G2889".parse().unwrap()))
    );
    drop(h);
}

#[test]
fn text_comparison_and_search_rows_offer_their_verse() {
    let mut h = Harness::new();
    let _files = setup(&mut h, "rows");
    let kjv = open_at_john_3_16(&mut h, "kjv");
    let _ = kjv;
    let comparison = h
        .shell
        .run("tool.text_comparison", &json!({ "passage": "Jn 3:16" }))
        .unwrap()["tab"]
        .as_u64()
        .map(TabId)
        .unwrap();
    settle_all(&mut h);
    let tiles = &h.shell.tree.tiles;
    let pane = tiles.rect(tiles.find_pane(&comparison).unwrap()).unwrap();
    // The Verse block's reference line (below the header box, which shows it too).
    let block = h
        .shapes
        .iter()
        .filter(|(r, t)| t == "John 3:16" && pane.contains(r.center()))
        .map(|(r, _)| *r)
        .max_by(|a, b| a.top().total_cmp(&b.top()))
        .unwrap_or_else(|| panic!("no John 3:16 block: {:?}", h.texts()));
    right_click(&mut h, block.center() + vec2(0.0, 30.0));
    assert!(menu_open(&h), "{:?}", h.texts());
    assert!(h.find("Copy with reference").is_some(), "{:?}", h.texts());
    h.click_text("Passage Guide");
    assert_eq!(focused_kind(&h), TabKind::Guide(Guide::PassageGuide));

    // A Search result row: its Verse.
    h.shell.run(
        "search.run",
        &json!({ "text": "strong:G25", "mode": "bible", "bibles": "kjv" }),
    );
    settle_search(&mut h);
    settle_all(&mut h);
    let search = h
        .shell
        .session()
        .workspace
        .tabs()
        .find(|t| t.kind == TabKind::Tool(Tool::Search))
        .map(|t| t.id)
        .unwrap();
    let row = h.shell.st.tabs[&search]
        .search
        .as_ref()
        .unwrap()
        .row_rects
        .first()
        .map(|(_, r)| *r)
        .unwrap_or_else(|| panic!("no result rows: {:?}", h.texts()));
    right_click(&mut h, row.center());
    assert!(menu_open(&h), "{:?}", h.texts());
    h.click_text("Cross-references");
    assert_eq!(focused_kind(&h), TabKind::Tool(Tool::CrossReferences));
    let ws = &h.shell.session().workspace;
    assert_eq!(
        ws.tab(ws.focused().unwrap()).unwrap().history.current(),
        Some(Verse::new(Book::John, 3, 16))
    );
    drop(h);
}

#[test]
fn the_verse_target_copies_with_reference_and_adds_a_note_on_the_verse() {
    let db = temp_db("context-menu-note");
    let mut h = Harness::launch(Some(&db));
    let _files = setup(&mut h, "note");
    let kjv = open_at_john_3_16(&mut h, "kjv");
    let loved = super::word_info::word_spot(&h, kjv, "loved");
    right_click(&mut h, loved);
    hover(&mut h, "John 3:16");
    h.click_text("Copy with reference");
    let copied = h.copied.last().cloned().unwrap_or_default();
    assert!(
        copied.starts_with("For God so loved the world")
            && copied.ends_with("everlasting life.\nJohn 3:16 (KJV)"),
        "{copied:?}"
    );

    right_click(&mut h, loved);
    hover(&mut h, "John 3:16");
    h.click_text("✏ Add Note on verse");
    assert!(h.shell.st.note_editor.is_some());
    h.events
        .push(Event::Text("God's love for the world".into()));
    h.frame();
    press(&mut h, egui::Key::Enter, Modifiers::COMMAND);
    assert!(h.shell.st.note_editor.is_none());
    let notes = h.shell.st.session.annotations(None).unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].body(), "God's love for the world");
    let Some(obs_engine::Anchor::Passage { passage, .. }) = notes[0].anchor else {
        panic!("{:?}", notes[0].anchor);
    };
    assert_eq!(
        passage,
        obs_scripture::Passage::verse(Verse::new(Book::John, 3, 16))
    );
    drop(h);
    let _ = std::fs::remove_dir_all(db.parent().unwrap());
}

#[test]
fn a_strongs_link_in_a_lexicon_is_a_strongs_target() {
    let mut h = Harness::new();
    let _files = setup_with(
        &mut h,
        "lexicon",
        &obs_engine::testing::starter_lexicon_excerpts(),
    );
    let tbesh = h
        .shell
        .run("resource.open", &json!({ "id": "tbesh", "split": true }))
        .unwrap()["tab"]
        .as_u64()
        .map(TabId)
        .unwrap();
    h.frames(2);
    h.shell
        .run("tab.go", &json!({ "tab": tbesh.0, "text": "H1H" }));
    settle_all(&mut h);
    let link = super::word_info::word_spot(&h, tbesh, "H2438H");
    right_click(&mut h, link);
    for t in ["Strong's number", "H2438H"] {
        assert!(h.find(t).is_some(), "no {t}: {:?}", h.texts());
    }
    assert!(h.find("Verse").is_none(), "a Lexicon entry has no Verse");
    h.click_text("Word Study");
    assert_eq!(focused_kind(&h), TabKind::Guide(Guide::WordStudy));
    assert_eq!(
        focused_key(&h),
        Some(Key::StrongsNumber("H2438H".parse().unwrap()))
    );
    drop(h);
}

fn search_tabs(h: &Harness) -> Vec<TabId> {
    h.shell
        .session()
        .workspace
        .tabs()
        .filter(|t| t.kind == TabKind::Tool(Tool::Search))
        .map(|t| t.id)
        .collect()
}

fn query_of(h: &Harness, tab: TabId) -> String {
    h.shell.session().search_tab(tab).unwrap().query.clone()
}

#[test]
fn ctrl_cmd_click_on_a_search_action_opens_a_new_search_tab() {
    let mut h = Harness::new();
    let _files = setup(&mut h, "search-new-tab");
    let kjv = open_at_john_3_16(&mut h, "kjv");
    // A Search Tab is open with its own query.
    h.shell.run(
        "search.run",
        &json!({ "text": "world", "mode": "bible", "bibles": "kjv" }),
    );
    settle_search(&mut h);
    let open = search_tabs(&h);
    assert_eq!(open.len(), 1);
    // New Search Tabs split Panels off: find the word again each time.
    let loved = |h: &mut Harness| {
        h.shell.session_mut().workspace.focus(kjv);
        h.frames(2);
        super::word_info::word_spot(h, kjv, "loved")
    };

    // Search Bible for G25 with Ctrl/Cmd: a new Search Tab; the open one keeps "world".
    let at = loved(&mut h);
    right_click(&mut h, at);
    hover(&mut h, "G25");
    h.modifiers = Modifiers::COMMAND;
    h.click_text("🔎 Search Bible for G25");
    h.modifiers = Modifiers::NONE;
    let tabs = search_tabs(&h);
    assert_eq!(tabs.len(), 2, "a second Search Tab");
    assert_eq!(query_of(&h, open[0]), "world");
    let new = *tabs.iter().find(|t| **t != open[0]).unwrap();
    assert_eq!(query_of(&h, new), "strong:G25");

    // Search this with Ctrl/Cmd: another one.
    let at = loved(&mut h);
    right_click(&mut h, at);
    hover(&mut h, "“loved”");
    h.modifiers = Modifiers::COMMAND;
    h.click_text("🔎 Search this");
    h.modifiers = Modifiers::NONE;
    let tabs = search_tabs(&h);
    assert_eq!(tabs.len(), 3, "a third Search Tab");
    assert_eq!(query_of(&h, open[0]), "world");
    assert_eq!(query_of(&h, new), "strong:G25");
    assert!(tabs.iter().any(|t| query_of(&h, *t) == "loved"));

    // Without Ctrl/Cmd the most recent Search Tab runs it.
    let at = loved(&mut h);
    right_click(&mut h, at);
    hover(&mut h, "G25");
    h.click_text("🔎 Search Bible for G25");
    assert_eq!(search_tabs(&h).len(), 3);
    drop(h);
}

#[test]
fn a_right_click_with_nothing_to_act_on_never_shows_the_last_menu() {
    let mut h = Harness::new();
    let _files = setup(&mut h, "stale");
    let _kjv = open_at_john_3_16(&mut h, "kjv");
    h.shell.run(
        "search.run",
        &json!({ "text": "strong:G25", "mode": "bible", "bibles": "kjv" }),
    );
    settle_search(&mut h);
    settle_all(&mut h);
    let search = search_tabs(&h)[0];
    let rows = |h: &Harness| {
        h.shell.st.tabs[&search]
            .search
            .as_ref()
            .unwrap()
            .row_rects
            .clone()
    };
    let tiles = &h.shell.tree.tiles;
    let pane = tiles.rect(tiles.find_pane(&search).unwrap()).unwrap();
    let (_, row) = *rows(&h).first().expect("a result row");
    right_click(&mut h, row.center());
    assert!(menu_open(&h));
    assert!(h.find("Copy with reference").is_some(), "{:?}", h.texts());
    press(&mut h, egui::Key::Escape, Modifiers::NONE);
    assert!(!menu_open(&h));

    // Blank space below the rows: no menu, not the last one.
    let last = rows(&h).last().unwrap().1;
    let blank = pos2(last.center().x, (last.bottom() + pane.bottom()) / 2.0);
    assert!(blank.y > last.bottom() + 10.0, "room below the rows");
    right_click(&mut h, blank);
    assert!(!menu_open(&h), "{:?}", h.texts());
    assert!(h.find("Copy with reference").is_none());

    // The same Search Tab in Library mode: its rows have no menu either.
    h.shell.run(
        "search.run",
        &json!({ "tab": search.0, "text": "love", "mode": "library" }),
    );
    settle_search(&mut h);
    settle_all(&mut h);
    let (_, row) = *rows(&h).first().expect("a Library result row");
    right_click(&mut h, row.center());
    assert!(!menu_open(&h), "{:?}", h.texts());
    assert!(h.find("Copy with reference").is_none());
    assert!(h.find("Cross-references").is_none());
    drop(h);
}

#[test]
fn search_results_copy_the_hit_with_its_reference() {
    let mut h = Harness::new();
    let _files = setup(&mut h, "search-copy");
    let _kjv = open_at_john_3_16(&mut h, "kjv");
    h.shell.run(
        "search.run",
        &json!({ "text": "loved", "mode": "bible", "bibles": "kjv" }),
    );
    settle_search(&mut h);
    settle_all(&mut h);
    let search = search_tabs(&h)[0];
    let row = h.shell.st.tabs[&search]
        .search
        .as_ref()
        .unwrap()
        .row_rects
        .first()
        .map(|(_, r)| *r)
        .expect("a result row");
    right_click(&mut h, row.center());
    h.click_text("Copy with reference");
    let copied = h.copied.last().cloned().unwrap_or_default();
    assert!(
        copied.starts_with("For God so loved the world")
            && copied.ends_with("everlasting life.\nJohn 3:16 (KJV)"),
        "{copied:?}"
    );
    drop(h);
}

#[test]
fn comparison_copy_cites_the_rows_own_numbering() {
    // TB, HEB and HNUM: HNUM numbers Genesis 32:1 what the Standard numbers 31:55.
    let mut h = Harness::new();
    let tb = h.tab_id("TB");
    h.shell.st.session.workspace.user_focused(tb);
    let cmp = h
        .shell
        .run(
            "tool.text_comparison",
            &json!({ "passage": "Gen 31:55", "split": true }),
        )
        .unwrap()["tab"]
        .as_u64()
        .map(TabId)
        .unwrap();
    for _ in 0..400 {
        h.paced_frame();
        let pending = h.shell.st.tabs[&cmp]
            .comparison
            .as_ref()
            .is_some_and(|v| v.has_pending());
        if !pending {
            break;
        }
    }
    h.frames(2);
    let tiles = &h.shell.tree.tiles;
    let pane = tiles.rect(tiles.find_pane(&cmp).unwrap()).unwrap();
    let in_pane = |h: &Harness, text: &str| {
        h.shapes
            .iter()
            .filter(|(r, t)| t == text && pane.contains(r.center()))
            .map(|(r, _)| *r)
            .min_by(|a, b| a.top().total_cmp(&b.top()))
            .unwrap_or_else(|| panic!("no {text}: {:?}", h.texts()))
    };
    // HNUM's row in the Gen 31:55 block, by its own number.
    let hnum = in_pane(&h, "32:1");
    right_click(&mut h, hnum.center());
    assert!(menu_open(&h), "{:?}", h.texts());
    assert!(h.find("Gen 31:55").is_some(), "{:?}", h.texts());
    h.click_text("Copy with reference");
    let copied = h.copied.last().cloned().unwrap_or_default();
    assert!(copied.ends_with("\nGenesis 32:1 (HNUM)"), "{copied:?}");

    // TB's row in the same block: the Standard numbering is its own.
    let block = hnum.top();
    let tb_row = h
        .shapes
        .iter()
        .filter(|(r, t)| t == "TB" && pane.contains(r.center()) && r.top() < block)
        .map(|(r, _)| *r)
        .max_by(|a, b| a.top().total_cmp(&b.top()))
        .unwrap_or_else(|| panic!("no TB row: {:?}", h.texts()));
    right_click(&mut h, tb_row.center());
    h.click_text("Copy with reference");
    let copied = h.copied.last().cloned().unwrap_or_default();
    assert!(copied.ends_with("\nGenesis 31:55 (TB)"), "{copied:?}");
    drop(h);
}

#[test]
fn native_references_cover_bridges_and_several_verses() {
    use crate::shell::context_menu::native_reference;
    use obs_scripture::Standard;
    let cases: [(&[(Verse, u16)], &str); 4] = [
        (&[(ge(32, 1), 1)], "Genesis 32:1"),
        (&[(ge(1, 1), 2)], "Genesis 1:1-2"),
        (&[(ge(31, 55), 55), (ge(32, 1), 1)], "Genesis 31:55-32:1"),
        (&[(ge(31, 54), 55)], "Genesis 31:54-55"),
    ];
    for (natives, want) in cases {
        assert_eq!(native_reference(natives, &Standard).as_deref(), Some(want));
    }
    assert_eq!(native_reference(&[], &Standard), None);
}

#[test]
fn a_scripture_reference_still_being_translated_fills_in_the_open_menu() {
    use obs_scripture::Passage;
    let john = |n| Passage::verse(Verse::new(Book::John, 3, n));
    let prose = "God gave his Son; on why, see the next verse (v. 17) at length.";
    let mut commentary = obs_engine::testing::commentary(
        "refcomm",
        "Reference Commentary (test)",
        "RC",
        &[(john(16), None, prose)],
    );
    let at = prose.find("v. 17").unwrap();
    commentary.units[0].spans.push(obs_model::Span {
        start: at as u32,
        end: (at + "v. 17".len()) as u32,
        kind: obs_model::SpanKind::ScriptureRef {
            targets: vec![john(17)].into(),
        },
    });
    commentary.manifest.content_hash = commentary.content_hash();
    let mut h = Harness::new();
    let _files = setup_with(&mut h, "pending-reference", &[commentary]);
    let tab = open_at_john_3_16(&mut h, "refcomm");
    let span = h.shell.st.tabs[&tab]
        .view
        .as_ref()
        .unwrap()
        .doc
        .units
        .iter()
        .find_map(|u| {
            u.spans
                .iter()
                .position(|s| matches!(s.kind, obs_model::SpanKind::ScriptureRef { .. }))
        })
        .expect("the reference span");
    // The worker translates nothing until released.
    h.shell.st.references.targets = crate::shell::references::TargetCache::held();
    let section_verse = |h: &Harness| h.texts().iter().filter(|t| **t == "John 3:16").count();
    let before = section_verse(&h);
    let spot = super::references::reference_spot(&h, tab, span);
    right_click(&mut h, spot);
    assert!(menu_open(&h), "{:?}", h.texts());
    // The reference as written, not the section's Verse around it.
    assert!(h.find("v. 17").is_some(), "{:?}", h.texts());
    assert_eq!(section_verse(&h), before, "{:?}", h.texts());
    assert!(h.find("John 3:17").is_none(), "{:?}", h.texts());

    // The targets arrive: the open menu offers the referenced Verse.
    h.shell.st.references.targets.release();
    h.frames(3);
    assert!(menu_open(&h), "{:?}", h.texts());
    assert!(h.find("John 3:17").is_some(), "{:?}", h.texts());
    // Off the reference first: its preview, now ready, shows "John 3:17" too.
    hover(&mut h, "Verse");
    hover(&mut h, "John 3:17");
    h.click_text("Passage Guide");
    assert_eq!(focused_kind(&h), TabKind::Guide(Guide::PassageGuide));
    let ws = &h.shell.session().workspace;
    assert_eq!(
        ws.tab(ws.focused().unwrap()).unwrap().history.current(),
        Some(Verse::new(Book::John, 3, 17))
    );
    drop(h);
}
