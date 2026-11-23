#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use obs_model::{SpanKind, UnitId, UnitKind};

use super::*;

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/assets/tcp")
        .join(name);
    std::fs::read_to_string(path).unwrap()
}

fn options(id: &str) -> TeiBookOptions {
    TeiBookOptions {
        resource_id: id.into(),
        source_file: format!("{id}.xml"),
        tool_version: "test".into(),
        ..TeiBookOptions::default()
    }
}

fn import(xml: &str) -> (Resource, ImportReport) {
    import_tcp_tei(xml, &options("tcp-test"), |_, _| true).unwrap()
}

fn unit<'a>(r: &'a Resource, id: &str) -> &'a obs_model::TextUnit {
    r.unit(&UnitId::new(id).unwrap()).unwrap()
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

/// The structure of a TCP text, as TCP marks it up.
const SAMPLE: &str = r#"<TEI xmlns="http://www.tei-c.org/ns/1.0">
  <teiHeader>
    <fileDesc>
      <titleStmt><title>Own title</title></titleStmt>
      <publicationStmt>
        <idno type="DLPS">A99999</idno>
        <availability><p>This keyboarded and encoded edition is available for reuse,
          according to the terms of Creative Commons 0 1.0 Universal.</p></availability>
      </publicationStmt>
      <sourceDesc><biblFull>
        <titleStmt><title>Of the mortification of sin in believers: the 1. Necessity</title>
          <author>Owen, John, 1616-1683.</author></titleStmt>
        <publicationStmt><date>1668.</date></publicationStmt>
      </biblFull></sourceDesc>
    </fileDesc>
  </teiHeader>
  <text>
    <front>
      <div type="title_page"><pb facs="tcp:1:1"/><p>OF THE MORTIFICATION OF SIN</p></div>
      <div type="to_the_Christian_reader">
        <opener><salute>Christian Reader,</salute></opener>
        <p><hi>I</hi> shall in a few words acquaint thee with the en<g ref="char:EOLhyphen"/>suing
          Discourse.</p>
      </div>
    </front>
    <body>
      <div type="part" n="1">
        <head>Part I.</head>
        <div n="1" type="chapter">
          <pb n="1" facs="tcp:1:4"/>
          <head>CHAP. I. The foundation laid in Rom. viii. 13.</head>
          <p>If ye through the Spirit do mortifie the deeds of the body,<note place="margin">Rom. 8. 13.</note>
             ye shall live. The words are <foreign xml:lang="lat">verba</foreign>, plain, and
             <gap reason="illegible" extent="4 letters"><desc>••••</desc></gap> certain.
             <pb n="2" facs="tcp:1:5"/>The duty is mortification.</p>
          <lg><l>O God, our help</l><l>in ages past,</l></lg>
          <p>A list follows:<list><item>first,</item><item>second.</item></list>and more.</p>
          <figure><p>an engraving</p></figure>
        </div>
        <div n="2" type="chapter">
          <p>The second chapter has no head.</p>
        </div>
      </div>
    </body>
    <back>
      <div type="errata"><head>Errata.</head><p>Pag. 3. for sinne read sin.</p></div>
    </back>
  </text>
</TEI>"#;

#[test]
fn a_tcp_text_becomes_a_book_with_divisions_pages_notes_and_references() {
    let (r, report) = import(SAMPLE);
    let m = &r.manifest;
    assert_eq!(m.resource_type, obs_model::ResourceType::Book);
    assert_eq!(m.title, "Of the mortification of sin in believers");
    assert_eq!(m.abbreviation, "A99999");
    assert_eq!(m.publisher.as_deref(), Some("Owen, John, 1616-1683."));
    assert_eq!(m.edition.as_deref(), Some("1668."));
    assert_eq!(m.source_format, TCP_TEI_FORMAT);
    assert!(m.key_schemes.is_empty());
    assert_eq!(m.license, LICENSE_UNKNOWN, "the licence is the caller's");

    let titles: Vec<(&str, &str)> = r
        .units
        .iter()
        .filter(|u| u.kind == UnitKind::Division)
        .map(|u| (u.id.as_str(), u.title.as_deref().unwrap_or("")))
        .collect();
    assert_eq!(
        titles,
        [
            ("div.1", "Title page"),
            ("div.2", "To the Christian reader"),
            (
                "div.3",
                "Part I. · CHAP. I. The foundation laid in Rom. viii. 13."
            ),
            ("div.4", "Part I. · Chapter 2"),
            ("div.5", "Errata."),
        ]
    );

    // A word broken at the line's end is joined; italic, foreign, gaps.
    let reader = unit(&r, "div.2");
    assert!(
        reader
            .text
            .contains("I shall in a few words acquaint thee with the ensuing Discourse."),
        "{}",
        reader.text
    );
    assert_eq!(spans_of(reader, |k| *k == SpanKind::Italic), ["I"]);

    // The part's head opens its first chapter; the chapter's pages, notes and references.
    let ch1 = unit(&r, "div.3");
    assert!(ch1.text.starts_with("Part I.\nCHAP. I."), "{}", ch1.text);
    assert_eq!(
        spans_of(ch1, |k| matches!(k, SpanKind::Heading { .. })),
        ["Part I.", "CHAP. I. The foundation laid in Rom. viii. 13."]
    );
    let p = pages(ch1);
    assert_eq!(
        p.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>(),
        ["1", "2"]
    );
    assert!(
        ch1.text[p[0].0..].starts_with("CHAP. I."),
        "page 1 begins at the chapter's head"
    );
    assert!(ch1.text[p[1].0..].starts_with("The duty is mortification."));
    assert!(
        ch1.text
            .contains("The words are verba, plain, and •••• certain."),
        "{}",
        ch1.text
    );
    assert_eq!(spans_of(ch1, |k| *k == SpanKind::Foreign), ["verba"]);
    let refs = spans_of(ch1, |k| matches!(k, SpanKind::ScriptureRef { .. }));
    assert_eq!(refs, ["Rom. viii. 13"]);
    // The margin note is a footnote, with its own reference.
    let note = ch1
        .spans
        .iter()
        .find_map(|s| match &s.kind {
            SpanKind::NoteRef { note, .. } => Some(note.clone()),
            _ => None,
        })
        .unwrap();
    let note = r.unit(&note).unwrap();
    assert_eq!(note.text, "Rom. 8. 13.");
    assert_eq!(
        spans_of(note, |k| matches!(k, SpanKind::ScriptureRef { .. })),
        ["Rom. 8. 13"]
    );
    // Verse lines, and a list breaking a paragraph.
    let poetry = spans_of(ch1, |k| {
        *k == SpanKind::Para {
            style: ParaStyle::Poetry1,
        }
    });
    assert_eq!(poetry, ["O God, our help", "in ages past,"]);
    let list = spans_of(ch1, |k| {
        *k == SpanKind::Para {
            style: ParaStyle::List,
        }
    });
    assert_eq!(list, ["first,", "second."]);
    assert!(
        ch1.text
            .contains("A list follows:\nfirst,\nsecond.\nand more."),
        "{}",
        ch1.text
    );
    assert!(!ch1.text.contains("engraving"));
    assert_eq!(report.dropped_markup.get("figure"), Some(&1));
    assert_eq!(report.entries, 5);
    assert_eq!(report.notes, 1);
    assert_eq!(report.scripture_refs, 2);
}

/// A real Evans-TCP text: Edwards, *Sinners in the Hands of an Angry God* (1741).
#[test]
fn edwards_sinners_in_the_hands_of_an_angry_god_reads_whole() {
    let xml = fixture("N03833.xml");
    let header = tei_header(&Document::parse(&xml).unwrap());
    assert_eq!(header.id, "N03833");
    assert!(
        header
            .availability
            .contains("Creative Commons 0 1.0 Universal")
    );
    assert_eq!(header.language, "en");
    assert_eq!(read_tei_header(&xml).unwrap(), header);
    assert!(read_tei_header("<TEI><text>").is_err());
    let (r, report) = import(&xml);
    assert_eq!(
        r.manifest.title,
        "Sinners in the hands of an angry God. A sermon"
    );
    let text: String = r
        .units
        .iter()
        .filter(|u| u.kind == UnitKind::Division)
        .map(|u| u.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text.contains("Their Foot shall slide in due Time"),
        "epigraph kept"
    );
    assert!(
        text.contains("unbelieving Israelites"),
        "a word broken at the line end is joined"
    );
    assert!(!text.contains("Isra elites"));
    let all_pages: Vec<String> = r.units.iter().flat_map(pages).map(|(_, l)| l).collect();
    assert_eq!(all_pages.first().map(String::as_str), Some("3"));
    assert!(all_pages.len() >= 20, "{all_pages:?}");
    assert!(report.scripture_refs > 0);
}

/// A real EEBO-TCP Phase II text: the Westminster Assembly's Confession (1646), released under
/// TCP's CC0 waiver.
#[test]
fn the_westminster_confession_of_1646_reads_by_chapter() {
    let xml = fixture("A96226.xml");
    let header = tei_header(&Document::parse(&xml).unwrap());
    assert_eq!(header.id, "A96226");
    assert!(
        header
            .availability
            .contains("CC0 1.0 Public Domain Dedication")
    );
    let (r, report) = import(&xml);
    assert!(r.units.iter().any(|u| {
        u.title
            .as_deref()
            .is_some_and(|t| t.contains("CHAP. XXXIII."))
    }));
    let chapters = r
        .units
        .iter()
        .filter(|u| {
            u.kind == UnitKind::Division && u.title.as_deref().unwrap_or("").contains("CHAP")
        })
        .count();
    assert!(
        chapters >= 30,
        "{:?}",
        r.units.iter().map(|u| u.title.clone()).collect::<Vec<_>>()
    );
    // The 1646 first edition has no Scripture proofs (they came in 1647): no references.
    assert_eq!(report.scripture_refs, 0);
    let pages: usize = r.units.iter().map(|u| pages(u).len()).sum();
    assert!(pages >= 50, "{pages}");
}

/// Several works printed together: `<text><group><text>…</text><text>…</text></group></text>`,
/// and a group inside a `<front>`; each work's Divisions in turn.
#[test]
fn works_printed_together_read_in_turn() {
    let xml = r#"<TEI><text><front><div type="title_page"><p>Two treatises</p></div>
        <group><text><body><div type="preface"><p>Inside the front.</p></div></body></text></group>
        </front><group>
        <text><body><div type="treatise"><head>The first treatise.</head><p>One.</p></div></body></text>
        <text><front><div type="title_page"><p>The second</p></div></front>
          <body><div type="treatise"><head>The second treatise.</head><p>Two.</p></div></body></text>
        </group></text></TEI>"#;
    let (r, report) = import(xml);
    let titles: Vec<&str> = r
        .units
        .iter()
        .filter(|u| u.kind == UnitKind::Division)
        .map(|u| u.title.as_deref().unwrap_or(""))
        .collect();
    assert_eq!(
        titles,
        [
            "Title page",
            "Preface",
            "The first treatise.",
            "Title page",
            "The second treatise."
        ]
    );
    assert_eq!(report.entries, 5);
}

/// TCP's marks the reading font has no glyph for are written as their equivalents; an
/// ornament is dropped, so a word broken at the line's end around it is still joined.
#[test]
fn tcp_marks_are_written_in_glyphs_the_reading_font_has() {
    let xml = "<TEI><text><body><div><head><g ref=\"char:leaf\">\u{2767}</g> THE CON<g ref=\"char:EOLhyphen\"/>\n\
               <g ref=\"char:leaf\">\u{2767}</g><lb/>tentes</head>\
               <p>A <gap reason=\"illegible\"><desc>\u{3008}\u{25ca}\u{3009}</desc></gap> word<g ref=\"char:punc\">\u{25aa}</g> \
               o\u{254c}pen \u{271a} \u{261e} see<gap reason=\"missing\"/>.</p></div></body></text></TEI>";
    let (r, report) = import(xml);
    let u = &r.units[0];
    assert_eq!(u.title.as_deref(), Some("THE CONtentes"));
    assert_eq!(
        u.text,
        "THE CONtentes\nA \u{27e8}\u{25ca}\u{27e9} word\u{b7} o-pen + \u{2192} see\u{27e8}\u{2026}\u{27e9}."
    );
    assert_eq!(report.dropped_markup.get("ornament"), Some(&2));
}

/// A sermon's long head is not a prefix for its parts: the first part takes it as its title,
/// the rest their own.
#[test]
fn a_long_part_title_titles_its_first_part_and_prefixes_none() {
    let xml = "<TEI><text><body><div type=\"text\"><head>THE BLACK DEVILL OR The APOSTATE.</head>\
               <div type=\"part\"><head>The Person</head><p>One.</p></div>\
               <div type=\"part\"><head>1. By Nature</head><p>Two.</p></div>\
               </div></body></text></TEI>";
    let (r, _) = import(xml);
    let titles: Vec<&str> = r
        .units
        .iter()
        .map(|u| u.title.as_deref().unwrap_or(""))
        .collect();
    assert_eq!(
        titles,
        ["THE BLACK DEVILL OR The APOSTATE.", "1. By Nature"]
    );
}

#[test]
fn what_is_not_a_tcp_text_is_refused_plainly() {
    for bad in [
        "",
        "not xml",
        "<TEI><teiHeader/></TEI>",
        "<TEI><text><body/></text></TEI>",
    ] {
        assert!(
            import_tcp_tei(bad, &options("tcp-bad"), |_, _| true).is_err(),
            "{bad:?}"
        );
    }
    // A DTD (entity expansion) is refused.
    let dtd = r#"<!DOCTYPE TEI [<!ENTITY a "aaaaaaaaaa">]><TEI><text><body><p>&a;</p></body></text></TEI>"#;
    assert!(matches!(
        import_tcp_tei(dtd, &options("tcp-dtd"), |_, _| true),
        Err(ImportError::Xml(_))
    ));
    assert!(matches!(
        import_tcp_tei(SAMPLE, &options("Not An Id"), |_, _| true),
        Err(ImportError::InvalidResourceId(_))
    ));
    assert!(matches!(
        import_tcp_tei(SAMPLE, &options("tcp-cancel"), |_, _| false),
        Err(ImportError::Cancelled)
    ));
}

#[test]
fn deep_nesting_and_many_notes_neither_panic_nor_hang() {
    let deep = format!(
        "<TEI><text><body><div><p>{}x{}</p></div></body></text></TEI>",
        "<hi>".repeat(5_000),
        "</hi>".repeat(5_000)
    );
    let (r, _) = import(&deep);
    assert!(r.units[0].text.contains('x'));
    let notes = format!(
        "<TEI><text><body><div><p>{}</p></div></body></text></TEI>",
        "a<note>n</note>".repeat(5_000)
    );
    let (_, report) = import(&notes);
    assert_eq!(report.notes, 5_000);
}

/// TCP keys an abbreviation stroke as a combining macron in an element of its own after its
/// letter (`whe<g ref="char:cmbAbbrStroke">̄</g>`): the text is still NFC ("whē"), its spans
/// moved with it.
#[test]
fn a_combining_mark_in_its_own_element_composes_with_its_letter() {
    let xml = "<TEI><text><body><div><p><hi>whe</hi><g ref=\"char:cmbAbbrStroke\">\u{304}</g> \
               calle<pb n=\"2\"/>d</p></div></body></text></TEI>";
    let (r, _) = import(xml);
    let u = &r.units[0];
    assert_eq!(u.text, "wh\u{113} called");
    assert!(unicode_normalization::is_nfc(&u.text));
    // The italic run ended between the letter and its mark: it takes the composed letter.
    assert_eq!(spans_of(u, |k| *k == SpanKind::Italic), ["wh\u{113}"]);
    assert_eq!(pages(u), [(u.text.len() - 1, "2".to_string())]);
}

/// TCP titles are whole title pages: cut at the subtitle or the statement of responsibility.
#[test]
fn short_titles_cut_where_the_title_page_goes_on() {
    for (full, short) in [
        (
            "The bruised reede, and smoaking flax Some sermons contracted out of the 12. of \
             Matth. 20. At the desire, and for the good of weaker Christians. By R. Sibbes",
            "The bruised reede, and smoaking flax Some sermons contracted out of the 12. of \
             Matth. 20. At the…",
        ),
        (
            "The Churches deliuerance contayning meditations and short notes vppon the booke \
             of Hester",
            "The Churches deliuerance",
        ),
        (
            "A treatise of faith and repentance by Francis Fuller",
            "A treatise of faith and repentance",
        ),
        ("Bonifacius. An essay upon the good", "Bonifacius"),
        (
            "Joshua redivivus, or, Mr. Rutherfoord's letters",
            "Joshua redivivus, or, Mr. Rutherfoord's letters",
        ),
        (
            "A letter written by Mr. Cotton",
            "A letter written by Mr. Cotton",
        ),
        (
            "The mischief of sinne it brings a person low / published by Thomas Watson",
            "The mischief of sinne it brings a person low",
        ),
        ("Grace", "Grace"),
    ] {
        assert_eq!(short_title(full), short, "{full:?}");
    }
}

#[test]
fn short_titles_cut_at_the_first_colon() {
    assert_eq!(short_title("A: b; c"), "A");
    assert_eq!(
        short_title("The bruised reede, and smoaking flax; Some sermons"),
        "The bruised reede, and smoaking flax"
    );
    assert_eq!(short_title("Plain."), "Plain");
    let long = short_title(&"word ".repeat(60));
    assert!(
        long.ends_with("word…") && long.chars().count() <= 121,
        "{long}"
    );
}

/// Every TCP text in `$OBS_TCP_DIR` imports (a manual check of the pilot's texts).
#[test]
#[ignore = "needs TCP texts in OBS_TCP_DIR"]
fn every_text_in_a_folder_imports() {
    let Ok(dir) = std::env::var("OBS_TCP_DIR") else {
        return;
    };
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "xml"))
        .collect();
    names.sort();
    for path in names {
        let xml = std::fs::read_to_string(&path).unwrap();
        let started = std::time::Instant::now();
        let (r, report) = import(&xml);
        let pages: usize = r.units.iter().map(|u| pages(u).len()).sum();
        let chars: usize = r.units.iter().map(|u| u.text.len()).sum();
        println!(
            "{} {:?}: {} divisions, {} notes, {} pages, {} refs, {} KB text, dropped {:?}, {:?}",
            path.file_name().unwrap().to_string_lossy(),
            r.manifest.title,
            report.entries,
            report.notes,
            pages,
            report.scripture_refs,
            chars / 1000,
            report.dropped_markup,
            started.elapsed()
        );
        for u in r
            .units
            .iter()
            .filter(|u| u.kind == UnitKind::Division)
            .take(40)
        {
            println!("    {}", u.title.as_deref().unwrap_or(""));
        }
    }
}
