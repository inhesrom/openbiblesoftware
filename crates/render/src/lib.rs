//! Glyph rasterization into the shared glyph atlas (no egui).
//!
//! Layer L3. This crate must stay free of UI dependencies and compile for
//! `wasm32-unknown-unknown` (checked by `cargo xtask layers` and `cargo xtask wasm`).
