//! The only crate that uses SQLite: `.obr` reader/writer, User database and Search index caches.
//!
//! Layer L2. This crate must stay free of UI dependencies and compile for
//! `wasm32-unknown-unknown` (checked by `cargo xtask layers` and `cargo xtask wasm`).
