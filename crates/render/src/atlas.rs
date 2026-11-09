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
    /// The count ([`GlyphAtlas::take_used_area`]) the glyph was last placed in.
    used: u64,
}

impl Entry {
    /// Atlas area the glyph takes, the gap after it included.
    fn area(&self) -> usize {
        (usize::from(self.w) + 1) * (usize::from(self.h) + 1)
    }
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

/// Part of a block's glyphs placed in the atlas ([`GlyphAtlas::quads_from`]).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlacedGlyphs {
    pub quads: Vec<GlyphQuad>,
    /// Where the atlas ran out of room: the index of the first glyph not placed, to go on
    /// from once these quads are drawn and the atlas has started over. `None`: all placed.
    pub resume: Option<usize>,
}

/// What the atlas has for a glyph.
enum Slot {
    Ink(Entry),
    /// No outline (a space) or none that can be rastered: nothing to draw.
    Blank,
    /// No room left in the largest atlas.
    Full,
}

/// Counters for the performance overlay and tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AtlasStats {
    pub glyphs: usize,
    pub rasters: u64,
    pub side: usize,
    /// Bumped when the atlas grows or starts over: textures must be re-uploaded whole.
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
    /// The current count of [`Self::take_used_area`], and the area it has so far.
    used: (u64, usize),
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
    let face = face(key.face).ok()?;
    let font = face.font_ref()?;
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
            used: (0, 0),
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

    /// Atlas area (device px², gaps included) of the distinct glyphs placed since the last
    /// call: what a frame's text needs of the atlas, when called once a frame. A glyph placed
    /// again after [`Self::start_over`] counts again.
    pub fn take_used_area(&mut self) -> usize {
        let (count, area) = self.used;
        self.used = (count + 1, 0);
        area
    }

    /// The region changed since the last call, `[x0, y0, x1, y1]`, for a partial upload.
    pub fn take_dirty(&mut self) -> Option<[usize; 4]> {
        self.dirty.take()
    }

    /// Double the atlas, keeping every glyph where it is (so quads made before stay right
    /// in texels); `false` at [`MAX_SIZE`].
    fn grow(&mut self) -> bool {
        if self.side >= MAX_SIZE {
            return false;
        }
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
        self.row_height = 0;
        self.dirty = None;
        self.generation += 1;
        true
    }

    /// Empty the atlas so it packs anew, at the size it has. Quads made before sample the
    /// texture as it was: draw them with that texture, not with this one (#255).
    pub fn start_over(&mut self) {
        self.coverage.iter_mut().for_each(|a| *a = 0);
        self.map.clear();
        self.cursor = (1, 1);
        self.row_height = 0;
        self.dirty = None;
        self.generation += 1;
    }

    /// Room for a `w` × `h` glyph, growing the atlas if it must; `None` when even the
    /// largest atlas is full.
    fn alloc(&mut self, w: usize, h: usize) -> Option<(usize, usize)> {
        loop {
            if self.cursor.0 + w + 1 > self.side {
                self.cursor = (1, self.cursor.1 + self.row_height + 1);
                self.row_height = 0;
            }
            if self.cursor.1 + h + 1 > self.side {
                if self.grow() {
                    continue;
                }
                return None;
            }
            let p = self.cursor;
            self.cursor.0 += w + 1;
            self.row_height = self.row_height.max(h);
            return Some(p);
        }
    }

    fn entry(&mut self, key: GlyphKey) -> Slot {
        if let Some(slot) = self.map.get_mut(&key) {
            let Some(e) = slot else {
                return Slot::Blank;
            };
            if e.used != self.used.0 {
                e.used = self.used.0;
                self.used.1 += e.area();
            }
            return Slot::Ink(*e);
        }
        self.rasters += 1;
        let Some((cov, w, h, left, top)) = raster(key) else {
            self.map.insert(key, None);
            return Slot::Blank;
        };
        // A glyph with no room is not kept: it is rastered again once there is room.
        let Some((x, y)) = self.alloc(w, h) else {
            return Slot::Full;
        };
        for row in 0..h {
            let dst = (y + row) * self.side + x;
            self.coverage[dst..dst + w].copy_from_slice(&cov[row * w..(row + 1) * w]);
        }
        let d = self.dirty.get_or_insert([x, y, x + w, y + h]);
        *d = [d[0].min(x), d[1].min(y), d[2].max(x + w), d[3].max(y + h)];
        let e = Entry {
            x: x as u16,
            y: y as u16,
            w: w as u16,
            h: h as u16,
            left,
            top,
            used: self.used.0,
        };
        self.used.1 += e.area();
        self.map.insert(key, Some(e));
        Slot::Ink(e)
    }

    /// Quads for every glyph of a laid block at `scale` device px per point, with the block's
    /// top-left at `origin` (points), sampling the atlas as it is when this returns. Positions
    /// snap to device pixels vertically and to a quarter pixel horizontally.
    ///
    /// When the glyphs do not all fit with those already in the atlas, it starts over once
    /// ([`Self::start_over`]); glyphs past what even an empty atlas holds are left out. To
    /// draw every glyph whatever their number, draw in parts with [`Self::quads_from`].
    pub fn quads(&mut self, block: &LaidBlock, origin: [f32; 2], scale: f32) -> Vec<GlyphQuad> {
        let everywhere = [f32::NEG_INFINITY, f32::INFINITY];
        let placed = self.quads_from(block, origin, scale, everywhere, 0);
        if placed.resume.is_none() {
            return placed.quads;
        }
        self.start_over();
        self.quads_from(block, origin, scale, everywhere, 0).quads
    }

    /// Quads for the glyphs of a laid block from its `from`th on (in [`LaidBlock::glyphs`]
    /// order), as [`Self::quads`] places them, for the lines that reach into `visible` (a
    /// range of y, in the same points as `origin`). It stops where the atlas, grown to
    /// [`MAX_SIZE`], has no room left: then [`PlacedGlyphs::resume`] says where to go on
    /// from once this part is drawn and the atlas has started over. It never starts over
    /// itself, so every quad it returns samples the atlas as it is when it returns.
    pub fn quads_from(
        &mut self,
        block: &LaidBlock,
        origin: [f32; 2],
        scale: f32,
        visible: [f32; 2],
        from: usize,
    ) -> PlacedGlyphs {
        let mut placed: Vec<(Entry, f32, f32, ColorRole)> = Vec::new();
        let mut resume = None;
        for (i, (line, item, x, y, glyph)) in block.glyphs().enumerate().skip(from) {
            // A line's ink can reach past its box (marks, descenders): a text size of slack.
            let size = item.style.size;
            if origin[1] + line.bottom + size < visible[0]
                || origin[1] + line.top - size > visible[1]
            {
                continue;
            }
            let px = size * scale;
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
            match self.entry(key) {
                Slot::Ink(e) => placed.push((e, xi, dev_y, item.style.role)),
                Slot::Blank => {}
                Slot::Full => {
                    resume = Some(i);
                    break;
                }
            }
        }
        // Growing kept every glyph in place, so all of them are normalized by the final side.
        let s = self.side as f32;
        let quads = placed
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
        PlacedGlyphs { quads, resume }
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

    /// Every character from U+0020 to U+2FFF the Regular face has (controls and spaces left
    /// out), a space after each, at `size`: at Text size 40 and 2x, far more glyphs than the
    /// largest atlas holds at once (AUD-LAYOUT-TEXT-1, #255).
    fn oversized_block(size: f32) -> LaidBlock {
        let regular = obs_fonts::face(FaceId::Regular).unwrap();
        let text: String = ('\u{20}'..='\u{2FFF}')
            .filter(|&c| !c.is_control() && !c.is_whitespace() && regular.covers(c))
            .flat_map(|c| [c, ' '])
            .collect();
        assert_eq!(text.chars().count(), 4622);
        let p = snippet_paragraph(
            &text,
            ColorRole::Ink,
            false,
            &TextSettings {
                size,
                ..TextSettings::default()
            },
        );
        layout(&p, 600.0, ShapeCache::shared())
    }

    /// Runs `f` on a thread and fails if it has not returned in a minute.
    fn bounded<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        rx.recv_timeout(std::time::Duration::from_secs(60))
            .expect("GlyphAtlas::quads did not return")
    }

    #[test]
    fn a_block_larger_than_the_atlas_returns() {
        let (quads, stats) = bounded(|| {
            let mut atlas = GlyphAtlas::new();
            let quads = atlas.quads(&oversized_block(40.0), [0.0, 0.0], 2.0);
            (quads.len(), atlas.stats())
        });
        assert!(quads > 0);
        assert_eq!(stats.side, MAX_SIZE);
    }

    /// Ink under a quad, in the atlas as it is.
    fn ink(atlas: &GlyphAtlas, q: &GlyphQuad) -> u32 {
        let side = atlas.side() as f32;
        let [x0, y0] = q.uv_min.map(|v| (v * side).round() as usize);
        let [x1, y1] = q.uv_max.map(|v| (v * side).round() as usize);
        (y0..y1)
            .flat_map(|y| (x0..x1).map(move |x| (x, y)))
            .map(|(x, y)| u32::from(atlas.coverage()[y * atlas.side() + x]))
            .sum()
    }

    #[test]
    fn a_block_larger_than_the_atlas_is_drawn_whole_in_parts() {
        let b = oversized_block(40.0);
        let mut atlas = GlyphAtlas::new();
        let (mut from, mut parts, mut quads) = (0, 0, 0);
        loop {
            let placed = atlas.quads_from(&b, [0.0, 0.0], 2.0, [f32::MIN, f32::MAX], from);
            parts += 1;
            quads += placed.quads.len();
            // Each part samples the atlas as it is before it starts over.
            assert!(placed.quads.iter().all(|q| ink(&atlas, q) > 0));
            match placed.resume {
                Some(next) => {
                    assert!(next > from, "no progress from glyph {from}");
                    from = next;
                    atlas.start_over();
                }
                None => break,
            }
        }
        assert!(parts > 1, "the block fit in one atlas");
        assert!(parts < 20, "{parts} parts");
        // Every glyph with ink, once: one for each character but the spaces after them,
        // less the few with no outline.
        let glyphs = b.glyphs().count();
        assert!(
            quads * 2 + 100 >= glyphs && quads * 2 <= glyphs,
            "{quads} of {glyphs}"
        );
    }

    #[test]
    fn only_the_lines_in_view_are_placed() {
        let b = oversized_block(40.0);
        let mut atlas = GlyphAtlas::new();
        let placed = atlas.quads_from(&b, [0.0, -500.0], 2.0, [0.0, 800.0], 0);
        assert_eq!(placed.resume, None, "a screenful filled the atlas");
        assert!(placed.quads.len() > 100, "{}", placed.quads.len());
        // Lines reaching into view, give or take a text size and a line.
        for q in &placed.quads {
            assert!(q.max[1] > -120.0 && q.min[1] < 920.0, "{q:?}");
        }
        assert!(placed.quads.len() * 4 < b.glyphs().count());
    }

    #[test]
    fn growing_keeps_every_glyph_where_it_was() {
        let mut atlas = GlyphAtlas::new();
        let grace = block("Grace", false);
        let before = atlas.quads(&grace, [0.0, 0.0], 2.0);
        let side = atlas.side() as f32;
        let at = |quads: &[GlyphQuad], side: f32| -> Vec<[f32; 2]> {
            quads.iter().map(|q| q.uv_min.map(|v| v * side)).collect()
        };
        let texels = at(&before, side);
        atlas.quads(&oversized_block(17.0), [0.0, 0.0], 2.0);
        assert!(atlas.side() > side as usize, "the atlas did not grow");
        let after = atlas.quads(&grace, [0.0, 0.0], 2.0);
        assert_eq!(at(&after, atlas.side() as f32), texels);
        assert!(after.iter().all(|q| ink(&atlas, q) > 0));
    }

    #[test]
    fn used_area_counts_each_glyph_placed_since_the_last_take_once() {
        let mut atlas = GlyphAtlas::new();
        let grace = block("Grace", false);
        atlas.quads(&grace, [0.0, 0.0], 2.0);
        let once = atlas.take_used_area();
        assert!(once > 0);
        // Drawn twice in one count, or again in the next with nothing rastered: the same.
        atlas.quads(&grace, [0.0, 0.0], 2.0);
        atlas.quads(&grace, [0.0, 0.0], 2.0);
        assert_eq!(atlas.take_used_area(), once);
        assert_eq!(atlas.take_used_area(), 0);
        // Half the device scale: about a quarter of the area, never more.
        atlas.quads(&grace, [0.0, 0.0], 1.0);
        let half = atlas.take_used_area();
        assert!(half * 2 < once && once <= half * 4, "{half} vs {once}");
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
