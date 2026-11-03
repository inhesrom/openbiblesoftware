//! Verse maps: how one Bible's own Verses correspond to the Standard versification (ADR 0002).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::book::Book;
use crate::verse::{Passage, Verse};
use crate::versification::Versification;

/// One Bible Verse (own numbering) and the Standard Passage it corresponds to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VerseMapEntry {
    /// The Bible Verse, in the Bible's own numbering.
    pub verse: Verse,
    /// Last verse number it covers (`\v 1-2` → 2); equals `verse.verse` otherwise.
    pub last: u16,
    /// The Standard Passage, or `None` for an Unmapped verse.
    pub standard: Option<Passage>,
}

/// A two-way correspondence between one Bible's Verses and the Standard versification, in
/// whole Verses. One Bible Verse maps to a Standard Passage (usually one Verse); several Bible
/// Verses may share a Standard Verse; Unmapped and Absent verses are explicit.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerseMap {
    /// One entry per Bible Verse, in the Bible's document order.
    entries: Vec<VerseMapEntry>,
    /// Standard Verses this Bible does not contain, in canonical order.
    absent: Vec<Verse>,
    /// Standard Verse → indexes into `entries`.
    #[serde(skip)]
    reverse: BTreeMap<Verse, Vec<usize>>,
}

impl VerseMap {
    /// Build from entries (document order) and Absent Standard Verses.
    pub fn new(
        entries: Vec<VerseMapEntry>,
        mut absent: Vec<Verse>,
        standard: &impl Versification,
    ) -> Self {
        absent.sort();
        absent.dedup();
        let mut map = Self {
            entries,
            absent,
            reverse: BTreeMap::new(),
        };
        map.index(standard);
        map
    }

    fn index<V: Versification + ?Sized>(&mut self, standard: &V) {
        self.reverse.clear();
        for (i, e) in self.entries.iter().enumerate() {
            if let Some(p) = e.standard {
                for v in p.verses(standard) {
                    self.reverse.entry(v).or_default().push(i);
                }
            }
        }
    }

    /// Rebuild the reverse index after deserializing.
    pub fn reindex<V: Versification + ?Sized>(&mut self, standard: &V) {
        self.index(standard);
    }

    pub fn entries(&self) -> &[VerseMapEntry] {
        &self.entries
    }

    /// Absent verses: Standard Verses this Bible lacks, within the Bible books it has.
    pub fn absent(&self) -> &[Verse] {
        &self.absent
    }

    pub fn is_absent(&self, standard: Verse) -> bool {
        self.absent.binary_search(&standard).is_ok()
    }

    /// Bible Verses with no Standard counterpart.
    pub fn unmapped(&self) -> impl Iterator<Item = &VerseMapEntry> {
        self.entries.iter().filter(|e| e.standard.is_none())
    }

    /// The entry for a Bible Verse (own numbering); a number inside a verse range finds it.
    pub fn entry(&self, native: Verse) -> Option<&VerseMapEntry> {
        self.entries.iter().find(|e| {
            e.verse.book == native.book
                && e.verse.chapter == native.chapter
                && e.verse.verse <= native.verse
                && native.verse <= e.last
        })
    }

    /// The Standard Passage of a Bible Verse; `None` if the verse is Unmapped or unknown.
    pub fn to_standard(&self, native: Verse) -> Option<Passage> {
        self.entry(native).and_then(|e| e.standard)
    }

    /// The Bible Verses that correspond to a Standard Verse, in document order.
    pub fn to_native(&self, standard: Verse) -> Vec<Verse> {
        self.reverse
            .get(&standard)
            .map(|ix| ix.iter().map(|&i| self.entries[i].verse).collect())
            .unwrap_or_default()
    }

    /// The first Bible Verse a Standard Verse maps to, as Link-set followers use (ADR 0002
    /// rule 9).
    pub fn first_native(&self, standard: Verse) -> Option<Verse> {
        self.reverse
            .get(&standard)
            .and_then(|ix| ix.first())
            .map(|&i| self.entries[i].verse)
    }

    /// Whether every mapped entry is the identity (the Bible numbers like the Standard).
    pub fn is_identity(&self) -> bool {
        self.entries.iter().all(|e| {
            e.standard.is_some_and(|p| {
                p.start() == e.verse && p.end() == Verse::new(e.verse.book, e.verse.chapter, e.last)
            })
        }) && self.absent.is_empty()
    }
}

/// A Versification read off a Bible's own verse list: what the Bible actually numbers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListedVersification {
    /// (book, chapter) → highest verse number.
    last: BTreeMap<(Book, u16), u16>,
    chapters: BTreeMap<Book, u16>,
    titles: BTreeSet<(Book, u16)>,
}

impl ListedVersification {
    /// From `(verse, last)` pairs; `last` > `verse.verse` for verse ranges.
    pub fn new(verses: impl IntoIterator<Item = (Verse, u16)>) -> Self {
        let mut s = Self::default();
        for (v, last) in verses {
            let end = last.max(v.verse);
            let slot = s.last.entry((v.book, v.chapter)).or_insert(0);
            *slot = (*slot).max(end);
            let ch = s.chapters.entry(v.book).or_insert(0);
            *ch = (*ch).max(v.chapter);
            if v.verse == 0 {
                s.titles.insert((v.book, v.chapter));
            }
        }
        s
    }
}

impl Versification for ListedVersification {
    fn chapter_count(&self, book: Book) -> u16 {
        self.chapters.get(&book).copied().unwrap_or(0)
    }

    fn last_verse(&self, book: Book, chapter: u16) -> Option<u16> {
        if chapter == 0 {
            return None;
        }
        match self.last.get(&(book, chapter)) {
            Some(&v) => Some(v),
            // A chapter inside the book's range with no verses (rare) still exists, empty.
            None if chapter <= self.chapter_count(book) => Some(0),
            None => None,
        }
    }

    fn has_title(&self, book: Book, chapter: u16) -> bool {
        self.titles.contains(&(book, chapter))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::versification::Standard;

    fn v(b: Book, c: u16, n: u16) -> Verse {
        Verse::new(b, c, n)
    }

    #[test]
    fn two_way_lookup_with_merges_splits_and_gaps() {
        let p = |a, b| Passage::new(a, b).unwrap();
        let entries = vec![
            // Hebrew Ps 51:1-2 are the Standard title; 51:3 is Standard 51:1.
            VerseMapEntry {
                verse: v(Book::Psalms, 51, 1),
                last: 1,
                standard: Some(Passage::verse(v(Book::Psalms, 51, 0))),
            },
            VerseMapEntry {
                verse: v(Book::Psalms, 51, 2),
                last: 2,
                standard: Some(Passage::verse(v(Book::Psalms, 51, 0))),
            },
            VerseMapEntry {
                verse: v(Book::Psalms, 51, 3),
                last: 3,
                standard: Some(Passage::verse(v(Book::Psalms, 51, 1))),
            },
            // A verse range covering two Standard Verses.
            VerseMapEntry {
                verse: v(Book::John, 1, 1),
                last: 2,
                standard: Some(p(v(Book::John, 1, 1), v(Book::John, 1, 2))),
            },
            VerseMapEntry {
                verse: v(Book::John, 1, 3),
                last: 3,
                standard: None,
            },
        ];
        let map = VerseMap::new(entries, vec![v(Book::Matthew, 17, 21)], &Standard);
        assert_eq!(
            map.to_native(v(Book::Psalms, 51, 0)),
            [v(Book::Psalms, 51, 1), v(Book::Psalms, 51, 2)]
        );
        assert_eq!(
            map.first_native(v(Book::Psalms, 51, 1)),
            Some(v(Book::Psalms, 51, 3))
        );
        assert_eq!(
            map.to_standard(v(Book::John, 1, 2)),
            Some(p(v(Book::John, 1, 1), v(Book::John, 1, 2)))
        );
        assert_eq!(map.to_native(v(Book::John, 1, 2)), [v(Book::John, 1, 1)]);
        assert_eq!(map.unmapped().count(), 1);
        assert!(map.is_absent(v(Book::Matthew, 17, 21)));
        assert!(!map.is_identity());

        let json = serde_json::to_string(&map).unwrap();
        let mut back: VerseMap = serde_json::from_str(&json).unwrap();
        back.reindex(&Standard);
        assert_eq!(back, map);
    }

    #[test]
    fn listed_versification() {
        let lv = ListedVersification::new([
            (v(Book::Psalms, 3, 0), 0),
            (v(Book::Psalms, 3, 1), 1),
            (v(Book::Psalms, 3, 2), 3),
            (v(Book::Genesis, 32, 33), 33),
        ]);
        assert!(lv.has_title(Book::Psalms, 3));
        assert_eq!(lv.last_verse(Book::Psalms, 3), Some(3));
        assert_eq!(lv.chapter_count(Book::Genesis), 32);
        assert!(lv.contains(v(Book::Genesis, 32, 33)));
        assert!(!lv.contains(v(Book::Genesis, 32, 34)));
        assert_eq!(lv.chapter_count(Book::Exodus), 0);
    }
}
