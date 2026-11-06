//! Export Notes and Print / Save as PDF (#154).
//!
//! - **Export** (Markdown or HTML) asks where to save with the native save dialog, on its own
//!   thread like Import's picker, then runs `notes.export` with that path.
//! - **Print / Save as PDF** writes a print page (an HTML document that opens the print
//!   dialog when loaded) to the temporary folder and opens it in the system browser, whose
//!   print dialog prints it or saves it as a PDF. The app has no printing of its own.

#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use obs_engine::NotesFormat;
use serde_json::{Value, json};

use super::State;

/// Asks where to save an export: (suggested file name, format) → the chosen path.
pub(crate) type SaveDialog = Arc<dyn Fn(&str, NotesFormat) -> Option<PathBuf> + Send + Sync>;

/// A Notes export waiting for its save dialog.
pub(crate) struct ExportUi {
    pub(super) save_dialog: SaveDialog,
    /// The `notes.export` params and the dialog's answer.
    waiting: Option<Receiver<(Value, Option<PathBuf>)>>,
}

impl Default for ExportUi {
    fn default() -> Self {
        Self {
            save_dialog: Arc::new(native_save_dialog),
            waiting: None,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn native_save_dialog(name: &str, format: NotesFormat) -> Option<PathBuf> {
    let (label, ext) = match format {
        NotesFormat::Markdown => ("Markdown", "md"),
        NotesFormat::Html | NotesFormat::Print => ("HTML", "html"),
    };
    rfd::FileDialog::new()
        .set_title("Export Notes")
        .set_file_name(name)
        .add_filter(label, &[ext])
        .save_file()
}

#[cfg(target_arch = "wasm32")]
fn native_save_dialog(_: &str, _: NotesFormat) -> Option<PathBuf> {
    None
}

/// A `file://` URL for a local path.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn file_url(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    if text.starts_with('/') {
        format!("file://{text}")
    } else {
        format!("file:///{text}")
    }
}

/// Where print pages are written: `<temp>/openbible-print/<name>.html`.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn print_path(name: &str) -> PathBuf {
    let stem: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " -_.,".contains(c) {
                c
            } else {
                ' '
            }
        })
        .collect();
    let stem = stem.split_whitespace().collect::<Vec<_>>().join(" ");
    let stem = if stem.is_empty() {
        "print".into()
    } else {
        stem
    };
    std::env::temp_dir()
        .join("openbible-print")
        .join(format!("{stem}.html"))
}

impl State {
    /// Export Notes as `format` (Markdown or HTML): ask where, then write. `params` chooses
    /// what (`notebook` or `tab`, see `notes.export`).
    pub(super) fn export_notes(&mut self, ctx: &egui::Context, params: Value, format: NotesFormat) {
        let mut params = params;
        params["format"] = json!(format.id());
        // The file name, and a check that there is something to export.
        let Some(out) = self.run("notes.export", &params) else {
            return;
        };
        let name = out["file_name"].as_str().unwrap_or("Notes").to_string();
        let dialog = Arc::clone(&self.export_ui.save_dialog);
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("obs-export-dialog".into())
            .spawn(move || {
                let path = dialog(&name, format);
                let _ = tx.send((params, path));
                ctx.request_repaint();
            });
        match spawned {
            Ok(_) => self.export_ui.waiting = Some(rx),
            Err(e) => self.toast(format!("Could not open the save dialog: {e}")),
        }
    }

    /// Write the export once its save dialog has answered; never wait for it.
    pub(super) fn poll_export(&mut self) {
        let Some(rx) = &self.export_ui.waiting else {
            return;
        };
        match rx.try_recv() {
            Ok((mut params, path)) => {
                self.export_ui.waiting = None;
                let Some(path) = path else {
                    return;
                };
                params["path"] = json!(path.to_string_lossy());
                if let Some(out) = self.run("notes.export", &params) {
                    let n = out["annotations"].as_u64().unwrap_or(0);
                    self.toast(format!(
                        "Exported {n} Annotation{} to {}",
                        if n == 1 { "" } else { "s" },
                        path.display()
                    ));
                }
            }
            Err(TryRecvError::Disconnected) => {
                self.export_ui.waiting = None;
                self.toast("The save dialog closed unexpectedly");
            }
            Err(TryRecvError::Empty) => {}
        }
    }

    /// Whether an export is waiting for its save dialog (frames keep polling).
    pub(super) fn export_waiting(&self) -> bool {
        self.export_ui.waiting.is_some()
    }

    /// Print Notes: the print page of `notes.export` (`params` chooses what).
    pub(super) fn print_notes(&mut self, ctx: &egui::Context, params: Value) {
        let mut params = params;
        params["format"] = json!(NotesFormat::Print.id());
        let Some(out) = self.run("notes.export", &params) else {
            return;
        };
        let title = out["title"].as_str().unwrap_or("Notes").to_string();
        let page = out["text"].as_str().unwrap_or_default().to_string();
        self.open_print_page(ctx, &title, &page);
    }

    /// Write a print page and open it in the system browser.
    #[cfg(target_arch = "wasm32")]
    pub(super) fn open_print_page(&mut self, _: &egui::Context, _: &str, _: &str) {
        self.toast("Printing needs the desktop app");
    }

    /// Write a print page and open it in the system browser.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn open_print_page(&mut self, ctx: &egui::Context, name: &str, page: &str) {
        let path = print_path(name);
        let written = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&path, page));
        match written {
            Ok(()) => {
                ctx.open_url(egui::OpenUrl::new_tab(file_url(&path)));
                self.toast("Opened in your browser: print it, or choose Save as PDF");
            }
            Err(e) => self.toast(format!("Could not write {}: {e}", path.display())),
        }
    }
}
