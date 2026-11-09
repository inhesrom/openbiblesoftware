//! Help → User guide (#218) through the headless harness: the Tab from the Help menu, its
//! pages drawn by the Notes renderer with their screenshots, links between pages, and its
//! search box.

use std::path::Path;

use obs_engine::{GuideBlock, TabId, USER_GUIDE, guide_blocks, guide_images};

use super::*;
use crate::shell::user_guide_tab::{GUIDE_IMAGES, decode_png};
use crate::tokens::size;

fn preview(h: &Harness, name: &str) {
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
        std::fs::create_dir_all(&dir).unwrap();
        h.screenshot(&dir.join(format!("{name}.ppm")));
    }
}

/// Frames until the page's lines are laid out (by the layout workers).
fn settle_guide(h: &mut Harness) {
    let mut idle = 0;
    for _ in 0..300 {
        h.paced_frame();
        let busy = h.shell.st.notes_ui.has_pending();
        idle = if busy { 0 } else { idle + 1 };
        if idle >= 3 {
            return;
        }
    }
}

fn guide_tab(h: &Harness) -> TabId {
    h.shell
        .st
        .session
        .workspace
        .tabs()
        .find(|t| t.kind == TabKind::UserGuide)
        .expect("a User guide Tab")
        .id
}

fn page(h: &Harness) -> &'static str {
    h.shell.st.session.user_guide().page().id
}

fn drawn(h: &Harness) -> Vec<String> {
    h.shell.st.user_guide_ui.drawn.clone()
}

/// The centre of `part` of the drawn line `line`.
fn spot(h: &Harness, line: &str, part: &str) -> Pos2 {
    let l = (h.shell.st.notes_ui.drawn_lines.iter())
        .find(|l| l.laid.text == line)
        .unwrap_or_else(|| panic!("{line:?} not drawn"));
    let at = l.laid.text.find(part).unwrap();
    let rects = crate::compare_view::range_rects(&l.laid, &(at..at + part.len()));
    let [x0, y0, x1, y1] = rects[0];
    l.text_rect.min + vec2((x0 + x1) / 2.0, (y0 + y1) / 2.0)
}

/// Wheel the page down until `done` holds.
fn wheel_until(h: &mut Harness, tab: TabId, what: &str, done: impl Fn(&Harness) -> bool) {
    let tiles = &h.shell.tree.tiles;
    let pane = tiles.rect(tiles.find_pane(&tab).unwrap()).unwrap();
    for _ in 0..200 {
        if done(h) {
            // Wheel scrolling is animated: let it stop.
            h.frames(30);
            return;
        }
        h.events.push(Event::PointerMoved(pane.center()));
        h.events.push(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, -60.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: Modifiers::NONE,
        });
        h.paced_frame();
    }
    panic!("{what} never scrolled into view");
}

/// Where `text` is painted as a widget's text inside `tab`'s Panel, right of the page list.
fn in_page(h: &Harness, tab: TabId, text: &str) -> Pos2 {
    let tiles = &h.shell.tree.tiles;
    let pane = tiles.rect(tiles.find_pane(&tab).unwrap()).unwrap();
    h.shapes
        .iter()
        .find(|(r, t)| {
            t == text && pane.contains(r.center()) && r.left() > pane.left() + size::SIDE_LIST
        })
        .map(|(r, _)| r.center())
        .unwrap_or_else(|| panic!("{text:?} not in the page: {:?}", h.texts()))
}

/// Acceptance (#218): Help → User guide opens the offline guide in a Tab, with a page for every
/// main feature, drawn by the Notes renderer, with its screenshots.
#[test]
fn help_user_guide_opens_every_page_in_one_tab_with_its_screenshots() {
    let mut h = Harness::new();
    let tb = h.tab_id("TB");
    h.open_menu("Help");
    h.click_text("User guide");
    let tab = guide_tab(&h);
    // It wants the room: it opens with the Bibles, not beside the Library.
    let ws = &h.shell.st.session.workspace;
    assert_eq!(ws.panel_of(tab), ws.panel_of(tb));
    settle_guide(&mut h);
    assert_eq!(page(&h), "getting-started");
    // Every page is listed.
    for p in USER_GUIDE {
        assert!(h.find(p.title).is_some(), "{} not listed", p.title);
    }
    let shown = drawn(&h);
    assert_eq!(shown[0], "Getting started");
    assert!(
        shown
            .iter()
            .any(|l| l.starts_with("The window: the menu bar"))
    );
    // Its heading is drawn as a Note's heading is.
    let heading = (h.shell.st.notes_ui.drawn_lines.iter())
        .find(|l| l.laid.text == "Getting started")
        .unwrap();
    assert_eq!(heading.kind, obs_model::NoteLineKind::Heading(1));
    // The screenshot is decoded and drawn.
    assert!(
        (h.shell.st.user_guide_ui).has_texture("user-guide-window.png"),
        "the screenshot was not drawn"
    );
    preview(&h, "user-guide-getting-started-kittest");

    // Each page shows when clicked in the list.
    for p in USER_GUIDE.iter().skip(1) {
        h.click_text(p.title);
        settle_guide(&mut h);
        assert_eq!(page(&h), p.id);
        assert_eq!(drawn(&h)[0], p.title);
        for file in guide_images(p) {
            assert!(h.shell.st.user_guide_ui.has_texture(&file), "{file}");
        }
    }
    preview(&h, "user-guide-keyboard-kittest");
    // F1 brings it back to the front, the same Tab.
    h.shell.run("tab.close", &json!({ "tab": tab.0 }));
    h.frames(2);
    h.shell.run("help.user_guide", &json!({}));
    h.frames(2);
    assert_eq!(
        (h.shell.st.session.workspace.tabs())
            .filter(|t| t.kind == TabKind::UserGuide)
            .count(),
        1
    );
}

/// A link to another page shows that page; Next at the bottom shows the next one.
#[test]
fn a_link_to_another_page_shows_it() {
    let mut h = Harness::new();
    h.shell.run("help.user_guide", &json!({}));
    settle_guide(&mut h);
    let tab = guide_tab(&h);
    let line = (h.shell.st.notes_ui.drawn_lines.iter())
        .map(|l| l.laid.text.clone())
        .find(|t| t.starts_with("Read on in this guide"))
        .unwrap();
    let tiles = &h.shell.tree.tiles;
    let pane = tiles.rect(tiles.find_pane(&tab).unwrap()).unwrap();
    wheel_until(&mut h, tab, "the line of links", |h| {
        pane.shrink(40.0).contains(spot(h, &line, "Link sets"))
    });
    let at = spot(&h, &line, "Link sets");
    h.click(at);
    settle_guide(&mut h);
    assert_eq!(page(&h), "link-sets");
    // Scrolled to the bottom, Next shows the next page.
    wheel_until(&mut h, tab, "Next", |h| {
        (h.shapes.iter())
            .any(|(r, t)| t == "Next page: Guides" && pane.shrink(20.0).contains(r.center()))
    });
    h.click(in_page(&h, tab, "Next page: Guides"));
    settle_guide(&mut h);
    assert_eq!(page(&h), "guides");
}

/// The header box searches every page; a result shows its page, and clears the search.
#[test]
fn the_guide_search_lists_the_pages_that_match_and_a_click_shows_one() {
    let mut h = Harness::new();
    h.shell.run("help.user_guide", &json!({}));
    settle_guide(&mut h);
    let tab = guide_tab(&h);
    let field = h.find("Search the guide").expect("the search box");
    h.click(field.center());
    h.events.push(Event::Text("@aorist".into()));
    h.frames(3);
    let shown = drawn(&h);
    assert_eq!(shown[0], "Search", "{shown:?}");
    assert!(shown[1].contains("@aorist imperative"), "{shown:?}");
    preview(&h, "user-guide-search-kittest");
    h.click(in_page(&h, tab, "Search"));
    settle_guide(&mut h);
    assert_eq!(page(&h), "search");
    assert_eq!(drawn(&h)[0], "Search");
    assert!(h.shell.st.tabs[&tab].filter.is_empty());

    h.shell.st.tabs.get_mut(&tab).unwrap().filter = "zzyzx".into();
    h.frames(2);
    assert_eq!(drawn(&h), ["Nothing in the User guide matches “zzyzx”."]);
}

/// Every screenshot a page names is compiled in and decodes, and every one compiled in is used.
#[test]
fn every_guide_screenshot_is_compiled_in_and_decodes() {
    let mut used = Vec::new();
    for p in USER_GUIDE {
        for block in guide_blocks(p) {
            if let GuideBlock::Image { file, alt } = block {
                assert!(!alt.is_empty(), "{file} has a description");
                let bytes = (GUIDE_IMAGES.iter())
                    .find(|(f, _)| *f == file)
                    .unwrap_or_else(|| panic!("{file} is not compiled in"))
                    .1;
                let image = decode_png(bytes).unwrap_or_else(|| panic!("{file} is not a PNG"));
                assert!(image.size[0] >= 800, "{file} is too small to read");
                used.push(file);
            }
        }
    }
    for (file, _) in GUIDE_IMAGES {
        assert!(
            used.iter().any(|u| u == file),
            "{file} is compiled in but unused"
        );
    }
}
