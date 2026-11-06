//! `cargo xtask atlas-basemap`: regenerate the Atlas's built-in base map (#150),
//! `crates/engine/assets/atlas/basemap.bin`, from Natural Earth (public domain).
//!
//! Inputs, pinned by sha256 and cached with the Starter downloads: Natural Earth's 1:10m land,
//! lakes and rivers (`ne_10m_land`, `ne_10m_lakes`, `ne_10m_rivers_lake_centerlines`, GeoJSON
//! from `nvkelso/natural-earth-vector` at a fixed commit).
//!
//! Kept: the Bible lands and their neighbours ([`BOUNDS`]: Spain to the Indus, Ethiopia to the
//! Black Sea). Land and natural lakes (reservoirs are modern) become triangle meshes, cut to the
//! bounds and triangulated here so the app only has to draw triangles; their shores and the
//! larger rivers (scale rank up to [`MAX_RIVER_RANK`]) become lines. Every outline is
//! simplified (Douglas-Peucker, [`TOLERANCE`]) and rounded to 1/[`UNITS_PER_DEGREE`] of a
//! degree. The file layout is documented in `obs_engine::base_map`.
//!
//! The output is deterministic: the same inputs give the same bytes.

use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::starter::{Config, PinnedFile, fetch_file};

const OUT: &str = "crates/engine/assets/atlas/basemap.bin";

const COMMIT: &str = "ca96624a56bd078437bca8184e78163e5039ad19";
const LAND: (&str, &str) = (
    "ne_10m_land.geojson",
    "1ac90796408bc6ad6911d69448485d3c4dbf2190370080368a09976e1c9f7416",
);
const LAKES: (&str, &str) = (
    "ne_10m_lakes.geojson",
    "2d036f53dedec578001c5c30c2959ee7d4eebc1306900fa4367c49929ec8f2d9",
);
const RIVERS: (&str, &str) = (
    "ne_10m_rivers_lake_centerlines.geojson",
    "bb854a900ecbd3b408df46d5e16e3e0f974ba55993f9d8b5c26e855273c0905a",
);

/// West, south, east, north, in degrees.
const BOUNDS: [f64; 4] = [-12.0, 6.0, 72.0, 50.0];
/// Coordinates are stored as whole 1/200ths of a degree (about 550 m).
const UNITS_PER_DEGREE: i32 = 200;
/// Douglas-Peucker tolerance, in degrees.
const TOLERANCE: f64 = 0.012;
/// Natural Earth river scale ranks kept (the Jordan is 6, the Tiber 7).
const MAX_RIVER_RANK: u8 = 7;

/// Section kinds of the file (see `obs_engine::base_map`).
const LAND_MESH: u8 = 1;
const LAKE_MESH: u8 = 2;
const COAST_LINES: u8 = 3;
const SHORE_LINES: u8 = 4;
const RIVER_LINES: u8 = 5;

const USAGE: &str = "usage: cargo xtask atlas-basemap [--cache <dir>]

Regenerates crates/engine/assets/atlas/basemap.bin from Natural Earth (downloads are pinned by
sha256 and cached in target/starter-cache).";

type Pt = [f64; 2];

pub fn run(root: &Path, args: &[String]) -> Result<()> {
    let mut cache = Config::defaults(root).cache;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--cache" => {
                cache = it.next().context("--cache needs a directory")?.into();
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => bail!("unknown option `{other}`\n\n{USAGE}"),
        }
    }
    let read = |(file, sha256): (&str, &str)| -> Result<Value> {
        let pinned = PinnedFile {
            url: format!(
                "https://raw.githubusercontent.com/nvkelso/natural-earth-vector/{COMMIT}/geojson/{file}"
            ),
            sha256: sha256.to_string(),
        };
        let bytes = fetch_file("atlas-basemap", &pinned, &cache)?;
        serde_json::from_slice(&bytes).with_context(|| format!("{file} is not GeoJSON"))
    };
    let land = read(LAND)?;
    let lakes = read(LAKES)?;
    let rivers = read(RIVERS)?;
    let bytes = build(&land, &lakes, &rivers)?;
    let path = root.join(OUT);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, &bytes).with_context(|| format!("cannot write {}", path.display()))?;
    eprintln!("atlas-basemap: wrote {} bytes to {OUT}", bytes.len());
    Ok(())
}

/// The polygons (outer ring, then holes) of a Polygon or MultiPolygon feature.
fn polygons(feature: &Value) -> Vec<Vec<Vec<Pt>>> {
    let geometry = &feature["geometry"];
    let ring = |r: &Value| -> Vec<Pt> {
        r.as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| Some([c[0].as_f64()?, c[1].as_f64()?]))
            .collect()
    };
    let polygon =
        |p: &Value| -> Vec<Vec<Pt>> { p.as_array().into_iter().flatten().map(ring).collect() };
    match geometry["type"].as_str() {
        Some("Polygon") => vec![polygon(&geometry["coordinates"])],
        Some("MultiPolygon") => geometry["coordinates"]
            .as_array()
            .into_iter()
            .flatten()
            .map(polygon)
            .collect(),
        _ => Vec::new(),
    }
}

/// The lines of a LineString or MultiLineString feature.
fn lines(feature: &Value) -> Vec<Vec<Pt>> {
    let geometry = &feature["geometry"];
    let line = |l: &Value| -> Vec<Pt> {
        l.as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| Some([c[0].as_f64()?, c[1].as_f64()?]))
            .collect()
    };
    match geometry["type"].as_str() {
        Some("LineString") => vec![line(&geometry["coordinates"])],
        Some("MultiLineString") => geometry["coordinates"]
            .as_array()
            .into_iter()
            .flatten()
            .map(line)
            .collect(),
        _ => Vec::new(),
    }
}

fn features(collection: &Value) -> impl Iterator<Item = &Value> {
    collection["features"].as_array().into_iter().flatten()
}

/// The base map file from Natural Earth's land, lakes and rivers.
pub fn build(land: &Value, lakes: &Value, rivers: &Value) -> Result<Vec<u8>> {
    let land_polygons: Vec<Vec<Vec<Pt>>> = features(land).flat_map(polygons).collect();
    // Natural lakes only: reservoirs are modern.
    let lake_polygons: Vec<Vec<Vec<Pt>>> = features(lakes)
        .filter(|f| f["properties"]["featurecla"].as_str() != Some("Reservoir"))
        .flat_map(polygons)
        .collect();
    let mut river_lines: Vec<(u8, Vec<Pt>)> = Vec::new();
    for f in features(rivers) {
        let rank = f["properties"]["scalerank"].as_f64().unwrap_or(99.0);
        if rank > f64::from(MAX_RIVER_RANK) {
            continue;
        }
        // Scale ranks are small whole numbers.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let rank = rank as u8;
        for l in lines(f) {
            river_lines.push((rank, l));
        }
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"OBSMAP\x01\x00");
    put_i32(&mut out, UNITS_PER_DEGREE);
    for v in BOUNDS {
        put_i16(&mut out, units(v)?);
    }
    let sections: [(u8, Section); 5] = [
        (LAND_MESH, Section::Mesh(mesh(&land_polygons)?)),
        (LAKE_MESH, Section::Mesh(mesh(&lake_polygons)?)),
        (COAST_LINES, Section::Lines(shores(&land_polygons)?)),
        (SHORE_LINES, Section::Lines(shores(&lake_polygons)?)),
        (RIVER_LINES, Section::Lines(clipped_lines(&river_lines)?)),
    ];
    put_u32(&mut out, sections.len());
    for (kind, section) in &sections {
        out.push(*kind);
        match section {
            Section::Mesh((vertices, indices)) => {
                put_u32(&mut out, vertices.len());
                for [x, y] in vertices {
                    put_i16(&mut out, *x);
                    put_i16(&mut out, *y);
                }
                put_u32(&mut out, indices.len());
                for i in indices {
                    out.extend_from_slice(&i.to_le_bytes());
                }
                eprintln!(
                    "atlas-basemap: section {kind}: {} vertices, {} triangles",
                    vertices.len(),
                    indices.len() / 3
                );
            }
            Section::Lines(lines) => {
                put_u32(&mut out, lines.len());
                for (rank, points) in lines {
                    out.push(*rank);
                    put_u32(&mut out, points.len());
                    for [x, y] in points {
                        put_i16(&mut out, *x);
                        put_i16(&mut out, *y);
                    }
                }
                eprintln!(
                    "atlas-basemap: section {kind}: {} lines, {} points",
                    lines.len(),
                    lines.iter().map(|(_, p)| p.len()).sum::<usize>()
                );
            }
        }
    }
    Ok(out)
}

enum Section {
    Mesh((Vec<[i16; 2]>, Vec<u32>)),
    Lines(Vec<(u8, Vec<[i16; 2]>)>),
}

fn put_i16(out: &mut Vec<u8>, v: i16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_i32(out: &mut Vec<u8>, v: i32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_u32(out: &mut Vec<u8>, n: usize) {
    out.extend_from_slice(&u32::try_from(n).unwrap_or(u32::MAX).to_le_bytes());
}

/// Degrees → stored units.
fn units(degrees: f64) -> Result<i16> {
    let v = (degrees * f64::from(UNITS_PER_DEGREE)).round();
    if !(f64::from(i16::MIN)..=f64::from(i16::MAX)).contains(&v) {
        bail!("{degrees}° is out of the file's range");
    }
    // Checked to fit just above.
    #[allow(clippy::cast_possible_truncation)]
    Ok(v as i16)
}

/// A ring or line in stored units, simplified, without repeated points.
fn quantized(points: &[Pt]) -> Result<Vec<[i16; 2]>> {
    let mut out: Vec<[i16; 2]> = Vec::with_capacity(points.len());
    for p in simplify(points, TOLERANCE) {
        let q = [units(p[0])?, units(p[1])?];
        if out.last() != Some(&q) {
            out.push(q);
        }
    }
    Ok(out)
}

/// Douglas-Peucker: the points of `line` within `tolerance` of the simplified line are dropped.
/// The first and last points are kept (a closed ring stays closed).
fn simplify(line: &[Pt], tolerance: f64) -> Vec<Pt> {
    if line.len() < 3 {
        return line.to_vec();
    }
    let mut keep = vec![false; line.len()];
    keep[0] = true;
    keep[line.len() - 1] = true;
    let mut stack = vec![(0usize, line.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let mut far = (0.0, a);
        for i in a + 1..b {
            let d = distance_to_segment(line[i], line[a], line[b]);
            if d > far.0 {
                far = (d, i);
            }
        }
        if far.0 > tolerance {
            keep[far.1] = true;
            stack.push((a, far.1));
            stack.push((far.1, b));
        }
    }
    line.iter()
        .zip(keep)
        .filter_map(|(p, k)| k.then_some(*p))
        .collect()
}

fn distance_to_segment(p: Pt, a: Pt, b: Pt) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0)
    };
    let (x, y) = (a[0] + t * dx, a[1] + t * dy);
    ((p[0] - x).powi(2) + (p[1] - y).powi(2)).sqrt()
}

/// Sutherland-Hodgman: `ring` (open or closed) cut to [`BOUNDS`]; open.
fn clip_ring(ring: &[Pt]) -> Vec<Pt> {
    let [w, s, e, n] = BOUNDS;
    let mut pts: Vec<Pt> = ring.to_vec();
    if pts.len() > 1 && pts.first() == pts.last() {
        pts.pop();
    }
    // (inside, intersect) for each edge.
    type Inside = fn(Pt, f64) -> bool;
    type Cross = fn(Pt, Pt, f64) -> Pt;
    let edges: [(Inside, Cross, f64); 4] = [
        (|p, v| p[0] >= v, cross_x, w),
        (|p, v| p[0] <= v, cross_x, e),
        (|p, v| p[1] >= v, cross_y, s),
        (|p, v| p[1] <= v, cross_y, n),
    ];
    for (inside, cross, v) in edges {
        if pts.is_empty() {
            break;
        }
        let input = std::mem::take(&mut pts);
        let mut prev = input[input.len() - 1];
        for &cur in &input {
            match (inside(cur, v), inside(prev, v)) {
                (true, true) => pts.push(cur),
                (true, false) => {
                    pts.push(cross(prev, cur, v));
                    pts.push(cur);
                }
                (false, true) => pts.push(cross(prev, cur, v)),
                (false, false) => {}
            }
            prev = cur;
        }
    }
    pts
}

fn cross_x(a: Pt, b: Pt, x: f64) -> Pt {
    let t = (x - a[0]) / (b[0] - a[0]);
    [x, a[1] + t * (b[1] - a[1])]
}

fn cross_y(a: Pt, b: Pt, y: f64) -> Pt {
    let t = (y - a[1]) / (b[1] - a[1]);
    [a[0] + t * (b[0] - a[0]), y]
}

fn ring_bbox_touches_bounds(ring: &[Pt]) -> bool {
    let [w, s, e, n] = BOUNDS;
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in ring {
        x0 = x0.min(p[0]);
        y0 = y0.min(p[1]);
        x1 = x1.max(p[0]);
        y1 = y1.max(p[1]);
    }
    x1 >= w && x0 <= e && y1 >= s && y0 <= n
}

/// Polygons cut to [`BOUNDS`], simplified and triangulated: (vertices, triangle indices).
fn mesh(polygons: &[Vec<Vec<Pt>>]) -> Result<(Vec<[i16; 2]>, Vec<u32>)> {
    let mut vertices: Vec<[i16; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut earcut = earcut::Earcut::<f64>::new();
    let mut triangles: Vec<u32> = Vec::new();
    for polygon in polygons {
        let Some(outer) = polygon.first() else {
            continue;
        };
        if !ring_bbox_touches_bounds(outer) {
            continue;
        }
        let mut data: Vec<[f64; 2]> = Vec::new();
        let mut holes: Vec<u32> = Vec::new();
        let mut q_vertices: Vec<[i16; 2]> = Vec::new();
        for (i, ring) in polygon.iter().enumerate() {
            let mut q = quantized(&clip_ring(&simplify_closed(ring)))?;
            if q.len() > 1 && q.first() == q.last() {
                q.pop();
            }
            if q.len() < 3 {
                if i == 0 {
                    break;
                }
                continue;
            }
            if i > 0 {
                holes.push(u32::try_from(q_vertices.len())?);
            }
            data.extend(q.iter().map(|[x, y]| [f64::from(*x), f64::from(*y)]));
            q_vertices.extend(q);
        }
        if q_vertices.len() < 3 {
            continue;
        }
        earcut.earcut(data.iter().copied(), &holes, &mut triangles);
        let base = u32::try_from(vertices.len())?;
        vertices.extend(q_vertices);
        indices.extend(triangles.iter().map(|i| base + i));
    }
    Ok((vertices, indices))
}

/// A closed ring simplified as two halves, so Douglas-Peucker keeps more than its end point.
fn simplify_closed(ring: &[Pt]) -> Vec<Pt> {
    if ring.len() < 8 {
        return ring.to_vec();
    }
    let mid = ring.len() / 2;
    let mut out = simplify(&ring[..=mid], TOLERANCE);
    out.pop();
    out.extend(simplify(&ring[mid..], TOLERANCE));
    out
}

/// The outlines of polygons (every ring), cut to [`BOUNDS`], as lines of rank 0.
fn shores(polygons: &[Vec<Vec<Pt>>]) -> Result<Vec<(u8, Vec<[i16; 2]>)>> {
    let rings: Vec<(u8, Vec<Pt>)> = polygons
        .iter()
        .flatten()
        .filter(|r| ring_bbox_touches_bounds(r))
        .map(|r| (0, simplify_closed(r)))
        .collect();
    clipped_lines(&rings)
}

/// Lines cut to [`BOUNDS`] (a line leaving and re-entering becomes several), simplified.
fn clipped_lines(lines: &[(u8, Vec<Pt>)]) -> Result<Vec<(u8, Vec<[i16; 2]>)>> {
    let [w, s, e, n] = BOUNDS;
    let inside = |p: Pt| p[0] >= w && p[0] <= e && p[1] >= s && p[1] <= n;
    let mut out = Vec::new();
    for (rank, line) in lines {
        let mut run: Vec<Pt> = Vec::new();
        let mut flush = |run: &mut Vec<Pt>| -> Result<()> {
            if run.len() >= 2 {
                let q = quantized(run)?;
                if q.len() >= 2 {
                    out.push((*rank, q));
                }
            }
            run.clear();
            Ok(())
        };
        for pair in line.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            match clip_segment(a, b, [w, s, e, n]) {
                Some((ca, cb)) => {
                    if run.last() != Some(&ca) {
                        flush(&mut run)?;
                        run.push(ca);
                    }
                    run.push(cb);
                    if !inside(b) {
                        flush(&mut run)?;
                    }
                }
                None => flush(&mut run)?,
            }
        }
        flush(&mut run)?;
    }
    Ok(out)
}

/// Liang-Barsky: the part of segment a-b inside `bounds`.
fn clip_segment(a: Pt, b: Pt, [w, s, e, n]: [f64; 4]) -> Option<(Pt, Pt)> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [
        (-dx, a[0] - w),
        (dx, e - a[0]),
        (-dy, a[1] - s),
        (dy, n - a[1]),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
        }
    }
    (t0 <= t1).then(|| {
        (
            [a[0] + t0 * dx, a[1] + t0 * dy],
            [a[0] + t1 * dx, a[1] + t1 * dy],
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rings_are_cut_to_the_bounds() {
        let square = [[-20.0, 0.0], [10.0, 0.0], [10.0, 20.0], [-20.0, 20.0]];
        let cut = clip_ring(&square);
        assert!(cut.iter().all(|p| p[0] >= -12.0 && p[1] >= 6.0));
        assert!(cut.contains(&[-12.0, 6.0]) && cut.contains(&[10.0, 20.0]));
    }

    #[test]
    fn lines_leaving_the_bounds_are_split() {
        let line = vec![(
            3,
            vec![[0.0, 10.0], [80.0, 10.0], [80.0, 20.0], [0.0, 20.0]],
        )];
        let cut = clipped_lines(&line).unwrap();
        assert_eq!(cut.len(), 2);
        assert_eq!(cut[0].1, [[0, 2000], [14400, 2000]]);
        assert_eq!(cut[1].1, [[14400, 4000], [0, 4000]]);
        assert!(cut.iter().all(|(rank, _)| *rank == 3));
    }

    #[test]
    fn straight_runs_simplify_to_their_ends() {
        let line: Vec<Pt> = (0..10).map(|i| [f64::from(i), 0.0]).collect();
        assert_eq!(simplify(&line, 0.01), [[0.0, 0.0], [9.0, 0.0]]);
    }

    #[test]
    fn a_square_with_a_hole_triangulates() {
        let polygon = vec![
            vec![
                [0.0, 10.0],
                [10.0, 10.0],
                [10.0, 20.0],
                [0.0, 20.0],
                [0.0, 10.0],
            ],
            vec![
                [4.0, 14.0],
                [4.0, 16.0],
                [6.0, 16.0],
                [6.0, 14.0],
                [4.0, 14.0],
            ],
        ];
        let (vertices, indices) = mesh(&[polygon]).unwrap();
        assert_eq!(vertices.len(), 8);
        // 8 triangles cover a square ring with a square hole.
        assert_eq!(indices.len(), 24);
    }
}
