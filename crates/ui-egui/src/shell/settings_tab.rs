//! Settings (#213, Ctrl+,): every preference in one place, under its section, with the
//! search box in the Tab's header. Each Setting is drawn from [`obs_engine::SETTINGS`] and
//! changed only by running the command the View menu, the toolbar, Home or Search already run
//! for it, so a change applies at once everywhere; its Reset link runs `settings.reset`.
//! Help → Keyboard shortcuts is embedded in its section (the same rows, filtered by the same
//! search).

use egui::{RichText, Ui};
use obs_engine::{FontSlot, SETTINGS, Setting, SettingKind, SettingsSection, TabId};
use serde_json::{Value, json};

use super::State;
use super::library_tab::section;
use crate::icons::{self, Icon};
use crate::theme;
use crate::tokens::{size, space};

/// A command a row asked for.
type Run = Option<(&'static str, Value)>;

/// The Settings Tab's state across frames.
#[derive(Debug, Default)]
pub(crate) struct SettingsUi {
    /// The Privacy and data action waiting for Clear or Cancel.
    confirming: Option<&'static str>,
    /// The frame the Settings Tab's search box was last drawn with the Tab focused: the box
    /// takes the keyboard when the Tab is focused again (Ctrl+, opens it ready to type).
    search_focused_frame: Option<u64>,
}

impl SettingsUi {
    /// The search box of the Settings Tab was drawn; returns whether it should take the
    /// keyboard: the Tab has just become the focused one.
    pub(super) fn search_drawn(&mut self, ui: &Ui, tab_focused: bool) -> bool {
        let frame = ui.ctx().cumulative_frame_nr();
        let was = self.search_focused_frame.is_some_and(|f| f + 1 >= frame);
        self.search_focused_frame = tab_focused.then_some(frame);
        tab_focused && !was
    }
}

/// Whether the Setting's row is shown for `query`: it matches, or for Keyboard shortcuts any
/// binding does (the section then lists those).
fn shown(state: &State, setting: &Setting, query: &str) -> bool {
    obs_engine::setting_matches(&state.session, setting, query)
        || (matches!(setting.kind, SettingKind::KeyboardShortcuts)
            && !query.is_empty()
            && state.any_binding_matches(query))
}

impl State {
    /// A Setting that asks first was run from the Command box (Clear History, #226): Settings
    /// Tab `tab` shows only it, asking Clear or Cancel.
    pub(super) fn confirm_setting(&mut self, tab: TabId, setting: &'static str) {
        let Some(found) = obs_engine::setting_by_id(setting) else {
            return;
        };
        if let Some(t) = self.tabs.get_mut(&tab) {
            t.filter = found.label.to_string();
        }
        self.settings_ui.confirming = Some(found.id);
    }

    pub(super) fn settings_body(&mut self, ui: &mut Ui, tab: TabId) {
        let query = (self.tabs.get(&tab))
            .map(|t| t.filter.trim().to_lowercase())
            .unwrap_or_default();
        let rows: Vec<&'static Setting> = (SETTINGS.iter())
            .filter(|s| shown(self, s, &query))
            .collect();
        if (rows.iter()).any(|s| matches!(s.kind, SettingKind::KeyboardShortcuts)) {
            self.begin_key_bindings(ui);
        }
        let mut run: Run = None;
        crate::components::page_frame().show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            egui::ScrollArea::vertical()
                .id_salt(("settings", tab))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.label(crate::components::title_text("Settings"));
                    ui.label(
                        RichText::new(
                            "Every preference in one place. Changes apply at once and are \
                                 kept in the User database; Reset puts one back to its default.",
                        )
                        .color(theme::c().muted),
                    );
                    for section_kind in SettingsSection::ALL {
                        let in_section: Vec<&Setting> = (rows.iter().copied())
                            .filter(|s| s.section == section_kind)
                            .collect();
                        if in_section.is_empty() {
                            continue;
                        }
                        ui.add_space(space::XL);
                        section(ui, section_kind.title());
                        for setting in in_section {
                            if let Some(r) = self.setting_row(ui, setting, &query) {
                                run = Some(r);
                            }
                        }
                    }
                    if rows.is_empty() {
                        ui.add_space(space::XL);
                        ui.label(
                            RichText::new(format!("No setting matches “{query}”."))
                                .color(theme::c().muted),
                        );
                    }
                });
        });
        if let Some((id, params)) = run {
            if id.starts_with("keys.") {
                self.run_key_action(id, &params);
            } else {
                self.run(id, &params);
            }
        }
    }

    /// One Setting: its name (a check box for an on/off one) and Reset link, the line saying
    /// what it does, and its control.
    fn setting_row(&mut self, ui: &mut Ui, setting: &'static Setting, query: &str) -> Run {
        let mut run = None;
        let is_default = self.session.setting_is_default(setting.id);
        ui.add_space(space::M);
        ui.horizontal(|ui| {
            match setting.kind {
                SettingKind::Toggle {
                    command,
                    param,
                    get,
                    ..
                } => {
                    let before = get(&mut self.session);
                    let mut on = before;
                    crate::components::toggle(
                        ui,
                        &mut on,
                        RichText::new(setting.label).strong().color(theme::c().ink),
                    );
                    if on != before {
                        run = Some((command, toggle_params(param, on)));
                    }
                }
                SettingKind::Action { .. } => {}
                _ => {
                    ui.label(RichText::new(setting.label).strong().color(theme::c().ink));
                }
            }
            if let Some(is_default) = is_default {
                // A list of Resources' own sizes, each with its Reset: this resets them all.
                let reset = match setting.kind {
                    SettingKind::ResourceTextSizes => "Reset all",
                    _ => "Reset",
                };
                let r = crate::components::link_enabled(ui, !is_default, reset)
                    .accessible_name(format!("{reset} {}", setting.label))
                    .on_hover_text("Back to the default")
                    .on_disabled_hover_text("Already the default");
                if r.clicked() {
                    run = Some(("settings.reset", json!({ "setting": setting.id })));
                }
            }
        });
        if !matches!(setting.kind, SettingKind::Action { .. }) {
            ui.label(crate::components::hint_text(setting.help));
        }
        self.setting_control(ui, setting, query).or(run)
    }

    /// The control that changes a Setting, through its command.
    fn setting_control(&mut self, ui: &mut Ui, setting: &'static Setting, query: &str) -> Run {
        let mut run = None;
        match setting.kind {
            SettingKind::Choice { command, .. } => {
                let (choices, current) = obs_engine::choices(&self.session, command);
                ui.horizontal_wrapped(|ui| {
                    for (id, label) in choices {
                        let r = crate::components::radio(ui, id == current, label)
                            .accessible_name(format!("{}: {label}", setting.label));
                        if r.clicked() && id != current {
                            run = Some((command, json!({ "value": id })));
                        }
                    }
                });
                // The tone the pages are in now, which Auto leaves to the theme (#240).
                if command == "view.page_tone" {
                    let tone = self.session.appearance.page_tone;
                    muted(ui, &super::page_tone::effective_text(tone, theme::c().dark));
                }
            }
            SettingKind::Toggle { .. } => {}
            SettingKind::TextSize => {
                ui.horizontal(|ui| {
                    let ink = theme::c().ink;
                    let smaller = icons::image(ui.ctx(), Icon::Remove, icons::BODY, ink);
                    if ui
                        .add(crate::components::button(smaller))
                        .accessible_name("Smaller text")
                        .on_hover_text(super::with_key(
                            &self.session,
                            "Smaller text",
                            "view.text_smaller",
                        ))
                        .clicked()
                    {
                        run = Some(("view.text_smaller", json!({})));
                    }
                    ui.label(
                        RichText::new(format!("{:.0} pt", self.session.text.size))
                            .color(theme::c().ink),
                    );
                    let larger = icons::image(ui.ctx(), Icon::Add, icons::BODY, ink);
                    if ui
                        .add(crate::components::button(larger))
                        .accessible_name("Larger text")
                        .on_hover_text(super::with_key(
                            &self.session,
                            "Larger text",
                            "view.text_larger",
                        ))
                        .clicked()
                    {
                        run = Some(("view.text_larger", json!({})));
                    }
                });
            }
            SettingKind::ScrollSpeed => {
                use obs_engine::appearance::{SCROLL_SPEED_MAX, SCROLL_SPEED_MIN};
                let before = self.session.appearance.scroll_speed;
                let mut speed = before;
                // Logarithmic, so 1× sits in the middle of 0.25× and 4×. Scrolling this Tab
                // uses a new speed from the next frame: its own live preview.
                ui.scope(|ui| {
                    // egui paints the rail in the field colour, which is the Tab's own: draw
                    // it in the rule colour so the track shows in every theme.
                    ui.visuals_mut().widgets.inactive.bg_fill = theme::c().rule;
                    ui.spacing_mut().slider_width = size::FIELD;
                    ui.add(
                        egui::Slider::new(&mut speed, SCROLL_SPEED_MIN..=SCROLL_SPEED_MAX)
                            .logarithmic(true)
                            .custom_formatter(|v, _| obs_engine::appearance::scroll_speed_label(v))
                            .custom_parser(|t| {
                                t.trim().trim_end_matches(['×', 'x']).trim().parse().ok()
                            }),
                    )
                    .accessible_name(setting.label);
                });
                if speed != before {
                    run = Some(("view.scroll_speed", json!({ "value": f64::from(speed) })));
                }
            }
            SettingKind::ResourceTextSizes => {
                let own: Vec<(String, f32)> = (self.session.resource_text_sizes().iter())
                    .map(|(id, size)| (id.to_string(), size))
                    .collect();
                // None: the help line says the others follow the Text size.
                for (id, size) in own {
                    let name = self.session.text_size_name(&id);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("{name} — {size:.0} pt")).color(theme::c().ink),
                        );
                        let r = crate::components::link(ui, "Reset")
                            .accessible_name(format!("Reset the Text size of {name}"))
                            .on_hover_text(format!("{name} follows the Text size again"));
                        if r.clicked() {
                            run = Some((
                                "settings.reset",
                                json!({ "setting": setting.id, "resource": id }),
                            ));
                        }
                    });
                }
            }
            SettingKind::Font(slot) => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(font_name(&self.session, slot)).color(theme::c().ink));
                    if ui
                        .button("Choose…")
                        .accessible_name(format!("Choose the {}", setting.label))
                        .on_hover_text("Opens the Fonts window")
                        .clicked()
                    {
                        run = Some(("view.fonts", json!({})));
                    }
                });
            }
            SettingKind::NotebooksInText => match self.session.notebooks() {
                Ok(notebooks) => {
                    for n in notebooks {
                        let mut on = n.show_in_text;
                        crate::components::toggle(ui, &mut on, &n.name)
                            .on_hover_text(format!("Draw “{}” in Resource text", n.name));
                        if on != n.show_in_text {
                            run =
                                Some(("notebook.show_in_text", json!({ "id": n.id, "show": on })));
                        }
                    }
                }
                Err(_) if self.session.user.is_none() => muted(
                    ui,
                    "Notebooks are kept in the User database, which is not open.",
                ),
                // The plain message; the error itself on hover (#223).
                Err(e) => {
                    ui.label(
                        RichText::new("The Notebooks could not be read.").color(theme::c().muted),
                    )
                    .on_hover_text(e);
                }
            },
            SettingKind::HomeCards => {
                let home = self.session.home_settings();
                let hidden: Vec<&str> = home.hidden().map(|c| c.title()).collect();
                let text = if hidden.is_empty() {
                    "Every card is shown".to_string()
                } else {
                    format!("Hidden: {}", hidden.join(", "))
                };
                ui.horizontal(|ui| {
                    muted(ui, &text);
                    if ui.button("Open Home").clicked() {
                        run = Some(("home.open", json!({})));
                    }
                });
            }
            SettingKind::GuideSections => {
                let arranged: Vec<&str> = (obs_engine::Guide::ALL.into_iter())
                    .filter(|g| {
                        self.session.guides.arrangement(*g)
                            != obs_engine::GuideArrangement::default_for(*g)
                    })
                    .map(obs_engine::Guide::title)
                    .collect();
                if arranged.is_empty() {
                    muted(ui, "Every Guide has its sections in their usual order");
                } else {
                    muted(ui, &format!("Arranged: {}", arranged.join(", ")));
                }
            }
            SettingKind::ToolbarLaunchers => {
                let shown: Vec<&str> = (self.session.toolbar.shown().into_iter())
                    .map(obs_engine::Launcher::title)
                    .collect();
                ui.horizontal(|ui| {
                    let text = if shown.is_empty() {
                        "None shown".to_string()
                    } else {
                        shown.join(", ")
                    };
                    ui.label(RichText::new(text).color(theme::c().ink));
                    if ui.button("Customize toolbar…").clicked() {
                        run = Some(("view.toolbar_customize", json!({})));
                    }
                });
            }
            SettingKind::KeyboardShortcuts => {
                // The whole list when the search is about shortcuts; else the bindings it
                // names.
                let filter = if obs_engine::setting_matches(&self.session, setting, query) {
                    ""
                } else {
                    query
                };
                let (_, r) = self.key_binding_rows(ui, filter);
                run = r;
            }
            SettingKind::LibraryFolder => {
                let dir = (self.session.library.directory()).map_or_else(
                    || "No library folder".to_string(),
                    |d| d.display().to_string(),
                );
                ui.add(
                    egui::Label::new(RichText::new(dir).monospace().color(theme::c().ink))
                        .selectable(true),
                );
                muted(
                    ui,
                    "Set with the OBS_LIBRARY environment variable before launch.",
                );
            }
            SettingKind::Action { command, confirm } => {
                run = self.action(ui, setting, command, confirm);
            }
        }
        run
    }

    /// A Setting that is something to do: its button (Privacy and data asks first) and what
    /// it does.
    fn action(
        &mut self,
        ui: &mut Ui,
        setting: &'static Setting,
        command: &'static str,
        asks: bool,
    ) -> Run {
        let mut run = None;
        let enabled = self.session.is_enabled(command);
        if asks && self.settings_ui.confirming == Some(setting.id) {
            ui.horizontal(|ui| {
                ui.colored_label(
                    theme::c().warn,
                    format!("{}? This cannot be undone.", setting.label),
                );
                if ui
                    .button("Clear")
                    .accessible_name(format!("Confirm {}", setting.label))
                    .clicked()
                {
                    self.settings_ui.confirming = None;
                    run = Some((command, json!({})));
                }
                if ui.button("Cancel").clicked() {
                    self.settings_ui.confirming = None;
                }
            });
            return run;
        }
        let label = if asks {
            format!("{}…", setting.label)
        } else {
            (self.session.commands().get(command))
                .map_or(setting.label, |c| c.label)
                .to_string()
        };
        ui.horizontal(|ui| {
            let r = ui
                .add_enabled(enabled, crate::components::button(label))
                .on_disabled_hover_text("Nothing to do now");
            if r.clicked() {
                if asks {
                    self.settings_ui.confirming = Some(setting.id);
                } else {
                    run = Some((command, json!({})));
                }
            }
            muted(ui, setting.help);
        });
        run
    }
}

/// `{ param: on }`, or nothing for a command that toggles.
fn toggle_params(param: Option<&'static str>, on: bool) -> Value {
    match param {
        Some(name) => {
            let mut m = serde_json::Map::new();
            m.insert(name.to_string(), Value::Bool(on));
            Value::Object(m)
        }
        None => json!({}),
    }
}

/// The font a slot draws with now: the chosen one, or the bundled one.
fn font_name(session: &obs_engine::Session, slot: FontSlot) -> String {
    match session.fonts.get(slot) {
        Some(c) if c.style == "Regular" => c.family.clone(),
        Some(c) => format!("{} {}", c.family, c.style),
        None => match slot {
            FontSlot::Interface => "Bundled".to_string(),
            _ => format!("{} (bundled)", slot.bundled_family()),
        },
    }
}

fn muted(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).color(theme::c().muted));
}
