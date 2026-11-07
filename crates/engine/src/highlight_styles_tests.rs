//! Highlight styles and palettes (#181, ADR 0012): making, editing, reordering and deleting
//! styles as undo steps, palettes switched and exported, and both acceptance criteria at the
//! engine level: "Promises" survives a restart with its Highlight on Rom 8:28, and deleting a
//! used style moves its Highlights, undone together.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use obs_scripture::Book;
use serde_json::{Value, json};

use crate::testing::bibles::{bible, v, write_library};
use crate::*;

const ROM_8_28: &str = "And we know that all things work together for good to them that love God, to them who are the called according to his purpose.";

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "obs-highlight-styles-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        write_library(
            &root.join("library"),
            &[bible(
                "kjv",
                "KJV",
                &[(v(Book::Romans, 8, 28), ROM_8_28)],
                None,
                &[],
            )],
        );
        Self(root)
    }

    fn db(&self) -> PathBuf {
        self.0.join("user.db")
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A launch of the app: the library scanned and the User database opened.
fn launch(t: &Temp) -> Session {
    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(&t.0.join("library")).is_empty());
    let (store, _) = obs_store::UserStore::open(&t.db()).unwrap();
    s.attach_user(Box::new(store));
    s
}

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.run(cmd, &params)
        .unwrap_or_else(|e| panic!("{cmd} {params}: {e}"))
}

/// Highlight all of Rom 8:28 with `style` (None: the command's default).
fn highlight_rom_8_28(s: &mut Session, style: Option<&str>) -> String {
    let mut p = json!({
        "resource": "kjv",
        "start_unit": "ROM",
        "start": 0,
        "end": ROM_8_28.len(),
    });
    if let Some(style) = style {
        p["style"] = json!(style);
    }
    let out = run(s, "annotation.highlight", p);
    out["ids"][0].as_str().unwrap().to_string()
}

fn style_of(s: &Session, id: &str) -> String {
    match s.annotation(id).unwrap().content {
        Content::Highlight { style } => style,
        Content::Note { .. } => panic!("not a Highlight"),
    }
}

fn labels(s: &Session) -> Vec<String> {
    let styles = s.active_highlight_styles().unwrap();
    styles.iter().map(|st| style_label(st, &styles)).collect()
}

#[test]
fn the_standard_palette_is_active_with_the_five_built_ins_and_the_first_is_the_default() {
    let t = Temp::new("standard");
    let mut s = launch(&t);
    assert_eq!(
        s.active_highlight_palette().unwrap().id,
        STANDARD_PALETTE_ID
    );
    assert_eq!(
        labels(&s),
        ["Yellow", "Green", "Blue", "Pink", "Red underline"]
    );
    let h = highlight_rom_8_28(&mut s, None);
    assert_eq!(style_of(&s, &h), "yellow");
    let err = s
        .run(
            "annotation.highlight",
            &json!({ "resource": "kjv", "start_unit": "ROM", "start": 0, "end": 3,
                     "style": "nope" }),
        )
        .unwrap_err();
    assert!(err.to_string().contains("no Highlight style"), "{err}");
}

/// Acceptance criterion 1 (engine side): "Promises" (gold box) is made, Rom 8:28 highlighted
/// with it, and after a restart both are there as made. Drawing it in every Page tone is the
/// UI's (ui-egui `highlight_styles_tests`).
#[test]
fn a_promises_style_and_its_highlight_on_rom_8_28_survive_a_restart() {
    let t = Temp::new("promises");
    let (style, highlight) = {
        let mut s = launch(&t);
        let out = run(
            &mut s,
            "highlight_style.create",
            json!({ "name": "Promises", "look": "box", "color": "#C9A227" }),
        );
        let style = out["id"].as_str().unwrap().to_string();
        assert_eq!(out["style"]["look"], "box");
        assert_eq!(out["style"]["color"], "#c9a227");
        assert_eq!(out["style"]["palette_id"], STANDARD_PALETTE_ID);
        let h = highlight_rom_8_28(&mut s, Some(&style));
        (style, h)
    };
    let s = launch(&t);
    let promises = s.highlight_style(&style).unwrap();
    assert_eq!(promises.name, "Promises");
    assert_eq!(
        promises.appearance,
        StyleAppearance::new(HighlightLook::Box, Rgb::new(0xc9, 0xa2, 0x27))
    );
    assert_eq!(labels(&s).last().map(String::as_str), Some("Promises"));
    let a = s.annotation(&highlight).unwrap();
    assert_eq!(style_of(&s, &highlight), style);
    assert_eq!(a.anchor.unwrap().passage().unwrap().to_string(), "Rom 8:28");
}

/// Acceptance criterion 2: deleting a used style needs a replacement; with one, its
/// Highlights move to it and the style goes, as one undo step that brings back both.
#[test]
fn deleting_a_used_style_moves_its_highlights_and_one_undo_restores_both() {
    let t = Temp::new("delete");
    let mut s = launch(&t);
    let style = run(
        &mut s,
        "highlight_style.create",
        json!({ "name": "Promises", "look": "box", "color": "#c9a227" }),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    let first = highlight_rom_8_28(&mut s, Some(&style));
    let second = highlight_rom_8_28(&mut s, Some(&style));
    let other = highlight_rom_8_28(&mut s, Some("blue"));
    let before = s.user.as_ref().unwrap().annotation_dump().unwrap();

    let err = s
        .run("highlight_style.delete", &json!({ "id": style }))
        .unwrap_err();
    assert!(err.to_string().contains("2 Highlight(s) use it"), "{err}");
    let err = s
        .run(
            "highlight_style.delete",
            &json!({ "id": style, "replacement": style }),
        )
        .unwrap_err();
    assert!(err.to_string().contains("not being deleted"), "{err}");
    assert_eq!(s.user.as_ref().unwrap().annotation_dump().unwrap(), before);

    let out = run(
        &mut s,
        "highlight_style.delete",
        json!({ "id": style, "replacement": "green" }),
    );
    assert_eq!(out["moved"], 2);
    assert!(s.highlight_style(&style).is_err());
    assert_eq!(style_of(&s, &first), "green");
    assert_eq!(style_of(&s, &second), "green");
    assert_eq!(style_of(&s, &other), "blue");
    assert_eq!(
        s.undo_history().undo_label(),
        Some("Delete Highlight style")
    );

    assert_eq!(
        run(&mut s, "edit.undo", json!({}))["undone"],
        "Delete Highlight style"
    );
    assert_eq!(s.user.as_ref().unwrap().annotation_dump().unwrap(), before);
    assert_eq!(s.highlight_style(&style).unwrap().name, "Promises");
    assert_eq!(style_of(&s, &first), style);
    assert_eq!(style_of(&s, &second), style);

    run(&mut s, "edit.redo", json!({}));
    assert!(s.highlight_style(&style).is_err());
    assert_eq!(style_of(&s, &first), "green");
}

#[test]
fn an_unused_style_deletes_without_a_replacement_and_names_are_checked() {
    let t = Temp::new("unused");
    let mut s = launch(&t);
    let id = run(
        &mut s,
        "highlight_style.create",
        json!({ "name": "People", "color": "#336699" }),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    for (name, why) in [
        ("people", "already has a style named"),
        ("a\tb", "control characters"),
    ] {
        let err = s
            .run(
                "highlight_style.create",
                &json!({ "name": name, "color": "#336699" }),
            )
            .unwrap_err();
        assert!(err.to_string().contains(why), "{name}: {err}");
    }
    let err = s
        .run("highlight_style.create", &json!({ "color": "gold" }))
        .unwrap_err();
    assert!(err.to_string().contains("#rrggbb"), "{err}");
    // An unnamed style is called by its place.
    run(
        &mut s,
        "highlight_style.create",
        json!({ "color": "#112233", "look": "text_color" }),
    );
    assert_eq!(labels(&s)[5..], ["People", "Style 7"]);
    assert_eq!(
        run(&mut s, "highlight_style.delete", json!({ "id": id }))["moved"],
        0
    );
    assert_eq!(labels(&s)[5..], ["Style 6"]);
}

#[test]
fn renaming_recolouring_and_reordering_are_undo_steps_and_the_order_is_the_key_order() {
    let t = Temp::new("edit");
    let mut s = launch(&t);
    // Rename, and adjust the Dark tone.
    run(
        &mut s,
        "highlight_style.edit",
        json!({ "id": "pink", "name": "Sin", "dark": "#502030" }),
    );
    let pink = s.highlight_style("pink").unwrap();
    assert_eq!(pink.name, "Sin");
    assert_eq!(pink.appearance.dark, Some(Rgb::new(0x50, 0x20, 0x30)));
    assert_eq!(pink.appearance.sepia, Some(Rgb::new(240, 200, 208)), "kept");
    // Back to a derived Dark colour.
    run(
        &mut s,
        "highlight_style.edit",
        json!({ "id": "pink", "dark": "" }),
    );
    assert_eq!(s.highlight_style("pink").unwrap().appearance.dark, None);
    // A new colour drops every adjustment; a new look keeps the colour.
    run(
        &mut s,
        "highlight_style.edit",
        json!({ "id": "pink", "color": "#ff88aa", "look": "underline" }),
    );
    let pink = s.highlight_style("pink").unwrap();
    assert_eq!(
        pink.appearance,
        StyleAppearance::new(HighlightLook::Underline, Rgb::new(0xff, 0x88, 0xaa))
    );
    // Move it first: Ctrl+1 is now Sin.
    run(
        &mut s,
        "highlight_style.move",
        json!({ "id": "pink", "position": 0 }),
    );
    assert_eq!(
        labels(&s),
        ["Sin", "Yellow", "Green", "Blue", "Red underline"]
    );
    assert_eq!(
        s.undo_history().undo_label(),
        Some("Reorder Highlight styles")
    );
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(
        labels(&s),
        ["Yellow", "Green", "Blue", "Sin", "Red underline"]
    );
    for _ in 0..3 {
        run(&mut s, "edit.undo", json!({}));
    }
    assert_eq!(
        s.highlight_style("pink").unwrap(),
        obs_store::built_in_styles()[3]
    );
}

#[test]
fn palettes_switch_the_active_styles_and_deleting_one_moves_its_highlights() {
    let t = Temp::new("palettes");
    let mut s = launch(&t);
    let themes = run(
        &mut s,
        "highlight_palette.create",
        json!({ "name": "Themes" }),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        s.run("highlight_palette.create", &json!({ "name": "themes" }))
            .is_err()
    );
    // Still Standard until switched; then the new (empty) palette's styles.
    assert_eq!(
        s.active_highlight_palette().unwrap().id,
        STANDARD_PALETTE_ID
    );
    let rev = s.annotations_revision();
    run(
        &mut s,
        "highlight_palette.activate",
        json!({ "id": themes }),
    );
    assert!(s.annotations_revision() > rev, "views reload the palette");
    assert!(s.active_highlight_styles().unwrap().is_empty());
    let err = s
        .run(
            "annotation.highlight",
            &json!({ "resource": "kjv", "start_unit": "ROM", "start": 0, "end": 3 }),
        )
        .unwrap_err();
    assert!(err.to_string().contains("has no styles"), "{err}");
    let grace = run(
        &mut s,
        "highlight_style.create",
        json!({ "name": "Grace", "color": "#2e8b57" }),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(s.highlight_style(&grace).unwrap().palette_id, themes);
    let h = highlight_rom_8_28(&mut s, None);
    assert_eq!(style_of(&s, &h), grace);
    // The active palette survives a restart.
    drop(s);
    let mut s = launch(&t);
    assert_eq!(s.active_highlight_palette().unwrap().id, themes);
    run(
        &mut s,
        "highlight_palette.rename",
        json!({ "id": themes, "name": "Themes of Romans" }),
    );
    // Deleting it moves the Highlight to a style of another palette.
    assert!(
        s.run("highlight_palette.delete", &json!({ "id": themes }))
            .is_err()
    );
    let out = run(
        &mut s,
        "highlight_palette.delete",
        json!({ "id": themes, "replacement": "yellow" }),
    );
    assert_eq!(out["moved"], 1);
    assert_eq!(style_of(&s, &h), "yellow");
    assert_eq!(
        s.active_highlight_palette().unwrap().id,
        STANDARD_PALETTE_ID
    );
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(style_of(&s, &h), grace);
    assert_eq!(
        s.highlight_palette(&themes).unwrap().name,
        "Themes of Romans"
    );
    // The last palette is never deleted.
    run(&mut s, "edit.redo", json!({}));
    let err = s
        .run(
            "highlight_palette.delete",
            &json!({ "id": STANDARD_PALETTE_ID, "replacement": "yellow" }),
        )
        .unwrap_err();
    assert!(err.to_string().contains("last Highlight palette"), "{err}");
}

#[test]
fn a_palette_exports_to_a_small_json_file_and_imports_as_a_new_palette() {
    let t = Temp::new("export");
    let mut s = launch(&t);
    run(
        &mut s,
        "highlight_style.create",
        json!({ "name": "Promises", "look": "box", "color": "#c9a227" }),
    );
    let path = t.0.join("standard.json");
    let out = run(
        &mut s,
        "highlight_palette.export",
        json!({ "id": STANDARD_PALETTE_ID, "path": path.to_string_lossy() }),
    );
    assert_eq!(out["styles"], 6);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.len() < 2048, "{} bytes", text.len());
    let file: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(file["format"], "openbible-highlight-palette");
    assert_eq!(file["name"], "Standard");
    assert_eq!(
        file["styles"][5],
        json!({ "name": "Promises", "look": "box", "color": "#c9a227" })
    );
    assert_eq!(
        file["styles"][0],
        json!({ "name": "Yellow", "look": "background", "color": "#ffec8c",
                "sepia": "#f8e080", "dark": "#463c10" })
    );
    assert!(!text.contains("\"id\""), "no ids: {text}");

    let out = run(
        &mut s,
        "highlight_palette.import",
        json!({ "path": path.to_string_lossy() }),
    );
    let imported = out["id"].as_str().unwrap().to_string();
    assert_eq!(out["palette"]["name"], "Standard (2)");
    let styles = s.palette_styles(&imported).unwrap();
    assert_eq!(styles.len(), 6);
    assert_eq!(styles[5].name, "Promises");
    assert_eq!(
        styles[0].appearance,
        obs_store::built_in_styles()[0].appearance
    );
    assert!(styles.iter().all(|st| st.id != "yellow"), "new ids");
    assert_eq!(
        s.undo_history().undo_label(),
        Some("Import Highlight palette")
    );
    run(&mut s, "edit.undo", json!({}));
    assert!(s.highlight_palette(&imported).is_err());

    for bad in [
        "not json",
        r#"{"format":"other","version":1,"name":"X","styles":[]}"#,
        r#"{"format":"openbible-highlight-palette","version":1,"name":"X","styles":[{"look":"box","color":"gold"}]}"#,
    ] {
        assert!(
            s.run("highlight_palette.import", &json!({ "text": bad }))
                .is_err(),
            "{bad}"
        );
    }
}

/// Exported Notes name a Highlight by its style, escaped: a name is the user's own text.
#[test]
fn notes_exports_name_highlights_by_their_style() {
    let t = Temp::new("notes-export");
    let mut s = launch(&t);
    let style = run(
        &mut s,
        "highlight_style.create",
        json!({ "name": "<b>Promises</b>", "look": "box", "color": "#c9a227" }),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    highlight_rom_8_28(&mut s, Some(&style));
    let html = run(
        &mut s,
        "notes.export",
        json!({ "format": "html", "notebook": "all" }),
    )["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        html.contains("<em>Highlight (&lt;b&gt;Promises&lt;/b&gt;)</em>"),
        "{html}"
    );
}
