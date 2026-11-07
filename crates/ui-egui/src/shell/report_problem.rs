//! Report a problem (#219): at launch, a new Crash report is offered; Help → Report a
//! problem… shows the newest one, or a problem report made now. The user reads the whole
//! report here first. Nothing leaves the computer unless the user copies it (Copy report) or
//! opens a GitHub issue prefilled with it (Open GitHub issue…) and submits that in the
//! browser. Help → Open log folder shows the Log folder ([`obs_engine::diagnostics`]).

use std::path::Path;
use std::sync::Arc;

use egui::{RichText, vec2};
use obs_engine::CrashReport;
use obs_engine::diagnostics::{self, issue_url, mark_seen, problem_report};

use super::State;
use crate::theme;

/// Shows a folder in the system file manager.
pub(crate) type FolderOpener = Arc<dyn Fn(&Path) -> Result<(), String> + Send + Sync>;

const WINDOW: &str = "report-a-problem";

/// The Report a problem window's state across frames.
pub(crate) struct ReportProblemUi {
    pub(super) open_folder: FolderOpener,
    pub(super) shown: Option<Shown>,
}

impl Default for ReportProblemUi {
    fn default() -> Self {
        Self {
            open_folder: Arc::new(system_open_folder),
            shown: None,
        }
    }
}

/// What the window shows.
pub(crate) enum Shown {
    /// The launch offer for a new Crash report.
    Offer(CrashReport),
    /// A report to review: a Crash report, or (none) a problem report made now.
    Review {
        report: Option<CrashReport>,
        text: String,
    },
}

/// What a click asks for, done after the window is drawn.
enum Act {
    Review,
    NotNow,
    Copy,
    OpenIssue,
    OpenFolder,
    Delete,
    Close,
}

#[cfg(not(target_arch = "wasm32"))]
fn system_open_folder(path: &Path) -> Result<(), String> {
    let program = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{program}: {e}"))
}

#[cfg(target_arch = "wasm32")]
fn system_open_folder(_: &Path) -> Result<(), String> {
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
        self.report_ui.shown = Some(Shown::Review { report, text });
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
        if let Err(e) = (self.report_ui.open_folder)(path) {
            self.toast(format!(
                "Could not open the folder ({e}). It is {}",
                path.display()
            ));
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
        let title = match shown {
            Shown::Offer(_) => "Open Bible Software closed unexpectedly",
            Shown::Review { .. } => "Report a problem",
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
                Shown::Review { report, text } => {
                    ui.set_width(620.0);
                    ui.label(
                        "Read the report below. Nothing leaves this computer unless you copy \
                         it, or open the GitHub issue and submit it there yourself.",
                    );
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
                    ui.horizontal(|ui| {
                        if ui.button("Copy report").clicked() {
                            acts.push(Act::Copy);
                        }
                        if ui
                            .button("Open GitHub issue…")
                            .on_hover_text(
                                "Opens the issue form in your browser with this report filled \
                                 in; it is sent only if you submit it there",
                            )
                            .clicked()
                        {
                            acts.push(Act::OpenIssue);
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
                Shown::Review { .. } => Act::Close,
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
                    self.toast("Opened in your browser: nothing is sent until you submit it");
                }
                self.report_ui.shown = Some(shown);
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
            (Act::Close, Shown::Review { .. }) => {}
            (_, shown) => self.report_ui.shown = Some(shown),
        }
    }
}
