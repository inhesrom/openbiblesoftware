//! Page tone follows the Theme (#240), through the shell's real-event headless harness: with
//! Page tone Auto (the default) an open BSB Tab and every other reading surface turn dark on
//! the frame after Theme → Dark and light again after Theme → Light; a light Page tone the
//! user chose is kept, with a one-time hint offering Auto; Settings shows the tone in effect.

use obs_engine::{PageTone, TabId, Theme};

use super::missing_resource::{release_set, settle};
use super::*;
use crate::Palette;

/// Visual evidence for `docs/previews/<name>.png` when `UPDATE_PREVIEWS` is set.
fn preview(h: &Harness, name: &str) {
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
        std::fs::create_dir_all(&dir).unwrap();
        h.screenshot(&dir.join(format!("{name}.ppm")));
    }
}

/// Genesis 1:1-3 of the BSB as its USFM download marks it up (public domain): a heading, a
/// footnote and a cross-reference, imported as the Starter library imports the BSB.
fn bsb_genesis_1() -> obs_model::Resource {
    let usfm = "\\id GEN\n\\c 1\n\\s1 The Creation\n\\p\n\\v 1 In the beginning God created \
                the heavens and the earth.\\x - \\xo 1:1 \\xt Genesis 1:3\\x*\n\\v 2 Now the \
                earth was formless and void, and darkness was over the surface of the deep. \
                And the Spirit of God was hovering over the surface of the waters.\n\\v 3 And \
                God said, “Let there be light,”\\f + \\fr 1:3 \\ft Cited in 2 Corinthians \
                4:6\\f* and there was light.\n";
    let mut import = obs_formats::UsfmImport::new(obs_formats::UsfmOptions {
        resource_id: "bsb".into(),
        title: "Berean Standard Bible (test)".into(),
        abbreviation: "BSB".into(),
        language: "en".into(),
        source_file: "GEN.usfm".into(),
        tool_version: "test".into(),
        ..Default::default()
    });
    import.add_book("GEN.usfm", usfm.into()).unwrap();
    let mut bsb = import.finish().unwrap().0;
    bsb.manifest.license = "LicenseRef-PublicDomain".into();
    bsb
}

/// A launch with the released Starter library's Resources (a Commentary, Cross-references,
/// the TAGNT) and the BSB in the Library, and a User database.
fn harness(name: &str) -> (Harness, super::missing_resource::Lab) {
    let mut set = release_set();
    set.push(bsb_genesis_1());
    let (mut h, lab) = super::missing_resource::setup(&format!("page-tone-{name}"), &set);
    settle(&mut h);
    (h, lab)
}

/// Open the BSB at Genesis 1:1 in a Panel of its own; its Tab.
fn open_bsb(h: &mut Harness) -> TabId {
    let tab = h
        .shell
        .run("resource.open", &json!({ "id": "bsb", "split": true }))
        .unwrap()["tab"]
        .as_u64()
        .unwrap();
    h.shell
        .run("tab.go", &json!({ "tab": tab, "text": "Gen 1:1" }));
    settle(h);
    settle_all(h);
    TabId(tab)
}

fn pane_of(h: &Harness, tab: TabId) -> Rect {
    let tiles = &h.shell.tree.tiles;
    tiles.rect(tiles.find_pane(&tab).unwrap()).unwrap()
}

fn focused_pane(h: &Harness) -> Rect {
    pane_of(h, h.shell.session().workspace.focused().unwrap())
}

/// The fills of last frame's solid rectangles whose middle is in `region`.
fn fills_in(h: &Harness, region: Rect) -> Vec<egui::Color32> {
    (h.rects.iter())
        .filter(|(r, _)| region.contains(r.center()))
        .map(|(_, c)| *c)
        .collect()
}

/// The colours of the glyph quads (textured meshes: Resource text, its markers and
/// interlinear lines) drawn last frame in `region`.
fn inks_in(h: &Harness, region: Rect) -> Vec<egui::Color32> {
    fn walk(shape: &Shape, region: Rect, out: &mut Vec<egui::Color32>) {
        match shape {
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, region, out)),
            Shape::Mesh(m) if m.texture_id != egui::TextureId::default() => out.extend(
                (m.vertices.iter())
                    .filter(|v| region.contains(v.pos))
                    .map(|v| v.color),
            ),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for c in &h.last_shapes {
        walk(&c.shape, c.clip_rect.intersect(region), &mut out);
    }
    out
}

/// What last frame drew in `region` is on `want`'s page in `want`'s ink, and no other tone's
/// page or ink shows there.
fn assert_on_tone(h: &Harness, surface: &str, region: Rect, want: &Palette) {
    let fills = fills_in(h, region);
    let inks = inks_in(h, region);
    let tone = format!("{:?}", want.tone);
    assert!(
        fills.contains(&want.page),
        "{surface}: no {tone} page in {region:?}"
    );
    assert!(inks.contains(&want.ink), "{surface}: no {tone} ink");
    for other in [Palette::PAPER, Palette::SEPIA, Palette::DARK] {
        if other.tone == want.tone {
            continue;
        }
        assert!(
            !fills.contains(&other.page),
            "{surface}: a {:?} page is left in the {tone} tone",
            other.tone
        );
        assert!(
            !inks.contains(&other.ink),
            "{surface}: {:?} ink is left in the {tone} tone",
            other.tone
        );
    }
}

/// Where in the Tab's reading pane its cross-reference note's `↗` marker (#208) is drawn.
fn cross_reference_marker(h: &Harness, tab: TabId) -> Pos2 {
    let view = h.shell.st.tabs[&tab].view.as_ref().unwrap();
    let [ox, oy] = view.text_origin;
    let m = (view.footnote_markers(900.0).into_iter())
        .find(|m| m.kind == obs_model::NoteKind::CrossReference)
        .expect("a cross-reference note's marker on screen");
    let [x0, y0, x1, y1] = m.rect;
    pos2(ox + (x0 + x1) / 2.0, oy + (y0 + y1) / 2.0)
}

/// Acceptance (#240): with Page tone Auto, an open BSB Tab is on the dark page in dark ink on
/// the very frame after Theme → Dark, and on paper again the frame after Theme → Light; the
/// same when Follow system follows the OS.
#[test]
fn an_open_bsb_tab_turns_dark_within_one_frame_of_switching_to_dark_and_back() {
    let (mut h, _lab) = harness("bsb");
    let bsb = open_bsb(&mut h);
    let page = pane_of(&h, bsb);
    assert_eq!(h.shell.session().appearance.page_tone, PageTone::Auto);
    assert_on_tone(&h, "BSB", page, &Palette::PAPER);
    h.shell.run("view.theme", &json!({ "value": "dark" }));
    h.frame();
    assert_on_tone(&h, "BSB after Theme → Dark", page, &Palette::DARK);
    h.shell.run("view.theme", &json!({ "value": "light" }));
    h.frame();
    assert_on_tone(&h, "BSB after Theme → Light", page, &Palette::PAPER);
    // Follow system: the OS turning dark and light again.
    h.shell.run("view.theme", &json!({ "value": "system" }));
    h.frame();
    assert_on_tone(&h, "BSB, Follow system, light OS", page, &Palette::PAPER);
    h.os_theme = egui::Theme::Dark;
    h.frame();
    assert_on_tone(&h, "BSB after the OS turned dark", page, &Palette::DARK);
    h.os_theme = egui::Theme::Light;
    h.frame();
    assert_on_tone(&h, "BSB after the OS turned light", page, &Palette::PAPER);
}

/// A reading surface: its name, how to open it over the default layout, and the region it
/// draws in.
type Surface = (&'static str, fn(&mut Harness) -> Rect);

fn every_reading_surface() -> Vec<Surface> {
    vec![
        ("bible", |h| {
            let bsb = open_bsb(h);
            pane_of(h, bsb)
        }),
        ("footnote-markers", |h| {
            let bsb = open_bsb(h);
            let page = pane_of(h, bsb);
            let palette = h.shell.st.palette;
            assert!(
                inks_in(h, page).contains(&palette.note_marker),
                "the BSB's footnote and cross-reference markers"
            );
            page
        }),
        ("resource", |h| {
            let tab = h
                .shell
                .run("resource.open", &json!({ "id": "mhc", "split": true }))
                .unwrap()["tab"]
                .as_u64()
                .unwrap();
            h.shell
                .run("tab.go", &json!({ "tab": tab, "text": "Jn 3:16" }));
            settle(h);
            settle_all(h);
            pane_of(h, TabId(tab))
        }),
        ("text-comparison", |h| {
            h.shell.run(
                "tool.text_comparison",
                &json!({ "passage": "Gen 1:1-3", "split": true }),
            );
            settle(h);
            settle_all(h);
            focused_pane(h)
        }),
        ("passage-guide", |h| {
            h.shell.run(
                "guide.passage_guide",
                &json!({ "passage": "Jn 3:16", "split": true }),
            );
            settle(h);
            settle_all(h);
            focused_pane(h)
        }),
        ("notes-cards", |h| {
            h.shell
                .run(
                    "annotation.note",
                    &json!({ "passage": "Gen 1:1", "body": "In the beginning" }),
                )
                .unwrap();
            h.shell.run("tool.notes", &json!({ "split": true }));
            settle(h);
            settle_all(h);
            focused_pane(h)
        }),
        ("interlinear", |h| {
            let tab = h
                .shell
                .run(
                    "resource.open",
                    &json!({ "id": "tagnt-grace", "split": true }),
                )
                .unwrap()["tab"]
                .as_u64()
                .unwrap();
            h.shell
                .run("tab.go", &json!({ "tab": tab, "text": "Lk 1:30" }));
            h.shell
                .run("tab.interlinear", &json!({ "tab": tab, "on": true }));
            settle(h);
            settle_all(h);
            pane_of(h, TabId(tab))
        }),
        ("reference-popover", |h| {
            let bsb = open_bsb(h);
            // The cross-reference note's popover (#208), pinned; then its "Genesis 1:3" link.
            h.click(cross_reference_marker(h, bsb));
            let mut link = None;
            for _ in 0..600 {
                h.paced_frame();
                if let Some((_, f)) = h.shell.st.footnotes.shown()
                    && let Some(r) = f.link_rects.first()
                {
                    link = Some(r.center());
                    break;
                }
            }
            let link = link.expect("the cross-reference note's popover shows its link");
            h.events.push(Event::PointerMoved(link));
            for _ in 0..600 {
                h.paced_frame();
                if h.find("Gen 1:3").is_some() && h.meshes.len() > 4 {
                    let label = h.find("Gen 1:3").unwrap();
                    let popover = Rect::from_min_size(label.left_top(), vec2(360.0, 120.0));
                    if fills_in(h, popover).contains(&h.shell.st.palette.page) {
                        return popover;
                    }
                }
            }
            panic!("no reference preview: {:?}", h.texts());
        }),
    ]
}

/// Acceptance (#240): with Page tone Auto, every reading surface (Bible and Resource panes,
/// Text Comparison, Guides, Notes cards, the reference popover, interlinear lines and the
/// footnote markers) is on the Paper page in Light and on the Dark page in Dark, with no
/// trace of the other tone. Snapshots of each in both themes go to `target/previews` when
/// `UPDATE_PREVIEWS` is set.
#[test]
fn every_reading_surface_follows_the_theme_with_page_tone_auto() {
    let (mut h, _lab) = harness("surfaces");
    for (name, open) in every_reading_surface() {
        h.shell.run("view.theme", &json!({ "value": "light" }));
        h.shell.reset_layout();
        settle(&mut h);
        let region = open(&mut h);
        for (theme, palette) in [("light", Palette::PAPER), ("dark", Palette::DARK)] {
            h.shell.run("view.theme", &json!({ "value": theme }));
            h.frame();
            preview(&h, &format!("page-tone-{name}-{theme}-kittest"));
            assert_on_tone(&h, &format!("{name} ({theme})"), region, &palette);
        }
        h.events.push(Event::PointerMoved(pos2(1395.0, 895.0)));
        h.frames(2);
    }
}

/// The hint's question, as it reads for `tone`.
fn hint(tone: &str) -> String {
    format!("Page tone is set to {tone}. Use Auto to follow the theme?")
}

/// Acceptance (#240): Paper chosen explicitly is kept when the theme turns dark, and the hint
/// offering Auto appears that once: not on the next switch, nor after a restart. Its Use Auto
/// button turns the page dark at once.
#[test]
fn an_explicit_paper_choice_is_kept_and_the_hint_appears_once() {
    let (mut h, lab) = harness("hint");
    let bsb = open_bsb(&mut h);
    let page = pane_of(&h, bsb);
    h.shell.run("view.theme", &json!({ "value": "light" }));
    h.shell.run("view.page_tone", &json!({ "value": "paper" }));
    h.frames(2);
    assert!(h.find(&hint("Paper")).is_none(), "no hint in a light theme");
    h.shell.run("view.theme", &json!({ "value": "dark" }));
    h.frames(2);
    assert_eq!(h.ctx.theme(), egui::Theme::Dark);
    assert_eq!(h.shell.session().appearance.page_tone, PageTone::Paper);
    assert_on_tone(&h, "BSB on chosen Paper in Dark", page, &Palette::PAPER);
    assert!(h.find(&hint("Paper")).is_some(), "{:?}", h.texts());
    // Past its fade-in, for the snapshot.
    h.frames(15);
    preview(&h, "page-tone-hint-kittest");
    // Keep Paper: the hint goes, the page stays Paper.
    h.click_text("Keep Paper");
    assert!(h.find(&hint("Paper")).is_none());
    assert_eq!(h.shell.session().appearance.page_tone, PageTone::Paper);
    // Never again: not on the next switch to Dark …
    h.shell.run("view.theme", &json!({ "value": "light" }));
    h.frames(2);
    h.shell.run("view.theme", &json!({ "value": "dark" }));
    h.frames(2);
    assert!(h.find(&hint("Paper")).is_none(), "the hint appears once");
    assert_on_tone(&h, "BSB, Paper kept", page, &Palette::PAPER);
    // … nor after a restart with the same User database.
    drop(h);
    let mut h = Harness::launch(Some(&lab.root.join("user.db")));
    h.frames(3);
    assert_eq!(h.shell.session().appearance.page_tone, PageTone::Paper);
    assert_eq!(h.shell.session().appearance.theme, Theme::Dark);
    h.shell.run("view.theme", &json!({ "value": "light" }));
    h.frames(2);
    h.shell.run("view.theme", &json!({ "value": "dark" }));
    h.frames(2);
    assert!(h.find(&hint("Paper")).is_none(), "once per User database");
}

/// The hint's Use Auto runs View → Page tone → Auto: the pages turn dark with the theme.
#[test]
fn the_page_tone_hint_use_auto_follows_the_theme() {
    let (mut h, _lab) = harness("use-auto");
    let bsb = open_bsb(&mut h);
    let page = pane_of(&h, bsb);
    h.shell.run("view.theme", &json!({ "value": "light" }));
    h.shell.run("view.page_tone", &json!({ "value": "sepia" }));
    h.frames(2);
    h.shell.run("view.theme", &json!({ "value": "dark" }));
    h.frames(2);
    assert_on_tone(&h, "BSB on chosen Sepia in Dark", page, &Palette::SEPIA);
    h.click_text("Use Auto");
    assert!(h.find(&hint("Sepia")).is_none());
    assert_eq!(h.shell.session().appearance.page_tone, PageTone::Auto);
    assert_on_tone(&h, "BSB after Use Auto", page, &Palette::DARK);
}

/// Acceptance (#240): Settings shows the Page tone in effect: what Auto shows in this theme,
/// and that a chosen tone stays in every theme.
#[test]
fn settings_shows_the_page_tone_in_effect() {
    let (mut h, _lab) = harness("settings");
    h.shell.run("view.theme", &json!({ "value": "light" }));
    let tab = h
        .shell
        .run("app.settings", &json!({ "split": true }))
        .unwrap()["tab"]
        .as_u64()
        .unwrap();
    // Its search narrows Settings to Theme and Page tone.
    h.shell.st.tabs.get_mut(&TabId(tab)).unwrap().filter = "page tone".into();
    h.frames(3);
    let auto_light = "Showing Paper pages now: Auto follows the theme.";
    assert!(h.find(auto_light).is_some(), "{:?}", h.texts());
    h.shell.run("view.theme", &json!({ "value": "dark" }));
    h.frame();
    assert!(
        h.find("Showing Dark pages now: Auto follows the theme.")
            .is_some()
    );
    preview(&h, "page-tone-settings-kittest");
    h.shell.run("view.page_tone", &json!({ "value": "sepia" }));
    h.frame();
    assert!(
        h.find("Showing Sepia pages in every theme. Choose Auto to follow the theme.")
            .is_some(),
        "{:?}",
        h.texts()
    );
}
