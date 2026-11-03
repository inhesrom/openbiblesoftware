//! Resource manifest, text units, the standoff span vocabulary, Keys, `ResourceBuilder`, and
//! the shared text normalizer and transliteration.
//!
//! Layer L1. No SQLite and no UI here: the `.obr` writer in `obs-store` turns a [`Resource`]
//! into a file (ADR 0003). Must compile for `wasm32-unknown-unknown`.
//!
//! ```
//! use obs_model::*;
//! use obs_scripture::{Book, Verse};
//!
//! let mut manifest = Manifest::new("web", "World English Bible", "WEB", ResourceType::Bible, "en");
//! manifest.key_schemes = vec![KeyScheme::Passage];
//! manifest.license = "LicenseRef-PD".into();
//! let mut b = ResourceBuilder::new(manifest);
//! let jhn = UnitId::book(Book::John);
//! b.add_unit(jhn.clone(), UnitKind::BibleBook, None, "For God so loved the world.".into());
//! b.add_verse(&jhn, 0..27, Verse::new(Book::John, 3, 16), 16);
//! let resource = b.build().unwrap();
//! assert!(resource.manifest.content_hash.starts_with("sha256:"));
//! assert_eq!(normalize("Ἐν ἀρχῇ"), "εν αρχη");
//! assert_eq!(transliterate("ἀγάπη"), "agape");
//! ```

mod builder;
mod dataset;
mod key;
mod manifest;
pub mod morph;
mod normalize;
mod translit;
mod unit;

pub use builder::{Resource, ResourceBuilder, ValidationError, ValidationErrors};
pub use dataset::{CrossReference, Entity, EntityKind, EntityRelation, Mention, Relation};
pub use key::{Key, KeyEntry};
pub use manifest::{
    Direction, FORMAT_VERSION, KeyScheme, LICENSE_UNKNOWN, Manifest, Origin, ResourceType,
    is_valid_resource_id,
};
pub use morph::{Morph, MorphError, MorphLanguage, MorphPart, PartOfSpeech};
pub use normalize::{NORMALIZER_VERSION, Token, normalize, tokenize};
pub use translit::{TRANSLITERATION_VERSION, transliterate};
pub use unit::{
    BibleVerse, MorphemePart, NoteKind, ParaStyle, Span, SpanKind, TextAccumulator, TextUnit,
    UnitId, UnitIdError, UnitKind,
};
