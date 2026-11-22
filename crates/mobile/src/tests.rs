//! `MobileApp` over `obs_engine::testing` fixtures, jobs inline (CI-run on every OS), and,
//! `#[ignore]`d, over the real Bundled set (`cargo xtask ios bundled-set`, then
//! `cargo test -p obs-mobile -- --ignored`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use obs_engine::JobMode;
use obs_engine::testing::bibles::{bible, bsb_john_1, tahot, v};
use obs_engine::testing::{greek_lexicon, write_starter_bundle};
use obs_reading::TonePalette;
use obs_scripture::{Book, Verse};
use serde_json::json;

use crate::*;

/// A fresh folder for one test.
fn root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("obs-mobile-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

/// A few Verses of Genesis, John 3 and 1 Corinthians 13, in the Standard numbering.
fn web() -> obs_model::Resource {
    bible(
        "web",
        "WEB",
        &[
            (
                v(Book::Genesis, 1, 1),
                "In the beginning, God created the heavens and the earth.",
            ),
            (v(Book::Genesis, 1, 2), "The earth was formless and empty."),
            (
                v(Book::John, 3, 16),
                "For God so loved the world, that he gave his one and only Son.",
            ),
            (
                v(Book::John, 3, 17),
                "For God didn't send his Son into the world to judge the world.",
            ),
            (
                v(Book::FirstCorinthians, 13, 1),
                "If I speak with the languages of men and of angels.",
            ),
            (
                v(Book::FirstCorinthians, 13, 4),
                "Love is patient and is kind.",
            ),
        ],
        None,
        &[],
    )
}

/// A Bundled set of the BSB's Isaiah 40 and John 1, the WEB excerpt, a TAHOT excerpt and a
/// Greek Lexicon, laid out as `cargo xtask ios bundled-set` lays one out.
fn bundle(root: &Path) -> PathBuf {
    let dir = root.join("bundle");
    let mut set = vec![bsb_john_1(), web(), tahot(), greek_lexicon()];
    for r in &mut set {
        r.manifest.origin = obs_model::Origin::Starter;
    }
    write_starter_bundle(&dir, &set);
    dir
}

fn paths(root: &Path) -> Paths {
    Paths::under(&root.join("app"))
}

fn boot(root: &Path) -> MobileApp {
    let bundle = root.join("bundle");
    let app = MobileApp::boot_with(paths(root), Some(&bundle), JobMode::Inline).unwrap();
    assert!(app.problems().is_empty(), "{:?}", app.problems());
    app
}

/// A booted app over a fresh fixture Bundled set.
fn app(name: &str) -> (PathBuf, MobileApp) {
    let root = root(name);
    bundle(&root);
    let app = boot(&root);
    (root, app)
}

const R0: ReaderId = ReaderId(0);
const R1: ReaderId = ReaderId(1);
const PHONE: Viewport = Viewport {
    width: 390.0,
    height: 700.0,
    scale: 3.0,
};
const LIGHT: SystemAppearance = SystemAppearance { dark: false };

fn frame(app: &mut MobileApp, r: ReaderId, offset: f64) -> ReaderFrame {
    app.frame(r, offset, PHONE, LIGHT).unwrap()
}

/// Reader 0 on the BSB at John 1:1.
fn bsb(app: &mut MobileApp) -> ReaderId {
    app.open(R0, "bsb", Some("John 1:1")).unwrap();
    R0
}

#[test]
fn first_launch_installs_the_bundled_set_and_opens_the_first_bible_at_its_start() {
    let (root, app) = app("first");
    let mut ids: Vec<String> = app.library().into_iter().map(|r| r.resource_id).collect();
    ids.sort();
    assert_eq!(ids, ["bsb", "lexicon", "tahot", "web"]);
    for id in ["bsb", "web", "tahot", "lexicon"] {
        assert!(root.join(format!("app/library/{id}.obr")).is_file(), "{id}");
    }
    assert!(root.join("app/user.db").is_file());
    let readers = app.readers();
    assert_eq!(readers.len(), 1);
    let first = &readers[0];
    let bibles: Vec<LibraryResource> = (app.library().into_iter())
        .filter(|r| r.resource_type == ResourceType::Bible)
        .collect();
    assert_eq!(
        first.resource_id, bibles[0].resource_id,
        "the first Bible in Preferred order"
    );
    let doc = &crate::frame::view(&app, R0).doc;
    let start = doc.verses[0].verse;
    assert_eq!(
        first.location,
        obs_scripture::format_passage(
            obs_scripture::Passage::verse(start),
            &doc.versification,
            obs_scripture::RefStyle::Long
        ),
        "at the Bible's first chapter"
    );
    assert!(!first.can_go_back && !first.can_go_forward);
}

#[test]
fn a_relaunch_restores_each_reader_where_it_was() {
    let (root, mut app) = app("relaunch");
    bsb(&mut app);
    app.navigate(R0, "John 1:14").unwrap();
    app.open(R1, "lexicon", Some("G3439")).unwrap();
    app.save_session().unwrap();
    let visits = app.history().iter().map(|d| d.visits.len()).sum::<usize>();
    drop(app);

    let mut again = boot(&root);
    let readers = again.readers();
    assert_eq!(readers.len(), 2);
    assert_eq!(
        (
            readers[0].resource_id.as_str(),
            readers[0].location.as_str()
        ),
        ("bsb", "John 1:14")
    );
    assert_eq!(
        (
            readers[1].resource_id.as_str(),
            readers[1].location.as_str()
        ),
        ("lexicon", "G3439")
    );
    assert!(readers[1].keyed);
    assert_eq!(frame(&mut again, R0, 0.0).location, "John 1:14");
    // A restore is no Visit.
    again.save_session().unwrap();
    let after = again
        .history()
        .iter()
        .map(|d| d.visits.len())
        .sum::<usize>();
    assert_eq!(after, visits);
}

#[test]
fn a_snapshot_of_another_format_opens_the_default_reader() {
    let root = root("other-format");
    bundle(&root);
    drop(boot(&root));
    let (store, _) = obs_store::UserStore::open(&paths(&root).user_db).unwrap();
    obs_engine::UserData::save_last_session(&store, r#"{"tabs":[],"panels":[]}"#).unwrap();
    drop(store);
    let app = boot(&root);
    assert_eq!(app.readers().len(), 1);
}

#[test]
fn jn_3_16_and_1_co_13_parse_in_the_readers_own_numbering() {
    let (_root, mut app) = app("parse");
    app.open(R0, "web", None).unwrap();
    let jn = app.parse_reference(R0, "Jn 3:16").unwrap();
    assert_eq!(
        (jn.label.as_str(), jn.id.as_str()),
        ("John 3:16", "John.3.16")
    );
    let co = app.parse_reference(R0, "1 Co 13").unwrap();
    assert_eq!(
        (co.label.as_str(), co.id.as_str()),
        ("1 Corinthians 13", "1Cor.13.1")
    );
    for bad in ["Hezekiah 4", "", "Rev 22:21"] {
        assert!(
            matches!(
                app.parse_reference(R0, bad),
                Err(MobileError::Reference { .. })
            ),
            "{bad}"
        );
    }
    let at = app.navigate(R0, "1 Co 13").unwrap();
    assert_eq!(at.location, "1 Corinthians 13:1");
    // The Current location is the context of a bare verse.
    assert_eq!(
        app.navigate(R0, "4").unwrap().location,
        "1 Corinthians 13:4"
    );
    let err = app.navigate(R0, "Hezekiah 4").unwrap_err();
    assert!(err.to_string().contains("Hezekiah"), "{err}");
    assert_eq!(
        app.reader(R0).unwrap().location,
        "1 Corinthians 13:4",
        "unmoved"
    );
}

#[test]
fn back_and_forward_step_through_the_readers_jumps() {
    let (_root, mut app) = app("back");
    app.open(R0, "web", None).unwrap();
    app.navigate(R0, "Jn 3:16").unwrap();
    app.navigate(R0, "1 Co 13:4").unwrap();
    let back = app.back(R0).unwrap();
    assert_eq!(back.location, "John 3:16");
    assert!(back.can_go_back && back.can_go_forward);
    assert_eq!(frame(&mut app, R0, 0.0).location, "John 3:16");
    let start = app.back(R0).unwrap();
    assert_eq!(
        (start.resource_id.as_str(), start.location.as_str()),
        ("web", "Genesis 1:1")
    );
    // Before it, the Bible boot opened (the switch to the WEB was a step too).
    let boot = app.back(R0).unwrap();
    assert_ne!(boot.resource_id, "web");
    assert!(!boot.can_go_back);
    assert_eq!(app.back(R0).unwrap(), boot, "nothing before the start");
    assert_eq!(app.forward(R0).unwrap().location, "Genesis 1:1");
    assert_eq!(app.forward(R0).unwrap().location, "John 3:16");
    assert_eq!(app.forward(R0).unwrap().location, "1 Corinthians 13:4");
    assert!(!app.reader(R0).unwrap().can_go_forward);
}

#[test]
fn a_hebrew_resource_starts_right_to_left() {
    let (_root, mut app) = app("hebrew");
    let r = app.open(R0, "tahot", None).unwrap();
    assert!(r.right_to_left);
    assert!(!app.open(R0, "web", None).unwrap().right_to_left);
}

#[test]
fn another_bible_in_a_reader_switches_at_the_same_place_and_back_returns() {
    let (_root, mut app) = app("parallel");
    app.open(R0, "bsb", Some("John 1:1")).unwrap();
    app.navigate(R0, "Jn 1:1").unwrap();
    let web = app.open(R0, "web", None).unwrap();
    assert_eq!(web.resource_id, "web");
    // The WEB excerpt has no John 1: it opens where it can.
    assert!(web.can_go_back, "the switch is a step in its history");
    let before = frame(&mut app, R0, 0.0);
    let back = app.back(R0).unwrap();
    assert_eq!(
        (back.resource_id.as_str(), back.location.as_str()),
        ("bsb", "John 1:1")
    );
    let after = frame(&mut app, R0, 0.0);
    assert_ne!(
        after.tiles[0].key.epoch, before.tiles[0].key.epoch,
        "another document's tiles"
    );
    // Another Resource type starts afresh.
    let lex = app.open(R0, "lexicon", None).unwrap();
    assert!(lex.keyed && !lex.can_go_back);
    assert_eq!(app.readers().len(), 1);
    let types: Vec<String> = (app.parallel_resources(R0).unwrap().into_iter())
        .map(|r| r.resource_id)
        .collect();
    assert_eq!(types, ["lexicon"]);
}

#[test]
fn a_reader_switched_or_replaced_answers_nothing_before_its_next_frame() {
    let (_root, mut app) = app("stale-window");
    let r = bsb(&mut app);
    // Down John 1, where the BSB's blocks are past the WEB excerpt's last.
    app.navigate(r, "John 1:23").unwrap();
    let f = frame(&mut app, r, 0.0);
    let y = f.position + 5.0;
    assert!(!app.accessibility_items(r).unwrap().is_empty());
    app.open(r, "web", None).unwrap();
    assert_eq!(app.hit(r, 40.0, y).unwrap(), None);
    assert_eq!(app.closest_offset(r, 40.0, y).unwrap(), 0);
    assert!(app.accessibility_items(r).unwrap().is_empty());
    assert!(!frame(&mut app, r, 0.0).tiles.is_empty());
    assert!(!app.accessibility_items(r).unwrap().is_empty());
    // A new revision of the Resource a reader shows (a Starter update) is another document.
    app.session
        .workspace
        .push_effect(obs_engine::Effect::Replaced("web".into()));
    app.apply_effects();
    assert_eq!(app.hit(r, 40.0, y).unwrap(), None);
    assert!(app.accessibility_items(r).unwrap().is_empty());
}

#[test]
fn a_lexicon_reader_goes_to_entry_keys() {
    let (_root, mut app) = app("lexicon");
    app.open(R0, "lexicon", None).unwrap();
    assert_eq!(app.reader(R0).unwrap().location, "G25", "its first entry");
    assert_eq!(app.navigate(R0, "g3439").unwrap().location, "G3439");
    assert_eq!(app.parse_reference(R0, "G3588").unwrap().label, "G3588");
    assert!(app.navigate(R0, "G9999").is_err());
    assert_eq!(app.back(R0).unwrap().location, "G25");
    assert!(app.books(R0).unwrap().is_empty());
}

#[test]
fn the_grid_lists_books_chapters_and_verses_and_goes_there() {
    let (_root, mut app) = app("grid");
    app.open(R0, "web", None).unwrap();
    let books = app.books(R0).unwrap();
    let names: Vec<&str> = books.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["Genesis", "John", "1 Corinthians"]);
    assert_eq!(books[2].id, "1Cor");
    assert_eq!(app.chapters(R0, "1Cor").unwrap(), [13]);
    assert_eq!(app.verses(R0, "1Cor", 13).unwrap(), [1, 4]);
    let at = app.go_to_verse(R0, "1Cor", 13, Some(4)).unwrap();
    assert_eq!(at.location, "1 Corinthians 13:4");
    assert_eq!(
        app.go_to_verse(R0, "John", 3, None).unwrap().location,
        "John 3:16"
    );
    assert!(app.go_to_verse(R0, "John", 4, None).is_err());
    assert!(app.chapters(R0, "Nope").is_err());
    assert_eq!(
        app.back(R0).unwrap().location,
        "1 Corinthians 13:4",
        "a jump"
    );
}

#[test]
fn history_records_jumps_and_a_visit_opens_again() {
    let (_root, mut app) = app("history");
    app.open(R0, "web", None).unwrap();
    app.navigate(R0, "Jn 3:16").unwrap();
    app.navigate(R0, "1 Co 13").unwrap();
    let changed = app.pump();
    assert!(changed.contains(&Changed::History), "{changed:?}");
    assert!(!app.pump().contains(&Changed::History), "reported once");
    let days = app.history();
    assert_eq!(days[0].heading, "Today");
    let labels: Vec<&str> = days[0].visits.iter().map(|v| v.label.as_str()).collect();
    assert_eq!(labels[..2], ["1 Corinthians 13:1 · WEB", "John 3:16 · WEB"]);
    let john = days[0].visits[1].id;
    let at = app.open_visit(R0, john).unwrap();
    assert_eq!(
        (at.resource_id.as_str(), at.location.as_str()),
        ("web", "John 3:16")
    );
    assert!(app.open_visit(R0, -1).is_err());
}

#[test]
fn settings_change_through_the_view_commands_and_are_kept() {
    let (root, mut app) = app("settings");
    bsb(&mut app);
    let s = app.settings();
    assert_eq!(s.page_tone.current, "auto");
    assert_eq!(s.page_tone.choices.len(), 4);
    assert_eq!(s.theme.current, "system");
    assert_eq!((s.text_size_min, s.text_size_max), (12.0, 40.0));
    assert_eq!(s.reading_fonts.len(), 3);
    let f = frame(&mut app, R0, 0.0);
    assert_eq!(f.page, TonePalette::PAPER.page.0);
    assert_eq!(
        app.set_page_tone("sepia").unwrap().page_tone.current,
        "sepia"
    );
    assert!(app.set_page_tone("purple").is_err());
    let g = frame(&mut app, R0, f.position);
    assert_eq!(g.page, TonePalette::SEPIA.page.0);
    assert_ne!(g.tiles[0].key.epoch, f.tiles[0].key.epoch, "drawn again");
    assert!(app.set_reduce_motion(true).unwrap().reduce_motion);
    let fonts = app.set_reading_font("greek", Some("Galatia SIL")).unwrap();
    assert_eq!(
        fonts.reading_fonts[1].family.as_deref(),
        Some("Galatia SIL")
    );
    assert!(app.set_reading_font("interface", None).is_err());
    assert_eq!(app.set_text_size(99.0).unwrap().text_size, 40.0);
    assert_eq!(
        frame(&mut app, R0, 0.0).location,
        "John 1:1",
        "a new size keeps the place"
    );
    app.set_theme("dark").unwrap();
    assert_eq!(
        frame(&mut app, R0, 0.0).page,
        TonePalette::SEPIA.page.0,
        "a chosen tone stays"
    );
    app.set_page_tone("auto").unwrap();
    assert_eq!(
        frame(&mut app, R0, 0.0).page,
        TonePalette::DARK.page.0,
        "Auto on a dark Theme"
    );
    drop(app);
    let again = boot(&root);
    let s = again.settings();
    assert_eq!(
        (s.theme.current.as_str(), s.page_tone.current.as_str()),
        ("dark", "auto")
    );
    assert!(s.reduce_motion);
    assert_eq!(s.text_size, 40.0);
}

#[test]
fn dynamic_type_sets_the_text_size_within_its_bounds_and_keeps_the_place() {
    let (_root, mut app) = app("dynamic-type");
    bsb(&mut app);
    app.navigate(R0, "John 1:14").unwrap();
    let f = frame(&mut app, R0, 0.0);
    assert_eq!(
        app.dynamic_type("UICTContentSizeCategoryAccessibilityXXXL"),
        40.0
    );
    assert_eq!(app.dynamic_type("UICTContentSizeCategoryXS"), 15.0);
    assert_eq!(app.dynamic_type("no such category"), 20.0);
    assert_eq!(
        app.dynamic_type("UICTContentSizeCategoryAccessibilityXXXL"),
        40.0
    );
    let g = frame(&mut app, R0, 12345.0);
    assert_eq!(
        g.location, "John 1:14",
        "the shell's offset is ignored after a new size"
    );
    assert_ne!(g.tiles[0].key.epoch, f.tiles[0].key.epoch);
    assert_eq!(app.settings().text_size, 40.0);
}

#[test]
fn a_dark_system_turns_auto_dark_and_starts_a_new_epoch() {
    let (_root, mut app) = app("dark");
    bsb(&mut app);
    let f = frame(&mut app, R0, 0.0);
    let dark = app
        .frame(R0, f.position, PHONE, SystemAppearance { dark: true })
        .unwrap();
    assert_eq!(dark.page, TonePalette::DARK.page.0);
    assert_ne!(dark.tiles[0].key.epoch, f.tiles[0].key.epoch);
    assert_eq!(app.tile(R0, f.tiles[0].key, 3.0).unwrap(), None, "stale");
    app.set_theme("light").unwrap();
    let light = app
        .frame(R0, f.position, PHONE, SystemAppearance { dark: true })
        .unwrap();
    assert_eq!(
        light.page,
        TonePalette::PAPER.page.0,
        "a Light Theme ignores the system"
    );
}

#[test]
fn a_frame_lists_tiles_over_the_view_and_a_viewport_either_side() {
    let (_root, mut app) = app("frame");
    let r = bsb(&mut app);
    let f = frame(&mut app, r, 0.0);
    assert_eq!(f.location, "John 1:1");
    assert!(f.total > f.position + 700.0, "{f:?}");
    let first = f.tiles.first().unwrap();
    let last = f.tiles.last().unwrap();
    assert!(first.y <= f.position, "tiles start at or above the view");
    assert!(last.y + f64::from(last.height) >= f.position + 1400.0);
    assert!(f.tiles.iter().all(|t| t.height > 0.0 && t.height <= BAND));
    for pair in f.tiles.windows(2) {
        assert!(pair[0].y <= pair[1].y, "top first");
    }
    let t = app.tile(r, first.key, 3.0).unwrap().unwrap();
    assert_eq!(t.tile.width, 1170);
    assert_eq!(t.tile.height, (first.height * 3.0).round() as u32);
    assert_eq!(
        t.tile.rgba.len(),
        (t.tile.width * t.tile.height * 4) as usize
    );
    // At the very top, over the excerpt's first heading (Isaiah 40 comes before John 1),
    // the bar names the first Verse below it.
    let (_root, mut top) = self::app("frame-top");
    top.open(R0, "bsb", None).unwrap();
    assert_eq!(frame(&mut top, R0, 0.0).location, "Isaiah 40:1");
}

#[test]
fn scrolling_moves_by_the_offset_and_a_tile_costs_one_block_wherever_it_is() {
    let (_root, mut app) = app("scroll");
    let r = bsb(&mut app);
    let f = frame(&mut app, r, 0.0);
    let before = app.reader(r).unwrap();
    let f = frame(&mut app, r, f.position);
    let g = frame(&mut app, r, f.position + 500.0);
    assert!(
        (g.position - (f.position + 500.0)).abs() < 1.0,
        "{} {}",
        f.position,
        g.position
    );
    assert_ne!(g.location, f.location);
    let after = app.reader(r).unwrap();
    assert_eq!(after.location, g.location, "the Tab follows the scroll");
    assert_eq!(
        after.can_go_back, before.can_go_back,
        "scrolling is no jump"
    );
    app.navigate(r, "John 1:23").unwrap();
    let h = frame(&mut app, r, 0.0);
    assert_eq!(h.location, "John 1:23");
    let key = h.tiles.last().unwrap().key;
    let before = crate::frame::view(&app, r).layouts;
    app.tile(r, key, 3.0).unwrap().unwrap();
    let after = crate::frame::view(&app, r).layouts;
    assert!(after - before <= 1, "{before} → {after}");
}

#[test]
fn tiles_need_a_scale_above_zero_and_at_most_the_pixel_cap() {
    let (_root, mut app) = app("tile-size");
    let r = bsb(&mut app);
    let key = frame(&mut app, r, 0.0).tiles[0].key;
    for scale in [0.0, -3.0, f32::NAN, f32::INFINITY] {
        assert_eq!(app.tile(r, key, scale), Err(MobileError::TileSize));
    }
    assert!(app.tile(r, key, 3.0).unwrap().is_some());
    for bad in [
        Viewport {
            width: 0.0,
            ..PHONE
        },
        Viewport {
            height: -1.0,
            ..PHONE
        },
        Viewport {
            scale: f32::NAN,
            ..PHONE
        },
    ] {
        assert_eq!(app.frame(r, 0.0, bad, LIGHT), Err(MobileError::TileSize));
    }
    // A page wider than MAX_TILE_PX device pixels: its tiles are refused, the app lives.
    let wide = Viewport {
        width: (MAX_TILE_PX as f32 / 3.0) + 10.0,
        ..PHONE
    };
    let f = app.frame(r, 0.0, wide, LIGHT).unwrap();
    assert_eq!(app.tile(r, f.tiles[0].key, 3.0), Err(MobileError::TileSize));
    assert_eq!(
        app.frame(ReaderId(5), 0.0, PHONE, LIGHT),
        Err(MobileError::NoReader(ReaderId(5)))
    );
}

#[test]
fn a_hit_names_the_verse_a_tagged_word_and_a_footnote_marker() {
    let (_root, mut app) = app("hit");
    let r = bsb(&mut app);
    frame(&mut app, r, 0.0);
    let items = app.accessibility_items(r).unwrap();
    let v = items.iter().find(|i| i.id == "John.1.1").unwrap();
    let hit = app
        .hit(r, v.frame.x + 40.0, v.frame.y + 5.0)
        .unwrap()
        .unwrap();
    let verse = hit.verse.unwrap();
    assert_eq!(
        (verse.id.as_str(), verse.title.as_str()),
        ("John.1.1", "John 1:1 (BSB)")
    );
    assert!(
        verse.text.starts_with("In the beginning was the Word"),
        "{}",
        verse.text
    );
    assert_eq!(verse.cross_reference_label, "0 cross-references");
    assert_eq!(hit.word, None, "the BSB has no Tagged words");
    assert_eq!(app.hit(r, 1.0, -5000.0).unwrap(), None);
    // John 1:23 has a Footnote citing Isaiah 40:3: a tap on its marker names it.
    app.navigate(r, "John 1:23").unwrap();
    let f = frame(&mut app, r, 0.0);
    let left = app.readers[0].frame.left;
    let shown = crate::frame::view(&app, r).footnote_markers(PHONE.height);
    let m = shown.first().expect("a Footnote marker in view");
    let (x, y) = ((m.rect[0] + m.rect[2]) / 2.0, (m.rect[1] + m.rect[3]) / 2.0);
    let marker = (app.hit(r, left + x, f.position + f64::from(y)).unwrap())
        .and_then(|h| h.footnote)
        .expect("the marker under its own box");
    assert_eq!(marker.label, m.label);
    assert_eq!(marker.note, m.note.to_string());
    assert!(!marker.label.is_empty());
    let note = app.footnote(r, &marker.note).unwrap();
    assert!(!note.text.is_empty(), "{note:?}");
    assert!(app.footnote(r, "no-such-note").is_err());
    // The Hebrew Bible's words carry Strong's numbers.
    app.open(R1, "tahot", None).unwrap();
    frame(&mut app, R1, 0.0);
    let items = app.accessibility_items(R1).unwrap();
    let v = items.iter().find(|i| !i.heading).unwrap();
    let word = (0..40)
        .filter_map(|k| {
            let x = v.frame.x + v.frame.width - 4.0 * k as f32;
            app.hit(R1, x, v.frame.y + 8.0).unwrap()
        })
        .find_map(|h| h.word)
        .expect("a Tagged word on the first line");
    assert!(word.strongs.starts_with('H'), "{word:?}");
}

#[test]
fn a_note_makes_room_for_its_marker_in_the_text() {
    let (_root, mut app) = app("note-room");
    let r = bsb(&mut app);
    frame(&mut app, r, 0.0);
    let doc = std::sync::Arc::clone(&crate::frame::view(&app, r).doc);
    let i = doc
        .verses
        .iter()
        .position(|v| v.verse == v_john(1))
        .unwrap();
    let first = &doc.verses[i];
    let unit = doc.verse_unit(i).unwrap();
    let block = doc.block_at(unit, first.start as usize).unwrap();
    let unit = &doc.units[unit];
    let before = app.readers[0].view.laid(block).unwrap();
    app.session
        .run(
            "annotation.note",
            &json!({
                "body": "The Word",
                "resource": "bsb",
                "start_unit": unit.id.as_str(),
                "start": first.start,
                "end": first.end,
            }),
        )
        .unwrap();
    let changed = app.pump();
    assert!(changed.contains(&Changed::Reader(r)), "{changed:?}");
    let after = app.readers[0].view.laid(block).unwrap();
    assert!(
        after.text != before.text || after.lines.len() != before.lines.len(),
        "room for the marker"
    );
}

fn v_john(verse: u16) -> Verse {
    Verse::new(Book::John, 1, verse)
}

#[test]
fn words_verses_rects_and_copy_come_from_the_text_under_a_point() {
    let (_root, mut app) = app("select");
    let r = bsb(&mut app);
    frame(&mut app, r, 0.0);
    let items = app.accessibility_items(r).unwrap();
    let v = items.iter().find(|i| i.id == "John.1.1").unwrap();
    let at = app
        .closest_offset(r, v.frame.x + 40.0, v.frame.y + 5.0)
        .unwrap();
    let word = app
        .enclosing_span(r, at, TextUnitKind::Word)
        .unwrap()
        .unwrap();
    let text = app.text_in(r, word).unwrap();
    assert!(!text.trim().is_empty() && !text.contains(' '), "{text:?}");
    let verse = app
        .enclosing_span(r, at, TextUnitKind::Sentence)
        .unwrap()
        .unwrap();
    let verse_text = app.text_in(r, verse).unwrap();
    assert!(
        verse_text.starts_with("In the beginning was the Word"),
        "{verse_text}"
    );
    let rects = app.span_rects(r, verse).unwrap();
    assert!(!rects.is_empty() && rects.iter().all(|r| r.width > 0.0 && r.height > 0.0));
    assert!(app.copy_text(r, verse).unwrap().ends_with("John 1:1 (BSB)"));
    assert!(app.caret_rect(r, word.start).unwrap().height > 0.0);
    let end = app.text_end(r).unwrap();
    assert_eq!(app.move_offset(r, end, 5).unwrap(), end);
    assert_eq!(app.move_offset(r, 0, -5).unwrap(), 0);
    assert_eq!(app.move_offset(r, word.start, 1).unwrap(), word.start + 1);
}

#[test]
fn one_accessibility_element_per_verse_in_reading_order() {
    let (_root, mut app) = app("a11y");
    let r = bsb(&mut app);
    frame(&mut app, r, 0.0);
    let items = app.accessibility_items(r).unwrap();
    let verses: Vec<&AccessibilityItem> = items.iter().filter(|i| !i.heading).collect();
    assert!(verses.len() > 3, "{items:?}");
    let mut ids: Vec<&str> = verses.iter().map(|i| i.id.as_str()).collect();
    let n = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), n, "one element per Verse");
    let one = verses.iter().find(|i| i.id == "John.1.1").unwrap();
    assert!(
        one.label
            .starts_with("John 1:1. In the beginning was the Word"),
        "{}",
        one.label
    );
    for pair in verses.windows(2) {
        let a = Verse::from_osis(&pair[0].id).unwrap();
        let b = Verse::from_osis(&pair[1].id).unwrap();
        assert!(a < b, "reading order: {} then {}", pair[0].id, pair[1].id);
    }
}

#[test]
fn bad_resources_and_readers_are_refused() {
    let (_root, mut app) = app("refused");
    assert_eq!(
        app.open(R0, "nasb", None).unwrap_err(),
        MobileError::NoResource("nasb".into())
    );
    assert_eq!(
        app.open(ReaderId(2), "web", None).unwrap_err(),
        MobileError::NoReader(ReaderId(2))
    );
    app.open(R1, "web", None).unwrap();
    assert_eq!(
        app.open(ReaderId(2), "web", None).unwrap_err(),
        MobileError::NoReader(ReaderId(2)),
        "at most MAX_READERS"
    );
    assert_eq!(app.readers().len(), MAX_READERS);
    assert!(app.reader(ReaderId(7)).is_err());
    assert!(app.navigate(ReaderId(7), "Jn 3:16").is_err());
    assert!(app.back(ReaderId(7)).is_err());
}

#[test]
fn boot_without_a_bundle_or_a_bible_has_no_reader() {
    let root = root("empty");
    let app = MobileApp::boot_with(paths(&root), None, JobMode::Inline).unwrap();
    assert!(app.library().is_empty());
    assert!(app.readers().is_empty());
    assert_eq!(
        MobileApp::engine_version(),
        obs_engine::updates::APP_VERSION
    );
    // A file where a folder must go stops boot, saying which.
    let blocked = root.join("blocked");
    std::fs::create_dir_all(&blocked).unwrap();
    std::fs::write(blocked.join("library"), "").unwrap();
    let err = MobileApp::boot_with(Paths::under(&blocked), None, JobMode::Inline)
        .err()
        .unwrap();
    assert!(err.to_string().contains("library"), "{err}");
}

#[test]
fn the_pump_reports_each_change_once() {
    let (_root, mut app) = app("pump");
    assert!(
        app.pump().iter().all(|c| !matches!(c, Changed::Library)),
        "nothing new"
    );
    app.set_text_size(30.0).unwrap();
    app.session.run("view.text_larger", &json!({})).unwrap();
    app.apply_effects();
    let changed = app.pump();
    assert!(
        changed.contains(&Changed::Message("Text 31 pt".into())),
        "{changed:?}"
    );
    assert!(app.pump().is_empty(), "once");
    assert!(!app.busy());
}

/// The real Bundled set (`apps/ios/Bundled/starter` from `cargo xtask ios bundled-set`), else
/// the whole Starter library.
fn real_bundle() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    ["apps/ios/Bundled/starter", "dist/starter"]
        .into_iter()
        .map(|d| root.join(d))
        .find(|d| d.join("starter-manifest.json").is_file())
}

#[test]
#[ignore = "needs apps/ios/Bundled/starter from `cargo xtask ios bundled-set`"]
fn the_real_bundled_set_boots_navigates_and_relaunches() {
    let bundle = real_bundle().expect("no Bundled set: run `cargo xtask ios bundled-set`");
    let root = root("real");
    let app_paths = paths(&root);
    let mut app = MobileApp::boot(app_paths.clone(), Some(&bundle)).unwrap();
    assert!(app.problems().is_empty(), "{:?}", app.problems());
    let ids: Vec<String> = app.library().into_iter().map(|r| r.resource_id).collect();
    for id in ["bsb", "openbible-xrefs", "strongs-hebrew", "strongs-greek"] {
        assert!(ids.contains(&id.to_string()), "{id} in {ids:?}");
    }
    let first = app.reader(R0).unwrap();
    assert_eq!(
        (first.resource_id.as_str(), first.location.as_str()),
        ("bsb", "Genesis 1:1")
    );
    assert_eq!(
        app.parse_reference(R0, "Jn 3:16").unwrap().label,
        "John 3:16"
    );
    assert_eq!(
        app.parse_reference(R0, "1 Co 13").unwrap().label,
        "1 Corinthians 13"
    );
    assert_eq!(app.navigate(R0, "Jn 3:16").unwrap().location, "John 3:16");
    let f = frame(&mut app, R0, 0.0);
    assert_eq!(f.location, "John 3:16");
    let items = app.accessibility_items(R0).unwrap();
    let v = items.iter().find(|i| i.id == "John.3.16").unwrap();
    let hit = app
        .hit(R0, v.frame.x + 40.0, v.frame.y + 5.0)
        .unwrap()
        .unwrap();
    let verse = hit.verse.unwrap();
    assert!(verse.cross_reference_count > 0, "{verse:?}");
    app.navigate(R0, "1 Co 13").unwrap();
    assert_eq!(app.back(R0).unwrap().location, "John 3:16");
    assert_eq!(app.forward(R0).unwrap().location, "1 Corinthians 13:1");
    assert_eq!(app.books(R0).unwrap().len(), 66);
    let lex = app.open(R1, "strongs-greek", Some("G26")).unwrap();
    assert_eq!(lex.location, "G26");
    app.save_session().unwrap();
    drop(app);
    let again = MobileApp::boot(app_paths, Some(&bundle)).unwrap();
    let readers = again.readers();
    assert_eq!(readers[0].location, "1 Corinthians 13:1");
    assert_eq!(readers[1].location, "G26");
    drop(again);
    std::fs::remove_dir_all(&root).unwrap();
}

/// Cold start on the host: a second launch (the Bundled set already copied) booting and
/// laying out its first frame, and drawing the tiles in view. Before S13, `boot` took 0.73 s
/// on the owner's Mac (release; 1.3–1.4 s in the simulator, where cold start to first text
/// was 1.53–1.95 s against #331's 1.5 s). ADR 0023 is not written yet (S13 ran before S09),
/// so the target comes from that 1.5 s: the simulator took about twice the host's time for
/// `boot`, and process start and the first tiles 0.3–0.6 s more, so a host boot and first
/// frame within 0.4 s, in the test profile (`opt-level = 1`) and under load, leaves the
/// simulator inside 1.5 s. Measured at S13: 0.22 s here, 0.15 s in release.
const COLD_START_TARGET: std::time::Duration = std::time::Duration::from_millis(400);

#[test]
#[ignore = "needs apps/ios/Bundled/starter from `cargo xtask ios bundled-set`"]
fn a_second_launch_boots_and_draws_its_first_frame_within_the_target() {
    use std::time::{Duration, Instant};
    let bundle = real_bundle().expect("no Bundled set: run `cargo xtask ios bundled-set`");
    let root = root("cold-start");
    let started = Instant::now();
    drop(MobileApp::boot(paths(&root), Some(&bundle)).unwrap());
    let first = started.elapsed();
    let mut runs: Vec<(Duration, Duration, Duration)> = Vec::new();
    for _ in 0..3 {
        let started = Instant::now();
        let mut app = MobileApp::boot(paths(&root), Some(&bundle)).unwrap();
        let booted = started.elapsed();
        let f = frame(&mut app, R0, 0.0);
        let framed = started.elapsed();
        for t in f
            .tiles
            .iter()
            .filter(|t| t.y < f.position + f64::from(PHONE.height))
        {
            app.tile(R0, t.key, PHONE.scale).unwrap();
        }
        runs.push((booted, framed, started.elapsed()));
    }
    runs.sort();
    let (boot, frame_at, text_at) = runs[runs.len() / 2];
    eprintln!(
        "first launch (copying the Bundled set) {first:?}; second launch, median of 3: boot \
         {boot:?}, first frame at {frame_at:?}, tiles in view at {text_at:?} (target \
         {COLD_START_TARGET:?})"
    );
    assert!(
        frame_at <= COLD_START_TARGET,
        "{frame_at:?} > {COLD_START_TARGET:?}"
    );
    std::fs::remove_dir_all(&root).unwrap();
}
