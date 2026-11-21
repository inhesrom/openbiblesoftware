//! The Open catalogue (#134): openly licensed Resources that are not SWORD modules, listed in
//! Get Resources beside the SWORD repositories' catalogs. Each entry of
//! `data/open-catalogue.toml` names the publisher's own URL, the importer that reads it, its
//! licence and licence class, and archived licence evidence (`data/open-evidence/`), in the
//! style of the Starter library lock (`starter/sources.toml`).
//!
//! A source collection (ADR 0024) adds entries generated from one publisher's pinned list:
//! `data/collections/tcp-books.tsv`, the Text Creation Partnership's texts (#345), written by
//! `cargo xtask tcp-books`. Each row becomes an entry ([`TcpBook::source`]) checked like any
//! other: by its sha256, its licence being one of TCP's CC0 statements ([`TCP_CC0_STATEMENTS`]),
//! quoted in `open-evidence/tcp.md`.
//!
//! A download is checked before it is imported: against its pinned sha256 when the publisher's
//! files are stable, and always against the pinned sha256 of the licence notice inside it when
//! the entry names one (eBible.org regenerates its zips, so its entries are checked that way
//! instead). The notice is never read for its meaning: it must be the very notice the entry's
//! evidence quotes, or the download is refused. The
//! entries are compiled in, so the Open catalogue lists offline; only Download needs the
//! network, through the app's transport ([`crate::Session::set_fetch`]).

use std::sync::OnceLock;

use obs_model::{Origin, Resource, ResourceType};
use serde::Deserialize;

use crate::get_bibles::{ListingSource, ResourceListing};
use crate::licence::{LicenceClass, licence_class};

const OPEN_CATALOGUE_TOML: &str = include_str!("../data/open-catalogue.toml");
const TCP_BOOKS_TSV: &str = include_str!("../data/collections/tcp-books.tsv");

/// The importers an Open catalogue entry may name. `usfm-zip`: a publisher's zip of USFM
/// books (eBible.org's `<id>_usfm.zip`) → a Bible. `tcp-tei`: one TEI text of the Text
/// Creation Partnership (EEBO-TCP, Evans-TCP, ECCO-TCP) → a Book (#345).
pub const OPEN_IMPORTERS: &[&str] = &["usfm-zip", "tcp-tei"];

/// The Open catalogue section's title in Get Resources.
pub const OPEN_CATALOGUE: &str = "Open catalogue";

/// Hosts never downloaded from (#20: CCEL's site terms restrict republishing).
const REFUSED_HOSTS: &[&str] = &["ccel.org"];

/// One Open catalogue entry, as written in `data/open-catalogue.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenSource {
    /// Fixed once listed and never reused; the Resource id is made from it.
    pub id: String,
    /// One of [`OPEN_IMPORTERS`].
    pub importer: String,
    /// The Resource type, as in commands (`bible`, `lexicon`, …).
    #[serde(rename = "type")]
    pub kind: String,
    /// The publisher's own file.
    pub url: String,
    /// Lowercase hex sha256 of `url`, when the publisher's files are stable.
    #[serde(default)]
    pub sha256: Option<String>,
    /// A file inside the download holding its licence notice.
    #[serde(default)]
    pub licence_file: Option<String>,
    /// Lowercase hex sha256 of the notice in `licence_file` read as text ([`notice_text`]),
    /// which the evidence file quotes.
    #[serde(default)]
    pub licence_notice_sha256: Option<String>,
    pub title: String,
    pub abbreviation: String,
    /// BCP 47.
    pub language: String,
    pub edition: String,
    pub publisher: String,
    pub homepage: String,
    /// The licence as the publisher names it, shown on the row.
    pub licence: String,
    /// SPDX expression, written to the Resource's manifest.
    pub spdx: String,
    /// Declared here, with the evidence; it must be what `spdx` classifies as.
    pub licence_class: LicenceClass,
    pub attribution: String,
    /// "Changes we made", shown in the Resource's About view.
    pub changes: String,
    /// Evidence file, relative to `crates/engine/data/`.
    pub evidence: String,
    /// Internet Archive capture of the publisher's licence page.
    pub evidence_url: String,
    /// USFM books (`\id` codes) left out of the Import.
    #[serde(default)]
    pub skip_books: Vec<String>,
    /// A Book's author, as shown and searched ("John Owen").
    #[serde(default)]
    pub author: String,
    /// A Book's date as printed ("1668").
    #[serde(default)]
    pub date: String,
    /// Made from a source collection's row (ADR 0024), not written in the catalogue file.
    #[serde(skip)]
    pub collection: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenCatalogueFile {
    #[serde(rename = "source", default)]
    sources: Vec<OpenSource>,
}

/// Parse an Open catalogue file.
pub fn parse_open_catalogue(text: &str) -> Result<Vec<OpenSource>, String> {
    toml::from_str::<OpenCatalogueFile>(text)
        .map(|f| f.sources)
        .map_err(|e| format!("open-catalogue.toml: {e}"))
}

/// The hand-written entries of `data/open-catalogue.toml` (empty if it does not parse; a test
/// checks it does).
pub fn open_catalogue_toml_sources() -> &'static [OpenSource] {
    static SOURCES: OnceLock<Vec<OpenSource>> = OnceLock::new();
    SOURCES.get_or_init(|| parse_open_catalogue(OPEN_CATALOGUE_TOML).unwrap_or_default())
}

/// The compiled-in Open catalogue: the hand-written entries, then the source collections'
/// (a text a hand-written entry lists already is left out of its collection).
pub fn open_sources() -> &'static [OpenSource] {
    static SOURCES: OnceLock<Vec<OpenSource>> = OnceLock::new();
    SOURCES.get_or_init(|| {
        let mut out = open_catalogue_toml_sources().to_vec();
        let listed: std::collections::HashSet<String> = out
            .iter()
            .filter_map(|s| TcpBook::id_of_url(&s.url))
            .collect();
        out.extend(
            tcp_books()
                .iter()
                .filter(|b| !listed.contains(&b.id))
                .map(TcpBook::source),
        );
        out
    })
}

/// The header line of `data/collections/tcp-books.tsv`.
pub const TCP_BOOKS_HEADER: &str =
    "# id\tcommit\tsha256\tbytes\tlanguage\tdate\tauthor\ttitle\tstatement\n";

/// The two statements TCP puts in a text's `<availability>`, whitespace collapsed as
/// `obs_formats::read_tei_header` reads it: (code in `tcp-books.tsv`, statement). EEBO-TCP
/// Phase I and Evans-TCP texts carry the first, Phase II texts the waiver. A text with any
/// other statement is not listed. `open-evidence/tcp.md` quotes both.
pub const TCP_CC0_STATEMENTS: [(&str, &str); 2] = [
    (
        "cc0-phase1",
        "This keyboarded and encoded edition of the work described above is co-owned by the \
         institutions providing financial support to the Early English Books Online Text \
         Creation Partnership. This Phase I text is available for reuse, according to the \
         terms of Creative Commons 0 1.0 Universal . The text can be copied, modified, \
         distributed and performed, even for commercial purposes, all without asking \
         permission.",
    ),
    (
        "cc0-waiver",
        "To the extent possible under law, the Text Creation Partnership has waived all \
         copyright and related or neighboring rights to this keyboarded and encoded edition of \
         the work described above, according to the terms of the CC0 1.0 Public Domain \
         Dedication (http://creativecommons.org/publicdomain/zero/1.0/). This waiver does not \
         extend to any page images or other supplementary files associated with this work, \
         which may be protected by copyright or other license restrictions. Please go to \
         https://www.textcreationpartnership.org/ for more information about the project.",
    ),
];

/// The code of the CC0 statement `availability` is, whitespace aside; `None` for any other.
pub fn tcp_statement(availability: &str) -> Option<&'static str> {
    let words = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let got = words(availability);
    TCP_CC0_STATEMENTS
        .iter()
        .find(|(_, text)| words(text) == got)
        .map(|(code, _)| *code)
}

/// TCP's FAQ, archived: the release of every TCP text to the public (open-evidence/tcp.md).
pub const TCP_EVIDENCE_URL: &str =
    "https://web.archive.org/web/20260314193425/https://textcreationpartnership.org/faq/";

/// "Changes we made" for every TCP Book.
pub const TCP_CHANGES: &str = "Converted from TCP's TEI to the Open Bible resource format on \
    download: chapters from its divisions, printed pages from its page breaks, margin notes as \
    footnotes, Bible references as links. The text and its original spelling are unchanged; a \
    word TCP marks as broken at a line's end is joined; figures are left out.";

/// One row of `data/collections/tcp-books.tsv`: a TCP text pinned at a commit of its
/// repository, with what its header says (ADR 0024).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcpBook {
    /// The TCP id (`A53715`, `K070362.001`).
    pub id: String,
    /// The repository commit the file is read at.
    pub commit: String,
    /// Lowercase hex sha256 of the file.
    pub sha256: String,
    pub bytes: u64,
    /// BCP 47.
    pub language: String,
    /// The source edition's date, as the header gives it ("1668.").
    pub date: String,
    /// The author as the header gives it ("Owen, John, 1616-1683.").
    pub author: String,
    /// The title, cut at its first colon (`obs_formats::short_title`).
    pub title: String,
    /// The code of its CC0 statement ([`TCP_CC0_STATEMENTS`]).
    pub statement: String,
}

/// Every source collection row's TSV fields, in order.
const TCP_FIELDS: usize = 9;

impl TcpBook {
    /// The file at `commit` of TCP's repository for `id`.
    pub fn raw_url(id: &str, commit: &str) -> String {
        format!("https://raw.githubusercontent.com/textcreationpartnership/{id}/{commit}/{id}.xml")
    }

    /// The TCP id a raw URL names (as [`Self::raw_url`] writes it).
    pub fn id_of_url(url: &str) -> Option<String> {
        let rest =
            url.strip_prefix("https://raw.githubusercontent.com/textcreationpartnership/")?;
        let (id, rest) = rest.split_once('/')?;
        let (_, file) = rest.split_once('/')?;
        (file == format!("{id}.xml")).then(|| id.to_string())
    }

    /// The TCP collection it belongs to, from its id's letter.
    pub fn collection_name(&self) -> &'static str {
        match self.id.as_bytes().first() {
            Some(b'K') => "ECCO-TCP",
            Some(b'N' | b'W') => "Evans-TCP",
            _ => "EEBO-TCP",
        }
    }

    /// Its Open catalogue entry id: `tcp-` and the TCP id, lowercased.
    pub fn entry_id(&self) -> String {
        format!("tcp-{}", self.id.to_ascii_lowercase().replace('.', "-"))
    }

    /// The row as a TSV line (no newline); tabs and line breaks in a field become spaces.
    pub fn to_tsv(&self) -> String {
        let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
        [
            clean(&self.id),
            clean(&self.commit),
            clean(&self.sha256),
            self.bytes.to_string(),
            clean(&self.language),
            clean(&self.date),
            clean(&self.author),
            clean(&self.title),
            clean(&self.statement),
        ]
        .join("\t")
    }

    /// A row from its TSV line.
    pub fn from_tsv(line: &str) -> Result<Self, String> {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() != TCP_FIELDS {
            return Err(format!("{} fields, not {TCP_FIELDS}: {line:?}", f.len()));
        }
        Ok(Self {
            id: f[0].into(),
            commit: f[1].into(),
            sha256: f[2].into(),
            bytes: f[3].parse().map_err(|_| format!("bytes {:?}", f[3]))?,
            language: f[4].into(),
            date: f[5].into(),
            author: f[6].into(),
            title: f[7].into(),
            statement: f[8].into(),
        })
    }

    /// What is wrong with the row: a malformed id, commit or sha256, an unknown statement,
    /// no title, or an entry id that is not a Resource id.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut bad = |why: &str| out.push(format!("{}: {why}", self.id));
        let hex = |s: &str, n: usize| {
            s.len() == n
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        let (main, part) = self.id.split_once('.').unwrap_or((&self.id, "0"));
        if main.len() < 5
            || !main.as_bytes()[0].is_ascii_uppercase()
            || !main[1..].bytes().all(|b| b.is_ascii_digit())
            || part.is_empty()
            || !part.bytes().all(|b| b.is_ascii_digit())
        {
            bad("not a TCP id");
        }
        if !hex(&self.commit, 40) {
            bad("commit must be 40 lowercase hex digits");
        }
        if !hex(&self.sha256, 64) {
            bad("sha256 must be 64 lowercase hex digits");
        }
        if !TCP_CC0_STATEMENTS.iter().any(|(c, _)| *c == self.statement) {
            bad("not one of TCP's CC0 statements");
        }
        if self.title.trim().is_empty() {
            bad("no title");
        }
        if !obs_model::is_valid_resource_id(&self.entry_id()) {
            bad("its entry id is not a Resource id");
        }
        if self.language.is_empty() {
            bad("no language");
        }
        out
    }

    /// Its Open catalogue entry.
    pub fn source(&self) -> OpenSource {
        let author = display_author(&self.author);
        let date = display_date(&self.date);
        let collection = self.collection_name();
        let by = if author.is_empty() {
            String::new()
        } else {
            format!(", by {author}")
        };
        let edition = [author.as_str(), date.as_str()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
        OpenSource {
            id: self.entry_id(),
            importer: "tcp-tei".into(),
            kind: "book".into(),
            url: Self::raw_url(&self.id, &self.commit),
            sha256: Some(self.sha256.clone()),
            licence_file: None,
            licence_notice_sha256: None,
            title: self.title.clone(),
            abbreviation: self.id.clone(),
            language: self.language.clone(),
            edition: format!("{edition} ({collection} {})", self.id)
                .trim_start()
                .to_string(),
            publisher: "Text Creation Partnership".into(),
            homepage: format!("https://github.com/textcreationpartnership/{}", self.id),
            licence: "CC0 1.0".into(),
            spdx: "CC0-1.0".into(),
            licence_class: LicenceClass::FreeToShare,
            attribution: format!(
                "{}{by}. Keyboarded and encoded by the Text Creation Partnership ({collection} \
                 {}), released under CC0 1.0.",
                self.title, self.id
            ),
            changes: TCP_CHANGES.into(),
            evidence: "open-evidence/tcp.md".into(),
            evidence_url: TCP_EVIDENCE_URL.into(),
            skip_books: Vec::new(),
            author,
            date,
            collection: true,
        }
    }
}

/// Parse `tcp-books.tsv`: lines starting `#` are comments.
pub fn parse_tcp_books(text: &str) -> Result<Vec<TcpBook>, String> {
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| TcpBook::from_tsv(l).map_err(|e| format!("tcp-books.tsv: {e}")))
        .collect()
}

/// The compiled-in TCP collection (empty if it does not parse; a test checks it does).
pub fn tcp_books() -> &'static [TcpBook] {
    static ROWS: OnceLock<Vec<TcpBook>> = OnceLock::new();
    ROWS.get_or_init(|| parse_tcp_books(TCP_BOOKS_TSV).unwrap_or_default())
}

/// A catalogue date as a year to show: the first four-digit year, with its "?" ("1668." →
/// "1668", "MDCLVII [1657]" → "1657", "[1650?]" → "1650?"); else the date as given, without
/// its brackets and full stop.
pub fn display_date(raw: &str) -> String {
    let b = raw.as_bytes();
    for i in 0..b.len().saturating_sub(3) {
        let four = &b[i..i + 4];
        let before = i.checked_sub(1).map(|j| b[j]);
        let after = b.get(i + 4).copied();
        if four.iter().all(u8::is_ascii_digit)
            && !before.is_some_and(|c| c.is_ascii_digit())
            && !after.is_some_and(|c| c.is_ascii_digit())
        {
            let year = &raw[i..i + 4];
            return if after == Some(b'?') {
                format!("{year}?")
            } else {
                year.to_string()
            };
        }
    }
    raw.trim()
        .trim_matches(['[', ']'])
        .trim_end_matches('.')
        .trim()
        .to_string()
}

/// A catalogue author heading as a name to show: "Owen, John, 1616-1683." → "John Owen";
/// dates ("1616-1683", "d. 1555", "ca. 1514-1572") and anything in parentheses
/// ("Westminster Assembly (1643-1652)", "C. H. (Charles Haddon)") dropped; a heading that is
/// not "Surname, Given" ("Church of England.", "Charles I, King of England, 1600-1649.") kept
/// in its order.
pub fn display_author(heading: &str) -> String {
    let heading = heading.trim().trim_end_matches('.').trim();
    let mut s = String::new();
    let mut depth = 0usize;
    for c in heading.chars() {
        match c {
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            c if depth == 0 => s.push(c),
            _ => {}
        }
    }
    let parts: Vec<&str> = s
        .split(',')
        .map(str::trim)
        .filter(|p| {
            !p.is_empty()
                && !p.chars().any(|c| c.is_ascii_digit())
                && !matches!(*p, "d" | "ca" | "fl" | "b")
        })
        .collect();
    let name = match parts.as_slice() {
        [surname, given]
            if given.chars().next().is_some_and(char::is_uppercase) && !given.contains(" of ") =>
        {
            format!("{given} {surname}")
        }
        [surname, given, ..]
            if given.chars().next().is_some_and(char::is_uppercase)
                && !given.contains(" of ")
                && !given.contains(' ') =>
        {
            format!("{given} {surname}")
        }
        parts => parts.join(", "),
    };
    name.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The URL's host, lowercased, without `www.`.
fn host(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = host.rsplit('@').next().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host).to_ascii_lowercase();
    host.strip_prefix("www.").unwrap_or(&host).to_string()
}

/// A refused host anywhere in `url`, including inside an archive wrapper
/// (`web.archive.org/web/…/https://ccel.org/…`).
fn refused_host(url: &str) -> Option<&'static str> {
    let lower = url.to_ascii_lowercase();
    REFUSED_HOSTS.iter().copied().find(|h| {
        lower
            .split("://")
            .skip(1)
            .map(host)
            .any(|found| found == *h || found.ends_with(&format!(".{h}")))
    })
}

impl OpenSource {
    /// The Resource type it imports as.
    pub fn resource_type(&self) -> Option<ResourceType> {
        match crate::get_bibles::type_filter_from_id(&self.kind) {
            Ok(crate::get_bibles::ResourceFilter::Type(t)) => Some(t),
            _ => None,
        }
    }

    /// Its Get Resources row.
    pub fn listing(&self) -> ResourceListing {
        ResourceListing {
            module: self.id.clone(),
            kind: self.resource_type().unwrap_or(ResourceType::Bible),
            repo: self.publisher.clone(),
            title: self.title.clone(),
            abbreviation: self.abbreviation.clone(),
            language: self.language.clone(),
            size: None,
            licence: Some(self.licence.clone()),
            version: None,
            strongs: false,
            morphology: false,
            refused: None,
            source: ListingSource::Open,
            evidence_url: Some(self.evidence_url.clone()),
            declared_licence_class: Some(self.licence_class),
            author: Some(self.author.clone()).filter(|a| !a.is_empty()),
            date: Some(self.date.clone()).filter(|d| !d.is_empty()),
            collection: self.collection,
        }
    }

    /// What is wrong with the entry, for the catalogue's tests: unknown importer or type, a
    /// licence class that does not match its SPDX id or is not Free to share, evidence that
    /// is not archived, a refused host, or a download that is checked neither by sha256 nor
    /// by its licence notice.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut bad = |why: String| out.push(format!("{}: {why}", self.id));
        if !OPEN_IMPORTERS.contains(&self.importer.as_str()) {
            bad(format!("unknown importer `{}`", self.importer));
        }
        match (self.importer.as_str(), self.resource_type()) {
            (_, None) => bad(format!("unknown type `{}`", self.kind)),
            ("usfm-zip", Some(t)) if t != ResourceType::Bible => {
                bad(format!("the usfm-zip importer makes Bibles, not {t:?}"));
            }
            ("tcp-tei", Some(t)) if t != ResourceType::Book => {
                bad(format!("the tcp-tei importer makes Books, not {t:?}"));
            }
            _ => {}
        }
        if !self.url.starts_with("https://") {
            bad("the download must be an https:// URL".into());
        }
        for url in [&self.url, &self.homepage, &self.evidence_url] {
            if let Some(h) = refused_host(url) {
                bad(format!("{url}: files hosted by {h} are never used"));
            }
        }
        let class = licence_class(&self.spdx);
        if class != self.licence_class {
            bad(format!(
                "licence_class {:?} but {} is {class:?}",
                self.licence_class, self.spdx
            ));
        }
        if self.licence_class != LicenceClass::FreeToShare {
            bad("only Free to share works are listed".into());
        }
        if !self
            .evidence_url
            .starts_with("https://web.archive.org/web/")
        {
            bad("evidence_url must be an Internet Archive capture".into());
        }
        if !self.evidence.starts_with("open-evidence/") || !self.evidence.ends_with(".md") {
            bad("evidence must be a file in open-evidence/".into());
        }
        let is_sha256 = |sha: &str| sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit());
        for (name, sha) in [
            ("sha256", &self.sha256),
            ("licence_notice_sha256", &self.licence_notice_sha256),
        ] {
            if sha.as_deref().is_some_and(|s| !is_sha256(s)) {
                bad(format!("{name} must be 64 hex digits"));
            }
        }
        if self.sha256.is_none()
            && (self.licence_file.is_none() || self.licence_notice_sha256.is_none())
        {
            bad("an unpinned download needs licence_file and licence_notice_sha256".into());
        }
        if self.licence_file.is_some() != self.licence_notice_sha256.is_some() {
            bad("licence_file and licence_notice_sha256 go together".into());
        }
        if self.attribution.trim().is_empty() || self.changes.trim().is_empty() {
            bad("attribution and changes are required".into());
        }
        out
    }

    /// Check a download: its sha256 when pinned, and the sha256 of the licence notice inside
    /// it when the entry names one. Any difference refuses it. Returns the download's sha256.
    pub fn check_download(&self, bytes: &[u8]) -> Result<String, String> {
        let got = crate::starter::sha256_hex(bytes);
        if let Some(want) = &self.sha256
            && !want.eq_ignore_ascii_case(&got)
        {
            return Err(format!(
                "the file at {} has changed (sha256 {got}, expected {want}); not imported",
                self.url
            ));
        }
        if let (Some(file), Some(want)) = (&self.licence_file, &self.licence_notice_sha256) {
            let notice = obs_formats::zip_text_file(bytes, file)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| {
                    format!("the download has no licence notice ({file}); not imported")
                })?;
            let got = crate::starter::sha256_hex(notice_text(&notice).as_bytes());
            if !want.eq_ignore_ascii_case(&got) {
                return Err(format!(
                    "the licence notice in the download ({file}) is not the one its evidence \
                     quotes (sha256 {got}, expected {want}); not imported"
                ));
            }
        }
        Ok(got)
    }

    /// Import a checked download as Resource `resource_id`. `progress(done, total)` returns
    /// `false` to cancel. The manifest carries the entry's licence, Attribution and changes,
    /// so the About view and Help → Credits show them.
    pub fn import(
        &self,
        bytes: &[u8],
        resource_id: &str,
        tool_version: &str,
        progress: impl FnMut(usize, usize) -> bool,
    ) -> Result<(Resource, obs_formats::ImportReport), String> {
        let (mut resource, mut report) = match self.importer.as_str() {
            "usfm-zip" => self.import_usfm_zip(bytes, resource_id, tool_version, progress)?,
            "tcp-tei" => self.import_tcp_tei(bytes, resource_id, tool_version, progress)?,
            other => return Err(format!("unknown importer `{other}`")),
        };
        let m = &mut resource.manifest;
        m.edition = Some(self.edition.clone());
        m.publisher = Some(self.publisher.clone());
        m.source_url = Some(self.homepage.clone());
        m.license.clone_from(&self.spdx);
        m.attribution.clone_from(&self.attribution);
        m.changes = Some(self.changes.clone());
        m.origin = Origin::Imported;
        resource.validate().map_err(|e| e.to_string())?;
        report.license_missing = false;
        Ok((resource, report))
    }

    fn import_tcp_tei(
        &self,
        bytes: &[u8],
        resource_id: &str,
        tool_version: &str,
        progress: impl FnMut(usize, usize) -> bool,
    ) -> Result<(Resource, obs_formats::ImportReport), String> {
        let xml = std::str::from_utf8(bytes)
            .map_err(|_| "the TEI text is not UTF-8; not imported".to_string())?;
        let file = self.url.rsplit('/').next().unwrap_or(&self.url).to_string();
        let options = obs_formats::TeiBookOptions {
            resource_id: resource_id.into(),
            title: self.title.clone(),
            abbreviation: self.abbreviation.clone(),
            language: self.language.clone(),
            source_file: file,
            tool_version: tool_version.into(),
        };
        obs_formats::import_tcp_tei(xml, &options, progress).map_err(|e| e.to_string())
    }

    fn import_usfm_zip(
        &self,
        bytes: &[u8],
        resource_id: &str,
        tool_version: &str,
        progress: impl FnMut(usize, usize) -> bool,
    ) -> Result<(Resource, obs_formats::ImportReport), String> {
        use obs_formats::{UsfmImport, UsfmOptions, usfm_book_code, usfm_zip_books};
        let file = self.url.rsplit('/').next().unwrap_or(&self.url).to_string();
        let mut import = UsfmImport::new(UsfmOptions {
            resource_id: resource_id.into(),
            title: self.title.clone(),
            abbreviation: self.abbreviation.clone(),
            language: self.language.clone(),
            source_file: file,
            tool_version: tool_version.into(),
            ..UsfmOptions::default()
        });
        for (name, text) in usfm_zip_books(bytes).map_err(|e| e.to_string())? {
            let code = usfm_book_code(&text).unwrap_or_default();
            if self
                .skip_books
                .iter()
                .any(|b| b.eq_ignore_ascii_case(&code))
            {
                continue;
            }
            import.add_book(&name, text).map_err(|e| e.to_string())?;
        }
        import
            .finish_with_progress(progress)
            .map_err(|e| e.to_string())
    }
}

/// Where eBible's build stamp starts in `copr.htm` ("HTML generated with Haiola by eBible.org
/// 30 Oct 2026 from source files dated …"). It is re-dated on every build, so the notice
/// ends before it.
const EBIBLE_BUILD_STAMP: &str = "<div class=\"fine\">";

/// The licence notice in an HTML licence file, as text: its `<body>` up to eBible's build
/// stamp, read by [`plain_text`]. The whole file is used when either mark is missing.
pub(crate) fn notice_text(html: &str) -> String {
    let body = html.find("<body").map_or(html, |at| &html[at..]);
    let body = body.find(EBIBLE_BUILD_STAMP).map_or(body, |at| &body[..at]);
    plain_text(body)
}

/// HTML reduced to its words, for the licence notice check: tags dropped, entities
/// `&nbsp;`/`&amp;` read, whitespace collapsed.
fn plain_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                out.push(' ');
            }
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    let out = out.replace("&nbsp;", " ").replace("&amp;", "&");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::path::Path;

    use super::*;

    fn data_dir() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("data")
    }

    /// The notice an evidence file quotes: the one line of its `text` block after
    /// "## Pinned notice".
    fn quoted_notice(evidence: &str) -> Option<&str> {
        let (_, rest) = evidence.split_once("## Pinned notice")?;
        let (_, block) = rest.split_once("```text\n")?;
        let (notice, _) = block.split_once("\n```")?;
        Some(notice)
    }

    #[test]
    fn the_compiled_in_catalogue_parses_and_every_entry_is_sound() {
        let sources = parse_open_catalogue(OPEN_CATALOGUE_TOML).unwrap();
        assert!(!sources.is_empty());
        assert_eq!(open_catalogue_toml_sources(), sources.as_slice());
        assert_eq!(&open_sources()[..sources.len()], sources.as_slice());
        let sources = open_sources();
        let mut ids: Vec<&str> = sources.iter().map(|s| s.id.as_str()).collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n, "duplicate ids");
        let problems: Vec<String> = sources.iter().flat_map(OpenSource::problems).collect();
        assert!(problems.is_empty(), "{problems:#?}");
    }

    /// #134 acceptance: at least one eBible.org USFM Bible, downloaded from the publisher.
    #[test]
    fn the_catalogue_offers_ebible_usfm_bibles_from_the_publisher() {
        let ebible: Vec<&OpenSource> = open_sources()
            .iter()
            .filter(|s| s.importer == "usfm-zip" && host(&s.url) == "ebible.org")
            .collect();
        assert!(!ebible.is_empty());
        for s in ebible {
            assert_eq!(s.resource_type(), Some(ResourceType::Bible), "{}", s.id);
            // eBible regenerates its zips: unpinned, checked by the notice inside instead.
            assert_eq!(s.sha256, None, "{}", s.id);
            assert_eq!(s.licence_file.as_deref(), Some("copr.htm"), "{}", s.id);
        }
    }

    /// #134 acceptance: each entry links to archived licence evidence, and the evidence file
    /// names that archive, the download and the licence it relies on.
    #[test]
    fn each_entry_links_to_archived_licence_evidence() {
        // A source collection's rows are their own record (`tcp_collection_rows_are_sound`).
        for s in open_catalogue_toml_sources() {
            let path = data_dir().join(&s.evidence);
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{}: {}: {e}", s.id, path.display()));
            assert!(text.contains(&s.evidence_url), "{}: evidence_url", s.id);
            assert!(text.contains(&s.url), "{}: url", s.id);
            assert!(text.contains(&s.spdx), "{}: spdx", s.id);
            if let Some(sha) = &s.licence_notice_sha256 {
                // The evidence quotes the pinned notice whole, as `notice_text` reads it.
                let quoted = quoted_notice(&text)
                    .unwrap_or_else(|| panic!("{}: evidence quotes no notice", s.id));
                assert_eq!(
                    &crate::starter::sha256_hex(quoted.as_bytes()),
                    sha,
                    "{}: the quoted notice is not the pinned one",
                    s.id
                );
                assert!(
                    text.contains(sha.as_str()),
                    "{}: licence_notice_sha256",
                    s.id
                );
            }
            if let Some(sha) = &s.sha256 {
                assert!(text.contains(sha.as_str()), "{}: sha256", s.id);
            }
        }
        // No evidence file is left over from a removed entry.
        for entry in std::fs::read_dir(data_dir().join("open-evidence")).unwrap() {
            let name = format!(
                "open-evidence/{}",
                entry.unwrap().file_name().to_string_lossy()
            );
            assert!(
                open_sources().iter().any(|s| s.evidence == name),
                "{name} is not cited by any entry"
            );
        }
    }

    /// #345: every row of the TCP collection is pinned and carries one of TCP's CC0
    /// statements, which the evidence quotes; none repeats a hand-written entry; each makes a
    /// sound entry, listed in All only.
    #[test]
    fn tcp_collection_rows_are_sound() {
        let rows = parse_tcp_books(TCP_BOOKS_TSV).unwrap();
        assert_eq!(tcp_books(), rows.as_slice());
        assert!(TCP_BOOKS_TSV.starts_with(TCP_BOOKS_HEADER));
        let problems: Vec<String> = rows.iter().flat_map(TcpBook::problems).collect();
        assert!(problems.is_empty(), "{problems:#?}");
        let mut ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        assert!(
            ids.windows(2).all(|w| w[0] < w[1]),
            "sorted by id, no repeats"
        );
        ids.dedup();
        let evidence = std::fs::read_to_string(data_dir().join("open-evidence/tcp.md")).unwrap();
        let words = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
        for (code, statement) in TCP_CC0_STATEMENTS {
            assert!(
                words(&evidence).contains(&words(statement)),
                "{code} is not quoted"
            );
            assert!(evidence.contains(code), "{code} is not named");
        }
        assert!(evidence.contains(TCP_EVIDENCE_URL));
        assert!(evidence.contains("tcp-books.tsv"));
        let listed: Vec<String> = open_catalogue_toml_sources()
            .iter()
            .filter_map(|s| TcpBook::id_of_url(&s.url))
            .collect();
        assert_eq!(listed.len(), 10, "the hand-written TCP entries");
        for row in &rows {
            assert!(
                !listed.contains(&row.id),
                "{} is a hand-written entry too",
                row.id
            );
            let source = row.source();
            assert_eq!(source.problems(), Vec::<String>::new(), "{}", row.id);
            assert_eq!(
                TcpBook::id_of_url(&source.url).as_deref(),
                Some(row.id.as_str())
            );
            assert!(source.collection && source.listing().collection);
            assert_eq!(TcpBook::from_tsv(&row.to_tsv()).as_ref(), Ok(row));
        }
        let all = open_sources();
        assert_eq!(all.len(), open_catalogue_toml_sources().len() + rows.len());
        let mut entry_ids: Vec<&str> = all.iter().map(|s| s.id.as_str()).collect();
        entry_ids.sort_unstable();
        let n = entry_ids.len();
        entry_ids.dedup();
        assert_eq!(entry_ids.len(), n, "entry ids are unique");
    }

    /// The statements as `read_tei_header` reads them from real TCP files: the Evans text
    /// carries the Phase I statement, the Westminster Confession (Phase II) the waiver.
    #[test]
    fn tcp_statements_match_the_files() {
        let fixture = |name: &str| {
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../formats/tests/assets/tcp")
                .join(name);
            obs_formats::read_tei_header(&std::fs::read_to_string(path).unwrap()).unwrap()
        };
        assert_eq!(
            tcp_statement(&fixture("N03833.xml").availability),
            Some("cc0-phase1")
        );
        assert_eq!(
            tcp_statement(&fixture("A96226.xml").availability),
            Some("cc0-waiver")
        );
        assert_eq!(tcp_statement("This text is available for reuse."), None);
        assert_eq!(tcp_statement(""), None);
    }

    #[test]
    fn a_tcp_row_makes_an_entry_and_reads_back() {
        let row = TcpBook {
            id: "A53715".into(),
            commit: "588ac23f659abf650a1391e7a67517a48b4b7df8".into(),
            sha256: "4603d4fd73838e698e523487eebb04841f242be20f61b01c3a510bfa35cd661d".into(),
            bytes: 615_441,
            language: "en".into(),
            date: "1668.".into(),
            author: "Owen, John, 1616-1683.".into(),
            title: "Of the mortification of sin in believers".into(),
            statement: "cc0-phase1".into(),
        };
        assert!(row.problems().is_empty(), "{:?}", row.problems());
        let s = row.source();
        assert_eq!(s.id, "tcp-a53715");
        assert_eq!(s.author, "John Owen");
        assert_eq!(s.date, "1668");
        assert_eq!(s.edition, "John Owen, 1668 (EEBO-TCP A53715)");
        assert_eq!(s.resource_type(), Some(ResourceType::Book));
        assert!(s.attribution.starts_with(
            "Of the mortification of sin in believers, by John Owen. Keyboarded and encoded"
        ));
        assert_eq!(TcpBook::from_tsv(&row.to_tsv()), Ok(row.clone()));
        let mut bad = row.clone();
        bad.statement = "cc-by".into();
        bad.commit = "HEAD".into();
        assert_eq!(bad.problems().len(), 2, "{:?}", bad.problems());
        let mut part = row;
        part.id = "K070362.001".into();
        assert!(part.problems().is_empty());
        assert_eq!(part.entry_id(), "tcp-k070362-001");
        assert_eq!(part.collection_name(), "ECCO-TCP");
        assert_eq!(TcpBook::id_of_url("https://example.org/A1/x/A1.xml"), None);
    }

    #[test]
    fn dates_read_as_years() {
        for (raw, year) in [
            ("1668.", "1668"),
            ("MDCLVII [1657]", "1657"),
            ("Anno. 1574]", "1574"),
            ("[1650?]", "1650?"),
            ("M.DC.XXX.", "M.DC.XXX"),
            ("", ""),
        ] {
            assert_eq!(display_date(raw), year, "{raw:?}");
        }
    }

    #[test]
    fn author_headings_read_as_names() {
        for (heading, name) in [
            ("Owen, John, 1616-1683.", "John Owen"),
            ("Flavel, John, 1630?-1691.", "John Flavel"),
            ("Hooper, John, d. 1555.", "John Hooper"),
            ("Knox, John, ca. 1514-1572.", "John Knox"),
            ("Westminster Assembly (1643-1652)", "Westminster Assembly"),
            ("Church of England.", "Church of England"),
            (
                "Charles I, King of England, 1600-1649.",
                "Charles I, King of England",
            ),
            (
                "Spurgeon, C. H. (Charles Haddon), 1834-1892.",
                "C. H. Spurgeon",
            ),
            ("", ""),
        ] {
            assert_eq!(display_author(heading), name, "{heading:?}");
        }
    }

    #[test]
    fn ccel_is_refused_even_inside_an_archive_capture() {
        assert_eq!(refused_host("https://www.ccel.org/x.zip"), Some("ccel.org"));
        assert_eq!(
            refused_host("https://web.archive.org/web/2020id_/https://ccel.org/h/henry.zip"),
            Some("ccel.org")
        );
        assert_eq!(refused_host("https://ebible.org/Scriptures/x.zip"), None);
        let mut s = open_sources()[0].clone();
        s.url = "https://ccel.org/ccel/n/nave/bible.xml".into();
        assert!(
            s.problems().iter().any(|p| p.contains("never used")),
            "{:?}",
            s.problems()
        );
    }

    #[test]
    fn an_entry_whose_class_does_not_match_its_licence_is_a_problem() {
        let mut s = open_sources()[0].clone();
        s.spdx = "CC-BY-NC-4.0".into();
        let problems = s.problems();
        assert!(
            problems.iter().any(|p| p.contains("licence_class")),
            "{problems:?}"
        );
        s.licence_class = LicenceClass::PersonalUse;
        s.licence = "CC BY-NC 4.0".into();
        assert!(
            s.problems()
                .iter()
                .any(|p| p.contains("only Free to share")),
            "{:?}",
            s.problems()
        );
        let mut unpinned = open_sources()[0].clone();
        unpinned.sha256 = None;
        unpinned.licence_notice_sha256 = None;
        assert!(
            unpinned
                .problems()
                .iter()
                .any(|p| p.contains("unpinned download")),
            "{:?}",
            unpinned.problems()
        );
    }

    /// eBible re-dates its build stamp on every build; the notice before it is what is
    /// pinned, so the same notice built on another day has the same sha256.
    #[test]
    fn the_notice_ends_before_ebibles_build_stamp() {
        let built = |day: &str| {
            format!(
                "<html><head><title>YLT</title></head><body><p>Public Domain</p>\
                 <div class=\"fine\">HTML generated by eBible.org {day} 2026</div></body></html>"
            )
        };
        assert_eq!(notice_text(&built("30 Oct")), "Public Domain");
        assert_eq!(notice_text(&built("30 Oct")), notice_text(&built("2 Dec")));
        // Without the marks the whole file is the notice.
        assert_eq!(notice_text("<p>Public Domain</p>"), "Public Domain");
    }

    #[test]
    fn notices_are_read_as_plain_text() {
        assert_eq!(
            plain_text("<p>Creative Commons\n <b>Attribution</b>&nbsp;Share-Alike</p>"),
            "Creative Commons Attribution Share-Alike"
        );
    }
}
