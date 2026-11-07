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
                        let y = top + (line.top + line.bottom) / 2.0;
                        return pos2(ox + (x0 + x1) / 2.0, oy + y);
                    }
                }
            }
        }
    }
    panic!("byte {byte} of unit {unit} is not laid out in view");
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
    let wash = crate::theme::c().lapis.gamma_multiply(0.18);
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
fn a_resource_dragged_out_of_the_library_tab_opens_where_it_is_dropped() {
    let mut h = Harness::new();
    let (left, _) = h.panels()[0].clone();
    let row = h.at("Hebrew-numbered Bible");
    let at_edge = pos2(left.right() - 20.0, left.center().y);
    h.drag(row, at_edge);
    let panels = h.panels();
    assert_eq!(panels.len(), 3, "{panels:?}");
    assert_eq!(panels[1].1, ["HNUM"], "beside the left Panel: {panels:?}");
    assert!(h.shell.st.toolbar.dragging.is_none());
}

fn note_folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("obs-drag-drop-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
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
    let grip = h.at("✏");
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
    let grip = h.at("✏");
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
    drop(h);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_note_card_drops_only_on_a_resources_text() {
    let dir = note_folder("note-targets");
    let mut h = Harness::launch_full(Some(&dir.join("user.db")), JobMode::Inline, Some(&dir));
    settle_all(&mut h);
    h.shell.st.drag.dragging = Some((
        super::super::drag_drop::Dragged::Note {
            id: "n".into(),
            label: "✏ Genesis 30:1 (TB)".into(),
        },
        egui::ViewportId::ROOT,
    ));
    let (left, _) = h.panels()[0].clone();
    let (right, _) = h.panels()[1].clone();
    // The Library's Panel and any tab bar or edge: no target, so no highlight.
    for p in [
        right.center(),
        pos2(left.center().x, left.top() + 15.0),
        pos2(left.right() - 10.0, left.center().y),
    ] {
        h.events.push(Event::PointerMoved(p));
        h.button(p, true);
        h.frame();
        let wash = crate::theme::c().lapis.gamma_multiply(0.18);
        assert!(!h.rects.iter().any(|(_, c)| *c == wash), "{p:?}");
    }
    h.button(right.center(), false);
    h.frames(2);
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
    let grip = h.at("✏");
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
