//! Parallel resources (#151) in the real shell: the ◀ ▶ buttons, the dropdown and
//! Ctrl+[ / Ctrl+] in a Resource Tab's header, driven with real pointer and key events.

use egui::{Event, Key as EguiKey, Modifiers};
use obs_engine::TabId;
use obs_engine::testing::bibles::{bsb_psalm_3, tahot_psalms};
use obs_scripture::{Book, Verse};

use super::*;

fn ps(chapter: u16, verse: u16) -> Verse {
    Verse::new(Book::Psalms, chapter, verse)
}

/// The default Workspace plus the BSB and TAHOT (after the test Bibles in Preferred order),
/// with the BSB open in a Panel of its own at Ps 3:1.
fn bsb_at_ps_3_1() -> (Harness, TabId) {
    let mut h = Harness::new();
    for r in [bsb_psalm_3(), tahot_psalms()] {
        add_doc(&mut h.shell, Arc::new(ReadingDoc::from_resource(r)));
    }
    let tab = TabId(
        h.shell
            .run("resource.open", &json!({ "id": "bsb", "split": true }))
            .unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    h.shell
        .run("tab.go", &json!({ "tab": tab.0, "text": "Ps 3:1" }));
    settle_all(&mut h);
    (h, tab)
}

/// Where `text` is painted inside the Panel showing `tab`, below its tab bar.
fn in_panel(h: &Harness, tab: TabId, text: &str) -> egui::Pos2 {
    let title = h.shell.st.session.workspace.tab(tab).unwrap().title.clone();
    let (panel, _) = h
        .panels()
        .into_iter()
        .find(|(_, titles)| titles.contains(&title))
        .unwrap_or_else(|| panic!("no Panel with {title}"));
    h.shapes
        .iter()
        .rev()
        .find(|(r, t)| t == text && panel.contains(r.center()) && r.top() > panel.top() + 31.0)
        .map(|(r, _)| r.center())
        .unwrap_or_else(|| panic!("{text:?} not in the {title} Panel: {:?}", h.texts()))
}

fn shows(h: &Harness, tab: TabId) -> (String, Option<Verse>) {
    let t = h.shell.st.session.workspace.tab(tab).unwrap();
    let TabKind::Resource(id) = &t.kind else {
        panic!("not a Resource Tab");
    };
    (id.clone(), t.history.current())
}

/// The Verse at the top of the Tab's reading view, as laid out.
fn top_verse(h: &mut Harness, tab: TabId) -> Option<Verse> {
    let view = h.shell.st.tabs.get_mut(&tab)?.view.as_mut()?;
    assert_eq!(
        TabKind::Resource(view.doc.manifest.resource_id.clone()),
        h.shell.st.session.workspace.tab(tab)?.kind,
        "the view shows the Tab's Resource"
    );
    view.current_location()
}

/// Where the Tab's reading view is scrolled to.
fn top_anchor(h: &mut Harness, tab: TabId) -> crate::view::TextAnchor {
    let view = h
        .shell
        .st
        .tabs
        .get_mut(&tab)
        .unwrap()
        .view
        .as_mut()
        .unwrap();
    view.text_anchor().unwrap()
}

/// The texts painted in the Tab's reading pane, top to bottom.
fn texts_in(h: &Harness, tab: TabId) -> Vec<String> {
    let tiles = &h.shell.tree.tiles;
    let pane = tiles.rect(tiles.find_pane(&tab).unwrap()).unwrap();
    let body = egui::Rect::from_min_max(pane.min + egui::vec2(0.0, 40.0), pane.max);
    h.shapes
        .iter()
        .filter(|(r, _)| body.contains(r.center()))
        .map(|(_, t)| t.clone())
        .collect()
}

/// Turn the mouse wheel over the Tab.
fn wheel(h: &mut Harness, tab: TabId, dy: f32) {
    let tiles = &h.shell.tree.tiles;
    let over = tiles.rect(tiles.find_pane(&tab).unwrap()).unwrap().center();
    h.events.push(Event::PointerMoved(over));
    h.events.push(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, dy),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: Modifiers::NONE,
    });
    h.frame();
}

fn press(h: &mut Harness, key: EguiKey) {
    h.events.push(Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::COMMAND,
    });
    h.frame();
    settle_all(h);
}

#[test]
fn next_button_in_a_bsb_tab_at_ps_3_1_lands_on_tahot_hebrew_ps_3_2_and_back_returns() {
    let (mut h, tab) = bsb_at_ps_3_1();
    assert_eq!(h.shell.st.location_text(tab), "Psalms 3:1");
    let link = h.shell.st.session.workspace.tab(tab).unwrap().link;
    h.click(in_panel(&h, tab, "[chevron-right]"));
    settle_all(&mut h);
    assert_eq!(shows(&h, tab), ("tahot".into(), Some(ps(3, 2))));
    assert_eq!(h.shell.st.location_text(tab), "Psalms 3:2");
    assert_eq!(top_verse(&mut h, tab), Some(ps(3, 2)));
    assert!(
        matches!(h.shell.st.tabs[&tab].flash, Some((crate::pane::Flash::Verse(v), _)) if v == ps(3, 2)),
        "the place it landed on is marked"
    );
    // The Tab bar and the dropdown name the new Resource; the Link set is kept.
    let t = h.shell.st.session.workspace.tab(tab).unwrap();
    assert_eq!((t.title.as_str(), t.link), ("TAHOT", link));
    h.frame();
    in_panel(&h, tab, "TAHOT");

    // Back (the header's ‹) returns to the BSB at the same place.
    h.click(in_panel(&h, tab, "[arrow-left]"));
    settle_all(&mut h);
    assert_eq!(shows(&h, tab), ("bsb".into(), Some(ps(3, 1))));
    assert_eq!(h.shell.st.location_text(tab), "Psalms 3:1");
    assert_eq!(top_verse(&mut h, tab), Some(ps(3, 1)));
    // And Forward to TAHOT again.
    h.click(in_panel(&h, tab, "[arrow-right]"));
    settle_all(&mut h);
    assert_eq!(shows(&h, tab), ("tahot".into(), Some(ps(3, 2))));
}

#[test]
fn ctrl_brackets_cycle_through_the_bibles_in_preferred_order() {
    let (mut h, tab) = bsb_at_ps_3_1();
    let bibles: Vec<String> = obs_engine::parallel_resources(h.shell.session(), tab)
        .iter()
        .map(|e| e.manifest.resource_id.clone())
        .collect();
    assert_eq!(
        bibles,
        ["test-bible", "heb-test", "heb-num", "bsb", "tahot"]
    );
    h.shell.st.session.workspace.focus(tab);
    h.shell.apply_effects();
    press(&mut h, EguiKey::CloseBracket);
    assert_eq!(shows(&h, tab), ("tahot".into(), Some(ps(3, 2))));
    press(&mut h, EguiKey::OpenBracket);
    assert_eq!(shows(&h, tab), ("bsb".into(), Some(ps(3, 1))));
    // ◀ from the BSB is the Bible before it.
    h.click(in_panel(&h, tab, "[chevron-left]"));
    settle_all(&mut h);
    assert_eq!(shows(&h, tab).0, "heb-num");
}

#[test]
fn the_dropdown_lists_the_tabs_resource_type_and_switches_to_the_one_chosen() {
    let (mut h, tab) = bsb_at_ps_3_1();
    h.click(in_panel(&h, tab, "BSB"));
    let texts = h.texts();
    for row in [
        "TB — Test Bible",
        "BSB — Berean Standard Bible (test)",
        "TAHOT — TAHOT (test)",
    ] {
        assert!(texts.contains(&row), "{row:?} not listed: {texts:?}");
    }
    h.click_text("TAHOT — TAHOT (test)");
    settle_all(&mut h);
    assert_eq!(shows(&h, tab), ("tahot".into(), Some(ps(3, 2))));
}

#[test]
fn a_type_with_one_resource_shows_the_control_disabled() {
    let mut h = Harness::new();
    let commentary = obs_engine::testing::commentary(
        "tc",
        "Test Commentary",
        "TC",
        &[(
            obs_scripture::Passage::verse(ps(3, 1)),
            None,
            "On the psalm.",
        )],
    );
    add_doc(
        &mut h.shell,
        Arc::new(ReadingDoc::from_resource(commentary)),
    );
    let tab = TabId(
        h.shell
            .run("resource.open", &json!({ "id": "tc", "split": true }))
            .unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    h.frames(3);
    h.click(in_panel(&h, tab, "[chevron-right]"));
    h.click(in_panel(&h, tab, "TC"));
    assert_eq!(shows(&h, tab).0, "tc");
    assert!(
        !h.texts().contains(&"TC — Test Commentary"),
        "no dropdown opened"
    );
}

#[test]
fn back_across_a_switch_returns_to_the_scroll_position_inside_a_long_commentary_section() {
    let mut h = Harness::new();
    let long: String = (1..=150)
        .map(|n| format!("Remark {n} on the psalm of flight."))
        .collect::<Vec<_>>()
        .join(" ");
    for (id, title, abbreviation) in [
        ("tc", "Test Commentary", "TC"),
        ("oc", "Other Commentary", "OC"),
    ] {
        let commentary = obs_engine::testing::commentary(
            id,
            title,
            abbreviation,
            &[
                (
                    obs_scripture::Passage::verse(ps(3, 1)),
                    Some("Psalm 3"),
                    &long,
                ),
                (
                    obs_scripture::Passage::verse(ps(4, 1)),
                    Some("Psalm 4"),
                    "On the next psalm.",
                ),
            ],
        );
        add_doc(
            &mut h.shell,
            Arc::new(ReadingDoc::from_resource(commentary)),
        );
    }
    let tab = TabId(
        h.shell
            .run("resource.open", &json!({ "id": "tc", "split": true }))
            .unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    h.shell
        .run("tab.go", &json!({ "tab": tab.0, "text": "Ps 3:1" }));
    settle_all(&mut h);
    let start = top_anchor(&mut h, tab);
    // Halfway down the section, with the wheel (scrolling is animated: let it stop).
    for _ in 0..6 {
        wheel(&mut h, tab, -230.0);
    }
    h.frames(30);
    settle_all(&mut h);
    let scrolled = top_anchor(&mut h, tab);
    assert_eq!(scrolled.unit, start.unit, "still in the Psalm 3 section");
    assert!(
        scrolled.byte > start.byte + 500,
        "scrolled well into the section: {start:?} → {scrolled:?}"
    );
    assert!(
        scrolled.into > 1.0,
        "part of a line scrolled off: {scrolled:?}"
    );
    assert_eq!(shows(&h, tab), ("tc".into(), Some(ps(3, 1))));
    let seen = texts_in(&h, tab);

    h.click(in_panel(&h, tab, "[chevron-right]"));
    settle_all(&mut h);
    assert_eq!(shows(&h, tab), ("oc".into(), Some(ps(3, 1))));
    // A switch opens the other Commentary at the section's start.
    assert_eq!(top_anchor(&mut h, tab).byte, start.byte);

    h.click(in_panel(&h, tab, "[arrow-left]"));
    settle_all(&mut h);
    assert_eq!(shows(&h, tab), ("tc".into(), Some(ps(3, 1))));
    let back = top_anchor(&mut h, tab);
    assert_eq!((back.unit, back.byte), (scrolled.unit, scrolled.byte));
    assert!(
        (back.into - scrolled.into).abs() < 0.5,
        "{scrolled:?} → {back:?}"
    );
    assert_eq!(texts_in(&h, tab), seen, "the same text is in view");

    // Forward returns to the other Commentary where it was left.
    h.click(in_panel(&h, tab, "[arrow-right]"));
    settle_all(&mut h);
    assert_eq!(shows(&h, tab).0, "oc");
    assert_eq!(top_anchor(&mut h, tab).byte, start.byte);
}

#[test]
fn the_reference_box_leaves_room_for_the_parallel_controls_in_a_narrow_panel() {
    let (mut h, tab) = bsb_at_ps_3_1();
    h.frames(2);
    let panel = h
        .panels()
        .into_iter()
        .find(|(_, titles)| titles.contains(&"BSB".to_string()))
        .unwrap()
        .0;
    assert!(panel.width() < 430.0, "a narrow Panel: {panel:?}");
    let text = in_panel(&h, tab, "Psalms 3:1");
    // The reference box: the narrowest filled rect around its text.
    let field = h
        .rects
        .iter()
        .map(|(r, _)| *r)
        .filter(|r| r.contains(text) && r.width() > 40.0 && r.height() < 40.0)
        .min_by(|a, b| a.width().total_cmp(&b.width()))
        .expect("the reference box");
    let previous = in_panel(&h, tab, "[chevron-left]");
    assert!(
        field.right() < previous.x - 4.0,
        "the box {field:?} overlaps ◀ at {previous:?}"
    );
    assert!(
        panel.contains(in_panel(&h, tab, "[ellipsis]")),
        "the Tab menu is in the Panel"
    );
}

/// Screenshots for the PR and `docs/previews/`: the BSB at Ps 3:1, the dropdown, TAHOT at
/// Hebrew Ps 3:2, and Back.
#[test]
fn parallel_resources_previews() {
    if std::env::var_os("UPDATE_PREVIEWS").is_none() {
        return;
    }
    let (mut h, tab) = bsb_at_ps_3_1();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/previews/parallel-resources");
    std::fs::create_dir_all(&dir).unwrap();
    h.screenshot(&dir.join("bsb-ps-3-1.ppm"));
    h.click(in_panel(&h, tab, "BSB"));
    // Past the popup's fade-in.
    h.frames(30);
    h.screenshot(&dir.join("dropdown.ppm"));
    h.events.push(Event::Key {
        key: EguiKey::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.frames(2);
    h.click(in_panel(&h, tab, "[chevron-right]"));
    settle_all(&mut h);
    h.screenshot(&dir.join("tahot-ps-3-2.ppm"));
    h.click(in_panel(&h, tab, "[arrow-left]"));
    settle_all(&mut h);
    h.screenshot(&dir.join("back-bsb-ps-3-1.ppm"));
}
