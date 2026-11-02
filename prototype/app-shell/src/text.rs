//! PROTOTYPE: thin slice of the ADR 0004 text pipeline.
//! harfrust + skrifa shaping, unicode-bidi (UAX #9), unicode-linebreak (UAX #14),
//! own hit-testing, vello_cpu raster into one texture per block. No egui types below `raster`.

use harfrust::{Direction, Feature, ShaperData, Tag, UnicodeBuffer};
use skrifa::MetadataProvider;
use std::sync::Arc;
use unicode_bidi::{BidiInfo, Level};
use vello_cpu::peniko;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontId {
    Regular = 0,
    Italic = 1,
    Bold = 2,
    Hebrew = 3,
}

pub struct Face {
    bytes: Arc<Vec<u8>>,
    shaper: ShaperData,
    pub upem: f32,
    pub ascent: f32,
    pub descent: f32,
    pub peniko: peniko::FontData,
}

impl Face {
    fn load(bytes: Vec<u8>) -> Face {
        let bytes = Arc::new(bytes);
        let fr = skrifa::FontRef::new(&bytes).expect("font");
        let shaper = ShaperData::new(&fr);
        let m = fr.metrics(skrifa::instance::Size::unscaled(), skrifa::instance::LocationRef::default());
        let peniko = peniko::FontData::new(peniko::Blob::new(bytes.clone()), 0);
        Face { shaper, upem: m.units_per_em as f32, ascent: m.ascent, descent: -m.descent, peniko, bytes }
    }
    pub fn font_ref(&self) -> skrifa::FontRef<'_> {
        skrifa::FontRef::new(&self.bytes).unwrap()
    }
}

pub struct Fonts {
    pub faces: [Face; 4],
}

impl Fonts {
    pub fn load(dir: &std::path::Path) -> Fonts {
        let f = |n: &str| {
            Face::load(std::fs::read(dir.join(n)).unwrap_or_else(|e| panic!("font {n}: {e}. Run `python3 prep_data.py`.")))
        };
        Fonts {
            faces: [
                f("GentiumPlus-Regular.ttf"),
                f("GentiumPlus-Italic.ttf"),
                f("GentiumPlus-Bold.ttf"),
                f("SILEOT.ttf"),
            ],
        }
    }
    pub fn face(&self, id: FontId) -> &Face {
        &self.faces[id as usize]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub font: FontId,
    pub size: f32,
    pub color: [u8; 4],
    pub smcp: bool,
    /// baseline raise, in px (positive = up)
    pub raise: f32,
}

/// Where a run's text came from: real Resource text (byte offset of run start in the
/// text unit) or a synthetic object (verse number, note marker) anchored at a unit byte.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Src {
    Text(usize),
    Object { anchor: usize, id: u32 },
}

#[derive(Clone, Debug)]
pub struct Run {
    pub text: String,
    pub style: Style,
    pub src: Src,
}

#[derive(Clone, Debug)]
pub struct Para {
    pub runs: Vec<Run>,
    pub rtl: bool,
    pub line_spacing: f32,
    pub indent: f32,
    pub space_before: f32,
    pub space_after: f32,
    pub center: bool,
}

#[derive(Clone, Debug)]
pub struct PGlyph {
    pub id: u32,
    pub x: f32,
    pub y: f32,
}

/// One cluster (grapheme-ish unit from the shaper) with its visual extent.
#[derive(Clone, Copy, Debug)]
pub struct Cl {
    pub x0: f32,
    pub x1: f32,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub x: f32,
    pub w: f32,
    pub run: usize,
    pub rtl: bool,
    pub style: Style,
    pub glyphs: Vec<PGlyph>,
    /// visual order, x relative to item
    pub clusters: Vec<Cl>,
    pub start: usize,
    pub end: usize,
    trail_ws: f32,
    breakable: bool,
}

#[derive(Clone, Debug)]
pub struct Line {
    pub top: f32,
    pub baseline: f32,
    pub bottom: f32,
    pub items: Vec<Item>,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug)]
pub struct Laid {
    pub width: f32,
    pub height: f32,
    pub text: String,
    pub run_starts: Vec<usize>,
    pub run_src: Vec<Src>,
    pub lines: Vec<Line>,
}

fn is_ws(c: char) -> bool {
    c.is_whitespace()
}

/// Shaped segment, cluster offsets relative to the segment start.
pub struct Shaped {
    glyphs: Vec<PGlyph>,
    clusters: Vec<Cl>,
    w: f32,
    trail_ws: f32,
}

#[derive(PartialEq, Eq, Hash)]
struct ShapeKey {
    font: u8,
    size: u32,
    smcp: bool,
    rtl: bool,
    text: String,
}

static SHAPE_CACHE: std::sync::OnceLock<std::sync::RwLock<std::collections::HashMap<ShapeKey, Arc<Shaped>>>> = std::sync::OnceLock::new();
pub static SHAPE_HITS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static SHAPE_MISSES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static SHAPE_CACHE_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

fn shape_cached(fonts: &Fonts, seg: &str, style: Style, rtl: bool) -> Arc<Shaped> {
    use std::sync::atomic::Ordering::Relaxed;
    let on = SHAPE_CACHE_ON.load(Relaxed);
    let key = ShapeKey { font: style.font as u8, size: style.size.to_bits(), smcp: style.smcp, rtl, text: seg.to_string() };
    let cache = SHAPE_CACHE.get_or_init(Default::default);
    if on {
        if let Some(v) = cache.read().unwrap().get(&key) {
            SHAPE_HITS.fetch_add(1, Relaxed);
            return v.clone();
        }
    }
    SHAPE_MISSES.fetch_add(1, Relaxed);
    let v = Arc::new(shape_seg(fonts, seg, style, rtl));
    if on {
        let mut c = cache.write().unwrap();
        if c.len() > 400_000 {
            c.clear();
        }
        c.insert(key, v.clone());
    }
    v
}

fn shape_seg(fonts: &Fonts, seg: &str, style: Style, rtl: bool) -> Shaped {
    let face = fonts.face(style.font);
    let scale = style.size / face.upem;
    let fr = face.font_ref();
    let shaper = face.shaper.shaper(&fr).build();
    let mut buf = UnicodeBuffer::new();
    buf.push_str(seg);
    buf.guess_segment_properties();
    buf.set_direction(if rtl { Direction::RightToLeft } else { Direction::LeftToRight });
    let feats = [Feature::new(Tag::new(b"smcp"), 1, ..)];
    let opts = harfrust::ShapeOptions::new().features(if style.smcp { &feats[..] } else { &[] });
    let gb = shaper.shape(buf, opts);
    let mut glyphs = Vec::with_capacity(gb.len());
    let mut clusters: Vec<Cl> = Vec::new();
    let mut pen = 0.0f32;
    for (info, pos) in gb.glyph_infos().iter().zip(gb.glyph_positions()) {
        let c = info.cluster as usize;
        glyphs.push(PGlyph { id: info.glyph_id, x: pen + pos.x_offset as f32 * scale, y: -(pos.y_offset as f32) * scale });
        let adv = pos.x_advance as f32 * scale;
        match clusters.last_mut() {
            Some(cl) if cl.start == c => cl.x1 = pen + adv,
            _ => clusters.push(Cl { x0: pen, x1: pen + adv, start: c, end: c }),
        }
        pen += adv;
    }
    let mut starts: Vec<usize> = clusters.iter().map(|c| c.start).collect();
    starts.sort();
    starts.dedup();
    for cl in clusters.iter_mut() {
        let i = starts.partition_point(|s| *s <= cl.start);
        cl.end = starts.get(i).copied().unwrap_or(seg.len());
    }
    let mut trail_ws = 0.0;
    let mut logical: Vec<&Cl> = clusters.iter().collect();
    logical.sort_by_key(|c| std::cmp::Reverse(c.start));
    for cl in logical {
        if seg[cl.start..cl.end].chars().all(is_ws) {
            trail_ws += cl.x1 - cl.x0;
        } else {
            break;
        }
    }
    Shaped { glyphs, clusters, w: pen, trail_ws }
}

pub fn layout(fonts: &Fonts, para: &Para, width: f32) -> Laid {
    let mut text = String::new();
    let mut run_starts = Vec::new();
    for r in &para.runs {
        run_starts.push(text.len());
        text.push_str(&r.text);
    }
    let run_src: Vec<Src> = para.runs.iter().map(|r| r.src).collect();
    let n = text.len();
    let base = if para.rtl { Level::rtl() } else { Level::ltr() };
    let bidi = BidiInfo::new(&text, Some(base));
    let levels = &bidi.levels;

    // segment boundaries
    let mut bounds: Vec<usize> = vec![0, n];
    bounds.extend(run_starts.iter().copied());
    let mut breaks = std::collections::HashSet::new();
    for (i, _opp) in unicode_linebreak::linebreaks(&text) {
        if i < n {
            bounds.push(i);
            breaks.insert(i);
        }
    }
    for i in 1..n {
        if text.is_char_boundary(i) && levels[i] != levels[i - 1] {
            bounds.push(i);
        }
    }
    bounds.sort();
    bounds.dedup();
    let run_of = |p: usize| run_starts.partition_point(|s| *s <= p).saturating_sub(1);

    let mut items: Vec<Item> = Vec::new();
    for w in bounds.windows(2) {
        let (a, b) = (w[0], w[1]);
        if a >= b {
            continue;
        }
        let run = run_of(a);
        let style = para.runs[run].style;
        let rtl = levels[a].is_rtl();
        let sh = shape_cached(fonts, &text[a..b], style, rtl);
        let glyphs = sh.glyphs.clone();
        let clusters: Vec<Cl> = sh.clusters.iter().map(|c| Cl { x0: c.x0, x1: c.x1, start: c.start + a, end: c.end + a }).collect();
        let (pen, trail_ws) = (sh.w, sh.trail_ws);
        items.push(Item {
            x: 0.0,
            w: pen,
            run,
            rtl,
            style,
            glyphs,
            clusters,
            start: a,
            end: b,
            trail_ws,
            breakable: breaks.contains(&b) || b == n,
        });
    }

    // greedy line breaking over breakable chunks
    let mut lines_items: Vec<Vec<Item>> = Vec::new();
    let mut cur: Vec<Item> = Vec::new();
    let mut cur_w = 0.0;
    let mut chunk: Vec<Item> = Vec::new();
    let mut chunk_w = 0.0;
    for it in items {
        chunk_w += it.w;
        let br = it.breakable;
        let trail = it.trail_ws;
        chunk.push(it);
        if br {
            let avail = width - if lines_items.is_empty() { para.indent } else { 0.0 };
            if cur.is_empty() || cur_w + chunk_w - trail <= avail {
                cur.append(&mut chunk);
                cur_w += chunk_w;
            } else {
                lines_items.push(std::mem::take(&mut cur));
                cur.append(&mut chunk);
                cur_w = chunk_w;
            }
            chunk_w = 0.0;
        }
    }
    cur.append(&mut chunk);
    if !cur.is_empty() || lines_items.is_empty() {
        lines_items.push(cur);
    }

    let mut lines = Vec::new();
    let mut y = para.space_before;
    let base_size = para.runs.first().map(|r| r.style.size).unwrap_or(16.0);
    for (li, mut its) in lines_items.into_iter().enumerate() {
        let start = its.iter().map(|i| i.start).min().unwrap_or(n);
        let end = its.iter().map(|i| i.end).max().unwrap_or(n);
        // UAX #9 L2: reverse runs from the highest level down to the lowest odd level
        let lv: Vec<u8> = its.iter().map(|i| levels[i.start].number()).collect();
        let mut idx: Vec<usize> = (0..its.len()).collect();
        if let (Some(&max), Some(&min)) = (lv.iter().max(), lv.iter().min()) {
            let lowest_odd = if min % 2 == 1 { min } else { min + 1 };
            let mut l = max;
            while l >= lowest_odd && l > 0 {
                let mut i = 0;
                while i < idx.len() {
                    if lv[idx[i]] >= l {
                        let s = i;
                        while i < idx.len() && lv[idx[i]] >= l {
                            i += 1;
                        }
                        idx[s..i].reverse();
                    } else {
                        i += 1;
                    }
                }
                l -= 1;
            }
        }
        let mut ordered: Vec<Item> = Vec::with_capacity(its.len());
        let mut slots: Vec<Option<Item>> = its.drain(..).map(Some).collect();
        for i in idx {
            ordered.push(slots[i].take().unwrap());
        }
        // metrics: tallest font on the line, not the first
        let (mut asc, mut desc) = (0.0f32, 0.0f32);
        for it in &ordered {
            let f = fonts.face(it.style.font);
            let s = it.style.size / f.upem;
            asc = asc.max(f.ascent * s + it.style.raise);
            desc = desc.max(f.descent * s - it.style.raise);
        }
        if ordered.is_empty() {
            let f = fonts.face(FontId::Regular);
            asc = f.ascent * base_size / f.upem;
            desc = f.descent * base_size / f.upem;
        }
        let natural = asc + desc;
        let lh = natural * para.line_spacing;
        let lw: f32 = ordered.iter().map(|i| i.w).sum();
        let indent = if li == 0 { para.indent } else { 0.0 };
        let mut x = if para.center {
            ((width - lw) / 2.0).max(0.0)
        } else if para.rtl {
            width - lw - indent
        } else {
            indent
        };
        for it in ordered.iter_mut() {
            it.x = x;
            x += it.w;
        }
        let top = y;
        let baseline = (top + (lh - natural) / 2.0 + asc).round();
        y += lh;
        lines.push(Line { top, baseline, bottom: y, items: ordered, start, end });
    }
    Laid { width, height: y + para.space_after, text, run_starts, run_src, lines }
}

impl Laid {
    pub fn run_of(&self, p: usize) -> usize {
        self.run_starts.partition_point(|s| *s <= p).saturating_sub(1)
    }

    /// P byte -> (unit byte, object id if synthetic)
    pub fn to_unit(&self, p: usize) -> (usize, Option<u32>) {
        if self.run_src.is_empty() {
            return (0, None);
        }
        let r = self.run_of(p);
        match self.run_src[r] {
            Src::Text(s) => (s + (p - self.run_starts[r]), None),
            Src::Object { anchor, id } => (anchor, Some(id)),
        }
    }

    /// unit byte -> P byte (first text run containing it)
    pub fn from_unit(&self, ub: usize) -> usize {
        let mut best = self.text.len();
        for (i, src) in self.run_src.iter().enumerate() {
            if let Src::Text(s) = *src {
                let len = self.run_starts.get(i + 1).copied().unwrap_or(self.text.len()) - self.run_starts[i];
                if ub >= s && ub <= s + len {
                    return self.run_starts[i] + (ub - s);
                }
                if s > ub {
                    best = best.min(self.run_starts[i]);
                }
            }
        }
        best
    }

    fn line_at_y(&self, y: f32) -> usize {
        let i = self.lines.partition_point(|l| l.bottom <= y);
        i.min(self.lines.len().saturating_sub(1))
    }

    pub fn line_index_at(&self, y: f32) -> usize {
        self.line_at_y(y)
    }

    /// Cluster exactly under the point.
    pub fn hit(&self, x: f32, y: f32) -> Option<Cl> {
        if y < 0.0 || y > self.height {
            return None;
        }
        let line = self.lines.get(self.line_at_y(y))?;
        if y < line.top || y > line.bottom {
            return None;
        }
        for it in &line.items {
            if x >= it.x && x < it.x + it.w {
                for cl in &it.clusters {
                    if x >= it.x + cl.x0 && x < it.x + cl.x1 {
                        return Some(*cl);
                    }
                }
            }
        }
        None
    }

    /// Logical caret position (P byte) nearest the point.
    pub fn caret_at(&self, x: f32, y: f32) -> usize {
        let Some(line) = self.lines.get(self.line_at_y(y)) else { return 0 };
        if line.items.is_empty() {
            return line.start;
        }
        let first = &line.items[0];
        let last = line.items.last().unwrap();
        // outside the line: visual edges
        let edge = |it: &Item, left: bool| if left ^ it.rtl { it.start } else { it.end };
        if x < first.x {
            return edge(first, true);
        }
        if x >= last.x + last.w {
            return edge(last, false);
        }
        for it in &line.items {
            if x >= it.x && x < it.x + it.w {
                for cl in &it.clusters {
                    let (a, b) = (it.x + cl.x0, it.x + cl.x1);
                    if x >= a && x < b {
                        let left_half = x < (a + b) / 2.0;
                        return if left_half ^ it.rtl { cl.start } else { cl.end };
                    }
                }
                return it.start;
            }
        }
        line.end
    }

    /// Visual rects (x0, x1, top, bottom) covering logical range [p0, p1).
    pub fn range_rects(&self, p0: usize, p1: usize) -> Vec<[f32; 4]> {
        let mut out = Vec::new();
        if p0 >= p1 {
            return out;
        }
        for line in &self.lines {
            if line.end <= p0 || line.start >= p1 {
                continue;
            }
            let mut cur: Option<[f32; 4]> = None;
            for it in &line.items {
                for cl in &it.clusters {
                    if cl.start < p1 && cl.end > p0 {
                        let (a, b) = (it.x + cl.x0, it.x + cl.x1);
                        match &mut cur {
                            Some(r) if (r[1] - a).abs() < 0.5 => r[1] = b,
                            _ => {
                                if let Some(r) = cur.take() {
                                    out.push(r);
                                }
                                cur = Some([a, b, line.top, line.bottom]);
                            }
                        }
                    }
                }
            }
            if let Some(r) = cur {
                out.push(r);
            }
        }
        out
    }
}

/// Raster a laid block with vello_cpu. Returns premultiplied RGBA8 and pixel size.
pub fn raster(fonts: &Fonts, laid: &Laid, ppp: f32) -> (Vec<u8>, [usize; 2]) {
    raster_range(fonts, laid, ppp, 0.0, laid.height)
}

/// Raster the horizontal band y0..y1 (points) of a laid block.
pub fn raster_range(fonts: &Fonts, laid: &Laid, ppp: f32, y0: f32, y1: f32) -> (Vec<u8>, [usize; 2]) {
    let w = ((laid.width * ppp).ceil() as usize).clamp(1, 8192);
    let h = (((y1 - y0) * ppp).ceil() as usize).clamp(1, 8192);
    let mut ctx = vello_cpu::RenderContext::new(w as u16, h as u16);
    let mut res = vello_cpu::Resources::new();
    for line in &laid.lines {
        if line.bottom < y0 - 4.0 || line.top > y1 + 4.0 {
            continue;
        }
        for it in &line.items {
            if it.glyphs.is_empty() {
                continue;
            }
            let face = fonts.face(it.style.font);
            let [r, g, b, a] = it.style.color;
            ctx.set_paint(peniko::Color::from_rgba8(r, g, b, a));
            let by = line.baseline - it.style.raise;
            ctx.glyph_run(&mut res, &face.peniko)
                .font_size(it.style.size * ppp)
                .hint(false)
                .fill_glyphs(it.glyphs.iter().map(|g| vello_cpu::Glyph {
                    id: g.id,
                    x: (it.x + g.x) * ppp,
                    y: (by + g.y - y0) * ppp,
                }));
        }
    }
    let mut pix = vello_cpu::Pixmap::new(w as u16, h as u16);
    ctx.render(&mut pix, &mut res);
    (pix.data_as_u8_slice().to_vec(), [w, h])
}

#[allow(dead_code)]
fn _assert_send_sync() {
    fn f<T: Send + Sync>() {}
    f::<Fonts>();
    f::<Laid>();
}
