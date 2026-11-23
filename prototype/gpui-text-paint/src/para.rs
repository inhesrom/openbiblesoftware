//! PROTOTYPE — throwaway (#372). One GPUI element per laid-out block: it paints our shaped,
//! bidi-ordered, line-broken glyphs with `Window::paint_glyph` (approach (a)), and exposes the
//! block to assistive technology as a `Role::Paragraph` with its text, plus its Tagged words as
//! synthetic `Role::Link` children.

use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::{
    App, Bounds, Element, ElementId, GlobalElementId, GlyphId, Hsla, InspectorElementId,
    IntoElement, LayoutId, Pixels, Position, Role, Style, Window, accesskit, point, px,
};
use obs_fonts::FaceId;
use obs_layout::{ColorRole, LaidBlock};
use obs_model::SpanKind;

/// Glyphs drawn last frame (all panels), for the bench.
pub static GLYPHS_DRAWN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// obs-fonts face → GPUI font, resolved once after the same font files were registered with
/// GPUI's text system.
pub type FontMap = HashMap<FaceId, gpui_kit::FontId>;

pub fn color(role: ColorRole) -> Hsla {
    // The egui app's Paper palette (crates/ui-egui/src/palette.rs).
    let c = match role {
        ColorRole::Ink => 0x202629,
        ColorRole::VerseNumber => 0x6e521a,
        ColorRole::Link => 0x26447a,
        ColorRole::NoteMarker => 0x1e6258,
        ColorRole::Accent => 0x26447a,
        ColorRole::WordsOfJesus => 0x96201e,
        ColorRole::Interlinear => 0x505b60,
    };
    gpui_kit::rgb(c).into()
}

pub struct ParaElement {
    pub id: ElementId,
    /// Position and size in the reading area, in points.
    pub origin: (f32, f32),
    pub size: (f32, f32),
    pub laid: Arc<LaidBlock>,
    pub fonts: Arc<FontMap>,
    /// The part of the block that is in view, in block coordinates (y range).
    pub visible: (f32, f32),
    /// For the accessibility tree: the Bible, the block's unit and its unit byte range.
    pub bible: Arc<crate::data::Bible>,
    pub unit_ix: usize,
    pub unit_range: std::ops::Range<usize>,
}

pub struct Prepaint {
    bounds: Bounds<Pixels>,
    scale: f32,
}

impl IntoElement for ParaElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for ParaElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

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
        style.position = Position::Absolute;
        style.inset.left = px(self.origin.0).into();
        style.inset.top = px(self.origin.1).into();
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
    ) -> Prepaint {
        Prepaint { bounds, scale: window.scale_factor() }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _rl: &mut (),
        _pp: &mut Prepaint,
        window: &mut Window,
        _cx: &mut App,
    ) {
        let (top, bottom) = self.visible;
        let ox = f32::from(bounds.origin.x);
        let oy = f32::from(bounds.origin.y);
        let mut n = 0u64;
        for (line, item, x, y, glyph) in self.laid.glyphs() {
            let size = item.style.size;
            // A line's ink can reach past its box (marks, descenders): a text size of slack.
            if line.bottom + size < top || line.top - size > bottom {
                continue;
            }
            let Some(&font_id) = self.fonts.get(&item.style.face) else {
                continue;
            };
            let _ = window.paint_glyph(
                point(px(ox + x), px(oy + y)),
                font_id,
                GlyphId(glyph),
                px(size),
                color(item.style.role),
            );
            n += 1;
        }
        GLYPHS_DRAWN.fetch_add(n, std::sync::atomic::Ordering::Relaxed);
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

    fn a11y_synthetic_children(
        &mut self,
        pp: &mut Prepaint,
        builder: &mut gpui_kit::A11ySubtreeBuilder,
    ) {
        // Tagged words as links. Bounds: like GPUI's own element nodes (element.rs), relative to
        // the window's content, in device pixels (points × scale factor), with no transform.
        let ox = f32::from(pp.bounds.origin.x);
        let oy = f32::from(pp.bounds.origin.y);
        let s = pp.scale;
        let mut pushed = 0;
        let Some(unit) = self.bible.units.get(self.unit_ix) else { return };
        for span in &unit.spans {
            if pushed >= 100 {
                break;
            }
            if !matches!(span.kind, SpanKind::TaggedWord { .. } | SpanKind::ScriptureRef { .. }) {
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
            let mut node = accesskit::Node::new(Role::Link);
            if let Some(text) = unit.text.get(us..ue) {
                node.set_label(text.to_string());
            }
            node.set_bounds(accesskit::Rect {
                x0: f64::from((ox + rect.0) * s),
                y0: f64::from((oy + rect.1) * s),
                x1: f64::from((ox + rect.2) * s),
                y1: f64::from((oy + rect.3) * s),
            });
            let id = builder.synthetic_node_id(us);
            if builder.push_child(id, node) {
                pushed += 1;
            }
        }
    }
}

/// The box of laid bytes `range` on the first line it reaches: (x0, y0, x1, y1) in block points.
fn word_rect(laid: &LaidBlock, range: std::ops::Range<usize>) -> Option<(f32, f32, f32, f32)> {
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
