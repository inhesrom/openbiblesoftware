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
        "# Grace\n## Two\n####### seven\n#tag\n- one\n* two\n+ three\n-not\n1. first\n12) twelfth\n1.5 pounds\n> quoted\n>tight\n\n   \nplain",
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
            (NoteLineKind::Bullet { level: 0 }, "one"),
            (NoteLineKind::Bullet { level: 0 }, "two"),
            (NoteLineKind::Bullet { level: 0 }, "three"),
            (NoteLineKind::Text, "-not"),
            (
                NoteLineKind::Numbered {
                    level: 0,
                    number: 1
                },
                "first"
            ),
            (
                NoteLineKind::Numbered {
                    level: 0,
                    number: 12
                },
                "twelfth"
            ),
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
fn list_items_nest_two_spaces_a_level_under_the_item_above() {
    let lines = parse_note_text("- a\n  - b\n      - c\n  1. d\n- e\n  indented text\n  - f");
    let kinds: Vec<NoteLineKind> = lines.iter().map(|l| l.kind).collect();
    assert_eq!(
        kinds,
        [
            NoteLineKind::Bullet { level: 0 },
            NoteLineKind::Bullet { level: 1 },
            // Never more than one level below the item above.
            NoteLineKind::Bullet { level: 2 },
            NoteLineKind::Numbered {
                level: 1,
                number: 1
            },
            NoteLineKind::Bullet { level: 0 },
            NoteLineKind::Text,
            // Not under a list item: an indented dash is text.
            NoteLineKind::Text,
        ]
    );
    assert_eq!(lines[5].text, "  indented text");
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
    let l = line("See [the **love** chapter](1 Cor 13) and [site](<https://example.org/a b>).");
    assert_eq!(l.text, "See the love chapter and site.");
    assert_eq!(
        marks(&l),
        [
            (NoteMarkKind::Link("1 Cor 13".into()), "the love chapter"),
            (NoteMarkKind::Bold, "love"),
            (NoteMarkKind::Link("https://example.org/a b".into()), "site"),
        ]
    );
}

#[test]
fn what_is_not_markup_shows_as_typed() {
    for text in [
        "snake_case_name",
        "2 * 3 * 4",
        "a ** b",
        "**unclosed",
        "[no link]",
        "[empty]()",
        "C:\\Users\\me",
        "`code` <b>html</b> & | ~",
        "x*y",
    ] {
        let l = line(text);
        assert_eq!(l.text, text, "{text}");
        assert!(l.marks.is_empty(), "{text}: {:?}", l.marks);
    }
    // Crossing runs: the first closer wins and the run between them shows as typed.
    let l = line("*a __b* c__");
    assert_eq!(l.text, "a __b c__");
    assert_eq!(marks(&l), [(NoteMarkKind::Italic, "a __b")]);
    // A backslash ending a line is CommonMark's hard line break: every line break is one.
    assert_eq!(line("first\\").text, "first");
    // Escapes show the character.
    let l = line(r"\*not italic\* \# \\");
    assert_eq!(l.text, r"*not italic* # \");
    assert!(l.marks.is_empty());
}

#[test]
fn offsets_map_shown_text_back_to_the_body() {
    let body = "intro\n## A **bold** [link](Rom 5:8)";
    let l = &parse_note_text(body)[1];
    assert_eq!(l.source, 6..body.len());
    let bold = &l.marks[0];
    assert_eq!(&body[l.source_range(bold.range.clone())], "bold");
    let link = &l.marks[1];
    assert_eq!(&body[l.source_range(link.range.clone())], "link");
    let markup: Vec<&str> = l.markup().into_iter().map(|r| &body[r]).collect();
    assert_eq!(markup, ["## ", "**", "**", "[", "](Rom 5:8)"]);
}

#[test]
fn plain_text_drops_the_markup() {
    assert_eq!(
        note_plain_text("# Title\n- **grace** and [peace](Rom 1:7)\n> quoted"),
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
