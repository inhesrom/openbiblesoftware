//! Report a problem (#219): at launch, a new Crash report is offered; Help → Report a
//! problem… shows the newest one, or a problem report made now. The user reads the whole
//! report here first. Nothing leaves the computer unless the user copies it (Copy report) or
//! opens GitHub's new-issue form with it (Open GitHub issue…): the report is in the web
//! address, so opening the form sends it to GitHub, and submitting the issue there publishes
//! it. The window says so before either button is clicked. Help → Open log folder shows the
//! Log folder ([`obs_engine::diagnostics`]).
//!
//! Save report as zip… (and Help → Save diagnostics as zip…, with the log files only, #249)
//! lists what the Diagnostics zip will hold, asks where to save it, writes it with
//! `help.save_diagnostics_zip` and shows it in the file manager, for the user to attach to an
//! email or a message themselves.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use egui::{RichText, vec2};
use obs_engine::diagnostics::{self, issue_url, mark_seen, problem_report};
use obs_engine::{CrashReport, ZipEntry};
use serde_json::json;

use super::State;
use crate::theme;

/// Shows a folder in the system file manager. It returns once the file manager is started;
/// how that ends (it may exit with an error) comes later on the receiver, when there is one.
pub(crate) type FolderOpener =
    Arc<dyn Fn(&Path) -> Result<Option<FolderShown>, String> + Send + Sync>;

/// The file manager's outcome once it exits.
pub(crate) type FolderShown = Receiver<Result<(), String>>;

/// Asks where to save a Diagnostics zip: (suggested file name, suggested folder) → the path.
pub(crate) type ZipDialog = Arc<dyn Fn(&str, Option<&Path>) -> Option<PathBuf> + Send + Sync>;

/// What Report a problem's window says before Copy report and Open GitHub issue… are clicked.
pub(crate) const REVIEW_NOTE: &str = "Read the report below. Nothing has left this computer. \
     Copy report puts it on the clipboard. Open GitHub issue… sends it to GitHub inside the \
     address of the new-issue form; it becomes a public issue only if you submit the form. \
     Save report as zip… saves it with the log files in one file on this computer, for you \
     to attach to an email or a message yourself.";

/// What the zip windows say under the list of the zip's files.
pub(crate) const ZIP_NOTE: &str = "Nothing else: none of your Notes, Highlights, Workspace \
     or User database. Nothing is sent; you choose where to save it and whether to send it.";

const WINDOW: &str = "report-a-problem";

/// The Report a problem window's state across frames.
pub(crate) struct ReportProblemUi {
    pub(super) open_folder: FolderOpener,
    /// Shows a saved file in the file manager, selected where the system can.
    pub(super) reveal_file: FolderOpener,
    pub(super) save_dialog: ZipDialog,
    pub(super) shown: Option<Shown>,
    /// A folder or file being shown, until its file manager exits.
    pub(super) opening: Option<(PathBuf, FolderShown)>,
    /// The save dialog's answer, with the Crash report to include, waited for without
    /// blocking a frame.
    saving: Option<Receiver<(Option<String>, Option<PathBuf>)>>,
}

impl Default for ReportProblemUi {
    fn default() -> Self {
        Self {
            open_folder: Arc::new(system_open_folder),
            reveal_file: Arc::new(system_reveal_file),
            save_dialog: Arc::new(native_zip_dialog),
            shown: None,
            opening: None,
            saving: None,
        }
    }
}

/// What the window shows.
pub(crate) enum Shown {
    /// The launch offer for a new Crash report.
    Offer(CrashReport),
    /// A report to review: a Crash report, or (none) a problem report made now; `zip` is
    /// what Save report as zip… saves.
    Review {
        report: Option<CrashReport>,
        text: String,
        zip: Vec<ZipEntry>,
    },
    /// Help → Save diagnostics as zip…: what the zip of the log files holds.
    Diagnostics { zip: Vec<ZipEntry> },
}

/// What a click asks for, done after the window is drawn.
enum Act {
    Review,
    NotNow,
    Copy,
    OpenIssue,
    OpenFolder,
    SaveZip,
    Delete,
    Close,
}

#[cfg(not(target_arch = "wasm32"))]
fn native_zip_dialog(name: &str, folder: Option<&Path>) -> Option<PathBuf> {
    let dialog = rfd::FileDialog::new()
        .set_title("Save as zip")
        .set_file_name(name)
        .add_filter("Zip", &["zip"]);
    match folder {
        Some(f) => dialog.set_directory(f),
        None => dialog,
    }
    .save_file()
}

#[cfg(target_arch = "wasm32")]
fn native_zip_dialog(_: &str, _: Option<&Path>) -> Option<PathBuf> {
    None
}

/// Explorer selects the file (`/select,`); its exit code says nothing, so only starting it
/// can fail.
#[cfg(windows)]
fn system_reveal_file(path: &Path) -> Result<Option<FolderShown>, String> {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("explorer")
        .raw_arg(format!("/select,\"{}\"", path.display()))
        .spawn()
        .map_err(|e| format!("explorer: {e}"))?;
    Ok(None)
}

/// Finder selects the file (`open -R`).
#[cfg(target_os = "macos")]
fn system_reveal_file(path: &Path) -> Result<Option<FolderShown>, String> {
    let mut finder = std::process::Command::new("open");
    finder.arg("-R").arg(path);
    run_in_turn(vec![finder])
}

/// The file manager selects the file where it offers the freedesktop `ShowItems` call
/// (Files, Dolphin, Nemo, Caja, Thunar, …); otherwise the folder opens.
#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
fn system_reveal_file(path: &Path) -> Result<Option<FolderShown>, String> {
    let mut show_items = std::process::Command::new("dbus-send");
    show_items.args([
        "--session",
        "--print-reply",
        "--dest=org.freedesktop.FileManager1",
        "--type=method_call",
        "/org/freedesktop/FileManager1",
        "org.freedesktop.FileManager1.ShowItems",
    ]);
    show_items.arg(format!("array:string:{}", super::export::file_url(path)));
    show_items.arg("string:");
    let mut folder = std::process::Command::new("xdg-open");
    folder.arg(path.parent().unwrap_or(path));
    run_in_turn(vec![show_items, folder])
}

#[cfg(target_arch = "wasm32")]
fn system_reveal_file(_: &Path) -> Result<Option<FolderShown>, String> {
    Err("files cannot be shown here".into())
}

/// Run `commands` one after another on a thread of their own until one succeeds; the
/// receiver gets `Ok` then, or every failure. (Windows shows a file with Explorer alone.)
#[cfg(unix)]
pub(crate) fn run_in_turn(
    commands: Vec<std::process::Command>,
) -> Result<Option<FolderShown>, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("show file".into())
        .spawn(move || {
            let mut failures = Vec::new();
            for mut c in commands {
                let program = c.get_program().to_string_lossy().into_owned();
                let status = c
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
                match status {
                    Ok(s) if s.success() => {
                        let _ = tx.send(Ok(()));
                        return;
                    }
                    Ok(s) => failures.push(format!("{program} failed: {s}")),
                    Err(e) => failures.push(format!("{program}: {e}")),
                }
            }
            let _ = tx.send(Err(failures.join("; ")));
        })
        .map_err(|e| format!("showing the file: {e}"))?;
    Ok(Some(rx))
}

#[cfg(not(target_arch = "wasm32"))]
fn system_open_folder(path: &Path) -> Result<Option<FolderShown>, String> {
    if cfg!(target_os = "windows") {
        // Explorer's exit code says nothing (it is 1 when the folder opened), so only
        // starting it can fail.
        open_folder_with("explorer", path, false)
    } else if cfg!(target_os = "macos") {
        open_folder_with("open", path, true)
    } else {
        open_folder_with("xdg-open", path, true)
    }
}

/// Start `program` on `path`; with `exit_status`, its exit is awaited on a thread of its own
/// and a failure comes on the receiver.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn open_folder_with(
    program: &'static str,
    path: &Path,
    exit_status: bool,
) -> Result<Option<FolderShown>, String> {
    let mut child = std::process::Command::new(program)
        .arg(path)
        .spawn()
        .map_err(|e| format!("{program}: {e}"))?;
    if !exit_status {
        return Ok(None);
    }
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("open folder".into())
        .spawn(move || {
            let outcome = match child.wait() {
                Ok(status) if status.success() => Ok(()),
                Ok(status) => Err(format!("{program} failed: {status}")),
                Err(e) => Err(format!("{program}: {e}")),
            };
            let _ = tx.send(outcome);
        })
        .map_err(|e| format!("{program}: {e}"))?;
    Ok(Some(rx))
}

#[cfg(target_arch = "wasm32")]
fn system_open_folder(_: &Path) -> Result<Option<FolderShown>, String> {
    Err("folders cannot be opened here".into())
}

impl State {
    /// At launch: offer Report a problem for the newest Crash report, if it is new.
    pub(super) fn offer_crash_report(&mut self) {
        if let Some(report) = (self.session.crash_reports().into_iter()).find(|r| r.is_new) {
            self.report_ui.shown = Some(Shown::Offer(report));
        }
    }

    /// Help → Report a problem…: the newest Crash report, else a problem report made now.
    pub(super) fn show_report_problem(&mut self) {
        let report = self.session.crash_reports().into_iter().next();
        self.review(report);
    }

    fn review(&mut self, report: Option<CrashReport>) {
        let report = report.map(|r| self.seen(r));
        let text = match &report {
            Some(r) => r.text.clone(),
            None => problem_report(
                &diagnostics::recent_log_lines(60),
                diagnostics::home_dir().as_deref(),
            ),
        };
        let zip = self.session.diagnostics_zip_entries(report.as_ref());
        self.report_ui.shown = Some(Shown::Review { report, text, zip });
    }

    /// Help → Save diagnostics as zip…: what the zip of the log files will hold, to save.
    pub(super) fn show_diagnostics_zip(&mut self) {
        let zip = self.session.diagnostics_zip_entries(None);
        self.report_ui.shown = Some(Shown::Diagnostics { zip });
    }

    /// Ask where to save the zip, on its own thread; `report` is the Crash report's file name.
    fn ask_zip_path(&mut self, ctx: &egui::Context, report: Option<String>) {
        let dialog = Arc::clone(&self.report_ui.save_dialog);
        let name = self.session.diagnostics_zip_file_name();
        let folder = diagnostics::default_zip_folder();
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("obs-zip-dialog".into())
            .spawn(move || {
                let path = dialog(&name, folder.as_deref());
                let _ = tx.send((report, path));
                ctx.request_repaint();
            });
        match spawned {
            Ok(_) => self.report_ui.saving = Some(rx),
            Err(e) => self.toast(format!("Could not open the file dialog: {e}")),
        }
    }

    /// Whether the zip's save dialog is open (frames keep polling).
    pub(super) fn zip_dialog_waiting(&self) -> bool {
        self.report_ui.saving.is_some()
    }

    /// Save the zip once the dialog answers, then show it in the file manager; never wait.
    pub(super) fn poll_zip_dialog(&mut self) {
        let Some(rx) = &self.report_ui.saving else {
            return;
        };
        let (report, path) = match rx.try_recv() {
            Ok(answer) => answer,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                self.report_ui.saving = None;
                self.toast("The file dialog closed unexpectedly");
                return;
            }
        };
        self.report_ui.saving = None;
        let Some(path) = path else {
            return;
        };
        let mut p = json!({ "path": path.to_string_lossy() });
        if let Some(r) = report {
            p["report"] = json!(r);
        }
        let Some(out) = self.run("help.save_diagnostics_zip", &p) else {
            return;
        };
        let saved = out["path"].as_str().map_or(path, PathBuf::from);
        self.toast(format!(
            "Saved {}. Nothing was sent: attach it to an email or a message yourself.",
            saved.display()
        ));
        if matches!(self.report_ui.shown, Some(Shown::Diagnostics { .. })) {
            self.report_ui.shown = None;
        }
        match (self.report_ui.reveal_file)(&saved) {
            Ok(Some(shown)) => self.report_ui.opening = Some((saved, shown)),
            Ok(None) => {}
            Err(e) => self.folder_not_shown(&saved, &e),
        }
    }

    /// Mark a Crash report offered, so the next launch does not offer it again.
    fn seen(&mut self, report: CrashReport) -> CrashReport {
        match mark_seen(&report) {
            Ok(path) => CrashReport {
                path,
                is_new: false,
                ..report
            },
            Err(e) => {
                log::warn!("could not mark a crash report seen: {e}");
                report
            }
        }
    }

    /// Help → Open log folder, and the window's button.
    pub(super) fn open_folder(&mut self, path: &Path) {
        match (self.report_ui.open_folder)(path) {
            Ok(Some(shown)) => self.report_ui.opening = Some((path.to_path_buf(), shown)),
            Ok(None) => {}
            Err(e) => self.folder_not_shown(path, &e),
        }
    }

    fn folder_not_shown(&mut self, path: &Path, error: &str) {
        self.toast(format!(
            "Could not show it in the file manager ({error}). It is {}",
            path.display()
        ));
    }

    /// Whether a file manager showing a folder has yet to exit.
    pub(super) fn folder_opening(&self) -> bool {
        self.report_ui.opening.is_some()
    }

    /// A file manager that exited with an error says so, and where the folder is.
    pub(super) fn poll_folder_opening(&mut self) {
        let Some((path, rx)) = &self.report_ui.opening else {
            return;
        };
        let outcome = match rx.try_recv() {
            Ok(outcome) => outcome,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Ok(()),
        };
        let path = path.clone();
        self.report_ui.opening = None;
        if let Err(e) = outcome {
            self.folder_not_shown(&path, &e);
        }
    }

    pub(super) fn report_problem_window(&mut self, ctx: &egui::Context) {
        let Some(shown) = &self.report_ui.shown else {
            return;
        };
        let mut open = true;
        let escape = super::escape_closes_window(ctx);
        let mut acts = Vec::new();
        let max_height = ctx.content_rect().height() * 0.45;
        let saving = self.report_ui.saving.is_some();
        let title = match shown {
            Shown::Offer(_) => "Open Bible Software closed unexpectedly",
            Shown::Review { .. } => "Report a problem",
            Shown::Diagnostics { .. } => "Save diagnostics as zip",
        };
        let window = egui::Window::new(title)
            .id(egui::Id::unique(WINDOW))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(640.0)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| match shown {
                Shown::Offer(_) => {
                    ui.set_max_width(460.0);
                    ui.label(
                        "Last time, the app stopped because of a problem. It saved a crash \
                         report on this computer: the app version, your operating system, \
                         where in the program it happened and recent log lines. It holds \
                         none of your Notes, Highlights or Resource text.",
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Nothing has been sent. You can read the report first.")
                            .strong(),
                    );
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Report a problem…").clicked() {
                            acts.push(Act::Review);
                        }
                        if ui.button("Not now").clicked() {
                            acts.push(Act::NotNow);
                        }
                    });
                }
                Shown::Diagnostics { zip } => {
                    ui.set_max_width(460.0);
                    ui.label(
                        "One zip of the app's log files, to send to the developers by email or \
                         a message when something goes wrong.",
                    );
                    ui.add_space(4.0);
                    zip_list(ui, "Save as zip… saves one file holding:", zip);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(!saving, egui::Button::new("Save as zip…"))
                            .clicked()
                        {
                            acts.push(Act::SaveZip);
                        }
                        if ui.button("Close").clicked() {
                            acts.push(Act::Close);
                        }
                    });
                }
                Shown::Review { report, text, zip } => {
                    ui.set_width(620.0);
                    ui.label(REVIEW_NOTE);
                    ui.add_space(4.0);
                    egui::Frame::new()
                        .fill(theme::c().field)
                        .inner_margin(egui::Margin::same(8))
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt("report-text")
                                .max_height(max_height)
                                .show(ui, |ui| {
                                    let mut view = text.as_str();
                                    ui.add(
                                        egui::TextEdit::multiline(&mut view)
                                            .id_salt("report-text-edit")
                                            .font(egui::TextStyle::Monospace)
                                            .desired_width(f32::INFINITY),
                                    )
                                    .accessible_name("Report");
                                });
                        });
                    ui.add_space(6.0);
                    // Without a Log folder (the web build) there is nothing to save.
                    if !zip.is_empty() {
                        zip_list(ui, "Save report as zip… saves one file holding:", zip);
                        ui.add_space(6.0);
                    }
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Copy report").clicked() {
                            acts.push(Act::Copy);
                        }
                        if ui
                            .button("Open GitHub issue…")
                            .on_hover_text(
                                "Opens GitHub's new-issue form in your browser with this report \
                                 in its address, which sends the report to GitHub. It becomes a \
                                 public issue only if you submit the form.",
                            )
                            .clicked()
                        {
                            acts.push(Act::OpenIssue);
                        }
                        if !zip.is_empty()
                            && ui
                                .add_enabled(!saving, egui::Button::new("Save report as zip…"))
                                .clicked()
                        {
                            acts.push(Act::SaveZip);
                        }
                        if ui.button("Open log folder").clicked() {
                            acts.push(Act::OpenFolder);
                        }
                        if report.is_some() && ui.button("Delete report").clicked() {
                            acts.push(Act::Delete);
                        }
                        if ui.button("Close").clicked() {
                            acts.push(Act::Close);
                        }
                    });
                }
            });
        if !open || escape(window.map(|r| r.response.layer_id)) {
            acts.push(match shown {
                Shown::Offer(_) => Act::NotNow,
                Shown::Review { .. } | Shown::Diagnostics { .. } => Act::Close,
            });
        }
        for act in acts {
            self.report_act(ctx, act);
        }
    }

    fn report_act(&mut self, ctx: &egui::Context, act: Act) {
        let Some(shown) = self.report_ui.shown.take() else {
            return;
        };
        match (act, shown) {
            (Act::Review, Shown::Offer(report)) => self.review(Some(report)),
            (Act::NotNow, Shown::Offer(report)) => {
                self.seen(report);
            }
            (Act::Copy, shown @ Shown::Review { .. }) => {
                if let Shown::Review { text, .. } = &shown {
                    ctx.copy_text(text.clone());
                    self.toast("Report copied: paste it where you choose");
                }
                self.report_ui.shown = Some(shown);
            }
            (Act::OpenIssue, shown @ Shown::Review { .. }) => {
                if let Shown::Review { text, .. } = &shown {
                    ctx.open_url(egui::OpenUrl::new_tab(issue_url(text)));
                    self.toast(
                        "Sent to GitHub's new-issue form in your browser: it becomes public \
                         only if you submit it",
                    );
                }
                self.report_ui.shown = Some(shown);
            }
            (Act::SaveZip, shown @ (Shown::Review { .. } | Shown::Diagnostics { .. })) => {
                let report = match &shown {
                    Shown::Review {
                        report: Some(r), ..
                    } => r
                        .path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(String::from),
                    _ => None,
                };
                self.report_ui.shown = Some(shown);
                self.ask_zip_path(ctx, report);
            }
            (Act::OpenFolder, shown) => {
                if let Some(dir) = self.session.log_dir().map(Path::to_path_buf) {
                    self.open_folder(&dir);
                } else {
                    self.toast("This app keeps no log folder");
                }
                self.report_ui.shown = Some(shown);
            }
            (
                Act::Delete,
                Shown::Review {
                    report: Some(report),
                    ..
                },
            ) => match std::fs::remove_file(&report.path) {
                Ok(()) => self.toast("Crash report deleted"),
                Err(e) => self.toast(format!("Could not delete the crash report: {e}")),
            },
            (Act::Close, Shown::Review { .. } | Shown::Diagnostics { .. }) => {}
            (_, shown) => self.report_ui.shown = Some(shown),
        }
    }
}

/// The files of a Diagnostics zip, each with what it is and its size, then [`ZIP_NOTE`].
fn zip_list(ui: &mut egui::Ui, heading: &str, zip: &[ZipEntry]) {
    ui.label(RichText::new(heading).strong());
    ui.indent("zip-files", |ui| {
        for f in zip {
            ui.label(format!(
                "{} — {} ({})",
                f.name,
                f.about,
                size_label(f.text.len())
            ));
        }
    });
    ui.label(RichText::new(ZIP_NOTE).color(theme::c().muted));
}

/// `512 bytes`, `12 KB`, `1.2 MB`.
fn size_label(bytes: usize) -> String {
    match bytes {
        b if b < 1024 => format!("{b} bytes"),
        b if b < 1024 * 1024 => format!("{} KB", b.div_ceil(1024)),
        b => format!("{:.1} MB", b as f64 / (1024.0 * 1024.0)),
    }
}
