//! Brown-Driver-Briggs from the Open Scriptures Hebrew Bible project's lexicon:
//! `BrownDriverBriggs.xml` for the entries and `LexicalIndex.xml` for their Strong's numbers.
//!
//! One unit per BDB `<entry id>` (the id, `b.cw.aa`, is the unit id). The lexical index maps a
//! BDB id to a Strong's number with OSHB's augment letter (`strong="1254" aug="a"` → `H1254A`)
//! and to the pointed headword, which becomes the Lemma Key (the entry's first `<w>` when the
//! index has none). Markup: the headword `<w>` bold, `<def>` bold, `<pos>`/`<stem>`/`<asp>`/
//! `<em>` italic, `<ref r>` → `scripture_ref`, `<w src>` → linked to the Strong's number of
//! the entry it points at, `<sense n>` → a new block numbered `n.`. `<status>` and `<page>`
//! are editorial and left out.

use std::collections::HashMap;

use obs_model::{Resource, SpanKind};
use obs_scripture::StrongsNumber;
use roxmltree::Node;

use super::{Entry, LexiconError, LexiconOptions, LexiconWriter, parse_xml};
use crate::report::ImportReport;

/// What the lexical index says about one BDB entry.
#[derive(Debug, Default, Clone)]
struct Indexed {
    strongs: Vec<StrongsNumber>,
    lemma: Option<String>,
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
            if slot.lemma.is_none() {
                slot.lemma = lemma.map(str::to_string);
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
        let lemma = indexed.lemma.clone().or(first_w).unwrap_or_default();
        let mut e = Entry::new(id, lemma.as_str());
        for n in &indexed.strongs {
            e.key_strongs(*n);
        }
        e.key_lemma(&lemma);
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
                e.scripture_ref(start, child.attribute("r").unwrap_or_default());
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
