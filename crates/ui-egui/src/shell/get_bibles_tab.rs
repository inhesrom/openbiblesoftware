//! The Get Resources Tab (#91, #132, #133): Featured Bibles, Hebrew & Greek, Lexicons,
//! Commentaries and Dictionaries first, then every one from every repository on CrossWire's
//! master list, each one click from the Library. A filter row (the types and Hebrew & Greek)
//! sits beside the search and language filter. Bible rows carry "Strong's" and "Morphology"
//! tag badges. Downloaded Resources whose repository offers a newer version show Update
//! available, with Update all above the list (#136). A licence filter (Free to share, Free for
//! personal use, All licences) applies to every section, and the Open catalogue's rows (#134)
//! follow the Featured sections; they list even before the SWORD catalog is loaded.
//! Languages show by name (#244): the language filter is a searchable list of names with how
//! many Resources each has, the OS language first, and each row names its language, the code
//! on hover.

use egui::{Color32, RichText, Ui};
use obs_engine::get_bibles::{
    LanguageChoice, ListingStatus, ResourceFilter, ResourceListing, TAG_MORPHOLOGY, TAG_STRONGS,
    UpdateCheck, UpdatesSummary,
};
use obs_engine::{ImportPhase, LanguageName, LicenceClass, LicenceFilter, language, licence_class};
use serde_json::json;

use super::State;
use super::import_tab::megabytes;
use super::library_tab::{section, type_singular};
use crate::icons::{self, Icon};
use crate::theme;
use crate::tokens::{size, space};

/// Every row (and section header) is this tall, so the list can be virtualized.
const ROW_HEIGHT: f32 = size::ROW_TRIPLE;

/// The Tab's own search and language filter.
#[derive(Debug, Default)]
pub(super) struct GetBiblesUi {
    pub(super) query: String,
    pub(super) language: Option<String>,
    /// What is typed into the language filter's search.
    pub(super) language_search: String,
}

/// A language's name as the chrome can draw it: "Bangla — বাংলা", or only "Bangla" when the
/// interface fonts lack the native name's letters (it would draw as boxes).
pub(super) fn language_label(ui: &Ui, name: &LanguageName) -> String {
    let font = egui::TextStyle::Body.resolve(ui.style());
    match name.native {
        Some(native) if ui.ctx().fonts_mut(|f| f.has_glyphs(&font, native)) => name.label(),
        _ => name.english_label(),
    }
}

/// On hover over a language's name: its code, and the other names a search finds it by.
fn language_hover(name: &LanguageName) -> String {
    let mut text = format!("Language code: {}", name.tag);
    if !name.also.is_empty() {
        text.push_str(&format!("\nAlso called {}", name.also.join(", ")));
    }
    text
}

/// The language filter: a button naming the chosen language, opening a searchable list of
/// every language with how many Resources each has. A search matches English names, names in
/// the language itself and codes; Enter takes the first match.
fn language_filter(
    ui: &mut Ui,
    filter: &mut GetBiblesUi,
    choices: &dyn Fn() -> std::sync::Arc<[LanguageChoice]>,
) {
    let shown = filter
        .language
        .as_deref()
        .map_or("All languages".to_string(), |l| {
            language_label(ui, &language(l))
        });
    let chevron = icons::image(ui.ctx(), Icon::ChevronDown, icons::SMALL, theme::c().muted);
    let button = ui
        .add(crate::components::button((shown, chevron)))
        .accessible_name("Language");
    let button = match filter.language.as_deref() {
        Some(l) => button.on_hover_text(language_hover(&language(l))),
        None => button.on_hover_text("Show Resources in one language"),
    };
    let popup_id = ui.make_persistent_id("get-bibles-language");
    let opening = button.clicked() && !egui::Popup::is_id_open(ui.ctx(), popup_id);
    egui::Popup::menu(&button)
        .id(popup_id)
        .width(size::COLUMN)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            if opening {
                filter.language_search.clear();
            }
            let search = ui.add(
                egui::TextEdit::singleline(&mut filter.language_search)
                    .hint_text("Search languages: name or code")
                    .desired_width(f32::INFINITY),
            );
            let search = search.accessible_name("Search languages");
            if opening {
                search.request_focus();
            }
            let enter = search.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let choices = choices();
            // Best matches first ("ben" is Bengali's code before it is part of "Benabena"),
            // each group in the list's order.
            let mut found: Vec<(u8, &LanguageChoice)> = choices
                .iter()
                .filter_map(|c| Some((c.name.match_rank(&filter.language_search)?, c)))
                .collect();
            found.sort_by_key(|(rank, _)| *rank);
            let found: Vec<&LanguageChoice> = found.into_iter().map(|(_, c)| c).collect();
            if enter && let Some(first) = found.first() {
                filter.language = Some(first.code.clone());
                ui.close();
            }
            ui.add_space(space::XS);
            egui::ScrollArea::vertical()
                .max_height(size::POPOVER)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                    if filter.language_search.trim().is_empty()
                        && ui
                            .selectable_label(filter.language.is_none(), "All languages")
                            .clicked()
                    {
                        filter.language = None;
                        ui.close();
                    }
                    if found.is_empty() {
                        ui.label(crate::components::hint_text("No language matches"));
                    }
                    for c in found {
                        let selected = filter.language.as_deref() == Some(c.code.as_str());
                        let label = format!("{} · {}", language_label(ui, &c.name), c.count);
                        if ui
                            .selectable_label(selected, label)
                            .on_hover_text(language_hover(&c.name))
                            .clicked()
                        {
                            filter.language = Some(c.code.clone());
                            ui.close();
                        }
                    }
                });
        });
}

enum Item<'a> {
    Header(String, usize),
    Listing(&'a ResourceListing),
}

enum Action {
    Run(&'static str, serde_json::Value),
}

/// A short licence badge and its colour, from a conf's `DistributionLicense` or an SPDX id.
/// The colour is the licence class (#134): Free to share, Free for personal use, or unclear.
pub(super) fn licence_badge(licence: Option<&str>) -> (&'static str, Color32) {
    let class = licence.map_or(LicenceClass::Unclear, licence_class);
    (licence_badge_text(licence), licence_class_colour(class))
}

/// A short licence badge and its colour, from a manifest's SPDX expression: "Public domain"
/// for `LicenseRef-PublicDomain` and SWORD's `LicenseRef-PD`, the raw id at most in a tooltip.
pub(super) fn spdx_badge(spdx: &str) -> (&'static str, Color32) {
    if spdx.contains("PublicDomain") || spdx.split_whitespace().any(|t| t == "LicenseRef-PD") {
        licence_badge(Some("public domain"))
    } else if spdx == obs_model::LICENSE_UNKNOWN {
        licence_badge(None)
    } else {
        licence_badge(Some(spdx).filter(|s| !s.is_empty()))
    }
}

fn licence_class_colour(class: LicenceClass) -> Color32 {
    match class {
        LicenceClass::FreeToShare => theme::c().verdigris,
        LicenceClass::PersonalUse => theme::c().warn,
        LicenceClass::Unclear => theme::c().muted,
    }
}

fn licence_badge_text(licence: Option<&str>) -> &'static str {
    let l = licence.unwrap_or_default().to_ascii_lowercase();
    if l.is_empty() {
        "Licence unknown"
    } else if l.contains("public domain") || l.contains("publicdomain") {
        "Public domain"
    } else if l.contains("cc0") {
        "CC0"
    } else if l.contains("by-nc") || l.contains("by nc") {
        "CC BY-NC"
    } else if l.contains("by-sa") || l.contains("by sa") {
        "CC BY-SA"
    } else if l.contains("by-nd") || l.contains("by nd") {
        "CC BY-ND"
    } else if l.contains("creative commons") || l.contains("cc by") || l.contains("cc-by") {
        "CC BY"
    } else if l.contains("gpl") {
        "GPL"
    } else if l.contains("non-commercial") || l.contains("noncommercial") {
        "Non-commercial"
    } else if l.contains("copyright") {
        "Copyrighted"
    } else {
        "Other licence"
    }
}

/// "Version 1.1 is available (you have 1.0)", on hover over Update available.
pub(super) fn update_versions(installed: &str, available: &str) -> String {
    if installed.is_empty() {
        format!("Version {available} is available")
    } else {
        format!("Version {available} is available (you have {installed})")
    }
}

/// The line above the list about updates, with Update all when there are any. It says the
/// downloaded Resources are up to date only when every one of them was checked against a
/// catalog fetched online in this Session (`UpdatesSummary::up_to_date`); offline it says
/// they could not be checked, and with only the catalog saved by an earlier run it says when
/// that was.
fn updates_line(ui: &mut Ui, summary: &UpdatesSummary, names: &dyn Fn(&str) -> String) -> bool {
    let mut update_all = false;
    let n = summary.updates.len();
    if n > 0 {
        crate::components::notice_frame(theme::c().verdigris)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let what = if n == 1 {
                        "1 update available".to_string()
                    } else {
                        format!("{n} updates available")
                    };
                    let list: Vec<String> = summary
                        .updates
                        .iter()
                        .map(|u| format!("{} {}", names(&u.resource_id), u.available))
                        .collect();
                    ui.colored_label(
                        theme::c().verdigris,
                        RichText::new(format!("{what}: {}", list.join(", "))).strong(),
                    );
                    if ui
                        .button("Update all")
                        .on_hover_text(
                            "Download each newer version and replace the Resource with it;                              Notes and Highlights move with their text",
                        )
                        .clicked()
                    {
                        update_all = true;
                    }
                });
            });
        ui.add_space(space::XS);
    }
    let muted = |ui: &mut Ui, text: String| {
        ui.label(crate::components::hint_text(text));
    };
    match &summary.check {
        _ if summary.downloaded == 0 => {}
        UpdateCheck::Offline(_) => muted(
            ui,
            "Updates to your downloaded Resources could not be checked while offline.".into(),
        ),
        UpdateCheck::Saved { at } => muted(
            ui,
            format!(
                "Updates to your downloaded Resources were last checked {}. Refresh to check \
                 again.",
                ago(*at)
            ),
        ),
        UpdateCheck::Checked { at } if summary.up_to_date() => muted(
            ui,
            format!(
                "Your downloaded Resources are up to date (checked {}).",
                ago(*at)
            ),
        ),
        UpdateCheck::Checked { .. } if !summary.unchecked.is_empty() => {
            let list: Vec<String> = summary.unchecked.iter().map(|id| names(id)).collect();
            muted(
                ui,
                format!(
                    "Could not check {} for updates: its repository did not answer or no \
                     longer lists it.",
                    list.join(", ")
                ),
            );
        }
        _ => {}
    }
    update_all
}

/// "3 hours ago" for the offline banner.
fn ago(then: u64) -> String {
    let now = u64::try_from(obs_store::clock::now_secs()).unwrap_or(then);
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
        let mut new_filter: Option<ResourceFilter> = None;
        let mut new_licence: Option<LicenceFilter> = None;
        let mut clear = false;
        crate::components::page_frame().show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            let gb = &self.session.get_bibles;
            // Status: refreshing, offline (cache shown), or never fetched (#223's states).
            if gb.is_refreshing() {
                crate::components::loading_state(ui, "Checking SWORD repositories for Resources…");
                ui.add_space(space::XS);
            } else if let Some(why) = gb.offline() {
                let msg = match gb.catalog() {
                    Some(c) => format!(
                        "Offline: showing the catalog from {}. Downloads need a connection.",
                        ago(c.fetched_at)
                    ),
                    None => "Offline: the Resource catalog could not be loaded, and there is \
                             no saved copy yet."
                        .to_string(),
                };
                if crate::components::error_state(ui, &msg, why) {
                    action = Some(Action::Run("getbibles.refresh", json!({})));
                }
                ui.add_space(space::XS);
            }
            let summary = self.session.resource_updates();
            let library = &self.session.library;
            let names = |id: &str| library.abbreviation_of(id).unwrap_or(id).to_string();
            if updates_line(ui, &summary, &names) {
                action = Some(Action::Run("getbibles.update_all", json!({})));
            }
            // Without a fetched catalog the Open catalogue (compiled in) still lists.
            if gb.catalog().is_none() && !gb.is_refreshing() && gb.offline().is_none() {
                ui.horizontal(|ui| {
                    if crate::components::empty_state(
                        ui,
                        "The SWORD repositories' catalog has not been loaded.",
                        "Load catalog",
                    )
                    .clicked()
                    {
                        action = Some(Action::Run("getbibles.refresh", json!({})));
                    }
                });
            }

            // Type filter, licence filter, then search and language filter for "All".
            let filter = &mut self.get_bibles_ui;
            let shown_filter = gb.filter();
            let shown_licence = gb.licence_filter();
            ui.horizontal_wrapped(|ui| {
                for f in ResourceFilter::CHOICES {
                    let chip = ui.selectable_label(shown_filter == f, f.label());
                    let chip = if f == ResourceFilter::HebrewAndGreek {
                        chip.on_hover_text(
                            "Bibles in Hebrew, Aramaic and Greek. Those marked Strong's and \
                                 Morphology fill in Word Info and Word Study › Forms.",
                        )
                    } else {
                        chip
                    };
                    if chip.clicked() && shown_filter != f {
                        new_filter = Some(f);
                    }
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(crate::components::hint_text("Licence"));
                for l in LicenceFilter::CHOICES {
                    if ui
                        .selectable_label(shown_licence == l, l.label())
                        .on_hover_text(l.hover())
                        .clicked()
                        && shown_licence != l
                    {
                        new_licence = Some(l);
                    }
                }
            });
            // Never wider than the Panel: a wide row would widen every row below it and
            // push their buttons out of view.
            let search_width = (ui.available_width() - size::SIDE_LIST)
                .clamp(size::FIELD_NARROW, size::FIELD_WIDE);
            ui.horizontal_wrapped(|ui| {
                ui.add(crate::components::search_field(
                    &mut filter.query,
                    "Search by name or abbreviation",
                    search_width,
                ))
                .accessible_name("Search by name or abbreviation");
                language_filter(ui, filter, &|| gb.languages(shown_filter, shown_licence));
                if !gb.is_refreshing()
                    && ui
                        .small_button("Refresh")
                        .on_hover_text("Fetch the repositories' catalogs again")
                        .clicked()
                {
                    action = Some(Action::Run("getbibles.refresh", json!({})));
                }
            });
            ui.add_space(space::S);

            // The filter's Featured sections (with All, empty ones are left out), then the
            // Open catalogue.
            let mut items: Vec<Item> = Vec::new();
            for (title, rows) in gb.sections(shown_filter, shown_licence) {
                items.push(Item::Header(title, rows.len()));
                items.extend(rows.into_iter().map(Item::Listing));
            }
            let all = gb.all_listings(
                &filter.query,
                filter.language.as_deref(),
                shown_filter,
                shown_licence,
            );
            let all_title = match shown_filter {
                ResourceFilter::All => "All Resources".to_string(),
                f => format!("All {}", f.label()),
            };
            // Nothing matches (#223): said first, not under the Featured sections' rows.
            if all.is_empty() {
                ui.horizontal(|ui| {
                    if crate::components::empty_state(
                        ui,
                        "Nothing matches the search, type, language and licence.",
                        "Show everything",
                    )
                    .on_hover_text("Clear the search and language, and show every type and licence")
                    .clicked()
                    {
                        clear = true;
                    }
                });
                ui.add_space(space::S);
            }
            items.push(Item::Header(all_title, all.len()));
            items.extend(all.into_iter().map(Item::Listing));

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show_rows(ui, ROW_HEIGHT, items.len(), |ui, range| {
                    for item in &items[range] {
                        ui.allocate_ui(egui::vec2(ui.available_width(), ROW_HEIGHT), |ui| {
                            ui.set_min_height(ROW_HEIGHT);
                            match item {
                                Item::Header(title, n) => {
                                    ui.add_space(ROW_HEIGHT - size::CONTROL_COMPACT);
                                    section(ui, &format!("{title} · {n}"));
                                }
                                Item::Listing(b) => {
                                    let status = self.session.listing_status(b);
                                    let show_type = shown_filter == ResourceFilter::All;
                                    if let Some(a) = listing_row(ui, b, &status, show_type) {
                                        action = Some(a);
                                    }
                                }
                            }
                        });
                    }
                });
        });
        if clear {
            self.get_bibles_ui.query.clear();
            self.get_bibles_ui.language = None;
            new_filter = Some(ResourceFilter::All);
            new_licence = Some(LicenceFilter::All);
        }
        if let Some(f) = new_filter {
            self.session.get_bibles.set_filter(f);
        }
        if let Some(l) = new_licence {
            self.session.get_bibles.set_licence_filter(l);
        }
        if let Some(Action::Run(command, params)) = action {
            self.run(command, &params);
        }
    }
}

/// Download a row: a SWORD module from its repository, or an Open catalogue entry from its
/// publisher (#134).
fn download(b: &ResourceListing) -> Action {
    if b.is_open() {
        Action::Run("getbibles.download_open", json!({"id": b.module}))
    } else {
        Action::Run("getbibles.download", json!({"module": b.module}))
    }
}

/// What a tag badge means, on hover.
fn tag_hover(tag: &str) -> &'static str {
    match tag {
        TAG_STRONGS => {
            "Tagged with Strong's numbers: Word Info, Word Study and Key words work with this Bible."
        }
        TAG_MORPHOLOGY => {
            "Tagged with Morphology: Word Info shows each word's parsing in plain English, and \
             Word Study › Forms fills in."
        }
        _ => "",
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
                ui.add_enabled(false, crate::components::button("Download"))
                    .accessible_name(format!("Download {}", b.title))
                    .on_disabled_hover_text(format!("Import cannot read this module yet: {why}"));
                ui.label(crate::components::hint_text("Not supported yet"))
                    .on_hover_text(why);
            }
            ListingStatus::Available => {
                if ui
                    .button("Download")
                    .accessible_name(format!("Download {}", b.title))
                    .on_hover_text(format!("Download {} {}", b.module, b.source_label()))
                    .clicked()
                {
                    action = Some(download(b));
                }
            }
            ListingStatus::InProgress {
                resource_id,
                phase,
                done,
                total,
            } => {
                if ui
                    .small_button("Cancel")
                    .accessible_name(format!("Cancel {}", b.title))
                    .clicked()
                {
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
                        .desired_width(size::FIELD_NARROW)
                        .text(text),
                );
            }
            ListingStatus::Installed { resource_id } => {
                if ui
                    .button("Open")
                    .accessible_name(format!("Open {}", b.title))
                    .clicked()
                {
                    action = Some(Action::Run("resource.open", json!({"id": resource_id})));
                }
                ui.label(crate::components::hint_text("Installed"));
            }
            ListingStatus::UpdateAvailable {
                resource_id,
                installed,
                available,
            } => {
                if ui
                    .button("Update")
                    .accessible_name(format!("Update {}", b.title))
                    .on_hover_text(format!(
                        "Download version {available} {} and replace the Resource with it; \
                         Notes and Highlights move with their text",
                        b.source_label()
                    ))
                    .clicked()
                {
                    action = Some(Action::Run("getbibles.update", json!({"module": b.module})));
                }
                if ui
                    .button("Open")
                    .accessible_name(format!("Open {}", b.title))
                    .clicked()
                {
                    action = Some(Action::Run("resource.open", json!({"id": resource_id})));
                }
                ui.label(
                    RichText::new("Update available")
                        .small()
                        .strong()
                        .color(theme::c().verdigris),
                )
                .on_hover_text(update_versions(installed, available));
            }
            ListingStatus::Failed { error, .. } => {
                if ui
                    .button("Retry")
                    .accessible_name(format!("Retry {}", b.title))
                    .on_hover_text(error.as_str())
                    .clicked()
                {
                    action = Some(download(b));
                }
                ui.colored_label(theme::c().warn, "Failed")
                    .on_hover_text(error.as_str());
            }
        }
        ui.add_space(space::M);
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.add(
                egui::Label::new(RichText::new(&b.title).strong().color(theme::c().ink)).truncate(),
            );
            ui.horizontal(|ui| {
                // An Open catalogue row's class is declared by its entry, not read from text.
                let badge = licence_badge_text(b.licence.as_deref());
                let colour = licence_class_colour(b.licence_class());
                ui.label(RichText::new(badge).small().strong().color(colour))
                    .on_hover_text(format!(
                        "{}: {}",
                        b.licence_class().label(),
                        b.licence
                            .as_deref()
                            .unwrap_or("no DistributionLicense in the conf")
                    ));
                for tag in b.tags() {
                    ui.label(RichText::new(tag).small().strong().color(theme::c().lapis))
                        .on_hover_text(tag_hover(tag));
                }
                // Type, abbreviation, language (its code on hover), size, repository.
                let fact = |t: String| crate::components::hint_text(t);
                let mut before = Vec::new();
                if show_type {
                    before.push(type_singular(b.kind).to_string());
                }
                before.push(b.abbreviation.clone());
                let mut after = Vec::new();
                if let Some(size) = b.size {
                    after.push(megabytes(size));
                }
                after.push(b.source_label());
                let name = language(&b.language);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label(fact(format!("{} · ", before.join(" · "))));
                    ui.label(fact(language_label(ui, &name)))
                        .on_hover_text(language_hover(&name));
                    ui.add(egui::Label::new(fact(format!(" · {}", after.join(" · ")))).truncate());
                });
            });
            let notes: Vec<&str> = b
                .used_by()
                .into_iter()
                .chain(b.licence.as_deref())
                .collect();
            if let Some(url) = &b.evidence_url {
                // Open catalogue rows: the archived licence evidence, one click away.
                ui.horizontal(|ui| {
                    crate::components::hyperlink_to(
                        ui,
                        RichText::new("Licence evidence").small(),
                        url,
                    )
                    .on_hover_text(url);
                    ui.add(
                        egui::Label::new(crate::components::hint_text(notes.join(" · ")))
                            .truncate(),
                    );
                });
            } else if !notes.is_empty() {
                ui.add(
                    egui::Label::new(crate::components::hint_text(notes.join(" · "))).truncate(),
                );
            }
        });
    });
    action
}
