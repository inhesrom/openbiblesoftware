//! Export Notes (#154): a Notebook exported to a Markdown file reads back with every
//! Annotation's Passage, quoted text and Note text; HTML and print pages; a Notes search
//! result; the Attribution of a quoted Resource whose licence asks for it.

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
        let mut s = Session::new(JobMode::Inline);
        assert!(s.library.scan(&self.root.join("library")).is_empty());
        let (store, _) = obs_store::UserStore::open(&self.root.join("user.db")).unwrap();
        s.attach_user(Box::new(store));
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
                json!({ "body": AWKWARD, "notebook": sermon }),
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
    assert!(text.contains("<blockquote><p>Love worketh no ill</p></blockquote>"));
    assert!(text.contains("<p>Line one &lt;b&gt;<br>\nLine two &amp; more</p>"));
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
