//! Bible books, Verses, Passages, the Standard versification and Strong's numbers.
//!
//! Layer L0. This crate must stay free of UI dependencies and compile for
//! `wasm32-unknown-unknown` (checked by `cargo xtask layers` and `cargo xtask wasm`).
//!
//! ```
//! use obs_scripture::{Book, Passage, Standard, Verse, Versification};
//!
//! let ps51 = Passage::chapter(&Standard, Book::Psalms, 51).unwrap();
//! assert_eq!(ps51.start(), Verse::new(Book::Psalms, 51, 0)); // title slot
//! assert!(ps51.contains_verse(Verse::new(Book::Psalms, 51, 10)));
//! assert_eq!(Standard.ordinal(Verse::new(Book::Genesis, 1, 1)), Some(0));
//! ```

mod book;
mod standard_data;
mod strongs;
mod verse;
mod versification;

pub use book::{Book, Testament};
pub use strongs::{StrongsLanguage, StrongsNumber, StrongsParseError};
pub use verse::{Passage, PassageError, PassageList, Verse};
pub use versification::{Standard, Versification};
