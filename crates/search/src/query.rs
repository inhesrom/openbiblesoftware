//! The search grammar (#16): one syntax for Bible, Library and Notes searches.
//!
//! | Form | Meaning |
//! |---|---|
//! | `love neighbour` | all words (implicit AND) |
//! | `"love thy neighbour"` | phrase |
//! | `faith OR hope`, `faith -works`, `faith NOT works`, `(faith OR hope) love` | Boolean; operators are uppercase, `-` is NOT |
//! | `lov*` | prefix |
//! | `strong:G26`, `G26`, `H1254A` | Strong's number; extended matches exactly, base matches all homographs |
//! | `lemma:ἀγάπη`, `lemma:agape` | Lemma, in original script or transliteration |
//! | `@verb aorist imperative`, `morph:genitive` | Morphology criteria that one Tagged word meets (#145) |
//! | `strong:G26 @noun genitive`, `lemma:ἀγαπάω @verb aorist` | The same word has that Strong's number or Lemma too |
//!
//! [`parse`] is lenient: unclosed quotes and parentheses are closed, stray `)` and dangling
//! operators are ignored. Words go through the shared normalizer, so search is case- and
//! diacritic-insensitive. [`compile`] turns the [`Query`] into an FTS5 MATCH expression over
//! the `text`, `strong` and `lemma` columns in which every term is a quoted string: nothing
//! the user types ever reaches FTS5 as syntax.
//!
//! Morphology criteria ([`crate::morph`]) start at `@` (or `morph:`) and take the words after
//! it while they name criteria; they all hold for one morpheme part, and a Strong's number or
//! Lemma directly before the `@` (no `AND` or parenthesis between them) is that part's too.
//! Which codes meet them depends on the Search index, so such a query compiles to
//! [`Compiled::Morph`], which each index fills in from its own codes ([`MorphQuery::expand`]).

use std::fmt;
use std::ops::Range;

use std::cell::RefCell;

use obs_model::tokenize;
use obs_scripture::{StrongsLanguage, StrongsNumber};

use crate::morph::{MorphCriteria, MorphCriterion};

/// Which indexed column a term searches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// Normalized surface words.
    Text,
    /// Strong's numbers, both extended (`h1254a`) and base (`h1254`) forms.
    Strong,
    /// Normalized Lemmas.
    Lemma,
    /// Each morpheme part of a Tagged word in turn: its Strong's numbers (extended, then
    /// base), its Morphology code's token, then its Lemma's words, so that a phrase finds
    /// them in one part.
    Morph,
}

impl Field {
    pub fn column(self) -> &'static str {
        match self {
            Field::Text => "text",
            Field::Strong => "strong",
            Field::Lemma => "lemma",
            Field::Morph => "morph",
        }
    }
}

/// A parsed query. Spans are byte ranges in the query text, for inline errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Query {
    /// One normalized word; `prefix` for a trailing `*`.
    Word {
        word: String,
        prefix: bool,
        span: Range<usize>,
    },
    /// Normalized words in order; `prefix` makes the last one a prefix.
    Phrase {
        words: Vec<String>,
        prefix: bool,
        span: Range<usize>,
    },
    Strong {
        number: StrongsNumber,
        span: Range<usize>,
    },
    /// A Lemma as typed (original script or transliteration).
    Lemma {
        text: String,
        span: Range<usize>,
    },
    /// One morpheme part that meets Morphology criteria, and has a Strong's number or Lemma
    /// when one came directly before the `@`.
    Morph {
        strong: Option<StrongsNumber>,
        lemma: Option<String>,
        criteria: MorphCriteria,
        span: Range<usize>,
    },
    And(Vec<Query>),
    Or(Vec<Query>),
    Not {
        query: Box<Query>,
        span: Range<usize>,
    },
}

impl Query {
    /// The query text this node came from.
    pub fn span(&self) -> Range<usize> {
        match self {
            Query::Word { span, .. }
            | Query::Phrase { span, .. }
            | Query::Strong { span, .. }
            | Query::Lemma { span, .. }
            | Query::Morph { span, .. }
            | Query::Not { span, .. } => span.clone(),
            Query::And(q) | Query::Or(q) => {
                let start = q.first().map_or(0, |q| q.span().start);
                let end = q.last().map_or(0, |q| q.span().end);
                start..end
            }
        }
    }
}

/// What is wrong with a query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryErrorKind {
    /// Nothing to search for.
    Empty,
    /// `strong:` followed by something that is not a Strong's number.
    BadStrongs(String),
    /// `foo:` where `foo` is not `text`, `strong` or `lemma`.
    UnknownField(String),
    /// `strong:` or `lemma:` with nothing after it.
    EmptyField(&'static str),
    /// Only excluded terms (`-works`): a search needs something that must match.
    OnlyExcluded,
    /// An excluded term as an alternative (`faith OR -works`).
    ExcludedAlternative,
    /// `@` or `morph:` with nothing after it.
    EmptyMorph,
    /// A word after `@` that names no Morphology criterion.
    UnknownMorph(String),
    /// Morphology criteria in a Notes search: Notes have no Tagged words.
    MorphInNotes,
}

impl fmt::Display for QueryErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueryErrorKind::Empty => write!(f, "type something to search for"),
            QueryErrorKind::BadStrongs(s) => {
                write!(f, "“{s}” is not a Strong's number (try G5485 or H1254A)")
            }
            QueryErrorKind::UnknownField(s) => {
                write!(f, "“{s}:” is not a search field (use strong: or lemma:)")
            }
            QueryErrorKind::EmptyField(s) => write!(f, "{s}: needs something after it"),
            QueryErrorKind::OnlyExcluded => {
                write!(
                    f,
                    "add a word that must match; excluded words alone match nothing"
                )
            }
            QueryErrorKind::ExcludedAlternative => {
                write!(f, "an excluded word cannot be an OR alternative")
            }
            QueryErrorKind::EmptyMorph => {
                write!(f, "@ needs Morphology after it, such as @verb aorist")
            }
            QueryErrorKind::UnknownMorph(s) => write!(
                f,
                "“{s}” is not Morphology (try verb, aorist, imperative, genitive or Qal)"
            ),
            QueryErrorKind::MorphInNotes => {
                write!(
                    f,
                    "Morphology (@…) searches the Tagged words of Bibles, not Notes"
                )
            }
        }
    }
}

/// A query error and where in the query text it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryError {
    pub kind: QueryErrorKind,
    pub span: Range<usize>,
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

impl std::error::Error for QueryError {}

fn error(kind: QueryErrorKind, span: Range<usize>) -> QueryError {
    QueryError { kind, span }
}

// ------------------------------------------------------------------------------- lexer

#[derive(Debug, Clone, PartialEq)]
enum Lex {
    Open,
    Close,
    Minus,
    /// A quoted phrase (closed automatically at the end of the query); `true` when a `*`
    /// follows the closing quote.
    Quoted(String, bool),
    /// A run of other characters.
    Raw(String),
    /// `name:` directly followed by a value.
    Field(String),
}

fn lex(input: &str) -> Vec<(Lex, Range<usize>)> {
    let mut out = Vec::new();
    let mut it = input.char_indices().peekable();
    // Whether the next character starts a word (after whitespace, `(` or the start).
    let mut at_start = true;
    while let Some(&(i, c)) = it.peek() {
        match c {
            _ if c.is_whitespace() => {
                it.next();
                at_start = true;
            }
            '(' => {
                it.next();
                out.push((Lex::Open, i..i + 1));
                at_start = true;
            }
            ')' => {
                it.next();
                out.push((Lex::Close, i..i + 1));
                at_start = true;
            }
            '-' if at_start => {
                it.next();
                // Only a NOT when directly followed by a term.
                let next = it.peek().map(|&(_, n)| n);
                if next.is_some_and(|n| !n.is_whitespace() && n != ')') {
                    out.push((Lex::Minus, i..i + 1));
                }
                at_start = true;
            }
            '"' => {
                it.next();
                let start = i + 1;
                let mut end = input.len();
                let mut close = input.len();
                for (j, d) in it.by_ref() {
                    if d == '"' {
                        end = j;
                        close = j + 1;
                        break;
                    }
                }
                let mut prefix = false;
                if close < input.len() && input[close..].starts_with('*') {
                    while it.peek().is_some_and(|&(_, d)| d == '*') {
                        it.next();
                        close += 1;
                    }
                    prefix = true;
                }
                out.push((Lex::Quoted(input[start..end].to_string(), prefix), i..close));
                at_start = false;
            }
            _ => {
                let start = i;
                let mut end = input.len();
                while let Some(&(j, d)) = it.peek() {
                    if d.is_whitespace() || d == '(' || d == ')' || d == '"' {
                        end = j;
                        break;
                    }
                    it.next();
                    if d == ':' {
                        let name = &input[start..j];
                        let is_name = !name.is_empty() && name.chars().all(char::is_alphabetic);
                        let next = it.peek().map(|&(_, n)| n);
                        let has_value = next.is_some_and(|n| !n.is_whitespace() && n != ')');
                        if is_name && has_value {
                            out.push((Lex::Field(name.to_string()), start..j + 1));
                            end = usize::MAX;
                            break;
                        }
                        if is_name && !has_value {
                            // `strong:` with nothing after it.
                            out.push((Lex::Field(name.to_string()), start..j + 1));
                            end = usize::MAX;
                            break;
                        }
                    }
                }
                if end != usize::MAX {
                    out.push((Lex::Raw(input[start..end].to_string()), start..end));
                }
                at_start = false;
            }
        }
    }
    out
}

// ------------------------------------------------------------------------------ parser

struct Parser {
    toks: Vec<(Lex, Range<usize>)>,
    pos: usize,
}

fn is_op(l: &Lex, op: &str) -> bool {
    matches!(l, Lex::Raw(r) if r == op)
}

impl Parser {
    fn peek(&self) -> Option<&Lex> {
        self.toks.get(self.pos).map(|(l, _)| l)
    }

    fn next(&mut self) -> Option<(Lex, Range<usize>)> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    /// or := and ("OR" and)*
    fn or(&mut self, depth: usize) -> Result<Option<Query>, QueryError> {
        let mut alts = Vec::new();
        loop {
            if let Some(q) = self.and(depth)? {
                alts.push(q);
            }
            if self.peek().is_some_and(|l| is_op(l, "OR")) {
                self.pos += 1;
                continue;
            }
            break;
        }
        Ok(match alts.len() {
            0 => None,
            1 => alts.pop(),
            _ => Some(Query::Or(alts)),
        })
    }

    /// and := unary (("AND")? unary)*, up to `)`, `OR` or the end.
    fn and(&mut self, depth: usize) -> Result<Option<Query>, QueryError> {
        let mut all = Vec::new();
        // Whether the last term was a bare Strong's number or Lemma with nothing (no `AND`, no
        // group boundary) between it and the next token, so that an `@` there describes it.
        let mut adjacent = false;
        while let Some(l) = self.peek() {
            if is_op(l, "OR") {
                break;
            }
            if matches!(l, Lex::Close) {
                if depth > 0 {
                    break;
                }
                // A stray `)`: ignore it.
                self.pos += 1;
                adjacent = false;
                continue;
            }
            if is_op(l, "AND") {
                self.pos += 1;
                adjacent = false;
                continue;
            }
            let group = matches!(l, Lex::Open);
            let was_adjacent = std::mem::replace(&mut adjacent, false);
            match self.unary(depth)? {
                // `a (b c)` is `a b c`.
                Some(Query::And(inner)) => all.extend(inner),
                Some(q) => {
                    adjacent = !group && matches!(q, Query::Strong { .. } | Query::Lemma { .. });
                    let q = if was_adjacent {
                        attach_morph(&mut all, q)
                    } else {
                        Some(q)
                    };
                    all.extend(q);
                }
                None => {}
            }
        }
        Ok(match all.len() {
            0 => None,
            1 => all.pop(),
            _ => Some(Query::And(all)),
        })
    }

    /// unary := ("-" | "NOT") unary | primary
    fn unary(&mut self, depth: usize) -> Result<Option<Query>, QueryError> {
        let negate = match self.peek() {
            Some(Lex::Minus) => true,
            Some(l) => is_op(l, "NOT"),
            None => false,
        };
        if negate {
            let Some((_, op)) = self.next() else {
                return Ok(None);
            };
            // A dangling NOT (`faith NOT`, `NOT OR`) is ignored.
            let next_ok = self
                .peek()
                .is_some_and(|l| !matches!(l, Lex::Close) && !is_op(l, "OR") && !is_op(l, "AND"));
            if !next_ok {
                return Ok(None);
            }
            return Ok(self.unary(depth)?.map(|q| {
                let span = op.start..q.span().end;
                match q {
                    // `--x` and `NOT -x` cancel out.
                    Query::Not { query, .. } => *query,
                    q => Query::Not {
                        query: Box::new(q),
                        span,
                    },
                }
            }));
        }
        self.primary(depth)
    }

    fn primary(&mut self, depth: usize) -> Result<Option<Query>, QueryError> {
        let Some((lex, span)) = self.next() else {
            return Ok(None);
        };
        match lex {
            Lex::Open => {
                let inner = self.or(depth + 1)?;
                // Close it if the user did; otherwise it closes at the end.
                if matches!(self.peek(), Some(Lex::Close)) {
                    self.pos += 1;
                }
                Ok(inner)
            }
            Lex::Close | Lex::Minus => Ok(None),
            Lex::Quoted(text, prefix) => Ok(words(&text, prefix, span)),
            Lex::Raw(raw) if raw.starts_with('@') => self.morph(&raw[1..], span),
            Lex::Raw(raw) => Ok(raw_term(&raw, span)),
            Lex::Field(name) => self.field(&name, span),
        }
    }

    fn field(&mut self, name: &str, name_span: Range<usize>) -> Result<Option<Query>, QueryError> {
        let field = match name.to_lowercase().as_str() {
            "strong" => Field::Strong,
            "lemma" => Field::Lemma,
            "text" => Field::Text,
            "morph" => Field::Morph,
            _ => {
                return Err(error(
                    QueryErrorKind::UnknownField(name.to_string()),
                    name_span,
                ));
            }
        };
        let value = match self.toks.get(self.pos) {
            Some((Lex::Raw(r), s)) if s.start == name_span.end => Some((r.clone(), s.clone())),
            Some((Lex::Quoted(r, _), s)) if s.start == name_span.end => {
                Some((r.clone(), s.clone()))
            }
            _ => None,
        };
        let Some((value, vspan)) = value else {
            let label = match field {
                Field::Strong => "strong",
                Field::Lemma => "lemma",
                Field::Text => "text",
                Field::Morph => return Err(error(QueryErrorKind::EmptyMorph, name_span)),
            };
            return Err(error(QueryErrorKind::EmptyField(label), name_span));
        };
        self.pos += 1;
        let span = name_span.start..vspan.end;
        match field {
            Field::Strong => {
                let number: StrongsNumber = value
                    .trim()
                    .parse()
                    .map_err(|_| error(QueryErrorKind::BadStrongs(value.clone()), vspan.clone()))?;
                Ok(Some(Query::Strong { number, span }))
            }
            Field::Lemma => {
                if value.trim().is_empty() {
                    return Err(error(QueryErrorKind::EmptyField("lemma"), span));
                }
                Ok(Some(Query::Lemma {
                    text: value.trim().to_string(),
                    span,
                }))
            }
            Field::Text => Ok(raw_term(&value, span)),
            Field::Morph => self.morph(&value, span),
        }
    }

    /// The token at `i` if it is a plain word that could name a Morphology criterion.
    fn criterion_word(&self, i: usize) -> Option<(String, Range<usize>)> {
        match self.toks.get(i) {
            Some((Lex::Raw(r), span))
                if !r.starts_with('@') && !["OR", "AND", "NOT"].contains(&r.as_str()) =>
            {
                Some((r.clone(), span.clone()))
            }
            _ => None,
        }
    }

    /// The criteria of `@verb aorist` (`first` is the text after the `@` or `morph:`, which
    /// must all be criteria), then the words after it while they name criteria. A value of
    /// two words may be written as two (`consecutive imperfect`) or with `-`.
    fn morph(&mut self, first: &str, span: Range<usize>) -> Result<Option<Query>, QueryError> {
        let words: Vec<&str> = first.split_whitespace().collect();
        if words.is_empty() {
            return Err(error(QueryErrorKind::EmptyMorph, span));
        }
        let mut criteria: Vec<MorphCriterion> = Vec::new();
        let mut end = span.end;
        let mut i = 0;
        while i < words.len() {
            let w = words[i];
            if let Some(n) = words.get(i + 1)
                && let Some(c) = MorphCriterion::from_word(&format!("{w}-{n}"))
            {
                criteria.push(c);
                i += 2;
                continue;
            }
            if i + 1 == words.len()
                && let Some((n, nspan)) = self.criterion_word(self.pos)
                && let Some(c) = MorphCriterion::from_word(&format!("{w}-{n}"))
            {
                criteria.push(c);
                end = nspan.end;
                self.pos += 1;
                break;
            }
            let c = MorphCriterion::from_word(w)
                .ok_or_else(|| error(QueryErrorKind::UnknownMorph(w.to_string()), span.clone()))?;
            criteria.push(c);
            i += 1;
        }
        while let Some((w, wspan)) = self.criterion_word(self.pos) {
            if let Some((n, nspan)) = self.criterion_word(self.pos + 1)
                && let Some(c) = MorphCriterion::from_word(&format!("{w}-{n}"))
            {
                criteria.push(c);
                end = nspan.end;
                self.pos += 2;
                continue;
            }
            let Some(c) = MorphCriterion::from_word(&w) else {
                break;
            };
            criteria.push(c);
            end = wspan.end;
            self.pos += 1;
        }
        Ok(Some(Query::Morph {
            strong: None,
            lemma: None,
            criteria: MorphCriteria::new(criteria),
            span: span.start..end,
        }))
    }
}

/// `@…` directly after a Strong's number or Lemma (the last of `all`) describes that same
/// word.
fn attach_morph(all: &mut [Query], q: Query) -> Option<Query> {
    let Query::Morph {
        strong: None,
        lemma: None,
        criteria,
        span,
    } = q
    else {
        return Some(q);
    };
    let morph = |strong, lemma, start| Query::Morph {
        strong,
        lemma,
        criteria: criteria.clone(),
        span: start..span.end,
    };
    let attached = match all.last() {
        Some(Query::Strong { number, span: s }) => morph(Some(*number), None, s.start),
        Some(Query::Lemma { text, span: s }) => morph(None, Some(text.clone()), s.start),
        _ => {
            return Some(Query::Morph {
                strong: None,
                lemma: None,
                criteria,
                span,
            });
        }
    };
    if let Some(last) = all.last_mut() {
        *last = attached;
    }
    None
}

/// A bare token shaped like a Strong's number: uppercase `G` or `H`, 1–5 digits, an optional
/// homograph letter.
fn bare_strongs(raw: &str) -> Option<StrongsNumber> {
    let mut chars = raw.chars();
    let lang = match chars.next()? {
        'G' => StrongsLanguage::Greek,
        'H' => StrongsLanguage::Hebrew,
        _ => return None,
    };
    let rest = chars.as_str();
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 || digits > 5 {
        return None;
    }
    let (num, suffix) = rest.split_at(digits);
    let homograph = match suffix.chars().collect::<Vec<_>>()[..] {
        [] => None,
        [c] if c.is_ascii_alphabetic() => Some(c),
        _ => return None,
    };
    StrongsNumber::new(lang, num.parse().ok()?, homograph)
}

fn raw_term(raw: &str, span: Range<usize>) -> Option<Query> {
    let trimmed = raw.trim_end_matches('*');
    let prefix = trimmed.len() < raw.len();
    if !prefix && let Some(number) = bare_strongs(trimmed) {
        return Some(Query::Strong { number, span });
    }
    words(trimmed, prefix, span)
}

/// Normalized words of `text`: one word, a phrase, or nothing (punctuation only).
fn words(text: &str, prefix: bool, span: Range<usize>) -> Option<Query> {
    let mut w: Vec<String> = tokenize(text).into_iter().map(|t| t.text).collect();
    match w.len() {
        0 => None,
        1 => w.pop().map(|word| Query::Word { word, prefix, span }),
        _ => Some(Query::Phrase {
            words: w,
            prefix,
            span,
        }),
    }
}

/// Parse a query leniently. Errors carry the byte range to underline.
pub fn parse(input: &str) -> Result<Query, QueryError> {
    let mut p = Parser {
        toks: lex(input),
        pos: 0,
    };
    let mut parts = Vec::new();
    while p.pos < p.toks.len() {
        if let Some(q) = p.or(0)? {
            parts.push(q);
        } else {
            // Leftovers such as a dangling `OR`.
            p.pos += 1;
        }
    }
    match parts.len() {
        0 => Err(error(QueryErrorKind::Empty, 0..input.len())),
        1 => parts
            .pop()
            .ok_or_else(|| error(QueryErrorKind::Empty, 0..input.len())),
        _ => Ok(Query::And(parts)),
    }
}

// ---------------------------------------------------------------------------- compiler

/// Finds Lemmas by transliteration (a Lexicon's transliteration column, #41).
pub trait LemmaLookup {
    /// Lemmas (original script) whose transliteration is `translit` (already normalized).
    fn lemmas_for_transliteration(&self, translit: &str) -> Vec<String>;
}

/// No Lexicon: transliterated `lemma:` terms match nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoLexicon;

impl LemmaLookup for NoLexicon {
    fn lemmas_for_transliteration(&self, _: &str) -> Vec<String> {
        Vec::new()
    }
}

impl<F: Fn(&str) -> Vec<String>> LemmaLookup for F {
    fn lemmas_for_transliteration(&self, translit: &str) -> Vec<String> {
        self(translit)
    }
}

/// Finds the Morphology codes of one Search index that meet some criteria.
pub trait MorphVocabulary {
    /// The index's tokens (in its `morph` column) for the codes that meet `criteria`.
    fn tokens(&self, criteria: &MorphCriteria) -> Vec<String>;
}

/// Stands in for every index's codes, to check a query for errors before it is searched.
struct AnyCode;

impl MorphVocabulary for AnyCode {
    fn tokens(&self, _: &MorphCriteria) -> Vec<String> {
        vec!["morph".into()]
    }
}

/// A query ready for FTS5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Compiled {
    /// An FTS5 MATCH expression.
    Match(String),
    /// A query with Morphology criteria: each Search index makes its MATCH expression from
    /// its own codes ([`MorphQuery::expand`]).
    Morph(MorphQuery),
    /// The query cannot match anything (e.g. an unknown transliteration); skip the search.
    Nothing,
}

/// A query with Morphology criteria, its transliterated Lemmas already looked up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MorphQuery {
    query: Query,
    /// Transliteration (normalized) → the Lemmas the Lexicons gave for it.
    lemmas: Vec<(String, Vec<String>)>,
}

impl MorphQuery {
    /// The MATCH expression for one Search index, given its codes.
    pub fn expand(&self, vocabulary: &dyn MorphVocabulary) -> Compiled {
        let lookup = |translit: &str| {
            self.lemmas
                .iter()
                .find(|(t, _)| t == translit)
                .map(|(_, l)| l.clone())
                .unwrap_or_default()
        };
        match compile_part(&self.query, &lookup, vocabulary, Target::Index) {
            Ok(Part::Atom(s) | Part::Group(s)) => Compiled::Match(s),
            Ok(Part::Nothing) | Err(_) => Compiled::Nothing,
        }
    }
}

/// A [`LemmaLookup`] that remembers what it was asked and answered.
struct Recording<'a> {
    inner: &'a dyn LemmaLookup,
    seen: RefCell<Vec<(String, Vec<String>)>>,
}

impl LemmaLookup for Recording<'_> {
    fn lemmas_for_transliteration(&self, translit: &str) -> Vec<String> {
        if let Some((_, l)) = self.seen.borrow().iter().find(|(t, _)| t == translit) {
            return l.clone();
        }
        let l = self.inner.lemmas_for_transliteration(translit);
        self.seen
            .borrow_mut()
            .push((translit.to_string(), l.clone()));
        l
    }
}

fn has_morph(q: &Query) -> bool {
    match q {
        Query::Morph { .. } => true,
        Query::And(all) | Query::Or(all) => all.iter().any(has_morph),
        Query::Not { query, .. } => has_morph(query),
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    /// A single term or phrase: never needs parentheses.
    Atom(String),
    /// Anything with an operator in it.
    Group(String),
    Nothing,
}

impl Part {
    fn wrapped(&self) -> Option<String> {
        match self {
            Part::Atom(s) => Some(s.clone()),
            Part::Group(s) => Some(format!("({s})")),
            Part::Nothing => None,
        }
    }
}

/// An FTS5 string literal: double quotes, with `"` doubled. Nothing inside is syntax.
fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// Which FTS5 table a query is compiled for: a Search index (columns `text`, `strong`,
/// `lemma`) or the Notes index (one column, `body`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Index,
    Notes,
}

impl Target {
    fn column(self, field: Field) -> &'static str {
        match self {
            Target::Index => field.column(),
            Target::Notes => "body",
        }
    }
}

fn term(target: Target, field: Field, words: &[String], prefix: bool) -> Part {
    let mut s = format!("{} : {}", target.column(field), quote(&words.join(" ")));
    if prefix {
        s.push_str(" *");
    }
    Part::Atom(s)
}

pub(crate) fn is_original_script(s: &str) -> bool {
    s.chars().any(|c| {
        matches!(c as u32, 0x0370..=0x03FF | 0x1F00..=0x1FFF | 0x0590..=0x05FF | 0xFB1D..=0xFB4F)
    })
}

fn words_of(s: &str) -> Vec<String> {
    tokenize(s).into_iter().map(|t| t.text).collect()
}

/// The Lemmas a `lemma:` term means, as normalized words.
fn lemma_alternatives(text: &str, lookup: &dyn LemmaLookup) -> Vec<Vec<String>> {
    let lemmas: Vec<Vec<String>> = if is_original_script(text) {
        vec![words_of(text)]
    } else {
        let translit = words_of(text).join(" ");
        let mut l: Vec<Vec<String>> = lookup
            .lemmas_for_transliteration(&translit)
            .iter()
            .map(|s| words_of(s))
            .collect();
        l.sort();
        l.dedup();
        l
    };
    lemmas.into_iter().filter(|w| !w.is_empty()).collect()
}

fn lemma_part(text: &str, lookup: &dyn LemmaLookup) -> Part {
    let alts: Vec<String> = lemma_alternatives(text, lookup)
        .iter()
        .filter_map(|w| term(Target::Index, Field::Lemma, w, false).wrapped())
        .collect();
    match alts.len() {
        0 => Part::Nothing,
        1 => alts.into_iter().next().map_or(Part::Nothing, Part::Atom),
        _ => Part::Group(alts.join(" OR ")),
    }
}

/// One morpheme part with the Morphology codes `vocabulary` finds, and the Strong's number
/// and Lemma given: a phrase per code (and Lemma) in the `morph` column, which lists each
/// part's Strong's numbers, code and Lemma in turn.
fn morph_part(
    strong: Option<StrongsNumber>,
    lemma: Option<&str>,
    criteria: &MorphCriteria,
    lookup: &dyn LemmaLookup,
    vocabulary: &dyn MorphVocabulary,
) -> Part {
    let lemmas: Vec<Vec<String>> = match lemma {
        Some(text) => {
            let l = lemma_alternatives(text, lookup);
            if l.is_empty() {
                return Part::Nothing;
            }
            l
        }
        None => vec![Vec::new()],
    };
    // An extended number is followed by its base form; a base number matches either.
    let mut head: Vec<String> = Vec::new();
    if let Some(n) = strong {
        head.push(n.to_string().to_lowercase());
        if n.is_extended() {
            head.push(n.base().to_string().to_lowercase());
        }
    }
    let mut alts = Vec::new();
    for token in vocabulary.tokens(criteria) {
        for lemma in &lemmas {
            let mut words = head.clone();
            words.push(token.clone());
            words.extend(lemma.iter().cloned());
            alts.extend(term(Target::Index, Field::Morph, &words, false).wrapped());
        }
    }
    match alts.len() {
        0 => Part::Nothing,
        1 => alts.into_iter().next().map_or(Part::Nothing, Part::Atom),
        _ => Part::Group(alts.join(" OR ")),
    }
}

fn compile_part(
    q: &Query,
    lookup: &dyn LemmaLookup,
    vocabulary: &dyn MorphVocabulary,
    target: Target,
) -> Result<Part, QueryError> {
    match q {
        Query::Word { word, prefix, .. } => Ok(term(
            target,
            Field::Text,
            std::slice::from_ref(word),
            *prefix,
        )),
        Query::Phrase { words, prefix, .. } => Ok(term(target, Field::Text, words, *prefix)),
        // A Note has no Strong's or Lemma column: the number or Lemma as written in it.
        Query::Strong { number, .. } if target == Target::Notes => Ok(term(
            target,
            Field::Text,
            &words_of(&number.to_string()),
            false,
        )),
        Query::Lemma { text, .. } if target == Target::Notes => {
            let words = words_of(text);
            Ok(if words.is_empty() {
                Part::Nothing
            } else {
                term(target, Field::Text, &words, false)
            })
        }
        Query::Strong { number, .. } => Ok(term(
            target,
            Field::Strong,
            &[number.to_string().to_lowercase()],
            false,
        )),
        Query::Lemma { text, .. } => Ok(lemma_part(text, lookup)),
        Query::Morph { span, .. } if target == Target::Notes => {
            Err(error(QueryErrorKind::MorphInNotes, span.clone()))
        }
        Query::Morph {
            strong,
            lemma,
            criteria,
            ..
        } => Ok(morph_part(
            *strong,
            lemma.as_deref(),
            criteria,
            lookup,
            vocabulary,
        )),
        Query::Not { span, .. } => Err(error(QueryErrorKind::OnlyExcluded, span.clone())),
        Query::Or(alts) => {
            let mut parts = Vec::new();
            for a in alts {
                if let Query::Not { span, .. } = a {
                    return Err(error(QueryErrorKind::ExcludedAlternative, span.clone()));
                }
                if let Some(s) = compile_part(a, lookup, vocabulary, target)?.wrapped() {
                    parts.push(s);
                }
            }
            Ok(match parts.len() {
                0 => Part::Nothing,
                1 => parts.into_iter().next().map_or(Part::Nothing, Part::Atom),
                _ => Part::Group(parts.join(" OR ")),
            })
        }
        Query::And(all) => {
            let mut must = Vec::new();
            let mut not = Vec::new();
            let mut nothing = false;
            for a in all {
                match a {
                    Query::Not { query, .. } => {
                        if let Some(s) = compile_part(query, lookup, vocabulary, target)?.wrapped()
                        {
                            not.push(s);
                        }
                    }
                    a => match compile_part(a, lookup, vocabulary, target)?.wrapped() {
                        Some(s) => must.push(s),
                        None => nothing = true,
                    },
                }
            }
            if must.is_empty() && !nothing {
                return Err(error(QueryErrorKind::OnlyExcluded, q.span()));
            }
            if nothing {
                return Ok(Part::Nothing);
            }
            let mut s = must.join(" AND ");
            let negated = !not.is_empty();
            if negated {
                if must.len() > 1 {
                    s = format!("({s})");
                }
                for n in not {
                    s.push_str(" NOT ");
                    s.push_str(&n);
                }
            }
            Ok(if must.len() == 1 && !negated {
                Part::Atom(s)
            } else {
                Part::Group(s)
            })
        }
    }
}

/// Compile a parsed query to an FTS5 MATCH expression, or (with Morphology criteria) to a
/// [`MorphQuery`] that each index expands. Errors show either way.
pub fn compile(q: &Query, lemmas: &dyn LemmaLookup) -> Result<Compiled, QueryError> {
    if has_morph(q) {
        let recording = Recording {
            inner: lemmas,
            seen: RefCell::new(Vec::new()),
        };
        compile_part(q, &recording, &AnyCode, Target::Index)?;
        return Ok(Compiled::Morph(MorphQuery {
            query: q.clone(),
            lemmas: recording.seen.into_inner(),
        }));
    }
    Ok(match compile_part(q, lemmas, &AnyCode, Target::Index)? {
        Part::Atom(s) | Part::Group(s) => Compiled::Match(s),
        Part::Nothing => Compiled::Nothing,
    })
}

/// Compile a parsed query for the Notes index (#16: one grammar for Bible, Library and
/// Notes). Note bodies have one column, `body`, of normalized words: words, phrases,
/// prefixes and boolean operators work as everywhere; a Strong's number or a Lemma matches
/// itself written in the Note.
pub fn compile_notes(q: &Query) -> Result<Compiled, QueryError> {
    Ok(
        match compile_part(q, &NoLexicon, &AnyCode, Target::Notes)? {
            Part::Atom(s) | Part::Group(s) => Compiled::Match(s),
            Part::Nothing => Compiled::Nothing,
        },
    )
}

/// Parse and compile in one step.
pub fn to_fts5(input: &str, lemmas: &dyn LemmaLookup) -> Result<Compiled, QueryError> {
    compile(&parse(input)?, lemmas)
}
