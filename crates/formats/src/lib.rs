//! Importers and dataset converters writing into `ResourceBuilder`: USFM, Markdown books, Lexicons,
//! OpenBible.info Cross-references, a USFM Bible's parallel passages, STEPBible TAHOT/TAGNT and TIPNR, Starter Commentaries and
//! Dictionaries, and the TVTMS Verse-map builder.
//!
//! Layer L2. No SQLite and no UI here; compiles for `wasm32-unknown-unknown`.

mod commentary;
mod dataset;
mod error;
#[cfg(test)]
mod hostile_input;
mod html_book;
mod lexicon;
mod markdown_book;
mod openbible;
mod openbible_geo;
mod parallels;
mod report;
mod stepbible;
mod sword;
mod tcp_tei;
mod text_buf;
mod theographic;
mod tipnr;
mod usfm;
mod usfm_tree;
mod usfm_zip;
mod verse_map;

pub use commentary::{
    DictionaryData, StudyImport, import_dictionary, import_jfb_book, import_mhc,
    scripture_references, scripture_references_in,
};
pub use dataset::{DatasetError, DatasetOptions};
pub use error::ImportError;
pub use html_book::{
    HtmlBookOptions, MAX_HTML_BYTES, PG_HTML_FORMAT, decode_html_bytes, import_pg_html,
};
pub use lexicon::{
    LexiconError, LexiconOptions, import_abbott_smith, import_bdb, import_strongs_greek,
    import_strongs_hebrew, import_tbes,
};
pub use markdown_book::{
    BookFile, BookFiles, MARKDOWN_BOOK_FORMAT, MAX_BOOK_FILE_BYTES, MarkdownBookOptions,
    book_license_spdx, book_stem, group_book_files, import_markdown_book, is_markdown_name,
    split_front_matter,
};
pub use openbible::{DROPPED_NOT_STANDARD, OPENBIBLE_FORMAT, import_openbible_cross_references};
pub use openbible_geo::{OPENBIBLE_GEO_FORMAT, import_openbible_geocoding};
pub use parallels::{
    DROPPED_PARALLEL_NOT_STANDARD, DROPPED_PARALLEL_SELF, DROPPED_PARALLEL_SPANS_BOOKS,
    USFM_PARALLELS_FORMAT, import_usfm_parallel_passages,
};
pub use report::{BookReport, ImportReport, VerseMapCounts};
pub use stepbible::{StepbibleImport, StepbibleOptions, StepbibleText};
pub use sword::repo as sword_repo;
pub use sword::{SwordModuleInfo, SwordOptions, SwordSource, import_sword, sword_import_type};
pub use tcp_tei::{
    MAX_TEI_BYTES, TCP_TEI_FORMAT, TeiBookOptions, TeiHeader, import_tcp_tei, read_tei_header,
    short_title, tei_header,
};
pub use theographic::{ERAS, EraBound, EraRule, THEOGRAPHIC_FORMAT, import_theographic_events};
pub use tipnr::{TIPNR_FORMAT, import_tipnr, tipnr_description_texts};
pub use usfm::{UsfmImport, UsfmOptions, slugify, usfm_book_code};
pub use usfm_zip::{usfm_zip_books, zip_text_file};
pub use verse_map::{VerseFacts, VerseMapOutcome, build_verse_map, tvtms_rule_count};
