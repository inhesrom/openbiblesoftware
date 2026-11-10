//! Headers and rows that fit a narrow Panel (#303): in a side Panel at its default width, and
//! at a Larger Interface size, Search's mode and query rows, its Analysis table and Library
//! group headers, the Notes Tab's header and cards, a Bible Tab's header and Settings lay out
//! within the Panel, with no control drawn over another; and a new Tab at the end of a full
//! tab bar is scrolled into view.
//!
//! Each case opens as the QA sweep did, in its window (1280 × 800 points), on the Screenshot
//! gallery's fixtures; the gallery draws the same cases ([`panel_fit`]) as regression images.

use obs_engine::TabId;
use obs_scripture::{Book, Passage, Verse};

use super::a11y_audit::Overlay;
use super::gallery::{LOOKS, launch, settle_search};
use super::missing_resource::settle;
use super::*;
use crate::tokens::{size, space};

/// The QA sweep's window, in points: the default side Panel is about 420 wide.
const WINDOW: egui::Vec2 = egui::vec2(1280.0, 800.0);

/// A Commentary with a long title, as the JFB is in the Starter library.
const LONG_TITLE: &str = "Commentary Critical and Explanatory on the Whole Bible";

fn focused_tab(h: &Harness) -> TabId {
    h.shell.session().workspace.focused().unwrap()
}

/// The Tab's Panel, below its tab bar.
fn pane(h: &Harness, tab: TabId) -> Rect {
    let tiles = &h.shell.tree.tiles;
    tiles.rect(tiles.find_pane(&tab).unwrap()).unwrap()
}

/// Whether Tab `tab` is in the rightmost Panel, beside another.
fn in_side_panel(h: &Harness, tab: TabId) -> bool {
    let panels = h.panels();
    panels.len() > 1
        && h.shell.tree.tiles.find_pane(&tab).is_some()
        && pane(h, tab).right() >= panels.last().unwrap().0.right() - 0.5
}

/// Texts and icons drawn in `area` (by their centre), top to bottom.
fn shapes_in(h: &Harness, area: Rect) -> Vec<(Rect, &str)> {
    let mut out: Vec<(Rect, &str)> = (h.shapes.iter())
        .filter(|(r, _)| area.contains(r.center()))
        .map(|(r, t)| (*r, t.as_str()))
        .collect();
    out.sort_by(|a, b| a.0.top().total_cmp(&b.0.top()));
    out
}

/// Where `text` is drawn in Tab `tab`'s Panel (the topmost).
fn in_pane(h: &Harness, tab: TabId, text: &str) -> Option<Rect> {
    (shapes_in(h, pane(h, tab)).into_iter())
        .find(|(_, t)| *t == text)
        .map(|(r, _)| r)
}

fn at(h: &Harness, tab: TabId, text: &str) -> Rect {
    in_pane(h, tab, text).unwrap_or_else(|| {
        panic!(
            "{text:?} not in the Panel: {:?}",
            shapes_in(h, pane(h, tab))
        )
    })
}

/// No text or icon in `area` runs past its right edge (where the Panel would cut it off).
fn assert_within(h: &Harness, area: Rect, what: &str) {
    let past: Vec<_> = (shapes_in(h, area).into_iter())
        .filter(|(r, _)| r.right() > area.right() + 0.5)
        .collect();
    assert!(
        past.is_empty(),
        "{what}: drawn past the Panel's right edge at {}: {past:?}",
        area.right()
    );
}

/// `a` and `b` are at least `gap` apart, side by side, or one above the other.
fn assert_apart_by(a: (Rect, &str), b: (Rect, &str), gap: f32, what: &str) {
    let apart = a.0.right() + gap <= b.0.left()
        || b.0.right() + gap <= a.0.left()
        || a.0.bottom() <= b.0.top()
        || b.0.bottom() <= a.0.top();
    assert!(apart, "{what}: {:?} and {:?} touch or overlap", a, b);
}

/// `a` and `b` do not touch: a gap between them that reads as one.
fn assert_apart(a: (Rect, &str), b: (Rect, &str), what: &str) {
    assert_apart_by(a, b, space::XS - 0.5, what);
}

/// A label or Button drawn on one line (not wrapped inside its Button).
fn assert_one_line(r: Rect, text: &str) {
    assert!(
        r.height() < 1.5 * crate::tokens::text::BODY * 1.3,
        "{text:?} is wrapped onto more lines: {r:?}"
    );
}

/// The window the QA sweep used, and the Interface size. A Larger size zooms the whole
/// window: the same window holds fewer points, as on the desktop.
fn window(h: &mut Harness, larger: bool) {
    h.screen = WINDOW;
    if larger {
        h.shell
            .run("view.interface_size", &json!({ "value": "larger" }));
        h.screen = WINDOW / obs_engine::appearance::InterfaceSize::Larger.zoom();
    }
    h.frames(4);
}

/// Run jobs and draw until the frame settles (search indexes, laid-out text, measured rows).
fn settled(h: &mut Harness) {
    settle(h);
    settle_all(h);
    h.frames(4);
}

fn wheel(h: &mut Harness, over: Pos2, delta: egui::Vec2) {
    h.events.push(Event::PointerMoved(over));
    h.events.push(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta,
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: Modifiers::NONE,
    });
    h.frames(3);
}

// ---------------------------------------------------------------------------------------
// The cases: what each opens. The gallery draws each in every Look.

/// SEARCH-1, SETUP-1, SEARCH-8: Search in the default side Panel, a phrase searched.
fn search_side(h: &mut Harness) {
    search_in_side_panel(h, false);
}

/// SEARCH-1 at a Larger Interface size.
fn search_side_larger(h: &mut Harness) {
    search_in_side_panel(h, true);
}

fn search_in_side_panel(h: &mut Harness, larger: bool) {
    window(h, larger);
    h.shell.st.session.refresh_search_indexes();
    settle_search(h);
    h.shell.run("tool.search", &json!({}));
    h.frames(2);
    let tab = focused_tab(h);
    h.shell.run(
        "search.run",
        &json!({ "tab": tab.0, "text": "\"the earth was without form\"" }),
    );
    settle_search(h);
    settled(h);
}

/// SEARCH-2: `strong:G26` over KJV and TAGNT in the Analysis view, in `panel` (the side
/// Panel when `None`).
fn analysis_in(h: &mut Harness, centre: bool) {
    window(h, false);
    h.shell.st.session.refresh_search_indexes();
    settle_search(h);
    let mut params = json!({});
    if centre {
        let kjv = h.tab_id("KJV");
        let left = h.shell.session().workspace.panel_of(kjv).unwrap();
        params["panel"] = json!(left.0);
    }
    h.shell.run("tool.search", &params);
    h.frames(2);
    let tab = focused_tab(h);
    h.shell.run(
        "search.run",
        &json!({ "tab": tab.0, "text": "strong:G26", "bibles": "kjv-strongs,tagnt-love" }),
    );
    settle_search(h);
    h.shell
        .run("search.view", &json!({ "tab": tab.0, "value": "analysis" }));
    settle_search(h);
    settled(h);
}

fn analysis_side(h: &mut Harness) {
    analysis_in(h, false);
}

fn analysis_centre(h: &mut Harness) {
    analysis_in(h, true);
}

/// SEARCH-8: Library mode in the side Panel, finding a paragraph of a Commentary with a long
/// title.
fn library_search_side(h: &mut Harness) {
    window(h, false);
    let jfb = obs_engine::testing::commentary(
        "jfb",
        LONG_TITLE,
        "JFB",
        &[(
            Passage::verse(Verse::new(Book::John, 3, 16)),
            None,
            "16. For God so loved the world--What proclamation of the Gospel has been so oft \
             on the lips of missionaries and preachers in every age?",
        )],
    );
    let library = &mut h.shell.st.session.library;
    let path = library.directory().unwrap().join("jfb.obr");
    obs_store::write_obr(&jfb, &path).unwrap();
    library.add_file(&path).unwrap();
    h.shell.st.session.refresh_search_indexes();
    settle_search(h);
    h.shell.run("tool.search", &json!({}));
    h.frames(2);
    let tab = focused_tab(h);
    h.shell
        .run("search.mode", &json!({ "tab": tab.0, "value": "library" }));
    h.shell.run(
        "search.run",
        &json!({ "tab": tab.0, "text": "proclamation" }),
    );
    settle_search(h);
    settled(h);
}

/// SETUP-2: the Notes Tab in the side Panel, with a Note.
fn notes_side(h: &mut Harness) {
    window(h, false);
    h.shell.run(
        "annotation.note",
        &json!({ "passage": "Gen 1:1", "body": "Beginnings: before all things." }),
    );
    h.shell.run("tool.notes", &json!({}));
    settled(h);
}

/// WORKSPACE-MAC-5: a tagged Bible (it has Interlinear, as KJV+Strong's has) opened in the
/// side Panel.
fn bible_side(h: &mut Harness) {
    window(h, false);
    let library = h.tab_id("Library");
    let right = h.shell.session().workspace.panel_of(library).unwrap();
    let tab = h.shell.run(
        "resource.open",
        &json!({ "id": "tagnt-grace", "panel": right.0 }),
    );
    let tab = tab.unwrap()["tab"].as_u64().unwrap();
    h.shell
        .run("tab.go", &json!({ "tab": tab, "text": "Luke 1:30" }));
    settled(h);
}

/// WORKSPACE-MAC-5 at a Larger Interface size, in the Bible + Commentary Layout.
fn bible_and_commentary_larger(h: &mut Harness) {
    window(h, true);
    h.shell
        .run("layout.builtin", &json!({ "name": "Bible + Commentary" }))
        .unwrap();
    settled(h);
}

/// WORKSPACE-MAC-7: Settings in the side Panel, scrolled down to Keyboard shortcuts.
fn settings_side(h: &mut Harness) {
    window(h, false);
    h.shell.run("app.settings", &json!({}));
    settled(h);
    let tab = focused_tab(h);
    let middle = pane(h, tab).center();
    for _ in 0..200 {
        if in_pane(h, tab, "KEYBOARD SHORTCUTS").is_some_and(|r| r.top() < middle.y) {
            break;
        }
        wheel(h, middle, vec2(0.0, -120.0));
    }
    h.frames(4);
}

/// WORKSPACE-MAC-4: every other Resource of the library opened into the main Panel, more than
/// its tab bar shows, and a Passage Guide opened at its end.
fn full_tab_bar(h: &mut Harness) {
    window(h, false);
    let kjv = h.tab_id("KJV");
    let main = h.shell.session().workspace.panel_of(kjv).unwrap();
    let ids: Vec<String> = (h.shell.st.session.library.entries().iter())
        .map(|e| e.manifest.resource_id.clone())
        .filter(|id| id != "kjv-strongs")
        .collect();
    for id in ids {
        h.shell
            .run("resource.open", &json!({ "id": id, "panel": main.0 }))
            .unwrap_or_else(|| panic!("{id} did not open"));
        h.frames(2);
    }
    h.shell.run(
        "guide.passage_guide",
        &json!({ "passage": "Jn 3:16", "panel": main.0 }),
    );
    settled(h);
}

fn shows(h: &Harness, text: &str) -> bool {
    h.find(text).is_some()
}

/// The cases as the gallery draws them: a name, what opens it, and what shows it is open.
pub(super) fn panel_fit() -> Vec<Overlay> {
    vec![
        ("Search in a side Panel", search_side, |h| {
            shows(h, "Saved & history") && in_side_panel(h, focused_tab(h))
        }),
        (
            "Search in a side Panel, Larger Interface size",
            search_side_larger,
            |h| shows(h, "Saved & history") && in_side_panel(h, focused_tab(h)),
        ),
        ("Search Analysis in a side Panel", analysis_side, |h| {
            shows(h, "Reference") && in_side_panel(h, focused_tab(h))
        }),
        ("Search Analysis in a centre Panel", analysis_centre, |h| {
            shows(h, "Context") && !in_side_panel(h, focused_tab(h))
        }),
        ("Library Search in a side Panel", library_search_side, |h| {
            shows(h, " · 1 hit")
        }),
        ("Notes in a side Panel", notes_side, |h| {
            shows(h, "Last modified") && shows(h, "Genesis 1:1")
        }),
        ("Bible Tab in a side Panel", bible_side, |h| {
            (h.shell.session().workspace.tabs())
                .any(|t| t.title == "TAGNT" && in_side_panel(h, t.id))
        }),
        (
            "Bible and Commentary, Larger Interface size",
            bible_and_commentary_larger,
            |h| shows(h, "MHC") && shows(h, "KJV"),
        ),
        (
            "Settings in a side Panel, Keyboard shortcuts",
            settings_side,
            |h| shows(h, "KEYBOARD SHORTCUTS"),
        ),
        (
            "A new Tab at the end of a full tab bar",
            full_tab_bar,
            |h| shows(h, "Passage Guide"),
        ),
    ]
}

/// A fresh gallery launch (Light) with case `name` open.
fn open_case(name: &str) -> (Harness, super::missing_resource::Lab) {
    let (_, open, is_open) = (panel_fit().into_iter())
        .find(|(n, ..)| *n == name)
        .unwrap();
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let (mut h, lab) = launch(&format!("panel-fit-{n}"), &LOOKS[0]);
    open(&mut h);
    assert!(is_open(&h), "{name} is not open: {:?}", h.texts());
    (h, lab)
}

/// With `UPDATE_PREVIEWS`, each case in Light and Dark to `target/previews/panel-fit/`: the
/// before and after images of #303's PR, made once on the branch it is based on and once on
/// its own.
#[test]
fn panel_fit_previews() {
    if std::env::var_os("UPDATE_PREVIEWS").is_none() {
        return;
    }
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews/panel-fit");
    for (name, open, _) in panel_fit() {
        for (look, slug) in LOOKS.iter().zip(["light", "dark"]) {
            let (mut h, _lab) = launch(&format!("panel-fit-preview-{slug}"), look);
            open(&mut h);
            let file: String = (format!("{name} {slug}").chars())
                .map(|c| {
                    if c.is_ascii_alphanumeric() {
                        c.to_ascii_lowercase()
                    } else {
                        '-'
                    }
                })
                .collect();
            h.screenshot(&dir.join(format!("{file}.ppm")));
        }
    }
}

// ---------------------------------------------------------------------------------------
// The checks.

/// The Search Tab's top band: its modes, query row and Scope, above the results.
fn search_header(h: &Harness, tab: TabId) -> Rect {
    let p = pane(h, tab);
    let scope = at(h, tab, "Scope");
    Rect::from_min_max(p.min, pos2(p.right(), scope.bottom()))
}

fn check_search_header(h: &Harness, what: &str) {
    let tab = focused_tab(h);
    assert!(in_side_panel(h, tab), "{what}: Search is in the side Panel");
    let morphology = at(h, tab, "Morphology…");
    let saved = at(h, tab, "Saved & history");
    assert_apart(
        (morphology, "Morphology…"),
        (saved, "Saved & history"),
        what,
    );
    for button in [
        "Search",
        "Recent",
        "Save…",
        "Morphology…",
        "Saved & history",
    ] {
        assert_one_line(at(h, tab, button), button);
    }
    // Nothing in the band drawn over anything else.
    let band = shapes_in(h, search_header(h, tab));
    for (i, a) in band.iter().enumerate() {
        for b in &band[i + 1..] {
            assert_apart_by(*a, *b, 0.0, what);
        }
    }
    assert_within(h, search_header(h, tab), what);
}

/// SEARCH-1, SETUP-1: Morphology… and Saved & history are separate, readable buttons at
/// the default and a Larger Interface size, and no Button wraps its label ("Save / …").
#[test]
fn search_mode_row_keeps_its_buttons_apart_in_a_side_panel() {
    let (h, _lab) = open_case("Search in a side Panel");
    check_search_header(&h, "default size");
    let (h, _lab) = open_case("Search in a side Panel, Larger Interface size");
    check_search_header(&h, "Larger size");
}

/// SEARCH-8: in a narrow Search Tab the query box takes the whole row and the buttons go
/// under it, so a phrase shows whole.
#[test]
fn search_query_box_takes_the_row_in_a_side_panel() {
    let (h, _lab) = open_case("Search in a side Panel");
    let tab = focused_tab(&h);
    let query = at(&h, tab, "\"the earth was without form\"");
    let search = at(&h, tab, "Search");
    assert!(
        search.top() >= query.bottom(),
        "the buttons are under the query box: {query:?} {search:?}"
    );
    let field = pane(&h, tab).width() - 2.0 * space::L;
    assert!(
        query.width() < field - 2.0 * space::M,
        "the phrase fits the box"
    );
    assert_within(&h, search_header(&h, tab), "query row");

    // In a wide Tab the buttons stay beside the box.
    let mut h = h;
    h.screen = vec2(2400.0, 800.0);
    h.frames(6);
    let query = at(&h, tab, "\"the earth was without form\"");
    let search = (shapes_in(&h, pane(&h, tab)).into_iter())
        .filter(|(r, t)| *t == "Search" && r.top() >= query.top() - space::M)
        .map(|(r, _)| r)
        .next()
        .expect("the Search button");
    assert!(
        (search.center().y - query.center().y).abs() < space::M,
        "the buttons are beside the box in a wide Tab: {query:?} {search:?}"
    );
}

/// SEARCH-2: in a centre Panel the Analysis table shows all six columns; in a side Panel it
/// scrolls sideways, with a scroll bar that shows, to Morphology and Context.
#[test]
fn analysis_shows_every_column_in_a_centre_panel_and_scrolls_in_a_side_panel() {
    const COLUMNS: [&str; 6] = [
        "Reference",
        "Bible",
        "Word",
        "Lemma",
        "Morphology",
        "Context",
    ];
    let (h, _lab) = open_case("Search Analysis in a centre Panel");
    let tab = focused_tab(&h);
    let p = pane(&h, tab);
    for column in COLUMNS {
        let r = at(&h, tab, column);
        assert!(
            r.right() <= p.right(),
            "{column} is in view in the centre Panel: {r:?} in {p:?}"
        );
    }

    let (mut h, _lab) = open_case("Search Analysis in a side Panel");
    let tab = focused_tab(&h);
    let p = pane(&h, tab);
    // A scroll bar along the bottom of the table shows that it scrolls sideways: a thumb in
    // a colour of its own over what is under it.
    let rects = &h.rects;
    let thumb = rects.iter().any(|(r, c)| {
        p.expand(0.5).contains_rect(*r)
            && r.bottom() > p.bottom() - size::SCROLLBAR
            && r.height() <= size::SCROLLBAR
            && r.width() > size::THUMB_MIN
            && r.width() < p.width() - size::THUMB_MIN
            && c.a() > 0
            && (rects.iter())
                .filter(|(under, _)| under.contains_rect(*r) && under.area() > r.area() * 4.0)
                .all(|(_, under)| under != c)
    });
    let low: Vec<_> = (h.rects.iter())
        .filter(|(r, _)| r.bottom() > p.bottom() - size::SCROLLBAR)
        .collect();
    assert!(thumb, "no sideways scroll bar in {p:?}: {low:?}");
    let context = at(&h, tab, "Context");
    assert!(
        context.right() > p.right(),
        "Context is past the Panel's edge"
    );
    // Scrolled sideways, the header follows the rows: Context comes into view.
    let row = at(&h, tab, "1 John 4:8").center();
    for _ in 0..20 {
        wheel(&mut h, row, vec2(-120.0, 0.0));
    }
    let context = at(&h, tab, "Context");
    assert!(
        context.right() <= p.right(),
        "scrolled to Context: {context:?} in {p:?}"
    );
}

/// SEARCH-8: a Library group's header keeps its hit count in view, its Resource's name cut
/// short with "…" instead.
#[test]
fn library_group_header_keeps_its_hit_count_in_a_side_panel() {
    let (h, _lab) = open_case("Library Search in a side Panel");
    let tab = focused_tab(&h);
    let p = pane(&h, tab);
    let hits = at(&h, tab, " · 1 hit");
    assert!(
        hits.right() <= p.right() - size::SCROLLBAR,
        "the count is clear of the scroll bar: {hits:?} in {p:?}"
    );
    let name = shapes_in(&h, p)
        .into_iter()
        .find(|(r, t)| t.starts_with("JFB — ") && (r.center().y - hits.center().y).abs() < 1.0);
    let (name, _) = name.expect("the Resource's name beside its count");
    assert!(name.right() <= hits.left() + 0.5, "{name:?} {hits:?}");
}

/// SETUP-2: the Notes Tab's order switch and count, and a card's reference, Notebook and
/// menu, are apart and within the side Panel; its empty state wraps within it too.
#[test]
fn notes_header_and_cards_fit_a_side_panel() {
    let (h, _lab) = open_case("Notes in a side Panel");
    let tab = focused_tab(&h);
    assert!(in_side_panel(&h, tab));
    let p = pane(&h, tab);
    let modified = at(&h, tab, "Last modified");
    let count = at(&h, tab, "1 Annotation");
    assert_apart(
        (modified, "Last modified"),
        (count, "1 Annotation"),
        "header",
    );
    let reference = at(&h, tab, "Genesis 1:1");
    let notebook = at(&h, tab, "My Notebook");
    assert_apart(
        (reference, "Genesis 1:1"),
        (notebook, "My Notebook"),
        "card",
    );
    let menu = at(&h, tab, "…");
    assert_apart((reference, "Genesis 1:1"), (menu, "…"), "card");
    assert_apart((notebook, "My Notebook"), (menu, "…"), "card");
    assert_within(&h, p, "Notes with a Note");

    // Empty: the message wraps within the Panel.
    let (mut h, _lab) = launch("panel-fit-notes-empty", &LOOKS[0]);
    window(&mut h, false);
    h.shell.run("tool.notes", &json!({}));
    settled(&mut h);
    let tab = focused_tab(&h);
    assert!(in_side_panel(&h, tab));
    assert!(
        (h.texts().iter()).any(|t| t.starts_with("No Notes or Highlights yet")),
        "{:?}",
        h.texts()
    );
    assert_within(&h, pane(&h, tab), "Notes, empty");
}

/// The Go-to box of Tab `tab`'s header and what it shows.
fn reference_box(h: &Harness, tab: TabId) -> Rect {
    let p = pane(h, tab);
    let tree = h.tree.as_ref().expect("an AccessKit tree");
    (tree.nodes.iter())
        .filter(|(_, n)| n.label() == Some("Go to passage"))
        .filter_map(|(_, n)| n.bounds())
        .map(|b| {
            Rect::from_min_max(
                pos2(b.x0 as f32, b.y0 as f32),
                pos2(b.x1 as f32, b.y1 as f32),
            )
        })
        .find(|r| p.contains(r.center()))
        .expect("the Go-to box")
}

/// A Bible Tab's header: its Go-to box shows a whole reference, and nothing else in the
/// header is drawn over the box.
fn check_bible_header(h: &Harness, tab: TabId, reference: &str, what: &str) {
    let p = pane(h, tab);
    let shown = at(h, tab, reference);
    let field = reference_box(h, tab);
    assert!(
        field.contains_rect(shown),
        "{what}: the box shows {reference:?} whole: {shown:?} in {field:?}"
    );
    let header = Rect::from_min_max(p.min, pos2(p.right(), field.bottom() + size::CONTROL));
    for (r, t) in shapes_in(h, header) {
        if t != reference {
            assert!(
                !r.intersects(field),
                "{what}: {t:?} is drawn over the Go-to box: {r:?} {field:?}"
            );
        }
    }
    assert_within(h, header, what);
}

/// WORKSPACE-MAC-5: a Bible in the side Panel has a Go-to box that shows a whole reference;
/// its parallel resources and Interlinear go on a second line of the header, Interlinear
/// whole (not "Int.").
#[test]
fn a_bible_header_in_a_side_panel_shows_a_whole_reference() {
    let (h, _lab) = open_case("Bible Tab in a side Panel");
    let tab = (h.shell.session().workspace.tabs())
        .find(|t| t.title == "TAGNT" && in_side_panel(&h, t.id))
        .map(|t| t.id)
        .expect("TAGNT in the side Panel");
    check_bible_header(&h, tab, "Luke 1:30", "side Panel");
    let field = reference_box(&h, tab);
    let interlinear = at(&h, tab, "Interlinear");
    assert!(
        interlinear.top() >= field.bottom(),
        "Interlinear is on the second line: {interlinear:?} {field:?}"
    );
}

/// WORKSPACE-MAC-5 at a Larger Interface size: the Bible and the Commentary each show a
/// whole reference, and a Resource's name is shown with room or not at all (never a bare
/// "…").
#[test]
fn bible_and_commentary_headers_at_a_larger_size_show_whole_references() {
    let (h, _lab) = open_case("Bible and Commentary, Larger Interface size");
    let kjv = h.tab_id("KJV");
    let mhc = h.tab_id("MHC");
    for (tab, what) in [(kjv, "KJV"), (mhc, "MHC")] {
        let p = pane(&h, tab);
        let field = reference_box(&h, tab);
        let shown = shapes_in(&h, field)
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("{what}: the box is empty"));
        assert!(
            field.contains_rect(shown.0),
            "{what}: the box shows {:?} whole: {field:?}",
            shown
        );
        check_bible_header(&h, tab, shown.1, what);
        let header = Rect::from_min_max(p.min, pos2(p.right(), field.bottom()));
        for (r, t) in shapes_in(&h, header) {
            if t == "…" {
                panic!("{what}: a bare … in the header at {r:?}");
            }
        }
    }
}

/// WORKSPACE-MAC-7: Settings in the side Panel wraps every description within the Panel,
/// all the way down, and each Keyboard shortcut's key, Clear and Reset are in view.
#[test]
fn settings_in_a_side_panel_lays_out_within_it_all_the_way_down() {
    let (mut h, _lab) = open_case("Settings in a side Panel, Keyboard shortcuts");
    let tab = focused_tab(&h);
    assert!(in_side_panel(&h, tab));
    let p = pane(&h, tab);
    assert_within(&h, p, "Settings, at Keyboard shortcuts");
    let clear = at(&h, tab, "Clear");
    assert!(clear.right() <= p.right(), "Clear is in view: {clear:?}");
    // Back to the top, then down a screen at a time.
    for _ in 0..200 {
        wheel(&mut h, p.center(), vec2(0.0, 400.0));
        if in_pane(&h, tab, "Settings").is_some() {
            break;
        }
    }
    for step in 0..60 {
        assert_within(&h, p, &format!("Settings, {step} screens down"));
        let before = h.shapes.clone();
        wheel(&mut h, p.center(), vec2(0.0, -p.height() / 2.0));
        if h.shapes == before {
            break;
        }
    }
}

/// WORKSPACE-MAC-4: a Tab opened at the end of a full tab bar is scrolled into view, its name
/// and Close button clear of the bar's scroll arrow; scrolling the bar away by hand stays put.
#[test]
fn a_new_tab_at_the_end_of_a_full_tab_bar_is_scrolled_into_view() {
    let (h, _lab) = open_case("A new Tab at the end of a full tab bar");
    let guide = h.tab_id("Passage Guide");
    let p = pane(&h, guide);
    let bar = Rect::from_min_max(
        pos2(p.left(), p.top() - size::TAB_BAR),
        pos2(p.right(), p.top()),
    );
    let title = shapes_in(&h, bar)
        .into_iter()
        .find(|(_, t)| *t == "Passage Guide")
        .map(|(r, _)| r)
        .expect("the Passage Guide's Tab in the bar");
    // Right of it: its Close button, then the bar's ▸ arrow and + button.
    let add = shapes_in(&h, bar)
        .into_iter()
        .find(|(_, t)| *t == "[plus]")
        .map(|(r, _)| r)
        .expect("the bar's + button");
    // The last Tab wholly in view, the bar has no ▸ arrow: its × is just left of the +
    // button, and its name right of the ◂ arrow (a square as tall as the bar).
    assert!(
        title.right() + size::ICON <= add.left() - space::S,
        "the new Tab's name and × are in view: {title:?}, the + button at {add:?}"
    );
    assert!(
        title.left() >= p.left() + size::TAB_BAR,
        "the new Tab's name is clear of the ◂ arrow: {title:?}"
    );
    let kjv = |h: &Harness| {
        (shapes_in(h, bar).into_iter())
            .find(|(_, t)| *t == "KJV")
            .map(|(r, _)| r)
    };
    assert!(
        kjv(&h).is_none_or(|r| r.left() < p.left()),
        "the bar scrolled: KJV is out of view at the left: {:?}",
        kjv(&h)
    );

    // Scrolled back by hand, the bar stays there.
    let mut h = h;
    for _ in 0..10 {
        wheel(&mut h, bar.center(), vec2(120.0, 0.0));
    }
    h.frames(10);
    assert!(
        kjv(&h).is_some_and(|r| r.left() >= p.left()),
        "the bar stays where it was scrolled: {:?}",
        kjv(&h)
    );
}
