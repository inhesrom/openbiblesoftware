#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use obs_model::{SpanKind, UnitKind};

use super::*;

fn options(id: &str) -> HtmlBookOptions {
    HtmlBookOptions {
        resource_id: id.into(),
        title: "The Vicar's Daughter".into(),
        source_file: format!("{id}.txt"),
        tool_version: "test".into(),
        ..HtmlBookOptions::default()
    }
}

fn import(text: &str) -> (Resource, ImportReport) {
    import_pg_text(text, &options("pg-text-test"), |_, _| true).unwrap()
}

/// PG's plain text: its header and footer, a contents list (one blank line between entries),
/// chapters after four blank lines, a subtitle in capitals, italics, a poem, an illustration.
const SAMPLE: &str = "The Project Gutenberg EBook of The Vicar's Daughter\r
\r
*** START OF THIS PROJECT GUTENBERG EBOOK THE VICAR'S DAUGHTER ***\r
\r
Produced by Jonathan Ingram and the Project\r
Gutenberg Online Distributed Proofreading Team\r
\r
\r
\r
THE VICAR'S DAUGHTER\r
\r
BY GEORGE MACDONALD\r
\r
CONTENTS.\r
\r
CHAPTER I.\r
INTRODUCTORY\r
\r
CHAPTER II.\r
I TRY\r
\r
\r
\r
\r
CHAPTER I.\r
\r
INTRODUCTORY.\r
\r
I am not the vicar's _daughter_, but his\r
granddaughter; see John 3:16.\r
\r
    O Lord, my God,\r
    when I in awesome wonder\r
\r
[Illustration: A picture.]\r
\r
\r
\r
\r
CHAPTER II.\r
\r
I TRY.\r
\r
I tried. The snake_case word stays.\r
\r
End of the Project Gutenberg EBook of The Vicar's Daughter\r
\r
*** END OF THIS PROJECT GUTENBERG EBOOK THE VICAR'S DAUGHTER ***\r
\r
Section 1. General Terms of Use.\r
";

#[test]
fn a_gutenberg_text_becomes_a_book_by_its_chapters() {
    let (r, report) = import(SAMPLE);
    assert_eq!(r.manifest.source_format, PG_TEXT_FORMAT);
    assert_eq!(r.manifest.title, "The Vicar's Daughter");
    let all: String = r
        .units
        .iter()
        .map(|u| u.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!all.to_lowercase().contains("gutenberg"), "{all}");
    let titles: Vec<&str> = r
        .units
        .iter()
        .filter(|u| u.kind == UnitKind::Division)
        .map(|u| u.title.as_deref().unwrap_or(""))
        .collect();
    // The contents list's "CHAPTER I." (one blank line before it) is not a Division.
    assert_eq!(
        titles,
        ["THE VICAR'S DAUGHTER", "CHAPTER I.", "CHAPTER II."]
    );
    let ch1 = &r.units[1];
    assert!(
        ch1.text
            .starts_with("CHAPTER I.\nINTRODUCTORY.\nI am not the vicar's daughter, but his granddaughter; see John 3:16."),
        "{}",
        ch1.text
    );
    let spans = |u: &obs_model::TextUnit, f: &dyn Fn(&SpanKind) -> bool| {
        u.spans
            .iter()
            .filter(|s| f(&s.kind))
            .map(|s| u.text[s.range()].to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(spans(ch1, &|k| *k == SpanKind::Italic), ["daughter"]);
    assert_eq!(
        spans(ch1, &|k| *k == SpanKind::Heading { level: 2 }),
        ["INTRODUCTORY."]
    );
    assert_eq!(
        spans(ch1, &|k| *k
            == SpanKind::Para {
                style: ParaStyle::Poetry1
            }),
        ["O Lord, my God,", "when I in awesome wonder"]
    );
    assert_eq!(
        spans(ch1, &|k| matches!(k, SpanKind::ScriptureRef { .. })),
        ["John 3:16"]
    );
    assert!(r.units[2].text.contains("The snake_case word stays."));
    assert_eq!(report.dropped_markup.get("illustration"), Some(&1));
    assert_eq!(report.entries, 3);
}

#[test]
fn without_gaps_chapter_words_divide_the_book() {
    let text = "*** START OF THE PROJECT GUTENBERG EBOOK X ***\n\nSERMON I.\n\nFirst.\n\nSERMON II.\n\nSecond.\n\n*** END OF THE PROJECT GUTENBERG EBOOK X ***\n";
    let (r, _) = import(text);
    let titles: Vec<&str> = r
        .units
        .iter()
        .map(|u| u.title.as_deref().unwrap_or(""))
        .collect();
    assert_eq!(titles, ["SERMON I.", "SERMON II."]);
}

#[test]
fn credits_dates_and_braced_pages_are_not_chapters() {
    let text = "*** START OF THE PROJECT GUTENBERG EBOOK X ***\n\n\n\n\nProduced by A. Volunteer\n\n\n\n\n1900\n\n\n\n\nCHAPTER I.\n\n{1} One {2} two.\n\n{3}\n\nThree {iv} four {5} five.\n\n\n\n\nCHAPTER II.\n\nSix.\n\n\n\n\nCHAPTER III.\n\nSeven.\n\n*** END OF THE PROJECT GUTENBERG EBOOK X ***\n";
    let (r, _) = import(text);
    let titles: Vec<&str> = r
        .units
        .iter()
        .map(|u| u.title.as_deref().unwrap_or(""))
        .collect();
    assert_eq!(
        titles,
        ["Front matter", "CHAPTER I.", "CHAPTER II.", "CHAPTER III."]
    );
    assert!(r.units[0].text.contains("Produced by A. Volunteer"));
    let ch1 = &r.units[1];
    assert_eq!(ch1.text, "CHAPTER I.\nOne two.\nThree four five.");
    let pages: Vec<&str> = ch1
        .spans
        .iter()
        .filter_map(|s| match &s.kind {
            SpanKind::Page { label } => Some(label.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(pages, ["1", "2", "3", "iv", "5"]);
}

#[test]
fn what_is_not_a_gutenberg_text_is_refused() {
    assert!(import_pg_text("Just text.", &options("pg-text-bad"), |_, _| true).is_err());
    let empty = "*** START OF THE PROJECT GUTENBERG EBOOK X ***\n\n*** END OF THE PROJECT GUTENBERG EBOOK X ***\n";
    assert!(matches!(
        import_pg_text(empty, &options("pg-text-empty"), |_, _| true),
        Err(ImportError::NoText)
    ));
    assert!(matches!(
        import_pg_text(SAMPLE, &options("pg-text-cancel"), |_, _| false),
        Err(ImportError::Cancelled)
    ));
    // Unpaired underscores and an underscore alone neither panic nor style.
    let odd = "*** START OF THE PROJECT GUTENBERG EBOOK X ***\n\n_a __ b_ c _d\n\n*** END OF THE PROJECT GUTENBERG EBOOK X ***\n";
    let (r, _) = import(odd);
    assert!(r.units[0].text.contains('c'));
}

/// Every PG text in `$OBS_PG_DIR` imports with no trace of Project Gutenberg (a manual check
/// of real files).
#[test]
#[ignore = "needs Project Gutenberg text files in OBS_PG_DIR"]
fn every_text_in_a_folder_imports() {
    let Ok(dir) = std::env::var("OBS_PG_DIR") else {
        return;
    };
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "txt"))
        .collect();
    names.sort();
    for path in names {
        let text = crate::decode_html_bytes(&std::fs::read(&path).unwrap());
        let Ok((r, report)) = import_pg_text(&text, &options("pg-text-test"), |_, _| true) else {
            println!("{}: refused", path.display());
            continue;
        };
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
        println!(
            "{}: {} divisions, {} refs, {} KB; {:?}",
            path.file_name().unwrap().to_string_lossy(),
            report.entries,
            report.scripture_refs,
            r.units.iter().map(|u| u.text.len()).sum::<usize>() / 1000,
            r.units
                .iter()
                .take(4)
                .map(|u| u.title.clone().unwrap_or_default())
                .collect::<Vec<_>>()
        );
    }
}
