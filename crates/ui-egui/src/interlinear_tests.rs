//! Interlinear lines in Bible Tabs (#143) through the shell's real-event headless harness:
//! the Tab header's toggle and line picker, Greek Lemmas under KJV John 3:16, glosses under
//! TAHOT Genesis 1:1 right to left, the Current location kept across toggles, a click on a
//! line opening the right-click menu of its whole (even multiword) Tagged word, and
//! selecting and copying the lines (which Annotations never anchor to).

use obs_engine::{InterlinearLine, Library, TabId};
use obs_layout::{LaidBlock, LaidCell};
use obs_scripture::{Book, Verse};

use super::*;
use crate::view::ReadingView;

struct Files(std::path::PathBuf);
impl Drop for Files {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The Greek Lemmas of the KJV John 3:16 words checked below, as a Lexicon gives them.
const JOHN_3_16_LEMMAS: &[(&str, &str, &str)] = &[
    ("For", "G1063", "γάρ"),
    ("God", "G2316", "θεός"),
    ("so", "G3779", "οὕτω"),
    ("loved", "G25", "ἀγαπάω"),
    ("world", "G2889", "κόσμος"),
    ("that", "G5620", "ὥστε"),
    ("he gave", "G1325", "δίδωμι"),
    ("his", "G846", "αὐτός"),
    ("only begotten", "G3439", "μονογενής"),
    ("Son", "G5207", "υἱός"),
];

/// A library of real `.obr` files: KJV John with Strong's numbers, TAHOT Genesis 1:1 and a
/// Greek Lexicon with the John 3:16 Lemmas.
fn setup(h: &mut Harness, name: &str) -> Files {
    let dir = std::env::temp_dir().join(format!(
        "obs-shell-interlinear-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let files = Files(dir);
    let entries: Vec<(&str, &str, &str)> = JOHN_3_16_LEMMAS
        .iter()
        .map(|(_, n, lemma)| (*n, *lemma, "a gloss"))
        .collect();
    h.shell.st.session.library = Library::new();
    for r in [
        obs_engine::testing::kjv_john(),
        obs_engine::testing::bibles::tahot(),
        obs_engine::testing::lexicon("greek", "GRK", &entries),
    ] {
        let path = files.0.join(format!("{}.obr", r.manifest.resource_id));
        obs_store::write_obr(&r, &path).unwrap();
        h.shell.st.session.library.add_file(&path).unwrap();
    }
    h.shell.reset_layout();
    h.frames(2);
    files
}

/// Click a control in `tab`'s header (the Panel its text is in), by its label (`Interlinear`
/// stands for its short label in a narrow Panel too).
fn click_header(h: &mut Harness, tab: TabId, text: &str) {
    let short = if text == "Interlinear" { "Int." } else { text };
    let [ox, oy] = view(h, tab).text_origin;
    let panel = h
        .panels()
        .into_iter()
        .map(|(r, _)| r)
        .find(|r| r.contains(pos2(ox + 1.0, oy + 1.0)))
        .expect("the Tab's Panel");
    let p = h
        .shapes
        .iter()
        .rev()
        .find(|(r, t)| (t == text || t == short) && panel.contains(r.center()) && r.center().y < oy)
        .map(|(r, _)| r.center())
        .unwrap_or_else(|| panic!("no {text:?} in the header: {:?}", h.texts()));
    h.click(p);
}

/// Open a Bible at `reference` in its own Panel and let its text lay out.
fn open_at(h: &mut Harness, id: &str, reference: &str) -> TabId {
    let tab = h
        .shell
        .run("resource.open", &json!({ "id": id, "split": true }))
        .unwrap()["tab"]
        .as_u64()
        .map(TabId)
        .unwrap();
    h.frames(2);
    h.shell
        .run("tab.go", &json!({ "tab": tab.0, "text": reference }));
    settle_all(h);
    tab
}

fn view(h: &Harness, tab: TabId) -> &ReadingView {
    h.shell.st.tabs[&tab].view.as_ref().unwrap()
}

/// Unit bytes of a Verse's text (in unit 0).
fn verse_range(h: &Harness, tab: TabId, verse: Verse) -> std::ops::Range<usize> {
    let doc = &view(h, tab).doc;
    let (unit, byte) = doc.verse_position(verse).unwrap();
    assert_eq!(unit, 0);
    doc.verse_range_at(unit, byte).unwrap()
}

/// The laid-out block holding unit bytes `at` of the view, with its top (viewport y).
fn block_at(h: &Harness, tab: TabId, unit: usize, at: usize) -> (f32, Arc<LaidBlock>) {
    let v = view(h, tab);
    v.laid_in_view(900.0)
        .into_iter()
        .find(|(i, _, _)| {
            let b = &v.doc.blocks[*i];
            b.unit == unit && b.source.range.contains(&at)
        })
        .map(|(_, top, laid)| (top, laid))
        .expect("the block is laid out in view")
}

/// Unit bytes of `word` (the first match at or after byte `from`) in unit 0.
fn word_range(h: &Harness, tab: TabId, word: &str, from: usize) -> std::ops::Range<usize> {
    let text = &view(h, tab).doc.units[0].text;
    let at = text[from..].find(word).unwrap() + from;
    at..at + word.len()
}

/// The interlinear cell of row `row` under the Tagged word at unit bytes `word`.
fn cell(laid: &LaidBlock, word: &std::ops::Range<usize>, row: usize) -> LaidCell {
    laid.cells
        .iter()
        .find(|c| c.word == *word && c.row == row)
        .cloned()
        .unwrap_or_else(|| panic!("no row {row} under {word:?}"))
}

/// The word's text box `[x0, y0, x1, y1]`, relative to the text's left edge and the viewport:
/// its own glyphs (not the verse number anchored at its first byte).
fn word_box(h: &Harness, tab: TabId, word: &std::ops::Range<usize>) -> [f32; 4] {
    let (top, laid) = block_at(h, tab, 0, word.start);
    let mut out: Option<[f32; 4]> = None;
    for line in &laid.lines {
        for item in &line.items {
            for (bytes, x0, x1) in item.clusters() {
                if let Some(obs_layout::UnitPos::Text(b)) = laid.to_unit(bytes.start)
                    && word.contains(&b)
                {
                    let r = [x0, top + line.top, x1, top + line.text_bottom];
                    out = Some(match out {
                        None => r,
                        Some(o) => {
                            assert_eq!(o[1], r[1], "{word:?} is on one line");
                            [o[0].min(x0), o[1], o[2].max(x1), o[3]]
                        }
                    });
                }
            }
        }
    }
    out.unwrap_or_else(|| panic!("{word:?} is not laid out"))
}

/// Where a cell is on screen.
fn cell_spot(h: &Harness, tab: TabId, top: f32, c: &LaidCell) -> Pos2 {
    let [ox, oy] = view(h, tab).text_origin;
    pos2(
        ox + c.x + c.width / 2.0,
        oy + top + (c.top + c.bottom) / 2.0,
    )
}

#[test]
fn kjv_john_3_16_shows_greek_lemmas_under_its_english_words_aligned() {
    let mut h = Harness::new();
    let _files = setup(&mut h, "kjv");
    let kjv = open_at(&mut h, "kjv", "Jn 3:16");
    assert!(view(&h, kjv).interlinear().is_none());
    // The Tab header's toggle turns the lines on: the reverse interlinear's default lines,
    // the Greek word and its Gloss without the number (#243).
    click_header(&mut h, kjv, "Interlinear");
    settle_all(&mut h);
    let source = view(&h, kjv).interlinear().expect("lines shown").clone();
    assert_eq!(
        source.lines,
        [
            InterlinearLine::Lemma,
            InterlinearLine::Transliteration,
            InterlinearLine::Gloss
        ]
    );
    let verse = verse_range(&h, kjv, Verse::new(Book::John, 3, 16));
    let (top, laid) = block_at(&h, kjv, 0, verse.start);
    let mut from = verse.start;
    let mut last_x = (usize::MAX, f32::MIN);
    for (word, _, lemma) in JOHN_3_16_LEMMAS {
        let range = word_range(&h, kjv, word, from);
        from = range.end;
        let [x0, y0, x1, y1] = word_box(&h, kjv, &range);
        // The Lemma, its transliteration and the Gloss, stacked under the word.
        let lemma_cell = cell(&laid, &range, 0);
        assert_eq!(lemma_cell.text, *lemma, "under {word}");
        assert_eq!(cell(&laid, &range, 2).text, "a gloss", "under {word}");
        assert!(!cell(&laid, &range, 1).text.is_empty(), "under {word}");
        // Aligned: each line is centred under its word, below it, and the words' columns
        // never overlap.
        for row in 0..3 {
            let c = cell(&laid, &range, row);
            let centre = c.x + c.width / 2.0;
            assert!(
                (centre - (x0 + x1) / 2.0).abs() < 1.0,
                "row {row} under {word}: {centre} vs {x0}..{x1}"
            );
            assert!(
                top + c.top >= y1 - 0.5,
                "row {row} under {word} is below it"
            );
            if c.line == last_x.0 {
                // A line may reach right up to the text before it, not under it.
                assert!(
                    c.x >= last_x.1 - 0.01,
                    "{word}'s column overlaps the one before: row {row} {} at {} w {} vs {}",
                    c.text,
                    c.x,
                    c.width,
                    last_x.1
                );
            }
        }
        assert!(y0 < top + lemma_cell.top);
        last_x = (
            lemma_cell.line,
            (0..3)
                .map(|row| {
                    let c = cell(&laid, &range, row);
                    c.x + c.width
                })
                .fold(x1, f32::max),
        );
    }
    // The Greek is painted in the interlinear colour.
    let colour = h.shell.st.palette.color(obs_layout::ColorRole::Interlinear);
    assert!(h.meshes.contains(&colour), "no interlinear glyphs painted");
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        // After the jump's flash and the menu's fade-in.
        h.frames(80);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/previews/interlinear");
        h.screenshot(&dir.join("interlinear-kjv-john-3-16.ppm"));
    }
    // The lines menu (a chevron) picks the lines: adding the Strong's number puts it in
    // its place, above the Gloss.
    click_header(&mut h, kjv, "[chevron-down]");
    h.click_text("Strong's number");
    settle_all(&mut h);
    let source = view(&h, kjv).interlinear().unwrap();
    assert_eq!(source.lines[2], InterlinearLine::Strongs);
    let (_, laid) = block_at(&h, kjv, 0, verse.start);
    let loved = word_range(&h, kjv, "loved", verse.start);
    assert_eq!(cell(&laid, &loved, 2).text, "G25");
    assert_eq!(cell(&laid, &loved, 3).text, "a gloss");
    // And off again: no lines.
    h.events.push(Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.frames(2);
    click_header(&mut h, kjv, "Interlinear");
    settle_all(&mut h);
    assert!(view(&h, kjv).interlinear().is_none());
    let (_, laid) = block_at(&h, kjv, 0, verse.start);
    assert!(laid.cells.is_empty());
    drop(h);
}

#[test]
fn tahot_genesis_1_1_shows_glosses_right_to_left_under_the_hebrew() {
    let mut h = Harness::new();
    let _files = setup(&mut h, "tahot");
    let tahot = open_at(&mut h, "tahot", "Gn 1:1");
    // Hebrew's interlinear: Gloss first, then the transliteration.
    click_header(&mut h, tahot, "Interlinear");
    settle_all(&mut h);
    let source = view(&h, tahot).interlinear().expect("lines shown").clone();
    assert_eq!(
        source.lines,
        [InterlinearLine::Gloss, InterlinearLine::Transliteration]
    );
    let doc = Arc::clone(&view(&h, tahot).doc);
    let verse = verse_range(&h, tahot, Verse::new(Book::Genesis, 1, 1));
    let (_, laid) = block_at(&h, tahot, 0, verse.start);
    let words: Vec<std::ops::Range<usize>> = obs_layout::tagged_words(
        &doc.units[0],
        &obs_layout::BlockSource {
            kind: obs_layout::BlockKind::Plain,
            range: verse.clone(),
        },
    )
    .into_iter()
    .map(|(r, _)| r)
    .collect();
    assert_eq!(words.len(), 7, "Gen 1:1 has seven words");
    let glosses: Vec<String> = words.iter().map(|w| cell(&laid, w, 0).text).collect();
    assert_eq!(
        glosses,
        [
            "in beginning",
            "he created",
            "God",
            "<obj.>",
            "the heavens",
            "and <obj.>",
            "the earth"
        ]
    );
    // Right to left: each word's column is left of the one before, and the English inside
    // a cell still reads left to right.
    let mut right = (usize::MAX, f32::MAX);
    for w in &words {
        let [x0, _, x1, _] = word_box(&h, tahot, w);
        let c = cell(&laid, w, 0);
        assert!(
            (c.x + c.width / 2.0 - (x0 + x1) / 2.0).abs() < 1.0,
            "the gloss is centred under its word"
        );
        if c.line == right.0 {
            assert!(c.x + c.width <= right.1 + 0.5, "{} is out of order", c.text);
        }
        right = (c.line, c.x);
        assert!(
            c.items.iter().all(|i| !i.rtl),
            "{} reads left to right",
            c.text
        );
        assert!(!cell(&laid, w, 1).text.is_empty(), "a transliteration");
    }
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        // After the jump's flash and the menu's fade-in.
        h.frames(80);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/previews/interlinear");
        h.screenshot(&dir.join("interlinear-tahot-genesis-1-1.ppm"));
    }
    drop(h);
}

#[test]
fn toggling_and_picking_lines_keeps_the_current_location() {
    let mut h = Harness::new();
    // A long Bible with Tagged words (the bench's: Genesis to Deuteronomy).
    let doc = crate::bench::synthetic_bible();
    let id = doc.manifest.resource_id.clone();
    add_doc(&mut h.shell, doc);
    h.shell.reset_layout();
    h.frames(2);
    let tab = open_at(&mut h, &id, "Gn 30:5");
    let here = Verse::new(Book::Genesis, 30, 5);
    let current = |h: &mut Harness| {
        h.shell
            .st
            .tabs
            .get_mut(&tab)
            .and_then(|t| t.view.as_mut())
            .and_then(|v| v.current_location())
    };
    assert_eq!(current(&mut h), Some(here));
    for params in [
        json!({ "tab": tab.0, "on": true }),
        json!({ "tab": tab.0, "line": "gloss" }),
        json!({ "tab": tab.0, "line": "lemma", "shown": false }),
        json!({ "tab": tab.0, "on": false }),
        json!({ "tab": tab.0 }),
    ] {
        h.shell.run("tab.interlinear", &params).unwrap();
        settle_all(&mut h);
        assert_eq!(current(&mut h), Some(here), "after {params}");
        let history = h
            .shell
            .session()
            .workspace
            .tab(tab)
            .unwrap()
            .history
            .current();
        assert_eq!(history, Some(here), "after {params}");
    }
    assert!(view(&h, tab).interlinear().is_some());
    drop(h);
}

#[test]
fn clicking_an_interlinear_line_opens_the_words_right_click_menu() {
    let mut h = Harness::new();
    let _files = setup(&mut h, "click");
    let kjv = open_at(&mut h, "kjv", "Jn 3:16");
    h.shell
        .run("tab.interlinear", &json!({ "tab": kjv.0, "on": true }))
        .unwrap();
    settle_all(&mut h);
    let verse = verse_range(&h, kjv, Verse::new(Book::John, 3, 16));
    let loved = word_range(&h, kjv, "loved", verse.start);
    let (top, laid) = block_at(&h, kjv, 0, verse.start);
    let spot = cell_spot(&h, kjv, top, &cell(&laid, &loved, 2));
    h.click(spot);
    assert!(egui::Popup::is_any_open(&h.ctx), "the menu opened");
    for t in [
        "“loved”",
        "John 3:16",
        "Lemma",
        "ἀγαπάω",
        "Strong's number",
        "G25",
    ] {
        assert!(h.find(t).is_some(), "no {t}: {:?}", h.texts());
    }
    // The word is what the menu acts on.
    let sel = view(&h, kjv).selection.unwrap();
    let (a, b) = sel.range(&view(&h, kjv).doc);
    assert_eq!((a.byte, b.byte), (loved.start, loved.end));
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        // After the jump's flash and the menu's fade-in.
        h.frames(80);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/previews/interlinear");
        h.screenshot(&dir.join("interlinear-click-menu.ppm"));
    }
    drop(h);
}

/// A cell stands for its whole Tagged word: a click under "he gave" (one Strong's number on
/// two English words) makes the menu act on "he gave", not on "he".
#[test]
fn clicking_the_line_under_a_multiword_tagged_word_acts_on_the_whole_word() {
    let mut h = Harness::launch(Some(&temp_db("interlinear-multiword")));
    let _files = setup(&mut h, "multiword");
    let kjv = open_at(&mut h, "kjv", "Jn 3:16");
    h.shell
        .run("tab.interlinear", &json!({ "tab": kjv.0, "on": true }))
        .unwrap();
    settle_all(&mut h);
    let verse = verse_range(&h, kjv, Verse::new(Book::John, 3, 16));
    for (n, (words, strongs, lemma)) in [
        ("he gave", "G1325", "δίδωμι"),
        ("only begotten", "G3439", "μονογενής"),
    ]
    .into_iter()
    .enumerate()
    {
        let word = word_range(&h, kjv, words, verse.start);
        let (top, laid) = block_at(&h, kjv, 0, word.start);
        let lemma_row = cell(&laid, &word, 0);
        assert_eq!(lemma_row.text, lemma);
        let spot = cell_spot(&h, kjv, top, &cell(&laid, &word, 2));

        // The menu: the whole Tagged word is the selection, with its Strong's number.
        h.click(spot);
        assert!(egui::Popup::is_any_open(&h.ctx), "the menu opened");
        let title = format!("“{words}”");
        for t in [title.as_str(), lemma, strongs] {
            assert!(h.find(t).is_some(), "no {t}: {:?}", h.texts());
        }
        let sel = view(&h, kjv).selection.unwrap();
        let (a, b) = sel.range(&view(&h, kjv).doc);
        assert_eq!((a.byte, b.byte), (word.start, word.end), "{words}");

        // Copy from the menu's Selection: the whole word, its lines, then the reference.
        hover_text(&mut h, &title);
        h.click_text("Copy");
        let copied = h.copied.last().cloned().unwrap_or_default();
        let lines: Vec<&str> = copied.lines().collect();
        assert_eq!(lines[0], words, "{copied}");
        let row = format!("{words}\t{lemma}\t");
        assert!(lines.iter().any(|l| l.starts_with(&row)), "{copied}");
        assert!(
            lines[1..]
                .iter()
                .all(|l| !l.starts_with("he\t") && !l.starts_with("only\t"))
        );
        assert_eq!(lines.last(), Some(&"John 3:16 (KJV)"), "{copied}");

        // A Highlight from the menu anchors on the whole word.
        h.click(spot);
        hover_text(&mut h, &title);
        h.click_text("Yellow");
        let stored = h.shell.st.session.annotations(None).unwrap();
        assert_eq!(stored.len(), n + 1);
        let range = stored
            .iter()
            .filter_map(|a| a.anchor.as_ref()?.text_range())
            .find(|r| r.start as usize == word.start)
            .unwrap_or_else(|| panic!("no Highlight on {words}"));
        assert_eq!(range.end as usize, word.end);
        assert_eq!(range.quote, words);
    }
    drop(h);
}

#[test]
fn interlinear_lines_are_selected_and_copied_with_their_words_but_never_anchored() {
    let mut h = Harness::launch(Some(&temp_db("interlinear-copy")));
    let _files = setup(&mut h, "copy");
    let kjv = open_at(&mut h, "kjv", "Jn 3:16");
    h.shell
        .run("tab.interlinear", &json!({ "tab": kjv.0, "on": true }))
        .unwrap();
    settle_all(&mut h);
    let verse = verse_range(&h, kjv, Verse::new(Book::John, 3, 16));
    let first = word_range(&h, kjv, "For", verse.start);
    let loved = word_range(&h, kjv, "loved", verse.start);
    let (top, laid) = block_at(&h, kjv, 0, verse.start);
    // Drag along the Lemma line, from under "loved" back to just left of "For" (egui starts
    // a drag a few points along it, so it starts at the far end).
    let from = cell(&laid, &first, 0);
    let to = cell(&laid, &loved, 0);
    let [ox, oy] = view(&h, kjv).text_origin;
    let y = oy + top + (from.top + from.bottom) / 2.0;
    let [x0, ..] = word_box(&h, kjv, &first);
    h.drag(pos2(ox + to.x + to.width - 0.5, y), pos2(ox + x0 - 4.0, y));
    // The selection is of the text: it starts and ends on the words, never in a line.
    let sel = view(&h, kjv).selection.unwrap();
    let (a, b) = sel.range(&view(&h, kjv).doc);
    assert_eq!(a.byte, first.start);
    assert!(
        b.byte >= loved.start && b.byte <= loved.end,
        "{b:?} vs {loved:?}"
    );
    // The selected words' lines are drawn selected too.
    let selection = h.shell.st.palette.selection;
    let painted: Vec<Rect> = h
        .rects
        .iter()
        .filter(|(_, c)| *c == selection)
        .map(|(r, _)| *r)
        .collect();
    let lemma_spot = cell_spot(&h, kjv, top, &cell(&laid, &first, 0));
    assert!(
        painted.iter().any(|r| r.contains(lemma_spot)),
        "the Lemma under For is not drawn selected"
    );
    // Copy: the words, then a row per word with its lines, then the reference.
    h.events.push(Event::Copy);
    h.frames(2);
    let copied = h.copied.last().cloned().unwrap_or_default();
    let lines: Vec<&str> = copied.lines().collect();
    assert!(lines[0].starts_with("For God so lov"), "{copied}");
    assert!(lines.contains(&"For\tγάρ\tgar\ta gloss"), "{copied}");
    assert!(lines.contains(&"God\tθεός\ttheos\ta gloss"), "{copied}");
    assert_eq!(lines.last(), Some(&"John 3:16 (KJV)"), "{copied}");
    // A Highlight made from it anchors on the text only.
    press(&mut h, egui::Key::Num2, Modifiers::ALT);
    let stored = h.shell.st.session.annotations(None).unwrap();
    assert_eq!(stored.len(), 1);
    let range = stored[0].anchor.as_ref().unwrap().text_range().unwrap();
    assert_eq!(range.start as usize, first.start);
    let text = &view(&h, kjv).doc.units[0].text;
    assert_eq!(range.quote, text[a.byte..b.byte]);
    assert!(!range.quote.contains('γ'));
    drop(h);
}

/// An updated Lexicon keeps its resource id (and maybe its revision): the open Bible's lines
/// look its words up again, on screen and in what is copied, without the lines being turned
/// off and on.
#[test]
fn a_replaced_lexicon_updates_the_lines_of_open_bibles() {
    let mut h = Harness::new();
    let files = setup(&mut h, "replaced");
    let kjv = open_at(&mut h, "kjv", "Jn 3:16");
    h.shell
        .run(
            "tab.interlinear",
            &json!({ "tab": kjv.0, "on": true, "line": "gloss" }),
        )
        .unwrap();
    settle_all(&mut h);
    let verse = verse_range(&h, kjv, Verse::new(Book::John, 3, 16));
    let loved = word_range(&h, kjv, "loved", verse.start);
    let rows = |h: &Harness| {
        let lines = view(h, kjv).interlinear().unwrap().lines.clone();
        let lemma = lines
            .iter()
            .position(|l| *l == InterlinearLine::Lemma)
            .unwrap();
        let gloss = lines
            .iter()
            .position(|l| *l == InterlinearLine::Gloss)
            .unwrap();
        (lemma, gloss)
    };
    let shown = |h: &Harness| {
        let (lemma, gloss) = rows(h);
        let (_, laid) = block_at(h, kjv, 0, loved.start);
        (
            cell(&laid, &loved, lemma).text,
            cell(&laid, &loved, gloss).text,
        )
    };
    let copied = |h: &Harness| {
        let v = view(h, kjv);
        let sel = crate::selection::Selection {
            anchor: crate::selection::DocPos {
                unit: 0,
                byte: loved.start,
            },
            head: crate::selection::DocPos {
                unit: 0,
                byte: loved.end,
            },
            granularity: Default::default(),
        };
        v.copy_text(&sel)
    };
    assert_eq!(shown(&h), ("ἀγαπάω".into(), "a gloss".into()));

    // The update: the same Lexicon, a new revision with a new Lemma and Gloss for G25,
    // installed the way an Import or the Starter library replaces one.
    let replace = |h: &mut Harness, revision: u32, lemma: &str, gloss: &str| {
        let entries: Vec<(&str, &str, &str)> = JOHN_3_16_LEMMAS
            .iter()
            .map(|(_, n, l)| {
                if *n == "G25" {
                    (*n, lemma, gloss)
                } else {
                    (*n, *l, "a gloss")
                }
            })
            .collect();
        let mut r = obs_engine::testing::lexicon("greek", "GRK", &entries);
        r.manifest.revision = revision;
        let path = files.0.join(format!("greek-{revision}-{gloss}.obr"));
        obs_store::write_obr(&r, &path).unwrap();
        let session = &mut h.shell.st.session;
        session.library.add_file(&path).unwrap();
        session
            .workspace
            .push_effect(obs_engine::Effect::Replaced("greek".into()));
        settle_all(h);
    };
    let before = view(&h, kjv).interlinear().unwrap().clone();
    replace(&mut h, 2, "ἀγαπῶ", "to love");
    assert!(view(&h, kjv).interlinear().unwrap().lines == before.lines);
    assert_eq!(shown(&h), ("ἀγαπῶ".into(), "to love".into()));
    assert!(copied(&h).contains("loved\tἀγαπῶ\t"), "{}", copied(&h));
    assert!(copied(&h).contains("\tto love"), "{}", copied(&h));

    // Replaced again without a new revision: still looked up afresh.
    replace(&mut h, 2, "ἀγαπάω", "to cherish");
    assert_eq!(shown(&h), ("ἀγαπάω".into(), "to cherish".into()));
    assert!(copied(&h).contains("\tto cherish"), "{}", copied(&h));
    drop(h);
}

/// Hover a text (the menu's left column chooses the row under the pointer).
fn hover_text(h: &mut Harness, text: &str) {
    let p = h.at(text);
    h.events.push(Event::PointerMoved(p));
    h.frames(2);
}

fn press(h: &mut Harness, key: egui::Key, modifiers: Modifiers) {
    h.events.push(Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    });
    h.frames(2);
}

/// The acceptance checks against the real Starter library (KJV with Strong's, TAHOT and the
/// Lexicons, in the platform data folder). Run with
/// `cargo test -p obs-ui-egui real_starter_interlinear -- --ignored --nocapture`.
#[test]
#[ignore = "needs the installed Starter library"]
fn real_starter_interlinear() {
    let Some(dir) = obs_engine::default_library_dir()
        .filter(|d| d.join("kjv-strongs.obr").is_file() && d.join("tahot.obr").is_file())
    else {
        eprintln!("no Starter library: skipped");
        return;
    };
    let mut h = Harness::new();
    h.shell.st.session.library = Library::new();
    assert!(h.shell.st.session.library.scan(&dir).is_empty());
    h.shell.reset_layout();
    h.frames(2);
    let id = |h: &Harness, file: &str| {
        h.shell
            .st
            .session
            .library
            .entries()
            .iter()
            .find(|e| e.path.file_stem().is_some_and(|s| s == file))
            .map(|e| e.manifest.resource_id.clone())
            .unwrap()
    };
    let kjv_id = id(&h, "kjv-strongs");
    let kjv = open_at(&mut h, &kjv_id, "Jn 3:16");
    h.shell
        .run(
            "tab.interlinear",
            &json!({ "tab": kjv.0, "on": true, "line": "gloss" }),
        )
        .unwrap();
    settle_all(&mut h);
    let doc = Arc::clone(&view(&h, kjv).doc);
    let (unit, byte) = doc.verse_position(Verse::new(Book::John, 3, 16)).unwrap();
    let verse = doc.verse_range_at(unit, byte).unwrap();
    let (_, laid) = {
        let v = view(&h, kjv);
        v.laid_in_view(900.0)
            .into_iter()
            .find(|(i, _, _)| {
                let b = &v.doc.blocks[*i];
                b.unit == unit && b.source.range.contains(&verse.start)
            })
            .map(|(_, top, laid)| (top, laid))
            .unwrap()
    };
    let text = &doc.units[unit].text;
    let mut rows: Vec<(String, Vec<String>)> = Vec::new();
    for c in laid.cells.iter().filter(|c| verse.contains(&c.word.start)) {
        let word = text[c.word.clone()].to_string();
        match rows.last_mut() {
            Some((w, cells)) if *w == word && cells.len() == c.row => cells.push(c.text.clone()),
            _ => rows.push((word, vec![c.text.clone()])),
        }
    }
    for (word, cells) in &rows {
        eprintln!("KJV {word:>16} | {}", cells.join(" | "));
    }
    let loved = rows.iter().find(|(w, _)| w == "loved").unwrap();
    assert_eq!(loved.1[0], "ἀγαπάω");
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        h.frames(80);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/previews/interlinear");
        h.screenshot(&dir.join("interlinear-real-kjv-john-3-16.ppm"));
    }
    // TAHOT Gen 1:1: glosses under the Hebrew, right to left.
    let tahot_id = id(&h, "tahot");
    let tahot = open_at(&mut h, &tahot_id, "Gn 1:1");
    h.shell
        .run("tab.interlinear", &json!({ "tab": tahot.0, "on": true }))
        .unwrap();
    settle_all(&mut h);
    let doc = Arc::clone(&view(&h, tahot).doc);
    let (unit, byte) = doc.verse_position(Verse::new(Book::Genesis, 1, 1)).unwrap();
    let verse = doc.verse_range_at(unit, byte).unwrap();
    let v = view(&h, tahot);
    let laid = v
        .laid_in_view(900.0)
        .into_iter()
        .find(|(i, _, _)| {
            let b = &v.doc.blocks[*i];
            b.unit == unit && b.source.range.contains(&verse.start)
        })
        .map(|(_, _, laid)| laid)
        .unwrap();
    // In the text's order (cells are in visual order).
    let mut cells: Vec<&LaidCell> = laid
        .cells
        .iter()
        .filter(|c| c.row == 0 && verse.contains(&c.word.start))
        .collect();
    cells.sort_by_key(|c| c.word.start);
    let glosses: Vec<(f32, usize, String)> = cells
        .iter()
        .map(|c| (c.x, c.line, c.text.clone()))
        .collect();
    for (x, line, g) in &glosses {
        eprintln!("TAHOT line {line} x {x:>6.1} {g}");
    }
    assert!(glosses[0].2.contains("beginning"), "{glosses:?}");
    assert!(
        glosses
            .windows(2)
            .all(|w| w[0].1 != w[1].1 || w[0].0 > w[1].0)
    );
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        h.frames(80);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/previews/interlinear");
        h.screenshot(&dir.join("interlinear-real-tahot-genesis-1-1.ppm"));
    }
    drop(h);
}

/// A library with only the BSB (Gen 1:1-5 and John 1:1 as CrossWire's module tags them): no
/// Lexicon, no Hebrew or Greek text (#243).
fn setup_bsb_only(h: &mut Harness, name: &str) -> Files {
    let dir = std::env::temp_dir().join(format!(
        "obs-shell-interlinear-bsb-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let files = Files(dir);
    h.shell.st.session.library = Library::new();
    let r = obs_engine::testing::bsb_gen_1_john_1();
    let path = files.0.join("bsb.obr");
    obs_store::write_obr(&r, &path).unwrap();
    h.shell.st.session.library.add_file(&path).unwrap();
    h.shell.reset_layout();
    h.frames(2);
    files
}

/// The lines under each Tagged word of a laid block, in text order: (word, its lines).
fn rows_of(h: &Harness, tab: TabId, laid: &LaidBlock) -> Vec<(String, Vec<String>)> {
    let text = &view(h, tab).doc.units[0].text;
    let mut cells: Vec<&LaidCell> = laid.cells.iter().collect();
    cells.sort_by_key(|c| (c.word.start, c.row));
    let mut rows: Vec<(String, Vec<String>)> = Vec::new();
    for c in cells {
        let word = text[c.word.clone()].to_string();
        match rows.last_mut() {
            Some((w, lines)) if *w == word && lines.len() == c.row => lines.push(c.text.clone()),
            _ => rows.push((word, vec![c.text.clone()])),
        }
    }
    rows
}

/// #243 acceptance: with only the BSB installed, Gen 1:1 shows בְּרֵאשִׁית / bᵊrēʾšîṯ /
/// "beginning" under "In the beginning" — the default lines, the Hebrew word and its Gloss,
/// read from the Bible and the built-in Strong's index — and no hint, since every line reads.
#[test]
fn bsb_alone_shows_the_hebrew_word_its_transliteration_and_gloss_under_in_the_beginning() {
    let mut h = Harness::new();
    let _files = setup_bsb_only(&mut h, "alone");
    let bsb = open_at(&mut h, "bsb", "Gn 1:1");
    click_header(&mut h, bsb, "Interlinear");
    settle_all(&mut h);
    let verse = verse_range(&h, bsb, Verse::new(Book::Genesis, 1, 1));
    let (_, laid) = block_at(&h, bsb, 0, verse.start);
    let rows = rows_of(&h, bsb, &laid);
    assert_eq!(
        rows[0],
        (
            "In the beginning".to_string(),
            vec!["בְּרֵאשִׁית".into(), "bᵊrēʾšîṯ".into(), "beginning".into()]
        )
    );
    let created = rows.iter().find(|(w, _)| w == "created").unwrap();
    assert_eq!(created.1, ["בָּרָא", "bārāʾ", "to create"]);
    let earth = rows.iter().find(|(w, _)| w == "the earth").unwrap();
    assert_eq!(
        earth.1,
        ["הָאָרֶץ", "hāʾāreṣ", "land"],
        "no sof pasuq in the word"
    );
    for (word, lines) in &rows {
        assert!(lines.iter().all(|l| !l.is_empty()), "{word}: {lines:?}");
    }
    h.frames(2);
    assert!(
        !h.texts().iter().any(|t| t.contains("line is empty")),
        "no hint when every line reads"
    );
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        h.frames(80);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/previews/interlinear");
        h.screenshot(&dir.join("interlinear-bsb-genesis-1-1-after.ppm"));
    }
    drop(h);
}

/// #243 review: a Hebrew Bible whose words carry no Gloss reads an installed Lexicon's Gloss
/// before the built-in Strong's index's, and the index's where the Lexicon has none.
#[test]
fn an_original_text_reads_an_installed_lexicon_gloss_before_the_built_in_index() {
    let mut h = Harness::new();
    let dir = std::env::temp_dir().join(format!(
        "obs-shell-interlinear-hebrew-lexicon-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let files = Files(dir);
    h.shell.st.session.library = Library::new();
    for r in [
        obs_engine::testing::hebrew_bara(),
        obs_engine::testing::lexicon("hebrew", "HEB", &[("H1254", "בָּרָא", "to bring into being")]),
    ] {
        let path = files.0.join(format!("{}.obr", r.manifest.resource_id));
        obs_store::write_obr(&r, &path).unwrap();
        h.shell.st.session.library.add_file(&path).unwrap();
    }
    h.shell.reset_layout();
    h.frames(2);
    let hebrew = open_at(&mut h, "heb-bara", "Gn 1:1");
    click_header(&mut h, hebrew, "Interlinear");
    settle_all(&mut h);
    let source = view(&h, hebrew).interlinear().expect("lines shown").clone();
    assert_eq!(
        source.lines,
        [InterlinearLine::Gloss, InterlinearLine::Transliteration]
    );
    assert!(source.looks_up_in("hebrew"), "the Gloss is looked up");
    let verse = verse_range(&h, hebrew, Verse::new(Book::Genesis, 1, 1));
    let (_, laid) = block_at(&h, hebrew, 0, verse.start);
    let rows = rows_of(&h, hebrew, &laid);
    assert_eq!(
        rows[..2],
        [
            (
                "בָּרָא".to_string(),
                vec!["to bring into being".to_string(), "bārāʾ".into()]
            ),
            (
                "אֱלֹהִים".to_string(),
                vec!["God".to_string(), "ʾĕlōhîm".into()]
            ),
        ]
    );
    drop(h);
}

/// #243: a line with nothing to show under any word (Morphology: the BSB's words carry
/// none) brings a hint with the #135 buttons; Close keeps it closed on that Tab.
#[test]
fn an_empty_line_brings_a_hint_that_closes_for_good() {
    let mut h = Harness::launch(Some(&temp_db("interlinear-hint")));
    let _files = setup_bsb_only(&mut h, "hint");
    let bsb = open_at(&mut h, "bsb", "Gn 1:1");
    h.shell
        .run(
            "tab.interlinear",
            &json!({ "tab": bsb.0, "line": "morphology" }),
        )
        .unwrap();
    settle_all(&mut h);
    h.frames(2);
    let hint = |h: &Harness| {
        h.shapes
            .iter()
            .find(|(_, t)| t.starts_with("The Morphology line is empty"))
            .map(|(r, _)| *r)
    };
    let message = hint(&h).unwrap_or_else(|| panic!("no hint: {:?}", h.texts()));
    assert!(
        h.find("Get Hebrew & Greek texts…").is_some(),
        "{:?}",
        h.texts()
    );
    // The text moved down under it, and the lines still read.
    let [_, oy] = view(&h, bsb).text_origin;
    assert!(oy > message.bottom(), "{oy} vs {message:?}");
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        h.frames(80);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/previews/interlinear");
        h.screenshot(&dir.join("interlinear-bsb-morphology-hint.ppm"));
    }
    // Get Hebrew & Greek texts… opens Get Resources at that filter.
    h.click_text("Get Hebrew & Greek texts…");
    settle_all(&mut h);
    assert!(
        h.shell
            .st
            .session
            .workspace
            .tabs()
            .any(|t| t.kind == obs_engine::TabKind::GetBibles),
        "Get Resources opened"
    );
    assert_eq!(
        h.shell.st.session.get_bibles.filter(),
        obs_engine::get_bibles::ResourceFilter::HebrewAndGreek
    );
    // Back in the Bible: Close.
    h.shell.st.session.workspace.focus(bsb);
    h.frames(3);
    let message = hint(&h).expect("the hint is still there");
    let close = h
        .shapes
        .iter()
        .rev()
        .find(|(r, t)| t == "[x]" && (r.center().y - message.center().y).abs() < 40.0)
        .map(|(r, _)| r.center())
        .unwrap_or_else(|| panic!("no Close by the hint: {:?}", h.texts()));
    h.click(close);
    settle_all(&mut h);
    assert!(hint(&h).is_none(), "closed");
    assert!(
        h.shell
            .st
            .session
            .workspace
            .tab(bsb)
            .unwrap()
            .interlinear
            .hint_dismissed
    );
    // Turning the lines off and on again does not bring it back.
    for on in [false, true] {
        h.shell
            .run("tab.interlinear", &json!({ "tab": bsb.0, "on": on }))
            .unwrap();
        settle_all(&mut h);
    }
    h.frames(2);
    assert!(hint(&h).is_none(), "it stays closed on this Tab");
    drop(h);
}

/// One laid block as a spacing snapshot: each line's items (text and x) and its cells (row,
/// text, x and width), in px rounded to 0.1.
fn spacing_snapshot(laid: &LaidBlock) -> serde_json::Value {
    let r = |v: f32| (v * 10.0).round() / 10.0;
    let lines: Vec<serde_json::Value> = laid
        .lines
        .iter()
        .enumerate()
        .map(|(li, l)| {
            let items: Vec<serde_json::Value> = l
                .items
                .iter()
                .map(|i| json!([&laid.text[i.bytes.clone()], r(i.x), r(i.width)]))
                .collect();
            let mut cells: Vec<&LaidCell> = laid.cells.iter().filter(|c| c.line == li).collect();
            cells.sort_by(|a, b| (a.row, a.x).partial_cmp(&(b.row, b.x)).unwrap());
            let cells: Vec<serde_json::Value> = cells
                .iter()
                .map(|c| json!([c.row, c.text, r(c.x), r(c.width)]))
                .collect();
            json!({ "items": items, "cells": cells })
        })
        .collect();
    json!(lines)
}

/// The spacing rules every interlinear line keeps (#243): punctuation stays against its
/// word, a gap opens between two pieces of text only beside a word with a line wider than
/// itself, cells never come closer than the column gap, and never stand under an untagged
/// word.
fn check_spacing(laid: &LaidBlock, size: f32) {
    // Each Tagged word on each line: its x extent and whether a line under it is wider.
    for (li, line) in laid.lines.iter().enumerate() {
        let mut words: Vec<(f32, f32, bool)> = Vec::new();
        let mut seen: Vec<std::ops::Range<usize>> = Vec::new();
        for c in laid.cells.iter().filter(|c| c.line == li) {
            if seen.contains(&c.word) {
                continue;
            }
            seen.push(c.word.clone());
            let p = laid.from_unit(c.word.start).unwrap()..laid.from_unit(c.word.end).unwrap();
            let (l, r) = line
                .items
                .iter()
                .filter(|i| i.bytes.start >= p.start && i.bytes.end <= p.end)
                .fold((f32::MAX, f32::MIN), |(l, r), i| {
                    (l.min(i.x), r.max(i.x + i.width))
                });
            let widest = laid
                .cells
                .iter()
                .filter(|o| o.line == li && o.word == c.word)
                .map(|o| o.width)
                .fold(0.0, f32::max);
            words.push((l, r, widest > r - l + 0.01));
        }
        for pair in line.items.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            let gap = b.x - (a.x + a.width);
            let text = &laid.text[b.bytes.clone()];
            if text.starts_with([',', '.', '”', '’', ';', ':', '!', '?', '—']) {
                assert!(gap.abs() < 0.01, "{text:?} is pushed {gap} from its word");
            }
            if gap > 0.01 {
                let at = a.x + a.width;
                let left = words
                    .iter()
                    .filter(|w| w.1 <= at + 0.01)
                    .max_by(|x, y| x.1.total_cmp(&y.1));
                let right = words
                    .iter()
                    .filter(|w| w.0 >= b.x - 0.01)
                    .min_by(|x, y| x.0.total_cmp(&y.0));
                assert!(
                    left.is_some_and(|w| w.2) || right.is_some_and(|w| w.2),
                    "a {gap} px gap before {text:?} with no wide line beside it: line {li} at {at} words {words:?}",
                );
            }
        }
        for row in 0..3 {
            let mut cells: Vec<&LaidCell> = laid
                .cells
                .iter()
                .filter(|c| c.line == li && c.row == row)
                .collect();
            cells.sort_by(|a, b| a.x.total_cmp(&b.x));
            for w in cells.windows(2) {
                let gap = w[1].x - (w[0].x + w[0].width);
                assert!(
                    gap >= size * 0.6 - 0.05,
                    "{} / {}: {gap}",
                    w[0].text,
                    w[1].text
                );
            }
        }
        // No cell stands under an untagged word (#243 review).
        let tagged: Vec<std::ops::Range<usize>> = laid
            .cells
            .iter()
            .map(|c| laid.from_unit(c.word.start).unwrap()..laid.from_unit(c.word.end).unwrap())
            .collect();
        for (b, x0, x1) in line.items.iter().flat_map(|i| i.clusters()) {
            let cluster = &laid.text[b.clone()];
            if tagged.iter().any(|w| w.contains(&b.start))
                || !cluster.chars().any(char::is_alphanumeric)
            {
                continue;
            }
            let (left, right) = (x0.min(x1), x0.max(x1));
            for c in laid.cells.iter().filter(|c| c.line == li) {
                assert!(
                    c.x >= right - 0.01 || c.x + c.width <= left + 0.01,
                    "{:?} at {}..{} reaches under untagged {cluster:?} at {left}..{right}",
                    c.text,
                    c.x,
                    c.x + c.width
                );
            }
        }
    }
}

/// #243 spacing goldens: the BSB's Gen 1:1-5 and John 1:1 with the default lines from the
/// built-in Strong's index, laid out at a reading width. Rerun with `UPDATE_GOLDEN=1` and
/// review the diff when the layout changes on purpose.
#[test]
fn bsb_interlinear_spacing_goldens() {
    use obs_engine::InterlinearKind;
    let doc = crate::doc::ReadingDoc::from_resource(obs_engine::testing::bsb_gen_1_john_1());
    let kind = InterlinearKind::Reverse;
    let source =
        crate::InterlinearSource::new(kind, kind.default_lines().to_vec(), vec![], vec![]).unwrap();
    let settings = obs_layout::TextSettings::default();
    let size = settings.size * obs_layout::INTERLINEAR_SCALE;
    for (name, book) in [
        ("interlinear-bsb-gen-1-1-5", "GEN"),
        ("interlinear-bsb-john-1-1", "JHN"),
    ] {
        let unit = doc
            .units
            .iter()
            .position(|u| u.id.as_str() == book)
            .unwrap();
        let mut blocks = Vec::new();
        for (i, b) in doc.blocks.iter().enumerate() {
            if b.unit != unit {
                continue;
            }
            let para = source.paragraph(&doc, i, &settings, false).unwrap();
            let laid = obs_layout::layout(&para, 620.0, obs_fonts::ShapeCache::shared());
            if laid.cells.is_empty() {
                continue;
            }
            check_spacing(&laid, size);
            blocks.push(spacing_snapshot(&laid));
        }
        assert!(!blocks.is_empty(), "{name}");
        check_golden(name, &json!(blocks));
    }
}

fn check_golden(name: &str, actual: &serde_json::Value) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.json"));
    let text = serde_json::to_string_pretty(actual).unwrap() + "\n";
    if std::env::var_os("UPDATE_GOLDEN").is_some() || !path.exists() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &text).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap()
        .replace("\r\n", "\n");
    assert!(
        expected == text,
        "golden {} differs; rerun with UPDATE_GOLDEN=1 and review",
        path.display()
    );
}
