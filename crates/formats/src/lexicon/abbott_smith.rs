//! G. Abbott-Smith, *A Manual Greek Lexicon of the New Testament* (1922), TEI XML from the
//! translatable-exegetical-tools project.
//!
//! One unit per `<entry n="lemma|G1234|…">` in the `<body>`; the unit id is the lemma. The
//! `G` numbers in `n` become Strong's number Keys and the lemma a Lemma Key. A
//! `<superEntry>`'s own heading is written at the start of its first entry. Markup: `<orth>`
//! and `<gloss>` bold, `<emph>` and editorial `<note>`s italic, `<ref osisRef>` →
//! `scripture_ref`, `<foreign n="H2580">` → linked to that Strong's number, `<sense n>` and
//! `<etym>` → a new block (senses numbered as printed). The NT occurrence count
//! (`<note type="occurrencesNT">`) is left out and counted; page breaks are ignored.

use obs_model::{Resource, SpanKind};
use obs_scripture::StrongsNumber;
use roxmltree::Node;

use super::{Entry, LexiconError, LexiconOptions, LexiconWriter, parse_xml, strongs_word};
use crate::report::ImportReport;

/// Convert Abbott-Smith's TEI file.
pub fn import_abbott_smith(
    o: &LexiconOptions,
    xml: &str,
) -> Result<(Resource, ImportReport), LexiconError> {
    let file = if o.source_file.is_empty() {
        "abbott-smith.tei.xml".to_string()
    } else {
        o.source_file.clone()
    };
    let doc = parse_xml(&file, xml)?;
    let mut w = LexiconWriter::new(o, "abbott-smith-tei", vec![file.clone()])?;
    let Some(body) = doc.descendants().find(|n| n.has_tag_name("body")) else {
        return Err(LexiconError::Parse {
            file,
            message: "no TEI <body>".into(),
        });
    };
    visit(&mut w, body);
    w.finish()
}

fn visit(w: &mut LexiconWriter, node: Node) {
    for child in node.children().filter(Node::is_element) {
        match child.tag_name().name() {
            "entry" => convert(w, child, None),
            "superEntry" => {
                let heading: Vec<Node> = child
                    .children()
                    .filter(|c| !c.has_tag_name("entry"))
                    .collect();
                for (i, entry) in child
                    .children()
                    .filter(|c| c.has_tag_name("entry"))
                    .enumerate()
                {
                    convert(w, entry, (i == 0).then_some(heading.as_slice()));
                }
            }
            _ => visit(w, child),
        }
    }
}

fn convert(w: &mut LexiconWriter, entry: Node, heading: Option<&[Node]>) {
    let Some(n) = entry.attribute("n") else {
        w.drop_markup("<entry> without n");
        return;
    };
    let mut parts = n.split('|').map(str::trim).filter(|p| !p.is_empty());
    let lemma = parts.next().unwrap_or_default();
    let strongs: Vec<StrongsNumber> = parts.filter_map(strongs_word).collect();
    let mut e = Entry::new(lemma, lemma);
    for s in strongs {
        e.key_strongs(s);
    }
    e.key_lemma(lemma);
    if let Some(nodes) = heading {
        for node in nodes {
            one(&mut e, *node);
        }
        e.block();
    }
    inline(&mut e, entry);
    w.add(e);
}

fn inline(e: &mut Entry, node: Node) {
    for child in node.children() {
        one(e, child);
    }
}

fn one(e: &mut Entry, node: Node) {
    if node.is_text() {
        e.text(node.text().unwrap_or_default());
        return;
    }
    if !node.is_element() {
        return;
    }
    let name = node.tag_name().name();
    match name {
        "pb" => return,
        "lb" => {
            e.text(" ");
            return;
        }
        "note" if node.attribute("type") == Some("occurrencesNT") => {
            e.drop_markup("<note type=occurrencesNT>");
            return;
        }
        "sense" | "etym" => {
            e.block();
            if let Some(n) = node.attribute("n") {
                e.text(&format!("{n} "));
            }
            inline(e, node);
            e.block();
            return;
        }
        _ => {}
    }
    let start = e.mark();
    inline(e, node);
    match name {
        "orth" | "gloss" => e.span(start, SpanKind::Bold),
        "emph" | "note" => e.span(start, SpanKind::Italic),
        "ref" => {
            if let Some(osis) = node.attribute("osisRef") {
                e.scripture_ref(start, osis);
            }
        }
        "foreign" => {
            if let Some(n) = node.attribute("n").and_then(strongs_word) {
                e.strongs_link(start, n);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_with_strongs_lemmas_refs_and_links() {
        let xml = r#"<TEI xmlns="http://www.crosswire.org/2013/TEIOSIS/namespace"><text><front><entry n="front|G1"/></front><body><div type="letter" n="Χ">
<entry n="χάρις|G5485">
  <note type="occurrencesNT">155</note>
  <form><orth>χάρις, -ιτος, ἡ</orth>, acc., <foreign xml:lang="grc">χάριν</foreign> (<ref osisRef="Acts.24.27">Ac 24:27</ref>), </form>
<etym><seg type="septuagint">[in LXX chiefly for <foreign xml:lang="heb" n="H2580">חֵן</foreign> ;]</seg></etym>
  <sense><sense n="1.">objectively, <gloss>grace</gloss> (<ref osisRef="Sir.21.16">Si 21:16</ref>): <ref osisRef="Col.4.6">Col 4:6</ref>; <pb n="479"/></sense></sense>
</entry>
<superEntry n="γαμίζω|γαμίσκω|G1061"><form><orth>γαμ-</orth> forms</form>
  <entry n="γαμίζω"><sense><gloss>to give in marriage</gloss></sense></entry>
  <entry n="γαμίσκω|G1061"><sense>id.</sense></entry>
</superEntry>
</div></body></text></TEI>"#;
        let o = LexiconOptions {
            resource_id: "abbott-smith".into(),
            ..LexiconOptions::default()
        };
        let (r, report) = import_abbott_smith(&o, xml).unwrap();
        let ids: Vec<&str> = r.units.iter().map(|u| u.id.as_str()).collect();
        assert_eq!(ids, ["χάρις", "γαμίζω", "γαμίσκω"]);
        let u = &r.units[0];
        assert_eq!(
            u.text,
            "χάρις, -ιτος, ἡ, acc., χάριν (Ac 24:27),\n[in LXX chiefly for חֵן ;]\n1. objectively, grace (Si 21:16): Col 4:6;"
        );
        let spans: Vec<(&str, &str)> = u
            .spans
            .iter()
            .filter(|s| s.kind.name() != "para")
            .map(|s| (s.kind.name(), &u.text[s.range()]))
            .collect();
        assert_eq!(
            spans,
            [
                ("bold", "χάρις, -ιτος, ἡ"),
                ("scripture_ref", "Ac 24:27"),
                ("tagged_word", "חֵן"),
                ("bold", "grace"),
                ("scripture_ref", "Col 4:6"),
            ]
        );
        assert_eq!(r.units[1].text, "γαμ- forms\nto give in marriage");
        let g1061: Vec<&str> = r
            .keys
            .iter()
            .filter(|k| k.key.to_string() == "G1061")
            .map(|k| k.unit.as_str())
            .collect();
        assert_eq!(g1061, ["γαμίσκω"]);
        assert_eq!(report.scripture_refs, 2);
        assert_eq!(r.dropped_markup.get("<note type=occurrencesNT>"), Some(&1));
    }
}
