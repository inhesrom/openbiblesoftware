//! Strong's Hebrew and Greek dictionaries, from openscriptures/strongs.
//!
//! **Hebrew** (`StrongHebrewG.xml`, OSIS): one `<div type="entry">` per number. The headword
//! `<w lemma xlit POS ID>` gives the Lemma and the first block (lemma, transliteration,
//! pronunciation); the `exegesis` and `explanation` notes (Strong's derivation and definition)
//! make the second block, and the `translation` note the "KJV:" block. Inside notes `<w src>`
//! becomes the lemma linked to its Strong's number and `<hi>` italic. Dropped and counted: the
//! `<list>` sense outline and the `<foreign>` Greek-equivalent numbers, which are not part of
//! Strong's 1894 text, and the editors' `x-typo` notes.
//!
//! **Greek** (`strongsgreek.xml`): one `<entry strongs>` per number, read in document order.
//! The first `<greek>` and `<pronunciation>` make the headword block; `strongs_derivation` and
//! `strongs_def` the second; `kjv_def` the "KJV:" block; `<see>` a "See also" block.
//! `<strongsref>` and `<see>` become linked Strong's numbers, later `<greek>` its Greek text and
//! `<latin>` italic.

use obs_model::{Resource, SpanKind};
use obs_scripture::{StrongsLanguage, StrongsNumber};
use roxmltree::Node;

use super::{Entry, LexiconError, LexiconOptions, LexiconWriter, parse_xml};
use crate::report::ImportReport;

fn number(language: StrongsLanguage, digits: &str) -> Option<StrongsNumber> {
    format!("{}{}", language.prefix(), digits.trim())
        .parse()
        .ok()
}

fn file_name(o: &LexiconOptions, default: &str) -> String {
    if o.source_file.is_empty() {
        default.to_string()
    } else {
        o.source_file.clone()
    }
}

/// Convert Strong's Hebrew dictionary (OSIS XML).
pub fn import_strongs_hebrew(
    o: &LexiconOptions,
    xml: &str,
) -> Result<(Resource, ImportReport), LexiconError> {
    let file = file_name(o, "StrongHebrewG.xml");
    let doc = parse_xml(&file, xml)?;
    let mut w = LexiconWriter::new(o, "strongs-hebrew-osis", vec![file])?;
    for div in doc
        .descendants()
        .filter(|n| n.has_tag_name("div") && n.attribute("type") == Some("entry"))
    {
        let Some(head) = div
            .children()
            .find(|n| n.has_tag_name("w") && n.attribute("ID").is_some())
        else {
            w.warn(format!(
                "entry {} has no headword",
                div.attribute("n").unwrap_or("?")
            ));
            continue;
        };
        let id = head.attribute("ID").unwrap_or_default();
        let Ok(n) = id.parse::<StrongsNumber>() else {
            w.warn(format!("entry `{id}` is not a Strong's number"));
            continue;
        };
        let lemma = head.attribute("lemma").unwrap_or_default();
        let mut e = Entry::new(n.to_string(), lemma);
        e.key_strongs(n);
        e.key_lemma(lemma);
        headword(
            &mut e,
            lemma,
            head.attribute("xlit").unwrap_or_default(),
            head.attribute("POS").unwrap_or_default(),
        );
        let mut kjv: Option<Node> = None;
        for child in div.children().filter(Node::is_element) {
            match (child.tag_name().name(), child.attribute("type")) {
                ("w", _) if child == head => {}
                ("note", Some("exegesis" | "explanation")) => {
                    hebrew_inline(&mut e, child);
                    e.text(" ");
                }
                ("note", Some("translation")) => kjv = Some(child),
                ("note", kind) => {
                    e.drop_markup(&format!("<note type={}>", kind.unwrap_or_default()));
                }
                (name, _) => e.drop_markup(&format!("<{name}>")),
            }
        }
        e.block();
        if let Some(note) = kjv {
            e.styled(SpanKind::Bold, "KJV:");
            e.text(" ");
            hebrew_inline(&mut e, note);
            e.block();
        }
        w.add(e);
    }
    w.finish()
}

/// Lemma (bold), transliteration (italic) and pronunciation, as the first block.
fn headword(e: &mut Entry, lemma: &str, xlit: &str, pronunciation: &str) {
    e.styled(SpanKind::Bold, lemma);
    if !xlit.is_empty() {
        e.text(" ");
        e.styled(SpanKind::Italic, xlit);
    }
    if !pronunciation.is_empty() {
        e.text(&format!(" ({pronunciation})"));
    }
    e.block();
}

fn hebrew_inline(e: &mut Entry, node: Node) {
    for child in node.children() {
        if child.is_text() {
            e.text(child.text().unwrap_or_default());
            continue;
        }
        if !child.is_element() {
            continue;
        }
        match child.tag_name().name() {
            "w" => {
                let shown = child
                    .attribute("lemma")
                    .or(child.attribute("xlit"))
                    .unwrap_or_default();
                let start = e.mark();
                e.text(shown);
                match child
                    .attribute("src")
                    .and_then(|s| number(StrongsLanguage::Hebrew, s))
                {
                    Some(n) => e.strongs_link(start, n),
                    None => hebrew_inline(e, child),
                }
            }
            "hi" => {
                let start = e.mark();
                hebrew_inline(e, child);
                e.span(start, SpanKind::Italic);
            }
            "note" => e.drop_markup(&format!(
                "<note type={}>",
                child.attribute("type").unwrap_or_default()
            )),
            "foreign" => hebrew_inline(e, child),
            other => {
                e.drop_markup(&format!("<{other}>"));
                hebrew_inline(e, child);
            }
        }
    }
}

/// Convert Strong's Greek dictionary (Ulrik Petersen's XML).
pub fn import_strongs_greek(
    o: &LexiconOptions,
    xml: &str,
) -> Result<(Resource, ImportReport), LexiconError> {
    let file = file_name(o, "strongsgreek.xml");
    let doc = parse_xml(&file, xml)?;
    let mut w = LexiconWriter::new(o, "strongs-greek-xml", vec![file])?;
    for entry in doc.descendants().filter(|n| n.has_tag_name("entry")) {
        let digits = entry.attribute("strongs").unwrap_or_default();
        let Some(n) = number(StrongsLanguage::Greek, digits) else {
            w.warn(format!("entry `{digits}` is not a Strong's number"));
            continue;
        };
        let head = entry.children().find(|c| c.has_tag_name("greek"));
        let lemma = head
            .and_then(|h| h.attribute("unicode"))
            .unwrap_or_default();
        let mut e = Entry::new(n.to_string(), lemma);
        e.key_strongs(n);
        e.key_lemma(lemma);
        let pronunciation = entry
            .children()
            .find(|c| c.has_tag_name("pronunciation"))
            .and_then(|p| p.attribute("strongs"))
            .unwrap_or_default();
        headword(
            &mut e,
            lemma,
            head.and_then(|h| h.attribute("translit"))
                .unwrap_or_default(),
            pronunciation,
        );
        let mut in_see = false;
        for child in entry.children() {
            if child.is_text() {
                let text = child.text().unwrap_or_default();
                if !(in_see && text.trim().is_empty()) {
                    e.text(text);
                }
                continue;
            }
            if !child.is_element() {
                continue;
            }
            let name = child.tag_name().name();
            if in_see && name != "see" {
                e.text(".");
                e.block();
                in_see = false;
            }
            match name {
                "strongs" => {}
                "greek" if Some(child) == head => {}
                "pronunciation" => {}
                "kjv_def" => {
                    e.block();
                    e.styled(SpanKind::Bold, "KJV:");
                    e.text(" ");
                    greek_inline(&mut e, child);
                    e.block();
                }
                "see" => {
                    if in_see {
                        e.text(", ");
                    } else {
                        e.block();
                        e.text("See also ");
                        in_see = true;
                    }
                    strongsref(&mut e, child);
                }
                _ => greek_inline(&mut e, child),
            }
        }
        if in_see {
            e.text(".");
        }
        w.add(e);
    }
    w.finish()
}

fn strongsref(e: &mut Entry, node: Node) {
    let language = match node.attribute("language") {
        Some("HEBREW") => StrongsLanguage::Hebrew,
        _ => StrongsLanguage::Greek,
    };
    match node.attribute("strongs").and_then(|s| number(language, s)) {
        Some(n) => e.strongs_number(n),
        None => e.drop_markup("<strongsref> without a number"),
    }
}

fn greek_inline(e: &mut Entry, node: Node) {
    for (i, child) in node.children().enumerate() {
        if child.is_text() {
            let text = child.text().unwrap_or_default();
            // Strong's KJV renderings open with `:--`.
            let text = if i == 0 && node.has_tag_name("kjv_def") {
                text.trim_start().trim_start_matches(":--")
            } else {
                text
            };
            e.text(text);
            continue;
        }
        if !child.is_element() {
            continue;
        }
        match child.tag_name().name() {
            "strongsref" | "see" => strongsref(e, child),
            "greek" => e.text(child.attribute("unicode").unwrap_or_default()),
            "latin" => {
                let start = e.mark();
                greek_inline(e, child);
                e.span(start, SpanKind::Italic);
            }
            "pronunciation" => {}
            other => {
                e.drop_markup(&format!("<{other}>"));
                greek_inline(e, child);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(id: &str) -> LexiconOptions {
        LexiconOptions {
            resource_id: id.into(),
            ..LexiconOptions::default()
        }
    }

    #[test]
    fn greek_entry_links_strongsrefs_and_drops_the_kjv_lead() {
        let xml = r#"<strongsdictionary><entries><entry strongs="05485">
 <strongs>5485</strongs>   <greek BETA="XA/RIS" unicode="χάρις" translit="cháris"/>   <pronunciation strongs="khar'-ece"/>

 <strongs_derivation>from <strongsref language="GREEK" strongs="5463"/>;</strongs_derivation><strongs_def> graciousness (<latin>gratia</latin>)</strongs_def><kjv_def>:--acceptable, grace.</kjv_def>
<see language="GREEK" strongs="5463"/>
<see language="HEBREW" strongs="2580"/>
</entry></entries></strongsdictionary>"#;
        let (r, report) = import_strongs_greek(&options("sg"), xml).unwrap();
        let u = &r.units[0];
        assert_eq!(u.id.as_str(), "G5485");
        assert_eq!(
            u.text,
            "χάρις cháris (khar'-ece)\nfrom G5463; graciousness (gratia)\nKJV: acceptable, grace.\nSee also G5463, H2580."
        );
        assert_eq!(report.tagged_words, 3);
        let keys: Vec<String> = r.keys.iter().map(|k| k.key.to_string()).collect();
        assert_eq!(keys, ["G5485", "χάρις"]);
    }

    #[test]
    fn hebrew_entry_keeps_strongs_text_and_drops_the_outline() {
        let xml = r#"<osis><osisText><div type="glossary"><div type="entry" n="2">
        <w gloss="2553" lemma="אַב" morph="n-m" POS="ab" xlit="ʼab" ID="H2" xml:lang="arc">אב</w>
        <foreign xml:lang="grc"><w gloss="G:5" /></foreign>
        <list><item>1) father</item></list>
        <note type="exegesis">(Aramaic) corresponding to <w lemma="אָב" POS="awb" src="1" xlit="ʼâb"/><note type="x-typo">typo</note></note>
        <note type="explanation">{<hi>father</hi>}</note>
        <note type="translation">father.</note>
      </div></div></osisText></osis>"#;
        let (r, _) = import_strongs_hebrew(&options("sh"), xml).unwrap();
        let u = &r.units[0];
        assert_eq!(u.id.as_str(), "H2");
        assert_eq!(
            u.text,
            "אַב ʼab (ab)\n(Aramaic) corresponding to אָב {father}\nKJV: father."
        );
        assert!(!u.text.contains("1) father"));
        assert_eq!(r.dropped_markup.get("<list>"), Some(&1));
        assert_eq!(r.dropped_markup.get("<foreign>"), Some(&1));
        assert_eq!(r.dropped_markup.get("<note type=x-typo>"), Some(&1));
    }
}
