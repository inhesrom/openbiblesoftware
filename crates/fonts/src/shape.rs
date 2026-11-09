//! Shaping with harfrust, and the shaped-segment cache shared by all threads (#19).

use std::collections::HashMap;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

use harfrust::{Buffer, Direction, Feature, ShapeOptions, ShaperFont, Tag};
use skrifa::instance::{LocationRef, Size};
use skrifa::{GlyphId, MetadataProvider};

use crate::face::{FaceError, FaceId, face};

/// How a segment is shaped. Everything that changes glyphs or positions is in here, so it is
/// also the cache key (with the text).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShapeStyle {
    pub face: FaceId,
    /// Font size in px, as `f32::to_bits` so the style can be hashed.
    size_bits: u32,
    /// OpenType `smcp` (small capitals), for the divine name.
    pub small_caps: bool,
    pub rtl: bool,
}

impl ShapeStyle {
    pub fn new(face: FaceId, size: f32, small_caps: bool, rtl: bool) -> Self {
        Self {
            face,
            size_bits: size.to_bits(),
            small_caps,
            rtl,
        }
    }

    pub fn size(&self) -> f32 {
        f32::from_bits(self.size_bits)
    }
}

/// One positioned glyph, in px, relative to the segment's pen start (y up).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glyph {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub advance: f32,
    /// Byte offset in the segment of the cluster this glyph belongs to.
    pub cluster: u32,
}

/// One cluster: the glyphs that draw a byte range of the text, and their horizontal extent.
#[derive(Debug, Clone, PartialEq)]
pub struct Cluster {
    /// Bytes of the segment this cluster draws.
    pub bytes: Range<usize>,
    /// Extent in px from the segment start, `x0 <= x1`.
    pub x0: f32,
    pub x1: f32,
}

/// A shaped segment: glyphs and clusters in visual (left-to-right) order.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapedRun {
    pub glyphs: Vec<Glyph>,
    pub clusters: Vec<Cluster>,
    /// Total advance in px.
    pub width: f32,
    /// Width of trailing whitespace (logical end), which line breaking may hang.
    pub trailing_space: f32,
}

impl ShapedRun {
    /// The cluster that contains byte `offset` of the segment.
    pub fn cluster_at(&self, offset: usize) -> Option<&Cluster> {
        self.clusters.iter().find(|c| c.bytes.contains(&offset))
    }

    /// Glyphs that are `.notdef` (missing from the face).
    pub fn missing_glyphs(&self) -> usize {
        self.glyphs.iter().filter(|g| g.id == 0).count()
    }
}

/// How far `run`'s glyphs reach above its baseline, in px (shaped with `style`): the top of
/// their ink, not the face's ascent. `None` when no glyph has ink.
pub fn ink_top(run: &ShapedRun, style: ShapeStyle) -> Option<f32> {
    let f = face(style.face).ok()?;
    let font = f.font_ref()?;
    let metrics = font.glyph_metrics(Size::new(style.size()), LocationRef::default());
    run.glyphs
        .iter()
        .filter_map(|g| {
            let b = metrics.bounds(GlyphId::new(g.id))?;
            (b.y_max > b.y_min).then_some(b.y_max + g.y)
        })
        .reduce(f32::max)
}

/// Lowercase ASCII letters only, keeping every byte offset, for small-caps text whose source
/// is already uppercase (`\nd LORD`): shape `lord` with `smcp` to draw small capitals while
/// standoff spans stay valid (#13).
pub fn small_caps_source(text: &str) -> String {
    text.to_ascii_lowercase()
}

/// Shape `text` with no cache.
pub fn shape_uncached(text: &str, style: ShapeStyle) -> Result<ShapedRun, FaceError> {
    let face = face(style.face)?;
    let font = ShaperFont::new(&face.shaping);
    let mut buffer = Buffer::new();
    buffer.push_str(text);
    buffer.guess_segment_properties();
    buffer.set_direction(if style.rtl {
        Direction::RightToLeft
    } else {
        Direction::LeftToRight
    });
    let smcp = [Feature::new(Tag::new(b"smcp"), 1, ..)];
    let features: &[Feature] = if style.small_caps { &smcp } else { &[] };
    if harfrust::shape(&font, &mut buffer, ShapeOptions::new().features(features)).is_err() {
        return Ok(ShapedRun {
            glyphs: Vec::new(),
            clusters: Vec::new(),
            width: 0.0,
            trailing_space: 0.0,
        });
    }
    let scale = style.size() / f32::from(face.metrics.units_per_em.max(1));
    let mut glyphs = Vec::with_capacity(buffer.len());
    let mut pen = 0.0f32;
    for (info, pos) in buffer.glyph_infos().iter().zip(buffer.glyph_positions()) {
        let advance = pos.x_advance as f32 * scale;
        glyphs.push(Glyph {
            id: info.glyph_id,
            x: pen + pos.x_offset as f32 * scale,
            y: pos.y_offset as f32 * scale,
            advance,
            cluster: info.cluster,
        });
        pen += advance;
    }
    let clusters = clusters(text, &glyphs);
    let trailing_space = trailing_space(text, &clusters);
    Ok(ShapedRun {
        glyphs,
        clusters,
        width: pen,
        trailing_space,
    })
}

/// Group glyphs by cluster (visual order) and give each its byte range.
fn clusters(text: &str, glyphs: &[Glyph]) -> Vec<Cluster> {
    let mut out: Vec<Cluster> = Vec::new();
    let mut pen = 0.0;
    for g in glyphs {
        let start = g.cluster as usize;
        match out.last_mut() {
            Some(c) if c.bytes.start == start => c.x1 = pen + g.advance,
            _ => out.push(Cluster {
                bytes: start..start,
                x0: pen,
                x1: pen + g.advance,
            }),
        }
        pen += g.advance;
    }
    let mut starts: Vec<usize> = out.iter().map(|c| c.bytes.start).collect();
    starts.sort_unstable();
    starts.dedup();
    for c in &mut out {
        let i = starts.partition_point(|&s| s <= c.bytes.start);
        c.bytes.end = starts.get(i).copied().unwrap_or(text.len());
    }
    out
}

fn trailing_space(text: &str, clusters: &[Cluster]) -> f32 {
    let mut logical: Vec<&Cluster> = clusters.iter().collect();
    logical.sort_by_key(|c| std::cmp::Reverse(c.bytes.start));
    let mut width = 0.0;
    for c in logical {
        let all_space = text
            .get(c.bytes.clone())
            .is_some_and(|t| !t.is_empty() && t.chars().all(char::is_whitespace));
        if !all_space {
            break;
        }
        width += c.x1 - c.x0;
    }
    width
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key {
    style: ShapeStyle,
    text: String,
}

/// Counters for the shaped-segment cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub entries: usize,
}

impl CacheStats {
    /// Hits as a share of lookups, 0 when there were none.
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

/// Shaped segments keyed by face, size, features, direction and text, shared by the UI thread
/// and layout workers. When it grows past its capacity it is cleared (segments are cheap to
/// reshape; the prototype measured a 90% hit rate on Genesis).
#[derive(Debug)]
pub struct ShapeCache {
    map: RwLock<HashMap<Key, Arc<ShapedRun>>>,
    capacity: usize,
    hits: AtomicU64,
    misses: AtomicU64,
}

impl ShapeCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            map: RwLock::new(HashMap::new()),
            capacity: capacity.max(1),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }

    /// The process-wide cache (400k segments, as in the prototype).
    pub fn shared() -> &'static ShapeCache {
        static SHARED: OnceLock<ShapeCache> = OnceLock::new();
        SHARED.get_or_init(|| ShapeCache::new(400_000))
    }

    /// Shape `text`, reusing an earlier result for the same text and style.
    pub fn shape(&self, text: &str, style: ShapeStyle) -> Result<Arc<ShapedRun>, FaceError> {
        let key = Key {
            style,
            text: text.to_string(),
        };
        if let Some(hit) = self.map.read().unwrap_or_else(|p| p.into_inner()).get(&key) {
            self.hits.fetch_add(1, Ordering::Relaxed);
            return Ok(Arc::clone(hit));
        }
        self.misses.fetch_add(1, Ordering::Relaxed);
        let run = Arc::new(shape_uncached(text, style)?);
        let mut map = self.map.write().unwrap_or_else(|p| p.into_inner());
        if map.len() >= self.capacity {
            map.clear();
        }
        map.insert(key, Arc::clone(&run));
        Ok(run)
    }

    pub fn stats(&self) -> CacheStats {
        CacheStats {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            entries: self.map.read().map(|m| m.len()).unwrap_or(0),
        }
    }

    pub fn clear(&self) {
        self.map.write().unwrap_or_else(|p| p.into_inner()).clear();
    }
}
