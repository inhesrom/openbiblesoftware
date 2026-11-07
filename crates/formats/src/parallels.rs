//! A USFM Bible's parallel passage references (`\r`) → a Cross-reference set's Parallel
//! passages (#212, ADR 0014).
//!
//! USFM's `\r` line, under a section heading (`\s`, `\s1`…`\s4`, `\ms`…), lists the Passages
//! parallel to that section: the Berean Standard Bible writes `\s1 The Baptism of Jesus` then
//! `\r (\ref Mark 1:9–11|MRK 1:9-11\ref*; \ref Luke 3:21–22|LUK 3:21-22\ref*)` before Matthew
//! 3:13. Each such section becomes one [`ParallelSection`]: the Verses from the heading to the
//! next heading (or the end of the book), the heading as its title, and the references in the
//! order written.
//!
//! References are read from the `\ref` target (`MRK 1:9-11`), or from its label when the target
//! names no book (`Song of Solomon 1:1–17|1:1-17`) or there is none (`\ref Genesis 4–9`).
//! Verse numbers are read in the Standard versification: the BSB numbers every verse as the
//! Standard does (its Verse map is the identity but for two Psalm headings). A reference that
//! is not a Standard Passage, spans Bible books (`Joshua–Malachi`) or overlaps its own section
//! is dropped and listed in the report; a section left with no parallel is left out.

use obs_model::{ParallelSection, Resource, ResourceType};
use obs_scripture::{Book, Passage, Standard, Verse, Versification, parse_reference};

use crate::dataset::{DatasetError, DatasetOptions, start};
use crate::report::ImportReport;
use crate::usfm::usfm_book_code;

/// `source_format` of a Cross-reference set made from a USFM Bible's `\r` lines.
pub const USFM_PARALLELS_FORMAT: &str = "usfm-parallel-passages";

/// Dropped-reference reason: not a Passage of one Bible book in the Standard versification.
pub const DROPPED_PARALLEL_NOT_STANDARD: &str = "\\r: reference outside the Standard versification";
/// Dropped-reference reason: a reference that spans Bible books.
pub const DROPPED_PARALLEL_SPANS_BOOKS: &str = "\\r: reference spanning Bible books";
/// Dropped-reference reason: a reference to (part of) its own section.
pub const DROPPED_PARALLEL_SELF: &str = "\\r: reference overlapping its own section";

/// A section heading being read: its title, its first and last Verse so far, and the
/// references its `\r` line gave.
struct Open {
    /// The heading's level: 0 for `\ms`, 1 for `\s` and `\s1`, 2 for `\s2`…
    level: u8,
    title: String,
    first: Option<Verse>,
    last: Option<Verse>,
    refs: Vec<(String, usize)>,
}

/// A section read from a book: its Passage, heading, and references with their line numbers.
type ReadSection = (Passage, String, Vec<(String, usize)>);

/// Convert the parallel passage references of a USFM Bible's books (file name, text).
pub fn import_usfm_parallel_passages(
    books: &[(String, String)],
    options: &DatasetOptions,
) -> Result<(Resource, ImportReport), DatasetError> {
    let (mut builder, mut report) = start(
        options,
        ResourceType::CrossReferenceSet,
        USFM_PARALLELS_FORMAT,
    )?;
    report.source_files = books.iter().map(|(name, _)| name.clone()).collect();
    let mut sections = Vec::new();
    for (name, text) in books {
        let Some(book) = usfm_book_code(text).and_then(|c| Book::from_usfm(&c)) else {
            continue;
        };
        read_book(name, book, text, &mut sections, &mut report);
    }
    let mut kept = 0usize;
    for (passage, title, refs) in sections {
        let mut parallels: Vec<Passage> = Vec::new();
        for (written, line) in refs {
            match parallel(&written, passage) {
                Ok(p) => {
                    if !parallels.contains(&p) {
                        parallels.push(p);
                    }
                }
                Err(reason) => {
                    builder.count_dropped(reason);
                    report.warnings.push(format!(
                        "line {line}: dropped `{written}` beside {passage}: {}",
                        reason.trim_start_matches("\\r: ")
                    ));
                }
            }
        }
        if parallels.is_empty() {
            continue;
        }
        kept += 1;
        builder.add_parallel_section(ParallelSection {
            passage,
            title,
            parallels,
        });
    }
    if kept == 0 {
        return Err(DatasetError::NoRecords);
    }
    let resource = builder.build().map_err(DatasetError::Invalid)?;
    report.parallel_sections = resource.parallel_sections.len();
    report.dropped_markup = resource.dropped_markup.clone();
    Ok((resource, report))
}

/// Read one book's sections that have a `\r` line: (section Passage, heading, references with
/// their line numbers).
fn read_book(
    name: &str,
    book: Book,
    text: &str,
    out: &mut Vec<ReadSection>,
    report: &mut ImportReport,
) {
    let mut chapter: u16 = 0;
    let mut open: Option<Open> = None;
    let close = |open: Option<Open>, out: &mut Vec<_>, report: &mut ImportReport| {
        let Some(o) = open else { return };
        if o.refs.is_empty() {
            return;
        }
        match o
            .first
            .zip(o.last)
            .and_then(|(a, b)| Passage::new(a, b).ok())
        {
            Some(p) if Standard.contains(p.start()) && Standard.contains(p.end()) => {
                out.push((p, o.title, o.refs));
            }
            _ => report.warnings.push(format!(
                "{name}: section “{}” has parallel passages but no Standard Verses",
                o.title
            )),
        }
    };
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim_start();
        let (marker, rest) = match line.strip_prefix('\\') {
            Some(m) => m.split_once(char::is_whitespace).unwrap_or((m, "")),
            None => ("", line),
        };
        if marker == "c" {
            if let Some(n) = rest.split_whitespace().next().and_then(|n| n.parse().ok()) {
                chapter = n;
            }
            continue;
        }
        if let Some(level) = heading_level(marker) {
            // A subheading (Proverbs' `\s2 Saying 7` under `\s1 True Riches`) stays in
            // its section; a major section's (`\ms`) headings start sections of their own.
            if open
                .as_ref()
                .is_some_and(|o| o.level > 0 && level > o.level)
            {
                continue;
            }
            close(open.take(), out, report);
            open = Some(Open {
                level,
                title: plain(rest),
                first: None,
                last: None,
                refs: Vec::new(),
            });
            continue;
        }
        if marker == "r" {
            if let Some(o) = open.as_mut().filter(|o| o.first.is_none()) {
                o.refs
                    .extend(references(rest).into_iter().map(|r| (r, i + 1)));
            }
            continue;
        }
        for verse in verse_numbers(line) {
            let v = Verse::new(book, chapter, verse);
            if let Some(o) = open.as_mut() {
                o.first.get_or_insert(v);
                o.last = Some(v);
            }
        }
    }
    close(open.take(), out, report);
}

/// The level of a marker that starts a section: `ms`, `ms1`…`ms3` are 0 (a major section);
/// `s` and `s1` are 1, `s2` is 2 and so on. `None` for any other marker.
fn heading_level(marker: &str) -> Option<u8> {
    let number = |n: &str| {
        if n.is_empty() {
            Some(1)
        } else {
            n.parse::<u8>().ok()
        }
    };
    if let Some(n) = marker.strip_prefix("ms") {
        return number(n).map(|_| 0);
    }
    marker.strip_prefix('s').and_then(number)
}

/// A heading's text without inline markers (`\\it`, `\\it*`), whitespace collapsed.
fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            while chars
                .peek()
                .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '+' || *c == '*')
            {
                chars.next();
            }
            continue;
        }
        out.push(c);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The verse numbers of the `\v` markers on a line (`\v 3`, or the last of `\v 3-4`).
fn verse_numbers(line: &str) -> Vec<u16> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(at) = rest.find("\\v ") {
        rest = &rest[at + 3..];
        let number: String = rest
            .trim_start()
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '-')
            .collect();
        let mut parts = number.split('-').filter_map(|n| n.parse::<u16>().ok());
        if let Some(first) = parts.next() {
            out.push(first);
            out.extend(parts.next_back());
        }
    }
    out
}

/// The references of a `\r` line, as written: each `\ref label|target\ref*` gives its target,
/// or `label` when the target names no book; a `\ref` without a target gives its label.
fn references(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in line.split("\\ref*") {
        let Some(at) = part.find("\\ref ") else {
            continue;
        };
        let body = part[at + 5..].trim();
        let (label, target) = match body.split_once('|') {
            Some((label, target)) => (label.trim(), Some(target.trim())),
            None => (body, None),
        };
        let usable = target.filter(|t| {
            t.split_whitespace()
                .next()
                .is_some_and(|code| Book::from_usfm(code).is_some())
        });
        out.push(usable.unwrap_or(label).to_string());
    }
    out
}

/// One written reference as a Passage of one Bible book in the Standard versification, not
/// overlapping `section`; otherwise why it is dropped.
fn parallel(written: &str, section: Passage) -> Result<Passage, &'static str> {
    // A target names its book by USFM code (`MRK 1:9-11`); write the book's name for the
    // reference reader.
    let text = match written.split_once(' ') {
        Some((code, rest)) => match Book::from_usfm(code) {
            Some(book) => format!("{} {rest}", book.name()),
            None => written.to_string(),
        },
        None => written.to_string(),
    };
    let list =
        parse_reference(&text, &Standard, None).map_err(|_| DROPPED_PARALLEL_NOT_STANDARD)?;
    let [passage] = list.passages() else {
        return Err(DROPPED_PARALLEL_NOT_STANDARD);
    };
    if passage.start().book != passage.end().book {
        return Err(DROPPED_PARALLEL_SPANS_BOOKS);
    }
    if passage.overlaps(section) {
        return Err(DROPPED_PARALLEL_SELF);
    }
    Ok(*passage)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn targets_labels_and_book_ranges_read_as_the_bsb_writes_them() {
        let line = "(\\ref Isaiah 40:1–5|ISA 40:1-5\\ref*; \\ref Song of Solomon 1:1–17|1:1-17\\ref*; \
                    \\ref Genesis 4–9\\ref*; \\ref Joshua–Malachi\\ref*)";
        assert_eq!(
            references(line),
            [
                "ISA 40:1-5",
                "Song of Solomon 1:1–17",
                "Genesis 4–9",
                "Joshua–Malachi"
            ]
        );
        let section = Passage::verse(Verse::new(Book::Mark, 1, 1));
        let p = |b, c, v, e| Passage::new(Verse::new(b, c, v), Verse::new(b, c, e)).unwrap();
        assert_eq!(
            parallel("ISA 40:1-5", section),
            Ok(p(Book::Isaiah, 40, 1, 5))
        );
        assert_eq!(
            parallel("Song of Solomon 1:1–17", section),
            Ok(p(Book::SongOfSongs, 1, 1, 17))
        );
        assert_eq!(
            parallel("3JN 13-14", section),
            Ok(p(Book::ThirdJohn, 1, 13, 14))
        );
        assert_eq!(
            parallel("1CH 15:29-16:3", section).map(|p| (p.start(), p.end())),
            Ok((
                Verse::new(Book::FirstChronicles, 15, 29),
                Verse::new(Book::FirstChronicles, 16, 3)
            ))
        );
        assert_eq!(
            parallel("Genesis 4–9", section).map(|p| p.end()),
            Ok(Verse::new(Book::Genesis, 9, 29))
        );
        assert_eq!(
            parallel("Joshua–Malachi", section),
            Err(DROPPED_PARALLEL_SPANS_BOOKS)
        );
        assert_eq!(parallel("MRK 1:1-3", section), Err(DROPPED_PARALLEL_SELF));
        assert_eq!(
            parallel("3JN 1:15", section),
            Err(DROPPED_PARALLEL_NOT_STANDARD)
        );
    }

    /// A subheading stays in its section; a major section's headings start their own; a
    /// section ends at the next heading, across chapters.
    #[test]
    fn sections_run_from_their_heading_to_the_next() {
        let psalms = "\\id PSA\n\\ms BOOK I\n\\c 1\n\\s1 The Two Paths\n\
                      \\r (\\ref Matthew 7:13–14|MAT 7:13-14\\ref*)\n\\q1 \\v 1 Blessed\n\
                      \\s2 Under it\n\\q1 \\v 2 But\n\\c 2\n\\q1 \\v 1 Why\n\
                      \\s1 Next\n\\q1 \\v 2 The kings\n";
        let (r, report) = import_usfm_parallel_passages(
            &[("PSA.usfm".into(), psalms.into())],
            &DatasetOptions {
                resource_id: "parallels".into(),
                ..DatasetOptions::default()
            },
        )
        .unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(
            r.parallel_sections,
            [ParallelSection {
                passage: Passage::new(
                    Verse::new(Book::Psalms, 1, 1),
                    Verse::new(Book::Psalms, 2, 1)
                )
                .unwrap(),
                title: "The Two Paths".into(),
                parallels: vec![
                    Passage::new(
                        Verse::new(Book::Matthew, 7, 13),
                        Verse::new(Book::Matthew, 7, 14)
                    )
                    .unwrap()
                ],
            }]
        );
    }

    #[test]
    fn headings_and_verse_markers() {
        for (heading, level) in [("s", 1), ("s1", 1), ("s2", 2), ("ms", 0), ("ms1", 0)] {
            assert_eq!(heading_level(heading), Some(level), "{heading}");
        }
        for other in ["sp", "sr", "r", "mr", "mt", "d", "p", "c"] {
            assert_eq!(heading_level(other), None, "{other}");
        }
        assert_eq!(
            verse_numbers("\\p \\v 9 In those days \\v 10 As soon"),
            [9, 10]
        );
        assert_eq!(verse_numbers("\\q1 \\v 3-4 text"), [3, 4]);
        assert_eq!(
            plain("David’s \\it Military\\it* Census"),
            "David’s Military Census"
        );
    }
}
