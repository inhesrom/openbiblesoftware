//! eframe app, the reading pane (glyph-atlas painter over the virtualized document), and later
//! the egui_tiles Workspace and Tools.
//!
//! Layer L6: the only layer allowed to depend on egui, eframe, winit or rfd.

mod a11y;
pub mod bench;
mod compare_view;
mod doc;
mod heights;
mod palette;
mod pane;
mod selection;
mod shell;
pub mod theme;
mod view;
mod workers;

use std::path::PathBuf;

pub use compare_view::{ComparisonOutput, ComparisonView, comparison_pane};
pub use doc::{DocBlock, ReadingDoc};
pub use heights::HeightIndex;
pub use palette::{HIGHLIGHT_STYLES, Palette, contrast};
pub use pane::{AtlasTexture, MEASURE_EMS, PaneOutput, reading_pane};
pub use selection::{
    DocPos, Granularity, Selection, copy_text, selected_text, selection_rects, word_at,
};
pub use shell::{Shell, parse_shortcut};
pub use view::{Frame, FrameBlock, FrameStats, ReadingView, SYNC_BUDGET, VisibleBlock};
pub use workers::{LayoutClient, SnippetCache, lay_out, set_layout_slowdown};

/// Window title used until the product is named.
pub const APP_TITLE: &str = "Open Bible Software";

/// The desktop application.
pub struct App {
    shell: Shell,
    styled: bool,
    frame: u32,
    /// `OBS_SCRIPT`: commands to run at given frames; `OBS_SHOT`: a screenshot to take.
    script: Vec<(u32, String, serde_json::Value)>,
    shot: Option<(u32, PathBuf)>,
}

impl App {
    /// Start with the library (the default library folder) and, if given, a Resource file,
    /// added to it and opened. Like any Resource, that file takes its place in the Preferred
    /// order: at the bottom of its type unless the order already ranks it.
    pub fn with_file(path: Option<PathBuf>) -> Self {
        #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
        let mut session = obs_engine::Session::new(obs_engine::JobMode::default_for_host());
        let mut problems = Vec::new();
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(p) = &path
                && let Err(e) = session.library.add_file(p)
            {
                problems.push(format!("{}: {e}", p.display()));
            }
            if let Some(dir) = obs_engine::default_library_dir() {
                for (f, e) in session.library.scan(&dir) {
                    problems.push(format!("{}: {e}", f.display()));
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        if let Some(p) = &path {
            problems.push(format!(
                "{}: opening files is not supported here",
                p.display()
            ));
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(db) = obs_engine::default_user_db() {
            match obs_store::UserStore::open(&db) {
                Ok((store, _)) => session.attach_user(Box::new(store)),
                Err(e) => problems.push(format!("User database {}: {e}", db.display())),
            }
        }
        let mut shell = Shell::new(session);
        shell.restore_or_default();
        // A file given on the command line opens on top of the restored Workspace.
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(p) = &path
            && let Some(id) = shell
                .session()
                .library
                .entries()
                .iter()
                .find(|e| e.path == *p)
                .map(|e| e.manifest.resource_id.clone())
            && !shell
                .session()
                .workspace
                .tabs()
                .any(|t| t.kind == obs_engine::TabKind::Resource(id.clone()))
        {
            shell.run("resource.open", &serde_json::json!({ "id": id }));
        }
        if let Some(p) = problems.first() {
            shell
                .session_mut()
                .workspace
                .push_effect(obs_engine::Effect::Message(p.clone()));
            shell.apply_effects();
        }
        Self {
            shell,
            styled: false,
            frame: 0,
            script: Vec::new(),
            shot: None,
        }
    }

    /// Smoke-check automation. `OBS_SCRIPT="frame command {json}; …"` runs commands through
    /// the registry at the given frames (a `"tab": "focused"` parameter means the focused
    /// Tab). `OBS_SHOT=out.ppm` saves a screenshot at `OBS_SHOT_FRAME` (default 60) and closes.
    pub fn with_automation_from_env(mut self) -> Self {
        if let Ok(script) = std::env::var("OBS_SCRIPT") {
            for step in script.split(';') {
                let mut it = step.trim().splitn(3, ' ');
                let (Some(f), Some(cmd)) = (it.next(), it.next()) else {
                    continue;
                };
                let params = it
                    .next()
                    .and_then(|j| serde_json::from_str(j).ok())
                    .unwrap_or(serde_json::json!({}));
                if let Ok(f) = f.parse() {
                    self.script.push((f, cmd.to_string(), params));
                }
            }
        }
        if let Ok(path) = std::env::var("OBS_SHOT") {
            let at = std::env::var("OBS_SHOT_FRAME")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(60);
            self.shot = Some((at, PathBuf::from(path)));
        }
        self
    }

    fn automate(&mut self, ctx: &egui::Context) {
        self.frame += 1;
        if self.script.is_empty() && self.shot.is_none() {
            return;
        }
        ctx.request_repaint();
        let now = self.frame;
        let due: Vec<(String, serde_json::Value)> = self
            .script
            .iter()
            .filter(|(f, _, _)| *f == now)
            .map(|(_, c, p)| (c.clone(), p.clone()))
            .collect();
        for (cmd, mut params) in due {
            if params.get("tab").is_some_and(serde_json::Value::is_string)
                && let Some(t) = self.shell.session().workspace.focused()
            {
                params["tab"] = serde_json::json!(t.0);
            }
            self.shell.run(&cmd, &params);
        }
        if let Some((at, path)) = &self.shot {
            if now == *at {
                let path = path.clone();
                let c2 = ctx.clone();
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                    egui::ScreenshotCallback::new(move |img| {
                        let [w, h] = img.size;
                        let mut out = format!("P6 {w} {h} 255\n").into_bytes();
                        for p in &img.pixels {
                            out.extend([p.r(), p.g(), p.b()]);
                        }
                        if let Err(e) = std::fs::write(&path, out) {
                            eprintln!("openbible: {}: {e}", path.display());
                        }
                        c2.send_viewport_cmd(egui::ViewportCommand::Close);
                        c2.request_repaint();
                    }),
                ));
            }
            if now > *at + 240 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    pub fn shell(&mut self) -> &mut Shell {
        &mut self.shell
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn dropped(&mut self, path: PathBuf) {
        let added = self
            .shell
            .session_mut()
            .library
            .add_file(&path)
            .map(|e| e.manifest.resource_id.clone());
        match added {
            Ok(id) => {
                self.shell
                    .run("resource.open", &serde_json::json!({ "id": id }));
            }
            Err(e) => {
                let msg = format!("{}: {e}", path.display());
                self.shell
                    .session_mut()
                    .workspace
                    .push_effect(obs_engine::Effect::Message(msg));
                self.shell.apply_effects();
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if !self.styled {
            theme::install(ui.ctx());
            self.styled = true;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let dropped = ui.ctx().input(|i| {
                i.raw
                    .dropped_files
                    .iter()
                    .map(|f| f.path().to_path_buf())
                    .filter(|p| !p.as_os_str().is_empty())
                    .collect::<Vec<_>>()
            });
            for p in dropped {
                self.dropped(p);
            }
        }
        self.shell.ui(ui);
        let ctx = ui.ctx().clone();
        self.automate(&ctx);
    }
}

#[cfg(test)]
mod tests;
