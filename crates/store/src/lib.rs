//! The only crate that uses SQLite: the `.obr` Resource file reader and writer (ADR 0003), and
//! later the User database and Search index caches.
//!
//! Layer L2. The storage traits ([`ResourceStore`]) compile everywhere, including
//! `wasm32-unknown-unknown`. The SQLite implementations ([`ObrReader`], [`write_obr`]) need
//! rusqlite's bundled C SQLite, which does not build for `wasm32-unknown-unknown`; they are
//! compiled only off wasm until a sqlite-wasm-rs backend lands (#12 rule 5).

// Pure column encodings; the wasm backend will use them too once it exists.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
mod codec;
mod error;
#[cfg(not(target_arch = "wasm32"))]
mod obr;
mod resource_store;
#[cfg(not(target_arch = "wasm32"))]
mod sample;
#[cfg(not(target_arch = "wasm32"))]
mod user;

pub use error::StoreError;
#[cfg(not(target_arch = "wasm32"))]
pub use obr::{APPLICATION_ID, ObrReader, write_obr};
pub use resource_store::{ResourceStore, Summary};
#[cfg(not(target_arch = "wasm32"))]
pub use sample::sample_resource;
#[cfg(not(target_arch = "wasm32"))]
pub use user::{OpenReport, ROLLING_BACKUPS, USER_APPLICATION_ID, USER_SCHEMA_VERSION, UserStore};
