//! The reading pane on GPUI (#380): a Resource read through `obs-reading`'s [`ReadingView`]
//! (virtualized blocks, the scroll anchor, jumps, the layout workers within
//! [`SYNC_BUDGET`]), painted the way ADR 0004's #372 amendment decides: every glyph our own
//! pipeline laid out is drawn with GPUI's glyph painter (`Window::paint_glyph`), from the same
//! font files registered with GPUI's text system, at our positions.
//!
//! What the egui app's `pane.rs` does, without egui: the Reading column (#209) centred in the
//! pane, scrolling by wheel, trackpad and keyboard at the Scroll speed (#293), keyboard
//! scrolling and jumps easing unless motion is reduced (#225), the scroll bar, and one
//! AccessKit paragraph node per laid block with its Tagged words and references as link
//! children (#19). Marks and highlights (#382) and selection (#381) come later.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use std::time::Instant;

use gpui_kit::prelude::*;
use gpui_kit::{
    A11ySubtreeBuilder, AnyElement, App, AvailableSpace, Bounds, ContentMask, Context, Element,
    ElementId, FocusHandle, FontId, Global, GlobalElementId, GlyphId, Hsla, InspectorElementId,
    KeyDownEvent, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Role, ScrollDelta, ScrollEvent, Style, Window, accesskit, div, fill, font, point, px, relative,
    size,
};
use obs_engine::Session;
use obs_fonts::FaceId;
use obs_layout::{ColorRole, LaidBlock, TextSettings};
use obs_model::SpanKind;
use obs_reading::reading_tokens::{motion, space};
use obs_reading::view::MARGIN;
use obs_reading::{FrameStats, ReadingDoc, ReadingView, SYNC_BUDGET, TonePalette};
use obs_scripture::Verse;

/// How far an arrow key scrolls at a Scroll speed of 1×, in points (the egui pane's
/// `ARROW_STEP`).
pub const ARROW_STEP: f32 = 40.0;

/// How far one wheel line scrolls at a Scroll speed of 1×, in points (egui's
/// `line_scroll_speed` on the desktop, which the egui app scrolls by).
pub const LINE_STEP: f32 = 40.0;

/// The share of the pane's height Page Down, Page Up and Space scroll (the egui pane's).
pub const PAGE_SHARE: f32 = 0.9;

/// The scroll bar's width, and the track the text keeps clear of (the egui pane's
/// `size::SCROLLBAR`).
pub const SCROLLBAR: f32 = 10.0;

/// The thumb's least height, so it can be grabbed in a long Resource.
const THUMB_MIN: f32 = 24.0;

/// The bundled faces `obs-fonts` shapes with, each resolved once to the GPUI font of the same
/// file ([`crate::bundled_fonts`] registers the files): `paint_glyph` then draws the glyph ids
/// our pipeline shaped.
#[derive(Debug, Clone, Default)]
pub struct ReadingFonts(HashMap<FaceId, FontId>);

impl Global for ReadingFonts {}

impl ReadingFonts {
    /// Resolve the bundled faces; the font files must have been added to the text system.
    pub fn resolve(cx: &App) -> Self {
        let faces = FaceId::ALL.iter().map(|&face| {
            let f = match face {
                FaceId::Italic => font("Gentium Plus").italic(),
                FaceId::Bold => font("Gentium Plus").bold(),
                FaceId::BoldItalic => font("Gentium Plus").bold().italic(),
                FaceId::Hebrew => font("Ezra SIL"),
                _ => font("Gentium Plus"),
            };
            (face, cx.text_system().resolve_font(&f))
        });
        Self(faces.collect())
    }

    /// The GPUI font that draws `face`'s glyph ids. `None` for a face loaded from the computer
    /// (#138): its file is not registered with GPUI yet, and another face's glyphs would be the
    /// wrong ones.
    pub fn get(&self, face: FaceId) -> Option<FontId> {
        self.0.get(&face).copied()
    }
}

/// Register the reading fonts' GPUI ids; [`crate::install`] calls it after adding the files.
pub fn install(cx: &mut App) {
    let fonts = ReadingFonts::resolve(cx);
    cx.set_global(fonts);
}

fn hsla(c: obs_engine::Rgb) -> Hsla {
    crate::tokens::rgba(c).into()
}

/// The Page-tone palette the Session's appearance asks for, in a Look whose chrome is dark or
/// not (Auto follows it), as the egui app's `Palette` is chosen.
pub fn palette_for(session: &Session, look: &crate::theme::Look) -> TonePalette {
    let tone = match session.appearance.page_tone {
        obs_engine::PageTone::Auto if look.chrome.dark => obs_engine::PageTone::Dark,
        obs_engine::PageTone::Auto => obs_engine::PageTone::Paper,
        t => t,
    };
    TonePalette::for_theme(tone, look.chrome.high_contrast)
}

/// Resource `resource` of `session`'s library, opened for reading, with its Text settings.
pub fn open_doc(
    session: &Session,
    resource: &str,
) -> Result<(Arc<ReadingDoc>, TextSettings), String> {
    let entry = session
        .library
        .get(resource)
        .ok_or_else(|| format!("{resource} is not in the library"))?;
    let doc = ReadingDoc::open(&entry.path).map_err(|e| format!("{resource}: {e}"))?;
    Ok((Arc::new(doc), session.text_settings_of(resource)))
}

/// One Resource read in a pane.
pub struct ReadingPane {
    resource: String,
    view: ReadingView,
    palette: TonePalette,
    scroll_speed: f32,
    reduce_motion: bool,
    focus: FocusHandle,
    started: Instant,
    /// The last frame's viewport height, for Page Down.
    height: f32,
    /// The last frame's track (pane coordinates: top, height) while the thumb is dragged.
    dragging: bool,
    /// The pane's bounds as last drawn, in window points.
    bounds: Bounds<Pixels>,
    /// What the last frame cost.
    last_frame: Option<FrameStats>,
}

impl ReadingPane {
    /// A pane over `doc`, with `settings`.
    pub fn with_doc(doc: Arc<ReadingDoc>, settings: TextSettings, cx: &mut Context<Self>) -> Self {
        let resource = doc.manifest.resource_id.clone();
        Self {
            resource,
            view: ReadingView::new(doc, settings),
            palette: TonePalette::for_theme(obs_engine::PageTone::Paper, false),
            scroll_speed: 1.0,
            reduce_motion: true,
            focus: cx.focus_handle(),
            started: Instant::now(),
            height: 0.0,
            dragging: false,
            bounds: Bounds::default(),
            last_frame: None,
        }
    }

    pub fn resource(&self) -> &str {
        &self.resource
    }

    pub fn view(&self) -> &ReadingView {
        &self.view
    }

    pub fn view_mut(&mut self) -> &mut ReadingView {
        &mut self.view
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// What the last frame cost: its synchronous layout, and the blocks still placeholders.
    pub fn last_frame(&self) -> Option<&FrameStats> {
        self.last_frame.as_ref()
    }

    /// The pane's bounds as last drawn, in window points.
    pub fn bounds(&self) -> Bounds<Pixels> {
        self.bounds
    }

    /// The Session's state the pane draws with, each frame: the Resource's Text settings (the
    /// Text size and its own Resource text size, the Reading layout), the Page tone's palette,
    /// the Scroll speed and whether motion is reduced. New settings lay the text out again with
    /// the Current location kept ([`ReadingView::set_settings`]).
    pub fn sync(&mut self, session: &Session, palette: TonePalette, reduce_motion: bool) {
        self.view
            .set_settings(session.text_settings_of(&self.resource));
        self.palette = palette;
        self.scroll_speed = session.appearance.scroll_speed;
        self.reduce_motion = reduce_motion;
    }

    /// Set the Scroll speed and motion directly (a pane drawn without a Session).
    pub fn set_motion(&mut self, scroll_speed: f32, reduce_motion: bool) {
        self.scroll_speed = scroll_speed;
        self.reduce_motion = reduce_motion;
    }

    fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// Jump to `verse` (the Resource's own numbering): its line goes to the top and stays the
    /// Current location until the user scrolls; the block it lands in is laid out at once. It
    /// eases into place unless motion is reduced, as the egui app's jumps do.
    pub fn jump_to(&mut self, verse: Verse) -> bool {
        let now = self.now();
        self.view.set_clock(now);
        let from = self.view.shown_position();
        let jumped = self.view.jump_to(verse);
        if jumped && !self.reduce_motion {
            self.view.glide_from(from, motion::JUMP, motion::ease_out);
        }
        jumped
    }

    /// Go to a Bible reference typed for this Resource, read in its own Versification.
    pub fn go(&mut self, text: &str) -> Result<Verse, String> {
        let doc = Arc::clone(&self.view.doc);
        let context = self.view.current_location();
        let list = obs_scripture::parse_reference(text, &doc.versification, context)
            .map_err(|e| e.kind.to_string())?;
        let verse = list
            .passages()
            .first()
            .map(|p| p.start())
            .ok_or_else(|| format!("no reference in {text:?}"))?;
        if self.jump_to(verse) {
            Ok(verse)
        } else {
            Err(format!("No section or Verse for {verse} in this Resource"))
        }
    }

    /// Scroll by `dy` points of input (positive: down the text) at the Scroll speed. An eased
    /// scroll (a key with motion) glides from where the text is drawn.
    fn scroll(&mut self, dy: f32, eased: bool) {
        let now = self.now();
        self.view.set_clock(now);
        let shown = self.view.shown_position();
        self.view.scroll_by(dy);
        if eased && !self.reduce_motion {
            self.view
                .glide_from(shown, motion::SCROLL, motion::ease_in_out);
        }
    }

    /// A wheel or trackpad scroll event: trackpad points and wheel lines alike at the Scroll
    /// speed (#293). Ctrl or Cmd with the wheel is a zoom, never a scroll.
    pub fn wheel(&mut self, ev: &ScrollEvent) -> bool {
        if ev.modifiers.control || ev.modifiers.platform {
            return false;
        }
        let dy = match ev.delta {
            ScrollDelta::Pixels(d) => -f32::from(d.y),
            ScrollDelta::Lines(d) => -d.y * LINE_STEP,
        };
        if dy == 0.0 {
            return false;
        }
        self.scroll(dy * self.scroll_speed, false);
        true
    }

    /// A key: Page Down, Space and Page Up move a page; the arrows a step at the Scroll speed.
    /// They ease unless motion is reduced.
    pub fn key(&mut self, key: &str) -> bool {
        let page = self.height * PAGE_SHARE;
        let arrow = ARROW_STEP * self.scroll_speed;
        let dy = match key {
            "pagedown" | "space" => page,
            "pageup" => -page,
            "down" => arrow,
            "up" => -arrow,
            _ => return false,
        };
        self.scroll(dy, true);
        true
    }

    /// The scroll bar's track and thumb, in pane coordinates (x, y, w, h); `None` when the text
    /// fits.
    fn thumb(&mut self, height: f32, width: f32) -> Option<([f32; 4], f64)> {
        let (pos, total) = self.view.position();
        let height64 = f64::from(height);
        if total <= height64 + 0.5 {
            return None;
        }
        let share = (height64 / total).clamp(0.0, 1.0) as f32;
        let h = (height * share).max(THUMB_MIN).min(height);
        let travel = (height - h).max(0.0);
        let at = (pos / (total - height64).max(1.0)).clamp(0.0, 1.0) as f32;
        Some(([width - SCROLLBAR, travel * at, SCROLLBAR, h], total))
    }

    /// Drag the thumb to window point `y`: the position the track's fraction asks for.
    fn drag_to(&mut self, y: Pixels) {
        let bounds = self.bounds;
        let height = f32::from(bounds.size.height);
        if height <= 0.0 {
            return;
        }
        let frac = ((f32::from(y - bounds.origin.y)) / height).clamp(0.0, 1.0);
        let (_, total) = self.view.position();
        self.view.scroll_to_position(f64::from(frac) * total);
    }
}

impl Render for ReadingPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = hsla(self.palette.page);
        let pane = cx.entity();
        div()
            .id("reading-pane")
            .track_focus(&self.focus)
            .size_full()
            .overflow_hidden()
            .bg(page)
            .on_key_down(cx.listener(|pane, ev: &KeyDownEvent, _, cx| {
                if pane.key(ev.keystroke.key.as_str()) {
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .on_scroll(cx.listener(|pane, ev: &ScrollEvent, _, cx| {
                if pane.wheel(ev) {
                    cx.notify();
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|pane, ev: &MouseDownEvent, window, cx| {
                    window.focus(&pane.focus, cx);
                    let b = pane.bounds;
                    let track = f32::from(b.origin.x + b.size.width) - SCROLLBAR;
                    if f32::from(ev.position.x) >= track - space::XS {
                        pane.dragging = true;
                        pane.drag_to(ev.position.y);
                    }
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|pane, ev: &MouseMoveEvent, _, cx| {
                if pane.dragging && ev.pressed_button == Some(MouseButton::Left) {
                    pane.drag_to(ev.position.y);
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|pane, _: &MouseUpEvent, _, _| pane.dragging = false),
            )
            .child(PaneElement { pane })
    }
}

/// The text area of a [`ReadingPane`]: lays the frame out in prepaint, where the pane's size is
/// known, and paints the visible blocks.
struct PaneElement {
    pane: gpui_kit::Entity<ReadingPane>,
}

#[derive(Default)]
struct PanePrepaint {
    paras: Vec<AnyElement>,
    placeholders: Vec<Bounds<Pixels>>,
    thumb: Option<Bounds<Pixels>>,
    placeholder_color: Option<Hsla>,
    thumb_color: Option<Hsla>,
}

impl IntoElement for PaneElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

/// One laid block to draw.
struct DrawBlock {
    index: usize,
    top: f32,
    height: f32,
    laid: Option<Arc<LaidBlock>>,
}

impl Element for PaneElement {
    type RequestLayoutState = ();
    type PrepaintState = PanePrepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _rl: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> PanePrepaint {
        #[cfg(feature = "test-support")]
        observed::begin();
        let width = f32::from(bounds.size.width);
        let height = f32::from(bounds.size.height);
        let fonts = cx.try_global::<ReadingFonts>().cloned().unwrap_or_default();
        let (doc, palette, left, measure, blocks, thumb, animating) =
            self.pane.update(cx, |pane, _| {
                pane.bounds = bounds;
                pane.height = height;
                let now = pane.now();
                pane.view.set_clock(now);
                // The Reading column (#209), centred in wide panes, as the egui pane does.
                let room = (width - 2.0 * MARGIN - SCROLLBAR)
                    .max(obs_reading::reading_tokens::size::TEXT_MIN);
                let settings = *pane.view.settings();
                let measure = settings
                    .layout
                    .column
                    .ems()
                    .map_or(room, |ems| room.min(settings.size * ems));
                let left = MARGIN + ((room - measure) / 2.0).floor();
                pane.view.set_width(measure);
                pane.view.text_origin = [
                    f32::from(bounds.origin.x) + left,
                    f32::from(bounds.origin.y),
                ];
                pane.view.pane_rect = [
                    f32::from(bounds.origin.x),
                    f32::from(bounds.origin.y),
                    f32::from(bounds.origin.x) + width,
                    f32::from(bounds.origin.y) + height,
                ];
                let frame = pane.view.frame(height, Some(SYNC_BUDGET));
                pane.last_frame = Some(frame.stats);
                let blocks: Vec<DrawBlock> = frame
                    .blocks
                    .into_iter()
                    .map(|b| DrawBlock {
                        index: b.index,
                        top: b.top,
                        height: b.height,
                        laid: b.laid,
                    })
                    .collect();
                let thumb = pane.thumb(height, width).map(|(r, _)| r);
                let animating = pane.view.is_gliding() || pane.view.has_pending();
                (
                    Arc::clone(&pane.view.doc),
                    pane.palette,
                    left,
                    pane.view.width(),
                    blocks,
                    thumb,
                    animating,
                )
            });
        let fonts = Arc::new(fonts);
        let palette = Arc::new(palette);
        let mut out = PanePrepaint {
            placeholder_color: Some(hsla(palette.ink).opacity(0.06)),
            thumb_color: Some(hsla(palette.ink).opacity(0.25)),
            ..PanePrepaint::default()
        };
        for b in blocks {
            let origin = point(bounds.origin.x + px(left), bounds.origin.y + px(b.top));
            match b.laid {
                Some(laid) => {
                    let Some(block) = doc.blocks.get(b.index) else {
                        continue;
                    };
                    let visible = (-b.top, height - b.top);
                    let mut el = ParaElement {
                        id: ElementId::NamedInteger("reading-block".into(), b.index as u64),
                        size: (measure, b.height),
                        laid,
                        fonts: Arc::clone(&fonts),
                        palette: Arc::clone(&palette),
                        visible,
                        doc: Arc::clone(&doc),
                        unit: block.unit,
                        unit_range: block.source.range.clone(),
                    }
                    .into_any_element();
                    el.prepaint_as_root(
                        origin,
                        size(
                            AvailableSpace::Definite(px(measure)),
                            AvailableSpace::Definite(px(b.height)),
                        ),
                        window,
                        cx,
                    );
                    out.paras.push(el);
                }
                // Not laid out yet (a worker has it): a faint bar where its text will be.
                None => out.placeholders.push(Bounds {
                    origin,
                    size: size(px(measure), px(b.height.max(1.0))),
                }),
            }
        }
        out.thumb = thumb.map(|[x, y, w, h]| Bounds {
            origin: point(bounds.origin.x + px(x), bounds.origin.y + px(y)),
            size: size(px(w), px(h)),
        });
        if animating {
            window.request_animation_frame();
        }
        out
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _rl: &mut (),
        pp: &mut PanePrepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            if let Some(color) = pp.placeholder_color {
                for b in &pp.placeholders {
                    window.paint_quad(fill(*b, color));
                }
            }
            for el in &mut pp.paras {
                el.paint(window, cx);
            }
            if let (Some(thumb), Some(color)) = (pp.thumb, pp.thumb_color) {
                window.paint_quad(fill(thumb, color).corner_radii(px(SCROLLBAR / 2.0)));
            }
        });
    }
}

/// One laid-out block: its glyphs painted with `paint_glyph`, and its accessibility node (a
/// paragraph with its text, and its Tagged words and references as link children).
pub struct ParaElement {
    id: ElementId,
    size: (f32, f32),
    laid: Arc<LaidBlock>,
    fonts: Arc<ReadingFonts>,
    palette: Arc<TonePalette>,
    /// The part of the block in view, in block coordinates (top, bottom).
    visible: (f32, f32),
    doc: Arc<ReadingDoc>,
    unit: usize,
    unit_range: Range<usize>,
}

pub struct ParaPrepaint {
    bounds: Bounds<Pixels>,
    scale: f32,
}

impl IntoElement for ParaElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

/// A link child of a paragraph node: its label and bounds in block points (x0, y0, x1, y1).
#[derive(Debug, Clone, PartialEq)]
pub struct LinkNode {
    pub label: String,
    pub rect: (f32, f32, f32, f32),
    /// The unit byte it starts at (its synthetic node's key).
    pub start: usize,
}

/// The most link children one paragraph node carries.
const MAX_LINKS: usize = 200;

impl ParaElement {
    /// The block's Tagged words and references, in text order.
    pub fn links(&self) -> Vec<LinkNode> {
        let Some(unit) = self.doc.units.get(self.unit) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for span in &unit.spans {
            if out.len() >= MAX_LINKS {
                break;
            }
            if !matches!(
                span.kind,
                SpanKind::TaggedWord { .. } | SpanKind::ScriptureRef { .. } | SpanKind::Xref { .. }
            ) {
                continue;
            }
            let (us, ue) = (span.start as usize, span.end as usize);
            if us < self.unit_range.start || ue > self.unit_range.end || us >= ue {
                continue;
            }
            let (Some(ls), Some(le)) = (self.laid.from_unit(us), self.laid.from_unit(ue)) else {
                continue;
            };
            let Some(rect) = word_rect(&self.laid, ls..le) else {
                continue;
            };
            let label = unit.text.get(us..ue).unwrap_or_default().to_string();
            out.push(LinkNode {
                label,
                rect,
                start: us,
            });
        }
        out
    }
}

impl Element for ParaElement {
    type RequestLayoutState = ();
    type PrepaintState = ParaPrepaint;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = px(self.size.0).into();
        style.size.height = px(self.size.1).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _rl: &mut (),
        window: &mut Window,
        _cx: &mut App,
    ) -> ParaPrepaint {
        ParaPrepaint {
            bounds,
            scale: window.scale_factor(),
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _rl: &mut (),
        _pp: &mut ParaPrepaint,
        window: &mut Window,
        _cx: &mut App,
    ) {
        let (top, bottom) = self.visible;
        let ox = f32::from(bounds.origin.x);
        let oy = f32::from(bounds.origin.y);
        let mut painted = 0usize;
        for (line, item, x, y, glyph) in self.laid.glyphs() {
            let size = item.style.size;
            // A line's ink can reach past its box (marks, descenders): a text size of slack.
            if line.bottom + size < top || line.top - size > bottom {
                continue;
            }
            let Some(font_id) = self.fonts.get(item.style.face) else {
                continue;
            };
            let color = hsla(self.palette.color(item.style.role));
            if window
                .paint_glyph(
                    point(px(ox + x), px(oy + y)),
                    font_id,
                    GlyphId(glyph),
                    px(size),
                    color,
                )
                .is_ok()
            {
                painted += 1;
            }
        }
        #[cfg(feature = "test-support")]
        observed::record(observed::Paragraph {
            role: Role::Paragraph,
            value: self.laid.text.clone(),
            rtl: self.laid.rtl,
            links: self.links(),
            bounds,
            glyphs: painted,
        });
        let _ = painted;
    }

    fn a11y_role(&self) -> Option<Role> {
        Some(Role::Paragraph)
    }

    fn write_a11y_info(&self, node: &mut accesskit::Node) {
        node.set_value(self.laid.text.clone());
        if self.laid.rtl {
            node.set_text_direction(accesskit::TextDirection::RightToLeft);
        }
    }

    fn a11y_synthetic_children(&mut self, pp: &mut ParaPrepaint, builder: &mut A11ySubtreeBuilder) {
        // Bounds as GPUI gives its own element nodes: relative to the window's content, in
        // device pixels (ADR 0004, #372 amendment).
        let ox = f32::from(pp.bounds.origin.x);
        let oy = f32::from(pp.bounds.origin.y);
        let s = pp.scale;
        for link in self.links() {
            let mut node = accesskit::Node::new(Role::Link);
            node.set_label(link.label);
            let (x0, y0, x1, y1) = link.rect;
            node.set_bounds(accesskit::Rect {
                x0: f64::from((ox + x0) * s),
                y0: f64::from((oy + y0) * s),
                x1: f64::from((ox + x1) * s),
                y1: f64::from((oy + y1) * s),
            });
            let id = builder.synthetic_node_id(link.start);
            builder.push_child(id, node);
        }
    }
}

/// The box of laid bytes `range` on the first line it reaches, in block points.
fn word_rect(laid: &LaidBlock, range: Range<usize>) -> Option<(f32, f32, f32, f32)> {
    for line in &laid.lines {
        if line.bytes.end <= range.start || line.bytes.start >= range.end {
            continue;
        }
        let (mut x0, mut x1) = (f32::INFINITY, f32::NEG_INFINITY);
        for item in &line.items {
            for (b, a, z) in item.clusters() {
                if b.start < range.end && b.end > range.start {
                    x0 = x0.min(a);
                    x1 = x1.max(z);
                }
            }
        }
        if x0 < x1 {
            return Some((x0, line.top, x1, line.bottom));
        }
    }
    None
}

/// What the colour roles of laid text are painted in (the egui `Palette`'s roles).
pub fn role_color(palette: &TonePalette, role: ColorRole) -> Hsla {
    hsla(palette.color(role))
}

/// Tests (#380): the paragraph nodes the last frame painted. GPUI's test windows never turn
/// accessibility on (#374), so the node each block would give AccessKit is recorded here, as
/// its element writes it.
#[cfg(feature = "test-support")]
pub mod observed {
    use std::cell::RefCell;

    use super::LinkNode;
    use gpui_kit::{Bounds, Pixels, Role};

    /// A paragraph node, as the block's element writes it.
    #[derive(Debug, Clone)]
    pub struct Paragraph {
        pub role: Role,
        pub value: String,
        pub rtl: bool,
        pub links: Vec<LinkNode>,
        pub bounds: Bounds<Pixels>,
        /// Glyphs painted.
        pub glyphs: usize,
    }

    thread_local! {
        static FRAME: RefCell<Vec<Paragraph>> = const { RefCell::new(Vec::new()) };
    }

    pub(super) fn begin() {
        FRAME.with(|f| f.borrow_mut().clear());
    }

    pub(super) fn record(p: Paragraph) {
        FRAME.with(|f| f.borrow_mut().push(p));
    }

    /// The paragraphs the last frame's reading panes painted, top to bottom.
    pub fn paragraphs() -> Vec<Paragraph> {
        FRAME.with(|f| f.borrow().clone())
    }
}
