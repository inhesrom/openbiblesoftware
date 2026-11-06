//! Interlinear lines under Tagged words (#143): columns, alignment, right-to-left order,
//! unbroken words, hit-testing and height estimates.

use obs_fonts::ShapeCache;
use obs_model::{Direction, MorphemePart, ParaStyle, Span, SpanKind, TextUnit, UnitId, UnitKind};
use obs_scripture::{Book, Verse};

use super::*;

fn tagged(text: &str, words: &[(&str, &str)]) -> Vec<Span> {
    let mut spans = vec![Span {
        start: 0,
        end: text.len() as u32,
        kind: SpanKind::Para {
            style: ParaStyle::Prose,
        },
    }];
    let mut from = 0;
    for (w, strongs) in words {
        let start = from + text[from..].find(w).unwrap();
        from = start + w.len();
        spans.push(Span {
            start: start as u32,
            end: from as u32,
            kind: SpanKind::TaggedWord {
                parts: vec![MorphemePart::new(strongs.parse().unwrap())],
            },
        });
    }
    spans
}

/// KJV Jn 3:16's opening words, tagged as KJV+Strong's tags them ("the world" is one word).
fn kjv_unit() -> TextUnit {
    let text = "For God so loved the world, that he gave his only begotten Son.".to_string();
    let mut spans = tagged(
        &text,
        &[
            ("For", "G1063"),
            ("God", "G2316"),
            ("so", "G3779"),
            ("loved", "G25"),
            ("the world", "G2889"),
            ("that", "G5620"),
            ("he gave", "G1325"),
            ("his", "G846"),
            ("only begotten", "G3439"),
            ("Son", "G5207"),
        ],
    );
    spans.push(Span {
        start: 0,
        end: text.len() as u32,
        kind: SpanKind::Verse {
            verse: Verse::new(Book::John, 3, 16),
            last: 16,
        },
    });
    spans.sort_by_key(|s| (s.start, std::cmp::Reverse(s.end)));
    TextUnit {
        id: UnitId::book(Book::John),
        kind: UnitKind::BibleBook,
        title: None,
        text,
        spans,
    }
}

/// Gen 1:1 as the TAHOT writes it (each word tagged).
fn tahot_unit() -> TextUnit {
    let text = "בְּרֵאשִׁ֖ית בָּרָ֣א אֱלֹהִ֑ים אֵ֥ת הַשָּׁמַ֖יִם וְאֵ֥ת הָאָֽרֶץ׃".to_string();
    let words: Vec<(&str, &str)> = text
        .trim_end_matches('׃')
        .split(' ')
        .zip(["H7225", "H1254", "H430", "H853", "H8064", "H853", "H776"])
        .collect();
    let spans = tagged(&text, &words);
    TextUnit {
        id: UnitId::book(Book::Genesis),
        kind: UnitKind::BibleBook,
        title: None,
        text,
        spans,
    }
}

const KJV_LEMMAS: [(&str, &str); 10] = [
    ("G1063", "γάρ"),
    ("G2316", "θεός"),
    ("G3779", "οὕτω"),
    ("G25", "ἀγαπάω"),
    ("G2889", "κόσμος"),
    ("G5620", "ὥστε"),
    ("G1325", "δίδωμι"),
    ("G846", "αὐτός"),
    ("G3439", "μονογενής"),
    ("G5207", "υἱός"),
];

const TAHOT_GLOSSES: [(&str, &str); 5] = [
    ("H7225", "in beginning"),
    ("H1254", "he created"),
    ("H430", "God"),
    ("H853", "<obj.>"),
    ("H8064", "the heavens"),
];

/// Lay out a one-block unit with `rows` lines per word: each word's Strong's number looked up
/// in `table` for row 0, and the number itself for row 1.
fn interlinear_block(
    unit: &TextUnit,
    direction: Direction,
    width: f32,
    table: &[(&str, &str)],
    rows: usize,
) -> (Paragraph, LaidBlock) {
    let settings = TextSettings::default();
    let block = unit_blocks(unit).remove(0);
    let mut para = block_paragraph(unit, &block, direction, &settings);
    add_interlinear(
        &mut para,
        unit,
        &block,
        &settings,
        rows,
        &mut |_written, parts| {
            let n = parts[0].strongs.to_string();
            let first = table
                .iter()
                .find(|(k, _)| *k == n)
                .map_or("", |(_, v)| v)
                .to_string();
            vec![first, n]
        },
    );
    let laid = layout(&para, width, ShapeCache::shared());
    (para, laid)
}

/// The x range and line of the text item(s) drawing unit bytes `range`.
fn word_box(laid: &LaidBlock, range: std::ops::Range<usize>) -> (usize, f32, f32) {
    let p = laid.from_unit(range.start).unwrap()..laid.from_unit(range.end).unwrap();
    let mut found = None;
    for (li, line) in laid.lines.iter().enumerate() {
        for it in &line.items {
            if it.bytes.start >= p.start && it.bytes.end <= p.end {
                let (l, r) = found.map_or((f32::MAX, f32::MIN), |(_, l, r)| (l, r));
                found = Some((li, l.min(it.x), r.max(it.x + it.width)));
            }
        }
    }
    found.unwrap()
}

fn find(text: &str, needle: &str) -> std::ops::Range<usize> {
    let s = text.find(needle).unwrap();
    s..s + needle.len()
}

/// Acceptance (#143): the Greek Lemma under each English word, centred on it, below its text.
#[test]
fn lemmas_stand_centred_under_their_english_words() {
    let unit = kjv_unit();
    let (_, laid) = interlinear_block(&unit, Direction::Ltr, 600.0, &KJV_LEMMAS, 2);
    for (word, lemma) in [
        ("loved", "ἀγαπάω"),
        ("God", "θεός"),
        ("the world", "κόσμος"),
        ("only begotten", "μονογενής"),
    ] {
        let range = find(&unit.text, word);
        let (li, left, right) = word_box(&laid, range.clone());
        let cell = laid
            .cells
            .iter()
            .find(|c| c.row == 0 && c.word == range)
            .unwrap_or_else(|| panic!("no Lemma under {word}"));
        assert_eq!(cell.text, lemma);
        assert_eq!(cell.line, li);
        let centre = (left + right) / 2.0;
        assert!(
            (cell.x + cell.width / 2.0 - centre).abs() < 0.5,
            "{lemma} centred under {word}"
        );
        let line = &laid.lines[li];
        assert!(cell.top >= line.baseline, "{lemma} is under the text");
        assert!(cell.top >= line.text_bottom - 1.0 && cell.bottom <= line.bottom);
        let number = laid
            .cells
            .iter()
            .find(|c| c.row == 1 && c.word == range)
            .unwrap();
        assert!(number.top >= cell.bottom - 0.5, "rows are stacked in order");
    }
    // Columns never overlap: each line's cells of a row go left to right without touching.
    for li in 0..laid.lines.len() {
        let mut row0: Vec<&LaidCell> = laid
            .cells
            .iter()
            .filter(|c| c.line == li && c.row == 0)
            .collect();
        row0.sort_by(|a, b| a.x.total_cmp(&b.x));
        for w in row0.windows(2) {
            assert!(
                w[0].x + w[0].width < w[1].x,
                "{} / {}",
                w[0].text,
                w[1].text
            );
        }
    }
    // The cells' glyphs are painted too.
    let text_glyphs: usize = laid
        .lines
        .iter()
        .flat_map(|l| &l.items)
        .map(|i| i.shaped.glyphs.len())
        .sum();
    assert!(laid.glyphs().count() > text_glyphs);
}

#[test]
fn a_tagged_word_is_never_broken_and_its_lines_make_lines_taller() {
    let unit = kjv_unit();
    let plain = {
        let settings = TextSettings::default();
        let block = unit_blocks(&unit).remove(0);
        let para = block_paragraph(&unit, &block, Direction::Ltr, &settings);
        layout(&para, 600.0, ShapeCache::shared())
    };
    for width in [90.0, 120.0, 160.0, 220.0] {
        let (_, laid) = interlinear_block(&unit, Direction::Ltr, width, &KJV_LEMMAS, 2);
        for word in ["the world", "only begotten", "he gave"] {
            let range = find(&unit.text, word);
            let p = laid.from_unit(range.start).unwrap()..laid.from_unit(range.end).unwrap();
            let lines: Vec<usize> = laid
                .lines
                .iter()
                .enumerate()
                .filter(|(_, l)| l.bytes.start < p.end && l.bytes.end > p.start)
                .map(|(i, _)| i)
                .collect();
            assert_eq!(lines.len(), 1, "{word} on one line at {width}");
        }
    }
    let (_, laid) = interlinear_block(&unit, Direction::Ltr, 600.0, &KJV_LEMMAS, 2);
    let tall = laid.lines[0].bottom - laid.lines[0].top;
    let short = plain.lines[0].bottom - plain.lines[0].top;
    assert!(tall > short * 1.8, "{tall} vs {short}");
}

/// Acceptance (#143): TAHOT Gen 1:1's glosses run right to left under the Hebrew.
#[test]
fn glosses_run_right_to_left_under_hebrew() {
    let unit = tahot_unit();
    let (_, laid) = interlinear_block(&unit, Direction::Rtl, 700.0, &TAHOT_GLOSSES, 1);
    let words: Vec<&str> = unit.text.trim_end_matches('׃').split(' ').collect();
    let mut previous: Option<f32> = None;
    for (word, gloss) in words.iter().zip(TAHOT_GLOSSES.iter().map(|(_, g)| *g)) {
        let range = find(&unit.text, word);
        let (li, left, right) = word_box(&laid, range.clone());
        assert_eq!(li, 0);
        let cell = laid
            .cells
            .iter()
            .find(|c| c.row == 0 && c.word == range)
            .unwrap();
        assert_eq!(cell.text, gloss);
        // The English reads left to right inside its cell.
        assert!(cell.items.iter().all(|i| !i.rtl));
        assert!((cell.x + cell.width / 2.0 - (left + right) / 2.0).abs() < 0.5);
        if let Some(prev) = previous {
            assert!(
                cell.x + cell.width < prev,
                "{gloss} is left of the gloss before it"
            );
        }
        previous = Some(cell.x);
    }
    // The first word, and its gloss, are at the right.
    let first = laid
        .cells
        .iter()
        .find(|c| c.text == "in beginning")
        .unwrap();
    assert!(laid.cells.iter().all(|c| c.x <= first.x));
}

#[test]
fn an_interlinear_cell_hits_as_its_word() {
    let unit = kjv_unit();
    let (_, laid) = interlinear_block(&unit, Direction::Ltr, 600.0, &KJV_LEMMAS, 2);
    let loved = find(&unit.text, "loved");
    let cell = laid
        .cells
        .iter()
        .find(|c| c.row == 1 && c.word == loved)
        .unwrap();
    let hit = laid
        .hit(cell.x + cell.width / 2.0, (cell.top + cell.bottom) / 2.0)
        .unwrap();
    assert_eq!(
        hit.source,
        UnitPos::Object {
            anchor: loved.start,
            object: InlineObject::InterlinearCell { row: 1 },
        }
    );
    assert_eq!(
        &laid.text[hit.bytes.clone()],
        "loved",
        "the hit's bytes are the word's"
    );
    // A caret among the lines goes by x: it falls in the word above, never in the lines.
    let caret = laid.caret_at(cell.x + 1.0, (cell.top + cell.bottom) / 2.0);
    let word = laid.from_unit(loved.start).unwrap()..laid.from_unit(loved.end).unwrap();
    assert!(
        word.start <= caret && caret <= word.end,
        "{caret} in {word:?}"
    );
    // Text above the lines still hits as text.
    let (li, left, right) = word_box(&laid, loved.clone());
    let line = &laid.lines[li];
    let hit = laid.hit((left + right) / 2.0, line.baseline - 3.0).unwrap();
    assert!(matches!(hit.source, UnitPos::Text(b) if loved.contains(&b)));
    // Off every cell, under the text, nothing.
    assert!(laid.hit(0.5, cell.bottom - 1.0).is_none() || laid.cells.iter().any(|c| c.x <= 0.5));
}

#[test]
fn interlinear_estimates_are_close_to_real_heights() {
    let unit = kjv_unit();
    for width in [200.0, 360.0, 600.0] {
        let (para, laid) = interlinear_block(&unit, Direction::Ltr, width, &KJV_LEMMAS, 2);
        let mut estimate_para = para.clone();
        if let Some(il) = estimate_para.interlinear.as_mut() {
            il.words.clear();
        }
        let est = estimate_height(&estimate_para, width);
        let one_line = laid.lines[0].bottom - laid.lines[0].top;
        let ratio = est / laid.height;
        assert!(
            (0.6..1.5).contains(&ratio) || (est - laid.height).abs() <= one_line * 1.05,
            "estimate {est} vs {} at {width}",
            laid.height
        );
    }
}

#[test]
fn without_rows_a_paragraph_lays_out_as_before() {
    let unit = kjv_unit();
    let settings = TextSettings::default();
    let block = unit_blocks(&unit).remove(0);
    let plain = block_paragraph(&unit, &block, Direction::Ltr, &settings);
    let mut none = plain.clone();
    add_interlinear(&mut none, &unit, &block, &settings, 0, &mut |_, _| {
        Vec::new()
    });
    assert_eq!(none, plain);
    let a = layout(&plain, 300.0, ShapeCache::shared());
    assert!(a.cells.is_empty());
    assert!(a.lines.iter().all(|l| l.text_bottom == l.bottom));
}
