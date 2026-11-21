#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use obs_model::{SpanKind, UnitKind};

use super::*;

fn options(id: &str) -> HtmlBookOptions {
    HtmlBookOptions {
        resource_id: id.into(),
        source_file: format!("{id}.htm"),
        tool_version: "test".into(),
        ..HtmlBookOptions::default()
    }
}

fn import(html: &str) -> (Resource, ImportReport) {
    import_pg_html(html, &options("pg-test"), |_, _| true).unwrap()
}

fn divisions(r: &Resource) -> Vec<(&str, &str)> {
    r.units
        .iter()
        .filter(|u| u.kind == UnitKind::Division)
        .map(|u| (u.title.as_deref().unwrap_or(""), u.text.as_str()))
        .collect()
}

fn spans_of(u: &obs_model::TextUnit, f: impl Fn(&SpanKind) -> bool) -> Vec<&str> {
    u.spans
        .iter()
        .filter(|s| f(&s.kind))
        .map(|s| &u.text[s.range()])
        .collect()
}

fn pages(u: &obs_model::TextUnit) -> Vec<(usize, String)> {
    u.spans
        .iter()
        .filter_map(|s| match &s.kind {
            SpanKind::Page { label } => Some((s.start as usize, label.clone())),
            _ => None,
        })
        .collect()
}

/// The shape of a Project Gutenberg HTML book: PG's header and footer around the text, a
/// title page, chapters in `h2`, page numbers, two styles of footnote, a poem, a list.
const SAMPLE: &str = r##"<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.0 Strict//EN">
<html xmlns="http://www.w3.org/1999/xhtml" lang="en">
<head><title>The Project Gutenberg eBook of All of Grace, by C. H. Spurgeon</title>
<style>p { margin: 0 } .pagenum { position: absolute }</style></head>
<body>
<div>The Project Gutenberg eBook of All of Grace. This eBook is for the use of anyone
anywhere at no cost; see the Project Gutenberg License.</div>
<div style='text-align:center'>*** START OF THE PROJECT GUTENBERG EBOOK ALL OF GRACE ***</div>
<p>Produced by Project Gutenberg volunteers.</p>
<h1>ALL OF GRACE</h1>
<p class="center">BY C. H. SPURGEON</p>
<h2>TO THE READER</h2>
<p><span class="pagenum"><a name="Page_5" id="Page_5">[5]</a></span>The object of this book
is the <i>salvation</i> of the reader.<a name="FNanchor1" id="FNanchor1"></a><a href="#Footnote1"
class="fnanchor">[A]</a> See Rom. 3:24 &amp; Eph. 2:8&mdash;grace.</p>
<h2>CHAPTER I.<br />WHAT ARE WE AT? <small class="toclink"><a href="#toc">Contents</a></small></h2>
<p>He that hears <b>shall</b> live.<a name="citation9"></a><a href="#footnote9"
class="citation">[9]</a> He
<span class="pagenum">[Pg 6]</span>goes on.</p>
<div class="poem"><div class="stanza">
<span class="i0">Just as I am, without one plea,<br /></span>
<span class="i2">But that Thy blood was shed for me,<br /></span>
</div></div>
<ul><li>first,</li><li>second.</li></ul>
<p><img src="images/i.png" alt="a picture" /></p>
<div class="footnote"><p><a name="Footnote1" id="Footnote1"></a><a href="#FNanchor1"><span
class="label">[A]</span></a> A note on grace, Gal. 2:21.</p></div>
<p><a name="footnote9"></a><a href="#citation9" class="footnote">[9]</a>&nbsp; Another note.</p>
<p>End of the Project Gutenberg EBook of All of Grace.</p>
<div style='text-align:center'>*** END OF THE PROJECT GUTENBERG EBOOK ALL OF GRACE ***</div>
<p>Section 1. General Terms of Use. Project Gutenberg License.</p>
</body></html>"##;

#[test]
fn a_gutenberg_book_becomes_a_book_without_gutenberg_in_it() {
    let (r, report) = import(SAMPLE);
    assert_eq!(r.manifest.resource_type, ResourceType::Book);
    assert_eq!(r.manifest.title, "All of Grace");
    assert_eq!(r.manifest.language, "en");
    assert_eq!(r.manifest.source_format, PG_HTML_FORMAT);
    assert_eq!(
        r.manifest.license, LICENSE_UNKNOWN,
        "the licence is the caller's"
    );
    let all: String = r
        .units
        .iter()
        .map(|u| u.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!all.to_lowercase().contains("gutenberg"), "{all}");
    assert!(!all.contains("General Terms"));

    let d = divisions(&r);
    assert_eq!(
        d.iter().map(|(t, _)| *t).collect::<Vec<_>>(),
        // The title (`h1`, above the chapters' `h2`) is a Division of its own.
        [
            "ALL OF GRACE",
            "TO THE READER",
            "CHAPTER I. WHAT ARE WE AT?"
        ]
    );
    assert_eq!(d[0].1, "ALL OF GRACE\nBY C. H. SPURGEON");

    let reader = &r.units[1];
    assert!(
        reader
            .text
            .starts_with("TO THE READER\nThe object of this book is the salvation of the reader. See Rom. 3:24 & Eph. 2:8—grace."),
        "{}",
        reader.text
    );
    assert_eq!(spans_of(reader, |k| *k == SpanKind::Italic), ["salvation"]);
    assert_eq!(
        pages(reader)
            .iter()
            .map(|(_, l)| l.as_str())
            .collect::<Vec<_>>(),
        ["5"]
    );
    let refs = spans_of(reader, |k| matches!(k, SpanKind::ScriptureRef { .. }));
    assert_eq!(refs, ["Rom. 3:24", "Eph. 2:8"]);

    let ch1 = &r.units[2];
    assert_eq!(spans_of(ch1, |k| *k == SpanKind::Bold), ["shall"]);
    assert!(
        ch1.text.contains("He that hears shall live. He goes on."),
        "{}",
        ch1.text
    );
    let p6 = pages(ch1);
    assert_eq!(p6.len(), 1);
    assert!(
        ch1.text[p6[0].0..].starts_with("goes on."),
        "{}",
        &ch1.text[p6[0].0..]
    );
    let poetry = spans_of(ch1, |k| {
        *k == SpanKind::Para {
            style: ParaStyle::Poetry1,
        }
    });
    assert_eq!(
        poetry,
        [
            "Just as I am, without one plea,",
            "But that Thy blood was shed for me,"
        ]
    );
    let list = spans_of(ch1, |k| {
        *k == SpanKind::Para {
            style: ParaStyle::List,
        }
    });
    assert_eq!(list, ["first,", "second."]);
    assert_eq!(report.dropped_markup.get("img"), Some(&1));

    // Both footnotes, each referenced where it stands; neither is in the main text.
    assert!(!ch1.text.contains("Another note") && !ch1.text.contains("[9]"));
    let notes: Vec<&str> = r
        .units
        .iter()
        .filter(|u| u.kind == UnitKind::Note)
        .map(|u| u.text.as_str())
        .collect();
    assert_eq!(notes, ["A note on grace, Gal. 2:21.", "Another note."]);
    let note_refs = |u: &obs_model::TextUnit| {
        u.spans
            .iter()
            .filter(|s| matches!(s.kind, SpanKind::NoteRef { .. }))
            .count()
    };
    assert_eq!(note_refs(reader), 1);
    assert_eq!(note_refs(ch1), 1);
    assert_eq!(report.notes, 2);
    assert!(report.scripture_refs >= 3);
}

#[test]
fn entities_and_tags_are_read_tolerantly() {
    assert_eq!(
        decode_entities("a&amp;b &mdash; &#8212; &#x2014; &AElig; &bogus; & x"),
        "a&b — — — Æ &bogus; & x"
    );
    assert_eq!(decode_entities("&nbsp"), "\u{a0}");
    let tokens =
        tokenize("<P CLASS=pagenum>x<br>y</p><!-- c --><script>if (a<b) {}</script>z").unwrap();
    assert_eq!(
        tokens,
        [
            Token::Start {
                name: "p".into(),
                attrs: vec![("class".into(), "pagenum".into())]
            },
            Token::Text("x".into()),
            Token::Start {
                name: "br".into(),
                attrs: vec![]
            },
            Token::Text("y".into()),
            Token::End("p".into()),
            Token::Start {
                name: "script".into(),
                attrs: vec![]
            },
            Token::End("script".into()),
            Token::Text("z".into()),
        ]
    );
    assert_eq!(page_label("[Pg 23]").as_deref(), Some("23"));
    assert_eq!(page_label("[p. iv]").as_deref(), Some("iv"));
    assert_eq!(page_label("Page_23").as_deref(), Some("23"));
    assert_eq!(page_label("[Illustration]"), None);
}

#[test]
fn latin_1_files_are_read_as_windows_1252() {
    assert_eq!(decode_html_bytes(b"caf\xe9 \x93q\x94"), "café “q”");
    assert_eq!(decode_html_bytes("café".as_bytes()), "café");
    assert_eq!(decode_html_bytes(b"\xef\xbb\xbfx"), "x");
}

#[test]
fn what_is_not_a_gutenberg_book_is_refused_plainly() {
    for bad in ["", "<html><body><p>No markers.</p></body></html>"] {
        assert!(
            import_pg_html(bad, &options("pg-bad"), |_, _| true).is_err(),
            "{bad:?}"
        );
    }
    let empty = "<p>*** START OF THE PROJECT GUTENBERG EBOOK X ***</p><p>*** END OF THE PROJECT GUTENBERG EBOOK X ***</p>";
    assert!(matches!(
        import_pg_html(empty, &options("pg-empty"), |_, _| true),
        Err(ImportError::NoText)
    ));
    assert!(matches!(
        import_pg_html(SAMPLE, &options("Not An Id"), |_, _| true),
        Err(ImportError::InvalidResourceId(_))
    ));
    assert!(matches!(
        import_pg_html(SAMPLE, &options("pg-cancel"), |_, _| false),
        Err(ImportError::Cancelled)
    ));
}

#[test]
fn deep_nesting_unclosed_tags_and_stray_brackets_neither_panic_nor_hang() {
    let wrap = |inner: String| {
        format!(
            "<p>*** START OF THE PROJECT GUTENBERG EBOOK X ***</p>{inner}<p>*** END OF THE PROJECT GUTENBERG EBOOK X ***</p>"
        )
    };
    let deep = wrap(format!("{}x", "<div><i>".repeat(5_000)));
    assert!(import(&deep).0.units[0].text.contains('x'));
    let messy = wrap(
        "<p>a < b <p>c<i>d<p>e</b></i> <a href=\"#Footnote_9\" class=\"fnanchor\">[9]</a> f".into(),
    );
    let (r, _) = import(&messy);
    let text = &r.units[0].text;
    assert!(
        text.contains("a < b") && text.contains('c') && text.contains('f'),
        "{text}"
    );
    // The reference to a footnote that never comes is dropped.
    assert!(
        r.units[0]
            .spans
            .iter()
            .all(|s| !matches!(s.kind, SpanKind::NoteRef { .. }))
    );
}

/// Every PG book in `$OBS_PG_DIR` imports (a manual check of real files).
#[test]
#[ignore = "needs Project Gutenberg HTML files in OBS_PG_DIR"]
fn every_book_in_a_folder_imports() {
    let Ok(dir) = std::env::var("OBS_PG_DIR") else {
        return;
    };
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "htm" || e == "html"))
        .collect();
    names.sort();
    for path in names {
        let html = decode_html_bytes(&std::fs::read(&path).unwrap());
        let (r, report) = import(&html);
        // No trace of Project Gutenberg in the text or a Division's title.
        for u in &r.units {
            let title = u.title.as_deref().unwrap_or_default();
            assert!(
                !format!("{title} {}", u.text)
                    .to_lowercase()
                    .contains("gutenberg"),
                "{}: {} {title:?}",
                path.display(),
                u.id
            );
        }
        let pages: usize = r.units.iter().map(|u| pages(u).len()).sum();
        println!(
            "{} {:?}: {} divisions, {} notes, {} pages, {} refs, {} KB",
            path.file_name().unwrap().to_string_lossy(),
            r.manifest.title,
            report.entries,
            report.notes,
            pages,
            report.scripture_refs,
            r.units.iter().map(|u| u.text.len()).sum::<usize>() / 1000,
        );
        for u in r
            .units
            .iter()
            .filter(|u| u.kind == UnitKind::Division)
            .take(12)
        {
            println!("    {}", u.title.as_deref().unwrap_or(""));
        }
    }
}
