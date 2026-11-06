use super::*;

fn line(body: &str) -> NoteLine {
    parse_note_text(body).remove(0)
}

/// Each mark as (kind, the text it covers).
fn marks(l: &NoteLine) -> Vec<(NoteMarkKind, &str)> {
    l.marks
        .iter()
        .map(|m| (m.kind.clone(), &l.text[m.range.clone()]))
        .collect()
}

#[test]
fn line_kinds_follow_their_markers() {
    let lines = parse_note_text(
        "# Grace\n## Two\n####### seven\n#tag\n\n- one\n* two\n+ three\n\n-not\n1. first\n12) twelfth\n\n1.5 pounds\n> quoted\n>tight\n\n   \nplain",
    );
    let kinds: Vec<(NoteLineKind, &str)> =
        lines.iter().map(|l| (l.kind, l.text.as_str())).collect();
    assert_eq!(
        kinds,
        [
            (NoteLineKind::Heading(1), "Grace"),
            (NoteLineKind::Heading(2), "Two"),
            (NoteLineKind::Text, "####### seven"),
            (NoteLineKind::Text, "#tag"),
            (NoteLineKind::Blank, ""),
            (NoteLineKind::Bullet { level: 0 }, "one"),
            // Another bullet character starts another list, as CommonMark reads it.
            (NoteLineKind::Bullet { level: 0 }, "two"),
            (NoteLineKind::Bullet { level: 0 }, "three"),
            (NoteLineKind::Blank, ""),
            (NoteLineKind::Text, "-not"),
            (
                NoteLineKind::Numbered {
                    level: 0,
                    number: 1
                },
                "first"
            ),
            // Another delimiter starts another list, at its own number.
            (
                NoteLineKind::Numbered {
                    level: 0,
                    number: 12
                },
                "twelfth"
            ),
            (NoteLineKind::Blank, ""),
            (NoteLineKind::Text, "1.5 pounds"),
            (NoteLineKind::Quote, "quoted"),
            (NoteLineKind::Quote, "tight"),
            (NoteLineKind::Blank, ""),
            (NoteLineKind::Blank, ""),
            (NoteLineKind::Text, "plain"),
        ]
    );
}

#[test]
fn lists_nest_and_number_as_commonmark_reads_them() {
    let lines = parse_note_text("- a\n  - b\n    - c\n  1. d\n- e\n\n  more of e\n- f\n5. g\n7. h");
    let kinds: Vec<NoteLineKind> = lines.iter().map(|l| l.kind).collect();
    let numbered = |level, number| NoteLineKind::Numbered { level, number };
    assert_eq!(
        kinds,
        [
            NoteLineKind::Bullet { level: 0 },
            NoteLineKind::Bullet { level: 1 },
            NoteLineKind::Bullet { level: 2 },
            numbered(1, 1),
            NoteLineKind::Bullet { level: 0 },
            NoteLineKind::Blank,
            // A paragraph of the item above: text.
            NoteLineKind::Text,
            NoteLineKind::Bullet { level: 0 },
            // A numbered list counts on from its first number.
            numbered(0, 5),
            numbered(0, 6),
        ]
    );
    assert_eq!(lines[6].text, "more of e");
    // Under a numbered item, nested items go under its text (three columns in).
    let lines = parse_note_text("1. a\n   - b\n2. c");
    let kinds: Vec<NoteLineKind> = lines.iter().map(|l| l.kind).collect();
    assert_eq!(
        kinds,
        [
            numbered(0, 1),
            NoteLineKind::Bullet { level: 1 },
            numbered(0, 2)
        ]
    );
}

#[test]
fn every_line_break_is_shown_and_spans_are_marked_on_each_line() {
    let lines = parse_note_text("first **bold\nstill bold** then\n> a\n>\n> b\n\n\nend");
    let shown: Vec<(NoteLineKind, &str)> =
        lines.iter().map(|l| (l.kind, l.text.as_str())).collect();
    assert_eq!(
        shown,
        [
            (NoteLineKind::Text, "first bold"),
            (NoteLineKind::Text, "still bold then"),
            (NoteLineKind::Quote, "a"),
            (NoteLineKind::Quote, ""),
            (NoteLineKind::Quote, "b"),
            (NoteLineKind::Blank, ""),
            (NoteLineKind::Blank, ""),
            (NoteLineKind::Text, "end"),
        ]
    );
    assert_eq!(marks(&lines[0]), [(NoteMarkKind::Bold, "bold")]);
    assert_eq!(marks(&lines[1]), [(NoteMarkKind::Bold, "still bold")]);
}

#[test]
fn bold_italic_and_links_are_marked_on_the_shown_text() {
    let l = line("Grace **alone**, *through* faith, __by__ _grace_ ***all***.");
    assert_eq!(l.text, "Grace alone, through faith, by grace all.");
    assert_eq!(
        marks(&l),
        [
            (NoteMarkKind::Bold, "alone"),
            (NoteMarkKind::Italic, "through"),
            (NoteMarkKind::Bold, "by"),
            (NoteMarkKind::Italic, "grace"),
            (NoteMarkKind::Bold, "all"),
            (NoteMarkKind::Italic, "all"),
        ]
    );
    let l = line(
        "See [the **love** chapter](<1 Cor 13>), [site](<https://example.org/a b>) and <https://example.org>.",
    );
    assert_eq!(
        l.text,
        "See the love chapter, site and https://example.org."
    );
    assert_eq!(
        marks(&l),
        [
            (NoteMarkKind::Link("1 Cor 13".into()), "the love chapter"),
            (NoteMarkKind::Bold, "love"),
            (NoteMarkKind::Link("https://example.org/a b".into()), "site"),
            (
                NoteMarkKind::Link("https://example.org".into()),
                "https://example.org"
            ),
        ]
    );
}

#[test]
fn link_targets_read_escapes_and_references_as_commonmark_does() {
    let target = |body: &str| match &line(body).marks[..] {
        [
            NoteMark {
                kind: NoteMarkKind::Link(t),
                ..
            },
        ] => Some(t.clone()),
        _ => None,
    };
    assert_eq!(target("[a](x?q=\\&amp;)").as_deref(), Some("x?q=&amp;"));
    assert_eq!(target("[a](x?q=&amp;)").as_deref(), Some("x?q=&"));
    assert_eq!(target("[a](x\\(1\\))").as_deref(), Some("x(1)"));
    assert_eq!(target("[a](<a \\> b>)").as_deref(), Some("a > b"));
    assert_eq!(target("[a](x\\\\)").as_deref(), Some("x\\"));
    // `\` before anything else is itself.
    assert_eq!(target("[a](x\\y)").as_deref(), Some("x\\y"));
    // A target with a space needs `<…>`.
    assert_eq!(target("[a](Rom 5:8)"), None);
}

#[test]
fn what_is_not_markup_shows_as_typed() {
    for text in [
        "snake_case_name",
        "2 * 3 * 4",
        "a ** b",
        "**unclosed",
        "[no link]",
        "C:\\Users\\me",
        "<b>html</b> & | ~",
        "x*y",
    ] {
        let l = line(text);
        assert_eq!(l.text, text, "{text}");
        assert!(l.marks.is_empty(), "{text}: {:?}", l.marks);
    }
    // A code span shows its text, without the backticks.
    assert_eq!(line("say `x *y*`").text, "say x *y*");
    // A backslash ending a line is CommonMark's hard line break: every line break is one.
    assert_eq!(line("first\\\nsecond").text, "first");
    // Escapes and character references show the character.
    let l = line(r"\*not italic\* \# \\ &amp;");
    assert_eq!(l.text, r"*not italic* # \ &");
    assert!(l.marks.is_empty());
}

#[test]
fn a_code_span_over_lines_keeps_every_character_on_its_line() {
    // CommonMark reads the line break as a space and takes the quote markers, list indent
    // and a CRLF's `\r` off the next line before reading the span.
    for (body, shown) in [
        ("`a\nb`", "a\nb"),
        ("> `a\n> b`", "a\nb"),
        ("> > `ab\n> > cd`", "ab\ncd"),
        ("- `a\n  b`", "a\nb"),
        ("> `a\r\n> b`", "a\nb"),
        ("`a  \nb`", "a  \nb"),
        ("` a\nb `", "a\nb"),
    ] {
        let lines = parse_note_text(body);
        let text: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(text.join("\n"), shown, "{body:?}");
        assert_eq!(note_plain_text(body), shown, "{body:?}");
        // Each shown character maps onto itself in the body.
        for l in &lines {
            let r = l.source_range(0..l.text.len());
            let typed: String = body[r]
                .chars()
                .filter(|c| !"`> \r\n".contains(*c))
                .collect();
            let seen: String = l.text.chars().filter(|c| !"> ".contains(*c)).collect();
            assert_eq!(typed, seen, "{body:?}");
        }
    }
    let quoted = parse_note_text("> `a\n> b`");
    assert_eq!(quoted[1].kind, NoteLineKind::Quote);
    assert_eq!(quoted[1].content, 7);
}

#[test]
fn offsets_map_shown_text_back_to_the_body() {
    let body = "intro\n## A **bold** [link](<Rom 5:8>) \\*";
    let l = &parse_note_text(body)[1];
    assert_eq!(l.source, 6..body.len());
    let bold = &l.marks[0];
    assert_eq!(&body[l.source_range(bold.range.clone())], "bold");
    let link = &l.marks[1];
    assert_eq!(&body[l.source_range(link.range.clone())], "link");
    let markup: Vec<&str> = l.markup().into_iter().map(|r| &body[r]).collect();
    assert_eq!(markup, ["## ", "**", "**", "[", "](<Rom 5:8>)", "\\"]);
    // Its content starts after the heading marker, at the `\` of an escape.
    let l = &parse_note_text("- \\# not a heading")[0];
    assert_eq!(l.content, 2);
}

#[test]
fn plain_text_drops_the_markup() {
    assert_eq!(
        note_plain_text("# Title\n- **grace** and [peace](<Rom 1:7>)\n> quoted"),
        "Title\ngrace and peace\nquoted"
    );
}

/// Every line of `plain` shows exactly as typed once escaped, as one text line each.
fn shows_as_typed(plain: &str) {
    let body = escape_note_text(plain);
    let lines = parse_note_text(&body);
    let wanted: Vec<&str> = plain.split('\n').collect();
    assert_eq!(lines.len(), wanted.len(), "{body}");
    for (l, w) in lines.iter().zip(wanted) {
        if w.trim().is_empty() {
            assert_eq!(l.kind, NoteLineKind::Blank, "{w:?}");
        } else {
            assert_eq!(l.kind, NoteLineKind::Text, "{w:?} in {body}");
            assert_eq!(l.text, w, "{body}");
            assert!(l.marks.is_empty(), "{w:?}: {:?}", l.marks);
        }
    }
}

#[test]
fn escaped_plain_text_shows_exactly_as_typed() {
    shows_as_typed(
        "Love fulfils the law.\n# not a heading\n1. not a list\n- nor this\n  indented *stars* & &amp; [link](x) <b>bold</b> a_b `code`\n\nSecond paragraph, see Mt 22:39.\n---\n> not a quote",
    );
    shows_as_typed("**strong** __u__ _i_ *e* ***x*** \\* \\\\ C:\\path [a](b) [c]");
    shows_as_typed("+ plus\n* star\n12) twelve\n#tag\n\t- tabbed\n   \nend");
    shows_as_typed("ends in a backslash \\\nC:\\\n\\");
    // What else CommonMark reads as markup: setext underlines, fences, code indents, HTML
    // blocks, link reference definitions, autolinks, character references and code spans;
    // spaces it drops at the start and end of a line.
    shows_as_typed(
        "Title\n===\nSub\n---\n```\nx\n~~~\n    code\n<div>\n[x]: y\n<https://a.b>\n&copy; &#32; &\n`tick` ``\ntrailing  \n\tt\n***\n___\n1) a\n![img](x)",
    );
    // Every arrangement of the characters that can be markup, two and three at a time.
    let parts = [
        "*", "**", "_", "a", " ", "[x]", "(y)", "\\", "#", "-", ">", "1.", "]",
    ];
    for a in parts {
        for b in parts {
            shows_as_typed(&format!("{a}{b}"));
            for c in parts {
                shows_as_typed(&format!("{a}{b}{c}w"));
                shows_as_typed(&format!("w{a}{b}{c}"));
            }
        }
    }
}

#[test]
fn plain_text_that_reads_as_typed_is_left_alone() {
    for text in [
        "Esau ran to meet him; compare Gen 32:24 here.",
        "a_b and 2 * 3",
        "C:\\Users",
        "",
        "Line one\n\nLine two",
    ] {
        assert_eq!(escape_note_text(text), text);
    }
    assert_eq!(escape_note_text("# x\n1. y\n*z*"), "\\# x\n1\\. y\n\\*z\\*");
}
