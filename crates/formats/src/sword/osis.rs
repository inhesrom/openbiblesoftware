//! OSIS fragments (the SourceType of most CrossWire and eBible modules) → [`Writer`].
//!
//! | OSIS | span |
//! |---|---|
//! | `<w lemma="strong:G25" morph="robinson:V-AAI-3S">` | `tagged_word` (one part per Strong's number) |
//! | `<q who="Jesus">` (container or `sID`/`eID`) | `woj` |
//! | `<transChange>` `<divineName>` `<foreign>` | `added` `divine_name` `foreign` |
//! | `<hi type="italic\|bold\|emphasis\|underline">` | `italic` `bold` `emphasis` |
//! | `<catchWord>` `<rdg>` | `italic` |
//! | `<title>` / `<title type="psalm">` | `heading` / `psalm_title` (Verse 0 when it leads a Psalm) |
//! | `<p>` `<l level="n">` `<item>` | `para` prose / poetry n / list |
//! | `<milestone type="x-p">` `<lb/>` `<div type="paragraph" sID>` | paragraph break |
//! | `<note>` / `<note type="crossReference">` | note unit + `note_ref` (+ `xref`) |
//! | `<reference osisRef="…">` | `scripture_ref` |
//!
//! `<div>`, `<chapter>`, `<verse>`, `<lg>` and `<list>` are structure and not counted. Other
//! elements keep their text and are counted as dropped; `<figure>` and `<index>` lose their
//! content too.

use obs_model::{MorphemePart, NoteKind, ParaStyle, SpanKind};
use obs_scripture::StrongsNumber;

use super::writer::{Element, Writer, parse_osis_ref};
use super::xml::{Token, tokenize};

/// Write one OSIS entry.
pub(crate) fn feed(raw: &str, w: &mut Writer) {
    for token in tokenize(raw) {
        match token {
            Token::Text(t) => w.text(&t),
            Token::End(name) => w.close(name),
            Token::Start { name, attrs, empty } => start(name, &attrs, empty, w),
        }
    }
}

fn start(name: &str, attrs: &[(&str, String)], empty: bool, w: &mut Writer) {
    let attr = |n: &str| Token::attr(attrs, n);
    let ty = attr("type").unwrap_or_default();
    // Milestone pairs: `<q sID="x" who="Jesus"/>` … `<q eID="x"/>`.
    if empty && attr("eID").is_some() {
        match name {
            "div" if is_paragraph(&ty) => w.para_break(None),
            "l" => w.para_break(None),
            _ => w.close(name),
        }
        return;
    }
    let milestone_start = empty && attr("sID").is_some();
    let element = match name {
        "chapter" | "verse" if empty => return,
        "div" if empty => {
            if is_paragraph(&ty) {
                w.para_break(None);
            }
            return;
        }
        "milestone" => {
            if matches!(ty.as_str(), "x-p" | "x-paragraph" | "paragraph" | "line") {
                w.para_break(None);
            } else {
                w.drop_markup(&format!("<milestone type=\"{ty}\">"));
            }
            return;
        }
        "lb" => {
            w.para_break(None);
            return;
        }
        "l" if milestone_start => {
            w.para_break(Some(poetry(attr("level").as_deref())));
            return;
        }
        "w" => match tagged_word(attrs, w) {
            Some(kind) => Element::Span(kind),
            None => Element::Container,
        },
        "q" => {
            if attr("who").is_some_and(|who| who.eq_ignore_ascii_case("jesus")) {
                Element::Span(SpanKind::Woj)
            } else {
                Element::Dropped("<q>".into())
            }
        }
        "transChange" => Element::Span(SpanKind::Added),
        "divineName" => Element::Span(SpanKind::DivineName),
        "foreign" => Element::Span(SpanKind::Foreign),
        "catchWord" | "rdg" => Element::Span(SpanKind::Italic),
        "hi" => match ty.as_str() {
            "italic" | "i" => Element::Span(SpanKind::Italic),
            "bold" | "b" => Element::Span(SpanKind::Bold),
            "emphasis" | "underline" => Element::Span(SpanKind::Emphasis),
            other => Element::Dropped(format!("<hi type=\"{other}\">")),
        },
        "title" => match ty.as_str() {
            "psalm" => Element::PsalmTitle,
            "x-gen" => Element::Hidden("<title type=\"x-gen\">".into()),
            _ => Element::Heading(
                attr("level")
                    .and_then(|l| l.parse::<u8>().ok())
                    .unwrap_or(if ty == "sub" { 2 } else { 1 })
                    .clamp(1, 4),
            ),
        },
        "p" => Element::Para(ParaStyle::Prose),
        "l" => Element::Para(poetry(attr("level").as_deref())),
        "item" => Element::Para(ParaStyle::List),
        "note" => Element::Note(if ty == "crossReference" {
            NoteKind::CrossReference
        } else {
            NoteKind::Footnote
        }),
        "reference" => Element::Reference(attr("osisRef").and_then(|r| parse_osis_ref(&r))),
        "div" | "chapter" | "verse" | "lg" | "list" => Element::Container,
        "figure" | "index" => Element::Hidden(format!("<{name}>")),
        other => Element::Dropped(format!("<{other}>")),
    };
    w.open(name, element);
    if empty && !milestone_start {
        w.close(name);
    }
}

fn is_paragraph(ty: &str) -> bool {
    matches!(ty, "paragraph" | "x-p")
}

fn poetry(level: Option<&str>) -> ParaStyle {
    match level {
        Some("2") => ParaStyle::Poetry2,
        Some("3" | "4") => ParaStyle::Poetry3,
        _ => ParaStyle::Poetry1,
    }
}

/// A `tagged_word` from `lemma="strong:H0853 strong:H01254"` and its parallel `morph`.
fn tagged_word(attrs: &[(&str, String)], w: &mut Writer) -> Option<SpanKind> {
    let lemma = Token::attr(attrs, "lemma")?;
    let morphs: Vec<String> = Token::attr(attrs, "morph")
        .unwrap_or_default()
        .split_whitespace()
        .map(|m| m.split_once(':').map_or(m, |(_, code)| code).to_string())
        .collect();
    let mut parts = Vec::new();
    for item in lemma.split_whitespace() {
        let Some((scheme, value)) = item.split_once(':') else {
            continue;
        };
        if !scheme.eq_ignore_ascii_case("strong") {
            continue;
        }
        match value.parse::<StrongsNumber>() {
            Ok(n) => {
                let mut part = MorphemePart::new(n);
                part.morph = morphs.get(parts.len()).cloned();
                parts.push(part);
            }
            Err(_) => w.warn(format!("bad Strong's number `{value}`")),
        }
    }
    (!parts.is_empty()).then_some(SpanKind::TaggedWord { parts })
}

#[cfg(test)]
mod tests {
    use obs_scripture::{Book, Verse};

    use super::*;

    #[test]
    fn verse_with_words_quote_and_crossref() {
        let mut w = Writer::new();
        w.set_book(Some(Book::John));
        w.start_chapter(3);
        w.open_verse(Verse::new(Book::John, 3, 16), 16);
        feed(
            "<title type=\"section\" subType=\"x-preverse\">Love</title>\
             <q who=\"Jesus\" marker=\"\"><w lemma=\"strong:G1063\">For</w> \
             <w lemma=\"strong:G2316 strong:G3588\" morph=\"robinson:N-NSM robinson:T-NSM\">God</w> \
             so loved<note type=\"crossReference\"><reference osisRef=\"Rom.5.8\">Rom 5:8</reference></note> \
             <seg>the</seg> world</q>",
            &mut w,
        );
        let unit = w.finish_unit();
        assert_eq!(unit.text, "Love\nFor God so loved the world");
        assert_eq!(unit.verses[0].0, 5..31);
        let woj = unit.spans.iter().find(|(_, k)| *k == SpanKind::Woj);
        assert_eq!(woj.map(|(r, _)| r.clone()), Some(5..31));
        let god = unit
            .spans
            .iter()
            .find_map(|(r, k)| match k {
                SpanKind::TaggedWord { parts } if *r == (9..12) => Some(parts.clone()),
                _ => None,
            })
            .unwrap_or_default();
        assert_eq!(god.len(), 2);
        assert_eq!(god[1].morph.as_deref(), Some("T-NSM"));
        assert!(unit.spans.iter().any(|(r, k)| *r == (21..21)
            && matches!(k, SpanKind::Xref { targets } if targets.to_string() == "Rom 5:8")));
        assert_eq!(w.dropped.get("<seg>"), Some(&1));
    }

    #[test]
    fn psalm_title_before_verse_one_is_the_title_slot() {
        let mut w = Writer::new();
        w.set_book(Some(Book::Psalms));
        w.start_chapter(3);
        w.open_verse(Verse::new(Book::Psalms, 3, 1), 1);
        feed(
            "<title type=\"psalm\" canonical=\"true\">A Psalm of David.</title>LORD, how are they increased",
            &mut w,
        );
        let unit = w.finish_unit();
        let verses: Vec<(String, &str)> = unit
            .verses
            .iter()
            .map(|(r, v, _)| (v.to_string(), &unit.text[r.clone()]))
            .collect();
        assert_eq!(
            verses,
            vec![
                ("Ps 3:title".to_string(), "A Psalm of David."),
                ("Ps 3:1".to_string(), "LORD, how are they increased"),
            ]
        );
    }

    #[test]
    fn milestoned_quote_spans_verses() {
        let mut w = Writer::new();
        w.set_book(Some(Book::John));
        w.start_chapter(3);
        w.open_verse(Verse::new(Book::John, 3, 3), 3);
        feed(
            "Jesus answered, <q who=\"Jesus\" sID=\"q1\"/>Verily,",
            &mut w,
        );
        w.open_verse(Verse::new(Book::John, 3, 4), 4);
        feed("verily.<q eID=\"q1\"/> Then", &mut w);
        let unit = w.finish_unit();
        assert!(
            unit.spans.contains(&(16..31, SpanKind::Woj)),
            "{:?}",
            unit.spans
        );
    }
}
