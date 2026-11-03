//! Lexicon converters (#41): source files → [`ResourceBuilder`] → a Lexicon [`Resource`] keyed
//! by Strong's number and Lemma, plus an [`ImportReport`].
//!
//! | source | function | one unit per | unit id |
//! |---|---|---|---|
//! | STEPBible TBESH / TBESG (TSV) | [`import_tbes`] | row | dStrong number, `H1254A` |
//! | Strong's Hebrew (openscriptures/strongs OSIS XML) | [`import_strongs_hebrew`] | entry | `H1254` |
//! | Strong's Greek (openscriptures/strongs XML) | [`import_strongs_greek`] | entry | `G5485` |
//! | BDB (OSHB `BrownDriverBriggs.xml` + `LexicalIndex.xml`) | [`import_bdb`] | entry | BDB id, `b.cw.aa` |
//! | Abbott-Smith (TEI) | [`import_abbott_smith`] | entry | Lemma, `χάρις` |
//!
//! Every entry gets a Strong's number Key for each number it is for (extended numbers such as
//! `H1254A` as they are), a secondary Key for the base number of each extended one (`H1254`),
//! and a Lemma Key for each headword form. `ResourceBuilder` adds the transliteration index.
//!
//! Entry markup maps onto the span vocabulary the same way in every converter: one `para`
//! (prose) per block, `bold` / `italic` for emphasis, `scripture_ref` for a Bible reference the
//! source marks up (read in the Standard versification, ADR 0002), and a one-part
//! `tagged_word` for a Strong's cross-link, so a link to `G5463` behaves like any other word
//! carrying a Strong's number. References outside the 66-book canon stay plain text and are
//! counted in the report.

mod abbott_smith;
mod bdb;
mod strongs;
mod tbes;

use std::collections::{BTreeSet, HashSet};
use std::fmt;

use obs_model::{
    FORMAT_VERSION, Key, KeyScheme, LICENSE_UNKNOWN, Manifest, MorphemePart, Origin, ParaStyle,
    Resource, ResourceBuilder, ResourceType, SpanKind, UnitId, UnitKind, ValidationErrors,
    is_valid_resource_id,
};
use obs_scripture::{PassageList, Standard, StrongsNumber, parse_reference};
use unicode_normalization::UnicodeNormalization;

use crate::report::ImportReport;
use crate::text_buf::TextBuf;

pub use abbott_smith::import_abbott_smith;
pub use bdb::import_bdb;
pub use strongs::{import_strongs_greek, import_strongs_hebrew};
pub use tbes::import_tbes;

/// Identity and metadata for the Lexicon a converter creates.
#[derive(Debug, Clone, Default)]
pub struct LexiconOptions {
    pub resource_id: String,
    pub title: String,
    pub abbreviation: String,
    /// File name shown in About and the report.
    pub source_file: String,
    /// Version of the tool doing the conversion, recorded in the manifest.
    pub tool_version: String,
}

/// Why a conversion failed outright.
#[derive(Debug)]
pub enum LexiconError {
    InvalidResourceId(String),
    /// A source file could not be read as the expected format.
    Parse {
        file: String,
        message: String,
    },
    /// A column asked to be skipped is not in the source, or may not be skipped.
    UnknownColumn(String),
    NoEntries,
    Invalid(ValidationErrors),
}

impl fmt::Display for LexiconError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LexiconError::InvalidResourceId(id) => write!(
                f,
                "resource id `{id}` must be 1-64 chars of a-z, 0-9 and `-`"
            ),
            LexiconError::Parse { file, message } => write!(f, "{file}: {message}"),
            LexiconError::UnknownColumn(c) => {
                write!(f, "column `{c}` is not one the source has and may skip")
            }
            LexiconError::NoEntries => f.write_str("no lexicon entries found"),
            LexiconError::Invalid(e) => write!(f, "converted lexicon is invalid: {e}"),
        }
    }
}

impl std::error::Error for LexiconError {}

fn parse_xml<'a>(file: &str, xml: &'a str) -> Result<roxmltree::Document<'a>, LexiconError> {
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..roxmltree::ParsingOptions::default()
    };
    roxmltree::Document::parse_with_options(xml, options).map_err(|e| LexiconError::Parse {
        file: file.to_string(),
        message: e.to_string(),
    })
}

/// Read a Bible reference written as OSIS (`Gen.1.1`, `Gen.1.1-Gen.1.5`) or in STEPBible's
/// form (`1Co.3.2; 10.13`, `Rev.1.1, 4, 9`) in the Standard versification. `None` for books
/// outside the 66-book canon (`Sir.21.16`), LXX numbering (`LXX:Ps.3.1`) and anything that
/// does not parse.
pub(crate) fn parse_osis_ref(osis: &str) -> Option<PassageList> {
    let t = osis
        .trim()
        .trim_end_matches(|c: char| matches!(c, '.' | ',' | ';') || c.is_whitespace());
    if t.is_empty() || t.contains(':') {
        return None;
    }
    // `Book.C.V` → `Book C.V`, which the reference parser reads as chapter.verse.
    let chars: Vec<char> = t.chars().collect();
    let text: String = chars
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            let after_letter = i > 0 && chars[i - 1].is_ascii_alphabetic();
            let before_digit = chars.get(i + 1).is_some_and(char::is_ascii_digit);
            if c == '.' && after_letter && before_digit {
                ' '
            } else {
                c
            }
        })
        .collect();
    parse_reference(&text, &Standard, None)
        .ok()
        .filter(|list| !list.is_empty())
}

/// One Lexicon entry under construction: its text in blocks, spans, and Keys.
pub(crate) struct Entry {
    id: String,
    title: String,
    buf: TextBuf,
    /// Start of the open block, if one has text.
    block_start: Option<usize>,
    strongs: Vec<StrongsNumber>,
    lemmas: Vec<String>,
    dropped: Vec<String>,
    links: usize,
    refs: usize,
}

impl Entry {
    pub(crate) fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into().trim().to_string(),
            buf: TextBuf::new(),
            block_start: None,
            strongs: Vec::new(),
            lemmas: Vec::new(),
            dropped: Vec::new(),
            links: 0,
            refs: 0,
        }
    }

    fn ensure_block(&mut self) {
        if self.block_start.is_none() {
            self.block_start = Some(self.buf.start_block());
        }
    }

    /// End the open block; the next text starts a new one.
    pub(crate) fn block(&mut self) {
        if let Some(start) = self.block_start.take() {
            let end = self.buf.pos();
            if end > start {
                self.buf.add_span(
                    start..end,
                    SpanKind::Para {
                        style: ParaStyle::Prose,
                    },
                );
            } else if start > 0 {
                // Nothing was written: take back the block separator.
                self.buf.text.truncate(start - 1);
            }
        }
    }

    /// Append text; whitespace runs collapse to one space.
    pub(crate) fn text(&mut self, s: &str) {
        if self.block_start.is_none() && s.trim().is_empty() {
            return;
        }
        self.ensure_block();
        self.buf.push_text(s);
    }

    /// Where a span starting now begins (after any pending space).
    pub(crate) fn mark(&mut self) -> usize {
        self.ensure_block();
        self.buf.flush_space();
        self.buf.pos()
    }

    /// Close a span opened at `start`, if it covers any text.
    pub(crate) fn span(&mut self, start: usize, kind: SpanKind) {
        let end = self.buf.pos();
        if end > start {
            self.buf.add_span(start..end, kind);
        }
    }

    /// Append `s` under one span.
    pub(crate) fn styled(&mut self, kind: SpanKind, s: &str) {
        let start = self.mark();
        self.text(s);
        self.span(start, kind);
    }

    /// Make the text since `start` a Strong's cross-link to `number`.
    pub(crate) fn strongs_link(&mut self, start: usize, number: StrongsNumber) {
        if self.buf.pos() > start {
            self.span(
                start,
                SpanKind::TaggedWord {
                    parts: vec![MorphemePart::new(number)],
                },
            );
            self.links += 1;
        }
    }

    /// Append a Strong's number as a cross-link to itself (`G5463`).
    pub(crate) fn strongs_number(&mut self, number: StrongsNumber) {
        let start = self.mark();
        self.text(&number.to_string());
        self.strongs_link(start, number);
    }

    /// Append `s`, linking every Strong's number written in it (`(H2438H)`).
    pub(crate) fn text_linking_strongs(&mut self, s: &str) {
        let mut rest = s;
        while !rest.is_empty() {
            let word_start = rest
                .find(|c: char| c.is_ascii_alphanumeric())
                .unwrap_or(rest.len());
            let (before, from) = rest.split_at(word_start);
            self.text(before);
            let word_len = from
                .find(|c: char| !c.is_ascii_alphanumeric())
                .unwrap_or(from.len());
            let (word, after) = from.split_at(word_len);
            match strongs_word(word) {
                Some(n) => {
                    let start = self.mark();
                    self.text(word);
                    self.strongs_link(start, n);
                }
                None => self.text(word),
            }
            rest = after;
        }
    }

    /// Make the text since `start` a Scripture reference to `osis`, if it reads as one.
    pub(crate) fn scripture_ref(&mut self, start: usize, osis: &str) {
        if self.buf.pos() <= start {
            return;
        }
        match parse_osis_ref(osis) {
            Some(targets) => {
                self.span(start, SpanKind::ScriptureRef { targets });
                self.refs += 1;
            }
            None => self.drop_markup("ref outside the Standard versification"),
        }
    }

    /// Count source markup that has no place in the span vocabulary.
    pub(crate) fn drop_markup(&mut self, what: &str) {
        self.dropped.push(what.to_string());
    }

    /// A Strong's number this entry is for. Its base number becomes a secondary Key.
    pub(crate) fn key_strongs(&mut self, number: StrongsNumber) {
        if !self.strongs.contains(&number) {
            self.strongs.push(number);
        }
    }

    /// A headword form (original script) this entry is for.
    pub(crate) fn key_lemma(&mut self, lemma: &str) {
        let lemma: String = lemma.trim().nfc().collect();
        if !lemma.is_empty() && !self.lemmas.contains(&lemma) {
            self.lemmas.push(lemma);
        }
    }
}

/// `H1254A`, `G5485`, `H0001G` as a Strong's number; other words are not.
fn strongs_word(word: &str) -> Option<StrongsNumber> {
    let mut chars = word.chars();
    let first = chars.next()?;
    let digits = chars
        .as_str()
        .trim_end_matches(|c: char| c.is_ascii_alphabetic());
    let letters = chars.as_str().len() - digits.len();
    let ok = matches!(first, 'H' | 'G')
        && !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && letters <= 1;
    if ok { word.parse().ok() } else { None }
}

/// Collects entries into one Lexicon Resource and its Import report.
pub(crate) struct LexiconWriter {
    builder: ResourceBuilder,
    report: ImportReport,
    ids: HashSet<String>,
}

impl LexiconWriter {
    pub(crate) fn new(
        o: &LexiconOptions,
        source_format: &str,
        source_files: Vec<String>,
    ) -> Result<Self, LexiconError> {
        if !is_valid_resource_id(&o.resource_id) {
            return Err(LexiconError::InvalidResourceId(o.resource_id.clone()));
        }
        let title = if o.title.is_empty() {
            o.resource_id.clone()
        } else {
            o.title.clone()
        };
        let abbreviation = if o.abbreviation.is_empty() {
            o.resource_id.to_ascii_uppercase().chars().take(8).collect()
        } else {
            o.abbreviation.clone()
        };
        let mut manifest = Manifest::new(
            &o.resource_id,
            &title,
            abbreviation,
            ResourceType::Lexicon,
            "en",
        );
        manifest.key_schemes = vec![KeyScheme::StrongsNumber, KeyScheme::Lemma];
        manifest.license = LICENSE_UNKNOWN.to_string();
        manifest.origin = Origin::Imported;
        manifest.source_format = source_format.into();
        manifest.source_file = (!o.source_file.is_empty()).then(|| o.source_file.clone());
        manifest.import_tool_version = o.tool_version.clone();
        manifest.format_version = FORMAT_VERSION;
        Ok(Self {
            builder: ResourceBuilder::new(manifest),
            report: ImportReport {
                resource_id: o.resource_id.clone(),
                title,
                source_format: source_format.into(),
                source_files,
                license_missing: true,
                ..ImportReport::default()
            },
            ids: HashSet::new(),
        })
    }

    pub(crate) fn warn(&mut self, warning: String) {
        self.report.warnings.push(warning);
    }

    /// Count markup dropped outside any entry.
    pub(crate) fn drop_markup(&mut self, what: &str) {
        self.builder.count_dropped(what);
    }

    pub(crate) fn add(&mut self, mut entry: Entry) {
        entry.block();
        let mut id = entry.id.trim().to_string();
        if self.ids.contains(&id) {
            let mut n = 2;
            while self.ids.contains(&format!("{id}.{n}")) {
                n += 1;
            }
            let unique = format!("{id}.{n}");
            self.warn(format!("entry id `{id}` repeats; this one is `{unique}`"));
            id = unique;
        }
        let unit = match UnitId::new(id.clone()) {
            Ok(u) => u,
            Err(e) => {
                self.warn(format!("entry skipped: {e}"));
                return;
            }
        };
        self.ids.insert(id);
        let title = (!entry.title.is_empty()).then(|| entry.title.nfc().collect());
        self.builder
            .add_unit(unit.clone(), UnitKind::Entry, title, entry.buf.text);
        for (range, kind) in entry.buf.spans {
            self.builder.add_span(&unit, range, kind);
        }
        let exact: BTreeSet<StrongsNumber> = entry.strongs.iter().copied().collect();
        for n in &entry.strongs {
            self.builder
                .add_key(Key::StrongsNumber(*n), &unit, None, false);
        }
        for base in entry.strongs.iter().map(|n| n.base()) {
            if !exact.contains(&base) {
                self.builder
                    .add_key(Key::StrongsNumber(base), &unit, None, true);
            }
        }
        for lemma in entry.lemmas {
            self.builder.add_key(Key::Lemma(lemma), &unit, None, false);
        }
        for what in &entry.dropped {
            self.builder.count_dropped(what);
        }
        self.report.entries += 1;
        self.report.tagged_words += entry.links;
        self.report.scripture_refs += entry.refs;
    }

    pub(crate) fn finish(self) -> Result<(Resource, ImportReport), LexiconError> {
        if self.report.entries == 0 {
            return Err(LexiconError::NoEntries);
        }
        let resource = self.builder.build().map_err(LexiconError::Invalid)?;
        let mut report = self.report;
        report.dropped_markup = resource.dropped_markup.clone();
        Ok((resource, report))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use obs_scripture::{Book, Passage, Verse};

    fn refs(osis: &str) -> Option<Vec<String>> {
        parse_osis_ref(osis).map(|l| l.iter().map(|p| p.to_string()).collect())
    }

    #[test]
    fn osis_and_stepbible_references_read_in_the_standard_versification() {
        let one = |b, c, v| Passage::verse(Verse::new(b, c, v)).to_string();
        assert_eq!(refs("Gen.1.1"), Some(vec![one(Book::Genesis, 1, 1)]));
        assert_eq!(refs("Acts.24.27"), Some(vec![one(Book::Acts, 24, 27)]));
        assert_eq!(refs("Jhn.3.16"), Some(vec![one(Book::John, 3, 16)]));
        assert_eq!(refs("Jude.1.4"), Some(vec![one(Book::Jude, 1, 4)]));
        assert_eq!(refs("Jude.4"), Some(vec![one(Book::Jude, 1, 4)]));
        assert_eq!(refs("Luk.4.34."), Some(vec![one(Book::Luke, 4, 34)]));
        assert_eq!(refs("1Co.3.2; 10.13").map(|v| v.len()), Some(2));
        assert_eq!(refs("Rev.1.1, 4, 9").map(|v| v.len()), Some(3));
        assert_eq!(
            refs("Gen.1.1-Gen.1.5"),
            Some(vec![
                Passage::new(
                    Verse::new(Book::Genesis, 1, 1),
                    Verse::new(Book::Genesis, 1, 5)
                )
                .unwrap()
                .to_string()
            ])
        );
        for outside in [
            "Sir.21.16",
            "2Macc.1.1",
            "LXX:Ps.3.1",
            "3Ki.14.1",
            "",
            "Gen.99.1",
        ] {
            assert_eq!(refs(outside), None, "{outside}");
        }
    }

    #[test]
    fn strongs_words() {
        assert_eq!(
            strongs_word("H2438H").map(|n| n.to_string()),
            Some("H2438H".into())
        );
        assert_eq!(
            strongs_word("G0001").map(|n| n.to_string()),
            Some("G1".into())
        );
        for not in ["Hebrew", "G", "H12AB", "X12", "Gen", "H123456"] {
            assert_eq!(strongs_word(not), None, "{not}");
        }
    }

    #[test]
    fn entries_build_blocks_spans_and_keys() {
        let mut e = Entry::new("H1254A", "בָּרָא");
        e.styled(SpanKind::Bold, "בָּרָא");
        e.text(" ba.ra ");
        e.block();
        e.text("   ");
        e.block();
        e.text("see ");
        let s = e.mark();
        e.text("Gen 1:1");
        e.scripture_ref(s, "Gen.1.1");
        e.text_linking_strongs(" and (H1254B).");
        e.key_strongs("H1254A".parse().unwrap());
        e.key_lemma(" בָּרָא ");
        let mut w = LexiconWriter::new(
            &LexiconOptions {
                resource_id: "lex".into(),
                ..LexiconOptions::default()
            },
            "test",
            vec![],
        )
        .unwrap();
        w.add(e);
        let (r, report) = w.finish().unwrap();
        let unit = &r.units[0];
        assert_eq!(unit.text, "בָּרָא ba.ra\nsee Gen 1:1 and (H1254B).");
        let kinds: Vec<(&str, &str)> = unit
            .spans
            .iter()
            .map(|s| (s.kind.name(), &unit.text[s.range()]))
            .collect();
        assert_eq!(
            kinds,
            [
                ("para", "בָּרָא ba.ra"),
                ("bold", "בָּרָא"),
                ("para", "see Gen 1:1 and (H1254B)."),
                ("scripture_ref", "Gen 1:1"),
                ("tagged_word", "H1254B"),
            ]
        );
        let keys: Vec<(String, bool)> = r
            .keys
            .iter()
            .map(|k| (k.key.to_string(), k.alias))
            .collect();
        assert_eq!(
            keys,
            [
                ("H1254".to_string(), true),
                ("H1254A".to_string(), false),
                ("בָּרָא".to_string(), false),
            ]
        );
        assert_eq!(r.transliterations[0].transliteration, "bara");
        assert_eq!(
            (report.entries, report.scripture_refs, report.tagged_words),
            (1, 1, 1)
        );
    }
}
