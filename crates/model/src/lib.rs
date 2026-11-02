//! Resource manifest, text units, standoff span vocabulary, Keys, `ResourceBuilder`, Annotation and Workspace types, and the shared text normalizer.
//!
//! Layer L1. This crate must stay free of UI dependencies and compile for
//! `wasm32-unknown-unknown` (checked by `cargo xtask layers` and `cargo xtask wasm`).
