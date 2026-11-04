//! The Verse-map builder: runs STEPBible TVTMS's conditional tests against the Verses a Bible
//! actually contains, and maps each Bible Verse to a Standard Passage (ADR 0002, #10).
//!
//! TVTMS rows are rules: "in a Bible where these tests are true, SourceRef is StandardRef".
//! A rule applies when all of its tests pass. Then, per Bible Verse:
//!
//! - every applying rule whose SourceRef covers the verse adds its StandardRef. Subverses
//!   (`!a`, `.1`) are rounded out to the whole Verses that contain them, and the Verse maps to
//!   the smallest Standard Passage covering everything it collected;
//! - a rule whose StandardRef lies outside the Standard versification (e.g. Greek additions in
//!   Daniel or Esther) makes the verse an **Unmapped verse**, unless other rules also map it;
//! - a verse no rule covers maps by identity if the Standard has it, else it is Unmapped.
//!
//! Standard Verses that no Bible Verse reaches, within the books the Bible has, are **Absent**.
//!
//! Test semantics (from the TVTMS header): `R=Exist` the verse has text; `R=NotExist` it does
//! not; `R=Last` it is the last verse of its chapter; `B.C:TextBeforeV1=Exist` the chapter has
//! a title (verse 0); `A<B`, `A>B`, `A*n<B`, `A<B+C` compare word counts, needing a 25%
//! difference. `Last` and titles count only verses with text. A subverse `R.n`
//! with `n ≥ 1` never exists (imports keep whole verses). Unknown tests make a rule not apply.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::OnceLock;

use obs_scripture::{
    Book, ListedVersification, Passage, Standard, Verse, VerseMap, VerseMapEntry, Versification,
};

/// The reformatted TVTMS rules (see `assets/tvtms/README.md`).
const TVTMS: &str = include_str!("../assets/tvtms/tvtms-canon.tsv");

/// Facts about one Bible Verse that the tests need.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerseFacts {
    /// The Bible Verse in the Bible's own numbering.
    pub verse: Verse,
    /// Last verse number it covers (`\v 1-2` → 2).
    pub last: u16,
    /// Words of text in it (0 for an empty verse).
    pub words: u32,
}

/// The built map plus what went into it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerseMapOutcome {
    pub map: VerseMap,
    /// Applying rules that moved a verse off its own number, by TVTMS SourceType (tradition).
    pub traditions: BTreeMap<String, usize>,
}

/// A reference in TVTMS notation, resolved as far as the Standard canon allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Point {
    /// `None` for books outside the 66-book canon.
    book: Option<Book>,
    /// `None` for lettered chapters (Greek Esther `Est.A:1`).
    chapter: Option<u16>,
    verse: u16,
    /// Subverse number (`.1`); letters (`!a`) round to the verse and are not kept.
    sub: Option<u16>,
}

impl Point {
    fn verse(self) -> Option<Verse> {
        Some(Verse::new(self.book?, self.chapter?, self.verse))
    }
}

#[derive(Debug, Clone, Copy)]
struct Range {
    start: Point,
    end: Point,
}

#[derive(Debug, Clone)]
enum Test {
    Exist(Point, bool),
    Last(Point),
    Title(Option<Book>, Option<u16>, bool),
    /// sum(words × multiplier) on the left `<` (or `>`) the same on the right.
    Words {
        a: Vec<(Point, u32)>,
        b: Vec<(Point, u32)>,
        less: bool,
    },
    Unknown,
}

#[derive(Debug, Clone)]
struct Rule {
    source_type: &'static str,
    source: Vec<Range>,
    standard: Vec<Range>,
    tests: Vec<Test>,
    identity: bool,
}

/// Parse `Gen.31:55`, `Psa.3:Title`, `Gen.3:1!a`, `Gen.6:1.1`, `Gen.5:31; 6:1`, `3Jn.1:14-15`,
/// `Gen.31:55--32:32`, `Est.A:1`. `book` carries over between `;` parts.
fn parse_refs(s: &str) -> Option<Vec<Range>> {
    let mut out = Vec::new();
    let mut book: Option<Option<Book>> = None;
    for part in s.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let (b, rest) = match part.split_once('.') {
            Some((b, rest)) if b.len() == 3 && rest.contains(':') => (Some(b), rest),
            _ => (None, part),
        };
        let bk = match b {
            Some(code) => Book::from_usfm(code),
            None => book?,
        };
        book = Some(bk);
        let (first, second) = match rest.split_once("--") {
            Some((a, b)) => (a, Some(b)),
            None => match rest.split_once('-') {
                Some((a, b)) => (a, Some(b)),
                None => (rest, None),
            },
        };
        let start = parse_point(bk, None, first)?;
        let end = match second {
            Some(e) => parse_point(bk, start.chapter, e)?,
            None => start,
        };
        out.push(Range { start, end });
    }
    (!out.is_empty()).then_some(out)
}

/// `C:V…` or, with a default chapter, `V…`.
fn parse_point(book: Option<Book>, default_chapter: Option<u16>, s: &str) -> Option<Point> {
    let s = s.trim();
    let (chapter, verse_part) = match s.split_once(':') {
        Some((c, v)) => (c.trim().parse::<u16>().ok(), v),
        None => (default_chapter, s),
    };
    if s.contains(':') && chapter.is_none() && !s.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }
    let verse_part = verse_part.trim();
    if verse_part.eq_ignore_ascii_case("title") {
        return Some(Point {
            book,
            chapter,
            verse: 0,
            sub: None,
        });
    }
    let digits_end = verse_part
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(verse_part.len());
    let verse: u16 = verse_part[..digits_end].parse().ok()?;
    let suffix = &verse_part[digits_end..];
    let sub = suffix.strip_prefix('.').and_then(|n| n.parse::<u16>().ok());
    Some(Point {
        book,
        chapter,
        verse,
        sub,
    })
}

/// `REF`, `REF*n`, or a sum of them (`Mal.4:4+Mal.4:6`).
fn parse_operand(s: &str) -> Option<Vec<(Point, u32)>> {
    s.split('+')
        .map(|term| {
            let (r, m) = match term.split_once('*') {
                Some((r, m)) => (r, m.trim().parse().ok()?),
                None => (term, 1),
            };
            let range = parse_refs(r.trim())?;
            Some((range.first()?.start, m))
        })
        .collect()
}

fn parse_test(t: &str) -> Test {
    let t = t.trim();
    if let Some((r, v)) = t.split_once(":TextBeforeV1=") {
        let (b, c) = r.split_once('.').unwrap_or((r, ""));
        let exist = match v.trim() {
            "Exist" => true,
            "NotExist" => false,
            _ => return Test::Unknown,
        };
        return Test::Title(Book::from_usfm(b), c.parse().ok(), exist);
    }
    if let Some((r, what)) = t.split_once('=') {
        let Some(p) = parse_refs(r).and_then(|v| v.first().map(|x| x.start)) else {
            return Test::Unknown;
        };
        return match what.trim() {
            "Exist" => Test::Exist(p, true),
            "NotExist" => Test::Exist(p, false),
            "Last" => Test::Last(p),
            _ => Test::Unknown,
        };
    }
    for (op, less) in [('<', true), ('>', false)] {
        if let Some((a, b)) = t.split_once(op) {
            return match (parse_operand(a), parse_operand(b)) {
                (Some(a), Some(b)) => Test::Words { a, b, less },
                _ => Test::Unknown,
            };
        }
    }
    Test::Unknown
}

fn rules() -> &'static [Rule] {
    static RULES: OnceLock<Vec<Rule>> = OnceLock::new();
    RULES.get_or_init(|| {
        let mut out = Vec::new();
        for line in TVTMS.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let cols: Vec<&'static str> = line.split('\t').collect();
            let [source_type, src, std, action, tests] = cols[..] else {
                continue;
            };
            let (Some(source), Some(standard)) = (parse_refs(src), parse_refs(std)) else {
                continue;
            };
            let tests = if tests.trim().is_empty() {
                Vec::new()
            } else {
                tests.split('&').map(parse_test).collect()
            };
            let identity = src == std && action.starts_with("Keep");
            out.push(Rule {
                source_type,
                source,
                standard,
                tests,
                identity,
            });
        }
        out
    })
}

/// Known Hebrew numbering for BDB references, unlike a Bible whose numbering must be
/// detected from its text. TVTMS's Hebrew rows declare the source tradition explicitly;
/// their tests distinguish that tradition from other Bibles, not individual BDB citations.
/// Whole-verse targets are unioned, including merged verses and numbered Psalm titles.
pub(crate) struct HebrewReferences {
    pub(crate) numbering: ListedVersification,
    targets: BTreeMap<Verse, BTreeSet<Verse>>,
}

impl HebrewReferences {
    pub(crate) fn get() -> &'static Self {
        static HEBREW: OnceLock<HebrewReferences> = OnceLock::new();
        HEBREW.get_or_init(|| {
            let mut targets: BTreeMap<Verse, BTreeSet<Verse>> = BTreeMap::new();
            let mut replaced = BTreeSet::new();
            for rule in rules()
                .iter()
                .filter(|r| r.source_type.split('+').any(|t| t == "Hebrew"))
            {
                let standard: BTreeSet<Verse> = rule
                    .standard
                    .iter()
                    .filter_map(standard_verses)
                    .flatten()
                    .collect();
                replaced.extend(standard.iter().copied());
                for range in &rule.source {
                    let (Some(start), Some(end)) = (range.start.verse(), range.end.verse()) else {
                        continue;
                    };
                    // Hebrew TVTMS ranges stay within one Bible book. The sole
                    // cross-chapter range (1Sa 20:42–21:1) has an unchanged first chapter.
                    for chapter in start.chapter..=end.chapter {
                        let first = if chapter == start.chapter {
                            start.verse
                        } else {
                            1
                        };
                        let last = if chapter == end.chapter {
                            end.verse
                        } else {
                            Standard.last_verse(start.book, chapter).unwrap_or(0)
                        };
                        for verse in first..=last {
                            targets
                                .entry(Verse::new(start.book, chapter, verse))
                                .or_default()
                                .extend(standard.iter().copied());
                        }
                    }
                }
            }
            // Only verses not covered by Hebrew rules retain Standard numbering.
            // Removing replaced targets also removes obsolete chapter ends/title slots.
            for verse in Standard.verses().filter(|v| !replaced.contains(v)) {
                targets.entry(verse).or_default().insert(verse);
            }
            let numbering = ListedVersification::new(targets.keys().map(|v| (*v, v.verse)));
            Self { numbering, targets }
        })
    }

    pub(crate) fn to_standard(
        &self,
        native: &obs_scripture::PassageList,
    ) -> Option<obs_scripture::PassageList> {
        let mut result = obs_scripture::PassageList::new();
        for passage in native {
            for verse in passage.verses(&self.numbering) {
                let targets = self.targets.get(&verse)?;
                if targets.is_empty() {
                    return None;
                }
                for target in targets {
                    result.push(Passage::verse(*target));
                }
            }
        }
        (!result.is_empty()).then(|| result.merged(&Standard))
    }
}

/// Number of TVTMS rules the engine knows (for diagnostics).
pub fn tvtms_rule_count() -> usize {
    rules().len()
}

struct Facts<'a> {
    /// Every verse number a Bible Verse covers → index into `verses`.
    covers: HashMap<(Book, u16, u16), usize>,
    verses: &'a [VerseFacts],
    /// Numbering of every listed verse (for expanding ranges).
    v11n: ListedVersification,
    /// Numbering of verses with text (for `=Last` and titles).
    with_text: ListedVersification,
}

impl Facts<'_> {
    fn entry(&self, p: Point) -> Option<&VerseFacts> {
        let (b, c) = (p.book?, p.chapter?);
        self.covers.get(&(b, c, p.verse)).map(|&i| &self.verses[i])
    }

    fn exists(&self, p: Point) -> bool {
        if p.sub.is_some_and(|s| s >= 1) {
            return false;
        }
        self.entry(p).is_some_and(|e| e.words > 0)
    }

    fn words(&self, p: Point) -> Option<u32> {
        self.entry(p).map(|e| e.words)
    }

    fn sum(&self, terms: &[(Point, u32)]) -> Option<u64> {
        terms.iter().try_fold(0u64, |acc, &(p, m)| {
            Some(acc + u64::from(self.words(p)?) * u64::from(m))
        })
    }

    fn holds(&self, t: &Test) -> bool {
        match *t {
            Test::Exist(p, want) => self.exists(p) == want,
            Test::Last(p) => match (p.book, p.chapter) {
                (Some(b), Some(c)) => {
                    self.with_text.last_verse(b, c) == Some(p.verse) && p.verse > 0
                }
                _ => false,
            },
            Test::Title(b, c, want) => match (b, c) {
                (Some(b), Some(c)) => self.with_text.has_title(b, c) == want,
                _ => !want,
            },
            Test::Words { ref a, ref b, less } => match (self.sum(a), self.sum(b)) {
                // Require a clear difference (25%). The tests were written for the source
                // languages; in English the two clauses of e.g. Phil 1:16-17 are near-equal in
                // either order (KJV 15:16, WEB 16:18, ESV 17:17), while Greek is 9:14. Without
                // a clear signal no rule applies and the verse maps by identity.
                (Some(x), Some(y)) if less => x * 4 < y * 3,
                (Some(x), Some(y)) => x * 3 > y * 4,
                _ => false,
            },
            Test::Unknown => false,
        }
    }

    /// Verse numbers of a range in this Bible's own numbering.
    fn source_points(&self, r: &Range) -> Vec<(Book, u16, u16)> {
        let (Some(book), Some(c1), Some(c2)) = (r.start.book, r.start.chapter, r.end.chapter)
        else {
            return Vec::new();
        };
        if c1 == c2 {
            return (r.start.verse..=r.end.verse.max(r.start.verse))
                .map(|v| (book, c1, v))
                .collect();
        }
        let (a, b) = (
            Verse::new(book, c1, r.start.verse),
            Verse::new(book, c2, r.end.verse),
        );
        match Passage::new(a, b) {
            Ok(p) => p
                .verses(&self.v11n)
                .map(|v| (v.book, v.chapter, v.verse))
                .collect(),
            Err(_) => Vec::new(),
        }
    }
}

/// Standard Verses of a range; `None` if any part lies outside the Standard versification.
fn standard_verses(r: &Range) -> Option<Vec<Verse>> {
    let (a, b) = (r.start.verse()?, r.end.verse()?);
    if !Standard.contains(a) || !Standard.contains(b) {
        return None;
    }
    let p = Passage::new(a, b).ok()?;
    Some(p.verses(&Standard).collect())
}

/// Build the Verse map of a Bible from its verse list (document order).
pub fn build_verse_map(verses: &[VerseFacts]) -> VerseMapOutcome {
    let mut covers = HashMap::new();
    for (i, f) in verses.iter().enumerate() {
        for n in f.verse.verse..=f.last.max(f.verse.verse) {
            covers
                .entry((f.verse.book, f.verse.chapter, n))
                .or_insert(i);
        }
    }
    let facts = Facts {
        covers,
        verses,
        v11n: ListedVersification::new(verses.iter().map(|f| (f.verse, f.last))),
        with_text: ListedVersification::new(
            verses
                .iter()
                .filter(|f| f.words > 0)
                .map(|f| (f.verse, f.last)),
        ),
    };
    let books: HashSet<Book> = verses.iter().map(|f| f.verse.book).collect();

    // Per Bible Verse: Standard Verses collected, and whether a rule sent it outside.
    let mut targets: Vec<BTreeSet<Verse>> = vec![BTreeSet::new(); verses.len()];
    let mut outside = vec![false; verses.len()];
    let mut touched = vec![false; verses.len()];
    let mut traditions: BTreeMap<String, usize> = BTreeMap::new();
    for rule in rules() {
        let Some(first) = rule.source.first() else {
            continue;
        };
        if !first.start.book.is_some_and(|b| books.contains(&b)) {
            continue;
        }
        if !rule.tests.iter().all(|t| facts.holds(t)) {
            continue;
        }
        let mut idx: BTreeSet<usize> = BTreeSet::new();
        for r in &rule.source {
            for key in facts.source_points(r) {
                if let Some(&i) = facts.covers.get(&key) {
                    idx.insert(i);
                }
            }
        }
        if idx.is_empty() {
            continue;
        }
        let mut std: Vec<Verse> = Vec::new();
        let mut out_of_standard = false;
        for r in &rule.standard {
            match standard_verses(r) {
                Some(v) => std.extend(v),
                None => out_of_standard = true,
            }
        }
        let moves = !rule.identity
            && idx.iter().any(|&i| {
                (std.is_empty() && out_of_standard) || std.iter().any(|s| *s != verses[i].verse)
            });
        if moves {
            *traditions.entry(rule.source_type.to_string()).or_insert(0) += 1;
        }
        for i in idx {
            touched[i] = true;
            targets[i].extend(std.iter().copied());
            outside[i] |= out_of_standard && std.is_empty();
        }
    }

    let mut entries = Vec::with_capacity(verses.len());
    let mut reached: HashSet<Verse> = HashSet::new();
    for (i, f) in verses.iter().enumerate() {
        let standard = if touched[i] && !targets[i].is_empty() {
            let (first, last) = (targets[i].first().copied(), targets[i].last().copied());
            first.zip(last).and_then(|(a, b)| Passage::new(a, b).ok())
        } else if touched[i] && outside[i] {
            None
        } else {
            identity(f)
        };
        if let Some(p) = standard {
            reached.extend(p.verses(&Standard));
        }
        entries.push(VerseMapEntry {
            verse: f.verse,
            last: f.last,
            standard,
        });
    }
    let absent: Vec<Verse> = Standard
        .verses()
        .filter(|v| books.contains(&v.book) && !reached.contains(v))
        .collect();
    VerseMapOutcome {
        map: VerseMap::new(entries, absent, &Standard),
        traditions,
    }
}

/// The Standard Passage with the same numbers, as far as the Standard has them.
fn identity(f: &VerseFacts) -> Option<Passage> {
    let start = f.verse;
    if !Standard.contains(start) {
        return None;
    }
    let mut end = start;
    for n in f.verse.verse + 1..=f.last {
        let v = Verse::new(start.book, start.chapter, n);
        if Standard.contains(v) {
            end = v;
        }
    }
    Passage::new(start, end).ok()
}

#[cfg(test)]
#[path = "verse_map_tests.rs"]
mod tests;
