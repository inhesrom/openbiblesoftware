//! Get Resources' licence filter and the Open catalogue (#134), through the shell's
//! real-event headless harness, against the mock SWORD repositories and a mock Open catalogue
//! (an eBible-style USFM zip at a `file://` URL). No network.

use std::path::PathBuf;

use obs_engine::ImportPhase;
use obs_engine::testing::{MOCK_OPEN_NOTICE, MockRepos, mock_open_catalogue};

use super::*;

const NASB: &str = "New American Standard Bible - NASB 1995";
const OPEN_ROW: &str = "World English Bible excerpt";

fn setup(name: &str) -> (Harness, Arc<MockRepos>, PathBuf) {
    let root = std::env::temp_dir().join(format!("obs-shell-open-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let repos = Arc::new(MockRepos::new(&root.join("repos")));
    let mut h = Harness::new();
    let s = &mut h.shell.st.session;
    s.set_fetch(repos.clone());
    s.get_bibles.set_cache_path(root.join("sword-catalog.json"));
    s.get_bibles.set_sword_dir(root.join("sword"));
    s.get_bibles
        .set_open_catalogue(mock_open_catalogue(&root.join("open"), MOCK_OPEN_NOTICE));
    std::fs::create_dir_all(root.join("library")).unwrap();
    s.library.set_directory(root.join("library"));
    h.frames(2);
    (h, repos, root)
}

fn settle(h: &mut Harness) {
    h.shell.st.session.jobs().pump(4);
    h.frames(3);
}

fn open_get_resources(h: &mut Harness) {
    h.open_menu("Library");
    h.click_text("Get Resources…");
    settle(h);
    assert!(
        h.panels()
            .iter()
            .any(|(_, t)| t.iter().any(|t| t == "Get Resources")),
        "{:?}",
        h.panels()
    );
}

/// The topmost place `text` is painted.
fn first(h: &Harness, text: &str) -> Rect {
    h.shapes
        .iter()
        .filter(|(_, t)| t == text)
        .map(|(r, _)| *r)
        .min_by(|a, b| a.top().total_cmp(&b.top()))
        .unwrap_or_else(|| panic!("{text:?} not on screen: {:?}", h.texts()))
}

/// The control labelled `label` on the topmost row whose title is `title`.
fn row_button(h: &Harness, title: &str, label: &str) -> Pos2 {
    let row = first(h, title);
    h.shapes
        .iter()
        .filter(|(r, t)| {
            t == label && r.center().y > row.top() - 4.0 && r.center().y < row.top() + 72.0
        })
        .map(|(r, _)| r.center())
        .next()
        .unwrap_or_else(|| panic!("no {label:?} on row {title:?}: {:?}", h.texts()))
}

/// Wheel the Get Resources Tab until `text` is painted well inside it (rows are virtualized),
/// and let the scroll settle.
fn scroll_to(h: &mut Harness, text: &str) {
    let tab = h.tab_id("Get Resources");
    let pane = {
        let tiles = &h.shell.tree.tiles;
        tiles.rect(tiles.find_pane(&tab).unwrap()).unwrap()
    };
    let inside = |h: &Harness| {
        h.shapes
            .iter()
            .any(|(r, t)| t == text && pane.shrink(40.0).contains(r.center()))
    };
    for _ in 0..400 {
        if inside(h) {
            h.frames(30);
            if inside(h) {
                return;
            }
        }
        h.events.push(Event::PointerMoved(pane.center()));
        h.events.push(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, -60.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: Modifiers::NONE,
        });
        h.frame();
    }
    panic!("{text:?} never scrolled into view: {:?}", h.texts());
}

fn preview(h: &Harness, name: &str) {
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
        std::fs::create_dir_all(&dir).unwrap();
        h.screenshot(&dir.join(format!("{name}.ppm")));
    }
}

/// #134 acceptance: "Free to share" hides every row whose licence is not Public Domain, CC0,
/// CC BY, CC BY-SA or GPL; "Free for personal use" brings back the permission ones.
#[test]
fn free_to_share_hides_the_rows_that_are_not_free_to_share() {
    let (mut h, _repos, root) = setup("licence-filter");
    open_get_resources(&mut h);
    h.click_text("Bibles");
    assert!(h.find(NASB).is_some(), "{:?}", h.texts());
    assert!(h.find("All licences").is_some());
    preview(&h, "get-resources-all-licences");

    h.click_text("Free to share");
    assert_eq!(
        h.shell.st.session.get_bibles.licence_filter(),
        obs_engine::LicenceFilter::FreeToShare
    );
    assert!(
        h.find(NASB).is_none(),
        "NASB 1995 is permission-to-distribute"
    );
    // Featured Bibles has nothing free to share left in the mock.
    assert!(h.find("FEATURED BIBLES · 0").is_some(), "{:?}", h.texts());
    assert!(h.find("Copyrighted").is_none(), "{:?}", h.texts());
    assert!(h.find("OPEN CATALOGUE · 1").is_some(), "{:?}", h.texts());
    assert!(
        h.find(OPEN_ROW).is_some(),
        "the Open catalogue is free to share"
    );
    preview(&h, "get-resources-free-to-share");
    // Every licence badge down the whole list is a free one.
    scroll_to(&mut h, "ALL BIBLES · 7");
    for badge in [
        "Copyrighted",
        "Non-commercial",
        "CC BY-NC",
        "CC BY-ND",
        "Licence unknown",
    ] {
        assert!(h.find(badge).is_none(), "{badge} shown: {:?}", h.texts());
    }
    assert!(h.find(NASB).is_none());

    h.click_text("Free for personal use");
    scroll_to(&mut h, NASB);
    h.click_text("All licences");
    scroll_to(&mut h, NASB);
    drop(h);
    std::fs::remove_dir_all(root).unwrap();
}

/// #134 acceptance: one click on an Open catalogue Bible downloads the publisher's USFM zip,
/// checks it and imports it; its row links to the archived licence evidence; Help → Credits
/// credits it from its own Attribution.
#[test]
fn one_click_downloads_an_open_catalogue_bible_and_credits_it() {
    let (mut h, repos, root) = setup("download");
    open_get_resources(&mut h);
    scroll_to(&mut h, OPEN_ROW);
    let source = h
        .shell
        .st
        .session
        .get_bibles
        .open_source("web-excerpt")
        .unwrap()
        .clone();

    // The row links to its archived licence evidence.
    let before = h.opened.len();
    let link = row_button(&h, OPEN_ROW, "Licence evidence");
    h.click(link);
    assert_eq!(h.opened[before..], [source.evidence_url.as_str()]);

    let at = row_button(&h, OPEN_ROW, "Download");
    h.click(at);
    let id = "web-excerpt-1";
    assert_eq!(
        h.shell.st.session.imports.report(id).unwrap().phase,
        ImportPhase::Download
    );
    settle(&mut h);
    let record = h.shell.st.session.imports.report(id).unwrap().clone();
    assert_eq!(record.phase, ImportPhase::Complete, "{:?}", record.error);
    assert!(root.join("library/web-excerpt-1.obr").is_file());
    assert!(repos.fetched.lock().unwrap().contains(&source.url));
    let m = &h.shell.st.session.library.get(id).unwrap().manifest;
    assert_eq!(m.attribution, source.attribution);
    assert_eq!(m.license, source.spdx);

    // Back on Get Resources the row says Installed.
    let tab = h.tab_at("Get Resources");
    h.click(tab);
    scroll_to(&mut h, OPEN_ROW);
    assert!(
        row_button(&h, OPEN_ROW, "Installed").y > 0.0,
        "{:?}",
        h.texts()
    );
    preview(&h, "get-resources-open-catalogue-installed");

    // Help → Credits lists it under Downloaded open Resources, from its own manifest.
    h.open_menu("Help");
    h.click_text("Credits");
    h.frames(2);
    let tab = h.tab_id("Credits");
    let mut seen = std::collections::BTreeSet::new();
    let pane = {
        let tiles = &h.shell.tree.tiles;
        tiles.rect(tiles.find_pane(&tab).unwrap()).unwrap()
    };
    for _ in 0..400 {
        seen.extend(
            h.shapes
                .iter()
                .filter(|(r, _)| pane.contains(r.center()))
                .map(|(_, t)| t.clone()),
        );
        if seen.contains("DOWNLOADED OPEN RESOURCES") && seen.contains(OPEN_ROW) {
            break;
        }
        h.events.push(Event::PointerMoved(pane.center()));
        h.events.push(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, -120.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: Modifiers::NONE,
        });
        h.frame();
    }
    assert!(seen.contains("DOWNLOADED OPEN RESOURCES"), "{seen:?}");
    assert!(seen.contains(OPEN_ROW), "{seen:?}");
    // The licence in words (#307), its SPDX id on hover.
    let licence = super::super::get_bibles_tab::licence_name(&source.spdx);
    assert!(seen.contains(&licence), "{seen:?}");
    drop(h);
    std::fs::remove_dir_all(root).unwrap();
}

/// The Open catalogue is compiled in: it lists before (and without) the SWORD catalog.
#[test]
fn the_open_catalogue_lists_while_the_sword_catalog_is_not_loaded() {
    let mut h = Harness::new();
    h.shell
        .st
        .session
        .run("library.get_resources", &json!({}))
        .unwrap();
    h.frames(3);
    assert!(h.shell.st.session.get_bibles.catalog().is_none());
    let n = obs_engine::open_catalogue::open_sources().len();
    assert!(
        h.find(&format!("OPEN CATALOGUE · {n}")).is_some(),
        "{:?}",
        h.texts()
    );
    assert!(h.find("Young's Literal Translation").is_some());
}
