//! Collections (#153) through the shell's real-event harness: New Collection… makes a
//! rule-based Collection in the Library, its chip filters the Library, the Search Tool's
//! Choose… menu narrows a Library search to it, a Library row's Collections submenu adds a
//! Resource by hand, and the Passage Guide's Commentaries filter to a Collection.

use std::path::{Path, PathBuf};
use std::time::Duration;

use obs_engine::testing::{bibles, commentary, study};
use obs_engine::{Library, TabId};
use obs_scripture::{Book, Passage, Verse};

use super::*;

struct Files(PathBuf);

impl Drop for Files {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Visual evidence for `docs/previews/<name>.png` when `UPDATE_PREVIEWS` is set.
fn preview(h: &Harness, name: &str) {
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
        std::fs::create_dir_all(&dir).unwrap();
        h.screenshot(&dir.join(format!("{name}.ppm")));
    }
}

fn john(c: u16, v: u16) -> Verse {
    Verse::new(Book::John, c, v)
}

/// The KJV, three Commentaries (TCC, ALPHA and BETA, the last two on John 3), a Lexicon and a
/// Dictionary, with their Search indexes; the Library Tab open.
fn harness(name: &str) -> (Harness, Files) {
    let root =
        std::env::temp_dir().join(format!("obs-ui-collections-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let files = Files(root.clone());
    let chapter = Passage::new(john(3, 1), john(3, 21)).unwrap();
    let resources = [
        bibles::kjv(),
        study::tcc(),
        commentary(
            "alpha",
            "Alpha Commentary",
            "ALPHA",
            &[(
                chapter,
                Some("The Night Visit"),
                "A teacher comes by night with questions about new birth and the kingdom.",
            )],
        ),
        commentary(
            "beta",
            "Beta Notes",
            "BETA",
            &[(
                Passage::verse(john(3, 16)),
                None,
                "16. Love, gift, belief and life together in one verse.",
            )],
        ),
        study::tlx(),
        study::tbd(),
    ];
    bibles::write_library(&root.join("library"), &resources);
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
    h.shell.run("library.open", &json!({}));
    h.frames(3);
    (h, files)
}

fn settle(h: &mut Harness) {
    for _ in 0..2000 {
        h.shell.st.session.jobs().pump(16);
        h.frame();
        let s = &h.shell.st.session;
        let layouts =
            (h.shell.st.tabs.values()).any(|t| t.search.as_ref().is_some_and(|v| v.has_pending()));
        if !s.search.is_building()
            && !s.bible_search_running()
            && !s.bible_search.is_reading_ahead()
            && !s.guides.is_busy()
            && !layouts
        {
            h.frames(2);
            return;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    panic!("never settled");
}

/// The Panel holding the Tab titled `title`.
fn panel_of(h: &Harness, title: &str) -> Rect {
    h.panels()
        .into_iter()
        .find(|(_, t)| t.iter().any(|t| t == title))
        .map(|(r, _)| r)
        .unwrap_or_else(|| panic!("no Panel with {title}"))
}

/// The Library rows shown, top to bottom.
fn library_rows(h: &Harness) -> Vec<String> {
    const ABBREVS: [&str; 6] = ["KJV", "TCC", "ALPHA", "BETA", "TLX", "TBD"];
    let panel = panel_of(h, "Library");
    let mut out: Vec<(f32, String)> = (h.shapes.iter())
        .filter(|(r, t)| ABBREVS.contains(&t.as_str()) && panel.contains(r.center()))
        .map(|(r, t)| (r.center().y, t.clone()))
        .collect();
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out.dedup_by(|a, b| a.1 == b.1);
    out.into_iter().map(|(_, t)| t).collect()
}

/// Replace a text field's contents: click it, select all, type.
fn type_into(h: &mut Harness, at: Pos2, text: &str) {
    h.click(at);
    h.events.push(Event::Key {
        key: egui::Key::A,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::COMMAND,
    });
    h.events.push(Event::Text(text.into()));
    h.frames(2);
}

#[test]
fn a_rule_collection_made_with_new_collection_filters_the_library_and_scopes_a_search() {
    let (mut h, _files) = harness("library");
    assert_eq!(
        library_rows(&h),
        ["KJV", "TCC", "ALPHA", "BETA", "TLX", "TBD"]
    );
    h.click_text("New Collection…");
    assert!(h.find("New Collection").is_some(), "{:?}", h.texts());
    // The window settles in the centre before it is typed into.
    h.frames(25);
    let name = h.at("Older commentaries");
    type_into(&mut h, name, "Commentaries");
    h.frames(25);
    let rules = h.at("type:commentary language:en");
    type_into(&mut h, rules, "type:commentary");
    assert!(
        h.find("3 Resources in the Library now").is_some(),
        "{:?}",
        h.texts()
    );
    preview(&h, "collections-new");
    h.frames(25);
    h.click_text("Save");
    assert!(
        h.find("New Collection").is_none(),
        "the window closed: {:?} {:?}",
        h.texts(),
        h.shell.st.session.library.collections()
    );
    let lib = &h.shell.st.session.library;
    assert_eq!(
        lib.collection_members("Commentaries"),
        ["tcc", "alpha", "beta"]
    );

    // Its chip filters the Library; All shows everything again.
    h.frames(25);
    let chip = (h.shapes.iter())
        .filter(|(r, t)| t == "Commentaries" && panel_of(&h, "Library").contains(r.center()))
        .map(|(r, _)| r.center())
        .next()
        .unwrap();
    h.click(chip);
    assert_eq!(library_rows(&h), ["TCC", "ALPHA", "BETA"]);
    preview(&h, "collections-library-filter");
    h.frames(25);
    h.click_text("All");
    assert_eq!(library_rows(&h).len(), 6);

    // The Search Tool: Choose… → the Collection searches just its Commentaries.
    h.shell.run(
        "search.run",
        &json!({ "text": "covenant", "mode": "library" }),
    );
    settle(&mut h);
    let summary = |h: &Harness| {
        (h.texts().iter())
            .find(|t| t.ends_with(" searched)"))
            .map(|t| t.to_string())
            .unwrap_or_else(|| panic!("no summary: {:?}", h.texts()))
    };
    assert!(
        summary(&h).ends_with("in 3 Resources (5 searched)"),
        "{}",
        summary(&h)
    );
    h.click_text("Choose…");
    let item = (h.shapes.iter())
        .rev()
        .find(|(_, t)| t == "Commentaries")
        .map(|(r, _)| r.center())
        .unwrap();
    h.click(item);
    settle(&mut h);
    assert_eq!(summary(&h), "3 paragraphs in 1 Resource (3 searched)");
    assert!(h.find("Collection: Commentaries").is_some(), "its chip");
    preview(&h, "collections-search-scope");
}

#[test]
fn a_rows_collections_submenu_adds_by_hand_and_the_guide_filters_commentaries() {
    let (mut h, _files) = harness("guide");
    h.shell
        .run("collection.save", &json!({ "name": "Night reading" }));
    h.frames(3);
    // Right-click BETA → Collections → Night reading.
    let panel = panel_of(&h, "Library");
    let beta = (h.shapes.iter())
        .find(|(r, t)| t == "BETA" && panel.contains(r.center()))
        .map(|(r, _)| r.center())
        .unwrap();
    h.events.push(Event::PointerMoved(beta));
    h.frame();
    for pressed in [true, false] {
        h.events.push(Event::PointerButton {
            pos: beta,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.frame();
    }
    h.frames(2);
    h.click_text("Collections");
    h.click_text("Night reading");
    assert_eq!(
        h.shell
            .st
            .session
            .library
            .collection_members("Night reading"),
        ["beta"]
    );

    // The Passage Guide at John 3:16: both Commentaries, then only the Collection's.
    let guide = h
        .shell
        .run("guide.passage_guide", &json!({ "split": true }))
        .unwrap()["tab"]
        .as_u64()
        .map(TabId)
        .unwrap();
    h.shell
        .run("tab.go", &json!({ "tab": guide.0, "text": "Jn 3:16" }));
    settle(&mut h);
    let guide_panel = |h: &Harness| {
        let tiles = &h.shell.tree.tiles;
        tiles.rect(tiles.find_pane(&guide).unwrap()).unwrap()
    };
    let shown = |h: &Harness, text: &str| {
        let r = guide_panel(h);
        (h.shapes.iter()).any(|(s, t)| t == text && r.contains(s.center()))
    };
    assert!(
        shown(&h, "Alpha Commentary") && shown(&h, "Beta Notes"),
        "{:?}",
        h.texts()
    );
    assert!(shown(&h, "Every Resource"), "the Collection chooser");
    h.click_text("Every Resource");
    h.click_text("Night reading");
    settle(&mut h);
    assert!(
        shown(&h, "Beta Notes") && !shown(&h, "Alpha Commentary"),
        "{:?}",
        h.texts()
    );
    assert_eq!(
        h.shell.st.session.guide_filter(
            obs_engine::Guide::PassageGuide,
            obs_engine::GuideSection::Commentaries
        ),
        Some("Night reading".into())
    );
    h.events.push(Event::PointerMoved(pos2(1300.0, 850.0)));
    h.frames(3);
    preview(&h, "collections-guide-filter");
    // A Collection with no Commentary on the Passage says so.
    h.shell.run(
        "collection.toggle",
        &json!({ "name": "Night reading", "id": "beta" }),
    );
    settle(&mut h);
    assert!(
        (h.texts().iter()).any(|t| t.starts_with("Nothing in the Collection “Night reading”")),
        "{:?}",
        h.texts()
    );
}
