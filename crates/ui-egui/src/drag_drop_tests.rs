//! Drag and drop across the Workspace (#216), through the headless harness with real pointer
//! events: a Bible reference or a selected word dragged out of a Commentary, a Resource out
//! of the Library Tab, and a Note card re-anchored onto a Bible's text.

use std::path::PathBuf;

use obs_engine::TabId;
use obs_model::{
    Key, KeyScheme, Manifest, ParaStyle, Resource, ResourceBuilder, ResourceType, SpanKind,
    TextAccumulator, UnitId, UnitKind,
};
use obs_scripture::{Book, Passage, Verse};

use super::annotation::double_click;
use super::*;

const COMMENTARY_TEXT: &str = "All things work together for good (Rom 8:28) to them that love God.";

fn rom(chapter: u16, verse: u16) -> Verse {
    Verse::new(Book::Romans, chapter, verse)
}

/// Romans 8, one paragraph per four verses.
fn romans_bible() -> Resource {
    let mut m = Manifest::new("rom-bible", "Romans Bible", "RB", ResourceType::Bible, "en");
    m.key_schemes = vec![KeyScheme::Passage];
    m.license = "LicenseRef-PD".into();
    let mut b = ResourceBuilder::new(m);
    let unit = UnitId::book(Book::Romans);
    let mut t = TextAccumulator::new();
    let mut verses = Vec::new();
    let mut paras = Vec::new();
    let mut para_start = 0;
    for v in 1..=39 {
        if t.pos() > para_start {
            t.push(" ");
        }
        let r = t.push(&format!(
            "Verse {v} of the eighth chapter, written out so that it wraps."
        ));
        verses.push((r.clone(), rom(8, v)));
        if v % 4 == 0 || v == 39 {
            paras.push(para_start..r.end);
            t.push("\n");
            para_start = t.pos();
        }
    }
    b.add_unit(
        unit.clone(),
        UnitKind::BibleBook,
        Some("Romans".into()),
        t.into_string(),
    );
    for p in paras {
        b.add_span(
            &unit,
            p,
            SpanKind::Para {
                style: ParaStyle::Prose,
            },
        );
    }
    for (r, v) in verses {
        b.add_verse(&unit, r, v, v.verse);
    }
    b.build().unwrap()
}

/// One section on Romans 8 whose text names "Rom 8:28" as a Scripture reference.
fn commentary() -> Resource {
    let mut m = Manifest::new(
        "rom-commentary",
        "Romans Commentary",
        "RC",
        ResourceType::Commentary,
        "en",
    );
    m.key_schemes = vec![KeyScheme::Passage];
    m.license = "CC0-1.0".into();
    let mut b = ResourceBuilder::new(m);
    let unit = UnitId::new("ROM.8.1").unwrap();
    b.add_unit(
        unit.clone(),
        UnitKind::Section,
        None,
        COMMENTARY_TEXT.into(),
    );
    b.add_span(
        &unit,
        0..COMMENTARY_TEXT.len(),
        SpanKind::Para {
            style: ParaStyle::Prose,
        },
    );
    let at = COMMENTARY_TEXT.find("Rom 8:28").unwrap();
    b.add_span(
        &unit,
        at..at + "Rom 8:28".len(),
        SpanKind::ScriptureRef {
            targets: vec![Passage::verse(rom(8, 28))].into(),
        },
    );
    b.add_key(
        Key::Passage(Passage::new(rom(8, 1), rom(8, 39)).unwrap()),
        &unit,
        None,
        false,
    );
    b.build().unwrap()
}

/// The Romans Bible (★) at Rom 8:1 in the left Panel; the Library and Word Info on the
/// right, with the Commentary opened there.
fn romans_harness() -> (Harness, TabId, TabId) {
    let mut shell = Shell::new(Session::new(JobMode::Inline));
    add_doc(
        &mut shell,
        Arc::new(ReadingDoc::from_resource(romans_bible())),
    );
    add_doc(
        &mut shell,
        Arc::new(ReadingDoc::from_resource(commentary())),
    );
    shell.restore_or_default();
    let mut h = Harness::from_shell(shell);
    let bible = h.tab_id("RB");
    let right = h.shell.session().workspace.panel_of(h.tab_id("Library"));
    let out = h.shell.run(
        "resource.open",
        &json!({ "id": "rom-commentary", "panel": right.unwrap().0 }),
    );
    let commentary = TabId(out.unwrap()["tab"].as_u64().unwrap());
    settle_all(&mut h);
    (h, bible, commentary)
}

/// Where byte `byte` of unit `unit` of Tab `tab`'s text is on screen.
fn text_spot(h: &Harness, tab: TabId, unit: usize, byte: usize) -> Pos2 {
    glyph_rect(h, tab, unit, byte).center()
}

/// The screen box of the glyph at byte `byte` of unit `unit` of Tab `tab`'s text, its line
/// high.
fn glyph_rect(h: &Harness, tab: TabId, unit: usize, byte: usize) -> Rect {
    let view = h.shell.st.tabs[&tab].view.as_ref().unwrap();
    let [ox, oy] = view.text_origin;
    for (block, top, laid) in view.laid_in_view(900.0) {
        if view.doc.blocks[block].unit != unit {
            continue;
        }
        for line in &laid.lines {
            for item in &line.items {
                for (bytes, x0, x1) in item.clusters() {
                    if laid.to_unit(bytes.start) == Some(obs_layout::UnitPos::Text(byte)) {
                        return Rect::from_min_max(
                            pos2(ox + x0, oy + top + line.top),
                            pos2(ox + x1, oy + top + line.bottom),
                        );
                    }
                }
            }
        }
    }
    panic!("byte {byte} of unit {unit} is not laid out in view");
}

/// The centre of a glyph of Tab `tab`'s text right of `x` (in a Panel's right quarter), inside
/// its pane.
fn glyph_right_of(h: &Harness, tab: TabId, x: f32) -> Pos2 {
    let view = h.shell.st.tabs[&tab].view.as_ref().unwrap();
    let [ox, oy] = view.text_origin;
    let [_, pane_top, _, pane_bottom] = view.pane_rect;
    for (_, top, laid) in view.laid_in_view(900.0) {
        for line in &laid.lines {
            let y = oy + top + (line.top + line.text_bottom) / 2.0;
            if y <= pane_top + 4.0 || y >= pane_bottom - 4.0 {
                continue;
            }
            for item in &line.items {
                for (bytes, x0, x1) in item.clusters() {
                    let c = ox + (x0 + x1) / 2.0;
                    if c > x + 4.0 && x1 - x0 > 2.0 && laid.to_unit(bytes.start).is_some() {
                        return pos2(c, y);
                    }
                }
            }
        }
    }
    panic!("no text right of {x}");
}

/// Where `word` of the Commentary's text is on screen.
fn commentary_spot(h: &Harness, tab: TabId, word: &str) -> Pos2 {
    let at = COMMENTARY_TEXT.find(word).unwrap();
    text_spot(h, tab, 0, at + 1)
}

/// Frames until a dropped reference has gone where it was dropped.
fn settle_drop(h: &mut Harness) {
    for _ in 0..600 {
        if h.shell.st.drag.dragging.is_none() && !h.shell.st.drag.has_pending() {
            settle_all(h);
            return;
        }
        h.paced_frame();
    }
    panic!("the drop never landed");
}

/// Press at `from` and move to `to` without letting go.
fn drag_to(h: &mut Harness, from: Pos2, to: Pos2) {
    h.events.push(Event::PointerMoved(from));
    h.frame();
    h.button(from, true);
    h.frame();
    for i in 1..=20 {
        let t = i as f32 / 20.0;
        h.events.push(Event::PointerMoved(from + (to - from) * t));
        h.frame();
    }
}

fn release(h: &mut Harness, at: Pos2) {
    h.button(at, false);
    h.frames(3);
}

/// Whether a drop-target highlight (the lapis wash) covers `area`.
fn highlighted(h: &Harness, area: Rect) -> bool {
    let wash = crate::theme::c()
        .lapis
        .gamma_multiply(crate::tokens::alpha::TINT);
    h.rects.iter().any(|(r, c)| {
        *c == wash && (r.min - area.min).length() < 2.0 && (r.max - area.max).length() < 2.0
    })
}

#[test]
fn rom_8_28_dragged_from_a_commentary_navigates_a_bible_tab_and_on_its_edge_opens_a_new_panel() {
    let (mut h, bible, commentary) = romans_harness();
    let ws = &h.shell.session().workspace;
    assert_eq!(ws.tab(bible).unwrap().history.current(), Some(rom(8, 1)));
    let tabs = ws.tabs().count();
    let (left, _) = h.panels()[0].clone();
    let from = commentary_spot(&h, commentary, "Rom 8:28");

    // Over the Bible's text: its body is highlighted, and the ghost names the reference.
    let body = pos2(left.left() + 120.0, left.center().y);
    drag_to(&mut h, from, body);
    assert!(h.shell.st.drag.dragging.is_some());
    let edge = left.right() - left.width() * 0.25;
    assert!(
        highlighted(
            &h,
            Rect::from_min_max(
                pos2(left.left(), left.top() + 31.0),
                pos2(edge, left.bottom())
            )
        ),
        "{:?}",
        h.rects
    );
    assert!(h.find("Rom 8:28").is_some(), "{:?}", h.texts());
    release(&mut h, body);
    settle_drop(&mut h);
    let ws = &h.shell.session().workspace;
    assert_eq!(ws.tab(bible).unwrap().history.current(), Some(rom(8, 28)));
    assert_eq!(ws.tabs().count(), tabs, "no new Tab");
    assert!(
        h.shell.session().command_log().contains(&"workspace.drop"),
        "{:?}",
        h.shell.session().command_log()
    );

    // On the Bible Panel's right edge: a new Panel beside it, its Bible at Rom 8:28.
    let panels = h.panels().len();
    let from = commentary_spot(&h, commentary, "Rom 8:28");
    let at_edge = pos2(left.right() - 20.0, left.center().y);
    drag_to(&mut h, from, at_edge);
    assert!(highlighted(
        &h,
        Rect::from_min_max(pos2(edge, left.top()), left.max)
    ));
    release(&mut h, at_edge);
    settle_drop(&mut h);
    let after = h.panels();
    assert_eq!(after.len(), panels + 1, "{after:?}");
    assert_eq!(after[1].1, ["RB"], "beside the Bible's Panel: {after:?}");
    let ws = &h.shell.session().workspace;
    let new = ws
        .tabs()
        .find(|t| t.title == "RB" && t.id != bible)
        .unwrap();
    assert_eq!(new.history.current(), Some(rom(8, 28)));
    assert_eq!(ws.focused(), Some(new.id));
}

#[test]
fn rom_8_28_dragged_from_a_commentary_onto_a_search_tab_searches_for_it_as_written() {
    let (mut h, bible, commentary) = romans_harness();
    let left = h.shell.session().workspace.panel_of(bible).unwrap();
    let out = h
        .shell
        .run(
            "workspace.drop",
            &json!({ "text": "love", "panel": left.0 }),
        )
        .unwrap();
    let search = TabId(out["tab"].as_u64().unwrap());
    settle_all(&mut h);
    let tabs = h.shell.session().workspace.tabs().count();
    let (rect, _) = h.panels()[0].clone();
    let body = pos2(rect.left() + 120.0, rect.center().y);
    let from = commentary_spot(&h, commentary, "Rom 8:28");
    drag_to(&mut h, from, body);
    release(&mut h, body);
    settle_drop(&mut h);
    let ws = &h.shell.session().workspace;
    assert_eq!(ws.tabs().count(), tabs, "no new Tab");
    assert_eq!(ws.tab(bible).unwrap().history.current(), Some(rom(8, 1)));
    assert_eq!(
        h.shell.session().search_tab(search).unwrap().query,
        "Rom 8:28"
    );
    assert_eq!(ws.focused(), Some(search));
}

#[test]
fn a_selected_word_dragged_onto_a_tab_bar_searches_for_it_there_and_esc_cancels_a_drag() {
    let (mut h, bible, commentary) = romans_harness();
    let (left, _) = h.panels()[0].clone();
    let love = commentary_spot(&h, commentary, "love");
    double_click(&mut h, love);
    let sel = h.shell.st.tabs[&commentary]
        .view
        .as_ref()
        .unwrap()
        .selection
        .unwrap();
    let doc = &h.shell.st.tabs[&commentary].view.as_ref().unwrap().doc;
    assert_eq!(crate::selection::selected_text(doc, &sel), "love");

    // Esc drops nothing, and the selection stays.
    let bar = pos2(left.center().x, left.top() + 15.0);
    drag_to(&mut h, love, bar);
    assert!(highlighted(
        &h,
        Rect::from_min_max(
            left.min,
            pos2(left.right() - left.width() * 0.25, left.top() + 31.0)
        )
    ));
    h.events.push(Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.frame();
    assert!(h.shell.st.drag.dragging.is_none());
    release(&mut h, bar);
    assert!(!h.shell.session().command_log().contains(&"workspace.drop"));
    assert!(
        h.shell.st.tabs[&commentary]
            .view
            .as_ref()
            .unwrap()
            .selection
            .is_some()
    );

    // Dropped on the Bible Panel's tab bar: a Search for it, in that Panel.
    drag_to(&mut h, love, bar);
    release(&mut h, bar);
    settle_drop(&mut h);
    let ws = &h.shell.session().workspace;
    let search = ws
        .tabs()
        .find(|t| t.kind == TabKind::Tool(Tool::Search))
        .expect("a Search Tab");
    assert_eq!(ws.panel_of(search.id), ws.panel_of(bible));
    assert_eq!(
        h.shell.session().search_tab(search.id).unwrap().query,
        "love"
    );
}

#[test]
fn a_drag_started_inside_the_selection_moves_it_and_one_outside_selects_anew() {
    let (mut h, _, commentary) = romans_harness();
    let love = commentary_spot(&h, commentary, "love");
    double_click(&mut h, love);
    // Dropped back on its own Tab: nothing happens.
    let elsewhere = commentary_spot(&h, commentary, "things");
    h.drag(love, elsewhere);
    let view = h.shell.st.tabs[&commentary].view.as_ref().unwrap();
    let sel = view.selection.unwrap();
    assert_eq!(crate::selection::selected_text(&view.doc, &sel), "love");
    assert!(!h.shell.session().command_log().contains(&"workspace.drop"));
    // Started outside the selection: a new selection, as before.
    let good = commentary_spot(&h, commentary, "good");
    h.drag(elsewhere, good);
    let view = h.shell.st.tabs[&commentary].view.as_ref().unwrap();
    let text = crate::selection::selected_text(&view.doc, &view.selection.unwrap());
    assert!(text.contains("work together for"), "{text:?}");
}

#[test]
fn a_drag_carries_the_selection_only_from_text_inside_it() {
    let (mut h, _, commentary) = romans_harness();
    let love = commentary_spot(&h, commentary, "love");
    double_click(&mut h, love);
    // The right half of the selection's last glyph is in it, though the caret nearest there
    // is the selection's end.
    let e = glyph_rect(&h, commentary, 0, COMMENTARY_TEXT.find("love").unwrap() + 3);
    let right_half = pos2(e.right() - e.width() / 4.0, e.center().y);
    let elsewhere = commentary_spot(&h, commentary, "things");
    drag_to(&mut h, right_half, elsewhere);
    assert!(
        h.shell.st.drag.dragging.is_some(),
        "the selection is carried"
    );
    release(&mut h, elsewhere);
    settle_all(&mut h);
    let view = h.shell.st.tabs[&commentary].view.as_ref().unwrap();
    let sel = view.selection.unwrap();
    assert_eq!(crate::selection::selected_text(&view.doc, &sel), "love");

    // The margin beside a selection that starts its line is not in it: a press there selects
    // anew, though the caret nearest there is the selection's start.
    let all = commentary_spot(&h, commentary, "All");
    double_click(&mut h, all);
    let view = h.shell.st.tabs[&commentary].view.as_ref().unwrap();
    let sel = view.selection.unwrap();
    assert_eq!(crate::selection::selected_text(&view.doc, &sel), "All");
    let first = glyph_rect(&h, commentary, 0, 0);
    let margin = pos2(first.left() - crate::view::MARGIN / 2.0, first.center().y);
    let good = commentary_spot(&h, commentary, "good");
    drag_to(&mut h, margin, good);
    assert!(h.shell.st.drag.dragging.is_none(), "nothing is carried");
    release(&mut h, good);
    let view = h.shell.st.tabs[&commentary].view.as_ref().unwrap();
    let text = crate::selection::selected_text(&view.doc, &view.selection.unwrap());
    assert!(text.starts_with("All things work together"), "{text:?}");
    assert!(!h.shell.session().command_log().contains(&"workspace.drop"));
}

#[test]
fn a_reference_selected_in_a_hebrew_numbered_bible_moves_an_english_one_to_the_same_verse() {
    let mut h = Harness::new();
    let tb = h.tab_id("TB");
    let out = h
        .shell
        .run("resource.open", &json!({ "id": "heb-num", "split": true }))
        .unwrap();
    let hn = TabId(out["tab"].as_u64().unwrap());
    h.shell.run("tab.link", &json!({ "tab": hn.0, "set": "" }));
    settle_all(&mut h);
    assert!(
        h.shell
            .session()
            .workspace
            .tab(hn)
            .unwrap()
            .verse_map
            .is_some()
    );
    // "Genesis 32:1" selected in HNUM's text and dragged onto TB's text.
    h.shell.st.drag.dragging = Some((
        super::super::drag_drop::Dragged::Text {
            text: "Genesis 32:1".into(),
            from: hn,
            bible: Some("heb-num".into()),
        },
        egui::ViewportId::ROOT,
    ));
    let (left, _) = h.panels()[0].clone();
    let body = pos2(left.left() + 120.0, left.center().y);
    h.events.push(Event::PointerMoved(body));
    h.button(body, true);
    h.frame();
    release(&mut h, body);
    settle_drop(&mut h);
    // HNUM's 32:1 is the Standard (and TB's) 31:55.
    let ws = &h.shell.session().workspace;
    assert_eq!(ws.tab(tb).unwrap().history.current(), Some(ge(31, 55)));
}

#[test]
fn a_resource_dragged_out_of_the_library_tab_opens_where_it_is_dropped() {
    let mut h = Harness::new();
    let (left, _) = h.panels()[0].clone();
    let row = h.at("Hebrew-numbered Bible");
    let at_edge = pos2(left.right() - 20.0, left.center().y);
    h.drag(row, at_edge);
    let panels = h.panels();
    assert_eq!(panels.len(), 3, "{panels:?}");
    assert_eq!(panels[1].1, ["HNUM"], "beside the left Panel: {panels:?}");
    assert!(h.shell.st.drag.dragging.is_none());
}

fn note_folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("obs-drag-drop-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The grip (its Note icon) of the Note card showing `reference`: on the reference's line,
/// before it.
fn note_grip(h: &Harness, reference: &str) -> Pos2 {
    let r = h.find(reference).unwrap();
    h.shapes
        .iter()
        .find(|(icon, t)| {
            t == "[pencil]"
                && (icon.center().y - r.center().y).abs() < 8.0
                && icon.right() <= r.left()
        })
        .map(|(icon, _)| icon.center())
        .unwrap_or_else(|| panic!("no grip by {reference:?}: {:?}", h.texts()))
}

fn ge(chapter: u16, verse: u16) -> Verse {
    Verse::new(Book::Genesis, chapter, verse)
}

#[test]
fn a_note_card_dragged_onto_a_verse_moves_there_after_confirming() {
    let dir = note_folder("note");
    let mut h = Harness::launch_full(Some(&dir.join("user.db")), JobMode::Inline, Some(&dir));
    settle_all(&mut h);
    let tb = h.tab_id("TB");
    let (unit, start) = {
        let doc = &h.shell.st.tabs[&tb].view.as_ref().unwrap().doc;
        doc.verse_position(ge(30, 1)).unwrap()
    };
    let unit_id = h.shell.st.tabs[&tb].view.as_ref().unwrap().doc.units[unit]
        .id
        .clone();
    let out = h
        .shell
        .run(
            "annotation.note",
            &json!({
                "resource": "test-bible",
                "start_unit": unit_id.as_str(),
                "start": start,
                "end": start + 3,
                "body": "Moved by dragging",
            }),
        )
        .unwrap();
    assert!(out["id"].is_string());
    let notes = TabId(
        h.shell.run("tool.notes", &json!({})).unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    for _ in 0..300 {
        h.paced_frame();
        if h.find("Genesis 30:1 (TB)").is_some() && !h.shell.st.notes_ui.has_pending() {
            break;
        }
    }
    // The card's reference, as the Notes Tool shows it.
    let reference = |h: &Harness| {
        ["Genesis 30:1 (TB)", "Genesis 30:5 (TB)"]
            .into_iter()
            .find(|r| h.find(r).is_some())
            .unwrap_or_default()
    };
    assert_eq!(reference(&h), "Genesis 30:1 (TB)");
    let grip = note_grip(&h, "Genesis 30:1 (TB)");
    let (unit, byte) = {
        let doc = &h.shell.st.tabs[&tb].view.as_ref().unwrap().doc;
        doc.verse_position(ge(30, 5)).unwrap()
    };
    let target = text_spot(&h, tb, unit, byte + 2);

    // Cancel: it stays where it was.
    h.drag(grip, target);
    assert!(
        h.find("Move this Note from Genesis 30:1 (TB) to Genesis 30:5 (TB)?")
            .is_some(),
        "{:?}",
        h.texts()
    );
    h.click_text("Cancel");
    assert!(h.find("Move Note").is_none());
    assert_eq!(reference(&h), "Genesis 30:1 (TB)");

    // Move: anchored on Genesis 30:5, as one undo step.
    let grip = note_grip(&h, "Genesis 30:1 (TB)");
    h.drag(grip, target);
    h.click_text("Move");
    for _ in 0..300 {
        h.paced_frame();
        if reference(&h) != "Genesis 30:1 (TB)" && !h.shell.st.notes_ui.has_pending() {
            break;
        }
    }
    assert_eq!(reference(&h), "Genesis 30:5 (TB)");
    assert!(
        h.shell
            .session()
            .command_log()
            .contains(&"annotation.reattach"),
        "{:?}",
        h.shell.session().command_log()
    );
    h.shell.run("edit.undo", &json!({}));
    for _ in 0..300 {
        h.paced_frame();
        if reference(&h) == "Genesis 30:1 (TB)" {
            break;
        }
    }
    assert_eq!(reference(&h), "Genesis 30:1 (TB)");
    assert!(h.shell.session().workspace.tab(notes).is_some());

    // The click way (no drag): select the text, then the card's menu, Move to selected text:
    // the same Move Note box.
    double_click(&mut h, target);
    let menu = h
        .shapes
        .iter()
        .find(|(r, t)| {
            t == "…" && (r.center().y - h.find("Genesis 30:1 (TB)").unwrap().center().y).abs() < 8.0
        })
        .map(|(r, _)| r.center())
        .unwrap();
    h.click(menu);
    h.frame();
    h.click_text("Move to selected text…");
    h.frame();
    assert!(
        h.find("Move this Note from Genesis 30:1 (TB) to Genesis 30:5 (TB)?")
            .is_some(),
        "{:?}",
        h.texts()
    );
    h.click_text("Cancel");
    assert_eq!(reference(&h), "Genesis 30:1 (TB)");
    drop(h);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_note_card_drops_only_on_a_resources_text() {
    let dir = note_folder("note-targets");
    let mut h = Harness::launch_full(Some(&dir.join("user.db")), JobMode::Inline, Some(&dir));
    settle_all(&mut h);
    let tb = h.tab_id("TB");
    h.shell.st.drag.dragging = Some((
        super::super::drag_drop::Dragged::Note {
            id: "n".into(),
            reference: "Genesis 30:1 (TB)".into(),
        },
        egui::ViewportId::ROOT,
    ));
    let (left, _) = h.panels()[0].clone();
    let (right, _) = h.panels()[1].clone();
    let view = h.shell.st.tabs[&tb].view.as_ref().unwrap();
    let [pane_left, pane_top, _, pane_bottom] = view.pane_rect;
    let [text_left, _] = view.text_origin;
    // The Tab's header (its reference box) lies between the tab bar and the text.
    assert!(pane_top > left.top() + 31.0 + 8.0, "{pane_top} {left:?}");
    let wash = crate::theme::c()
        .lapis
        .gamma_multiply(crate::tokens::alpha::TINT);
    let over = |h: &mut Harness, p: Pos2| {
        h.events.push(Event::PointerMoved(p));
        h.button(p, true);
        h.frame();
        h.rects.iter().any(|(_, c)| *c == wash)
    };
    // The Library's Panel, any tab bar or edge, the Tab's header and the pane's margin beside
    // the text: no target, so no highlight.
    for p in [
        right.center(),
        pos2(left.center().x, left.top() + 15.0),
        pos2(left.right() - 10.0, left.center().y),
        pos2(text_left + 20.0, pane_top - 6.0),
        pos2(
            (pane_left + text_left) / 2.0,
            (pane_top + pane_bottom) / 2.0,
        ),
    ] {
        assert!(!over(&mut h, p), "{p:?}");
    }
    // Released there, nothing is offered.
    h.button(pos2(text_left + 20.0, pane_top - 6.0), false);
    h.frames(2);
    assert!(h.shell.st.drag.confirm.is_none());
    // With its Contents open, the Contents pane is no target either.
    h.shell
        .run("tab.contents", &json!({ "tab": tb.0, "on": true }))
        .unwrap();
    settle_all(&mut h);
    h.shell.st.drag.dragging = Some((
        super::super::drag_drop::Dragged::Note {
            id: "n".into(),
            reference: "Genesis 30:1 (TB)".into(),
        },
        egui::ViewportId::ROOT,
    ));
    let contents = h.shell.st.tabs[&tb].contents.rect.unwrap();
    assert!(!over(&mut h, contents.center()), "{contents:?}");
    // On the words themselves it is a target.
    let (unit, byte) = {
        let doc = &h.shell.st.tabs[&tb].view.as_ref().unwrap().doc;
        doc.verse_position(ge(30, 5)).unwrap()
    };
    let word = text_spot(&h, tb, unit, byte + 2);
    assert!(over(&mut h, word), "{word:?}");
    h.events.push(Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.frame();
    h.button(word, false);
    h.frames(2);
    assert!(h.shell.st.drag.confirm.is_none());
    // Words in the Panel's right quarter take it too (no new Panel for a Note): the body is
    // highlighted, and dropped there the Move Note box asks.
    h.shell
        .run("tab.contents", &json!({ "tab": tb.0, "on": false }));
    settle_all(&mut h);
    let (left, _) = h.panels()[0].clone();
    let edge = left.right() - left.width() * 0.25;
    let far = glyph_right_of(&h, tb, edge);
    h.shell.st.drag.dragging = Some((
        super::super::drag_drop::Dragged::Note {
            id: "n".into(),
            reference: "Genesis 30:1 (TB)".into(),
        },
        egui::ViewportId::ROOT,
    ));
    assert!(over(&mut h, far), "{far:?}");
    assert!(highlighted(
        &h,
        Rect::from_min_max(pos2(left.left(), left.top() + 31.0), left.max)
    ));
    h.button(far, false);
    h.frames(2);
    assert!(h.find("Move Note").is_some(), "{:?}", h.texts());
    h.click_text("Cancel");
    assert!(h.shell.st.drag.confirm.is_none());
    drop(h);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Preview images for the PR (`UPDATE_PREVIEWS=1`): Rom 8:28 dragged from a Commentary onto a
/// Bible Tab and onto its Panel's edge, before and after; and the Move Note box.
#[test]
fn previews() {
    if std::env::var_os("UPDATE_PREVIEWS").is_none() {
        return;
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
    let (mut h, _, commentary) = romans_harness();
    let (left, _) = h.panels()[0].clone();
    h.frames(5);
    h.screenshot(&dir.join("drag-drop-before.ppm"));
    let from = commentary_spot(&h, commentary, "Rom 8:28");
    let body = pos2(left.left() + 160.0, left.center().y);
    drag_to(&mut h, from, body);
    h.screenshot(&dir.join("drag-drop-over-tab.ppm"));
    release(&mut h, body);
    settle_drop(&mut h);
    h.frames(5);
    h.screenshot(&dir.join("drag-drop-tab-navigated.ppm"));
    let from = commentary_spot(&h, commentary, "Rom 8:28");
    let at_edge = pos2(left.right() - 30.0, left.center().y);
    drag_to(&mut h, from, at_edge);
    h.screenshot(&dir.join("drag-drop-over-edge.ppm"));
    release(&mut h, at_edge);
    settle_drop(&mut h);
    h.frames(80);
    h.screenshot(&dir.join("drag-drop-new-panel.ppm"));

    let notes = note_folder("previews");
    let mut h = Harness::launch_full(Some(&notes.join("user.db")), JobMode::Inline, Some(&notes));
    settle_all(&mut h);
    let tb = h.tab_id("TB");
    let (unit, start) = {
        let doc = &h.shell.st.tabs[&tb].view.as_ref().unwrap().doc;
        doc.verse_position(ge(30, 1)).unwrap()
    };
    let unit_id = h.shell.st.tabs[&tb].view.as_ref().unwrap().doc.units[unit]
        .id
        .clone();
    h.shell.run(
        "annotation.note",
        &json!({
            "resource": "test-bible",
            "start_unit": unit_id.as_str(),
            "start": start,
            "end": start + 3,
            "body": "Laban's heap of witness",
        }),
    );
    h.shell.run("tool.notes", &json!({}));
    for _ in 0..300 {
        h.paced_frame();
        if h.find("Genesis 30:1 (TB)").is_some() && !h.shell.st.notes_ui.has_pending() {
            break;
        }
    }
    let grip = note_grip(&h, "Genesis 30:1 (TB)");
    let (unit, byte) = {
        let doc = &h.shell.st.tabs[&tb].view.as_ref().unwrap().doc;
        doc.verse_position(ge(30, 5)).unwrap()
    };
    let target = text_spot(&h, tb, unit, byte + 2);
    drag_to(&mut h, grip, target);
    h.screenshot(&dir.join("drag-drop-note-over-verse.ppm"));
    release(&mut h, target);
    h.frames(30);
    h.screenshot(&dir.join("drag-drop-move-note.ppm"));
    drop(h);
    let _ = std::fs::remove_dir_all(&notes);
}
