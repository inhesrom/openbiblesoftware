//! Narrow Panels (#301): a Passage Guide, a Text Comparison, an About page, the Atlas and the
//! Timeline in a side Panel at its default width lay out within the Panel. Nothing they draw
//! runs past its right edge (where it would be cut off), a Tool's summary is shown whole, and
//! Atlas labels keep clear of other places' markers.
//!
//! Each case opens as the QA sweep did, in its window (1280 × 800 points); the Screenshot
//! gallery draws the same cases ([`narrow_panels`]) as regression images.

use obs_engine::testing::bibles;
use obs_engine::{Library, TabId};
use obs_scripture::{Book, Passage, Verse};

use super::a11y_audit::Overlay;
use super::*;

/// The QA sweep's window, in points: a side Panel at its default width is about 420 wide.
const WINDOW: egui::Vec2 = egui::vec2(1280.0, 800.0);

/// The test's library folder, removed when the test ends.
struct Files(std::path::PathBuf);
impl Drop for Files {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A fresh launch in the QA sweep's window, with the KJV John excerpt in an empty library.
fn harness(name: &str) -> (Harness, Files) {
    let root = std::env::temp_dir().join(format!("obs-ui-narrow-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let files = Files(root.clone());
    bibles::write_library(&root.join("library"), &[obs_engine::testing::kjv_john()]);
    let mut h = Harness::new();
    h.screen = WINDOW;
    let s = &mut h.shell.st.session;
    s.library = Library::new();
    assert!(s.library.scan(&root.join("library")).is_empty());
    s.set_alias_cache_dir(root.join("cache"));
    h.shell.reset_layout();
    h.frames(2);
    (h, files)
}

/// Add `resources` to the library, as `.obr` files in its folder (replacing any of the same
/// id).
fn add(h: &mut Harness, resources: &[obs_model::Resource]) {
    let library = &mut h.shell.st.session.library;
    let dir = library.directory().unwrap().to_path_buf();
    bibles::write_library(&dir, resources);
    for r in resources {
        let path = dir.join(format!("{}.obr", r.manifest.resource_id));
        library.add_file(&path).unwrap();
    }
}

/// Run jobs and draw until `done` holds.
fn settle_until(h: &mut Harness, what: &str, done: impl Fn(&Harness) -> bool) {
    for _ in 0..600 {
        if done(h) {
            return;
        }
        h.shell.st.session.jobs().pump(16);
        h.frames(1);
    }
    panic!("never saw {what}: {:?}", h.texts());
}

/// Run `command` (it opens a Tab where it opens by default) and return the Tab.
fn opened(h: &mut Harness, command: &str, params: &serde_json::Value) -> TabId {
    let tab = h.shell.run(command, params).unwrap()["tab"]
        .as_u64()
        .unwrap();
    h.frames(2);
    TabId(tab)
}

fn focused_tab(h: &Harness) -> TabId {
    h.shell.session().workspace.focused().unwrap()
}

fn pane(h: &Harness, tab: TabId) -> Rect {
    let tiles = &h.shell.tree.tiles;
    tiles.rect(tiles.find_pane(&tab).unwrap()).unwrap()
}

/// Whether the focused Tab is in the rightmost Panel, beside another.
fn in_side_panel(h: &Harness) -> bool {
    let panels = h.panels();
    let tab = focused_tab(h);
    panels.len() > 1
        && h.shell.tree.tiles.find_pane(&tab).is_some()
        && pane(h, tab).right() >= panels.last().unwrap().0.right() - 0.5
}

// ---------------------------------------------------------------------------------------
// The cases: what each adds to the library and opens.

const JFB: &str = "Commentary Critical and Explanatory on the Whole Bible";

/// GUIDES-2, SETUP-3: a Passage Guide at John 3:16 whose Commentary row is a long title.
fn passage_guide(h: &mut Harness) {
    add(
        h,
        &[obs_engine::testing::commentary(
            "jfb",
            JFB,
            "JFB",
            &[(
                Passage::verse(Verse::new(Book::John, 3, 16)),
                None,
                "16. For God so loved, &c.--What proclamation of the Gospel has been so oft on \
                 the lips of missionaries and preachers in every age since it was first uttered?",
            )],
        )],
    );
    opened(h, "guide.passage_guide", &json!({ "passage": "John 3:16" }));
    settle_until(h, "the Commentary row", |h| h.find(JFB).is_some());
}

/// The Starter library's TAGNT as its About page shows it: long Publisher, Edition,
/// Attribution and Changes (`starter/sources.toml`, the Changes cut short).
fn tagnt() -> obs_model::Resource {
    let mut r = obs_engine::testing::tagnt_grace();
    let m = &mut r.manifest;
    m.resource_id = "tagnt".into();
    m.title = "Greek New Testament (STEPBible TAGNT)".into();
    m.abbreviation = "TAGNT".into();
    m.license = "CC-BY-4.0".into();
    m.publisher = Some("STEPBible.org, based on work at Tyndale House Cambridge".into());
    m.edition = Some(
        "Translators Amalgamated Greek NT: the words of NA27 in NA28 spelling, with NA verse \
         numbers (STEPBible-Data commit 5de0a47, 2026-11-02)"
            .into(),
    );
    m.source_url = Some("https://STEPBible.github.io/STEPBible-Data/".into());
    m.attribution = "Translators Amalgamated Greek NT (TAGNT), data created by \
                     www.STEPBible.org based on work at Tyndale House Cambridge, licensed CC BY \
                     4.0 (https://creativecommons.org/licenses/by/4.0/). Credit: STEP Bible, \
                     www.STEPBible.org. Source of this data: \
                     https://STEPBible.github.io/STEPBible-Data"
        .into();
    m.changes = Some(
        "Converted from STEPBible's tab-separated TAGNT files to the Open Bible resource \
         format. The text is the words TAGNT marks as in NA27, including its double-bracketed \
         passages; where TAGNT gives the NA27 word only as a variant of another edition's, the \
         NA27 word is used. Each word keeps its dStrong numbers (leading zeros removed) and \
         morphology codes."
            .into(),
    );
    m.origin = obs_model::Origin::Starter;
    r
}

/// LIBRARY-1: TAGNT's About page.
fn about_page(h: &mut Harness) {
    add(h, &[tagnt()]);
    opened(h, "resource.about", &json!({ "id": "tagnt" }));
    h.frames(4);
}

/// Bibles with Matthew 3:13-14, so a Text Comparison has text in each.
fn matthew_bibles() -> Vec<obs_model::Resource> {
    let matthew = |v: u16, text: &'static str| (Verse::new(Book::Matthew, 3, v), text);
    let bible = |id: &str, abbrev: &str, verses: &[(Verse, &str)]| {
        bibles::bible(id, abbrev, verses, None, &[])
    };
    vec![
        bible(
            "asv",
            "ASV",
            &[
                matthew(
                    13,
                    "Then cometh Jesus from Galilee to the Jordan unto John, to be baptized of \
                     him.",
                ),
                matthew(
                    14,
                    "But John would have hindered him, saying, I have need to be baptized of \
                     thee, and comest thou to me?",
                ),
            ],
        ),
        bible(
            "web",
            "WEB",
            &[matthew(
                13,
                "Then Jesus came from Galilee to the Jordan to John, to be baptized by him.",
            )],
        ),
        bible(
            "tagnt-matthew",
            "TAGNT",
            &[matthew(
                13,
                "Τότε παραγίνεται ὁ Ἰησοῦς ἀπὸ τῆς Γαλιλαίας ἐπὶ τὸν Ἰορδάνην πρὸς τὸν \
                 Ἰωάννην τοῦ βαπτισθῆναι ὑπ’ αὐτοῦ.",
            )],
        ),
        bibles::tahot(),
    ]
}

/// GUIDES-15: a Passage Guide at Mark 1:9-11, then "Matt 3:13-17" under Parallel passages,
/// which opens a Text Comparison in a narrow Panel beside the Guide.
fn comparison_from_parallels(h: &mut Harness) {
    let mut resources = matthew_bibles();
    resources.push(obs_engine::testing::bsb_parallels());
    add(h, &resources);
    opened(
        h,
        "guide.passage_guide",
        &json!({ "passage": "Mark 1:9-11" }),
    );
    for section in ["commentaries", "cross_references"] {
        h.shell.run(
            "guide.collapse_section",
            &json!({ "guide": "passage_guide", "section": section, "collapsed": true }),
        );
    }
    settle_until(h, "the parallel passages", |h| {
        h.find("Matt 3:13-17").is_some()
    });
    h.click_text("Matt 3:13-17");
    settle_until(h, "Matthew 3:13 compared", |h| {
        h.find("Matthew 3:13").is_some()
    });
    h.frames(4);
}

/// SETUP-5: the Atlas at Acts 13, with TIPNR's places of Paul's first journey.
fn atlas(h: &mut Harness) {
    let tipnr = obs_engine::testing::tipnr_acts_13();
    let geo = obs_engine::testing::openbible_geo(Some(&tipnr));
    add(h, &[tipnr, geo]);
    opened(h, "tool.atlas", &json!({ "passage": "Acts 13" }));
    settle_until(h, "the places", |h| {
        h.texts().iter().any(|t| t.starts_with("Acts 13 · "))
    });
    h.frames(4);
}

/// SETUP-5: the Timeline at Genesis 12.
fn timeline(h: &mut Harness) {
    let tipnr = obs_engine::testing::tipnr_timeline();
    let events = obs_engine::testing::theographic(Some(&tipnr));
    add(h, &[tipnr, events]);
    opened(h, "tool.timeline", &json!({ "passage": "Gen 12" }));
    settle_until(h, "the summary", |h| {
        h.texts().iter().any(|t| t.starts_with("Genesis 12 · "))
    });
    h.frames(4);
}

/// The cases as the Screenshot gallery draws them: a name, what opens it, and what shows it
/// is open (in a side Panel).
pub(super) fn narrow_panels() -> Vec<Overlay> {
    vec![
        ("Passage Guide in a side Panel", passage_guide, |h| {
            in_side_panel(h) && h.find(JFB).is_some()
        }),
        ("About page in a side Panel", about_page, |h| {
            in_side_panel(h) && h.find("Greek New Testament (STEPBible TAGNT)").is_some()
        }),
        (
            "Text Comparison beside a Passage Guide",
            comparison_from_parallels,
            |h| in_side_panel(h) && h.find("Matthew 3:13").is_some(),
        ),
        ("Atlas in a side Panel", atlas, |h| {
            in_side_panel(h) && h.texts().iter().any(|t| t.starts_with("Acts 13 · "))
        }),
        ("Timeline in a side Panel", timeline, |h| {
            in_side_panel(h) && h.texts().iter().any(|t| t.starts_with("Genesis 12 · "))
        }),
    ]
}

// ---------------------------------------------------------------------------------------
// What the tests look for.

/// What Tab `tab` drew past its Panel's right edge, where it is cut off: each text with how
/// far it runs over, and the Resource-text glyphs as one count.
fn spills(h: &Harness, tab: TabId) -> Vec<String> {
    fn walk(shape: &Shape, right: f32, texts: &mut Vec<String>, glyphs: &mut usize) {
        match shape {
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, right, texts, glyphs)),
            Shape::Text(t) => {
                let r = Rect::from_min_size(t.pos, t.galley.size());
                if r.right() > right + 0.5 {
                    texts.push(format!("{:?} by {:.0}", t.galley.text(), r.right() - right));
                }
            }
            Shape::Mesh(m) if m.texture_id != egui::TextureId::default() => {
                for q in m.vertices.chunks(4) {
                    if q.iter().any(|v| v.pos.x > right + 0.5) {
                        *glyphs += 1;
                    }
                }
            }
            _ => {}
        }
    }
    let panel = pane(h, tab);
    let mut texts = Vec::new();
    let mut glyphs = 0;
    for c in &h.last_shapes {
        // Drawn by this Tab: clipped to (a part of) its Panel.
        if panel.expand(0.5).contains_rect(c.clip_rect) {
            walk(&c.shape, panel.right(), &mut texts, &mut glyphs);
        }
    }
    if glyphs > 0 {
        texts.push(format!("{glyphs} glyphs of Resource text"));
    }
    texts
}

/// The texts Tab `tab` drew cut short with "…".
fn elided(h: &Harness, tab: TabId) -> Vec<String> {
    fn walk(shape: &Shape, out: &mut Vec<String>) {
        match shape {
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            Shape::Text(t) if t.galley.elided => out.push(t.galley.text().to_string()),
            _ => {}
        }
    }
    let panel = pane(h, tab);
    let mut out = Vec::new();
    for c in &h.last_shapes {
        if panel.expand(0.5).contains_rect(c.clip_rect) {
            walk(&c.shape, &mut out);
        }
    }
    out
}

/// Labels on Tab `tab`'s map that cover a marker: a text drawn with the markers (in the same
/// clip) whose halo comes within a circle's outline.
fn labels_over_markers(h: &Harness, tab: TabId) -> Vec<String> {
    type Drawn = (Vec<(Pos2, f32)>, Vec<(Rect, String)>);
    fn walk(shape: &Shape, drawn: &mut Drawn) {
        match shape {
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, drawn)),
            Shape::Circle(c) => drawn.0.push((c.center, c.radius + c.stroke.width / 2.0)),
            Shape::Text(t) => drawn.1.push((
                Rect::from_min_size(t.pos, t.galley.size()),
                t.galley.text().to_string(),
            )),
            _ => {}
        }
    }
    let panel = pane(h, tab);
    let mut by_clip: Vec<(Rect, Drawn)> = Vec::new();
    for c in &h.last_shapes {
        if !panel.expand(0.5).contains_rect(c.clip_rect) {
            continue;
        }
        let i = by_clip
            .iter()
            .position(|(r, _)| *r == c.clip_rect)
            .unwrap_or_else(|| {
                by_clip.push((c.clip_rect, Drawn::default()));
                by_clip.len() - 1
            });
        walk(&c.shape, &mut by_clip[i].1);
    }
    let mut out = Vec::new();
    for (_, (circles, labels)) in &by_clip {
        for (rect, text) in labels {
            // The label's halo.
            let halo = rect.expand(crate::tokens::space::XXS);
            for (centre, reach) in circles {
                if halo.distance_to_pos(*centre) < *reach {
                    out.push(format!("{text:?} covers the marker at {centre:?}"));
                }
            }
        }
    }
    out
}

/// Visual evidence when `UPDATE_PREVIEWS` is set: `target/previews/narrow/<name>-light.ppm`
/// and `-dark.ppm`.
fn preview(h: &mut Harness, name: &str) {
    if std::env::var_os("UPDATE_PREVIEWS").is_none() {
        return;
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews/narrow");
    for theme in ["dark", "light"] {
        h.shell.run("view.theme", &json!({ "value": theme }));
        h.frames(6);
        h.screenshot(&dir.join(format!("{name}-{theme}.ppm")));
    }
}

/// Open `case` in the QA sweep's window and check its Tab keeps within its Panel.
fn check(name: &str, case: fn(&mut Harness)) -> (Harness, Files, TabId) {
    let (mut h, files) = harness(name);
    case(&mut h);
    assert!(in_side_panel(&h), "not in a side Panel: {:?}", h.panels());
    let tab = focused_tab(&h);
    preview(&mut h, name);
    let spilled = spills(&h, tab);
    assert!(
        spilled.is_empty(),
        "drawn past the Panel's edge: {spilled:#?}"
    );
    (h, files, tab)
}

// ---------------------------------------------------------------------------------------
// The findings.

/// GUIDES-2, SETUP-3: the Commentary row's long title wraps (the reference goes under it
/// whole), so the excerpts, the Cross-references header's × and every row under it keep to
/// the Panel.
#[test]
fn a_passage_guide_in_a_side_panel_wraps_its_commentary_rows() {
    let (h, _files, tab) = check("passage-guide", passage_guide);
    let panel = pane(&h, tab);
    let title = h.find(JFB).unwrap();
    let reference = h.find("John 3:16").unwrap();
    assert!(title.right() <= panel.right() && reference.right() <= panel.right());
}

/// LIBRARY-1: Publisher and Edition wrap in their column; the Attribution and Changes wrap
/// at the Panel's width.
#[test]
fn an_about_page_in_a_side_panel_wraps_its_fields_attribution_and_changes() {
    let (mut h, _files, tab) = check("about-page", about_page);
    let panel = pane(&h, tab);
    let edition = h
        .shapes
        .iter()
        .find(|(_, t)| t.starts_with("Translators Amalgamated Greek NT: the words"))
        .unwrap()
        .0;
    assert!(edition.right() <= panel.right(), "{edition:?} {panel:?}");
    // The rest of the page, scrolled to its end: the Changes and the change record's link.
    h.events.push(Event::PointerMoved(panel.center()));
    for _ in 0..30 {
        h.events.push(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, -100.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: Modifiers::NONE,
        });
        h.frame();
    }
    h.frames(6);
    let link = h
        .find("CHANGES-tagnt.md")
        .expect("the change record's link");
    assert!(link.right() <= panel.right(), "{link:?} {panel:?}");
    let spilled = spills(&h, tab);
    assert!(
        spilled.is_empty(),
        "drawn past the Panel's edge: {spilled:#?}"
    );
}

/// GUIDES-15: the Bible chips wrap whole onto the next row, and each Bible's text wraps at
/// the Panel's width.
#[test]
fn a_text_comparison_beside_a_guide_wraps_its_chips_and_each_bibles_text() {
    let (h, _files, tab) = check("text-comparison", comparison_from_parallels);
    let panel = pane(&h, tab);
    for chip in ["ASV", "WEB", "TAHOT"] {
        let r = h
            .shapes
            .iter()
            .find(|(r, t)| t == chip && panel.contains(r.center()))
            .unwrap_or_else(|| panic!("no {chip} chip: {:?}", h.texts()))
            .0;
        assert!(r.right() <= panel.right(), "{chip} {r:?} {panel:?}");
    }
}

/// SETUP-5: the Atlas's summary is shown whole, and no label covers a place's marker (its
/// own or another's); every place is still on the map.
#[test]
fn an_atlas_in_a_side_panel_keeps_labels_clear_of_markers() {
    let (h, _files, tab) = check("atlas", atlas);
    let cut = elided(&h, tab);
    assert!(cut.is_empty(), "{cut:?}");
    let covered = labels_over_markers(&h, tab);
    assert!(covered.is_empty(), "{covered:#?}");
    for place in [
        "Antioch",
        "Pamphylia",
        "Perga",
        "Salamis",
        "Paphos",
        "Seleucia",
    ] {
        assert!(h.find(place).is_some(), "{place}: {:?}", h.texts());
    }
}

/// SETUP-5: the Timeline's summary goes under its controls rather than being cut short.
#[test]
fn a_timeline_in_a_side_panel_shows_its_whole_summary() {
    let (h, _files, tab) = check("timeline", timeline);
    let cut = elided(&h, tab);
    assert!(cut.is_empty(), "{cut:?}");
    let summary = h
        .shapes
        .iter()
        .find(|(_, t)| t.starts_with("Genesis 12 · "))
        .unwrap()
        .0;
    let fit = h.find("Fit").unwrap();
    assert!(summary.top() > fit.bottom(), "{summary:?} under {fit:?}");
}

/// The summary stays beside the controls when the Panel has room for it.
#[test]
fn a_timeline_in_a_wide_panel_keeps_its_summary_beside_the_controls() {
    let (mut h, _files) = harness("timeline-wide");
    h.screen = egui::vec2(WINDOW.x * 2.0, WINDOW.y);
    timeline(&mut h);
    h.frames(6);
    let summary = h
        .shapes
        .iter()
        .find(|(_, t)| t.starts_with("Genesis 12 · "))
        .unwrap()
        .0;
    let fit = h.find("Fit").unwrap();
    assert!(
        (summary.center().y - fit.center().y).abs() < fit.height(),
        "{summary:?} beside {fit:?}"
    );
}
