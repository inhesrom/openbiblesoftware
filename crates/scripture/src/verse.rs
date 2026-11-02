//! Verses, Passages and Passage lists.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::book::Book;
use crate::versification::Versification;

/// One point in Scripture: Bible book, chapter and verse number in some Versification.
///
/// Verse 0 is a chapter's title slot (a Psalm superscription). Verses order canonically:
/// by book, then chapter, then verse. Whether a Verse exists is a question for a
/// [`Versification`], not for this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Verse {
    pub book: Book,
    pub chapter: u16,
    pub verse: u16,
}

impl Verse {
    pub const fn new(book: Book, chapter: u16, verse: u16) -> Self {
        Self {
            book,
            chapter,
            verse,
        }
    }

    /// Whether this is a chapter's title slot (verse 0).
    pub const fn is_title(self) -> bool {
        self.verse == 0
    }

    /// OSIS-style id, e.g. `John.3.16`, or `Ps.3.0` for a title slot.
    pub fn osis(self) -> String {
        format!("{}.{}.{}", self.book.osis(), self.chapter, self.verse)
    }
}

impl fmt::Display for Verse {
    /// Short form, e.g. `John 3:16`, `Ps 3:title`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_title() {
            write!(f, "{} {}:title", self.book.abbrev(), self.chapter)
        } else {
            write!(f, "{} {}:{}", self.book.abbrev(), self.chapter, self.verse)
        }
    }
}

/// Why a [`Passage`] could not be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassageError {
    /// The end comes before the start.
    Reversed { start: Verse, end: Verse },
}

impl fmt::Display for PassageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PassageError::Reversed { start, end } => {
                write!(f, "passage end {end} comes before its start {start}")
            }
        }
    }
}

impl std::error::Error for PassageError {}

/// A contiguous range of Verses, inclusive at both ends. It may cross chapters or books.
/// One Verse or a whole chapter is still a Passage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "PassageRepr", into = "PassageRepr")]
pub struct Passage {
    start: Verse,
    end: Verse,
}

#[derive(Serialize, Deserialize)]
struct PassageRepr {
    start: Verse,
    end: Verse,
}

impl TryFrom<PassageRepr> for Passage {
    type Error = PassageError;
    fn try_from(r: PassageRepr) -> Result<Self, Self::Error> {
        Passage::new(r.start, r.end)
    }
}

impl From<Passage> for PassageRepr {
    fn from(p: Passage) -> Self {
        PassageRepr {
            start: p.start,
            end: p.end,
        }
    }
}

impl Passage {
    /// A Passage from `start` to `end` inclusive.
    pub fn new(start: Verse, end: Verse) -> Result<Self, PassageError> {
        if end < start {
            return Err(PassageError::Reversed { start, end });
        }
        Ok(Self { start, end })
    }

    /// A one-Verse Passage.
    pub const fn verse(verse: Verse) -> Self {
        Self {
            start: verse,
            end: verse,
        }
    }

    /// A whole chapter (title slot included), if the Versification has it.
    pub fn chapter(v11n: &impl Versification, book: Book, chapter: u16) -> Option<Self> {
        Some(Self {
            start: v11n.first_verse(book, chapter)?,
            end: v11n.last_verse_of(book, chapter)?,
        })
    }

    /// A whole book, if the Versification has it.
    pub fn book(v11n: &impl Versification, book: Book) -> Option<Self> {
        Some(Self {
            start: v11n.first_verse_of_book(book)?,
            end: v11n.last_verse_of_book(book)?,
        })
    }

    pub const fn start(self) -> Verse {
        self.start
    }

    pub const fn end(self) -> Verse {
        self.end
    }

    pub fn is_single_verse(self) -> bool {
        self.start == self.end
    }

    /// Whether `verse` lies inside this Passage.
    pub fn contains_verse(self, verse: Verse) -> bool {
        self.start <= verse && verse <= self.end
    }

    /// Whether `other` lies entirely inside this Passage.
    pub fn contains(self, other: Passage) -> bool {
        self.start <= other.start && other.end <= self.end
    }

    /// Whether the two Passages share at least one Verse.
    pub fn overlaps(self, other: Passage) -> bool {
        self.start <= other.end && other.start <= self.end
    }

    /// The Verses shared by both Passages.
    pub fn intersection(self, other: Passage) -> Option<Passage> {
        self.overlaps(other).then(|| Passage {
            start: self.start.max(other.start),
            end: self.end.min(other.end),
        })
    }

    /// The smallest Passage covering both.
    pub fn span(self, other: Passage) -> Passage {
        Passage {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    /// Every Verse of this Passage that exists in the Versification, in order.
    pub fn verses<'a, V: Versification>(self, v11n: &'a V) -> impl Iterator<Item = Verse> + 'a {
        let first = if v11n.contains(self.start) {
            Some(self.start)
        } else {
            first_at_or_after(v11n, self.start)
        };
        std::iter::successors(first, move |&v| v11n.next_verse(v))
            .take_while(move |&v| v <= self.end)
    }
}

/// The first Verse of the Versification at or after `verse` (which need not exist).
fn first_at_or_after(v11n: &impl Versification, verse: Verse) -> Option<Verse> {
    let mut book = Some(verse.book);
    while let Some(b) = book {
        let chapters = v11n.chapter_count(b);
        let (from_chapter, from_verse) = if b == verse.book {
            (verse.chapter.max(1), verse.verse)
        } else {
            (1, 0)
        };
        for c in from_chapter..=chapters {
            let start = if c == from_chapter { from_verse } else { 0 };
            let Some(first) = v11n.first_verse(b, c) else {
                continue;
            };
            let Some(last) = v11n.last_verse(b, c) else {
                continue;
            };
            let candidate = start.max(first.verse);
            if candidate <= last {
                return Some(Verse::new(b, c, candidate));
            }
        }
        book = b.next();
    }
    None
}

impl fmt::Display for Passage {
    /// Short form, e.g. `John 3:16-18`, `Gen 1:1-2:3`, `Mal 4:6-Matt 1:1`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (s, e) = (self.start, self.end);
        if s == e {
            return write!(f, "{s}");
        }
        let verse = |v: Verse| {
            if v.is_title() {
                "title".to_string()
            } else {
                v.verse.to_string()
            }
        };
        if s.book != e.book {
            write!(f, "{s}-{e}")
        } else if s.chapter != e.chapter {
            write!(f, "{s}-{}:{}", e.chapter, verse(e))
        } else {
            write!(f, "{s}-{}", verse(e))
        }
    }
}

/// An ordered list of Passages, used where a single location is not required
/// (for example a Search scope).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PassageList(Vec<Passage>);

impl PassageList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, passage: Passage) {
        self.0.push(passage);
    }

    pub fn passages(&self) -> &[Passage] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Passage> {
        self.0.iter()
    }

    pub fn contains_verse(&self, verse: Verse) -> bool {
        self.0.iter().any(|p| p.contains_verse(verse))
    }

    pub fn overlaps(&self, passage: Passage) -> bool {
        self.0.iter().any(|p| p.overlaps(passage))
    }

    /// Sorted, with overlapping Passages and Passages adjacent in `v11n` merged.
    pub fn merged(&self, v11n: &impl Versification) -> PassageList {
        let mut sorted = self.0.clone();
        sorted.sort();
        let mut out: Vec<Passage> = Vec::with_capacity(sorted.len());
        for p in sorted {
            if let Some(last) = out.last_mut() {
                let adjacent = v11n.next_verse(last.end) == Some(p.start);
                if last.overlaps(p) || adjacent {
                    *last = last.span(p);
                    continue;
                }
            }
            out.push(p);
        }
        PassageList(out)
    }
}

impl From<Vec<Passage>> for PassageList {
    fn from(v: Vec<Passage>) -> Self {
        Self(v)
    }
}

impl FromIterator<Passage> for PassageList {
    fn from_iter<I: IntoIterator<Item = Passage>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl<'a> IntoIterator for &'a PassageList {
    type Item = &'a Passage;
    type IntoIter = std::slice::Iter<'a, Passage>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl fmt::Display for PassageList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, p) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str("; ")?;
            }
            write!(f, "{p}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::versification::Standard;

    fn v(book: Book, c: u16, v: u16) -> Verse {
        Verse::new(book, c, v)
    }

    fn p(a: Verse, b: Verse) -> Passage {
        Passage::new(a, b).unwrap()
    }

    #[test]
    fn reversed_passage_is_an_error() {
        let a = v(Book::John, 3, 16);
        let b = v(Book::John, 3, 15);
        assert_eq!(
            Passage::new(a, b),
            Err(PassageError::Reversed { start: a, end: b })
        );
    }

    #[test]
    fn containment_across_chapters_and_books() {
        let gen1_to_3 = p(v(Book::Genesis, 1, 1), v(Book::Genesis, 3, 24));
        assert!(gen1_to_3.contains_verse(v(Book::Genesis, 2, 25)));
        assert!(!gen1_to_3.contains_verse(v(Book::Genesis, 4, 1)));
        let mal_to_matt = p(v(Book::Malachi, 4, 1), v(Book::Matthew, 1, 5));
        assert!(mal_to_matt.contains_verse(v(Book::Malachi, 4, 6)));
        assert!(mal_to_matt.contains_verse(v(Book::Matthew, 1, 1)));
        assert!(!mal_to_matt.contains_verse(v(Book::Matthew, 1, 6)));
        assert!(mal_to_matt.contains(p(v(Book::Malachi, 4, 6), v(Book::Matthew, 1, 1))));
        assert!(!mal_to_matt.contains(p(v(Book::Malachi, 3, 18), v(Book::Malachi, 4, 1))));
    }

    #[test]
    fn overlap_and_intersection() {
        let a = p(v(Book::Romans, 8, 28), v(Book::Romans, 9, 5));
        let b = p(v(Book::Romans, 9, 5), v(Book::Romans, 9, 10));
        let c = p(v(Book::Romans, 9, 6), v(Book::Romans, 9, 10));
        assert!(a.overlaps(b) && b.overlaps(a));
        assert!(!a.overlaps(c) && !c.overlaps(a));
        assert_eq!(
            a.intersection(b),
            Some(Passage::verse(v(Book::Romans, 9, 5)))
        );
        assert_eq!(a.intersection(c), None);
        assert_eq!(a.span(c), p(v(Book::Romans, 8, 28), v(Book::Romans, 9, 10)));
    }

    #[test]
    fn overlap_matches_brute_force() {
        // Every pair of Passages over a window that crosses a chapter and a book boundary.
        let window: Vec<Verse> = Passage::new(v(Book::Malachi, 3, 16), v(Book::Matthew, 1, 3))
            .unwrap()
            .verses(&Standard)
            .collect();
        assert_eq!(window.len(), 3 + 6 + 3);
        let passages: Vec<Passage> = (0..window.len())
            .flat_map(|i| (i..window.len()).map(move |j| (i, j)))
            .map(|(i, j)| p(window[i], window[j]))
            .collect();
        for &a in &passages {
            let set_a: Vec<Verse> = a.verses(&Standard).collect();
            for &b in &passages {
                let set_b: Vec<Verse> = b.verses(&Standard).collect();
                let shared = set_a.iter().any(|x| set_b.contains(x));
                assert_eq!(a.overlaps(b), shared, "{a} vs {b}");
                let inside = set_b.iter().all(|x| set_a.contains(x));
                assert_eq!(a.contains(b), inside, "{a} contains {b}");
            }
        }
    }

    #[test]
    fn chapter_and_book_passages() {
        let ps51 = Passage::chapter(&Standard, Book::Psalms, 51).unwrap();
        assert_eq!(ps51.start(), v(Book::Psalms, 51, 0));
        assert_eq!(ps51.end(), v(Book::Psalms, 51, 19));
        assert_eq!(ps51.verses(&Standard).count(), 20);
        let jude = Passage::book(&Standard, Book::Jude).unwrap();
        assert_eq!(jude.to_string(), "Jude 1:1-25");
        assert_eq!(Passage::chapter(&Standard, Book::Jude, 2), None);
    }

    #[test]
    fn verses_skips_numbers_the_versification_lacks() {
        // Starts at a verse that does not exist (Jn 3:40), so iteration begins at Jn 4:1.
        let odd = p(v(Book::John, 3, 40), v(Book::John, 4, 2));
        let got: Vec<_> = odd.verses(&Standard).collect();
        assert_eq!(got, vec![v(Book::John, 4, 1), v(Book::John, 4, 2)]);
    }

    #[test]
    fn display_forms() {
        assert_eq!(v(Book::John, 3, 16).to_string(), "John 3:16");
        assert_eq!(v(Book::Psalms, 3, 0).to_string(), "Ps 3:title");
        assert_eq!(
            p(v(Book::John, 3, 16), v(Book::John, 3, 18)).to_string(),
            "John 3:16-18"
        );
        assert_eq!(
            p(v(Book::Genesis, 1, 1), v(Book::Genesis, 2, 3)).to_string(),
            "Gen 1:1-2:3"
        );
        assert_eq!(
            p(v(Book::Malachi, 4, 6), v(Book::Matthew, 1, 1)).to_string(),
            "Mal 4:6-Matt 1:1"
        );
        assert_eq!(v(Book::Psalms, 3, 0).osis(), "Ps.3.0");
    }

    #[test]
    fn passage_list_merges_overlaps_and_neighbours() {
        let list: PassageList = vec![
            p(v(Book::Romans, 5, 8), v(Book::Romans, 5, 8)),
            p(v(Book::John, 3, 16), v(Book::John, 3, 18)),
            p(v(Book::John, 3, 17), v(Book::John, 3, 21)),
            p(v(Book::John, 3, 36), v(Book::John, 4, 1)),
            p(v(Book::Romans, 5, 9), v(Book::Romans, 5, 10)),
        ]
        .into();
        let merged = list.merged(&Standard);
        assert_eq!(
            merged.to_string(),
            "John 3:16-21; John 3:36-4:1; Rom 5:8-10"
        );
        assert!(list.contains_verse(v(Book::John, 3, 20)));
        assert!(!list.contains_verse(v(Book::John, 3, 22)));
    }

    #[test]
    fn serde_round_trip_and_rejects_reversed() {
        let a = p(v(Book::Psalms, 51, 0), v(Book::Psalms, 51, 2));
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(
            json,
            r#"{"start":{"book":"PSA","chapter":51,"verse":0},"end":{"book":"PSA","chapter":51,"verse":2}}"#
        );
        assert_eq!(serde_json::from_str::<Passage>(&json).unwrap(), a);
        let bad = r#"{"start":{"book":"PSA","chapter":51,"verse":2},"end":{"book":"PSA","chapter":51,"verse":0}}"#;
        assert!(serde_json::from_str::<Passage>(bad).is_err());
    }
}
