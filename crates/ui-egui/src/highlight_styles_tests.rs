//! Custom Highlight styles and palettes (#181) through the headless harness: a real `.obr`
//! with Rom 8:27-29 and a real User database. "Promises" (a gold box) on Rom 8:28 survives a
//! restart and is drawn in every Page tone; deleting a used style in View → Highlight
//! styles… moves its Highlight and Ctrl+Z brings both back; Alt+1…9 and the menus follow the
//! active palette; tone adjustments that fail WCAG AA are refused.

use std::path::{Path, PathBuf};

use egui::Color32;
use obs_engine::testing::bibles::{bible, v, write_library};
use obs_engine::{Content, HighlightLook, Rgb, StyleAppearance, StyleTone, TabId};
use obs_scripture::{Book, Verse};
use serde_json::Value;

use super::*;
use crate::palette::{MARK_CONTRAST, Palette, to_color};
use crate::selection::{DocPos, range_rects};

const ROM_8: [&str; 3] = [
    "And he that searcheth the hearts knoweth what is the mind of the Spirit, because he maketh intercession for the saints according to the will of God.",
    "And we know that all things work together for good to them that love God, to them who are the called according to his purpose.",
    "For whom he did foreknow, he also did predestinate to be conformed to the image of his Son, that he might be the firstborn among many brethren.",
];

fn rom(n: u16) -> Verse {
    v(Book::Romans, 8, n)
}

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("obs-styles-ui-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Launch (or relaunch) on `dir`'s User database with the KJV of Rom 8:27-29 open at 8:28.
fn launch(dir: &Path) -> Harness {
    let kjv = bible(
        "kjv",
        "KJV",
        &(27..=29)
            .map(|n| (rom(n), ROM_8[n as usize - 27]))
            .collect::<Vec<_>>(),
        None,
        &[],
    );
    write_library(dir, &[kjv]);
    let mut h = Harness::launch_full(Some(&dir.join("user.db")), JobMode::Inline, Some(dir));
    h.shell
        .st
        .session
        .library
        .add_file(&dir.join("kjv.obr"))
        .unwrap();
    let open = h
        .shell
        .st
        .session
        .workspace
        .tabs()
        .any(|t| t.title == "KJV");
    if !open {
        h.shell.run("resource.open", &json!({ "id": "kjv" }));
        h.frames(2);
    }
    let tab = h.tab_id("KJV");
    h.shell
        .run("tab.go", &json!({ "tab": tab.0, "text": "Rom 8:28" }));
    settle_all(&mut h);
    h
}

fn view(h: &Harness, tab: TabId) -> &crate::view::ReadingView {
    h.shell.st.tabs[&tab].view.as_ref().unwrap()
}

/// The KJV's reading pane on screen.
fn pane(h: &Harness) -> Rect {
    let (r, _) = h
        .panels()
        .into_iter()
        .find(|(_, titles)| titles.iter().any(|t| t == "KJV"))
        .unwrap();
    Rect::from_min_max(pos2(r.left(), r.top() + 60.0), r.max)
}

/// Rom 8:28 as `annotation.highlight` params, and its line rects on screen.
fn rom_8_28(h: &Harness) -> (Value, Vec<Rect>) {
    let tab = h.tab_id("KJV");
    let v = view(h, tab);
    let (unit, start) = v.doc.verse_position(rom(28)).unwrap();
    let range = v.doc.verse_range_at(unit, start).unwrap();
    let a = DocPos {
        unit,
        byte: range.start,
    };
    let b = DocPos {
        unit,
        byte: range.end,
    };
    let [ox, oy] = v.text_origin;
    let rects = range_rects(v, &[(a, b)], pane(h).bottom() - oy)
        .remove(0)
        .into_iter()
        .map(|[x0, y0, x1, y1]| Rect::from_min_max(pos2(ox + x0, oy + y0), pos2(ox + x1, oy + y1)))
        .collect();
    let unit_id = v.doc.units[unit].id.as_str();
    let params = json!({
        "resource": "kjv", "start_unit": unit_id, "start": range.start,
        "end_unit": unit_id, "end": range.end,
    });
    (params, rects)
}

/// Rects (fills, outlines, lines) painted in `color` in the KJV's pane.
fn painted(h: &Harness, color: Color32) -> Vec<Rect> {
    let pane = pane(h);
    (h.rects.iter())
        .filter(|(r, c)| *c == color && pane.contains(r.center()))
        .map(|(r, _)| *r)
        .collect()
}

/// Whether every piece of `verse` (its rects on screen) is marked by a rect painted in `color`
/// spanning its columns on its line (a box runs around a line's run of words).
fn marks_each(h: &Harness, color: Color32, verse: &[Rect]) -> bool {
    let painted = painted(h, color);
    verse.iter().all(|piece| {
        painted.iter().any(|r| {
            r.left() <= piece.left() + 0.6
                && r.right() >= piece.right() - 0.6
                && r.top() < piece.center().y
                && r.bottom() > piece.center().y
        })
    })
}

fn press_key(h: &mut Harness, key: egui::Key, modifiers: Modifiers) {
    h.events.push(Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    });
    h.frames(2);
}

fn create_promises(h: &mut Harness) -> String {
    let out = h
        .shell
        .run(
            "highlight_style.create",
            &json!({ "name": "Promises", "look": "box", "color": "#c9a227" }),
        )
        .unwrap();
    out["id"].as_str().unwrap().to_string()
}

fn style_of(h: &Harness, id: &str) -> String {
    match h.shell.st.session.annotation(id).unwrap().content {
        Content::Highlight { style } => style,
        Content::Note { .. } => panic!("not a Highlight"),
    }
}

/// Acceptance criterion 1: "Promises" (gold box) made, Rom 8:28 highlighted with it, the app
/// restarted: it is drawn as a box around the verse in Paper, Sepia and Dark, each time in a
/// colour that meets WCAG AA against that tone's page.
#[test]
fn promises_as_a_gold_box_on_rom_8_28_renders_in_every_page_tone_after_a_restart() {
    let dir = folder("promises");
    let (style, highlight) = {
        let mut h = launch(&dir);
        let style = create_promises(&mut h);
        let (mut params, _) = rom_8_28(&h);
        params["style"] = json!(style);
        let out = h.shell.run("annotation.highlight", &params).unwrap();
        (style, out["ids"][0].as_str().unwrap().to_string())
    };
    let mut h = launch(&dir);
    assert_eq!(
        style_of(&h, &highlight),
        style,
        "the Highlight kept its style"
    );
    let promises = h.shell.st.session.highlight_style(&style).unwrap();
    assert_eq!(promises.name, "Promises");
    let mut colours = Vec::new();
    for tone in StyleTone::ALL {
        h.shell
            .run("view.page_tone", &json!({ "value": tone.id() }));
        h.frames(3);
        let palette = h.shell.st.palette;
        assert_eq!(palette.tone, tone);
        let paint = palette.paint(&promises.appearance);
        assert_eq!(paint.look, HighlightLook::Box);
        assert_eq!(palette.check(HighlightLook::Box, paint.color), Ok(()));
        assert!(crate::contrast(paint.color, palette.page) >= MARK_CONTRAST);
        let (_, verse) = rom_8_28(&h);
        assert!(
            marks_each(&h, paint.color, &verse),
            "{tone:?}: {:?} around {verse:?}",
            painted(&h, paint.color)
        );
        colours.push(paint.color);
    }
    // Gold is too faint for a line on light paper: there it deepens; on Dark it is itself.
    assert_ne!(colours[0], to_color(promises.appearance.color));
    assert_eq!(colours[2], to_color(promises.appearance.color));
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Acceptance criterion 2, in View → Highlight styles…: Delete on a used style asks where its
/// Highlights go, moves them there and deletes it; Ctrl+Z brings back both.
#[test]
fn deleting_a_used_style_in_the_window_moves_its_highlight_and_ctrl_z_restores_both() {
    let dir = folder("delete");
    let mut h = launch(&dir);
    let style = create_promises(&mut h);
    let (mut params, verse) = rom_8_28(&h);
    params["style"] = json!(style);
    let out = h.shell.run("annotation.highlight", &params).unwrap();
    let highlight = out["ids"][0].as_str().unwrap().to_string();
    h.frames(2);
    let gold = h.shell.st.palette.paint(
        &h.shell
            .st
            .session
            .highlight_style(&style)
            .unwrap()
            .appearance,
    );
    assert!(marks_each(&h, gold.color, &verse));

    h.shell.run("view.highlight_styles", &json!({}));
    h.frames(3);
    assert!(h.find("Highlight styles").is_some(), "{:?}", h.texts());
    // One Delete… per style, top to bottom: Promises is the sixth.
    let mut deletes: Vec<Rect> = (h.shapes.iter())
        .filter(|(_, t)| t == "Delete…")
        .map(|(r, _)| *r)
        .collect();
    deletes.sort_by(|a, b| a.top().total_cmp(&b.top()));
    assert_eq!(deletes.len(), 6, "{:?}", h.texts());
    h.click(deletes[5].center());
    let question = "1 Highlight(s) use “Promises”. Move them to:";
    assert!(h.find(question).is_some(), "{:?}", h.texts());
    assert!(
        h.shell.st.session.highlight_style(&style).is_ok(),
        "not yet"
    );
    h.click_text("Delete and move");
    assert!(h.find(question).is_none());
    assert!(h.shell.st.session.highlight_style(&style).is_err());
    assert_eq!(style_of(&h, &highlight), "yellow");
    h.frames(2);
    let yellow = h.shell.st.palette.built_in_color("yellow");
    assert!(marks_each(&h, yellow, &verse));
    assert!(!marks_each(&h, gold.color, &verse));

    press_key(&mut h, egui::Key::Z, Modifiers::COMMAND);
    assert_eq!(
        h.shell.st.session.highlight_style(&style).unwrap().name,
        "Promises"
    );
    assert_eq!(style_of(&h, &highlight), style);
    assert!(!marks_each(&h, yellow, &verse));
    assert!(marks_each(&h, gold.color, &verse));
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Alt+6 is the sixth style of the active palette; the right-click menu lists the active
/// palette; switching to an empty palette leaves Alt+1 to nothing.
#[test]
fn the_keys_and_the_right_click_menu_follow_the_active_palette() {
    let dir = folder("keys");
    let mut h = launch(&dir);
    let style = create_promises(&mut h);
    let tab = h.tab_id("KJV");
    let (_, verse) = rom_8_28(&h);
    let word = h
        .shapes
        .iter()
        .find(|(r, t)| t == "know" && verse[0].contains(r.center()))
        .map(|(r, _)| r.center());
    let at = word.unwrap_or_else(|| verse[0].left_center() + vec2(60.0, 0.0));
    super::annotation::double_click(&mut h, at);
    assert!(h.shell.st.has_selection(tab));
    press_key(&mut h, egui::Key::Num6, Modifiers::ALT);
    let made = h.shell.st.session.annotations(None).unwrap();
    assert_eq!(made.len(), 1);
    assert_eq!(
        made[0].content,
        Content::Highlight {
            style: style.clone()
        }
    );

    // The right-click menu lists the palette, Promises last.
    h.events.push(Event::PointerMoved(at));
    h.frame();
    for pressed in [true, false] {
        h.events.push(Event::PointerButton {
            pos: at,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.frame();
    }
    h.frame();
    for label in ["Yellow", "Red underline", "Promises", "Highlight styles…"] {
        assert!(h.find(label).is_some(), "{label}: {:?}", h.texts());
    }
    // Each style after its swatch.
    let swatches = h
        .texts()
        .iter()
        .filter(|t| **t == "[square-filled]")
        .count();
    assert!(swatches >= 3, "{:?}", h.texts());
    press_key(&mut h, egui::Key::Escape, Modifiers::NONE);

    // A new palette, made active: no styles, so Alt+1 highlights nothing.
    let out = h
        .shell
        .run("highlight_palette.create", &json!({ "name": "Themes" }))
        .unwrap();
    h.shell
        .run("highlight_palette.activate", &json!({ "id": out["id"] }));
    super::annotation::double_click(&mut h, at);
    press_key(&mut h, egui::Key::Num1, Modifiers::ALT);
    assert_eq!(h.shell.st.session.annotations(None).unwrap().len(), 1);
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A tone colour typed in the window that fails WCAG AA there is refused with the reason; one
/// that passes is saved. One stored past the window is drawn derived instead.
#[test]
fn a_tone_adjustment_that_fails_aa_is_refused_and_never_drawn() {
    let dir = folder("adjust");
    let mut h = launch(&dir);
    h.shell.run("view.highlight_styles", &json!({}));
    h.frames(2);
    h.click_text("+ New style");
    let book = h.shell.st.session.active_highlight_styles().unwrap();
    let made = book.last().unwrap().clone();
    assert_eq!(book.len(), 6);
    assert_eq!(made.appearance.look, HighlightLook::Background);
    // Its tone fields, Paper first, are empty: the colours are derived.
    let mut fields: Vec<Rect> = (h.shapes.iter())
        .filter(|(_, t)| t == "#rrggbb")
        .map(|(r, _)| *r)
        .collect();
    fields.sort_by(|a, b| a.top().total_cmp(&b.top()));
    assert_eq!(fields.len(), 3, "{:?}", h.texts());
    let paper = fields[0].center();
    // Near-black behind dark ink: refused, nothing saved.
    h.click(paper);
    h.events.push(Event::Text("#202020".into()));
    h.frame();
    press_key(&mut h, egui::Key::Enter, Modifiers::NONE);
    let refused = h
        .texts()
        .into_iter()
        .find(|t| t.contains("does not meet WCAG AA"))
        .map(String::from);
    assert!(refused.is_some(), "{:?}", h.texts());
    assert!(refused.unwrap().contains("not saved"));
    let now = h.shell.st.session.highlight_style(&made.id).unwrap();
    assert_eq!(now.appearance.paper, None);
    // A light rose passes: saved, and drawn as typed.
    h.click(paper);
    h.events.push(Event::Text("#ffe0f0".into()));
    h.frame();
    press_key(&mut h, egui::Key::Enter, Modifiers::NONE);
    let now = h.shell.st.session.highlight_style(&made.id).unwrap();
    assert_eq!(now.appearance.paper, Some(Rgb::new(0xff, 0xe0, 0xf0)));
    assert_eq!(
        Palette::PAPER.paint(&now.appearance).color,
        Color32::from_rgb(0xff, 0xe0, 0xf0)
    );
    // Stored without the window's check: drawn derived, never the failing colour.
    let faint = StyleAppearance {
        paper: Some(Rgb::new(0x20, 0x20, 0x20)),
        ..now.appearance
    };
    let drawn = Palette::PAPER.paint(&faint).color;
    assert_ne!(drawn, Color32::from_rgb(0x20, 0x20, 0x20));
    assert_eq!(
        Palette::PAPER.check(HighlightLook::Background, drawn),
        Ok(())
    );
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The name field on the row whose key reads `key` ("Alt+1"): the text just right of it.
fn name_field(h: &Harness, key: &str) -> Option<(Rect, String)> {
    let k = h.find(key)?;
    (h.shapes.iter())
        .filter(|(r, t)| {
            (r.center().y - k.center().y).abs() < 6.0 && r.left() > k.right() && !t.is_empty()
        })
        .min_by(|a, b| a.0.left().total_cmp(&b.0.left()))
        .cloned()
}

/// A rename typed in the window, undone with Ctrl+Z outside the field: the field shows the
/// old name again, and focusing and leaving it without typing neither renames nor loses redo.
#[test]
fn undoing_a_rename_typed_in_the_window_refreshes_the_field_and_keeps_redo() {
    let dir = folder("undo-rename");
    let mut h = launch(&dir);
    h.shell.run("view.highlight_styles", &json!({}));
    h.frames(3);
    let (field, text) = name_field(&h, "Alt+1").unwrap();
    assert_eq!(text, "Yellow");
    h.click(field.right_center() + vec2(2.0, 0.0));
    press_key(&mut h, egui::Key::End, Modifiers::NONE);
    h.events.push(Event::Text("ish".into()));
    h.frame();
    press_key(&mut h, egui::Key::Enter, Modifiers::NONE);
    let name = |h: &Harness| h.shell.st.session.highlight_style("yellow").unwrap().name;
    assert_eq!(name(&h), "Yellowish");
    assert_eq!(name_field(&h, "Alt+1").unwrap().1, "Yellowish");

    press_key(&mut h, egui::Key::Z, Modifiers::COMMAND);
    assert_eq!(name(&h), "Yellow");
    assert_eq!(name_field(&h, "Alt+1").unwrap().1, "Yellow");
    assert_eq!(
        h.shell.st.session.undo_history().redo_label(),
        Some("Edit Highlight style")
    );
    // In and out of the field without typing: nothing is saved, redo still there.
    let (field, _) = name_field(&h, "Alt+1").unwrap();
    h.click(field.center());
    press_key(&mut h, egui::Key::Enter, Modifiers::NONE);
    assert_eq!(name(&h), "Yellow");
    assert_eq!(
        h.shell.st.session.undo_history().redo_label(),
        Some("Edit Highlight style")
    );
    h.shell.run("edit.redo", &json!({}));
    h.frames(2);
    assert_eq!(name(&h), "Yellowish");
    assert_eq!(name_field(&h, "Alt+1").unwrap().1, "Yellowish");
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A new colour clears a style's tone adjustments: the tone field empties with it, and
/// focusing and leaving it does not bring the old adjustment back.
#[test]
fn a_new_colour_empties_the_adjusted_tone_field_for_good() {
    let dir = folder("colour-reset");
    let mut h = launch(&dir);
    h.shell.run("view.highlight_styles", &json!({}));
    h.frames(2);
    h.click_text("+ New style");
    let made = (h.shell.st.session.active_highlight_styles().unwrap())
        .last()
        .unwrap()
        .clone();
    let tone_fields = |h: &Harness| {
        let mut f: Vec<Rect> = (h.shapes.iter())
            .filter(|(_, t)| t == "#rrggbb")
            .map(|(r, _)| *r)
            .collect();
        f.sort_by(|a, b| a.top().total_cmp(&b.top()));
        f
    };
    let paper = tone_fields(&h)[0].center();
    h.click(paper);
    h.events.push(Event::Text("#ffe0f0".into()));
    h.frame();
    press_key(&mut h, egui::Key::Enter, Modifiers::NONE);
    let style = |h: &Harness| h.shell.st.session.highlight_style(&made.id).unwrap();
    assert_eq!(style(&h).appearance.paper, Some(Rgb::new(0xff, 0xe0, 0xf0)));
    assert_eq!(tone_fields(&h).len(), 2, "Paper shows its adjustment");
    assert!(h.find("#ffe0f0").is_some(), "{:?}", h.texts());
    // The style's colour field shows its colour.
    let lavender = made.appearance.color.to_string();
    assert!(h.find(&lavender).is_some(), "{:?}", h.texts());
    h.frames(3);

    h.shell.run(
        "highlight_style.edit",
        &json!({ "id": made.id, "color": "#c8eebe" }),
    );
    h.frames(2);
    assert_eq!(style(&h).appearance.paper, None, "a new colour clears it");
    assert_eq!(tone_fields(&h).len(), 3, "{:?}", h.texts());
    assert!(h.find("#ffe0f0").is_none(), "{:?}", h.texts());
    assert!(h.find(&lavender).is_none(), "{:?}", h.texts());
    assert!(h.find("#c8eebe").is_some(), "{:?}", h.texts());
    h.click(paper);
    press_key(&mut h, egui::Key::Enter, Modifiers::NONE);
    assert_eq!(style(&h).appearance.paper, None, "not brought back");
    assert_eq!(
        h.shell.st.session.undo_history().undo_label(),
        Some("Edit Highlight style")
    );
    // Undo brings the adjustment back, and the field shows it again.
    press_key(&mut h, egui::Key::Z, Modifiers::COMMAND);
    assert_eq!(style(&h).appearance.paper, Some(Rgb::new(0xff, 0xe0, 0xf0)));
    assert!(h.find("#ffe0f0").is_some(), "{:?}", h.texts());
    assert!(h.find(&lavender).is_some(), "{:?}", h.texts());
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A text-colour Highlight over a background Highlight: each passes WCAG AA alone on Paper,
/// but grey on grey does not, so the glyphs are drawn deepened until they read on the
/// background actually under them (#21).
#[test]
fn text_coloured_over_a_background_highlight_still_meets_wcag_aa() {
    let dir = folder("overlap");
    let mut h = launch(&dir);
    let make = |h: &mut Harness, look: &str, color: &str| {
        let out = h
            .shell
            .run(
                "highlight_style.create",
                &json!({ "look": look, "color": color }),
            )
            .unwrap();
        out["id"].as_str().unwrap().to_string()
    };
    let grey = make(&mut h, "background", "#d0d0d0");
    let slate = make(&mut h, "text_color", "#5e5e5e");
    let palette = h.shell.st.palette;
    assert_eq!(palette.tone, StyleTone::Paper);
    let appearance =
        |h: &Harness, id: &str| h.shell.st.session.highlight_style(id).unwrap().appearance;
    let ground = palette.paint(&appearance(&h, &grey)).color;
    let text = palette.paint(&appearance(&h, &slate)).color;
    assert_eq!(ground, Color32::from_rgb(0xd0, 0xd0, 0xd0), "passes alone");
    assert_eq!(text, Color32::from_rgb(0x5e, 0x5e, 0x5e), "passes alone");
    assert!(crate::contrast(text, ground) < 4.5, "but not together");

    let (mut params, verse) = rom_8_28(&h);
    for style in [&grey, &slate] {
        params["style"] = json!(style);
        h.shell.run("annotation.highlight", &params).unwrap();
    }
    // Past the jump flash.
    h.frames(150);
    assert!(marks_each(&h, ground, &verse));
    let drawn: Vec<Color32> = (h.glyphs.iter().zip(h.meshes.chunks(4)))
        .filter(|(g, _)| verse.iter().any(|r| r.contains(g.center())))
        .map(|(_, c)| c[0])
        .collect();
    assert!(drawn.len() > 50, "{} glyphs", drawn.len());
    for c in &drawn {
        let ratio = crate::contrast(*c, ground);
        assert!(ratio >= 4.5, "{c:?} on {ground:?}: {ratio:.2}:1");
        assert_ne!(*c, palette.ink, "still a text colour, not the ink");
    }
    assert!(drawn.iter().all(|c| *c == drawn[0]), "{drawn:?}");
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Export the active palette to a JSON file and import it back, as the window's file dialogs
/// would: the copy is a new palette, made active.
#[test]
fn a_palette_exported_from_the_window_imports_as_a_new_active_palette() {
    let dir = folder("files");
    let mut h = launch(&dir);
    create_promises(&mut h);
    let file = dir.join("standard.highlights.json");
    h.shell
        .st
        .palette_file_chosen(Some("standard".into()), &file);
    assert!(file.is_file());
    h.shell.st.palette_file_chosen(None, &file);
    let active = h.shell.st.session.active_highlight_palette().unwrap();
    assert_eq!(active.name, "Standard (2)");
    let styles = h.shell.st.session.active_highlight_styles().unwrap();
    assert_eq!(styles.len(), 6);
    assert_eq!(styles[5].name, "Promises");
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Whatever colour a user picks, each look's derived colour meets WCAG AA in every tone, and
/// its margin mark shows against the page (#21).
#[test]
fn derived_colours_meet_wcag_aa_for_any_colour_look_and_tone() {
    let steps = [0u8, 51, 102, 153, 204, 255];
    let mut checked = 0;
    for tone in StyleTone::ALL {
        let palette = Palette::for_style_tone(tone);
        for r in steps {
            for g in steps {
                for b in steps {
                    for look in HighlightLook::ALL {
                        let a = StyleAppearance::new(look, Rgb::new(r, g, b));
                        let paint = palette.paint(&a);
                        assert_eq!(
                            palette.check(look, paint.color),
                            Ok(()),
                            "{tone:?} {look:?} {r},{g},{b} → {:?}",
                            paint.color
                        );
                        let mark = crate::contrast(paint.mark, palette.page);
                        assert!(mark >= MARK_CONTRAST, "{tone:?} {look:?} mark {mark:.2}");
                        checked += 1;
                    }
                }
            }
        }
    }
    assert_eq!(checked, 3 * 216 * 4);
    // A colour that already passes is kept as chosen.
    let gold = StyleAppearance::new(HighlightLook::Box, Rgb::new(0xc9, 0xa2, 0x27));
    assert_eq!(
        Palette::DARK.paint(&gold).color,
        Color32::from_rgb(0xc9, 0xa2, 0x27)
    );
}

/// Writes `target/previews/highlight-styles-*.ppm` when `UPDATE_PREVIEWS` is set: the styles
/// window with "Promises", and Rom 8:28 boxed in Paper, Sepia and Dark.
#[test]
fn previews() {
    if std::env::var_os("UPDATE_PREVIEWS").is_none() {
        return;
    }
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
    let dir = folder("previews");
    let mut h = launch(&dir);
    let style = create_promises(&mut h);
    let (mut params, _) = rom_8_28(&h);
    params["style"] = json!(style);
    h.shell.run("annotation.highlight", &params);
    let (mut grace, _) = rom_8_28(&h);
    grace["start"] = json!(grace["start"].as_u64().unwrap() + 4);
    grace["end"] = json!(grace["start"].as_u64().unwrap() + 7);
    grace["style"] = json!("green");
    h.shell.run("annotation.highlight", &grace);
    // Past the jump flash.
    h.frames(150);
    for tone in StyleTone::ALL {
        h.shell
            .run("view.page_tone", &json!({ "value": tone.id() }));
        h.frames(3);
        h.screenshot(&out.join(format!("highlight-styles-rom-8-28-{}.ppm", tone.id())));
    }
    h.shell.run("view.page_tone", &json!({ "value": "paper" }));
    h.shell.run("view.highlight_styles", &json!({}));
    h.frames(30);
    // Promises' swatch, left of its key: its colours in each tone below.
    let swatch = h.at("Alt+6") - vec2(48.0, 0.0);
    h.click(swatch);
    h.frames(30);
    h.screenshot(&out.join("highlight-styles-window.ppm"));
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The window's symbols are in the chrome fonts (no missing-glyph boxes).
#[test]
fn the_styles_window_symbols_are_in_the_chrome_fonts() {
    let h = Harness::new();
    h.ctx.fonts_mut(|f| {
        let missing: Vec<&str> = ["…"]
            .into_iter()
            .filter(|s| !f.has_glyphs(&egui::FontId::proportional(14.0), s))
            .collect();
        assert!(missing.is_empty(), "{missing:?}");
    });
}
