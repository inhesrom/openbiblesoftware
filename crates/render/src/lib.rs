//! Glyph rasterization into the shared glyph atlas, quads for laid blocks (ADR 0004
//! amendment), and a CPU compositor of those quads into RGBA tiles (the Mobile app's). No
//! egui: the UI layer uploads the atlas and paints the quads, or shows the tiles.
//!
//! Layer L3. Compiles for `wasm32-unknown-unknown`.

mod atlas;
mod composite;
mod raster;

pub use atlas::{AtlasStats, GlyphAtlas, GlyphQuad, MAX_SIZE, PlacedGlyphs, SUBPIXEL_BINS};
pub use composite::{RoleColours, composite, composite_over, tile_size};
pub use raster::Rasterizer;
