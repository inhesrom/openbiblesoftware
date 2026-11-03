//! The scripted scroll/fling/jump benchmark behind `cargo xtask bench` (#19), ported from the
//! app-shell prototype's `OBS_BENCH` harness.
//!
//! It drives several reading panes headlessly, frame by frame at the display refresh rate,
//! doing what the UI thread does per frame (apply worker layouts, lay out within the budget,
//! build glyph-atlas quads) and timing it. GPU submission and egui tessellation are not
//! included; the prototype measured those as a small, constant share of the frame.

use std::sync::Arc;
use std::time::{Duration, Instant};

use obs_layout::TextSettings;
use obs_model::{
    KeyScheme, Manifest, ParaStyle, ResourceBuilder, ResourceType, SpanKind, TextAccumulator,
    UnitId, UnitKind,
};
use obs_render::GlyphAtlas;
use obs_scripture::{Book, Standard, Verse, Versification};
use serde::Serialize;

use crate::doc::ReadingDoc;
use crate::view::{ReadingView, SYNC_BUDGET};

#[derive(Debug, Clone)]
pub struct BenchConfig {
    pub refresh_hz: f32,
    pub panes: usize,
    /// Pane size in points.
    pub viewport: (f32, f32),
    /// Device pixels per point.
    pub scale: f32,
    pub steady_frames: usize,
    /// Wheel scroll per frame, points.
    pub steady_px: f32,
    pub flings: usize,
    pub jumps: usize,
    /// Sleep out each frame interval, so workers get real time between frames.
    pub pace: bool,
}

impl Default for BenchConfig {
    fn default() -> Self {
        Self {
            refresh_hz: 60.0,
            panes: 4,
            viewport: (480.0, 900.0),
            scale: 2.0,
            steady_frames: 600,
            steady_px: 14.0,
            flings: 4,
            jumps: 24,
            pace: true,
        }
    }
}

/// Timings of one scenario.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ScenarioReport {
    pub name: String,
    pub frames: usize,
    /// Frames whose UI-thread work exceeded the frame interval.
    pub missed_frames: usize,
    pub missed_pct: f64,
    /// Frames in which any pane drew a placeholder.
    pub placeholder_frames: usize,
    pub cpu_p50_ms: f64,
    pub cpu_p99_ms: f64,
    pub sync_layout_p50_ms: f64,
    pub sync_layout_p99_ms: f64,
    pub sync_layout_max_ms: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BenchReport {
    pub refresh_hz: f32,
    pub panes: usize,
    pub blocks: usize,
    pub scenarios: Vec<ScenarioReport>,
    /// UI-thread layout per frame over every measured frame, p99 (#19: ≤ 2 ms).
    pub sync_layout_p99_ms: f64,
    /// Frames after a jump until the jumping pane has no placeholders (0 = settled at once).
    pub jump_origin_settle_frames_max: usize,
    /// Frames after a jump until every pane has no placeholders.
    pub jump_all_settle_frames_max: usize,
    /// Glyph atlas as uploaded (RGBA), MiB.
    pub text_gpu_mib: f64,
    pub shape_cache_hit_rate: f64,
}

#[derive(Default)]
struct Samples {
    cpu: Vec<f64>,
    sync: Vec<f64>,
    placeholder_frames: usize,
}

fn percentile(v: &[f64], p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    let i = ((s.len() as f64 - 1.0) * p).round() as usize;
    s[i.min(s.len() - 1)]
}

impl Samples {
    fn report(&self, name: &str, interval_ms: f64) -> ScenarioReport {
        let missed = self.cpu.iter().filter(|&&c| c > interval_ms).count();
        ScenarioReport {
            name: name.into(),
            frames: self.cpu.len(),
            missed_frames: missed,
            missed_pct: 100.0 * missed as f64 / self.cpu.len().max(1) as f64,
            placeholder_frames: self.placeholder_frames,
            cpu_p50_ms: percentile(&self.cpu, 0.5),
            cpu_p99_ms: percentile(&self.cpu, 0.99),
            sync_layout_p50_ms: percentile(&self.sync, 0.5),
            sync_layout_p99_ms: percentile(&self.sync, 0.99),
            sync_layout_max_ms: self.sync.iter().copied().fold(0.0, f64::max),
        }
    }
}

struct Bench {
    cfg: BenchConfig,
    views: Vec<ReadingView>,
    atlas: GlyphAtlas,
    interval: Duration,
    budget: Duration,
}

impl Bench {
    /// One frame of every pane, with `extra` work (e.g. a jump) counted in its time.
    /// Returns per-pane placeholder counts.
    fn frame(
        &mut self,
        samples: &mut Samples,
        extra: impl FnOnce(&mut [ReadingView]),
    ) -> Vec<usize> {
        let start = Instant::now();
        extra(&mut self.views);
        let mut sync = Duration::ZERO;
        let mut placeholders = Vec::with_capacity(self.views.len());
        for (k, v) in self.views.iter_mut().enumerate() {
            // One UI budget per frame, shared by every pane (#19: ≤ 2 ms per frame).
            let left = self.budget.saturating_sub(sync);
            let f = v.frame(self.cfg.viewport.1, Some(left));
            sync += f.stats.sync_layout;
            placeholders.push(f.stats.placeholders);
            let x = k as f32 * self.cfg.viewport.0;
            for b in &f.blocks {
                if let Some(l) = &b.laid {
                    std::hint::black_box(self.atlas.quads(l, [x, b.top], self.cfg.scale));
                }
            }
        }
        let elapsed = start.elapsed();
        samples.cpu.push(elapsed.as_secs_f64() * 1000.0);
        samples.sync.push(sync.as_secs_f64() * 1000.0);
        if placeholders.iter().any(|&p| p > 0) {
            samples.placeholder_frames += 1;
        }
        if self.cfg.pace && elapsed < self.interval {
            std::thread::sleep(self.interval - elapsed);
        }
        placeholders
    }

    fn settle(&mut self) {
        let mut scratch = Samples::default();
        for _ in 0..600 {
            self.frame(&mut scratch, |_| {});
            if self.views.iter().all(|v| !v.has_pending()) {
                return;
            }
        }
    }
}

/// Run the benchmark on a document.
pub fn run(doc: Arc<ReadingDoc>, cfg: BenchConfig) -> BenchReport {
    let interval = Duration::from_secs_f64(1.0 / f64::from(cfg.refresh_hz));
    let panes = cfg.panes.max(1);
    let mut views: Vec<ReadingView> = (0..panes)
        .map(|_| {
            let mut v = ReadingView::new(Arc::clone(&doc), TextSettings::default());
            v.set_width(cfg.viewport.0 - 2.0 * crate::view::MARGIN);
            v
        })
        .collect();
    // Spread the panes through the document, as four Resources at one place would stress
    // layout like four places in one Resource.
    let verses = &doc.verses;
    for (k, v) in views.iter_mut().enumerate() {
        if let Some(bv) = verses.get(verses.len() * k / (panes + 1)) {
            v.jump_to(bv.verse);
        }
    }
    let mut bench = Bench {
        cfg: cfg.clone(),
        views,
        atlas: GlyphAtlas::new(),
        interval,
        budget: SYNC_BUDGET,
    };
    let interval_ms = interval.as_secs_f64() * 1000.0;
    bench.settle();

    let mut steady = Samples::default();
    for _ in 0..cfg.steady_frames {
        let dy = cfg.steady_px;
        bench.frame(&mut steady, |views| {
            views.iter_mut().for_each(|v| v.scroll_by(dy))
        });
    }

    let mut fling = Samples::default();
    for f in 0..cfg.flings {
        let mut velocity = if f % 2 == 0 { 140.0f32 } else { -140.0 };
        while velocity.abs() > 1.0 {
            let dy = velocity;
            bench.frame(&mut fling, |views| {
                views.iter_mut().for_each(|v| v.scroll_by(dy))
            });
            velocity *= 0.94;
        }
    }

    // The panes are Tabs in one Link set; a jump goes through the engine's Link-set routing
    // (#36), and its Follow effects move the other panes.
    let mut ws = obs_engine::Workspace::new();
    let tabs: Vec<obs_engine::TabId> = (0..panes)
        .map(|k| {
            let kind = obs_engine::TabKind::Resource(doc.manifest.resource_id.clone());
            let req = obs_engine::OpenRequest {
                panel: None,
                split: k > 0,
            };
            let t = ws.open(kind, format!("pane {k}"), req);
            if let Some(tab) = ws.tab_mut(t) {
                tab.versification = Some(Arc::new(doc.versification.clone()));
                tab.verse_map = doc.verse_map.clone();
            }
            ws.set_link(t, Some('A'));
            t
        })
        .collect();
    let _ = ws.take_effects();

    let mut jumps = Samples::default();
    let (mut origin_max, mut all_max) = (0usize, 0usize);
    for j in 0..cfg.jumps {
        bench.settle();
        let Some(target) = verses
            .get((j * 7919 + 31) % verses.len().max(1))
            .map(|v| v.verse)
        else {
            break;
        };
        let mut placeholders = bench.frame(&mut jumps, |views| {
            // The origin jumps at once (its target block is laid out now, exempt from the
            // budget); the Link set's followers follow off-thread.
            ws.navigate(tabs[0], target);
            for e in ws.take_effects() {
                match e {
                    obs_engine::Effect::Navigate { tab, verse } => {
                        if let Some(i) = tabs.iter().position(|t| *t == tab) {
                            views[i].jump_to(verse);
                        }
                    }
                    obs_engine::Effect::Follow { tab, verse, .. } => {
                        if let Some(i) = tabs.iter().position(|t| *t == tab) {
                            views[i].follow(verse);
                        }
                    }
                    _ => {}
                }
            }
        });
        let (mut origin, mut all) = (None, None);
        for frame in 0..60usize {
            if origin.is_none() && placeholders[0] == 0 {
                origin = Some(frame);
            }
            if placeholders.iter().all(|&p| p == 0) {
                all = Some(frame);
                break;
            }
            placeholders = bench.frame(&mut jumps, |_| {});
        }
        origin_max = origin_max.max(origin.unwrap_or(60));
        all_max = all_max.max(all.unwrap_or(60));
    }

    let all_sync: Vec<f64> = [&steady, &fling, &jumps]
        .iter()
        .flat_map(|s| s.sync.iter().copied())
        .collect();
    BenchReport {
        refresh_hz: cfg.refresh_hz,
        sync_layout_p99_ms: percentile(&all_sync, 0.99),
        panes,
        blocks: doc.blocks.len(),
        scenarios: vec![
            steady.report("steady", interval_ms),
            fling.report("fling", interval_ms),
            jumps.report("jumps", interval_ms),
        ],
        jump_origin_settle_frames_max: origin_max,
        jump_all_settle_frames_max: all_max,
        text_gpu_mib: (bench.atlas.bytes() * 4) as f64 / (1024.0 * 1024.0),
        shape_cache_hit_rate: obs_fonts::ShapeCache::shared().stats().hit_rate(),
    }
}

/// A deterministic Bible-shaped document for CI, where no real Bible is available: Genesis to
/// Deuteronomy chapter and verse counts, prose paragraphs of 3–8 verses, every fifth chapter
/// poetry, verses of 12–60 words.
pub fn synthetic_bible() -> Arc<ReadingDoc> {
    // Common words repeat as in real text; the rest are made from syllables, giving a
    // vocabulary of thousands of words so the shape cache sees realistic misses.
    const WORDS: &[&str] = &[
        "and", "the", "LORD", "said", "unto", "of", "that", "they", "me", "an", "every", "man",
        "his", "ye", "shall", "my", "this", "is", "which", "in", "to", "be", "for", "with",
    ];
    const SYLLABLES: &[&str] = &[
        "ab", "ra", "ham", "el", "i", "sa", "ac", "ja", "cob", "mo", "ses", "ben", "ja", "min",
        "na", "than", "she", "ba", "le", "vi", "ju", "dah", "gad", "ash", "er", "zeb", "u", "lun",
        "dan", "naph", "ta", "li", "jo", "seph", "re", "u", "ben", "sim", "e", "on",
    ];
    let mut m = Manifest::new(
        "bench-bible",
        "Bench Bible",
        "BB",
        ResourceType::Bible,
        "en",
    );
    m.key_schemes = vec![KeyScheme::Passage];
    m.license = "LicenseRef-PD".into();
    let mut b = ResourceBuilder::new(m);
    let mut seed: u64 = 0x5eed;
    let mut rand = move |n: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % n
    };
    for book in [
        Book::Genesis,
        Book::Exodus,
        Book::Leviticus,
        Book::Numbers,
        Book::Deuteronomy,
    ] {
        let id = UnitId::book(book);
        let mut t = TextAccumulator::new();
        let mut verses = Vec::new();
        let mut paras = Vec::new();
        for chapter in 1..=Standard.chapter_count(book) {
            let last = Standard.last_verse(book, chapter).unwrap_or(1);
            let poetry = chapter % 5 == 0;
            let mut para_start = t.pos();
            let mut in_para = 0;
            let mut para_len = 3 + rand(6);
            for v in 1..=last {
                if t.pos() > para_start {
                    t.push(" ");
                }
                let words = 12 + rand(49) as usize;
                let text: Vec<String> = (0..words)
                    .map(|_| {
                        if rand(10) < 6 {
                            WORDS[rand(WORDS.len() as u64) as usize].to_string()
                        } else {
                            (0..1 + rand(3))
                                .map(|_| SYLLABLES[rand(SYLLABLES.len() as u64) as usize])
                                .collect()
                        }
                    })
                    .collect();
                let r = t.push(&format!("{}.", text.join(" ")));
                verses.push((r.clone(), Verse::new(book, chapter, v)));
                in_para += 1;
                if poetry || in_para as u64 >= para_len || v == last {
                    let style = if poetry {
                        ParaStyle::Poetry1
                    } else {
                        ParaStyle::Prose
                    };
                    paras.push((para_start..r.end, style));
                    t.push("\n");
                    para_start = t.pos();
                    in_para = 0;
                    para_len = 3 + rand(6);
                }
            }
        }
        b.add_unit(
            id.clone(),
            UnitKind::BibleBook,
            Some(book.name().into()),
            t.into_string(),
        );
        for (r, style) in paras {
            b.add_span(&id, r, SpanKind::Para { style });
        }
        for (r, v) in verses {
            b.add_verse(&id, r, v, v.verse);
        }
    }
    match b.build() {
        Ok(r) => Arc::new(ReadingDoc::from_resource(r)),
        Err(e) => unreachable!("synthetic bible is invalid: {e}"),
    }
}
