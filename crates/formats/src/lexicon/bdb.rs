//! Brown-Driver-Briggs from the Open Scriptures Hebrew Bible project's lexicon:
//! `BrownDriverBriggs.xml` for the entries and `LexicalIndex.xml` for their Strong's numbers.
//!
//! One unit per BDB `<entry id>` (the id, `b.cw.aa`, is the unit id). The lexical index maps a
//! BDB id to a Strong's number with OSHB's augment letter (`strong="1254" aug="a"` → `H1254A`)
//! and to all distinct indexed headwords, which become Lemma Keys (the entry's first `<w>` when the
//! index has none). Markup: the headword `<w>` bold, `<def>` bold, `<pos>`/`<stem>`/`<asp>`/
//! `<em>` italic, `<ref r>` → `scripture_ref`, `<w src>` → linked to the Strong's number of
//! the entry it points at, `<sense n>` → a new block numbered `n.`. `<status>` and `<page>`
//! are editorial and left out. Hebrew-numbered references normalize through TVTMS before
//! becoming Standard-versification links.

use std::collections::HashMap;

use obs_model::{Resource, SpanKind};
use obs_scripture::StrongsNumber;
use roxmltree::Node;

use super::{Entry, LexiconError, LexiconOptions, LexiconWriter, parse_osis_ref_in, parse_xml};
use crate::report::ImportReport;
use crate::verse_map::HebrewReferences;

/// What the lexical index says about one BDB entry.
#[derive(Debug, Default, Clone)]
struct Indexed {
    strongs: Vec<StrongsNumber>,
    lemmas: Vec<String>,
}

fn read_index(xml: &str) -> Result<HashMap<String, Indexed>, LexiconError> {
    let doc = parse_xml("LexicalIndex.xml", xml)?;
    let mut map: HashMap<String, Indexed> = HashMap::new();
    for entry in doc.descendants().filter(|n| n.has_tag_name("entry")) {
        let lemma = entry
            .children()
            .find(|c| c.has_tag_name("w"))
            .and_then(|w| w.text())
            .map(str::trim)
            .filter(|t| !t.is_empty());
        for xref in entry.children().filter(|c| c.has_tag_name("xref")) {
            let Some(bdb) = xref.attribute("bdb") else {
                continue;
            };
            let slot = map.entry(bdb.to_string()).or_default();
            if let Some(lemma) = lemma
                && !slot.lemmas.iter().any(|l| l == lemma)
            {
                slot.lemmas.push(lemma.to_string());
            }
            let number = xref.attribute("strong").and_then(|s| {
                format!("H{}{}", s.trim(), xref.attribute("aug").unwrap_or_default())
                    .parse::<StrongsNumber>()
                    .ok()
            });
            if let Some(n) = number
                && !slot.strongs.contains(&n)
            {
                slot.strongs.push(n);
            }
        }
    }
    Ok(map)
}

/// Convert BDB with its lexical index.
pub fn import_bdb(
    o: &LexiconOptions,
    bdb_xml: &str,
    lexical_index_xml: &str,
) -> Result<(Resource, ImportReport), LexiconError> {
    let index = read_index(lexical_index_xml)?;
    let doc = parse_xml("BrownDriverBriggs.xml", bdb_xml)?;
    let mut w = LexiconWriter::new(
        o,
        "oshb-bdb",
        vec!["BrownDriverBriggs.xml".into(), "LexicalIndex.xml".into()],
    )?;
    for entry in doc.descendants().filter(|n| n.has_tag_name("entry")) {
        let Some(id) = entry.attribute("id") else {
            w.drop_markup("<entry> without an id");
            continue;
        };
        let indexed = index.get(id).cloned().unwrap_or_default();
        let first_w = entry
            .children()
            .find(|c| c.has_tag_name("w"))
            .map(|c| text_of(c).trim().to_string())
            .filter(|t| !t.is_empty());
        let lemmas = if indexed.lemmas.is_empty() {
            first_w.into_iter().collect()
        } else {
            indexed.lemmas
        };
        let mut e = Entry::new(id, lemmas.first().map(String::as_str).unwrap_or_default());
        for n in &indexed.strongs {
            e.key_strongs(*n);
        }
        for lemma in &lemmas {
            e.key_lemma(lemma);
        }
        let mut headword_done = false;
        inline(&mut e, entry, &index, &mut headword_done);
        w.add(e);
    }
    w.finish()
}

fn text_of(node: Node) -> String {
    node.descendants()
        .filter(Node::is_text)
        .filter_map(|n| n.text())
        .collect()
}

fn inline(e: &mut Entry, node: Node, index: &HashMap<String, Indexed>, headword_done: &mut bool) {
    for child in node.children() {
        if child.is_text() {
            e.text(child.text().unwrap_or_default());
            continue;
        }
        let name = child.tag_name().name();
        if !child.is_element() || matches!(name, "status" | "page") {
            continue;
        }
        if name == "sense" {
            e.block();
            if let Some(n) = child.attribute("n") {
                e.text(&format!("{n}. "));
            }
            inline(e, child, index, headword_done);
            e.block();
            continue;
        }
        let start = e.mark();
        match name {
            "w" => {
                inline(e, child, index, headword_done);
                if !*headword_done {
                    *headword_done = true;
                    e.span(start, SpanKind::Bold);
                }
                let target = child
                    .attribute("src")
                    .and_then(|src| index.get(src))
                    .and_then(|i| i.strongs.first());
                if let Some(n) = target {
                    e.strongs_link(start, *n);
                }
            }
            "def" => {
                inline(e, child, index, headword_done);
                e.span(start, SpanKind::Bold);
            }
            "pos" | "stem" | "asp" | "em" => {
                inline(e, child, index, headword_done);
                e.span(start, SpanKind::Italic);
            }
            "ref" => {
                inline(e, child, index, headword_done);
                let hebrew = HebrewReferences::get();
                let targets =
                    parse_osis_ref_in(child.attribute("r").unwrap_or_default(), &hebrew.numbering)
                        .and_then(|native| hebrew.to_standard(&native));
                e.scripture_targets(start, targets);
            }
            "foreign" => inline(e, child, index, headword_done),
            other => {
                e.drop_markup(&format!("<{other}>"));
                inline(e, child, index, headword_done);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX: &str = r#"<index xmlns="http://openscriptures.github.com/morphhb/namespace"><part xml:lang="heb">
        <entry id="bxy"><w xlit="bārāʾ">בָּרָא</w> <pos>V</pos> <def>shape</def>
            <xref bdb="b.cw.aa" strong="1254" aug="a" twot="278"/></entry>
        <entry id="bxx"><w xlit="bārāʾ">בָּרָא</w><xref bdb="b.cx.aa" strong="1254" aug="b"/></entry>
    </part></index>"#;

    const BDB: &str = r#"<lexicon xmlns="http://openscriptures.github.com/morphhb/namespace"><part id="b"><section id="b.cw">
        <entry id="b.cw.aa" type="root"><w>בָּרָא</w> <pos>vb</pos>. <def>shape</def>, <def>create</def>
            <sense n="1"><stem>Qal</stem> <ref r="Gen.1.1">Gn 1:1</ref>; cf. <w src="b.cx.aa">ברא</w> II.</sense>
            <status p="135">base</status></entry>
        <entry id="b.cw.ab"><w>בְּרִיאָה</w> <ref r="Num.16.30">Nu 16:30</ref> <ref r="Sir.1.1">Si 1:1</ref><page p="135"/></entry>
    </section></part></lexicon>"#;

    #[test]
    fn hebrew_references_link_to_exact_standard_targets() {
        use obs_scripture::{Book, Passage, Verse};
        let cases = [
            ("Ps.41.6", Book::Psalms, 41, 5),
            ("Num.17.27", Book::Numbers, 17, 12),
            ("Ps.51.1-2", Book::Psalms, 51, 0),
            ("Gen.32.1", Book::Genesis, 31, 55),
            ("Gen.32.33", Book::Genesis, 32, 32),
            ("Mal.3.24", Book::Malachi, 4, 6),
            ("Gen.1.1", Book::Genesis, 1, 1),
        ];
        for (reference, book, chapter, verse) in cases {
            let xml = format!(
                "<lexicon><entry id='a.ac.aa'><w>אָבַד</w><ref r='{reference}'>citation</ref></entry></lexicon>"
            );
            let (r, report) = import_bdb(
                &LexiconOptions {
                    resource_id: "bdb".into(),
                    ..LexiconOptions::default()
                },
                &xml,
                "<index/>",
            )
            .unwrap();
            let targets = r.units[0]
                .spans
                .iter()
                .find_map(|s| match &s.kind {
                    SpanKind::ScriptureRef { targets } => Some(targets),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("no link for {reference}"));
            assert_eq!(
                targets.passages(),
                [Passage::verse(Verse::new(book, chapter, verse))],
                "{reference}"
            );
            assert_eq!(report.scripture_refs, 1);
        }
    }

    #[test]
    fn shared_entry_keeps_each_lemma_and_its_transliteration() {
        let index = r#"<index>
            <entry><w>אבדה</w><xref bdb="a.ac.af" strong="10"/></entry>
            <entry><w>אֲבַדּוֹן</w><xref bdb="a.ac.af" strong="11"/></entry>
            <entry><w>אֲבַדּוֹן</w><xref bdb="a.ac.af" strong="11"/></entry>
        </index>"#;
        let xml =
            r#"<lexicon><entry id="a.ac.af"><w>אֲבַדּוֹן</w><def>Destruction</def></entry></lexicon>"#;
        let (r, _) = import_bdb(
            &LexiconOptions {
                resource_id: "bdb".into(),
                ..LexiconOptions::default()
            },
            xml,
            index,
        )
        .unwrap();
        for lemma in ["אבדה", "אֲבַדּוֹן"] {
            let keys: Vec<_> = r
                .keys
                .iter()
                .filter(|k| k.key == obs_model::Key::Lemma(lemma.into()))
                .collect();
            assert_eq!(keys.len(), 1);
            assert_eq!(keys[0].unit.as_str(), "a.ac.af");
            assert!(
                r.transliterations
                    .iter()
                    .any(|t| t.lemma == lemma
                        && t.transliteration == obs_model::transliterate(lemma))
            );
        }
        for number in ["H10", "H11"] {
            assert!(
                r.keys
                    .iter()
                    .any(|k| k.key.to_string() == number && k.unit.as_str() == "a.ac.af")
            );
        }
    }

    #[test]
    fn entries_are_keyed_through_the_lexical_index() {
        let o = LexiconOptions {
            resource_id: "bdb".into(),
            ..LexiconOptions::default()
        };
        let (r, report) = import_bdb(&o, BDB, INDEX).unwrap();
        let u = &r.units[0];
        assert_eq!(u.id.as_str(), "b.cw.aa");
        assert_eq!(u.text, "בָּרָא vb. shape, create\n1. Qal Gn 1:1; cf. ברא II.");
        let spans: Vec<(&str, &str)> = u
            .spans
            .iter()
            .filter(|s| s.kind.name() != "para")
            .map(|s| (s.kind.name(), &u.text[s.range()]))
            .collect();
        assert_eq!(
            spans,
            [
                ("bold", "בָּרָא"),
                ("italic", "vb"),
                ("bold", "shape"),
                ("bold", "create"),
                ("italic", "Qal"),
                ("scripture_ref", "Gn 1:1"),
                ("tagged_word", "ברא"),
            ]
        );
        let keys: Vec<(String, String, bool)> = r
            .keys
            .iter()
            .map(|k| (k.key.to_string(), k.unit.to_string(), k.alias))
            .collect();
        assert!(keys.contains(&("H1254A".into(), "b.cw.aa".into(), false)));
        assert!(keys.contains(&("H1254".into(), "b.cw.aa".into(), true)));
        assert!(keys.contains(&("בְּרִיאָה".into(), "b.cw.ab".into(), false)));
        assert_eq!((report.entries, report.scripture_refs), (2, 2));
        assert_eq!(
            r.dropped_markup
                .get("ref outside the Standard versification"),
            Some(&1)
        );
    }
}
