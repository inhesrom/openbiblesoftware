//! OpenBible.info Cross-references (`cross_references.txt`, CC BY) → a Cross-reference set.
//!
//! The source is tab-separated: `From Verse`, `To Verse`, `Votes`, with a header line. A Verse
//! is OSIS-style (`Gen.1.1`) and either end may be a range (`Rom.1.19-Rom.1.20`). The numbering
//! is the ESV's, which matches the Standard versification except that the ESV splits 3 John
//! 1:14 into 1:14-15; 3 John 1:15 is read as 1:14. A row whose source or target is still not
//! a valid Standard reference is dropped and listed in the report, so every stored
//! Cross-reference target is a valid Standard reference. Every other row is kept, votes
//! included (negative votes too: ranking is the reader's job).

use obs_model::{CrossReference, Resource, ResourceType};
use obs_scripture::{Book, Passage, Standard, Verse, Versification};

use crate::dataset::{DatasetError, DatasetOptions, start};
use crate::report::ImportReport;

/// `source_format` of a Cross-reference set converted from OpenBible.info.
pub const OPENBIBLE_FORMAT: &str = "openbible-cross-references";

/// Dropped-row reason: an end is not a Verse of the Standard versification.
pub const DROPPED_NOT_STANDARD: &str = "row: reference outside the Standard versification";

/// Convert OpenBible.info's `cross_references.txt`.
pub fn import_openbible_cross_references(
    text: &str,
    options: &DatasetOptions,
) -> Result<(Resource, ImportReport), DatasetError> {
    let (mut builder, mut report) =
        start(options, ResourceType::CrossReferenceSet, OPENBIBLE_FORMAT)?;
    let mut rows = 0usize;
    for (i, raw) in text.lines().enumerate() {
        let line = i + 1;
        let row = raw.trim_start_matches('\u{feff}').trim_end();
        if row.is_empty() || row.starts_with("From Verse") {
            continue;
        }
        let syntax = |message: String| DatasetError::Syntax { line, message };
        let mut fields = row.split('\t');
        let (Some(from), Some(to), Some(votes)) = (fields.next(), fields.next(), fields.next())
        else {
            return Err(syntax(format!("expected 3 tab-separated fields: `{row}`")));
        };
        let from = parse_passage(from).ok_or_else(|| syntax(format!("bad source `{from}`")))?;
        let to = parse_passage(to).ok_or_else(|| syntax(format!("bad target `{to}`")))?;
        let (from, to) = (to_standard(from), to_standard(to));
        let votes: i32 = votes
            .trim()
            .parse()
            .map_err(|_| syntax(format!("bad votes `{votes}`")))?;
        rows += 1;
        if !is_standard(from) || !is_standard(to) {
            builder.count_dropped(DROPPED_NOT_STANDARD);
            report.warnings.push(format!(
                "line {line}: dropped {from} → {to}: not in the Standard versification"
            ));
            continue;
        }
        builder.add_cross_reference(CrossReference { from, to, votes });
    }
    if rows == 0 {
        return Err(DatasetError::NoRecords);
    }
    let resource = builder.build().map_err(DatasetError::Invalid)?;
    report.cross_references = resource.cross_references.len();
    report.dropped_markup = resource.dropped_markup.clone();
    Ok((resource, report))
}

/// ESV 3 John 1:15 is the end of the Standard's 3 John 1:14.
const ESV_3JN_1_15: Verse = Verse::new(Book::ThirdJohn, 1, 15);
const STD_3JN_1_14: Verse = Verse::new(Book::ThirdJohn, 1, 14);

fn to_standard(p: Passage) -> Passage {
    let fix = |v: Verse| if v == ESV_3JN_1_15 { STD_3JN_1_14 } else { v };
    Passage::new(fix(p.start()), fix(p.end())).unwrap_or(p)
}

fn is_standard(p: Passage) -> bool {
    Standard.contains(p.start()) && Standard.contains(p.end())
}

/// `Gen.1.1` or `Gen.1.1-Gen.1.3`.
fn parse_passage(s: &str) -> Option<Passage> {
    let s = s.trim();
    match s.split_once('-') {
        None => parse_verse(s).map(Passage::verse),
        Some((a, b)) => Passage::new(parse_verse(a)?, parse_verse(b)?).ok(),
    }
}

/// `Gen.1.1`, `1John.3.16`, `Ps.23.1`.
fn parse_verse(s: &str) -> Option<Verse> {
    let mut parts = s.split('.');
    let (Some(book), Some(chapter), Some(verse), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    Some(Verse::new(
        Book::from_osis(book)?,
        chapter.parse().ok()?,
        verse.parse().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verses_and_ranges_parse() {
        assert_eq!(
            parse_passage("1John.3.16"),
            Some(Passage::verse(Verse::new(Book::FirstJohn, 3, 16)))
        );
        let range = parse_passage("Rom.1.19-Rom.1.20").unwrap();
        assert_eq!(range.start(), Verse::new(Book::Romans, 1, 19));
        assert_eq!(range.end(), Verse::new(Book::Romans, 1, 20));
        assert_eq!(parse_passage("Rom.1.20-Rom.1.19"), None);
        assert_eq!(parse_passage("Xyz.1.1"), None);
        assert_eq!(parse_passage("Gen.1"), None);
    }

    #[test]
    fn esv_3_john_1_15_is_read_as_1_14() {
        let p = to_standard(parse_passage("2John.1.13-3John.1.15").unwrap());
        assert_eq!(p.start(), Verse::new(Book::SecondJohn, 1, 13));
        assert_eq!(p.end(), STD_3JN_1_14);
        let p = to_standard(parse_passage("3John.1.15").unwrap());
        assert_eq!(p, Passage::verse(STD_3JN_1_14));
        let jn = parse_passage("John.10.3").unwrap();
        assert_eq!(to_standard(jn), jn);
    }
}
