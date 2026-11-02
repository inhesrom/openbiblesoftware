//! Bible books of the 66-book Protestant canon.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Which half of the canon a Bible book belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Testament {
    Old,
    New,
}

macro_rules! books {
    ($( $variant:ident = $usfm:literal, $osis:literal, $name:literal, $abbrev:literal; )*) => {
        /// One book of the biblical canon, in canonical order.
        ///
        /// Serialized as its USFM code (`"GEN"`), which is also the text-unit id of a Bible book.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(u8)]
        pub enum Book {
            $( $variant, )*
        }

        impl Book {
            /// All 66 books in canonical order.
            pub const ALL: [Book; 66] = [ $( Book::$variant, )* ];

            /// USFM / Paratext book code, e.g. `GEN`, `1JN`.
            pub const fn usfm(self) -> &'static str {
                match self { $( Book::$variant => $usfm, )* }
            }

            /// OSIS book id, e.g. `Gen`, `1John`.
            pub const fn osis(self) -> &'static str {
                match self { $( Book::$variant => $osis, )* }
            }

            /// Full English name, e.g. `1 John`.
            pub const fn name(self) -> &'static str {
                match self { $( Book::$variant => $name, )* }
            }

            /// Short display abbreviation, e.g. `1 Jn`.
            pub const fn abbrev(self) -> &'static str {
                match self { $( Book::$variant => $abbrev, )* }
            }
        }
    };
}

books! {
    Genesis = "GEN", "Gen", "Genesis", "Gen";
    Exodus = "EXO", "Exod", "Exodus", "Exod";
    Leviticus = "LEV", "Lev", "Leviticus", "Lev";
    Numbers = "NUM", "Num", "Numbers", "Num";
    Deuteronomy = "DEU", "Deut", "Deuteronomy", "Deut";
    Joshua = "JOS", "Josh", "Joshua", "Josh";
    Judges = "JDG", "Judg", "Judges", "Judg";
    Ruth = "RUT", "Ruth", "Ruth", "Ruth";
    FirstSamuel = "1SA", "1Sam", "1 Samuel", "1 Sam";
    SecondSamuel = "2SA", "2Sam", "2 Samuel", "2 Sam";
    FirstKings = "1KI", "1Kgs", "1 Kings", "1 Kgs";
    SecondKings = "2KI", "2Kgs", "2 Kings", "2 Kgs";
    FirstChronicles = "1CH", "1Chr", "1 Chronicles", "1 Chr";
    SecondChronicles = "2CH", "2Chr", "2 Chronicles", "2 Chr";
    Ezra = "EZR", "Ezra", "Ezra", "Ezra";
    Nehemiah = "NEH", "Neh", "Nehemiah", "Neh";
    Esther = "EST", "Esth", "Esther", "Esth";
    Job = "JOB", "Job", "Job", "Job";
    Psalms = "PSA", "Ps", "Psalms", "Ps";
    Proverbs = "PRO", "Prov", "Proverbs", "Prov";
    Ecclesiastes = "ECC", "Eccl", "Ecclesiastes", "Eccl";
    SongOfSongs = "SNG", "Song", "Song of Songs", "Song";
    Isaiah = "ISA", "Isa", "Isaiah", "Isa";
    Jeremiah = "JER", "Jer", "Jeremiah", "Jer";
    Lamentations = "LAM", "Lam", "Lamentations", "Lam";
    Ezekiel = "EZK", "Ezek", "Ezekiel", "Ezek";
    Daniel = "DAN", "Dan", "Daniel", "Dan";
    Hosea = "HOS", "Hos", "Hosea", "Hos";
    Joel = "JOL", "Joel", "Joel", "Joel";
    Amos = "AMO", "Amos", "Amos", "Amos";
    Obadiah = "OBA", "Obad", "Obadiah", "Obad";
    Jonah = "JON", "Jonah", "Jonah", "Jonah";
    Micah = "MIC", "Mic", "Micah", "Mic";
    Nahum = "NAM", "Nah", "Nahum", "Nah";
    Habakkuk = "HAB", "Hab", "Habakkuk", "Hab";
    Zephaniah = "ZEP", "Zeph", "Zephaniah", "Zeph";
    Haggai = "HAG", "Hag", "Haggai", "Hag";
    Zechariah = "ZEC", "Zech", "Zechariah", "Zech";
    Malachi = "MAL", "Mal", "Malachi", "Mal";
    Matthew = "MAT", "Matt", "Matthew", "Matt";
    Mark = "MRK", "Mark", "Mark", "Mark";
    Luke = "LUK", "Luke", "Luke", "Luke";
    John = "JHN", "John", "John", "John";
    Acts = "ACT", "Acts", "Acts", "Acts";
    Romans = "ROM", "Rom", "Romans", "Rom";
    FirstCorinthians = "1CO", "1Cor", "1 Corinthians", "1 Cor";
    SecondCorinthians = "2CO", "2Cor", "2 Corinthians", "2 Cor";
    Galatians = "GAL", "Gal", "Galatians", "Gal";
    Ephesians = "EPH", "Eph", "Ephesians", "Eph";
    Philippians = "PHP", "Phil", "Philippians", "Phil";
    Colossians = "COL", "Col", "Colossians", "Col";
    FirstThessalonians = "1TH", "1Thess", "1 Thessalonians", "1 Thess";
    SecondThessalonians = "2TH", "2Thess", "2 Thessalonians", "2 Thess";
    FirstTimothy = "1TI", "1Tim", "1 Timothy", "1 Tim";
    SecondTimothy = "2TI", "2Tim", "2 Timothy", "2 Tim";
    Titus = "TIT", "Titus", "Titus", "Titus";
    Philemon = "PHM", "Phlm", "Philemon", "Phlm";
    Hebrews = "HEB", "Heb", "Hebrews", "Heb";
    James = "JAS", "Jas", "James", "Jas";
    FirstPeter = "1PE", "1Pet", "1 Peter", "1 Pet";
    SecondPeter = "2PE", "2Pet", "2 Peter", "2 Pet";
    FirstJohn = "1JN", "1John", "1 John", "1 Jn";
    SecondJohn = "2JN", "2John", "2 John", "2 Jn";
    ThirdJohn = "3JN", "3John", "3 John", "3 Jn";
    Jude = "JUD", "Jude", "Jude", "Jude";
    Revelation = "REV", "Rev", "Revelation", "Rev";
}

impl Book {
    /// Zero-based canonical index (Genesis = 0, Revelation = 65).
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The book at a zero-based canonical index.
    pub fn from_index(index: usize) -> Option<Book> {
        Book::ALL.get(index).copied()
    }

    /// Look up a book by its USFM code, ignoring ASCII case.
    pub fn from_usfm(code: &str) -> Option<Book> {
        Book::ALL
            .into_iter()
            .find(|b| b.usfm().eq_ignore_ascii_case(code))
    }

    /// Look up a book by its OSIS id, ignoring ASCII case.
    pub fn from_osis(id: &str) -> Option<Book> {
        Book::ALL
            .into_iter()
            .find(|b| b.osis().eq_ignore_ascii_case(id))
    }

    pub const fn testament(self) -> Testament {
        if (self as u8) < (Book::Matthew as u8) {
            Testament::Old
        } else {
            Testament::New
        }
    }

    /// The next book in canonical order.
    pub fn next(self) -> Option<Book> {
        Book::from_index(self.index() + 1)
    }

    /// The previous book in canonical order.
    pub fn prev(self) -> Option<Book> {
        self.index().checked_sub(1).and_then(Book::from_index)
    }
}

impl fmt::Display for Book {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl Serialize for Book {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.usfm())
    }
}

impl<'de> Deserialize<'de> for Book {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let code = <std::borrow::Cow<'de, str>>::deserialize(d)?;
        Book::from_usfm(&code)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown USFM book code `{code}`")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sixty_six_books_in_order() {
        assert_eq!(Book::ALL.len(), 66);
        for (i, b) in Book::ALL.into_iter().enumerate() {
            assert_eq!(b.index(), i);
            assert_eq!(Book::from_index(i), Some(b));
            assert_eq!(Book::from_usfm(b.usfm()), Some(b));
            assert_eq!(Book::from_osis(b.osis()), Some(b));
        }
        assert_eq!(Book::from_index(66), None);
    }

    #[test]
    fn codes_are_unique() {
        let mut usfm: Vec<_> = Book::ALL.iter().map(|b| b.usfm()).collect();
        usfm.sort_unstable();
        usfm.dedup();
        assert_eq!(usfm.len(), 66);
        let mut osis: Vec<_> = Book::ALL.iter().map(|b| b.osis()).collect();
        osis.sort_unstable();
        osis.dedup();
        assert_eq!(osis.len(), 66);
    }

    #[test]
    fn testaments() {
        assert_eq!(Book::Malachi.testament(), Testament::Old);
        assert_eq!(Book::Matthew.testament(), Testament::New);
        assert_eq!(
            Book::ALL
                .iter()
                .filter(|b| b.testament() == Testament::Old)
                .count(),
            39
        );
    }

    #[test]
    fn serde_uses_usfm_code() {
        let json = serde_json::to_string(&Book::FirstJohn).unwrap();
        assert_eq!(json, "\"1JN\"");
        assert_eq!(
            serde_json::from_str::<Book>("\"1jn\"").unwrap(),
            Book::FirstJohn
        );
        assert!(serde_json::from_str::<Book>("\"XYZ\"").is_err());
    }
}
