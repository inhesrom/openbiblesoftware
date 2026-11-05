use super::*;
use obs_model::{Manifest, ResourceType, SpanKind};
use obs_scripture::{Book, Standard, Verse, parse_reference};

fn import(kind: ResourceType) -> StudyImport {
    StudyImport::new(Manifest::new("fixture", "Fixture", "F", kind, "en"))
}

#[test]
fn roman_and_online_references_are_standard_passages() {
    for (source, expected) in [
        ("Eph. iv. 21", "Eph 4:21"),
        ("John iv. 21, &c.; 1 Tim. ii. 8", "John 4:21; 1 Tim 2:8"),
        ("3Jo+1:1-14", "3 John 1:1-14"),
        ("Bible:Gen.11.27", "Gen 11:27"),
    ] {
        assert_eq!(
            references::parse(source, None, true).unwrap_or_else(|| panic!("{source}")),
            parse_reference(expected, &Standard, None).unwrap(),
            "{source}"
        );
    }
    assert!(references::parse("Gen 999:1", None, false).is_none());
}

#[test]
fn mhc_sections_intro_markup_and_notes() {
    let xml = r#"<mediawiki><page><title>Matthew Henry's Commentary on the Whole Bible/Volume 5/John</title><revision><text>{{header|title=test}}
=Preface=
An introduction.
=CHAP. 3.=
Chapter introduction.
==Verses 16-18==
====God's love====
''He loved'' {{small-caps|the world}}. [[Bible (King James)|Eph. iv. 21]]&lt;ref&gt;A note.&lt;/ref&gt; Next word.
&lt;poem&gt;First line
Second line&lt;/poem&gt;
==Versees 19-21==
More text.
</text></revision></page></mediawiki>"#;
    let mut i = import(ResourceType::Commentary);
    import_mhc(&mut i, xml).unwrap();
    let (r, report) = i.finish().unwrap();
    let p = parse_reference("John 3:16-18", &Standard, None)
        .unwrap()
        .passages()[0];
    let key = r.keys.iter().find(|k| k.key == Key::Passage(p)).unwrap();
    let unit = r.units.iter().find(|u| u.id == key.unit).unwrap();
    assert!(
        unit.text.contains("He loved the world. Eph. iv. 21"),
        "{}",
        unit.text
    );
    assert!(
        unit.spans
            .iter()
            .any(|s| matches!(s.kind, SpanKind::ScriptureRef { .. }))
    );
    assert!(
        unit.spans
            .iter()
            .any(|s| matches!(s.kind, SpanKind::Italic))
    );
    assert!(
        unit.spans
            .iter()
            .any(|s| matches!(s.kind, SpanKind::Heading { .. }))
    );
    assert!(unit.text.contains("Eph. iv. 21 Next word."));
    assert!(unit.text.contains("First line\nSecond line"));
    assert_eq!(report.notes, 1);
    assert_eq!(r.keys.len(), 4);
    r.validate().unwrap();
}

#[test]
fn jfb_empty_anchors_extend_sections_without_empty_units_or_navigation() {
    let mut i = import(ResourceType::Commentary);
    import_jfb_book(&mut i,Book::Genesis,r#"<TABLE>Navigation</TABLE><A NAME="Ge1_1"> </A><P><B>1.</B> Creation <A HREF="http://bible.gospelcom.net/bible?passage=Joh+1:3">John 1:3</A>.</P><A NAME="Ge1_2"> </A><A NAME="Ge1_2"> </A><A NAME="Ge1_3"> </A><P>Light.</P><HR><TABLE>Next</TABLE>"#).unwrap();
    let (r, _) = i.finish().unwrap();
    assert_eq!(r.units.len(), 2);
    assert!(r.keys.iter().any(|k| {
        k.key
            == Key::Passage(
                Passage::new(
                    Verse::new(Book::Genesis, 1, 1),
                    Verse::new(Book::Genesis, 1, 2),
                )
                .unwrap(),
            )
    }));
    assert!(
        !r.units
            .iter()
            .any(|u| u.text.contains("Navigation") || u.text.contains("Next"))
    );
    assert!(
        r.units[0]
            .spans
            .iter()
            .any(|s| matches!(s.kind, SpanKind::ScriptureRef { .. }))
    );
}

#[test]
fn smith_headword_and_alias_preserve_both_entries() {
    let data=b"ABRAHAM\r\nFather. <scripRef passage=\"ge 12:1\">Genesis 12:1</scripRef>ABRAM\r\nEarlier name.";
    let split = data.windows(7).position(|s| s == b"ABRAM\r\n").unwrap();
    let mut index = Vec::new();
    for (off, len) in [(0, split), (split, data.len() - split)] {
        index.extend_from_slice(&(off as u32).to_le_bytes());
        index.extend_from_slice(&(len as u16).to_le_bytes());
    }
    let mut i = import(ResourceType::Dictionary);
    import_dictionary(
        &mut i,
        DictionaryData::Smith {
            index: &index,
            data,
        },
    )
    .unwrap();
    let (r, _) = i.finish().unwrap();
    assert_eq!(r.units.len(), 2);
    assert!(r.keys.iter().any(|k| k.key == Key::Headword("Abram".into())
        && k.alias
        && k.unit.as_str() == "Abraham"));
    assert!(
        r.keys
            .iter()
            .any(|k| k.key == Key::Headword("Abraham".into()) && !k.alias)
    );
    assert!(
        r.units[0]
            .spans
            .iter()
            .any(|s| matches!(s.kind, SpanKind::ScriptureRef { .. }))
    );
}

#[test]
fn fallback_paragraphs_exclude_heading_text_even_with_unwrapped_prose() {
    let text = html::render(
        "Before <title>Heading</title> after",
        None,
        &mut ImportReport::default(),
    );
    let structural: Vec<_> = text
        .spans
        .iter()
        .filter(|(_, kind)| matches!(kind, SpanKind::Para { .. } | SpanKind::Heading { .. }))
        .collect();
    assert_eq!(structural.len(), 3);
    for (i, (range, _)) in structural.iter().enumerate() {
        for (other, _) in &structural[i + 1..] {
            assert!(range.end <= other.start || other.end <= range.start);
        }
    }
    assert!(
        structural
            .iter()
            .any(|(range, kind)| matches!(kind, SpanKind::Heading { .. })
                && &text.text[range.clone()] == "Heading")
    );
}

#[test]
fn malformed_dictionary_offsets_fail_without_panicking() {
    let mut i = import(ResourceType::Dictionary);
    assert!(
        import_dictionary(
            &mut i,
            DictionaryData::Smith {
                index: &[255; 6],
                data: b""
            }
        )
        .is_err()
    );
}

#[test]
fn mhc_contextual_verse_and_chapter_references_do_not_confuse_roman_five() {
    let context = Some(Verse::new(Book::John, 3, 1));
    for (source, expected) in [
        ("v. 16", "John 3:16"),
        ("John v. 16", "John 5:16"),
        ("ch. iv. 21", "John 4:21"),
        ("ch. xvii. xviii. xix.", "John 17; John 18; John 19"),
    ] {
        assert_eq!(
            references::parse(source, context, true).unwrap(),
            parse_reference(expected, &Standard, None).unwrap(),
            "{source}"
        );
    }
}

#[test]
fn jfb_missing_verse_anchor_is_repaired_only_in_its_own_chapter() {
    let mut i = import(ResourceType::Commentary);
    import_jfb_book(&mut i,Book::Mark,r#"<A NAME="Mr1_21"> </A><B>21.</B> First chapter.<A NAME="Chapter4"> </A><A NAME="Mr4_20"> </A><B>20.</B> Fourth chapter.<B>21.</B> A candle.<A NAME="Mr4_22"> </A><B>22.</B> Manifested."#).unwrap();
    let (r, _) = i.finish().unwrap();
    let key = r
        .keys
        .iter()
        .find(|k| matches!(k.key,Key::Passage(p) if p.start()==Verse::new(Book::Mark,4,21)))
        .unwrap();
    let unit = r.units.iter().find(|u| u.id == key.unit).unwrap();
    assert!(unit.text.contains("A candle."));
    assert!(!unit.text.contains("First chapter"));
}

#[test]
fn jfb_leading_empty_anchor_and_section_title_keep_first_verse() {
    let mut i = import(ResourceType::Commentary);
    import_jfb_book(&mut i,Book::Exodus,r#"<A NAME="Ex9_1"> </A><A NAME="Ex9_2"> </A><P>Ex 9:1-7. MURRAIN OF BEASTS.</P><A NAME="Ex9_3"> </A><B>3-5.</B> Cattle."#).unwrap();
    let (r, _) = i.finish().unwrap();
    assert!(
        r.keys
            .iter()
            .any(|k| matches!(k.key,Key::Passage(p) if p.start()==Verse::new(Book::Exodus,9,1)))
    );
}

#[test]
fn prose_references_preserve_utf8_offsets_and_do_not_duplicate_links() {
    let mut report = ImportReport::default();
    let text = html::render(
        "é. Acts xvii. 24; <a data-reference=\"John iii. 16\">John iii. 16</a>. Gen 999:1.",
        None,
        &mut report,
    );
    let refs: Vec<_> = text
        .spans
        .iter()
        .filter(|(_, k)| matches!(k, SpanKind::ScriptureRef { .. }))
        .collect();
    assert_eq!(refs.len(), 2);
    assert_eq!(&text.text[refs[1].0.clone()], "Acts xvii. 24");
}

#[test]
fn prose_micah_references_are_not_roman_numerals() {
    for source in ["Mic. v. 7", "mic. v. 7", "MIC. V. 7"] {
        let text = html::render(source, None, &mut ImportReport::default());
        let expected = parse_reference("Micah 5:7", &Standard, None).unwrap();
        assert!(
            text.spans.iter().any(|(range, kind)| {
                &text.text[range.clone()] == source
                    && matches!(kind, SpanKind::ScriptureRef { targets } if targets == &expected)
            }),
            "{source}"
        );
    }
}

#[test]
fn prose_reference_lists_keep_contextual_chapter_continuations() {
    for (source, expected) in [
        ("Ps. cxxi. 2; cxxiv. 8", "Ps 121:2; 124:8"),
        ("Ps. civ. 13; lxv. 9, 10", "Ps 104:13; 65:9,10"),
        ("John i. 9; ix. 5", "John 1:9; 9:5"),
        ("Ps 121:2; 124:8", "Ps 121:2; 124:8"),
        ("Ps 121:2, 124:8", "Ps 121:2; 124:8"),
    ] {
        let text = html::render(
            &format!("é. {source}. Next sentence."),
            None,
            &mut ImportReport::default(),
        );
        let refs: Vec<_> = text
            .spans
            .iter()
            .filter_map(|(range, kind)| {
                if let SpanKind::ScriptureRef { targets } = kind {
                    Some((&text.text[range.clone()], targets))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            refs,
            vec![(source, &parse_reference(expected, &Standard, None).unwrap())]
        );
    }
}

#[test]
fn mhc_wiki_indentation_is_not_visible_text() {
    let mut i = import(ResourceType::Commentary);
    import_mhc(&mut i, r#"<mediawiki><page><title>MHC/Volume 1/Genesis/Chapter 1</title><revision><text>==Verses 1-2==
:'''I.''' First.

::'''1.''' Second.

:::'''(1.)''' Third: prose colon stays.
::'''2.''' Fourth.
</text></revision></page></mediawiki>"#).unwrap();
    let (resource, _) = i.finish().unwrap();
    assert_eq!(
        resource.units[0].text.trim_end(),
        "I. First.\n1. Second.\n(1.) Third: prose colon stays.\n2. Fourth."
    );
    assert!(
        !resource.units[0]
            .text
            .lines()
            .any(|line| line.starts_with(':'))
    );
}

#[test]
fn mhc_real_fixture_preserves_micah_and_psalm_targets_without_wiki_markers() {
    let mut i = import(ResourceType::Commentary);
    import_mhc(&mut i, include_str!("../../tests/fixtures/study/mhc.xml")).unwrap();
    let (resource, _) = i.finish().unwrap();
    for (source, expected) in [
        ("Mic. v. 7", "Micah 5:7"),
        ("Ps. cxxi. 2; cxxiv. 8", "Ps 121:2; 124:8"),
    ] {
        let expected = parse_reference(expected, &Standard, None).unwrap();
        assert!(
            resource.units.iter().any(|unit| {
                unit.spans.iter().any(|span| {
            &unit.text[span.range()] == source
                && matches!(&span.kind, SpanKind::ScriptureRef { targets } if targets == &expected)
        })
            }),
            "{source}"
        );
    }
    for unit in &resource.units {
        assert!(
            !unit.text.lines().any(|line| line.starts_with(':')),
            "{}",
            unit.id
        );
    }
}

#[test]
fn invalid_mhc_section_and_xml_fail_instead_of_shipping_bad_keys() {
    let mut i = import(ResourceType::Commentary);
    assert!(import_mhc(&mut i, "<mediawiki><page></mediawiki>").is_err());
    let mut i = import(ResourceType::Commentary);
    assert!(import_mhc(&mut i,"<mediawiki><page><title>MHC/Volume 1/Genesis/Chapter 1</title><revision><text>==Verses 40-44==\nInvalid.</text></revision></page></mediawiki>").is_err());
}

/// Note bodies (#64): references with a book and chapter:verse become Standard Passages with
/// their byte ranges; other numbers and bare chapters stay text.
#[test]
fn scripture_references_in_typed_text() {
    let text = "Compare Jn 3:16-18; Rom 5:8 with 1 Cor 13:4, 7. Job 5 times; I am 3:15 late. \
                Also Gen 99:1 and Mt 5:43; 1990 was a year. Then Psalm 23:1.";
    let found = scripture_references(text);
    let shown: Vec<(&str, String)> = found
        .iter()
        .map(|(r, l)| {
            (
                &text[r.clone()],
                obs_scripture::format_passage_list(l, &Standard, obs_scripture::RefStyle::Short),
            )
        })
        .collect();
    let want = |s: &str| {
        obs_scripture::format_passage_list(
            &parse_reference(s, &Standard, None).unwrap(),
            &Standard,
            obs_scripture::RefStyle::Short,
        )
    };
    assert_eq!(
        shown,
        vec![
            ("Jn 3:16-18; Rom 5:8", want("John 3:16-18; Romans 5:8")),
            ("1 Cor 13:4, 7", want("1 Corinthians 13:4, 7")),
            // "; 1990" is not a chapter of Matthew: the list stops before it.
            ("Mt 5:43", want("Matthew 5:43")),
            ("Psalm 23:1", want("Psalms 23:1")),
        ]
    );
    assert!(scripture_references("").is_empty());
    assert!(scripture_references("\u{202E}Jn 3:\u{FEFF}16").is_empty());
}

/// English words that are also book spellings (`Is`, `So`, `Re`, `Am`) are times or prose in a
/// Note, not references, unless written with a period; a list stops before one.
#[test]
fn english_words_are_not_books_in_typed_text_without_a_period() {
    let text = "Is 2:30 OK? So 3:15 then. Re 4:00 call. I Am 1:1 sure. Is. 53:5 and Isa 7:14; \
                Rom 5:8; So 3 of us. Gal 2:20, Is 4:1; Rev 3:20, Re. 22:21.";
    let shown: Vec<(&str, String)> = scripture_references(text)
        .iter()
        .map(|(r, l)| {
            (
                &text[r.clone()],
                obs_scripture::format_passage_list(l, &Standard, obs_scripture::RefStyle::Short),
            )
        })
        .collect();
    let want = |s: &str| {
        obs_scripture::format_passage_list(
            &parse_reference(s, &Standard, None).unwrap(),
            &Standard,
            obs_scripture::RefStyle::Short,
        )
    };
    assert_eq!(
        shown,
        vec![
            ("Is. 53:5", want("Isaiah 53:5")),
            // The list stops before "So 3".
            ("Isa 7:14; Rom 5:8", want("Isaiah 7:14; Romans 5:8")),
            ("Gal 2:20", want("Galatians 2:20")),
            (
                "Rev 3:20, Re. 22:21",
                want("Revelation 3:20; Revelation 22:21")
            ),
        ]
    );
}
