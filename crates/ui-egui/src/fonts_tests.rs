//! View → Fonts (#138) through the shell's headless harness: reading fonts per script, the
//! interface font, coverage warnings, restarts and fonts missing on another computer.

use std::collections::{BTreeMap, BTreeSet};

use obs_fonts::{FaceId, FaceSet, ReadingScript};

use super::*;
use crate::{FontFile, InstalledFamily, SystemFonts};

/// Installed fonts for tests, made from the bundled files: "Test Serif" (no Hebrew letters)
/// and "Test Hebrew".
struct TestFonts;

impl SystemFonts for TestFonts {
    fn families(&self) -> Vec<InstalledFamily> {
        vec![
            InstalledFamily {
                name: "Test Hebrew".into(),
                styles: vec!["Regular".into()],
            },
            InstalledFamily {
                name: "Test Serif".into(),
                styles: vec!["Regular".into(), "Italic".into()],
            },
        ]
    }

    fn load(&self, family: &str, style: &str, _bold: bool, italic: bool) -> Option<FontFile> {
        let file = match family {
            "Test Serif" if italic || style == "Italic" => FaceId::BoldItalic,
            "Test Serif" => FaceId::Bold,
            "Test Hebrew" => FaceId::Hebrew,
            _ => return None,
        };
        Some(FontFile {
            data: Arc::new(file.bundled_bytes()?.to_vec()),
            index: 0,
            key: format!("{family}/{}", file.file()),
        })
    }
}

fn with_fonts(db: Option<&std::path::Path>) -> Harness {
    Harness::launch_with_fonts(db, JobMode::Inline, None, Some(Arc::new(TestFonts)))
}

/// The faces each open reading view has laid out in view, by Resource id.
fn faces_in_view(h: &Harness) -> BTreeMap<String, BTreeSet<FaceId>> {
    let mut out: BTreeMap<String, BTreeSet<FaceId>> = BTreeMap::new();
    for t in h.shell.st.tabs.values() {
        if let Some(v) = &t.view {
            let faces = out.entry(v.doc.manifest.resource_id.clone()).or_default();
            for (_, _, block) in v.laid_in_view(2000.0) {
                for item in block.lines.iter().flat_map(|l| &l.items) {
                    faces.insert(item.style.face);
                }
            }
        }
    }
    out
}

/// Hebrew-text items (right-to-left body text at Hebrew size, not the chapter number) in view in `id`'s views: their faces
/// and any `.notdef` glyphs.
fn hebrew_items(h: &Harness, id: &str) -> (BTreeSet<FaceId>, usize) {
    let mut faces = BTreeSet::new();
    let mut missing = 0;
    for t in h.shell.st.tabs.values() {
        let Some(v) = t.view.as_ref().filter(|v| v.doc.manifest.resource_id == id) else {
            continue;
        };
        let hebrew_size = v.settings().size * v.settings().hebrew_scale;
        for (_, _, block) in v.laid_in_view(2000.0) {
            for item in block.lines.iter().flat_map(|l| &l.items) {
                let text = item.style.role == obs_layout::ColorRole::Ink;
                if text && item.rtl && (item.style.size - hebrew_size).abs() < 0.01 {
                    faces.insert(item.style.face);
                    missing += item.shaped.missing_glyphs();
                }
            }
        }
    }
    (faces, missing)
}

fn open_bibles(h: &mut Harness) {
    for id in ["heb-test", "heb-num"] {
        h.shell
            .run("resource.open", &json!({ "id": id, "split": true }));
    }
    settle_all(h);
}

fn user_faces(set: &BTreeSet<FaceId>) -> Vec<FaceId> {
    set.iter().copied().filter(|f| !f.is_bundled()).collect()
}

#[test]
fn a_system_serif_for_latin_changes_every_bible_pane_at_once() {
    let mut h = with_fonts(None);
    open_bibles(&mut h);
    let before = faces_in_view(&h);
    assert_eq!(before.len(), 3, "{before:?}");
    assert!(before.values().all(|f| user_faces(f).is_empty()));
    // A selection and each pane's location, which a font change must keep.
    let selection = crate::Selection {
        anchor: crate::DocPos { unit: 0, byte: 4 },
        head: crate::DocPos { unit: 0, byte: 40 },
        granularity: Default::default(),
    };
    let mut locations = BTreeMap::new();
    for (id, t) in h.shell.st.tabs.iter_mut() {
        if let Some(v) = &mut t.view {
            v.selection = Some(selection);
            locations.insert(*id, v.current_location());
        }
    }

    h.shell.run(
        "view.font",
        &json!({ "slot": "latin", "family": "Test Serif", "style": "Regular" }),
    );
    h.frame();
    // One frame later every pane has the new Text settings.
    let fonts = h.shell.st.session.text.fonts;
    let serif = fonts.latin.regular;
    assert!(!serif.is_bundled());
    assert_eq!(fonts.latin.italic, fonts.latin.bold_italic);
    assert_ne!(
        fonts.latin.italic, serif,
        "the family's italic is its own face"
    );
    assert_eq!(fonts.greek, FaceSet::bundled(ReadingScript::Greek));
    assert_eq!(fonts.hebrew, FaceSet::bundled(ReadingScript::Hebrew));
    for t in h.shell.st.tabs.values() {
        if let Some(v) = &t.view {
            assert_eq!(v.settings().fonts, fonts, "{}", v.doc.manifest.resource_id);
        }
    }
    settle_all(&mut h);
    let after = faces_in_view(&h);
    for (id, faces) in &after {
        assert!(faces.contains(&serif), "{id}: {faces:?}");
        assert!(
            !faces.contains(&FaceId::Regular),
            "{id}: Latin text left in Gentium"
        );
    }
    for (id, t) in h.shell.st.tabs.iter_mut() {
        if let Some(v) = &mut t.view {
            assert_eq!(v.selection, Some(selection), "selection kept");
            assert_eq!(v.current_location(), locations[id], "location kept");
        }
    }
    // Hebrew is unaffected: still Ezra SIL, every glyph present.
    assert_eq!(
        hebrew_items(&h, "heb-test"),
        (BTreeSet::from([FaceId::Hebrew]), 0)
    );

    // Back to the bundled font.
    h.shell
        .run("view.font", &json!({ "slot": "latin", "family": "" }));
    settle_all(&mut h);
    assert_eq!(h.shell.st.session.text.fonts, Default::default());
    assert!(faces_in_view(&h).values().all(|f| user_faces(f).is_empty()));
}

#[test]
fn a_font_without_hebrew_is_flagged_and_hebrew_still_renders() {
    let mut h = with_fonts(None);
    open_bibles(&mut h);
    h.shell.run("view.fonts", &json!({}));
    h.frames(2);
    assert!(h.find("Fonts").is_some(), "{:?}", h.texts());
    assert!(h.find("Test Serif").is_none(), "nothing chosen yet");
    h.shell.run(
        "view.font",
        &json!({ "slot": "hebrew", "family": "Test Serif" }),
    );
    h.frames(3);
    assert!(
        h.find("This font has no Hebrew letters; Ezra SIL will be used.")
            .is_some(),
        "{:?}",
        h.texts()
    );
    settle_all(&mut h);
    assert!(!h.shell.st.session.text.fonts.hebrew.regular.is_bundled());
    assert_eq!(
        hebrew_items(&h, "heb-test"),
        (BTreeSet::from([FaceId::Hebrew]), 0),
        "Hebrew falls back to Ezra SIL"
    );
    // A Hebrew font is not flagged.
    h.shell.run(
        "view.font",
        &json!({ "slot": "hebrew", "family": "Test Hebrew" }),
    );
    h.frames(3);
    assert!(
        h.find("This font has no Hebrew letters; Ezra SIL will be used.")
            .is_none()
    );
    settle_all(&mut h);
    let hebrew = h.shell.st.session.text.fonts.hebrew.regular;
    assert_eq!(hebrew_items(&h, "heb-test"), (BTreeSet::from([hebrew]), 0));
}

#[test]
fn the_choice_survives_a_restart_and_a_missing_font_falls_back_once() {
    let root = std::env::temp_dir().join(format!("obs-fonts-ui-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let db = root.join("user.db");
    let mut h = with_fonts(Some(&db));
    h.shell.run(
        "view.font",
        &json!({ "slot": "latin", "family": "Test Serif", "style": "Italic" }),
    );
    h.frame();
    drop(h);

    // Restart on this computer: the font is loaded again.
    let h = with_fonts(Some(&db));
    let latin = h.shell.st.session.text.fonts.latin;
    assert!(!latin.regular.is_bundled());
    assert_eq!(latin.regular, latin.italic, "Italic chosen as the style");
    assert!(h.shell.st.toast.is_none());
    drop(h);

    // Another computer without the font: the bundled font, with a notice once.
    let mut h = Harness::launch(Some(&db));
    assert_eq!(h.shell.st.session.text.fonts, Default::default());
    let (msg, _) = h.shell.st.toast.clone().unwrap();
    assert_eq!(
        msg,
        "The font “Test Serif Italic” is not installed on this computer; Gentium Plus is used for Latin text."
    );
    h.shell.run("view.fonts", &json!({}));
    h.frames(2);
    assert!(
        h.find("Not installed on this computer; Gentium Plus is used.")
            .is_some()
    );
    drop(h);
    let h = Harness::launch(Some(&db));
    assert!(h.shell.st.toast.is_none(), "told once");
    assert_eq!(
        h.shell
            .st
            .session
            .fonts
            .get(obs_engine::FontSlot::Latin)
            .unwrap()
            .family,
        "Test Serif",
        "the choice is kept for computers that have the font"
    );
    drop(h);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_interface_font_goes_first_and_keeps_the_symbol_fallbacks() {
    let mut h = with_fonts(None);
    h.shell.run(
        "view.font",
        &json!({ "slot": "interface", "family": "Test Serif" }),
    );
    h.frames(2);
    h.ctx.fonts_mut(|fonts| {
        let families = fonts.definitions().families.clone();
        let proportional = &families[&egui::FontFamily::Proportional];
        assert_eq!(proportional[0], "interface");
        assert!(proportional.len() > 1, "egui's fonts stay behind it");
        assert!(fonts.has_glyph(&egui::FontId::proportional(14.0), '★'));
        assert!(fonts.has_glyph(&egui::FontId::proportional(14.0), 'A'));
        // Headings keep Gentium Plus Bold.
        let display = &families[&egui::FontFamily::Name("display".into())];
        assert_eq!(display[0], "gentium-bold");
    });
    assert_eq!(
        h.shell.st.session.text.fonts,
        Default::default(),
        "reading fonts are separate"
    );
    h.shell
        .run("view.font", &json!({ "slot": "interface", "family": "" }));
    h.frames(2);
    h.ctx.fonts(|fonts| {
        let proportional = &fonts.definitions().families[&egui::FontFamily::Proportional];
        assert!(!proportional.iter().any(|f| f == "interface"));
    });
}
