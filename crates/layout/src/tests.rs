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
    let settings = TextSettings::default();
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
    // Beyond the right of an RTL line is its logical start; beyond the left, its end.
    assert_eq!(block.caret_at(block.width + 50.0, y), line.bytes.start);
    assert_eq!(block.caret_at(-50.0, y), line.bytes.end);
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
