//! Format 3 span columns. Stable integer kind ids and positional binary attrs; no model
//! serde representation is changed. See docs/dev/obr-span-storage.md for the wire layout.

use obs_model::{MorphemePart, NoteKind, ParaStyle, SpanKind, UnitId};
use obs_scripture::{Book, Passage, PassageList, Verse};

use crate::StoreError;

fn bad() -> StoreError {
    StoreError::Corrupt("invalid compact span attrs".into())
}

fn number(out: &mut Vec<u8>, mut n: usize) {
    while n >= 128 {
        out.push((n as u8 & 127) | 128);
        n >>= 7;
    }
    out.push(n as u8);
}

fn text(out: &mut Vec<u8>, s: &str) {
    number(out, s.len());
    out.extend_from_slice(s.as_bytes());
}

fn verse(out: &mut Vec<u8>, v: Verse) {
    number(out, v.book.index());
    number(out, usize::from(v.chapter));
    number(out, usize::from(v.verse));
}

pub(crate) fn span_columns(kind: &SpanKind) -> (u8, Vec<u8>) {
    let mut out = Vec::new();
    let id = match kind {
        SpanKind::Verse { verse: v, last } => {
            verse(&mut out, *v);
            number(&mut out, usize::from(*last));
            0
        }
        SpanKind::Chapter { chapter } => {
            number(&mut out, usize::from(*chapter));
            1
        }
        SpanKind::Para { style } => {
            out.push(match style {
                ParaStyle::Prose => 0,
                ParaStyle::Poetry1 => 1,
                ParaStyle::Poetry2 => 2,
                ParaStyle::Poetry3 => 3,
                ParaStyle::List => 4,
                ParaStyle::Blank => 5,
            });
            2
        }
        SpanKind::Heading { level } => {
            number(&mut out, usize::from(*level));
            3
        }
        SpanKind::TaggedWord { parts } => {
            number(&mut out, parts.len());
            for part in parts {
                text(&mut out, &part.strongs.to_string());
                for field in [&part.lemma, &part.morph, &part.gloss] {
                    match field {
                        None => number(&mut out, 0),
                        Some(s) => {
                            number(&mut out, s.len() + 1);
                            out.extend_from_slice(s.as_bytes());
                        }
                    }
                }
            }
            4
        }
        SpanKind::NoteRef { note, note_kind } => {
            text(&mut out, note.as_str());
            out.push(match note_kind {
                NoteKind::Footnote => 0,
                NoteKind::CrossReference => 1,
            });
            5
        }
        SpanKind::Xref { targets } | SpanKind::ScriptureRef { targets } => {
            number(&mut out, targets.len());
            for p in targets.iter() {
                verse(&mut out, p.start());
                verse(&mut out, p.end());
            }
            if matches!(kind, SpanKind::Xref { .. }) {
                6
            } else {
                7
            }
        }
        SpanKind::Woj => 8,
        SpanKind::Added => 9,
        SpanKind::DivineName => 10,
        SpanKind::Emphasis => 11,
        SpanKind::Italic => 12,
        SpanKind::Bold => 13,
        SpanKind::Foreign => 14,
        SpanKind::Selah => 15,
        SpanKind::PsalmTitle => 16,
    };
    (id, out)
}

struct Input<'a>(&'a [u8]);

impl<'a> Input<'a> {
    fn number(&mut self) -> Result<usize, StoreError> {
        // Bound every integer to u32 on all platforms, including wasm32. Reject overlong
        // encodings and overflow rather than accepting multiple encodings of one value.
        let mut n = 0_u32;
        for shift in (0..35).step_by(7) {
            let (&byte, rest) = self.0.split_first().ok_or_else(bad)?;
            self.0 = rest;
            if shift == 28 && byte > 15 {
                return Err(bad());
            }
            n |= u32::from(byte & 127) << shift;
            if byte < 128 {
                if shift != 0 && byte == 0 {
                    return Err(bad());
                }
                return Ok(n as usize);
            }
        }
        Err(bad())
    }

    fn string(&mut self, len: usize) -> Result<String, StoreError> {
        let bytes = self.0.get(..len).ok_or_else(bad)?;
        let s = std::str::from_utf8(bytes).map_err(|_| bad())?.to_owned();
        self.0 = &self.0[len..];
        Ok(s)
    }

    fn text(&mut self) -> Result<String, StoreError> {
        let len = self.number()?;
        self.string(len)
    }

    fn optional_text(&mut self) -> Result<Option<String>, StoreError> {
        let len = self.number()?;
        if len == 0 {
            Ok(None)
        } else {
            self.string(len - 1).map(Some)
        }
    }

    fn u16(&mut self) -> Result<u16, StoreError> {
        u16::try_from(self.number()?).map_err(|_| bad())
    }

    fn verse(&mut self) -> Result<Verse, StoreError> {
        let book = Book::from_index(self.number()?).ok_or_else(bad)?;
        Ok(Verse::new(book, self.u16()?, self.u16()?))
    }

    fn count(&mut self) -> Result<usize, StoreError> {
        let n = self.number()?;
        // Every item occupies at least one byte. Never allocate from an untrusted count.
        if n > self.0.len() { Err(bad()) } else { Ok(n) }
    }
}

pub(crate) fn span_from(id: i64, attrs: &[u8]) -> Result<SpanKind, StoreError> {
    let mut input = Input(attrs);
    let kind = match id {
        0 => SpanKind::Verse {
            verse: input.verse()?,
            last: input.u16()?,
        },
        1 => SpanKind::Chapter {
            chapter: input.u16()?,
        },
        2 => SpanKind::Para {
            style: match input.number()? {
                0 => ParaStyle::Prose,
                1 => ParaStyle::Poetry1,
                2 => ParaStyle::Poetry2,
                3 => ParaStyle::Poetry3,
                4 => ParaStyle::List,
                5 => ParaStyle::Blank,
                _ => return Err(bad()),
            },
        },
        3 => SpanKind::Heading {
            level: u8::try_from(input.number()?).map_err(|_| bad())?,
        },
        4 => {
            let count = input.count()?;
            let mut parts = Vec::new();
            for _ in 0..count {
                parts.push(MorphemePart {
                    strongs: input.text()?.parse().map_err(|_| bad())?,
                    lemma: input.optional_text()?,
                    morph: input.optional_text()?,
                    gloss: input.optional_text()?,
                });
            }
            SpanKind::TaggedWord { parts }
        }
        5 => SpanKind::NoteRef {
            note: UnitId::new(input.text()?).map_err(|_| bad())?,
            note_kind: match input.number()? {
                0 => NoteKind::Footnote,
                1 => NoteKind::CrossReference,
                _ => return Err(bad()),
            },
        },
        6 | 7 => {
            let count = input.count()?;
            let mut targets = PassageList::new();
            for _ in 0..count {
                targets.push(Passage::new(input.verse()?, input.verse()?).map_err(|_| bad())?);
            }
            if id == 6 {
                SpanKind::Xref { targets }
            } else {
                SpanKind::ScriptureRef { targets }
            }
        }
        8 => SpanKind::Woj,
        9 => SpanKind::Added,
        10 => SpanKind::DivineName,
        11 => SpanKind::Emphasis,
        12 => SpanKind::Italic,
        13 => SpanKind::Bold,
        14 => SpanKind::Foreign,
        15 => SpanKind::Selah,
        16 => SpanKind::PsalmTitle,
        _ => return Err(StoreError::Corrupt(format!("unknown span kind {id}"))),
    };
    if !input.0.is_empty() {
        return Err(bad());
    }
    Ok(kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds() -> Vec<SpanKind> {
        let mut targets = PassageList::new();
        targets.push(Passage::verse(Verse::new(Book::Psalms, 3, 0)));
        targets.push(
            Passage::new(Verse::new(Book::John, 3, 16), Verse::new(Book::Acts, 1, 2)).unwrap(),
        );
        // Duplicate targets and morpheme order are content, not sets to normalize here.
        targets.push(targets.passages()[0]);
        let mut kinds = vec![
            SpanKind::Verse {
                verse: Verse::new(Book::Psalms, 119, 176),
                last: 177,
            },
            SpanKind::Chapter { chapter: u16::MAX },
            SpanKind::Heading { level: u8::MAX },
            SpanKind::TaggedWord {
                parts: vec![
                    MorphemePart::new("H9001".parse().unwrap()),
                    MorphemePart {
                        strongs: "H1254A".parse().unwrap(),
                        lemma: Some("בָּרָא".into()),
                        morph: Some("Vqp3ms".into()),
                        gloss: Some("create\0\n\"".into()),
                    },
                    MorphemePart {
                        strongs: "G99999Z".parse().unwrap(),
                        lemma: Some("χάρις".into()),
                        morph: None,
                        gloss: Some(String::new()),
                    },
                ],
            },
            SpanKind::TaggedWord { parts: vec![] },
            SpanKind::Xref {
                targets: targets.clone(),
            },
            SpanKind::ScriptureRef { targets },
            SpanKind::Xref {
                targets: PassageList::new(),
            },
            SpanKind::Woj,
            SpanKind::Added,
            SpanKind::DivineName,
            SpanKind::Emphasis,
            SpanKind::Italic,
            SpanKind::Bold,
            SpanKind::Foreign,
            SpanKind::Selah,
            SpanKind::PsalmTitle,
        ];
        for style in [
            ParaStyle::Prose,
            ParaStyle::Poetry1,
            ParaStyle::Poetry2,
            ParaStyle::Poetry3,
            ParaStyle::List,
            ParaStyle::Blank,
        ] {
            kinds.push(SpanKind::Para { style });
        }
        for note_kind in [NoteKind::Footnote, NoteKind::CrossReference] {
            kinds.push(SpanKind::NoteRef {
                note: UnitId::new("note.שלום").unwrap(),
                note_kind,
            });
        }
        kinds
    }

    #[test]
    fn every_span_kind_and_morpheme_field_round_trips_losslessly() {
        for kind in kinds() {
            let (id, attrs) = span_columns(&kind);
            assert_eq!(span_from(i64::from(id), &attrs).unwrap(), kind);
            for end in 0..attrs.len() {
                assert!(
                    span_from(i64::from(id), &attrs[..end]).is_err(),
                    "{kind:?} truncated at {end}"
                );
            }
            let mut trailing = attrs;
            trailing.push(0);
            assert!(span_from(i64::from(id), &trailing).is_err());
        }
    }

    #[test]
    fn compact_span_wire_goldens() {
        assert_eq!(
            span_columns(&SpanKind::Verse {
                verse: Verse::new(Book::John, 3, 16),
                last: 18
            }),
            (0, vec![42, 3, 16, 18])
        );
        assert_eq!(
            span_columns(&SpanKind::TaggedWord {
                parts: vec![MorphemePart::new("H123".parse().unwrap())]
            }),
            (4, b"\x01\x04H123\0\0\0".to_vec())
        );
        assert_eq!(span_columns(&SpanKind::PsalmTitle), (16, vec![]));
        for (kind, id) in [
            (SpanKind::Woj, 8),
            (SpanKind::Added, 9),
            (SpanKind::DivineName, 10),
            (SpanKind::Emphasis, 11),
            (SpanKind::Italic, 12),
            (SpanKind::Bold, 13),
            (SpanKind::Foreign, 14),
            (SpanKind::Selah, 15),
        ] {
            assert_eq!(span_columns(&kind), (id, vec![]));
        }
    }

    #[test]
    fn malformed_attrs_fail_without_large_allocations_or_panics() {
        for (id, bytes) in [
            (-1, vec![]),
            (17, vec![]),
            (i64::MAX, vec![]),
            (1, vec![0x80, 0]),                      // noncanonical varint
            (1, vec![0xff, 0xff, 0xff, 0xff, 0x10]), // u32 overflow
            (1, vec![0x80, 0x80, 4]),                // u16 overflow
            (2, vec![6]),
            (3, vec![0x80, 2]),
            (0, vec![66, 1, 1, 1]),                // unknown Bible book
            (4, vec![0xff, 0xff, 0xff, 0xff, 15]), // impossible count
            (4, vec![1, 1, 0xff, 0, 0, 0]),        // invalid UTF-8
            (4, vec![1, 1, b'X', 0, 0, 0]),        // invalid Strong's
            (5, vec![0, 0]),                       // empty UnitId
            (5, vec![1, b'n', 2]),                 // unknown note kind
            (6, vec![1, 42, 3, 18, 42, 3, 16]),    // backwards Passage
            (8, vec![0]),                          // attrs forbidden
        ] {
            assert!(
                matches!(span_from(id, &bytes), Err(StoreError::Corrupt(_))),
                "{id}: {bytes:?}"
            );
        }
        let mut out = Vec::new();
        for n in [0, 127, 128, 16383, 16384, u32::MAX as usize] {
            out.clear();
            number(&mut out, n);
            assert_eq!(Input(&out).number().unwrap(), n);
        }
    }
}
