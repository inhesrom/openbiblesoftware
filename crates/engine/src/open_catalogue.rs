//! The Open catalogue (#134): openly licensed Resources that are not SWORD modules, listed in
//! Get Resources beside the SWORD repositories' catalogs. Each entry of
//! `data/open-catalogue.toml` names the publisher's own URL, the importer that reads it, its
//! licence and licence class, and archived licence evidence (`data/open-evidence/`), in the
//! style of the Starter library lock (`starter/sources.toml`).
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

/// The importers an Open catalogue entry may name. `usfm-zip`: a publisher's zip of USFM
/// books (eBible.org's `<id>_usfm.zip`) → a Bible.
pub const OPEN_IMPORTERS: &[&str] = &["usfm-zip"];

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

/// The compiled-in Open catalogue (empty if it does not parse; a test checks it does).
pub fn open_sources() -> &'static [OpenSource] {
    static SOURCES: OnceLock<Vec<OpenSource>> = OnceLock::new();
    SOURCES.get_or_init(|| parse_open_catalogue(OPEN_CATALOGUE_TOML).unwrap_or_default())
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
        match self.resource_type() {
            Some(ResourceType::Bible) => {}
            Some(t) if self.importer == "usfm-zip" => {
                bad(format!("the usfm-zip importer makes Bibles, not {t:?}"));
            }
            Some(_) => {}
            None => bad(format!("unknown type `{}`", self.kind)),
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
        assert_eq!(open_sources(), sources.as_slice());
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
        for s in open_sources() {
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
