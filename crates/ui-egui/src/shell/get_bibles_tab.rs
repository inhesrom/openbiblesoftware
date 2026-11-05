//! The Get Resources Tab (#91, #132): Featured Bibles, Lexicons, Commentaries and
//! Dictionaries first, then every one from every repository on CrossWire's master list, each
//! one click from the Library. A type filter sits beside the search and language filter.

use egui::{Color32, RichText, Ui};
use obs_engine::get_bibles::{LISTED_TYPES, ListingStatus, ResourceListing};
use obs_engine::library_search::type_plural;
use obs_engine::{ImportPhase, language_name};
use obs_model::ResourceType;
use serde_json::json;

use super::State;
use super::import_tab::megabytes;
use super::library_tab::{section, type_singular};
use crate::theme;

/// Every row (and section header) is this tall, so the list can be virtualized.
const ROW_HEIGHT: f32 = 80.0;

/// The Tab's own search and language filter.
#[derive(Debug, Default)]
pub(super) struct GetBiblesUi {
    pub(super) query: String,
    pub(super) language: Option<String>,
}

enum Item<'a> {
    Header(String, usize),
    Listing(&'a ResourceListing),
    Empty(&'static str),
}

enum Action {
    Run(&'static str, serde_json::Value),
}

/// A short licence badge and its colour, from a conf's `DistributionLicense`.
pub(super) fn licence_badge(licence: Option<&str>) -> (&'static str, Color32) {
    let l = licence.unwrap_or_default().to_ascii_lowercase();
    let open = theme::c().verdigris;
    let restricted = theme::c().warn;
    if l.is_empty() {
        ("Licence unknown", theme::c().muted)
    } else if l.contains("public domain") {
        ("Public domain", open)
    } else if l.contains("cc0") {
        ("CC0", open)
    } else if l.contains("by-sa") {
        ("CC BY-SA", open)
    } else if l.contains("by-nc") {
        ("CC BY-NC", restricted)
    } else if l.contains("by-nd") {
        ("CC BY-ND", restricted)
    } else if l.contains("creative commons") || l.contains("cc by") || l.contains("cc-by") {
        ("CC BY", open)
    } else if l.contains("gpl") {
        ("GPL", open)
    } else if l.contains("non-commercial") || l.contains("noncommercial") {
        ("Non-commercial", restricted)
    } else if l.contains("copyright") {
        ("Copyrighted", restricted)
    } else {
        ("Other licence", theme::c().muted)
    }
}

/// "3 hours ago" for the offline banner.
fn ago(then: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(then);
    let s = now.saturating_sub(then);
    match s {
        0..120 => "just now".into(),
        120..7200 => format!("{} minutes ago", s / 60),
        7200..172_800 => format!("{} hours ago", s / 3600),
        _ => format!("{} days ago", s / 86400),
    }
}

impl State {
    pub(super) fn get_bibles_body(&mut self, ui: &mut Ui) {
        let mut action: Option<Action> = None;
        let mut new_type: Option<Option<ResourceType>> = None;
        egui::Frame::new()
            .fill(theme::c().field)
            .inner_margin(egui::Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                let gb = &self.session.get_bibles;
                // Status line: refreshing, offline (cache shown), or never fetched.
                if gb.is_refreshing() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Checking SWORD repositories for Resources…");
                    });
                }
                if let Some(why) = gb.offline() {
                    egui::Frame::new()
                        .fill(theme::c().warn.gamma_multiply(0.12))
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .corner_radius(4.0)
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                let msg = match gb.catalog() {
                                    Some(c) => format!(
                                        "Offline: showing the catalog from {}. Downloads need \
                                         a connection.",
                                        ago(c.fetched_at)
                                    ),
                                    None => "Offline: the Resource catalog could not be loaded, \
                                             and there is no saved copy yet."
                                        .to_string(),
                                };
                                ui.colored_label(theme::c().warn, msg).on_hover_text(why);
                                if !gb.is_refreshing() && ui.button("Try again").clicked() {
                                    action = Some(Action::Run("getbibles.refresh", json!({})));
                                }
                            });
                        });
                    ui.add_space(4.0);
                }
                let Some(catalog) = gb.catalog() else {
                    if !gb.is_refreshing() && gb.offline().is_none() {
                        ui.horizontal(|ui| {
                            ui.label("The Resource catalog has not been loaded.");
                            if ui.button("Load catalog").clicked() {
                                action = Some(Action::Run("getbibles.refresh", json!({})));
                            }
                        });
                    }
                    return;
                };

                // Type filter, then search and language filter for the "All" list.
                let filter = &mut self.get_bibles_ui;
                let ty = gb.type_filter();
                ui.horizontal_wrapped(|ui| {
                    let types = LISTED_TYPES.into_iter().map(Some).chain([None]);
                    for t in types {
                        let label = t.map_or("All", type_plural);
                        if ui.selectable_label(ty == t, label).clicked() && ty != t {
                            new_type = Some(t);
                        }
                    }
                });
                // Never wider than the Panel: a wide row would widen every row below it and
                // push their buttons out of view.
                let search_width = (ui.available_width() - 220.0).clamp(120.0, 280.0);
                ui.horizontal_wrapped(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut filter.query)
                            .hint_text("Search by name or abbreviation")
                            .desired_width(search_width),
                    );
                    let shown = filter
                        .language
                        .as_deref()
                        .map_or("All languages".to_string(), |l| {
                            format!("{} ({l})", language_name(l))
                        });
                    egui::ComboBox::from_id_salt("get-bibles-language")
                        .selected_text(shown)
                        .height(400.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut filter.language, None, "All languages");
                            for (code, n) in catalog.languages(ty) {
                                let label = format!("{} ({code}) · {n}", language_name(&code));
                                ui.selectable_value(&mut filter.language, Some(code), label);
                            }
                        });
                    if !self.session.get_bibles.is_refreshing()
                        && ui
                            .small_button("Refresh")
                            .on_hover_text("Fetch the repositories' catalogs again")
                            .clicked()
                    {
                        action = Some(Action::Run("getbibles.refresh", json!({})));
                    }
                });
                ui.add_space(6.0);

                // A Featured section per type shown; with All, empty ones are left out.
                let mut items: Vec<Item> = Vec::new();
                for t in LISTED_TYPES
                    .into_iter()
                    .filter(|t| ty.is_none_or(|ty| ty == *t))
                {
                    let featured = catalog.featured(t);
                    if ty.is_none() && featured.is_empty() {
                        continue;
                    }
                    items.push(Item::Header(
                        format!("Featured {}", type_plural(t)),
                        featured.len(),
                    ));
                    items.extend(featured.into_iter().map(Item::Listing));
                }
                let all = catalog.all(&filter.query, filter.language.as_deref(), ty);
                items.push(Item::Header(
                    format!("All {}", ty.map_or("Resources", type_plural)),
                    all.len(),
                ));
                if all.is_empty() {
                    items.push(Item::Empty(
                        "Nothing matches the search, type and language.",
                    ));
                }
                items.extend(all.into_iter().map(Item::Listing));

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show_rows(ui, ROW_HEIGHT, items.len(), |ui, range| {
                        for item in &items[range] {
                            ui.allocate_ui(egui::vec2(ui.available_width(), ROW_HEIGHT), |ui| {
                                ui.set_min_height(ROW_HEIGHT);
                                match item {
                                    Item::Header(title, n) => {
                                        ui.add_space(ROW_HEIGHT - 22.0);
                                        section(ui, &format!("{title} · {n}"));
                                    }
                                    Item::Empty(msg) => {
                                        ui.label(RichText::new(*msg).color(theme::c().muted));
                                    }
                                    Item::Listing(b) => {
                                        let status = self.session.listing_status(b);
                                        if let Some(a) = listing_row(ui, b, &status, ty.is_none()) {
                                            action = Some(a);
                                        }
                                    }
                                }
                            });
                        }
                    });
            });
        if let Some(ty) = new_type {
            self.session.get_bibles.set_type_filter(ty);
        }
        if let Some(Action::Run(command, params)) = action {
            self.run(command, &params);
        }
    }
}

/// One listing: title, facts, what it is used by, licence, and its Download / progress /
/// Open / Retry control. A module Import refuses has a disabled Download, the reason on
/// hover. The control is laid out first, from the right, so long titles or licences never
/// push it out of the Panel; the text is truncated into what is left. `show_type` names the
/// Resource type among the facts (the All filter).
fn listing_row(
    ui: &mut Ui,
    b: &ResourceListing,
    status: &ListingStatus,
    show_type: bool,
) -> Option<Action> {
    let mut action = None;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        match status {
            ListingStatus::Available if b.refused.is_some() => {
                let why = b.refused.as_deref().unwrap_or_default();
                ui.add_enabled(false, egui::Button::new("Download"))
                    .on_disabled_hover_text(format!("Import cannot read this module yet: {why}"));
                ui.label(
                    RichText::new("Not supported yet")
                        .small()
                        .color(theme::c().muted),
                )
                .on_hover_text(why);
            }
            ListingStatus::Available => {
                if ui
                    .button("Download")
                    .on_hover_text(format!("Download {} {}", b.module, b.source_label()))
                    .clicked()
                {
                    action = Some(Action::Run(
                        "getbibles.download",
                        json!({"module": b.module}),
                    ));
                }
            }
            ListingStatus::InProgress {
                resource_id,
                phase,
                done,
                total,
            } => {
                if ui.small_button("Cancel").clicked() {
                    action = Some(Action::Run("import.cancel", json!({"id": resource_id})));
                }
                #[allow(clippy::cast_precision_loss)]
                let (fraction, text) = match phase {
                    ImportPhase::Download if *total > 0 => (
                        *done as f32 / *total as f32,
                        format!("{} of {}", megabytes(*done), megabytes(*total)),
                    ),
                    ImportPhase::Download => (0.0, "Downloading…".to_string()),
                    _ => (1.0, "Importing…".to_string()),
                };
                ui.add(
                    egui::ProgressBar::new(fraction)
                        .desired_width(130.0)
                        .text(text),
                );
            }
            ListingStatus::Installed { resource_id } => {
                if ui.button("Open").clicked() {
                    action = Some(Action::Run("resource.open", json!({"id": resource_id})));
                }
                ui.label(RichText::new("Installed").small().color(theme::c().muted));
            }
            ListingStatus::Failed { error, .. } => {
                if ui.button("Retry").on_hover_text(error.as_str()).clicked() {
                    action = Some(Action::Run(
                        "getbibles.download",
                        json!({"module": b.module}),
                    ));
                }
                ui.colored_label(theme::c().warn, "Failed")
                    .on_hover_text(error.as_str());
            }
        }
        ui.add_space(8.0);
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.add(
                egui::Label::new(RichText::new(&b.title).strong().color(theme::c().ink)).truncate(),
            );
            ui.horizontal(|ui| {
                let (badge, colour) = licence_badge(b.licence.as_deref());
                ui.label(RichText::new(badge).small().strong().color(colour))
                    .on_hover_text(
                        b.licence
                            .as_deref()
                            .unwrap_or("No DistributionLicense in the conf"),
                    );
                let mut facts = Vec::new();
                if show_type {
                    facts.push(type_singular(b.kind).to_string());
                }
                facts.push(b.abbreviation.clone());
                facts.push(language_name(&b.language).to_string());
                if let Some(size) = b.size {
                    facts.push(megabytes(size));
                }
                facts.push(b.source_label());
                ui.add(
                    egui::Label::new(
                        RichText::new(facts.join(" · "))
                            .small()
                            .color(theme::c().muted),
                    )
                    .truncate(),
                );
            });
            let notes: Vec<&str> = b
                .used_by()
                .into_iter()
                .chain(b.licence.as_deref())
                .collect();
            if !notes.is_empty() {
                ui.add(
                    egui::Label::new(
                        RichText::new(notes.join(" · "))
                            .small()
                            .color(theme::c().muted),
                    )
                    .truncate(),
                );
            }
        });
    });
    action
}
