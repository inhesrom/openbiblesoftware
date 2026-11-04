//! Importers and dataset converters writing into `ResourceBuilder`: USFM, Lexicons,
//! OpenBible.info Cross-references, STEPBible TIPNR, Starter Commentaries and Dictionaries,
//! and the TVTMS Verse-map builder.
//!
//! Layer L2. No SQLite and no UI here; compiles for `wasm32-unknown-unknown`.

mod commentary;
mod dataset;
mod lexicon;
mod openbible;
mod report;
mod text_buf;
mod tipnr;
mod usfm;
mod usfm_tree;
mod verse_map;

pub use commentary::{DictionaryData, StudyImport, import_dictionary, import_jfb_book, import_mhc};
pub use dataset::{DatasetError, DatasetOptions};
pub use lexicon::{
    LexiconError, LexiconOptions, import_abbott_smith, import_bdb, import_strongs_greek,
    import_strongs_hebrew, import_tbes,
};
pub use openbible::{DROPPED_NOT_STANDARD, OPENBIBLE_FORMAT, import_openbible_cross_references};
pub use report::{BookReport, ImportReport, VerseMapCounts};
pub use tipnr::{TIPNR_FORMAT, import_tipnr, tipnr_description_texts};
pub use usfm::{ImportError, UsfmImport, UsfmOptions, slugify, usfm_book_code};
pub use verse_map::{VerseFacts, VerseMapOutcome, build_verse_map, tvtms_rule_count};
