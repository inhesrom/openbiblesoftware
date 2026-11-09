//! eframe app, the reading pane (glyph-atlas painter over the virtualized document), and later
//! the egui_tiles Workspace and Tools.
//!
//! Layer L6: the only layer allowed to depend on egui, eframe, winit or rfd.

mod a11y;
pub mod bench;
mod compare_view;
mod doc;
mod filters;
mod find;
mod heights;
pub mod icons;
mod interlinear;
mod key_labels;
mod marks;
mod palette;
mod pane;
mod rich_clipboard;
pub mod search_view;
mod selection;
mod shell;
mod system_fonts;
pub mod theme;
mod view;
mod workers;

use std::path::PathBuf;

pub use compare_view::{
    ComparisonContext, ComparisonOutput, ComparisonRow, ComparisonView, comparison_pane,
};
pub use doc::{DocBlock, ReadingDoc};
pub use find::{FindMatch, Found};
pub use heights::HeightIndex;
pub use interlinear::InterlinearSource;
pub use marks::{HighlightLooks, Mark, MarkKind, marks_for};
pub use palette::{ContrastShort, Palette, StylePaint, contrast};
pub use pane::{AtlasTexture, MEASURE_EMS, PaneOutput, reading_pane};
pub use rich_clipboard::RichClipboard;
pub use selection::{
    DocPos, Granularity, Selection, copy_text, range_rects, selected_text, selection_rects, word_at,
};
pub use shell::{DesktopUnits, Shell, parse_shortcut};
pub use system_fonts::{FontFile, InstalledFamily, NoSystemFonts, SystemFonts};
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
    input_script: Vec<(u32, egui::Event)>,
    shot: Option<(u32, PathBuf)>,
    /// Open Library → Starter library… on the first frame, to offer the download (#131).
    /// Not at construction: its list comes through the transport [`Self::with_fetch`] sets.
    offer_starter: bool,
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
            // Get Bibles opens on the last fetched catalog, even offline (#91).
            if let Some(cache) = obs_engine::get_bibles::default_catalog_cache() {
                session.get_bibles.set_cache_path(cache);
            }
            // Search indexes (#58): Imports build theirs into the cache folder (this installs
            // the Import indexer); missing or stale ones rebuild in the background.
            if let Some(dir) = obs_engine::default_search_cache_dir() {
                session.set_search_cache_dir(dir);
            }
            session.refresh_search_indexes();
            // People & places ↔ Dictionary aliases (#54): a derived cache per pair, rebuilt in
            // the background when either Resource changed (or used prebuilt for the Starter).
            if let Some(dir) = obs_engine::aliases::default_alias_cache_dir() {
                session.set_alias_cache_dir(dir);
            }
            session.refresh_alias_caches();
            // Help → Open log folder and Report a problem (#219).
            if let Some(dir) = obs_engine::diagnostics::default_log_dir() {
                session.set_log_dir(dir);
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
        // The Starter library (#131): added from the bundle on the first run (newer revisions
        // and new Starter Resources later); without a bundle, offered for download once.
        #[cfg(not(target_arch = "wasm32"))]
        let offer_starter = {
            session
                .starter
                .set_download_base(&obs_engine::starter::default_download_base());
            let sync =
                session.sync_starter_library(obs_engine::starter::find_starter_bundle().as_deref());
            problems.extend(sync.problems);
            sync.offer_download
        };
        #[cfg(target_arch = "wasm32")]
        let offer_starter = false;
        let mut shell = Shell::new(session);
        shell.restore_at_launch();
        // A Crash report from the last run is offered, on top of the restored Workspace.
        shell.offer_crash_report();
        // A file given on the command line shows on top of the restored Workspace and Home.
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(p) = &path
            && let Some(id) = shell
                .session()
                .library
                .entries()
                .iter()
                .find(|e| e.path == *p)
                .map(|e| e.manifest.resource_id.clone())
        {
            shell.show_startup_resource(&id);
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
            input_script: Vec::new(),
            shot: None,
            offer_starter,
        }
    }

    /// Hand the engine the app's transport for SWORD repositories (Get Bibles, #91). Without
    /// one, only local `file://` repositories answer.
    pub fn with_fetch(mut self, fetch: std::sync::Arc<dyn obs_engine::sword_repo::Fetch>) -> Self {
        self.shell.session_mut().set_fetch(fetch);
        self
    }

    /// The user's OS language (BCP 47, `bn-BD`), whose Resources head Get Resources' language
    /// filter (#244).
    pub fn with_system_language(mut self, tag: Option<String>) -> Self {
        self.shell.session_mut().get_bibles.set_system_language(tag);
        self
    }

    /// Hand the shell the app's installed fonts (View → Fonts, #138). Without them, only the
    /// bundled fonts can be chosen.
    pub fn with_system_fonts(mut self, fonts: std::sync::Arc<dyn SystemFonts>) -> Self {
        self.shell.set_system_fonts(fonts);
        self
    }

    /// The system clipboard, for copying rich text (Copy verses, #154). Without one, rich
    /// text is copied as plain text.
    pub fn with_rich_clipboard(mut self, clipboard: std::sync::Arc<dyn RichClipboard>) -> Self {
        self.shell.set_rich_clipboard(clipboard);
        self
    }

    /// Smoke-check automation. `OBS_SCRIPT="frame command {json}; …"` runs commands through
    /// the registry at the given frames (a `"tab": "focused"` parameter means the focused
    /// Tab). `OBS_SHOT=out.ppm` saves a screenshot at `OBS_SHOT_FRAME` (default 60) and closes.
    /// `OBS_INPUT` is a JSON array of `[frame, egui::Event]` pairs, injected before that
    /// frame's UI. It exercises pointer/keyboard-only controls without OS input permissions.
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
        if let Ok(input) = std::env::var("OBS_INPUT") {
            match serde_json::from_str(&input) {
                Ok(events) => self.input_script = events,
                Err(e) => eprintln!("openbible: invalid OBS_INPUT: {e}"),
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
        if self.script.is_empty() && self.input_script.is_empty() && self.shot.is_none() {
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

    /// Files dropped on the window from the system this frame (a `.obr`) join the library and
    /// open; drags inside the Workspace (#216) never carry files, so this is unaffected by them.
    #[cfg(not(target_arch = "wasm32"))]
    fn files_dropped(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| {
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
                self.shell.session_mut().refresh_search_index(&id);
                self.shell.session_mut().remember_resource_names();
                // A new revision of a Resource with Annotations re-anchors them (#65).
                self.shell.session_mut().reanchor_resource(&id);
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
    fn raw_input_hook(&mut self, _ctx: &egui::Context, input: &mut egui::RawInput) {
        for (_, event) in self
            .input_script
            .iter()
            .filter(|(at, _)| *at == self.frame + 1)
        {
            input.events.push(event.clone());
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if !self.styled {
            theme::install(ui.ctx());
            self.styled = true;
        }
        // Floating windows (#152) are kept on screens that are connected.
        #[cfg(not(target_arch = "wasm32"))]
        if self.shell.wants_screens()
            && let Some(window) = frame.winit_window()
        {
            let zoom = ui.ctx().zoom_factor();
            let units = DesktopUnits::NATIVE;
            let mut screens: Vec<obs_engine::ScreenRect> = Vec::new();
            for m in window
                .current_monitor()
                .into_iter()
                .chain(window.available_monitors())
            {
                let (at, size) = (m.position(), m.size());
                let screen = units.screen(
                    (at.x, at.y),
                    (size.width, size.height),
                    m.scale_factor() as f32,
                    zoom,
                );
                if !screens.contains(&screen) {
                    screens.push(screen);
                }
            }
            self.shell.set_screens(screens);
        }
        #[cfg(target_arch = "wasm32")]
        let _ = frame;
        #[cfg(not(target_arch = "wasm32"))]
        self.files_dropped(ui.ctx());
        if std::mem::take(&mut self.offer_starter) {
            self.shell.run("library.starter", &serde_json::json!({}));
        }
        self.shell.ui(ui);
        let ctx = ui.ctx().clone();
        self.automate(&ctx);
    }
}

#[cfg(test)]
mod tests;
