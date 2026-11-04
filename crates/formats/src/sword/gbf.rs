//! GBF (General Bible Format) fragments → [`Writer`]. Tags are `<XY…>`: an upper-case second
//! letter opens a pair and a lower-case one closes it (`<FR>`…`<Fr>`).
//!
//! | GBF | span |
//! |---|---|
//! | `<FR>` | `woj` |
//! | `<FI>` `<FB>` `<FU>` | `italic` `bold` `emphasis` |
//! | `<TS>` `<TT>` `<TH>` | `heading` |
//! | `<RF>` | note unit + `note_ref` |
//! | `<RX>` | `scripture_ref` (its text is parsed) |
//! | `<WG25>` `<WH430>` / `<WT…>` | `tagged_word` on the word before / its Morphology |
//! | `<CM>` `<CL>` | paragraph break |
//!
//! Every other tag is counted as dropped by its two-letter code; paired ones keep their text.

use obs_model::{NoteKind, SpanKind};

use super::writer::{Element, Writer};

/// Write one GBF entry.
pub(crate) fn feed(raw: &str, w: &mut Writer) {
    let mut rest = raw;
    while let Some(lt) = rest.find('<') {
        w.text(&rest[..lt]);
        let tail = &rest[lt..];
        let Some(gt) = tail.find('>') else {
            w.text(tail);
            return;
        };
        let tag = &tail[1..gt];
        if !tag_like(tag) {
            w.text(&tail[..=gt]);
        } else {
            handle(tag, w);
        }
        rest = &tail[gt + 1..];
    }
    w.text(rest);
}

fn tag_like(tag: &str) -> bool {
    let mut c = tag.chars();
    matches!((c.next(), c.next()), (Some(a), Some(b)) if a.is_ascii_uppercase() && b.is_ascii_alphabetic())
}

fn handle(tag: &str, w: &mut Writer) {
    let code = &tag[..2];
    let arg = &tag[2..];
    let upper = code.to_ascii_uppercase();
    if code.as_bytes()[1].is_ascii_lowercase() {
        w.close(&upper);
        return;
    }
    let element = match code {
        "WG" | "WH" => {
            match tag[1..].parse() {
                Ok(n) => w.strongs_after(n),
                Err(_) => w.warn(format!("bad Strong's number in `<{tag}>`")),
            }
            return;
        }
        "WT" => {
            w.morph_after(arg);
            return;
        }
        "CM" | "CL" => {
            w.para_break(None);
            return;
        }
        "FR" => Element::Span(SpanKind::Woj),
        "FI" => Element::Span(SpanKind::Italic),
        "FB" => Element::Span(SpanKind::Bold),
        "FU" => Element::Span(SpanKind::Emphasis),
        "TS" | "TT" | "TH" => Element::Heading(1),
        "RF" => Element::Note(NoteKind::Footnote),
        "RX" => Element::Reference(None),
        // Pairs whose text is kept; anything else is a lone marker.
        "FO" | "FS" | "FV" | "FC" | "FN" | "RB" => Element::Dropped(format!("<{code}>")),
        _ => {
            w.drop_markup(&format!("<{code}>"));
            return;
        }
    };
    w.open(&upper, element);
}

#[cfg(test)]
mod tests {
    use obs_scripture::{Book, Verse};

    use super::*;

    #[test]
    fn strongs_after_words_and_woj() {
        let mut w = Writer::new();
        w.set_book(Some(Book::John));
        w.start_chapter(1);
        w.open_verse(Verse::new(Book::John, 1, 1), 1);
        feed(
            "<TS>The Word<Ts>In<WG1722> the beginning<WG746><WTN-DSF> was<WG2258> <FR>the Word<Fr><RF>Or, Logos<Rf>.<CM><FO>x<Fo><HX>",
            &mut w,
        );
        let unit = w.finish_unit();
        assert_eq!(unit.text, "The Word\nIn the beginning was the Word.\nx");
        assert_eq!(unit.verses[0].0, 9..41);
        assert!(unit.spans.contains(&(30..38, SpanKind::Woj)));
        let begin = unit.spans.iter().find_map(|(r, k)| match k {
            SpanKind::TaggedWord { parts } if *r == (16..25) => Some(parts.clone()),
            _ => None,
        });
        let begin = begin.unwrap_or_default();
        assert_eq!(begin.len(), 1);
        assert_eq!(begin[0].morph.as_deref(), Some("N-DSF"));
        assert_eq!(w.take_notes()[0].1.text, "Or, Logos");
        assert_eq!(w.dropped.get("<FO>"), Some(&1));
        assert_eq!(w.dropped.get("<HX>"), Some(&1));
    }
}
