//! The bench's feature scenarios that drive the whole Shell (#183): the Atlas map, the Search
//! Tool's Grid and Chart views, the Exegetical and Topic Guides and Get Resources' catalog
//! filtering. Each builds a library of synthetic Resources ([`super::library`]) in a temporary
//! folder, opens the feature as a user would (commands, pointer and keys through a headless
//! egui context), waits untimed until it has its data, then times what the user does with it
//! frame by frame at the display refresh rate.
//!
//! A frame's time is the UI thread's: the Shell's whole frame (`Shell::ui`, every Panel) and
//! egui's tessellation, as eframe does them before handing the GPU its meshes. GPU work is not
//! included. Settle frames count frames after an action (a Link set moving, a view switched,
//! a key typed) until the feature shows its result; 0 is the action's own frame.
//!
//! A frame's synchronous feature work (its `sync` sample, held to #19's 2 ms per frame) is
//! the feature Tab's own drawing on the UI thread: its body, egui text layout included, timed
//! in the Shell's pane drawing ([`crate::shell::TimedTab`]). Every timed frame must draw the
//! Tab. The frames from an action whose result is made off the UI thread or drawn afresh (a
//! Link set moving, a view switched) until it shows are exempt, as #19 exempts the block a jump
//! lands in: they are held to the settle and missed-frame budgets instead. An action done on
//! the UI thread in its own frame (Get Resources filtering its catalog) is not exempt
//! ([`ActionWork`]).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use egui::{Event, Modifiers, Pos2, RawInput, Rect, Shape, Vec2, pos2, vec2};
use obs_engine::get_bibles::ResourceFilter;
use obs_engine::{JobMode, Library, Session, TabId, TabKind};
use obs_model::{Resource, ResourceType};
use obs_scripture::{Book, Passage, RefStyle, Standard, format_passage};
use serde_json::json;

use super::library;
use super::{BenchConfig, SETTLE_LIMIT, Samples, ScenarioReport};
use crate::Shell;

/// The window, in points (the shell tests' size).
const SCREEN: Vec2 = vec2(1400.0, 900.0);

/// Device pixels per point, as in the reading bench.
const PIXELS_PER_POINT: f32 = 2.0;

/// Longest a scenario's untimed setup may take (Search indexes, first loads).
const SETUP_LIMIT: Duration = Duration::from_secs(180);

/// Removes the scenario's library folder when the scenario ends.
struct Folder(PathBuf);

/// Numbers the scenario folders made in this process, so scenarios run at once (tests in
/// parallel threads, one scenario twice) never share, and so never delete, each other's folder.
static FOLDERS_MADE: AtomicU64 = AtomicU64::new(0);

impl Folder {
    /// A fresh folder of its own for a run of the scenario `name`: named for the scenario,
    /// this process and this run.
    fn new(name: &str) -> Self {
        let run = FOLDERS_MADE.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("obs-bench-{name}-{}-{run}", std::process::id()));
        // A leftover from an earlier process that reused this PID.
        let _ = std::fs::remove_dir_all(&root);
        Self(root)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The Shell in a headless egui context, timed frame by frame.
pub(super) struct ShellBench {
    pub(super) ctx: egui::Context,
    pub(super) shell: Shell,
    time: f64,
    pub(super) events: Vec<Event>,
    interval: Duration,
    pace: bool,
    /// Texts painted last frame, with where.
    pub(super) texts: Vec<(Rect, String)>,
    /// Seeded extra work in the feature Tab's body ([`BenchConfig::feature_slowdown`]).
    slowdown: Duration,
    /// Seeded extra work in the feature Tab's body in each action's own frame only
    /// ([`BenchConfig::action_slowdown`]).
    action_slowdown: Duration,
    /// Timed frames in which the feature Tab was not drawn.
    undrawn_frames: usize,
    /// When the last frame's work ended, before any pacing.
    frame_end: Instant,
    /// The next frame is an action's own ([`Self::settle`]).
    action_frame: bool,
    /// The frames from an action whose result is exempt from the synchronous-work budget
    /// ([`ActionWork::Exempt`]) until it shows.
    in_exempt_action: bool,
    /// Kept until the Shell is dropped (it may hold its files open); `None` for a Shell the
    /// scenario made itself ([`Self::over`]).
    _folder: Option<Folder>,
}

fn collect_texts(shape: &Shape, out: &mut Vec<(Rect, String)>) {
    match shape {
        Shape::Vec(v) => v.iter().for_each(|s| collect_texts(s, out)),
        Shape::Text(t) => out.push((t.visual_bounding_rect(), t.galley.text().to_string())),
        _ => {}
    }
}

impl ShellBench {
    /// A Shell over a library of `resources` (in this Preferred order) written to a fresh
    /// folder of its own ([`Folder::new`]), in the default layout.
    pub(super) fn new(
        cfg: &BenchConfig,
        name: &str,
        resources: &[Resource],
        session: impl FnOnce(&mut Session, &Path),
    ) -> Result<Self, String> {
        let folder = Folder::new(name);
        let root = folder.0.clone();
        let dir = root.join("library");
        library::write_library(&dir, resources)?;
        let mut s = Session::new(JobMode::default_for_host());
        s.library = Library::new();
        if let Some((path, e)) = s.library.scan(&dir).into_iter().next() {
            return Err(format!("{}: {e}", path.display()));
        }
        s.library.set_preferred_order(
            resources
                .iter()
                .map(|r| r.manifest.resource_id.clone())
                .collect(),
        );
        session(&mut s, &root);
        let mut shell = Shell::new(s);
        shell.reset_layout();
        let mut b = Self::over(cfg, shell);
        b._folder = Some(folder);
        for _ in 0..3 {
            b.frame(None, |_| {});
        }
        Ok(b)
    }

    /// A Shell the scenario made (the app's own launch, say), before its first frame.
    pub(super) fn over(cfg: &BenchConfig, mut shell: Shell) -> Self {
        // Motion (#225) as the bench is set: on, unless it measures Reduce motion.
        shell.st.session.appearance.reduce_motion = !cfg.motion;
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        Self {
            ctx,
            shell,
            time: 0.0,
            events: Vec::new(),
            interval: Duration::from_secs_f64(1.0 / f64::from(cfg.refresh_hz)),
            pace: cfg.pace,
            texts: Vec::new(),
            slowdown: cfg.feature_slowdown,
            action_slowdown: cfg.action_slowdown,
            undrawn_frames: 0,
            frame_end: Instant::now(),
            action_frame: false,
            in_exempt_action: false,
            _folder: None,
        }
    }

    /// One frame, with `action` (a command, say) done first, inside the timed work; timed into
    /// `samples` when given. Paced to the refresh rate when the bench is.
    pub(super) fn frame(&mut self, samples: Option<&mut Samples>, action: impl FnOnce(&mut Shell)) {
        let mut viewports = egui::ViewportIdMap::default();
        viewports.insert(
            egui::ViewportId::ROOT,
            egui::ViewportInfo {
                native_pixels_per_point: Some(PIXELS_PER_POINT),
                ..Default::default()
            },
        );
        let input = RawInput {
            viewports,
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)),
            time: Some(self.time),
            events: std::mem::take(&mut self.events),
            ..Default::default()
        };
        self.time += self.interval.as_secs_f64();
        self.shell.st.timed_tab.took = None;
        self.shell.st.timed_tab.slowdown = if std::mem::take(&mut self.action_frame) {
            self.slowdown + self.action_slowdown
        } else {
            self.slowdown
        };
        let start = Instant::now();
        action(&mut self.shell);
        let shell = &mut self.shell;
        let mut out = self.ctx.run_ui(input, |ui| shell.ui(ui));
        let ui_time = start.elapsed();
        // No GPU: texture uploads are dropped.
        out.textures_delta.clear();
        self.texts.clear();
        for c in &out.shapes {
            collect_texts(&c.shape, &mut self.texts);
        }
        let start = Instant::now();
        std::hint::black_box(self.ctx.tessellate(out.shapes, out.pixels_per_point));
        let elapsed = ui_time + start.elapsed();
        self.frame_end = Instant::now();
        let feature = self.shell.st.timed_tab.took.take();
        if let Some(s) = samples {
            s.cpu.push(elapsed.as_secs_f64() * 1000.0);
            match feature {
                Some(_) if self.in_exempt_action => {}
                Some(d) => s.sync.push(d.as_secs_f64() * 1000.0),
                None => self.undrawn_frames += 1,
            }
        }
        if self.pace && elapsed < self.interval {
            std::thread::sleep(self.interval - elapsed);
        }
    }

    /// Time Tab `tab`'s drawing from now on: the scenario's synchronous feature work.
    pub(super) fn time_tab(&mut self, tab: TabId) {
        self.shell.st.timed_tab.tab = Some(tab);
    }

    /// The scenario's samples, once every timed frame drew the feature Tab: a scenario whose
    /// synchronous work went unmeasured must not pass.
    pub(super) fn measured(&self, s: Samples) -> Result<Samples, String> {
        if self.undrawn_frames > 0 || s.sync.is_empty() {
            return Err(format!(
                "the feature's Tab was not drawn in {} of {} timed frames",
                self.undrawn_frames,
                s.cpu.len()
            ));
        }
        Ok(s)
    }

    /// Timed frames until `ready`, after `action`: how many frames after the action's own
    /// ([`SETTLE_LIMIT`] if never). `work` says whether those frames are held to the
    /// synchronous-work budget.
    pub(super) fn settle(
        &mut self,
        samples: &mut Samples,
        work: ActionWork,
        action: impl FnOnce(&mut Shell),
        ready: impl Fn(&Self) -> bool,
    ) -> usize {
        self.in_exempt_action = work == ActionWork::Exempt;
        self.action_frame = true;
        self.frame(Some(samples), action);
        let mut frames = SETTLE_LIMIT;
        for frame in 0..SETTLE_LIMIT {
            if ready(self) {
                frames = frame;
                break;
            }
            self.frame(Some(samples), |_| {});
        }
        self.in_exempt_action = false;
        frames
    }

    /// [`Self::settle`], also timed in milliseconds: from the action's frame starting to the end
    /// of the frame that shows its result (pacing included, as the user waits it out). Both
    /// are recorded in `samples`.
    pub(super) fn settle_timed(
        &mut self,
        samples: &mut Samples,
        work: ActionWork,
        action: impl FnOnce(&mut Shell),
        ready: impl Fn(&Self) -> bool,
    ) {
        let start = Instant::now();
        let frames = self.settle(samples, work, action, ready);
        samples.settle.push(frames);
        let ms = self.frame_end.duration_since(start).as_secs_f64() * 1000.0;
        samples.action_ms.push(ms);
    }

    /// A timed frame on the way to an action's result (a key repeat before the last one, say):
    /// exempt from the synchronous-work budget, as [`ActionWork::Exempt`] frames are.
    pub(super) fn exempt_frame(&mut self, samples: &mut Samples) {
        self.in_exempt_action = true;
        self.frame(Some(samples), |_| {});
        self.in_exempt_action = false;
    }

    /// Untimed frames until `ready` (the scenario's setup).
    pub(super) fn wait(&mut self, what: &str, ready: impl Fn(&Self) -> bool) -> Result<(), String> {
        let start = Instant::now();
        while start.elapsed() < SETUP_LIMIT {
            self.frame(None, |_| {});
            if ready(self) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(4));
        }
        let shown: Vec<&str> = self.texts.iter().map(|(_, t)| t.as_str()).collect();
        Err(format!("never saw {what}; showing {shown:?}"))
    }

    pub(super) fn has_text(&self, text: &str) -> bool {
        self.texts.iter().any(|(_, t)| t == text)
    }

    pub(super) fn has_text_starting(&self, prefix: &str) -> bool {
        self.texts.iter().any(|(_, t)| t.starts_with(prefix))
    }

    pub(super) fn text_at(&self, text: &str) -> Option<Pos2> {
        self.texts
            .iter()
            .find(|(_, t)| t == text)
            .map(|(r, _)| r.center())
    }

    /// Where Tab `tab`'s pane is drawn.
    pub(super) fn pane(&self, tab: TabId) -> Option<Rect> {
        let tiles = &self.shell.tree.tiles;
        tiles.find_pane(&tab).and_then(|id| tiles.rect(id))
    }

    /// The first Tab of `kind`.
    pub(super) fn tab_of(&self, kind: &TabKind) -> Option<TabId> {
        self.shell
            .session()
            .workspace
            .tabs()
            .find(|t| t.kind == *kind)
            .map(|t| t.id)
    }

    /// Run a command whose answer names a Tab (`{"tab": n}`).
    pub(super) fn open(
        &mut self,
        command: &str,
        params: &serde_json::Value,
    ) -> Result<TabId, String> {
        self.shell
            .run(command, params)
            .and_then(|v| v["tab"].as_u64())
            .map(TabId)
            .ok_or_else(|| format!("{command} opened no Tab"))
    }

    /// `frames` timed frames of the wheel over `at`, `dy` points a frame (negative: down).
    pub(super) fn wheel(&mut self, samples: &mut Samples, at: Pos2, dy: f32, frames: usize) {
        for _ in 0..frames {
            self.events.push(Event::PointerMoved(at));
            self.events.push(Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, dy),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Wheel,
                modifiers: Modifiers::NONE,
            });
            self.frame(Some(samples), |_| {});
        }
    }

    /// Click `text` over untimed frames, once scrolling has come to rest.
    pub(super) fn click_text(&mut self, text: &str) -> Result<(), String> {
        for _ in 0..30 {
            self.frame(None, |_| {});
        }
        let at = self.text_at(text).ok_or_else(|| format!("no {text:?}"))?;
        self.click(at);
        Ok(())
    }

    /// A click at `at` over untimed frames (focusing a field, say).
    pub(super) fn click(&mut self, at: Pos2) {
        self.events.push(Event::PointerMoved(at));
        self.frame(None, |_| {});
        for pressed in [true, false] {
            self.events.push(Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            self.frame(None, |_| {});
        }
    }
}

/// How the frames from an action until its result shows count toward #19's synchronous-work
/// budget (2 ms a frame).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ActionWork {
    /// The result is made off the UI thread (a worker finds a chapter's places, a Guide reads
    /// its Passage) or drawn afresh (a Search view switched): exempt, as #19 exempts the
    /// block a jump lands in, and held to the settle budget instead.
    Exempt,
    /// The result is made on the UI thread in the action's own frame (Get Resources filters
    /// and sorts its catalog as a key is typed): held to the 2 ms budget like any frame.
    Budgeted,
}

/// Run every feature scenario: the reports of those that ran, and why the others could not
/// (the gate fails on any: it must not pass on a scenario that measured nothing).
pub(crate) fn run(cfg: &BenchConfig) -> (Vec<ScenarioReport>, Vec<String>) {
    let interval_ms = 1000.0 / f64::from(cfg.refresh_hz);
    let mut reports = Vec::new();
    let mut errors = Vec::new();
    let mut add = |name: &str, s: Result<Samples, String>| match s {
        Ok(s) => reports.push(s.report(name, interval_ms)),
        Err(e) => errors.push(format!("{name}: {e}")),
    };
    add("atlas", atlas(cfg));
    match search_views(cfg) {
        Ok((grid, chart)) => {
            add("search-grid", Ok(grid));
            add("search-chart", Ok(chart));
        }
        Err(e) => {
            add("search-grid", Err(e.clone()));
            add("search-chart", Err(e));
        }
    }
    add("exegetical-guide", exegetical_guide(cfg));
    add("topic-guide", topic_guide(cfg));
    add("get-resources", get_resources(cfg));
    add("text-size", super::text_size::text_size(cfg));
    add("text-size-tools", super::text_size::text_size_tools(cfg));
    (reports, errors)
}

pub(super) fn built(r: Option<Resource>, what: &str) -> Result<Resource, String> {
    r.ok_or_else(|| format!("the synthetic {what} is invalid"))
}

/// The reading bench's Bible as a Resource (Genesis to Deuteronomy, Tagged words).
fn torah_bible() -> Result<Resource, String> {
    built(super::synthetic_bible_resource(), "Bible")
}

fn chapter_label(book: Book, chapter: u16) -> String {
    Passage::chapter(&Standard, book, chapter)
        .map(|p| format_passage(p, &Standard, RefStyle::Short))
        .unwrap_or_default()
}

/// The Atlas beside a Bible in Link set A, on a chapter naming 120 places: the map panned with
/// the pointer and zoomed with the wheel, then the Bible moved to other chapters (the Atlas
/// follows, finding each chapter's places on a worker).
fn atlas(cfg: &BenchConfig) -> Result<Samples, String> {
    let resources = [torah_bible()?, built(library::places(), "places")?];
    let mut b = ShellBench::new(cfg, "atlas", &resources, |_, _| {})?;
    let bible = b
        .tab_of(&TabKind::Resource("bench-bible".into()))
        .ok_or("no Bible Tab")?;
    let busy = library::BUSY_CHAPTER;
    b.shell.run(
        "tab.go",
        &json!({ "tab": bible.0, "text": format!("Gen {busy}:1") }),
    );
    let atlas = b.open("tool.atlas", &json!({ "split": true }))?;
    b.time_tab(atlas);
    b.shell
        .run("tab.link", &json!({ "tab": atlas.0, "set": "A" }));
    let status = format!("{} · ", chapter_label(Book::Genesis, busy));
    b.wait("the Atlas's places", |b| b.has_text_starting(&status))?;
    let map = b.pane(atlas).ok_or("the Atlas is not drawn")?;
    let center = pos2(map.left() + map.width() * 0.35, map.center().y);
    let mut s = Samples::default();
    // Pan: drag across the map and back.
    b.events.push(Event::PointerMoved(center));
    b.events.push(Event::PointerButton {
        pos: center,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    b.frame(Some(&mut s), |_| {});
    let mut at = center;
    for f in 0..cfg.tool_frames {
        let step = if (f / 30) % 2 == 0 { 3.0 } else { -3.0 };
        at += vec2(step, step * 0.5);
        b.events.push(Event::PointerMoved(at));
        b.frame(Some(&mut s), |_| {});
    }
    b.events.push(Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    b.frame(Some(&mut s), |_| {});
    // Zoom in and out with the wheel.
    let half = cfg.tool_frames / 4;
    b.wheel(&mut s, center, 40.0, half);
    b.wheel(&mut s, center, -40.0, half);
    // Follow the Link set to other chapters.
    let chapters = [
        (Book::Genesis, 1),
        (Book::Exodus, busy),
        (Book::Leviticus, 3),
        (Book::Numbers, busy),
        (Book::Deuteronomy, 2),
        (Book::Genesis, busy),
    ];
    for &(book, chapter) in chapters.iter().cycle().take(cfg.tool_steps) {
        let status = format!("{} · ", chapter_label(book, chapter));
        let go = format!("{} {chapter}:1", book.name());
        let frames = b.settle(
            &mut s,
            ActionWork::Exempt,
            |shell| {
                shell.run("tab.go", &json!({ "tab": bible.0, "text": go }));
            },
            |b| b.has_text_starting(&status),
        );
        s.settle.push(frames);
    }
    b.measured(s)
}

/// A whole-Bible search across three Bibles (every book has hits), shown as the Grid (books ×
/// Bibles) and the Chart (bars by book, then by chapter of Psalms), each scrolled.
fn search_views(cfg: &BenchConfig) -> Result<(Samples, Samples), String> {
    let ids = ["bench-a", "bench-b", "bench-c"];
    let resources = ids
        .iter()
        .zip(["BA", "BB", "BC"])
        .zip(1u64..)
        .map(|((id, abbreviation), seed)| {
            built(
                library::english_bible(id, abbreviation, &Book::ALL, 6..18, false, seed),
                "whole Bible",
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut b = ShellBench::new(cfg, "search", &resources, |s, root| {
        s.set_search_cache_dir(root.join("cache"));
        s.refresh_search_indexes();
    })?;
    b.wait("the Search indexes", |b| {
        let s = b.shell.session();
        !s.search.is_building() && ids.iter().all(|id| s.search.is_ready(id))
    })?;
    let tab = b.open(
        "search.run",
        &json!({
            "text": "LORD",
            "bibles": ids.join(","),
            "scope": "Whole Bible",
            "split": true,
        }),
    )?;
    b.time_tab(tab);
    b.wait("the search results", |b| {
        let s = b.shell.session();
        !s.bible_search_running()
            && s.search_tab(tab)
                .is_some_and(|t| t.all_results().is_some_and(|r| !r.rows.is_empty()))
    })?;
    let pane = b.pane(tab).ok_or("the Search Tab is not drawn")?;
    let over = pos2(pane.center().x, pane.top() + pane.height() * 0.7);
    let half = cfg.tool_frames / 2;

    let mut grid = Samples::default();
    for _ in 0..cfg.tool_steps.div_ceil(2) {
        let frames = b.settle(
            &mut grid,
            ActionWork::Exempt,
            |shell| {
                shell.run("search.view", &json!({ "tab": tab.0, "value": "grid" }));
            },
            |b| b.has_text("Bible book"),
        );
        grid.settle.push(frames);
        b.wheel(&mut grid, over, -30.0, half);
        b.wheel(&mut grid, over, 30.0, half);
        // Away and back: each switch draws the view afresh.
        b.shell
            .run("search.view", &json!({ "tab": tab.0, "value": "verses" }));
        b.frame(None, |_| {});
    }

    let mut chart = Samples::default();
    for _ in 0..cfg.tool_steps.div_ceil(2) {
        let frames = b.settle(
            &mut chart,
            ActionWork::Exempt,
            |shell| {
                shell.run("search.view", &json!({ "tab": tab.0, "value": "chart" }));
                shell.run("search.chart", &json!({ "tab": tab.0, "value": "books" }));
            },
            |b| b.has_text("By book"),
        );
        chart.settle.push(frames);
        b.wheel(&mut chart, over, -30.0, half / 2);
        b.wheel(&mut chart, over, 30.0, half / 2);
        let frames = b.settle(
            &mut chart,
            ActionWork::Exempt,
            |shell| {
                shell.run("search.chart", &json!({ "tab": tab.0, "value": "Psalms" }));
            },
            |b| b.has_text("Chapters of Psalms"),
        );
        chart.settle.push(frames);
        b.wheel(&mut chart, over, -30.0, half / 2);
        b.wheel(&mut chart, over, 30.0, half / 2);
        b.shell
            .run("search.view", &json!({ "tab": tab.0, "value": "verses" }));
        b.frame(None, |_| {});
    }
    let grid = b.measured(grid)?;
    Ok((grid, b.measured(chart)?))
}

/// The Exegetical Guide on a whole chapter of the Hebrew Torah (Genesis 24: 67 verses, about
/// 1,300 words) scrolled from top to bottom, then moved to other chapters.
fn exegetical_guide(cfg: &BenchConfig) -> Result<Samples, String> {
    let hebrew = built(library::hebrew_bible(), "Hebrew Bible")?;
    let chapters = [
        (Book::Numbers, 7),
        (Book::Exodus, 20),
        (Book::Genesis, 1),
        (Book::Deuteronomy, 28),
    ];
    // What the Word by word header counts, chapter by chapter: it shows once the words are in.
    let counts: Vec<String> = chapters
        .iter()
        .map(|&(book, chapter)| library::tagged_words(&hebrew, book, chapter).to_string())
        .collect();
    let resources = [
        torah_bible()?,
        hebrew,
        built(library::lexicon(), "Lexicon")?,
    ];
    let mut b = ShellBench::new(cfg, "exegetical", &resources, |_, _| {})?;
    let bible = b
        .tab_of(&TabKind::Resource("bench-bible".into()))
        .ok_or("no Bible Tab")?;
    b.shell
        .run("tab.go", &json!({ "tab": bible.0, "text": "Gen 24:1" }));
    let guide = b.open(
        "guide.exegetical_guide",
        &json!({ "passage": "Gen 24", "split": true }),
    )?;
    b.time_tab(guide);
    b.wait("Genesis 24 word by word", |b| {
        !b.shell.session().guides.is_busy() && b.has_text("Genesis 24:1 (BHOT)")
    })?;
    let pane = b.pane(guide).ok_or("the Guide is not drawn")?;
    let over = pane.center();
    let mut s = Samples::default();
    b.wheel(&mut s, over, -60.0, cfg.tool_frames);
    b.wheel(&mut s, over, 60.0, cfg.tool_frames / 2);
    for k in 0..cfg.tool_steps {
        let (book, chapter) = chapters[k % chapters.len()];
        let count = &counts[k % chapters.len()];
        let label = format!("{} {chapter}", book.name());
        let go = chapter_label(book, chapter);
        let frames = b.settle(
            &mut s,
            ActionWork::Exempt,
            |shell| {
                shell.run("tab.go", &json!({ "tab": guide.0, "text": go }));
            },
            |b| !b.shell.session().guides.is_busy() && b.has_text(&label) && b.has_text(count),
        );
        s.settle.push(frames);
        b.wheel(&mut s, over, -60.0, cfg.tool_frames / 4);
    }
    b.measured(s)
}

/// The Topic Guide on a long topic (Prayer: 60 subtopics, 1,200 Passages in each of two
/// topical indexes) scrolled, then moved between topics.
fn topic_guide(cfg: &BenchConfig) -> Result<Samples, String> {
    let resources = [
        torah_bible()?,
        built(library::topical_index("nave"), "Nave's")?,
        built(library::topical_index("torrey"), "Torrey's")?,
        built(library::dictionary(), "Dictionary")?,
    ];
    let mut b = ShellBench::new(cfg, "topic", &resources, |_, _| {})?;
    let (first, _, _) = library::LONG_TOPICS[0];
    let guide = b.open(
        "guide.topic_guide",
        &json!({ "topic": first, "split": true }),
    )?;
    b.time_tab(guide);
    b.wait("the Prayer topic", |b| {
        !b.shell.session().guides.is_busy() && b.has_text(first)
    })?;
    let pane = b.pane(guide).ok_or("the Guide is not drawn")?;
    let over = pane.center();
    let mut s = Samples::default();
    b.wheel(&mut s, over, -60.0, cfg.tool_frames / 2);
    b.wheel(&mut s, over, 60.0, cfg.tool_frames / 2);
    // Show every subtopic (two entries of 60, 1,200 Passages each), then scroll through them.
    let (_, subtopics, _) = library::LONG_TOPICS[0];
    let show_all = format!("Show all {subtopics} subtopics");
    for _ in 0..SETTLE_LIMIT {
        if b.has_text(&show_all) {
            break;
        }
        b.wheel(&mut s, over, -60.0, 1);
    }
    b.click_text(&show_all)?;
    // The button now reads "Show fewer", far below.
    b.wait("every subtopic", |b| !b.has_text(&show_all))?;
    b.wheel(&mut s, over, 120.0, cfg.tool_frames / 2);
    b.wheel(&mut s, over, -60.0, cfg.tool_frames);
    for k in 0..cfg.tool_steps {
        let (topic, _, _) = library::LONG_TOPICS[(k + 1) % library::LONG_TOPICS.len()];
        let frames = b.settle(
            &mut s,
            ActionWork::Exempt,
            |shell| {
                shell.run("guide.topic_guide", &json!({ "topic": topic }));
            },
            |b| !b.shell.session().guides.is_busy() && b.has_text(topic),
        );
        s.settle.push(frames);
        b.wheel(&mut s, over, -60.0, cfg.tool_frames / 4);
    }
    b.measured(s)
}

/// Get Resources over a catalog of 3,000 modules: a search typed a key at a time and erased,
/// the type filter changed and back, and the list scrolled. Filtering is held to the 2 ms
/// budget in the frame it happens ([`ActionWork::Budgeted`]).
pub(crate) fn get_resources(cfg: &BenchConfig) -> Result<Samples, String> {
    let resources = [torah_bible()?];
    let fetched_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let mut b = ShellBench::new(cfg, "get-resources", &resources, |s, root| {
        let cache = root.join("catalog.json");
        if let Ok(json) = serde_json::to_string(&library::catalog(fetched_at)) {
            let _ = std::fs::write(&cache, json);
        }
        s.get_bibles.set_cache_path(cache);
    })?;
    let tab = b.open(
        "library.get_resources",
        &json!({ "type": "all", "split": true }),
    )?;
    b.time_tab(tab);
    b.wait("the catalog", |b| {
        b.shell.session().get_bibles.catalog().is_some()
            && b.has_text("Search by name or abbreviation")
    })?;
    b.click_text("Search by name or abbreviation")?;
    let pane = b.pane(tab).ok_or("Get Resources is not drawn")?;
    let over = pos2(pane.center().x, pane.top() + pane.height() * 0.7);
    let mut s = Samples::default();
    // Each key filters and sorts the whole catalog in the frame it is typed (the field shows
    // the query from that frame on): UI-thread work held to the 2 ms budget.
    let query = "rabe";
    let key = |key| Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    };
    for _ in 0..cfg.tool_steps.div_ceil(2) {
        for n in 1..=query.len() {
            b.events.push(Event::Text(query[n - 1..n].into()));
            let typed = &query[..n];
            let frames = b.settle(&mut s, ActionWork::Budgeted, |_| {}, |b| b.has_text(typed));
            s.settle.push(frames);
        }
        for n in (1..query.len()).rev() {
            b.events.push(key(egui::Key::Backspace));
            let typed = &query[..n];
            let frames = b.settle(&mut s, ActionWork::Budgeted, |_| {}, |b| b.has_text(typed));
            s.settle.push(frames);
        }
        b.events.push(key(egui::Key::Backspace));
        let frames = b.settle(
            &mut s,
            ActionWork::Budgeted,
            |_| {},
            |b| b.has_text("Search by name or abbreviation"),
        );
        s.settle.push(frames);
    }
    // The type filter changed, as its chips do, and back: the lists are made again in that
    // frame. Bibles shows its Featured section; All names each row's type.
    let filters = [
        (
            ResourceFilter::Type(ResourceType::Bible),
            "FEATURED BIBLES · ",
        ),
        (ResourceFilter::All, "Bible · "),
    ];
    for (filter, shows) in filters {
        let frames = b.settle(
            &mut s,
            ActionWork::Budgeted,
            |shell| shell.session_mut().get_bibles.set_filter(filter),
            |b| b.has_text_starting(shows),
        );
        s.settle.push(frames);
    }
    b.wheel(&mut s, over, -50.0, cfg.tool_frames);
    b.wheel(&mut s, over, 50.0, cfg.tool_frames / 2);
    b.measured(s)
}

#[cfg(test)]
mod tests {
    use super::Folder;

    /// Runs of one scenario at once (the shell tests share a process and run in parallel
    /// threads) each get a folder of their own, and ending one run leaves the others' library
    /// in place.
    #[test]
    fn a_scenario_run_at_once_never_shares_or_removes_another_runs_folder() {
        let folders: Vec<Folder> = std::thread::scope(|s| {
            let runs: Vec<_> = (0..8)
                .map(|_| s.spawn(|| Folder::new("same-scenario")))
                .collect();
            runs.into_iter().map(|r| r.join().unwrap()).collect()
        });
        let mut paths: Vec<_> = folders.iter().map(|f| f.0.clone()).collect();
        paths.sort();
        paths.dedup();
        assert_eq!(paths.len(), folders.len(), "{paths:?}");
        for f in &folders {
            std::fs::create_dir_all(f.0.join("library")).unwrap();
        }
        let mut folders = folders.into_iter();
        let first = folders.next().unwrap();
        let first_path = first.0.clone();
        drop(first);
        assert!(!first_path.exists());
        let rest: Vec<Folder> = folders.collect();
        for f in &rest {
            assert!(f.0.join("library").is_dir(), "{}", f.0.display());
        }
        // A run starting now leaves the running ones' folders alone too.
        let late = Folder::new("same-scenario");
        for f in &rest {
            assert!(f.0.join("library").is_dir(), "{}", f.0.display());
        }
        drop(late);
    }
}
