//! Bible references: the written form of one or more Passages ("Jn 3:16-18; Rom 5:8").
//!
//! [`parse_reference`] reads typed text into a [`PassageList`] in a given Versification (a
//! Bible's own numbering in that Bible's Tab, the Standard versification everywhere else, per
//! ADR 0002). [`format_passage`] and [`format_passage_list`] write canonical text back, and
//! anything they write parses back to the same Passages.
//!
//! Grammar, informally:
//!
//! - A list is items separated by `;` or `,`. Empty items are ignored.
//! - An item is an optional Bible book, a location, and an optional `-` (or en/em dash) range end.
//! - Locations: `3` (chapter), `3:16` or `3.16` (verse), `51:0` or `51:title` (Psalm title slot).
//!   A lettered verse (`16a`) belongs to its numbered verse.
//! - In a single-chapter book (Obadiah, Philemon, 2–3 John, Jude) a bare number is a verse.
//! - Without a book, an item continues the previous one: after `,` a bare number is a verse if
//!   the previous item ended on a verse; after `;` it is a chapter. A first item without a book
//!   uses the context Verse: `2:4` is in the same book, `5` in the same chapter.
//! - A range end may be a verse (`-18`), chapter and verse (`-4:2`), or start with a book
//!   (`-Matt 1:1`, `-Deut`).

use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::ops::Range;
use std::sync::OnceLock;

use crate::book::Book;
use crate::verse::{Passage, PassageList, Verse};
use crate::versification::Versification;

/// Which book names [`format_passage`] writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RefStyle {
    /// Abbreviations: `1 Jn 4:8`.
    #[default]
    Short,
    /// Full names: `1 John 4:8`.
    Long,
}

/// What went wrong while reading a Bible reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReferenceErrorKind {
    /// Nothing but whitespace and separators.
    Empty,
    UnexpectedChar(char),
    /// Something other than `;` or `,` after a complete reference.
    UnexpectedToken,
    NumberTooLarge,
    UnknownBook(String),
    AmbiguousBook {
        text: String,
        candidates: Vec<Book>,
    },
    /// A number with no book before it and no context Verse.
    MissingBook,
    MissingNumber,
    BookNotInVersification(Book),
    ChapterOutOfRange {
        book: Book,
        chapter: u16,
        last: u16,
    },
    VerseOutOfRange {
        book: Book,
        chapter: u16,
        verse: u16,
        last: u16,
    },
    NoTitle {
        book: Book,
        chapter: u16,
    },
    /// The range ends before it starts.
    Reversed,
}

/// A Bible reference that could not be read, with the byte range of the input to underline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceError {
    pub kind: ReferenceErrorKind,
    pub span: Range<usize>,
}

impl ReferenceError {
    fn new(kind: ReferenceErrorKind, span: Range<usize>) -> Self {
        Self { kind, span }
    }
}

impl fmt::Display for ReferenceErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ReferenceErrorKind::*;
        match self {
            Empty => f.write_str("no reference given"),
            UnexpectedChar(c) => write!(f, "unexpected `{c}`"),
            UnexpectedToken => f.write_str("expected `;` or `,` between references"),
            NumberTooLarge => f.write_str("number is too large"),
            UnknownBook(t) => write!(f, "unknown book `{t}`"),
            AmbiguousBook { text, candidates } => {
                let names: Vec<&str> = candidates.iter().map(|b| b.name()).collect();
                write!(f, "`{text}` could be {}", names.join(" or "))
            }
            MissingBook => f.write_str("no book given"),
            MissingNumber => f.write_str("expected a chapter or verse number"),
            BookNotInVersification(b) => write!(f, "{b} is not in this Bible"),
            ChapterOutOfRange {
                book,
                chapter,
                last,
            } => write!(
                f,
                "{book} {chapter} does not exist ({book} has {last} chapters)"
            ),
            VerseOutOfRange {
                book,
                chapter,
                verse,
                last,
            } => write!(
                f,
                "{book} {chapter}:{verse} does not exist ({book} {chapter} has {last} verses)"
            ),
            NoTitle { book, chapter } => write!(f, "{book} {chapter} has no title"),
            Reversed => f.write_str("the range ends before it starts"),
        }
    }
}

impl fmt::Display for ReferenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} (at {}..{})",
            self.kind, self.span.start, self.span.end
        )
    }
}

impl std::error::Error for ReferenceError {}

/// Read a Bible reference in `v11n`. `context` is the Verse that relative forms (`2:4`, `5`)
/// are read against, typically the Tab's Current location.
pub fn parse_reference<V: Versification + ?Sized>(
    input: &str,
    v11n: &V,
    context: Option<Verse>,
) -> Result<PassageList, ReferenceError> {
    let tokens = tokenize(input)?;
    let mut parser = Parser {
        tokens,
        pos: 0,
        v11n,
        input,
    };
    parser.parse_list(context)
}

/// Canonical text for one Passage: whole books and chapters collapse (`Gen`, `Gen 1-3`), title
/// slots read `title`, and the result always parses back to the same Passage.
pub fn format_passage<V: Versification + ?Sized>(
    passage: Passage,
    v11n: &V,
    style: RefStyle,
) -> String {
    let (s, e) = (passage.start(), passage.end());
    let name = |b: Book| match style {
        RefStyle::Short => b.abbrev(),
        RefStyle::Long => b.name(),
    };
    let label = |v: Verse| {
        if v.is_title() {
            "title".to_string()
        } else {
            v.verse.to_string()
        }
    };
    let single = |b: Book| v11n.chapter_count(b) == 1;

    let whole_books =
        v11n.first_verse_of_book(s.book) == Some(s) && v11n.last_verse_of_book(e.book) == Some(e);
    if whole_books {
        return if s.book == e.book {
            name(s.book).to_string()
        } else {
            format!("{}-{}", name(s.book), name(e.book))
        };
    }

    let whole_chapters = v11n.first_verse(s.book, s.chapter) == Some(s)
        && v11n.last_verse_of(e.book, e.chapter) == Some(e);
    if whole_chapters {
        let chapter = |b: Book, c: u16| {
            if single(b) {
                name(b).to_string()
            } else {
                format!("{} {c}", name(b))
            }
        };
        return if s.book != e.book {
            format!(
                "{}-{}",
                chapter(s.book, s.chapter),
                chapter(e.book, e.chapter)
            )
        } else if s.chapter == e.chapter {
            chapter(s.book, s.chapter)
        } else {
            format!("{} {}-{}", name(s.book), s.chapter, e.chapter)
        };
    }

    let start = format!("{} {}:{}", name(s.book), s.chapter, label(s));
    if s == e {
        start
    } else if s.book != e.book {
        format!("{start}-{} {}:{}", name(e.book), e.chapter, label(e))
    } else if s.chapter != e.chapter {
        format!("{start}-{}:{}", e.chapter, label(e))
    } else {
        format!("{start}-{}", label(e))
    }
}

/// Canonical text for a Passage list: each Passage as [`format_passage`], joined by `; `.
pub fn format_passage_list<V: Versification + ?Sized>(
    list: &PassageList,
    v11n: &V,
    style: RefStyle,
) -> String {
    list.iter()
        .map(|p| format_passage(*p, v11n, style))
        .collect::<Vec<_>>()
        .join("; ")
}

// ---------------------------------------------------------------- book names

struct BookKeys {
    exact: HashMap<String, Book>,
    /// Every exact key, for unique-prefix matching.
    all: Vec<(String, Book)>,
}

/// Extra spellings beyond USFM codes, OSIS ids, full names and our abbreviations.
const ALIASES: &[(&str, Book)] = {
    use Book::*;
    &[
        ("gn", Genesis),
        ("ge", Genesis),
        ("ex", Exodus),
        ("lv", Leviticus),
        ("le", Leviticus),
        ("nm", Numbers),
        ("nu", Numbers),
        ("dt", Deuteronomy),
        ("jsh", Joshua),
        ("jg", Judges),
        ("jdgs", Judges),
        ("rt", Ruth),
        ("ru", Ruth),
        ("1sm", FirstSamuel),
        ("2sm", SecondSamuel),
        ("1kg", FirstKings),
        ("2kg", SecondKings),
        ("jb", Job),
        ("psalm", Psalms),
        ("pss", Psalms),
        ("psm", Psalms),
        ("pr", Proverbs),
        ("prv", Proverbs),
        ("ec", Ecclesiastes),
        ("qoh", Ecclesiastes),
        ("qoheleth", Ecclesiastes),
        ("songofsolomon", SongOfSongs),
        ("sos", SongOfSongs),
        ("canticles", SongOfSongs),
        ("is", Isaiah),
        ("jr", Jeremiah),
        ("lm", Lamentations),
        ("eze", Ezekiel),
        ("dn", Daniel),
        ("da", Daniel),
        ("hs", Hosea),
        ("jl", Joel),
        ("am", Amos),
        ("ob", Obadiah),
        ("jnh", Jonah),
        ("mc", Micah),
        ("na", Nahum),
        ("zp", Zephaniah),
        ("hg", Haggai),
        ("zc", Zechariah),
        ("ml", Malachi),
        ("mt", Matthew),
        ("mk", Mark),
        ("mr", Mark),
        ("lk", Luke),
        ("jn", John),
        ("ac", Acts),
        ("rm", Romans),
        ("ro", Romans),
        ("ga", Galatians),
        ("ep", Ephesians),
        ("tt", Titus),
        ("jm", James),
        ("1pt", FirstPeter),
        ("2pt", SecondPeter),
        ("jd", Jude),
        ("re", Revelation),
        ("rv", Revelation),
        ("revelations", Revelation),
    ]
};

/// Lowercase and keep only letters and digits.
fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Build the key table, returning keys claimed by two different books (should be none).
fn build_keys() -> (BookKeys, Vec<String>) {
    let mut exact = HashMap::new();
    let mut conflicts = Vec::new();
    let mut add = |key: String, book: Book| match exact.get(&key) {
        Some(&other) if other != book => conflicts.push(key),
        Some(_) => {}
        None => {
            exact.insert(key, book);
        }
    };
    for book in Book::ALL {
        add(norm(book.usfm()), book);
        add(norm(book.osis()), book);
        add(norm(book.name()), book);
        add(norm(book.abbrev()), book);
    }
    for &(key, book) in ALIASES {
        add(key.to_string(), book);
    }
    let mut all: Vec<(String, Book)> = exact.iter().map(|(k, b)| (k.clone(), *b)).collect();
    all.sort();
    (BookKeys { exact, all }, conflicts)
}

fn book_keys() -> &'static BookKeys {
    static KEYS: OnceLock<BookKeys> = OnceLock::new();
    KEYS.get_or_init(|| build_keys().0)
}

enum Lookup {
    Found(Book),
    Ambiguous(Vec<Book>),
    Missing,
}

/// Exact key first, then a unique prefix of any known key (at least two characters).
fn lookup(key: &str) -> Lookup {
    let keys = book_keys();
    if let Some(&book) = keys.exact.get(key) {
        return Lookup::Found(book);
    }
    if key.chars().count() < 2 {
        return Lookup::Missing;
    }
    let found: BTreeSet<Book> = keys
        .all
        .iter()
        .filter(|(k, _)| k.starts_with(key))
        .map(|(_, b)| *b)
        .collect();
    let mut found = found.into_iter();
    match (found.next(), found.next()) {
        (None, _) => Lookup::Missing,
        (Some(b), None) => Lookup::Found(b),
        (Some(a), Some(b)) => Lookup::Ambiguous([a, b].into_iter().chain(found).collect()),
    }
}

/// `I`, `II`, `III`, `First`… as the digit of a numbered book.
fn ordinal_word(word: &str) -> Option<char> {
    match word {
        "i" | "first" => Some('1'),
        "ii" | "second" => Some('2'),
        "iii" | "third" => Some('3'),
        _ => None,
    }
}

// ---------------------------------------------------------------- tokens

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Num(u16),
    /// Lowercased letters.
    Word(String),
    Colon,
    Dash,
    Semi,
    Comma,
}

#[derive(Debug, Clone)]
struct Token {
    tok: Tok,
    span: Range<usize>,
}

fn tokenize(input: &str) -> Result<Vec<Token>, ReferenceError> {
    let mut tokens: Vec<Token> = Vec::new();
    let mut chars = input.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let (tok, end) = match c {
            c if c.is_whitespace() => continue,
            c if c.is_ascii_digit() => {
                let mut value = c.to_digit(10).unwrap_or(0);
                let mut end = i + 1;
                while let Some(&(j, d)) = chars.peek() {
                    let Some(digit) = d.to_digit(10) else { break };
                    value = value.saturating_mul(10).saturating_add(digit);
                    end = j + 1;
                    chars.next();
                }
                let n = u16::try_from(value)
                    .map_err(|_| ReferenceError::new(ReferenceErrorKind::NumberTooLarge, i..end))?;
                (Tok::Num(n), end)
            }
            c if c.is_alphabetic() => {
                let mut word: String = c.to_lowercase().collect();
                let mut end = i + c.len_utf8();
                while let Some(&(j, d)) = chars.peek() {
                    if !d.is_alphabetic() {
                        break;
                    }
                    word.extend(d.to_lowercase());
                    end = j + d.len_utf8();
                    chars.next();
                }
                (Tok::Word(word), end)
            }
            '.' => {
                // `3.16` is chapter.verse; any other period (`Gen.`) is ignored.
                let after_number = matches!(
                    tokens.last(),
                    Some(Token { tok: Tok::Num(_), span }) if span.end == i
                );
                let before_digit = input
                    .get(i + 1..)
                    .is_some_and(|rest| rest.starts_with(|d: char| d.is_ascii_digit()));
                if after_number && before_digit {
                    (Tok::Colon, i + 1)
                } else {
                    continue;
                }
            }
            ':' => (Tok::Colon, i + 1),
            ';' => (Tok::Semi, i + 1),
            ',' => (Tok::Comma, i + 1),
            '-' | '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' => {
                (Tok::Dash, i + c.len_utf8())
            }
            other => {
                return Err(ReferenceError::new(
                    ReferenceErrorKind::UnexpectedChar(other),
                    i..i + other.len_utf8(),
                ));
            }
        };
        tokens.push(Token { tok, span: i..end });
    }
    Ok(tokens)
}

// ---------------------------------------------------------------- parser

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sep {
    Semi,
    Comma,
}

/// A parsed location before it is checked against the Versification. Spans point at the
/// text to underline if the check fails.
#[derive(Debug, Clone)]
enum Loc {
    Book(Book, Range<usize>),
    Chapter(Book, u16, Range<usize>),
    Verse(Book, u16, Range<usize>, u16, Range<usize>),
}

impl Loc {
    fn book(&self) -> Book {
        match self {
            Loc::Book(b, _) | Loc::Chapter(b, ..) | Loc::Verse(b, ..) => *b,
        }
    }

    fn chapter(&self) -> Option<u16> {
        match self {
            Loc::Book(..) => None,
            Loc::Chapter(_, c, _) | Loc::Verse(_, c, ..) => Some(*c),
        }
    }
}

/// What the previous item leaves for the next one to continue from.
#[derive(Debug, Default)]
struct State {
    book: Option<Book>,
    chapter: Option<u16>,
    ended_on_verse: bool,
}

struct Parser<'a, V: Versification + ?Sized> {
    tokens: Vec<Token>,
    pos: usize,
    v11n: &'a V,
    input: &'a str,
}

impl<V: Versification + ?Sized> Parser<'_, V> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn tok_at(&self, index: usize) -> Option<&Tok> {
        self.tokens.get(index).map(|t| &t.tok)
    }

    fn next_span(&self) -> Range<usize> {
        self.peek()
            .map_or(self.input.len()..self.input.len(), |t| t.span.clone())
    }

    fn eat(&mut self, tok: &Tok) -> bool {
        if self.tok_at(self.pos) == Some(tok) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn single_chapter(&self, book: Book) -> bool {
        self.v11n.chapter_count(book) == 1
    }

    fn parse_list(&mut self, context: Option<Verse>) -> Result<PassageList, ReferenceError> {
        let mut list = PassageList::new();
        let mut state = State::default();
        let mut sep = None;
        loop {
            while let Some(tok) = self.tok_at(self.pos) {
                match tok {
                    Tok::Semi => sep = Some(Sep::Semi),
                    Tok::Comma if sep.is_none() => sep = Some(Sep::Comma),
                    Tok::Comma => {}
                    _ => break,
                }
                self.pos += 1;
            }
            let Some(token) = self.peek() else { break };
            if !list.is_empty() && sep.is_none() {
                return Err(ReferenceError::new(
                    ReferenceErrorKind::UnexpectedToken,
                    token.span.clone(),
                ));
            }
            list.push(self.parse_item(&mut state, sep, context)?);
            sep = None;
        }
        if list.is_empty() {
            return Err(ReferenceError::new(
                ReferenceErrorKind::Empty,
                0..self.input.len(),
            ));
        }
        Ok(list)
    }

    fn parse_item(
        &mut self,
        state: &mut State,
        sep: Option<Sep>,
        context: Option<Verse>,
    ) -> Result<Passage, ReferenceError> {
        let item_start = self.next_span().start;
        let start_loc = match self.try_book()? {
            Some((book, span)) => self.location_after_book(book, span)?,
            None => {
                let (book, chapter, first_relative) = match (state.book, context) {
                    (Some(b), _) => (b, state.chapter, false),
                    (None, Some(ctx)) => (ctx.book, Some(ctx.chapter), true),
                    (None, None) => {
                        return Err(ReferenceError::new(
                            ReferenceErrorKind::MissingBook,
                            self.next_span(),
                        ));
                    }
                };
                let (n, n_span) = self.number()?;
                if self.eat(&Tok::Colon) {
                    let (v, v_span) = self.verse_number()?;
                    Loc::Verse(book, n, n_span, v, v_span)
                } else {
                    let single = self.single_chapter(book);
                    let as_verse = single
                        || first_relative
                        || (sep == Some(Sep::Comma) && state.ended_on_verse);
                    match chapter {
                        _ if single => {
                            self.skip_subverse(&n_span);
                            Loc::Verse(book, 1, n_span.clone(), n, n_span)
                        }
                        Some(c) if as_verse => {
                            self.skip_subverse(&n_span);
                            Loc::Verse(book, c, n_span.clone(), n, n_span)
                        }
                        _ => Loc::Chapter(book, n, n_span),
                    }
                }
            }
        };

        let end_loc = if self.eat(&Tok::Dash) {
            match self.try_book()? {
                Some((book, span)) => self.location_after_book(book, span)?,
                None => {
                    let book = start_loc.book();
                    let (n, n_span) = self.number()?;
                    if self.eat(&Tok::Colon) {
                        let (v, v_span) = self.verse_number()?;
                        Loc::Verse(book, n, n_span, v, v_span)
                    } else {
                        match &start_loc {
                            Loc::Verse(_, c, c_span, ..) => {
                                self.skip_subverse(&n_span);
                                Loc::Verse(book, *c, c_span.clone(), n, n_span)
                            }
                            _ if self.single_chapter(book) => {
                                self.skip_subverse(&n_span);
                                Loc::Verse(book, 1, n_span.clone(), n, n_span)
                            }
                            _ => Loc::Chapter(book, n, n_span),
                        }
                    }
                }
            }
        } else {
            start_loc.clone()
        };

        let start = self.resolve(&start_loc, false)?;
        let end = self.resolve(&end_loc, true)?;
        let item_end = self
            .pos
            .checked_sub(1)
            .and_then(|i| self.tokens.get(i))
            .map_or(item_start, |t| t.span.end);
        let passage = Passage::new(start, end)
            .map_err(|_| ReferenceError::new(ReferenceErrorKind::Reversed, item_start..item_end))?;

        state.book = Some(end_loc.book());
        state.chapter = end_loc.chapter();
        state.ended_on_verse = matches!(end_loc, Loc::Verse(..));
        Ok(passage)
    }

    /// A book name at the cursor, if there is one: an optional ordinal (`1`, `I`, `First`,
    /// `1st`) and up to four words, longest match first.
    fn try_book(&mut self) -> Result<Option<(Book, Range<usize>)>, ReferenceError> {
        let start = self.pos;
        let mut i = self.pos;
        let mut ordinal = None;
        match (self.tok_at(i), self.tok_at(i + 1)) {
            (Some(Tok::Num(n @ 1..=3)), Some(Tok::Word(_))) => {
                ordinal = char::from_digit(u32::from(*n), 10);
                i += 1;
                if matches!(self.tok_at(i), Some(Tok::Word(w)) if matches!(w.as_str(), "st" | "nd" | "rd"))
                    && matches!(self.tok_at(i + 1), Some(Tok::Word(_)))
                {
                    i += 1;
                }
            }
            (Some(Tok::Word(w)), Some(Tok::Word(_))) if ordinal_word(w).is_some() => {
                ordinal = ordinal_word(w);
                i += 1;
            }
            (Some(Tok::Word(_)), _) => {}
            _ => return Ok(None),
        }
        let mut words: Vec<(&str, Range<usize>)> = Vec::new();
        while let Some(Token {
            tok: Tok::Word(w),
            span,
        }) = self.tokens.get(i + words.len())
        {
            if words.len() == 4 {
                break;
            }
            words.push((w.as_str(), span.clone()));
        }
        let first_span = self.tokens.get(start).map_or(0..0, |t| t.span.clone());
        let prefix: String = ordinal.map(String::from).unwrap_or_default();
        let mut ambiguous = None;
        for n in (1..=words.len()).rev() {
            let key: String = words[..n]
                .iter()
                .fold(prefix.clone(), |acc, (w, _)| acc + w);
            match lookup(&key) {
                Lookup::Found(book) => {
                    let end = words[n - 1].1.end;
                    self.pos = i + n;
                    return Ok(Some((book, first_span.start..end)));
                }
                Lookup::Ambiguous(candidates) if n == 1 => ambiguous = Some(candidates),
                _ => {}
            }
        }
        let end = words.first().map_or(first_span.end, |(_, s)| s.end);
        let span = first_span.start..end;
        let text = self.input.get(span.clone()).unwrap_or_default().to_string();
        let kind = match ambiguous {
            Some(candidates) => ReferenceErrorKind::AmbiguousBook { text, candidates },
            None => ReferenceErrorKind::UnknownBook(text),
        };
        Err(ReferenceError::new(kind, span))
    }

    fn location_after_book(
        &mut self,
        book: Book,
        book_span: Range<usize>,
    ) -> Result<Loc, ReferenceError> {
        if !matches!(self.tok_at(self.pos), Some(Tok::Num(_))) {
            return Ok(Loc::Book(book, book_span));
        }
        let (n, n_span) = self.number()?;
        if self.eat(&Tok::Colon) {
            let (v, v_span) = self.verse_number()?;
            Ok(Loc::Verse(book, n, n_span, v, v_span))
        } else if self.single_chapter(book) {
            self.skip_subverse(&n_span);
            Ok(Loc::Verse(book, 1, n_span.clone(), n, n_span))
        } else {
            Ok(Loc::Chapter(book, n, n_span))
        }
    }

    fn number(&mut self) -> Result<(u16, Range<usize>), ReferenceError> {
        match self.peek() {
            Some(Token {
                tok: Tok::Num(n),
                span,
            }) => {
                let out = (*n, span.clone());
                self.pos += 1;
                Ok(out)
            }
            _ => Err(ReferenceError::new(
                ReferenceErrorKind::MissingNumber,
                self.next_span(),
            )),
        }
    }

    /// A verse number after `:`, or `title` for verse 0.
    fn verse_number(&mut self) -> Result<(u16, Range<usize>), ReferenceError> {
        if let Some(Token {
            tok: Tok::Word(w),
            span,
        }) = self.peek()
            && w == "title"
        {
            let span = span.clone();
            self.pos += 1;
            return Ok((0, span));
        }
        let (n, span) = self.number()?;
        self.skip_subverse(&span);
        Ok((n, span))
    }

    /// Skip a verse letter written straight after its number (`16a`).
    fn skip_subverse(&mut self, number_span: &Range<usize>) {
        if let Some(Token {
            tok: Tok::Word(w),
            span,
        }) = self.peek()
            && span.start == number_span.end
            && matches!(w.as_str(), "a" | "b" | "c" | "d" | "e" | "f")
        {
            self.pos += 1;
        }
    }

    fn check_chapter(
        &self,
        book: Book,
        chapter: u16,
        span: &Range<usize>,
    ) -> Result<u16, ReferenceError> {
        let count = self.v11n.chapter_count(book);
        if count == 0 {
            return Err(ReferenceError::new(
                ReferenceErrorKind::BookNotInVersification(book),
                span.clone(),
            ));
        }
        self.v11n.last_verse(book, chapter).ok_or_else(|| {
            ReferenceError::new(
                ReferenceErrorKind::ChapterOutOfRange {
                    book,
                    chapter,
                    last: count,
                },
                span.clone(),
            )
        })
    }

    fn resolve(&self, loc: &Loc, at_end: bool) -> Result<Verse, ReferenceError> {
        let missing = |book: Book, span: &Range<usize>| {
            ReferenceError::new(
                ReferenceErrorKind::BookNotInVersification(book),
                span.clone(),
            )
        };
        match loc {
            Loc::Book(book, span) => if at_end {
                self.v11n.last_verse_of_book(*book)
            } else {
                self.v11n.first_verse_of_book(*book)
            }
            .ok_or_else(|| missing(*book, span)),
            Loc::Chapter(book, chapter, span) => {
                self.check_chapter(*book, *chapter, span)?;
                if at_end {
                    self.v11n.last_verse_of(*book, *chapter)
                } else {
                    self.v11n.first_verse(*book, *chapter)
                }
                .ok_or_else(|| missing(*book, span))
            }
            Loc::Verse(book, chapter, c_span, verse, v_span) => {
                let last = self.check_chapter(*book, *chapter, c_span)?;
                let (book, chapter, verse) = (*book, *chapter, *verse);
                if verse == 0 && !self.v11n.has_title(book, chapter) {
                    return Err(ReferenceError::new(
                        ReferenceErrorKind::NoTitle { book, chapter },
                        v_span.clone(),
                    ));
                }
                if verse > last {
                    return Err(ReferenceError::new(
                        ReferenceErrorKind::VerseOutOfRange {
                            book,
                            chapter,
                            verse,
                            last,
                        },
                        v_span.clone(),
                    ));
                }
                Ok(Verse::new(book, chapter, verse))
            }
        }
    }
}

#[cfg(test)]
#[path = "reference_tests.rs"]
mod tests;
