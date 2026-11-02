//! Importers and dataset converters (USFM, SWORD, STEPBible, OpenBible.info, lexicons) and the TVTMS Verse-map builder, all writing into `ResourceBuilder`.
//!
//! Layer L2. This crate must stay free of UI dependencies and compile for
//! `wasm32-unknown-unknown` (checked by `cargo xtask layers` and `cargo xtask wasm`).
