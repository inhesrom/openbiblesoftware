//! PROTOTYPE — throwaway, never merged (#372). The GPUI shell slice of `prototype/gpui-shell`,
//! with Resource text laid out by the app's own pipeline (obs-layout and obs-fonts: harfrust
//! shaping, bidi, line breaking, ADR 0004) on worker threads, and painted glyph by glyph with
//! GPUI's `Window::paint_glyph` from the same font files registered with GPUI's text system.
//!
//! Run: `cargo run --release --manifest-path prototype/gpui-text-paint/Cargo.toml [-- --bibles kjv-strongs,web]`
//! Library: `$OBS_LIBRARY`, else `~/.local/share/OpenBible/library` (opened read-only).
//! Bench: `OBS_PROTO_BENCH=prototype/gpui-text-paint/scenario.json OBS_PROTO_BENCH_OUT=out.json`.

mod bench;
mod data;
mod layout;
mod para;

use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use gpui_kit::{
    AnyElement, App, Bounds, Context, FocusHandle, Font, FontWeight, Hsla, KeyDownEvent,
    MouseButton, ScrollDelta, ScrollEvent, TitlebarOptions, Window, WindowBounds,
    WindowOptions, canvas, div, font, point, prelude::*, px, rgb, size,
};
use obs_fonts::FaceId;
use obs_layout::HeightEstimate;
use obs_scripture::{Standard, Verse, parse_reference};

use data::Bible;
use layout::{Budget, Layouts, width_key};
use para::{FontMap, ParaElement};

static STARTED: OnceLock<Instant> = OnceLock::new();
/// ms from start to GPUI's `Application::run` callback (platform and GPU up).
static FRAMEWORK_READY: OnceLock<f64> = OnceLock::new();
/// Frames rendered so far (the Xvfb wake-up stops once frames are flowing).
static FRAMES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

const HPAD: f32 = 24.0;
const TITLE_H: f32 = 40.0;
const TAB_H: f32 = 34.0;
const HEADER_H: f32 = 44.0;
const GAP: f32 = 6.0;
const PAGE_TOP: f32 = 12.0;

// The egui app's light tokens and Paper palette (crates/ui-egui/src/tokens.rs, palette.rs).
const SHELL: u32 = 0xe8eceb;
const SURFACE: u32 = 0xf4f6f4;
const FIELD: u32 = 0xfffffd;
const RULE: u32 = 0xcdd5d3;
const INK: u32 = 0x202629;
const MUTED: u32 = 0x505b60;
const LAPIS: u32 = 0x2d4e86;
const PAGE: u32 = 0xfafbf8;
const HOVER: u32 = 0xe2ebf7;

fn hsla(c: u32) -> Hsla {
    rgb(c).into()
}

fn format_verse(v: Verse) -> String {
    format!("{} {}:{}", v.book.name(), v.chapter, v.verse)
}

struct Panel {
    bible: usize,
    /// Top of the view: (block, points into it).
    anchor: (usize, f32),
    focus: FocusHandle,
    editing: Option<String>,
    error: Option<String>,
    linked: bool,
    /// The verse at the top of the Panel (its own numbering), as of this frame.
    top: Option<Verse>,
}

struct BenchRun {
    scenario: bench::Scenario,
    out: String,
    hz: f64,
    next: Option<bench::Cursor>,
}

/// Per-frame counters for the bench (beyond bench.rs's frame timings).
#[derive(Default)]
struct FrameStats {
    /// Frames that showed at least one block not yet laid out (an estimate drawn blank).
    placeholder_frames: u64,
    placeholder_blocks: u64,
    /// UI-thread layout time per frame, ms (sync budget plus jump blocks).
    sync_ms: Vec<f64>,
    glyphs: Vec<u64>,
}

struct Shell {
    ids: Vec<String>,
    bibles: Vec<Arc<Bible>>,
    layouts: Option<Arc<Layouts>>,
    fonts: Arc<FontMap>,
    glyph_check: serde_json::Value,
    panels: Vec<Panel>,
    status: Option<String>,
    leader: usize,
    followed: Option<Verse>,
    rec: Rc<RefCell<bench::Recorder>>,
    stats: FrameStats,
    first_paint: Rc<std::cell::Cell<Option<f64>>>,
    framework_ready_ms: Option<f64>,
    data_loaded_ms: Option<f64>,
    window_ms: Option<f64>,
    ready_ms: Option<f64>,
    bench: Option<BenchRun>,
    /// Text width and reading-area height of a Panel, this frame (points).
    area: (f32, f32),
    sync_ms_this_frame: f64,
    missing_this_frame: usize,
    /// Frames in which GPUI built the accessibility tree (an assistive technology was active).
    a11y_active_frames: u64,
}

impl Shell {
    fn new(ids: Vec<String>, bench: Option<BenchRun>, fonts: Arc<FontMap>, cx: &mut Context<Self>) -> Self {
        let lib = data::library_dir();
        let tasks: Vec<_> = ids
            .iter()
            .map(|id| {
                let path = lib.join(format!("{id}.obr"));
                cx.background_executor().spawn(async move {
                    let b = data::load(&path)?;
                    // Height estimates for every block, off the UI thread (as the egui app's
                    // HeightIndex does).
                    let settings = obs_layout::TextSettings::default();
                    let est: Vec<HeightEstimate> = (0..b.blocks.len())
                        .map(|i| b.paragraph(i, &settings).map(|p| HeightEstimate::of(&p)).unwrap_or_default())
                        .collect();
                    Ok::<_, String>((b, est))
                })
            })
            .collect();
        cx.spawn(async move |this, cx| {
            let mut bibles = Vec::new();
            let mut estimates = Vec::new();
            let mut problems = Vec::new();
            for t in tasks {
                match t.await {
                    Ok((b, e)) => {
                        bibles.push(Arc::new(b));
                        estimates.push(e);
                    }
                    Err(e) => problems.push(e),
                }
            }
            let _ = this.update(cx, |s, cx| {
                s.data_loaded_ms = STARTED.get().map(|t| t.elapsed().as_secs_f64() * 1000.0);
                s.status = problems.first().cloned();
                s.layouts = Some(Layouts::start(bibles.clone(), estimates));
                for i in 0..bibles.len() {
                    s.panels.push(Panel {
                        bible: i,
                        anchor: (0, 0.0),
                        focus: cx.focus_handle(),
                        editing: None,
                        error: None,
                        linked: true,
                        top: None,
                    });
                }
                s.bibles = bibles;
                cx.notify();
            });
        })
        .detach();
        Self {
            ids,
            bibles: Vec::new(),
            layouts: None,
            fonts,
            glyph_check: serde_json::Value::Null,
            panels: Vec::new(),
            status: Some("Loading…".into()),
            leader: 0,
            followed: None,
            rec: Rc::default(),
            stats: FrameStats::default(),
            first_paint: Rc::default(),
            framework_ready_ms: FRAMEWORK_READY.get().copied(),
            data_loaded_ms: None,
            window_ms: None,
            ready_ms: None,
            bench,
            area: (400.0, 600.0),
            sync_ms_this_frame: 0.0,
            missing_this_frame: 0,
            a11y_active_frames: 0,
        }
    }

    fn wk(&self) -> u32 {
        width_key(self.area.0)
    }

    fn height(&self, p: usize, block: usize) -> f32 {
        let b = self.panels[p].bible;
        self.layouts.as_ref().map_or(40.0, |l| l.height((b, block, self.wk())))
    }

    fn blocks(&self, p: usize) -> usize {
        self.bibles[self.panels[p].bible].blocks.len()
    }

    /// Keep the anchor inside its block (blocks above may have changed height: no jump).
    fn normalize(&mut self, p: usize) {
        let n = self.blocks(p);
        loop {
            let (b, off) = self.panels[p].anchor;
            if off < 0.0 {
                if b == 0 {
                    self.panels[p].anchor = (0, 0.0);
                    return;
                }
                let h = self.height(p, b - 1);
                self.panels[p].anchor = (b - 1, off + h);
            } else {
                let h = self.height(p, b);
                if off >= h && b + 1 < n {
                    self.panels[p].anchor = (b + 1, off - h);
                } else {
                    return;
                }
            }
        }
    }

    fn scroll_by(&mut self, p: usize, dy: f32) {
        self.panels[p].anchor.1 += dy;
        self.normalize(p);
    }

    /// Put Standard verse `s` at the top of Panel `p`, laying its block out now (a jump's block
    /// is exempt from the sync budget, as in the egui app). False if the Bible lacks it.
    fn show(&mut self, p: usize, s: Verse) -> bool {
        let bi = self.panels[p].bible;
        let bible = Arc::clone(&self.bibles[bi]);
        let Some((unit, byte)) = bible.locate_standard(s) else {
            return false;
        };
        let Some(block) = bible.block_at(unit, byte) else {
            return false;
        };
        let Some(layouts) = self.layouts.clone() else {
            return false;
        };
        let t = Instant::now();
        let Some(laid) = layouts.now((bi, block, self.wk())) else {
            return false;
        };
        self.sync_ms_this_frame += t.elapsed().as_secs_f64() * 1000.0;
        let top = laid
            .from_unit(byte)
            .and_then(|q| laid.lines.iter().find(|l| l.bytes.end > q))
            .map_or(0.0, |l| l.top);
        self.panels[p].anchor = (block, top);
        true
    }

    fn jump(&mut self, p: usize, text: &str) {
        let target = parse_reference(text, &Standard, None)
            .map_err(|e| e.to_string())
            .and_then(|l| l.passages().first().map(|q| q.start()).ok_or("no reference".into()));
        self.panels[p].error = None;
        match target {
            Ok(s) => {
                if self.show(p, s) {
                    self.leader = p;
                    self.followed = None;
                } else {
                    let msg = format!("{} is not in {}", format_verse(s), self.bibles[self.panels[p].bible].abbrev);
                    self.panels[p].error = Some(msg);
                }
            }
            Err(e) => self.panels[p].error = Some(e),
        }
    }

    fn chapter(&mut self, p: usize, delta: i32) {
        let Some(top) = self.panels[p].top else { return };
        let b = Arc::clone(&self.bibles[self.panels[p].bible]);
        let target = if delta > 0 {
            b.verse_at.range(Verse::new(top.book, top.chapter + 1, 0)..).next().map(|(v, _)| *v)
        } else {
            b.verse_at
                .range(..Verse::new(top.book, top.chapter, 0))
                .next_back()
                .map(|(v, _)| Verse::new(v.book, v.chapter, 1))
        };
        if let Some(v) = target {
            self.show(p, b.to_standard(v));
            self.leader = p;
            self.followed = None;
        }
    }

    /// The verse on the top line of Panel `p`, if its block is laid out.
    fn top_verse(&self, p: usize) -> Option<Verse> {
        let panel = &self.panels[p];
        let bible = &self.bibles[panel.bible];
        let (block, off) = panel.anchor;
        let b = bible.blocks.get(block)?;
        let laid = self.layouts.as_ref()?.get((panel.bible, block, self.wk()));
        let unit_byte = laid
            .and_then(|l| {
                let li = l.line_index_at(off.max(0.0));
                let start = l.lines.get(li)?.bytes.start;
                match l.to_unit(start)? {
                    obs_layout::UnitPos::Text(u) => Some(u),
                    obs_layout::UnitPos::Object { anchor, .. } => Some(anchor),
                }
            })
            .unwrap_or(b.source.range.start);
        bible.verse_at_unit(b.unit, unit_byte)
    }

    /// Linked Panels follow the leader's top verse (Link set A).
    fn sync_links(&mut self) {
        if self.panels.is_empty() {
            return;
        }
        let l = self.leader.min(self.panels.len() - 1);
        let top = self.top_verse(l);
        self.panels[l].top = top;
        if let Some(v) = top
            && self.panels[l].linked
        {
            let s = self.bibles[self.panels[l].bible].to_standard(v);
            if self.followed != Some(s) {
                self.followed = Some(s);
                for j in 0..self.panels.len() {
                    if j != l && self.panels[j].linked {
                        self.show(j, s);
                    }
                }
            }
        }
        for j in 0..self.panels.len() {
            if j != l {
                self.panels[j].top = self.top_verse(j);
            }
        }
    }

    /// The blocks of Panel `p` in view this frame: laid ones painted, missing ones laid out
    /// within the sync budget or else left to the workers (drawn blank at their estimate).
    /// Prefetches ±1.5 viewports for the workers.
    fn visible(&mut self, p: usize, budget: &Budget) -> Vec<AnyElement> {
        let Some(layouts) = self.layouts.clone() else { return Vec::new() };
        let bi = self.panels[p].bible;
        let bible = Arc::clone(&self.bibles[bi]);
        let wk = self.wk();
        let (width, height) = self.area;
        let n = bible.blocks.len();
        let (mut block, off) = self.panels[p].anchor;
        let mut y = PAGE_TOP - off;
        let mut out = Vec::new();
        let mut missing = 0;
        let mut prefetch = Vec::new();
        while block < n && y < height {
            let key = (bi, block, wk);
            let laid = match layouts.get(key) {
                Some(l) => Some(l),
                None if budget.left() => {
                    let t = Instant::now();
                    let l = layouts.now(key);
                    self.sync_ms_this_frame += t.elapsed().as_secs_f64() * 1000.0;
                    l
                }
                None => {
                    missing += 1;
                    prefetch.push(key);
                    None
                }
            };
            let h = laid.as_ref().map_or_else(|| layouts.estimate(bi, block, width), |l| l.height);
            if let Some(laid) = laid {
                let b = &bible.blocks[block];
                out.push(
                    ParaElement {
                        id: gpui_kit::ElementId::NamedInteger("para".into(), ((p as u64) << 32) | block as u64),
                        origin: (HPAD, y),
                        size: (width, h),
                        laid,
                        fonts: Arc::clone(&self.fonts),
                        visible: (-y, height - y),
                        bible: Arc::clone(&bible),
                        unit_ix: b.unit,
                        unit_range: b.source.range.clone(),
                    }
                    .into_any_element(),
                );
            }
            y += h;
            block += 1;
        }
        // Prefetch: 1.5 viewports below and above.
        let mut below = 0.0;
        let mut b = block;
        while b < n && below < height * 1.5 {
            prefetch.push((bi, b, wk));
            below += layouts.height((bi, b, wk));
            b += 1;
        }
        let mut above = 0.0;
        let mut b = self.panels[p].anchor.0;
        while b > 0 && above < height * 1.5 {
            b -= 1;
            prefetch.push((bi, b, wk));
            above += layouts.height((bi, b, wk));
        }
        layouts.prefetch(prefetch);
        if missing > 0 {
            self.missing_this_frame += missing;
        }
        out
    }

    fn bench_step(&mut self, cx: &mut Context<Self>) -> String {
        let Some(run) = &mut self.bench else { return String::new() };
        let Some(cursor) = &mut run.next else { return "setup".into() };
        let Some((phase, action)) = cursor.step(&run.scenario, Instant::now()) else {
            self.rec.borrow_mut().frame_start(Instant::now(), "done");
            let (pf, pb) = (self.stats.placeholder_frames, self.stats.placeholder_blocks);
            let sync = stats(&mut self.stats.sync_ms.clone());
            let glyphs = stats(&mut self.stats.glyphs.iter().map(|g| *g as f64).collect::<Vec<_>>());
            let layouts = self.layouts.as_ref();
            let extra = serde_json::json!({
                "app": "gpui-text-paint",
                "gpui": "gpui-kit 0.10.0 (gpui-pre 0.3.12)",
                "paint": "obs-layout + obs-fonts, Window::paint_glyph",
                "window": run.scenario.window,
                "bibles": self.ids,
                "frames": run.scenario.frames.len(),
                "startup": {
                    "framework_ready_ms": self.framework_ready_ms,
                    "data_loaded_ms": self.data_loaded_ms,
                    "window_ms": self.window_ms,
                    "first_frame_ms": self.first_paint.get(),
                    "ready_ms": self.ready_ms,
                },
                "text": {
                    "placeholder_frames": pf,
                    "placeholder_blocks": pb,
                    "ui_sync_layout_ms": sync,
                    "glyphs_per_frame": glyphs,
                    "ui_layouts": layouts.map(|l| l.ui_layouts.load(Ordering::Relaxed)),
                    "worker_layouts": layouts.map(|l| l.worker_layouts.load(Ordering::Relaxed)),
                    "shape_cache_hit_rate": obs_fonts::ShapeCache::shared().stats().hit_rate(),
                },
                "glyph_id_check": self.glyph_check,
                "a11y_active_frames": self.a11y_active_frames,
            });
            let report = bench::report(&self.rec.borrow(), run.hz, extra);
            let text = serde_json::to_string_pretty(&report).unwrap_or_default();
            if let Err(e) = std::fs::write(&run.out, &text) {
                eprintln!("bench: {}: {e}", run.out);
            } else {
                eprintln!("bench: wrote {}", run.out);
            }
            cx.quit();
            return "done".into();
        };
        match action {
            bench::Action::None | bench::Action::Settle(_) => {}
            bench::Action::Scroll(dy) => {
                self.leader = 0;
                self.scroll_by(0, dy);
            }
            bench::Action::Jump(to) => self.jump(0, &to),
            bench::Action::Shot(name) => bench::mark_shot("gpui", &name),
        }
        phase
    }

    fn title_bar(&self) -> AnyElement {
        let menu = |label: &'static str| {
            div().px(px(10.)).py(px(4.)).rounded(px(4.)).hover(|s| s.bg(hsla(HOVER))).child(label)
        };
        div()
            .flex()
            .flex_row()
            .items_center()
            .h(px(TITLE_H))
            .px(px(12.))
            .gap(px(2.))
            .bg(hsla(SURFACE))
            .border_b_1()
            .border_color(hsla(RULE))
            .child(
                div()
                    .font_family("Gentium Plus")
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(21.))
                    .mr(px(18.))
                    .child("Open Bible"),
            )
            .children(["File", "Edit", "View", "Go", "Tools", "Window", "Help"].map(menu))
            .child(div().flex_grow(1.))
            .child(div().px(px(10.)).py(px(4.)).border_1().border_color(hsla(RULE)).rounded(px(5.)).child("⌂ Home"))
            .child(
                div()
                    .ml(px(10.))
                    .w(px(380.))
                    .px(px(10.))
                    .py(px(4.))
                    .bg(hsla(FIELD))
                    .border_1()
                    .border_color(hsla(RULE))
                    .rounded(px(5.))
                    .text_color(hsla(MUTED))
                    .child("Go to, study, open or search (Ctrl+L)"),
            )
            .into_any_element()
    }

    fn panel(&mut self, i: usize, budget: &Budget, cx: &mut Context<Self>) -> AnyElement {
        let paras = self.visible(i, budget);
        let p = &self.panels[i];
        let b = Arc::clone(&self.bibles[p.bible]);
        let leader = self.leader == i;
        let reference = match (&p.editing, p.top) {
            (Some(t), _) => format!("{t}▏"),
            (None, Some(v)) => format_verse(v),
            (None, None) => String::new(),
        };
        let editing = p.editing.is_some();
        let focus = p.focus.clone();
        let focus2 = p.focus.clone();
        let n = b.blocks.len().max(1) as f32;
        let thumb = p.anchor.0 as f32 / n;
        let tab = div()
            .flex()
            .flex_row()
            .items_end()
            .h(px(TAB_H))
            .bg(hsla(SHELL))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(12.))
                    .h(px(30.))
                    .bg(hsla(SURFACE))
                    .rounded_t(px(5.))
                    .child(b.abbrev.clone())
                    .child(div().text_color(hsla(MUTED)).child("×")),
            )
            .child(div().flex_grow(1.))
            .child(div().mb(px(3.)).mr(px(4.)).px(px(8.)).border_1().border_color(hsla(RULE)).rounded(px(4.)).child("+"));
        let chip = div()
            .id(("link", i))
            .flex_none()
            .size(px(26.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(hsla(if p.linked { LAPIS } else { RULE }))
            .text_color(hsla(if p.linked { 0xffffff } else { INK }))
            .child("A")
            .on_click(cx.listener(move |s, _, _, cx| {
                s.panels[i].linked = !s.panels[i].linked;
                s.followed = None;
                cx.notify();
            }));
        let button = |id: &'static str, label: &'static str| {
            div()
                .id((id, i))
                .flex_none()
                .px(px(6.))
                .rounded(px(4.))
                .text_color(hsla(MUTED))
                .hover(|s| s.bg(hsla(HOVER)))
                .child(label)
        };
        let ref_box = div()
            .id(("ref", i))
            .flex_none()
            .w(px(128.))
            .px(px(8.))
            .py(px(3.))
            .bg(hsla(FIELD))
            .border_1()
            .border_color(hsla(if editing { LAPIS } else { RULE }))
            .rounded(px(5.))
            .overflow_hidden()
            .whitespace_nowrap()
            .child(reference)
            .on_click(cx.listener(move |s, _, window, cx| {
                s.panels[i].editing = Some(String::new());
                window.focus(&s.panels[i].focus, cx);
                cx.notify();
            }));
        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.))
            .h(px(HEADER_H))
            .px(px(8.))
            .bg(hsla(SURFACE))
            .border_l_4()
            .border_color(hsla(if leader { LAPIS } else { SURFACE }))
            .child(chip)
            .child(button("back", "←"))
            .child(button("fwd", "→"))
            .child(ref_box)
            .child(button("prev", "‹").on_click(cx.listener(move |s, _, _, cx| {
                s.chapter(i, -1);
                cx.notify();
            })))
            .child(button("next", "›").on_click(cx.listener(move |s, _, _, cx| {
                s.chapter(i, 1);
                cx.notify();
            })))
            .child(div().flex_grow(1.).overflow_hidden().whitespace_nowrap().text_color(hsla(MUTED)).child(b.title.clone()));
        let error = p.error.clone().map(|e| div().px(px(HPAD)).py(px(4.)).text_color(hsla(0x963c1e)).child(e));
        let reading = div()
            .id(("reading", i))
            .relative()
            .flex_grow(1.)
            .overflow_hidden()
            .bg(hsla(PAGE))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |s, _, window, cx| {
                    s.leader = i;
                    window.focus(&focus2, cx);
                    cx.notify();
                }),
            )
            .on_scroll(cx.listener(move |s, ev: &ScrollEvent, _, cx| {
                let dy = match ev.delta {
                    ScrollDelta::Pixels(d) => -f32::from(d.y),
                    ScrollDelta::Lines(d) => -d.y * 40.0,
                };
                s.leader = i;
                s.scroll_by(i, dy);
                cx.notify();
            }))
            .children(paras)
            .child(
                div()
                    .absolute()
                    .right(px(2.))
                    .top(gpui_kit::relative(thumb * 0.95))
                    .w(px(5.))
                    .h(gpui_kit::relative(0.05))
                    .rounded(px(3.))
                    .bg(hsla(0xb8ab88)),
            );
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .track_focus(&focus)
            .on_key_down(cx.listener(move |s, ev: &KeyDownEvent, _, cx| {
                s.key(i, ev);
                cx.notify();
            }))
            .child(tab)
            .child(header)
            .children(error)
            .child(reading)
            .into_any_element()
    }

    fn key(&mut self, i: usize, ev: &KeyDownEvent) {
        let key = ev.keystroke.key.as_str();
        if let Some(t) = &mut self.panels[i].editing {
            match key {
                "enter" => {
                    let t = std::mem::take(t);
                    self.panels[i].editing = None;
                    self.jump(i, &t);
                }
                "escape" => self.panels[i].editing = None,
                "backspace" => {
                    t.pop();
                }
                _ => {
                    if let Some(c) = &ev.keystroke.key_char {
                        t.push_str(c);
                    }
                }
            }
            return;
        }
        let page = self.area.1 * 0.9;
        self.leader = i;
        match key {
            "pagedown" | "space" => self.scroll_by(i, page),
            "pageup" => self.scroll_by(i, -page),
            "down" => self.scroll_by(i, 40.),
            "up" => self.scroll_by(i, -40.),
            "]" => self.chapter(i, 1),
            "[" => self.chapter(i, -1),
            "g" => self.panels[i].editing = Some(String::new()),
            _ => {}
        }
    }

    /// Check that glyph ids from our harfrust shaping are the ids GPUI's text system gives the
    /// same text in the same font file (so `paint_glyph` draws the glyph we shaped).
    fn check_glyph_ids(&mut self, window: &mut Window) {
        let samples = [
            (FaceId::Regular, "In the beginning God created the heaven", false),
            (FaceId::Italic, "was without form", false),
            (FaceId::Regular, "Ἐν ἀρχῇ ἦν ὁ λόγος", false),
            (FaceId::Hebrew, "בְּרֵאשִׁ֖ית בָּרָ֣א אֱלֹהִ֑ים", true),
        ];
        let mut out = Vec::new();
        for (face, text, rtl) in samples {
            let ours: Vec<u32> = obs_fonts::ShapeCache::shared()
                .shape(text, obs_fonts::ShapeStyle::new(face, 20.0, false, rtl))
                .map(|r| r.glyphs.iter().map(|g| g.id).collect())
                .unwrap_or_default();
            let Some(&fid) = self.fonts.get(&face) else { continue };
            let Some(f) = window.text_system().get_font_for_id(fid) else { continue };
            let run = gpui_kit::TextRun {
                len: text.len(),
                font: f.clone(),
                color: hsla(INK),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let line = window.text_system().layout_line(text, px(20.), &[run], None);
            let mut theirs: Vec<u32> = line.runs.iter().flat_map(|r| r.glyphs.iter().map(|g| g.id.0)).collect();
            let mut a = ours.clone();
            a.sort_unstable();
            theirs.sort_unstable();
            let same = a == theirs;
            out.push(serde_json::json!({
                "face": format!("{face:?}"),
                "gpui_family": f.family.to_string(),
                "text": text,
                "ours": ours.len(),
                "gpui": theirs.len(),
                "same_glyph_ids": same,
            }));
            eprintln!("glyph ids {face:?} ({}): ours {} gpui {} same={same}", f.family, ours.len(), theirs.len());
        }
        self.glyph_check = serde_json::Value::Array(out);
    }
}

fn stats(v: &mut [f64]) -> serde_json::Value {
    if v.is_empty() {
        return serde_json::json!({ "count": 0 });
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let q = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
    serde_json::json!({ "count": v.len(), "p50": q(0.5), "p95": q(0.95), "p99": q(0.99), "max": v[v.len() - 1] })
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        FRAMES.fetch_add(1, Ordering::Relaxed);
        if self.window_ms.is_none() {
            self.window_ms = STARTED.get().map(|s| now.duration_since(*s).as_secs_f64() * 1000.0);
            self.check_glyph_ids(window);
        }
        // Reading area of each Panel, from the window size (the layout below is fixed).
        let vs = window.viewport_size();
        let n = self.panels.len().max(1) as f32;
        let pw = (f32::from(vs.width) - 2.0 * GAP - GAP * (n - 1.0)) / n;
        let ah = f32::from(vs.height) - TITLE_H - TAB_H - HEADER_H - 2.0 * GAP;
        self.area = ((pw - 2.0 * HPAD).max(50.0), ah.max(50.0));
        if self.ready_ms.is_none() && !self.panels.is_empty() {
            self.ready_ms = STARTED.get().map(|s| now.duration_since(*s).as_secs_f64() * 1000.0);
            // `OBS_PROTO_START=<reference>`: open the first Panel there (screenshots, no input).
            if let Ok(r) = std::env::var("OBS_PROTO_START") {
                self.jump(0, &r);
            }
            if let Some(b) = &mut self.bench {
                b.next = Some(bench::Cursor::default());
            }
        }
        self.sync_ms_this_frame = 0.0;
        self.missing_this_frame = 0;
        let phase = self.bench_step(cx);
        if self.bench.is_some() && phase != "done" {
            self.rec.borrow_mut().frame_start(now, &phase);
        }
        self.sync_links();
        let budget = Budget::new();
        let panels: Vec<_> = (0..self.panels.len()).map(|i| self.panel(i, &budget, cx)).collect();
        if self.bench.is_some() && phase != "done" && phase != "setup" && phase != "shot" {
            if self.missing_this_frame > 0 {
                self.stats.placeholder_frames += 1;
                self.stats.placeholder_blocks += self.missing_this_frame as u64;
            }
            self.stats.sync_ms.push(self.sync_ms_this_frame);
            self.stats.glyphs.push(para::GLYPHS_DRAWN.swap(0, Ordering::Relaxed));
        } else {
            para::GLYPHS_DRAWN.store(0, Ordering::Relaxed);
        }
        // Keep frames coming while a bench runs or workers are still laying out what is shown.
        if self.bench.is_some() || self.missing_this_frame > 0 {
            window.request_animation_frame();
        }
        // Accessibility: GPUI builds the AccessKit tree only while an assistive technology has
        // asked for it. `OBS_PROTO_A11Y_DUMP=<file>` writes the last frame's tree as JSON then.
        if window.is_a11y_active() {
            self.a11y_active_frames += 1;
            if self.a11y_active_frames % 60 == 1
                && let Ok(path) = std::env::var("OBS_PROTO_A11Y_DUMP")
                && let Some(json) = window.debug_a11y_tree_json()
            {
                let _ = std::fs::write(path, json);
            }
        }
        let rec = self.rec.clone();
        let first = self.first_paint.clone();
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(hsla(SHELL))
            .font_family("Ubuntu")
            .text_size(px(15.))
            .text_color(hsla(INK))
            .child(self.title_bar())
            .children(self.status.clone().map(|s| div().p(px(16.)).child(s)))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_grow(1.)
                    .min_h(px(0.))
                    .gap(px(GAP))
                    .p(px(GAP))
                    .children(panels),
            )
            .child(
                canvas(
                    |_, _, _| (),
                    move |_, _, _, _| {
                        let t = Instant::now();
                        if first.get().is_none() {
                            first.set(STARTED.get().map(|s| t.duration_since(*s).as_secs_f64() * 1000.0));
                        }
                        rec.borrow_mut().paint_end(t);
                    },
                )
                .absolute()
                .size(px(1.)),
            )
    }
}

/// Xvfb (no window manager) workaround: GPUI's X11 client can miss the events queued while the
/// window was created and then never start its frame loop. A property change on our own
/// window from a second connection wakes it; repeated until frames flow. Harmless on a real
/// desktop, where frames flow at once and it stops.
fn x11_poke() {
    if std::env::var_os("DISPLAY").is_none() || std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return;
    }
    std::thread::spawn(|| {
        let _ = (|| -> Option<()> {
            use x11rb::connection::Connection;
            use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, PropMode};
            use x11rb::wrapper::ConnectionExt as _;
            let (conn, screen) = x11rb::connect(None).ok()?;
            let root = conn.setup().roots.get(screen)?.root;
            let pid_atom = conn.intern_atom(false, b"_NET_WM_PID").ok()?.reply().ok()?.atom;
            let poke = conn.intern_atom(false, b"_OBS_PROTO_POKE").ok()?.reply().ok()?.atom;
            for _ in 0..120 {
                std::thread::sleep(std::time::Duration::from_millis(250));
                if FRAMES.load(Ordering::Relaxed) > 3 {
                    break;
                }
                for w in conn.query_tree(root).ok()?.reply().ok()?.children {
                    let Some(prop) = conn
                        .get_property(false, w, pid_atom, AtomEnum::CARDINAL, 0, 1)
                        .ok()
                        .and_then(|c| c.reply().ok())
                    else {
                        continue;
                    };
                    if prop.value32().and_then(|mut v| v.next()) == Some(std::process::id()) {
                        let _ = conn.change_property8(PropMode::REPLACE, w, poke, AtomEnum::STRING, b"1");
                        let _ = conn.flush();
                    }
                }
            }
            Some(())
        })();
    });
}

/// `--x11-raise <title>`: raise the top-level windows with that title (WM_NAME) above the others,
/// for bench.sh's screenshots on a display several apps share with no window manager.
fn x11_raise(title: &str) {
    let _ = (|| -> Option<()> {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::{AtomEnum, ConfigureWindowAux, ConnectionExt, StackMode};
        let (conn, screen) = x11rb::connect(None).ok()?;
        let root = conn.setup().roots.get(screen)?.root;
        for w in conn.query_tree(root).ok()?.reply().ok()?.children {
            let name = conn
                .get_property(false, w, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 256)
                .ok()
                .and_then(|c| c.reply().ok())
                .map(|r| String::from_utf8_lossy(&r.value).into_owned());
            if name.as_deref() == Some(title) {
                let _ = conn.configure_window(w, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE));
            }
        }
        conn.flush().ok()?;
        Some(())
    })();
}

fn main() {
    STARTED.get_or_init(Instant::now);
    env_logger::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--x11-raise") {
        x11_raise(args.get(1).map_or("", String::as_str));
        return;
    }
    let bench = std::env::var("OBS_PROTO_BENCH").ok().map(|path| {
        let scenario = bench::load(&path).unwrap_or_else(|e| {
            eprintln!("bench: {e}");
            std::process::exit(2)
        });
        BenchRun {
            scenario,
            out: std::env::var("OBS_PROTO_BENCH_OUT").unwrap_or_else(|_| "gpui-bench.json".into()),
            hz: std::env::var("OBS_PROTO_HZ").ok().and_then(|v| v.parse().ok()).unwrap_or(60.0),
            next: None,
        }
    });
    let ids: Vec<String> = match (args.iter().position(|a| a == "--bibles"), &bench) {
        (Some(i), _) => args.get(i + 1).map(|s| s.split(',').map(String::from).collect()).unwrap_or_default(),
        (None, Some(b)) => b.scenario.bibles.clone(),
        (None, None) => ["kjv-strongs", "web", "tahot", "tagnt"].map(String::from).to_vec(),
    };
    let [w, h] = bench.as_ref().map_or([1400.0, 900.0], |b| b.scenario.window);
    x11_poke();
    let mut init = Some((ids, bench));
    gpui_kit::application().run(move |cx: &mut App| {
        if let Some(t) = STARTED.get() {
            let _ = FRAMEWORK_READY.set(t.elapsed().as_secs_f64() * 1000.0);
        }
        gpui_kit::init(cx);
        // The same font files obs-fonts shapes with, registered with GPUI's text system.
        let faces = [FaceId::Regular, FaceId::Italic, FaceId::Bold, FaceId::BoldItalic, FaceId::Hebrew];
        let files = faces
            .iter()
            .filter_map(|f| f.bundled_bytes())
            .map(Cow::Borrowed)
            .chain([Cow::Borrowed(epaint_default_fonts::UBUNTU_LIGHT)])
            .collect();
        if let Err(e) = cx.text_system().add_fonts(files) {
            eprintln!("fonts: {e}");
        }
        let fonts: FontMap = faces
            .iter()
            .map(|&f| {
                let gf: Font = match f {
                    FaceId::Italic => font("Gentium Plus").italic(),
                    FaceId::Bold => font("Gentium Plus").bold(),
                    FaceId::BoldItalic => font("Gentium Plus").bold().italic(),
                    FaceId::Hebrew => font("Ezra SIL"),
                    _ => font("Gentium Plus"),
                };
                (f, cx.text_system().resolve_font(&gf))
            })
            .collect();
        let fonts = Arc::new(fonts);
        let bounds = Bounds { origin: point(px(0.), px(0.)), size: size(px(w), px(h)) };
        let opened = gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Open Bible Software (GPUI text-paint prototype)".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            cx,
            move |_, cx| {
                let (ids, bench) = init.take().unwrap_or_default();
                let fonts = Arc::clone(&fonts);
                cx.new(|cx| Shell::new(ids, bench, fonts, cx))
            },
        );
        if let Err(e) = opened {
            eprintln!("window: {e}");
            cx.quit();
        }
    });
}
