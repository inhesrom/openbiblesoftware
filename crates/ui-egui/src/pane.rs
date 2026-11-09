//! The reading-pane widget: scroll input, glyph-atlas texture upload, and one textured mesh
//! per visible block.

use egui::{
    Color32, ColorImage, Mesh, Pos2, Rect, Sense, Shape, TextureHandle, TextureOptions, Ui, pos2,
    vec2,
};
use obs_engine::HighlightLook;
use obs_render::GlyphAtlas;

use crate::find::Found;
use crate::marks::{Mark, MarkKind};
use crate::palette::Palette;
use crate::selection::{
    DocPos, Granularity, Selection, range_rects, selection_rects, text_range_rects,
};
use crate::tokens::{alpha, mark, motion, radius, scale, size, space, stroke};
use crate::view::{FootnoteMarker, MARGIN, ReadingView, SYNC_BUDGET};
use obs_layout::{InlineObject, UnitPos};
use obs_scripture::Verse;

const SCROLLBAR: f32 = size::SCROLLBAR;

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

/// A Footnote marker the pointer or the keyboard is on (#208): which Resource and note, where
/// its marker is in the unit, its label as drawn and its box on screen.
#[derive(Debug, Clone, PartialEq)]
pub struct FootnoteHit {
    /// The Resource whose note it is: two Resources can share unit, anchor and note id.
    pub resource: String,
    pub unit: usize,
    pub anchor: usize,
    pub note: obs_model::UnitId,
    pub kind: obs_model::NoteKind,
    pub label: String,
    pub rect: Rect,
    /// The marker's widget, which keyboard focus returns to when its popover closes.
    pub widget: egui::Id,
}

impl FootnoteHit {
    /// What the marker is called, to a screen reader and in its popover.
    pub fn name(&self) -> String {
        match self.kind {
            obs_model::NoteKind::Footnote => format!("Footnote {}", self.label),
            obs_model::NoteKind::CrossReference => "Cross-reference note".to_string(),
        }
    }
}

/// What a drag that starts on the text carries out of the pane (#216).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragOut {
    /// The Scripture reference it started on: unit and span indexes.
    Reference { unit: usize, span: usize },
    /// The selection it started inside.
    Selection,
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
    /// The Note marker clicked this frame: the Note's id (it opens in the Notes Tool, #64).
    pub clicked_note: Option<String>,
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
    /// Where in the text the pointer is when the right-click menu opens (by a right-click or
    /// from the keyboard): its Verse and Tagged word are there, not at the selection's start.
    pub pointer_at: Option<DocPos>,
    /// An interlinear line (#143) was clicked: the right-click menu opens for its word, which
    /// is now the selection.
    pub cell_clicked: bool,
    /// The Footnote marker under the pointer (#208).
    pub hovered_footnote: Option<FootnoteHit>,
    /// The Footnote marker with the keyboard focus.
    pub focused_footnote: Option<FootnoteHit>,
    /// The Footnote marker clicked (or Enter pressed on it) this frame: its popover pins.
    pub clicked_footnote: Option<FootnoteHit>,
    /// `clicked_footnote` came from the keyboard (or a screen reader), not the pointer: focus
    /// moves into its popover.
    pub footnote_by_keyboard: bool,
    /// A drag started this frame on a Scripture reference or inside the selection: it carries
    /// that out of the pane instead of selecting (#216).
    pub drag_out: Option<DragOut>,
}

/// One Visual filter's matches in view (#211): its look, and the rectangles of its words in
/// text coordinates (as [`range_rects`] gives them).
pub type FilterRects = (obs_engine::StyleAppearance, Vec<[f32; 4]>);

/// What a reading pane draws over its text besides the Find bar's matches.
#[derive(Debug, Default)]
pub struct PaneMarks<'a> {
    /// Highlights and Notes, oldest first.
    pub annotations: &'a [Mark],
    /// The Tab's Visual filters (#211), drawn under the Annotations: their matches in the
    /// blocks this frame shows.
    pub filters: Option<&'a mut crate::filters::FilterPainter>,
}

/// Draw the reading pane into all the space `ui` has, and handle its input. `flash` marks a
/// verse or a Text range (after a jump) with a colour whose alpha fades it out. `marks` are
/// the Highlights and Notes to draw, oldest first: Highlight backgrounds under the glyphs
/// (newest on top), the red underline and Note markers over them; the Visual filters'
/// matches go under them, in the same looks. `found` are the Find bar's matches (#175), drawn
/// over the Highlights and under the selection.
pub fn reading_pane(
    ui: &mut Ui,
    view: &mut ReadingView,
    atlas: &mut AtlasTexture,
    palette: &Palette,
    flash: Option<(Flash, Color32)>,
    pane_marks: PaneMarks<'_>,
    found: Found<'_>,
) -> PaneOutput {
    let marks = pane_marks.annotations;
    let mut out = PaneOutput::default();
    let rect = ui.available_rect_before_wrap();
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    // The body is pinned to the pane rect, whatever the header widgets do (#13).
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, palette.page);

    // The Reading column (#209), centred in wide panes: Medium is a comfortable measure of
    // about 70 characters.
    let room = (rect.width() - 2.0 * MARGIN - SCROLLBAR).max(size::TEXT_MIN);
    let settings = view.settings();
    let measure = (settings.layout.column.ems()).map_or(room, |ems| room.min(settings.size * ems));
    let left = rect.left() + MARGIN + ((room - measure) / 2.0).floor();
    let text_rect = Rect::from_min_max(pos2(left, rect.top()), pos2(left + measure, rect.bottom()));
    view.set_width(text_rect.width());
    view.text_origin = [text_rect.left(), rect.top()];
    view.pane_rect = [rect.left(), rect.top(), rect.right(), rect.bottom()];
    view.set_clock(ui.input(|i| i.time));
    let before = view.position().0;

    // Input: wheel and trackpad, keys, scrollbar drag.
    if response.hovered() {
        let dy = ui.input(|i| i.smooth_scroll_delta.y);
        view.scroll_by(-dy);
    }
    if response.has_focus() || response.hovered() {
        let page = rect.height() * 0.9;
        // Space on a focused Footnote widget (#208: a marker, or its popover's link or ×)
        // activates it, and is for no pane: not this one (paging would carry the marker,
        // and its popover with it, out of view), nor another under the pointer.
        let footnote_focused = footnote_widget_focused(ui.ctx());
        // A key's scroll eases to its place (#225), unless motion is reduced.
        let (shown, at) = (view.shown_position(), view.position().0);
        ui.input(|i| {
            if i.key_pressed(egui::Key::PageDown)
                || (i.key_pressed(egui::Key::Space) && !footnote_focused)
            {
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
        if view.position().0 != at && !crate::motion::reduced(ui.ctx()) {
            view.glide_from(shown, motion::SCROLL, motion::ease_in_out);
        }
    }
    let track = Rect::from_min_max(pos2(rect.right() - SCROLLBAR, rect.top()), rect.max);
    if response.drag_started()
        && let Some(p) = response.interact_pointer_pos()
    {
        view.dragging_scrollbar = track.expand(space::XS).contains(p);
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
    out.drag_out = made.drag_out;
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
                let line = view.settings().size * scale::LINE;
                let bottom = rect.top() + b.top + b.height - line * scale::PLACEHOLDER_INSET;
                let mut y = rect.top() + b.top + line * scale::PLACEHOLDER_INSET;
                while y < bottom {
                    crate::components::text_placeholder(
                        &painter,
                        pos2(text_rect.left(), y - line * scale::PLACEHOLDER_TOP),
                        text_rect.width(),
                        line,
                        palette.verse_number.gamma_multiply(alpha::WASH),
                    );
                    y += line;
                }
            }
        }
    }
    if view.has_pending() || view.is_gliding() {
        ui.ctx().request_repaint();
    }
    let filtered = match pane_marks.filters {
        Some(f) => f.rects(view, rect.height(), Some(ui.ctx())),
        None => Vec::new(),
    };
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
                radius::XS,
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
    // Each style's colours in this tone, worked out once per frame (#181).
    let mut paints = std::collections::HashMap::new();
    let mut paint =
        |a: obs_engine::StyleAppearance| *paints.entry(a).or_insert_with(|| palette.paint(&a));
    // Text-colour Highlights recolour the glyphs inside their rects, the newest on top.
    let mut recolour: Vec<(Rect, Color32)> = Vec::new();
    // What is painted under the glyphs above the page, in paint order (the last one holding a
    // glyph is the one it sits on): a recoloured glyph must read on it, not only on the page.
    let mut grounds: Vec<(Rect, Color32)> = Vec::new();
    // Visual filters (#211) first, so a Highlight shows over them.
    let filter_looks = filtered.iter().map(|(a, rects)| (*a, rects));
    let mark_looks = marks
        .iter()
        .zip(&mark_rects)
        .filter_map(|(m, rects)| match m.kind {
            MarkKind::Highlight(a) => Some((a, rects)),
            _ => None,
        });
    for (a, rects) in filter_looks.chain(mark_looks) {
        let p = paint(a);
        for r in rects {
            let r = to_screen(text_band(*r, size));
            match p.look {
                HighlightLook::Background => {
                    painter.rect_filled(r, mark::RADIUS, p.color);
                    grounds.push((r, p.color));
                }
                HighlightLook::TextColor => recolour.push((r, p.color)),
                HighlightLook::Underline | HighlightLook::Box => {}
            }
        }
    }
    // Find matches (#175), over the Highlights and under the selection.
    for (k, rects) in found_rects(view, found, rect.height()) {
        for r in rects {
            let r = to_screen(text_band(r, size));
            let current = found.current == Some(k);
            paint_find_match(&painter, r, current, palette);
            let ground = if current {
                palette.search_hit
            } else {
                palette.find_match
            };
            grounds.push((r, ground));
        }
    }
    if let Some(sel) = view.selection {
        for r in selection_rects(view, &sel, rect.height()) {
            let r = to_screen(r);
            painter.rect_filled(r, 0.0, palette.selection);
            grounds.push((r, palette.selection));
            out.selection_rect = Some(out.selection_rect.map_or(r, |u| u.union(r)));
        }
    }
    // The Tagged word under the pointer is underlined at once, pinned or not. Its
    // interlinear lines (#143) point at it too.
    let mut on_cell = false;
    let hovered_word = response.hover_pos().and_then(|p| {
        let (unit, pos) = view.hit_in_unit_at(p.x - text_rect.left(), p.y - rect.top())?;
        let byte = match pos {
            UnitPos::Text(byte) => byte,
            UnitPos::Object {
                anchor,
                object: InlineObject::InterlinearCell { .. },
            } => {
                on_cell = true;
                anchor
            }
            UnitPos::Object { .. } => return None,
        };
        let (range, _) = view.doc.tagged_word_at(unit, byte)?;
        Some(WordHit { unit, range })
    });
    if let Some(w) = &hovered_word {
        let word = (
            DocPos {
                unit: w.unit,
                byte: w.range.start,
            },
            DocPos {
                unit: w.unit,
                byte: w.range.end,
            },
        );
        let rects = range_rects(view, &[word], rect.height());
        for &[x0, _, x1, y1] in rects.iter().flatten() {
            let y = rect.top() + y1 - mark::LINE_MIN;
            painter.line_segment(
                [
                    pos2(text_rect.left() + x0, y),
                    pos2(text_rect.left() + x1, y),
                ],
                egui::Stroke::new(stroke::HAIR, palette.underline),
            );
        }
        if !view.dragging_scrollbar && response.clicked() {
            if on_cell {
                // A click on an interlinear line opens the word's right-click menu (#142),
                // acting on the whole Tagged word (e.g. "he gave"), not its first word.
                select_tagged_word_for_menu(view, word);
                out.pointer_at = Some(word.0);
                out.cell_clicked = true;
            } else {
                out.clicked_word = Some(w.clone());
            }
        }
    }
    out.hovered_word = hovered_word;
    let texture = atlas.sync(ui.ctx());
    let mut mesh = Mesh::with_texture(texture);
    mesh.reserve_vertices(quads.len() * 4);
    mesh.reserve_triangles(quads.len() * 2);
    for q in quads {
        let glyph = Rect::from_min_max(Pos2::from(q.min), Pos2::from(q.max));
        let at = glyph.center();
        let color = (recolour.iter().rev())
            .find(|(r, _)| r.contains(at))
            .map_or_else(
                || palette.color(q.role),
                |(_, c)| {
                    let ground = (grounds.iter().rev())
                        .find(|(r, _)| r.contains(at))
                        .map_or(palette.page, |(_, g)| *g);
                    palette.text_on(*c, ground)
                },
            );
        mesh.add_rect_with_uv(
            glyph,
            Rect::from_min_max(Pos2::from(q.uv_min), Pos2::from(q.uv_max)),
            color,
        );
    }
    painter.add(Shape::mesh(mesh));
    // Over the glyphs: the red underline, then Note markers at the end of their ranges.
    let rtl = view.doc.direction() == obs_model::Direction::Rtl;
    let hover = response.hover_pos();
    let in_view: Vec<usize> = if marks
        .iter()
        .any(|m| m.kind.is_note() || m.kind.at_verse_number())
    {
        view.laid_in_view(rect.height())
            .iter()
            .map(|(i, _, _)| *i)
            .collect()
    } else {
        Vec::new()
    };
    // Marks for a verse number go in the column's margin on the number's line (left of the
    // column in left-to-right text, right of it in right-to-left text): a verse that starts
    // mid-line has the previous verse's words just before its number. Space taken so far in
    // each line's margin, keyed by the line's top.
    let mut beside: std::collections::HashMap<i64, f32> = std::collections::HashMap::new();
    let gap = (size * mark::GAP_EMS).max(mark::GAP_MIN);
    // The pane's own room, in text coordinates: marks never go under the scrollbar or
    // outside the pane.
    let (room_left, room_right) = (
        rect.left() - text_rect.left() + space::PX,
        track.left() - text_rect.left() - space::PX,
    );
    for (a, rects) in &filtered {
        draw_line_or_box(&painter, paint(*a), rects, size, &to_screen);
    }
    for (mark, rects) in marks.iter().zip(&mark_rects) {
        match mark.kind {
            MarkKind::Highlight(a) => {
                // A line or box runs unbroken along each line of the range (#181).
                draw_line_or_box(&painter, paint(a), rects, size, &to_screen);
            }
            MarkKind::Note => {
                let Some(marker) =
                    note_marker(view, &in_view, mark, rects, size, rtl).map(to_screen)
                else {
                    continue;
                };
                paint_note_marker(&painter, marker, palette);
                if hover.is_some_and(|p| marker.expand(mark::HIT_SLOP).contains(p)) {
                    out.hovered_note = Some((mark.id.clone(), marker));
                    if response.clicked() {
                        out.clicked_note = Some(mark.id.clone());
                    }
                }
            }
            MarkKind::Margin(_) | MarkKind::VerseNote => {
                let Some(band) = verse_number_line(view, &in_view, mark, rects, size) else {
                    continue;
                };
                // Laid outward from the column, one after another, in Mark order.
                let used = beside.entry((band[1] * 4.0).round() as i64).or_insert(0.0);
                let (w, h, round) = match mark.kind {
                    MarkKind::Margin(_) => {
                        let w = (size * mark::MARGIN_EMS).max(mark::MARGIN_MIN);
                        (w, band[3] - band[1], w / 2.0)
                    }
                    _ => (
                        size * mark::NOTE_WIDTH_EMS,
                        size * mark::NOTE_HEIGHT_EMS,
                        mark::RADIUS,
                    ),
                };
                let x = if rtl {
                    (text_rect.width() + gap + *used).min(room_right - w)
                } else {
                    (-gap - *used - w).max(room_left)
                };
                *used += w + gap;
                let r = to_screen([x, band[1], x + w, band[1] + h]);
                match mark.kind {
                    MarkKind::Margin(a) => {
                        painter.rect_filled(r, round, paint(a).mark);
                    }
                    _ => {
                        paint_note_marker(&painter, r, palette);
                        if hover.is_some_and(|p| r.expand(mark::HIT_SLOP).contains(p)) {
                            out.hovered_note = Some((mark.id.clone(), r));
                        }
                    }
                }
            }
        }
    }

    footnote_markers(ui, &response, view, rect, text_rect, &painter, &mut out);

    // Scrollbar.
    let (y, total) = view.position();
    if total > f64::from(rect.height()) {
        let frac = (y / total) as f32;
        // Never taller than the track: a Panel growing in (#225) starts shorter than a thumb.
        let thumb_h = (rect.height() * rect.height() / total as f32)
            .max(size::THUMB_MIN)
            .min(rect.height());
        let thumb_top = track.top() + frac * (track.height() - thumb_h);
        let thumb_w = SCROLLBAR - 2.0 * space::XXS;
        painter.rect_filled(
            Rect::from_min_size(
                pos2(track.left() + space::XXS, thumb_top),
                vec2(thumb_w, thumb_h),
            ),
            thumb_w / 2.0,
            palette.verse_number.gamma_multiply(alpha::HALF),
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
    if let Some(p) = response.hover_pos()
        && !track.contains(p)
        && (response.secondary_clicked() || ui.input(menu_key_pressed))
    {
        let y = p.y.clamp(rect.top(), rect.bottom() - 1.0) - rect.top();
        out.pointer_at = view.pos_at(p.x - text_rect.left(), y);
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

/// The Footnote widgets (markers, popover links and ×) drawn this pass and the pass before,
/// in any reading pane of one viewport: the set the keyboard focus is checked against, so no
/// state about who had the focus is kept from one frame to the next.
#[derive(Clone, Default)]
struct FootnoteWidgets {
    pass: u64,
    this: std::collections::HashSet<egui::Id>,
    last: std::collections::HashSet<egui::Id>,
}

impl FootnoteWidgets {
    /// One set per viewport: pass numbers and keyboard focus are each viewport's own, and a
    /// floating window's native viewport draws within the main window's frame.
    fn key(ctx: &egui::Context) -> egui::Id {
        egui::Id::unique(("footnote-widgets", ctx.viewport_id()))
    }

    /// Move to `pass`: this pass's widgets become the last pass's, or are dropped when no
    /// widget was drawn in the pass just before.
    fn roll(&mut self, pass: u64) {
        if pass != self.pass {
            self.last = if pass == self.pass + 1 {
                std::mem::take(&mut self.this)
            } else {
                Default::default()
            };
            self.this.clear();
            self.pass = pass;
        }
    }
}

/// Register `id`, a Footnote widget (a marker, a popover's link or ×) drawn this pass.
pub(crate) fn register_footnote_widget(ctx: &egui::Context, id: egui::Id) {
    let pass = ctx.cumulative_pass_nr();
    let key = FootnoteWidgets::key(ctx);
    ctx.data_mut(|d| {
        let w = d.get_temp_mut_or_default::<FootnoteWidgets>(key);
        w.roll(pass);
        w.this.insert(id);
    });
}

/// Whether the keyboard focus is now on a Footnote widget (a marker, a popover's link or ×)
/// drawn this pass or the last, in any reading pane of this viewport: its activation keys
/// (Space, Enter) are for it, so no pane pages on Space.
pub(crate) fn footnote_widget_focused(ctx: &egui::Context) -> bool {
    let Some(focused) = ctx.memory(|m| m.focused()) else {
        return false;
    };
    let pass = ctx.cumulative_pass_nr();
    let key = FootnoteWidgets::key(ctx);
    ctx.data_mut(|d| {
        let w = d.get_temp_mut_or_default::<FootnoteWidgets>(key);
        w.roll(pass);
        w.this.contains(&focused) || w.last.contains(&focused)
    })
}

/// The widget of Footnote marker `m` in the reading pane `pane`.
fn footnote_marker_id(pane: egui::Id, m: &FootnoteMarker) -> egui::Id {
    pane.with(("footnote", m.unit, m.anchor, m.note.as_str()))
}

/// Each Footnote marker in view (#208) is a small widget of its own over the text, so the
/// pointer and the keyboard (Tab to it, Enter) reach it, and a screen reader names it and
/// reads its note.
fn footnote_markers(
    ui: &mut Ui,
    pane: &egui::Response,
    view: &ReadingView,
    rect: Rect,
    text_rect: Rect,
    painter: &egui::Painter,
    out: &mut PaneOutput,
) {
    let mut focused = None;
    for m in view.footnote_markers(rect.height()) {
        let [x0, y0, x1, y1] = m.rect;
        let r = Rect::from_min_max(
            pos2(text_rect.left() + x0, rect.top() + y0),
            pos2(text_rect.left() + x1, rect.top() + y1),
        );
        if !rect.intersects(r) {
            continue;
        }
        let id = footnote_marker_id(pane.id, &m);
        let response = ui.interact(r.expand(mark::HIT_SLOP).intersect(rect), id, Sense::click());
        let hit = FootnoteHit {
            resource: view.doc.manifest.resource_id.clone(),
            unit: m.unit,
            anchor: m.anchor,
            note: m.note,
            kind: m.kind,
            label: m.label,
            rect: r,
            widget: id,
        };
        let name = hit.name();
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
        if let Some(body) = view.doc.footnote(&hit.note) {
            let text = body.text.clone();
            ui.ctx()
                .accesskit_node_builder(response.id, |n| n.set_description(text));
        }
        register_footnote_widget(ui.ctx(), id);
        if response.has_focus() {
            painter.rect_stroke(
                r.expand(space::XXS),
                radius::XS,
                ui.visuals().selection.stroke,
                egui::StrokeKind::Outside,
            );
            focused = Some(hit.clone());
        }
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            out.hovered_footnote = Some(hit.clone());
        }
        if response.clicked() {
            out.footnote_by_keyboard = !response.clicked_by(egui::PointerButton::Primary);
            out.clicked_footnote = Some(hit);
        }
    }
    out.focused_footnote = focused;
}

/// The Find matches (#175) in the blocks laid out in a viewport `height` tall, by index, with
/// their rectangles (letters only: never a verse number before them). Only the matches reaching into those
/// blocks are looked at, however many the Resource has.
pub fn found_rects(
    view: &ReadingView,
    found: Found<'_>,
    height: f32,
) -> Vec<(usize, Vec<[f32; 4]>)> {
    if found.matches.is_empty() {
        return Vec::new();
    }
    let mut shown: Vec<usize> = Vec::new();
    for (index, _, _) in view.laid_in_view(height) {
        let block = &view.doc.blocks[index];
        shown.extend(found.within(block.unit, 0, block.source.range.clone()));
    }
    shown.sort_unstable();
    shown.dedup();
    let ranges: Vec<(DocPos, DocPos)> = shown
        .iter()
        .map(|&k| (found.matches[k].start(), found.matches[k].end()))
        .collect();
    shown
        .into_iter()
        .zip(text_range_rects(view, &ranges, height))
        .collect()
}

/// One Find match (#175): the current one in the search-hit overlay with an outline, the
/// others in the paler find-match overlay. Neither is a Highlight style.
pub(crate) fn paint_find_match(painter: &egui::Painter, r: Rect, current: bool, palette: &Palette) {
    if current {
        painter.rect_filled(r, radius::XS, palette.search_hit);
        painter.rect_stroke(
            r,
            radius::XS,
            egui::Stroke::new(stroke::MEDIUM, palette.find_current),
            egui::StrokeKind::Outside,
        );
    } else {
        painter.rect_filled(r, radius::XS, palette.find_match);
    }
}

/// A Highlight's or Visual filter's underline or box: unbroken along each line of its range.
fn draw_line_or_box(
    painter: &egui::Painter,
    p: crate::palette::StylePaint,
    rects: &[[f32; 4]],
    size: f32,
    to_screen: &dyn Fn([f32; 4]) -> Rect,
) {
    if !matches!(p.look, HighlightLook::Underline | HighlightLook::Box) {
        return;
    }
    for r in line_runs(rects, size * mark::JOIN_EMS) {
        let r = to_screen(text_band(r, size));
        if p.look == HighlightLook::Underline {
            let y = r.bottom() - (size * mark::UNDERLINE_DROP_EMS).max(mark::LINE_MIN);
            painter.line_segment(
                [pos2(r.left(), y), pos2(r.right(), y)],
                egui::Stroke::new((size * mark::UNDERLINE_EMS).max(mark::LINE_MIN), p.color),
            );
        } else {
            painter.rect_stroke(
                r,
                radius::XS,
                egui::Stroke::new((size * mark::BOX_EMS).max(mark::LINE_MIN), p.color),
                egui::StrokeKind::Outside,
            );
        }
    }
}

/// The part of a line rectangle `[x0, y0, x1, y1]` that text of `size` occupies: a band
/// about 1.4 em tall around the line's middle (all of it in tight lines).
/// `rects` (in order) with neighbours on the same line and at most `gap` apart joined into
/// one, so a box or underline over several words is one, not one per word.
fn line_runs(rects: &[[f32; 4]], gap: f32) -> Vec<[f32; 4]> {
    let mut out: Vec<[f32; 4]> = Vec::new();
    for &[x0, y0, x1, y1] in rects {
        if let Some(last) = out.last_mut()
            && (last[1] - y0).abs() < 0.5
            && (last[3] - y1).abs() < 0.5
            && x0 <= last[2] + gap
            && x1 >= last[0] - gap
        {
            last[0] = last[0].min(x0);
            last[2] = last[2].max(x1);
        } else {
            out.push([x0, y0, x1, y1]);
        }
    }
    out
}

fn text_band([x0, y0, x1, y1]: [f32; 4], size: f32) -> [f32; 4] {
    let inset = ((y1 - y0 - size * mark::BAND_EMS) / 2.0).max(0.0);
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
    let (w, h) = (size * mark::NOTE_WIDTH_EMS, size * mark::NOTE_HEIGHT_EMS);
    let x = if rtl {
        last[0] - w - space::PX
    } else {
        last[2] + space::PX
    };
    // Raised like a superscript: its top at the text band's top, so it sits over the cap
    // height rather than over the next word's lowercase letters.
    let band = text_band(*last, size);
    let y = band[1];
    Some([x, y, x + w, y + h])
}

/// The text band (relative to the text's left edge and the viewport top) of the line that
/// holds the verse number of the Verse a Mark covers: its range starts at the Verse, where
/// the number is laid out, so that is the range's first line. `None` while the Verse's start
/// is not laid out in view.
fn verse_number_line(
    view: &ReadingView,
    in_view: &[usize],
    mark: &Mark,
    rects: &[[f32; 4]],
    size: f32,
) -> Option<[f32; 4]> {
    let first = rects.first()?;
    let start_block = view.doc.block_at(mark.start.unit, mark.start.byte)?;
    if !in_view.contains(&start_block) {
        return None;
    }
    Some(text_band(*first, size))
}

/// A small note: a filled card with two ruled lines in the page colour.
fn paint_note_marker(painter: &egui::Painter, r: Rect, palette: &Palette) {
    painter.rect_filled(r, mark::RADIUS, palette.note_marker);
    let stroke = egui::Stroke::new(stroke::HAIR, palette.page);
    for f in mark::NOTE_RULES {
        let y = r.top() + r.height() * f;
        painter.line_segment(
            [
                pos2(r.left() + space::XXS, y),
                pos2(r.right() - space::XXS, y),
            ],
            stroke,
        );
    }
}

/// What the pointer did to the selection this frame.
struct SelectionInput {
    selection_made: bool,
    context_clicked: bool,
    drag_out: Option<DragOut>,
}

/// Drag to select (auto-scrolling past the pane's edges), double-click a word, triple-click a
/// verse, click to clear; Copy puts the text with its reference on the clipboard. A drag that
/// starts on a Scripture reference, or inside the selection, carries it out of the pane
/// instead (#216): the selection stays as it is.
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
        drag_out: None,
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
            let origin = ui.input(|i| i.pointer.press_origin()).or(pointer);
            input.drag_out = origin.and_then(|p| drag_out_at(view, p, rect, text_rect));
            view.dragging_out = input.drag_out.is_some();
            if !view.dragging_out {
                view.selection = pointer.and_then(|p| pos_at(view, p)).map(Selection::caret);
            }
        } else if view.dragging_out {
            // Carrying a reference or the selection: the Shell draws where it would drop.
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
            if let Some(at) = pointer.and_then(|p| pos_at(view, p)) {
                select_for_menu(view, at);
            }
            input.context_clicked = true;
        }
        if response.drag_stopped()
            && !view.dragging_out
            && view.selection.is_some_and(|s| has_text(view, &s))
        {
            input.selection_made = true;
        }
    }
    if response.drag_stopped() || !response.dragged() {
        view.dragging_out = false;
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
        ui.ctx().copy_text(view.copy_text(&sel));
    }
    input
}

/// What a drag pressed at `p` (screen points) carries out of the pane: the Scripture reference
/// there, else the selection when the text exactly under `p` is in it (not the nearest caret:
/// the right half of a selection's last glyph is in it, the margin beside it is not); `None`
/// selects as usual.
fn drag_out_at(view: &mut ReadingView, p: Pos2, rect: Rect, text_rect: Rect) -> Option<DragOut> {
    if !rect.contains(p) {
        return None;
    }
    let (x, y) = (p.x - text_rect.left(), p.y - rect.top());
    if let Some((unit, span)) = view.reference_at(x, y) {
        return Some(DragOut::Reference { unit, span });
    }
    let sel = view.selection.filter(|s| has_text(view, s))?;
    let (unit, hit) = view.hit_in_unit_at(x, y)?;
    let byte = match hit {
        UnitPos::Text(b) => b,
        UnitPos::Object { anchor, .. } => anchor,
    };
    sel.contains(&view.doc, DocPos { unit, byte })
        .then_some(DragOut::Selection)
}

/// Shift+F10 or the Menu key: open the right-click menu from the keyboard (#142).
pub(crate) fn menu_key_pressed(i: &egui::InputState) -> bool {
    i.events.iter().any(|e| {
        matches!(
            e,
            egui::Event::Key { key, pressed: true, modifiers, .. }
                if *key == egui::Key::Menu || (*key == egui::Key::F10 && modifiers.shift)
        )
    })
}

/// The right-click menu acts on the selection under `at`, else on the word there (which
/// becomes the selection; none when `at` is not on a word).
pub(crate) fn select_for_menu(view: &mut ReadingView, at: DocPos) {
    if view
        .selection
        .is_some_and(|s| has_text(view, &s) && s.contains(&view.doc, at))
    {
        return;
    }
    let word = Selection {
        anchor: at,
        head: at,
        granularity: Granularity::Word,
    };
    view.selection = has_text(view, &word).then_some(word);
}

/// The menu of an interlinear cell acts on the selection when it covers the cell's whole
/// Tagged word `word`, else on that word's full range (which becomes the selection).
fn select_tagged_word_for_menu(view: &mut ReadingView, word: (DocPos, DocPos)) {
    if view.selection.is_some_and(|s| {
        let (a, b) = s.range(&view.doc);
        has_text(view, &s) && a <= word.0 && word.1 <= b
    }) {
        return;
    }
    let sel = Selection {
        anchor: word.0,
        head: word.1,
        granularity: Granularity::Grapheme,
    };
    view.selection = has_text(view, &sel).then_some(sel);
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
