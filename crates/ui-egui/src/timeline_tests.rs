//! The Timeline (#185) through the shell's real-event harness: 1 Kings 6 brings out the
//! temple's construction beside David's and Solomon's lives in the United Kingdom, the
//! patriarchs and the Exile show where their Passages are, a click on a Lifespan opens its
//! person's Factbook and a click on an Event opens its Passage.

use std::path::{Path, PathBuf};

use obs_engine::testing::{bibles, theographic, tipnr_timeline};
use obs_engine::{Guide, Library, TabId};
use obs_model::Key;
use obs_scripture::{Book, Verse};

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

/// TIPNR, Theographic's events matched to it and a little KJV with 1 Kings 5:1 and 6:1, as
/// `.obr` files in a fresh library.
fn harness(name: &str) -> (Harness, Files) {
    let root = std::env::temp_dir().join(format!("obs-ui-timeline-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let files = Files(root.clone());
    let tipnr = tipnr_timeline();
    let events = theographic(Some(&tipnr));
    let kjv = bibles::bible(
        "kjv",
        "KJV",
        &[
            (
                Verse::new(Book::FirstKings, 5, 1),
                "And Hiram king of Tyre sent his servants unto Solomon.",
            ),
            (
                Verse::new(Book::FirstKings, 6, 1),
                "And it came to pass in the four hundred and eightieth year after the children \
                 of Israel were come out of the land of Egypt, that he began to build the house \
                 of the LORD.",
            ),
        ],
        None,
        &[],
    );
    bibles::write_library(&root.join("library"), &[kjv, tipnr, events]);
    let mut h = Harness::new();
    let s = &mut h.shell.st.session;
    s.library = Library::new();
    assert!(s.library.scan(&root.join("library")).is_empty());
    s.set_alias_cache_dir(root.join("cache"));
    h.shell.reset_layout();
    h.frames(2);
    (h, files)
}

fn pane(h: &Harness, tab: TabId) -> Rect {
    let tiles = &h.shell.tree.tiles;
    let id = tiles.find_pane(&tab).unwrap();
    tiles.rect(id).unwrap()
}

/// Every place `text` is painted in Tab `tab`, top to bottom.
fn all_in(h: &Harness, tab: TabId, text: &str) -> Vec<Rect> {
    let r = pane(h, tab);
    let mut found: Vec<Rect> = h
        .shapes
        .iter()
        .filter(|(s, t)| t == text && r.contains(s.center()))
        .map(|(s, _)| *s)
        .collect();
    found.sort_by(|a, b| a.top().total_cmp(&b.top()));
    found
}

fn until(h: &mut Harness, what: &str, done: impl Fn(&Harness) -> bool) {
    for _ in 0..600 {
        if done(h) {
            return;
        }
        h.paced_frame();
    }
    panic!("never saw {what}: {:?}", h.texts());
}

fn open_timeline(h: &mut Harness, passage: &str) -> TabId {
    let tab = TabId(
        h.shell
            .run(
                "tool.timeline",
                &json!({ "passage": passage, "split": true }),
            )
            .unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    h.frames(2);
    tab
}

/// The second acceptance check, on screen: opening 1 Kings 6 centres the Timeline on the
/// temple's construction and lists it first among the chapter's events, beside the United
/// Kingdom, David's life and Solomon's. A click on David's Lifespan opens his Factbook; a
/// click on the temple's construction moves the Link set's Bible to its Passage.
#[test]
fn opening_1_kings_6_emphasises_the_temple_and_clicks_open_factbooks_and_passages() {
    let (mut h, _files) = harness("temple");
    let timeline = open_timeline(&mut h, "1 Kings 6");
    let temple = "Construction of Solomon's Temple";
    until(&mut h, "the temple on the axis and in the list", |h| {
        all_in(h, timeline, temple).len() == 2
    });
    let texts = h.texts();
    assert!(texts.contains(&"1 Kings 6 · 2 events"), "{texts:?}");
    assert!(texts.contains(&"In 1 Kings 6"), "{texts:?}");
    assert!(texts.contains(&"1012–1005 BC"), "{texts:?}");
    for shown in ["United Kingdom", "David", "Solomon", "Reign of Solomon"] {
        assert!(
            !all_in(&h, timeline, shown).is_empty(),
            "{shown} is not on the axis: {texts:?}"
        );
    }
    // Lifespans are above Events; the temple's label is on the axis, above the list.
    let (david, on_axis) = (
        all_in(&h, timeline, "David")[0],
        all_in(&h, timeline, temple)[0],
    );
    assert!(david.top() < on_axis.top());
    preview(&h, "timeline-1-kings-6-kittest");

    // David's Lifespan: his Factbook.
    h.click(david.center());
    h.frames(3);
    let ws = &h.shell.session().workspace;
    let factbook = ws.tab(ws.focused().unwrap()).unwrap();
    assert_eq!(factbook.kind, TabKind::Guide(Guide::Factbook));
    assert_eq!(
        factbook.entry_history.current(),
        Some(Key::Headword("David@Rut.4.17-Rev".into()))
    );

    // The temple's construction: its Passage, in the Bible.
    h.shell.session_mut().workspace.focus(timeline);
    h.frames(3);
    until(&mut h, "the Timeline again", |h| {
        h.shell.tree.tiles.find_pane(&timeline).is_some() && !all_in(h, timeline, temple).is_empty()
    });
    let on_axis = all_in(&h, timeline, temple)[0];
    h.click(on_axis.center());
    h.frames(3);
    // The Timeline shares Link set A with the Bible, so the Link set moves there.
    until(&mut h, "the Bible at 1 Kings 5:1", |h| {
        h.shell.st.session.workspace.tabs().any(|t| {
            t.kind == TabKind::Resource("kjv".into())
                && t.history.current() == Some(Verse::new(Book::FirstKings, 5, 1))
        })
    });
}

/// The first acceptance check, on screen: the patriarchs, the kingdom and the Exile each
/// show as an Era where their Passages are, with David's life inside the United Kingdom.
#[test]
fn the_patriarchs_kingdom_and_exile_eras_show_with_davids_life() {
    let (mut h, _files) = harness("eras");
    let timeline = open_timeline(&mut h, "Genesis 25:7");
    until(&mut h, "the Patriarchs", |h| {
        !all_in(h, timeline, "Patriarchs").is_empty() && !all_in(h, timeline, "Abraham").is_empty()
    });
    preview(&h, "timeline-patriarchs-kittest");
    h.shell
        .run("tab.go", &json!({ "tab": timeline.0, "text": "1 Kings 6" }));
    until(&mut h, "the United Kingdom", |h| {
        !all_in(h, timeline, "United Kingdom").is_empty()
    });
    // David's life lies inside the Era: his bar starts after it and ends before it does.
    let kingdom = all_in(&h, timeline, "United Kingdom")[0];
    let david = all_in(&h, timeline, "David")[0];
    assert!(david.top() > kingdom.bottom());
    h.shell.run(
        "tab.go",
        &json!({ "tab": timeline.0, "text": "2 Kings 24:18" }),
    );
    until(&mut h, "the Exile", |h| {
        !all_in(h, timeline, "Exile").is_empty()
            && !all_in(h, timeline, "Divided Kingdom").is_empty()
    });
    // The Divided Kingdom ends where the Exile begins: the Exile's name is to its right.
    let divided = all_in(&h, timeline, "Divided Kingdom")[0];
    let exile = all_in(&h, timeline, "Exile")[0];
    assert!(divided.right() < exile.left());
    preview(&h, "timeline-exile-kittest");
}

/// Without a Passage the Timeline shows everything, emphasising nothing; without an index
/// that has events it says what would fill it.
#[test]
fn the_whole_timeline_opens_without_a_passage_and_says_when_it_has_no_events() {
    let (mut h, _files) = harness("whole");
    // The Bible leaves its Link set, so the Timeline opens on no Passage.
    let bible = h
        .shell
        .st
        .session
        .workspace
        .tabs()
        .find(|t| t.kind == TabKind::Resource("kjv".into()))
        .unwrap()
        .id;
    h.shell
        .run("tab.link", &json!({ "tab": bible.0, "set": "" }));
    let tab = TabId(
        h.shell
            .run("tool.timeline", &json!({ "split": true }))
            .unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    h.frames(2);
    until(&mut h, "the whole Timeline", |h| {
        h.texts()
            .iter()
            .any(|t| t.starts_with("Enter a Bible reference"))
            && !all_in(h, tab, "3000 BC").is_empty()
    });

    let root = std::env::temp_dir().join(format!("obs-ui-timeline-none-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _files = Files(root.clone());
    bibles::write_library(&root.join("library"), &[tipnr_timeline()]);
    let s = &mut h.shell.st.session;
    s.library = Library::new();
    assert!(s.library.scan(&root.join("library")).is_empty());
    until(&mut h, "the empty state", |h| {
        h.texts()
            .iter()
            .any(|t| t.starts_with("None in your library."))
    });
}

/// Fit shows the whole Timeline across the whole axis, as the Timeline opens without a
/// Passage: its first and last years 24 points in from the axis's ends, not squeezed into
/// what the controls left of their row.
#[test]
fn fit_shows_the_whole_timeline_across_the_whole_axis() {
    let (mut h, _files) = harness("fit");
    h.ctx.enable_accesskit();
    let bible = h
        .shell
        .st
        .session
        .workspace
        .tabs()
        .find(|t| t.kind == TabKind::Resource("kjv".into()))
        .unwrap()
        .id;
    h.shell
        .run("tab.link", &json!({ "tab": bible.0, "set": "" }));
    let tab = TabId(
        h.shell
            .run("tool.timeline", &json!({ "split": true }))
            .unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    h.frames(2);
    until(&mut h, "the whole Timeline", |h| {
        !all_in(h, tab, "3000 BC").is_empty() && !all_in(h, tab, "Fit").is_empty()
    });
    let view = |h: &Harness| {
        (h.shell.st.tabs.get(&tab))
            .and_then(|t| t.timeline.as_ref())
            .and_then(|t| t.view())
            .expect("the Timeline's view")
    };
    let axis_width = |h: &Harness| {
        let tree = h.tree.as_ref().expect("the AccessKit tree");
        let node = (tree.nodes.iter().map(|(_, n)| n))
            .find(|n| {
                n.role() == egui::accesskit::Role::Image
                    && n.label().is_some_and(|l| l.starts_with("Timeline from"))
            })
            .expect("the Timeline's axis");
        let b = node.bounds().unwrap();
        b.x1 - b.x0
    };
    let st = &mut h.shell.st;
    let timeline = st
        .references
        .timeline(&st.session.library)
        .unwrap()
        .unwrap();
    let bounds = timeline.bounds().unwrap();
    let opened = view(&h);
    let width = axis_width(&h);
    let ends = |v: obs_engine::timeline::TimelineView| (v.x(bounds.0, width), v.x(bounds.1, width));
    let (first, last) = ends(opened);
    assert!(
        (first - 24.0).abs() < 1.0 && (width - 24.0 - last).abs() < 1.0,
        "the Timeline opened on {first}..{last} of an axis {width} wide"
    );

    // Zoom in, then Fit: the same view as on opening.
    let plus = all_in(&h, tab, "[zoom-in]")[0];
    h.click(plus.center());
    h.frames(3);
    assert!(view(&h).scale > opened.scale * 1.5, "+ did not zoom in");
    let fit = all_in(&h, tab, "Fit")[0];
    h.click(fit.center());
    h.frames(3);
    let fitted = view(&h);
    let (first, last) = ends(fitted);
    assert!(
        (fitted.scale - opened.scale).abs() < opened.scale * 1e-6,
        "Fit showed {fitted:?}, not the whole Timeline as it opened: {opened:?}"
    );
    assert!(
        (first - 24.0).abs() < 1.0 && (width - 24.0 - last).abs() < 1.0,
        "Fit put the Timeline on {first}..{last} of an axis {width} wide"
    );
}

/// #293: the wheel zooms the Timeline as much at every Scroll speed.
#[test]
fn the_wheel_zooms_the_timeline_the_same_at_every_scroll_speed() {
    let (mut h, _files) = harness("scroll-speed");
    let tab = open_timeline(&mut h, "1 Kings 6");
    let temple = "Construction of Solomon's Temple";
    until(&mut h, "the temple on the axis", |h| {
        !all_in(h, tab, temple).is_empty()
    });
    let scale = |h: &Harness| {
        (h.shell.st.tabs.get(&tab))
            .and_then(|t| t.timeline.as_ref())
            .and_then(|t| t.view())
            .expect("the Timeline's view")
            .scale
    };
    let over = all_in(&h, tab, temple)[0].center();
    let mut zooms = Vec::new();
    for speed in [1.0, 0.5, 2.0] {
        h.shell.run("view.scroll_speed", &json!({ "value": speed }));
        h.frames(2);
        for unit in [egui::MouseWheelUnit::Line, egui::MouseWheelUnit::Point] {
            let before = scale(&h);
            h.events.push(Event::PointerMoved(over));
            h.events.push(Event::MouseWheel {
                unit,
                delta: vec2(
                    0.0,
                    if unit == egui::MouseWheelUnit::Line {
                        1.0
                    } else {
                        40.0
                    },
                ),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Wheel,
                modifiers: egui::Modifiers::NONE,
            });
            h.frames(60);
            zooms.push((speed, unit, scale(&h) / before));
        }
    }
    for (speed, unit, zoom) in &zooms {
        assert!(
            *zoom > 1.05,
            "{speed}× {unit:?}: the wheel zooms in ({zoom})"
        );
        let at_one = zooms.iter().find(|z| z.0 == 1.0 && z.1 == *unit).unwrap().2;
        assert!(
            (zoom - at_one).abs() < 1e-3,
            "{speed}× {unit:?}: {zoom}, at 1×: {at_one}"
        );
    }
}
