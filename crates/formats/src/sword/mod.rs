//! SWORD Import: a module in a SWORD library (Bible, Commentary, Lexicon or Dictionary) →
//! [`ResourceBuilder`] → [`Resource`] plus an [`ImportReport`].
//!
//! Modules are read with the CC0 `sword-modules` crate (no libsword). Their entries are OSIS,
//! ThML, GBF, TEI or plain-text fragments, which our own parsers map to the span vocabulary
//! (see [`osis`], [`thml`], [`gbf`]); markup with no place in it is dropped and counted.
//!
//! - **Bible**: one `bible_book` unit per Bible book. Verses keep the module's own numbering;
//!   the conf's `Versification=` only tells the reader how the module is laid out and is
//!   reported as a hint. The Verse map is built from TVTMS like any other Bible's.
//! - **Commentary**: one `section` unit per entry (linked verses are one entry), keyed by its
//!   Passage. A book or chapter introduction is keyed by the whole book or chapter.
//! - **Lexicon** (conf `Feature=GreekDef` / `HebrewDef`): one `entry` unit per Strong's number.
//!   **Dictionary**: one `entry` unit per headword; `@LINK` entries become Headword aliases.
//!
//! The conf's `DistributionLicense`, `TextSource` and copyright keys are copied into the
//! manifest (#11 rule 15): an SPDX id where one is known, and the text verbatim in the
//! Attribution. Import is never blocked on licence.

mod gbf;
mod osis;
pub mod repo;
mod thml;
mod writer;
mod xml;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use obs_model::{
    Direction, FORMAT_VERSION, Key, KeyScheme, LICENSE_UNKNOWN, Manifest, Origin, ParaStyle,
    Resource, ResourceBuilder, ResourceType, SpanKind, UnitId, UnitKind, is_valid_resource_id,
};
use obs_scripture::{
    Book, ListedVersification, Passage, Standard, StrongsNumber, Verse, Versification,
};
use sword_modules::conf::rtf_to_text;
use sword_modules::{Markup, ModDrv, ModuleConf, ModuleKind, SwordLibrary};

use crate::error::ImportError;
use crate::report::{BookReport, ImportReport, VerseMapCounts};
use crate::usfm::slugify;
use crate::verse_map::{VerseFacts, build_verse_map};
use writer::{Element, UnitText, Writer};
use xml::{Token, tokenize};

/// A SWORD library opened for Import: a folder holding `mods.d/` and `modules/`.
#[derive(Debug)]
pub struct SwordSource {
    library: SwordLibrary,
    /// The modules of the conf file given instead of a folder, if any.
    chosen: Vec<String>,
}

/// One module found in a [`SwordSource`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwordModuleInfo {
    pub name: String,
    pub description: Option<String>,
    /// `None` for kinds v0.1 does not import (general books, devotionals).
    pub resource_type: Option<ResourceType>,
}

impl SwordSource {
    /// Open a SWORD library folder (the one holding `mods.d/`), or a module's `.conf` file
    /// inside `mods.d/` (then that module is the [default](Self::default_module)).
    pub fn open(path: &Path) -> Result<Self, ImportError> {
        let is_conf = path.is_file()
            && path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("conf"));
        let root: PathBuf = if is_conf {
            path.parent()
                .and_then(Path::parent)
                .map(Path::to_path_buf)
                .unwrap_or_default()
        } else {
            path.to_path_buf()
        };
        let library = SwordLibrary::open(&root).map_err(|e| {
            ImportError::Sword(format!(
                "{}: not a SWORD library folder (no readable mods.d/): {e}",
                root.display()
            ))
        })?;
        let chosen = if is_conf {
            let text = std::fs::read(path)
                .map_err(|e| ImportError::Sword(format!("{}: {e}", path.display())))?;
            let text = String::from_utf8_lossy(&text);
            sword_modules::conf::parse_conf(&text)
                .map_err(|e| ImportError::Sword(format!("{}: {e}", path.display())))?
                .into_iter()
                .filter(|c| c.get("ModDrv").is_some())
                .map(|c| c.name().to_string())
                .collect()
        } else {
            Vec::new()
        };
        Ok(Self { library, chosen })
    }

    /// Every module in the library.
    pub fn modules(&self) -> Vec<SwordModuleInfo> {
        self.library
            .modules()
            .map(|c| SwordModuleInfo {
                name: c.name().to_string(),
                description: c.description().map(str::to_string),
                resource_type: resource_type(c).ok(),
            })
            .collect()
    }

    /// The module to import when none is named: the one in the given conf file, or the
    /// library's only module.
    pub fn default_module(&self) -> Option<String> {
        match self.chosen.as_slice() {
            [one] => Some(one.clone()),
            [] => {
                let names: Vec<&str> = self.library.modules().map(ModuleConf::name).collect();
                match names.as_slice() {
                    [one] => Some((*one).to_string()),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

/// Identity overrides for a SWORD Import. Empty fields come from the module's conf.
#[derive(Debug, Clone, Default)]
pub struct SwordOptions {
    pub resource_id: String,
    pub title: String,
    pub abbreviation: String,
    /// BCP 47; defaults to the conf's `Lang`, else `und`.
    pub language: String,
    /// File or folder name shown in About and the report; defaults to the module name.
    pub source_file: String,
    /// Version of the tool doing the Import, recorded in the manifest.
    pub tool_version: String,
}

/// Import one module of `source` as a Resource.
pub fn import_sword(
    source: &SwordSource,
    module: &str,
    options: &SwordOptions,
) -> Result<(Resource, ImportReport), ImportError> {
    let Some(conf) = source.library.conf(module).cloned() else {
        return Err(ImportError::UnknownModule {
            module: module.to_string(),
            available: source
                .library
                .modules()
                .map(|c| c.name().to_string())
                .collect(),
        });
    };
    let name = conf.name().to_string();
    let unsupported = |reason: String| ImportError::UnsupportedModule {
        module: name.clone(),
        reason,
    };
    if conf.is_locked() {
        return Err(unsupported(
            "it is enciphered (locked) and needs an unlock key from its publisher".into(),
        ));
    }
    let resource_type = resource_type(&conf).map_err(unsupported)?;
    let (manifest, license_missing) = manifest(&conf, resource_type, options)?;
    let sword_err = |e: sword_modules::Error| ImportError::Sword(format!("{name}: {e}"));
    let opened = source.library.open_module(&name).map_err(sword_err)?;
    let mut report = ImportReport {
        resource_id: manifest.resource_id.clone(),
        title: manifest.title.clone(),
        source_format: format!("sword ({})", conf.source_type().as_str()),
        source_files: vec![manifest.source_file.clone().unwrap_or_default()],
        license_missing,
        ..ImportReport::default()
    };
    let mut builder = ResourceBuilder::new(manifest);
    let mut w = Writer::new();
    match resource_type {
        ResourceType::Bible => {
            let module = opened.into_verse().map_err(sword_err)?;
            report.versification_hint = Some(conf.versification().to_string());
            import_bible(&module, &conf, &mut builder, &mut w, &mut report).map_err(sword_err)?;
        }
        ResourceType::Commentary => {
            let module = opened.into_verse().map_err(sword_err)?;
            let v11n = conf.versification();
            report.versification_hint = Some(v11n.to_string());
            if !matches!(v11n, "KJV" | "KJVA") {
                return Err(unsupported(format!(
                    "Commentary numbering {v11n} cannot yet be normalized to Standard; import refused"
                )));
            }
            import_commentary(&module, &mut builder, &mut w, &mut report).map_err(sword_err)?;
        }
        ResourceType::Lexicon | ResourceType::Dictionary => {
            let module = opened.into_dictionary().map_err(sword_err)?;
            let lexicon = lexicon_language(&conf);
            import_dictionary(&module, &conf, lexicon, &mut builder, &mut w, &mut report)
                .map_err(sword_err)?;
        }
        ResourceType::CrossReferenceSet | ResourceType::PeopleAndPlaces => {
            return Err(unsupported("not a SWORD module kind".into()));
        }
    }
    if builder.unit_count() == 0 {
        return Err(ImportError::EmptyModule(name));
    }
    let notes = w.take_notes();
    report.notes = notes.len();
    for (id, buf) in notes {
        builder.add_unit(id.clone(), UnitKind::Note, None, buf.text);
        for (range, kind) in buf.spans {
            count_span(&kind, &mut report);
            builder.add_span(&id, range, kind);
        }
    }
    for (marker, count) in &w.dropped {
        for _ in 0..*count {
            builder.count_dropped(marker);
        }
    }
    let resource = builder.build().map_err(ImportError::Invalid)?;
    report.dropped_markup = resource.dropped_markup.clone();
    report.warnings = std::mem::take(&mut w.warnings);
    Ok((resource, report))
}

/// What a module imports as, or why it cannot be imported.
fn resource_type(conf: &ModuleConf) -> Result<ResourceType, String> {
    let drv = conf.mod_drv().map_err(|e| e.to_string())?;
    match drv.kind() {
        ModuleKind::Bible => Ok(ResourceType::Bible),
        ModuleKind::Commentary if drv == ModDrv::HrefCom => {
            Err("HREFCom commentaries link to web pages and hold no text".into())
        }
        ModuleKind::Commentary => Ok(ResourceType::Commentary),
        ModuleKind::Dictionary => {
            let category = conf.category().unwrap_or_default();
            if category.eq_ignore_ascii_case("Daily Devotional") {
                return Err("daily devotionals are not imported in v0.1".into());
            }
            Ok(if lexicon_language(conf).is_some() {
                ResourceType::Lexicon
            } else {
                ResourceType::Dictionary
            })
        }
        ModuleKind::GenBook => Err("general books are not imported in v0.1".into()),
        ModuleKind::Unknown => Err(format!("unknown module driver {drv:?}")),
    }
}

/// The Strong's prefix of a Strong's-keyed lexicon (`Feature=GreekDef` → `G`).
fn lexicon_language(conf: &ModuleConf) -> Option<char> {
    let features = conf.features();
    let has = |f: &str| features.iter().any(|x| x.eq_ignore_ascii_case(f));
    match (has("GreekDef"), has("HebrewDef")) {
        (true, false) => Some('G'),
        (false, true) => Some('H'),
        (true, true) => Some('?'),
        (false, false) => None,
    }
}

fn manifest(
    conf: &ModuleConf,
    resource_type: ResourceType,
    o: &SwordOptions,
) -> Result<(Manifest, bool), ImportError> {
    let or = |given: &str, conf_value: Option<&str>, fallback: &str| {
        if !given.is_empty() {
            given.to_string()
        } else {
            conf_value
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .unwrap_or(fallback)
                .to_string()
        }
    };
    let name = conf.name();
    let resource_id = or(&o.resource_id, None, &slugify(name));
    if !is_valid_resource_id(&resource_id) {
        return Err(ImportError::InvalidResourceId(resource_id));
    }
    let title = or(&o.title, conf.description(), name);
    let abbreviation = or(&o.abbreviation, conf.get("Abbreviation"), name);
    let language = or(&o.language, conf.lang(), "und");
    let mut m = Manifest::new(&resource_id, title, abbreviation, resource_type, language);
    if conf.direction() == sword_modules::conf::Direction::RightToLeft {
        m.direction = Direction::Rtl;
    }
    m.key_schemes = vec![match resource_type {
        ResourceType::Lexicon => KeyScheme::StrongsNumber,
        ResourceType::Dictionary => KeyScheme::Headword,
        _ => KeyScheme::Passage,
    }];
    let license = conf
        .get("DistributionLicense")
        .map(str::trim)
        .filter(|l| !l.is_empty());
    m.license = license.map_or_else(|| LICENSE_UNKNOWN.to_string(), sword_license);
    m.attribution = sword_attribution(conf);
    m.source_url = conf
        .get("TextSource")
        .map(str::trim)
        .filter(|s| s.starts_with("http://") || s.starts_with("https://"))
        .map(str::to_string);
    m.origin = Origin::Imported;
    m.source_format = "sword".into();
    m.source_file = Some(or(&o.source_file, None, name));
    m.import_tool_version = o.tool_version.clone();
    m.format_version = FORMAT_VERSION;
    Ok((m, license.is_none()))
}

/// An SPDX expression for a conf's `DistributionLicense`: a listed id where the text names one
/// exactly (`Public Domain`, `Creative Commons: BY-SA 4.0`), else a `LicenseRef-` made from the
/// text. The text itself is kept verbatim in the Attribution.
pub(crate) fn sword_license(text: &str) -> String {
    let t = text.trim();
    let lower = t.to_ascii_lowercase();
    if lower == "public domain" {
        return "LicenseRef-PD".into();
    }
    if let Some(cc) = lower
        .strip_prefix("creative commons:")
        .or_else(|| lower.strip_prefix("creative commons"))
    {
        let cc = cc
            .trim()
            .trim_start_matches("cc")
            .trim_start_matches(['-', ' ']);
        if cc == "0" || cc == "0 1.0" || cc == "zero" {
            return "CC0-1.0".into();
        }
        let mut words = cc.split_whitespace();
        if let (Some(kind), Some(version), None) = (words.next(), words.next(), words.next()) {
            let parts: Vec<&str> = kind.split('-').collect();
            let known =
                parts[0] == "by" && parts[1..].iter().all(|p| matches!(*p, "sa" | "nc" | "nd"));
            let versioned = matches!(version, "1.0" | "2.0" | "2.5" | "3.0" | "4.0");
            if known && versioned {
                return format!("CC-{}-{version}", kind.to_ascii_uppercase());
            }
        }
    }
    let mut id = String::from("LicenseRef-");
    for c in t.chars() {
        if c.is_ascii_alphanumeric() || c == '.' {
            id.push(c);
        } else if !id.ends_with('-') {
            id.push('-');
        }
    }
    while id.ends_with('-') {
        id.pop();
    }
    id
}

/// The conf's licence and copyright keys, verbatim, one per line.
pub(crate) fn sword_attribution(conf: &ModuleConf) -> String {
    const FIELDS: [(&str, &str); 9] = [
        ("DistributionLicense", "Licence"),
        ("DistributionNotes", "Licence notes"),
        ("TextSource", "Text source"),
        ("ShortCopyright", "Copyright"),
        ("Copyright", "Copyright"),
        ("CopyrightHolder", "Copyright holder"),
        ("CopyrightDate", "Copyright date"),
        ("CopyrightNotes", "Copyright notes"),
        ("CopyrightContactName", "Copyright contact"),
    ];
    let mut lines: Vec<String> = Vec::new();
    for (key, label) in FIELDS {
        for value in conf.get_all(key) {
            let value = rtf_to_text(value);
            let value = value.trim();
            let line = format!("{label}: {value}");
            if !value.is_empty() && !lines.contains(&line) {
                lines.push(line);
            }
        }
    }
    lines.push(format!("Imported from the SWORD module {}.", conf.name()));
    lines.join("\n")
}

/// Write one entry with the parser for its markup.
fn feed(markup: Markup, raw: &str, w: &mut Writer) {
    match markup {
        Markup::Osis => osis::feed(raw, w),
        Markup::Thml => thml::feed(raw, w),
        Markup::Gbf => gbf::feed(raw, w),
        Markup::Tei => feed_tei(raw, w),
        Markup::Plaintext => {
            for (i, line) in raw.lines().enumerate() {
                if i > 0 {
                    w.para_break(None);
                }
                w.text(line);
            }
        }
    }
}

/// TEI (a few lexicons): the dictionary-entry elements; everything else is dropped.
fn feed_tei(raw: &str, w: &mut Writer) {
    for token in tokenize(raw) {
        match token {
            Token::Text(t) => w.text(&t),
            Token::End(name) => w.close(name),
            Token::Start { name, attrs, empty } => {
                let element = match name {
                    "lb" => {
                        w.para_break(None);
                        continue;
                    }
                    "orth" => Element::Span(SpanKind::Bold),
                    "hi" | "emph" => match Token::attr(&attrs, "rend").as_deref() {
                        Some("bold") => Element::Span(SpanKind::Bold),
                        Some("italic") => Element::Span(SpanKind::Italic),
                        _ => Element::Span(SpanKind::Emphasis),
                    },
                    "foreign" => Element::Span(SpanKind::Foreign),
                    "title" => Element::Span(SpanKind::Italic),
                    "ref" => Element::Reference(
                        Token::attr(&attrs, "osisRef").and_then(|r| w.parse_osis_ref(&r)),
                    ),
                    "p" => Element::Para(ParaStyle::Prose),
                    "entryFree" | "superentry" | "entry" | "form" | "def" | "sense" => {
                        Element::Container
                    }
                    other => Element::Dropped(format!("<{other}>")),
                };
                w.open(name, element);
                if empty {
                    w.close(name);
                }
            }
        }
    }
}

fn add_unit(
    builder: &mut ResourceBuilder,
    id: &UnitId,
    kind: UnitKind,
    title: String,
    out: UnitText,
    report: &mut ImportReport,
) {
    builder.add_unit(id.clone(), kind, Some(title), out.text);
    for (range, kind) in out.spans {
        count_span(&kind, report);
        builder.add_span(id, range, kind);
    }
}

fn count_span(kind: &SpanKind, report: &mut ImportReport) {
    match kind {
        SpanKind::TaggedWord { .. } => report.tagged_words += 1,
        SpanKind::ScriptureRef { .. } => report.scripture_refs += 1,
        _ => {}
    }
}

// ---- Bible -------------------------------------------------------------------------------

struct BookUnit {
    id: UnitId,
    code: String,
    book: Option<Book>,
    other_verses: usize,
}

fn import_bible(
    module: &sword_modules::VerseModule,
    conf: &ModuleConf,
    builder: &mut ResourceBuilder,
    w: &mut Writer,
    report: &mut ImportReport,
) -> sword_modules::Result<()> {
    // Complete the native Verse list before resolving references, including references to
    // later Bible books and whole chapters. Probe output is discarded before the real pass.
    let mut probe = Writer::new();
    probe.unnumbered_psalm_titles = matches!(conf.versification(), "KJV" | "KJVA");
    let native = ListedVersification::new(
        read_bible(module, &mut probe)?
            .iter()
            .flat_map(|(_, out)| out.verses.iter().map(|(_, verse, last)| (*verse, *last))),
    );
    w.unnumbered_psalm_titles = probe.unnumbered_psalm_titles;
    drop(probe);
    w.native = Some(native);
    let mut facts: Vec<VerseFacts> = Vec::new();
    let mut other_verses = 0usize;
    for (unit, out) in read_bible(module, w)? {
        for (range, verse, last) in &out.verses {
            let words = out
                .text
                .get(range.clone())
                .map_or(0, |t| obs_model::tokenize(t).len());
            facts.push(VerseFacts {
                verse: *verse,
                last: *last,
                words: u32::try_from(words).unwrap_or(u32::MAX),
            });
        }
        report.books.push(BookReport {
            code: unit.code.clone(),
            book: unit.book,
            chapters: out.chapters,
            verses: out.verses.len().max(unit.other_verses),
        });
        report.verses += out.verses.len();
        other_verses += unit.other_verses;
        let verses = out.verses.clone();
        let (kind, title) = match unit.book {
            Some(b) => (UnitKind::BibleBook, b.name().to_string()),
            None => (UnitKind::Division, unit.code.clone()),
        };
        add_unit(builder, &unit.id, kind, title, out, report);
        for (range, verse, last) in verses {
            builder.add_verse(&unit.id, range, verse, last);
        }
    }
    if !facts.is_empty() {
        let outcome = build_verse_map(&facts);
        report.verse_map = Some(VerseMapCounts {
            // Verses of books outside the canon are Unmapped too.
            unmapped: outcome.map.unmapped().count() + other_verses,
            absent: outcome.map.absent().len(),
        });
        report.traditions = outcome.traditions;
        builder.set_verse_map(outcome.map);
    }
    Ok(())
}

fn read_bible(
    module: &sword_modules::VerseModule,
    w: &mut Writer,
) -> sword_modules::Result<Vec<(BookUnit, UnitText)>> {
    let mut units = Vec::new();
    let mut current: Option<BookUnit> = None;
    let mut chapter: Option<u8> = None;
    for span in module.iter_spans() {
        let span = span?;
        let (first, last) = (span.first, span.last);
        let osis = first.osis_book().unwrap_or("Unknown");
        if current.as_ref().is_none_or(|u| u.code != code_of(osis)) {
            if let Some(unit) = current.take() {
                units.push((unit, w.finish_unit()));
            }
            let book = Book::from_osis(osis);
            let id = match book {
                Some(b) => UnitId::book(b),
                None => UnitId::new(code_of(osis)).unwrap_or_else(|_| UnitId::book(Book::Genesis)),
            };
            w.set_book(book);
            chapter = None;
            current = Some(BookUnit {
                id,
                code: code_of(osis),
                book,
                other_verses: 0,
            });
        }
        let Some(unit) = current.as_mut() else {
            continue;
        };
        if first.chapter == 0 {
            w.start_intro();
        } else {
            if chapter != Some(first.chapter) {
                w.start_chapter(u16::from(first.chapter));
                chapter = Some(first.chapter);
            }
            let to = if last.chapter == first.chapter {
                last.verse
            } else {
                w.warn(format!(
                    "{}: linked across a chapter end; only its first chapter is kept as verses",
                    span.osis_ref().unwrap_or_default()
                ));
                first.verse
            };
            // A chapter introduction linked to verse 1 holds verse 1's text.
            let from = if first.verse == 0 && to > 0 {
                1
            } else {
                first.verse
            };
            match (unit.book, from) {
                (_, 0) => w.start_intro(),
                (Some(b), v) => w.open_verse(
                    Verse::new(b, u16::from(first.chapter), u16::from(v)),
                    u16::from(to.max(v)),
                ),
                (None, v) => {
                    unit.other_verses += usize::from(to.max(v) - v) + 1;
                    w.start_intro();
                }
            }
        }
        feed(span.entry.markup, &span.entry.raw, w);
    }
    if let Some(unit) = current.take() {
        units.push((unit, w.finish_unit()));
    }
    Ok(units)
}

/// Unit id and report code of a book: its USFM code, or the OSIS id outside the canon.
fn code_of(osis: &str) -> String {
    Book::from_osis(osis).map_or_else(|| osis.to_string(), |b| b.usfm().to_string())
}

// ---- Commentary --------------------------------------------------------------------------

fn import_commentary(
    module: &sword_modules::VerseModule,
    builder: &mut ResourceBuilder,
    w: &mut Writer,
    report: &mut ImportReport,
) -> sword_modules::Result<()> {
    let mut outside = 0usize;
    for span in module.iter_spans() {
        let span = span?;
        let (first, last) = (span.first, span.last);
        let Some(book) = first.osis_book().and_then(Book::from_osis) else {
            outside += 1;
            continue;
        };
        let (c, v) = (u16::from(first.chapter), u16::from(first.verse));
        let (lc, lv) = (u16::from(last.chapter), u16::from(last.verse));
        let (passage, id) = if c == 0 {
            (Passage::book(&Standard, book), book.usfm().to_string())
        } else if v == 0 && (lc, lv) == (c, 0) {
            (
                Passage::chapter(&Standard, book, c),
                format!("{}.{c}", book.usfm()),
            )
        } else {
            let start = Verse::new(book, c, v.max(1));
            let end = Verse::new(book, lc, lv.max(1));
            let id = if start == end {
                format!("{}.{c}.{}", book.usfm(), start.verse)
            } else if lc == c {
                format!("{}.{c}.{}-{}", book.usfm(), start.verse, end.verse)
            } else {
                format!("{}.{c}.{}-{lc}.{}", book.usfm(), start.verse, end.verse)
            };
            let in_standard = Standard.contains(start) && Standard.contains(end);
            (
                in_standard.then(|| Passage::new(start, end).ok()).flatten(),
                id,
            )
        };
        let (Some(passage), Ok(id)) = (passage, UnitId::new(id.clone())) else {
            w.warn(format!(
                "{id}: not in the Standard versification; section skipped"
            ));
            continue;
        };
        w.set_context(Some(passage.start()));
        feed(span.entry.markup, &span.entry.raw, w);
        let out = w.finish_unit();
        add_unit(
            builder,
            &id,
            UnitKind::Section,
            passage.to_string(),
            out,
            report,
        );
        builder.add_key(Key::Passage(passage), &id, None, false);
        report.entries += 1;
    }
    if outside > 0 {
        w.warn(format!(
            "{outside} section(s) on books outside the 66-book canon skipped (Commentary Keys are Standard Passages)"
        ));
    }
    Ok(())
}

// ---- Lexicon and Dictionary --------------------------------------------------------------

/// A lexicon key as a Strong's number: `00025` (with the module's prefix), `G0025`, `25a`.
fn strongs_key(key: &str, prefix: char) -> Option<StrongsNumber> {
    let key = key.trim();
    if key.starts_with(['G', 'g', 'H', 'h']) {
        return key.parse().ok();
    }
    if prefix == '?' {
        return None;
    }
    let digits = key.trim_start_matches('0');
    format!("{prefix}{digits}").parse().ok()
}

fn import_dictionary(
    module: &sword_modules::DictionaryModule,
    conf: &ModuleConf,
    lexicon: Option<char>,
    builder: &mut ResourceBuilder,
    w: &mut Writer,
    report: &mut ImportReport,
) -> sword_modules::Result<()> {
    let fold_stored = |key: &str| {
        if conf.case_sensitive_keys() {
            key.to_string()
        } else {
            key.to_uppercase()
        }
    };
    let fold_lookup = |key: &str| {
        let key = if conf.strongs_padding() {
            sword_modules::strongs_pad(key)
        } else {
            key.to_string()
        };
        fold_stored(&key)
    };
    let mut links: Vec<(String, String)> = Vec::new();
    let mut by_key: HashMap<String, UnitId> = HashMap::new();
    let mut ids: HashSet<UnitId> = HashSet::new();
    let mut skipped: BTreeMap<&'static str, usize> = BTreeMap::new();
    for i in 0..module.len() {
        let key = module.key_at(i)?;
        if let Some(target) = module.link_at(i)? {
            links.push((key, target));
            continue;
        }
        let Some(entry) = module.entry_at(i)? else {
            continue;
        };
        let (id, entry_key) = match lexicon {
            Some(prefix) => {
                let Some(n) = strongs_key(&key, prefix) else {
                    *skipped
                        .entry("keys that are not Strong's numbers")
                        .or_default() += 1;
                    continue;
                };
                (UnitId::new(n.to_string()), Key::StrongsNumber(n))
            }
            None => {
                let headword = key.trim().to_string();
                (UnitId::new(headword.clone()), Key::Headword(headword))
            }
        };
        let Ok(id) = id else {
            *skipped.entry("keys that cannot be unit ids").or_default() += 1;
            continue;
        };
        if !ids.insert(id.clone()) {
            w.warn(format!("{key}: a second entry for `{id}`; skipped"));
            continue;
        }
        feed(entry.markup, &entry.raw, w);
        let out = w.finish_unit();
        add_unit(builder, &id, UnitKind::Entry, id.to_string(), out, report);
        builder.add_key(entry_key, &id, None, false);
        by_key.insert(fold_stored(&key), id);
        report.entries += 1;
    }
    let targets: HashMap<String, String> = links
        .iter()
        .map(|(alias, target)| (fold_stored(alias), fold_lookup(target)))
        .collect();
    for (alias, target) in links {
        let mut key = fold_lookup(&target);
        let mut seen = HashSet::new();
        while !by_key.contains_key(&key) && seen.insert(key.clone()) {
            let Some(next) = targets.get(&key) else { break };
            key = next.clone();
        }
        let Some(id) = by_key.get(&key) else {
            w.warn(format!(
                "@LINK {alias} → {target}: missing or cyclic target"
            ));
            continue;
        };
        match lexicon {
            Some(prefix) => {
                if let Some(n) = strongs_key(&alias, prefix) {
                    builder.add_key(Key::StrongsNumber(n), id, None, false);
                }
            }
            None => builder.add_key(Key::Headword(alias.trim().to_string()), id, None, true),
        }
    }
    for (what, n) in skipped {
        w.warn(format!(
            "{n} entr{} skipped: {what}",
            if n == 1 { "y" } else { "ies" }
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn licences_map_to_spdx_where_named_exactly() {
        assert_eq!(sword_license("Public Domain"), "LicenseRef-PD");
        assert_eq!(sword_license("Creative Commons: BY-SA 4.0"), "CC-BY-SA-4.0");
        assert_eq!(
            sword_license("Creative Commons: by-nc-nd 3.0"),
            "CC-BY-NC-ND-3.0"
        );
        assert_eq!(sword_license("Creative Commons: CC0"), "CC0-1.0");
        assert_eq!(sword_license("GPL"), "LicenseRef-GPL");
        assert_eq!(
            sword_license("Copyrighted; Permission to distribute granted to CrossWire"),
            "LicenseRef-Copyrighted-Permission-to-distribute-granted-to-CrossWire"
        );
    }

    #[test]
    fn attribution_copies_conf_fields() {
        let conf = ModuleConf::new(
            "Test",
            vec![
                (
                    "DistributionLicense".into(),
                    "Creative Commons: BY 4.0".into(),
                ),
                ("TextSource".into(), "https://example.org/".into()),
                ("CopyrightHolder".into(), "A \\u8220?Holder\\u8221?".into()),
            ],
        );
        assert_eq!(
            sword_attribution(&conf),
            "Licence: Creative Commons: BY 4.0\nText source: https://example.org/\n\
             Copyright holder: A “Holder”\nImported from the SWORD module Test."
        );
    }

    #[test]
    fn lexicon_keys() {
        assert_eq!(
            strongs_key("00025", 'G').map(|n| n.to_string()),
            Some("G25".into())
        );
        assert_eq!(
            strongs_key("H0430", 'G').map(|n| n.to_string()),
            Some("H430".into())
        );
        assert_eq!(strongs_key("00025", '?'), None);
        assert_eq!(strongs_key("AGAPE", 'G'), None);
    }

    #[test]
    fn module_kinds() {
        let conf = |entries: &[(&str, &str)]| {
            ModuleConf::new(
                "M",
                entries
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            )
        };
        assert_eq!(
            resource_type(&conf(&[("ModDrv", "zText")])),
            Ok(ResourceType::Bible)
        );
        assert_eq!(
            resource_type(&conf(&[("ModDrv", "zCom")])),
            Ok(ResourceType::Commentary)
        );
        assert_eq!(
            resource_type(&conf(&[("ModDrv", "RawLD4"), ("Feature", "GreekDef")])),
            Ok(ResourceType::Lexicon)
        );
        assert_eq!(
            resource_type(&conf(&[("ModDrv", "zLD")])),
            Ok(ResourceType::Dictionary)
        );
        assert!(resource_type(&conf(&[("ModDrv", "RawGenBook")])).is_err());
        assert!(
            resource_type(&conf(&[
                ("ModDrv", "zLD"),
                ("Category", "Daily Devotional")
            ]))
            .is_err()
        );
    }
}
