//! Layout of Resource text: spans → styled runs with colour roles, UAX #9 bidi, UAX #14 line
//! breaking, blocks, line boxes, and hit-testing back to `(unit, byte)` (ADR 0004, #14).
//!
//! Layer L2. No UI, no RGB: colours are [`ColorRole`]s that the painter resolves (#21).
//! Compiles for `wasm32-unknown-unknown`.
//!
//! ```
//! use obs_layout::{TextSettings, layout, snippet_paragraph, ColorRole};
//! use obs_fonts::ShapeCache;
//!
//! let para = snippet_paragraph("In the beginning God created", ColorRole::Ink, false, &TextSettings::default());
//! let block = layout(&para, 120.0, ShapeCache::shared());
//! assert!(block.lines.len() >= 2);
//! let hit = block.hit(block.lines[0].items[0].x + 1.0, block.lines[0].baseline - 2.0).unwrap();
//! assert_eq!(hit.bytes.start, 0);
//! ```

mod estimate;
mod interlinear;
mod layout;
mod paragraph;
mod present;
mod spans;
mod style;

pub use estimate::estimate_height;
pub use interlinear::{
    INTERLINEAR_SCALE, Interlinear, InterlinearWord, add_interlinear, tagged_words,
};
pub use layout::{Hit, LaidBlock, LaidCell, LaidItem, LaidLine, UnitPos, layout};
pub use paragraph::{Align, InlineObject, Paragraph, Run, RunSource};
pub use present::{
    BlockKind, BlockSource, SYNTHETIC_SMALL_CAPS, SnippetStyle, block_paragraph, footnote_label,
    formatted_snippet_paragraph, snippet_paragraph, starts_rtl, styled_snippet_paragraph,
    unit_blocks,
};
pub use spans::SpanIndex;
pub use style::{ColorRole, RunStyle, TextSettings, VerseNumbers};

#[cfg(test)]
mod interlinear_tests;
#[cfg(test)]
mod reading_font_tests;
#[cfg(test)]
mod tests;
