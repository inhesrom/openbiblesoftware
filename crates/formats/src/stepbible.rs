//! STEPBible TAHOT / TAGNT Import: the Translators Amalgamated Hebrew OT and Greek NT
//! (tab-separated, one line per word) → a Bible in which every word is a Tagged word.
//!
//! - **Verses** use the numbering of the original-language text: the reference in round
//!   brackets when there is one (TAHOT `Gen.31.55(32.1)` is Hebrew Gen 32:1; TAGNT
//!   `Act.13.39(13.38)` is NA Act 13:38), else the English (NRSV) reference. The Verse map to
//!   the Standard versification is built from TVTMS, as for every Import.
//! - **Tagged words** carry their morpheme parts in order. A TAHOT word `וְ/הָ/אָ֗רֶץ` with
//!   dStrongs `H9002/H9009/{H0776G}` has three parts: the prefixes ו and ה, then the root, each
//!   with its own Strong's number, Lemma, Morphology code and Gloss. Strong's numbers are
//!   normalized (`H0776G` → `H776G`); root braces and the "covers the next word" `+` are
//!   dropped. Morphology codes after the first part get the language letter (`HC/Td/Ncfsa` →
//!   `HC`, `HTd`, `HNcfsa`) so each part's code stands alone.
//! - **Text**: TAHOT's Hebrew without its `/` and `\` separators; maqaf joins words, paseq
//!   stands between words, and petuha (פ) or setumah (ס) ends a paragraph. TAGNT's words found
//!   in NA27 (its base text, including the double-bracketed passages); `¶` ends a paragraph. A chapter always starts a paragraph. TAHOT words of a Psalm title
//!   (English verse 0) also get a `psalm_title` span.
//! - **Notes** (footnotes after the word): the Ketiv of every Qere; TAHOT's other meaning
//!   variants; words restored from a parallel passage (R) or the Septuagint (X); TAGNT's
//!   meaning variants; and words that other editions add and that change the translation.

use std::collections::BTreeMap;
use std::ops::Range;

use obs_model::{
    Direction, FORMAT_VERSION, KeyScheme, LICENSE_UNKNOWN, Manifest, MorphemePart, NoteKind,
    Origin, ParaStyle, Resource, ResourceBuilder, ResourceType, SpanKind, UnitId, UnitKind,
    is_valid_resource_id, tokenize,
};
use obs_scripture::{Book, StrongsNumber, Verse};
use unicode_normalization::char::is_combining_mark;

use crate::report::{BookReport, ImportReport, VerseMapCounts};
use crate::text_buf::TextBuf;
use crate::usfm::{ImportError, slugify};
use crate::verse_map::{VerseFacts, build_verse_map};

/// Which STEPBible dataset the files hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StepbibleText {
    /// Translators Amalgamated Hebrew OT (four files).
    #[default]
    Tahot,
    /// Translators Amalgamated Greek NT (two files).
    Tagnt,
}

impl StepbibleText {
    /// The manifest's `source_format`.
    pub fn source_format(self) -> &'static str {
        match self {
            StepbibleText::Tahot => "stepbible-tahot",
            StepbibleText::Tagnt => "stepbible-tagnt",
        }
    }

    pub fn direction(self) -> Direction {
        match self {
            StepbibleText::Tahot => Direction::Rtl,
            StepbibleText::Tagnt => Direction::Ltr,
        }
    }

    fn default_language(self) -> &'static str {
        match self {
            StepbibleText::Tahot => "hbo",
            StepbibleText::Tagnt => "grc",
        }
    }
}

/// Identity and metadata for the Resource. Empty fields get defaults.
#[derive(Debug, Clone, Default)]
pub struct StepbibleOptions {
    pub text: StepbibleText,
    pub resource_id: String,
    pub title: String,
    pub abbreviation: String,
    /// BCP 47; defaults to `hbo` (TAHOT) or `grc` (TAGNT).
    pub language: String,
    /// File name(s) shown in About and the report.
    pub source_file: String,
    /// Version of the tool doing the Import, recorded in the manifest.
    pub tool_version: String,
}

/// Collects the files of one dataset, then imports them at once in canonical book order.
#[derive(Debug, Default)]
pub struct StepbibleImport {
    options: StepbibleOptions,
    files: Vec<(String, String)>,
}

/// Where a word line sits: its book, its own-numbering Verse and its English verse number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WordRef<'a> {
    book: Book,
    english: (u16, u16),
    own: (u16, u16),
    /// TAHOT text type (`L`, `Q(K)`, `X` …) or TAGNT word type (`NKO`, `K` …).
    kind: &'a str,
}

/// Parse `Gen.31.55(32.1)#01=L`, `Mat.1.1#01=NKO`, `Act.2.11[2.10]#03=NKO`. `None` if the
/// column is not a word reference.
fn parse_word_ref(col: &str) -> Option<WordRef<'_>> {
    let (reference, rest) = col.split_once('#')?;
    let (number, kind) = rest.split_once('=')?;
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) || kind.is_empty() {
        return None;
    }
    let (code, rest) = reference.split_once('.')?;
    let book = Book::from_usfm(code)?;
    let end = rest.find(['(', '[', '{']).unwrap_or(rest.len());
    let (cv, mut brackets) = rest.split_at(end);
    let english = parse_cv(cv)?;
    let mut own = english;
    while let Some(open) = brackets.chars().next() {
        let close = match open {
            '(' => ')',
            '[' => ']',
            '{' => '}',
            _ => return None,
        };
        let stop = brackets.find(close)?;
        // Round brackets give the original-language numbering (Hebrew, or NA for TAGNT);
        // square (KJV) and curly (other editions) brackets are left to the Verse map.
        if open == '(' {
            own = parse_cv(&brackets[1..stop])?;
        }
        brackets = &brackets[stop + 1..];
    }
    Some(WordRef {
        book,
        english,
        own,
        kind,
    })
}

fn parse_cv(s: &str) -> Option<(u16, u16)> {
    let (c, v) = s.split_once('.')?;
    Some((c.parse().ok()?, v.parse().ok()?))
}

/// One word line of the source.
struct Line<'a> {
    verse: Verse,
    english_verse: u16,
    kind: &'a str,
    cols: Vec<&'a str>,
}

impl Line<'_> {
    fn col(&self, i: usize) -> &str {
        self.cols.get(i).copied().unwrap_or("").trim()
    }
}

struct Note {
    id: UnitId,
    text: String,
}

/// State shared across books: note bodies, counts and warnings.
#[derive(Default)]
struct Ctx {
    notes: Vec<Note>,
    warnings: Vec<String>,
    tagged_words: usize,
}

struct BookOutput {
    text: String,
    spans: Vec<(Range<usize>, SpanKind)>,
    verses: Vec<(Range<usize>, Verse)>,
    chapters: u16,
}

/// Writes one Bible book's text unit: words, paragraphs, chapters, verses and note markers.
struct BookEmitter {
    buf: TextBuf,
    chapter: Option<(u16, usize)>,
    chapters: u16,
    para_start: Option<usize>,
    /// A paragraph mark closed the last paragraph; the next word starts a new one.
    para_break: bool,
    verse: Option<(Verse, usize)>,
    verses: Vec<(Range<usize>, Verse)>,
    /// The next word follows without a space (after a maqaf, or as a verse's first word).
    join_next: bool,
    /// Psalm title words so far: (start, end).
    title: Option<(usize, usize)>,
}

impl BookEmitter {
    fn new() -> Self {
        Self {
            buf: TextBuf::new(),
            chapter: None,
            chapters: 0,
            para_start: None,
            para_break: false,
            verse: None,
            verses: Vec::new(),
            join_next: false,
            title: None,
        }
    }

    /// Make `verse` current: a new chapter and paragraph if needed, then the verse itself.
    fn enter(&mut self, verse: Verse) {
        if self.chapter.map(|(c, _)| c) != Some(verse.chapter) {
            self.close_chapter();
            let start = self.buf.start_block();
            self.chapter = Some((verse.chapter, start));
            self.chapters += 1;
            self.para_start = Some(start);
            self.para_break = false;
        } else if self.para_break {
            let start = self.buf.start_block();
            self.para_start = Some(start);
            self.para_break = false;
        }
        if self.verse.map(|(v, _)| v) != Some(verse) {
            self.close_verse();
            self.buf.push_text(" ");
            self.buf.flush_space();
            self.verse = Some((verse, self.buf.pos()));
            self.join_next = true;
        }
    }

    /// Start a word in `verse`; returns its start offset.
    fn begin_word(&mut self, verse: Verse) -> usize {
        self.enter(verse);
        if !self.join_next {
            self.buf.push_text(" ");
        }
        self.join_next = false;
        self.buf.flush_space();
        self.buf.pos()
    }

    fn tagged_word(&mut self, range: Range<usize>, parts: Vec<MorphemePart>, ctx: &mut Ctx) {
        if !parts.is_empty() && range.end > range.start {
            self.buf.add_span(range, SpanKind::TaggedWord { parts });
            ctx.tagged_words += 1;
        }
    }

    /// End the current paragraph at a paragraph mark.
    fn paragraph_mark(&mut self) {
        self.close_para();
        self.para_break = true;
    }

    /// A footnote marker here, pointing at a new note body.
    fn note(&mut self, text: String, ctx: &mut Ctx) {
        let n = u32::try_from(ctx.notes.len() + 1).unwrap_or(u32::MAX);
        let id = UnitId::note(n);
        let pos = self.buf.pos();
        self.buf.add_span(
            pos..pos,
            SpanKind::NoteRef {
                note: id.clone(),
                note_kind: NoteKind::Footnote,
            },
        );
        ctx.notes.push(Note { id, text });
    }

    fn extend_title(&mut self, start: usize) {
        let end = self.buf.pos();
        match &mut self.title {
            Some((_, e)) => *e = end,
            None => self.title = Some((start, end)),
        }
    }

    fn close_title(&mut self) {
        if let Some((start, end)) = self.title.take()
            && end > start
        {
            self.buf.add_span(start..end, SpanKind::PsalmTitle);
        }
    }

    fn close_verse(&mut self) {
        if let Some((verse, start)) = self.verse.take() {
            let end = self.buf.tracked_end.max(start);
            self.verses.push((start..end, verse));
        }
    }

    fn close_para(&mut self) {
        if let Some(start) = self.para_start.take() {
            let end = self.buf.pos().max(start);
            if end > start {
                self.buf.add_span(
                    start..end,
                    SpanKind::Para {
                        style: ParaStyle::Prose,
                    },
                );
            }
        }
    }

    fn close_chapter(&mut self) {
        self.close_verse();
        self.close_title();
        self.close_para();
        if let Some((chapter, start)) = self.chapter.take() {
            let end = self.buf.pos().max(start);
            self.buf.add_span(start..end, SpanKind::Chapter { chapter });
        }
    }

    fn finish(mut self) -> BookOutput {
        self.close_chapter();
        BookOutput {
            text: self.buf.text,
            spans: self.buf.spans,
            verses: self.verses,
            chapters: self.chapters,
        }
    }
}

fn parse_strongs(token: &str, at: &str, ctx: &mut Ctx) -> Option<StrongsNumber> {
    let t = token
        .trim()
        .trim_matches(|c: char| c == '{' || c == '}' || c == '+');
    match t.parse() {
        Ok(n) => Some(n),
        Err(_) => {
            ctx.warnings
                .push(format!("{at}: bad Strong's number `{token}`"));
            None
        }
    }
}

/// Trimmed, NFC-normalized text, or `None` if empty.
fn some_text(s: &str) -> Option<String> {
    use unicode_normalization::UnicodeNormalization;
    let t = s.trim();
    (!t.is_empty()).then(|| t.nfc().collect())
}

// ---------------------------------------------------------------------------------------
// TAHOT
// ---------------------------------------------------------------------------------------

/// TAHOT columns.
const H_HEBREW: usize = 1;
const H_TRANSLATION: usize = 3;
const H_DSTRONGS: usize = 4;
const H_GRAMMAR: usize = 5;
const H_MEANING_VARIANTS: usize = 6;
const H_SPELLING_VARIANTS: usize = 7;
const H_EXPANDED: usize = 11;

/// Split at `/` outside `{…}` (a gloss inside the braces may contain `/`).
fn split_outside_braces(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            '/' if depth <= 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

fn is_hebrew_letter(c: char) -> bool {
    ('\u{05D0}'..='\u{05EA}').contains(&c)
}

/// The Hebrew of a variant or Ketiv, without the `/` and `\` separators.
fn plain_hebrew(s: &str) -> String {
    s.chars().filter(|c| *c != '/' && *c != '\\').collect()
}

/// A gloss with its morpheme separators removed: `tent/ his` → `tent his`.
fn plain_gloss(s: &str) -> String {
    s.split('/')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The manuscripts and editions behind TAHOT's variant letters.
fn hebrew_source_name(letter: char) -> Option<&'static str> {
    Some(match letter {
        'A' => "Aleppo Codex",
        'B' => "BHS",
        'C' => "Cairo Codex",
        'D' => "Dead Sea Scrolls",
        'E' => "scholarly emendation",
        'F' => "different word division or pointing",
        'H' => "Ben Chayyim edition",
        'K' => "Ketiv (written text)",
        'L' => "Leningrad Codex",
        'P' => "alternative punctuation",
        'Q' => "Qere (read text)",
        'S' => "scribal tradition",
        'V' => "other manuscripts",
        'X' => "Septuagint",
        _ => return None,
    })
}

/// A TAHOT meaning variant, `K= 'o.ho.Lo/h (אָהֳלֹ/ה\׃) "tent/ his" (H0168G/…)`, as note text.
fn tahot_variant_note(raw: &str) -> String {
    let parsed = (|| {
        let (letters, rest) = raw.split_once('=')?;
        let names: Vec<&str> = letters
            .trim()
            .chars()
            .map(hebrew_source_name)
            .collect::<Option<_>>()?;
        if names.is_empty() {
            return None;
        }
        let open = rest.find('(')?;
        let close = open + rest[open..].find(')')?;
        let hebrew = plain_hebrew(&rest[open + 1..close]);
        let after = &rest[close + 1..];
        let gloss = after
            .split_once('"')
            .and_then(|(_, g)| g.split_once('"'))
            .map(|(g, _)| plain_gloss(g));
        let label = names.join(", ");
        Some(match gloss {
            Some(g) if !g.is_empty() => format!("{label}: {hebrew} “{g}”"),
            _ => format!("{label}: {hebrew}"),
        })
    })();
    parsed.unwrap_or_else(|| format!("Variant: {}", raw.trim()))
}

/// The Ketiv in TAHOT's spelling-variant column (`L= הַוְצֵ֣א ¦ ;K= הוֹצֵא`), if any.
fn spelling_ketiv(col: &str) -> Option<String> {
    col.split([';', '¦'])
        .find_map(|p| p.trim().strip_prefix("K="))
        .map(|k| plain_hebrew(k.trim()))
        .filter(|k| !k.is_empty())
}

const NOTE_LXX: &str = "Not in the Leningrad Codex: supplied from the Septuagint (Greek), as \
                        reconstructed in Hebrew by BHS or BHK.";
const NOTE_RESTORED: &str = "Not in the Leningrad Codex: restored from a parallel passage.";

/// Emits TAHOT word lines.
struct TahotBook {
    em: BookEmitter,
    /// The R or X run being noted: its kind and Verse.
    run: Option<(char, Verse)>,
}

impl TahotBook {
    fn word(&mut self, line: &Line, ctx: &mut Ctx) {
        let at = format!(
            "{} {}:{} ({})",
            line.verse.book.usfm(),
            line.verse.chapter,
            line.verse.verse,
            line.kind
        );
        let hebrew: Vec<&str> = line.col(H_HEBREW).split('/').collect();
        let strongs: Vec<&str> = line.col(H_DSTRONGS).split('/').collect();
        let grammar: Vec<&str> = line.col(H_GRAMMAR).split('/').collect();
        let glosses: Vec<&str> = line.col(H_TRANSLATION).split('/').collect();
        let expanded = split_outside_braces(line.col(H_EXPANDED));
        let lang = grammar
            .first()
            .and_then(|g| g.trim().chars().next())
            .filter(|c| matches!(c, 'H' | 'A'));

        let mut word: Option<usize> = None;
        let mut parts: Vec<MorphemePart> = Vec::new();
        let mut first_start = None;
        for (i, seg) in hebrew.iter().enumerate() {
            // An empty segment separates words (a Qere of two words for one Ketiv).
            if seg.trim().is_empty() {
                if let Some(start) = word.take() {
                    let end = self.em.buf.pos();
                    self.em
                        .tagged_word(start..end, std::mem::take(&mut parts), ctx);
                }
                continue;
            }
            let (letters, punct) = match seg.split_once('\\') {
                Some((l, p)) => (l, p.replace('\\', "")),
                None => (*seg, String::new()),
            };
            // A segment of punctuation only (a maqaf between two words of a Qere).
            if !letters.chars().any(is_hebrew_letter) {
                if let Some(start) = word.take() {
                    let end = self.em.buf.pos();
                    self.em
                        .tagged_word(start..end, std::mem::take(&mut parts), ctx);
                }
                self.punctuation(&plain_hebrew(seg));
                continue;
            }
            if word.is_none() {
                let start = self.em.begin_word(line.verse);
                first_start.get_or_insert(start);
                word = Some(start);
            }
            self.em.buf.push_text(letters);
            let part = (|| {
                let token = strongs.get(i)?.split('\\').next()?;
                let mut part = MorphemePart::new(parse_strongs(token, &at, ctx)?);
                part.morph = grammar.get(i).and_then(|g| {
                    let g = g.trim();
                    match lang {
                        Some(l) if i > 0 && !g.is_empty() => Some(format!("{l}{g}")),
                        _ => some_text(g),
                    }
                });
                part.gloss = glosses.get(i).and_then(|g| some_text(g));
                part.lemma = expanded.get(i).and_then(|e| {
                    let e = e.split('\\').next()?.trim_matches(|c| c == '{' || c == '}');
                    let lemma = e.split('=').nth(1)?;
                    some_text(lemma).filter(|l| l.chars().any(is_hebrew_letter))
                });
                Some(part)
            })();
            parts.extend(part);
            if !punct.is_empty() {
                if let Some(start) = word.take() {
                    let end = self.em.buf.pos();
                    self.em
                        .tagged_word(start..end, std::mem::take(&mut parts), ctx);
                }
                self.punctuation(&punct);
            }
        }
        if let Some(start) = word.take() {
            let end = self.em.buf.pos();
            self.em.tagged_word(start..end, parts, ctx);
        }
        if line.english_verse == 0 {
            if let Some(start) = first_start {
                self.em.extend_title(start);
            }
        } else {
            self.em.close_title();
        }
        self.notes(line, ctx);
    }

    fn punctuation(&mut self, punct: &str) {
        // Paseq stands between words.
        if punct.starts_with('׀') {
            self.em.buf.push_text(" ");
        }
        self.em.buf.push_text(punct);
        if punct.ends_with('־') {
            self.em.join_next = true;
        }
        if punct.contains(['פ', 'ס']) {
            self.em.paragraph_mark();
        }
    }

    fn notes(&mut self, line: &Line, ctx: &mut Ctx) {
        // Notes need the verse even when the word itself is empty (a Ketiv with no Qere).
        if self.em.verse.map(|(v, _)| v) != Some(line.verse) {
            self.em.enter(line.verse);
        }
        let meaning = line.col(H_MEANING_VARIANTS);
        if !meaning.is_empty() {
            self.em.note(tahot_variant_note(meaning), ctx);
        } else if line.kind.starts_with('Q')
            && let Some(ketiv) = spelling_ketiv(line.col(H_SPELLING_VARIANTS))
        {
            self.em.note(format!("Ketiv (written text): {ketiv}"), ctx);
        }
        let run = match line.kind.chars().next() {
            Some(k @ ('X' | 'R')) => Some((k, line.verse)),
            _ => None,
        };
        if run.is_some() && run != self.run {
            let text = if run.is_some_and(|(k, _)| k == 'X') {
                NOTE_LXX
            } else {
                NOTE_RESTORED
            };
            self.em.note(text.to_string(), ctx);
        }
        self.run = run;
    }
}

// ---------------------------------------------------------------------------------------
// TAGNT
// ---------------------------------------------------------------------------------------

/// TAGNT columns.
const G_GREEK: usize = 1;
const G_ENGLISH: usize = 2;
const G_DSTRONGS: usize = 3;
const G_DICTIONARY: usize = 4;
const G_EDITIONS: usize = 5;
const G_MEANING_VARIANTS: usize = 6;

/// The edition whose words make the text.
const BASE_EDITION: &str = "NA27";

/// The name in one entry of a TAGNT editions list, without the word-order mark that some
/// entries carry (`NA27»1`). The lines are already in NA27 order, so the marks are not used.
fn edition(token: &str) -> &str {
    let token = token.trim();
    token.find(['»', '«']).map_or(token, |i| &token[..i])
}

/// Whether a word is in the base edition.
fn in_base_edition(editions: &str) -> bool {
    editions.split('+').any(|t| edition(t) == BASE_EDITION)
}

/// Edition names of `editions`, without displacements.
fn edition_names(editions: &str) -> impl Iterator<Item = &str> {
    editions
        .split('+')
        .map(edition)
        .filter(|n| !n.is_empty())
}

/// A TAGNT English rendering without its verse-number hints (`{8.1} And` → `And`).
fn plain_english(s: &str) -> &str {
    let s = s.trim();
    match s.strip_prefix('{').and_then(|r| r.split_once('}')) {
        Some((_, rest)) => rest.trim(),
        None => s,
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || is_combining_mark(c) || matches!(c, '᾽' | '’' | '\'')
}

/// One TAGNT meaning variant: `ἐπηρώτων (t=epērōtōn) were asking - G1905=V-IAI-3P in: TR+Byz`.
struct GreekVariant<'a> {
    greek: &'a str,
    english: &'a str,
    tags: &'a str,
    editions: &'a str,
}

fn parse_tagnt_variant(entry: &str) -> Option<GreekVariant<'_>> {
    let (left, editions) = entry.trim().rsplit_once(" in: ")?;
    let (left, tags) = left.rsplit_once(" - ")?;
    let (greek, rest) = left.split_once(" (")?;
    let (_, english) = rest.split_once(") ")?;
    Some(GreekVariant {
        greek: greek.trim(),
        english: english.trim(),
        tags: tags.trim(),
        editions: editions.trim(),
    })
}

/// TAGNT meaning variants, several separated by `¦`, as note text.
fn tagnt_variant_note(raw: &str) -> String {
    let entries: Vec<String> = raw
        .split('¦')
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .map(|entry| match parse_tagnt_variant(entry) {
            Some(v) => {
                let names: Vec<&str> = edition_names(v.editions).collect();
                format!("{}: {} “{}”", names.join(", "), v.greek, v.english)
            }
            None => entry.to_string(),
        })
        .collect();
    let label = if entries.len() > 1 {
        "Variants"
    } else {
        "Variant"
    };
    format!("{label}: {}", entries.join("; "))
}

/// The Greek of a TAGNT word column, without its transliteration: `Βίβλος (Biblos)`.
fn greek_word(col: &str) -> &str {
    col.rsplit_once(" (").map_or(col, |(w, _)| w).trim()
}

/// The text of one word as emitted: the line's own, or the base edition's reading that
/// TAGNT lists only as a variant of it.
struct Reading<'a> {
    /// With punctuation and any `¶`.
    greek: String,
    english: &'a str,
    tags: &'a str,
    dictionary: &'a str,
    note: Option<String>,
}

impl<'a> Reading<'a> {
    fn of(line: &'a Line<'a>) -> Self {
        let variants = line.col(G_MEANING_VARIANTS);
        Reading {
            greek: greek_word(line.col(G_GREEK)).to_string(),
            english: line.col(G_ENGLISH),
            tags: line.col(G_DSTRONGS),
            dictionary: line.col(G_DICTIONARY),
            note: (!variants.is_empty()).then(|| tagnt_variant_note(variants)),
        }
    }

    /// For a word the base edition lacks, its reading there if a meaning variant gives it
    /// (`ἐπορεύθη` in TR, `ἐπορεύθησαν … in: NA28+NA27+WH`). The line's own reading becomes
    /// the note; the line's punctuation and paragraph mark are kept.
    fn base_variant(line: &'a Line<'a>) -> Option<Self> {
        let v = line
            .col(G_MEANING_VARIANTS)
            .split('¦')
            .filter_map(parse_tagnt_variant)
            .find(|v| in_base_edition(v.editions))?;
        let own = greek_word(line.col(G_GREEK));
        let trailing: String = {
            let kept = own.trim_end_matches(|c| !is_word_char(c));
            own[kept.len()..].to_string()
        };
        let mut greek = v.greek.to_string();
        if !greek.ends_with(|c| !is_word_char(c)) {
            greek.push_str(&trailing);
        }
        let same_lemma = line.col(G_DSTRONGS).split('=').next() == v.tags.split('=').next();
        let names: Vec<&str> = edition_names(line.col(G_EDITIONS)).collect();
        Some(Reading {
            greek,
            english: v.english,
            tags: v.tags,
            dictionary: if same_lemma {
                line.col(G_DICTIONARY)
            } else {
                ""
            },
            note: Some(format!(
                "Variant: {}: {} “{}”",
                names.join(", "),
                own.replace('¶', "").trim_end_matches(|c| !is_word_char(c)),
                plain_english(line.col(G_ENGLISH))
            )),
        })
    }
}

/// Words other editions add, noted where they would stand.
#[derive(Default)]
struct AddedRun {
    greek: Vec<String>,
    english: Vec<String>,
    editions: Vec<String>,
}

impl AddedRun {
    fn push(&mut self, line: &Line) {
        self.greek
            .push(greek_word(line.col(G_GREEK)).replace('¶', "").trim().to_string());
        let english = plain_english(line.col(G_ENGLISH));
        if !english.is_empty() {
            self.english.push(english.to_string());
        }
        for name in edition_names(line.col(G_EDITIONS)) {
            if !self.editions.iter().any(|e| e == name) {
                self.editions.push(name.to_string());
            }
        }
    }

    fn text(&self, whole_verse: Option<Verse>) -> String {
        let greek = self.greek.join(" ");
        let english = self.english.join(" ");
        let editions = self.editions.join(", ");
        let head = match whole_verse {
            Some(v) => format!(
                "Verse {}:{} is not in {BASE_EDITION}. {editions} have:",
                v.chapter, v.verse
            ),
            None => format!("Not in {BASE_EDITION}. {editions} add:"),
        };
        if english.is_empty() {
            format!("{head} {greek}")
        } else {
            format!("{head} {greek} “{english}”")
        }
    }
}

/// Emits TAGNT verses.
struct TagntBook {
    em: BookEmitter,
}

impl TagntBook {
    fn verse(&mut self, verse: Verse, lines: &[Line], ctx: &mut Ctx) {
        let mut run = AddedRun::default();
        let mut any_word = false;
        for line in lines {
            if in_base_edition(line.col(G_EDITIONS)) {
                if !run.greek.is_empty() {
                    self.em.enter(verse);
                    self.em.note(run.text(None), ctx);
                    run = AddedRun::default();
                }
                self.word(verse, &Reading::of(line), line.kind, ctx);
                any_word = true;
            } else if let Some(reading) = Reading::base_variant(line) {
                self.word(verse, &reading, line.kind, ctx);
                any_word = true;
            } else if line.kind.contains(['K', 'O']) {
                // Words other editions add: noted when they change the translation (an
                // upper-case K or O in the word type).
                run.push(line);
            }
        }
        if !run.greek.is_empty() {
            if any_word {
                self.em.note(run.text(None), ctx);
            } else {
                self.em.note(run.text(Some(verse)), ctx);
            }
        }
    }

    fn word(&mut self, verse: Verse, reading: &Reading, kind: &str, ctx: &mut Ctx) {
        let at = format!(
            "{} {}:{} ({kind})",
            verse.book.usfm(),
            verse.chapter,
            verse.verse
        );
        let paragraph = reading.greek.contains('¶');
        let greek = reading.greek.replace('¶', "");
        let start = self.em.begin_word(verse);
        self.em.buf.push_text(greek.trim());
        let end = self.em.buf.pos();
        let text = &self.em.buf.text[start..end];
        let lead = text.len() - text.trim_start_matches(|c| !is_word_char(c)).len();
        let trail = text.len() - text.trim_end_matches(|c| !is_word_char(c)).len();

        let tags: Vec<&str> = reading.tags.split(" + ").collect();
        let dictionary: Vec<&str> = reading.dictionary.split(" + ").collect();
        let single = tags.len() == 1;
        let mut parts = Vec::with_capacity(tags.len());
        for (i, tag) in tags.iter().enumerate() {
            let (strongs, morph) = tag.split_once('=').unwrap_or((tag, ""));
            let Some(n) = parse_strongs(strongs, &at, ctx) else {
                continue;
            };
            let mut part = MorphemePart::new(n);
            part.morph = some_text(morph);
            let (lemma, gloss) = dictionary
                .get(i)
                .and_then(|d| d.split_once('='))
                .unwrap_or(("", ""));
            part.lemma = lemma.split(',').next().and_then(some_text);
            part.gloss = if single {
                some_text(plain_english(reading.english))
            } else {
                some_text(gloss)
            };
            parts.push(part);
        }
        if start + lead < end - trail {
            self.em.tagged_word(start + lead..end - trail, parts, ctx);
        }
        if let Some(note) = &reading.note {
            self.em.note(note.clone(), ctx);
        }
        if paragraph {
            self.em.paragraph_mark();
        }
    }
}

// ---------------------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------------------

impl StepbibleImport {
    pub fn new(options: StepbibleOptions) -> Self {
        Self {
            options,
            files: Vec::new(),
        }
    }

    /// Add one source file; `name` is its file name, for the report.
    pub fn add_file(&mut self, name: &str, text: String) {
        self.files.push((name.to_string(), text));
    }

    /// Parse every file and build the Resource.
    pub fn finish(self) -> Result<(Resource, ImportReport), ImportError> {
        let o = &self.options;
        let kind = o.text;
        let mut ctx = Ctx::default();

        // Word lines by Bible book, in file order within each book.
        let mut books: BTreeMap<Book, Vec<Line>> = BTreeMap::new();
        for (name, text) in &self.files {
            for (n, raw) in text.lines().enumerate() {
                let first = raw.split('\t').next().unwrap_or("");
                if !first.starts_with(|c: char| c.is_ascii_alphanumeric()) {
                    continue;
                }
                let Some(r) = parse_word_ref(first) else {
                    if first.contains('#') && first.contains('=') && first.contains('.') {
                        ctx.warnings.push(format!(
                            "{name} line {}: unreadable reference `{first}`",
                            n + 1
                        ));
                    }
                    continue;
                };
                books.entry(r.book).or_default().push(Line {
                    verse: Verse::new(r.book, r.own.0, r.own.1),
                    english_verse: r.english.1,
                    kind: r.kind,
                    cols: raw.split('\t').collect(),
                });
            }
        }
        if books.is_empty() {
            return Err(ImportError::NoBooks);
        }

        let resource_id = if o.resource_id.is_empty() {
            slugify(&o.source_file)
        } else {
            o.resource_id.clone()
        };
        if !is_valid_resource_id(&resource_id) {
            return Err(ImportError::InvalidResourceId(resource_id));
        }
        let title = if o.title.is_empty() {
            resource_id.clone()
        } else {
            o.title.clone()
        };
        let abbreviation = if o.abbreviation.is_empty() {
            resource_id.to_ascii_uppercase().chars().take(8).collect()
        } else {
            o.abbreviation.clone()
        };
        let language = if o.language.is_empty() {
            kind.default_language().to_string()
        } else {
            o.language.clone()
        };
        let mut manifest = Manifest::new(
            &resource_id,
            &title,
            abbreviation,
            ResourceType::Bible,
            language,
        );
        manifest.direction = kind.direction();
        manifest.key_schemes = vec![KeyScheme::Passage];
        manifest.license = LICENSE_UNKNOWN.to_string();
        manifest.origin = Origin::Imported;
        manifest.source_format = kind.source_format().into();
        manifest.source_file = (!o.source_file.is_empty()).then(|| o.source_file.clone());
        manifest.import_tool_version = o.tool_version.clone();
        manifest.format_version = FORMAT_VERSION;

        let mut builder = ResourceBuilder::new(manifest);
        let mut report = ImportReport {
            resource_id,
            title,
            source_format: kind.source_format().into(),
            source_files: self.files.iter().map(|(n, _)| n.clone()).collect(),
            license_missing: true,
            ..ImportReport::default()
        };
        let mut facts = Vec::new();
        for (book, lines) in &books {
            let out = match kind {
                StepbibleText::Tahot => {
                    let mut b = TahotBook {
                        em: BookEmitter::new(),
                        run: None,
                    };
                    for line in lines {
                        b.word(line, &mut ctx);
                    }
                    b.em.finish()
                }
                StepbibleText::Tagnt => {
                    let mut b = TagntBook {
                        em: BookEmitter::new(),
                    };
                    let mut start = 0;
                    while start < lines.len() {
                        let verse = lines[start].verse;
                        let len = lines[start..]
                            .iter()
                            .position(|l| l.verse != verse)
                            .unwrap_or(lines.len() - start);
                        b.verse(verse, &lines[start..start + len], &mut ctx);
                        start += len;
                    }
                    b.em.finish()
                }
            };
            report.books.push(BookReport {
                code: book.usfm().to_string(),
                book: Some(*book),
                chapters: out.chapters,
                verses: out.verses.len(),
            });
            report.verses += out.verses.len();
            let unit = UnitId::book(*book);
            for (range, verse) in &out.verses {
                let words = out.text.get(range.clone()).map_or(0, |t| tokenize(t).len());
                facts.push(VerseFacts {
                    verse: *verse,
                    last: verse.verse,
                    words: u32::try_from(words).unwrap_or(u32::MAX),
                });
            }
            builder.add_unit(unit.clone(), UnitKind::BibleBook, None, out.text);
            for (range, span) in out.spans {
                builder.add_span(&unit, range, span);
            }
            for (range, verse) in out.verses {
                builder.add_verse(&unit, range, verse, verse.verse);
            }
        }
        report.notes = ctx.notes.len();
        report.tagged_words = ctx.tagged_words;
        for note in ctx.notes {
            let mut buf = TextBuf::new();
            buf.push_text(&note.text);
            builder.add_unit(note.id, UnitKind::Note, None, buf.text);
        }
        let outcome = build_verse_map(&facts);
        report.verse_map = Some(VerseMapCounts {
            unmapped: outcome.map.unmapped().count(),
            absent: outcome.map.absent().len(),
        });
        report.traditions = outcome.traditions;
        builder.set_verse_map(outcome.map);
        let resource = builder.build().map_err(ImportError::Invalid)?;
        report.warnings = ctx.warnings;
        Ok((resource, report))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_references() {
        let r = parse_word_ref("Gen.31.55(32.1)#01=L").unwrap();
        assert_eq!(
            (r.book, r.english, r.own, r.kind),
            (Book::Genesis, (31, 55), (32, 1), "L")
        );
        let r = parse_word_ref("Act.2.11[2.10]#03=N(k)O").unwrap();
        assert_eq!((r.english, r.own, r.kind), ((2, 11), (2, 11), "N(k)O"));
        let r = parse_word_ref("Jhn.7.53{8.1}#01=KO").unwrap();
        assert_eq!(r.own, (7, 53));
        let r = parse_word_ref("Psa.3.0(3.1)#01=L").unwrap();
        assert_eq!((r.english, r.own), ((3, 0), (3, 1)));
        assert!(parse_word_ref("# Gen.1.1").is_none());
        assert!(parse_word_ref("Eng (Heb) Ref & Type").is_none());
        assert!(parse_word_ref("Xyz.1.1#01=L").is_none());
    }

    #[test]
    fn variant_notes() {
        assert_eq!(
            tahot_variant_note(
                "K= 'o.ho.Lo/h (אָהֳלֹ/ה\\׃) \"tent/ his\" (H0168G/H9023\\H9016=HNcbsc/Sp3ms)"
            ),
            "Ketiv (written text): אָהֳלֹה׃ “tent his”"
        );
        assert_eq!(
            tahot_variant_note("AH= x (חֶ֖מֶר) \"wine\" (H2561=HNcmsa)"),
            "Aleppo Codex, Ben Chayyim edition: חֶ֖מֶר “wine”"
        );
        assert_eq!(tahot_variant_note(" odd"), "Variant: odd");
        assert_eq!(
            spelling_ketiv("L= הַוְצֵ֣א ¦ ;K= הוֹצֵא").as_deref(),
            Some("הוֹצֵא")
        );
        assert_eq!(spelling_ketiv("L= בָּנָ֖/ו ¦ ;"), None);
        assert_eq!(
            tagnt_variant_note(
                "ἐπηρώτων (t=epērōtōn) were asking - G1905=V-IAI-3P in: TR+Byz"
            ),
            "Variant: TR, Byz: ἐπηρώτων “were asking”"
        );
        assert_eq!(
            tagnt_variant_note(
                "a (t=a) x - G1=N in: TR ¦ b (o=b) y - G2=N in: Byz»1"
            ),
            "Variants: TR: a “x”; Byz: b “y”"
        );
    }

    #[test]
    fn base_edition() {
        assert!(in_base_edition("NA28+NA27+Tyn"));
        assert!(in_base_edition("NA28»1+NA27»1+Tyn"));
        assert!(in_base_edition("NA28«3+NA27«3"));
        assert!(!in_base_edition("TR+Byz"));
        assert!(!in_base_edition("NA28+TR"));
        assert_eq!(plain_english("{8.1} And"), "And");
    }
}
