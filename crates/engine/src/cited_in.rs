//! The Passage Guide's Cited in (ADR 0023, #344): the Books in the Library that cite the
//! Passage, earliest first, each with how often and the first place it does.
//!
//! A Book's Bible references are its `scripture_ref` spans, read once per Resource revision
//! ([`ObrReader::scripture_references`](obs_store::ObrReader::scripture_references)) into a
//! compact list ([`BookCitations`]) that later Passages are answered from without opening
//! the file again; only a Book that cites the Passage is opened, for its excerpt.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use obs_model::UnitId;
use obs_scripture::{Passage, PassageList};

use crate::Library;

/// Longest excerpt kept around a citing place.
pub const EXCERPT_CHARS: usize = 420;

/// One Book that cites the Passage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CitingBook {
    pub resource_id: String,
    pub title: String,
    pub abbreviation: String,
    /// Its author, as the Book names them.
    pub author: Option<String>,
    /// Its year, from its edition.
    pub year: Option<u16>,
    /// How many places in it cite the Passage.
    pub count: usize,
    /// The first of them.
    pub first: CitingPlace,
}

/// A place in a Book that cites the Passage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CitingPlace {
    pub unit: UnitId,
    /// The reference's bytes in the unit.
    pub range: (u32, u32),
    /// The Division it is in.
    pub division: Option<String>,
    /// The paragraph around it, whitespace collapsed, at most [`EXCERPT_CHARS`] characters,
    /// and where the reference is inside the excerpt (bytes).
    pub excerpt: String,
    pub mark: (usize, usize),
}

/// One citation of a [`BookCitations`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Citation {
    passage: Passage,
    /// Index into [`BookCitations::units`].
    unit: u32,
    start: u32,
    end: u32,
}

/// One Book's Bible references, compact: each Passage it cites, with where, in document
/// order, and each unit id once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BookCitations {
    citations: Vec<Citation>,
    units: Vec<UnitId>,
}

impl BookCitations {
    /// From a Book's references ([`obs_store::ObrReader::scripture_references`]).
    pub fn new(refs: Vec<(UnitId, std::ops::Range<u32>, PassageList)>) -> Self {
        let mut units: Vec<UnitId> = Vec::new();
        let mut citations = Vec::new();
        for (unit, range, targets) in refs {
            if units.last() != Some(&unit) {
                units.push(unit);
            }
            let u = u32::try_from(units.len() - 1).unwrap_or(u32::MAX);
            for passage in targets.iter() {
                citations.push(Citation {
                    passage: *passage,
                    unit: u,
                    start: range.start,
                    end: range.end,
                });
            }
        }
        Self { citations, units }
    }

    /// The places that cite any Verse of `passage`, in document order, one per reference
    /// (a reference to two Passages that both overlap counts once).
    pub fn citing(&self, passage: Passage) -> Vec<(UnitId, (u32, u32))> {
        let mut out: Vec<(UnitId, (u32, u32))> = Vec::new();
        for c in &self.citations {
            if c.passage.start() > passage.end() || c.passage.end() < passage.start() {
                continue;
            }
            let Some(unit) = self.units.get(c.unit as usize) else {
                continue;
            };
            let place = (unit.clone(), (c.start, c.end));
            if out.last() != Some(&place) {
                out.push(place);
            }
        }
        out
    }

    pub fn len(&self) -> usize {
        self.citations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.citations.is_empty()
    }
}

/// Each Book's citations, by (resource id, revision, content hash), loaded once and shared by
/// later jobs.
#[derive(Debug, Default)]
pub struct CitationCache {
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    books: Mutex<BTreeMap<(String, u32, String), Arc<BookCitations>>>,
}

/// The year in a Book's edition (`1656`, `c. 1650`).
pub fn year_of(edition: Option<&str>) -> Option<u16> {
    let edition = edition?;
    let mut digits = String::new();
    for c in edition.chars().chain([' ']) {
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        if (3..=4).contains(&digits.len())
            && let Ok(y) = digits.parse::<u16>()
            && (100..=2100).contains(&y)
        {
            return Some(y);
        }
        digits.clear();
    }
    None
}

/// The paragraph of `text` around `range`, whitespace collapsed and cut to about
/// [`EXCERPT_CHARS`] characters at word boundaries, keeping the reference in it; and where
/// the reference is in the excerpt.
pub fn excerpt_around(text: &str, range: std::ops::Range<usize>) -> (String, (usize, usize)) {
    let start = range.start.min(text.len());
    let end = range.end.clamp(start, text.len());
    let para_start = text[..start].rfind('\n').map_or(0, |i| i + 1);
    let para_end = text[end..].find('\n').map_or(text.len(), |i| end + i);
    let half = EXCERPT_CHARS / 2;
    // Characters before and after the reference, at most `half` each, on char boundaries.
    let before: Vec<(usize, char)> = text[para_start..start].char_indices().collect();
    let from = if before.len() > half {
        let cut = para_start + before[before.len() - half].0;
        // Start at the next word.
        text[cut..start].find(' ').map_or(cut, |i| cut + i + 1)
    } else {
        para_start
    };
    let after_end = text[end..para_end]
        .char_indices()
        .nth(half)
        .map_or(para_end, |(i, _)| end + i);
    let to = if after_end < para_end {
        text[end..after_end]
            .rfind(' ')
            .map_or(after_end, |i| end + i)
    } else {
        para_end
    };
    let mut out = String::new();
    if from > para_start {
        out.push('…');
    }
    let collapse = |s: &str, out: &mut String| {
        for (i, w) in s.split_whitespace().enumerate() {
            if i > 0 || (s.starts_with(char::is_whitespace) && !out.is_empty()) {
                out.push(' ');
            }
            out.push_str(w);
        }
        if s.ends_with(char::is_whitespace) && !s.trim().is_empty() {
            out.push(' ');
        }
    };
    collapse(&text[from..start], &mut out);
    let mark_start = out.len();
    collapse(&text[start..end], &mut out);
    let mark_end = out.len();
    collapse(&text[end..to], &mut out);
    if to < para_end {
        out.push('…');
    }
    (out, (mark_start, mark_end))
}

/// Order citing Books earliest first: by year (Books without one last), then title.
pub fn sort_earliest_first(books: &mut [CitingBook]) {
    books.sort_by(|a, b| {
        (a.year.is_none(), a.year, a.title.to_lowercase()).cmp(&(
            b.year.is_none(),
            b.year,
            b.title.to_lowercase(),
        ))
    });
}

#[cfg(not(target_arch = "wasm32"))]
mod desktop {
    use super::*;
    use obs_store::{ObrReader, ResourceStore, StoreError};

    impl CitationCache {
        fn citations(
            &self,
            entry: &crate::library::LibraryEntry,
            reader: &ObrReader,
        ) -> Result<Arc<BookCitations>, StoreError> {
            let m = &entry.manifest;
            let identity = (m.resource_id.clone(), m.revision, m.content_hash.clone());
            if let Some(c) = self
                .books
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .get(&identity)
            {
                return Ok(Arc::clone(c));
            }
            let loaded = Arc::new(BookCitations::new(reader.scripture_references()?));
            let mut books = self.books.lock().unwrap_or_else(|p| p.into_inner());
            books.retain(|(id, ..), _| *id != m.resource_id);
            books.insert(identity, Arc::clone(&loaded));
            Ok(loaded)
        }

        /// Whether a Book's citations are already loaded (no file read needed).
        fn loaded(&self, entry: &crate::library::LibraryEntry) -> Option<Arc<BookCitations>> {
            let m = &entry.manifest;
            self.books
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .get(&(m.resource_id.clone(), m.revision, m.content_hash.clone()))
                .cloned()
        }

        /// Every Book in `library` that cites `passage`, earliest first. A Book that cannot be
        /// read is left out; if every one fails, the section fails.
        pub fn citing_books(
            &self,
            library: &Library,
            passage: Passage,
            cancelled: &dyn Fn() -> bool,
        ) -> Result<Vec<CitingBook>, String> {
            let books: Vec<_> = library.of_type(obs_model::ResourceType::Book).collect();
            let mut out = Vec::new();
            let mut errors = Vec::new();
            for entry in &books {
                if cancelled() {
                    return Err("cancelled".into());
                }
                match self.book_row(entry, passage) {
                    Ok(Some(row)) => out.push(row),
                    Ok(None) => {}
                    Err(e) => errors.push(format!("{}: {e}", entry.manifest.abbreviation)),
                }
            }
            if !books.is_empty() && errors.len() == books.len() {
                return Err(errors.join("; "));
            }
            sort_earliest_first(&mut out);
            Ok(out)
        }

        fn book_row(
            &self,
            entry: &crate::library::LibraryEntry,
            passage: Passage,
        ) -> Result<Option<CitingBook>, StoreError> {
            let (citations, reader) = match self.loaded(entry) {
                Some(c) => (c, None),
                None => {
                    let reader = ObrReader::open(&entry.path)?;
                    (self.citations(entry, &reader)?, Some(reader))
                }
            };
            let places = citations.citing(passage);
            let Some((unit, (start, end))) = places.first().cloned() else {
                return Ok(None);
            };
            let reader = match reader {
                Some(r) => r,
                None => ObrReader::open(&entry.path)?,
            };
            let (division, excerpt, mark) = match reader.unit(&unit)? {
                Some(u) => {
                    let (excerpt, mark) = excerpt_around(&u.text, start as usize..end as usize);
                    (u.title, excerpt, mark)
                }
                None => (None, String::new(), (0, 0)),
            };
            let m = &entry.manifest;
            Ok(Some(CitingBook {
                resource_id: m.resource_id.clone(),
                title: m.title.clone(),
                abbreviation: m.abbreviation.clone(),
                author: m.publisher.clone(),
                year: year_of(m.edition.as_deref()),
                count: places.len(),
                first: CitingPlace {
                    unit,
                    range: (start, end),
                    division,
                    excerpt,
                    mark,
                },
            }))
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl CitationCache {
    pub fn citing_books(
        &self,
        _: &Library,
        _: Passage,
        _: &dyn Fn() -> bool,
    ) -> Result<Vec<CitingBook>, String> {
        Err("Opening Resources is not supported here".into())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use obs_scripture::{Book, Verse};

    #[test]
    fn citations_answer_overlapping_passages_once_per_reference() {
        let v = |c, n| Verse::new(Book::Romans, c, n);
        let list = |ps: &[Passage]| {
            let mut l = PassageList::new();
            for p in ps {
                l.push(*p);
            }
            l
        };
        let unit = |s: &str| UnitId::new(s).unwrap();
        let c = BookCitations::new(vec![
            (unit("div.1"), 10..20, list(&[Passage::verse(v(8, 13))])),
            (
                unit("div.1"),
                30..50,
                list(&[
                    Passage::new(v(8, 12), v(8, 14)).unwrap(),
                    Passage::verse(v(8, 13)),
                ]),
            ),
            (unit("div.2"), 5..9, list(&[Passage::verse(v(9, 1))])),
        ]);
        assert_eq!(c.len(), 4);
        assert_eq!(
            c.citing(Passage::verse(v(8, 13))),
            [(unit("div.1"), (10, 20)), (unit("div.1"), (30, 50))]
        );
        assert_eq!(c.citing(Passage::new(v(8, 14), v(9, 2)).unwrap()).len(), 2);
        assert!(c.citing(Passage::verse(v(1, 1))).is_empty());
    }

    #[test]
    fn excerpts_keep_the_reference_inside_its_paragraph() {
        let text =
            "Before.\nIf ye through the Spirit do mortify, Rom. 8. 13, ye shall live.\nAfter.";
        let at = text.find("Rom.").unwrap();
        let (excerpt, (a, b)) = excerpt_around(text, at..at + "Rom. 8. 13".len());
        assert_eq!(
            excerpt,
            "If ye through the Spirit do mortify, Rom. 8. 13, ye shall live."
        );
        assert_eq!(&excerpt[a..b], "Rom. 8. 13");
        let long = format!("{} Rom. 8. 13 {}", "word ".repeat(200), "more ".repeat(200));
        let at = long.find("Rom.").unwrap();
        let (excerpt, (a, b)) = excerpt_around(&long, at..at + 10);
        assert!(
            excerpt.starts_with('…') && excerpt.ends_with('…'),
            "{excerpt}"
        );
        assert!(excerpt.chars().count() <= EXCERPT_CHARS + 4);
        assert_eq!(&excerpt[a..b], "Rom. 8. 13");
    }

    #[test]
    fn years_and_order() {
        assert_eq!(year_of(Some("1656")), Some(1656));
        assert_eq!(year_of(Some("c. 1650")), Some(1650));
        assert_eq!(year_of(Some("vol. 2")), None);
        assert_eq!(year_of(None), None);
    }
}
