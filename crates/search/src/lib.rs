//! Search query parser, AST, scope resolution, result model and the `SearchIndex` trait.
//!
//! Layer L2. This crate must stay free of UI dependencies and compile for
//! `wasm32-unknown-unknown` (checked by `cargo xtask layers` and `cargo xtask wasm`).
