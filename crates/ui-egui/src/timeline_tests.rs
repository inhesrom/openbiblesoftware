//! The Timeline (#185) through the shell's real-event harness: 1 Kings 6 brings out the
//! temple's construction beside David's and Solomon's lives in the United Kingdom, the
//! patriarchs and the Exile show where their Passages are, a click on a Lifespan opens its
//! person's Factbook and a click on an Event opens its Passage.

use std::path::{Path, PathBuf};

use obs_engine::testing::{bibles, theographic, tipnr_timeline};
use obs_engine::{Guide, Library, TabId};
use obs_model::Key;
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

/// TIPNR, Theographic's events matched to it and a little KJV with 1 Kings 5:1 and 6:1, as
/// `.obr` files in a fresh library.
fn harness(name: &str) -> (Harness, Files) {
    harness_with(name, Vec::new())
}

/// [`harness`]'s library with `more` Resources in it.
fn harness_with(name: &str, more: Vec<obs_model::Resource>) -> (Harness, Files) {
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
    let mut resources = vec![kjv, tipnr, events];
    resources.extend(more);
    bibles::write_library(&root.join("library"), &resources);
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

/// Events of [`crowded_events`]: one a year from 1050 to 971 BC.
const CROWDED_EVENTS: i32 = 80;
/// Lifespans of [`crowded_events`], each across the whole of its Events.
const CROWDED_LIFESPANS: i32 = 10;

/// A People & places index (`crowded`) whose Events crowd the years around Solomon's
/// temple: more Events and Lifespans than an axis has rows for (#304). Each Event tells a
/// Verse of 1 Kings 4, so none is emphasised by 1 Kings 6.
pub(super) fn crowded_events() -> obs_model::Resource {
    use obs_model::{Event, EventKind, HistoricalDate};
    let mut r = theographic(None);
    r.manifest.resource_id = "crowded".into();
    r.manifest.title = "Crowded Timeline (test)".into();
    r.manifest.abbreviation = "Crowded".into();
    r.entities.clear();
    r.mentions.clear();
    let event =
        |id: String, title: String, kind, start, end: Option<i32>, passage: Option<Passage>| {
            Event {
                id,
                title,
                kind,
                start: HistoricalDate::bc(start),
                end: end.map(HistoricalDate::bc),
                approximate: false,
                part_of: None,
                people: Vec::new(),
                places: Vec::new(),
                passages: passage.into_iter().collect(),
            }
        };
    r.events = (0..CROWDED_EVENTS)
        .map(|n| {
            let verse = u16::try_from(n % 34 + 1).unwrap();
            event(
                format!("event:crowd-{n:02}"),
                format!("Crowded event {}", n + 1),
                EventKind::Event,
                1050 - n,
                None,
                Some(Passage::verse(Verse::new(Book::FirstKings, 4, verse))),
            )
        })
        .chain((0..CROWDED_LIFESPANS).map(|n| {
            event(
                format!("lifespan:crowd-{n:02}"),
                format!("Crowded life {}", n + 1),
                EventKind::Lifespan,
                1060 - n,
                Some(960 - n),
                None,
            )
        }))
        .collect();
    r.events.sort_by(|a, b| a.id.cmp(&b.id));
    r
}

/// An entry's or a "+N" chip's button on the Timeline's axis: its accessible name and where
/// it is.
fn axis_buttons(h: &Harness, tab: TabId) -> Vec<(String, Rect)> {
    let pane = pane(h, tab);
    let tree = h.tree.as_ref().expect("the AccessKit tree");
    (tree.nodes.iter().map(|(_, n)| n))
        .filter(|n| n.role() == egui::accesskit::Role::Button)
        .filter_map(|n| {
            let label = n.label()?.to_string();
            let b = n.bounds()?;
            #[allow(clippy::cast_possible_truncation)]
            let r = Rect::from_min_max(
                pos2(b.x0 as f32, b.y0 as f32),
                pos2(b.x1 as f32, b.y1 as f32),
            );
            pane.contains(r.center()).then_some((label, r))
        })
        .filter(|(label, _)| {
            label.starts_with("Event: ")
                || label.starts_with("Lifespan: ")
                || chip_count(label).is_some()
        })
        .collect()
}

/// How many entries a "+N" chip named `label` counts: "12 more Events" is 12.
fn chip_count(label: &str) -> Option<usize> {
    let (n, kind) = label.split_once(" more ")?;
    ["Event", "Events", "Lifespan", "Lifespans"]
        .contains(&kind)
        .then(|| n.parse().ok())
        .flatten()
}

/// How many Lifespans and Events the Timeline in `tab` has in view: its entries that reach
/// into the axis (by 4 points, as the axis lays them out).
fn in_view(h: &mut Harness, tab: TabId) -> usize {
    let view = (h.shell.st.tabs.get(&tab))
        .and_then(|t| t.timeline.as_ref())
        .and_then(|t| t.view())
        .expect("the Timeline's view");
    let tree = h.tree.as_ref().expect("the AccessKit tree");
    let axis = (tree.nodes.iter().map(|(_, n)| n))
        .find(|n| {
            n.role() == egui::accesskit::Role::Image
                && n.label().is_some_and(|l| l.starts_with("Timeline from"))
        })
        .and_then(|n| n.bounds())
        .expect("the Timeline's axis");
    let width = axis.x1 - axis.x0;
    let st = &mut h.shell.st;
    let timeline = st
        .references
        .timeline(&st.session.library)
        .unwrap()
        .unwrap();
    (timeline.entries.iter())
        .filter(|e| e.kind != obs_model::EventKind::Era)
        .filter(|e| view.x(e.to, width) >= -4.0 && view.x(e.from, width) <= width + 4.0)
        .count()
}

/// GUIDES-10 (#304): where Lifespans and Events crowd the axis, every bar and marker drawn
/// has its label, none covers another's, and those with no room are counted in "+N" chips
/// rather than piled up unlabelled in the last row. Each case of the finding: the whole
/// Timeline, a Passage (1 Kings 6) in the middle of the window and in the narrow right Panel.
#[test]
fn crowded_entries_are_counted_in_chips_not_piled_up() {
    let cases: [(&str, Option<&str>, bool); 3] = [
        ("whole", None, false),
        ("1-kings-6", Some("1 Kings 6"), false),
        ("1-kings-6-narrow", Some("1 Kings 6"), true),
    ];
    for (name, passage, narrow) in cases {
        let (mut h, _files) = harness_with(&format!("crowded-{name}"), vec![crowded_events()]);
        h.ctx.enable_accesskit();
        let bible = h.tab_id("KJV");
        let mut params = json!({ "split": true });
        match passage {
            Some(p) => params["passage"] = json!(p),
            // The Bible leaves its Link set, so the Timeline opens on no Passage.
            None => {
                h.shell
                    .run("tab.link", &json!({ "tab": bible.0, "set": "" }));
            }
        }
        if narrow {
            let right = h.shell.session().workspace.panel_of(h.tab_id("Library"));
            params = json!({ "passage": passage, "panel": right.unwrap().0 });
        }
        let tab = TabId(
            h.shell.run("tool.timeline", &params).unwrap()["tab"]
                .as_u64()
                .unwrap(),
        );
        h.frames(2);
        until(&mut h, "the crowded axis", |h| {
            axis_buttons(h, tab)
                .iter()
                .any(|(l, _)| chip_count(l).is_some())
        });
        h.frames(2);
        preview(&h, &format!("timeline-crowded-{name}-kittest"));
        let buttons = axis_buttons(&h, tab);
        for (i, (a, ra)) in buttons.iter().enumerate() {
            for (b, rb) in &buttons[i + 1..] {
                assert!(
                    !ra.shrink(0.5).intersects(rb.shrink(0.5)),
                    "{name}: {a} {ra:?} and {b} {rb:?} overlap"
                );
            }
        }
        let drawn = buttons
            .iter()
            .filter(|(l, _)| chip_count(l).is_none())
            .count();
        let counted: usize = buttons.iter().filter_map(|(l, _)| chip_count(l)).sum();
        assert_eq!(
            drawn + counted,
            in_view(&mut h, tab),
            "{name}: {drawn} drawn and {counted} in chips: {buttons:?}"
        );
        assert!(
            buttons.iter().any(|(l, _)| l.ends_with(" more Lifespans")),
            "{name}: the Lifespans that fit in no row are counted: {buttons:?}"
        );
    }
}

/// A "+N" chip's menu lists the Events it counts; one chosen opens its Passage, as a click
/// on the Event would.
#[test]
fn a_chips_menu_lists_its_events_and_opens_one() {
    let (mut h, _files) = harness_with("crowded-menu", vec![crowded_events()]);
    h.ctx.enable_accesskit();
    let tab = open_timeline(&mut h, "1 Kings 6");
    until(&mut h, "a chip of Events", |h| {
        axis_buttons(h, tab)
            .iter()
            .any(|(l, _)| l.ends_with(" more Events"))
    });
    let (label, chip) = axis_buttons(&h, tab)
        .into_iter()
        .find(|(l, _)| l.ends_with(" more Events"))
        .unwrap();
    let n = chip_count(&label).unwrap();
    h.click(chip.center());
    h.frames(3);
    // Each item is "title · dates"; a chip may count a Theographic Event among the crowded
    // ones ("Southern Kingdom (Judah)", where the chips fall once the summary has its own
    // row, #301).
    let items: Vec<String> = h
        .texts()
        .into_iter()
        .filter(|t| t.contains(" · ") && t.ends_with(" BC"))
        .map(str::to_string)
        .collect();
    assert!(egui::Popup::is_any_open(&h.ctx), "the menu opened");
    assert_eq!(
        items.len(),
        n,
        "the menu lists the chip's {n} Events: {items:?}"
    );
    preview(&h, "timeline-crowded-menu-kittest");
    let first = (items.iter())
        .find(|t| t.starts_with("Crowded event "))
        .expect("a crowded Event in the menu")
        .clone();
    let (title, _) = first.split_once(" · ").expect("title · dates");
    let number: i32 = title.trim_start_matches("Crowded event ").parse().unwrap();
    let verse = u16::try_from((number - 1) % 34 + 1).unwrap();
    let at = h.at(&first);
    h.click(at);
    h.frames(3);
    // The Timeline's Link set moves to it (the little KJV has no 1 Kings 4 to show).
    let at = Passage::verse(Verse::new(Book::FirstKings, 4, verse));
    until(&mut h, "the Link set at the Event's Verse", |h| {
        let ws = &h.shell.st.session.workspace;
        ws.tab(tab).and_then(|t| t.shown_passage()) == Some(at)
    });
}

/// Put [`crowded_events`] in the library of `h`, in its library folder.
fn add_crowded_events(h: &mut Harness) {
    let library = &mut h.shell.st.session.library;
    let dir = library.directory().expect("a library folder").to_path_buf();
    bibles::write_library(&dir, &[crowded_events()]);
    library.add_file(&dir.join("crowded.obr")).unwrap();
    h.frames(2);
}

/// The crowded Timeline open in `params`, the Tab focused once its axis shows its chips.
fn open_crowded(h: &mut Harness, params: serde_json::Value) {
    add_crowded_events(h);
    h.shell.run("tool.timeline", &params);
    super::missing_resource::settle(h);
}

/// The Timeline in focus shows "+N" chips.
fn crowded_shown(h: &Harness) -> bool {
    let ws = &h.shell.session().workspace;
    let focused = ws.focused().and_then(|t| ws.tab(t)).map(|t| &t.kind);
    focused == Some(&TabKind::Tool(obs_engine::Tool::Timeline))
        && h.texts()
            .iter()
            .any(|t| t.starts_with('+') && t[1..].parse::<usize>().is_ok())
}

/// The Screenshot gallery's crowded Timelines (#304): 1 Kings 6 beside the Bible and in the
/// narrow right Panel, and the whole Timeline.
pub(super) fn crowded_timelines() -> Vec<super::a11y_audit::Overlay> {
    vec![
        (
            "Timeline: crowded, 1 Kings 6",
            |h| open_crowded(h, json!({ "passage": "1 Kings 6", "split": true })),
            crowded_shown,
        ),
        (
            "Timeline: crowded, in the right Panel",
            |h| {
                let right = h.shell.session().workspace.panel_of(h.tab_id("Library"));
                let params = json!({ "passage": "1 Kings 6", "panel": right.unwrap().0 });
                open_crowded(h, params);
            },
            crowded_shown,
        ),
        (
            "Timeline: crowded, whole",
            |h| {
                let bible = h.tab_id("KJV");
                h.shell
                    .run("tab.link", &json!({ "tab": bible.0, "set": "" }));
                open_crowded(h, json!({ "split": true }));
            },
            crowded_shown,
        ),
    ]
}

/// The Screenshot gallery's menu of a "+N" chip (#304).
pub(super) fn crowded_timeline_menus() -> Vec<super::a11y_audit::Overlay> {
    vec![(
        "Timeline: a chip's menu",
        |h| {
            open_crowded(h, json!({ "passage": "1 Kings 6", "split": true }));
            h.frames(2);
            let chip = h.at("+20");
            h.click(chip);
            h.frames(3);
        },
        |h| {
            egui::Popup::is_any_open(&h.ctx)
                && (h.texts().iter()).any(|t| t.starts_with("Crowded event ") && t.contains(" · "))
        },
    )]
}
