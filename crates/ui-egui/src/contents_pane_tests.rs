//! Contents pane (#176) acceptance tests in the shell's real-event headless harness.

use obs_engine::TabId;
use obs_model::{
    Key, KeyScheme, Manifest, Resource, ResourceBuilder, ResourceType, UnitId, UnitKind,
};
use obs_scripture::{Book, Passage, Verse};

use super::*;

fn pane_rect(h: &Harness, tab: TabId) -> Rect {
    let tiles = &h.shell.tree.tiles;
    tiles.rect(tiles.find_pane(&tab).unwrap()).unwrap()
}

/// Where `text` was painted in `tab`'s Contents pane last frame.
fn find_in(h: &Harness, tab: TabId, text: &str) -> Option<Rect> {
    let t = h.shell.st.tabs.get(&tab)?;
    let open = h
        .shell
        .session()
        .workspace
        .tab(tab)
        .is_some_and(|t| t.contents);
    let pane = t.contents.rect?;
    if !open {
        return None;
    }
    h.shapes
        .iter()
        .rev()
        .find(|(r, t)| t == text && pane.contains(r.center()))
        .map(|(r, _)| *r)
}

/// Where `text` was painted in `tab`'s pane (header included) last frame.
fn find_in_tab(h: &Harness, tab: TabId, text: &str) -> Option<Rect> {
    let pane = pane_rect(h, tab);
    h.shapes
        .iter()
        .rev()
        .find(|(r, t)| t == text && pane.contains(r.center()))
        .map(|(r, _)| *r)
}

fn click_in(h: &mut Harness, tab: TabId, text: &str) {
    let r = find_in(h, tab, text)
        .or_else(|| find_in_tab(h, tab, text))
        .unwrap_or_else(|| panic!("{text:?} not in the pane: {:?}", h.texts()));
    h.click(r.center());
}

/// Click the Tab header's Contents button.
fn click_contents(h: &mut Harness, tab: TabId) {
    let r = h.shell.st.tabs[&tab]
        .contents
        .button
        .expect("no Contents button");
    h.click(r.center());
}

/// The highlighted Contents line of `tab`, by its label.
fn highlighted(h: &mut Harness, tab: TabId) -> Option<String> {
    let doc = Arc::clone(&h.shell.st.tabs[&tab].view.as_ref()?.doc);
    let contents = h.shell.st.contents.get(&doc, false)?;
    let line = h.shell.st.tabs[&tab].contents.current?;
    Some(contents.node(line).label.clone())
}

/// The highlight is painted: a filled selection rectangle behind the line's label.
fn painted_highlight(h: &Harness, tab: TabId, label: &str) -> bool {
    let Some(text) = find_in(h, tab, label) else {
        return false;
    };
    let fill = h.ctx.global_style().visuals.selection.bg_fill;
    h.rects
        .iter()
        .any(|(r, c)| *c == fill && r.contains(text.center()))
}

fn open_resource(h: &mut Harness, resource: Resource) -> TabId {
    let id = resource.manifest.resource_id.clone();
    add_doc(&mut h.shell, Arc::new(ReadingDoc::from_resource(resource)));
    let tab = h
        .shell
        .run("resource.open", &json!({ "id": id, "split": true }))
        .unwrap()["tab"]
        .as_u64()
        .unwrap();
    h.frames(3);
    TabId(tab)
}

fn shown_unit(h: &mut Harness, tab: TabId) -> String {
    let view = h
        .shell
        .st
        .tabs
        .get_mut(&tab)
        .unwrap()
        .view
        .as_mut()
        .unwrap();
    let unit = view.top_unit().unwrap();
    view.doc.units[unit].id.to_string()
}

fn ge(chapter: u16, verse: u16) -> Verse {
    Verse::new(Book::Genesis, chapter, verse)
}

fn prose(b: &mut ResourceBuilder, id: &UnitId, kind: UnitKind, title: &str, text: &str) {
    b.add_unit(id.clone(), kind, Some(title.into()), text.into());
    b.add_span(
        id,
        0..text.len(),
        obs_model::SpanKind::Para {
            style: obs_model::ParaStyle::Prose,
        },
    );
}

/// A Commentary in two volumes, with a book introduction keyed by the whole of Genesis and
/// a first section starting where it does, at Gen 1:1.
fn commentary() -> Resource {
    let mut m = Manifest::new(
        "test-mhc",
        "Test Commentary in Volumes",
        "TMH",
        ResourceType::Commentary,
        "en",
    );
    m.key_schemes = vec![KeyScheme::Passage];
    m.license = "CC0-1.0".into();
    let mut b = ResourceBuilder::new(m);
    let long = "Commentary prose. ".repeat(120);
    for (id, start, end, text) in [
        (
            "Gen.1.1-Gen.50.26.book-intro",
            ge(1, 1),
            ge(50, 26),
            "Genesis is the book of beginnings.",
        ),
        (
            "Gen.1.1-Gen.1.2",
            ge(1, 1),
            ge(1, 2),
            "The creation of heaven and earth.",
        ),
        ("Gen.31.50-Gen.31.54", ge(31, 50), ge(31, 54), long.as_str()),
        ("Gen.32.5-Gen.32.9", ge(32, 5), ge(32, 9), long.as_str()),
        (
            "Matt.1.1-Matt.1.17",
            Verse::new(Book::Matthew, 1, 1),
            Verse::new(Book::Matthew, 1, 17),
            "The genealogy.",
        ),
    ] {
        let unit = UnitId::new(id).unwrap();
        let passage = Passage::new(start, end).unwrap();
        prose(&mut b, &unit, UnitKind::Section, &passage.to_string(), text);
        b.add_key(Key::Passage(passage), &unit, None, false);
    }
    b.set_volume(Book::Genesis, "Volume 1");
    b.set_volume(Book::Matthew, "Volume 5");
    b.build().unwrap()
}

/// A Dictionary of headwords from A to E, with Abram as an alias of Abraham.
fn dictionary() -> Resource {
    let mut m = Manifest::new(
        "test-easton",
        "Test Bible Dictionary",
        "TBD",
        ResourceType::Dictionary,
        "en",
    );
    m.key_schemes = vec![KeyScheme::Headword];
    m.license = "CC0-1.0".into();
    let mut b = ResourceBuilder::new(m);
    for headword in [
        "Aaron",
        "Abaddon",
        "Abel",
        "Abraham",
        "Abraham’s bosom",
        "Adam",
        "Babel",
        "Bethel",
        "Cain",
        "David",
        "Eden",
    ] {
        let unit = UnitId::new(headword).unwrap();
        let text = format!("{headword}. {}", "An entry of the dictionary. ".repeat(30));
        prose(&mut b, &unit, UnitKind::Entry, headword, &text);
        b.add_key(Key::Headword(headword.into()), &unit, None, false);
    }
    b.add_key(
        Key::Headword("Abram".into()),
        &UnitId::new("Abraham").unwrap(),
        None,
        true,
    );
    b.build().unwrap()
}

#[test]
fn a_bible_tabs_contents_button_shows_its_chapters_highlights_where_it_is_and_jumps_the_link_set() {
    let (mut h, tb, hn) = linked_harness();
    h.shell
        .run("tab.go", &json!({ "tab": tb.0, "text": "Gen 30:1" }));
    settle_all(&mut h);
    assert!(find_in(&h, tb, "Chapter 30").is_none());
    click_contents(&mut h, tb);
    assert!(h.shell.session().workspace.tab(tb).unwrap().contents);
    h.frames(2);
    // Books → chapters; the Current location's book is open, its chapter highlighted.
    for line in [
        "Genesis",
        "Chapter 30",
        "Chapter 31",
        "Chapter 32",
        "Chapter 33",
    ] {
        assert!(find_in(&h, tb, line).is_some(), "{line}: {:?}", h.texts());
    }
    assert_eq!(highlighted(&mut h, tb).as_deref(), Some("Chapter 30"));
    assert!(painted_highlight(&h, tb, "Chapter 30"));
    // The pane is beside the text, not over it: the text starts right of it.
    let line = find_in(&h, tb, "Chapter 30").unwrap();
    let view = h.shell.st.tabs[&tb].view.as_ref().unwrap();
    assert!(view.text_origin[0] > line.right());
    // The other Tab has no pane.
    assert!(find_in(&h, hn, "Chapter 30").is_none());

    // Clicking a chapter jumps the Tab, and its Link set follows in its own numbering.
    click_in(&mut h, tb, "Chapter 32");
    settle_all(&mut h);
    assert_eq!(location(&mut h, tb), Some(ge(32, 1)));
    assert_eq!(
        h.shell
            .session()
            .workspace
            .tab(tb)
            .unwrap()
            .history
            .current(),
        Some(ge(32, 1))
    );
    assert_eq!(location(&mut h, hn), Some(ge(32, 2)));
    h.frame();
    assert_eq!(highlighted(&mut h, tb).as_deref(), Some("Chapter 32"));
    assert!(painted_highlight(&h, tb, "Chapter 32"));

    // The highlight follows the text as it scrolls.
    let over = view_point(&h, tb);
    h.events.push(Event::PointerMoved(over));
    let mut steps = 0;
    while location(&mut h, tb).map(|v| v.chapter) != Some(33) {
        h.events.push(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, -60.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: Modifiers::NONE,
        });
        h.frame();
        steps += 1;
        assert!(steps < 400, "stuck at {:?}", location(&mut h, tb));
    }
    h.frame();
    assert_eq!(highlighted(&mut h, tb).as_deref(), Some("Chapter 33"));
    assert!(painted_highlight(&h, tb, "Chapter 33"));

    // Collapsing the book hides its chapters; the button hides the pane.
    let book = find_in(&h, tb, "Genesis").unwrap();
    h.click(pos2(book.left() - 13.0, book.center().y));
    assert!(find_in(&h, tb, "Chapter 31").is_none(), "{:?}", h.texts());
    click_contents(&mut h, tb);
    assert!(!h.shell.session().workspace.tab(tb).unwrap().contents);
    assert!(find_in(&h, tb, "Genesis").is_none());
}

/// A point over the Tab's text, right of its Contents pane.
fn view_point(h: &Harness, tab: TabId) -> Pos2 {
    let pane = pane_rect(h, tab);
    pos2(pane.right() - 60.0, pane.center().y)
}

#[test]
fn a_commentarys_contents_has_volumes_books_chapters_and_sections_and_clicks_jump() {
    let mut h = Harness::new();
    let tb = h.tab_id("TB");
    h.shell
        .run("tab.go", &json!({ "tab": tb.0, "text": "Gen 31:51" }));
    let tc = open_resource(&mut h, commentary());
    assert_eq!(h.shell.session().workspace.tab(tc).unwrap().link, Some('A'));
    h.shell
        .run("tab.contents", &json!({ "tab": tc.0, "on": true }))
        .unwrap();
    settle_all(&mut h);
    assert_eq!(shown_unit(&mut h, tc), "Gen.31.50-Gen.31.54");
    for line in [
        "Volume 1 (Genesis)",
        "Genesis",
        "Introduction",
        "Chapter 31",
        "Verses 50–54",
        "Chapter 32",
        "Volume 5 (Matthew)",
    ] {
        assert!(find_in(&h, tc, line).is_some(), "{line}: {:?}", h.texts());
    }
    // Matthew's volume stays closed.
    assert!(find_in(&h, tc, "Matthew").is_none());
    assert_eq!(highlighted(&mut h, tc).as_deref(), Some("Verses 50–54"));

    // A chapter line opens it and goes to its first section, which is highlighted; the
    // Bible follows.
    click_in(&mut h, tc, "Chapter 32");
    settle_all(&mut h);
    assert_eq!(shown_unit(&mut h, tc), "Gen.32.5-Gen.32.9");
    assert_eq!(location(&mut h, tb), Some(ge(32, 5)));
    h.frame();
    assert_eq!(highlighted(&mut h, tc).as_deref(), Some("Verses 5–9"));
    assert!(painted_highlight(&h, tc, "Verses 5–9"));
    // A section line goes to that section.
    click_in(&mut h, tc, "Verses 50–54");
    settle_all(&mut h);
    assert_eq!(shown_unit(&mut h, tc), "Gen.31.50-Gen.31.54");
    assert_eq!(location(&mut h, tb), Some(ge(31, 50)));
    click_in(&mut h, tc, "Verses 5–9");
    settle_all(&mut h);
    assert_eq!(shown_unit(&mut h, tc), "Gen.32.5-Gen.32.9");

    // The book introduction, which a Passage alone would not reach.
    click_in(&mut h, tc, "Introduction");
    settle_all(&mut h);
    assert_eq!(shown_unit(&mut h, tc), "Gen.1.1-Gen.50.26.book-intro");
    h.frame();
    assert_eq!(highlighted(&mut h, tc).as_deref(), Some("Introduction"));
    // Back returns to where the Tab was.
    h.shell.run("tab.back", &json!({ "tab": tc.0 }));
    settle_all(&mut h);
    assert_eq!(shown_unit(&mut h, tc), "Gen.32.5-Gen.32.9");

    // When a Link set member moves, the Commentary follows and so does its highlight.
    h.shell
        .run("tab.go", &json!({ "tab": tb.0, "text": "Gen 31:52" }));
    settle_all(&mut h);
    h.frame();
    assert_eq!(highlighted(&mut h, tc).as_deref(), Some("Verses 50–54"));
}

#[test]
fn back_and_forward_return_to_the_book_introduction_and_the_section_sharing_its_start() {
    let mut h = Harness::new();
    let tb = h.tab_id("TB");
    h.shell
        .run("tab.go", &json!({ "tab": tb.0, "text": "Gen 31:51" }));
    let tc = open_resource(&mut h, commentary());
    h.shell
        .run("tab.contents", &json!({ "tab": tc.0, "on": true }))
        .unwrap();
    settle_all(&mut h);
    assert_eq!(shown_unit(&mut h, tc), "Gen.31.50-Gen.31.54");
    const INTRO: &str = "Gen.1.1-Gen.50.26.book-intro";
    const FIRST: &str = "Gen.1.1-Gen.1.2";

    click_in(&mut h, tc, "Introduction");
    settle_all(&mut h);
    assert_eq!(shown_unit(&mut h, tc), INTRO);
    // Chapter 1 goes to its first section, which starts at Gen 1:1 like the introduction:
    // still a step of its own.
    click_in(&mut h, tc, "Chapter 1");
    settle_all(&mut h);
    assert_eq!(shown_unit(&mut h, tc), FIRST);
    h.frame();
    assert_eq!(highlighted(&mut h, tc).as_deref(), Some("Verses 1–2"));

    let step = |h: &mut Harness, command: &str, unit: &str, line: &str| {
        h.shell.run(command, &json!({ "tab": tc.0 })).unwrap();
        settle_all(h);
        assert_eq!(shown_unit(h, tc), unit, "{command}");
        h.frame();
        assert_eq!(highlighted(h, tc).as_deref(), Some(line), "{command}");
    };
    step(&mut h, "tab.back", INTRO, "Introduction");
    step(&mut h, "tab.forward", FIRST, "Verses 1–2");
    step(&mut h, "tab.back", INTRO, "Introduction");
    step(&mut h, "tab.back", "Gen.31.50-Gen.31.54", "Verses 50–54");
    step(&mut h, "tab.forward", INTRO, "Introduction");

    // Scrolled from the introduction into the section sharing its first Verse, the Tab has
    // left the introduction: Back from a later jump returns to the section.
    let over = view_point(&h, tc);
    h.events.push(Event::PointerMoved(over));
    let mut steps = 0;
    while shown_unit(&mut h, tc) != FIRST {
        h.events.push(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, -8.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: Modifiers::NONE,
        });
        h.frame();
        steps += 1;
        assert!(steps < 200, "stuck in {}", shown_unit(&mut h, tc));
    }
    h.frame();
    assert_eq!(location(&mut h, tc), Some(ge(1, 1)));
    click_in(&mut h, tc, "Chapter 32");
    settle_all(&mut h);
    assert_eq!(shown_unit(&mut h, tc), "Gen.32.5-Gen.32.9");
    step(&mut h, "tab.back", FIRST, "Verses 1–2");
}

#[test]
fn a_dictionarys_contents_is_a_to_z_and_its_filter_finds_abraham() {
    let mut h = Harness::new();
    let td = open_resource(&mut h, dictionary());
    click_contents(&mut h, td);
    settle_all(&mut h);
    for line in ["A", "Aaron", "Abraham", "B", "C", "D", "E"] {
        assert!(find_in(&h, td, line).is_some(), "{line}: {:?}", h.texts());
    }
    assert!(find_in(&h, td, "Babel").is_none());
    assert_eq!(highlighted(&mut h, td).as_deref(), Some("Aaron"));

    // Type into the filter box: only matching entries, those starting with it first.
    click_in(&mut h, td, "Filter A–Z");
    h.events.push(Event::Text("abraham".into()));
    h.frames(2);
    assert!(find_in(&h, td, "Abraham").is_some(), "{:?}", h.texts());
    assert!(find_in(&h, td, "Abraham’s bosom").is_some());
    for gone in ["Aaron", "Abel", "B", "Babel"] {
        assert!(find_in(&h, td, gone).is_none(), "{gone}: {:?}", h.texts());
    }
    click_in(&mut h, td, "Abraham");
    settle_all(&mut h);
    assert_eq!(shown_unit(&mut h, td), "Abraham");
    assert_eq!(h.shell.st.location_text(td), "Abraham");
    h.frame();
    assert_eq!(highlighted(&mut h, td).as_deref(), Some("Abraham"));

    // Enter in the filter box opens the first match.
    click_in(&mut h, td, "abraham");
    h.events.push(Event::Key {
        key: egui::Key::A,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::COMMAND,
    });
    h.events.push(Event::Text("bethel".into()));
    h.frame();
    h.events.push(Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.frames(2);
    settle_all(&mut h);
    assert_eq!(shown_unit(&mut h, td), "Bethel");
}

#[test]
fn the_list_scrolls_to_keep_the_highlighted_line_in_view() {
    let mut h = Harness::new();
    let mut m = Manifest::new(
        "long",
        "Long Dictionary",
        "LD",
        ResourceType::Dictionary,
        "en",
    );
    m.key_schemes = vec![KeyScheme::Headword];
    m.license = "CC0-1.0".into();
    let mut b = ResourceBuilder::new(m);
    for n in 1..=300 {
        let headword = format!("Entry {n:03}");
        let unit = UnitId::new(headword.clone()).unwrap();
        prose(
            &mut b,
            &unit,
            UnitKind::Entry,
            &headword,
            &format!("{headword}. Text."),
        );
        b.add_key(Key::Headword(headword), &unit, None, false);
    }
    let td = open_resource(&mut h, b.build().unwrap());
    click_contents(&mut h, td);
    settle_all(&mut h);
    assert!(find_in(&h, td, "Entry 001").is_some());
    assert!(find_in(&h, td, "Entry 250").is_none());
    h.shell
        .run("tab.go", &json!({ "tab": td.0, "text": "Entry 250" }));
    settle_all(&mut h);
    h.frames(30);
    assert_eq!(highlighted(&mut h, td).as_deref(), Some("Entry 250"));
    let line = find_in(&h, td, "Entry 250").expect("Entry 250 scrolled into view");
    let pane = h.shell.st.tabs[&td].contents.rect.unwrap();
    assert!(pane.contains(line.center()));
    assert!(painted_highlight(&h, td, "Entry 250"));
    assert!(find_in(&h, td, "Entry 001").is_none());
    // A move within view leaves the list where it is.
    let before = line;
    h.shell
        .run("tab.go", &json!({ "tab": td.0, "text": "Entry 251" }));
    settle_all(&mut h);
    h.frames(30);
    assert_eq!(find_in(&h, td, "Entry 250"), Some(before));
}

#[test]
fn contents_are_built_on_a_worker_the_first_time_and_shared_by_the_resources_tabs() {
    let mut h = Harness::threaded();
    let tb = h.tab_id("TB");
    h.shell
        .run("tab.contents", &json!({ "tab": tb.0 }))
        .unwrap();
    let mut frames = 0;
    while find_in(&h, tb, "Chapter 30").is_none() {
        h.paced_frame();
        frames += 1;
        assert!(frames < 200, "{:?}", h.texts());
    }
    let doc = Arc::clone(&h.shell.st.tabs[&tb].view.as_ref().unwrap().doc);
    let first = h.shell.st.contents.get(&doc, true).unwrap();
    // A second Tab of the same Resource shows it at once, from the same Contents.
    h.shell.st.session.workspace.user_focused(tb);
    let again = TabId(
        h.shell
            .run(
                "resource.open",
                &json!({ "id": "test-bible", "split": true }),
            )
            .unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    h.shell
        .run("tab.contents", &json!({ "tab": again.0 }))
        .unwrap();
    h.frames(3);
    assert!(
        find_in(&h, again, "Chapter 30").is_some(),
        "{:?}",
        h.texts()
    );
    assert!(Arc::ptr_eq(
        &first,
        &h.shell.st.contents.get(&doc, true).unwrap()
    ));
}

/// Screenshots of the Contents pane for the PR: the test Resources always, and with
/// `OBS_REAL_STARTER=<dist/starter>` the Starter library's KJV, Matthew Henry and Easton.
/// `UPDATE_PREVIEWS=1 cargo test -p obs-ui-egui contents_pane_previews`
#[test]
fn contents_pane_previews() {
    if std::env::var_os("UPDATE_PREVIEWS").is_none() {
        return;
    }
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews/contents");
    std::fs::create_dir_all(&dir).unwrap();
    let Some(starter) = std::env::var_os("OBS_REAL_STARTER") else {
        let mut h = Harness::new();
        let td = open_resource(&mut h, dictionary());
        h.shell
            .run("tab.contents", &json!({ "tab": td.0 }))
            .unwrap();
        settle_all(&mut h);
        h.screenshot(&dir.join("test-dictionary.ppm"));
        return;
    };
    let starter = std::path::PathBuf::from(starter);
    let mut h = Harness::new();
    let open = |h: &mut Harness, id: &str| {
        h.shell
            .st
            .session
            .library
            .add_file(&starter.join(format!("{id}.obr")))
            .unwrap();
        let tab = TabId(
            h.shell
                .run("resource.open", &json!({ "id": id, "split": true }))
                .unwrap()["tab"]
                .as_u64()
                .unwrap(),
        );
        h.shell
            .run("tab.contents", &json!({ "tab": tab.0 }))
            .unwrap();
        h.frames(3);
        tab
    };
    let tb = h.tab_id("TB");
    h.shell.run("tab.close", &json!({ "tab": tb.0 }));
    let kjv = open(&mut h, "kjv-strongs");
    h.shell
        .run("tab.go", &json!({ "tab": kjv.0, "text": "Ps 23:1" }));
    let mhc = open(&mut h, "mhc");
    settle_all(&mut h);
    h.frames(3);
    h.shell
        .run("tab.go", &json!({ "tab": mhc.0, "text": "Ps 23:1" }));
    settle_all(&mut h);
    h.frames(3);
    h.screenshot(&dir.join("kjv-mhc-ps-23.ppm"));
    h.shell
        .run("tab.go", &json!({ "tab": mhc.0, "text": "Gen 1:1" }));
    settle_all(&mut h);
    h.frames(3);
    h.screenshot(&dir.join("mhc-volumes.ppm"));

    h.shell.run("tab.close", &json!({ "tab": mhc.0 }));
    let easton = open(&mut h, "easton");
    let _ = kjv;
    settle_all(&mut h);
    click_in(&mut h, easton, "Filter A–Z");
    h.events.push(Event::Text("Abraham".into()));
    h.frames(2);
    click_in(&mut h, easton, "Abraham");
    settle_all(&mut h);
    h.frames(3);
    h.screenshot(&dir.join("easton-abraham.ppm"));
}
