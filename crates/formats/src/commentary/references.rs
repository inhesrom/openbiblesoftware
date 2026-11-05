//! Bible references found in text: the spellings the locked Commentary editions use in their
//! links and prose, and references the user typed in a Note body (#64,
//! [`scripture_references`]). Both are read by the Standard reference parser; nothing here
//! adds a grammar of its own. Note prose is matched conservatively (see
//! [`scripture_references`]): English words that are also book abbreviations need a period.
use obs_scripture::{Book, PassageList, Standard, Verse, parse_reference};
use regex::{Captures, Regex};
use std::sync::OnceLock;

#[expect(
    clippy::expect_used,
    reason = "all patterns are source-code constants tested by the converters"
)]
pub(super) fn re(slot: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    slot.get_or_init(|| Regex::new(pattern).expect("constant regular expression"))
}

const ONLINE: [&str; 66] = [
    "Ge", "Ex", "Le", "Nu", "De", "Jos", "Jud", "Ru", "1Sa", "2Sa", "1Ki", "2Ki", "1Ch", "2Ch",
    "Ezr", "Ne", "Es", "Job", "Ps", "Pr", "Ec", "So", "Isa", "Jer", "La", "Eze", "Da", "Ho", "Joe",
    "Am", "Ob", "Jon", "Mic", "Na", "Hab", "Zep", "Hag", "Zec", "Mal", "Mt", "Mr", "Lu", "Joh",
    "Ac", "Ro", "1Co", "2Co", "Ga", "Eph", "Php", "Col", "1Th", "2Th", "1Ti", "2Ti", "Tit", "Phm",
    "Heb", "Jas", "1Pe", "2Pe", "1Jo", "2Jo", "3Jo", "Jude", "Re",
];

pub(super) fn parse(text: &str, context: Option<Verse>, roman: bool) -> Option<PassageList> {
    let mut text = html_escape::decode_html_entities(text)
        .replace('+', " ")
        .replace("Bible:", "");
    static OSIS: OnceLock<Regex> = OnceLock::new();
    text = re(&OSIS, r"([1-3]?[A-Za-z]+)\.(\d+)(?:\.(\d+))?")
        .replace_all(&text, |c: &Captures<'_>| {
            if let Some(v) = c.get(3) {
                format!("{} {}:{}", &c[1], &c[2], v.as_str())
            } else {
                format!("{} {}", &c[1], &c[2])
            }
        })
        .into_owned();
    // Normalize Bible-book names before Roman chapters: lowercase `mic` is Micah,
    // not the Roman numeral 1099. Canonical OSIS casing protects it below.
    static WORD: OnceLock<Regex> = OnceLock::new();
    text = re(&WORD, r"\b[1-3]?[A-Za-z]+\b")
        .replace_all(&text, |c: &Captures<'_>| {
            ONLINE
                .iter()
                .position(|a| a.eq_ignore_ascii_case(&c[0]))
                .map(|i| Book::ALL[i].osis().to_string())
                .unwrap_or_else(|| c[0].to_string())
        })
        .into_owned();
    if roman {
        text = text.replace(", &c.", "").replace("&c.", "");
        static INITIAL_VERSE: OnceLock<Regex> = OnceLock::new();
        text = re(&INITIAL_VERSE, r"^\s*v\.\s*")
            .replace(&text, "")
            .into_owned();
        static ROMAN: OnceLock<Regex> = OnceLock::new();
        text = re(&ROMAN, r"\b([ivxlcdm]+)(?:\.\s*|\b)([0-9]*)")
            .replace_all(&text, |c: &Captures<'_>| {
                let mut total = 0i32;
                let mut previous = 0;
                for ch in c[1].chars().rev() {
                    let n = match ch {
                        'i' => 1,
                        'v' => 5,
                        'x' => 10,
                        'l' => 50,
                        'c' => 100,
                        'd' => 500,
                        'm' => 1000,
                        _ => 0,
                    };
                    total += if n < previous { -n } else { n };
                    previous = n;
                }
                if c[2].is_empty() {
                    format!("{total} ")
                } else {
                    format!("{total}:{}", &c[2])
                }
            })
            .into_owned();
        static CHAPTER: OnceLock<Regex> = OnceLock::new();
        if let Some(v) = context {
            text = re(&CHAPTER, r"(?i)\b(?:ch|chap)\.\s*")
                .replace_all(&text, format!("{} ", v.book.osis()))
                .into_owned();
        }
        static JOIN: OnceLock<Regex> = OnceLock::new();
        text = re(&JOIN, r"\b(?:and|with)\b")
            .replace_all(&text, "; ")
            .into_owned();
        static ADJACENT: OnceLock<Regex> = OnceLock::new();
        for _ in 0..2 {
            text = re(&ADJACENT, r"(\d)\s+(\d)")
                .replace_all(&text, "$1; $2")
                .into_owned();
        }
        static VER: OnceLock<Regex> = OnceLock::new();
        text = re(&VER, r"(?i)\b(?:ver|vers|verse|verses|v)\.\s*")
            .replace_all(&text, "")
            .into_owned();
        text = text.replace(", &c.", "").replace("&c.", "");
    }
    parse_reference(
        text.trim().trim_end_matches([',', ';', '.']),
        &Standard,
        context,
    )
    .ok()
}

/// Unlinked references in prose (notably Genesis in MHC) use the same Standard parser.
/// Explicit link spans win; a failed candidate remains ordinary text.
#[expect(
    clippy::expect_used,
    reason = "pattern contains only escaped built-in Bible book names"
)]
pub(super) fn annotate_prose(text: &mut crate::text_buf::TextBuf, context: Option<Verse>) {
    static CANDIDATE: OnceLock<Regex> = OnceLock::new();
    let regex = CANDIDATE.get_or_init(|| {
        let mut names: Vec<String> = Book::ALL
            .into_iter()
            .flat_map(|b| [b.name(), b.osis(), b.abbrev()])
            .chain(ONLINE)
            .map(regex::escape)
            .collect();
        names.sort_by_key(|s| std::cmp::Reverse(s.len()));
        names.dedup();
        let chapter_verse = r"(?:[ivxlcdm]+\.\s*\d+|\d+[:.]\d+)";
        Regex::new(&format!(
            r"(?i)\b(?:{})\.?\s+{chapter_verse}(?:\s*[;,]\s*{chapter_verse}|\s*[-,]\s*\d+)*",
            names.join("|")
        ))
        .expect("Bible book names are escaped")
    });
    for found in regex.find_iter(&text.text) {
        if text.spans.iter().any(|(r, k)| {
            matches!(k, obs_model::SpanKind::ScriptureRef { .. })
                && r.start < found.end()
                && r.end > found.start()
        }) {
            continue;
        }
        if let Some(targets) = parse(&found.as_str().to_lowercase(), context, true) {
            text.spans
                .push((found.range(), obs_model::SpanKind::ScriptureRef { targets }));
        }
    }
}

/// English words that are also spellings of a Bible book (`Is` Isaiah, `So` Song of Songs,
/// `Re` Revelation): in Note prose they name a book only when written with a period
/// (`Is. 53:5`), since "Is 2:30 OK?" and "So 3:15 then" are times, not references.
const ENGLISH_WORDS: [&str; 10] = ["Am", "Is", "So", "Re", "Ex", "He", "Ho", "Ac", "Act", "Ob"];

/// The book word of a reference that starts `text`: the last run of letters before the first
/// number after one (`Cor` in `1 Cor 13:4`, `Am` in `I Am 3:15`).
fn book_word(text: &str) -> Option<std::ops::Range<usize>> {
    let mut word: Option<std::ops::Range<usize>> = None;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c.is_alphabetic() {
            let mut end = i + c.len_utf8();
            while let Some(&(j, d)) = chars.peek() {
                if !d.is_alphabetic() {
                    break;
                }
                end = j + d.len_utf8();
                chars.next();
            }
            word = Some(i..end);
        } else if c.is_ascii_digit() && word.is_some() {
            break;
        }
    }
    word
}

/// Whether the reference starting `text` names its book with an English word and no period.
fn english_book(text: &str) -> Option<usize> {
    let w = book_word(text)?;
    let dotted = text[w.end..].starts_with('.');
    (!dotted
        && ENGLISH_WORDS
            .iter()
            .any(|e| e.eq_ignore_ascii_case(&text[w.clone()])))
    .then_some(w.end)
}

/// Bible references written in plain text that the user typed (a Note body, #64), read in the
/// Standard versification (ADR 0002): each found reference's byte range and Passages, in text
/// order. A reference is a capitalized Bible book name or abbreviation (any spelling the
/// reference box accepts: `Jn`, `Rom`, `1 Cor`, `Psalm`) followed by chapter:verse, then
/// optionally more of a list (`Jn 3:16-18; Rom 5:8`, `1 Cor 13:4, 7`). A bare `Romans 8` stays
/// text, since book names double as words ("Job 5", "Mark 4"), and so does a lowercase word
/// ("I am 3:15" is not Amos). English words that are also book spellings ([`ENGLISH_WORDS`]:
/// `Am`, `Is`, `So`, `Re` …) name a book only with a period (`Is. 53:5`), so "Is 2:30 OK?"
/// stays text and a list stops before one ("Rom 5:8; So 3 of us" links Rom 5:8 only); the
/// app's own abbreviations (`Isa`, `Song`, `Rev`, `Amos`) always work. A candidate that does
/// not parse (verse out of range) shrinks to its longest leading list that does, else stays
/// text.
#[expect(clippy::expect_used, reason = "the pattern is a source-code constant")]
pub fn scripture_references(text: &str) -> Vec<(std::ops::Range<usize>, PassageList)> {
    static CANDIDATE: OnceLock<Regex> = OnceLock::new();
    static SEPARATOR: OnceLock<Regex> = OnceLock::new();
    let regex = CANDIDATE.get_or_init(|| {
        let book = r"(?:[1-3]\s?|I{1,3}\s)?\p{Lu}\p{L}+\.?\s*";
        let item = r"\d+(?::\d+)?(?:\s*[-–—]\s*\d+(?::\d+)?)?";
        Regex::new(&format!(
            r"\b{book}\d+:\d+(?:\s*[-–—]\s*\d+(?::\d+)?)?(?:\s*[;,]\s*(?:{book})?{item})*"
        ))
        .expect("constant regular expression")
    });
    let separator = re(&SEPARATOR, r"\s*[;,]\s*");
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(found) = regex.find_at(text, at) {
        let candidate = found.as_str();
        if let Some(end) = english_book(candidate) {
            // Not a reference; one may start after the word.
            at = found.start() + end;
            continue;
        }
        // The list stops before an item naming its book with an English word.
        let mut ends: Vec<usize> = Vec::new();
        let mut limit = candidate.len();
        for sep in separator.find_iter(candidate) {
            let rest = &candidate[sep.end()..];
            if rest.starts_with(char::is_alphabetic) && english_book(rest).is_some() {
                limit = sep.start();
                break;
            }
            ends.push(sep.start());
        }
        ends.push(limit);
        at = found.start() + limit;
        // The whole list, else each shorter leading list.
        for end in ends.into_iter().rev() {
            let Some(head) = candidate.get(..end) else {
                continue;
            };
            if let Some(list) = parse(head, None, false).filter(|l| !l.is_empty()) {
                out.push((found.start()..found.start() + end, list));
                break;
            }
        }
    }
    out
}
