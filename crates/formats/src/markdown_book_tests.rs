use super::*;

use obs_model::{SpanKind, UnitKind};

fn file(name: &str, text: &str) -> BookFile {
    BookFile {
        name: name.into(),
        text: text.into(),
    }
}

fn options() -> MarkdownBookOptions {
    MarkdownBookOptions {
        resource_id: "owen-mortification-1".into(),
        source_file: "Owen - Mortification.md".into(),
        tool_version: "test".into(),
    }
}

fn import(main: Option<&BookFile>, parts: &[BookFile]) -> (Resource, ImportReport) {
    import_markdown_book(main, parts, &options(), |_, _| true).unwrap()
}

fn spans<'a>(r: &'a Resource, unit: &str) -> Vec<(&'a str, &'a SpanKind)> {
    let u = r.unit(&UnitId::new(unit).unwrap()).unwrap();
    u.spans
        .iter()
        .map(|s| (&u.text[s.range()], &s.kind))
        .collect()
}

const COL_BOOK: &str = "---
title: \"Of the Mortification of Sin in Believers\"
author: John Owen
date: 1656
license: public domain
source: https://quod.lib.umich.edu/e/eebo/A53693.0001.001
type: book
---

# Of the Mortification of Sin in Believers

*by John Owen*

To the Reader, a short preface before any chapter.

<!-- p. 1 -->

## Chapter I

If ye through the Spirit do mortify the deeds of the body, ye shall live, Rom. 8. 13.[^1]
The words are *conditional* and **certain**, <!-- p. 2 --> and so they continue.

### The first observation

See also Rom. viii. 28 and John 3:16.

## Chapter II

The second chapter.

[^1]: Compare Col. 3. 5.
";

#[test]
fn a_col_book_imports_with_divisions_pages_notes_and_references() {
    let main = file("Owen - Mortification.md", COL_BOOK);
    let (r, report) = import(Some(&main), &[]);
    let m = &r.manifest;
    assert_eq!(m.resource_type, ResourceType::Book);
    assert_eq!(m.title, "Of the Mortification of Sin in Believers");
    assert_eq!(m.license, "LicenseRef-PublicDomain");
    assert_eq!(m.publisher.as_deref(), Some("John Owen"));
    assert_eq!(m.edition.as_deref(), Some("1656"));
    assert_eq!(
        m.source_url.as_deref(),
        Some("https://quod.lib.umich.edu/e/eebo/A53693.0001.001")
    );
    assert!(m.key_schemes.is_empty());
    assert_eq!(m.source_format, MARKDOWN_BOOK_FORMAT);
    assert!(!report.license_missing);

    // Front matter, Chapter I, Chapter II; the title heading and by-line are not text.
    let divisions: Vec<_> = r
        .units
        .iter()
        .filter(|u| u.kind == UnitKind::Division)
        .map(|u| (u.id.as_str(), u.title.as_deref().unwrap_or("")))
        .collect();
    assert_eq!(
        divisions,
        [
            ("div.1", "Front matter"),
            ("div.2", "Chapter I"),
            ("div.3", "Chapter II")
        ]
    );
    let front = r.unit(&UnitId::new("div.1").unwrap()).unwrap();
    assert_eq!(
        front.text,
        "To the Reader, a short preface before any chapter."
    );
    assert!(!front.text.contains("by John Owen"));
    // The page marker on its own line before Chapter I opens Chapter I.
    assert!(
        !spans(&r, "div.1")
            .iter()
            .any(|(_, k)| matches!(k, SpanKind::Page { .. }))
    );
    let ch1 = r.unit(&UnitId::new("div.2").unwrap()).unwrap();
    assert!(
        ch1.spans
            .iter()
            .any(|s| s.start == 0 && s.end == 0 && s.kind == SpanKind::Page { label: "1".into() })
    );

    assert!(ch1.text.starts_with("Chapter I\nIf ye through the Spirit"));
    let s = spans(&r, "div.2");
    assert!(s.contains(&("Chapter I", &SpanKind::Heading { level: 2 })));
    assert!(s.contains(&("The first observation", &SpanKind::Heading { level: 3 })));
    assert!(s.contains(&("conditional", &SpanKind::Italic)));
    assert!(s.contains(&("certain", &SpanKind::Bold)));
    // The inline page marker sits between "certain," and "and".
    let page2 = ch1
        .spans
        .iter()
        .find(|s| s.kind == SpanKind::Page { label: "2".into() })
        .unwrap();
    assert_eq!(page2.start, page2.end);
    assert!(ch1.text[page2.start as usize..].starts_with("and so they continue"));
    // Bible references: modern, old Arabic and old Roman forms.
    let refs: Vec<&str> = s
        .iter()
        .filter(|(_, k)| matches!(k, SpanKind::ScriptureRef { .. }))
        .map(|(t, _)| *t)
        .collect();
    assert_eq!(refs, ["Rom. 8. 13", "Rom. viii. 28", "John 3:16"]);
    // The footnote is a note unit, referenced where it was, with its own reference.
    let note_ref = s
        .iter()
        .find_map(|(_, k)| match k {
            SpanKind::NoteRef { note, note_kind } => Some((note.clone(), *note_kind)),
            _ => None,
        })
        .unwrap();
    assert_eq!(note_ref.1, NoteKind::Footnote);
    let note = r.unit(&note_ref.0).unwrap();
    assert_eq!(note.kind, UnitKind::Note);
    assert_eq!(note.text, "Compare Col. 3. 5.");
    assert!(
        note.spans
            .iter()
            .any(|s| matches!(s.kind, SpanKind::ScriptureRef { .. }))
    );
    assert_eq!(report.notes, 1);
    assert_eq!(report.entries, 3);
    assert_eq!(report.scripture_refs, 4);
}

#[test]
fn parts_are_one_book_in_numeric_order() {
    let main = file(
        "A/Big.md",
        "---\ntitle: Big\nauthor: A\nlicense: cc0\n---\n\n# Big\n\n## Parts\n\n- [Part 1](x)\n- [Part 2](y)\n",
    );
    let names: Vec<String> = [
        "A/Big — Part 10.md",
        "A/Big.md",
        "A/Big — Part 2.md",
        "A/Big — Part 1.md",
        "A/Other.md",
        "A/notes.txt",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(book_stem("A — Part 10.md"), "A");
    assert_eq!(book_stem("A, Part 2.md"), "A");
    assert_eq!(book_stem("Owen - Mortification.md"), "Owen - Mortification");
    let books = group_book_files(&names);
    assert_eq!(
        books,
        [
            BookFiles {
                main: Some("A/Big.md".into()),
                parts: vec![
                    "A/Big — Part 1.md".into(),
                    "A/Big — Part 2.md".into(),
                    "A/Big — Part 10.md".into()
                ],
            },
            BookFiles {
                main: Some("A/Other.md".into()),
                parts: vec![],
            },
        ]
    );
    let parts = [
        file("A/Big — Part 1.md", "# Big\n\n*by A*\n\nOne one one.\n"),
        file("A/Big — Part 2.md", "# Big\n\n*by A*\n\nTwo two two.\n"),
        file("A/Big — Part 10.md", "# Big\n\n*by A*\n\nTen ten ten.\n"),
    ];
    let (r, report) = import(Some(&main), &parts);
    assert_eq!(r.manifest.license, "CC0-1.0");
    let texts: Vec<(&str, &str)> = r
        .units
        .iter()
        .map(|u| (u.title.as_deref().unwrap_or(""), u.text.as_str()))
        .collect();
    // No heading level repeats, so each Part is a unit; the main file's list is not text.
    assert_eq!(
        texts,
        [
            ("Part 1", "One one one."),
            ("Part 2", "Two two two."),
            ("Part 3", "Ten ten ten.")
        ]
    );
    assert_eq!(report.source_files.len(), 4);
}

#[test]
fn a_chapter_may_run_across_parts() {
    let parts = [
        file(
            "B — Part 1.md",
            "## Chapter 1\n\nFirst half\n\n## Chapter 2\n\nStart of two",
        ),
        file(
            "B — Part 2.md",
            "and more of two.\n\n## Chapter 3\n\nThree.",
        ),
    ];
    let (r, _) = import(None, &parts);
    let units: Vec<_> = r
        .units
        .iter()
        .map(|u| (u.title.clone().unwrap_or_default(), u.text.clone()))
        .collect();
    assert_eq!(units.len(), 3);
    assert_eq!(units[1].0, "Chapter 2");
    assert!(units[1].1.contains("Start of two\nand more of two."));
    assert_eq!(r.manifest.title, "B — Part 1");
}

#[test]
fn an_unknown_licence_is_kept_but_never_free() {
    let main = file(
        "x.md",
        "---\ntitle: X\nlicense: Public Domain; no redistribution\n---\n\nText.\n",
    );
    let (r, report) = import(Some(&main), &[]);
    assert_eq!(r.manifest.license, LICENSE_UNKNOWN);
    assert!(report.license_missing);
    assert!(
        r.manifest
            .attribution
            .contains("Licence as given: Public Domain; no redistribution.")
    );
    assert_eq!(report.warnings.len(), 1);
}

#[test]
fn licence_allowlist_matches_whole_values_only() {
    for (value, spdx) in [
        ("Public Domain", Some("LicenseRef-PublicDomain")),
        ("public-domain", Some("LicenseRef-PublicDomain")),
        ("CC0", Some("CC0-1.0")),
        ("cc-by-sa-4.0", Some("CC-BY-SA-4.0")),
        ("CC BY-SA 4.0", Some("CC-BY-SA-4.0")),
        ("CC BY 4.0", Some("CC-BY-4.0")),
        ("Not public domain", None),
        ("CC BY-NC 4.0", None),
        ("", None),
    ] {
        assert_eq!(book_license_spdx(value), spdx, "{value}");
    }
}

#[test]
fn verse_lines_are_poetry_and_markup_outside_the_vocabulary_is_counted() {
    let main = file(
        "hymns.md",
        "---\ntitle: Hymns\n---\n\nO God, our help in ages past,\\\nOur hope for years to come,\n\n\
         Short prose\nwrapped in its file.\n\n\
         ![a picture](img.png)\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n<div>raw</div>\n\n\
         ```\ncode\n```\n\nA [linked](https://example.org) word.\n",
    );
    let (r, report) = import(Some(&main), &[]);
    let u = &r.units[0];
    let poetry: Vec<&str> = u
        .spans
        .iter()
        .filter(|s| {
            s.kind
                == SpanKind::Para {
                    style: ParaStyle::Poetry1,
                }
        })
        .map(|s| &u.text[s.range()])
        .collect();
    assert_eq!(
        poetry,
        [
            "O God, our help in ages past,",
            "Our hope for years to come,"
        ]
    );
    assert!(u.text.contains("\nShort prose wrapped in its file.\n"));
    assert!(u.text.ends_with("A linked word."));
    assert!(!u.text.contains("picture"));
    assert!(!u.text.contains("code"));
    for marker in ["image", "table", "html", "code"] {
        assert_eq!(report.dropped_markup.get(marker), Some(&1), "{marker}");
    }
}

#[test]
fn a_page_turning_inside_a_paragraph_keeps_it_one_paragraph() {
    let main = file(
        "ocr.md",
        "The first half of a sentence\n<!-- p. 12 -->\nand its second half.\n\n<!-- p. 13 -->\n\nNext.\n",
    );
    let (r, _) = import(Some(&main), &[]);
    let u = &r.units[0];
    assert_eq!(
        u.text,
        "The first half of a sentence and its second half.\nNext."
    );
    let pages: Vec<(usize, &str)> = u
        .spans
        .iter()
        .filter_map(|s| match &s.kind {
            SpanKind::Page { label } => Some((s.start as usize, label.as_str())),
            _ => None,
        })
        .collect();
    let second = u.text.find("and its").unwrap();
    let next = u.text.find("Next").unwrap();
    assert_eq!(pages, [(second, "12"), (next, "13")]);
    let paras = u
        .spans
        .iter()
        .filter(|s| matches!(s.kind, SpanKind::Para { .. }))
        .count();
    assert_eq!(paras, 2);
}

#[test]
fn front_matter_reads_plain_scalars_only() {
    let (meta, body) = split_front_matter(
        "---\ntitle: 'It''s a \"Title\"'\nanchor: &a big\nalias: *a\nlist:\n  - x\nnested:\n  key: v\nyear: 1650\n---\nBody\n",
    );
    assert_eq!(
        meta.get("title").map(String::as_str),
        Some("It's a \"Title\"")
    );
    assert_eq!(meta.get("year").map(String::as_str), Some("1650"));
    assert!(!meta.contains_key("anchor"));
    assert!(!meta.contains_key("alias"));
    assert!(!meta.contains_key("key"));
    assert_eq!(body, "Body\n");
    // Unclosed front matter is text, not metadata.
    let (meta, body) = split_front_matter("---\ntitle: x\nno end");
    assert!(meta.is_empty());
    assert!(body.starts_with("---"));
}

#[test]
fn page_labels_are_read_as_printed() {
    for (raw, want) in [
        ("3", "3"),
        ("{iii}", "iii"),
        ("page 4", "4"),
        ("pg323", "323"),
        ("14a", "14a"),
    ] {
        assert_eq!(page_label(raw), want, "{raw}");
    }
    assert_eq!(
        page_labels("<!-- p. 12 --> text <!-- p.13 --> <!-- other -->"),
        ["12", "13"]
    );
}

#[test]
fn empty_and_oversized_books_are_refused() {
    let empty = file("e.md", "---\ntitle: E\n---\n\n<!-- just a comment -->\n");
    assert!(matches!(
        import_markdown_book(Some(&empty), &[], &options(), |_, _| true),
        Err(ImportError::NoText)
    ));
    assert!(matches!(
        import_markdown_book(None, &[], &options(), |_, _| true),
        Err(ImportError::NoText)
    ));
    let big = file("big.md", &"a".repeat(MAX_BOOK_FILE_BYTES + 1));
    assert!(matches!(
        import_markdown_book(Some(&big), &[], &options(), |_, _| true),
        Err(ImportError::TooLarge(_))
    ));
    let bad_id = MarkdownBookOptions {
        resource_id: "Not An Id".into(),
        ..options()
    };
    assert!(matches!(
        import_markdown_book(Some(&empty), &[], &bad_id, |_, _| true),
        Err(ImportError::InvalidResourceId(_))
    ));
}

#[test]
fn cancel_stops_the_import() {
    let main = file("Owen - Mortification.md", COL_BOOK);
    assert!(matches!(
        import_markdown_book(Some(&main), &[], &options(), |_, _| false),
        Err(ImportError::Cancelled)
    ));
}

#[test]
fn hostile_input_neither_panics_nor_hangs() {
    let deep = format!("{}text", ">".repeat(5_000));
    let brackets = "[".repeat(50_000);
    let stars = "*a ".repeat(50_000);
    let notes = (0..2_000).map(|i| format!("x[^{i}]")).collect::<String>();
    let comments = "<!-- p. ".repeat(20_000);
    let front = format!("---\n{}---\nBody", "k: v\n".repeat(20_000));
    for text in [deep, brackets, stars, notes, comments, front] {
        let f = file("h.md", &text);
        let _ = import_markdown_book(Some(&f), &[], &options(), |_, _| true);
    }
}
