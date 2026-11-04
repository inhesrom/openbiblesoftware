//! Shared plumbing for the dataset converters (OpenBible.info Cross-references, TIPNR People &
//! places): options, errors and the empty Resource they fill in.

use std::fmt;

use obs_model::{
    FORMAT_VERSION, KeyScheme, LICENSE_UNKNOWN, Manifest, Origin, ResourceBuilder, ResourceType,
    ValidationErrors, is_valid_resource_id,
};

use crate::report::ImportReport;

/// Identity and metadata for the dataset Resource a converter creates.
#[derive(Debug, Clone, Default)]
pub struct DatasetOptions {
    pub resource_id: String,
    pub title: String,
    pub abbreviation: String,
    /// BCP 47; defaults to `en`.
    pub language: String,
    /// File name shown in About and the report.
    pub source_file: String,
    /// Version of the tool doing the conversion, recorded in the manifest.
    pub tool_version: String,
}

/// Why a dataset conversion failed outright.
#[derive(Debug)]
pub enum DatasetError {
    InvalidResourceId(String),
    /// A line the converter cannot read; `line` is 1-based.
    Syntax {
        line: usize,
        message: String,
    },
    NoRecords,
    /// Text from a column the converter must never copy ended up in the Resource.
    ExcludedTextKept {
        entity: String,
        text: String,
    },
    Invalid(ValidationErrors),
}

impl fmt::Display for DatasetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DatasetError::InvalidResourceId(id) => write!(
                f,
                "resource id `{id}` must be 1-64 chars of a-z, 0-9 and `-`"
            ),
            DatasetError::Syntax { line, message } => write!(f, "line {line}: {message}"),
            DatasetError::NoRecords => f.write_str("no records found"),
            DatasetError::ExcludedTextKept { entity, text } => write!(
                f,
                "entity `{entity}` would keep excluded description text `{text}`"
            ),
            DatasetError::Invalid(e) => write!(f, "converted resource is invalid: {e}"),
        }
    }
}

impl std::error::Error for DatasetError {}

/// An empty dataset Resource of `resource_type`, keyed by Passage, and its report.
pub(crate) fn start(
    o: &DatasetOptions,
    resource_type: ResourceType,
    source_format: &str,
) -> Result<(ResourceBuilder, ImportReport), DatasetError> {
    if !is_valid_resource_id(&o.resource_id) {
        return Err(DatasetError::InvalidResourceId(o.resource_id.clone()));
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
    let language = if o.language.is_empty() {
        "en".to_string()
    } else {
        o.language.clone()
    };
    let mut manifest = Manifest::new(
        &o.resource_id,
        &title,
        abbreviation,
        resource_type,
        language,
    );
    manifest.key_schemes = vec![KeyScheme::Passage];
    manifest.license = LICENSE_UNKNOWN.to_string();
    manifest.origin = Origin::Imported;
    manifest.source_format = source_format.into();
    manifest.source_file = (!o.source_file.is_empty()).then(|| o.source_file.clone());
    manifest.import_tool_version = o.tool_version.clone();
    manifest.format_version = FORMAT_VERSION;
    let report = ImportReport {
        resource_id: o.resource_id.clone(),
        title,
        source_format: source_format.into(),
        source_files: (!o.source_file.is_empty())
            .then(|| o.source_file.clone())
            .into_iter()
            .collect(),
        license_missing: true,
        ..ImportReport::default()
    };
    Ok((ResourceBuilder::new(manifest), report))
}
