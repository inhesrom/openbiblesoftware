//! Export Notes and Print / Save as PDF (#154).
//!
//! - **Export** (Markdown or HTML) asks where to save with the native save dialog, on its own
//!   thread like Import's picker, then runs `notes.export` with that path.
//! - **Print / Save as PDF** writes a print page (an HTML document that opens the print
//!   dialog when loaded) to a [`PrintDir`] and opens it in the system browser, whose print
//!   dialog prints it or saves it as a PDF. The app has no printing of its own.

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
    /// Where print pages go, made at the first print; it lives as long as the app, so the
    /// browser can load (and reload) the pages.
    #[cfg(not(target_arch = "wasm32"))]
    print_dir: Option<PrintDir>,
}

impl Default for ExportUi {
    fn default() -> Self {
        Self {
            save_dialog: Arc::new(native_save_dialog),
            waiting: None,
            #[cfg(not(target_arch = "wasm32"))]
            print_dir: None,
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

/// A `file://` URL for a local path, percent-encoded (print page names have spaces).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn file_url(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut url = String::from(if text.starts_with('/') {
        "file://"
    } else {
        "file:///"
    });
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"/:-._~".contains(&b) {
            url.push(char::from(b));
        } else {
            url.push_str(&format!("%{b:02X}"));
        }
    }
    url
}

/// A folder of this run's print pages, in the temporary folder, that no one else can read
/// or write: a new, uniquely named directory made only for the owner (`0700` on Unix), never
/// an existing one, so another user of a shared `/tmp` can neither read the Notes printed
/// nor plant a symlink there for a page to be written through. Deleted with the app.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct PrintDir {
    path: PathBuf,
}

#[cfg(not(target_arch = "wasm32"))]
impl PrintDir {
    /// Make a new print folder in `parent`.
    pub(crate) fn create(parent: &Path) -> std::io::Result<Self> {
        use std::hash::{BuildHasher, RandomState};
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        for attempt in 0u32..100 {
            // A name others cannot guess (std's hasher keys are random per process).
            let token = RandomState::new().hash_one((std::process::id(), attempt));
            let path = parent.join(format!("openbible-print-{token:016x}"));
            match builder.create(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("no free print folder name in {}", parent.display()),
        ))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Write print page `page` as a new file named after `name` (`<name>.html`, or
    /// `<name> 2.html` and so on when printed again), readable only by the owner.
    pub(crate) fn write_page(&self, name: &str, page: &str) -> std::io::Result<PathBuf> {
        use std::io::Write;
        let stem = print_stem(name);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        for n in 1u32..=1000 {
            let file = if n == 1 {
                format!("{stem}.html")
            } else {
                format!("{stem} {n}.html")
            };
            let path = self.path.join(file);
            match options.open(&path) {
                Ok(mut f) => {
                    f.write_all(page.as_bytes())?;
                    return Ok(path);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("too many print pages named {stem}"),
        ))
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for PrintDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// A print page's file name (without extension) for `name`: characters file systems or
/// URLs could trip on become spaces.
#[cfg(not(target_arch = "wasm32"))]
fn print_stem(name: &str) -> String {
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
    // No `.`, `..` or hidden file from a name starting or ending in dots.
    let stem = stem.trim_matches(['.', ' ']);
    if stem.is_empty() {
        "print".into()
    } else {
        stem.into()
    }
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
        let dir = match &mut self.export_ui.print_dir {
            Some(dir) => dir,
            empty @ None => match PrintDir::create(&std::env::temp_dir()) {
                Ok(dir) => empty.insert(dir),
                Err(e) => {
                    let temp = std::env::temp_dir();
                    self.toast(format!(
                        "Could not make a print folder in {}: {e}",
                        temp.display()
                    ));
                    return;
                }
            },
        };
        match dir.write_page(name, page) {
            Ok(path) => {
                ctx.open_url(egui::OpenUrl::new_tab(file_url(&path)));
                self.toast("Opened in your browser: print it, or choose Save as PDF");
            }
            Err(e) => {
                let at = dir.path().display().to_string();
                self.toast(format!("Could not write a print page in {at}: {e}"));
            }
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::PrintDir;

    /// A scratch folder standing in for the shared temporary folder.
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("obs-print-dir-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn print_pages_go_to_a_new_private_folder_and_never_through_a_planted_symlink() {
        let tmp = scratch("symlink");
        let victim = tmp.join("victim.txt");
        std::fs::write(&victim, "precious").unwrap();
        // Another user planted the old fixed folder, with a symlink named for the Notebook.
        let planted = tmp.join("openbible-print");
        std::fs::create_dir(&planted).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&victim, planted.join("My Notes.html")).unwrap();

        let dir = PrintDir::create(&tmp).unwrap();
        assert_ne!(dir.path(), planted);
        assert_eq!(dir.path().parent(), Some(tmp.as_path()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path()).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700, "{mode:o}");
        }
        let page = dir.write_page("My Notes", "<p>Private</p>").unwrap();
        assert_eq!(page, dir.path().join("My Notes.html"));
        assert_eq!(std::fs::read_to_string(&page).unwrap(), "<p>Private</p>");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&page).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "{mode:o}");
            // Even a symlink inside the folder is not written through: pages are new files.
            std::os::unix::fs::symlink(&victim, dir.path().join("Again.html")).unwrap();
            let again = dir.write_page("Again", "<p>Second</p>").unwrap();
            assert_eq!(again, dir.path().join("Again 2.html"));
        }
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "precious");

        // Printing the same Notebook again keeps the first page (the browser may show it).
        let second = dir.write_page("My Notes", "<p>Later</p>").unwrap();
        assert_eq!(second, dir.path().join("My Notes 2.html"));
        assert_eq!(std::fs::read_to_string(&page).unwrap(), "<p>Private</p>");

        // A second app gets its own folder; each goes away with its app.
        let other = PrintDir::create(&tmp).unwrap();
        assert_ne!(other.path(), dir.path());
        let path = dir.path().to_path_buf();
        drop(dir);
        assert!(!path.exists());
        drop(other);
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn print_page_names_cannot_leave_the_print_folder() {
        let tmp = scratch("names");
        let dir = PrintDir::create(&tmp).unwrap();
        for (name, file) in [
            ("../../etc/passwd", "etc passwd.html"),
            ("..", "print.html"),
            ("Notes matching “grace”", "Notes matching grace.html"),
        ] {
            let page = dir.write_page(name, "x").unwrap();
            assert_eq!(page, dir.path().join(file), "{name}");
        }
        drop(dir);
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
