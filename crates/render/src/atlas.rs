//! The shared glyph atlas (ADR 0004 amendment, #19): every glyph of Resource text is rastered
//! once into a single-channel atlas, subpixel-binned horizontally, and blocks are drawn as
//! textured quads. Colour is a role per quad, resolved by the painter, so a theme or Page tone
//! change never re-rasters.

use std::collections::HashMap;

use obs_fonts::{FaceId, face};
use obs_layout::{ColorRole, LaidBlock};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{GlyphId, MetadataProvider};

use crate::raster::Rasterizer;

/// Horizontal subpixel positions per pixel.
pub const SUBPIXEL_BINS: u8 = 4;
const START_SIZE: usize = 512;
/// Largest atlas side. 2048² is 4 MiB of coverage (16 MiB as RGBA once uploaded), inside the
/// 32 MB text budget whatever the Panel count (#19).
pub const MAX_SIZE: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphKey {
    face: FaceId,
    glyph: u32,
    /// Size in device px × 4.
    px_q: u32,
    bin: u8,
}

/// Where a glyph sits in the atlas, and its offset from the pen (device px, y down).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Entry {
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    left: f32,
    top: f32,
}

/// One glyph to draw: a rectangle in logical points and its texture coordinates (0..1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphQuad {
    pub min: [f32; 2],
    pub max: [f32; 2],
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    pub role: ColorRole,
}

/// Counters for the performance overlay and tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AtlasStats {
    pub glyphs: usize,
    pub rasters: u64,
    pub side: usize,
    /// Bumped when the atlas is regrown or reset: textures must be re-uploaded whole.
    pub generation: u64,
}

/// A growable single-channel glyph atlas packed in shelves.
pub struct GlyphAtlas {
    side: usize,
    coverage: Vec<u8>,
    cursor: (usize, usize),
    row_height: usize,
    map: HashMap<GlyphKey, Option<Entry>>,
    dirty: Option<[usize; 4]>,
    generation: u64,
    rasters: u64,
}

impl Default for GlyphAtlas {
    fn default() -> Self {
        Self::new()
    }
}

struct Pen<'a> {
    r: &'a mut Rasterizer,
    dx: f32,
    dy: f32,
}

impl OutlinePen for Pen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.r.move_to(x + self.dx, self.dy - y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.r.line_to(x + self.dx, self.dy - y);
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.r
            .quad_to(cx + self.dx, self.dy - cy, x + self.dx, self.dy - y);
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.r.cubic_to(
            (cx0 + self.dx, self.dy - cy0),
            (cx1 + self.dx, self.dy - cy1),
            x + self.dx,
            self.dy - y,
        );
    }
    fn close(&mut self) {
        self.r.close();
    }
}

/// Bounds of a glyph outline at a size, in px (y down), via a dry run.
struct Bounds {
    min: (f32, f32),
    max: (f32, f32),
}

impl OutlinePen for Bounds {
    fn move_to(&mut self, x: f32, y: f32) {
        self.add(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.add(x, y);
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.add(cx, cy);
        self.add(x, y);
    }
    fn curve_to(&mut self, a: f32, b: f32, c: f32, d: f32, x: f32, y: f32) {
        self.add(a, b);
        self.add(c, d);
        self.add(x, y);
    }
    fn close(&mut self) {}
}

impl Bounds {
    fn add(&mut self, x: f32, y: f32) {
        let y = -y;
        self.min = (self.min.0.min(x), self.min.1.min(y));
        self.max = (self.max.0.max(x), self.max.1.max(y));
    }
}

/// Raster one glyph: coverage, width, height, and its top-left offset from the pen.
fn raster(key: GlyphKey) -> Option<(Vec<u8>, usize, usize, f32, f32)> {
    let font = face(key.face).ok()?.font_ref()?;
    let outlines = font.outline_glyphs();
    let glyph = outlines.get(GlyphId::new(key.glyph))?;
    let size = Size::new(key.px_q as f32 / 4.0);
    let dx = f32::from(key.bin) / f32::from(SUBPIXEL_BINS);
    let mut b = Bounds {
        min: (f32::MAX, f32::MAX),
        max: (f32::MIN, f32::MIN),
    };
    glyph
        .draw(DrawSettings::unhinted(size, LocationRef::default()), &mut b)
        .ok()?;
    if b.max.0 <= b.min.0 || b.max.1 <= b.min.1 {
        return None;
    }
    let left = (b.min.0 + dx).floor() - 1.0;
    let top = b.min.1.floor() - 1.0;
    let w = ((b.max.0 + dx).ceil() - left) as usize + 1;
    let h = (b.max.1.ceil() - top) as usize + 1;
    if w > 512 || h > 512 {
        return None;
    }
    let mut r = Rasterizer::new(w, h);
    let mut pen = Pen {
        r: &mut r,
        dx: dx - left,
        dy: -top,
    };
    glyph
        .draw(
            DrawSettings::unhinted(size, LocationRef::default()),
            &mut pen,
        )
        .ok()?;
    Some((r.finish(), w, h, left, top))
}

impl GlyphAtlas {
    pub fn new() -> Self {
        Self {
            side: START_SIZE,
            coverage: vec![0; START_SIZE * START_SIZE],
            cursor: (1, 1),
            row_height: 0,
            map: HashMap::new(),
            dirty: None,
            generation: 0,
            rasters: 0,
        }
    }

    pub fn side(&self) -> usize {
        self.side
    }

    /// Coverage bytes, row-major, `side × side`.
    pub fn coverage(&self) -> &[u8] {
        &self.coverage
    }

    /// Bytes of coverage held (CPU side).
    pub fn bytes(&self) -> usize {
        self.coverage.len()
    }

    pub fn stats(&self) -> AtlasStats {
        AtlasStats {
            glyphs: self.map.len(),
            rasters: self.rasters,
            side: self.side,
            generation: self.generation,
        }
    }

    /// The region changed since the last call, `[x0, y0, x1, y1]`, for a partial upload.
    pub fn take_dirty(&mut self) -> Option<[usize; 4]> {
        self.dirty.take()
    }

    fn grow_or_reset(&mut self) {
        if self.side < MAX_SIZE {
            let ns = self.side * 2;
            let mut c = vec![0; ns * ns];
            for y in 0..self.side {
                c[y * ns..y * ns + self.side]
                    .copy_from_slice(&self.coverage[y * self.side..(y + 1) * self.side]);
            }
            self.coverage = c;
            // Keep packing below the old content.
            self.cursor = (1, self.side + 1);
            self.side = ns;
        } else {
            self.coverage.iter_mut().for_each(|a| *a = 0);
            self.map.clear();
            self.cursor = (1, 1);
        }
        self.row_height = 0;
        self.dirty = None;
        self.generation += 1;
    }

    fn alloc(&mut self, w: usize, h: usize) -> (usize, usize) {
        loop {
            if self.cursor.0 + w + 1 > self.side {
                self.cursor = (1, self.cursor.1 + self.row_height + 1);
                self.row_height = 0;
            }
            if self.cursor.1 + h + 1 > self.side {
                self.grow_or_reset();
                continue;
            }
            let p = self.cursor;
            self.cursor.0 += w + 1;
            self.row_height = self.row_height.max(h);
            return p;
        }
    }

    fn entry(&mut self, key: GlyphKey) -> Option<Entry> {
        if let Some(e) = self.map.get(&key) {
            return *e;
        }
        self.rasters += 1;
        let e = raster(key).map(|(cov, w, h, left, top)| {
            let (x, y) = self.alloc(w, h);
            for row in 0..h {
                let dst = (y + row) * self.side + x;
                self.coverage[dst..dst + w].copy_from_slice(&cov[row * w..(row + 1) * w]);
            }
            let d = self.dirty.get_or_insert([x, y, x + w, y + h]);
            *d = [d[0].min(x), d[1].min(y), d[2].max(x + w), d[3].max(y + h)];
            Entry {
                x: x as u16,
                y: y as u16,
                w: w as u16,
                h: h as u16,
                left,
                top,
            }
        });
        self.map.insert(key, e);
        e
    }

    /// Quads for every glyph of a laid block at `scale` device px per point, with the block's
    /// top-left at `origin` (points). Positions snap to device pixels vertically and to a
    /// quarter pixel horizontally.
    pub fn quads(&mut self, block: &LaidBlock, origin: [f32; 2], scale: f32) -> Vec<GlyphQuad> {
        loop {
            let generation = self.generation;
            let mut placed: Vec<(Entry, f32, f32, ColorRole)> = Vec::new();
            for (line, item, x, y, glyph) in block.glyphs() {
                let _ = line;
                let px = item.style.size * scale;
                let px_q = (px * 4.0).round() as u32;
                let dev_x = (origin[0] + x) * scale;
                let xi = dev_x.floor();
                let bin = (((dev_x - xi) * f32::from(SUBPIXEL_BINS)) as u8).min(SUBPIXEL_BINS - 1);
                let dev_y = ((origin[1] + y) * scale).round();
                let key = GlyphKey {
                    face: item.style.face,
                    glyph,
                    px_q,
                    bin,
                };
                if let Some(e) = self.entry(key) {
                    placed.push((e, xi, dev_y, item.style.role));
                }
            }
            if self.generation != generation {
                // The atlas grew or reset part-way: earlier entries may have moved.
                continue;
            }
            let s = self.side as f32;
            return placed
                .into_iter()
                .map(|(e, xi, y, role)| {
                    let x0 = (xi + e.left) / scale;
                    let y0 = (y + e.top) / scale;
                    GlyphQuad {
                        min: [x0, y0],
                        max: [x0 + f32::from(e.w) / scale, y0 + f32::from(e.h) / scale],
                        uv_min: [f32::from(e.x) / s, f32::from(e.y) / s],
                        uv_max: [f32::from(e.x + e.w) / s, f32::from(e.y + e.h) / s],
                        role,
                    }
                })
                .collect();
        }
    }
}

#[cfg(test)]
mod tests {
    use obs_fonts::ShapeCache;
    use obs_layout::{TextSettings, layout, snippet_paragraph};

    use super::*;

    fn block(text: &str, rtl: bool) -> LaidBlock {
        let p = snippet_paragraph(text, ColorRole::Ink, rtl, &TextSettings::default());
        layout(&p, 400.0, ShapeCache::shared())
    }

    #[test]
    fn glyphs_are_rastered_once_and_reused() {
        let mut atlas = GlyphAtlas::new();
        let b = block("In the beginning God created the heaven", false);
        let q1 = atlas.quads(&b, [0.0, 0.0], 1.0);
        let rasters = atlas.stats().rasters;
        assert!(rasters > 0 && rasters <= q1.len() as u64 + 8);
        let q2 = atlas.quads(&b, [0.0, 0.0], 1.0);
        assert_eq!(q1, q2);
        assert_eq!(atlas.stats().rasters, rasters, "second draw rastered again");
        // Spaces have no outline and no quad.
        assert!(q1.len() < b.text.chars().count());
        // Every quad has ink.
        for q in &q1 {
            let (x0, y0) = (
                (q.uv_min[0] * atlas.side() as f32) as usize,
                (q.uv_min[1] * atlas.side() as f32) as usize,
            );
            let (x1, y1) = (
                (q.uv_max[0] * atlas.side() as f32) as usize,
                (q.uv_max[1] * atlas.side() as f32) as usize,
            );
            let ink: u32 = (y0..y1)
                .flat_map(|y| (x0..x1).map(move |x| (x, y)))
                .map(|(x, y)| u32::from(atlas.coverage()[y * atlas.side() + x]))
                .sum();
            assert!(ink > 0);
        }
    }

    #[test]
    fn device_scale_changes_raster_new_glyphs() {
        let mut atlas = GlyphAtlas::new();
        let b = block("Grace", false);
        atlas.quads(&b, [0.0, 0.0], 1.0);
        let at1 = atlas.stats().rasters;
        let q2 = atlas.quads(&b, [0.0, 0.0], 2.0);
        assert!(atlas.stats().rasters > at1, "2x needs its own glyphs");
        // Quads are in points: the same size at either scale.
        let q1 = atlas.quads(&b, [0.0, 0.0], 1.0);
        let w1 = q1.last().unwrap().max[0] - q1[0].min[0];
        let w2 = q2.last().unwrap().max[0] - q2[0].min[0];
        // Quads carry one device pixel of raster padding per side: 1 pt at 1x, 0.5 pt at 2x.
        assert!((w1 - w2).abs() < 2.5, "{w1} vs {w2}");
    }

    #[test]
    fn colour_is_a_role_per_quad() {
        let mut atlas = GlyphAtlas::new();
        let b = block("בְּרֵאשִׁ֖ית", true);
        let q = atlas.quads(&b, [0.0, 0.0], 1.5);
        assert!(!q.is_empty());
        assert!(q.iter().all(|q| q.role == ColorRole::Ink));
    }

    #[test]
    fn atlas_grows_and_stays_within_budget() {
        let mut atlas = GlyphAtlas::new();
        let text: String = ('A'..='Z').chain('a'..='z').collect();
        for size in 0..40 {
            let p = snippet_paragraph(
                &text,
                ColorRole::Ink,
                false,
                &TextSettings {
                    size: 10.0 + size as f32 * 2.0,
                    ..TextSettings::default()
                },
            );
            let b = layout(&p, 4000.0, ShapeCache::shared());
            atlas.quads(&b, [0.0, 0.0], 1.0);
        }
        assert!(atlas.side() > START_SIZE);
        assert!(atlas.bytes() <= MAX_SIZE * MAX_SIZE);
    }
}
