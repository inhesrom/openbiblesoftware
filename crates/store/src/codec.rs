//! Text encodings for values stored in columns. Pure functions, shared by every backend.

use obs_model::{Key, KeyScheme, SpanKind};
use obs_scripture::{Book, Passage, Verse};
use serde_json::{Map, Value};

use crate::error::StoreError;

fn corrupt(what: &str, value: &str) -> StoreError {
    StoreError::Corrupt(format!("bad {what} `{value}`"))
}

/// A serde unit enum as its snake_case name.
pub(crate) fn enum_name<T: serde::Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(Value::String(s)) => s,
        _ => String::new(),
    }
}

pub(crate) fn enum_from<T: serde::de::DeserializeOwned>(
    what: &str,
    s: &str,
) -> Result<T, StoreError> {
    serde_json::from_value(Value::String(s.to_string())).map_err(|_| corrupt(what, s))
}

pub(crate) fn json_from<T: serde::de::DeserializeOwned>(
    what: &str,
    s: &str,
) -> Result<T, StoreError> {
    serde_json::from_str(s).map_err(|_| corrupt(what, s))
}

pub(crate) fn json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

/// `JHN.3.16`
pub(crate) fn verse_text(v: Verse) -> String {
    format!("{}.{}.{}", v.book.usfm(), v.chapter, v.verse)
}

pub(crate) fn verse_from(s: &str) -> Result<Verse, StoreError> {
    let mut parts = s.split('.');
    let (Some(b), Some(c), Some(v), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(corrupt("verse", s));
    };
    let book = Book::from_usfm(b).ok_or_else(|| corrupt("verse", s))?;
    let chapter = c.parse().map_err(|_| corrupt("verse", s))?;
    let verse = v.parse().map_err(|_| corrupt("verse", s))?;
    Ok(Verse::new(book, chapter, verse))
}

/// A Verse as the integer `BBCCCVVV` (book number 1-66, chapter, verse), used by the dataset
/// tables so that a Passage overlap is an integer range query. It orders canonically and is
/// derived from the Verse alone (unlike a Standard ordinal, it never shifts).
pub(crate) fn verse_number(v: Verse) -> i64 {
    (v.book.index() as i64 + 1) * 1_000_000 + i64::from(v.chapter) * 1_000 + i64::from(v.verse)
}

pub(crate) fn verse_from_number(n: i64) -> Result<Verse, StoreError> {
    let bad = || corrupt("verse number", &n.to_string());
    let book = usize::try_from(n / 1_000_000 - 1).map_err(|_| bad())?;
    let book = Book::from_index(book).ok_or_else(bad)?;
    let chapter = u16::try_from(n / 1_000 % 1_000).map_err(|_| bad())?;
    let verse = u16::try_from(n % 1_000).map_err(|_| bad())?;
    Ok(Verse::new(book, chapter, verse))
}

pub(crate) fn passage_from_numbers(start: i64, end: i64) -> Result<Passage, StoreError> {
    Passage::new(verse_from_number(start)?, verse_from_number(end)?)
        .map_err(|_| corrupt("passage", &format!("{start}-{end}")))
}

/// A Key as `(scheme, text, normalized text)`. Passages are `JHN.3.16-JHN.3.18`, which sorts
/// in canonical order only within a book; lookups by Passage use range queries in the engine.
pub(crate) fn key_columns(key: &Key) -> (String, String, String) {
    let scheme = enum_name(&key.scheme());
    let text = match key {
        Key::Passage(p) => format!("{}-{}", verse_text(p.start()), verse_text(p.end())),
        Key::StrongsNumber(s) => s.to_string(),
        Key::Lemma(s) | Key::Headword(s) => s.clone(),
    };
    let norm = match key {
        Key::Lemma(s) | Key::Headword(s) => obs_model::normalize(s),
        _ => text.clone(),
    };
    (scheme, text, norm)
}

pub(crate) fn key_from(scheme: &str, text: &str) -> Result<Key, StoreError> {
    let scheme: KeyScheme = enum_from("key scheme", scheme)?;
    Ok(match scheme {
        KeyScheme::Passage => {
            let (a, b) = text
                .split_once('-')
                .ok_or_else(|| corrupt("passage key", text))?;
            let p = Passage::new(verse_from(a)?, verse_from(b)?)
                .map_err(|_| corrupt("passage key", text))?;
            Key::Passage(p)
        }
        KeyScheme::StrongsNumber => {
            Key::StrongsNumber(text.parse().map_err(|_| corrupt("Strong's key", text))?)
        }
        KeyScheme::Lemma => Key::Lemma(text.to_string()),
        KeyScheme::Headword => Key::Headword(text.to_string()),
    })
}

/// A span kind as `(name, attrs)`, where attrs is the JSON of its fields or `None`.
pub(crate) fn span_columns(kind: &SpanKind) -> (&'static str, Option<String>) {
    let attrs = match serde_json::to_value(kind) {
        Ok(Value::Object(mut map)) => {
            map.remove("kind");
            (!map.is_empty()).then(|| Value::Object(map).to_string())
        }
        _ => None,
    };
    (kind.name(), attrs)
}

pub(crate) fn span_from(name: &str, attrs: Option<&str>) -> Result<SpanKind, StoreError> {
    let mut map = match attrs {
        None => Map::new(),
        Some(a) => match serde_json::from_str::<Value>(a) {
            Ok(Value::Object(m)) => m,
            _ => return Err(corrupt("span attrs", a)),
        },
    };
    map.insert("kind".into(), Value::String(name.to_string()));
    serde_json::from_value(Value::Object(map)).map_err(|_| corrupt("span", name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use obs_model::{MorphemePart, NoteKind, ParaStyle, UnitId};
    use obs_scripture::PassageList;

    #[test]
    fn keys_round_trip() {
        let keys = [
            Key::Passage(
                Passage::new(
                    Verse::new(Book::Psalms, 51, 0),
                    Verse::new(Book::Psalms, 51, 2),
                )
                .unwrap(),
            ),
            Key::StrongsNumber("H1254A".parse().unwrap()),
            Key::Lemma("ἀγάπη".into()),
            Key::Headword("Abraham".into()),
        ];
        for k in keys {
            let (scheme, text, _) = key_columns(&k);
            assert_eq!(key_from(&scheme, &text).unwrap(), k);
        }
        assert_eq!(key_columns(&Key::Headword("Ēden".into())).2, "eden");
        assert!(key_from("passage", "JHN.3.16").is_err());
        assert!(key_from("bogus", "x").is_err());
    }

    #[test]
    fn verse_numbers_round_trip_and_order_canonically() {
        let genesis = Verse::new(Book::Genesis, 1, 1);
        let ps = Verse::new(Book::Psalms, 119, 176);
        let rev = Verse::new(Book::Revelation, 22, 21);
        assert_eq!(verse_number(genesis), 1_001_001);
        assert_eq!(verse_number(rev), 66_022_021);
        for v in [genesis, ps, rev, Verse::new(Book::Psalms, 3, 0)] {
            assert_eq!(verse_from_number(verse_number(v)).unwrap(), v);
        }
        assert!(verse_number(genesis) < verse_number(ps) && verse_number(ps) < verse_number(rev));
        assert!(verse_from_number(0).is_err());
        assert!(verse_from_number(67_001_001).is_err());
        assert!(passage_from_numbers(1_001_002, 1_001_001).is_err());
    }

    #[test]
    fn spans_round_trip() {
        let kinds = [
            SpanKind::Woj,
            SpanKind::Para {
                style: ParaStyle::Poetry2,
            },
            SpanKind::Verse {
                verse: Verse::new(Book::John, 3, 16),
                last: 16,
            },
            SpanKind::NoteRef {
                note: UnitId::note(3),
                note_kind: NoteKind::CrossReference,
            },
            SpanKind::Xref {
                targets: PassageList::new(),
            },
            SpanKind::TaggedWord {
                parts: vec![MorphemePart::new("G26".parse().unwrap())],
            },
        ];
        for k in kinds {
            let (name, attrs) = span_columns(&k);
            assert_eq!(span_from(name, attrs.as_deref()).unwrap(), k);
        }
        assert_eq!(span_columns(&SpanKind::Woj).1, None);
        assert!(span_from("woj", Some("[1]")).is_err());
        assert!(span_from("nonsense", None).is_err());
    }
}
