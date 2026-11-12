use obs_fonts::{FaceId, ShapeCache};
use obs_model::{
    Direction, MorphemePart, NoteKind, ParaStyle, Span, SpanKind, TextUnit, UnitId, UnitKind,
};
use obs_scripture::{Book, Passage, PassageList, Verse};
use serde_json::{Value, json};

use super::*;

fn span(range: std::ops::Range<usize>, kind: SpanKind) -> Span {
    Span {
        start: range.start as u32,
        end: range.end as u32,
        kind,
    }
}

fn find(text: &str, needle: &str) -> std::ops::Range<usize> {
    let s = text
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} not in text"));
    s..s + needle.len()
}

/// Hebrew Gen 1:1-2 (pointed, with cantillation), one paragraph, two verses and a note.
fn hebrew_unit() -> TextUnit {
    let v1 = "בְּרֵאשִׁ֖ית בָּרָ֣א אֱלֹהִ֑ים אֵ֥ת הַשָּׁמַ֖יִם וְאֵ֥ת הָאָֽרֶץ׃";
    let v2 = "וְהָאָ֗רֶץ הָיְתָ֥ה תֹ֙הוּ֙ וָבֹ֔הוּ";
    let text = format!("{v1} {v2}");
    let r1 = 0..v1.len();
    let r2 = v1.len() + 1..text.len();
    let mut spans = vec![
        span(0..text.len(), SpanKind::Chapter { chapter: 1 }),
        span(
            0..text.len(),
            SpanKind::Para {
                style: ParaStyle::Prose,
            },
        ),
        span(
            r1.clone(),
            SpanKind::Verse {
                verse: Verse::new(Book::Genesis, 1, 1),
                last: 1,
            },
        ),
        span(
            r2.clone(),
            SpanKind::Verse {
                verse: Verse::new(Book::Genesis, 1, 2),
                last: 2,
            },
        ),
        span(
            r1.end..r1.end,
            SpanKind::NoteRef {
                note: UnitId::note(1),
                note_kind: NoteKind::Footnote,
            },
        ),
    ];
    spans.sort_by_key(|s| (s.start, std::cmp::Reverse(s.end)));
    TextUnit {
        id: UnitId::book(Book::Genesis),
        kind: UnitKind::BibleBook,
        title: None,
        text,
        spans,
    }
}

/// KJV Ps 3 title and verses 1-2 plus Jn 3:16, with every styling span.
fn english_unit() -> TextUnit {
    let title = "A Psalm of David, when he fled from Absalom his son.";
    let v1 = "LORD, how are they increased that trouble me! many are they that rise up against me.";
    let v2 = "For God so loved the world, that he gave his only begotten Son.";
    let text = format!("{title}\n{v1}\n{v2}");
    let t = 0..title.len();
    let p1 = title.len() + 1..title.len() + 1 + v1.len();
    let p2 = p1.end + 1..text.len();
    let mut spans = vec![
        span(t.clone(), SpanKind::PsalmTitle),
        span(
            t.clone(),
            SpanKind::Verse {
                verse: Verse::new(Book::Psalms, 3, 0),
                last: 0,
            },
        ),
        span(
            p1.clone(),
            SpanKind::Para {
                style: ParaStyle::Poetry1,
            },
        ),
        span(
            p1.clone(),
            SpanKind::Verse {
                verse: Verse::new(Book::Psalms, 3, 1),
                last: 1,
            },
        ),
        span(p1.start..p1.start + 4, SpanKind::DivineName),
        span(
            find(&text, "are they increased").start..find(&text, "are they increased").start + 3,
            SpanKind::Added,
        ),
        span(
            p2.clone(),
            SpanKind::Para {
                style: ParaStyle::Prose,
            },
        ),
        span(
            p2.clone(),
            SpanKind::Verse {
                verse: Verse::new(Book::Psalms, 3, 2),
                last: 2,
            },
        ),
        span(find(&text, "God so"), SpanKind::Woj),
        span(
            find(&text, "God"),
            SpanKind::TaggedWord {
                parts: vec![MorphemePart::new("G2316".parse().unwrap())],
            },
        ),
        span(
            find(&text, "loved"),
            SpanKind::TaggedWord {
                parts: vec![MorphemePart::new("G25".parse().unwrap())],
            },
        ),
        span(
            find(&text, "world"),
            SpanKind::ScriptureRef {
                targets: PassageList::from(vec![Passage::verse(Verse::new(Book::Romans, 5, 8))]),
            },
        ),
        span(
            p2.end..p2.end,
            SpanKind::NoteRef {
                note: UnitId::note(2),
                note_kind: NoteKind::CrossReference,
            },
        ),
    ];
    spans.sort_by_key(|s| (s.start, std::cmp::Reverse(s.end)));
    TextUnit {
        id: UnitId::book(Book::Psalms),
        kind: UnitKind::BibleBook,
        title: None,
        text,
        spans,
    }
}

fn laid(
    unit: &TextUnit,
    direction: Direction,
    width: f32,
) -> Vec<(BlockSource, Paragraph, LaidBlock)> {
    // The goldens were recorded at 17 pt: they pin the layout, not the default Text size (#241).
    let settings = TextSettings {
        size: 17.0,
        ..TextSettings::default()
    };
    unit_blocks(unit)
        .into_iter()
        .map(|b| {
            let p = block_paragraph(unit, &b, direction, &settings);
            let l = layout(&p, width, ShapeCache::shared());
            (b, p, l)
        })
        .collect()
}

#[test]
fn strongs_links_in_lexicon_entries_are_links_but_tagged_bible_words_are_not() {
    let text = "Compare G26 for the noun.";
    let tagged = |kind| TextUnit {
        id: UnitId::new("G25").unwrap(),
        kind,
        title: None,
        text: text.into(),
        spans: vec![span(
            find(text, "G26"),
            SpanKind::TaggedWord {
                parts: vec![MorphemePart::new("G26".parse().unwrap())],
            },
        )],
    };
    let role = |unit: &TextUnit| {
        laid(unit, Direction::Ltr, 600.0)[0]
            .1
            .runs
            .iter()
            .find(|r| r.text == "G26")
            .unwrap()
            .style
            .role
    };
    assert_eq!(role(&tagged(UnitKind::Entry)), ColorRole::Link);
    assert_eq!(role(&tagged(UnitKind::BibleBook)), ColorRole::Ink);
}

#[test]
fn blocks_follow_structural_spans() {
    let unit = english_unit();
    let kinds: Vec<BlockKind> = unit_blocks(&unit).iter().map(|b| b.kind).collect();
    assert_eq!(
        kinds,
        [
            BlockKind::PsalmTitle,
            BlockKind::Paragraph(ParaStyle::Poetry1),
            BlockKind::Paragraph(ParaStyle::Prose)
        ]
    );
}

#[test]
fn spans_become_styled_runs() {
    let unit = english_unit();
    let blocks = laid(&unit, Direction::Ltr, 600.0);
    let p1 = &blocks[1].1;
    // Verse number object first, then "Lord" in small caps (first letter kept a capital).
    assert!(
        matches!(&p1.runs[0].source, RunSource::Object { object: InlineObject::VerseNumber(v), .. } if v.verse == 1)
    );
    assert_eq!(
        p1.runs[0].style.role,
        ColorRole::Accent,
        "verse 1 shows the chapter number"
    );
    assert_eq!(p1.runs[0].text, "3\u{a0}");
    assert_eq!(p1.runs[1].text, "Lord");
    assert!(p1.runs[1].style.small_caps);
    assert_eq!(
        p1.runs[1].source,
        RunSource::Text {
            unit_start: blocks[1].0.range.start
        }
    );
    let added = p1.runs.iter().find(|r| r.text == "are").unwrap();
    assert_eq!(added.style.face, FaceId::Italic);
    let p2 = &blocks[2].1;
    let god = p2.runs.iter().find(|r| r.text == "God").unwrap();
    assert_eq!(god.style.role, ColorRole::WordsOfJesus);
    let world = p2.runs.iter().find(|r| r.text == "world").unwrap();
    assert_eq!(world.style.role, ColorRole::Link);
    assert!(matches!(
        p2.runs.last().unwrap().source,
        RunSource::Object {
            object: InlineObject::NoteMarker { .. },
            ..
        }
    ));
    // The title slot has no verse number.
    assert!(
        blocks[0]
            .1
            .runs
            .iter()
            .all(|r| matches!(r.source, RunSource::Text { .. }))
    );

    let plain = TextSettings {
        red_letter: false,
        verse_numbers: VerseNumbers::Hidden,
        ..TextSettings::default()
    };
    let p2 = block_paragraph(&unit, &blocks[2].0, Direction::Ltr, &plain);
    assert!(
        p2.runs
            .iter()
            .all(|r| r.style.role != ColorRole::WordsOfJesus)
    );
    assert!(!p2.runs.iter().any(|r| matches!(
        r.source,
        RunSource::Object {
            object: InlineObject::VerseNumber(_),
            ..
        }
    )));
}

fn snapshot(block: &LaidBlock) -> Value {
    let lines: Vec<Value> = block
        .lines
        .iter()
        .map(|l| {
            let items: Vec<Value> = l
                .items
                .iter()
                .map(|it| {
                    json!({
                        "text": &block.text[it.bytes.clone()],
                        "x": (it.x * 100.0).round() / 100.0,
                        "width": (it.width * 100.0).round() / 100.0,
                        "rtl": it.rtl,
                        "face": format!("{:?}", it.style.face),
                        "glyph_x": it.shaped.glyphs.iter().map(|g| ((it.x + g.x) * 10.0).round() / 10.0).collect::<Vec<_>>(),
                    })
                })
                .collect();
            json!({ "top": l.top, "baseline": l.baseline, "bottom": l.bottom, "items": items })
        })
        .collect();
    json!({ "height": block.height, "lines": lines })
}

fn check_golden(name: &str, actual: &Value) {
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

#[test]
fn hebrew_paragraph_with_ltr_verse_numbers_orders_correctly() {
    let unit = hebrew_unit();
    let blocks = laid(&unit, Direction::Rtl, 420.0);
    assert_eq!(blocks.len(), 1);
    let block = &blocks[0].2;
    assert!(block.lines.len() >= 2);
    let first = &block.lines[0];
    // The chapter number for verse 1 is the rightmost item of the first line (RTL start)…
    let rightmost = first.items.last().unwrap();
    assert!(matches!(
        block.to_unit(rightmost.bytes.start),
        Some(UnitPos::Object { object: InlineObject::VerseNumber(v), .. }) if v.verse == 1
    ));
    assert!(
        !rightmost.rtl,
        "digits stay left-to-right inside the RTL line"
    );
    // …it ends at the right margin, less the prose first-line indent…
    let indent = blocks[0].1.indent;
    assert!((rightmost.x + rightmost.width - (block.width - indent)).abs() < 0.5);
    // …and logical order runs right to left: the first Hebrew word is right of the second.
    let x_of = |unit_byte: usize| {
        let p = block.from_unit(unit_byte).unwrap();
        first.items.iter().find(|i| i.bytes.contains(&p)).unwrap().x
    };
    let w1 = 0;
    let w2 = unit.text.find(' ').unwrap() + 1;
    assert!(x_of(w1) > x_of(w2));
    // Verse 2's number is a left-to-right island inside right-to-left text somewhere later.
    // The digit is its own LTR item; the narrow space after it takes the paragraph's direction.
    let digit = block
        .lines
        .iter()
        .flat_map(|l| &l.items)
        .find(|i| {
            matches!(block.to_unit(i.bytes.start), Some(UnitPos::Object { object: InlineObject::VerseNumber(v), .. }) if v.verse == 2)
                && block.text[i.bytes.clone()].starts_with('2')
        })
        .unwrap();
    assert!(!digit.rtl);
    check_golden("hebrew-gen-1-1", &snapshot(block));
}

#[test]
fn english_golden() {
    let unit = english_unit();
    let blocks = laid(&unit, Direction::Ltr, 360.0);
    let all: Vec<Value> = blocks.iter().map(|(_, _, l)| snapshot(l)).collect();
    check_golden("english-ps-3", &json!(all));
}

/// Hit-testing the centre of every cluster returns that cluster, traced to the right unit byte.
#[test]
fn every_cluster_hits_its_source_byte() {
    for (unit, direction) in [
        (english_unit(), Direction::Ltr),
        (hebrew_unit(), Direction::Rtl),
    ] {
        let mut checked = 0;
        for (_, _, block) in laid(&unit, direction, 300.0) {
            for line in &block.lines {
                let y = (line.top + line.bottom) / 2.0;
                for item in &line.items {
                    for (bytes, x0, x1) in item.clusters() {
                        if x1 - x0 < 0.01 {
                            continue;
                        }
                        let hit = block
                            .hit((x0 + x1) / 2.0, y)
                            .unwrap_or_else(|| panic!("no hit at {x0}..{x1}"));
                        assert_eq!(hit.bytes, bytes);
                        match block.to_unit(bytes.start).unwrap() {
                            UnitPos::Text(u) => {
                                assert_eq!(hit.source, UnitPos::Text(u));
                                let shown = &block.text[bytes.clone()];
                                let source = &unit.text[u..u + shown.len()];
                                assert_eq!(shown.to_lowercase(), source.to_lowercase(), "byte {u}");
                            }
                            UnitPos::Object { .. } => {
                                assert!(matches!(hit.source, UnitPos::Object { .. }))
                            }
                        }
                        checked += 1;
                    }
                }
            }
        }
        assert!(checked > 40);
    }
}

#[test]
fn carets_land_on_grapheme_boundaries() {
    let unit = hebrew_unit();
    let blocks = laid(&unit, Direction::Rtl, 420.0);
    let block = &blocks[0].2;
    let line = &block.lines[0];
    let y = line.baseline - 2.0;
    let mut x = 0.0;
    while x < block.width {
        let p = block.caret_at(x, y);
        assert!(block.text.is_char_boundary(p));
        // Never between a Hebrew letter and its points.
        if let Some(c) = block.text[p..].chars().next() {
            assert!(
                !('\u{0591}'..='\u{05C7}').contains(&c) || c == '\u{05BE}' || c == '\u{05C3}',
                "caret before a mark at {p}"
            );
        }
        x += 3.0;
    }
    // Beyond the right of an RTL line is its logical start (its verse number, at the unit's
    // first byte); beyond the left, its end. Either is where the caret is just inside the edge.
    let unit_byte = |p: usize| match block.to_unit(p).unwrap() {
        UnitPos::Text(b) => b,
        UnitPos::Object { anchor, .. } => anchor,
    };
    assert_eq!(
        unit_byte(block.caret_at(block.width + 50.0, y)),
        unit_byte(line.bytes.start)
    );
    assert_eq!(block.caret_at(-50.0, y), line.bytes.end);
    let left = line.items.iter().map(|i| i.x).fold(f32::MAX, f32::min);
    assert_eq!(block.caret_at(-50.0, y), block.caret_at(left + 0.01, y));
}

#[test]
fn span_index_finds_interactive_spans() {
    let unit = english_unit();
    let index = SpanIndex::new(&unit);
    let god = find(&unit.text, "God");
    let at = index.at(&unit, god.start + 1);
    assert_eq!(at.len(), 1);
    assert!(matches!(at[0].kind, SpanKind::TaggedWord { .. }));
    let world = find(&unit.text, "world");
    assert!(matches!(
        index.at(&unit, world.start).first().map(|s| &s.kind),
        Some(SpanKind::ScriptureRef { .. })
    ));
    assert!(
        index
            .at(&unit, find(&unit.text, "Absalom").start)
            .is_empty()
    );
}

#[test]
fn line_heights_use_the_tallest_run() {
    let s = TextSettings::default();
    let small = snippet_paragraph("small", ColorRole::Ink, false, &s);
    let mut big = small.clone();
    big.runs[0].style.size *= 2.0;
    let a = layout(&small, 400.0, ShapeCache::shared());
    let b = layout(&big, 400.0, ShapeCache::shared());
    assert!(b.lines[0].bottom - b.lines[0].top > 1.9 * (a.lines[0].bottom - a.lines[0].top));
    // Hebrew is drawn larger but its line metrics are tightened.
    let heb = snippet_paragraph("בְּרֵאשִׁ֖ית", ColorRole::Ink, true, &s);
    let h = layout(&heb, 400.0, ShapeCache::shared());
    let h_height = h.lines[0].bottom - h.lines[0].top;
    let a_height = a.lines[0].bottom - a.lines[0].top;
    assert!(
        h_height < a_height * s.hebrew_scale,
        "{h_height} vs {a_height}"
    );
}

#[test]
fn estimates_are_close_to_real_heights() {
    let unit = english_unit();
    for width in [200.0, 360.0, 600.0] {
        for (_, para, block) in laid(&unit, Direction::Ltr, width) {
            let est = estimate_height(&para, width);
            // Within 40%, or off by at most one line (a line that just fits or just wraps).
            let one_line = TextSettings::default().size * 1.47 * 1.25;
            let ratio = est / block.height;
            assert!(
                (0.6..1.4).contains(&ratio) || (est - block.height).abs() <= one_line * 1.05,
                "estimate {est} vs {} at {width}",
                block.height
            );
        }
    }
}

/// A new Text size scales a block's height estimate rather than styling it again (#299): the
/// scaled estimate is what styling the block at the new size would measure, for English and
/// Hebrew blocks of every kind, with interlinear rows too.
#[test]
fn a_scaled_height_estimate_is_the_estimate_at_the_new_size() {
    let base = TextSettings::default();
    for (unit, direction) in [
        (english_unit(), Direction::Ltr),
        (hebrew_unit(), Direction::Rtl),
    ] {
        for block in unit_blocks(&unit) {
            for size in [12.0, 21.0, 40.0] {
                let at = TextSettings { size, ..base };
                let mut from = block_paragraph(&unit, &block, direction, &base);
                let mut fresh = block_paragraph(&unit, &block, direction, &at);
                for (p, s) in [(&mut from, &base), (&mut fresh, &at)] {
                    p.interlinear = Some(Interlinear::rows_only(2, s));
                }
                let scaled = HeightEstimate::of(&from).scaled(size / base.size);
                let expected = HeightEstimate::of(&fresh);
                for width in [200.0, 360.0, 600.0] {
                    let (a, b) = (scaled.at(width), expected.at(width));
                    assert!(
                        (a - b).abs() <= b * 1e-4,
                        "{:?} at {size} pt, {width} px: scaled {a}, styled {b}",
                        block.kind
                    );
                }
            }
        }
    }
}

/// Colour stays a role all the way through layout (#21 rule 4).
#[test]
fn no_layout_type_holds_a_colour_value() {
    for (name, src) in [
        ("style.rs", include_str!("style.rs")),
        ("paragraph.rs", include_str!("paragraph.rs")),
        ("layout.rs", include_str!("layout.rs")),
        ("present.rs", include_str!("present.rs")),
    ] {
        for banned in [
            "[u8; 4]",
            "[u8;4]",
            "Color32",
            "Rgba",
            "[f32; 4]",
            "color: u32",
        ] {
            assert!(!src.contains(banned), "{name} contains `{banned}`");
        }
    }
}

/// Lays out a long synthetic chapter and reports the time; run with `--ignored` to see it.
#[test]
#[ignore]
fn timing() {
    let unit = english_unit();
    let blocks = unit_blocks(&unit);
    let cache = ShapeCache::new(100_000);
    let t = std::time::Instant::now();
    for _ in 0..500 {
        for b in &blocks {
            let p = block_paragraph(&unit, b, Direction::Ltr, &TextSettings::default());
            std::hint::black_box(layout(&p, 450.0, &cache));
        }
    }
    eprintln!(
        "1500 blocks in {:?}; cache {:?}",
        t.elapsed(),
        cache.stats()
    );
}

/// Write SVG previews of laid blocks to `docs/previews/` when `UPDATE_PREVIEWS` is set.
#[test]
fn previews() {
    if std::env::var_os("UPDATE_PREVIEWS").is_none() {
        return;
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/previews");
    for (name, unit, direction, width) in [
        ("layout-gen-1-hebrew", hebrew_unit(), Direction::Rtl, 420.0),
        ("layout-ps-3-english", english_unit(), Direction::Ltr, 360.0),
    ] {
        let mut paths = String::new();
        let mut y = 12.0;
        for (_, _, block) in laid(&unit, direction, width) {
            for line in &block.lines {
                for it in &line.items {
                    let style = obs_fonts::ShapeStyle::new(
                        it.style.face,
                        it.style.size,
                        it.style.small_caps,
                        it.rtl,
                    );
                    let d = obs_fonts::run_path(
                        &it.shaped,
                        style,
                        12.0 + it.x,
                        y + line.baseline - it.style.raise,
                    )
                    .unwrap();
                    let fill = match it.style.role {
                        ColorRole::Ink => "#222",
                        ColorRole::VerseNumber => "#777",
                        ColorRole::Link => "#2a5db0",
                        ColorRole::NoteMarker => "#2e7d4f",
                        ColorRole::Accent => "#a0522d",
                        ColorRole::WordsOfJesus => "#b22222",
                        ColorRole::Interlinear => "#2e7d4f",
                    };
                    paths.push_str(&format!("  <path fill=\"{fill}\" d=\"{d}\"/>\n"));
                }
            }
            y += block.height;
        }
        let svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\">\n  <rect width=\"100%\" height=\"100%\" fill=\"#fffdf7\"/>\n  <rect x=\"12\" y=\"0\" width=\"{width}\" height=\"100%\" fill=\"none\" stroke=\"#ddd\"/>\n{paths}</svg>\n",
            width + 24.0,
            y + 12.0
        );
        std::fs::write(dir.join(format!("{name}.svg")), svg).unwrap();
    }
}

/// A Note body's Bible references are drawn in the link colour; positions still map back to
/// the body's bytes (#64).
#[test]
fn styled_snippets_colour_their_ranges_and_keep_positions() {
    let s = TextSettings::default();
    let text = "See Jn 3:16 and Rom 5:8 today";
    let a = find(text, "Jn 3:16");
    let b = find(text, "Rom 5:8");
    let para = styled_snippet_paragraph(
        text,
        ColorRole::Ink,
        &[(b.clone(), ColorRole::Link), (a.clone(), ColorRole::Link)],
        false,
        &s,
    );
    let joined: String = para.runs.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(joined, text);
    let mut at = 0;
    for r in &para.runs {
        let RunSource::Text { unit_start } = r.source else {
            panic!("{:?}", r.source)
        };
        assert_eq!(unit_start, at);
        let linked = a.contains(&at) || b.contains(&at);
        assert_eq!(r.style.role == ColorRole::Link, linked, "{:?}", r.text);
        at += r.text.len();
    }
    let laid = layout(&para, 400.0, ShapeCache::shared());
    assert_eq!(laid.text, text);
    // Out-of-range or overlapping ranges are clipped, never panic.
    let odd = styled_snippet_paragraph(
        text,
        ColorRole::Ink,
        &[(3..999, ColorRole::Link), (1..5, ColorRole::Accent)],
        false,
        &s,
    );
    assert_eq!(
        odd.runs.iter().map(|r| r.text.as_str()).collect::<String>(),
        text
    );
}

/// Note formatting (#186): bold and italic ranges take the bold and italic faces, a reference
/// inside bold text is bold and in the link colour, and a heading's scale sizes every run.
#[test]
fn formatted_snippets_take_bold_italic_and_link_styles_where_they_overlap() {
    let s = TextSettings::default();
    let text = "By grace see Rom 5:8 now";
    let bold = find(text, "grace see Rom 5:8");
    let reference = find(text, "Rom 5:8");
    let italic = find(text, "now");
    let style =
        |bold: bool, italic: bool, role: Option<ColorRole>| SnippetStyle { bold, italic, role };
    let para = formatted_snippet_paragraph(
        text,
        ColorRole::Ink,
        &[
            (bold.clone(), style(true, false, None)),
            (
                reference.clone(),
                style(false, false, Some(ColorRole::Link)),
            ),
            (italic.clone(), style(false, true, None)),
            (text.len()..text.len() + 9, style(true, true, None)),
        ],
        1.25,
        &s,
    );
    let joined: String = para.runs.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(joined, text);
    for r in &para.runs {
        let RunSource::Text { unit_start } = r.source else {
            panic!("{:?}", r.source)
        };
        let want_face = if bold.contains(&unit_start) {
            FaceId::Bold
        } else if italic.contains(&unit_start) {
            FaceId::Italic
        } else {
            FaceId::Regular
        };
        assert_eq!(r.style.face, want_face, "{:?}", r.text);
        let want_role = if reference.contains(&unit_start) {
            ColorRole::Link
        } else {
            ColorRole::Ink
        };
        assert_eq!(r.style.role, want_role, "{:?}", r.text);
        assert_eq!(r.style.size, s.size * 1.25);
    }
    let laid = layout(&para, 400.0, ShapeCache::shared());
    assert_eq!(laid.text, text);
}

/// A plain line of a Note (#186) is laid out exactly as Note text was before formatting: the
/// formatted paragraph with only its references as links is the styled one.
#[test]
fn a_plain_formatted_snippet_is_the_styled_snippet_it_replaces() {
    let s = TextSettings::default();
    let text = "  indented *stars* & see Gen 32:24 and Rom 5:8.";
    let refs = [find(text, "Gen 32:24"), find(text, "Rom 5:8")];
    let styled: Vec<(std::ops::Range<usize>, ColorRole)> =
        refs.iter().map(|r| (r.clone(), ColorRole::Link)).collect();
    let formatted: Vec<(std::ops::Range<usize>, SnippetStyle)> = refs
        .iter()
        .map(|r| {
            (
                r.clone(),
                SnippetStyle {
                    role: Some(ColorRole::Link),
                    ..SnippetStyle::default()
                },
            )
        })
        .collect();
    assert_eq!(
        formatted_snippet_paragraph(text, ColorRole::Ink, &formatted, 1.0, &s),
        styled_snippet_paragraph(text, ColorRole::Ink, &styled, false, &s)
    );
    assert_eq!(
        formatted_snippet_paragraph(text, ColorRole::Ink, &[], 1.0, &s),
        snippet_paragraph(text, ColorRole::Ink, false, &s)
    );
}

#[test]
fn a_line_reads_right_to_left_when_its_first_strong_character_does() {
    for rtl in ["בְּרֵאשִׁית", "1 בְּרֵאשִׁית Gen", "**1** בְּרֵאשִׁית", "“בְּרֵאשִׁית"]
    {
        assert!(crate::starts_rtl(rtl), "{rtl}");
    }
    for ltr in ["Genesis 1:1 (TAHOT)", "16 For God", "", "1 2 3"] {
        assert!(!crate::starts_rtl(ltr), "{ltr}");
    }
}

/// John 1:1-2 and 2:1 with Footnotes after 1:1, two after 1:2 and one after 2:1, and a
/// cross-reference note (with its zero-width Cross-reference) after 2:1 (#208).
fn footnoted_unit() -> TextUnit {
    let text =
        "In the beginning was the Word. He was with God.\nOn the third day there was a wedding.";
    let v1 = find(text, "In the beginning was the Word.");
    let v2 = find(text, "He was with God.");
    let v3 = find(text, "On the third day there was a wedding.");
    let verse = |r: std::ops::Range<usize>, c, v| {
        span(
            r,
            SpanKind::Verse {
                verse: Verse::new(Book::John, c, v),
                last: v,
            },
        )
    };
    let note = |at: usize, n: u32, note_kind| {
        span(
            at..at,
            SpanKind::NoteRef {
                note: UnitId::note(n),
                note_kind,
            },
        )
    };
    let mut spans = vec![
        span(
            0..v2.end,
            SpanKind::Para {
                style: ParaStyle::Prose,
            },
        ),
        span(
            v3.clone(),
            SpanKind::Para {
                style: ParaStyle::Prose,
            },
        ),
        verse(v1.clone(), 1, 1),
        verse(v2.clone(), 1, 2),
        verse(v3.clone(), 2, 1),
        note(v1.end, 1, NoteKind::Footnote),
        note(v2.end, 2, NoteKind::Footnote),
        note(v2.end, 3, NoteKind::Footnote),
        note(v3.end - 1, 4, NoteKind::Footnote),
        note(v3.end, 5, NoteKind::CrossReference),
        span(
            v3.end..v3.end,
            SpanKind::Xref {
                targets: PassageList::from(vec![Passage::verse(Verse::new(Book::John, 4, 46))]),
            },
        ),
    ];
    spans.sort_by(|a, b| {
        (a.start, std::cmp::Reverse(a.end), a.kind.name() != "verse").cmp(&(
            b.start,
            std::cmp::Reverse(b.end),
            b.kind.name() != "verse",
        ))
    });
    TextUnit {
        id: UnitId::book(Book::John),
        kind: UnitKind::BibleBook,
        title: None,
        text: text.into(),
        spans,
    }
}

fn object_runs(p: &Paragraph) -> Vec<(String, InlineObject)> {
    p.runs
        .iter()
        .filter_map(|r| match &r.source {
            RunSource::Object { object, .. } if !matches!(object, InlineObject::VerseNumber(_)) => {
                Some((r.text.clone(), object.clone()))
            }
            _ => None,
        })
        .collect()
}

/// #208: Footnote markers are raised italic letters counted from each chapter's first; a
/// cross-reference note and its Cross-reference show as one ↗, the note's marker.
#[test]
fn footnote_markers_are_letters_counted_from_each_chapters_first() {
    let unit = footnoted_unit();
    let blocks = laid(&unit, Direction::Ltr, 600.0);
    let first = object_runs(&blocks[0].1);
    let labels: Vec<&str> = first.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(labels, ["a", "b", "c"]);
    assert_eq!(
        first[1].1,
        InlineObject::NoteMarker {
            note: UnitId::note(2),
            kind: NoteKind::Footnote
        }
    );
    let marker = blocks[0]
        .1
        .runs
        .iter()
        .find(|r| r.text == "a")
        .unwrap()
        .style;
    assert_eq!(marker.role, ColorRole::NoteMarker);
    assert_eq!(marker.face, FaceId::Italic);
    assert!(marker.raise > 0.0, "raised, like a superscript");
    let second = object_runs(&blocks[1].1);
    let labels: Vec<&str> = second.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(labels, ["a", "↗"], "chapter 2 starts again at a");
    assert_eq!(
        second[1].1,
        InlineObject::NoteMarker {
            note: UnitId::note(5),
            kind: NoteKind::CrossReference
        },
        "the ↗ is the cross-reference note's marker, not a second reference"
    );
    assert_eq!(footnote_label(25), "z");
    assert_eq!(footnote_label(26), "aa");
    assert_eq!(footnote_label(27), "ab");
}

/// #208: a cross-reference note with no Cross-reference beside it is a `‡`; a zero-width
/// reference with no cross-reference note stays a reference's own `↗`.
#[test]
fn a_cross_reference_note_without_targets_is_a_double_dagger() {
    let mut unit = footnoted_unit();
    let v1_end = unit.text.find(" He was").unwrap();
    let v2_end = unit.text.find('\n').unwrap();
    unit.spans.push(span(
        v1_end..v1_end,
        SpanKind::ScriptureRef {
            targets: PassageList::from(vec![Passage::verse(Verse::new(Book::Genesis, 1, 1))]),
        },
    ));
    unit.spans.push(span(
        v2_end..v2_end,
        SpanKind::NoteRef {
            note: UnitId::note(6),
            note_kind: NoteKind::CrossReference,
        },
    ));
    unit.spans.sort_by(|a, b| {
        (a.start, std::cmp::Reverse(a.end), a.kind.name() != "verse").cmp(&(
            b.start,
            std::cmp::Reverse(b.end),
            b.kind.name() != "verse",
        ))
    });
    let blocks = laid(&unit, Direction::Ltr, 600.0);
    let first = object_runs(&blocks[0].1);
    let labels: Vec<&str> = first.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(labels, ["a", "↗", "b", "c", "‡"]);
    assert!(matches!(
        first[1].1,
        InlineObject::ScriptureReference { .. }
    ));
    assert_eq!(
        first[4].1,
        InlineObject::NoteMarker {
            note: UnitId::note(6),
            kind: NoteKind::CrossReference
        }
    );
}

/// #208: with the markers off nothing but the text (and verse numbers) is laid out, and the
/// settings key changes so views lay out again.
#[test]
fn footnote_markers_off_lay_out_no_markers() {
    let unit = footnoted_unit();
    let off = TextSettings {
        footnote_markers: false,
        ..TextSettings::default()
    };
    assert_ne!(off.key(), TextSettings::default().key());
    for b in unit_blocks(&unit) {
        let p = block_paragraph(&unit, &b, Direction::Ltr, &off);
        assert_eq!(object_runs(&p), []);
    }
}

/// Rom 8:31-32 as one prose paragraph.
fn romans_8_31_unit() -> TextUnit {
    let v31 = "If God be for us, who can be against us?";
    let v32 = "He that spared not his own Son.";
    let text = format!("{v31} {v32}");
    let r32 = v31.len() + 1..text.len();
    let verse = |range: std::ops::Range<usize>, n: u16| {
        span(
            range,
            SpanKind::Verse {
                verse: Verse::new(Book::Romans, 8, n),
                last: n,
            },
        )
    };
    let mut spans = vec![
        span(
            0..text.len(),
            SpanKind::Para {
                style: ParaStyle::Prose,
            },
        ),
        verse(0..v31.len() + 1, 31),
        verse(r32, 32),
    ];
    spans.sort_by_key(|s| (s.start, std::cmp::Reverse(s.end)));
    TextUnit {
        id: UnitId::book(Book::Romans),
        kind: UnitKind::BibleBook,
        title: None,
        text,
        spans,
    }
}

/// PERSONAL-5 (#305): a Note's end gets room for its marker, before the next verse number,
/// so the marker no longer covers that number or the next word.
#[test]
fn a_notes_end_has_room_for_its_marker_before_the_next_verse_number() {
    let unit = romans_8_31_unit();
    let settings = TextSettings {
        size: 17.0,
        ..TextSettings::default()
    };
    let block = unit_blocks(&unit).remove(0);
    let us = find(&unit.text, "us?").end;
    let v32 = find(&unit.text, "He that").start;
    let objects = |p: &Paragraph| -> Vec<(String, InlineObject)> {
        (p.runs.iter())
            .filter_map(|r| match &r.source {
                RunSource::Object { object, .. } => Some((r.text.clone(), object.clone())),
                RunSource::Text { .. } => None,
            })
            .collect()
    };
    let plain = block_paragraph(&unit, &block, Direction::Ltr, &settings);
    // Ends just after "us?", and at the next verse's start (after the space): the room comes
    // before the number either way.
    for end in [us, v32] {
        let p = block_paragraph_with_note_room(&unit, &block, Direction::Ltr, &settings, &[end]);
        assert_eq!(
            p.text().replace('\u{2003}', ""),
            plain.text(),
            "text is kept"
        );
        let objects: Vec<InlineObject> = objects(&p).into_iter().map(|(_, o)| o).collect();
        assert_eq!(
            objects,
            [
                InlineObject::VerseNumber(Verse::new(Book::Romans, 8, 31)),
                InlineObject::NoteRoom,
                InlineObject::VerseNumber(Verse::new(Book::Romans, 8, 32))
            ]
        );
        let room = p
            .runs
            .iter()
            .position(|r| {
                r.source
                    == RunSource::Object {
                        anchor: end,
                        object: InlineObject::NoteRoom,
                    }
            })
            .expect("a room anchored at the end");
        assert_eq!(p.runs[room - 1].text.trim_end().chars().last(), Some('?'));

        let laid = layout(&p, 600.0, ShapeCache::shared());
        let items: Vec<&LaidItem> = laid.lines.iter().flat_map(|l| &l.items).collect();
        let room = items
            .iter()
            .find(|i| {
                matches!(
                    laid.to_unit(i.bytes.start),
                    Some(UnitPos::Object {
                        object: InlineObject::NoteRoom,
                        ..
                    })
                )
            })
            .expect("the room is laid out");
        assert_eq!(room.shaped.missing_glyphs(), 0, "the face has the em space");
        // About 0.65 em: the marker's 0.42 em and a little space each side.
        let ems = room.width / 17.0;
        assert!((0.6..0.7).contains(&ems), "{ems} em");
        let number = items
            .iter()
            .find(|i| {
                matches!(
                    laid.to_unit(i.bytes.start),
                    Some(UnitPos::Object {
                        object: InlineObject::VerseNumber(v),
                        ..
                    }) if v.verse == 32
                )
            })
            .unwrap();
        assert!(
            number.x >= room.x + room.width - 0.01,
            "{number:?} after {room:?}"
        );
    }
    // An end at the block's start is the block before's; none is no room.
    let p = block_paragraph_with_note_room(&unit, &block, Direction::Ltr, &settings, &[0]);
    assert_eq!(p, plain);
}
