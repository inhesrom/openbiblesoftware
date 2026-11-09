//! Bundled fonts, harfrust shaping and the shaped-segment cache shared by all threads.
//!
//! Layer L1. No UI here; compiles for `wasm32-unknown-unknown`.
//!
//! - Bundled faces: Gentium Plus (English, polytonic Greek; `smcp` for the divine name) and Ezra SIL
//!   (Hebrew with niqqud and cantillation), both OFL (ADR 0004). They are compiled in.
//! - Reading fonts (#138): the app may load other faces at runtime ([`load_user_face`]) and
//!   choose them per script ([`ReadingFaces`]); [`script_coverage`] reads a face's cmap.
//! - [`shape_uncached`] / [`ShapeCache::shape`] turn one segment (one face, style and direction)
//!   into positioned glyphs with a cluster → byte mapping, for hit-testing (#14 rule 8).
//!
//! ```
//! use obs_fonts::{FaceId, ShapeCache, ShapeStyle};
//!
//! let style = ShapeStyle::new(FaceId::Regular, 20.0, false, false);
//! let run = ShapeCache::shared().shape("In the beginning", style).unwrap();
//! assert_eq!(run.missing_glyphs(), 0);
//! assert_eq!(run.cluster_at(3).unwrap().bytes, 3..4);
//! ```

mod face;
mod licenses;
mod preview;
mod reading;
mod shape;

pub use face::{
    Face, FaceError, FaceId, FaceMetrics, face, has_outlines, load_user_face, script_of,
    unload_user_face,
};
pub use licenses::{FONT_LICENSES, FontLicense};
pub use preview::{lines_svg, run_path};
pub use reading::{Coverage, FaceSet, ReadingFaces, ReadingScript, script_coverage};
pub use shape::{
    CacheStats, Cluster, Glyph, ShapeCache, ShapeStyle, ShapedRun, ink_top, shape_uncached,
    small_caps_source,
};
pub use unicode_script::Script;

#[cfg(test)]
mod tests;
