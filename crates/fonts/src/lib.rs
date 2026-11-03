//! Bundled fonts and harfrust/skrifa shaping with a shared shaped-segment cache.
//!
//! Layer L1. This crate must stay free of UI dependencies and compile for
//! `wasm32-unknown-unknown` (checked by `cargo xtask layers` and `cargo xtask wasm`).
