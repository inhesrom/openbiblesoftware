//! Get Resources → Books: the Christian Open Library (ADR 0024, #347). Connect with a GitHub
//! token or a local copy, browse and filter the catalogue, choose works and download them as
//! Books; Check for new books and Update.

use egui::{RichText, Ui};
use obs_engine::col::{COL_WEBSITE, ColWork, REQUEST_ACCESS_URL, WORK_TYPES, facet};
use obs_engine::col_get::{ColAccess, WorkStatus, size_of};
use serde_json::json;

use super::State;
use super::library_tab::section;
use crate::theme;
use crate::tokens::{size, space};

/// Where a fine-grained token that reads one repository is made.
const NEW_TOKEN_URL: &str = "https://github.com/settings/personal-access-tokens/new";

/// Every row is this tall, so the list can be virtualized.
const ROW_HEIGHT: f32 = size::ROW_DOUBLE;

/// The Books view's own fields.
#[derive(Debug, Default)]
pub(super) struct ColUi {
    /// The token being typed (cleared once sent).
    token: String,
    years: (String, String),
    /// Download everything was clicked: ask first.
    confirm_everything: bool,
    /// The folder picker for a local copy.
    #[cfg(not(target_arch = "wasm32"))]
    picker: Option<std::sync::mpsc::Receiver<Option<std::path::PathBuf>>>,
    /// Asked for the list once this run (opening Books reads it, ADR 0024).
    asked: bool,
}

/// What a click asked for.
enum Do {
    Run(&'static str, serde_json::Value),
    Open(&'static str),
    PickFolder,
}

impl State {
    /// The Books filter of Get Resources.
    pub(super) fn col_body(&mut self, ui: &mut Ui) {
        self.session.col_restore();
        if !self.col_ui.asked
            && self.session.col.access().is_some()
            && self.session.col.catalog().is_none()
            && !self.session.col.is_loading()
            && self.session.col.error().is_none()
        {
            self.col_ui.asked = true;
            self.run("col.refresh", &json!({}));
        }
        self.poll_col_picker();
        let mut action: Option<Do> = None;
        ui.label(crate::components::subtitle_text("Christian Open Library"));
        ui.horizontal_wrapped(|ui| {
            ui.label(
                "Sermons, treatises, confessions, commentaries and church records from the \
                 Church Fathers to the early 1900s, downloaded into your Library as Books.",
            );
            if crate::components::link(ui, "About the library").clicked() {
                action = Some(Do::Open(COL_WEBSITE));
            }
        });
        ui.add_space(space::S);
        match self.session.col.access().cloned() {
            None => self.col_connect(ui, &mut action),
            Some(access) => self.col_connected(ui, &access, &mut action),
        }
        match action {
            Some(Do::Run(command, params)) => {
                self.run(command, &params);
            }
            Some(Do::Open(url)) => ui.ctx().open_url(egui::OpenUrl::new_tab(url)),
            Some(Do::PickFolder) => self.pick_col_folder(ui.ctx()),
            None => {}
        }
    }

    /// Not connected: how to get access, the token field and a local copy.
    fn col_connect(&mut self, ui: &mut Ui, action: &mut Option<Do>) {
        if let Some(e) = self.session.col.error() {
            crate::components::error_notice(ui, &e.message(), &format!("{e:?}"));
            ui.add_space(space::S);
        }
        section(ui, "Connect");
        ui.label(
            "The library's books are shared with members by invitation on GitHub. Ask for \
             access, accept the invitation GitHub emails you, then connect here.",
        );
        ui.horizontal_wrapped(|ui| {
            if crate::components::link(ui, "Ask for access…").clicked() {
                *action = Some(Do::Open(REQUEST_ACCESS_URL));
            }
            if crate::components::link(ui, "Make a GitHub token…")
                .on_hover_text(
                    "A fine-grained token with read-only Contents on \
                     Christian-open-library/books is enough",
                )
                .clicked()
            {
                *action = Some(Do::Open(NEW_TOKEN_URL));
            }
        });
        ui.add_space(space::S);
        ui.horizontal_wrapped(|ui| {
            let r = ui
                .add(
                    egui::TextEdit::singleline(&mut self.col_ui.token)
                        .password(true)
                        .hint_text("GitHub token")
                        .desired_width(size::FIELD_WIDE),
                )
                .accessible_name("GitHub token");
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let connect = ui
                .add_enabled(
                    !self.col_ui.token.trim().is_empty(),
                    egui::Button::new("Connect"),
                )
                .on_hover_text(
                    "Kept in this computer's keychain, sent only to GitHub, and never \
                     written anywhere else",
                )
                .clicked();
            if (connect || enter) && !self.col_ui.token.trim().is_empty() {
                let token = std::mem::take(&mut self.col_ui.token);
                *action = Some(Do::Run("col.connect_token", json!({ "token": token })));
            }
        });
        ui.label(crate::components::hint_text(
            "A classic token needs the repo scope, which can also change your own private \
             repositories; a fine-grained token can be read-only.",
        ));
        ui.add_space(space::S);
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("Use a local copy…")
                .on_hover_text(
                    "A copy you keep with git or GitHub Desktop: no sign-in, and nothing is \
                     downloaded",
                )
                .clicked()
            {
                *action = Some(Do::PickFolder);
            }
            if ui
                .button("Browse the catalogue")
                .on_hover_text("COL's public list of its works; reading it needs no sign-in")
                .clicked()
            {
                *action = Some(Do::Run("col.browse_public", json!({})));
            }
        });
        if self.session.col.is_loading_public() {
            crate::components::loading_state(ui, "Reading the catalogue…");
        }
        if let Some(public) = self.session.col.public().cloned() {
            ui.add_space(space::M);
            section(ui, &format!("The catalogue · {}", public.len()));
            ui.label(crate::components::hint_text(
                "Connect to download any of them.",
            ));
            egui::ScrollArea::vertical()
                .id_salt("col-public")
                .auto_shrink([false, false])
                .show_rows(ui, ROW_HEIGHT, public.len(), |ui, rows| {
                    for w in &public[rows] {
                        ui.set_min_height(ROW_HEIGHT);
                        ui.label(RichText::new(&w.title).strong().color(theme::c().ink));
                        let mut facts = vec![w.author.clone()];
                        if let Some(y) = w.year {
                            facts.push(y.to_string());
                        }
                        facts.push(w.kind.clone());
                        ui.label(crate::components::hint_text(
                            facts
                                .into_iter()
                                .filter(|f| !f.is_empty())
                                .collect::<Vec<_>>()
                                .join(" · "),
                        ));
                    }
                });
        }
    }

    /// Connected: how, the changes since, the filters and the list.
    fn col_connected(&mut self, ui: &mut Ui, access: &ColAccess, action: &mut Option<Do>) {
        let col = &self.session.col;
        ui.horizontal_wrapped(|ui| {
            let how = match access {
                ColAccess::GitHub { .. } if col.token_for_session_only() => {
                    "Connected with GitHub (the token is kept for this session only: no \
                     keychain answered)"
                        .to_string()
                }
                ColAccess::GitHub { .. } => {
                    "Connected with GitHub (the token is in this computer's keychain)".into()
                }
                ColAccess::Local { root } => format!("Using the copy in {}", root.display()),
            };
            ui.label(crate::components::hint_text(how));
            if !col.is_loading() && ui.small_button("Check for new books").clicked() {
                *action = Some(Do::Run("col.refresh", json!({})));
            }
            if ui.small_button("Disconnect").clicked() {
                *action = Some(Do::Run("col.disconnect", json!({})));
            }
        });
        if col.is_loading() {
            crate::components::loading_state(ui, "Reading the Christian Open Library…");
            return;
        }
        if let Some(e) = col.error() {
            if crate::components::error_state(ui, &e.message(), &format!("{e:?}")) {
                *action = Some(Do::Run("col.refresh", json!({})));
            }
            return;
        }
        let Some(catalog) = col.catalog().cloned() else {
            return;
        };
        if let Some(changes) = col.changes() {
            let mut parts = Vec::new();
            if !changes.changed.is_empty() {
                parts.push(format!("{} changed", changes.changed.len()));
            }
            if !changes.removed.is_empty() {
                parts.push(format!("{} no longer in COL", changes.removed.len()));
            }
            let installed = changes.current + changes.changed.len() + changes.removed.len();
            if installed > 0 {
                ui.horizontal_wrapped(|ui| {
                    let line = if parts.is_empty() {
                        match installed {
                            1 => "Your COL book is up to date.".to_string(),
                            n => format!("Your {n} COL books are up to date."),
                        }
                    } else {
                        format!(
                            "Of your {installed} COL book{}: {}.",
                            if installed == 1 { "" } else { "s" },
                            parts.join(", ")
                        )
                    };
                    ui.label(line);
                    if !changes.changed.is_empty()
                        && col.downloading().is_none()
                        && ui
                            .button(format!("Update {}", changes.changed.len()))
                            .on_hover_text(
                                "Download COL's new copy of each and replace it; Notes and \
                                 Highlights move with their text",
                            )
                            .clicked()
                    {
                        *action = Some(Do::Run("col.update", json!({})));
                    }
                });
            }
        }
        ui.add_space(space::S);
        self.col_filters(ui, &catalog.works, action);
        let col = &self.session.col;
        let shown: Vec<&ColWork> = col.shown();
        let chosen = col.chosen();
        let (n_chosen, chosen_bytes) = size_of(&chosen);
        let (n_shown, shown_bytes) = size_of(&shown);
        let (n_all, all_bytes) = size_of(&catalog.works.iter().collect::<Vec<_>>());
        let busy = col.downloading().is_some();
        ui.horizontal_wrapped(|ui| {
            ui.label(format!(
                "Showing {n_shown} of {n_all} works · {n_chosen} chosen ({})",
                obs_engine::col::size_text(chosen_bytes)
            ));
        });
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("Choose all shown").clicked() {
                *action = Some(Do::Run("col.select", json!({ "on": true })));
            }
            if n_chosen > 0 && ui.small_button("Clear").clicked() {
                *action = Some(Do::Run("col.select", json!({ "on": false })));
            }
            if ui
                .add_enabled(
                    n_chosen > 0 && !busy,
                    egui::Button::new(format!("Download {n_chosen} chosen")),
                )
                .on_hover_text(obs_engine::col::size_text(chosen_bytes))
                .clicked()
            {
                *action = Some(Do::Run("col.download", json!({ "scope": "selected" })));
            }
            if n_shown < n_all
                && ui
                    .add_enabled(
                        n_shown > 0 && !busy,
                        egui::Button::new(format!("Download all {n_shown} shown")),
                    )
                    .on_hover_text(obs_engine::col::size_text(shown_bytes))
                    .clicked()
            {
                *action = Some(Do::Run("col.download", json!({ "scope": "shown" })));
            }
            if ui
                .add_enabled(!busy, egui::Button::new("Download everything…"))
                .on_hover_text(format!(
                    "{n_all} works, {}",
                    obs_engine::col::size_text(all_bytes)
                ))
                .clicked()
            {
                self.col_ui.confirm_everything = true;
            }
        });
        if self.col_ui.confirm_everything {
            let mut open = true;
            egui::Window::new("Download everything?")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ui.ctx(), |ui| {
                    ui.label(format!(
                        "All {n_all} works: {} of Markdown, and several times that once \
                         imported and indexed. It can take a long while; Cancel keeps the \
                         books already in.",
                        obs_engine::col::size_text(all_bytes)
                    ));
                    ui.horizontal(|ui| {
                        if ui.button("Download everything").clicked() {
                            *action =
                                Some(Do::Run("col.download", json!({ "scope": "everything" })));
                            self.col_ui.confirm_everything = false;
                        }
                        if ui.button("Cancel").clicked() {
                            self.col_ui.confirm_everything = false;
                        }
                    });
                });
            if !open {
                self.col_ui.confirm_everything = false;
            }
        }
        if busy {
            ui.label(crate::components::hint_text(
                "Downloading: the books come into your Library one by one; the Import report \
                 lists them.",
            ));
        }
        ui.add_space(space::S);
        if shown.is_empty() {
            ui.label("No work matches the filters.");
            return;
        }
        let rows: Vec<(ColWork, WorkStatus, bool)> = shown
            .iter()
            .map(|w| {
                (
                    (*w).clone(),
                    self.session.col_status(w),
                    self.session.col.selected.contains(&w.path),
                )
            })
            .collect();
        crate::components::keep_clear_of_scroll_bar(ui);
        egui::ScrollArea::vertical()
            .id_salt("col-works")
            .auto_shrink([false, false])
            .show_rows(ui, ROW_HEIGHT, rows.len(), |ui, range| {
                for (w, status, mut on) in rows[range].iter().cloned() {
                    ui.horizontal(|ui| {
                        ui.set_min_height(ROW_HEIGHT);
                        if crate::components::toggle(ui, &mut on, "")
                            .on_hover_text("Choose for Download")
                            .changed()
                        {
                            *action =
                                Some(Do::Run("col.select", json!({ "on": on, "paths": w.path })));
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            match status {
                                WorkStatus::NotInLibrary => {
                                    if ui
                                        .add_enabled(!busy, egui::Button::new("Download"))
                                        .accessible_name(format!("Download {}", w.title))
                                        .on_hover_text(obs_engine::col::size_text(w.size))
                                        .clicked()
                                    {
                                        *action = Some(Do::Run(
                                            "col.download",
                                            json!({ "paths": w.path }),
                                        ));
                                    }
                                }
                                WorkStatus::InLibrary | WorkStatus::Changed => {
                                    if ui
                                        .button("Open")
                                        .accessible_name(format!("Open {}", w.title))
                                        .clicked()
                                    {
                                        *action = Some(Do::Run(
                                            "resource.open",
                                            json!({ "id": w.resource_id() }),
                                        ));
                                    }
                                    let label = if status == WorkStatus::Changed {
                                        RichText::new("Changed in COL")
                                            .small()
                                            .strong()
                                            .color(theme::c().verdigris)
                                    } else {
                                        crate::components::hint_text("In your Library")
                                    };
                                    ui.label(label);
                                }
                            }
                            ui.add_space(space::M);
                            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                                ui.set_clip_rect(ui.max_rect().intersect(ui.clip_rect()));
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&w.title).strong().color(theme::c().ink),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(&w.path);
                                let mut facts = Vec::new();
                                if !w.author.is_empty() {
                                    facts.push(w.author.clone());
                                }
                                if let Some(y) = w.year {
                                    facts.push(y.to_string());
                                }
                                facts.push(w.kind.clone());
                                if !w.tradition.is_empty() {
                                    facts.push(w.tradition.clone());
                                }
                                facts.push(w.collection.clone());
                                facts.push(obs_engine::col::size_text(w.size));
                                ui.add(
                                    egui::Label::new(crate::components::hint_text(
                                        facts.join(" · "),
                                    ))
                                    .truncate(),
                                );
                            });
                        });
                    });
                }
            });
    }

    /// The search and the filters: type, tradition, collection, author and years.
    fn col_filters(&mut self, ui: &mut Ui, works: &[ColWork], action: &mut Option<Do>) {
        let mut f = self.session.col.filter.clone();
        let before = f.clone();
        let width =
            (ui.available_width() - size::SIDE_LIST).clamp(size::FIELD_NARROW, size::FIELD_WIDE);
        ui.horizontal_wrapped(|ui| {
            ui.add(crate::components::search_field(
                &mut f.text,
                "Search titles and authors",
                width,
            ))
            .accessible_name("Search titles and authors");
            let mut author = f.author.clone().unwrap_or_default();
            ui.add(
                egui::TextEdit::singleline(&mut author)
                    .hint_text("Author")
                    .desired_width(size::FIELD_NARROW),
            )
            .accessible_name("Author");
            f.author = Some(author).filter(|a| !a.trim().is_empty());
        });
        ui.horizontal_wrapped(|ui| {
            let combo = |ui: &mut Ui,
                         label: &str,
                         value: &mut Option<String>,
                         choices: Vec<(String, usize)>| {
                let shown = value
                    .clone()
                    .unwrap_or_else(|| format!("Any {}", label.to_lowercase()));
                egui::ComboBox::from_id_salt(("col-filter", label))
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(
                                value.is_none(),
                                format!("Any {}", label.to_lowercase()),
                            )
                            .clicked()
                        {
                            *value = None;
                        }
                        for (v, n) in choices {
                            let on = value.as_deref() == Some(v.as_str());
                            if ui.selectable_label(on, format!("{v} · {n}")).clicked() {
                                *value = Some(v);
                            }
                        }
                    });
            };
            let mut kinds = facet(works, |w| &w.kind);
            kinds.sort_by_key(|(k, _)| {
                WORK_TYPES
                    .iter()
                    .position(|t| t == k)
                    .unwrap_or(WORK_TYPES.len())
            });
            combo(ui, "Type", &mut f.kind, kinds);
            combo(
                ui,
                "Tradition",
                &mut f.tradition,
                facet(works, |w| &w.tradition),
            );
            combo(
                ui,
                "Collection",
                &mut f.collection,
                facet(works, |w| &w.collection),
            );
            ui.label(crate::components::hint_text("Years"));
            let (from, to) = &mut self.col_ui.years;
            for (text, hint) in [(from, "from"), (to, "to")] {
                ui.add(
                    egui::TextEdit::singleline(text)
                        .hint_text(hint)
                        .desired_width(size::HEADER_CHOOSER_NARROW),
                )
                .accessible_name(format!("Years {hint}"));
            }
        });
        f.year_from = self.col_ui.years.0.trim().parse().ok();
        f.year_to = self.col_ui.years.1.trim().parse().ok();
        if f != before {
            *action = Some(Do::Run(
                "col.filter",
                json!({
                    "text": f.text,
                    "kind": f.kind.unwrap_or_default(),
                    "author": f.author.unwrap_or_default(),
                    "tradition": f.tradition.unwrap_or_default(),
                    "collection": f.collection.unwrap_or_default(),
                    "year_from": f.year_from,
                    "year_to": f.year_to,
                }),
            ));
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn pick_col_folder(&mut self, _: &egui::Context) {}
    #[cfg(target_arch = "wasm32")]
    fn poll_col_picker(&mut self) {}

    #[cfg(not(target_arch = "wasm32"))]
    fn pick_col_folder(&mut self, ctx: &egui::Context) {
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("obs-col-picker".into())
            .spawn(move || {
                let chosen = rfd::FileDialog::new()
                    .set_title("Choose your copy of the Christian Open Library's books")
                    .pick_folder();
                let _ = tx.send(chosen);
                ctx.request_repaint();
            });
        match spawned {
            Ok(_) => self.col_ui.picker = Some(rx),
            Err(e) => self.toast(format!("Could not open the folder picker: {e}")),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn poll_col_picker(&mut self) {
        let Some(rx) = &self.col_ui.picker else {
            return;
        };
        match rx.try_recv() {
            Ok(path) => {
                self.col_ui.picker = None;
                if let Some(path) = path {
                    self.run("col.connect_local", &json!({ "path": path }));
                }
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.col_ui.picker = None,
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
    }
}
