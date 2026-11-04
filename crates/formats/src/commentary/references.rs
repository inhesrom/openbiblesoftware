//! Reference spellings used by the locked editions, not a new user-input grammar.
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
