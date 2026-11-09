//! Reading layout (#209) in the layout pipeline: one verse per line, drop numerals, hidden
//! section headings, justified text and English hyphenation, and Hebrew that stays right to
//! left and is never hyphenated.

use std::ops::Range;

use obs_fonts::{FaceId, ShapeCache, ShapeStyle, ink_top};
use obs_model::{Direction, MorphemePart, ParaStyle, Span, SpanKind, TextUnit, UnitId, UnitKind};
use obs_scripture::{Book, Verse};

use super::*;

/// BSB John 3:1-3 (public domain): a section heading over a prose paragraph of three verses.
const HEADING: &str = "Jesus and Nicodemus";
const VERSES: [&str; 3] = [
    "Now there was a man of the Pharisees named Nicodemus, a leader of the Jews.",
    "He came to Jesus at night and said, “Rabbi, we know that You are a teacher who has come \
     from God. For no one could perform the signs You are doing if God were not with him.”",
    "Jesus replied, “Truly, truly, I tell you, no one can see the kingdom of God unless he is \
     born again.”",
];

fn span(range: std::ops::Range<usize>, kind: SpanKind) -> Span {
    Span {
        start: range.start as u32,
        end: range.end as u32,
        kind,
    }
}

fn john_3() -> TextUnit {
    let mut text = format!("{HEADING}\n");
    let para_start = text.len();
    let mut spans = vec![span(0..HEADING.len(), SpanKind::Heading { level: 1 })];
    for (i, v) in VERSES.iter().enumerate() {
        if i > 0 {
            text.push(' ');
        }
        let start = text.len();
        text.push_str(v);
        let n = i as u16 + 1;
        spans.push(span(
            start..text.len(),
            SpanKind::Verse {
                verse: Verse::new(Book::John, 3, n),
                last: n,
            },
        ));
    }
    spans.push(span(
        para_start..text.len(),
        SpanKind::Para {
            style: ParaStyle::Prose,
        },
    ));
    spans.push(span(0..text.len(), SpanKind::Chapter { chapter: 3 }));
    spans.sort_by_key(|s| (s.start, std::cmp::Reverse(s.end)));
    TextUnit {
        id: UnitId::book(Book::John),
        kind: UnitKind::BibleBook,
        title: None,
        text,
        spans,
    }
}

/// Gen 1:1-3 in pointed Hebrew, one paragraph.
fn hebrew_genesis_1() -> TextUnit {
    let verses = [
        "בְּרֵאשִׁ֖ית בָּרָ֣א אֱלֹהִ֑ים אֵ֥ת הַשָּׁמַ֖יִם וְאֵ֥ת הָאָֽרֶץ׃",
        "וְהָאָ֗רֶץ הָיְתָ֥ה תֹ֙הוּ֙ וָבֹ֔הוּ וְחֹ֖שֶׁךְ עַל־פְּנֵ֣י תְה֑וֹם וְר֣וּחַ אֱלֹהִ֔ים מְרַחֶ֖פֶת עַל־פְּנֵ֥י הַמָּֽיִם׃",
        "וַיֹּ֥אמֶר אֱלֹהִ֖ים יְהִ֣י א֑וֹר וַֽיְהִי־אֽוֹר׃",
    ];
    let mut text = String::new();
    let mut spans = Vec::new();
    for (i, v) in verses.iter().enumerate() {
        if i > 0 {
            text.push(' ');
        }
        let start = text.len();
        text.push_str(v);
        let n = i as u16 + 1;
        spans.push(span(
            start..text.len(),
            SpanKind::Verse {
                verse: Verse::new(Book::Genesis, 1, n),
                last: n,
            },
        ));
    }
    spans.push(span(
        0..text.len(),
        SpanKind::Para {
            style: ParaStyle::Prose,
        },
    ));
    spans.sort_by_key(|s| (s.start, std::cmp::Reverse(s.end)));
    TextUnit {
        id: UnitId::book(Book::Genesis),
        kind: UnitKind::BibleBook,
        title: None,
        text,
        spans,
    }
}

fn settings(layout: ReadingLayout) -> TextSettings {
    TextSettings {
        layout,
        ..TextSettings::default()
    }
}

/// Each block of `unit` laid out at `width`, with English hyphenation where `hyphenate`.
fn lay(
    unit: &TextUnit,
    direction: Direction,
    layout_options: ReadingLayout,
    width: f32,
    hyphenate: bool,
) -> Vec<(BlockSource, LaidBlock)> {
    let s = settings(layout_options);
    unit_blocks(unit)
        .into_iter()
        .map(|b| {
            let mut p = block_paragraph(unit, &b, direction, &s);
            p.hyphenate = hyphenate && p.justify;
            let l = layout(&p, width, ShapeCache::shared());
            (b, l)
        })
        .collect()
}

fn prose(blocks: &[(BlockSource, LaidBlock)]) -> &LaidBlock {
    &blocks
        .iter()
        .find(|(b, _)| b.kind == BlockKind::Paragraph(ParaStyle::Prose))
        .unwrap()
        .1
}

/// The verse whose number object starts each line, if one does.
fn verse_starting(block: &LaidBlock, line: &LaidLine) -> Option<u16> {
    match block.to_unit(line.bytes.start)? {
        UnitPos::Object {
            object: InlineObject::VerseNumber(v),
            ..
        } => Some(v.verse),
        _ => None,
    }
}

/// A line's visual left and right edges, without the space hanging at its logical end.
fn edges(block: &LaidBlock, line: &LaidLine) -> (f32, f32) {
    let mut left = line.items.iter().map(|i| i.x).fold(f32::MAX, f32::min);
    let mut right = line
        .items
        .iter()
        .map(|i| i.x + i.width)
        .fold(f32::MIN, f32::max);
    if let Some(last) = line.items.iter().find(|i| i.bytes.end == line.bytes.end) {
        if block.rtl {
            left += last.shaped.trailing_space;
        } else {
            right -= last.shaped.trailing_space;
        }
    }
    (left, right)
}

/// Hyphens drawn at line ends: items with no text of their own.
fn hyphens(block: &LaidBlock) -> usize {
    block
        .lines
        .iter()
        .flat_map(|l| &l.items)
        .filter(|i| i.bytes.is_empty())
        .count()
}

#[test]
fn the_default_reading_layout_is_todays_layout() {
    let unit = john_3();
    let plain = TextSettings::default();
    for b in unit_blocks(&unit) {
        let p = block_paragraph(&unit, &b, Direction::Ltr, &plain);
        assert!(!p.justify && !p.hidden && p.drop_numeral.is_none());
        assert!(p.line_starts.is_empty());
    }
}

#[test]
fn one_verse_per_line_starts_every_verse_on_a_line_of_its_own() {
    let unit = john_3();
    let paragraphs = lay(
        &unit,
        Direction::Ltr,
        ReadingLayout::default(),
        420.0,
        false,
    );
    let flowing = prose(&paragraphs);
    let starts: Vec<u16> = (flowing.lines.iter())
        .filter_map(|l| verse_starting(flowing, l))
        .collect();
    assert_eq!(starts, [1], "in paragraphs only the chapter opens a line");

    let per_line = ReadingLayout {
        flow: TextFlow::VersePerLine,
        ..ReadingLayout::default()
    };
    let blocks = lay(&unit, Direction::Ltr, per_line, 420.0, false);
    let block = prose(&blocks);
    let starts: Vec<u16> = (block.lines.iter())
        .filter_map(|l| verse_starting(block, l))
        .collect();
    assert_eq!(starts, [1, 2, 3]);
    // Lines that continue a verse start at the margin too (no first-line indent).
    let left = block.lines[0].items[0].x;
    assert_eq!(left, 0.0);
    assert!(block.lines.len() >= flowing.lines.len());
}

#[test]
fn one_verse_per_line_works_with_verse_numbers_hidden() {
    let unit = john_3();
    let mut s = settings(ReadingLayout {
        flow: TextFlow::VersePerLine,
        ..ReadingLayout::default()
    });
    s.verse_numbers = VerseNumbers::Hidden;
    let b = unit_blocks(&unit)
        .into_iter()
        .find(|b| b.kind == BlockKind::Paragraph(ParaStyle::Prose))
        .unwrap();
    let laid = layout(
        &block_paragraph(&unit, &b, Direction::Ltr, &s),
        420.0,
        ShapeCache::shared(),
    );
    for (i, v) in VERSES.iter().enumerate().skip(1) {
        let start = unit.text.find(v).unwrap();
        let p = laid.from_unit(start).unwrap();
        assert!(
            laid.lines.iter().any(|l| l.bytes.start == p),
            "verse {} starts a line",
            i + 1
        );
    }
}

#[test]
fn hidden_section_headings_take_no_room_and_shown_ones_do() {
    let unit = john_3();
    let shown = lay(
        &unit,
        Direction::Ltr,
        ReadingLayout::default(),
        420.0,
        false,
    );
    let (b, heading) = &shown[0];
    assert_eq!(b.kind, BlockKind::Heading(1));
    assert!(heading.height > 10.0 && heading.text == HEADING);

    let hidden = ReadingLayout {
        section_headings: false,
        ..ReadingLayout::default()
    };
    let blocks = lay(&unit, Direction::Ltr, hidden, 420.0, false);
    let (b, heading) = &blocks[0];
    assert_eq!(b.kind, BlockKind::Heading(1), "the block is kept, empty");
    assert_eq!(heading.height, 0.0);
    assert!(heading.lines.is_empty() && heading.hit(10.0, 0.0).is_none());
    let s = settings(hidden);
    let p = block_paragraph(&unit, b, Direction::Ltr, &s);
    assert_eq!(estimate_height(&p, 420.0), 0.0);
    // The verses are untouched.
    assert_eq!(prose(&blocks).text, prose(&shown).text);
}

#[test]
fn a_commentarys_headings_stay_when_bible_section_headings_are_hidden() {
    let mut unit = john_3();
    unit.kind = UnitKind::Section;
    let hidden = ReadingLayout {
        section_headings: false,
        ..ReadingLayout::default()
    };
    let blocks = lay(&unit, Direction::Ltr, hidden, 420.0, false);
    assert!(blocks[0].1.height > 10.0);
}

#[test]
fn large_chapter_numbers_drop_beside_the_first_two_lines() {
    let unit = john_3();
    let inline = lay(
        &unit,
        Direction::Ltr,
        ReadingLayout::default(),
        420.0,
        false,
    );
    let inline = prose(&inline);
    let dropped = ReadingLayout {
        large_chapter_numbers: true,
        ..ReadingLayout::default()
    };
    let blocks = lay(&unit, Direction::Ltr, dropped, 420.0, false);
    let block = prose(&blocks);
    let numeral = &block.lines[0].items[0];
    assert_eq!(block.text.get(numeral.bytes.clone()), Some("3"));
    assert_eq!(numeral.x, 0.0, "the numeral stands at the margin");
    assert!(numeral.style.size > 2.5 * TextSettings::default().size);
    // Its baseline is the second line's, and the first two lines make room beside it.
    let drawn_at = block.lines[0].baseline - numeral.style.raise;
    assert!((drawn_at - block.lines[1].baseline).abs() < 0.5);
    let beside = numeral.x + numeral.width;
    for line in &block.lines[..2] {
        let text_left = line.items[1..].iter().map(|i| i.x).fold(f32::MAX, f32::min);
        assert!(text_left >= beside, "{text_left} beside {beside}");
    }
    assert!(
        block.lines[2].items[0].x < beside,
        "the third line is full width"
    );
    // Line heights are the text's, not the numeral's.
    let h = |l: &LaidLine| l.bottom - l.top;
    assert!((h(&block.lines[0]) - h(&block.lines[2])).abs() < 0.5);
    assert!(
        h(&block.lines[0]) < h(&inline.lines[0]),
        "nor the inline number's"
    );
    // The numeral hit-tests as the chapter's first verse.
    let hit = block
        .hit(
            numeral.x + numeral.width / 2.0,
            block.lines[0].baseline - 2.0,
        )
        .unwrap();
    assert!(matches!(
        hit.source,
        UnitPos::Object {
            object: InlineObject::VerseNumber(v),
            ..
        } if v == Verse::new(Book::John, 3, 1)
    ));
}

#[test]
fn a_one_line_paragraph_grows_to_hold_its_drop_numeral() {
    let unit = john_3();
    let dropped = ReadingLayout {
        large_chapter_numbers: true,
        ..ReadingLayout::default()
    };
    let blocks = lay(&unit, Direction::Ltr, dropped, 4000.0, false);
    let block = prose(&blocks);
    assert_eq!(block.lines.len(), 1);
    let line = &block.lines[0];
    assert!(block.height >= line.bottom + (line.bottom - line.top) - 0.5);
}

#[test]
fn justified_lines_fill_the_column_except_the_last() {
    let unit = john_3();
    let ragged = lay(
        &unit,
        Direction::Ltr,
        ReadingLayout::default(),
        300.0,
        false,
    );
    let ragged = prose(&ragged);
    let justified = ReadingLayout {
        justified: true,
        ..ReadingLayout::default()
    };
    let blocks = lay(&unit, Direction::Ltr, justified, 300.0, false);
    let block = prose(&blocks);
    assert_eq!(
        block.lines.len(),
        ragged.lines.len(),
        "only spacing changes"
    );
    let (last, full) = block.lines.split_last().unwrap();
    for line in full {
        let (_, right) = edges(block, line);
        assert!((right - 300.0).abs() < 0.5, "a full line ends at {right}");
    }
    assert!(edges(block, last).1 < 290.0, "the last line stays ragged");
    // Ragged lines mostly do not reach the margin.
    assert!(
        ragged.lines[..ragged.lines.len() - 1]
            .iter()
            .any(|l| edges(ragged, l).1 < 290.0)
    );
}

/// The clusters of a justified line leave no gaps: a stretched space's cluster takes its
/// room, so a verse's highlight and selection run unbroken across the words, and a click
/// between words lands on the space.
fn assert_clusters_abut(block: &LaidBlock, what: &str) {
    for (i, line) in block.lines.iter().enumerate() {
        let mut spans: Vec<(f32, f32)> = (line.items.iter())
            .flat_map(|it| it.clusters().map(|(_, x0, x1)| (x0, x1)))
            .collect();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        for pair in spans.windows(2) {
            assert!(
                (pair[1].0 - pair[0].1).abs() < 0.5,
                "{what}: line {i} has a gap from {} to {}",
                pair[0].1,
                pair[1].0
            );
        }
    }
}

#[test]
fn justified_clusters_cover_the_stretched_spaces() {
    let justified = ReadingLayout {
        justified: true,
        ..ReadingLayout::default()
    };
    let blocks = lay(&john_3(), Direction::Ltr, justified, 300.0, false);
    let block = prose(&blocks);
    assert_clusters_abut(block, "English");
    // A click in a stretched space is on that space.
    let line = &block.lines[0];
    let (word, next) = (&line.items[1], &line.items[2]);
    let gap = (word.x + word.width + next.x) / 2.0;
    assert!(
        next.x - (word.x + word.width) > 1.0,
        "the line is stretched"
    );
    let at = block.caret_at(gap, (line.top + line.bottom) / 2.0);
    assert!(at >= word.bytes.start && at <= next.bytes.start, "{at}");

    let blocks = lay(&hebrew_genesis_1(), Direction::Rtl, justified, 260.0, false);
    assert_clusters_abut(prose(&blocks), "Hebrew");
}

#[test]
fn justified_verse_per_line_leaves_each_verses_last_line_ragged() {
    let unit = john_3();
    let options = ReadingLayout {
        justified: true,
        flow: TextFlow::VersePerLine,
        ..ReadingLayout::default()
    };
    let blocks = lay(&unit, Direction::Ltr, options, 300.0, false);
    let block = prose(&blocks);
    for (i, line) in block.lines.iter().enumerate() {
        let next_starts_verse = block
            .lines
            .get(i + 1)
            .is_some_and(|n| verse_starting(block, n).is_some());
        if next_starts_verse {
            assert!(edges(block, line).1 < 299.0, "line {i} is a verse's last");
        }
    }
}

#[test]
fn justified_english_is_hyphenated_to_fit_and_reads_the_same() {
    let unit = john_3();
    let justified = ReadingLayout {
        justified: true,
        ..ReadingLayout::default()
    };
    // Narrow enough that long words (Pharisees, Nicodemus) land at line ends.
    let mut hyphenated = 0;
    for width in (110..=200).step_by(6) {
        let w = width as f32;
        let blocks = lay(&unit, Direction::Ltr, justified, w, true);
        let block = prose(&blocks);
        hyphenated += hyphens(block);
        // Every byte of the text is still on exactly one line, in order.
        let mut at = 0;
        for line in &block.lines {
            assert_eq!(line.bytes.start, at);
            at = line.bytes.end;
        }
        assert_eq!(at, block.text.len());
        for line in &block.lines {
            let Some(h) = line.items.iter().find(|i| i.bytes.is_empty()) else {
                continue;
            };
            // The hyphen ends its line, after a word's head of at least two letters.
            assert_eq!(line.items.last().unwrap().bytes, h.bytes);
            let head: String = block.text[line.bytes.clone()]
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_alphabetic())
                .collect();
            assert!(head.len() >= 2, "{head:?}");
            assert!(h.clusters().next().is_none(), "the hyphen is not text");
            assert!(h.shaped.glyphs.len() == 1);
        }
    }
    assert!(hyphenated > 0, "no word was hyphenated");
    // Without hyphenation, no hyphen is drawn.
    let blocks = lay(&unit, Direction::Ltr, justified, 140.0, false);
    assert_eq!(hyphens(prose(&blocks)), 0);
}

#[test]
fn hebrew_stays_right_to_left_justified_and_is_never_hyphenated() {
    let unit = hebrew_genesis_1();
    let options = ReadingLayout {
        justified: true,
        large_chapter_numbers: true,
        flow: TextFlow::VersePerLine,
        ..ReadingLayout::default()
    };
    for width in (140..=320).step_by(20) {
        let w = width as f32;
        // Even if asked to hyphenate, Hebrew words are never split.
        let blocks = lay(&unit, Direction::Rtl, options, w, true);
        let block = prose(&blocks);
        assert!(block.rtl);
        assert_eq!(hyphens(block), 0, "a Hebrew word was hyphenated at {w}");
        // The drop numeral stands on the right, the text's start side.
        let numeral = block.lines[0]
            .items
            .iter()
            .find(|i| block.text.get(i.bytes.clone()) == Some("1"))
            .unwrap();
        assert!((numeral.x + numeral.width - w).abs() < 0.5);
        for (i, line) in block.lines.iter().enumerate() {
            assert!(!line.items.is_empty(), "line {i} at {w} is empty");
            let (left, right) = edges(block, line);
            assert!(right <= w + 0.5, "line {i} overflows: {right}");
            // Visual order runs right to left: the logically first item is rightmost.
            let first = line.items.iter().min_by_key(|it| it.bytes.start).unwrap();
            assert!(
                line.items
                    .iter()
                    .all(|it| it.bytes.start == first.bytes.start || it.x <= first.x),
                "line {i} reads left to right"
            );
            assert!(left >= -0.5);
        }
    }
    // Hebrew text is drawn in the Hebrew face, as before.
    let blocks = lay(&unit, Direction::Rtl, options, 300.0, true);
    assert!(
        prose(&blocks)
            .lines
            .iter()
            .flat_map(|l| &l.items)
            .any(|i| i.style.face == FaceId::Hebrew)
    );
}

#[test]
fn a_hebrew_word_in_english_text_is_never_hyphenated() {
    let text = "the word בְּרֵאשִׁ֖ית־בָּרָ֣א־אֱלֹהִ֑ים־הַשָּׁמַ֖יִם is Hebrew and Nicodemus is not";
    let mut para = snippet_paragraph(text, ColorRole::Ink, false, &TextSettings::default());
    para.justify = true;
    para.hyphenate = true;
    for width in (60..=220).step_by(4) {
        let laid = layout(&para, width as f32, ShapeCache::shared());
        for line in &laid.lines {
            if line.items.iter().any(|i| i.bytes.is_empty()) {
                let before: String = laid.text[..line.bytes.end]
                    .chars()
                    .rev()
                    .take_while(|c| !c.is_whitespace())
                    .collect();
                assert!(
                    before.chars().all(|c| c.is_ascii_alphabetic()),
                    "hyphenated {before:?}"
                );
            }
        }
    }
}

#[test]
fn reading_layout_changes_the_layout_key_except_the_column_width() {
    let base = TextSettings::default();
    let key = |f: &dyn Fn(&mut ReadingLayout)| {
        let mut s = base;
        f(&mut s.layout);
        s.key()
    };
    assert_ne!(key(&|l| l.flow = TextFlow::VersePerLine), base.key());
    assert_ne!(key(&|l| l.large_chapter_numbers = true), base.key());
    assert_ne!(key(&|l| l.section_headings = false), base.key());
    assert_ne!(key(&|l| l.justified = true), base.key());
    // A column width changes the width, which keys the height index on its own.
    assert_eq!(key(&|l| l.column = ColumnWidth::Wide), base.key());
}

/// The drop numeral of `block` (its text is `numeral`) hit-tests and takes the caret as the
/// first verse `verse` over all of it: its upper half, beside the first line, and its lower
/// half, beside the second line or below a one-line paragraph.
fn assert_drop_numeral_hits(block: &LaidBlock, numeral: &str, verse: Verse) {
    let item = block.lines[0]
        .items
        .iter()
        .find(|i| block.text.get(i.bytes.clone()) == Some(numeral))
        .unwrap();
    let x = item.x + item.width / 2.0;
    let drawn_baseline = block.lines[0].baseline - item.style.raise;
    let first = &block.lines[0];
    assert!(
        drawn_baseline - 2.0 >= first.bottom,
        "its lower half is below the first line"
    );
    for y in [first.baseline - 2.0, drawn_baseline - 2.0] {
        let hit = block.hit(x, y).unwrap_or_else(|| panic!("no hit at {y}"));
        assert_eq!(hit.bytes, item.bytes, "at {y}");
        assert!(
            matches!(
                hit.source,
                UnitPos::Object {
                    object: InlineObject::VerseNumber(v),
                    ..
                } if v == verse
            ),
            "at {y}: {:?}",
            hit.source
        );
        let caret = block.caret_at(x, y);
        assert!(
            caret == item.bytes.start || caret == item.bytes.end,
            "caret {caret} at {y}"
        );
    }
}

#[test]
fn a_drop_numeral_hit_tests_over_its_lower_half() {
    let dropped = ReadingLayout {
        large_chapter_numbers: true,
        ..ReadingLayout::default()
    };
    let john_3_1 = Verse::new(Book::John, 3, 1);
    let blocks = lay(&john_3(), Direction::Ltr, dropped, 420.0, false);
    let block = prose(&blocks);
    assert!(block.lines.len() > 2);
    assert_drop_numeral_hits(block, "3", john_3_1);

    // Right to left: the numeral stands on the right.
    let blocks = lay(&hebrew_genesis_1(), Direction::Rtl, dropped, 300.0, false);
    let block = prose(&blocks);
    assert!(block.lines.len() > 2);
    assert_drop_numeral_hits(block, "1", Verse::new(Book::Genesis, 1, 1));

    // A one-line paragraph: the numeral's lower half is below its only line.
    let blocks = lay(&john_3(), Direction::Ltr, dropped, 4000.0, false);
    let block = prose(&blocks);
    assert_eq!(block.lines.len(), 1);
    assert_drop_numeral_hits(block, "3", john_3_1);
    // Beside the numeral, below the line, there is nothing.
    let line = &block.lines[0];
    assert!(block.hit(1000.0, line.bottom + 2.0).is_none());
}

/// Neighbouring items of `block`'s lines with room added between them by justification: the
/// left one, the right one and the space cluster that takes the room (its bytes and x).
fn stretched_spaces(block: &LaidBlock) -> Vec<(usize, Range<usize>, f32, f32)> {
    let mut found = Vec::new();
    for (li, line) in block.lines.iter().enumerate() {
        for pair in line.items.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            if b.x - (a.x + a.width) < 1.0 {
                continue;
            }
            // Left to right, the room follows the left item's space; right to left, it is
            // before (left of) the right item's.
            let space = if block.rtl {
                b.clusters().min_by(|p, q| p.1.total_cmp(&q.1))
            } else {
                a.clusters().max_by(|p, q| p.2.total_cmp(&q.2))
            };
            let (bytes, x0, x1) = space.unwrap();
            assert_eq!(&block.text[bytes.clone()], " ", "line {li}");
            found.push((li, bytes, x0, x1));
        }
    }
    found
}

#[test]
fn a_stretched_space_hit_tests_and_takes_the_caret_over_its_room() {
    let justified = ReadingLayout {
        justified: true,
        ..ReadingLayout::default()
    };
    let english = lay(&john_3(), Direction::Ltr, justified, 300.0, false);
    let hebrew = lay(&hebrew_genesis_1(), Direction::Rtl, justified, 260.0, false);
    for block in [prose(&english), prose(&hebrew)] {
        let spaces = stretched_spaces(block);
        assert!(spaces.len() > 3, "the lines are stretched");
        for (li, bytes, x0, x1) in spaces {
            let line = &block.lines[li];
            let y = (line.top + line.text_bottom) / 2.0;
            // Anywhere in the room, including just inside its far edge, is the space.
            for x in [x0 + 0.25, (x0 + x1) / 2.0, x1 - 0.25] {
                let hit = block.hit(x, y).unwrap_or_else(|| panic!("no hit at {x}"));
                assert_eq!(hit.bytes, bytes, "line {li} at {x}");
            }
            // The caret goes to the space's nearer edge: its start on the side its word is.
            let mid = (x0 + x1) / 2.0;
            let (left, right) = if block.rtl {
                (bytes.end, bytes.start)
            } else {
                (bytes.start, bytes.end)
            };
            assert_eq!(block.caret_at(x0 + 0.25, y), left, "line {li}");
            assert_eq!(block.caret_at(mid - 0.25, y), left, "line {li}");
            assert_eq!(block.caret_at(mid + 0.25, y), right, "line {li}");
            assert_eq!(block.caret_at(x1 - 0.25, y), right, "line {li}");
        }
    }
}

/// English hyphenation points of the word around paragraph byte `at`, as paragraph bytes.
fn hyphenation_points(text: &str, at: usize) -> Vec<usize> {
    let start = at
        - text[..at]
            .bytes()
            .rev()
            .take_while(u8::is_ascii_alphabetic)
            .count();
    let end = at
        + text[at..]
            .bytes()
            .take_while(u8::is_ascii_alphabetic)
            .count();
    hypher::hyphenate(&text[start..end], hypher::Lang::English)
        .scan(start, |p, syllable| {
            *p += syllable.len();
            Some(*p)
        })
        .collect()
}

#[test]
fn a_word_wider_than_a_narrow_column_is_hyphenated_line_by_line() {
    let text = "Nevertheless incomprehensibilities and unrighteousness are seen";
    let mut para = snippet_paragraph(text, ColorRole::Ink, false, &TextSettings::default());
    para.justify = true;
    let wide = layout(&para, 1000.0, ShapeCache::shared());
    let longest = wide.lines[0]
        .items
        .iter()
        .map(|i| i.width)
        .fold(0.0, f32::max);
    for width in [60.0f32, 70.0, 80.0, 90.0] {
        assert!(
            longest > width,
            "a word is wider than the column at {width}"
        );
        para.hyphenate = false;
        assert_eq!(hyphens(&layout(&para, width, ShapeCache::shared())), 0);

        para.hyphenate = true;
        let block = layout(&para, width, ShapeCache::shared());
        assert!(
            hyphens(&block) >= 3,
            "{width}: long words broken more than once"
        );
        let mut at = 0;
        for (i, line) in block.lines.iter().enumerate() {
            assert_eq!(line.bytes.start, at, "{width}: line {i} continues the text");
            at = line.bytes.end;
            let (left, right) = edges(&block, line);
            assert!(
                left >= -0.5 && right <= width + 0.5,
                "{width}: line {i} {:?} runs {left}..{right}",
                &block.text[line.bytes.clone()]
            );
            // A line that ends inside a word ends at its hyphenation point, in a drawn hyphen.
            let inside = |p: usize| {
                block
                    .text
                    .as_bytes()
                    .get(p)
                    .is_some_and(u8::is_ascii_alphabetic)
            };
            if at < block.text.len() && inside(at - 1) && inside(at) {
                assert!(
                    line.items.last().unwrap().bytes.is_empty(),
                    "{width}: line {i}"
                );
                assert!(
                    hyphenation_points(&block.text, at).contains(&at),
                    "{width}: line {i}"
                );
            }
        }
        assert_eq!(at, block.text.len());
    }
}

#[test]
fn a_tail_is_split_at_its_whole_words_hyphenation_points() {
    let word = "incomprehensibilities";
    let mut para = snippet_paragraph(word, ColorRole::Ink, false, &TextSettings::default());
    para.justify = true;
    para.hyphenate = true;
    let points = hyphenation_points(word, 0);
    // From the narrowest column its every syllable and a hyphen fit.
    for width in (45..=120).step_by(5) {
        let block = layout(&para, width as f32, ShapeCache::shared());
        assert!(block.lines.len() > 1, "{width}");
        for line in &block.lines[..block.lines.len() - 1] {
            assert!(
                points.contains(&line.bytes.end),
                "{width}: broken at {} of {word}",
                line.bytes.end
            );
        }
    }
}

/// `unit` with every word of its verses tagged, as an interlinear Bible tags them.
fn tag_words(mut unit: TextUnit) -> TextUnit {
    let verses: Vec<Range<usize>> = (unit.spans.iter())
        .filter(|s| matches!(s.kind, SpanKind::Verse { .. }))
        .map(|s| s.range())
        .collect();
    for verse in verses {
        let mut at = verse.start;
        for word in unit.text[verse.clone()].split(' ') {
            if !word.is_empty() {
                unit.spans.push(span(
                    at..at + word.len(),
                    SpanKind::TaggedWord {
                        parts: vec![MorphemePart::new("G1".parse().unwrap())],
                    },
                ));
            }
            at += word.len() + 1;
        }
    }
    unit.spans
        .sort_by_key(|s| (s.start, std::cmp::Reverse(s.end)));
    unit
}

/// [`lay`] with `rows` Interlinear lines (#143) under every Tagged word.
fn lay_interlinear(
    unit: &TextUnit,
    direction: Direction,
    layout_options: ReadingLayout,
    width: f32,
    rows: usize,
) -> Vec<(BlockSource, LaidBlock)> {
    let s = settings(layout_options);
    unit_blocks(unit)
        .into_iter()
        .map(|b| {
            let mut p = block_paragraph(unit, &b, direction, &s);
            add_interlinear(&mut p, unit, &b, &s, rows, &mut |written, _| {
                vec![written.to_uppercase(), "G1".into(), "N-NSM".into()]
            });
            let l = layout(&p, width, ShapeCache::shared());
            (b, l)
        })
        .collect()
}

/// The drop numeral of `block` (its text is `numeral`) spans its first two lines as drawn:
/// the top of its figures is level with the top of figures as large as the first line's
/// text, its baseline on the second line's (below a one-line paragraph, a line's height
/// down). Returns its size.
fn assert_drop_numeral_spans_two_lines(block: &LaidBlock, numeral: &str) -> f32 {
    let first = &block.lines[0];
    let item = (first.items.iter())
        .find(|i| block.text.get(i.bytes.clone()) == Some(numeral))
        .unwrap();
    let baseline = first.baseline - item.style.raise;
    let second = block
        .lines
        .get(1)
        .map_or(first.baseline + (first.bottom - first.top), |l| l.baseline);
    assert!((baseline - second).abs() < 0.5, "{baseline} on {second}");
    let figures = |size: f32| {
        let style = ShapeStyle::new(item.style.face, size, false, item.rtl);
        let shaped = ShapeCache::shared().shape(numeral, style).unwrap();
        ink_top(&shaped, style).unwrap()
    };
    let top = baseline - figures(item.style.size);
    let text = (first.items.iter())
        .filter(|i| i.run != item.run)
        .map(|i| i.style.size)
        .fold(0.0, f32::max);
    let level = first.baseline - figures(text);
    assert!(
        (top - level).abs() < 1.0,
        "its top {top}, figures' {level}; size {} lines {}",
        item.style.size,
        block.lines.len()
    );
    assert!(
        top > first.top && top < first.baseline - 0.5 * text,
        "its top {top} is in the first line's text, {}..{}",
        first.top,
        first.baseline
    );
    item.style.size
}

#[test]
fn a_drop_numeral_spans_its_first_two_lines_with_interlinear_lines() {
    let dropped = ReadingLayout {
        large_chapter_numbers: true,
        ..ReadingLayout::default()
    };
    let john = tag_words(john_3());
    let genesis = tag_words(hebrew_genesis_1());
    let cases = [
        (&john, Direction::Ltr, "3", 420.0),
        (&genesis, Direction::Rtl, "1", 300.0),
        // One line: the numeral's lower half is below it.
        (&john, Direction::Ltr, "3", 4000.0),
    ];
    for (unit, direction, numeral, width) in cases {
        let plain = lay(unit, direction, dropped, width, false);
        let plain = prose(&plain);
        let plain_size = assert_drop_numeral_spans_two_lines(plain, numeral);
        let mut sizes = vec![plain_size];
        for rows in 1..=3 {
            let blocks = lay_interlinear(unit, direction, dropped, width, rows);
            let block = prose(&blocks);
            assert!(!block.cells.is_empty(), "the lines are shown");
            assert_eq!(block.lines.len() == 1, width > 1000.0);
            let size = assert_drop_numeral_spans_two_lines(block, numeral);
            // The lines under the first line's words stand between it and the next: the
            // numeral grows to span them.
            assert!(
                size > sizes.last().unwrap() + 2.0,
                "{rows} rows at {width}: {size} after {sizes:?}"
            );
            sizes.push(size);
            assert_drop_numeral_hits(
                block,
                numeral,
                if direction == Direction::Rtl {
                    Verse::new(Book::Genesis, 1, 1)
                } else {
                    Verse::new(Book::John, 3, 1)
                },
            );
            // Nothing beside it overlaps it.
            let n = (block.lines[0].items.iter())
                .find(|i| block.text.get(i.bytes.clone()) == Some(numeral))
                .unwrap();
            let (left, right) = (n.x, n.x + n.width);
            for line in &block.lines[..block.lines.len().min(2)] {
                for it in line.items.iter().filter(|i| i.run != n.run) {
                    assert!(it.x + it.width <= left + 0.5 || it.x >= right - 0.5);
                }
            }
            for c in block.cells.iter().filter(|c| c.line < 2) {
                assert!(c.x + c.width <= left + 0.5 || c.x >= right - 0.5, "{c:?}");
            }
        }
    }
}

#[test]
fn a_drop_numeral_is_marked_over_its_whole_height() {
    let dropped = ReadingLayout {
        large_chapter_numbers: true,
        ..ReadingLayout::default()
    };
    let blocks = lay(&john_3(), Direction::Ltr, dropped, 420.0, false);
    let block = prose(&blocks);
    let first = &block.lines[0];
    let numeral = &first.items[0];
    assert_eq!(block.text.get(numeral.bytes.clone()), Some("3"));
    let (top, bottom) = block.range_band(0, numeral);
    assert!(top <= first.top);
    assert!(
        bottom >= first.baseline - numeral.style.raise,
        "below its baseline"
    );
    // The text beside it is marked in its own line's band.
    let text = &first.items[1];
    assert_eq!(block.range_band(0, text), (first.top, first.text_bottom));
}
