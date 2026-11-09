//! Glyph rasterization into the shared glyph atlas, and quads for laid blocks (ADR 0004
//! amendment). No egui: the UI layer uploads the atlas and paints the quads.
//!
//! Layer L3. Compiles for `wasm32-unknown-unknown`.

mod atlas;
mod raster;

pub use atlas::{AtlasStats, GlyphAtlas, GlyphQuad, MAX_SIZE, PlacedGlyphs, SUBPIXEL_BINS};
pub use raster::Rasterizer;
