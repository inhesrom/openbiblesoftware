//! Reading layout (#209) through the shell's real-event headless harness, on BSB John 3 as
//! its USFM download has it: View → Reading layout sets a Tab's own Paragraphs or One verse
//! per line, Large chapter numbers, Section headings, Reading column width and Justified text;
//! each changes the text as it should and the Tab keeps its Current location. Hebrew stays
//! right to left and is never hyphenated.

use obs_engine::TabId;
use obs_layout::{
    ColumnWidth, InlineObject, LaidBlock, LaidLine, ReadingLayout, TextFlow, TextSettings, UnitPos,
};
use obs_scripture::{Book, Verse};

use super::*;
use crate::selection::{DocPos, range_rects};
use crate::view::ReadingView;

/// Visual evidence for `docs/previews/<name>.png` when `UPDATE_PREVIEWS` is set.
fn preview(h: &Harness, name: &str) {
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
        h.screenshot(&dir.join(format!("{name}.ppm")));
    }
}

/// BSB John 3 as its USFM download marks it up (public domain): two section headings, a
/// cross-reference line, footnotes and words of Jesus.
fn bsb_john_3() -> obs_model::Resource {
    let mut import = obs_formats::UsfmImport::new(obs_formats::UsfmOptions {
        resource_id: "bsb".into(),
        title: "Berean Standard Bible (test)".into(),
        abbreviation: "BSB".into(),
        language: "en".into(),
        source_file: "JHN.usfm".into(),
        tool_version: "test".into(),
        ..Default::default()
    });
    import
        .add_book(
            "JHN.usfm",
            include_str!("../../formats/tests/assets/usfm/bsb/JHN-3.usfm").into(),
        )
        .unwrap();
    let mut bsb = import.finish().unwrap().0;
    bsb.manifest.license = "LicenseRef-PublicDomain".into();
    bsb
}

struct Lab(std::path::PathBuf);

impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A launch with a User database and these Resources in the Library.
fn launch(name: &str, resources: &[obs_model::Resource]) -> (Harness, Lab) {
    let root =
        std::env::temp_dir().join(format!("obs-reading-layout-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("library")).unwrap();
    let mut h = Harness::launch(Some(&root.join("user.db")));
    let s = &mut h.shell.st.session;
    s.library = obs_engine::Library::new();
    s.library.set_directory(root.join("library"));
    for r in resources {
        let path = root
            .join("library")
            .join(format!("{}.obr", r.manifest.resource_id));
        obs_store::write_obr(r, &path).unwrap();
        s.library.add_file(&path).unwrap();
    }
    h.shell.reset_layout();
    h.frames(2);
    (h, Lab(root))
}

const JN_3_16: Verse = Verse::new(Book::John, 3, 16);

/// Open the BSB at John 3:16 in a Panel of its own, then scroll it by a point, as a reader
/// would: the Current location is worked out from the text in view, not kept from the jump.
fn open_bsb(h: &mut Harness) -> TabId {
    let tab = h
        .shell
        .run("resource.open", &json!({ "id": "bsb", "split": false }))
        .unwrap()["tab"]
        .as_u64()
        .unwrap();
    h.shell
        .run("tab.go", &json!({ "tab": tab, "text": "Jn 3:16" }));
    settle_all(h);
    view(h, TabId(tab)).scroll_by(1.0);
    settle_all(h);
    TabId(tab)
}

fn view(h: &mut Harness, tab: TabId) -> &mut ReadingView {
    h.shell
        .st
        .tabs
        .get_mut(&tab)
        .and_then(|t| t.view.as_mut())
        .unwrap()
}

/// The Current location, and where John 3:16's line is drawn from the top of the view.
fn location(h: &mut Harness, tab: TabId) -> (Option<Verse>, f32) {
    let v = view(h, tab);
    let at = v.current_location();
    let top = v.verse_top(JN_3_16, 900.0).unwrap();
    (at, top)
}

/// Set the Tab's own Reading layout as View → Reading layout does, and draw.
fn set(h: &mut Harness, tab: TabId, mut params: serde_json::Value) {
    params["tab"] = json!(tab.0);
    h.shell.run("tab.reading_layout", &params).unwrap();
    settle_all(h);
}

/// The Current location is still John 3:16, with its line at the top of the view.
fn assert_kept(h: &mut Harness, tab: TabId, what: &str) {
    let (at, top) = location(h, tab);
    assert_eq!(at, Some(JN_3_16), "{what}: the Current location moved");
    let size = view(h, tab).settings().size;
    assert!(
        top <= 0.5 && top > -2.0 * size,
        "{what}: John 3:16's line is at {top}, not the top of the view"
    );
}

/// Each laid-out block in view with the doc block index.
fn in_view(h: &mut Harness, tab: TabId) -> Vec<(usize, std::sync::Arc<LaidBlock>)> {
    view(h, tab)
        .visible(900.0)
        .into_iter()
        .map(|b| (b.index, b.laid))
        .collect()
}

/// The line holding the start of `verse`, in its laid block, and that block.
fn verse_line(h: &mut Harness, tab: TabId, verse: Verse) -> (std::sync::Arc<LaidBlock>, usize) {
    let v = view(h, tab);
    let (unit, byte) = v.doc.verse_position(verse).unwrap();
    let block = v.doc.block_at(unit, byte).unwrap();
    let laid = v.laid(block).unwrap();
    let p = laid.from_unit(byte).unwrap();
    let line = laid.lines.iter().position(|l| l.bytes.end > p).unwrap();
    (laid, line)
}

/// Whether a line begins with `verse`'s number.
fn starts_with_verse(laid: &LaidBlock, line: &LaidLine, verse: Verse) -> bool {
    matches!(
        laid.to_unit(line.bytes.start),
        Some(UnitPos::Object {
            object: InlineObject::VerseNumber(v),
            ..
        }) if v == verse
    )
}

/// Doc blocks that are a section heading.
fn headings(h: &mut Harness, tab: TabId) -> Vec<usize> {
    let v = view(h, tab);
    (v.doc.blocks.iter().enumerate())
        .filter(|(_, b)| matches!(b.source.kind, obs_layout::BlockKind::Heading(_)))
        .map(|(i, _)| i)
        .collect()
}

/// A line's right edge, without the space hanging at its end.
fn right_edge(line: &LaidLine) -> f32 {
    let mut right = (line.items.iter())
        .map(|i| i.x + i.width)
        .fold(f32::MIN, f32::max);
    if let Some(last) = line.items.iter().find(|i| i.bytes.end == line.bytes.end) {
        right -= last.shaped.trailing_space;
    }
    right
}

/// Acceptance (#209): each Reading layout option changes BSB John 3 as expected, and the
/// Tab keeps its Current location (John 3:16, its line at the top) through every change.
#[test]
fn each_option_changes_bsb_john_3_and_keeps_the_current_location() {
    let (mut h, _lab) = launch("options", &[bsb_john_3()]);
    // Room for a full-width column wider than Wide.
    h.screen = vec2(2000.0, 900.0);
    let tab = open_bsb(&mut h);
    let pane = {
        let tiles = &h.shell.tree.tiles;
        tiles.rect(tiles.find_pane(&tab).unwrap()).unwrap()
    };
    let wide = 50.0 * view(&mut h, tab).settings().size;
    assert!(pane.width() > wide + 200.0, "{pane:?}");
    assert_kept(&mut h, tab, "opening");
    preview(&h, "reading-layout-before");
    let medium = view(&mut h, tab).width();

    // Paragraphs (the default): verses 16-18 run on in one paragraph.
    let (laid, line) = verse_line(&mut h, tab, Verse::new(Book::John, 3, 17));
    assert!(!starts_with_verse(
        &laid,
        &laid.lines[line],
        Verse::new(Book::John, 3, 17)
    ));

    // One verse per line: every verse in view starts a line of its own.
    set(&mut h, tab, json!({ "flow": "verse_per_line" }));
    assert_kept(&mut h, tab, "One verse per line");
    for n in 16..=21 {
        let verse = Verse::new(Book::John, 3, n);
        let (laid, line) = verse_line(&mut h, tab, verse);
        assert!(
            starts_with_verse(&laid, &laid.lines[line], verse),
            "verse {n} does not start its line"
        );
    }
    preview(&h, "reading-layout-verse-per-line");
    set(&mut h, tab, json!({ "flow": "paragraphs" }));
    assert_kept(&mut h, tab, "Paragraphs");

    // Large chapter numbers: the chapter opens with a numeral beside two lines.
    set(&mut h, tab, json!({ "large_chapter_numbers": true }));
    assert_kept(&mut h, tab, "Large chapter numbers");
    let (laid, line) = verse_line(&mut h, tab, Verse::new(Book::John, 3, 1));
    assert_eq!(line, 0);
    let numeral = &laid.lines[0].items[0];
    assert_eq!(laid.text.get(numeral.bytes.clone()), Some("3"));
    assert!(numeral.style.size > 2.5 * TextSettings::default().size);
    assert!(laid.lines[1].items[0].x >= numeral.x + numeral.width);
    view(&mut h, tab).jump_to(Verse::new(Book::John, 3, 1));
    h.frames(2);
    preview(&h, "reading-layout-large-chapter-numbers");
    view(&mut h, tab).jump_to(JN_3_16);
    view(&mut h, tab).scroll_by(1.0);
    settle_all(&mut h);
    set(&mut h, tab, json!({ "large_chapter_numbers": false }));
    assert_kept(&mut h, tab, "inline chapter numbers");

    // Section headings: hidden, the two headings take no room; shown, they are back.
    let heading_blocks = headings(&mut h, tab);
    assert_eq!(
        heading_blocks.len(),
        2,
        "Jesus and Nicodemus; John's Testimony"
    );
    set(&mut h, tab, json!({ "section_headings": false }));
    assert_kept(&mut h, tab, "Section headings hidden");
    for &b in &heading_blocks {
        let laid = view(&mut h, tab).laid(b).unwrap();
        assert_eq!(laid.height, 0.0, "a hidden heading takes room");
    }
    // "John’s Testimony about Jesus" (after 3:21) is not drawn.
    view(&mut h, tab).jump_to(Verse::new(Book::John, 3, 20));
    h.frames(2);
    preview(&h, "reading-layout-no-headings");
    assert!(
        !h.texts().iter().any(|t| t.contains("Testimony")),
        "the heading is not in the text either"
    );
    view(&mut h, tab).jump_to(JN_3_16);
    view(&mut h, tab).scroll_by(1.0);
    settle_all(&mut h);
    set(&mut h, tab, json!({ "section_headings": true }));
    assert_kept(&mut h, tab, "Section headings shown");
    let laid = view(&mut h, tab).laid(heading_blocks[0]).unwrap();
    assert!(laid.height > 10.0);

    // Reading column width: narrow < medium < wide < full, which is the pane's width.
    let mut widths = Vec::new();
    for column in ["narrow", "medium", "wide", "full"] {
        set(&mut h, tab, json!({ "column": column }));
        assert_kept(&mut h, tab, column);
        widths.push(view(&mut h, tab).width());
        if column == "narrow" {
            preview(&h, "reading-layout-narrow");
        }
    }
    assert_eq!(widths[1], medium, "medium is the default");
    assert!(widths.windows(2).all(|w| w[0] < w[1]), "{widths:?}");
    assert!(widths[3] > pane.width() - 2.0 * crate::view::MARGIN - 20.0);
    set(&mut h, tab, json!({ "column": "medium" }));

    // Justified text: every full line of John 3:16's paragraph reaches the right edge; its
    // last line does not.
    set(&mut h, tab, json!({ "justified": true }));
    assert_kept(&mut h, tab, "Justified text");
    let width = view(&mut h, tab).width();
    let (laid, _) = verse_line(&mut h, tab, JN_3_16);
    let (last, full) = laid.lines.split_last().unwrap();
    assert!(full.len() >= 3);
    for line in full {
        assert!(
            (right_edge(line) - width).abs() < 0.5,
            "{}",
            right_edge(line)
        );
    }
    assert!(right_edge(last) < width - 10.0);
    preview(&h, "reading-layout-justified");

    // Every option at once, then back to the defaults: still John 3:16.
    set(
        &mut h,
        tab,
        json!({ "flow": "verse_per_line", "large_chapter_numbers": true, "section_headings": false, "column": "narrow" }),
    );
    assert_kept(&mut h, tab, "every option");
    preview(&h, "reading-layout-all");
    set(&mut h, tab, json!({ "reset": true }));
    assert_kept(&mut h, tab, "Use defaults");
    assert_eq!(
        h.shell.session().reading_layout(tab),
        ReadingLayout::default()
    );
    assert_eq!(view(&mut h, tab).width(), medium);
}

/// A Highlight (or selection) over a chapter's first verse marks its drop numeral over the
/// numeral's whole height, not only the first line's band.
#[test]
fn a_range_over_the_first_verse_marks_its_whole_drop_numeral() {
    let (mut h, _lab) = launch("drop-range", &[bsb_john_3()]);
    let tab = open_bsb(&mut h);
    set(&mut h, tab, json!({ "large_chapter_numbers": true }));
    let jn_3_1 = Verse::new(Book::John, 3, 1);
    let v = view(&mut h, tab);
    v.jump_to(jn_3_1);
    let (unit, byte) = v.doc.verse_position(jn_3_1).unwrap();
    let block = v.doc.block_at(unit, byte).unwrap();
    let top = (v.visible(900.0).into_iter())
        .find(|b| b.index == block)
        .unwrap()
        .top;
    let laid = v.laid(block).unwrap();
    let first = &laid.lines[0];
    let numeral = &first.items[0];
    assert_eq!(laid.text.get(numeral.bytes.clone()), Some("3"));
    let range = (
        DocPos { unit, byte },
        DocPos {
            unit,
            byte: byte + 20,
        },
    );
    let rects = range_rects(v, &[range], 900.0).remove(0);
    let x = numeral.x + numeral.width / 2.0;
    let marked = (rects.iter())
        .find(|r| r[0] <= x && x <= r[2])
        .expect("the numeral is marked");
    let baseline = first.baseline - numeral.style.raise;
    assert!(marked[1] <= top + first.top + 0.5, "{marked:?}");
    assert!(
        marked[3] >= top + baseline,
        "{marked:?} stops above its baseline {}",
        top + baseline
    );
    // The text beside it is marked in its own line's band.
    let beside = (rects.iter())
        .find(|r| r[0] >= numeral.x + numeral.width)
        .unwrap();
    assert_eq!(beside[3], top + first.text_bottom);
}

/// Narrow justified columns hyphenate English words: a hyphen is drawn, not added to the text,
/// so copying and finding read the words whole.
#[test]
fn justified_bsb_hyphenates_long_words_without_changing_the_text() {
    let (mut h, _lab) = launch("hyphens", &[bsb_john_3()]);
    let tab = open_bsb(&mut h);
    set(
        &mut h,
        tab,
        json!({ "justified": true, "column": "narrow" }),
    );
    assert_kept(&mut h, tab, "narrow justified");
    let blocks = in_view(&mut h, tab);
    let hyphens: Vec<String> = blocks
        .iter()
        .flat_map(|(_, laid)| {
            (laid.lines.iter())
                .filter(|l| l.items.iter().any(|i| i.bytes.is_empty()))
                .map(|l| {
                    laid.text[l.bytes.clone()]
                        .split_whitespace()
                        .last()
                        .unwrap_or_default()
                        .to_string()
                })
        })
        .collect();
    assert!(!hyphens.is_empty(), "no word was hyphenated in view");
    // The text of every block is the unit's own: the hyphen is drawn only.
    let v = view(&mut h, tab);
    for (i, laid) in &blocks {
        let b = &v.doc.blocks[*i];
        let unit = &v.doc.units[b.unit].text;
        for line in &laid.lines {
            for item in &line.items {
                if let Some(UnitPos::Text(u)) = laid.to_unit(item.bytes.start)
                    && !item.bytes.is_empty()
                {
                    let shown = &laid.text[item.bytes.clone()];
                    assert!(unit[u..].starts_with(shown), "{shown:?}");
                }
            }
        }
    }
    preview(&h, "reading-layout-hyphenated");
}

/// The menu: View → Reading layout acts on the focused Tab, and only on it; Settings' defaults
/// reach every Tab that has not chosen.
#[test]
fn view_reading_layout_sets_the_focused_tab_and_settings_set_the_defaults() {
    let (mut h, _lab) = launch("menu", &[bsb_john_3()]);
    let tab = open_bsb(&mut h);
    h.shell.st.session.workspace.user_focused(tab);
    h.frames(2);
    h.open_menu("View");
    h.frames(1);
    h.click_text("Reading layout");
    // Past the submenu's fade-in, for the preview.
    h.frames(40);
    preview(&h, "reading-layout-menu");
    h.click_text("One verse per line");
    h.frames(2);
    let own = h.shell.session().workspace.tab(tab).unwrap().reading_layout;
    assert_eq!(own.flow, Some(TextFlow::VersePerLine));
    assert_eq!(
        view(&mut h, tab).settings().layout.flow,
        TextFlow::VersePerLine
    );
    assert_kept(&mut h, tab, "the menu");
    // Choosing closes the menu, as menus do.
    h.open_menu("View");
    h.frames(1);
    h.click_text("Reading layout");
    h.frames(2);
    h.click_text("Justified text");
    h.frames(2);
    assert!(view(&mut h, tab).settings().layout.justified);
    assert_kept(&mut h, tab, "Justified from the menu");

    // A second BSB Tab follows the defaults.
    let other = TabId(
        h.shell
            .run("resource.open", &json!({ "id": "bsb", "split": true }))
            .unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    settle_all(&mut h);
    assert_eq!(
        view(&mut h, other).settings().layout,
        ReadingLayout::default()
    );
    h.shell
        .run("view.column_width", &json!({ "value": "wide" }))
        .unwrap();
    settle_all(&mut h);
    assert_eq!(
        view(&mut h, other).settings().layout.column,
        ColumnWidth::Wide
    );
    assert_eq!(
        view(&mut h, tab).settings().layout.column,
        ColumnWidth::Wide
    );
    assert_eq!(
        view(&mut h, tab).settings().layout.flow,
        TextFlow::VersePerLine,
        "the Tab's own choice stays"
    );
}

/// Hebrew stays right to left with every option on, and is never hyphenated: an English
/// Bible's narrow justified text is.
#[test]
fn hebrew_stays_right_to_left_and_is_never_hyphenated() {
    let everything = TextSettings {
        layout: ReadingLayout {
            flow: TextFlow::VersePerLine,
            large_chapter_numbers: true,
            section_headings: false,
            column: ColumnWidth::Narrow,
            justified: true,
        },
        ..TextSettings::default()
    };
    let hyphens = |doc: std::sync::Arc<crate::doc::ReadingDoc>, verse: Verse| {
        let mut v = ReadingView::new(doc, everything);
        v.set_width(180.0);
        assert!(v.jump_to(verse));
        let blocks = v.visible(2000.0);
        let mut n = 0;
        for b in &blocks {
            for line in &b.laid.lines {
                n += line.items.iter().filter(|i| i.bytes.is_empty()).count();
                if b.laid.rtl {
                    // Right to left: the logically first item is rightmost on its line.
                    let first = line.items.iter().min_by_key(|i| i.bytes.start).unwrap();
                    assert!(line.items.iter().all(|i| i.x <= first.x));
                }
            }
        }
        (n, blocks.iter().any(|b| b.laid.rtl))
    };
    let (n, rtl) = hyphens(crate::tests::hebrew_doc(), Verse::new(Book::Genesis, 1, 1));
    assert!(rtl, "the Hebrew Bible reads right to left");
    assert_eq!(n, 0, "a Hebrew word was hyphenated");
    let english = std::sync::Arc::new(crate::doc::ReadingDoc::from_resource(bsb_john_3()));
    let (n, rtl) = hyphens(english, JN_3_16);
    assert!(!rtl);
    assert!(n > 0, "narrow justified English is hyphenated");
    assert!(crate::view::hyphenates("en") && crate::view::hyphenates("en-US"));
    assert!(!crate::view::hyphenates("hbo") && !crate::view::hyphenates("grc"));
}
