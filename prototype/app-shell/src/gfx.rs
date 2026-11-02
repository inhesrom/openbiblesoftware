//! PROTOTYPE (#19): two ways to put laid-out Resource text on screen, for measurement.
//!
//! * `Mode::Tiles`: ADR 0004 as written, hardened: each block is cut into tiles no taller than
//!   `TILE_MAX_PX` device pixels, rastered by vello_cpu on a worker pool, uploaded under a
//!   per-frame budget. Missing tiles show placeholders.
//! * `Mode::Atlas`: ADR 0004's fallback: glyphs rastered once into a shared atlas texture,
//!   each line emitted as textured quads in an egui Mesh.

use crate::text::{Fonts, Laid};
use egui::{pos2, Color32, ColorImage, Mesh, Rect, TextureHandle, TextureOptions};
use skrifa::MetadataProvider;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use vello_cpu::kurbo;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Tiles,
    Atlas,
}

pub const TILE_MAX_PX: f32 = 1024.0;

// ------------------------------------------------------------------ tiles + worker pool

/// (doc uid, generation, block, tile)
pub type TileKey = (u64, u64, usize, usize);

struct Job {
    key: TileKey,
    laid: Arc<Laid>,
    y0: f32,
    y1: f32,
    ppp: f32,
}

pub struct Pool {
    tx: Sender<Job>,
    pub done: Arc<Mutex<HashMap<TileKey, (Vec<u8>, [usize; 2], u64)>>>,
    pub inflight: Arc<AtomicUsize>,
    pub workers: usize,
    pub frame: u64,
}

impl Pool {
    pub fn new(fonts: Arc<Fonts>) -> Pool {
        let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).saturating_sub(1).clamp(1, 4);
        let (tx, rx) = channel::<Job>();
        let rx = Arc::new(Mutex::new(rx));
        let done = Arc::new(Mutex::new(HashMap::new()));
        let inflight = Arc::new(AtomicUsize::new(0));
        for _ in 0..workers {
            let rx: Arc<Mutex<Receiver<Job>>> = rx.clone();
            let done = done.clone();
            let inflight = inflight.clone();
            let fonts = fonts.clone();
            std::thread::spawn(move || {
                loop {
                    let job = { rx.lock().unwrap().recv() };
                    let Ok(job) = job else { break };
                    let (px, size) = crate::text::raster_range(&fonts, &job.laid, job.ppp, job.y0, job.y1);
                    done.lock().unwrap().insert(job.key, (px, size, 0));
                    inflight.fetch_sub(1, Ordering::Relaxed);
                }
            });
        }
        Pool { tx, done, inflight, workers, frame: 0 }
    }

    pub fn submit(&self, key: TileKey, laid: Arc<Laid>, y0: f32, y1: f32, ppp: f32) {
        self.inflight.fetch_add(1, Ordering::Relaxed);
        let _ = self.tx.send(Job { key, laid, y0, y1, ppp });
    }

    pub fn take(&self, key: &TileKey) -> Option<(Vec<u8>, [usize; 2])> {
        self.done.lock().unwrap().remove(key).map(|(p, s, _)| (p, s))
    }

    /// Drop results nobody collected (evicted blocks, stale generations).
    pub fn gc(&mut self) {
        self.frame += 1;
        let f = self.frame;
        let mut d = self.done.lock().unwrap();
        for v in d.values_mut() {
            if v.2 == 0 {
                v.2 = f;
            }
        }
        d.retain(|_, v| f - v.2 < 240);
    }
}

/// Split a laid block into tile y-ranges (points) no taller than TILE_MAX_PX device px.
pub fn tile_ranges(laid: &Laid, ppp: f32) -> Vec<(f32, f32)> {
    let max = TILE_MAX_PX / ppp;
    let mut out = Vec::new();
    let mut y0 = 0.0;
    let mut last = 0.0;
    for l in &laid.lines {
        if l.bottom - y0 > max && last > y0 {
            out.push((y0, last));
            y0 = last;
        }
        last = l.bottom;
    }
    out.push((y0, laid.height.max(last)));
    out
}

// ------------------------------------------------------------------ glyph atlas

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct GKey {
    font: u8,
    gid: u32,
    px_q: u32,
    bin: u8,
}

#[derive(Clone, Copy, Debug)]
struct GEntry {
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    ox: f32,
    oy: f32,
}

pub struct Atlas {
    size: usize,
    alpha: Vec<u8>,
    cx: usize,
    cy: usize,
    row_h: usize,
    map: HashMap<GKey, Option<GEntry>>,
    tex: Option<TextureHandle>,
    dirty: Option<[usize; 4]>,
    pub generation: u64,
    pub glyphs: usize,
    pub raster_ms: f32,
    pub rastered_this_frame: u32,
}

const BINS: f32 = 4.0;

struct Pen<'a> {
    path: &'a mut kurbo::BezPath,
    dx: f64,
}

impl skrifa::outline::OutlinePen for Pen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.path.move_to((x as f64 + self.dx, -y as f64));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to((x as f64 + self.dx, -y as f64));
    }
    fn quad_to(&mut self, a: f32, b: f32, x: f32, y: f32) {
        self.path.quad_to((a as f64 + self.dx, -b as f64), (x as f64 + self.dx, -y as f64));
    }
    fn curve_to(&mut self, a: f32, b: f32, c: f32, d: f32, x: f32, y: f32) {
        self.path.curve_to((a as f64 + self.dx, -b as f64), (c as f64 + self.dx, -d as f64), (x as f64 + self.dx, -y as f64));
    }
    fn close(&mut self) {
        self.path.close_path();
    }
}

fn raster_glyph(fonts: &Fonts, k: GKey) -> Option<(Vec<u8>, usize, usize, f32, f32)> {
    use kurbo::Shape;
    let face = &fonts.faces[k.font as usize];
    let fr = face.font_ref();
    let outlines = fr.outline_glyphs();
    let g = outlines.get(skrifa::GlyphId::new(k.gid))?;
    let mut path = kurbo::BezPath::new();
    let mut pen = Pen { path: &mut path, dx: k.bin as f64 / BINS as f64 };
    let size = k.px_q as f32 / 4.0;
    g.draw(skrifa::outline::DrawSettings::unhinted(skrifa::instance::Size::new(size), skrifa::instance::LocationRef::default()), &mut pen).ok()?;
    let b = path.bounding_box();
    if b.width() <= 0.0 || b.height() <= 0.0 {
        return None;
    }
    let (x0, y0) = (b.x0.floor(), b.y0.floor());
    let w = (b.x1.ceil() - x0) as usize + 1;
    let h = (b.y1.ceil() - y0) as usize + 1;
    let mut ctx = vello_cpu::RenderContext::new(w as u16, h as u16);
    ctx.set_transform(kurbo::Affine::translate((-x0, -y0)));
    ctx.set_paint(vello_cpu::color::OpaqueColor::<vello_cpu::color::Srgb>::WHITE);
    ctx.fill_path(&path);
    let mut pix = vello_cpu::Pixmap::new(w as u16, h as u16);
    let mut res = vello_cpu::Resources::new();
    ctx.render(&mut pix, &mut res);
    let a = pix.data_as_u8_slice().chunks_exact(4).map(|p| p[3]).collect();
    Some((a, w, h, x0 as f32, y0 as f32))
}

impl Atlas {
    pub fn new() -> Atlas {
        let size = 1024;
        Atlas {
            size,
            alpha: vec![0; size * size],
            cx: 1,
            cy: 1,
            row_h: 0,
            map: HashMap::new(),
            tex: None,
            dirty: None,
            generation: 0,
            glyphs: 0,
            raster_ms: 0.0,
            rastered_this_frame: 0,
        }
    }

    pub fn bytes(&self) -> usize {
        self.size * self.size * 4
    }

    fn grow(&mut self) {
        let ns = self.size * 2;
        let mut a = vec![0; ns * ns];
        for y in 0..self.size {
            a[y * ns..y * ns + self.size].copy_from_slice(&self.alpha[y * self.size..(y + 1) * self.size]);
        }
        self.alpha = a;
        // continue packing below the old content (wastes the top-right quadrant; prototype)
        self.cx = 1;
        self.cy = self.size + 1;
        self.row_h = 0;
        self.size = ns;
        self.tex = None;
        self.dirty = None;
        self.generation += 1;
    }

    fn alloc(&mut self, w: usize, h: usize) -> (usize, usize) {
        loop {
            if self.cx + w + 1 > self.size {
                self.cx = 1;
                self.cy += self.row_h + 1;
                self.row_h = 0;
            }
            if self.cy + h + 1 > self.size {
                if self.size >= 8192 {
                    // full: reset (prototype)
                    self.map.clear();
                    self.alpha.iter_mut().for_each(|a| *a = 0);
                    self.cx = 1;
                    self.cy = 1;
                    self.row_h = 0;
                    self.tex = None;
                    self.generation += 1;
                } else {
                    self.grow();
                }
                continue;
            }
            let p = (self.cx, self.cy);
            self.cx += w + 1;
            self.row_h = self.row_h.max(h);
            return p;
        }
    }

    fn get(&mut self, fonts: &Fonts, k: GKey) -> Option<GEntry> {
        if let Some(e) = self.map.get(&k) {
            return *e;
        }
        let t = Instant::now();
        let r = raster_glyph(fonts, k);
        let e = r.map(|(a, w, h, ox, oy)| {
            let (x, y) = self.alloc(w, h);
            for row in 0..h {
                self.alpha[(y + row) * self.size + x..(y + row) * self.size + x + w].copy_from_slice(&a[row * w..(row + 1) * w]);
            }
            let d = self.dirty.get_or_insert([x, y, x + w, y + h]);
            *d = [d[0].min(x), d[1].min(y), d[2].max(x + w), d[3].max(y + h)];
            GEntry { x: x as u16, y: y as u16, w: w as u16, h: h as u16, ox, oy }
        });
        self.raster_ms += t.elapsed().as_secs_f32() * 1000.0;
        self.rastered_this_frame += 1;
        self.glyphs += 1;
        self.map.insert(k, e);
        e
    }

    pub fn texture_id(&mut self, ctx: &egui::Context) -> egui::TextureId {
        let to_img = |a: &[u8], w: usize, h: usize| {
            ColorImage::new([w, h], a.iter().map(|&v| Color32::from_rgba_premultiplied(v, v, v, v)).collect())
        };
        if self.tex.is_none() {
            let img = to_img(&self.alpha, self.size, self.size);
            self.tex = Some(ctx.load_texture("glyph-atlas", img, TextureOptions::NEAREST));
            self.dirty = None;
        }
        if let Some([x0, y0, x1, y1]) = self.dirty.take() {
            let (w, h) = (x1 - x0, y1 - y0);
            let mut sub = vec![0u8; w * h];
            for r in 0..h {
                sub[r * w..(r + 1) * w].copy_from_slice(&self.alpha[(y0 + r) * self.size + x0..(y0 + r) * self.size + x1]);
            }
            self.tex.as_mut().unwrap().set_partial([x0, y0], to_img(&sub, w, h), TextureOptions::NEAREST);
        }
        self.tex.as_ref().unwrap().id()
    }

    /// Build a mesh for a laid block, positions in points relative to a device-pixel-aligned origin.
    pub fn mesh(&mut self, ctx: &egui::Context, fonts: &Fonts, laid: &Laid, ppp: f32) -> Mesh {
        let start_gen = self.generation;
        let mut entries: Vec<(GEntry, f32, f32, Color32)> = Vec::new();
        for line in &laid.lines {
            for it in &line.items {
                let face = it.style.font as u8;
                let px = it.style.size * ppp;
                let px_q = (px * 4.0).round() as u32;
                let [r, g, b, a] = it.style.color;
                let col = Color32::from_rgba_unmultiplied(r, g, b, a);
                let by = ((line.baseline - it.style.raise) * ppp).round();
                for gl in &it.glyphs {
                    let x = (it.x + gl.x) * ppp;
                    let xi = x.floor();
                    let bin = ((x - xi) * BINS).floor().min(BINS - 1.0) as u8;
                    let y = (by + gl.y * ppp).round();
                    if let Some(e) = self.get(fonts, GKey { font: face, gid: gl.id, px_q, bin }) {
                        entries.push((e, xi, y, col));
                    }
                }
            }
        }
        if self.generation != start_gen {
            // atlas grew or reset mid-build: rebuild against the new one
            return self.mesh(ctx, fonts, laid, ppp);
        }
        let mut m = Mesh::with_texture(self.texture_id(ctx));
        let s = self.size as f32;
        m.reserve_vertices(entries.len() * 4);
        m.reserve_triangles(entries.len() * 2);
        for (e, xi, y, col) in entries {
            let x0 = (xi + e.ox) / ppp;
            let y0 = (y + e.oy) / ppp;
            let r = Rect::from_min_max(pos2(x0, y0), pos2(x0 + e.w as f32 / ppp, y0 + e.h as f32 / ppp));
            let uv = Rect::from_min_max(pos2(e.x as f32 / s, e.y as f32 / s), pos2((e.x + e.w) as f32 / s, (e.y + e.h) as f32 / s));
            m.add_rect_with_uv(r, uv, col);
        }
        m
    }
}

// ------------------------------------------------------------------ layout workers

pub type LayoutKey = (u64, u64, usize);

pub struct LayoutPool {
    tx: Sender<(LayoutKey, crate::text::Para, f32)>,
    done: Arc<Mutex<HashMap<LayoutKey, crate::text::Laid>>>,
}

impl LayoutPool {
    pub fn new(fonts: Arc<Fonts>, workers: usize) -> LayoutPool {
        let (tx, rx) = channel::<(LayoutKey, crate::text::Para, f32)>();
        let rx = Arc::new(Mutex::new(rx));
        let done = Arc::new(Mutex::new(HashMap::new()));
        for _ in 0..workers {
            let rx = rx.clone();
            let done = done.clone();
            let fonts = fonts.clone();
            std::thread::spawn(move || {
                loop {
                    let job = { rx.lock().unwrap().recv() };
                    let Ok((key, para, w)) = job else { break };
                    let laid = crate::text::layout(&fonts, &para, w);
                    done.lock().unwrap().insert(key, laid);
                }
            });
        }
        LayoutPool { tx, done }
    }

    pub fn submit(&self, key: LayoutKey, para: crate::text::Para, w: f32) {
        let _ = self.tx.send((key, para, w));
    }

    pub fn take(&self, key: &LayoutKey) -> Option<crate::text::Laid> {
        self.done.lock().unwrap().remove(key)
    }
}
