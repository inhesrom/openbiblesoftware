//! A CPU compositor for glyph quads (plan `docs/plans/mobile-ios.md` A4, option B): a tile of
//! RGBA8 pixels, premultiplied, filled with a background and the quads drawn over it, each in
//! its role's colour with the atlas coverage as alpha. The Mobile app shows such tiles as
//! images; the desktop draws the same quads on the GPU instead.

use obs_layout::ColorRole;

use crate::atlas::{GlyphAtlas, GlyphQuad};

/// What a tile is painted in: its background and a colour for each [`ColorRole`]. Colours
/// are sRGB with straight (not premultiplied) alpha, `[r, g, b, a]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoleColours {
    pub background: [u8; 4],
    pub ink: [u8; 4],
    pub verse_number: [u8; 4],
    pub link: [u8; 4],
    pub note_marker: [u8; 4],
    pub accent: [u8; 4],
    pub words_of_jesus: [u8; 4],
    pub interlinear: [u8; 4],
}

impl RoleColours {
    pub fn of(&self, role: ColorRole) -> [u8; 4] {
        match role {
            ColorRole::Ink => self.ink,
            ColorRole::VerseNumber => self.verse_number,
            ColorRole::Link => self.link,
            ColorRole::NoteMarker => self.note_marker,
            ColorRole::Accent => self.accent,
            ColorRole::WordsOfJesus => self.words_of_jesus,
            ColorRole::Interlinear => self.interlinear,
        }
    }
}

/// The pixel size of `tile` (`[x0, y0, x1, y1]` in points) at `scale` device px per point:
/// from the device pixel its top-left edge rounds to up to the one its bottom-right edge
/// rounds to, so tiles that meet at a fractional offset meet on one pixel edge too.
pub fn tile_size(tile: [f32; 4], scale: f32) -> (usize, usize) {
    let px = |a: f32, b: f32| ((b * scale).round() - (a * scale).round()).max(0.0) as usize;
    (px(tile[0], tile[2]), px(tile[1], tile[3]))
}

/// The bytes of a `w × h` RGBA8 tile, or `None` when that does not fit in a `usize`.
fn tile_bytes(w: usize, h: usize) -> Option<usize> {
    w.checked_mul(h)?.checked_mul(4)
}

/// `x × y / 255`, rounded.
fn mul255(x: u32, y: u32) -> u32 {
    let t = x * y + 128;
    (t + (t >> 8)) >> 8
}

/// The pixels of `tile` (`[x0, y0, x1, y1]` in points, the space of the quads) at `scale`
/// device px per point: [`tile_size`] wide and high, row-major RGBA8, premultiplied. The
/// background is `colours.background`; `quads` are drawn over it as
/// [`composite_over`] draws them. A tile whose bytes do not fit in a `usize` is empty.
pub fn composite(
    quads: &[GlyphQuad],
    atlas: &GlyphAtlas,
    colours: &RoleColours,
    tile: [f32; 4],
    scale: f32,
) -> Vec<u8> {
    let (w, h) = tile_size(tile, scale);
    let Some(len) = tile_bytes(w, h) else {
        return Vec::new();
    };
    let [r, g, b, a] = colours.background;
    let a32 = u32::from(a);
    let bg = [r, g, b].map(|c| mul255(u32::from(c), a32) as u8);
    let mut pixels = [bg[0], bg[1], bg[2], a].repeat(len / 4);
    composite_over(&mut pixels, quads, atlas, colours, tile, scale);
    pixels
}

/// Draw `quads` over `pixels`, the premultiplied RGBA8 of `tile` at `scale` as [`composite`]
/// makes them: each glyph in `colours.of(role)`, its atlas coverage as alpha (source over).
/// The quads must sample `atlas` as it is now ([`GlyphAtlas::quads_from`]); parts of them
/// outside the tile are left out. `pixels` of another length than the tile's are left as
/// they are.
pub fn composite_over(
    pixels: &mut [u8],
    quads: &[GlyphQuad],
    atlas: &GlyphAtlas,
    colours: &RoleColours,
    tile: [f32; 4],
    scale: f32,
) {
    let (w, h) = tile_size(tile, scale);
    if tile_bytes(w, h) != Some(pixels.len()) {
        return;
    }
    let side = atlas.side();
    let coverage = atlas.coverage();
    // The tile's top-left in device pixels: quads sit on whole device pixels from there.
    let (ox, oy) = ((tile[0] * scale).round(), (tile[1] * scale).round());
    for q in quads {
        let [r, g, b, a] = colours.of(q.role);
        let a32 = u32::from(a);
        let colour = [r, g, b].map(|c| mul255(u32::from(c), a32));
        // The quad's place in the tile and in the atlas, in whole pixels.
        let x0 = (q.min[0] * scale).round() - ox;
        let y0 = (q.min[1] * scale).round() - oy;
        let ax = (q.uv_min[0] * side as f32).round() as i64;
        let ay = (q.uv_min[1] * side as f32).round() as i64;
        let gw = ((q.uv_max[0] - q.uv_min[0]) * side as f32).round() as i64;
        let gh = ((q.uv_max[1] - q.uv_min[1]) * side as f32).round() as i64;
        // A uv outside this atlas (a quad not made from it): skipped, so no index can panic.
        // A quad made from it before it grew or started over passes and is drawn from the
        // wrong coverage; that is the caller's precondition above.
        let side_i = side as i64;
        if ax < 0 || ay < 0 || ax + gw > side_i || ay + gh > side_i {
            continue;
        }
        let (x0, y0) = (x0 as i64, y0 as i64);
        let cols = x0.max(0)..x0.saturating_add(gw).min(w as i64);
        let rows = y0.max(0)..y0.saturating_add(gh).min(h as i64);
        for ty in rows {
            let src_row = (ay + ty - y0) as usize * side;
            let dst_row = ty as usize * w;
            for tx in cols.clone() {
                let c = u32::from(coverage[src_row + (ax + tx - x0) as usize]);
                if c == 0 {
                    continue;
                }
                let src_a = mul255(a32, c);
                let keep = 255 - src_a;
                let i = (dst_row + tx as usize) * 4;
                let px = &mut pixels[i..i + 4];
                for (k, ch) in colour.iter().enumerate() {
                    px[k] = (mul255(*ch, c) + mul255(u32::from(px[k]), keep)) as u8;
                }
                px[3] = (src_a + mul255(u32::from(px[3]), keep)) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use obs_fonts::ShapeCache;
    use obs_layout::{LaidBlock, TextSettings, layout, snippet_paragraph};

    use super::*;

    const PAGE: [u8; 4] = [250, 251, 248, 255];
    const INK: [u8; 4] = [32, 38, 41, 255];
    const ACCENT: [u8; 4] = [38, 68, 122, 255];

    fn colours() -> RoleColours {
        RoleColours {
            background: PAGE,
            ink: INK,
            verse_number: [110, 82, 26, 255],
            link: [38, 68, 122, 255],
            note_marker: [30, 98, 88, 255],
            accent: ACCENT,
            words_of_jesus: [150, 32, 30, 255],
            interlinear: [30, 98, 88, 255],
        }
    }

    fn block(text: &str, role: ColorRole) -> LaidBlock {
        let p = snippet_paragraph(text, role, false, &TextSettings::default());
        layout(&p, 400.0, ShapeCache::shared())
    }

    fn pixel(pixels: &[u8], w: usize, x: usize, y: usize) -> [u8; 4] {
        let i = (y * w + x) * 4;
        [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
    }

    /// Device-pixel rect of a quad in a tile at the origin.
    fn quad_px(q: &GlyphQuad, scale: f32) -> [usize; 4] {
        [
            (q.min[0] * scale).round() as usize,
            (q.min[1] * scale).round() as usize,
            (q.max[0] * scale).round() as usize,
            (q.max[1] * scale).round() as usize,
        ]
    }

    #[test]
    fn a_tile_is_its_size_in_device_pixels_and_filled_with_the_background() {
        let atlas = GlyphAtlas::new();
        let tile = [0.0, 0.0, 390.0, 256.0];
        assert_eq!(tile_size(tile, 3.0), (1170, 768));
        let px = composite(&[], &atlas, &colours(), tile, 3.0);
        assert_eq!(px.len(), 1170 * 768 * 4);
        assert!(px.as_chunks::<4>().0.iter().copied().all(|p| p == PAGE));
    }

    #[test]
    fn a_known_glyph_covers_the_pixels_its_atlas_coverage_has() {
        let mut atlas = GlyphAtlas::new();
        let scale = 3.0;
        let quads = atlas.quads(&block("I", ColorRole::Ink), [10.0, 10.0], scale);
        assert_eq!(quads.len(), 1);
        let q = quads[0];
        let tile = [0.0, 0.0, 100.0, 100.0];
        let (w, _) = tile_size(tile, scale);
        let px = composite(&quads, &atlas, &colours(), tile, scale);
        let [x0, y0, x1, y1] = quad_px(&q, scale);
        let side = atlas.side() as f32;
        let (ax, ay) = (
            (q.uv_min[0] * side).round() as usize,
            (q.uv_min[1] * side).round() as usize,
        );
        let mut inked = 0;
        for y in y0..y1 {
            for x in x0..x1 {
                let c = atlas.coverage()[(ay + y - y0) * atlas.side() + ax + x - x0];
                let p = pixel(&px, w, x, y);
                if c == 255 {
                    assert_eq!(p, INK, "full coverage is the ink at ({x}, {y})");
                    inked += 1;
                } else if c == 0 {
                    assert_eq!(p, PAGE, "no coverage is the page at ({x}, {y})");
                }
            }
        }
        // The stem of an "I" at 60 device px is solid ink.
        assert!(inked > 100, "{inked} fully inked pixels");
        // Nothing is drawn outside the glyph's rect.
        let outside = (0..w * w)
            .map(|i| (i % w, i / w))
            .filter(|&(x, y)| !(x0..x1).contains(&x) || !(y0..y1).contains(&y))
            .all(|(x, y)| pixel(&px, w, x, y) == PAGE);
        assert!(outside);
    }

    #[test]
    fn each_glyph_takes_its_roles_colour() {
        let mut atlas = GlyphAtlas::new();
        let scale = 2.0;
        let mut quads = atlas.quads(&block("H", ColorRole::Ink), [0.0, 0.0], scale);
        quads.extend(atlas.quads(&block("H", ColorRole::Accent), [50.0, 0.0], scale));
        let tile = [0.0, 0.0, 100.0, 40.0];
        let (w, _) = tile_size(tile, scale);
        let px = composite(&quads, &atlas, &colours(), tile, scale);
        let colours_in = |q: &GlyphQuad| {
            let [x0, y0, x1, y1] = quad_px(q, scale);
            let mut seen: Vec<[u8; 4]> = (y0..y1)
                .flat_map(|y| (x0..x1).map(move |x| (x, y)))
                .map(|(x, y)| pixel(&px, w, x, y))
                .filter(|p| *p == INK || *p == ACCENT)
                .collect();
            seen.dedup();
            seen
        };
        assert_eq!(colours_in(&quads[0]), [INK]);
        assert_eq!(colours_in(&quads[1]), [ACCENT]);
    }

    #[test]
    fn pixels_are_premultiplied() {
        let mut atlas = GlyphAtlas::new();
        let scale = 2.0;
        let quads = atlas.quads(&block("Grace", ColorRole::Ink), [0.0, 0.0], scale);
        let half_ink = RoleColours {
            background: [0, 0, 0, 0],
            ink: [200, 100, 50, 128],
            ..colours()
        };
        let tile = [0.0, 0.0, 80.0, 30.0];
        let px = composite(&quads, &atlas, &half_ink, tile, scale);
        let mut drawn = 0;
        for p in px.as_chunks::<4>().0.iter().copied() {
            assert!(p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3], "{p:?}");
            if p[3] > 0 {
                drawn += 1;
            }
            // No pixel is more opaque than the ink.
            assert!(p[3] <= 128, "{p:?}");
        }
        assert!(drawn > 0);
        // Full coverage of a half-opaque ink is the ink at half strength.
        assert!(
            px.as_chunks::<4>()
                .0
                .iter()
                .copied()
                .any(|p| p == [100, 50, 25, 128]),
            "a fully covered pixel"
        );
    }

    #[test]
    fn quads_outside_the_tile_are_clipped_and_a_tile_lower_down_shows_them() {
        let mut atlas = GlyphAtlas::new();
        let scale = 3.0;
        let quads = atlas.quads(
            &block("In the beginning", ColorRole::Ink),
            [0.0, 0.0],
            scale,
        );
        // A tile below the text: nothing drawn.
        let below = [0.0, 100.0, 200.0, 140.0];
        let px = composite(&quads, &atlas, &colours(), below, scale);
        assert!(px.as_chunks::<4>().0.iter().copied().all(|p| p == PAGE));
        // A tile over the lower half of the line shows part of the glyphs, at the same
        // device pixels as the whole tile does, shifted by its top.
        let whole = [0.0, 0.0, 200.0, 40.0];
        let lower = [0.0, 10.0, 200.0, 40.0];
        let (w, _) = tile_size(whole, scale);
        let a = composite(&quads, &atlas, &colours(), whole, scale);
        let b = composite(&quads, &atlas, &colours(), lower, scale);
        assert_eq!(&a[30 * w * 4..], &b[..]);
        assert!(b.as_chunks::<4>().0.iter().copied().any(|p| p != PAGE));
    }

    #[test]
    fn neighbouring_tiles_at_fractional_tops_tile_exactly() {
        let mut atlas = GlyphAtlas::new();
        let scale = 3.0;
        let mut quads = atlas.quads(
            &block("In the beginning", ColorRole::Ink),
            [0.0, 0.0],
            scale,
        );
        quads.extend(atlas.quads(
            &block("was the Word", ColorRole::Accent),
            [0.0, 20.0],
            scale,
        ));
        let whole = [0.0, 0.0, 200.5, 40.0];
        let (w, h) = tile_size(whole, scale);
        let all = composite(&quads, &atlas, &colours(), whole, scale);
        // Each pair of neighbours meets on one device-pixel edge: no row twice, none missed.
        let edges = [0.0, 0.4, 0.5, 7.3, 13.85, 20.1, 26.5, 33.37, 40.0];
        let mut stacked = Vec::new();
        for pair in edges.windows(2) {
            let tile = [0.0, pair[0], 200.5, pair[1]];
            let (tw, th) = tile_size(tile, scale);
            assert_eq!(tw, w);
            let px = composite(&quads, &atlas, &colours(), tile, scale);
            assert_eq!(px.len(), tw * th * 4);
            stacked.extend(px);
        }
        assert_eq!(stacked.len(), w * h * 4);
        assert!(stacked == all, "the stacked tiles differ from the whole");
        // Side by side too, at a fractional left edge.
        let left = composite(&quads, &atlas, &colours(), [0.0, 0.0, 66.7, 40.0], scale);
        let right = composite(&quads, &atlas, &colours(), [66.7, 0.0, 200.5, 40.0], scale);
        let (lw, _) = tile_size([0.0, 0.0, 66.7, 40.0], scale);
        let (rw, _) = tile_size([66.7, 0.0, 200.5, 40.0], scale);
        assert_eq!(lw + rw, w);
        for y in 0..h {
            let row = &all[y * w * 4..(y + 1) * w * 4];
            assert_eq!(&row[..lw * 4], &left[y * lw * 4..(y + 1) * lw * 4]);
            assert_eq!(&row[lw * 4..], &right[y * rw * 4..(y + 1) * rw * 4]);
        }
    }

    #[test]
    fn a_tile_too_big_to_count_in_bytes_is_empty_and_never_panics() {
        let mut atlas = GlyphAtlas::new();
        let quads = atlas.quads(&block("I", ColorRole::Ink), [0.0, 0.0], 1.0);
        let huge = [0.0, 0.0, 1e20, 1e20];
        assert_eq!(tile_size(huge, 1.0), (usize::MAX, usize::MAX));
        assert!(composite(&quads, &atlas, &colours(), huge, 1.0).is_empty());
        // A side that alone fits but whose bytes do not.
        let wide = [0.0, 0.0, 1e20, 1.0];
        assert!(composite(&quads, &atlas, &colours(), wide, 1.0).is_empty());
        let mut px = vec![7u8; 4];
        composite_over(&mut px, &quads, &atlas, &colours(), huge, 1.0);
        assert_eq!(px, vec![7u8; 4]);
    }

    #[test]
    fn pixels_of_the_wrong_length_are_left_alone() {
        let mut atlas = GlyphAtlas::new();
        let quads = atlas.quads(&block("I", ColorRole::Ink), [0.0, 0.0], 1.0);
        let mut px = vec![7u8; 12];
        composite_over(
            &mut px,
            &quads,
            &atlas,
            &colours(),
            [0.0, 0.0, 10.0, 10.0],
            1.0,
        );
        assert_eq!(px, vec![7u8; 12]);
    }
}
