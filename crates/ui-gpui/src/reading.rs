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
//! children (#19). Marks and highlights come with #382.
//!
//! Selection, copy, Find, the Tagged word under the pointer and the right-click (#381), as the
//! egui pane's `handle_selection` and its overlays: a drag selects, a double-click selects a
//! word and a triple-click a Verse, in `obs-reading`'s document coordinates
//! ([`Selection`]), hit-tested through `obs-layout`'s laid lines ([`ReadingView::pos_at`]), so
//! a selection survives scrolling and works the same in right-to-left Hebrew. The selection
//! and the Find bar's matches ([`crate::find`]) are painted under the glyphs in the Page
//! tone's colours; the Tagged word under the pointer is underlined. The Shell takes what the
//! pane reports (the hovered and clicked word, the right-click) for Word Info's dwell and the
//! context menu ([`crate::context_menu`]).

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use std::time::Instant;

use gpui_kit::base::TestSupportExt;
use gpui_kit::prelude::*;
use gpui_kit::{
    A11ySubtreeBuilder, AnyElement, App, AvailableSpace, Bounds, ClipboardItem, ContentMask,
    Context, Element, ElementId, FocusHandle, FontId, Global, GlobalElementId, GlyphId, Hsla,
    InspectorElementId, KeyDownEvent, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, Role, ScrollDelta, ScrollEvent, SharedString, Style, Window,
    accesskit, div, fill, font, point, px, relative, size,
};
use obs_engine::Session;
use obs_fonts::FaceId;
use obs_layout::{ColorRole, LaidBlock, TextSettings, UnitPos};
use obs_model::SpanKind;
use obs_reading::reading_tokens::{motion, space};
use obs_reading::selection::text_range_rects;
use obs_reading::view::MARGIN;
use obs_reading::{
    DocPos, FrameStats, Granularity, ReadingDoc, ReadingView, SYNC_BUDGET, Selection, TonePalette,
    range_rects, selection_rects,
};
use obs_scripture::Verse;

use crate::find::FindBar;
use crate::tokens::{mark, stroke};

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

/// How far the pointer moves with the button down before a press is a drag, not a click
/// (egui's `max_click_dist`).
pub const CLICK_SLOP: f32 = 6.0;

/// How fast a selection drag past the pane's top or bottom edge scrolls: this share of the
/// distance past it, per pointer move (the egui pane's).
const DRAG_SCROLL: f32 = 0.3;

/// A Tagged word in a pane: its unit and bytes (the egui pane's `WordHit`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordHit {
    pub unit: usize,
    pub range: Range<usize>,
}

impl WordHit {
    /// Its first and last positions, as a document range.
    pub fn span(&self) -> (DocPos, DocPos) {
        (
            DocPos {
                unit: self.unit,
                byte: self.range.start,
            },
            DocPos {
                unit: self.unit,
                byte: self.range.end,
            },
        )
    }
}

/// The right-click menu asked for on a pane (a right-click, or Shift+F10): what it opens on.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuRequest {
    /// Where it opens, in window points.
    pub at: Point<Pixels>,
    /// The text under the pointer, when it was on the text.
    pub pointer_at: Option<DocPos>,
    /// The Tagged word under the pointer.
    pub word: Option<WordHit>,
    /// The Scripture reference under the pointer: unit and span indexes.
    pub reference: Option<(usize, usize)>,
}

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
    /// The thumb is being dragged: the left button went down in the scroll bar's track and has
    /// not come up, over the pane or outside it.
    dragging: bool,
    /// The pane was drawn since the Shell last asked ([`Self::take_drawn`]): a pane not shown
    /// (a Tab behind another in its Panel) is never asked where it is, so it never lays out
    /// outside a frame's budget.
    drawn: bool,
    /// The user scrolled the pane (wheel, keys, scroll bar) since the Shell last asked
    /// ([`Self::take_user_moved`]).
    user_moved: bool,
    /// The pane's bounds as last drawn, in window points.
    bounds: Bounds<Pixels>,
    /// What the last frame cost.
    last_frame: Option<FrameStats>,
    /// A left press on the text that has not come up: where it went down, and whether it has
    /// moved past [`CLICK_SLOP`] (a drag selecting) since.
    pressed: Option<(Point<Pixels>, bool)>,
    /// The pointer over the pane, in window points.
    pointer: Option<Point<Pixels>>,
    /// The Tagged word under the pointer (underlined; Word Info follows it after its dwell).
    hovered_word: Option<WordHit>,
    /// The Scripture reference under the pointer: unit and span indexes.
    hovered_reference: Option<(usize, usize)>,
    /// The Tagged word clicked since the Shell last asked (Word Info pins it), and whether
    /// Ctrl or Cmd was held.
    clicked_word: Option<(WordHit, bool)>,
    /// The right-click menu asked for since the Shell last asked.
    menu_request: Option<MenuRequest>,
    /// The Tab's Find bar, while it is open (#175).
    find: Option<FindBar>,
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
            // Tab reaches the pane, so the keyboard can scroll it without a click.
            focus: cx.focus_handle().tab_stop(true),
            started: Instant::now(),
            height: 0.0,
            dragging: false,
            drawn: false,
            user_moved: false,
            bounds: Bounds::default(),
            last_frame: None,
            pressed: None,
            pointer: None,
            hovered_word: None,
            hovered_reference: None,
            clicked_word: None,
            menu_request: None,
            find: None,
        }
    }

    /// The selection, if any.
    pub fn selection(&self) -> Option<Selection> {
        self.view.selection
    }

    /// The Tagged word under the pointer.
    pub fn hovered_word(&self) -> Option<&WordHit> {
        self.hovered_word.as_ref()
    }

    /// The Scripture reference under the pointer: unit and span indexes.
    pub fn hovered_reference(&self) -> Option<(usize, usize)> {
        self.hovered_reference
    }

    /// Where the pointer is over the pane, in window points.
    pub fn pointer(&self) -> Option<Point<Pixels>> {
        self.pointer
    }

    /// The Tagged word clicked since this was last asked, and whether Ctrl or Cmd was held.
    pub fn take_clicked_word(&mut self) -> Option<(WordHit, bool)> {
        self.clicked_word.take()
    }

    /// The right-click menu asked for since this was last asked.
    pub fn take_menu_request(&mut self) -> Option<MenuRequest> {
        self.menu_request.take()
    }

    /// The Find bar, while it is open.
    pub fn find(&self) -> Option<&FindBar> {
        self.find.as_ref()
    }

    pub fn find_mut(&mut self) -> Option<&mut FindBar> {
        self.find.as_mut()
    }

    /// Open the Find bar (`tab.find`, Ctrl+F) with `text`, else the selection if it is short and
    /// on one line, else what it had; the keyboard goes into its field.
    pub fn open_find(&mut self, text: Option<String>) {
        let selection = crate::find::selection_text(&self.view);
        let bar = self.find.get_or_insert_with(FindBar::default);
        if let Some(text) = text.or(selection) {
            bar.text = text;
        }
        bar.focus = true;
    }

    /// Step the Find bar to its next match, or its previous one.
    pub fn step_find(&mut self, forward: bool) {
        if let Some(bar) = self.find.as_mut() {
            bar.step = Some(forward);
        }
    }

    /// Close the Find bar; its matches go.
    pub fn close_find(&mut self) {
        self.find = None;
    }

    /// A window point in the text's coordinates: from the text's left edge and the viewport
    /// top.
    fn text_point(&self, p: Point<Pixels>) -> (f32, f32) {
        let [ox, oy] = self.view.text_origin;
        (f32::from(p.x) - ox, f32::from(p.y) - oy)
    }

    /// The document position at window point `p`, its height clamped into the pane (a drag
    /// above or below it selects to the text at its edge).
    fn pos_at(&mut self, p: Point<Pixels>) -> Option<DocPos> {
        let (x, y) = self.text_point(p);
        let height = f32::from(self.bounds.size.height);
        self.view.pos_at(x, y.clamp(0.0, (height - 1.0).max(0.0)))
    }

    /// The Tagged word at window point `p`, when its block is laid out.
    fn word_at(&self, p: Point<Pixels>) -> Option<WordHit> {
        let (x, y) = self.text_point(p);
        let (unit, pos) = self.view.hit_in_unit_at(x, y)?;
        let UnitPos::Text(byte) = pos else {
            return None;
        };
        let (range, _) = self.view.doc.tagged_word_at(unit, byte)?;
        Some(WordHit { unit, range })
    }

    /// The left button went down on the text, the `count`th click in a row: one starts a
    /// selection there, two select the word, three the Verse (the egui pane's
    /// `handle_selection`).
    pub fn press(&mut self, p: Point<Pixels>, count: usize) {
        let Some(at) = self.pos_at(p) else {
            return;
        };
        if count >= 2 {
            self.pressed = None;
            self.view.selection = Some(Selection {
                anchor: at,
                head: at,
                granularity: if count >= 3 {
                    Granularity::Verse
                } else {
                    Granularity::Word
                },
            });
            return;
        }
        self.pressed = Some((p, false));
    }

    /// The pointer moved to `p` with the left button down: past [`CLICK_SLOP`] the press is a
    /// drag, and the selection runs from where it went down to here. Past the pane's top or
    /// bottom edge the text scrolls, in proportion to the distance.
    pub fn drag(&mut self, p: Point<Pixels>) {
        let Some((from, moved)) = self.pressed else {
            return;
        };
        if !moved {
            let (dx, dy) = (f32::from(p.x - from.x), f32::from(p.y - from.y));
            if dx.hypot(dy) < CLICK_SLOP {
                return;
            }
            self.pressed = Some((from, true));
            self.view.selection = self.pos_at(from).map(Selection::caret);
        }
        let b = self.bounds;
        let (top, bottom) = (b.origin.y, b.origin.y + b.size.height);
        let past = if p.y < top {
            f32::from(p.y - top)
        } else if p.y > bottom {
            f32::from(p.y - bottom)
        } else {
            0.0
        };
        if past != 0.0 {
            self.view.scroll_by(past * DRAG_SCROLL);
            self.user_moved = true;
        }
        if let (Some(head), Some(sel)) = (self.pos_at(p), self.view.selection.as_mut()) {
            sel.head = head;
        }
    }

    /// The left button came up at `p`, with Ctrl or Cmd held or not (`command`). A press that
    /// never moved is a click: it clears the selection, and on a Tagged word it is that word's
    /// click (Word Info pins it).
    pub fn release(&mut self, p: Point<Pixels>, command: bool) {
        let Some((_, moved)) = self.pressed.take() else {
            return;
        };
        if !moved {
            self.view.selection = None;
            self.clicked_word = self.word_at(p).map(|w| (w, command));
        }
    }

    /// The pointer is at `p` over the pane: the Tagged word and the Scripture reference under
    /// it, from the layout already in view (hover never lays a block out).
    pub fn hover(&mut self, p: Point<Pixels>) {
        self.pointer = Some(p);
        if !self.bounds.contains(&p) {
            self.leave();
            return;
        }
        self.hovered_word = self.word_at(p);
        let (x, y) = self.text_point(p);
        self.hovered_reference = self.view.reference_at(x, y);
    }

    /// The pointer left the pane.
    pub fn leave(&mut self) {
        self.pointer = None;
        self.hovered_word = None;
        self.hovered_reference = None;
    }

    /// A right-click at `p`: the menu acts on the selection under it, else on the word there
    /// (which becomes the selection), with the Verse and Tagged word there. As in the egui
    /// pane, the position is the text nearest the pointer (`pos_at`), so a click below the
    /// text acts on its last Verse.
    pub fn context_click(&mut self, p: Point<Pixels>) {
        self.hover(p);
        let pointer_at = self.pos_at(p);
        if let Some(at) = pointer_at {
            select_for_menu(&mut self.view, at);
        }
        self.menu_request = Some(MenuRequest {
            at: p,
            pointer_at,
            word: self.hovered_word.clone(),
            reference: self.hovered_reference,
        });
    }

    /// Shift+F10: the menu from the keyboard, at the word under the pointer, else on the
    /// selection; with neither, on the Verse at the top of the view, selected so it shows what
    /// the menu is for (the egui Shell's `reading_menu`).
    pub fn keyboard_menu(&mut self) {
        if let Some(p) = self.pointer
            && self.word_at(p).is_some()
        {
            self.context_click(p);
            return;
        }
        if !self
            .view
            .selection
            .is_some_and(|s| has_text(&self.view, &s))
        {
            let doc = Arc::clone(&self.view.doc);
            if let Some(verse) = self.view.current_location() {
                self.view.selection = verse_selection(&doc, verse);
            }
        }
        let b = self.bounds;
        let at = point(b.origin.x + px(space::XXXL), b.origin.y + px(space::XXXL));
        self.menu_request = Some(MenuRequest {
            at,
            pointer_at: None,
            word: None,
            reference: None,
        });
    }

    /// What Copy puts on the clipboard: the selection with its reference and the Resource's
    /// abbreviation (`obs-reading`'s `copy_text`); `None` with nothing selected.
    pub fn copy_text(&self) -> Option<String> {
        let sel = self
            .view
            .selection
            .filter(|s| !s.is_empty() || s.granularity != Granularity::Grapheme)?;
        Some(self.view.copy_text(&sel))
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

    /// Whether the pane was drawn since this was last asked.
    pub(crate) fn take_drawn(&mut self) -> bool {
        std::mem::take(&mut self.drawn)
    }

    /// Whether the user scrolled the pane since this was last asked.
    pub(crate) fn take_user_moved(&mut self) -> bool {
        std::mem::take(&mut self.user_moved)
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
        self.user_moved = true;
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
        self.user_moved = true;
    }
}

impl ReadingPane {
    /// The Find matches, the selection and the hovered word's underline in the blocks laid
    /// out in a viewport `height` tall, in window points; `origin` is the text's left edge
    /// and the viewport top.
    fn overlay(&self, [ox, oy]: [f32; 2], height: f32) -> Overlay {
        let to_window = |[x0, y0, x1, y1]: [f32; 4]| Bounds {
            origin: point(px(ox + x0), px(oy + y0)),
            size: size(px((x1 - x0).max(0.0)), px((y1 - y0).max(0.0))),
        };
        let p = &self.palette;
        let mut out = Overlay {
            colors: OverlayColors {
                find_match: hsla(p.find_match),
                search_hit: hsla(p.search_hit),
                find_current: hsla(p.find_current),
                selection: hsla(p.selection),
                underline: hsla(p.underline),
            },
            ..Overlay::default()
        };
        let text_size = self.view.settings().size;
        if let Some(bar) = &self.find {
            for (k, rects) in found_rects(&self.view, bar, height) {
                let current = bar.current == Some(k);
                for r in rects {
                    out.found
                        .push((to_window(text_band(r, text_size)), current));
                }
            }
        }
        if let Some(sel) = self.view.selection {
            out.selection = selection_rects(&self.view, &sel, height)
                .into_iter()
                .map(to_window)
                .collect();
        }
        if let Some(w) = &self.hovered_word {
            // A hairline just above the line's bottom, as the egui pane draws it.
            for [x0, _, x1, y1] in range_rects(&self.view, &[w.span()], height)
                .into_iter()
                .flatten()
            {
                let y = y1 - mark::LINE_MIN;
                out.underline.push(to_window([x0, y, x1, y + stroke::HAIR]));
            }
        }
        out
    }
}

/// The Find matches (#175) in the blocks laid out in a viewport `height` tall, by index, with
/// their rectangles (letters only: never a verse number before them). Only the matches
/// reaching into those blocks are looked at, however many the Resource has (the egui pane's
/// `found_rects`).
fn found_rects(view: &ReadingView, bar: &FindBar, height: f32) -> Vec<(usize, Vec<[f32; 4]>)> {
    if bar.matches().is_empty() {
        return Vec::new();
    }
    let mut shown: Vec<usize> = Vec::new();
    for (index, _, _) in view.laid_in_view(height) {
        let Some(block) = view.doc.blocks.get(index) else {
            continue;
        };
        shown.extend(bar.within(block.unit, block.source.range.clone()));
    }
    shown.sort_unstable();
    shown.dedup();
    let matches = bar.matches();
    let ranges: Vec<(DocPos, DocPos)> = shown
        .iter()
        .filter_map(|&k| matches.get(k).map(|m| (m.start(), m.end())))
        .collect();
    shown
        .into_iter()
        .zip(text_range_rects(view, &ranges, height))
        .collect()
}

/// A match's band: [`mark::BAND_EMS`] tall round the line's middle, not the line spacing
/// around it (the egui pane's `text_band`).
fn text_band([x0, y0, x1, y1]: [f32; 4], size: f32) -> [f32; 4] {
    let inset = ((y1 - y0 - size * mark::BAND_EMS) / 2.0).max(0.0);
    [x0, y0 + inset, x1, y1 - inset]
}

/// The right-click menu acts on the selection under `at`, else on the word there (which
/// becomes the selection; none when `at` is not on a word), as the egui pane's
/// `select_for_menu`.
pub fn select_for_menu(view: &mut ReadingView, at: DocPos) {
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

/// Whether a selection covers any text other than white space (stopping at the first such
/// character), as the egui pane's `has_text`.
pub fn has_text(view: &ReadingView, sel: &Selection) -> bool {
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

/// A whole Verse of a Bible as a selection.
pub fn verse_selection(doc: &ReadingDoc, verse: Verse) -> Option<Selection> {
    let (unit, byte) = doc.verse_position(verse)?;
    let range = doc.verse_range_at(unit, byte)?;
    Some(Selection {
        anchor: DocPos {
            unit,
            byte: range.start,
        },
        head: DocPos {
            unit,
            byte: range.end,
        },
        granularity: Granularity::Grapheme,
    })
}

impl Render for ReadingPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = hsla(self.palette.page);
        let pane = cx.entity();
        // The pane is one document named by its Resource's title, as the egui pane publishes
        // it (`a11y::publish`); its paragraphs are its blocks' nodes.
        let title = SharedString::from(self.view.doc.manifest.title.clone());
        div()
            .id("reading-pane")
            .role(Role::Document)
            .aria_label(title)
            .test_support()
            .track_focus(&self.focus)
            .size_full()
            .overflow_hidden()
            .bg(page)
            .on_key_down(cx.listener(|pane, ev: &KeyDownEvent, _, cx| {
                let ks = &ev.keystroke;
                let command = ks.modifiers.control || ks.modifiers.platform;
                if command && ks.key == "c" {
                    // Copy: the selection with its reference.
                    if let Some(text) = pane.copy_text() {
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                        cx.stop_propagation();
                    }
                    return;
                }
                if ks.key == "f10" && ks.modifiers.shift {
                    pane.keyboard_menu();
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                if !command && pane.key(ks.key.as_str()) {
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
                    let in_track = f32::from(ev.position.x) >= track - space::XS;
                    pane.dragging = in_track;
                    if in_track {
                        pane.drag_to(ev.position.y);
                    } else {
                        pane.press(ev.position, ev.click_count);
                    }
                    cx.notify();
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|pane, ev: &MouseDownEvent, window, cx| {
                    window.focus(&pane.focus, cx);
                    pane.context_click(ev.position);
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|pane, ev: &MouseMoveEvent, _, cx| {
                let left = ev.pressed_button == Some(MouseButton::Left);
                if pane.dragging && left {
                    pane.drag_to(ev.position.y);
                } else if left {
                    pane.drag(ev.position);
                }
                let before = (pane.hovered_word.clone(), pane.hovered_reference);
                pane.hover(ev.position);
                if left || before != (pane.hovered_word.clone(), pane.hovered_reference) {
                    cx.notify();
                }
            }))
            .on_hover(cx.listener(|pane, hovered: &bool, _, cx| {
                if !*hovered {
                    pane.leave();
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|pane, ev: &MouseUpEvent, _, cx| {
                    pane.dragging = false;
                    let m = ev.modifiers;
                    pane.release(ev.position, m.control || m.platform);
                    cx.notify();
                }),
            )
            // A drag let go outside the pane ends too, so the thumb does not stick to the
            // pointer and the selection stops following it.
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|pane, _: &MouseUpEvent, _, cx| {
                    pane.dragging = false;
                    if pane.pressed.take().is_some() {
                        cx.notify();
                    }
                }),
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
    overlay: Overlay,
}

/// What a pane paints with its text, in window points: the Find bar's matches and the
/// selection under the glyphs, the hovered Tagged word's underline over them.
#[derive(Default, Clone)]
pub(crate) struct Overlay {
    /// Each match's rectangles, and whether it is the current match.
    pub found: Vec<(Bounds<Pixels>, bool)>,
    pub selection: Vec<Bounds<Pixels>>,
    pub underline: Vec<Bounds<Pixels>>,
    pub colors: OverlayColors,
}

#[derive(Default, Clone, Copy)]
pub(crate) struct OverlayColors {
    pub find_match: Hsla,
    pub search_hit: Hsla,
    pub find_current: Hsla,
    pub selection: Hsla,
    pub underline: Hsla,
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
        let (doc, palette, left, measure, blocks, thumb, animating, overlay) =
            self.pane.update(cx, |pane, _| {
                pane.bounds = bounds;
                pane.height = height;
                pane.drawn = true;
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
                // The Find bar finds (on its worker) and steps before the frame, so a match it
                // scrolls to is drawn in this frame.
                let mut finding = false;
                if let Some(bar) = pane.find.as_mut() {
                    if bar.update(&mut pane.view, height) {
                        pane.user_moved = true;
                    }
                    finding = bar.is_finding();
                }
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
                let animating = pane.view.is_gliding() || pane.view.has_pending() || finding;
                let origin = [
                    f32::from(bounds.origin.x) + left,
                    f32::from(bounds.origin.y),
                ];
                let overlay = pane.overlay(origin, height);
                (
                    Arc::clone(&pane.view.doc),
                    pane.palette,
                    left,
                    pane.view.width(),
                    blocks,
                    thumb,
                    animating,
                    overlay,
                )
            });
        let fonts = Arc::new(fonts);
        let palette = Arc::new(palette);
        let mut out = PanePrepaint {
            placeholder_color: Some(hsla(palette.ink).opacity(0.06)),
            thumb_color: Some(hsla(palette.ink).opacity(0.25)),
            overlay,
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
            let o = &pp.overlay;
            #[cfg(feature = "test-support")]
            observed::record_overlay(observed::Overlay {
                found: o.found.clone(),
                selection: o.selection.clone(),
                underline: o.underline.clone(),
            });
            // Find matches (#175) under the selection: the current one in the search-hit
            // colour with an outline, the others in the paler find-match colour.
            for (r, current) in &o.found {
                if *current {
                    let w = px(stroke::MEDIUM);
                    let outer = r.dilate(w);
                    window.paint_quad(
                        fill(outer, o.colors.search_hit)
                            .corner_radii(px(crate::tokens::radius::XS))
                            .border_widths(w)
                            .border_color(o.colors.find_current),
                    );
                } else {
                    window.paint_quad(
                        fill(*r, o.colors.find_match).corner_radii(px(crate::tokens::radius::XS)),
                    );
                }
            }
            for r in &o.selection {
                window.paint_quad(fill(*r, o.colors.selection));
            }
            for el in &mut pp.paras {
                el.paint(window, cx);
            }
            for r in &o.underline {
                window.paint_quad(fill(*r, o.colors.underline));
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
        let painted = paint_laid(
            &self.laid,
            bounds.origin,
            self.visible,
            &self.fonts,
            &self.palette,
            window,
        );
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

/// Paint `laid`'s glyphs with its top left at `origin`, the lines reaching into `visible`
/// (top, bottom, in block points) only. Returns how many were painted.
fn paint_laid(
    laid: &LaidBlock,
    origin: Point<Pixels>,
    (top, bottom): (f32, f32),
    fonts: &ReadingFonts,
    palette: &TonePalette,
    window: &mut Window,
) -> usize {
    let ox = f32::from(origin.x);
    let oy = f32::from(origin.y);
    let mut painted = 0usize;
    for (line, item, x, y, glyph) in laid.glyphs() {
        let size = item.style.size;
        // A line's ink can reach past its box (marks, descenders): a text size of slack.
        if line.bottom + size < top || line.top - size > bottom {
            continue;
        }
        let Some(font_id) = fonts.get(item.style.face) else {
            continue;
        };
        let color = hsla(palette.color(item.style.role));
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
    painted
}

/// Resource text outside a reading pane (a reference's preview): one paragraph laid out by our
/// pipeline (ADR 0004: hover popups too) and painted with `paint_glyph`, `width` points wide.
pub struct SnippetElement {
    laid: Arc<LaidBlock>,
    fonts: Arc<ReadingFonts>,
    palette: Arc<TonePalette>,
}

impl SnippetElement {
    /// `text` laid out now, in `role`'s colour, `width` points wide.
    pub fn new(
        text: &str,
        rtl: bool,
        width: f32,
        settings: &TextSettings,
        palette: TonePalette,
        cx: &App,
    ) -> Self {
        let para = obs_layout::snippet_paragraph(text, ColorRole::Ink, rtl, settings);
        let laid = obs_reading::lay_out(&para, width);
        Self {
            laid: Arc::new(laid),
            fonts: Arc::new(cx.try_global::<ReadingFonts>().cloned().unwrap_or_default()),
            palette: Arc::new(palette),
        }
    }

    /// The text it lays out.
    pub fn text(&self) -> &str {
        &self.laid.text
    }
}

impl IntoElement for SnippetElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for SnippetElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

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
        style.size.width = px(self.laid.width).into();
        style.size.height = px(self.laid.height).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _rl: &mut (),
        _window: &mut Window,
        _cx: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _rl: &mut (),
        _pp: &mut (),
        window: &mut Window,
        _cx: &mut App,
    ) {
        let visible = (0.0, self.laid.height);
        paint_laid(
            &self.laid,
            bounds.origin,
            visible,
            &self.fonts,
            &self.palette,
            window,
        );
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

    /// What a pane painted with its text (#381), in window points.
    #[derive(Debug, Clone, Default)]
    pub struct Overlay {
        /// Each Find match's rectangles, and whether it is the current match.
        pub found: Vec<(Bounds<Pixels>, bool)>,
        pub selection: Vec<Bounds<Pixels>>,
        /// The hovered Tagged word's underline.
        pub underline: Vec<Bounds<Pixels>>,
    }

    thread_local! {
        static FRAME: RefCell<Vec<Paragraph>> = const { RefCell::new(Vec::new()) };
        static OVERLAYS: RefCell<Vec<Overlay>> = const { RefCell::new(Vec::new()) };
    }

    pub(super) fn begin() {
        FRAME.with(|f| f.borrow_mut().clear());
        OVERLAYS.with(|f| f.borrow_mut().clear());
    }

    pub(super) fn record_overlay(o: Overlay) {
        OVERLAYS.with(|f| f.borrow_mut().push(o));
    }

    /// What the last frame's reading panes painted with their text, one per pane.
    pub fn overlays() -> Vec<Overlay> {
        OVERLAYS.with(|f| f.borrow().clone())
    }

    pub(super) fn record(p: Paragraph) {
        FRAME.with(|f| f.borrow_mut().push(p));
    }

    /// The paragraphs the last frame's reading panes painted, top to bottom.
    pub fn paragraphs() -> Vec<Paragraph> {
        FRAME.with(|f| f.borrow().clone())
    }
}
