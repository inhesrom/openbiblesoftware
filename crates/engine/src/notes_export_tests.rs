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
    assert!(text.contains("<blockquote><p dir=\"auto\">Love worketh no ill</p></blockquote>"));
    assert!(text.contains("<p dir=\"auto\">Line one &lt;b&gt;<br>\nLine two &amp; more</p>"));
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
