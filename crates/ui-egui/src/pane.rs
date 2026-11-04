//! The reading-pane widget: scroll input, glyph-atlas texture upload, and one textured mesh
//! per visible block.

use egui::{
    Color32, ColorImage, Mesh, Pos2, Rect, Sense, Shape, TextureHandle, TextureOptions, Ui, pos2,
    vec2,
};
use obs_render::GlyphAtlas;

use crate::palette::Palette;
use crate::selection::{DocPos, Granularity, Selection, copy_text, selection_rects};
use crate::view::{MARGIN, ReadingView, SYNC_BUDGET};
use obs_layout::{InlineObject, UnitPos};
use obs_scripture::Verse;

const SCROLLBAR: f32 = 10.0;
/// Widest text column, in ems of the text size.
pub const MEASURE_EMS: f32 = 38.0;

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

/// A Tagged word the pointer is on: which unit and which bytes of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordHit {
    pub unit: usize,
    pub range: std::ops::Range<usize>,
}

/// What happened in the reading pane this frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PaneOutput {
    /// The user moved the view (wheel, keys, scrollbar, selection auto-scroll).
    pub user_moved: bool,
    /// The verse number under the pointer, in the Bible's own numbering.
    pub hovered_verse_number: Option<Verse>,
    /// Unit and span indexes of the Scripture reference under the pointer.
    pub hovered_reference: Option<(usize, usize)>,
    pub reference_clicked: bool,
    /// The Tagged word under the pointer (underlined this frame; Word Info follows it after
    /// its dwell, #18 rule 16).
    pub hovered_word: Option<WordHit>,
    /// The Tagged word clicked this frame (Word Info pins it).
    pub clicked_word: Option<WordHit>,
}

/// Draw the reading pane into all the space `ui` has, and handle its input. `flash` marks a
/// verse (after a jump) with a colour whose alpha fades it out.
pub fn reading_pane(
    ui: &mut Ui,
    view: &mut ReadingView,
    atlas: &mut AtlasTexture,
    palette: &Palette,
    flash: Option<(Verse, Color32)>,
) -> PaneOutput {
    let mut out = PaneOutput::default();
    let rect = ui.available_rect_before_wrap();
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    // The body is pinned to the pane rect, whatever the header widgets do (#13).
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, palette.page);

    // A comfortable measure (about 70 characters), centred in wide panes.
    let room = (rect.width() - 2.0 * MARGIN - SCROLLBAR).max(40.0);
    let measure = room.min(view.settings().size * MEASURE_EMS);
    let left = rect.left() + MARGIN + ((room - measure) / 2.0).floor();
    let text_rect = Rect::from_min_max(pos2(left, rect.top()), pos2(left + measure, rect.bottom()));
    view.set_width(text_rect.width());
    view.text_origin = [text_rect.left(), rect.top()];
    let before = view.position().0;

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
    if response.drag_started()
        && let Some(p) = response.interact_pointer_pos()
    {
        view.dragging_scrollbar = track.expand(4.0).contains(p);
    }
    if response.dragged()
        && view.dragging_scrollbar
        && let Some(p) = response.interact_pointer_pos()
    {
        let (_, total) = view.position();
        let frac = f64::from(((p.y - track.top()) / track.height()).clamp(0.0, 1.0));
        view.scroll_to_position(frac * total);
    }
    handle_selection(ui, &response, view, rect, text_rect);
    out.user_moved = (view.position().0 - before).abs() > 0.01;

    let ppp = ui.ctx().pixels_per_point();
    let frame = view.frame(rect.height(), Some(SYNC_BUDGET));
    let mut quads = Vec::new();
    for b in &frame.blocks {
        match &b.laid {
            Some(laid) => {
                quads.extend(
                    atlas
                        .atlas
                        .quads(laid, [text_rect.left(), rect.top() + b.top], ppp),
                );
            }
            None => {
                // Placeholder while a worker lays the block out: faint bars where lines go.
                let line = view.settings().size * 1.8;
                let bottom = rect.top() + b.top + b.height - line * 0.4;
                let mut y = rect.top() + b.top + line * 0.4;
                while y < bottom {
                    painter.rect_filled(
                        Rect::from_min_size(
                            pos2(text_rect.left(), y),
                            vec2(text_rect.width(), line * 0.35),
                        ),
                        2.0,
                        palette.verse_number.gamma_multiply(0.12),
                    );
                    y += line;
                }
            }
        }
    }
    if view.has_pending() {
        ui.ctx().request_repaint();
    }
    if let Some((verse, color)) = flash
        && let Some((unit, byte)) = view.doc.verse_position(verse)
        && let Some(range) = view.doc.verse_range_at(unit, byte)
    {
        let sel = Selection {
            anchor: DocPos {
                unit,
                byte: range.start,
            },
            head: DocPos {
                unit,
                byte: range.end,
            },
            granularity: Granularity::Grapheme,
        };
        for [x0, y0, x1, y1] in selection_rects(view, &sel, rect.height()) {
            painter.rect_filled(
                Rect::from_min_max(
                    pos2(text_rect.left() + x0, rect.top() + y0),
                    pos2(text_rect.left() + x1, rect.top() + y1),
                ),
                2.0,
                color,
            );
        }
    }
    if let Some(sel) = view.selection {
        for [x0, y0, x1, y1] in selection_rects(view, &sel, rect.height()) {
            painter.rect_filled(
                Rect::from_min_max(
                    pos2(text_rect.left() + x0, rect.top() + y0),
                    pos2(text_rect.left() + x1, rect.top() + y1),
                ),
                0.0,
                palette.selection,
            );
        }
    }
    // The Tagged word under the pointer is underlined at once, pinned or not.
    let hovered_word = response.hover_pos().and_then(|p| {
        let (unit, UnitPos::Text(byte)) =
            view.hit_in_unit_at(p.x - text_rect.left(), p.y - rect.top())?
        else {
            return None;
        };
        let (range, _) = view.doc.tagged_word_at(unit, byte)?;
        Some(WordHit { unit, range })
    });
    if let Some(w) = &hovered_word {
        let sel = Selection {
            anchor: DocPos {
                unit: w.unit,
                byte: w.range.start,
            },
            head: DocPos {
                unit: w.unit,
                byte: w.range.end,
            },
            granularity: Granularity::Grapheme,
        };
        for [x0, _, x1, y1] in selection_rects(view, &sel, rect.height()) {
            let y = rect.top() + y1 - 1.5;
            painter.line_segment(
                [
                    pos2(text_rect.left() + x0, y),
                    pos2(text_rect.left() + x1, y),
                ],
                egui::Stroke::new(1.0, palette.underline),
            );
        }
        if !view.dragging_scrollbar && response.clicked() {
            out.clicked_word = Some(w.clone());
        }
    }
    out.hovered_word = hovered_word;
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
    crate::a11y::publish(
        ui,
        response.id,
        view,
        [text_rect.left(), rect.top()],
        rect.height(),
    );
    if let Some(p) = response.hover_pos()
        && let Some(UnitPos::Object {
            object: InlineObject::VerseNumber(v),
            ..
        }) = view.hit_at(p.x - text_rect.left(), p.y - rect.top())
    {
        out.hovered_verse_number = Some(v);
    }
    if let Some(p) = response.hover_pos() {
        out.hovered_reference = view.reference_at(p.x - text_rect.left(), p.y - rect.top());
        if out.hovered_reference.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            out.reference_clicked = response.clicked();
        }
    }
    out
}

/// Drag to select (auto-scrolling past the pane's edges), double-click a word, triple-click a
/// verse, click to clear; Copy puts the text with its reference on the clipboard.
fn handle_selection(
    ui: &Ui,
    response: &egui::Response,
    view: &mut ReadingView,
    rect: Rect,
    text_rect: Rect,
) {
    let pos_at = |view: &mut ReadingView, p: Pos2| {
        let y = p.y.clamp(rect.top(), rect.bottom() - 1.0) - rect.top();
        view.pos_at(p.x - text_rect.left(), y)
    };
    let pointer = response.interact_pointer_pos();
    if !view.dragging_scrollbar {
        if response.triple_clicked() || response.double_clicked() {
            if let Some(at) = pointer.and_then(|p| pos_at(view, p)) {
                view.selection = Some(Selection {
                    anchor: at,
                    head: at,
                    granularity: if response.triple_clicked() {
                        Granularity::Verse
                    } else {
                        Granularity::Word
                    },
                });
            }
        } else if response.drag_started() {
            view.selection = pointer.and_then(|p| pos_at(view, p)).map(Selection::caret);
        } else if response.dragged()
            && let Some(p) = pointer
        {
            // Past the top or bottom edge, scroll in proportion to the distance.
            let past = if p.y < rect.top() {
                p.y - rect.top()
            } else if p.y > rect.bottom() {
                p.y - rect.bottom()
            } else {
                0.0
            };
            if past != 0.0 {
                view.scroll_by(past * 0.3);
                ui.ctx().request_repaint();
            }
            if let (Some(head), Some(sel)) = (pos_at(view, p), view.selection.as_mut()) {
                sel.head = head;
            }
        } else if response.clicked() {
            view.selection = None;
        }
    }
    if response.drag_stopped() {
        view.dragging_scrollbar = false;
    }
    let copy = ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy)));
    if copy
        && let Some(sel) = view
            .selection
            .filter(|s| !s.is_empty() || s.granularity != Granularity::Grapheme)
    {
        ui.ctx().copy_text(copy_text(&view.doc, &sel));
    }
}
