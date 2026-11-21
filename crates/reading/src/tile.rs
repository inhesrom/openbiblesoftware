//! Reading-page tiles for the Mobile app (plan `docs/plans/mobile-ios.md` A4, option B): a
//! band of a [`ReadingView`]'s page drawn on the CPU into premultiplied RGBA8 pixels, the page
//! colour first, then the glyphs in their roles' colours ([`obs_render::composite`]). A shell
//! shows the tiles as images. Highlights, the selection and Note markers are not drawn yet.

use obs_render::{GlyphAtlas, composite, composite_over, tile_size};

use crate::palette::TonePalette;
use crate::reading_tokens::size;
use crate::view::{MARGIN, ReadingView};

/// A tile's pixels: `width × height`, row-major RGBA8, premultiplied alpha.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tile {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Where the text column sits on a page `page_width` points wide: its left edge and its
/// width. The Reading column ([`obs_layout::ColumnWidth`]) is centred between the margins, as
/// the desktop's reading pane centres it; a phone has no scrollbar beside it.
pub fn text_column(view: &ReadingView, page_width: f32) -> (f32, f32) {
    let settings = view.settings();
    let room = (page_width - 2.0 * MARGIN).max(size::TEXT_MIN);
    let measure = (settings.layout.column.ems()).map_or(room, |ems| room.min(settings.size * ems));
    (MARGIN + ((room - measure) / 2.0).floor(), measure)
}

/// The tile of `view`'s page, `page_width` points wide, that is `height` points tall from
/// `top` points below the view's top (its Current location), at `scale` device px per point,
/// in `palette`'s colours. Lays out what it shows on this thread. Glyphs go into `atlas`;
/// when it fills, it starts over between parts of the tile, so every glyph is drawn.
pub fn compose_tile(
    view: &mut ReadingView,
    atlas: &mut GlyphAtlas,
    palette: &TonePalette,
    page_width: f32,
    top: f32,
    height: f32,
    scale: f32,
) -> Tile {
    let (left, measure) = text_column(view, page_width);
    view.set_width(measure);
    let rect = [0.0, top, page_width, top + height];
    let colours = palette.role_colours();
    let mut rgba = composite(&[], atlas, &colours, rect, scale);
    let visible = [top, top + height];
    for block in view.visible(top + height) {
        let origin = [left, block.top];
        let (mut from, mut fresh) = (0, false);
        loop {
            let placed = atlas.quads_from(&block.laid, origin, scale, visible, from);
            composite_over(&mut rgba, &placed.quads, atlas, &colours, rect, scale);
            match placed.resume {
                // An empty atlas that cannot take the next glyph never will: leave it out.
                Some(next) if !(fresh && next == from) => {
                    atlas.start_over();
                    (from, fresh) = (next, true);
                }
                _ => break,
            }
        }
    }
    let (w, h) = tile_size(rect, scale);
    Tile {
        width: w as u32,
        height: h as u32,
        rgba,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use obs_engine::testing::bibles::bsb_john_1;
    use obs_layout::TextSettings;

    use super::*;
    use crate::doc::ReadingDoc;

    fn john_1() -> ReadingView {
        let doc = Arc::new(ReadingDoc::from_resource(bsb_john_1()));
        let mut view = ReadingView::new(doc, TextSettings::default());
        let john_1_1 = obs_scripture::Verse::new(obs_scripture::Book::John, 1, 1);
        assert!(view.jump_to(john_1_1));
        view
    }

    fn px(t: &Tile, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * t.width + x) * 4) as usize;
        [t.rgba[i], t.rgba[i + 1], t.rgba[i + 2], t.rgba[i + 3]]
    }

    fn page(p: &TonePalette) -> [u8; 4] {
        let [r, g, b] = p.page.0;
        [r, g, b, 255]
    }

    #[test]
    fn a_phone_tile_is_page_coloured_with_ink_in_the_text_column_only() {
        let mut view = john_1();
        let mut atlas = GlyphAtlas::new();
        let p = TonePalette::PAPER;
        let tile = compose_tile(&mut view, &mut atlas, &p, 390.0, 0.0, 256.0, 3.0);
        assert_eq!((tile.width, tile.height), (1170, 768));
        assert_eq!(tile.rgba.len(), 1170 * 768 * 4);
        let (left, measure) = text_column(&view, 390.0);
        assert_eq!((left, measure), (MARGIN, 390.0 - 2.0 * MARGIN));
        // The margins are the page; the text column has ink, in the ink colour among others.
        let margin_px = (MARGIN * 3.0) as u32 - 3;
        for y in 0..tile.height {
            for x in (0..margin_px).chain(tile.width - margin_px..tile.width) {
                assert_eq!(px(&tile, x, y), page(&p), "margin at ({x}, {y})");
            }
        }
        let [r, g, b] = p.ink.0;
        let ink = tile
            .rgba
            .chunks_exact(4)
            .filter(|c| *c == [r, g, b, 255])
            .count();
        assert!(ink > 5_000, "{ink} pixels of full ink");
        assert!(tile.rgba.chunks_exact(4).all(|c| c[3] == 255), "opaque");
    }

    #[test]
    fn a_dark_tile_is_drawn_in_the_dark_tones_colours() {
        let mut view = john_1();
        let mut atlas = GlyphAtlas::new();
        let p = TonePalette::DARK;
        let tile = compose_tile(&mut view, &mut atlas, &p, 390.0, 0.0, 128.0, 2.0);
        assert_eq!((tile.width, tile.height), (780, 256));
        assert_eq!(px(&tile, 0, 0), page(&p));
        let [r, g, b] = p.ink.0;
        assert!(tile.rgba.chunks_exact(4).any(|c| c == [r, g, b, 255]));
    }

    #[test]
    fn the_next_tile_down_goes_on_where_the_first_stops() {
        let mut view = john_1();
        let mut atlas = GlyphAtlas::new();
        let p = TonePalette::PAPER;
        let tall = compose_tile(&mut view, &mut atlas, &p, 390.0, 0.0, 200.0, 2.0);
        let upper = compose_tile(&mut view, &mut atlas, &p, 390.0, 0.0, 100.0, 2.0);
        let lower = compose_tile(&mut view, &mut atlas, &p, 390.0, 100.0, 100.0, 2.0);
        let row = (tall.width * 4) as usize;
        assert_eq!(&tall.rgba[..200 * row], &upper.rgba[..]);
        assert_eq!(&tall.rgba[200 * row..], &lower.rgba[..]);
    }
}
