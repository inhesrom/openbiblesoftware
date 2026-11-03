//! ThML fragments (most CCEL-derived commentaries and some lexicons) → [`Writer`].
//!
//! | ThML | span |
//! |---|---|
//! | `<p>` `<blockquote>` / `<li>` | `para` prose / list |
//! | `<h1>`…`<h6>` | `heading` (levels above 4 become 4) |
//! | `<b>` `<strong>` `<term>` / `<i>` `<cite>` / `<em>` `<u>` | `bold` / `italic` / `emphasis` |
//! | `<added>` `<foreign>` | `added` `foreign` |
//! | `<note>` | note unit + `note_ref` |
//! | `<scripRef passage="…">` | `scripture_ref` |
//! | `<sync type="Strongs" value="G25"/>` / `type="morph"` | `tagged_word` on the word before |
//! | `<br/>` | paragraph break |
//!
//! `<div>`, `<ul>`, `<ol>`, `<def>` and `<span>` are structure and not counted; other elements
//! keep their text and are counted as dropped (`<img>` and `<pb>` have none).

use obs_model::{NoteKind, ParaStyle, SpanKind};
use obs_scripture::{Standard, parse_reference};

use super::writer::{Element, Writer};
use super::xml::{Token, tokenize};

/// Write one ThML entry.
pub(crate) fn feed(raw: &str, w: &mut Writer) {
    for token in tokenize(raw) {
        match token {
            Token::Text(t) => w.text(&t),
            Token::End(name) => w.close(&name.to_ascii_lowercase()),
            Token::Start { name, attrs, empty } => {
                let name = name.to_ascii_lowercase();
                start(&name, &attrs, empty, w);
            }
        }
    }
}

fn start(name: &str, attrs: &[(&str, String)], empty: bool, w: &mut Writer) {
    let attr = |n: &str| Token::attr(attrs, n);
    let element = match name {
        "br" => {
            w.para_break(None);
            return;
        }
        "sync" => {
            let ty = attr("type").unwrap_or_default().to_ascii_lowercase();
            let value = attr("value").unwrap_or_default();
            match ty.as_str() {
                "strongs" => {
                    let number = if value.starts_with(['G', 'g', 'H', 'h']) {
                        value.parse().ok()
                    } else {
                        None
                    };
                    match number {
                        Some(n) => w.strongs_after(n),
                        None => w.warn(format!("bad Strong's number `{value}`")),
                    }
                }
                "morph" => w.morph_after(&value),
                _ => w.drop_markup(&format!("<sync type=\"{ty}\">")),
            }
            return;
        }
        "p" | "blockquote" => Element::Para(ParaStyle::Prose),
        "li" => Element::Para(ParaStyle::List),
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let level = name[1..].parse::<u8>().unwrap_or(1).clamp(1, 4);
            Element::Heading(level)
        }
        "b" | "strong" | "term" => Element::Span(SpanKind::Bold),
        "i" | "cite" => Element::Span(SpanKind::Italic),
        "em" | "u" => Element::Span(SpanKind::Emphasis),
        "added" => Element::Span(SpanKind::Added),
        "foreign" => Element::Span(SpanKind::Foreign),
        "note" => Element::Note(NoteKind::Footnote),
        "scripref" => Element::Reference(
            attr("passage").and_then(|p| parse_reference(&p, &Standard, None).ok()),
        ),
        "div" | "ul" | "ol" | "def" | "span" => Element::Container,
        "img" | "pb" | "hr" => Element::Hidden(format!("<{name}>")),
        other => Element::Dropped(format!("<{other}>")),
    };
    w.open(name, element);
    if empty {
        w.close(name);
    }
}

#[cfg(test)]
mod tests {
    use obs_model::MorphemePart;

    use super::*;

    #[test]
    fn commentary_entry() {
        let mut w = Writer::new();
        feed(
            "<h3>The creation</h3><p><b>In the beginning</b> &mdash; see \
             <scripRef passage=\"John 1:1\">John 1:1</scripRef>.<note>A note.</note></p>\
             <p><font size=\"2\">small</font><img src=\"x.png\"/></p>",
            &mut w,
        );
        let unit = w.finish_unit();
        assert_eq!(
            unit.text,
            "The creation\nIn the beginning — see John 1:1.\nsmall"
        );
        assert!(
            unit.spans
                .contains(&(0..12, SpanKind::Heading { level: 3 }))
        );
        assert!(unit.spans.contains(&(13..29, SpanKind::Bold)));
        assert!(unit.spans.iter().any(|(r, k)| *r == (38..46)
            && matches!(k, SpanKind::ScriptureRef { targets } if targets.to_string() == "John 1:1")));
        assert_eq!(w.take_notes().len(), 1);
        assert_eq!(w.dropped.get("<font>"), Some(&1));
        assert_eq!(w.dropped.get("<img>"), Some(&1));
    }

    #[test]
    fn sync_tags_the_word_before() {
        let mut w = Writer::new();
        feed(
            "God<sync type=\"Strongs\" value=\"G2316\"/><sync type=\"morph\" value=\"N-NSM\"/> so",
            &mut w,
        );
        let unit = w.finish_unit();
        let mut part = MorphemePart::new("G2316".parse().unwrap_or_else(|_| unreachable!()));
        part.morph = Some("N-NSM".into());
        assert!(
            unit.spans
                .contains(&(0..3, SpanKind::TaggedWord { parts: vec![part] }))
        );
    }
}
