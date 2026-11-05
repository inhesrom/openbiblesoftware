//! The Starter library Tab (#131): which Starter Resources are in the Library, Add back for
//! the ones that are not, and the verified download when the app came without them.

use egui::{RichText, Ui};
use obs_engine::starter::{StarterRow, StarterStatus};
use serde_json::json;

use super::State;
use super::get_bibles_tab::licence_badge;
use super::import_tab::megabytes;
use super::library_tab::{section, type_singular};
use crate::theme;

/// Every row is this tall: a right-to-left row would otherwise take the whole height.
const ROW_HEIGHT: f32 = 64.0;

/// A short licence badge and its colour, from an SPDX expression.
fn spdx_badge(spdx: &str) -> (&'static str, egui::Color32) {
    if spdx.contains("PublicDomain") {
        licence_badge(Some("public domain"))
    } else if spdx == obs_model::LICENSE_UNKNOWN {
        licence_badge(None)
    } else {
        licence_badge(Some(spdx).filter(|s| !s.is_empty()))
    }
}

impl State {
    pub(super) fn starter_body(&mut self, ui: &mut Ui) {
        let mut action: Option<(&'static str, serde_json::Value)> = None;
        let session = &self.session;
        let starter = &session.starter;
        let bundled = starter.bundle().is_some();
        let rows = session.starter_rows();
        let wanted = session.starter_wanted();
        egui::Frame::new()
            .fill(theme::c().field)
            .inner_margin(egui::Margin::symmetric(18, 12))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                ui.label(
                    RichText::new("Starter library")
                        .font(theme::display(20.0))
                        .color(theme::c().ink),
                );
                ui.label(
                    RichText::new(if bundled {
                        "Open Bibles, original-language texts, lexicons, commentaries and \
                         reference data that come with the app. A Resource you remove stays \
                         removed until you add it back here."
                    } else {
                        "This copy of the app came without the Starter library: open Bibles, \
                         original-language texts, lexicons, commentaries and reference data. \
                         Download it from the app's releases; every file is checked before \
                         anything is added."
                    })
                    .color(theme::c().muted),
                );
                ui.add_space(8.0);
                if let Some(d) = starter.download() {
                    ui.horizontal(|ui| {
                        #[allow(clippy::cast_precision_loss)]
                        let fraction = if d.total > 0 {
                            d.done as f32 / d.total as f32
                        } else {
                            0.0
                        };
                        ui.add(egui::ProgressBar::new(fraction).desired_width(260.0).text(
                            format!(
                                "Downloading {} of {}",
                                megabytes(d.done),
                                megabytes(d.total)
                            ),
                        ));
                        if ui.button("Cancel").clicked() {
                            action = Some(("starter.cancel", json!({})));
                        }
                    });
                } else if starter.is_checking() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Loading the Starter library list…");
                    });
                } else if !wanted.is_empty() {
                    let bytes: u64 = wanted.iter().map(|i| i.bytes).sum();
                    let label = if bundled {
                        format!(
                            "Add {} missing Resource{}",
                            wanted.len(),
                            if wanted.len() == 1 { "" } else { "s" }
                        )
                    } else {
                        format!(
                            "Download Starter library (≈{} MB)",
                            bytes.div_ceil(1_000_000)
                        )
                    };
                    if ui.button(RichText::new(label).strong()).clicked() {
                        action = Some(("starter.add", json!({})));
                    }
                } else if starter.manifest().is_some() {
                    ui.label(
                        RichText::new(
                            "Every Starter Resource you have not removed is in your Library.",
                        )
                        .color(theme::c().muted),
                    );
                }
                if let Some(error) = starter.error() {
                    ui.add_space(4.0);
                    egui::Frame::new()
                        .fill(theme::c().warn.gamma_multiply(0.12))
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .corner_radius(4.0)
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.colored_label(theme::c().warn, error);
                                if starter.download().is_none()
                                    && !starter.is_checking()
                                    && ui.button("Retry").clicked()
                                {
                                    // The same attempt again: an Add back stays one.
                                    action = Some(if starter.manifest().is_none() {
                                        ("starter.check", json!({}))
                                    } else {
                                        ("starter.retry", json!({}))
                                    });
                                }
                            });
                        });
                }
                ui.add_space(10.0);
                if rows.is_empty() {
                    return;
                }
                // The ones that need the user first: removed, missing, outdated.
                let (have, need): (Vec<&StarterRow>, Vec<&StarterRow>) = rows
                    .iter()
                    .partition(|r| r.status == StarterStatus::InLibrary);
                egui::ScrollArea::vertical()
                    .id_salt("starter-library")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (title, group) in
                            [("Not in your Library", &need), ("In your Library", &have)]
                        {
                            if group.is_empty() {
                                continue;
                            }
                            section(ui, &format!("{title} · {}", group.len()));
                            for row in group {
                                ui.allocate_ui(
                                    egui::vec2(ui.available_width(), ROW_HEIGHT),
                                    |ui| {
                                        ui.set_min_height(ROW_HEIGHT);
                                        if let Some(a) = starter_row(ui, row, bundled) {
                                            action = Some(a);
                                        }
                                    },
                                );
                            }
                            ui.add_space(8.0);
                        }
                    });
            });
        if let Some((command, params)) = action {
            self.run(command, &params);
        }
    }
}

/// One Starter Resource: title; type, abbreviation, size and licence; what it powers; and its
/// control. The control is laid out first, from the right, so long titles never push it out.
fn starter_row(
    ui: &mut Ui,
    row: &StarterRow,
    bundled: bool,
) -> Option<(&'static str, serde_json::Value)> {
    let mut action = None;
    let id = json!({ "id": row.resource_id });
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let muted = |text: &str| RichText::new(text).small().color(theme::c().muted);
        match row.status {
            StarterStatus::InLibrary => {
                if ui.button("Open").clicked() {
                    action = Some(("resource.open", id.clone()));
                }
                ui.label(muted("In Library"));
            }
            StarterStatus::Removed => {
                if ui
                    .button("Add back")
                    .on_hover_text("You removed it; add it to your Library again")
                    .clicked()
                {
                    action = Some(("starter.add", id.clone()));
                }
                ui.label(RichText::new("Removed").small().color(theme::c().warn));
            }
            StarterStatus::Missing => {
                if ui
                    .button(if bundled { "Add" } else { "Download" })
                    .clicked()
                {
                    action = Some(("starter.add", id.clone()));
                }
                ui.label(muted("Not in Library"));
            }
            StarterStatus::Outdated { installed, offered } => {
                if ui.button("Update").clicked() {
                    action = Some(("starter.add", id.clone()));
                }
                ui.label(muted(&format!("Revision {offered} (you have {installed})")));
            }
            StarterStatus::Adding => {
                ui.spinner();
                ui.label(muted("Adding…"));
            }
        }
        ui.add_space(8.0);
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.add(
                egui::Label::new(RichText::new(&row.title).strong().color(theme::c().ink))
                    .truncate(),
            );
            ui.horizontal(|ui| {
                let (badge, colour) = spdx_badge(&row.license);
                ui.label(RichText::new(badge).small().strong().color(colour))
                    .on_hover_text(if row.license.is_empty() {
                        "No licence in the manifest"
                    } else {
                        row.license.as_str()
                    });
                let mut facts = Vec::new();
                if let Some(t) = row.resource_type {
                    facts.push(type_singular(t).to_string());
                }
                if !row.abbreviation.is_empty() {
                    facts.push(row.abbreviation.clone());
                }
                facts.push(megabytes(row.bytes));
                ui.add(
                    egui::Label::new(
                        RichText::new(facts.join(" · "))
                            .small()
                            .color(theme::c().muted),
                    )
                    .truncate(),
                );
            });
            if !row.features.is_empty() {
                ui.add(
                    egui::Label::new(
                        RichText::new(row.features.join(" · "))
                            .small()
                            .color(theme::c().lapis),
                    )
                    .truncate(),
                );
            }
        });
    });
    action
}
