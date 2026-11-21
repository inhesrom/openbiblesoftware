//! PROTOTYPE (GPUI comparison, branch `prototype-gpui-instead-of-egui` only; never merge).
//!
//! `OBS_PROTO_BENCH=prototype/gpui-shell/scenario.json openbible` runs the windowed scenario
//! the GPUI prototype runs (`prototype/gpui-shell`), in this egui app, and writes the same
//! JSON report to `OBS_PROTO_BENCH_OUT`. Inert unless that variable is set.
//!
//! Like the prototype it opens only the scenario's Bibles (read-only, nothing added to the
//! library), with no User database, no Search indexing, no Starter sync, no network and no
//! update check, so both apps do the same work. The Bibles open in Panels side by side, all
//! in Link set A; the scenario scrolls the first Panel with trackpad-style wheel events over
//! it (applied in full the frame they arrive, as the prototype's `scroll_by` is) and jumps it
//! with `tab.go`; the others follow as they do for a user. Motion is reduced, so a jump lands
//! at once as in the prototype.

use std::time::Instant;

use obs_engine::TabId;
use serde_json::json;

use crate::App;

#[path = "../../../prototype/gpui-shell/src/bench.rs"]
#[allow(clippy::all, dead_code)]
mod shared;

pub use shared::Scenario;

pub fn load_scenario(path: &str) -> Result<Scenario, String> {
    shared::load(path)
}

pub struct ProtoBench {
    app: App,
    started: Instant,
    scenario: Scenario,
    out: String,
    hz: f64,
    /// The Bibles' Tabs, leader first.
    tabs: Vec<TabId>,
    /// Where the scenario is; None until every Panel has been drawn once.
    next: Option<shared::Cursor>,
    /// This frame's phase and action, chosen before its input is read.
    current: (String, shared::Action),
    wheel_active: bool,
    rec: shared::Recorder,
    /// The eframe app creator was entered (winit and GL are up).
    framework_ready_ms: f64,
    /// `ProtoBench::new` returned: Shell built, the four Bibles opened and read.
    app_built_ms: f64,
    window_ms: Option<f64>,
    first_frame_ms: Option<f64>,
    ready_ms: Option<f64>,
    done: bool,
}

impl ProtoBench {
    pub fn new(started: Instant, scenario: Scenario, out: String, hz: f64) -> Self {
        let framework_ready_ms = started.elapsed().as_secs_f64() * 1000.0;
        let mut session = obs_engine::Session::new(obs_engine::JobMode::default_for_host());
        let dir = obs_engine::default_library_dir().unwrap_or_default();
        for id in &scenario.bibles {
            let path = dir.join(format!("{id}.obr"));
            if let Err(e) = session.library.add_file(&path) {
                eprintln!("proto bench: {}: {e}", path.display());
            }
        }
        session.appearance.reduce_motion = true;
        // The light theme, as the GPUI prototype draws (not the system's, which Xvfb has none of).
        session.appearance.theme = obs_engine::appearance::Theme::Light;
        let mut app = App {
            shell: crate::Shell::new(session),
            styled: false,
            frame: 0,
            script: Vec::new(),
            input_script: Vec::new(),
            shot: None,
            steps: crate::steps::Steps::default(),
            offer_starter: false,
        };
        let shell = app.shell();
        shell.run("tour.skip", &json!({}));
        let mut tabs = Vec::new();
        for (i, id) in scenario.bibles.iter().enumerate() {
            let out = shell.run("resource.open", &json!({ "id": id, "split": i > 0 }));
            if let Some(t) = out.and_then(|v| v["tab"].as_u64()) {
                shell.run("tab.link", &json!({ "tab": t, "set": "A" }));
                tabs.push(TabId(t));
            }
        }
        if let Some(&t) = tabs.first() {
            shell.session_mut().workspace.focus(t);
            shell.apply_effects();
        }
        Self {
            app,
            started,
            scenario,
            out,
            hz,
            tabs,
            next: None,
            current: ("setup".into(), shared::Action::None),
            wheel_active: false,
            rec: shared::Recorder::default(),
            framework_ready_ms,
            app_built_ms: started.elapsed().as_secs_f64() * 1000.0,
            window_ms: None,
            first_frame_ms: None,
            ready_ms: None,
            done: false,
        }
    }

    fn ms(&self, at: Instant) -> f64 {
        at.duration_since(self.started).as_secs_f64() * 1000.0
    }

    fn finish(&mut self, ctx: &egui::Context) {
        self.done = true;
        let extra = json!({
            "app": "egui",
            "egui_version": "0.37",
            "window": self.scenario.window,
            "bibles": self.scenario.bibles,
            "frames": self.scenario.frames.len(),
            "startup": {
                "framework_ready_ms": self.framework_ready_ms,
                "data_loaded_ms": self.app_built_ms,
                "window_ms": self.window_ms,
                "first_frame_ms": self.first_frame_ms,
                "ready_ms": self.ready_ms,
            },
        });
        let report = shared::report(&self.rec, self.hz, extra);
        let text = serde_json::to_string_pretty(&report).unwrap_or_default();
        match std::fs::write(&self.out, text) {
            Ok(()) => eprintln!("bench: wrote {}", self.out),
            Err(e) => eprintln!("bench: {}: {e}", self.out),
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

impl eframe::App for ProtoBench {
    fn raw_input_hook(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
        let now = Instant::now();
        if self.window_ms.is_none() {
            self.window_ms = Some(self.ms(now));
        }
        // Ready once every Bible's reading pane has been drawn.
        let rects: Vec<Option<[f32; 4]>> = self
            .tabs
            .iter()
            .map(|t| self.app.shell.proto_view_rect(*t))
            .collect();
        if self.next.is_none()
            && !rects.is_empty()
            && rects.iter().all(|r| r.is_some_and(|[l, _, r, _]| r > l))
        {
            self.ready_ms = Some(self.ms(now));
            self.next = Some(shared::Cursor::default());
        }
        self.current = match &mut self.next {
            None => ("setup".into(), shared::Action::None),
            Some(cursor) => cursor
                .step(&self.scenario, now)
                .unwrap_or_else(|| ("done".into(), shared::Action::None)),
        };
        if !self.done {
            self.rec.frame_start(now, &self.current.0);
        }
        // Wheel events over the leader's reading pane.
        let scroll = match self.current.1 {
            shared::Action::Scroll(dy) => Some(dy),
            _ => None,
        };
        let wheel = |delta: f32, phase: egui::TouchPhase| egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -delta),
            phase,
            source: egui::MouseWheelSource::Trackpad,
            modifiers: egui::Modifiers::default(),
        };
        match (scroll, self.wheel_active) {
            (Some(dy), active) => {
                if !active {
                    if let Some(Some([l, t, r, b])) = rects.first() {
                        let at = egui::pos2((l + r) / 2.0, (t + b) / 2.0);
                        input.events.push(egui::Event::PointerMoved(at));
                    }
                    input.events.push(wheel(0.0, egui::TouchPhase::Start));
                    self.wheel_active = true;
                }
                input.events.push(wheel(dy, egui::TouchPhase::Move));
            }
            (None, true) => {
                input.events.push(wheel(0.0, egui::TouchPhase::End));
                self.wheel_active = false;
            }
            (None, false) => {}
        }
        self.app.raw_input_hook(ctx, input);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if let Some(secs) = frame.info().cpu_usage
            && !self.done
        {
            self.rec.previous_frame_cpu(f64::from(secs) * 1000.0);
        }
        match self.current.1.clone() {
            shared::Action::Jump(to) => {
                if let Some(t) = self.tabs.first() {
                    let tab = t.0;
                    self.app
                        .shell
                        .run("tab.go", &json!({ "tab": tab, "text": to }));
                }
            }
            shared::Action::Shot(name) => shared::mark_shot("egui", &name),
            shared::Action::Scroll(_) | shared::Action::None | shared::Action::Settle(_) => {}
        }
        if self.current.0 == "done" && !self.done {
            self.finish(ui.ctx());
        }
        eframe::App::ui(&mut self.app, ui, frame);
        let end = Instant::now();
        if self.first_frame_ms.is_none() {
            self.first_frame_ms = Some(self.ms(end));
        }
        if !self.done {
            self.rec.paint_end(end);
        }
        ui.ctx().request_repaint();
    }
}
