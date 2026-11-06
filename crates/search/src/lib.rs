//! Search query parser, AST, scope resolution, result model and the `SearchIndex` trait.
//!
//! Layer L2. This crate must stay free of UI dependencies and compile for
//! `wasm32-unknown-unknown` (checked by `cargo xtask layers` and `cargo xtask wasm`).

pub mod find;
pub mod highlight;
pub mod index;
pub mod morph;
pub mod query;
pub mod stem;

pub use find::{FindOptions, Finder};
pub use highlight::Highlighter;
pub use index::{
    DocumentKind, HitOrder, INDEX_VERSION, SearchError, SearchHit, SearchIndex, SearchOptions,
};
pub use morph::{MorphCategory, MorphCriteria, MorphCriterion};
pub use query::{
    Compiled, Field, IndexQuery, IndexVocabulary, LemmaLookup, NoLexicon, Pattern, Plan, Query,
    QueryError, QueryErrorKind, compile, compile_notes, parse, to_fts5,
};
pub use stem::{stem, stem_prefix};

#[cfg(test)]
mod find_tests;
#[cfg(test)]
mod highlight_tests;
#[cfg(test)]
mod query_tests;
#[cfg(test)]
mod stem_tests;
