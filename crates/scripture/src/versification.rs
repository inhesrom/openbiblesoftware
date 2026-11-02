//! Versifications, and the Standard versification every non-Bible Resource speaks.

use std::sync::OnceLock;

use crate::book::Book;
use crate::standard_data::{CHAPTER_VERSES, TITLED_PSALMS};
use crate::verse::Verse;

/// A scheme for numbering chapters and verses.
///
/// Every Bible has its own (the numbering it shows); [`Standard`] is the shared one. Verse 0
/// of a chapter is its title slot, which exists only where [`Versification::has_title`] says so.
pub trait Versification {
    /// Number of chapters in `book`, or 0 if this Versification lacks the book.
    fn chapter_count(&self, book: Book) -> u16;

    /// Highest verse number in a chapter, or `None` if the chapter does not exist.
    fn last_verse(&self, book: Book, chapter: u16) -> Option<u16>;

    /// Whether the chapter has a title slot (verse 0), as Psalm superscriptions do.
    fn has_title(&self, book: Book, chapter: u16) -> bool;

    /// Whether `verse` exists in this Versification.
    fn contains(&self, verse: Verse) -> bool {
        match self.last_verse(verse.book, verse.chapter) {
            None => false,
            Some(last) if verse.verse == 0 => last > 0 && self.has_title(verse.book, verse.chapter),
            Some(last) => verse.verse <= last,
        }
    }

    /// The first Verse of a chapter: its title slot if it has one, else verse 1.
    fn first_verse(&self, book: Book, chapter: u16) -> Option<Verse> {
        self.last_verse(book, chapter)?;
        let v = if self.has_title(book, chapter) { 0 } else { 1 };
        Some(Verse::new(book, chapter, v))
    }

    /// The last Verse of a chapter.
    fn last_verse_of(&self, book: Book, chapter: u16) -> Option<Verse> {
        self.last_verse(book, chapter)
            .map(|v| Verse::new(book, chapter, v))
    }

    /// The first Verse of a book.
    fn first_verse_of_book(&self, book: Book) -> Option<Verse> {
        (self.chapter_count(book) > 0)
            .then(|| self.first_verse(book, 1))
            .flatten()
    }

    /// The last Verse of a book.
    fn last_verse_of_book(&self, book: Book) -> Option<Verse> {
        self.last_verse_of(book, self.chapter_count(book))
    }

    /// The Verse after `verse`, crossing chapter and book boundaries.
    fn next_verse(&self, verse: Verse) -> Option<Verse> {
        if !self.contains(verse) {
            return None;
        }
        let last = self.last_verse(verse.book, verse.chapter)?;
        if verse.verse < last {
            return Some(Verse::new(verse.book, verse.chapter, verse.verse + 1));
        }
        if verse.chapter < self.chapter_count(verse.book) {
            return self.first_verse(verse.book, verse.chapter + 1);
        }
        let mut book = verse.book.next();
        while let Some(b) = book {
            if let Some(v) = self.first_verse_of_book(b) {
                return Some(v);
            }
            book = b.next();
        }
        None
    }

    /// The Verse before `verse`, crossing chapter and book boundaries.
    fn prev_verse(&self, verse: Verse) -> Option<Verse> {
        if !self.contains(verse) {
            return None;
        }
        let first = self.first_verse(verse.book, verse.chapter)?;
        if verse.verse > first.verse {
            return Some(Verse::new(verse.book, verse.chapter, verse.verse - 1));
        }
        if verse.chapter > 1 {
            return self.last_verse_of(verse.book, verse.chapter - 1);
        }
        let mut book = verse.book.prev();
        while let Some(b) = book {
            if let Some(v) = self.last_verse_of_book(b) {
                return Some(v);
            }
            book = b.prev();
        }
        None
    }
}

/// The Standard versification: KJV numbering of the 66-book canon, with a title slot for each
/// Psalm superscription (ADR 0002).
///
/// Every Standard Verse also has an **ordinal**, its zero-based position in canonical order.
/// Ordinals are an in-memory convenience; never store them (store [`Verse`] values instead).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Standard;

struct Ordinals {
    /// Index into `chapter_start` of each book's chapter 1.
    book_base: [usize; 66],
    /// Ordinal of the first Verse (title slot or verse 1) of each chapter, plus a final total.
    chapter_start: Vec<u32>,
}

fn ordinals() -> &'static Ordinals {
    static TABLE: OnceLock<Ordinals> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut book_base = [0; 66];
        let mut chapter_start = Vec::with_capacity(1190);
        let mut next = 0u32;
        for book in Book::ALL {
            book_base[book.index()] = chapter_start.len();
            for (i, &count) in CHAPTER_VERSES[book.index()].iter().enumerate() {
                chapter_start.push(next);
                let title = u32::from(Standard.has_title(book, (i + 1) as u16));
                next += u32::from(count) + title;
            }
        }
        chapter_start.push(next);
        Ordinals {
            book_base,
            chapter_start,
        }
    })
}

impl Standard {
    /// Total number of Standard Verses, title slots included.
    pub fn verse_count(self) -> u32 {
        ordinals().chapter_start.last().copied().unwrap_or(0)
    }

    /// Zero-based canonical position of a Standard Verse.
    pub fn ordinal(self, verse: Verse) -> Option<u32> {
        if !self.contains(verse) {
            return None;
        }
        let table = ordinals();
        let start =
            table.chapter_start[table.book_base[verse.book.index()] + verse.chapter as usize - 1];
        let offset = if self.has_title(verse.book, verse.chapter) {
            u32::from(verse.verse)
        } else {
            u32::from(verse.verse) - 1
        };
        Some(start + offset)
    }

    /// The Standard Verse at a zero-based canonical position.
    pub fn verse_at(self, ordinal: u32) -> Option<Verse> {
        let table = ordinals();
        if ordinal >= self.verse_count() {
            return None;
        }
        // Index of the last chapter whose start is <= ordinal.
        let idx = table.chapter_start.partition_point(|&s| s <= ordinal) - 1;
        let book_idx = table.book_base.partition_point(|&b| b <= idx) - 1;
        let book = Book::from_index(book_idx)?;
        let chapter = (idx - table.book_base[book_idx] + 1) as u16;
        let offset = (ordinal - table.chapter_start[idx]) as u16;
        let verse = if self.has_title(book, chapter) {
            offset
        } else {
            offset + 1
        };
        Some(Verse::new(book, chapter, verse))
    }

    /// Every Standard Verse in canonical order.
    pub fn verses(self) -> impl Iterator<Item = Verse> {
        (0..self.verse_count()).filter_map(move |o| self.verse_at(o))
    }
}

impl Versification for Standard {
    fn chapter_count(&self, book: Book) -> u16 {
        CHAPTER_VERSES[book.index()].len() as u16
    }

    fn last_verse(&self, book: Book, chapter: u16) -> Option<u16> {
        let index = usize::from(chapter).checked_sub(1)?;
        CHAPTER_VERSES[book.index()].get(index).copied()
    }

    fn has_title(&self, book: Book, chapter: u16) -> bool {
        book == Book::Psalms && TITLED_PSALMS.binary_search(&chapter).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kjv_totals() {
        let chapters: u32 = Book::ALL
            .iter()
            .map(|&b| u32::from(Standard.chapter_count(b)))
            .sum();
        assert_eq!(chapters, 1189);
        assert_eq!(Standard.verse_count(), 31_102 + TITLED_PSALMS.len() as u32);
        assert_eq!(Standard.last_verse(Book::Psalms, 119), Some(176));
        assert_eq!(Standard.last_verse(Book::ThirdJohn, 1), Some(14));
        assert_eq!(Standard.last_verse(Book::Malachi, 4), Some(6));
        assert_eq!(Standard.last_verse(Book::Genesis, 51), None);
        assert_eq!(Standard.last_verse(Book::Genesis, 0), None);
    }

    #[test]
    fn every_verse_round_trips_through_its_ordinal() {
        let mut expected = 0u32;
        let mut prev: Option<Verse> = None;
        for verse in Standard.verses() {
            assert_eq!(Standard.ordinal(verse), Some(expected), "{verse:?}");
            if let Some(p) = prev {
                assert!(p < verse, "{p:?} !< {verse:?}");
                assert_eq!(Standard.next_verse(p), Some(verse));
                assert_eq!(Standard.prev_verse(verse), Some(p));
            }
            prev = Some(verse);
            expected += 1;
        }
        assert_eq!(expected, Standard.verse_count());
        assert_eq!(Standard.verse_at(expected), None);
        assert_eq!(
            Standard.next_verse(Verse::new(Book::Revelation, 22, 21)),
            None
        );
        assert_eq!(Standard.prev_verse(Verse::new(Book::Genesis, 1, 1)), None);
    }

    #[test]
    fn psalm_title_slots_match_tvtms() {
        let titled: Vec<u16> = (1..=150)
            .filter(|&c| Standard.has_title(Book::Psalms, c))
            .collect();
        assert_eq!(titled, TITLED_PSALMS.to_vec());
        assert_eq!(titled.len(), 117);
        // Spot checks against TVTMS StandardRef values.
        for c in [3, 51, 90, 119, 145] {
            assert!(
                Standard.contains(Verse::new(Book::Psalms, c, 0)),
                "Ps {c} title"
            );
        }
        for c in [1, 2, 10, 33, 150] {
            assert!(
                !Standard.contains(Verse::new(Book::Psalms, c, 0)),
                "Ps {c} has no title"
            );
        }
        assert!(!Standard.contains(Verse::new(Book::Genesis, 1, 0)));
        assert_eq!(
            Standard.first_verse(Book::Psalms, 51),
            Some(Verse::new(Book::Psalms, 51, 0))
        );
        assert_eq!(
            Standard.next_verse(Verse::new(Book::Psalms, 2, 12)),
            Some(Verse::new(Book::Psalms, 3, 0))
        );
    }

    #[test]
    fn book_boundaries() {
        assert_eq!(
            Standard.next_verse(Verse::new(Book::Malachi, 4, 6)),
            Some(Verse::new(Book::Matthew, 1, 1))
        );
        assert_eq!(
            Standard.prev_verse(Verse::new(Book::Matthew, 1, 1)),
            Some(Verse::new(Book::Malachi, 4, 6))
        );
    }

    #[test]
    fn invalid_verses_have_no_ordinal() {
        assert_eq!(Standard.ordinal(Verse::new(Book::John, 3, 37)), None);
        assert_eq!(Standard.ordinal(Verse::new(Book::John, 22, 1)), None);
        assert_eq!(Standard.next_verse(Verse::new(Book::John, 3, 37)), None);
    }
}
