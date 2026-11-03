//! Importers and dataset converters, all writing into `ResourceBuilder`: USFM, STEPBible
//! TAHOT/TAGNT and the TVTMS Verse-map builder; SWORD, OpenBible.info and lexicons in later
//! tickets.
//!
//! Layer L2. No SQLite and no UI here; compiles for `wasm32-unknown-unknown`.

mod report;
mod stepbible;
mod text_buf;
mod usfm;
mod usfm_tree;
mod verse_map;

pub use report::{BookReport, ImportReport, VerseMapCounts};
pub use stepbible::{StepbibleImport, StepbibleOptions, StepbibleText};
pub use usfm::{ImportError, UsfmImport, UsfmOptions, slugify, usfm_book_code};
pub use verse_map::{VerseFacts, VerseMapOutcome, build_verse_map, tvtms_rule_count};
