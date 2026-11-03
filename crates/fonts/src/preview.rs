//! SVG previews of shaped text, drawn from glyph outlines. For visual checks and `obs-cli
//! shape`; the app paints through the glyph atlas instead (#32).

use std::fmt::Write as _;

use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{GlyphId, MetadataProvider};

use crate::face::{FaceError, face};
use crate::shape::{ShapeStyle, ShapedRun};

struct SvgPen<'a> {
    out: &'a mut String,
    dx: f32,
    dy: f32,
}

impl OutlinePen for SvgPen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let _ = write!(self.out, "M{:.1} {:.1}", self.dx + x, self.dy - y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let _ = write!(self.out, "L{:.1} {:.1}", self.dx + x, self.dy - y);
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let _ = write!(
            self.out,
            "Q{:.1} {:.1} {:.1} {:.1}",
            self.dx + cx,
            self.dy - cy,
            self.dx + x,
            self.dy - y
        );
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let _ = write!(
            self.out,
            "C{:.1} {:.1} {:.1} {:.1} {:.1} {:.1}",
            self.dx + cx0,
            self.dy - cy0,
            self.dx + cx1,
            self.dy - cy1,
            self.dx + x,
            self.dy - y
        );
    }
    fn close(&mut self) {
        self.out.push('Z');
    }
}

/// One SVG `<path>` `d` attribute for a shaped run, with its pen at (`x`, `baseline`).
pub fn run_path(
    run: &ShapedRun,
    style: ShapeStyle,
    x: f32,
    baseline: f32,
) -> Result<String, FaceError> {
    let f = face(style.face)?;
    let Some(font) = f.font_ref() else {
        return Err(FaceError(style.face));
    };
    let outlines = font.outline_glyphs();
    let size = Size::new(style.size());
    let mut d = String::new();
    for g in &run.glyphs {
        if let Some(outline) = outlines.get(GlyphId::new(g.id)) {
            let mut pen = SvgPen {
                out: &mut d,
                dx: x + g.x,
                dy: baseline - g.y,
            };
            let settings = DrawSettings::unhinted(size, LocationRef::default());
            let _ = outline.draw(settings, &mut pen);
        }
    }
    Ok(d)
}

/// A standalone SVG of several lines of shaped text, one run per line.
pub fn lines_svg(lines: &[(ShapedRun, ShapeStyle)]) -> Result<String, FaceError> {
    let margin = 16.0;
    let mut y = margin;
    let mut width: f32 = 0.0;
    let mut paths = String::new();
    for (run, style) in lines {
        let (ascent, descent, _) = face(style.face)?.metrics.scaled(style.size());
        let baseline = y + ascent;
        let d = run_path(run, *style, margin, baseline)?;
        let _ = writeln!(paths, "  <path d=\"{d}\"/>");
        y = baseline - descent + style.size() * 0.3;
        width = width.max(run.width);
    }
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{:.0}\" height=\"{:.0}\">\n  <rect width=\"100%\" height=\"100%\" fill=\"#fffdf7\"/>\n  <g fill=\"#222\">\n{paths}  </g>\n</svg>\n",
        width + 2.0 * margin,
        y + margin
    ))
}
