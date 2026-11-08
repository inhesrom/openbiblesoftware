//! Highlights and Notes in the origin Bible (#62), through the headless harness with real
//! pointer and key events, a real `.obr` and a real User database in a temp folder.

use std::path::{Path, PathBuf};

use egui::Color32;
use obs_engine::{Content, HighlightLook, TabId};
use unicode_segmentation::UnicodeSegmentation;

use super::*;
use crate::selection::{DocPos, range_rects};
use crate::view::ReadingView;

/// A temp folder for the `.obr` and the User database.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("obs-annotate-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Launch (or relaunch) the app on the folder's User database, with TB from its `.obr`.
fn launch(dir: &Path) -> Harness {
    let mut h = Harness::launch_full(Some(&dir.join("user.db")), JobMode::Inline, Some(dir));
    settle_all(&mut h);
    h
}

fn view(h: &Harness, tab: TabId) -> &ReadingView {
    h.shell.st.tabs[&tab].view.as_ref().unwrap()
}

/// TB's reading pane on screen: its Panel below the header strip.
fn pane(h: &Harness) -> Rect {
    let panel = h.panels()[0].0;
    Rect::from_min_max(pos2(panel.left(), panel.top() + 70.0), panel.max)
}

/// Screen rects of a document range in a view, as the painter places them.
fn screen_rects(h: &Harness, tab: TabId, a: DocPos, b: DocPos) -> Vec<Rect> {
    let v = view(h, tab);
    let [ox, oy] = v.text_origin;
    range_rects(v, &[(a, b)], 900.0)
        .remove(0)
        .into_iter()
        .map(|[x0, y0, x1, y1]| Rect::from_min_max(pos2(ox + x0, oy + y0), pos2(ox + x1, oy + y1)))
        .collect()
}

/// A word of at least 5 letters (no `f`, which could form a ligature) shown on one line with
/// its top below `min_y`: its range, its text and its rect on screen.
fn word_in_view(h: &Harness, tab: TabId, min_y: f32) -> (DocPos, DocPos, String, Rect) {
    let v = view(h, tab);
    let bottom = pane(h).bottom() - 60.0;
    for (index, _, _) in v.laid_in_view(900.0) {
        let block = &v.doc.blocks[index];
        let text = &v.doc.units[block.unit].text;
        let range = block.source.range.clone();
        for (i, w) in text[range.clone()].split_word_bound_indices() {
            if w.len() < 5 || !w.chars().all(char::is_alphabetic) || w.contains('f') {
                continue;
            }
            let a = DocPos {
                unit: block.unit,
                byte: range.start + i,
            };
            let b = DocPos {
                byte: a.byte + w.len(),
                ..a
            };
            if let [r] = screen_rects(h, tab, a, b)[..]
                && r.top() > min_y
                && r.bottom() < bottom
            {
                return (a, b, w.to_string(), r);
            }
        }
    }
    panic!("no word in view below {min_y}");
}

fn press_key(h: &mut Harness, key: egui::Key, modifiers: Modifiers) {
    h.events.push(Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    });
    h.frame();
}

/// A frame in which the user closes the window: whether the app cancels the close.
pub(super) fn close_window(h: &mut Harness) -> bool {
    let mut input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))),
        time: Some(h.time),
        ..Default::default()
    };
    (input.viewports.entry(egui::ViewportId::ROOT).or_default())
        .events
        .push(egui::ViewportEvent::Close);
    h.time += 1.0 / 60.0;
    let shell = &mut h.shell;
    let mut out = h.ctx.run_ui(input, |ui| shell.ui(ui));
    // Not drawn: this frame's texture uploads are not kept.
    out.textures_delta.clear();
    (out.viewport_output.get(&egui::ViewportId::ROOT))
        .is_some_and(|v| v.commands.contains(&egui::ViewportCommand::CancelClose))
}

pub(super) fn double_click(h: &mut Harness, p: Pos2) {
    // Long after any earlier click, so this is not counted as a triple click.
    h.time += 1.0;
    h.events.push(Event::PointerMoved(p));
    h.frame();
    for _ in 0..2 {
        h.button(p, true);
        h.frame();
        h.button(p, false);
        h.frame();
    }
    // A new Area is measured on its first frame and shown on the next.
    h.frame();
}

/// Painted shapes of one colour inside TB's pane, whatever their size.
fn painted_any(h: &Harness, color: Color32) -> Vec<Rect> {
    let pane = pane(h);
    h.rects
        .iter()
        .filter(|(r, c)| *c == color && pane.contains(r.center()))
        .map(|(r, _)| *r)
        .collect()
}

/// Painted rects of one colour inside TB's pane.
fn painted(h: &Harness, color: Color32) -> Vec<Rect> {
    let pane = pane(h);
    h.rects
        .iter()
        // Toolbar swatches (18 points square) are not Highlights.
        .filter(|(r, c)| {
            *c == color && pane.contains(r.center()) && !(r.width() < 19.0 && r.height() < 19.0)
        })
        .map(|(r, _)| *r)
        .collect()
}

fn union(rects: &[Rect]) -> Rect {
    rects.iter().copied().reduce(|a, b| a.union(b)).unwrap()
}

/// A Highlight `h` covers the word drawn in line rect `w`: the same columns, and the text's
/// band of the line.
fn covers(h: Rect, w: Rect) -> bool {
    (h.left() - w.left()).abs() < 0.6
        && (h.right() - w.right()).abs() < 0.6
        && h.top() >= w.top() - 0.1
        && h.bottom() <= w.bottom() + 0.1
        && h.height() > w.height() * 0.5
}

fn cmd_shift() -> Modifiers {
    Modifiers::COMMAND | Modifiers::SHIFT
}

#[test]
fn ctrl_2_draws_a_green_highlight_at_once_and_ctrl_z_removes_it() {
    let dir = folder("ctrl2");
    let mut h = launch(&dir);
    let tb = h.tab_id("TB");
    let (a, b, word, r) = word_in_view(&h, tb, 200.0);
    double_click(&mut h, r.center());
    assert!(h.shell.st.has_selection(tb));
    // Released: the floating toolbar is up, with its actions.
    for label in ["Note", "Copy", "Search this"] {
        assert!(h.find(label).is_some(), "{label}: {:?}", h.texts());
    }
    let green = h.shell.st.palette.built_in_color("green");
    assert!(painted(&h, green).is_empty());
    // Ctrl+Alt+2 (AltGr+2 on Windows) is not Alt+2: no style (nor Ctrl+2, Go to Panel 2).
    press_key(&mut h, egui::Key::Num2, Modifiers::COMMAND | Modifiers::ALT);
    assert!(h.shell.st.session.annotations(None).unwrap().is_empty());
    assert!(h.shell.st.has_selection(tb));

    // Alt+2: green on the very frame the key arrives, exactly over the word.
    press_key(&mut h, egui::Key::Num2, Modifiers::ALT);
    let rects = painted(&h, green);
    assert!(!rects.is_empty(), "no green Highlight painted");
    assert!(covers(union(&rects), r), "{rects:?} vs word {r:?}");
    // The selection and the toolbar make way for it.
    assert!(!h.shell.st.has_selection(tb));
    assert!(h.find("Note").is_none());
    let stored = h.shell.st.session.annotations(None).unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(
        stored[0].content,
        Content::Highlight {
            style: "green".into()
        }
    );
    let range = stored[0].anchor.as_ref().unwrap().text_range().unwrap();
    assert_eq!(range.quote, word);
    assert_eq!((range.start as usize, range.end as usize), (a.byte, b.byte));

    // Ctrl+Shift+Z with nothing to redo does nothing: it is never Ctrl+Z.
    press_key(&mut h, egui::Key::Z, cmd_shift());
    press_key(&mut h, egui::Key::Z, cmd_shift());
    assert_eq!(h.shell.st.session.annotations(None).unwrap().len(), 1);
    assert!(covers(union(&painted(&h, green)), r));

    // Ctrl+Z removes it, again on the same frame; Ctrl+Shift+Z brings it back.
    press_key(&mut h, egui::Key::Z, Modifiers::COMMAND);
    assert!(painted(&h, green).is_empty(), "undo left the Highlight");
    assert!(h.shell.st.session.annotations(None).unwrap().is_empty());
    press_key(&mut h, egui::Key::Z, cmd_shift());
    assert!(covers(union(&painted(&h, green)), r));

    // While a text field has focus, Ctrl+Z is the field's: the Highlight stays.
    h.click_text("Genesis 30:1");
    assert!(h.ctx.text_edit_focused());
    press_key(&mut h, egui::Key::Z, Modifiers::COMMAND);
    h.frame();
    assert_eq!(h.shell.st.session.annotations(None).unwrap().len(), 1);
    assert!(!painted(&h, green).is_empty());
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn keys_that_need_shift_on_some_layouts_still_work_and_ctrl_shift_z_never_undoes() {
    let dir = folder("layouts");
    let mut h = launch(&dir);
    let tb = h.tab_id("TB");
    let (_, _, _, r) = word_in_view(&h, tb, 200.0);
    double_click(&mut h, r.center());
    // Digits need Shift on AZERTY: Alt+1 arrives as Num1 with Shift held.
    press_key(&mut h, egui::Key::Num1, Modifiers::ALT | Modifiers::SHIFT);
    let stored = h.shell.st.session.annotations(None).unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(
        stored[0].content,
        Content::Highlight {
            style: "yellow".into()
        }
    );

    // `+` needs Shift on a US keyboard: winit reports Ctrl+Shift+= as Plus with Shift held.
    // It is Larger text, not egui's whole-UI zoom.
    let size = h.shell.st.session.text.size;
    press_key(&mut h, egui::Key::Plus, cmd_shift());
    h.frame();
    assert_eq!(h.shell.st.session.text.size, size + 1.0);
    assert_eq!(h.ctx.zoom_factor(), 1.0);
    press_key(&mut h, egui::Key::Minus, Modifiers::COMMAND);
    assert_eq!(h.shell.st.session.text.size, size);

    // Ctrl+Shift+Z with nothing to redo still never undoes, nor does Ctrl+Alt+Z.
    press_key(&mut h, egui::Key::Z, cmd_shift());
    press_key(&mut h, egui::Key::Z, Modifiers::COMMAND | Modifiers::ALT);
    assert_eq!(h.shell.st.session.annotations(None).unwrap().len(), 1);
    press_key(&mut h, egui::Key::Z, Modifiers::COMMAND);
    assert!(h.shell.st.session.annotations(None).unwrap().is_empty());
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A Selection key rebound in Help → Keyboard shortcuts (#177) works at once, its old key no
/// longer does, and the selection toolbar's tooltip names the new key.
#[test]
fn a_rebound_selection_key_highlights_and_its_old_key_does_not() {
    let dir = folder("rebound");
    let mut h = launch(&dir);
    h.shell.run(
        "keys.set",
        &json!({ "command": "selection.highlight_2", "key": "F7" }),
    );
    let tb = h.tab_id("TB");
    let (_, _, _, r) = word_in_view(&h, tb, 200.0);
    double_click(&mut h, r.center());
    // The swatch's tooltip names the new key.
    let keys = crate::shell::annotate::SelectionKeyText::of(&h.shell.st.session);
    assert_eq!(keys.slot(1), "F7");
    assert_eq!(keys.note(), "Ctrl+Shift+N");
    press_key(&mut h, egui::Key::Num2, Modifiers::ALT);
    h.frame();
    assert!(h.shell.st.session.annotations(None).unwrap().is_empty());
    assert!(h.shell.st.has_selection(tb));
    press_key(&mut h, egui::Key::F7, Modifiers::NONE);
    h.frame();
    let stored = h.shell.st.session.annotations(None).unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(
        stored[0].content,
        Content::Highlight {
            style: "green".into()
        }
    );
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A Selection key swapped onto Ctrl+Plus (#177) runs on Ctrl+= too, as a command's does
/// (#137): Larger text rebound to Alt+1 and swapped with yellow Highlight.
#[test]
fn a_selection_key_on_ctrl_plus_also_runs_on_ctrl_equals() {
    let dir = folder("plus-equals");
    let mut h = launch(&dir);
    h.shell.run(
        "keys.set",
        &json!({ "command": "view.text_larger", "key": "Alt+1", "swap": true }),
    );
    assert_eq!(
        h.shell.st.session.shortcut("selection.highlight_1"),
        Some("Ctrl+Plus")
    );
    let size = h.shell.st.session.text.size;
    let tb = h.tab_id("TB");
    let yellow = Content::Highlight {
        style: "yellow".into(),
    };
    for (key, mods) in [
        (egui::Key::Plus, cmd_shift()),
        (egui::Key::Equals, Modifiers::COMMAND),
    ] {
        let (_, _, _, r) = word_in_view(&h, tb, 200.0);
        double_click(&mut h, r.center());
        assert!(h.shell.st.has_selection(tb));
        press_key(&mut h, key, mods);
        h.frame();
        let stored = h.shell.st.session.annotations(None).unwrap();
        assert_eq!(stored.len(), 1, "{key:?}");
        assert_eq!(stored[0].content, yellow, "{key:?}");
        assert_eq!(h.shell.st.session.text.size, size, "{key:?}");
        // Undo it so the next key highlights afresh.
        press_key(&mut h, egui::Key::Z, Modifiers::COMMAND);
        assert!(h.shell.st.session.annotations(None).unwrap().is_empty());
    }
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn toolbar_swatches_and_right_click_apply_styles_and_the_newest_is_drawn_on_top() {
    let dir = folder("toolbar");
    let mut h = launch(&dir);
    let tb = h.tab_id("TB");
    let palette = h.shell.st.palette;
    let (_, _, _, r) = word_in_view(&h, tb, 200.0);
    double_click(&mut h, r.center());
    // The toolbar's third swatch is blue: find it by its painted colour, above the word.
    let swatch = h
        .rects
        .iter()
        .filter(|(s, c)| {
            *c == palette.built_in_color("blue") && s.width() < 30.0 && s.bottom() <= r.top()
        })
        .map(|(s, _)| *s)
        .next()
        .unwrap_or_else(|| {
            panic!(
                "a blue swatch above {r:?}: {:?}",
                h.rects
                    .iter()
                    .filter(|(s, _)| s.width() < 30.0
                        && s.bottom() < r.top()
                        && s.top() > r.top() - 60.0)
                    .collect::<Vec<_>>()
            )
        });
    h.click(swatch.center());
    assert!(covers(
        union(&painted(&h, palette.built_in_color("blue"))),
        r
    ));

    // Right-click on the same word: the menu acts on it. Pink overlaps blue and wins.
    h.events.push(Event::PointerMoved(r.center()));
    h.frame();
    for pressed in [true, false] {
        h.events.push(Event::PointerButton {
            pos: r.center(),
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.frame();
    }
    h.frame();
    assert!(h.shell.st.has_selection(tb), "right-click selects the word");
    assert!(h.find("Red underline").is_some(), "{:?}", h.texts());
    h.click_text("Pink");
    let order: Vec<Color32> = h
        .rects
        .iter()
        .filter(|(s, _)| covers(*s, r))
        .map(|(_, c)| *c)
        .collect();
    let blue = order
        .iter()
        .position(|c| *c == palette.built_in_color("blue"))
        .unwrap();
    let pink = order
        .iter()
        .position(|c| *c == palette.built_in_color("pink"))
        .unwrap();
    assert!(pink > blue, "the newest Highlight is drawn last: {order:?}");

    // The red underline is drawn over the glyphs, along the word's bottom.
    double_click(&mut h, r.center());
    press_key(&mut h, egui::Key::Num5, Modifiers::ALT);
    let lines = painted(&h, palette.underline);
    assert!(!lines.is_empty());
    let line = union(&lines);
    assert!(
        (line.left() - r.left()).abs() < 0.6 && (line.right() - r.right()).abs() < 0.6,
        "{lines:?} vs {r:?}"
    );
    assert!(line.top() > r.center().y && line.bottom() <= r.bottom());
    assert_eq!(h.shell.st.session.annotations(None).unwrap().len(), 3);
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_note_saves_on_blur_and_survives_restart() {
    let dir = folder("note");
    let mut h = launch(&dir);
    let study = h
        .shell
        .run("notebook.create", &json!({ "name": "Study" }))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let tb = h.tab_id("TB");
    let (a, _, _, first) = word_in_view(&h, tb, 120.0);
    let (_, b, _, last) = word_in_view(&h, tb, first.bottom() + 1.0);
    // Drag from the first word to the end of the second: the toolbar comes up on release.
    h.drag(first.left_center() + vec2(1.0, 0.0), last.right_center());
    assert!(h.find("Note").is_some(), "{:?}", h.texts());
    h.click_text("Note");
    assert!(h.ctx.text_edit_focused(), "the editor takes the keyboard");
    // Ctrl+Z inside it is the field's own undo, not the app's (which would undo the new
    // Notebook).
    press_key(&mut h, egui::Key::Z, Modifiers::COMMAND);
    assert_eq!(h.shell.st.session.notebooks().unwrap().len(), 2);
    h.events.push(Event::Text("Jacob and Laban".into()));
    h.frame();
    // Pick the Notebook: the editor starts on the last used one (the default).
    h.click_text("My Notebook");
    h.click_text("Study");
    assert!(h.shell.st.session.annotations(None).unwrap().is_empty());

    // Blur: a click in the text, away from the editor.
    let p = pane(&h);
    h.click(pos2(p.left() + 30.0, p.bottom() - 30.0));
    assert!(h.shell.st.note_editor.is_none(), "the editor closed");
    let notes = h.shell.st.session.annotations(None).unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].body(), "Jacob and Laban");
    assert_eq!(notes[0].notebook_id, study);
    let range = notes[0].anchor.as_ref().unwrap().text_range().unwrap();
    assert_eq!((range.start as usize, range.end as usize), (a.byte, b.byte));

    // A marker at the end of the range; hovering it previews the Note.
    let marker_color = h.shell.st.palette.note_marker;
    let marker = |h: &Harness| {
        let m = painted_any(h, marker_color);
        assert_eq!(m.len(), 1, "{m:?}");
        m[0]
    };
    let end = screen_rects(&h, tb, a, b).last().copied().unwrap();
    let m = marker(&h);
    assert!(
        m.left() >= end.right() && m.left() - end.right() < 3.0,
        "{m:?} vs {end:?}"
    );
    assert!(m.top() >= end.top() && m.bottom() <= end.bottom());
    h.events.push(Event::PointerMoved(m.center()));
    h.frames(3);
    assert!(h.find("Jacob and Laban").is_some(), "{:?}", h.texts());
    h.shell.save_session(true);
    drop(h);

    // Relaunch: the Note, its Notebook and its marker are back.
    let mut h = launch(&dir);
    let notes = h.shell.st.session.annotations(None).unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].body(), "Jacob and Laban");
    assert_eq!(notes[0].notebook_id, study);
    let tb = h.tab_id("TB");
    let end = screen_rects(&h, tb, a, b).last().copied().unwrap();
    let m = marker(&h);
    assert!(m.left() >= end.right() && m.left() - end.right() < 3.0);
    // The next Note starts in the Notebook last used, remembered by the User database.
    let (_, _, _, r) = word_in_view(&h, tb, 120.0);
    double_click(&mut h, r.center());
    press_key(&mut h, egui::Key::N, cmd_shift());
    h.frame();
    assert_eq!(h.shell.st.note_editor.as_ref().unwrap().notebook, study);
    assert!(h.find("Study").is_some());
    // Ctrl+Enter saves.
    h.events.push(Event::Text("Second".into()));
    h.frame();
    press_key(&mut h, egui::Key::Enter, Modifiers::COMMAND);
    assert!(h.shell.st.note_editor.is_none());
    assert_eq!(h.shell.st.session.annotations(None).unwrap().len(), 2);
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_empty_note_is_not_saved_and_esc_saves_a_written_one() {
    let dir = folder("note-esc");
    let mut h = launch(&dir);
    let tb = h.tab_id("TB");
    let (_, _, _, r) = word_in_view(&h, tb, 120.0);
    double_click(&mut h, r.center());
    press_key(&mut h, egui::Key::N, cmd_shift());
    h.frame();
    assert!(h.shell.st.note_editor.is_some());
    // Alt+1 while typing is not a Highlight.
    press_key(&mut h, egui::Key::Num1, Modifiers::ALT);
    press_key(&mut h, egui::Key::Escape, Modifiers::NONE);
    assert!(h.shell.st.note_editor.is_none());
    assert!(h.shell.st.session.annotations(None).unwrap().is_empty());
    double_click(&mut h, r.center());
    press_key(&mut h, egui::Key::N, cmd_shift());
    h.events.push(Event::Text("kept".into()));
    h.frame();
    press_key(&mut h, egui::Key::Escape, Modifiers::NONE);
    let notes = h.shell.st.session.annotations(None).unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].body(), "kept");
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The Note editor saves its text as typed (#186 review): white space is CommonMark, so four
/// leading spaces make a code block whose `**` shows as typed, and trimming them would turn
/// it bold.
#[test]
fn the_note_editor_saves_white_space_as_typed() {
    let dir = folder("note-indent");
    let mut h = launch(&dir);
    let tb = h.tab_id("TB");
    let (_, _, _, r) = word_in_view(&h, tb, 120.0);
    double_click(&mut h, r.center());
    press_key(&mut h, egui::Key::N, cmd_shift());
    let typed = "    **faith**  \n\n\t_hope_  \n";
    h.events.push(Event::Text(typed.into()));
    h.frame();
    press_key(&mut h, egui::Key::Escape, Modifiers::NONE);
    let notes = h.shell.st.session.annotations(None).unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].body(), typed);
    let shown = obs_model::parse_note_text(notes[0].body());
    assert_eq!(shown[0].text, "**faith**  ");
    assert!(shown[0].marks.is_empty(), "{:?}", shown[0].marks);
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A Note whose save fails stays in its editor, text and error shown, even when another
/// Note is started.
#[test]
fn a_note_that_fails_to_save_keeps_its_text() {
    let dir = folder("note-fail");
    let mut h = launch(&dir);
    let tb = h.tab_id("TB");
    let (_, _, _, r) = word_in_view(&h, tb, 120.0);
    double_click(&mut h, r.center());
    press_key(&mut h, egui::Key::N, cmd_shift());
    h.events.push(Event::Text("precious words".into()));
    h.frame();
    // Its Notebook goes away underneath it: saving it fails.
    h.shell.st.note_editor.as_mut().unwrap().notebook = "gone".into();
    press_key(&mut h, egui::Key::Enter, Modifiers::COMMAND);
    h.frame();
    let ed = h
        .shell
        .st
        .note_editor
        .as_ref()
        .expect("the editor stays open");
    assert_eq!(ed.body, "precious words");
    let shows_error = |h: &Harness| {
        h.texts()
            .iter()
            .any(|t| t.ends_with("There is no Notebook gone"))
    };
    assert!(shows_error(&h), "{:?}", h.texts());
    // Starting a Note again does not replace it.
    let ctx = h.ctx.clone();
    h.shell
        .st
        .annotate(&ctx, tb, crate::shell::annotate::Annotate::Note, None);
    h.frame();
    assert_eq!(
        h.shell.st.note_editor.as_ref().unwrap().body,
        "precious words"
    );
    assert!(shows_error(&h));
    assert!(h.shell.st.session.annotations(None).unwrap().is_empty());
    // Closing the window keeps it open, with the Note, until it is discarded.
    assert!(close_window(&mut h));
    assert_eq!(
        h.shell.st.note_editor.as_ref().unwrap().body,
        "precious words"
    );
    h.frame();
    h.click_text("Discard");
    assert!(h.shell.st.note_editor.is_none());
    assert!(!close_window(&mut h));
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A new revision of TB in `dir`, under a file name of its own, with `edit` applied to the
/// text of unit `unit` (same length, so its spans still fit), put in the library as an Import
/// replacing it would: the library entry, then [`obs_engine::Effect::Replaced`].
fn replace_tb(h: &mut Harness, dir: &Path, unit: &str, edit: impl FnOnce(&mut String)) {
    let mut r = crate::tests::numbered_bible_resource("test-bible", "Test Bible", "TB", false);
    let u = r.units.iter_mut().find(|u| u.id.as_str() == unit).unwrap();
    let len = u.text.len();
    edit(&mut u.text);
    assert_eq!(u.text.len(), len);
    r.manifest.revision = 2;
    r.manifest.content_hash = r.content_hash();
    let path = (2..)
        .map(|n| dir.join(format!("test-bible-{n}.obr")))
        .find(|p| !p.exists())
        .unwrap();
    obs_store::write_obr(&r, &path).unwrap();
    h.shell.st.session.library.add_file(&path).unwrap();
    h.shell
        .st
        .session
        .workspace
        .push_effect(obs_engine::Effect::Replaced("test-bible".into()));
    h.frames(3);
}

/// #259 AUD-UI-SHELL-5: a new revision of the Resource arriving while the popup Note editor
/// is open never makes the Note quote other words. Where the new revision has other text at
/// the selection, saving is refused and the editor keeps the text and says why; where it has
/// the same text there, the Note saves on it as usual, against the new revision.
#[test]
fn a_revision_arriving_under_an_open_note_never_changes_its_quote() {
    let dir = folder("note-revision");
    let mut h = launch(&dir);
    let tb = h.tab_id("TB");
    let (a, b, word, r) = word_in_view(&h, tb, 120.0);
    let unit = view(&h, tb).doc.units[a.unit].id.as_str().to_string();
    double_click(&mut h, r.center());
    press_key(&mut h, egui::Key::N, cmd_shift());
    h.events.push(Event::Text("Started on the word".into()));
    h.frame();
    assert!(h.shell.st.note_editor.is_some());
    // The same bytes now read XXXXX.
    replace_tb(&mut h, &dir, &unit, |t| {
        t.replace_range(a.byte..b.byte, &"X".repeat(b.byte - a.byte));
    });
    press_key(&mut h, egui::Key::Enter, Modifiers::COMMAND);
    h.frame();
    let saved = h.shell.st.session.annotations(None).unwrap();
    let quotes: Vec<String> = saved
        .iter()
        .filter_map(|n| Some(n.anchor.as_ref()?.text_range()?.quote.clone()))
        .collect();
    assert!(
        quotes.is_empty(),
        "selected quote={word:?}; saved quote={quotes:?}: a background revision must not \
         silently change a pending Note's selected quote"
    );
    let ed = h
        .shell
        .st
        .note_editor
        .as_ref()
        .expect("the editor stays open, with its text");
    assert_eq!(ed.body, "Started on the word");
    assert!(
        h.texts().iter().any(|t| t.contains("TB changed")),
        "{:?}",
        h.texts()
    );
    h.click_text("Discard");
    assert!(h.shell.st.note_editor.is_none());

    // A word on a later line, and a revision that changes other text only (the X's go
    // back, and one letter before the word changes): the Note saves on it.
    let (a, b, word, r) = word_in_view(&h, tb, r.bottom() + 1.0);
    let unit = view(&h, tb).doc.units[a.unit].id.as_str().to_string();
    double_click(&mut h, r.center());
    press_key(&mut h, egui::Key::N, cmd_shift());
    h.events.push(Event::Text("Still here".into()));
    h.frame();
    replace_tb(&mut h, &dir, &unit, |t| {
        let at = if a.byte > 0 { 0 } else { b.byte + 1 };
        let c = t[at..].chars().next().unwrap();
        let to = if c == 'Q' { "Z" } else { "Q" };
        assert!(c.is_ascii(), "{t}");
        t.replace_range(at..at + 1, to);
    });
    press_key(&mut h, egui::Key::Enter, Modifiers::COMMAND);
    h.frame();
    assert!(h.shell.st.note_editor.is_none(), "saved");
    let notes = h.shell.st.session.annotations(None).unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].body(), "Still here");
    let range = notes[0].anchor.as_ref().unwrap().text_range().unwrap();
    assert_eq!(range.quote, word);
    assert_eq!(
        range.content_hash,
        h.shell.st.session.library.get("test-bible").unwrap().manifest.content_hash
    );
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// ADR 0006: a Passage anchor whose origin range no longer reads as its quote (the Resource
/// changed) is drawn over its whole Verse(s) rather than disappearing.
#[test]
fn a_passage_anchor_with_a_stale_origin_falls_back_to_its_whole_verses() {
    let dir = folder("stale");
    let mut h = launch(&dir);
    let tb = h.tab_id("TB");
    let (a, _, _, r) = word_in_view(&h, tb, 200.0);
    double_click(&mut h, r.center());
    press_key(&mut h, egui::Key::Num1, Modifiers::ALT);
    let mut stored = h.shell.st.session.annotations(None).unwrap();
    assert_eq!(stored.len(), 1);
    let doc = std::sync::Arc::clone(&view(&h, tb).doc);
    let exact = crate::marks_for(&doc, &stored, &crate::HighlightLooks::built_in());
    assert_eq!(exact.len(), 1);
    assert_eq!(exact[0].start, a);
    // An origin re-anchoring did not find (#65) is kept but not drawn as words, even where
    // the text still reads as its quote: whole Verse(s).
    let verse = doc.verse_range_at(a.unit, a.byte).unwrap();
    let mut not_found = stored.clone();
    if let Some(obs_engine::Anchor::Passage {
        origin: Some(origin),
        ..
    }) = not_found[0].anchor.as_mut()
    {
        origin.orphaned = true;
    }
    let marks = crate::marks_for(&doc, &not_found, &crate::HighlightLooks::built_in());
    assert_eq!(marks.len(), 1, "still drawn: {marks:?}");
    assert_eq!(
        (marks[0].start.byte, marks[0].end.byte),
        (verse.start, verse.end)
    );
    let Some(obs_engine::Anchor::Passage {
        origin: Some(origin),
        ..
    }) = stored[0].anchor.as_mut()
    else {
        panic!(
            "a Bible selection gets a Passage anchor: {:?}",
            stored[0].anchor
        );
    };
    origin.quote = "no longer here".into();
    let marks = crate::marks_for(&doc, &stored, &crate::HighlightLooks::built_in());
    assert_eq!(marks.len(), 1, "the Highlight is still drawn");
    assert_eq!(
        marks[0].start,
        DocPos {
            unit: a.unit,
            byte: verse.start
        }
    );
    assert_eq!(
        marks[0].end,
        DocPos {
            unit: a.unit,
            byte: verse.end
        }
    );
    // A bare Text range has no Verses to fall back to.
    let Some(obs_engine::Anchor::Passage { origin, .. }) = stored[0].anchor.clone() else {
        unreachable!()
    };
    stored[0].anchor = Some(obs_engine::Anchor::TextRange {
        range: origin.unwrap(),
    });
    assert!(crate::marks_for(&doc, &stored, &crate::HighlightLooks::built_in()).is_empty());
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The Highlight's painted rects cover exactly the glyphs of its word: as many glyph quads
/// as the word has letters lie inside, and no glyph straddles an edge.
fn assert_aligned(h: &Harness, color: Color32, word: &str) -> Rect {
    let rects = painted(h, color);
    assert!(!rects.is_empty(), "no Highlight painted");
    let inside: Vec<Rect> = h
        .glyphs
        .iter()
        .filter(|g| rects.iter().any(|r| r.contains(g.center())))
        .copied()
        .collect();
    assert_eq!(
        inside.len(),
        word.chars().count(),
        "glyphs under the Highlight of {word:?}: {inside:?} in {rects:?}"
    );
    for g in &h.glyphs {
        let overlaps = rects.iter().any(|r| {
            let i = r.intersect(*g);
            i.width() > 0.75 && i.height() > 0.75
        });
        let within = rects.iter().any(|r| r.expand(2.0).contains_rect(*g));
        assert!(
            !overlaps || within || g.height() > rects[0].height(),
            "glyph {g:?} straddles {rects:?}"
        );
    }
    union(&rects)
}

#[test]
fn highlights_stay_aligned_with_their_glyphs_while_scrolling_and_after_a_text_size_change() {
    let dir = folder("aligned");
    let mut h = launch(&dir);
    let tb = h.tab_id("TB");
    let yellow = h.shell.st.palette.built_in_color("yellow");
    let (a, b, word, r) = word_in_view(&h, tb, 250.0);
    double_click(&mut h, r.center());
    press_key(&mut h, egui::Key::Num1, Modifiers::ALT);
    let at = assert_aligned(&h, yellow, &word);
    // The word under the Highlight, by hit-testing the laid-out text.
    let hit = |h: &mut Harness, p: Pos2| {
        let v = h.shell.st.tabs.get_mut(&tb).unwrap().view.as_mut().unwrap();
        let [ox, oy] = v.text_origin;
        v.pos_at(p.x - ox, p.y - oy).unwrap()
    };
    let p = hit(&mut h, at.center());
    assert!(a <= p && p < b, "{p:?} not in {a:?}..{b:?}");

    // Scroll: the Highlight moves with its glyphs, frame by frame.
    h.events.push(Event::PointerMoved(pane(&h).center()));
    let mut moved = 0.0;
    for _ in 0..8 {
        h.events.push(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, -9.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: Modifiers::NONE,
        });
        h.frame();
        let now = assert_aligned(&h, yellow, &word);
        moved = at.top() - now.top();
        assert!((now.left() - at.left()).abs() < 0.6);
    }
    assert!(moved > 20.0, "the view did not scroll: {moved}");

    // Larger text: the text is laid out again and the Highlight with it.
    let before = assert_aligned(&h, yellow, &word);
    h.shell.run("view.text_larger", &json!({}));
    h.shell.run("view.text_larger", &json!({}));
    settle_all(&mut h);
    let after = assert_aligned(&h, yellow, &word);
    assert!(
        after.width() > before.width() * 1.05,
        "{after:?} vs {before:?}"
    );
    let p = hit(&mut h, after.center());
    assert!(a <= p && p < b, "{p:?} not in {a:?}..{b:?}");
    // And a theme switch recolours it without a relayout.
    h.shell.run("view.page_tone", &json!({ "value": "dark" }));
    h.frame();
    assert_aligned(&h, crate::Palette::DARK.built_in_color("yellow"), &word);
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Every Highlight style keeps WCAG AA contrast for every ink, in every tone (#21 rule 3);
/// the Note marker stands out from the page.
#[test]
fn highlight_swatches_and_note_markers_are_legible_in_every_tone() {
    for palette in [
        crate::Palette::PAPER,
        crate::Palette::SEPIA,
        crate::Palette::DARK,
    ] {
        for style in obs_store::built_in_styles() {
            let paint = palette.paint(&style.appearance);
            assert_eq!(
                palette.check(style.appearance.look, paint.color),
                Ok(()),
                "{style:?}"
            );
            if paint.look == HighlightLook::Background {
                for (_, ink) in palette.inks() {
                    assert!(
                        crate::contrast(ink, paint.color) >= 4.5,
                        "{style:?} in {palette:?}"
                    );
                }
            }
        }
        assert!(crate::contrast(palette.note_marker, palette.page) >= 3.0);
        // Margin marks in other Bibles (#63) are thin bars: WCAG's 3:1 for graphics.
        for style in obs_store::built_in_styles() {
            let c = crate::contrast(palette.paint(&style.appearance).mark, palette.page);
            assert!(c >= 3.0, "{} margin mark {c:.2}:1 in {palette:?}", style.id);
        }
        assert!(crate::contrast(palette.underline, palette.page) >= 3.0);
    }
}

#[test]
fn the_toolbar_symbols_are_in_the_chrome_fonts() {
    let h = Harness::new();
    h.ctx.fonts_mut(|f| {
        let missing: Vec<&str> = ["Note", "Search this"]
            .into_iter()
            .filter(|s| !f.has_glyphs(&egui::FontId::proportional(14.0), s))
            .collect();
        assert!(missing.is_empty(), "{missing:?}");
    });
}

/// Writes `target/previews/annotations-*.ppm` (the whole window, rasterized from the real
/// frame) when `UPDATE_PREVIEWS` is set: every style, a Note marker with its preview, the
/// toolbar and the Note editor, in a light and a dark tone. The text is the generated test
/// Bible.
#[test]
fn previews() {
    if std::env::var_os("UPDATE_PREVIEWS").is_none() {
        return;
    }
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
    let dir = folder("previews");
    let mut h = launch(&dir);
    let tb = h.tab_id("TB");
    let mut y = 120.0;
    for (i, key) in [
        egui::Key::Num1,
        egui::Key::Num2,
        egui::Key::Num3,
        egui::Key::Num4,
        egui::Key::Num5,
    ]
    .into_iter()
    .enumerate()
    {
        let (_, _, _, r) = word_in_view(&h, tb, y);
        y = r.bottom() + if i == 2 { 1.0 } else { 40.0 };
        double_click(&mut h, r.center());
        press_key(&mut h, key, Modifiers::ALT);
    }
    let (_, _, _, r) = word_in_view(&h, tb, y);
    double_click(&mut h, r.center());
    press_key(&mut h, egui::Key::N, cmd_shift());
    h.events
        .push(Event::Text("Laban's heap: a witness between them.".into()));
    h.frame();
    press_key(&mut h, egui::Key::Enter, Modifiers::COMMAND);
    h.frame();
    let m = painted_any(&h, h.shell.st.palette.note_marker)[0];
    h.events.push(Event::PointerMoved(m.center()));
    h.frames(3);
    h.screenshot(&out.join("annotations-marks-and-preview.ppm"));
    let (_, _, _, r) = word_in_view(&h, tb, 500.0);
    double_click(&mut h, r.center());
    h.screenshot(&out.join("annotations-toolbar.ppm"));
    press_key(&mut h, egui::Key::N, cmd_shift());
    h.events.push(Event::Text("Draft".into()));
    h.frames(2);
    h.screenshot(&out.join("annotations-note-editor.ppm"));
    press_key(&mut h, egui::Key::Escape, Modifiers::NONE);
    h.shell.run("view.theme", &json!({ "value": "dark" }));
    h.frames(2);
    h.screenshot(&out.join("annotations-dark.ppm"));
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// #65: an Orphaned annotation is listed under ⚠ Needs attention with its quote and is not
/// drawn; selecting text in TB and clicking Re-attach here moves it there (drawn at once) and
/// clears its warning; Ctrl+Z makes it orphaned again.
#[test]
fn reattach_moves_an_orphaned_annotation_to_the_selection_and_clears_its_warning() {
    let dir = folder("reattach");
    let mut h = launch(&dir);
    let tb = h.tab_id("TB");
    let (a, b, word, r) = word_in_view(&h, tb, 200.0);
    // A Highlight made against an older revision of TB, on words this one no longer has.
    let unit = view(&h, tb).doc.units[a.unit].id.clone();
    let session = &mut h.shell.st.session;
    let notebook = session.default_notebook().unwrap().id;
    let gone = obs_engine::Annotation {
        id: obs_store::annotation::new_id(),
        notebook_id: notebook,
        content: Content::Highlight {
            style: "yellow".into(),
        },
        anchor: Some(obs_engine::Anchor::TextRange {
            range: obs_engine::TextRange {
                resource_id: "test-bible".into(),
                unit,
                start: a.byte as u32,
                end: a.byte as u32 + 14,
                quote: "vanished words".into(),
                prefix: String::new(),
                suffix: String::new(),
                content_hash: "sha256:an-older-revision".into(),
                native_ref: None,
                whole_verses: false,
                orphaned: false,
                checked_hash: None,
            },
        }),
        created_at: 1,
        modified_at: 1,
    };
    let id = gone.id.clone();
    session
        .user
        .as_ref()
        .unwrap()
        .apply_changes(&[obs_engine::Change::insert(obs_engine::Row::Annotation(
            gone,
        ))])
        .unwrap();
    // Re-anchoring (the library-load job) cannot find it: it is orphaned, and says so.
    h.shell
        .run("annotation.reanchor", &json!({ "resource": "test-bible" }))
        .unwrap();
    for _ in 0..20 {
        h.frame();
        if !h.shell.st.session.reanchoring.is_busy() {
            break;
        }
    }
    h.frames(2);
    assert!(h.shell.st.session.annotation(&id).unwrap().is_orphaned());
    assert!(
        h.texts().iter().any(|t| t.contains("1 needs attention")),
        "the report toast: {:?}",
        h.texts()
    );
    let yellow = h.shell.st.palette.built_in_color("yellow");
    assert!(painted(&h, yellow).is_empty(), "an orphan is not drawn");

    // Notes → ⚠ Needs attention lists it with its quote and the warning.
    h.shell.run("tool.notes", &json!({})).unwrap();
    h.frames(10);
    assert!(h.find("Needs attention").is_some(), "{:?}", h.texts());
    h.click_text("Needs attention");
    h.frames(10);
    let warning = |h: &Harness| h.texts().iter().any(|t| t.contains("was not found in TB"));
    assert!(warning(&h), "{:?}", h.texts());
    assert_eq!(h.shell.st.notes_ui.cards_len(), 1);
    assert!(
        h.find("Keep without anchor").is_none(),
        "a Highlight cannot lose its text"
    );

    // Select a word in TB, then Re-attach here.
    double_click(&mut h, r.center());
    assert!(h.shell.st.has_selection(tb));
    h.frames(2);
    h.click_text("Re-attach here");
    h.frames(10);
    let a2 = h.shell.st.session.annotation(&id).unwrap();
    assert!(!a2.is_orphaned());
    let range = a2.anchor.as_ref().unwrap().text_range().unwrap();
    assert_eq!(range.quote, word);
    assert_eq!((range.start as usize, range.end as usize), (a.byte, b.byte));
    assert!(!warning(&h), "the warning is gone: {:?}", h.texts());
    assert_eq!(h.shell.st.notes_ui.cards_len(), 0);
    assert!(
        h.texts()
            .iter()
            .any(|t| t.starts_with("Nothing needs attention")),
        "{:?}",
        h.texts()
    );
    // Drawn over the word at once (the selection made way for it).
    assert!(!h.shell.st.has_selection(tb));
    let rects = painted(&h, yellow);
    assert!(covers(union(&rects), r), "{rects:?} vs word {r:?}");

    // One undo step: orphaned again, not drawn, listed again.
    press_key(&mut h, egui::Key::Z, Modifiers::COMMAND);
    h.frames(5);
    assert!(h.shell.st.session.annotation(&id).unwrap().is_orphaned());
    assert!(painted(&h, yellow).is_empty());
    assert!(warning(&h));
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// #65: while a re-anchoring job for TB is pending, its Marks are not painted (they may point
/// at text that moved); once the job is applied they are, over the same word.
#[test]
fn marks_are_not_painted_while_their_resource_is_being_reanchored() {
    let dir = folder("pending");
    let mut h = launch(&dir);
    let tb = h.tab_id("TB");
    let (_, _, _, r) = word_in_view(&h, tb, 200.0);
    double_click(&mut h, r.center());
    press_key(&mut h, egui::Key::Num2, Modifiers::ALT);
    let green = h.shell.st.palette.built_in_color("green");
    assert!(covers(union(&painted(&h, green)), r));

    // Its anchor now says it was made against an older revision of TB.
    let session = &mut h.shell.st.session;
    let before = session.annotations(None).unwrap().remove(0);
    let mut stale = before.clone();
    match stale.anchor.as_mut() {
        Some(obs_engine::Anchor::Passage {
            origin: Some(range),
            ..
        })
        | Some(obs_engine::Anchor::TextRange { range }) => {
            range.content_hash = "sha256:an-older-revision".into();
        }
        other => panic!("{other:?}"),
    }
    session
        .user
        .as_ref()
        .unwrap()
        .apply_changes(&[obs_engine::Change::update(
            obs_engine::Row::Annotation(before),
            obs_engine::Row::Annotation(stale),
        )])
        .unwrap();
    // Other background work queued first keeps the re-anchoring job waiting a few frames
    // (one job step runs per frame without worker threads).
    for _ in 0..3 {
        session
            .jobs()
            .submit_fn("Other work", |_| Ok::<_, String>(()));
    }
    let out = h
        .shell
        .run("annotation.reanchor", &json!({ "resource": "test-bible" }))
        .unwrap();
    assert_eq!(out["queued"], json!(["test-bible"]));
    for frame in 0..3 {
        h.frame();
        assert!(
            h.shell.st.session.is_reanchoring("test-bible"),
            "frame {frame}: still pending"
        );
        assert!(
            painted(&h, green).is_empty(),
            "frame {frame}: not painted while re-anchoring is pending"
        );
    }
    for _ in 0..20 {
        h.frame();
        if !h.shell.st.session.reanchoring.is_busy() {
            break;
        }
    }
    h.frames(2);
    assert!(!h.shell.st.session.is_reanchoring("test-bible"));
    assert!(
        covers(union(&painted(&h, green)), r),
        "painted again once re-anchored"
    );
    let hash = h
        .shell
        .st
        .session
        .library
        .get("test-bible")
        .unwrap()
        .manifest
        .content_hash
        .clone();
    let a = h.shell.st.session.annotations(None).unwrap().remove(0);
    assert_eq!(a.anchor.unwrap().text_range().unwrap().content_hash, hash);
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}
