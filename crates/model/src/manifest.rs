//! The manifest: one row of metadata that describes a Resource (#11 rule 11).

use serde::{Deserialize, Serialize};

/// The `.obr` span vocabulary and model version. Bump it whenever the meaning of stored data
/// changes (new span kind, changed attrs, new dataset tables); the engine opens only versions
/// it knows (ADR 0003). Version 2 added the Cross-reference and People & places tables.
/// Version 3 compacts span kinds and attrs without changing their logical content.
pub const FORMAT_VERSION: u32 = 3;

/// The kind of a Resource. Language is a separate attribute, so a Hebrew original is a Bible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceType {
    Bible,
    Commentary,
    Lexicon,
    Dictionary,
    CrossReferenceSet,
    PeopleAndPlaces,
}

/// The kind of a Key: what a Resource is organized by and can be opened at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyScheme {
    Passage,
    StrongsNumber,
    Lemma,
    Headword,
}

/// Base direction of a Resource's script.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    #[default]
    Ltr,
    Rtl,
}

/// Where a Resource came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Built by `xtask starter` and bundled with the installer.
    Starter,
    /// Imported by the user from a local file.
    Imported,
}

/// SPDX id used when the source carries no licence (e.g. USFM); the Import report warns.
pub const LICENSE_UNKNOWN: &str = "LicenseRef-unknown";

/// Metadata for one Resource. `content_hash` is filled in by [`crate::ResourceBuilder::build`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Stable slug, e.g. `kjv-strongs`. Never reused for a different edition.
    pub resource_id: String,
    pub title: String,
    pub abbreviation: String,
    pub resource_type: ResourceType,
    /// BCP 47 language tag, e.g. `en`, `hbo`, `grc`.
    pub language: String,
    pub direction: Direction,
    pub key_schemes: Vec<KeyScheme>,
    pub edition: Option<String>,
    /// Monotonic Resource revision; a corrected release keeps the id and bumps this.
    pub revision: u32,
    /// `sha256:<hex>` over the content (units, spans, verses, keys); set at build time.
    pub content_hash: String,
    pub publisher: Option<String>,
    pub source_url: Option<String>,
    /// SPDX licence expression, or [`LICENSE_UNKNOWN`].
    pub license: String,
    /// Credit text shown verbatim in the About view.
    pub attribution: String,
    /// Changes made to the source (required for CC BY data).
    pub changes: Option<String>,
    pub origin: Origin,
    /// Source format, e.g. `usfm`, `sword`, `stepbible-tahot`.
    pub source_format: String,
    pub source_file: Option<String>,
    pub import_tool_version: String,
    pub format_version: u32,
}

impl Manifest {
    /// A manifest with the required fields set and everything else empty or defaulted.
    pub fn new(
        resource_id: impl Into<String>,
        title: impl Into<String>,
        abbreviation: impl Into<String>,
        resource_type: ResourceType,
        language: impl Into<String>,
    ) -> Self {
        Self {
            resource_id: resource_id.into(),
            title: title.into(),
            abbreviation: abbreviation.into(),
            resource_type,
            language: language.into(),
            direction: Direction::Ltr,
            key_schemes: Vec::new(),
            edition: None,
            revision: 1,
            content_hash: String::new(),
            publisher: None,
            source_url: None,
            license: LICENSE_UNKNOWN.to_string(),
            attribution: String::new(),
            changes: None,
            origin: Origin::Imported,
            source_format: String::new(),
            source_file: None,
            import_tool_version: String::new(),
            format_version: FORMAT_VERSION,
        }
    }

    pub fn declares(&self, scheme: KeyScheme) -> bool {
        self.key_schemes.contains(&scheme)
    }

    /// Whether this is a Starter Resource under a CC BY licence, so the Starter build
    /// publishes a [`changes_record_file`] for it (#20 decision 4).
    pub fn has_changes_record(&self) -> bool {
        self.origin == Origin::Starter && is_cc_by(&self.license)
    }

    /// Whether text copied, exported or printed from this Resource must carry its Attribution
    /// (#154): it has one, and its licence is not one that asks for none.
    pub fn requires_attribution(&self) -> bool {
        !self.attribution.trim().is_empty() && !attribution_optional(&self.license)
    }
}

/// Licences that ask for no credit: public domain and CC0. Anything else, including an
/// unknown licence, is taken to require the Attribution.
const NO_ATTRIBUTION_LICENSES: &[&str] = &[
    "LicenseRef-PublicDomain",
    "LicenseRef-PD",
    "CC0-1.0",
    "CC-PDDC",
    "Unlicense",
];

/// Whether an SPDX expression lets copies go without credit: one of its `OR` alternatives
/// is public domain or CC0 (`LicenseRef-PD OR CC-BY-3.0` may be used as public domain).
pub fn attribution_optional(spdx: &str) -> bool {
    spdx.split(" OR ").any(|alternative| {
        let alternative = alternative.trim().trim_matches(['(', ')']).trim();
        NO_ATTRIBUTION_LICENSES.contains(&alternative)
    })
}

/// Whether an SPDX expression names a Creative Commons Attribution licence (`CC-BY-4.0`,
/// `CC-BY-SA-4.0`, …), which asks for the changes made to be indicated.
pub fn is_cc_by(spdx: &str) -> bool {
    spdx.split(|c: char| c.is_whitespace() || c == '(' || c == ')')
        .any(|license| license.starts_with("CC-BY-"))
}

/// The Creative Commons deed of the first CC BY licence in an SPDX expression, e.g.
/// `https://creativecommons.org/licenses/by/4.0/` for `CC-BY-4.0`.
pub fn cc_by_license_url(spdx: &str) -> Option<String> {
    let token = spdx
        .split(|c: char| c.is_whitespace() || c == '(' || c == ')')
        .find(|license| license.starts_with("CC-BY-"))?;
    let (kind, version) = token.strip_prefix("CC-")?.rsplit_once('-')?;
    if version.is_empty() || !version.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return None;
    }
    Some(format!(
        "https://creativecommons.org/licenses/{}/{version}/",
        kind.to_ascii_lowercase()
    ))
}

/// The file name of a CC BY Starter Resource's published change record.
pub fn changes_record_file(resource_id: &str) -> String {
    format!("CHANGES-{resource_id}.md")
}

/// Whether `id` is a valid `resource_id`: 1–64 chars of `a-z`, `0-9` and `-`, not starting or
/// ending with `-`.
pub fn is_valid_resource_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && !id.starts_with('-')
        && !id.ends_with('-')
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cc_by_is_found_anywhere_in_an_spdx_expression() {
        for yes in [
            "CC-BY-4.0",
            "CC-BY-SA-4.0",
            "(MIT AND CC-BY-4.0)",
            "LicenseRef-PD OR CC-BY-3.0",
        ] {
            assert!(is_cc_by(yes), "{yes}");
        }
        for no in [
            "CC0-1.0",
            "LicenseRef-PublicDomain",
            "MIT",
            "LicenseRef-CC-BY-4.0",
            "",
        ] {
            assert!(!is_cc_by(no), "{no}");
        }
    }

    #[test]
    fn only_cc_by_starter_resources_have_a_changes_record() {
        let mut m = Manifest::new("tahot", "TAHOT", "TAHOT", ResourceType::Bible, "hbo");
        m.license = "CC-BY-4.0".into();
        assert!(!m.has_changes_record(), "imported");
        m.origin = Origin::Starter;
        assert!(m.has_changes_record());
        m.license = "LicenseRef-PublicDomain".into();
        assert!(!m.has_changes_record());
        assert_eq!(changes_record_file("tahot"), "CHANGES-tahot.md");
    }

    #[test]
    fn only_resources_whose_licence_asks_for_credit_require_their_attribution() {
        let mut m = Manifest::new("tagnt", "TAGNT", "TAGNT", ResourceType::Bible, "grc");
        m.attribution = "Data created by www.STEPBible.org, CC BY 4.0.".into();
        for (license, required) in [
            ("CC-BY-4.0", true),
            ("CC-BY-SA-4.0", true),
            ("LicenseRef-Copyrighted-Permission-to-distribute", true),
            (LICENSE_UNKNOWN, true),
            ("LicenseRef-PublicDomain", false),
            ("LicenseRef-PD", false),
            ("CC0-1.0", false),
            ("LicenseRef-PD OR CC-BY-3.0", false),
            ("MIT AND CC-BY-4.0", true),
        ] {
            m.license = license.into();
            assert_eq!(m.requires_attribution(), required, "{license}");
        }
        m.license = "CC-BY-4.0".into();
        m.attribution = "  ".into();
        assert!(!m.requires_attribution(), "nothing to append");
    }

    #[test]
    fn cc_by_licences_link_to_their_deed() {
        assert_eq!(
            cc_by_license_url("CC-BY-4.0").as_deref(),
            Some("https://creativecommons.org/licenses/by/4.0/")
        );
        assert_eq!(
            cc_by_license_url("MIT AND CC-BY-SA-3.0").as_deref(),
            Some("https://creativecommons.org/licenses/by-sa/3.0/")
        );
        assert_eq!(cc_by_license_url("CC0-1.0"), None);
        assert_eq!(cc_by_license_url("CC-BY-"), None);
    }
}
