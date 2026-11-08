//! Lines of mixed-direction text and how they break (#260): carets past an item's visual edge,
//! a Note line's own direction, UAX #9 rule L1 on wrapped lines, mandatory breaks, words
//! wider than the column and soft hyphens.

use obs_fonts::ShapeCache;
use unicode_bidi::BidiInfo;

use super::*;

fn snippet(text: &str, rtl: bool, width: f32) -> LaidBlock {
    let para = snippet_paragraph(text, ColorRole::Ink, rtl, &TextSettings::default());
    layout(&para, width, ShapeCache::shared())
}

fn line_texts(block: &LaidBlock) -> Vec<&str> {
    block
        .lines
        .iter()
        .map(|l| &block.text[l.bytes.clone()])
        .collect()
}

/// Visual left and right of a line's items.
fn extent(line: &LaidLine) -> (f32, f32) {
    let left = line.items.iter().map(|i| i.x).fold(f32::MAX, f32::min);
    let right = line
        .items
        .iter()
        .map(|i| i.x + i.width)
        .fold(f32::MIN, f32::max);
    (left, right)
}

/// AUD-LAYOUT-TEXT-3: dragging across the visual edge of a line that ends (visually) in a run
/// of the other direction stays at the same logical boundary instead of jumping across the
/// word.
#[test]
fn a_caret_past_a_lines_visual_edge_keeps_the_edge_items_boundary() {
    let text = "abc אבג";
    let hebrew = find_in(text, "אבג");
    let block = snippet(text, false, 600.0);
    let line = &block.lines[0];
    let y = line.baseline - 2.0;
    let (left, right) = extent(line);
    let inside = block.caret_at(right - 0.01, y);
    let outside = block.caret_at(right + 0.01, y);
    assert_eq!(
        inside, hebrew.start,
        "the Hebrew word's right edge is its start"
    );
    assert_eq!(outside, inside, "inside={inside} outside={outside}");
    // The other edge is the Latin word's start, and the same in an RTL paragraph.
    assert_eq!(
        block.caret_at(left - 5.0, y),
        block.caret_at(left + 0.01, y)
    );
    let block = snippet("אבג abc", true, 600.0);
    let line = &block.lines[0];
    let (left, right) = extent(line);
    let y = line.baseline - 2.0;
    assert_eq!(
        block.caret_at(left - 0.01, y),
        block.caret_at(left + 0.01, y)
    );
    assert_eq!(
        block.caret_at(right + 5.0, y),
        block.caret_at(right - 0.01, y)
    );
    assert_eq!(block.caret_at(right + 5.0, y), 0);
}

/// AUD-LAYOUT-TEXT-4: a Note line (formatted or not) takes its base direction from its first
/// strong letter, as its export does with `dir="auto"`.
#[test]
fn a_formatted_snippet_reads_right_to_left_when_its_first_strong_letter_does() {
    let s = TextSettings::default();
    let hebrew = formatted_snippet_paragraph("שלום!", ColorRole::Ink, &[], 1.0, &s);
    assert!(hebrew.rtl);
    let bold = SnippetStyle {
        bold: true,
        ..SnippetStyle::default()
    };
    let mixed = formatted_snippet_paragraph("1 שלום Gen", ColorRole::Ink, &[(0..1, bold)], 1.0, &s);
    assert!(mixed.rtl);
    let english = formatted_snippet_paragraph("Peace שלום!", ColorRole::Ink, &[], 1.0, &s);
    assert!(!english.rtl);
    // Laid out, the RTL line starts at the right and its "!" sits at its left end.
    let block = layout(&hebrew, 400.0, ShapeCache::shared());
    let line = &block.lines[0];
    let (_, right) = extent(line);
    assert!((right - 400.0).abs() < 0.5, "right edge {right}");
    let first = &line.items[0];
    let leftmost = first.clusters().next().unwrap();
    assert_eq!(&block.text[leftmost.0], "!");
}

/// AUD-LAYOUT-TEXT-5: on a wrapped line, trailing whitespace takes the paragraph's level
/// (UAX #9 rule L1) before reordering, so the space after a Hebrew word that ends an LTR line
/// sits at the line's right end, not between the English and the Hebrew.
#[test]
fn trailing_space_on_a_wrapped_line_takes_the_paragraph_level() {
    let text = "abc אבג דהו זחט יכל";
    let block = snippet(text, false, 80.0);
    assert!(block.lines.len() > 1, "{:?}", line_texts(&block));
    let bidi = BidiInfo::new(text, Some(unicode_bidi::Level::ltr()));
    for line in &block.lines {
        let (levels, runs) = bidi.visual_runs(&bidi.paragraphs[0], line.bytes.clone());
        // Expected visual order of characters, from unicode-bidi's own line algorithm.
        let mut want = Vec::new();
        for run in runs {
            let mut chars: Vec<usize> = text[run.clone()]
                .char_indices()
                .map(|(i, _)| run.start + i)
                .collect();
            if levels[run.start].is_rtl() {
                chars.reverse();
            }
            want.extend(chars);
        }
        let got: Vec<usize> = line
            .items
            .iter()
            .flat_map(|it| it.clusters().map(|(b, _, _)| b.start))
            .collect();
        assert_eq!(got, want, "line {:?}", &text[line.bytes.clone()]);
    }
    let first = &block.lines[0];
    assert_eq!(&text[first.bytes.clone()], "abc אבג ");
    let last = first.items.last().unwrap().clusters().last().unwrap();
    assert_eq!(last.0, 10..11, "the trailing space is at the right end");
}

/// AUD-LAYOUT-TEXT-6: a mandatory break (a newline, or a line or paragraph separator) ends the
/// line whatever the width.
#[test]
fn a_mandatory_break_ends_the_line_at_any_width() {
    let block = snippet("first\nsecond", false, 600.0);
    assert_eq!(line_texts(&block), ["first\n", "second"]);
    let block = snippet("one\u{2028}two\u{2029}three", false, 600.0);
    assert_eq!(line_texts(&block), ["one\u{2028}", "two\u{2029}", "three"]);
    let block = snippet("שלום\nעולם", true, 600.0);
    assert_eq!(line_texts(&block), ["שלום\n", "עולם"]);
    // An empty line between two breaks is a line of its own.
    let block = snippet("a\n\nb", false, 600.0);
    assert_eq!(line_texts(&block), ["a\n", "\n", "b"]);
    // The break itself draws nothing.
    for line in &block.lines {
        for it in &line.items {
            assert_eq!(
                it.shaped.missing_glyphs(),
                0,
                "{:?}",
                &block.text[it.bytes.clone()]
            );
        }
    }
}

/// AUD-LAYOUT-TEXT-7: a word wider than the column wraps between grapheme clusters, so all of
/// it stays inside the column.
#[test]
fn a_word_wider_than_the_column_wraps_between_graphemes() {
    let text = "Mahershalalhashbaz";
    let block = snippet(text, false, 80.0);
    assert!(block.lines.len() > 1, "{:?}", line_texts(&block));
    for line in &block.lines {
        let (left, right) = extent(line);
        assert!(left >= -0.01 && right <= 80.01, "{left}..{right}");
    }
    assert_eq!(line_texts(&block).concat(), text);
    // With room for it, the same word stays whole, and so does a word that fits on the next
    // line.
    assert_eq!(snippet(text, false, 600.0).lines.len(), 1);
    let word = snippet(text, false, 600.0).lines[0].items[0].width;
    let block = snippet("In Mahershalalhashbaz the", false, word + 4.0);
    assert_eq!(line_texts(&block), ["In ", "Mahershalalhashbaz ", "the"]);
    // Hebrew with points never splits a letter from its marks.
    let hebrew = "בְּרֵאשִׁ֖יתבָּרָ֣אאֱלֹהִ֑ים";
    let block = snippet(hebrew, true, 60.0);
    assert!(block.lines.len() > 1);
    for line in &block.lines {
        let (left, right) = extent(line);
        assert!(left >= -0.01 && right <= 60.01, "{left}..{right}");
        let next = hebrew[line.bytes.end..].chars().next();
        assert!(
            next.is_none_or(|c| !('\u{0591}'..='\u{05C7}').contains(&c)),
            "line ends before a mark: {:?}",
            &hebrew[line.bytes.clone()]
        );
    }
    assert_eq!(line_texts(&block).concat(), hebrew);
}

/// AUD-LAYOUT-TEXT-8: a soft hyphen draws nothing, unless the line breaks at it; then it is a
/// visible hyphen, and it counts toward the line's width.
#[test]
fn a_soft_hyphen_shows_a_hyphen_only_where_the_line_breaks() {
    let text = "extra\u{00ad}ordinary";
    let whole = snippet(text, false, 600.0);
    assert_eq!(whole.lines.len(), 1);
    let width = |b: &LaidBlock| b.lines[0].items.iter().map(|i| i.width).sum::<f32>();
    let no_hyphen = snippet("extraordinary", false, 600.0);
    assert!((width(&whole) - width(&no_hyphen)).abs() < 0.01);

    // Room for "extra-" or "ordinary" but not both.
    let hyphen = snippet("extra-", false, 600.0);
    let column = width(&hyphen).max(width(&snippet("ordinary", false, 600.0))) + 1.0;
    let block = snippet(text, false, column);
    assert_eq!(line_texts(&block), ["extra\u{00ad}", "ordinary"]);
    let line = &block.lines[0];
    let glyphs = |l: &LaidLine| -> Vec<u32> {
        l.items
            .iter()
            .flat_map(|i| i.shaped.glyphs.iter().map(|g| g.id))
            .collect()
    };
    assert_eq!(glyphs(line), glyphs(&hyphen.lines[0]));
    assert!((width(&block) - width(&hyphen)).abs() < 0.01);
    // The hyphen still maps back to the soft hyphen's bytes.
    let (_, right) = extent(line);
    let hit = block.hit(right - 0.5, line.baseline - 2.0).unwrap();
    assert_eq!(hit.bytes, 5..7);
}

fn find_in(text: &str, needle: &str) -> std::ops::Range<usize> {
    let s = text.find(needle).unwrap();
    s..s + needle.len()
}
