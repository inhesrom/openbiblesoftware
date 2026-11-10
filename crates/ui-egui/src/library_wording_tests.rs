//! Library, Get Resources, Starter library, About and Import report wording and layout (#307),
//! through the shell's real-event headless harness with the Screenshot gallery's fixtures:
//! Get Resources' sections start a section gap below what is above them (LIBRARY-5); the
//! Starter library says it is missing only when none of it is in the Library (LIBRARY-6) and
//! its "what it powers" line is a hint, not a link's colour (LIBRARY-7); licences read in
//! words (LIBRARY-8) and languages by name (LIBRARY-9); an Import report counts in the words
//! of what was imported (LIBRARY-10); About's Source is a link as in Credits (LIBRARY-12); a
//! failed Import says what went wrong in plain words with a way on (LIBRARY-13); and list rows
//! keep their Buttons clear of the scroll bar (LIBRARY-14). [`library_wording`] are the
//! gallery's regression images of the same.

use std::path::Path;

use egui::Shape;
use obs_engine::get_bibles::ResourceFilter;

use super::a11y_audit::Overlay;
use super::gallery::{LOOKS, launch, run_split};
use super::*;
use crate::tokens::{size, space};

/// One piece of text painted last frame, with its colour.
#[derive(Debug, Clone)]
struct Painted {
    rect: Rect,
    text: String,
    color: egui::Color32,
}

fn walk(shape: &Shape, out: &mut Vec<Painted>) {
    match shape {
        Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
        Shape::Text(t) => out.push(Painted {
            rect: Rect::from_min_size(t.pos, t.galley.size()),
            text: t.galley.text().to_string(),
            color: t
                .galley
                .job
                .sections
                .first()
                .map_or(t.fallback_color, |s| s.format.color),
        }),
        _ => {}
    }
}

fn painted(h: &Harness) -> Vec<Painted> {
    let mut out = Vec::new();
    for c in &h.last_shapes {
        walk(&c.shape, &mut out);
    }
    out
}

/// The Panel showing the Tab titled `title`.
fn panel_of(h: &Harness, title: &str) -> Rect {
    h.panels()
        .into_iter()
        .find(|(_, t)| t.iter().any(|t| t == title))
        .map(|(r, _)| r)
        .unwrap_or_else(|| panic!("no {title:?} Panel: {:?}", h.panels()))
}

fn texts_in(h: &Harness, panel: Rect) -> Vec<&str> {
    h.shapes
        .iter()
        .filter(|(r, _)| panel.contains(r.center()))
        .map(|(_, t)| t.as_str())
        .collect()
}

fn dark() -> &'static super::gallery::Look {
    &LOOKS[1]
}

const GEN_USFM: &str = "\\id GEN\n\\c 1\n\\p\n\\v 1 In the beginning.\\f + \\ft A note.\\f*\n";

/// Import a USFM Genesis (no language, no licence) and run the Import to its end.
fn import_genesis(h: &mut Harness) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/gallery/wording");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("GEN.usfm"), GEN_USFM).unwrap();
    // Tests run in the crate's folder: the same path on every computer.
    h.shell.run(
        "import.start",
        &json!({ "path": "../../target/gallery/wording/GEN.usfm" }),
    );
    super::missing_resource::settle(h);
    settle_all(h);
}

fn import_missing_file(h: &mut Harness) {
    h.shell
        .run("import.start", &json!({ "path": "/nonexistent/foo.usfm" }));
    super::missing_resource::settle(h);
    settle_all(h);
}

/// Get Resources with the Lexicons filter, as in LIBRARY-5's re-check.
fn get_resources_lexicons(h: &mut Harness) {
    run_split(h, "library.get_resources", json!({}));
    h.shell
        .st
        .session
        .get_bibles
        .set_filter(ResourceFilter::Type(obs_model::ResourceType::Lexicon));
    h.frames(4);
}

/// Get Resources with every Resource listed, longer than its Panel, the pointer over the list
/// so its scroll bar shows.
fn get_resources_all(h: &mut Harness) {
    run_split(h, "library.get_resources", json!({}));
    hover_list(h, "Get Resources");
}

fn starter_library(h: &mut Harness) {
    run_split(h, "library.starter", json!({}));
    hover_list(h, "Starter library");
}

/// The pointer over a Panel's list at its right edge, where the scroll bar shows (and widens
/// under the pointer).
fn hover_list(h: &mut Harness, title: &str) {
    let panel = panel_of(h, title);
    h.events.push(Event::PointerMoved(pos2(
        panel.right() - space::XL - space::XS,
        panel.center().y + panel.height() / 4.0,
    )));
    h.frames(10);
}

fn about_with_source(h: &mut Harness) {
    let lib = &mut h.shell.st.session.library;
    let mut kjv = lib.get("kjv-strongs").unwrap().clone();
    kjv.manifest.source_url = Some("https://example.org/kjv".into());
    lib.add(kjv);
    run_split(h, "resource.about", json!({ "id": "kjv-strongs" }));
}

fn library_after_import(h: &mut Harness) {
    import_genesis(h);
    let at = h.tab_at("Library");
    h.click(at);
    h.frames(4);
}

/// The gallery's regression images of #307's findings (the Starter library, Get Resources,
/// About, Credits and Import report Tabs are in the Tabs section already).
pub(super) fn library_wording() -> Vec<Overlay> {
    vec![
        (
            "Get Resources: Lexicons, section gaps",
            get_resources_lexicons,
            |h| h.texts().iter().any(|t| t.starts_with("ALL LEXICONS")),
        ),
        (
            "Get Resources: rows beside the scroll bar",
            get_resources_all,
            |h| h.texts().iter().any(|t| t.starts_with("FEATURED BIBLES")),
        ),
        (
            "Starter library: rows beside the scroll bar",
            starter_library,
            |h| h.find("IN YOUR LIBRARY · 11").is_some(),
        ),
        (
            "Library: an Import with no language or licence",
            library_after_import,
            |h| h.find("Unknown language").is_some(),
        ),
        (
            "About: licence in words and Source link",
            about_with_source,
            |h| h.find("https://example.org/kjv").is_some(),
        ),
        (
            "Import report: the file does not exist",
            import_missing_file,
            |h| h.find("Import failed").is_some(),
        ),
    ]
}

/// The first text in `panel` starting with `start`.
fn find_start(h: &Harness, panel: Rect, start: &str) -> Rect {
    h.shapes
        .iter()
        .find(|(r, t)| t.starts_with(start) && panel.contains(r.center()))
        .map(|(r, _)| *r)
        .unwrap_or_else(|| panic!("{start:?} not in {panel:?}: {:?}", texts_in(h, panel)))
}

/// LIBRARY-5: about 75 points of nothing sat above every section's header.
#[test]
fn get_resources_sections_start_a_section_gap_below_what_is_above_them() {
    let (mut h, _lab) = launch("wording-sections", dark());
    get_resources_lexicons(&mut h);
    let panel = panel_of(&h, "Get Resources");
    let refresh = find_start(&h, panel, "Refresh");
    let featured = find_start(&h, panel, "FEATURED LEXICONS");
    let gap = featured.top() - refresh.bottom();
    assert!(
        gap <= space::XXL + space::L,
        "{gap} points from the search row to the first section"
    );
    // A later section: from the last text of the section above to its header.
    let all = find_start(&h, panel, "ALL LEXICONS");
    let above = h
        .shapes
        .iter()
        .filter(|(r, _)| panel.contains(r.center()) && r.bottom() <= all.top())
        .map(|(r, _)| r.bottom())
        .fold(f32::MIN, f32::max);
    let gap = all.top() - above;
    assert!(
        gap <= size::ROW_TRIPLE / 2.0,
        "{gap} points between sections: {:?}",
        texts_in(&h, panel)
    );
}

/// LIBRARY-6 and LIBRARY-7: with Starter Resources in the Library, the Tab did not say they
/// were missing above "Every Starter Resource … is in your Library."; the feature lines were in
/// the link colour and opened nothing.
#[test]
fn the_starter_library_says_it_is_missing_only_when_it_is_and_its_feature_lines_are_hints() {
    let (mut h, _lab) = launch("wording-starter", dark());
    starter_library(&mut h);
    let panel = panel_of(&h, "Starter library");
    let texts = texts_in(&h, panel);
    assert!(
        !texts.iter().any(|t| t.contains("came without")),
        "{texts:?}"
    );
    assert!(
        texts
            .iter()
            .any(|t| t.contains("downloaded from the app's releases")),
        "{texts:?}"
    );
    let features: Vec<Painted> = painted(&h)
        .into_iter()
        .filter(|p| {
            p.text.starts_with("Word Study: Lexicon entries") && panel.contains(p.rect.center())
        })
        .collect();
    assert!(!features.is_empty(), "{texts:?}");
    let c = crate::theme::c();
    for f in &features {
        assert_ne!(f.color, c.lapis, "{:?} is in the link colour", f.text);
        assert_eq!(f.color, c.muted, "{:?}", f.text);
    }
    // Clicking it opens nothing.
    let tabs = h.shell.st.session.workspace.tabs().count();
    h.click(features[0].rect.center());
    h.frames(4);
    assert_eq!(h.shell.st.session.workspace.tabs().count(), tabs);
}

#[test]
fn the_starter_intro_follows_what_is_in_the_library() {
    use super::super::starter_tab::{
        INTRO_BUNDLED, INTRO_DOWNLOADED, INTRO_WITHOUT, starter_intro,
    };
    use obs_engine::starter::{StarterRow, StarterStatus};
    let row = |status| StarterRow {
        resource_id: "kjv".into(),
        title: "KJV".into(),
        abbreviation: "KJV".into(),
        resource_type: None,
        language: "en".into(),
        license: String::new(),
        bytes: 1,
        features: Vec::new(),
        status,
    };
    assert_eq!(
        starter_intro(true, &[row(StarterStatus::InLibrary)]),
        INTRO_BUNDLED
    );
    assert_eq!(starter_intro(false, &[]), INTRO_WITHOUT);
    assert_eq!(
        starter_intro(false, &[row(StarterStatus::Missing)]),
        INTRO_WITHOUT
    );
    assert_eq!(
        starter_intro(false, &[row(StarterStatus::Adding)]),
        INTRO_WITHOUT
    );
    for status in [StarterStatus::InLibrary, StarterStatus::Removed] {
        assert_eq!(
            starter_intro(false, &[row(StarterStatus::Missing), row(status)]),
            INTRO_DOWNLOADED
        );
    }
}

/// LIBRARY-8 and LIBRARY-9: an imported Genesis with no language and no licence showed "und"
/// and "LicenseRef-unknown" beside "en" and "LicenseRef-PublicDomain".
#[test]
fn library_rows_name_languages_and_licences_in_words() {
    let (mut h, _lab) = launch("wording-library", dark());
    library_after_import(&mut h);
    let panel = panel_of(&h, "Library");
    let texts = texts_in(&h, panel);
    // Rows name their language as the chips do; the chip of the unknown one says so.
    for word in ["Unknown language", "English", "Greek"] {
        assert!(texts.contains(&word), "{word:?} missing: {texts:?}");
    }
    for raw in ["und", "en", "grc"] {
        assert!(
            !texts.contains(&raw),
            "the code {raw:?} is shown: {texts:?}"
        );
    }
    // Licences in words, never the SPDX id; an unknown language leaves the badges their room.
    for word in ["Public domain", "CC BY", "Licence unknown"] {
        assert!(texts.contains(&word), "{word:?} missing: {texts:?}");
    }
    assert!(
        !texts.iter().any(|t| t.starts_with("LicenseRef-")),
        "{texts:?}"
    );
    let genesis = h
        .shapes
        .iter()
        .find(|(r, t)| t == "GEN.usfm" && panel.contains(r.center()))
        .map(|(r, _)| *r)
        .unwrap();
    let row: Vec<&str> = h
        .shapes
        .iter()
        .filter(|(r, _)| {
            panel.contains(r.center()) && (r.center().y - genesis.center().y).abs() < 4.0
        })
        .map(|(_, t)| t.as_str())
        .collect();
    assert!(row.contains(&"Licence unknown"), "{row:?}");
    assert!(!row.contains(&"Unknown language"), "{row:?}");
}

/// LIBRARY-8 and LIBRARY-12: About showed "LicenseRef-PublicDomain" and its Source as plain
/// text, where Credits links it.
#[test]
fn about_names_the_licence_in_words_and_links_the_source() {
    let (mut h, _lab) = launch("wording-about", dark());
    about_with_source(&mut h);
    let panel = panel_of(&h, "About · KJV");
    let texts = texts_in(&h, panel);
    assert!(texts.contains(&"Public domain"), "{texts:?}");
    assert!(
        !texts.iter().any(|t| t.starts_with("LicenseRef-")),
        "{texts:?}"
    );
    h.click_text("https://example.org/kjv");
    h.frames(2);
    assert_eq!(h.opened, ["https://example.org/kjv"]);
}

#[test]
fn licence_names_read_in_words() {
    use super::super::get_bibles_tab::licence_name;
    for (spdx, name) in [
        ("LicenseRef-PublicDomain", "Public domain"),
        ("LicenseRef-PD", "Public domain"),
        ("LicenseRef-unknown", "Unknown"),
        ("", "Unknown"),
        ("CC-BY-4.0", "CC BY 4.0"),
        ("CC-BY-SA-4.0", "CC BY-SA 4.0"),
        ("CC0-1.0", "CC0 1.0"),
        ("OFL-1.1", "OFL-1.1"),
    ] {
        assert_eq!(licence_name(spdx), name, "{spdx}");
    }
}

/// LIBRARY-10: "1 notes", and a Commentary's report read "0 Bible book(s) · 0 Verses".
#[test]
fn import_report_counts_read_in_the_words_of_what_was_imported() {
    use super::super::import_tab::summary_counts;
    use obs_formats::BookReport;
    let book = BookReport {
        code: "GEN".into(),
        book: None,
        chapters: 1,
        verses: 3,
    };
    let bible = obs_engine::ImportReport {
        books: vec![book],
        verses: 3,
        notes: 1,
        tagged_words: 39,
        ..Default::default()
    };
    assert_eq!(
        summary_counts(&bible),
        "1 Bible book · 3 Verses · 1 note · 39 Tagged words"
    );
    let commentary = obs_engine::ImportReport {
        entries: 6,
        notes: 1,
        ..Default::default()
    };
    assert_eq!(summary_counts(&commentary), "6 entries · 1 note");
    let lexicon = obs_engine::ImportReport {
        entries: 1,
        tagged_words: 2,
        ..Default::default()
    };
    assert_eq!(summary_counts(&lexicon), "1 entry · 2 Strong's links");

    let (mut h, _lab) = launch("wording-report", dark());
    import_genesis(&mut h);
    let texts = h.texts();
    assert!(
        texts.contains(&"1 Bible book · 1 Verse · 1 note · 0 Tagged words"),
        "{texts:?}"
    );
    assert!(texts.contains(&"1 source file · usfm"), "{texts:?}");
    let report = h
        .shell
        .st
        .session
        .workspace
        .tabs()
        .find(|t| matches!(t.kind, TabKind::ImportReport(_)))
        .map(|t| t.title.clone());
    assert_eq!(report.as_deref(), Some("Import report · GEN-1"));
}

/// LIBRARY-13: "/nonexistent/foo.usfm: No such file or directory (os error 2)", three phases
/// of "not built" and no Button.
#[test]
fn a_failed_import_says_what_went_wrong_in_plain_words_with_a_way_on() {
    let (mut h, _lab) = launch("wording-failed", dark());
    import_missing_file(&mut h);
    let texts = h.texts();
    assert!(
        texts
            .iter()
            .any(|t| t.starts_with("That file or folder does not exist")),
        "{texts:?}"
    );
    for raw in ["os error", "not built"] {
        assert!(!texts.iter().any(|t| t.contains(raw)), "{raw:?}: {texts:?}");
    }
    // The error itself is one click away ("os error 2", or 3 on Windows).
    h.click_text(crate::components::DETAILS);
    h.frames(2);
    assert!(
        h.texts().iter().any(|t| t.contains("os error")),
        "{:?}",
        h.texts()
    );
    h.click_text("Import another…");
    h.frames(2);
    assert!(
        h.find("Choose a USFM file or a folder of Bible books.")
            .is_some(),
        "{:?}",
        h.texts()
    );
    h.shell.st.pick_import = false;
    h.frames(2);
    h.click_text("Close");
    h.frames(2);
    assert!(
        !h.shell
            .st
            .session
            .workspace
            .tabs()
            .any(|t| matches!(t.kind, TabKind::ImportReport(_))),
        "the report Tab is still open"
    );
}

/// The Buttons labelled `label` in `panel`, as their frames.
fn buttons(h: &Harness, panel: Rect, label: &str) -> Vec<Rect> {
    h.shapes
        .iter()
        .filter(|(r, t)| t == label && panel.contains(r.center()))
        .map(|(text, _)| {
            h.rects
                .iter()
                .map(|(r, _)| *r)
                .filter(|r| r.contains_rect(*text) && r.width() < text.width() + 4.0 * space::L)
                .min_by(|a, b| a.area().total_cmp(&b.area()))
                .unwrap_or(*text)
        })
        .collect()
}

/// LIBRARY-14: the scroll bar's thumb ran over the right edge of the first Download and Open
/// Buttons.
#[test]
fn list_rows_keep_their_buttons_clear_of_the_scroll_bar() {
    let bar = {
        let s = egui::style::ScrollStyle::default();
        s.bar_inner_margin + s.bar_width + s.bar_outer_margin
    };
    for (title, open, label) in [
        (
            "Get Resources",
            get_resources_all as fn(&mut Harness),
            "Download",
        ),
        ("Starter library", starter_library, "Open"),
    ] {
        let (mut h, _lab) = launch(&format!("wording-scroll-{}", label.to_lowercase()), dark());
        open(&mut h);
        let panel = panel_of(&h, title);
        let found = buttons(&h, panel, label);
        assert!(!found.is_empty(), "{title}: {:?}", texts_in(&h, panel));
        // The page's inner right edge, less the bar's room.
        let page_right = panel.right() - space::XL;
        for b in found {
            assert!(
                b.right() <= page_right - bar + 0.5,
                "{title}: a {label} Button ends at {} under the scroll bar (page ends at {page_right})",
                b.right()
            );
        }
    }
}
