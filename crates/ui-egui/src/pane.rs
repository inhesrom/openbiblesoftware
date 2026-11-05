//! The reading-pane widget: scroll input, glyph-atlas texture upload, and one textured mesh
//! per visible block.

use egui::{
    Color32, ColorImage, Mesh, Pos2, Rect, Sense, Shape, TextureHandle, TextureOptions, Ui, pos2,
    vec2,
};
use obs_render::GlyphAtlas;

use crate::marks::{Mark, MarkKind};
use crate::palette::Palette;
use crate::selection::{DocPos, Granularity, Selection, copy_text, range_rects, selection_rects};
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

/// What a jump marks for a moment: a verse (in the Resource's own numbering), or a Text
/// range of one unit (a Library search hit's paragraph, #60).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Flash {
    Verse(Verse),
    Text {
        unit: usize,
        range: std::ops::Range<usize>,
    },
}

/// A Tagged word the pointer is on: which unit and which bytes of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordHit {
    pub unit: usize,
    pub range: std::ops::Range<usize>,
}

/// What happened in the reading pane this frame.
#[derive(Debug, Clone, Default)]
pub struct PaneOutput {
    /// The pane's own response (for its right-click menu).
    pub response: Option<egui::Response>,
    /// Where the selection is drawn (the union of its rectangles in view), in screen points.
    pub selection_rect: Option<Rect>,
    /// A selection was just made: a drag released, or a word or verse double/triple-clicked.
    /// The selection toolbar appears (#18 rule 8).
    pub selection_made: bool,
    /// The pointer went down on the text (the selection toolbar goes away).
    pub pressed: bool,
    /// The pane was right-clicked; the selection is now what the menu acts on (the
    /// clicked word, unless the click was inside the selection).
    pub context_clicked: bool,
    /// The Note marker under the pointer: the Note's id and the marker's screen rect.
    pub hovered_note: Option<(String, Rect)>,
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
/// verse or a Text range (after a jump) with a colour whose alpha fades it out. `marks` are
/// the Highlights and Notes to draw, oldest first: Highlight backgrounds under the glyphs
/// (newest on top), the red underline and Note markers over them.
pub fn reading_pane(
    ui: &mut Ui,
    view: &mut ReadingView,
    atlas: &mut AtlasTexture,
    palette: &Palette,
    flash: Option<(Flash, Color32)>,
    marks: &[Mark],
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
    out.pressed = response.is_pointer_button_down_on() && !view.dragging_scrollbar;
    let made = handle_selection(ui, &response, view, rect, text_rect);
    out.selection_made = made.selection_made;
    out.context_clicked = made.context_clicked;
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
    let flashed = flash.and_then(|(f, color)| match f {
        Flash::Verse(verse) => {
            let (unit, byte) = view.doc.verse_position(verse)?;
            Some((unit, view.doc.verse_range_at(unit, byte)?, color))
        }
        Flash::Text { unit, range } => Some((unit, range, color)),
    });
    if let Some((unit, range, color)) = flashed {
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
    let to_screen = |[x0, y0, x1, y1]: [f32; 4]| {
        Rect::from_min_max(
            pos2(text_rect.left() + x0, rect.top() + y0),
            pos2(text_rect.left() + x1, rect.top() + y1),
        )
    };
    // Highlights: backgrounds under the glyphs, oldest first so the newest is on top. They
    // cover the text's band of the line, not the line spacing around it.
    let size = view.settings().size;
    let ranges: Vec<(DocPos, DocPos)> = marks.iter().map(|m| (m.start, m.end)).collect();
    let mark_rects = range_rects(view, &ranges, rect.height());
    for (mark, rects) in marks.iter().zip(&mark_rects) {
        if let MarkKind::Highlight(style) = mark.kind
            && let Some(fill) = palette.highlight(style)
        {
            for r in rects {
                painter.rect_filled(to_screen(text_band(*r, size)), 1.5, fill);
            }
        }
    }
    if let Some(sel) = view.selection {
        for r in selection_rects(view, &sel, rect.height()) {
            let r = to_screen(r);
            painter.rect_filled(r, 0.0, palette.selection);
            out.selection_rect = Some(out.selection_rect.map_or(r, |u| u.union(r)));
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
    // Over the glyphs: the red underline, then Note markers at the end of their ranges.
    let rtl = view.doc.direction() == obs_model::Direction::Rtl;
    let hover = response.hover_pos();
    let in_view: Vec<usize> = if marks.iter().any(|m| m.kind == MarkKind::Note) {
        view.laid_in_view(rect.height())
            .iter()
            .map(|(i, _, _)| *i)
            .collect()
    } else {
        Vec::new()
    };
    for (mark, rects) in marks.iter().zip(&mark_rects) {
        match mark.kind {
            MarkKind::Highlight(style) if palette.highlight(style).is_none() => {
                for r in rects {
                    let r = to_screen(text_band(*r, size));
                    let y = r.bottom() - (size * 0.1).max(1.5);
                    painter.line_segment(
                        [pos2(r.left(), y), pos2(r.right(), y)],
                        egui::Stroke::new((size * 0.09).max(1.5), palette.underline),
                    );
                }
            }
            MarkKind::Note => {
                let Some(marker) =
                    note_marker(view, &in_view, mark, rects, size, rtl).map(to_screen)
                else {
                    continue;
                };
                paint_note_marker(&painter, marker, palette);
                if hover.is_some_and(|p| marker.expand(3.0).contains(p)) {
                    out.hovered_note = Some((mark.id.clone(), marker));
                }
            }
            MarkKind::Highlight(_) => {}
        }
    }

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
    out.response = Some(response);
    out
}

/// The part of a line rectangle `[x0, y0, x1, y1]` that text of `size` occupies: a band
/// about 1.4 em tall around the line's middle (all of it in tight lines).
fn text_band([x0, y0, x1, y1]: [f32; 4], size: f32) -> [f32; 4] {
    let inset = ((y1 - y0 - size * 1.4) / 2.0).max(0.0);
    [x0, y0 + inset, x1, y1 - inset]
}

/// Where a Note's marker goes, relative to the text's left edge and the viewport top: just
/// past the end of its range (to the left of it in right-to-left text), raised like a
/// superscript. `None` while the range's end is not laid out in view.
fn note_marker(
    view: &ReadingView,
    in_view: &[usize],
    mark: &Mark,
    rects: &[[f32; 4]],
    size: f32,
    rtl: bool,
) -> Option<[f32; 4]> {
    let last = rects.last()?;
    // The last rectangle is the range's end only if the end's block is in view.
    let end_block = view
        .doc
        .block_at(mark.end.unit, mark.end.byte.saturating_sub(1))?;
    if !in_view.contains(&end_block) {
        return None;
    }
    let (w, h) = (size * 0.42, size * 0.48);
    let x = if rtl {
        last[0] - w - 1.0
    } else {
        last[2] + 1.0
    };
    // Raised like a superscript: its top at the text band's top, so it sits over the cap
    // height rather than over the next word's lowercase letters.
    let band = text_band(*last, size);
    let y = band[1];
    Some([x, y, x + w, y + h])
}

/// A small note: a filled card with two ruled lines in the page colour.
fn paint_note_marker(painter: &egui::Painter, r: Rect, palette: &Palette) {
    painter.rect_filled(r, 1.5, palette.note_marker);
    let stroke = egui::Stroke::new(1.0, palette.page);
    for f in [0.38, 0.66] {
        let y = r.top() + r.height() * f;
        painter.line_segment([pos2(r.left() + 2.0, y), pos2(r.right() - 2.0, y)], stroke);
    }
}

/// What the pointer did to the selection this frame.
struct SelectionInput {
    selection_made: bool,
    context_clicked: bool,
}

/// Drag to select (auto-scrolling past the pane's edges), double-click a word, triple-click a
/// verse, click to clear; Copy puts the text with its reference on the clipboard.
fn handle_selection(
    ui: &Ui,
    response: &egui::Response,
    view: &mut ReadingView,
    rect: Rect,
    text_rect: Rect,
) -> SelectionInput {
    let mut input = SelectionInput {
        selection_made: false,
        context_clicked: false,
    };
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
                input.selection_made = true;
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
        if response.secondary_clicked() {
            // Right-click acts on the selection under the pointer, else on the clicked word.
            if let Some(at) = pointer.and_then(|p| pos_at(view, p))
                && !view
                    .selection
                    .is_some_and(|s| has_text(view, &s) && s.contains(&view.doc, at))
            {
                let word = Selection {
                    anchor: at,
                    head: at,
                    granularity: Granularity::Word,
                };
                view.selection = has_text(view, &word).then_some(word);
            }
            input.context_clicked = true;
        }
        if response.drag_stopped() && view.selection.is_some_and(|s| has_text(view, &s)) {
            input.selection_made = true;
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
    input
}

/// Whether a selection covers any text other than white space (stopping at the first such
/// character: this runs every frame, whatever the selection's size).
pub(crate) fn has_text(view: &ReadingView, sel: &Selection) -> bool {
    let (a, b) = sel.range(&view.doc);
    a < b
        && (a.unit..=b.unit).any(|u| {
            let Some(text) = view.doc.units.get(u).map(|u| u.text.as_str()) else {
                return false;
            };
            let start = if u == a.unit { a.byte } else { 0 };
            let end = if u == b.unit { b.byte } else { text.len() };
            text.get(start.min(end)..end.min(text.len()))
                .is_some_and(|t| t.chars().any(|c| !c.is_whitespace()))
        })
}
