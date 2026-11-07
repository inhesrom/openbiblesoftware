//! The scripted scroll/fling/jump benchmark behind `cargo xtask bench` (#19), ported from the
//! app-shell prototype's `OBS_BENCH` harness.
//!
//! It drives several reading panes headlessly, frame by frame at the display refresh rate,
//! doing what the UI thread does per frame (apply worker layouts, lay out within the budget,
//! build glyph-atlas quads) and timing it. GPU submission and egui tessellation are not
//! included; the prototype measured those as a small, constant share of the frame.
//!
//! Feature scenarios (#183) follow the reading ones: `interlinear` (every pane with
//! interlinear lines) and `find` (stepping a Find bar through matches far apart) on the same
//! panes, and the Shell-driven ones of [`tools`] (Atlas, Search Grid and Chart, Exegetical and
//! Topic Guides, Get Resources), each with its own settle-frame count.

#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod library;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod tools;

/// The Shell-driven feature scenarios, in report order.
pub const SHELL_SCENARIOS: [&str; 6] = [
    "atlas",
    "search-grid",
    "search-chart",
    "exegetical-guide",
    "topic-guide",
    "get-resources",
];

/// Most frames an action may take to show its result before the bench gives up on it.
pub(crate) const SETTLE_LIMIT: usize = 600;

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::shell::references::{TargetCache, TargetKey};
use obs_layout::TextSettings;
use obs_model::{
    KeyScheme, Manifest, ParaStyle, ResourceBuilder, ResourceType, SpanKind, TextAccumulator,
    UnitId, UnitKind,
};
use obs_render::GlyphAtlas;
use obs_scripture::{Book, Passage, Standard, Verse, VerseMap, VerseMapEntry, Versification};
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
    /// How many of the panes (the last ones) show interlinear lines under their Tagged
    /// words (#143): a reverse interlinear's Lemma, transliteration and Strong's number.
    pub interlinear_panes: usize,
    /// Run the Shell-driven feature scenarios ([`tools`], #183) after the reading ones.
    pub shell_scenarios: bool,
    /// Frames of each feature scenario's main interaction (scrolling, panning).
    pub tool_frames: usize,
    /// Actions of each feature scenario (Link-set moves, view switches, Find steps, keys).
    pub tool_steps: usize,
    /// Extra work in each Shell-driven scenario's feature Tab every frame, for checking that
    /// the gate catches synchronous work over budget that still fits in a frame (0 normally).
    pub feature_slowdown: Duration,
    /// Extra work in each Shell-driven scenario's feature Tab in each action's own frame only
    /// (a key typed into Get Resources' search, say), for checking that the gate catches
    /// synchronous work done only when an action is taken (0 normally).
    pub action_slowdown: Duration,
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
            interlinear_panes: 1,
            shell_scenarios: true,
            tool_frames: 120,
            tool_steps: 6,
            feature_slowdown: Duration::ZERO,
            action_slowdown: Duration::ZERO,
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
    /// Synchronous work per frame (#19: ≤ 2 ms): the reading panes' UI-thread layout, or for
    /// a Shell-driven scenario its feature Tab's drawing ([`tools`]).
    pub sync_layout_p50_ms: f64,
    pub sync_layout_p99_ms: f64,
    pub sync_layout_max_ms: f64,
    /// Feature scenarios: most frames any action took to show its result (0: its own frame).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settle_frames_max: Option<usize>,
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
    /// Find in Resource (#175): matches of "the" in the whole document, which every frame
    /// draws over each pane, and how long finding them took on one thread.
    pub find_matches: usize,
    pub find_ms: f64,
    /// Feature scenarios that could not be set up or run (the gate fails on any).
    pub feature_errors: Vec<String>,
}

#[derive(Default)]
pub(crate) struct Samples {
    cpu: Vec<f64>,
    sync: Vec<f64>,
    placeholder_frames: usize,
    /// Settle frames of each action of a feature scenario.
    settle: Vec<usize>,
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
    pub(crate) fn report(&self, name: &str, interval_ms: f64) -> ScenarioReport {
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
            settle_frames_max: self.settle.iter().copied().max(),
        }
    }
}

struct Bench {
    cfg: BenchConfig,
    views: Vec<ReadingView>,
    atlas: GlyphAtlas,
    snippets: crate::SnippetCache,
    reference_targets: TargetCache,
    reference_doc: Arc<ReadingDoc>,
    reference_frame: usize,
    /// Every pane's Find bar has these matches (#175); a frame draws those in view.
    found: Vec<crate::FindMatch>,
    /// Each pane's Visual filters (#211), in the `filters` scenario; none otherwise.
    filters: Vec<crate::filters::FilterPainter>,
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
            let found = crate::Found {
                matches: &self.found,
                current: Some(k),
            };
            std::hint::black_box(crate::pane::found_rects(v, found, self.cfg.viewport.1));
            // Visual filters (#211): the rectangles of the blocks in view's matches, as the
            // pane draws them; matching itself is on a worker.
            if let Some(painter) = self.filters.get_mut(k) {
                let started = Instant::now();
                std::hint::black_box(painter.rects(v, self.cfg.viewport.1, None));
                sync += started.elapsed();
                if painter.missing() > 0 {
                    placeholders[k] += 1;
                }
            }
        }
        // Real native-target translation: a whole-Bible Passage against >31k map entries.
        // Switch spans periodically to include cold submission as well as cached hover polls.
        let span = (self.reference_frame / 30) % 16;
        std::hint::black_box(self.reference_targets.get(TargetKey::new(
            &self.reference_doc,
            0,
            span,
        )));
        self.reference_frame += 1;
        // Eight Cross-reference row previews plus a changing Scripture-reference popup.
        // Exercise the same worker cache and atlas as the Tool while all panes scroll.
        let location = self.views.first_mut().and_then(|v| v.current_location());
        for row in 0..9 {
            let text = format!(
                "{location:?} {row}: For God so loved the world, that he gave his only begotten Son."
            );
            if let Some(laid) = self.snippets.get(
                &text,
                obs_layout::ColorRole::Ink,
                false,
                360.0,
                &TextSettings::default(),
            ) {
                std::hint::black_box(self.atlas.quads(&laid, [0.0, 0.0], self.cfg.scale));
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
            if self.views.iter().all(|v| !v.has_pending())
                && self.filters.iter().all(|f| !f.is_pending())
            {
                return;
            }
        }
    }
}

/// Run the benchmark on a document.
pub fn run(doc: Arc<ReadingDoc>, cfg: BenchConfig) -> BenchReport {
    let interval = Duration::from_secs_f64(1.0 / f64::from(cfg.refresh_hz));
    let panes = cfg.panes.max(1);
    let kind = obs_engine::InterlinearKind::Reverse;
    let interlinear =
        crate::InterlinearSource::new(kind, kind.default_lines().to_vec(), vec![], vec![])
            .map(Arc::new);
    let mut views: Vec<ReadingView> = (0..panes)
        .map(|k| {
            let mut v = ReadingView::new(Arc::clone(&doc), TextSettings::default());
            v.set_width(cfg.viewport.0 - 2.0 * crate::view::MARGIN);
            if k + cfg.interlinear_panes >= panes {
                v.set_interlinear(interlinear.clone());
            }
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
    // Find "the" in the whole document, as a Find bar's worker does.
    let find_start = Instant::now();
    let found: Vec<crate::FindMatch> = obs_search::Finder::new("the", Default::default())
        .map(|finder| {
            doc.units
                .iter()
                .enumerate()
                .flat_map(|(unit, u)| {
                    finder
                        .find(&u.text)
                        .into_iter()
                        .map(move |range| crate::FindMatch {
                            unit,
                            row: 0,
                            range,
                        })
                })
                .collect()
        })
        .unwrap_or_default();
    let find_ms = find_start.elapsed().as_secs_f64() * 1000.0;
    let mut bench = Bench {
        cfg: cfg.clone(),
        views,
        atlas: GlyphAtlas::new(),
        snippets: crate::SnippetCache::default(),
        reference_targets: TargetCache::default(),
        reference_doc: large_reference_doc(),
        reference_frame: 0,
        found,
        filters: Vec::new(),
        interval,
        budget: SYNC_BUDGET,
    };
    let interval_ms = interval.as_secs_f64() * 1000.0;
    bench.settle();
    bench.reference_targets = TargetCache::default();
    bench.reference_frame = 0;

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

    let mut scenarios = vec![
        steady.report("steady", interval_ms),
        fling.report("fling", interval_ms),
        jumps.report("jumps", interval_ms),
    ];
    let mut all_sync: Vec<f64> = [&steady, &fling, &jumps]
        .iter()
        .flat_map(|s| s.sync.iter().copied())
        .collect();
    let lines = interlinear_scenario(&mut bench, interlinear.as_ref());
    let find = find_scenario(&mut bench);
    let filters = filters_scenario(&mut bench);
    all_sync.extend(
        (lines.sync.iter())
            .chain(&find.sync)
            .chain(&filters.sync)
            .copied(),
    );
    scenarios.push(lines.report("interlinear", interval_ms));
    scenarios.push(find.report("find", interval_ms));
    scenarios.push(filters.report("filters", interval_ms));
    let mut feature_errors = Vec::new();
    if cfg.shell_scenarios {
        // The reading panes' layouts are dropped first: the Shell lays out its own.
        bench.views.clear();
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (reports, errors) = tools::run(&cfg);
            scenarios.extend(reports);
            feature_errors = errors;
        }
        #[cfg(target_arch = "wasm32")]
        feature_errors.push("the Shell scenarios write a library to disk".into());
    }
    BenchReport {
        refresh_hz: cfg.refresh_hz,
        sync_layout_p99_ms: percentile(&all_sync, 0.99),
        panes,
        blocks: doc.blocks.len(),
        scenarios,
        jump_origin_settle_frames_max: origin_max,
        jump_all_settle_frames_max: all_max,
        text_gpu_mib: (bench.atlas.bytes() * 4) as f64 / (1024.0 * 1024.0),
        shape_cache_hit_rate: obs_fonts::ShapeCache::shared().stats().hit_rate(),
        find_matches: bench.found.len(),
        find_ms,
        feature_errors,
    }
}

/// Interlinear lines (#143) under every pane's Tagged words: steady scrolling, as in
/// `steady`, with twice the lines to lay out.
fn interlinear_scenario(
    bench: &mut Bench,
    interlinear: Option<&Arc<crate::InterlinearSource>>,
) -> Samples {
    for v in &mut bench.views {
        v.set_interlinear(interlinear.cloned());
    }
    bench.settle();
    let mut s = Samples::default();
    let dy = bench.cfg.steady_px;
    for _ in 0..bench.cfg.steady_frames / 2 {
        bench.frame(&mut s, |views| {
            views.iter_mut().for_each(|v| v.scroll_by(dy))
        });
    }
    for v in &mut bench.views {
        v.set_interlinear(None);
    }
    s
}

/// Three Visual filters (#211) on every pane, scrolling as in `steady`: Divine names (about
/// one Tagged word in six), Imperatives (aligned through Strong's numbers: the synthetic Bible
/// is English) and a phrase in Background. Each block's matches are found on a worker; a frame
/// that draws a block in view without its matches counts as a placeholder frame.
fn filters_scenario(bench: &mut Bench) -> Samples {
    let mut filters = obs_engine::built_in_filters();
    filters.retain(|f| f.id != "words-of-christ");
    filters.push(obs_engine::VisualFilter {
        id: "said-unto".into(),
        name: "Said unto".into(),
        query: "\"said unto\"".into(),
        marked: None,
        scope: String::new(),
        look: obs_engine::HighlightLook::Background,
        color: obs_engine::Rgb::new(255, 236, 140),
        all_tabs: true,
    });
    bench.filters = (0..bench.views.len())
        .map(|k| {
            let mut painter = crate::filters::FilterPainter::default();
            let source = crate::filters::FilterSource::new(filters.clone(), vec![], vec![]);
            painter.set(Some((k as u64, source)));
            painter
        })
        .collect();
    bench.settle();
    let mut s = Samples::default();
    let dy = bench.cfg.steady_px;
    for _ in 0..bench.cfg.steady_frames / 2 {
        bench.frame(&mut s, |views| {
            views.iter_mut().for_each(|v| v.scroll_by(dy))
        });
    }
    bench.filters.clear();
    s
}

/// A word the synthetic Bible has a few dozen times, far apart: each Find step jumps.
pub(crate) const FIND_SPARSE: &str = "moses";

/// Find in Resource (#175) in the first pane: typing the word, then Find next through the
/// matches, each far from the last. A step's settle frames count until the pane has no
/// placeholders, as a jump's origin (#19: ≤ 1 frame).
fn find_scenario(bench: &mut Bench) -> Samples {
    let ctx = egui::Context::default();
    let viewport = bench.cfg.viewport.1;
    let mut find = crate::find::FindBar::default();
    find.text = FIND_SPARSE.into();
    bench.settle();
    let mut s = Samples::default();
    // Typing: the whole document is searched on a worker (like a search, not a jump: its
    // time is `find_ms`), then the pane goes to the first match.
    for _ in 0..SETTLE_LIMIT {
        let placeholders = bench.frame(&mut s, |views| {
            find.update_reading(&mut views[0], viewport, &ctx);
        });
        if find.current.is_some() && placeholders[0] == 0 {
            break;
        }
    }
    if find.current.is_none() {
        s.settle.push(SETTLE_LIMIT);
        return s;
    }
    for _ in 0..bench.cfg.tool_steps.max(bench.cfg.jumps) {
        bench.settle();
        find.step = Some(true);
        let mut placeholders = bench.frame(&mut s, |views| {
            find.update_reading(&mut views[0], viewport, &ctx);
        });
        let mut frames = 0;
        while placeholders[0] > 0 && frames < SETTLE_LIMIT {
            frames += 1;
            placeholders = bench.frame(&mut s, |views| {
                find.update_reading(&mut views[0], viewport, &ctx);
            });
        }
        s.settle.push(frames);
    }
    s
}

/// A deterministic Bible-shaped document for CI, where no real Bible is available: Genesis to
/// Deuteronomy chapter and verse counts, prose paragraphs of 3–8 verses, every fifth chapter
/// poetry, verses of 12–60 words. About seven words in ten are Tagged words with a Hebrew
/// Lemma, as in an English Bible with Strong's numbers (for interlinear lines, #143).
pub fn synthetic_bible() -> Arc<ReadingDoc> {
    match synthetic_bible_resource() {
        Some(r) => Arc::new(ReadingDoc::from_resource(r)),
        None => unreachable!("synthetic bible is invalid"),
    }
}

/// [`synthetic_bible`] as a Resource (`bench-bible`), for the feature scenarios' libraries.
pub(crate) fn synthetic_bible_resource() -> Option<obs_model::Resource> {
    // Strong's number, Lemma and Gloss.
    const LEMMAS: &[(&str, &str, &str)] = &[
        ("H1254", "בָּרָא", "to create"),
        ("H430", "אֱלֹהִים", "God"),
        ("H776", "אֶרֶץ", "land"),
        ("H8064", "שָׁמַיִם", "heavens"),
        ("H120", "אָדָם", "man"),
        ("H3117", "יוֹם", "day"),
        ("H216", "אוֹר", "light"),
        ("H4325", "מַיִם", "water"),
        ("H559", "אָמַר", "to say"),
        ("H3068", "יְהוָה", "LORD"),
        ("H1121", "בֵּן", "son"),
        ("H3605", "כֹּל", "all"),
    ];
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
        let mut tagged = Vec::new();
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
                let mut at = r.start;
                for w in &text {
                    // From the word's position, not `rand`: the text stays as it was.
                    let mix = (at as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 32;
                    if mix % 10 < 7 {
                        let (strongs, lemma, gloss) = LEMMAS[(mix / 10) as usize % LEMMAS.len()];
                        let mut part = obs_model::MorphemePart::new(
                            strongs
                                .parse()
                                .unwrap_or_else(|_| unreachable!("{strongs}")),
                        );
                        part.lemma = Some(lemma.into());
                        part.gloss = Some(gloss.into());
                        tagged.push((at..at + w.len(), part));
                    }
                    at += w.len() + 1;
                }
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
        for (r, part) in tagged {
            b.add_span(&id, r, SpanKind::TaggedWord { parts: vec![part] });
        }
    }
    b.build().ok()
}

/// Small readable fixture with a full-size native Verse map and whole-Bible references.
/// Shared by the scroll benchmark and the hover regression so both hit the shell's cache.
pub(crate) fn large_reference_doc() -> Arc<ReadingDoc> {
    let Ok(passage) = Passage::new(
        Verse::new(Book::Genesis, 1, 1),
        Verse::new(Book::Revelation, 22, 21),
    ) else {
        unreachable!("Genesis precedes Revelation")
    };
    let manifest = Manifest::new(
        "large-reference",
        "Large reference",
        "LR",
        ResourceType::Bible,
        "en",
    );
    let mut builder = ResourceBuilder::new(manifest);
    let unit = UnitId::book(Book::Genesis);
    builder.add_unit(
        unit.clone(),
        UnitKind::BibleBook,
        None,
        "Whole Bible".into(),
    );
    builder.add_verse(&unit, 0..11, passage.start(), 1);
    let mut doc = match builder.build() {
        Ok(r) => ReadingDoc::from_resource(r),
        Err(e) => unreachable!("large reference fixture is invalid: {e}"),
    };
    doc.units[0].spans = (0..16)
        .map(|_| obs_model::Span {
            start: 0,
            end: 11,
            kind: SpanKind::ScriptureRef {
                targets: vec![passage].into(),
            },
        })
        .collect();
    doc.versification =
        obs_scripture::ListedVersification::new(passage.verses(&Standard).map(|v| (v, v.verse)));
    doc.verse_map = Some(Arc::new(VerseMap::new(
        passage
            .verses(&Standard)
            .map(|v| VerseMapEntry {
                verse: v,
                last: v.verse,
                standard: Some(Passage::verse(v)),
            })
            .collect(),
        vec![],
        &Standard,
    )));
    Arc::new(doc)
}
