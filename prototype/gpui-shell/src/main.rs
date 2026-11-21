//! PROTOTYPE — throwaway, never merged. An Open Bible Software shell slice drawn with GPUI
//! (Zed's UI framework) instead of egui, to see whether it looks different or runs faster.
//!
//! Run: `cargo run --release --manifest-path prototype/gpui-shell/Cargo.toml [-- --bibles kjv-strongs,web]`
//! Library: `$OBS_LIBRARY`, else `~/.local/share/OpenBible/library` (opened read-only).
//! Bench: `OBS_PROTO_BENCH=prototype/gpui-shell/scenario.json OBS_PROTO_BENCH_OUT=out.json`.
//!
//! Resource text goes through GPUI's own text system (cosmic-text shaping, GPUI wrapping), not
//! the app's harfrust/bidi pipeline: how it copes with Hebrew is one of the findings.

mod bench;
mod data;

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use gpui::{
    AnyElement, App, Application, Bounds, Context, FocusHandle, Font, FontFeatures, FontStyle,
    FontWeight, Hsla, KeyDownEvent, ListAlignment, ListOffset, ListState, MouseButton, Pixels,
    SharedString, TextRun, TitlebarOptions, Window, WindowBounds, WindowOptions, canvas, div, font,
    list, point, prelude::*, px, relative, rgb, size,
};
use obs_scripture::{Standard, Verse, parse_reference};

use data::{Bible, Flags, Kind};

static STARTED: OnceLock<Instant> = OnceLock::new();
/// ms from start to GPUI's `Application::run` callback (platform and GPU up).
static FRAMEWORK_READY: OnceLock<f64> = OnceLock::new();
/// Frames rendered so far (the Xvfb wake-up stops once frames are flowing).
static FRAMES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

const TEXT: f32 = 20.0;
const HEBREW_SCALE: f32 = 1.3;
/// Line height as a share of the font size, matched to the egui app's measured line pitch
/// (36.8 px at 20 px Latin/Greek, 39 px at 26 px Hebrew: its 1.25 × tallest-font rule).
const LINE: f32 = 1.84;
const LINE_HEBREW: f32 = 1.5;
const HPAD: f32 = 24.0;
const PARA_GAP: f32 = 12.0;
const INDENT: f32 = 24.0;

// The egui app's light tokens and Paper palette (crates/ui-egui/src/tokens.rs, palette.rs).
const SHELL: u32 = 0xe8eceb;
const SURFACE: u32 = 0xf4f6f4;
const FIELD: u32 = 0xfffffd;
const RULE: u32 = 0xcdd5d3;
const INK: u32 = 0x202629;
const MUTED: u32 = 0x505b60;
const LAPIS: u32 = 0x2d4e86;
const PAGE: u32 = 0xfafbf8;
const VERSE_NO: u32 = 0x6e521a;
const WOJ: u32 = 0x96201e;
const HOVER: u32 = 0xe2ebf7;

fn hsla(c: u32) -> Hsla {
    rgb(c).into()
}

fn para_size(bible: &Bible) -> f32 {
    if bible.rtl { TEXT * HEBREW_SCALE } else { TEXT }
}

fn line(bible: &Bible) -> f32 {
    if bible.rtl { LINE_HEBREW } else { LINE }
}

fn indent(p: &data::Para) -> f32 {
    match p.kind {
        Kind::Poetry(n) => INDENT * f32::from(n),
        _ => 0.0,
    }
}

fn top_pad(p: &data::Para) -> f32 {
    if p.kind == Kind::Heading {
        PARA_GAP
    } else {
        0.0
    }
}

fn runs(bible: &Bible, p: &data::Para) -> Vec<TextRun> {
    let family: SharedString = if bible.rtl {
        "Ezra SIL".into()
    } else {
        "Gentium Plus".into()
    };
    p.runs
        .iter()
        .map(|&(len, f): &(usize, Flags)| {
            let mut ft: Font = font(family.clone());
            if f.verse_number || f.chapter_number {
                ft = font("Gentium Plus");
            }
            if f.italic || p.kind == Kind::PsalmTitle {
                ft.style = FontStyle::Italic;
            }
            if f.bold || f.chapter_number || p.kind == Kind::Heading {
                ft.weight = FontWeight::BOLD;
            }
            if f.small_caps {
                ft.features = FontFeatures(Arc::new(vec![("smcp".into(), 1)]));
            }
            let color = if f.chapter_number {
                LAPIS
            } else if f.verse_number {
                VERSE_NO
            } else if f.woj {
                WOJ
            } else if p.kind == Kind::Heading {
                MUTED
            } else {
                INK
            };
            TextRun {
                len,
                font: ft,
                color: hsla(color),
                background_color: None,
                underline: None,
                strikethrough: None,
            }
        })
        .collect()
}

fn render_para(bible: &Bible, ix: usize) -> AnyElement {
    let Some(p) = bible.paras.get(ix) else {
        return div().into_any_element();
    };
    let size = para_size(bible);
    let mut d = div()
        .w_full()
        .pl(px(HPAD + indent(p)))
        .pr(px(HPAD))
        .pt(px(top_pad(p)))
        .pb(px(PARA_GAP))
        .text_size(px(size))
        .line_height(relative(line(bible)))
        .child(gpui::StyledText::new(p.text.clone()).with_runs(runs(bible, p)));
    if bible.rtl {
        d = d.text_right();
    }
    d.into_any_element()
}

fn format_verse(v: Verse) -> String {
    format!("{} {}:{}", v.book.name(), v.chapter, v.verse)
}

struct Panel {
    bible: Arc<Bible>,
    list: ListState,
    focus: FocusHandle,
    /// The reference box's text while it is being edited.
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
    /// Where the scenario is; None until the shell is ready.
    next: Option<bench::Cursor>,
}

struct Shell {
    ids: Vec<String>,
    panels: Vec<Panel>,
    status: Option<String>,
    leader: Rc<Cell<usize>>,
    /// The Standard verse the linked Panels last followed.
    followed: Option<Verse>,
    rec: Rc<RefCell<bench::Recorder>>,
    first_paint: Rc<Cell<Option<f64>>>,
    framework_ready_ms: Option<f64>,
    data_loaded_ms: Option<f64>,
    window_ms: Option<f64>,
    ready_ms: Option<f64>,
    bench: Option<BenchRun>,
}

impl Shell {
    fn new(ids: Vec<String>, bench: Option<BenchRun>, cx: &mut Context<Self>) -> Self {
        let lib = data::library_dir();
        let tasks: Vec<_> = ids
            .iter()
            .map(|id| {
                let path = lib.join(format!("{id}.obr"));
                cx.background_executor()
                    .spawn(async move { data::load(&path) })
            })
            .collect();
        cx.spawn(async move |this, cx| {
            let mut bibles = Vec::new();
            let mut problems = Vec::new();
            for t in tasks {
                match t.await {
                    Ok(b) => bibles.push(Arc::new(b)),
                    Err(e) => problems.push(e),
                }
            }
            let _ = this.update(cx, |s, cx| {
                s.data_loaded_ms = STARTED.get().map(|t| t.elapsed().as_secs_f64() * 1000.0);
                s.status = problems.first().cloned();
                for b in bibles {
                    let list = ListState::new(b.paras.len(), ListAlignment::Top, px(1200.));
                    let leader = s.leader.clone();
                    let ix = s.panels.len();
                    list.set_scroll_handler(move |_, _, _| leader.set(ix));
                    s.panels.push(Panel {
                        bible: b,
                        list,
                        focus: cx.focus_handle(),
                        editing: None,
                        error: None,
                        linked: true,
                        top: None,
                    });
                }
                cx.notify();
            });
        })
        .detach();
        Self {
            ids,
            panels: Vec::new(),
            status: Some("Loading…".into()),
            leader: Rc::new(Cell::new(0)),
            followed: None,
            rec: Rc::default(),
            first_paint: Rc::default(),
            framework_ready_ms: FRAMEWORK_READY.get().copied(),
            data_loaded_ms: None,
            window_ms: None,
            ready_ms: None,
            bench,
        }
    }

    fn text_width(&self, i: usize, p: &data::Para) -> Option<Pixels> {
        let w = self.panels[i].list.viewport_bounds().size.width - px(2.0 * HPAD + indent(p));
        (w > px(10.)).then_some(w)
    }

    /// Shape a paragraph as its text element does (GPUI caches the layout for the frame).
    fn shape(
        &self,
        i: usize,
        ix: usize,
        window: &mut Window,
    ) -> Option<(gpui::WrappedLine, Pixels)> {
        let b = &self.panels[i].bible;
        let p = b.paras.get(ix)?;
        let w = self.text_width(i, p)?;
        let size = para_size(b);
        let lines = window
            .text_system()
            .shape_text(p.text.clone(), px(size), &runs(b, p), Some(w), None)
            .ok()?;
        Some((lines.into_iter().next()?, px(size * line(b))))
    }

    fn top_verse(&self, i: usize, window: &mut Window) -> Option<Verse> {
        let panel = &self.panels[i];
        let top = panel.list.logical_scroll_top();
        let b = &panel.bible;
        let p = b.paras.get(top.item_ix)?;
        let y = top.offset_in_item - px(top_pad(p));
        let off = match self.shape(i, top.item_ix, window) {
            Some((line, lh)) if y > px(0.) => {
                let x = if b.rtl { line.width() } else { px(0.) };
                line.closest_index_for_position(point(x, y), lh)
                    .unwrap_or_else(|i| i)
            }
            _ => 0,
        };
        b.verse_at_offset(top.item_ix, off)
    }

    /// Put Standard verse `s` at the top of Panel `i`. False if the Bible does not have it.
    fn show(&mut self, i: usize, s: Verse, window: &mut Window) -> bool {
        let Some((ix, off)) = self.panels[i].bible.locate_standard(s) else {
            return false;
        };
        let p = &self.panels[i].bible.paras[ix];
        let pad = px(top_pad(p));
        let y = self
            .shape(i, ix, window)
            .and_then(|(line, lh)| line.position_for_index(off, lh))
            .map_or(px(0.), |pt| pt.y);
        self.panels[i].list.scroll_to(ListOffset {
            item_ix: ix,
            offset_in_item: pad + y,
        });
        true
    }

    fn jump(&mut self, i: usize, text: &str, window: &mut Window) {
        let target = parse_reference(text, &Standard, None)
            .map_err(|e| e.to_string())
            .and_then(|l| {
                l.passages()
                    .first()
                    .map(|p| p.start())
                    .ok_or("no reference".into())
            });
        self.panels[i].error = None;
        match target {
            Ok(s) => {
                if self.show(i, s, window) {
                    self.leader.set(i);
                    self.followed = None;
                } else {
                    let msg = format!(
                        "{} is not in {}",
                        format_verse(s),
                        self.panels[i].bible.abbrev
                    );
                    self.panels[i].error = Some(msg);
                }
            }
            Err(e) => self.panels[i].error = Some(e),
        }
    }

    fn chapter(&mut self, i: usize, delta: i32, window: &mut Window) {
        let Some(top) = self.panels[i].top else {
            return;
        };
        let b = self.panels[i].bible.clone();
        let this = Verse::new(top.book, top.chapter, 0);
        let target = if delta > 0 {
            b.verse_at
                .range(Verse::new(top.book, top.chapter + 1, 0)..)
                .next()
                .map(|(v, _)| *v)
        } else {
            b.verse_at
                .range(..this)
                .next_back()
                .map(|(v, _)| Verse::new(v.book, v.chapter, 1))
        };
        if let Some(v) = target {
            let s = b.to_standard(v);
            self.show(i, s, window);
            self.leader.set(i);
            self.followed = None;
        }
    }

    /// Linked Panels follow the leader's top verse (Link set A).
    fn sync(&mut self, window: &mut Window) {
        if self.panels.is_empty() {
            return;
        }
        let l = self.leader.get().min(self.panels.len() - 1);
        let top = self.top_verse(l, window);
        self.panels[l].top = top;
        if let Some(v) = top
            && self.panels[l].linked
        {
            let s = self.panels[l].bible.to_standard(v);
            if self.followed != Some(s) {
                self.followed = Some(s);
                for j in 0..self.panels.len() {
                    if j != l && self.panels[j].linked {
                        self.show(j, s, window);
                    }
                }
            }
        }
        for j in 0..self.panels.len() {
            if j != l {
                self.panels[j].top = self.top_verse(j, window);
            }
        }
    }

    fn bench_step(&mut self, window: &mut Window, cx: &mut Context<Self>) -> String {
        let Some(run) = &mut self.bench else {
            return String::new();
        };
        let Some(cursor) = &mut run.next else {
            return "setup".into();
        };
        let Some((phase, action)) = cursor.step(&run.scenario, Instant::now()) else {
            // Done: this frame's start closes the last interval.
            self.rec.borrow_mut().frame_start(Instant::now(), "done");
            let extra = serde_json::json!({
                "app": "gpui",
                "gpui_version": "0.2.2",
                "window": run.scenario.window,
                "window_actual": [
                    f32::from(window.viewport_size().width),
                    f32::from(window.viewport_size().height),
                ],
                "scale_factor": window.scale_factor(),
                "bibles": self.ids,
                "frames": run.scenario.frames.len(),
                "startup": {
                    "framework_ready_ms": self.framework_ready_ms,
                    "data_loaded_ms": self.data_loaded_ms,
                    "window_ms": self.window_ms,
                    "first_frame_ms": self.first_paint.get(),
                    "ready_ms": self.ready_ms,
                },
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
                self.leader.set(0);
                self.panels[0].list.scroll_by(px(dy));
            }
            bench::Action::Jump(to) => self.jump(0, &to, window),
            bench::Action::Shot(name) => bench::mark_shot("gpui", &name),
        }
        phase
    }

    fn title_bar(&self) -> AnyElement {
        let menu = |label: &'static str| {
            div()
                .px(px(10.))
                .py(px(4.))
                .rounded(px(4.))
                .hover(|s| s.bg(hsla(HOVER)))
                .child(label)
        };
        div()
            .flex()
            .flex_row()
            .items_center()
            .h(px(40.))
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
            .child(div().flex_grow())
            .child(
                div()
                    .px(px(10.))
                    .py(px(4.))
                    .border_1()
                    .border_color(hsla(RULE))
                    .rounded(px(5.))
                    .child("⌂ Home"),
            )
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

    fn panel(&self, i: usize, cx: &mut Context<Self>) -> AnyElement {
        let p = &self.panels[i];
        let b = p.bible.clone();
        let leader = self.leader.get() == i;
        let reference = match (&p.editing, p.top) {
            (Some(t), _) => format!("{t}▏"),
            (None, Some(v)) => format_verse(v),
            (None, None) => String::new(),
        };
        let editing = p.editing.is_some();
        let focus = p.focus.clone();
        let focus2 = p.focus.clone();
        let count = b.paras.len().max(1) as f32;
        let thumb = p.list.logical_scroll_top().item_ix as f32 / count;
        let bible = b.clone();
        let tab = div()
            .flex()
            .flex_row()
            .items_end()
            .h(px(34.))
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
            .child(div().flex_grow())
            .child(
                div()
                    .mb(px(3.))
                    .mr(px(4.))
                    .px(px(8.))
                    .border_1()
                    .border_color(hsla(RULE))
                    .rounded(px(4.))
                    .child("+"),
            );
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
                window.focus(&s.panels[i].focus);
                cx.notify();
            }));
        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.))
            .h(px(44.))
            .px(px(8.))
            .bg(hsla(SURFACE))
            .border_l_4()
            .border_color(hsla(if leader { LAPIS } else { SURFACE }))
            .child(chip)
            .child(button("back", "←"))
            .child(button("fwd", "→"))
            .child(ref_box)
            .child(
                button("prev", "‹").on_click(cx.listener(move |s, _, window, cx| {
                    s.chapter(i, -1, window);
                    cx.notify();
                })),
            )
            .child(
                button("next", "›").on_click(cx.listener(move |s, _, window, cx| {
                    s.chapter(i, 1, window);
                    cx.notify();
                })),
            )
            .child(
                div()
                    .flex_grow()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_color(hsla(MUTED))
                    .child(b.title.clone()),
            );
        let error = p.error.clone().map(|e| {
            div()
                .px(px(HPAD))
                .py(px(4.))
                .text_color(hsla(0x963c1e))
                .child(e)
        });
        let reading = div()
            .relative()
            .flex_grow()
            .bg(hsla(PAGE))
            .pt(px(12.))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |s, _, window, cx| {
                    s.leader.set(i);
                    window.focus(&focus2);
                    cx.notify();
                }),
            )
            .child(list(p.list.clone(), move |ix, _, _| render_para(&bible, ix)).size_full())
            .child(
                div()
                    .absolute()
                    .right(px(2.))
                    .top(relative(thumb * 0.95))
                    .w(px(5.))
                    .h(relative(0.05))
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
            .on_key_down(cx.listener(move |s, ev: &KeyDownEvent, window, cx| {
                s.key(i, ev, window);
                cx.notify();
            }))
            .child(tab)
            .child(header)
            .children(error)
            .child(reading)
            .into_any_element()
    }

    fn key(&mut self, i: usize, ev: &KeyDownEvent, window: &mut Window) {
        let key = ev.keystroke.key.as_str();
        if let Some(t) = &mut self.panels[i].editing {
            match key {
                "enter" => {
                    let t = std::mem::take(t);
                    self.panels[i].editing = None;
                    self.jump(i, &t, window);
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
        let page = self.panels[i].list.viewport_bounds().size.height * 0.9;
        self.leader.set(i);
        match key {
            "pagedown" | "space" => self.panels[i].list.scroll_by(page),
            "pageup" => self.panels[i].list.scroll_by(-page),
            "down" => self.panels[i].list.scroll_by(px(40.)),
            "up" => self.panels[i].list.scroll_by(px(-40.)),
            "]" => self.chapter(i, 1, window),
            "[" => self.chapter(i, -1, window),
            "g" => self.panels[i].editing = Some(String::new()),
            _ => {}
        }
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        FRAMES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if self.window_ms.is_none() {
            self.window_ms = STARTED
                .get()
                .map(|s| now.duration_since(*s).as_secs_f64() * 1000.0);
        }
        // The shell is ready once every Panel has been laid out once.
        if self.ready_ms.is_none()
            && !self.panels.is_empty()
            && self
                .panels
                .iter()
                .all(|p| p.list.viewport_bounds().size.width > px(0.))
        {
            self.ready_ms = STARTED
                .get()
                .map(|s| now.duration_since(*s).as_secs_f64() * 1000.0);
            if let Some(b) = &mut self.bench {
                b.next = Some(bench::Cursor::default());
            }
        }
        let phase = self.bench_step(window, cx);
        if self.bench.is_some() && phase != "done" {
            self.rec.borrow_mut().frame_start(now, &phase);
            window.request_animation_frame();
        }
        self.sync(window);
        let rec = self.rec.clone();
        let first = self.first_paint.clone();
        let trace = std::env::var_os("OBS_PROTO_TRACE").is_some().then(|| {
            let tops: Vec<String> = self
                .panels
                .iter()
                .map(|p| {
                    let t = p.list.logical_scroll_top();
                    let len = p.bible.paras.get(t.item_ix).map_or(0, |q| q.text.len());
                    format!(
                        "{}:{}({}B)",
                        p.bible.abbrev,
                        p.top.map_or("-".into(), format_verse),
                        len
                    )
                })
                .collect();
            (now, phase.clone(), tops.join(" "))
        });
        let panels: Vec<_> = (0..self.panels.len()).map(|i| self.panel(i, cx)).collect();
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
                    .flex_grow()
                    .min_h(px(0.))
                    .gap(px(6.))
                    .p(px(6.))
                    .children(panels),
            )
            .child(
                canvas(
                    |_, _, _| (),
                    move |_, _, _, _| {
                        let t = Instant::now();
                        if first.get().is_none() {
                            first.set(
                                STARTED
                                    .get()
                                    .map(|s| t.duration_since(*s).as_secs_f64() * 1000.0),
                            );
                        }
                        rec.borrow_mut().paint_end(t);
                        if let Some((start, phase, tops)) = &trace {
                            let ms = t.duration_since(*start).as_secs_f64() * 1000.0;
                            if ms > 12.0 {
                                eprintln!("slow frame {ms:.1} ms [{phase}] {tops}");
                            }
                        }
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
            let pid_atom = conn
                .intern_atom(false, b"_NET_WM_PID")
                .ok()?
                .reply()
                .ok()?
                .atom;
            let poke = conn
                .intern_atom(false, b"_OBS_PROTO_POKE")
                .ok()?
                .reply()
                .ok()?
                .atom;
            // Until frames flow (more than the first one), for at most 30 s.
            for _ in 0..120 {
                std::thread::sleep(std::time::Duration::from_millis(250));
                if FRAMES.load(std::sync::atomic::Ordering::Relaxed) > 3 {
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
                        let _ = conn.change_property8(
                            PropMode::REPLACE,
                            w,
                            poke,
                            AtomEnum::STRING,
                            b"1",
                        );
                        let _ = conn.flush();
                    }
                }
            }
            Some(())
        })();
    });
}

fn main() {
    STARTED.get_or_init(Instant::now);
    env_logger::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--dump") {
        dump();
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
            hz: std::env::var("OBS_PROTO_HZ")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(60.0),
            next: None,
        }
    });
    let ids: Vec<String> = match (args.iter().position(|a| a == "--bibles"), &bench) {
        (Some(i), _) => args
            .get(i + 1)
            .map(|s| s.split(',').map(String::from).collect())
            .unwrap_or_default(),
        (None, Some(b)) => b.scenario.bibles.clone(),
        (None, None) => ["kjv-strongs", "web", "tahot", "tagnt"]
            .map(String::from)
            .to_vec(),
    };
    let [w, h] = bench
        .as_ref()
        .map_or([1400.0, 900.0], |b| b.scenario.window);
    x11_poke();
    let mut init = Some((ids, bench));
    Application::new().run(move |cx: &mut App| {
        if let Some(t) = STARTED.get() {
            let _ = FRAMEWORK_READY.set(t.elapsed().as_secs_f64() * 1000.0);
        }
        let fonts = [
            obs_fonts::FaceId::Regular,
            obs_fonts::FaceId::Italic,
            obs_fonts::FaceId::Bold,
            obs_fonts::FaceId::BoldItalic,
            obs_fonts::FaceId::Hebrew,
        ]
        .iter()
        .filter_map(|f| f.bundled_bytes())
        .map(Cow::Borrowed)
        .chain([Cow::Borrowed(epaint_default_fonts::UBUNTU_LIGHT)])
        .collect();
        if let Err(e) = cx.text_system().add_fonts(fonts) {
            eprintln!("fonts: {e}");
        }
        let bounds = Bounds {
            origin: point(px(0.), px(0.)),
            size: size(px(w), px(h)),
        };
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Open Bible Software (GPUI prototype)".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |_, cx| {
                let (ids, bench) = init.take().unwrap_or_default();
                cx.new(|cx| Shell::new(ids, bench, cx))
            },
        );
        if let Err(e) = opened {
            eprintln!("window: {e}");
            cx.quit();
        }
    });
}

fn dump() {
    let lib = data::library_dir();
    for id in ["kjv-strongs", "web", "tahot", "tagnt"] {
        let t = Instant::now();
        let b = match data::load(&lib.join(format!("{id}.obr"))) {
            Ok(b) => b,
            Err(e) => {
                println!("{id}: {e}");
                continue;
            }
        };
        println!(
            "== {} ({}) rtl={} paras={} verses={} in {:?}",
            b.abbrev,
            b.title,
            b.rtl,
            b.paras.len(),
            b.verse_at.len(),
            t.elapsed()
        );
        let jn = Verse::new(obs_scripture::Book::John, 1, 1);
        match b.locate_standard(jn) {
            Some((p, _)) => println!(
                "  Jn 1:1 -> {}",
                b.paras[p].text.chars().take(100).collect::<String>()
            ),
            None => println!("  Jn 1:1 -> not in this Bible"),
        }
    }
}
