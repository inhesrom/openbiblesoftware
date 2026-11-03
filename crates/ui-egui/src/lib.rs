//! eframe app, the reading pane (glyph-atlas painter over the virtualized document), and later
//! the egui_tiles Workspace and Tools.
//!
//! Layer L6: the only layer allowed to depend on egui, eframe, winit or rfd.

mod doc;
mod heights;
mod palette;
mod pane;
mod view;

use std::path::PathBuf;

pub use doc::{DocBlock, ReadingDoc};
pub use heights::HeightIndex;
use obs_scripture::parse_reference;
pub use palette::Palette;
pub use pane::{AtlasTexture, reading_pane};
pub use view::{ReadingView, VisibleBlock};

/// Window title used until the product is named.
pub const APP_TITLE: &str = "Open Bible Software";

struct Open {
    view: ReadingView,
    path: PathBuf,
}

/// The desktop application state.
#[derive(Default)]
pub struct App {
    open: Option<Open>,
    atlas: AtlasTexture,
    reference: String,
    message: Option<String>,
    dark: bool,
}

impl App {
    /// Start with a Resource file open, if one is given.
    pub fn with_file(path: Option<PathBuf>) -> Self {
        let mut app = Self::default();
        if let Some(p) = path {
            app.open_file(p);
        }
        app
    }

    #[cfg(target_arch = "wasm32")]
    fn open_file(&mut self, path: PathBuf) {
        self.message = Some(format!(
            "{}: opening files is not supported here",
            path.display()
        ));
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn open_file(&mut self, path: PathBuf) {
        match ReadingDoc::open(&path) {
            Ok(doc) => {
                self.open = Some(Open {
                    view: ReadingView::new(
                        std::sync::Arc::new(doc),
                        obs_layout::TextSettings::default(),
                    ),
                    path,
                });
                self.message = None;
            }
            Err(e) => self.message = Some(format!("{}: {e}", path.display())),
        }
    }

    fn go(&mut self) {
        let Some(open) = self.open.as_mut() else {
            return;
        };
        let context = open.view.current_location();
        match parse_reference(&self.reference, &open.view.doc.versification, context) {
            Ok(list) => {
                if let Some(p) = list.passages().first()
                    && !open.view.jump_to(p.start())
                {
                    self.message = Some(format!("{} is not in this Bible", p.start()));
                } else {
                    self.message = None;
                }
            }
            Err(e) => self.message = Some(e.kind.to_string()),
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let dropped = ui.ctx().input(|i| {
            i.raw
                .dropped_files
                .first()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
        });
        if let Some(p) = dropped {
            self.open_file(p);
        }
        let palette = if self.dark {
            Palette::DARK
        } else {
            Palette::PAPER
        };
        egui::Panel::top("header").show(ui, |ui| {
            ui.horizontal(|ui| {
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.reference)
                        .hint_text("Go to… (Jn 3:16, Ps 51, 2:4)")
                        .desired_width(220.0),
                );
                if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    self.go();
                }
                if ui.button("Go").clicked() {
                    self.go();
                }
                if let Some(open) = self.open.as_mut() {
                    if let Some(v) = open.view.current_location() {
                        ui.label(egui::RichText::new(v.to_string()).strong());
                    }
                    ui.separator();
                    ui.label(&open.view.doc.manifest.title);
                    ui.weak(open.path.display().to_string());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.toggle_value(&mut self.dark, "Dark page");
                });
            });
            if let Some(m) = &self.message {
                ui.colored_label(ui.visuals().warn_fg_color, m);
            }
        });
        egui::CentralPanel::default().show(ui, |ui| match self.open.as_mut() {
            Some(open) => reading_pane(ui, &mut open.view, &mut self.atlas, &palette),
            None => {
                ui.centered_and_justified(|ui| {
                    ui.label("Open a Bible: run `openbible path/to/bible.obr`, or drop a .obr file here.\nMake one with `obs-cli import <usfm dir> -o bible.obr`.");
                });
            }
        });
    }
}

#[cfg(test)]
mod tests;
