//! The manifest: one row of metadata that describes a Resource (#11 rule 11).

use serde::{Deserialize, Serialize};

/// The `.obr` span vocabulary and model version. Bump it whenever the meaning of stored data
/// changes (new span kind, changed attrs); the engine opens only versions it knows (ADR 0003).
pub const FORMAT_VERSION: u32 = 1;

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
