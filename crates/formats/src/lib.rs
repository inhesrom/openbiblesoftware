//! Importers and dataset converters, all writing into `ResourceBuilder`: USFM, OpenBible.info
//! Cross-references and STEPBible TIPNR now; SWORD, other STEPBible data and lexicons in later
//! tickets; plus the TVTMS Verse-map builder.
//!
//! Layer L2. No SQLite and no UI here; compiles for `wasm32-unknown-unknown`.

mod dataset;
mod openbible;
mod report;
mod text_buf;
mod tipnr;
mod usfm;
mod usfm_tree;
mod verse_map;

pub use dataset::{DatasetError, DatasetOptions};
pub use openbible::{DROPPED_NOT_STANDARD, OPENBIBLE_FORMAT, import_openbible_cross_references};
pub use report::{BookReport, ImportReport, VerseMapCounts};
pub use tipnr::{TIPNR_FORMAT, import_tipnr, tipnr_description_texts};
pub use usfm::{ImportError, UsfmImport, UsfmOptions, slugify, usfm_book_code};
pub use verse_map::{VerseFacts, VerseMapOutcome, build_verse_map, tvtms_rule_count};
