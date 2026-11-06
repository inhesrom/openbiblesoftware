//! Export Notes (#154): a Notebook exported to a Markdown file reads back with every
//! Annotation's Passage, quoted text and Note text; HTML and print pages; a Notes search
//! result; the Attribution of a quoted Resource whose licence asks for it. Note formatting
//! (#186) is kept: as CommonMark in Markdown, as elements in HTML.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use obs_scripture::{PassageList, Standard, parse_reference};
use serde_json::{Value, json};

use crate::testing::bibles::{kjv, web, write_library};
use crate::*;

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    /// KJV (public domain) and a WEB marked CC BY, so quoting it needs its Attribution.
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("obs-export-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut cc_by = web();
        cc_by.manifest.license = "CC-BY-4.0".into();
        cc_by.manifest.attribution = "WEB test text, licensed CC BY 4.0.".into();
        write_library(&root.join("library"), &[kjv(), cc_by]);
        Self { root }
    }

    fn session(&self) -> Session {
        let (store, _) = obs_store::UserStore::open(&self.root.join("user.db")).unwrap();
        self.session_with(Box::new(store))
    }

    fn session_with(&self, user: Box<dyn UserData>) -> Session {
        let mut s = Session::new(JobMode::Inline);
        assert!(s.library.scan(&self.root.join("library")).is_empty());
        s.attach_user(user);
        s
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn created(out: Value) -> String {
    out["id"].as_str().unwrap().to_string()
}

/// `params` with a selection of the first `text` in `book` of Bible `resource`.
fn selection(resource: &str, book: &str, text: &str, mut params: Value) -> Value {
    let bible = if resource == "kjv" { kjv() } else { web() };
    let unit = bible.units.iter().find(|u| u.id.as_str() == book).unwrap();
    let at = unit.text.find(text).unwrap();
    params["resource"] = json!(resource);
    params["start_unit"] = json!(book);
    params["start"] = json!(at);
    params["end"] = json!(at + text.len());
    params
}

/// One section of an exported Markdown file, read back.
#[derive(Debug, PartialEq, Eq)]
struct ReadBack {
    passage: Option<PassageList>,
    quote: Option<String>,
    text: String,
}

/// Undo the export's Markdown escaping: backslash escapes of ASCII punctuation, hard line
/// breaks, and the character references written for leading spaces and tabs.
fn unescape(line: &str) -> String {
    let line = line.strip_suffix('\\').unwrap_or(line);
    let line = line.replace("&#32;", " ").replace("&#9;", "\t");
    let mut out = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\'
            && let Some(&n) = chars.peek()
            && n.is_ascii_punctuation()
        {
            out.push(n);
            chars.next();
        } else {
            out.push(c);
        }
    }
    out
}

/// Read an exported Markdown file: its title and its Annotations. A heading's reference
/// (less its Bible abbreviation) is read as a Bible reference in the Standard versification.
fn read_back(markdown: &str) -> (String, Vec<ReadBack>) {
    let mut lines = markdown.lines();
    let title = unescape(lines.next().unwrap().strip_prefix("# ").unwrap());
    let mut sections: Vec<(String, Vec<&str>)> = Vec::new();
    for line in lines {
        if line == "---" {
            break;
        }
        match line.strip_prefix("## ") {
            Some(heading) => sections.push((unescape(heading), Vec::new())),
            None => match sections.last_mut() {
                Some((_, body)) => body.push(line),
                None => assert!(line.is_empty(), "{line}"),
            },
        }
    }
    let read = sections
        .into_iter()
        .map(|(heading, body)| {
            let reference = match heading.rsplit_once(" (") {
                Some((r, abbrev)) if abbrev.ends_with(')') => r.to_string(),
                _ => heading.clone(),
            };
            let passage = parse_reference(&reference, &Standard, None).ok();
            let quote: Vec<String> = body
                .iter()
                .filter_map(|l| l.strip_prefix("> ").or(l.strip_prefix(">")))
                .map(unescape)
                .collect();
            let text: Vec<String> = body
                .iter()
                .filter(|l| !l.starts_with('>'))
                .map(|l| unescape(l))
                .collect();
            ReadBack {
                passage,
                quote: (!quote.is_empty()).then(|| quote.join("\n")),
                text: text.join("\n").trim_matches('\n').to_string(),
            }
        })
        .collect();
    (title, read)
}

fn passage(text: &str) -> Option<PassageList> {
    Some(parse_reference(text, &Standard, None).unwrap())
}

/// Plain text that would read as formatting: a Note written before Note formatting existed,
/// stored as the v10 migration leaves it (`escape_note_text`).
const AWKWARD: &str = "Love fulfils the law.\n# not a heading\n1. not a list\n- nor this\n  indented *stars* & &amp; [link](x) <b>bold</b> a_b `code`\n\nSecond paragraph, see Mt 22:39.\n---\n> not a quote";

#[test]
fn exporting_a_notebook_writes_markdown_that_round_trips_passages_and_note_text() {
    let f = Fixture::new("round-trip");
    let mut s = f.session();
    let sermon = created(
        s.run("notebook.create", &json!({ "name": "Sermon prep" }))
            .unwrap(),
    );
    let other = created(
        s.run("notebook.create", &json!({ "name": "Other" }))
            .unwrap(),
    );
    // A Note on selected KJV text (a Passage anchor quoting it), with awkward text.
    let romans = created(
        s.run(
            "annotation.note",
            &selection(
                "kjv",
                "ROM",
                "Love worketh no ill to his neighbour",
                json!({ "body": obs_model::escape_note_text(AWKWARD), "notebook": sermon }),
            ),
        )
        .unwrap(),
    );
    // A Note on a Passage, a Highlight and a Note without an anchor.
    s.run(
        "annotation.note",
        &json!({ "passage": "Lev 19:18", "body": "Love thy neighbour.", "notebook": sermon }),
    )
    .unwrap();
    s.run(
        "annotation.highlight",
        &selection(
            "kjv",
            "MAT",
            "love thy neighbour",
            json!({ "style": "yellow", "notebook": sermon }),
        ),
    )
    .unwrap();
    s.run(
        "annotation.note",
        &json!({ "body": "Unanchored thoughts.", "notebook": sermon }),
    )
    .unwrap();
    // Not in this Notebook.
    s.run(
        "annotation.note",
        &json!({ "passage": "Mt 22:39", "body": "Elsewhere.", "notebook": other }),
    )
    .unwrap();

    let path = f.root.join("Sermon prep.md");
    let out = s
        .run(
            "notes.export",
            &json!({ "format": "markdown", "notebook": sermon, "path": path.to_str().unwrap() }),
        )
        .unwrap();
    assert_eq!(out["file_name"], "Sermon prep.md");
    assert_eq!(out["annotations"], 4);
    let markdown = std::fs::read_to_string(&path).unwrap();
    assert_eq!(markdown, out["text"].as_str().unwrap());

    let (title, read) = read_back(&markdown);
    assert_eq!(title, "Sermon prep");
    let quote = "Love worketh no ill to his neighbour".to_string();
    assert_eq!(
        read,
        [
            ReadBack {
                passage: passage("Lev 19:18"),
                quote: None,
                text: "Love thy neighbour.".into(),
            },
            ReadBack {
                passage: passage("Mt 5:43"),
                quote: Some("love thy neighbour".into()),
                text: "*Highlight (yellow)*".into(),
            },
            ReadBack {
                passage: passage("Rom 13:10"),
                quote: Some(quote),
                text: AWKWARD.into(),
            },
            ReadBack {
                passage: None,
                quote: None,
                text: "Unanchored thoughts.".into(),
            },
        ]
    );
    // The Passages are the Annotations' own.
    let anchored = s.annotation(&romans).unwrap().anchor.unwrap().passage();
    assert_eq!(
        passage("Rom 13:10").unwrap().iter().next().copied(),
        anchored
    );
    // Nothing quoted needs credit: the KJV is public domain.
    assert!(!markdown.contains("\n---\n"));
    // As Markdown shows it: no line of the Note became a heading, list, rule or quote.
    for line in markdown.lines() {
        assert!(
            !(line.starts_with("# ") && line != "# Sermon prep")
                && !line.starts_with("1.")
                && !line.starts_with("- ")
                && line != "---",
            "{line}"
        );
    }
    // And as a CommonMark reader shows the Note: no formatting at all.
    let start = markdown.find("## Romans 13:10").unwrap();
    let end = start + markdown[start..].find("\n## No anchor").unwrap();
    let html = commonmark_html(&markdown[start..end]);
    for tag in ["<h3", "<li", "<ol", "<ul", "<em>", "<strong>", "<a ", "<hr"] {
        assert!(!html.contains(tag), "{tag} in {html}");
    }
    assert!(html.contains("# not a heading<br />"), "{html}");
}

/// Markdown as a CommonMark reader turns it into HTML.
fn commonmark_html(markdown: &str) -> String {
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, pulldown_cmark::Parser::new(markdown));
    html
}

/// A Note with every kind of Note formatting (#186), and a Bible reference in its text.
const FORMATTED: &str = "# Justified\nBy **grace** through *faith*, cf. Rom 5:8.\n- peace with God\n  - access by _faith_\n1. tribulation\n2. patience\n> hope maketh not ashamed\n> for the love of God\n\nSee [the chapter](<Rom 5>) and [notes](https://example.org/rom5).";

/// The events a CommonMark reader reads from `markdown`, with each soft line break taken
/// as a hard one, as the app shows it.
fn commonmark_events(markdown: &str) -> Vec<pulldown_cmark::Event<'_>> {
    use pulldown_cmark::Event;
    pulldown_cmark::Parser::new(markdown)
        .map(|e| match e {
            Event::SoftBreak => Event::HardBreak,
            // A code block left open at the end has no line break after its last line.
            Event::Text(t) => Event::Text(t.trim_end_matches('\n').to_string().into()),
            e => e,
        })
        .collect()
}

/// The Markdown export of a Note is its text as typed (never re-written): a CommonMark
/// reader reads the very same document from it, every line break kept.
fn assert_exported_as_typed(body: &str) {
    let markdown = crate::notes_export::note_markdown(body);
    // Each line is the line typed, or it with a `\` after it; a closing fence or HTML end
    // may follow, on a line of its own.
    let typed: Vec<&str> = body.split('\n').collect();
    let mut written: Vec<&str> = markdown.split('\n').collect();
    let ends_in_break = body.ends_with('\n');
    let closer = if written.len() > typed.len() || (ends_in_break && written.last() != Some(&"")) {
        written.pop()
    } else {
        None
    };
    let kept = match closer {
        Some(_) if ends_in_break => typed.len() - 1,
        _ => typed.len(),
    };
    assert_eq!(written.len(), kept, "{body:?} -> {markdown:?}");
    for (w, t) in written.iter().zip(&typed) {
        assert!(
            w == t || w.strip_suffix('\\') == Some(t),
            "{t:?} -> {w:?} in {markdown:?}"
        );
    }
    // A code fence or HTML block left open is closed: the closing line is read with it.
    let closed = match closer {
        Some(closer) if ends_in_break => format!("{body}{closer}"),
        Some(closer) => format!("{body}\n{closer}"),
        None => body.to_string(),
    };
    assert_eq!(
        commonmark_events(&markdown),
        commonmark_events(&closed),
        "{body:?} -> {markdown:?}"
    );
}

/// The Markdown export keeps every character typed (#186 review): blank lines before and
/// after the text, and spaces at the end of a line of code, which are part of the code.
#[test]
fn exported_markdown_keeps_white_space_as_typed() {
    use crate::notes_export::note_markdown;
    for (body, markdown) in [
        ("```\ncode  ", "```\ncode  \n```"),
        ("```\ncode  \n", "```\ncode  \n```"),
        ("```\ncode\n\n", "```\ncode\n\n```"),
        ("    code  \n    more  ", "    code  \n    more  "),
        ("\n\nfirst\nsecond\n\n", "\n\nfirst\\\nsecond\n\n"),
        ("  \nline  \n  ", "  \nline  \n  "),
    ] {
        assert_eq!(note_markdown(body), markdown, "{body:?}");
        assert_exported_as_typed(body);
    }
    // The code a reader reads is the code typed, its trailing spaces in it.
    let code = |md: &str| -> Vec<String> {
        pulldown_cmark::Parser::new(md)
            .filter_map(|e| match e {
                pulldown_cmark::Event::Text(t) => Some(t.to_string()),
                _ => None,
            })
            .collect()
    };
    assert_eq!(code(&note_markdown("```\ncode  ")), ["code  \n"]);
    assert_eq!(
        code(&note_markdown("```\ncode  \n\n")),
        code("```\ncode  \n\n")
    );
}

#[test]
fn exporting_a_formatted_note_keeps_its_formatting_as_markdown_and_html() {
    let f = Fixture::new("formatted");
    let mut s = f.session();
    s.run(
        "annotation.note",
        &json!({ "passage": "Rom 5:1", "body": FORMATTED }),
    )
    .unwrap();
    let markdown = s
        .run(
            "notes.export",
            &json!({ "format": "markdown", "notebook": "all" }),
        )
        .unwrap()["text"]
        .as_str()
        .unwrap()
        .to_string();
    // The Note as typed, a `\` (hard line break) before the line break inside its quote.
    let note = FORMATTED.replace("ashamed\n", "ashamed\\\n");
    assert_eq!(
        markdown,
        format!("# All Notebooks\n\n## Romans 5:1\n\n{note}\n"),
        "{markdown}"
    );
    assert_exported_as_typed(FORMATTED);
    // The app reads the export back as it shows the Note.
    let shown = |body: &str| {
        obs_model::parse_note_text(body)
            .into_iter()
            .map(|l| {
                // Where a mark is in the body differs by the export's `\`; what it marks not.
                let marks: Vec<_> = l.marks.into_iter().map(|m| (m.range, m.kind)).collect();
                (l.kind, l.text, marks)
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(shown(&note), shown(FORMATTED));
    // A CommonMark reader shows the same structure.
    let html = commonmark_html(&markdown);
    for part in [
        "<h1>Justified</h1>",
        "<p>By <strong>grace</strong> through <em>faith</em>, cf. Rom 5:8.</p>",
        "<ul>\n<li>peace with God\n<ul>\n<li>access by <em>faith</em></li>\n</ul>\n</li>\n</ul>",
        "<ol>\n<li>tribulation</li>\n<li>patience</li>\n</ol>",
        "<blockquote>\n<p>hope maketh not ashamed<br />\nfor the love of God</p>\n</blockquote>",
        "<a href=\"Rom%205\">the chapter</a>",
        "<a href=\"https://example.org/rom5\">notes</a>",
    ] {
        assert!(html.contains(part), "{part}\nin\n{html}");
    }

    let html = s
        .run(
            "notes.export",
            &json!({ "format": "html", "notebook": "all" }),
        )
        .unwrap()["text"]
        .as_str()
        .unwrap()
        .to_string();
    // The same, by the same CommonMark reader: headings two levels down, line breaks kept,
    // and a link to a Bible reference written as its text and the reference.
    let note_html = "<h3>Justified</h3>\n\
        <p dir=\"auto\">By <strong>grace</strong> through <em>faith</em>, cf. Rom 5:8.</p>\n\
        <ul>\n<li>peace with God\n<ul>\n<li>access by <em>faith</em></li>\n</ul>\n</li>\n</ul>\n\
        <ol>\n<li>tribulation</li>\n<li>patience</li>\n</ol>\n\
        <blockquote>\n<p dir=\"auto\">hope maketh not ashamed<br />\nfor the love of God</p>\n</blockquote>\n\
        <p dir=\"auto\">See the chapter (Rom 5) and <a href=\"https://example.org/rom5\">notes</a>.</p>";
    assert!(html.contains(note_html), "{html}");
}

/// Numbered lists (#186 review): the app, the Markdown export read by a CommonMark reader
/// and the HTML export show the same numbers, CommonMark's (a list counts on from its first
/// number).
#[test]
fn exported_numbered_items_show_the_numbers_the_app_shows() {
    use crate::notes_export::{note_html, note_markdown};
    let cases: [(&str, &[u32]); 8] = [
        ("1. first\n5. fifth\n6. sixth", &[1, 2, 3]),
        ("3. three\n3. three again\n1. one", &[3, 4, 5]),
        ("1. one\n\n1. one again", &[1, 2]),
        ("1. a\n   1. d\n   9. i\n2. b", &[1, 1, 2, 2]),
        // Nor may it start a nested list under an item's text.
        ("1. a\n   4. d\n2. b", &[1, 2]),
        ("1. a\n\n   4. d\n2. b", &[1, 4, 2]),
        // A list that starts at another number than 1 cannot interrupt a paragraph.
        ("- a\n  7. g\n  8. h", &[]),
        ("1. a\n2) b", &[1, 2]),
    ];
    // The numbers an HTML reader shows, in order.
    let numbers = |html: &str| {
        let mut shown = Vec::new();
        let mut open: Vec<u32> = Vec::new();
        let mut rest = html;
        while let Some(at) = rest.find('<') {
            rest = &rest[at + 1..];
            let tag = &rest[..rest.find('>').unwrap()];
            if let Some(attrs) = tag.strip_prefix("ol") {
                open.push(attrs.split('"').nth(1).map_or(1, |n| n.parse().unwrap()));
            } else if tag == "/ol" {
                open.pop();
            } else if tag == "li"
                && let Some(n) = open.last_mut()
            {
                shown.push(*n);
                *n += 1;
            }
        }
        shown
    };
    for (body, want) in cases {
        let app: Vec<u32> = obs_model::parse_note_text(body)
            .into_iter()
            .filter_map(|l| match l.kind {
                obs_model::NoteLineKind::Numbered { number, .. } => Some(number),
                _ => None,
            })
            .collect();
        assert_eq!(app, want, "{body}: the app");
        assert_eq!(
            numbers(&commonmark_html(&note_markdown(body))),
            want,
            "{body}: the Markdown"
        );
        assert_eq!(numbers(&note_html(body)), want, "{body}: the HTML");
        assert_exported_as_typed(body);
    }
}

#[test]
fn html_and_print_exports_are_pages_with_each_passage_quote_and_note() {
    let f = Fixture::new("html");
    let mut s = f.session();
    s.run(
        "annotation.note",
        &selection(
            "kjv",
            "ROM",
            "Love worketh no ill",
            json!({ "body": "Line one <b>\nLine two & more" }),
        ),
    )
    .unwrap();
    let html = s
        .run(
            "notes.export",
            &json!({ "format": "html", "notebook": "all" }),
        )
        .unwrap();
    let text = html["text"].as_str().unwrap();
    assert!(text.starts_with("<!DOCTYPE html>"), "{text}");
    assert!(text.contains("<h1>All Notebooks</h1>"));
    assert!(text.contains("<h2>Romans 13:10 (KJV)</h2>"));
    assert!(text.contains("<blockquote><p dir=\"auto\">Love worketh no ill</p></blockquote>"));
    assert!(text.contains("<p dir=\"auto\">Line one &lt;b&gt;<br />\nLine two &amp; more</p>"));
    assert!(!text.contains("window.print()"));
    assert_eq!(html["file_name"], "All Notebooks.html");
    let print = s
        .run(
            "notes.export",
            &json!({ "format": "print", "notebook": "all" }),
        )
        .unwrap();
    assert!(print["text"].as_str().unwrap().contains("window.print()"));
    assert!(s.run("notes.export", &json!({ "format": "pdf" })).is_err());
}

#[test]
fn quotes_and_note_paragraphs_take_their_direction_from_their_text() {
    let f = Fixture::new("direction");
    let mut s = f.session();
    s.run(
        "annotation.note",
        &selection(
            "kjv",
            "ROM",
            "Love worketh",
            json!({ "body": "שָׁלוֹם — peace.\n\nIn English." }),
        ),
    )
    .unwrap();
    let [_, html, print] = every_export(&mut s);
    for page in [html, print] {
        assert!(
            page.contains("<blockquote><p dir=\"auto\">Love worketh"),
            "{page}"
        );
        // A Hebrew paragraph of a Note reads right to left, the next left to right.
        assert!(
            page.contains("<p dir=\"auto\">שָׁלוֹם — peace.</p>\n<p dir=\"auto\">In English.</p>"),
            "{page}"
        );
    }
}

#[test]
fn exporting_a_notes_search_result_keeps_its_order_and_title() {
    let f = Fixture::new("search");
    let mut s = f.session();
    s.run(
        "annotation.note",
        &json!({ "passage": "Lev 19:18", "body": "Grace first." }),
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(3));
    s.run(
        "annotation.note",
        &json!({ "passage": "Rom 13:10", "body": "Grace later." }),
    )
    .unwrap();
    s.run(
        "annotation.note",
        &json!({ "passage": "Mt 5:43", "body": "Unrelated." }),
    )
    .unwrap();
    let out = s
        .run("search.run", &json!({ "text": "grace", "mode": "notes" }))
        .unwrap();
    let tab = out["tab"].as_u64().unwrap();
    let export = s
        .run("notes.export", &json!({ "format": "markdown", "tab": tab }))
        .unwrap();
    let (title, read) = read_back(export["text"].as_str().unwrap());
    assert_eq!(title, "Notes matching “grace”");
    let texts: Vec<&str> = read.iter().map(|r| r.text.as_str()).collect();
    // Newest change first, as the search lists them.
    assert_eq!(texts, ["Grace later.", "Grace first."]);
    assert_eq!(export["file_name"], "Notes matching grace.md");
}

#[test]
fn quoting_a_resource_whose_licence_requires_it_appends_its_attribution() {
    let f = Fixture::new("attribution");
    let mut s = f.session();
    s.run(
        "annotation.note",
        &selection(
            "web",
            "ROM",
            "Love doesn",
            json!({ "body": "From the WEB." }),
        ),
    )
    .unwrap();
    let markdown = s
        .run(
            "notes.export",
            &json!({ "format": "markdown", "notebook": "all" }),
        )
        .unwrap()["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        markdown.ends_with("\n---\n\nWEB test text, licensed CC BY 4.0.\n"),
        "{markdown}"
    );
    let html = s
        .run(
            "notes.export",
            &json!({ "format": "html", "notebook": "all" }),
        )
        .unwrap()["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(html.contains("<hr>\n<p><small>WEB test text, licensed CC BY 4.0.</small></p>"));
}

/// Markdown, HTML and print exports of every Notebook.
fn every_export(s: &mut Session) -> [String; 3] {
    ["markdown", "html", "print"].map(|format| {
        s.run(
            "notes.export",
            &json!({ "format": format, "notebook": "all" }),
        )
        .unwrap()["text"]
            .as_str()
            .unwrap()
            .to_string()
    })
}

#[test]
fn removing_a_quoted_resource_keeps_its_attribution_in_exports_after_a_restart() {
    let f = Fixture::new("removed-attribution");
    {
        let mut s = f.session();
        for (resource, text) in [("web", "Love doesn"), ("kjv", "Love worketh")] {
            s.run(
                "annotation.note",
                &selection(resource, "ROM", text, json!({ "body": "Kept." })),
            )
            .unwrap();
        }
        for id in ["web", "kjv"] {
            s.run("library.remove", &json!({ "id": id, "confirm": true }))
                .unwrap();
            assert!(s.library.get(id).is_none());
        }
    }
    // A restart: the Attribution comes from the User database, not the library.
    let mut s = f.session();
    let [markdown, html, print] = every_export(&mut s);
    assert!(markdown.contains("\n> Love doesn"), "{markdown}");
    assert!(markdown.contains("\n> Love worketh"), "{markdown}");
    assert!(
        markdown.ends_with("\n---\n\nWEB test text, licensed CC BY 4.0.\n"),
        "{markdown}"
    );
    for page in [html, print] {
        assert!(
            page.contains("<blockquote><p dir=\"auto\">Love doesn"),
            "{page}"
        );
        assert!(
            page.contains("<hr>\n<p><small>WEB test text, licensed CC BY 4.0.</small></p>"),
            "{page}"
        );
        // KJV is public domain: no credit, before or after removal.
        assert_eq!(page.matches("<small>").count(), 1, "{page}");
    }
}

#[test]
fn a_reimport_under_another_licence_replaces_the_remembered_attribution() {
    let f = Fixture::new("reimported-attribution");
    let kjv_path = f.root.join("library").join("kjv.obr");
    {
        let mut s = f.session();
        // Quoted from the public-domain revision, which is then removed: no credit kept.
        s.run(
            "annotation.note",
            &selection("kjv", "ROM", "Love worketh", json!({ "body": "Old." })),
        )
        .unwrap();
        s.run("library.remove", &json!({ "id": "kjv", "confirm": true }))
            .unwrap();
        // A revision licensed CC BY is imported, and quoted.
        let mut cc_by = kjv();
        cc_by.manifest.license = "CC-BY-4.0".into();
        cc_by.manifest.attribution = "KJV test revision, licensed CC BY 4.0.".into();
        crate::testing::bibles::write_library(&f.root.join("library"), &[cc_by.clone()]);
        s.added_to_library(vec![crate::library::LibraryEntry {
            manifest: cc_by.manifest,
            path: kjv_path.clone(),
        }]);
        s.run(
            "annotation.note",
            &selection("kjv", "ROM", "Love worketh", json!({ "body": "New." })),
        )
        .unwrap();
    }
    // Its file goes away outside the app; after a restart the credit comes from the User
    // database, and it is the CC BY revision's, not the public-domain one's.
    std::fs::remove_file(&kjv_path).unwrap();
    let mut s = f.session();
    assert!(s.library.get("kjv").is_none());
    let credit = "KJV test revision, licensed CC BY 4.0.";
    let [markdown, html, print] = every_export(&mut s);
    assert!(markdown.contains("\n> Love worketh"), "{markdown}");
    assert!(
        markdown.ends_with(&format!("\n---\n\n{credit}\n")),
        "{markdown}"
    );
    for page in [html, print] {
        assert!(
            page.contains("<blockquote><p dir=\"auto\">Love worketh"),
            "{page}"
        );
        assert!(
            page.contains(&format!("<hr>\n<p><small>{credit}</small></p>")),
            "{page}"
        );
    }
}

#[test]
fn a_quote_whose_resource_went_missing_is_left_out_of_exports() {
    let f = Fixture::new("missing-licence");
    {
        // A User database that could not remember the Resource's credit.
        let (store, _) = obs_store::UserStore::open(&f.root.join("user.db")).unwrap();
        let mut s = f.session_with(Box::new(crate::testing::WritesFail::preferences(store)));
        s.run(
            "annotation.note",
            &selection("web", "ROM", "Love doesn", json!({ "body": "Still mine." })),
        )
        .unwrap();
        // Its file goes away outside the app: its licence is unknown from now on.
        std::fs::remove_file(&s.library.get("web").unwrap().path).unwrap();
    }
    let mut s = f.session();
    assert!(s.library.get("web").is_none());
    let left_out = "Quoted text left out: web is not in the library, so the Attribution its \
                    licence may require is unknown. Import it again to export the quote.";
    for page in every_export(&mut s) {
        assert!(!page.contains("Love doesn"), "{page}");
        assert!(page.contains("Still mine."), "{page}");
        assert!(!page.contains("CC BY"), "{page}");
        assert!(page.contains(left_out), "{page}");
    }
}

/// Credits are confirmed to the millisecond: make sure what follows is later than what came
/// before.
fn next_millisecond() {
    std::thread::sleep(std::time::Duration::from_millis(2));
}

/// Run queued jobs (re-anchoring) and apply their results.
fn settle(s: &mut Session) {
    for _ in 0..20 {
        s.jobs().pump(64);
        let _ = s.poll_imports();
        if !s.reanchoring.is_busy() {
            return;
        }
    }
    panic!("re-anchoring did not finish");
}

/// The KJV fixture with `from` changed to `to`: a revision with other content.
fn kjv_changed(from: &str, to: &str) -> obs_model::Resource {
    use crate::testing::bibles::{LOVE_KJV, bible, verses};
    let list: Vec<_> = LOVE_KJV
        .iter()
        .map(|&(book, chapter, verse, text)| {
            let text: &'static str = Box::leak(text.replace(from, to).into_boxed_str());
            (book, chapter, verse, text)
        })
        .collect();
    bible(
        "kjv",
        "KJV",
        &verses(&list),
        None,
        &[("love", "H157"), ("neighbour", "H7453")],
    )
}

fn cc_by(mut resource: obs_model::Resource) -> obs_model::Resource {
    resource.manifest.license = "CC-BY-4.0".into();
    resource.manifest.attribution = "KJV test revision, licensed CC BY 4.0.".into();
    resource
}

/// Review of #154: the public-domain KJV's empty credit is saved and quoted; a revision
/// licensed CC BY is then imported while the User database cannot save preferences, and
/// quoted; its file goes away outside the app. After a restart the new quote is left out
/// (its credit was never saved), the public-domain quote still exports, and no export goes
/// out with the CC BY text uncredited. `changed` picks a revision with other text (another
/// content hash) or the same text under the new licence.
fn a_licence_change_that_was_not_saved(name: &str, changed: bool) {
    let f = Fixture::new(name);
    let kjv_path = f.root.join("library").join("kjv.obr");
    {
        let mut s = f.session();
        s.run(
            "annotation.note",
            &selection("kjv", "ROM", "Love worketh", json!({ "body": "Old." })),
        )
        .unwrap();
    }
    let revision = if changed {
        cc_by(kjv_changed("therefore love", "THEREFORE LOVE"))
    } else {
        cc_by(kjv())
    };
    {
        let (store, _) = obs_store::UserStore::open(&f.root.join("user.db")).unwrap();
        let mut s = f.session_with(Box::new(crate::testing::WritesFail::preferences(store)));
        crate::testing::bibles::write_library(
            &f.root.join("library"),
            std::slice::from_ref(&revision),
        );
        s.added_to_library(vec![crate::library::LibraryEntry {
            manifest: revision.manifest.clone(),
            path: kjv_path.clone(),
        }]);
        assert_eq!(s.library.get("kjv").unwrap().manifest.license, "CC-BY-4.0");
        settle(&mut s);
        next_millisecond();
        s.run(
            "annotation.note",
            &selection("kjv", "ROM", "Love worketh", json!({ "body": "New." })),
        )
        .unwrap();
    }
    std::fs::remove_file(&kjv_path).unwrap();
    let mut s = f.session();
    assert!(s.library.get("kjv").is_none());
    let left_out = "Quoted text left out: kjv is not in the library, so the Attribution its \
                    licence may require is unknown. Import it again to export the quote.";
    for page in every_export(&mut s) {
        assert!(page.contains("Old."), "{page}");
        assert!(page.contains("New."), "{page}");
        assert!(page.contains(left_out), "{page}");
        assert!(!page.contains("CC BY"), "{page}");
        if changed {
            // Re-anchoring moved the old Note onto the new revision's text: out as well.
            assert_eq!(page.matches(left_out).count(), 2, "{page}");
            assert!(!page.contains("Love worketh"), "{page}");
        } else {
            // The old Note was quoted while the KJV was public domain, and is kept.
            assert_eq!(page.matches(left_out).count(), 1, "{page}");
            assert_eq!(page.matches("Love worketh").count(), 1, "{page}");
        }
    }
}

#[test]
fn a_licence_change_that_was_not_saved_leaves_later_quotes_out_after_a_restart() {
    a_licence_change_that_was_not_saved("unsaved-licence", false);
}

#[test]
fn a_changed_revision_whose_credit_was_not_saved_leaves_its_quotes_out_after_a_restart() {
    a_licence_change_that_was_not_saved("unsaved-revision", true);
}

#[test]
fn an_orphaned_quote_keeps_the_credit_of_the_revision_it_was_taken_from() {
    let f = Fixture::new("orphan-credit");
    let kjv_path = f.root.join("library").join("kjv.obr");
    let mut s = f.session();
    // Quoted from a CC BY revision...
    let quoted = cc_by(kjv());
    crate::testing::bibles::write_library(&f.root.join("library"), std::slice::from_ref(&quoted));
    s.added_to_library(vec![crate::library::LibraryEntry {
        manifest: quoted.manifest.clone(),
        path: kjv_path.clone(),
    }]);
    settle(&mut s);
    let id = created(
        s.run(
            "annotation.note",
            &selection("kjv", "ROM", "Love worketh", json!({ "body": "Quoted." })),
        )
        .unwrap(),
    );
    // ...replaced by a public-domain revision without that text: the Note's Text range is
    // orphaned and keeps its quote, which still needs the CC BY revision's credit.
    let public = kjv_changed("Love worketh no ill", "Charity hurts none");
    crate::testing::bibles::write_library(&f.root.join("library"), std::slice::from_ref(&public));
    s.added_to_library(vec![crate::library::LibraryEntry {
        manifest: public.manifest.clone(),
        path: kjv_path,
    }]);
    settle(&mut s);
    let note = s.annotation(&id).unwrap();
    let range = note.anchor.as_ref().and_then(Anchor::text_range).unwrap();
    assert!(range.orphaned);
    assert_eq!(range.content_hash, quoted.manifest.content_hash);
    let credit = "KJV test revision, licensed CC BY 4.0.";
    for page in every_export(&mut s) {
        assert!(page.contains("Love worketh"), "{page}");
        assert!(page.contains(credit), "{page}");
    }
}

#[test]
fn a_credit_not_confirmed_when_quoted_is_confirmed_at_the_next_library_load() {
    let f = Fixture::new("late-confirmation");
    // The library load saves the WEB's credit; then a quote is taken while preferences
    // cannot be saved, so its credit is not confirmed as of the quote.
    drop(f.session());
    {
        let (store, _) = obs_store::UserStore::open(&f.root.join("user.db")).unwrap();
        let mut s = f.session_with(Box::new(crate::testing::WritesFail::preferences(store)));
        next_millisecond();
        s.run(
            "annotation.note",
            &selection("web", "ROM", "Love doesn", json!({ "body": "Later." })),
        )
        .unwrap();
    }
    // The next library load, with the WEB still there, confirms it; then its file goes.
    {
        let s = f.session();
        std::fs::remove_file(&s.library.get("web").unwrap().path).unwrap();
    }
    let mut s = f.session();
    assert!(s.library.get("web").is_none());
    for page in every_export(&mut s) {
        assert!(page.contains("Love doesn"), "{page}");
        assert!(
            page.contains("WEB test text, licensed CC BY 4.0."),
            "{page}"
        );
    }
}

/// Each character of a one-line Note as shown: (bold, italic, link target).
type Styled = Vec<(char, bool, bool, Option<String>)>;

/// How the app shows a one-line Note.
fn app_styled(body: &str) -> Styled {
    let line = obs_model::parse_note_text(body).remove(0);
    line.text
        .char_indices()
        .map(|(i, c)| {
            let on = |k: &dyn Fn(&obs_model::NoteMarkKind) -> bool| {
                line.marks
                    .iter()
                    .any(|m| m.range.contains(&i) && k(&m.kind))
            };
            let target = line.marks.iter().find_map(|m| match &m.kind {
                obs_model::NoteMarkKind::Link(t) if m.range.contains(&i) => Some(t.clone()),
                _ => None,
            });
            (
                c,
                on(&|k| *k == obs_model::NoteMarkKind::Bold),
                on(&|k| *k == obs_model::NoteMarkKind::Italic),
                target,
            )
        })
        .collect()
}

/// How a CommonMark reader shows one paragraph of Markdown.
fn commonmark_styled(markdown: &str) -> Styled {
    use pulldown_cmark::{Event, Tag, TagEnd};
    let (mut strong, mut em) = (0, 0);
    let mut link: Option<String> = None;
    let mut out = Styled::new();
    for event in pulldown_cmark::Parser::new(markdown) {
        match event {
            Event::Start(Tag::Strong) => strong += 1,
            Event::End(TagEnd::Strong) => strong -= 1,
            Event::Start(Tag::Emphasis) => em += 1,
            Event::End(TagEnd::Emphasis) => em -= 1,
            Event::Start(Tag::Link { dest_url, .. }) => link = Some(dest_url.to_string()),
            Event::End(TagEnd::Link) => link = None,
            Event::Text(t) => {
                out.extend(t.chars().map(|c| (c, strong > 0, em > 0, link.clone())));
            }
            Event::Start(Tag::Paragraph) | Event::End(TagEnd::Paragraph) => {}
            other => panic!("{other:?} in {markdown}"),
        }
    }
    out
}

/// Each body whose exported Markdown a CommonMark reader does not show as the app shows it.
fn misread(bodies: impl IntoIterator<Item = String>) -> Vec<String> {
    use crate::notes_export::note_markdown;
    let mut failures = Vec::new();
    for body in bodies {
        let markdown = note_markdown(&body);
        if app_styled(&body) != commonmark_styled(&markdown)
            || commonmark_events(&markdown) != commonmark_events(&body)
        {
            failures.push(format!("{body:?} -> {markdown:?}"));
        }
    }
    failures
}

/// Spans that are hard to write as CommonMark next to each other.
const EMPHASIS_PARTS: [&str; 14] = [
    "*a*",
    "_b_",
    "**c**",
    "__d__",
    "e",
    " ",
    "[f](x)",
    "**g _h_**",
    "_i **j**_",
    "***k***",
    ".",
    "\\*",
    "l*m*n",
    "“",
];

/// Bold and italic next to each other (#186 reviews): the Markdown export is the Note as
/// typed, so a CommonMark reader shows each character with the bold, italic and link the
/// app shows. The cases the earlier, re-written exports got wrong are among them.
#[test]
fn exported_emphasis_reads_back_as_the_app_shows_it() {
    use crate::notes_export::note_markdown;
    let mut bodies: Vec<String> = [
        "_faith_*hope*",
        "__faith__**hope**",
        "**a _b_**_c_",
        "_a **b**_**c**.",
        "**a _b_***c*d",
        "*a*[*b*](<Rom 5:8>)",
        "By **grace** through _faith_*,* cf. Rom 5:8.",
        "***k****a*e",
        "***k***__d__e",
        "*a****k***e",
        "_i **j**_***k***e",
        "**c*****k***e",
        "  *a* indented",
        "  *(a)* indented",
        "a *(b)*c",
    ]
    .map(String::from)
    .to_vec();
    // And every run of four of the parts (38,416 Notes).
    let parts = EMPHASIS_PARTS;
    for w in parts {
        for x in parts {
            for y in parts {
                for z in parts {
                    bodies.push(format!("{w}{x}{y}{z}"));
                }
            }
        }
    }
    let failures = misread(bodies);
    assert!(
        failures.is_empty(),
        "{} misread:\n{}",
        failures.len(),
        failures.join("\n")
    );
    // As typed.
    assert_eq!(note_markdown("_faith_*hope*"), "_faith_*hope*");
    assert_eq!(note_markdown("***k***e"), "***k***e");
}

/// Link targets (#186 review): the app, the Markdown export read by a CommonMark reader
/// and the HTML export link to the same place; `&amp;` in a target is CommonMark's `&`
/// everywhere, and `\&amp;` the text `&amp;`.
#[test]
fn exported_link_targets_are_the_ones_the_app_follows() {
    use crate::notes_export::{note_html, note_markdown};
    use pulldown_cmark::{Event, Tag};
    for (body, target) in [
        (
            "[site](https://example.org/?q=&amp;)",
            "https://example.org/?q=&",
        ),
        (
            "[site](https://example.org/?q=\\&amp;)",
            "https://example.org/?q=&amp;",
        ),
        (
            "[site](<https://example.org/a b>)",
            "https://example.org/a b",
        ),
        (
            "[site](https://example.org/\\(x\\))",
            "https://example.org/(x)",
        ),
        (
            "[site](https://example.org/a\\\\b)",
            "https://example.org/a\\b",
        ),
        ("[a](<x \\> y>)", "x > y"),
        ("[the verse](<Rom 5:8>)", "Rom 5:8"),
    ] {
        let app = obs_model::parse_note_text(body).remove(0).marks;
        assert_eq!(
            app[0].kind,
            obs_model::NoteMarkKind::Link(target.into()),
            "{body}"
        );
        let markdown = note_markdown(body);
        let read: Vec<String> = pulldown_cmark::Parser::new(&markdown)
            .filter_map(|e| match e {
                Event::Start(Tag::Link { dest_url, .. }) => Some(dest_url.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(read, [target], "{body} -> {markdown}");
        let html = note_html(body);
        let href = target
            .replace('&', "&amp;")
            .replace(' ', "%20")
            .replace('\\', "%5C");
        let href = format!("href=\"{href}\"");
        assert!(
            if target.starts_with("https://") {
                html.contains(&href)
            } else {
                html.contains(&format!("({target})").replace('>', "&gt;"))
            },
            "{body} -> {html}"
        );
    }
}

/// Samples of each part of the CommonMark spec, as Notes: the Markdown export reads as the
/// Note does, and the Note after it in an export is not read into it (a code fence or HTML
/// block left open is closed).
const SPEC_SAMPLES: &[&str] = &[
    // Thematic breaks and ATX / setext headings.
    "***\n---\n___",
    "# h1\n## h2 ##\n###### h6\n####### not",
    "Setext one\n===\n\nSetext two\nover two lines\n---",
    // Indented and fenced code.
    "    indented code\n\n      more",
    "```rust\nfn main() {}\n```",
    "~~~~\n```\nstill code\n~~~~",
    "```\nunclosed fence",
    "~~~\nunclosed tilde fence\n\nwith a gap",
    "- item\n\n  ```\n  unclosed in an item",
    "> ```\n> unclosed in a quote",
    // HTML blocks, closed and left open.
    "<div>\n*html*\n</div>\n\nafter",
    "<!-- open comment\nstill comment",
    "<script>\nlet x = 1;",
    "<pre>\nopen pre",
    "<?php open",
    "<![CDATA[ open",
    "<!DOCTYPE open",
    "<style>\nopen",
    "<textarea>\nopen",
    // Link reference definitions and paragraphs.
    "[foo]: /url \"title\"\n\n[foo]",
    "aaa\nbbb\n\nccc\n   ddd",
    // Block quotes, with laziness.
    "> # Foo\n> bar\nbaz",
    "> a\n>\n> b\n\n>> nested",
    // List items and lists.
    "1.  A paragraph\n    with two lines.\n\n        indented code\n\n    > A block quote.",
    "- one\n\n two",
    "- a\n- b\n\n- c",
    "1. a\n\n2. b\n\n3. c",
    "- a\n  - b\n    - c\n      - d",
    "10) ten\n11) eleven",
    "- \n  foo",
    "- [ ] not a task: no extensions",
    // Inlines.
    "`code` and `` a ` b `` and ` `` `",
    "\\*not emphasis\\* \\\\ \\` \\#",
    "&nbsp; &amp; &copy; &#35; &#x22; &bogus;",
    "*foo bar*\n_foo bar_\n**foo bar**\n__foo bar__",
    "*foo**bar**baz*\n***strong emph***\n**foo*bar*baz**",
    "foo*bar*\nfoo_bar_\n_foo_bar_baz_",
    "*(*foo*)*\n**foo \"*bar*\" foo**",
    "[link](/uri \"title\")\n[link](</my uri>)\n[a](<b)c>)",
    "[link [foo [bar]]](/uri)\n[link *foo **bar** `#`*](/uri)",
    "![foo](/url \"title\")\n<http://foo.bar.baz>\n<foo@bar.example.com>",
    "<a href=\"x\">raw</a> <!-- c --> <?php ?>",
    "foo  \nbar\nfoo\\\nbar",
    "line one\nline two\nline three",
    "trailing backslash\\",
];

#[test]
fn commonmark_spec_samples_export_as_they_read() {
    for sample in SPEC_SAMPLES {
        assert_exported_as_typed(sample);
    }
    // In an export, each sample's Note is followed by the next Annotation's heading.
    let f = Fixture::new("spec-samples");
    let mut s = f.session();
    for (i, sample) in SPEC_SAMPLES.iter().enumerate() {
        let verse = format!("Ps {}:1", i + 1);
        s.run(
            "annotation.note",
            &json!({ "passage": verse, "body": sample }),
        )
        .unwrap();
    }
    let markdown = s
        .run(
            "notes.export",
            &json!({ "format": "markdown", "notebook": "all" }),
        )
        .unwrap()["text"]
        .as_str()
        .unwrap()
        .to_string();
    let headings: Vec<String> = {
        use pulldown_cmark::{Event, HeadingLevel, Tag, TagEnd};
        let mut out = Vec::new();
        let mut in_h2 = false;
        for e in pulldown_cmark::Parser::new(&markdown) {
            match e {
                Event::Start(Tag::Heading {
                    level: HeadingLevel::H2,
                    ..
                }) => {
                    in_h2 = true;
                    out.push(String::new());
                }
                Event::End(TagEnd::Heading(HeadingLevel::H2)) => in_h2 = false,
                Event::Text(t) if in_h2 => out.last_mut().unwrap().push_str(&t),
                _ => {}
            }
        }
        out
    };
    // Every Annotation's heading is still one, whatever the Note before it.
    let annotations: Vec<&String> = headings
        .iter()
        .filter(|h| h.starts_with("Psalms"))
        .collect();
    let want: Vec<String> = (1..=SPEC_SAMPLES.len())
        .map(|c| format!("Psalms {c}:1"))
        .collect();
    assert_eq!(annotations, want.iter().collect::<Vec<_>>(), "{markdown}");
    // The HTML export of each sample is a CommonMark reader's.
    for sample in SPEC_SAMPLES {
        let html = crate::notes_export::note_html(sample);
        assert!(!html.contains("<script>"), "{html}");
        assert!(!html.contains("<div>"), "{html}");
    }
}
