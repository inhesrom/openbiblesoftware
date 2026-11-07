//! View → Fonts (#138) through the shell's headless harness: reading fonts per script, the
//! interface font, coverage warnings, restarts and fonts missing on another computer.

use std::collections::{BTreeMap, BTreeSet};

use obs_fonts::{FaceId, FaceSet, ReadingScript};

use super::*;
use crate::{FontFile, InstalledFamily, SystemFonts};

/// Installed fonts for tests, made from the bundled files: "Test Serif" (no Hebrew letters),
/// "Test Hebrew" and "Test Bitmap" (no outlines).
struct TestFonts;

/// Gentium Plus Regular with its `glyf` and `loca` tables renamed away: the cmap still maps
/// every letter, but no glyph has an outline, as in a bitmap-only font.
fn without_outlines(bytes: &[u8]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let tables = usize::from(u16::from_be_bytes([out[4], out[5]]));
    for i in 0..tables {
        let tag = 12 + i * 16;
        // Renamed in place, keeping the table records sorted by tag.
        match &out[tag..tag + 4] {
            b"glyf" => out[tag + 3] = b'F',
            b"loca" => out[tag + 3] = b'A',
            _ => {}
        }
    }
    out
}

impl SystemFonts for TestFonts {
    fn families(&self) -> Vec<InstalledFamily> {
        vec![
            InstalledFamily {
                name: "Test Bitmap".into(),
                styles: vec!["Regular".into()],
            },
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
        if family == "Test Bitmap" {
            return Some(FontFile {
                data: Arc::new(without_outlines(FaceId::Regular.bundled_bytes()?)),
                index: 0,
                key: "Test Bitmap/bitmap.ttf".into(),
            });
        }
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

/// Glyphs painted for the text in view, and whether each has ink in the atlas.
fn painted(h: &Harness) -> (usize, bool) {
    let mut atlas = obs_render::GlyphAtlas::new();
    let (mut quads, mut inked) = (0, true);
    for t in h.shell.st.tabs.values() {
        let Some(v) = &t.view else { continue };
        for (_, _, block) in v.laid_in_view(2000.0) {
            for q in atlas.quads(&block, [0.0, 0.0], 1.0) {
                let side = atlas.side() as f32;
                let (x0, y0) = ((q.uv_min[0] * side) as usize, (q.uv_min[1] * side) as usize);
                let (x1, y1) = ((q.uv_max[0] * side) as usize, (q.uv_max[1] * side) as usize);
                let ink: u32 = (y0..y1)
                    .flat_map(|y| (x0..x1).map(move |x| (x, y)))
                    .map(|(x, y)| u32::from(atlas.coverage()[y * atlas.side() + x]))
                    .sum();
                quads += 1;
                inked &= ink > 0;
            }
        }
    }
    (quads, inked)
}

/// A bitmap-only font maps letters in its cmap but has no outlines to paint: it is refused,
/// and the text is painted in the bundled font instead of disappearing.
#[test]
fn a_font_without_outlines_is_refused_and_text_still_paints() {
    let mut h = with_fonts(None);
    open_bibles(&mut h);
    let before = painted(&h);
    assert!(before.0 > 100 && before.1, "{before:?}");
    h.shell.run(
        "view.font",
        &json!({ "slot": "latin", "family": "Test Bitmap" }),
    );
    h.frame();
    assert_eq!(h.shell.st.session.text.fonts, Default::default());
    let (msg, _) = h.shell.st.toast.clone().unwrap();
    assert_eq!(
        msg,
        "The font “Test Bitmap Regular” cannot be drawn (no outline glyphs, or not a readable \
         font); Gentium Plus is used for Latin text."
    );
    settle_all(&mut h);
    assert_eq!(painted(&h), before, "every glyph still painted, with ink");
    h.shell.run("view.fonts", &json!({}));
    h.frames(2);
    assert!(
        h.find("Cannot be drawn (no outline glyphs); Gentium Plus is used.")
            .is_some(),
        "{:?}",
        h.texts()
    );

    // The interface font is refused the same way: egui's fonts stay as they were.
    h.shell.run(
        "view.font",
        &json!({ "slot": "interface", "family": "Test Bitmap" }),
    );
    h.frames(2);
    h.ctx.fonts(|fonts| {
        let proportional = &fonts.definitions().families[&egui::FontFamily::Proportional];
        assert!(!proportional.iter().any(|f| f == "interface"));
    });
}

/// The Topic Guide's subtopic row heights (#183), measured in the old interface font, are
/// forgotten when the font changes, even when the rows keep their width: every row, in view
/// or not, is measured again in the new face, so the Guide's scroll height never mixes the two.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn an_interface_font_change_remeasures_topic_rows_out_of_view() {
    use crate::bench::library;
    let mut h = with_fonts(None);
    let dir = std::env::temp_dir().join(format!("obs-fonts-topic-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let nave = library::topical_index("nave").unwrap();
    let path = dir.join("nave.obr");
    obs_store::write_obr(&nave, &path).unwrap();
    h.shell.st.session.library.add_file(&path).unwrap();
    h.shell.reset_layout();
    let font = |h: &mut Harness, style: &str| {
        h.shell.run(
            "view.font",
            &json!({ "slot": "interface", "family": "Test Serif", "style": style }),
        );
    };
    font(&mut h, "Regular");
    let (topic, subtopics, _) = library::LONG_TOPICS[0];
    let tab = obs_engine::TabId(
        h.shell
            .run(
                "guide.topic_guide",
                &json!({ "topic": topic, "split": true }),
            )
            .unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    h.frames(4);
    h.shell
        .st
        .tabs
        .get_mut(&tab)
        .unwrap()
        .guide_expanded
        .insert(obs_engine::GuideSection::TopicalEntries);
    let over = {
        let tiles = &h.shell.tree.tiles;
        tiles.rect(tiles.find_pane(&tab).unwrap()).unwrap().center()
    };
    let rows = |h: &Harness| h.shell.st.tabs[&tab].topic_rows.clone();
    // Wheel through the topic, every subtopic row drawn (and measured) on the way.
    let scroll = |h: &mut Harness, dy: f32| {
        for _ in 0..400 {
            h.events.push(Event::PointerMoved(over));
            h.events.push(Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, dy),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Wheel,
                modifiers: Modifiers::NONE,
            });
            h.frame();
        }
        h.frames(10);
    };
    scroll(&mut h, -200.0);
    let regular = rows(&h);
    assert_eq!(regular.len(), subtopics, "every row measured");
    scroll(&mut h, 200.0);
    assert_eq!(rows(&h), regular, "out of view, rows keep their heights");

    // Back at the top, most rows are out of view. The italic wraps some rows differently at
    // the same width.
    font(&mut h, "Italic");
    h.frame();
    let italic = rows(&h);
    // Measuring every row afresh in the new face gives the same heights: none is stale.
    h.shell.st.tabs.get_mut(&tab).unwrap().topic_rows.clear();
    h.frame();
    assert_eq!(rows(&h), italic);
    assert!(
        regular
            .iter()
            .any(|(id, (w, ht))| italic[id].0 == *w && italic[id].1 != *ht),
        "the italic wraps some row differently at the same width"
    );
    drop(h);
    let _ = std::fs::remove_dir_all(dir);
}
