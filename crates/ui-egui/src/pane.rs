//! The reading-pane widget: scroll input, glyph-atlas texture upload, and one textured mesh
//! per visible block.

use egui::{
    Color32, ColorImage, Mesh, Pos2, Rect, Sense, Shape, TextureHandle, TextureOptions, Ui, pos2,
    vec2,
};
use obs_render::GlyphAtlas;

use crate::palette::Palette;
use crate::view::{MARGIN, ReadingView};

const SCROLLBAR: f32 = 10.0;

/// The atlas plus its GPU copy.
pub struct AtlasTexture {
    pub atlas: GlyphAtlas,
    handle: Option<TextureHandle>,
    generation: u64,
}

impl Default for AtlasTexture {
    fn default() -> Self {
        Self {
            atlas: GlyphAtlas::new(),
            handle: None,
            generation: u64::MAX,
        }
    }
}

fn coverage_image(coverage: &[u8], side: usize, region: [usize; 4]) -> ColorImage {
    let [x0, y0, x1, y1] = region;
    let mut pixels = Vec::with_capacity((x1 - x0) * (y1 - y0));
    for y in y0..y1 {
        for &a in &coverage[y * side + x0..y * side + x1] {
            // Premultiplied white at the glyph's coverage; the vertex colour tints it.
            pixels.push(Color32::from_rgba_premultiplied(a, a, a, a));
        }
    }
    ColorImage::new([x1 - x0, y1 - y0], pixels)
}

impl AtlasTexture {
    /// Upload what changed and return the texture.
    pub fn sync(&mut self, ctx: &egui::Context) -> egui::TextureId {
        let side = self.atlas.side();
        let generation = self.atlas.stats().generation;
        if self.handle.is_none() || generation != self.generation {
            let img = coverage_image(self.atlas.coverage(), side, [0, 0, side, side]);
            self.handle = Some(ctx.load_texture("glyph-atlas", img, TextureOptions::NEAREST));
            self.generation = generation;
            let _ = self.atlas.take_dirty();
        } else if let Some(region) = self.atlas.take_dirty()
            && let Some(h) = self.handle.as_mut()
        {
            let img = coverage_image(self.atlas.coverage(), side, region);
            h.set_partial([region[0], region[1]], img, TextureOptions::NEAREST);
        }
        self.handle
            .as_ref()
            .map_or(egui::TextureId::default(), TextureHandle::id)
    }
}

/// Draw the reading pane into all the space `ui` has, and handle its input.
pub fn reading_pane(
    ui: &mut Ui,
    view: &mut ReadingView,
    atlas: &mut AtlasTexture,
    palette: &Palette,
) {
    let rect = ui.available_rect_before_wrap();
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    // The body is pinned to the pane rect, whatever the header widgets do (#13).
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, palette.page);

    let text_rect = Rect::from_min_max(
        pos2(rect.left() + MARGIN, rect.top()),
        pos2(rect.right() - MARGIN - SCROLLBAR, rect.bottom()),
    );
    view.set_width(text_rect.width());

    // Input: wheel and trackpad, keys, scrollbar drag.
    if response.hovered() {
        let dy = ui.input(|i| i.smooth_scroll_delta.y);
        view.scroll_by(-dy);
    }
    if response.has_focus() || response.hovered() {
        let page = rect.height() * 0.9;
        ui.input(|i| {
            if i.key_pressed(egui::Key::PageDown) || i.key_pressed(egui::Key::Space) {
                view.scroll_by(page);
            }
            if i.key_pressed(egui::Key::PageUp) {
                view.scroll_by(-page);
            }
            if i.key_pressed(egui::Key::ArrowDown) {
                view.scroll_by(40.0);
            }
            if i.key_pressed(egui::Key::ArrowUp) {
                view.scroll_by(-40.0);
            }
        });
    }
    let track = Rect::from_min_max(pos2(rect.right() - SCROLLBAR, rect.top()), rect.max);
    if response.dragged()
        && let Some(p) = response.interact_pointer_pos()
        && track.expand(4.0).contains(p)
    {
        let (_, total) = view.position();
        let frac = f64::from(((p.y - track.top()) / track.height()).clamp(0.0, 1.0));
        view.scroll_to_position(frac * total);
    }

    let ppp = ui.ctx().pixels_per_point();
    let visible = view.visible(rect.height());
    let mut quads = Vec::new();
    for vb in &visible {
        quads.extend(
            atlas
                .atlas
                .quads(&vb.laid, [text_rect.left(), rect.top() + vb.top], ppp),
        );
    }
    let texture = atlas.sync(ui.ctx());
    let mut mesh = Mesh::with_texture(texture);
    mesh.reserve_vertices(quads.len() * 4);
    mesh.reserve_triangles(quads.len() * 2);
    for q in quads {
        mesh.add_rect_with_uv(
            Rect::from_min_max(Pos2::from(q.min), Pos2::from(q.max)),
            Rect::from_min_max(Pos2::from(q.uv_min), Pos2::from(q.uv_max)),
            palette.color(q.role),
        );
    }
    painter.add(Shape::mesh(mesh));

    // Scrollbar.
    let (y, total) = view.position();
    if total > f64::from(rect.height()) {
        let frac = (y / total) as f32;
        let thumb_h = (rect.height() * rect.height() / total as f32).clamp(24.0, rect.height());
        let thumb_top = track.top() + frac * (track.height() - thumb_h);
        painter.rect_filled(
            Rect::from_min_size(
                pos2(track.left() + 2.0, thumb_top),
                vec2(SCROLLBAR - 4.0, thumb_h),
            ),
            3.0,
            palette.verse_number.gamma_multiply(0.5),
        );
    }
}
