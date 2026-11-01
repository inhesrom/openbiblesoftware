//! PROTOTYPE: Bible references (Standard versification ids, parsing, formatting).

/// (book index 0..66, chapter, verse). Ordered canonically.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VRef {
    pub book: u8,
    pub ch: u16,
    pub v: u16,
}

pub const OSIS: [&str; 66] = [
    "Gen", "Exod", "Lev", "Num", "Deut", "Josh", "Judg", "Ruth", "1Sam", "2Sam", "1Kgs", "2Kgs", "1Chr",
    "2Chr", "Ezra", "Neh", "Esth", "Job", "Ps", "Prov", "Eccl", "Song", "Isa", "Jer", "Lam", "Ezek", "Dan",
    "Hos", "Joel", "Amos", "Obad", "Jonah", "Mic", "Nah", "Hab", "Zeph", "Hag", "Zech", "Mal", "Matt",
    "Mark", "Luke", "John", "Acts", "Rom", "1Cor", "2Cor", "Gal", "Eph", "Phil", "Col", "1Thess", "2Thess",
    "1Tim", "2Tim", "Titus", "Phlm", "Heb", "Jas", "1Pet", "2Pet", "1John", "2John", "3John", "Jude", "Rev",
];

pub const NAMES: [&str; 66] = [
    "Genesis", "Exodus", "Leviticus", "Numbers", "Deuteronomy", "Joshua", "Judges", "Ruth", "1 Samuel",
    "2 Samuel", "1 Kings", "2 Kings", "1 Chronicles", "2 Chronicles", "Ezra", "Nehemiah", "Esther", "Job",
    "Psalms", "Proverbs", "Ecclesiastes", "Song of Songs", "Isaiah", "Jeremiah", "Lamentations", "Ezekiel",
    "Daniel", "Hosea", "Joel", "Amos", "Obadiah", "Jonah", "Micah", "Nahum", "Habakkuk", "Zephaniah",
    "Haggai", "Zechariah", "Malachi", "Matthew", "Mark", "Luke", "John", "Acts", "Romans", "1 Corinthians",
    "2 Corinthians", "Galatians", "Ephesians", "Philippians", "Colossians", "1 Thessalonians",
    "2 Thessalonians", "1 Timothy", "2 Timothy", "Titus", "Philemon", "Hebrews", "James", "1 Peter",
    "2 Peter", "1 John", "2 John", "3 John", "Jude", "Revelation",
];

pub const SHORT: [&str; 66] = [
    "Ge", "Ex", "Le", "Nu", "Dt", "Jos", "Jdg", "Ru", "1Sa", "2Sa", "1Ki", "2Ki", "1Ch", "2Ch", "Ezr", "Ne",
    "Es", "Job", "Ps", "Pr", "Ec", "So", "Is", "Je", "La", "Eze", "Da", "Ho", "Joe", "Am", "Ob", "Jon", "Mic",
    "Na", "Hab", "Zep", "Hag", "Zec", "Mal", "Mt", "Mk", "Lk", "Jn", "Ac", "Ro", "1Co", "2Co", "Ga", "Eph",
    "Php", "Col", "1Th", "2Th", "1Ti", "2Ti", "Tt", "Phm", "Heb", "Jas", "1Pe", "2Pe", "1Jn", "2Jn", "3Jn",
    "Jud", "Re",
];

impl VRef {
    pub fn new(book: u8, ch: u16, v: u16) -> Self {
        Self { book, ch, v }
    }

    /// "Gen.1.1"
    pub fn from_osis(s: &str) -> Option<Self> {
        let mut it = s.split('.');
        let b = it.next()?;
        let book = OSIS.iter().position(|x| *x == b)? as u8;
        let ch = it.next()?.parse().ok()?;
        let v = it.next().map(|x| x.parse().ok()).unwrap_or(Some(1))?;
        Some(Self { book, ch, v })
    }

    pub fn display(&self) -> String {
        format!("{} {}:{}", NAMES[self.book as usize], self.ch, self.v)
    }

    pub fn short(&self) -> String {
        format!("{} {}:{}", SHORT[self.book as usize], self.ch, self.v)
    }
}

/// "Gen.1.1-Gen.1.3" -> "Ge 1:1–3"
pub fn osis_range_short(s: &str) -> String {
    let mut parts = s.split('-');
    let a = parts.next().and_then(VRef::from_osis);
    let b = parts.next().and_then(VRef::from_osis);
    match (a, b) {
        (Some(a), Some(b)) if a.book == b.book && a.ch == b.ch => format!("{}–{}", a.short(), b.v),
        (Some(a), Some(b)) if a.book == b.book => format!("{}–{}:{}", a.short(), b.ch, b.v),
        (Some(a), Some(b)) => format!("{}–{}", a.short(), b.short()),
        (Some(a), None) => a.short(),
        _ => s.to_string(),
    }
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace() && *c != '.').flat_map(|c| c.to_lowercase()).collect()
}

fn book_from_str(s: &str) -> Option<u8> {
    let n = norm(s);
    if n.is_empty() {
        return None;
    }
    // exact on any list, then unique prefix on full names
    for (i, ((o, f), sh)) in OSIS.iter().zip(NAMES.iter()).zip(SHORT.iter()).enumerate() {
        if norm(o) == n || norm(f) == n || norm(sh) == n {
            return Some(i as u8);
        }
    }
    let extra: &[(&str, u8)] = &[("gn", 0), ("jn", 42), ("jhn", 42), ("mt", 39), ("mk", 40), ("mrk", 40), ("lk", 41), ("psalm", 18), ("pss", 18)];
    if let Some((_, b)) = extra.iter().find(|(k, _)| *k == n) {
        return Some(*b);
    }
    NAMES.iter().position(|f| norm(f).starts_with(&n)).map(|i| i as u8)
}

#[derive(Debug, PartialEq)]
pub enum Parsed {
    Verse(VRef),
    /// Only chapter (and book) were given.
    Chapter(VRef),
}

/// Parse what a user typed in a reference box. `ctx` supplies the book when only "3:16" is typed.
pub fn parse(input: &str, ctx: Option<VRef>) -> Option<Parsed> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }
    // split into book part and numeric part: the numeric tail is digits / ':' / '.' / spaces
    let bytes: Vec<char> = s.chars().collect();
    let mut split = bytes.len();
    while split > 0 {
        let c = bytes[split - 1];
        if c.is_ascii_digit() || c == ':' || c == '.' || c == ' ' || c == ',' {
            split -= 1;
        } else {
            break;
        }
    }
    // A leading digit belongs to the book name ("1 John 3"), so make sure book part isn't just a digit
    let (book_part, num_part): (String, String) = (bytes[..split].iter().collect(), bytes[split..].iter().collect());
    let (book, num) = if book_part.trim().is_empty() {
        (ctx.map(|c| c.book)?, num_part)
    } else {
        // "1 John" with no chapter: book_part="1 John"? (split only consumes trailing digits) ok
        (book_from_str(&book_part)?, num_part)
    };
    let nums: Vec<u16> = num
        .split(|c: char| c == ':' || c == '.' || c == ' ' || c == ',')
        .filter(|x| !x.is_empty())
        .filter_map(|x| x.parse().ok())
        .collect();
    match nums.as_slice() {
        [] => Some(Parsed::Chapter(VRef::new(book, 1, 1))),
        [c] if book_part.trim().is_empty() => ctx.map(|x| Parsed::Verse(VRef::new(book, x.ch, *c))),
        [c] => Some(Parsed::Chapter(VRef::new(book, *c, 1))),
        [c, v, ..] => Some(Parsed::Verse(VRef::new(book, *c, *v))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses() {
        assert_eq!(parse("Gen 1:3", None), Some(Parsed::Verse(VRef::new(0, 1, 3))));
        assert_eq!(parse("gn 3.15", None), Some(Parsed::Verse(VRef::new(0, 3, 15))));
        assert_eq!(parse("Genesis 2", None), Some(Parsed::Chapter(VRef::new(0, 2, 1))));
        assert_eq!(parse("1 John 3:16", None), Some(Parsed::Verse(VRef::new(61, 3, 16))));
        assert_eq!(parse("jn 3:16", None), Some(Parsed::Verse(VRef::new(42, 3, 16))));
        assert_eq!(parse("2:4", Some(VRef::new(0, 1, 1))), Some(Parsed::Verse(VRef::new(0, 2, 4))));
        assert_eq!(parse("5", Some(VRef::new(0, 1, 1))), Some(Parsed::Verse(VRef::new(0, 1, 5))));
    }
}
