//! Why an Import failed outright (USFM or SWORD).

use std::fmt;

use obs_model::ValidationErrors;

/// Why an Import failed outright.
#[derive(Debug)]
pub enum ImportError {
    NoBooks,
    /// The caller asked the Import to stop.
    Cancelled,
    /// A file without a `\id` line.
    MissingId(String),
    DuplicateBook {
        code: String,
        first: String,
        second: String,
    },
    InvalidResourceId(String),
    Usj(String),
    Invalid(ValidationErrors),
    /// The SWORD reader could not read the library or module.
    Sword(String),
    /// No SWORD module of that name in the library.
    UnknownModule {
        module: String,
        available: Vec<String>,
    },
    /// A SWORD module of a kind v0.1 does not import (general books, devotionals, locked).
    UnsupportedModule {
        module: String,
        reason: String,
    },
    /// A SWORD module with no entries.
    EmptyModule(String),
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::NoBooks => f.write_str("no Bible books found"),
            ImportError::Cancelled => f.write_str("Import cancelled"),
            ImportError::MissingId(name) => write!(f, "{name}: no \\id line"),
            ImportError::DuplicateBook {
                code,
                first,
                second,
            } => write!(f, "book {code} appears in both {first} and {second}"),
            ImportError::InvalidResourceId(id) => write!(
                f,
                "resource id `{id}` must be 1-64 chars of a-z, 0-9 and `-`"
            ),
            ImportError::Usj(e) => write!(f, "could not read parser output: {e}"),
            ImportError::Invalid(e) => write!(f, "imported resource is invalid: {e}"),
            ImportError::Sword(e) => write!(f, "SWORD: {e}"),
            ImportError::UnknownModule { module, available } => {
                write!(f, "no SWORD module `{module}` here")?;
                if available.is_empty() {
                    f.write_str(" (no modules found)")
                } else {
                    write!(f, " (found: {})", available.join(", "))
                }
            }
            ImportError::UnsupportedModule { module, reason } => {
                write!(f, "SWORD module `{module}` cannot be imported: {reason}")
            }
            ImportError::EmptyModule(module) => {
                write!(f, "SWORD module `{module}` has no entries")
            }
        }
    }
}

impl std::error::Error for ImportError {}
