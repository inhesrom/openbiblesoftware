//! A small anti-aliased coverage rasterizer for glyph outlines.
//!
//! Signed-area accumulation (the technique popularised by font-rs): every line segment adds
//! the area it covers to a row-major accumulation buffer, and a running sum over the buffer
//! gives each pixel's coverage. Curves are flattened to lines first. This is all glyph
//! rasterization needs (non-zero fill of closed outlines), with no extra dependency.

/// Flattening tolerance in px.
const TOLERANCE: f32 = 0.1;

/// Coverage accumulator for one bitmap. Coordinates are px with y down.
pub struct Rasterizer {
    width: usize,
    height: usize,
    acc: Vec<f32>,
    start: (f32, f32),
    pen: (f32, f32),
}

impl Rasterizer {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            acc: vec![0.0; width * height + 4],
            start: (0.0, 0.0),
            pen: (0.0, 0.0),
        }
    }

    pub fn move_to(&mut self, x: f32, y: f32) {
        self.close();
        self.start = (x, y);
        self.pen = (x, y);
    }

    pub fn line_to(&mut self, x: f32, y: f32) {
        self.line(self.pen, (x, y));
        self.pen = (x, y);
    }

    pub fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let p0 = self.pen;
        let dd = ((p0.0 - 2.0 * cx + x).powi(2) + (p0.1 - 2.0 * cy + y).powi(2)).sqrt();
        let n = (1.0 + (dd / (4.0 * TOLERANCE)).sqrt().floor()).clamp(1.0, 64.0) as usize;
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let mt = 1.0 - t;
            let px = mt * mt * p0.0 + 2.0 * mt * t * cx + t * t * x;
            let py = mt * mt * p0.1 + 2.0 * mt * t * cy + t * t * y;
            self.line_to(px, py);
        }
    }

    pub fn cubic_to(&mut self, c0: (f32, f32), c1: (f32, f32), x: f32, y: f32) {
        let p0 = self.pen;
        let d0 = ((p0.0 - 2.0 * c0.0 + c1.0).powi(2) + (p0.1 - 2.0 * c0.1 + c1.1).powi(2)).sqrt();
        let d1 = ((c0.0 - 2.0 * c1.0 + x).powi(2) + (c0.1 - 2.0 * c1.1 + y).powi(2)).sqrt();
        let dd = d0.max(d1);
        let n = (1.0 + (dd * 0.75 / TOLERANCE).sqrt().floor()).clamp(1.0, 96.0) as usize;
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let mt = 1.0 - t;
            let (a, b, c, d) = (mt * mt * mt, 3.0 * mt * mt * t, 3.0 * mt * t * t, t * t * t);
            self.line_to(
                a * p0.0 + b * c0.0 + c * c1.0 + d * x,
                a * p0.1 + b * c0.1 + c * c1.1 + d * y,
            );
        }
    }

    /// Close the current contour back to its start.
    pub fn close(&mut self) {
        if self.pen != self.start {
            self.line(self.pen, self.start);
            self.pen = self.start;
        }
    }

    fn line(&mut self, p0: (f32, f32), p1: (f32, f32)) {
        if (p0.1 - p1.1).abs() < f32::EPSILON {
            return;
        }
        let (dir, p0, p1) = if p0.1 < p1.1 {
            (1.0f32, p0, p1)
        } else {
            (-1.0f32, p1, p0)
        };
        let dxdy = (p1.0 - p0.0) / (p1.1 - p0.1);
        let mut x = p0.0;
        let y_start = p0.1.max(0.0);
        if p0.1 < 0.0 {
            x -= p0.1 * dxdy;
        }
        let y_end = (p1.1.ceil() as usize).min(self.height);
        let w = self.width as i64;
        let len = self.acc.len() as i64;
        let add = |acc: &mut [f32], i: i64, v: f32| {
            if (0..len).contains(&i) {
                acc[i as usize] += v;
            }
        };
        for y in (y_start as usize)..y_end {
            let row = y as i64 * w;
            let yf = y as f32;
            let dy = (yf + 1.0).min(p1.1) - yf.max(p0.1);
            let x_next = x + dxdy * dy;
            let d = dy * dir;
            let (x0, x1) = if x < x_next { (x, x_next) } else { (x_next, x) };
            let x0 = x0.max(0.0);
            let x1 = x1.max(0.0);
            let x0_floor = x0.floor();
            let x0i = x0_floor as i64;
            let x1_ceil = x1.ceil();
            let x1i = x1_ceil as i64;
            if x1i <= x0i + 1 {
                let xmf = 0.5 * (x + x_next) - x0_floor;
                add(&mut self.acc, row + x0i, d - d * xmf);
                add(&mut self.acc, row + x0i + 1, d * xmf);
            } else {
                let s = (x1 - x0).recip();
                let x0f = x0 - x0_floor;
                let a0 = 0.5 * s * (1.0 - x0f) * (1.0 - x0f);
                let x1f = x1 - x1_ceil + 1.0;
                let am = 0.5 * s * x1f * x1f;
                add(&mut self.acc, row + x0i, d * a0);
                if x1i == x0i + 2 {
                    add(&mut self.acc, row + x0i + 1, d * (1.0 - a0 - am));
                } else {
                    let a1 = s * (1.5 - x0f);
                    add(&mut self.acc, row + x0i + 1, d * (a1 - a0));
                    for xi in x0i + 2..x1i - 1 {
                        add(&mut self.acc, row + xi, d * s);
                    }
                    let a2 = a1 + (x1i - x0i - 3) as f32 * s;
                    add(&mut self.acc, row + x1i - 1, d * (1.0 - a2 - am));
                }
                add(&mut self.acc, row + x1i, d * am);
            }
            x = x_next;
        }
    }

    /// Coverage per pixel, 0..=255, row-major.
    pub fn finish(mut self) -> Vec<u8> {
        self.close();
        let mut sum = 0.0f32;
        self.acc
            .iter()
            .take(self.width * self.height)
            .map(|a| {
                sum += a;
                (sum.abs().min(1.0) * 255.0 + 0.5) as u8
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(r: &mut Rasterizer, x0: f32, y0: f32, x1: f32, y1: f32) {
        r.move_to(x0, y0);
        r.line_to(x1, y0);
        r.line_to(x1, y1);
        r.line_to(x0, y1);
        r.close();
    }

    #[test]
    fn pixel_aligned_square_is_solid() {
        let mut r = Rasterizer::new(8, 8);
        square(&mut r, 2.0, 2.0, 6.0, 6.0);
        let a = r.finish();
        for y in 0..8 {
            for x in 0..8 {
                let inside = (2..6).contains(&x) && (2..6).contains(&y);
                assert_eq!(a[y * 8 + x], if inside { 255 } else { 0 }, "({x},{y})");
            }
        }
    }

    #[test]
    fn half_pixel_edges_are_half_covered_and_winding_is_ignored() {
        let mut r = Rasterizer::new(6, 4);
        // Counter-clockwise this time.
        r.move_to(1.5, 1.0);
        r.line_to(1.5, 3.0);
        r.line_to(4.5, 3.0);
        r.line_to(4.5, 1.0);
        r.close();
        let a = r.finish();
        let row: Vec<u8> = a[6..12].to_vec();
        assert_eq!(row, [0, 128, 255, 255, 128, 0]);
    }

    #[test]
    fn circle_area_is_right() {
        let mut r = Rasterizer::new(40, 40);
        let (cx, cy, rad) = (20.0f32, 20.0f32, 15.0f32);
        let k = 0.552_284_8 * rad;
        r.move_to(cx + rad, cy);
        r.cubic_to((cx + rad, cy + k), (cx + k, cy + rad), cx, cy + rad);
        r.cubic_to((cx - k, cy + rad), (cx - rad, cy + k), cx - rad, cy);
        r.cubic_to((cx - rad, cy - k), (cx - k, cy - rad), cx, cy - rad);
        r.cubic_to((cx + k, cy - rad), (cx + rad, cy - k), cx + rad, cy);
        let area: f32 = r.finish().iter().map(|&v| f32::from(v) / 255.0).sum();
        let expected = std::f32::consts::PI * rad * rad;
        assert!(
            (area - expected).abs() / expected < 0.01,
            "{area} vs {expected}"
        );
    }
}
