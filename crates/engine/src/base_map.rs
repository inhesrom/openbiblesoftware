//! The Atlas's built-in base map (#150): the Bible lands drawn from Natural Earth (public
//! domain), shipped inside the app so the Atlas works offline with no tile server.
//!
//! `assets/atlas/basemap.bin` is written by `cargo xtask atlas-basemap` (all little-endian):
//!
//! | bytes | what |
//! |---|---|
//! | 8 | magic `OBSMAP\x01\x00` |
//! | 4 | `i32` units per degree (coordinates below are whole units) |
//! | 8 | `i16` west, south, east, north: the bounds the map covers |
//! | 4 | `u32` number of sections |
//! | … | each section: a `u8` kind, then its body |
//!
//! Kinds: 1 land and 2 lakes are triangle meshes (`u32` vertex count, `[i16; 2]` longitude and
//! latitude per vertex, `u32` index count, `u32` per index, three per triangle); 3 coasts,
//! 4 lake shores and 5 rivers are lines (`u32` line count; per line a `u8` rank, the source's
//! scale rank, smaller is more important, then a `u32` point count and its `[i16; 2]` points).
//! Unknown kinds end the read, so a newer file still shows what this build knows.
//!
//! [`MapView`] is the projection the Atlas and the Factbook's Map draw with: equirectangular,
//! with longitudes narrowed by the cosine of 36° N (the middle of the Bible lands) so shapes
//! keep their proportions there.

use std::sync::OnceLock;

/// The built-in base map file.
const BASE_MAP: &[u8] = include_bytes!("../assets/atlas/basemap.bin");

const MAGIC: &[u8; 8] = b"OBSMAP\x01\x00";

/// cos 36°: one degree of longitude is this many degrees of latitude wide on the map.
pub const LONGITUDE_SCALE: f64 = 0.809_016_994_374_947_4;

/// A box of longitudes and latitudes, in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeoBounds {
    pub west: f64,
    pub south: f64,
    pub east: f64,
    pub north: f64,
}

impl GeoBounds {
    /// The smallest box holding every `(lon, lat)` point, or `None` for none.
    pub fn around(points: impl IntoIterator<Item = (f64, f64)>) -> Option<Self> {
        let mut b: Option<GeoBounds> = None;
        for (lon, lat) in points {
            let next = match b {
                None => GeoBounds {
                    west: lon,
                    south: lat,
                    east: lon,
                    north: lat,
                },
                Some(b) => GeoBounds {
                    west: b.west.min(lon),
                    south: b.south.min(lat),
                    east: b.east.max(lon),
                    north: b.north.max(lat),
                },
            };
            b = Some(next);
        }
        b
    }

    /// The box grown to at least `degrees` of latitude on each side of its centre.
    pub fn at_least(self, degrees: f64) -> Self {
        let (cx, cy) = self.center();
        let half_h = ((self.north - self.south) / 2.0).max(degrees / 2.0);
        let half_w = ((self.east - self.west) / 2.0).max(degrees / 2.0 / LONGITUDE_SCALE);
        // Never smaller than it was (rounding aside).
        GeoBounds {
            west: (cx - half_w).min(self.west),
            south: (cy - half_h).min(self.south),
            east: (cx + half_w).max(self.east),
            north: (cy + half_h).max(self.north),
        }
    }

    pub fn center(&self) -> (f64, f64) {
        (
            (self.west + self.east) / 2.0,
            (self.south + self.north) / 2.0,
        )
    }

    pub fn contains(&self, lon: f64, lat: f64) -> bool {
        (self.west..=self.east).contains(&lon) && (self.south..=self.north).contains(&lat)
    }
}

/// Triangles: `points` are `[longitude, latitude]` in degrees; each three `triangles` indices
/// make one.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MapMesh {
    pub points: Vec<[f32; 2]>,
    pub triangles: Vec<u32>,
}

/// A line of `[longitude, latitude]` points; `rank` is the source's scale rank (smaller is
/// more important: drawn when zoomed out too).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MapLine {
    pub rank: u8,
    pub points: Vec<[f32; 2]>,
}

/// The base map: land and lakes to fill, coasts, lake shores and rivers to stroke.
#[derive(Debug, Clone, PartialEq)]
pub struct BaseMap {
    pub bounds: GeoBounds,
    pub land: MapMesh,
    pub lakes: MapMesh,
    pub coasts: Vec<MapLine>,
    pub shores: Vec<MapLine>,
    pub rivers: Vec<MapLine>,
}

impl Default for BaseMap {
    fn default() -> Self {
        Self {
            bounds: GeoBounds {
                west: -12.0,
                south: 6.0,
                east: 72.0,
                north: 50.0,
            },
            land: MapMesh::default(),
            lakes: MapMesh::default(),
            coasts: Vec::new(),
            shores: Vec::new(),
            rivers: Vec::new(),
        }
    }
}

/// The built-in base map, decoded once. An unreadable file (never shipped: a test decodes it)
/// gives an empty map, so the Atlas still shows its places.
pub fn builtin() -> &'static BaseMap {
    static MAP: OnceLock<BaseMap> = OnceLock::new();
    MAP.get_or_init(|| decode(BASE_MAP).unwrap_or_default())
}

/// Read a base map file (see the module docs).
pub fn decode(bytes: &[u8]) -> Result<BaseMap, String> {
    let mut r = Reader { bytes, at: 0 };
    if r.take(8)? != MAGIC {
        return Err("not a base map file".into());
    }
    let units = f64::from(r.i32()?);
    if units <= 0.0 {
        return Err("bad units per degree".into());
    }
    let mut corner = || -> Result<f64, String> { Ok(f64::from(r.i16()?) / units) };
    let bounds = GeoBounds {
        west: corner()?,
        south: corner()?,
        east: corner()?,
        north: corner()?,
    };
    let mut map = BaseMap {
        bounds,
        ..BaseMap::default()
    };
    // Degrees as f32: the map is drawn in f32 screen points anyway.
    #[allow(clippy::cast_possible_truncation)]
    let point = |x: i16, y: i16| [(f64::from(x) / units) as f32, (f64::from(y) / units) as f32];
    let sections = r.u32()?;
    for _ in 0..sections {
        let kind = r.take(1)?[0];
        match kind {
            1 | 2 => {
                let n = r.u32()? as usize;
                let mut points = Vec::with_capacity(n.min(1 << 20));
                for _ in 0..n {
                    let (x, y) = (r.i16()?, r.i16()?);
                    points.push(point(x, y));
                }
                let m = r.u32()? as usize;
                if !m.is_multiple_of(3) {
                    return Err("a mesh's index count is not a multiple of 3".into());
                }
                let mut triangles = Vec::with_capacity(m.min(1 << 22));
                for _ in 0..m {
                    let i = r.u32()?;
                    if i as usize >= n {
                        return Err("a triangle names a point the mesh lacks".into());
                    }
                    triangles.push(i);
                }
                let mesh = MapMesh { points, triangles };
                if kind == 1 {
                    map.land = mesh;
                } else {
                    map.lakes = mesh;
                }
            }
            3..=5 => {
                let count = r.u32()?;
                let mut lines = Vec::new();
                for _ in 0..count {
                    let rank = r.take(1)?[0];
                    let n = r.u32()? as usize;
                    let mut points = Vec::with_capacity(n.min(1 << 20));
                    for _ in 0..n {
                        let (x, y) = (r.i16()?, r.i16()?);
                        points.push(point(x, y));
                    }
                    lines.push(MapLine { rank, points });
                }
                match kind {
                    3 => map.coasts = lines,
                    4 => map.shores = lines,
                    _ => map.rivers = lines,
                }
            }
            _ => break,
        }
    }
    Ok(map)
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(n).filter(|e| *e <= self.bytes.len());
        let Some(end) = end else {
            return Err("the base map file ends early".into());
        };
        let out = &self.bytes[self.at..end];
        self.at = end;
        Ok(out)
    }

    fn i16(&mut self) -> Result<i16, String> {
        let b = self.take(2)?;
        Ok(i16::from_le_bytes([b[0], b[1]]))
    }

    fn i32(&mut self) -> Result<i32, String> {
        let b = self.take(4)?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u32(&mut self) -> Result<u32, String> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
}

/// Screen points per degree of latitude, at the closest and the farthest zoom.
pub const MAX_SCALE: f64 = 900.0;
pub const MIN_SCALE: f64 = 4.0;

/// What part of the map a view shows: its centre and its zoom. Screen positions are relative
/// to the top-left of the view's rectangle of `size` (width, height) points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapView {
    /// Longitude and latitude at the centre of the view.
    pub center: (f64, f64),
    /// Screen points per degree of latitude.
    pub scale: f64,
}

impl MapView {
    /// The view that shows all of `bounds` in a `size` rectangle with `margin` points to spare
    /// on every side.
    pub fn fit(bounds: GeoBounds, size: (f64, f64), margin: f64) -> Self {
        let w = ((bounds.east - bounds.west) * LONGITUDE_SCALE).max(1e-6);
        let h = (bounds.north - bounds.south).max(1e-6);
        let room_w = (size.0 - 2.0 * margin).max(1.0);
        let room_h = (size.1 - 2.0 * margin).max(1.0);
        let scale = (room_w / w).min(room_h / h).clamp(MIN_SCALE, MAX_SCALE);
        MapView {
            center: bounds.center(),
            scale,
        }
    }

    /// Where `(lon, lat)` is drawn.
    pub fn to_screen(&self, lon: f64, lat: f64, size: (f64, f64)) -> (f64, f64) {
        (
            size.0 / 2.0 + (lon - self.center.0) * LONGITUDE_SCALE * self.scale,
            size.1 / 2.0 - (lat - self.center.1) * self.scale,
        )
    }

    /// The longitude and latitude drawn at screen point `(x, y)`.
    pub fn to_geo(&self, x: f64, y: f64, size: (f64, f64)) -> (f64, f64) {
        (
            self.center.0 + (x - size.0 / 2.0) / (LONGITUDE_SCALE * self.scale),
            self.center.1 - (y - size.1 / 2.0) / self.scale,
        )
    }

    /// Move the map by `(dx, dy)` screen points (a drag), keeping its centre on the base map.
    pub fn pan(&mut self, dx: f64, dy: f64, within: GeoBounds) {
        self.center.0 -= dx / (LONGITUDE_SCALE * self.scale);
        self.center.1 += dy / self.scale;
        self.keep_within(within);
    }

    /// Zoom by `factor` (above 1 is closer) keeping the point under `(x, y)` where it is.
    pub fn zoom_about(&mut self, factor: f64, x: f64, y: f64, size: (f64, f64), within: GeoBounds) {
        let before = self.to_geo(x, y, size);
        self.scale = (self.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        let after = self.to_geo(x, y, size);
        self.center.0 += before.0 - after.0;
        self.center.1 += before.1 - after.1;
        self.keep_within(within);
    }

    /// The longitudes and latitudes a `size` view shows.
    pub fn visible(&self, size: (f64, f64)) -> GeoBounds {
        let (west, north) = self.to_geo(0.0, 0.0, size);
        let (east, south) = self.to_geo(size.0, size.1, size);
        GeoBounds {
            west,
            south,
            east,
            north,
        }
    }

    fn keep_within(&mut self, b: GeoBounds) {
        self.center.0 = self.center.0.clamp(b.west, b.east);
        self.center.1 = self.center.1.clamp(b.south, b.north);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_builtin_base_map_decodes_and_covers_the_bible_lands() {
        let map = decode(BASE_MAP).unwrap();
        assert_eq!(map, *builtin());
        assert!(
            map.land.triangles.len() > 3_000,
            "{}",
            map.land.triangles.len()
        );
        assert!(!map.lakes.triangles.is_empty());
        assert!(!map.coasts.is_empty() && !map.shores.is_empty() && !map.rivers.is_empty());
        // Rome, Jerusalem, Babylon and Susa are all on the map.
        for (lon, lat) in [
            (12.48, 41.89),
            (35.23, 31.78),
            (44.42, 32.54),
            (48.26, 32.19),
        ] {
            assert!(map.bounds.contains(lon, lat), "{lon},{lat}");
        }
        // Every point is inside the bounds.
        let inside = |p: &[f32; 2]| map.bounds.contains(f64::from(p[0]), f64::from(p[1]));
        assert!(map.land.points.iter().all(inside));
        assert!(map.rivers.iter().flat_map(|l| &l.points).all(inside));
    }

    #[test]
    fn jerusalem_is_on_land_and_the_mediterranean_is_not() {
        let map = builtin();
        let on_land = |lon: f32, lat: f32| {
            map.land.triangles.chunks(3).any(|t| {
                let [a, b, c] = [0, 1, 2].map(|i| map.land.points[t[i] as usize]);
                in_triangle([lon, lat], a, b, c)
            })
        };
        assert!(on_land(35.23, 31.78), "Jerusalem");
        assert!(on_land(44.42, 32.54), "Babylon");
        assert!(!on_land(18.0, 34.0), "the Mediterranean");
        assert!(!on_land(38.0, 20.0), "the Red Sea");
    }

    fn in_triangle(p: [f32; 2], a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> bool {
        let side = |p: [f32; 2], q: [f32; 2], r: [f32; 2]| {
            (p[0] - r[0]) * (q[1] - r[1]) - (q[0] - r[0]) * (p[1] - r[1])
        };
        let (d1, d2, d3) = (side(p, a, b), side(p, b, c), side(p, c, a));
        let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
        let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
        !(neg && pos)
    }

    #[test]
    fn a_truncated_or_foreign_file_is_refused() {
        assert!(decode(b"PNG").is_err());
        assert!(decode(&BASE_MAP[..BASE_MAP.len() / 2]).is_err());
    }

    #[test]
    fn views_fit_pan_and_zoom_about_the_pointer() {
        let size = (800.0, 600.0);
        let journey = GeoBounds {
            west: 30.8,
            south: 34.7,
            east: 36.2,
            north: 38.3,
        };
        let v = MapView::fit(journey, size, 20.0);
        for (lon, lat) in [(30.8, 34.7), (36.2, 38.3)] {
            let (x, y) = v.to_screen(lon, lat, size);
            assert!(
                (19.0..=781.0).contains(&x) && (19.0..=581.0).contains(&y),
                "{x},{y}"
            );
        }
        let (lon, lat) = v.to_geo(400.0, 300.0, size);
        assert!((lon - 33.5).abs() < 1e-9 && (lat - 36.5).abs() < 1e-9);
        // Zooming keeps the point under the pointer still.
        let world = builtin().bounds;
        let mut z = v;
        let under = z.to_geo(100.0, 150.0, size);
        z.zoom_about(2.0, 100.0, 150.0, size, world);
        assert!((z.scale - v.scale * 2.0).abs() < 1e-9);
        let (x, y) = z.to_screen(under.0, under.1, size);
        assert!((x - 100.0).abs() < 1e-6 && (y - 150.0).abs() < 1e-6);
        // Dragging right moves the map right: the centre goes west.
        let mut p = v;
        p.pan(100.0, 0.0, world);
        assert!(p.center.0 < v.center.0);
        // The view never leaves the base map, nor zooms past its limits.
        p.pan(1e9, -1e9, world);
        assert_eq!(p.center, (world.west, world.south));
        z.zoom_about(1e9, 0.0, 0.0, size, world);
        assert_eq!(z.scale, MAX_SCALE);
    }

    #[test]
    fn small_boxes_grow_so_one_place_is_not_drawn_at_full_zoom() {
        let one = GeoBounds::around([(35.23, 31.78)]).unwrap().at_least(4.0);
        assert!((one.north - one.south - 4.0).abs() < 1e-9);
        assert!(one.contains(35.23, 31.78));
        assert_eq!(GeoBounds::around(std::iter::empty()), None);
    }
}
