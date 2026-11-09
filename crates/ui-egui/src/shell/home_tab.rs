//! Home (#178): a card each for Today's reading, Continue where you left off, History and
//! Favorites, the Verse of the day, Quick start and the library summary, laid out in two
//! columns when there is room. Each card's header moves it up or down or hides it (the Home
//! arrangement is one preference); the line under the title turns "Open Home at launch" on
//! or off, resets the cards, and shows hidden cards again. Every row opens its
//! place with one click (Ctrl/Cmd-click: in a new split) through the same commands as the
//! Tools it comes from.

use egui::{RichText, Ui, vec2};
use obs_engine::{HomeCard, PlanDate, QUICK_STARTS, TabId, day_heading};
use serde_json::{Value, json};

use super::State;
use crate::icons::{self, Icon};
use crate::theme;
use crate::tokens::{size, space, text};

/// Narrower than this, the cards stack in one column.
const TWO_COLUMNS: f32 = size::TWO_COLUMNS;

/// A command a card asked for.
type Run = Option<(&'static str, Value)>;

impl State {
    /// The toolbar's Home button, left of Today's reading. Returns whether it ran a command.
    pub(super) fn home_button(&mut self, ui: &mut Ui) -> bool {
        let open = self
            .session
            .workspace
            .focused()
            .and_then(|t| self.session.workspace.tab(t))
            .is_some_and(|t| t.kind == obs_engine::TabKind::Home);
        let r = ui
            .add(
                crate::components::button((
                    icons::image(ui.ctx(), Icon::Home, icons::BODY, theme::c().muted),
                    crate::components::label_text("Home"),
                ))
                .selected(open),
            )
            .on_hover_text(super::with_key(
                &self.session,
                "Today's reading, where you left off, Quick start and your library",
                "home.open",
            ));
        if !r.clicked() {
            return false;
        }
        self.run("home.open", &json!({}));
        true
    }

    pub(super) fn home_body(&mut self, ui: &mut Ui, tab: TabId) {
        let settings = self.session.home_settings().clone();
        let visible: Vec<HomeCard> = settings.visible().collect();
        let hidden: Vec<HomeCard> = settings.hidden().collect();
        let today = self.session.plan_today();
        let mut run: Run = None;
        crate::components::page_frame().show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            egui::ScrollArea::vertical()
                .id_salt(("home", tab))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("Home")
                            .font(theme::display(text::DISPLAY))
                            .color(theme::c().ink),
                    );
                    ui.horizontal_wrapped(|ui| {
                        muted(ui, &today.long_label());
                        ui.add_space(space::L);
                        home_settings_row(ui, settings.open_at_launch, &mut run);
                    });
                    hidden_row(ui, &hidden, &mut run);
                    ui.add_space(space::L);
                    if visible.is_empty() {
                        muted(ui, "Every card is hidden.");
                    }
                    if ui.available_width() >= TWO_COLUMNS {
                        ui.columns(2, |cols| {
                            for (i, card) in visible.iter().enumerate() {
                                self.home_card(&mut cols[i % 2], *card, &visible, &mut run);
                            }
                        });
                    } else {
                        for card in &visible {
                            self.home_card(ui, *card, &visible, &mut run);
                        }
                    }
                });
        });
        if let Some((id, params)) = run {
            self.session.workspace.user_focused(tab);
            self.run(id, &params);
        }
    }

    /// One card: its header (title, move up/down, hide) and its body.
    fn home_card(&mut self, ui: &mut Ui, card: HomeCard, visible: &[HomeCard], run: &mut Run) {
        let place = visible.iter().position(|c| *c == card).unwrap_or(0);
        let all = self.session.home_settings().clone();
        crate::components::card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(crate::components::subtitle_text(card.title()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let title = card.title();
                    if card_button(ui, Icon::Close, &format!("Hide {title}")).clicked() {
                        *run = Some(("home.hide_card", json!({ "card": card.id() })));
                    }
                    // Moves are within the whole order, hidden cards included: past the
                    // next shown card down, to the previous shown card's place up.
                    let next = visible.get(place + 1).and_then(|c| all.position(*c));
                    let prev = place
                        .checked_sub(1)
                        .and_then(|i| visible.get(i))
                        .and_then(|c| all.position(*c));
                    if ui
                        .add_enabled_ui(next.is_some(), |ui| {
                            card_button(ui, Icon::MoveDown, &format!("Move {title} down"))
                        })
                        .inner
                        .clicked()
                        && let Some(to) = next
                    {
                        *run = Some(("home.move_card", json!({ "card": card.id(), "to": to })));
                    }
                    if ui
                        .add_enabled_ui(prev.is_some(), |ui| {
                            card_button(ui, Icon::MoveUp, &format!("Move {title} up"))
                        })
                        .inner
                        .clicked()
                        && let Some(to) = prev
                    {
                        *run = Some(("home.move_card", json!({ "card": card.id(), "to": to })));
                    }
                });
            });
            ui.add_space(space::XS);
            match card {
                HomeCard::TodaysReading => self.todays_reading_card(ui, run),
                HomeCard::ContinueReading => self.continue_card(ui, run),
                HomeCard::HistoryAndFavorites => self.history_favorites_card(ui, run),
                HomeCard::VerseOfTheDay => self.verse_of_the_day_card(ui, run),
                HomeCard::QuickStart => self.quick_start_card(ui, run),
                HomeCard::LibrarySummary => self.library_summary_card(ui, run),
            }
        });
        ui.add_space(space::L);
    }

    fn todays_reading_card(&mut self, ui: &mut Ui, run: &mut Run) {
        if self.session.user.is_none() {
            muted(
                ui,
                "Reading plans are kept in the User database, which is not open.",
            );
            return;
        }
        let plans = self.session.home_plans();
        if plans.is_empty() {
            muted(ui, "No Reading plan yet.");
            if ui.link("Start a Reading plan…").clicked() {
                *run = Some(("tool.reading_plans", json!({})));
            }
            return;
        }
        let split = self.split_held;
        for plan in &plans {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&plan.name).color(theme::c().ink));
                if plan.behind > 0 {
                    let behind = if plan.behind == 1 {
                        "Behind by 1 day".to_string()
                    } else {
                        format!("Behind by {} days", plan.behind)
                    };
                    if ui
                        .add(
                            crate::components::button(RichText::new(behind).color(theme::c().warn))
                                .frame(false),
                        )
                        .on_hover_text("Catch up or Reschedule in Reading plans")
                        .clicked()
                    {
                        *run = Some(("tool.reading_plans", json!({})));
                    }
                }
            });
            ui.horizontal_wrapped(|ui| {
                for r in &plan.readings {
                    let text = if r.read {
                        format!("{} · read", r.label)
                    } else {
                        r.label.clone()
                    };
                    if ui
                        .button(RichText::new(text).size(text::BODY))
                        .on_hover_text("Open in the plan's Bible (Ctrl/Cmd-click: in a new split)")
                        .clicked()
                    {
                        *run = Some((
                            "reading_plan.open",
                            json!({ "plan": plan.id, "position": r.position, "split": split }),
                        ));
                    }
                }
                if plan.readings.is_empty() {
                    let next = plan.next.map(PlanDate::short_label).unwrap_or_default();
                    muted(ui, &format!("Next reading: {next}"));
                }
            });
            ui.add_space(space::XS);
        }
    }

    fn continue_card(&mut self, ui: &mut Ui, run: &mut Run) {
        let Some(v) = self.session.home_continue().cloned() else {
            muted(
                ui,
                "Where you read last shows here once you have opened a Bible or a Guide.",
            );
            return;
        };
        if ui
            .add(
                crate::components::button(
                    RichText::new(&v.label)
                        .size(text::SUBTITLE)
                        .color(theme::c().lapis),
                )
                .frame(false),
            )
            .on_hover_text("Open it again (Ctrl/Cmd-click: in a new split)")
            .clicked()
        {
            *run = Some((
                "history.open",
                json!({ "id": v.id, "split": self.split_held }),
            ));
        }
        muted(
            ui,
            &format!("{}, {}", day_heading(&v.day, v.days_ago), v.time),
        );
    }

    fn history_favorites_card(&mut self, ui: &mut Ui, run: &mut Run) {
        let split = self.split_held;
        heading(ui, "History");
        let history: Vec<(i64, String)> = self
            .session
            .home_history()
            .iter()
            .map(|v| (v.id, v.label.clone()))
            .collect();
        if history.is_empty() {
            let shown = self.session.home_continue().is_some();
            muted(
                ui,
                if shown {
                    "Nothing else yet."
                } else {
                    "Nothing yet."
                },
            );
        }
        for (id, label) in &history {
            if place_row(ui, None, label) {
                *run = Some(("history.open", json!({ "id": id, "split": split })));
            }
        }
        if ui.link("All History…").clicked() {
            *run = Some(("tool.history", json!({})));
        }
        ui.add_space(space::S);
        heading(ui, "Favorites");
        let favorites: Vec<(i64, String)> = self
            .session
            .home_favorites()
            .iter()
            .map(|f| (f.id, f.label.clone()))
            .collect();
        if favorites.is_empty() {
            let star = super::with_key(
                &self.session,
                "Star a place with the star in a Tab's header",
                "favorite.add",
            );
            muted(ui, &format!("{star} to keep it here."));
        }
        for (id, label) in &favorites {
            if place_row(ui, Some(Icon::StarFilled), label) {
                *run = Some(("favorite.open", json!({ "id": id, "split": split })));
            }
        }
        if !favorites.is_empty() && ui.link("All Favorites…").clicked() {
            *run = Some(("tool.favorites", json!({})));
        }
    }

    fn verse_of_the_day_card(&mut self, ui: &mut Ui, run: &mut Run) {
        let votd = self.session.verse_of_the_day();
        if ui
            .add(
                crate::components::button(
                    crate::components::subtitle_text(&votd.label)
                        .color(theme::c().lapis),
                )
                .frame(false),
            )
            .on_hover_text(
                "Read it in the first Bible of your Preferred order (Ctrl/Cmd-click: in a new split)",
            )
            .clicked()
        {
            *run = Some((
                "home.read_verse_of_the_day",
                json!({ "split": self.split_held }),
            ));
        }
        if votd.bible.is_some() {
            self.preview(ui, votd.passage, false);
        } else {
            muted(ui, "Add a Bible to your library to read it here.");
            if ui.link("Get Bibles…").clicked() {
                *run = Some(("library.get_resources", json!({ "type": "bibles" })));
            }
        }
    }

    fn quick_start_card(&mut self, ui: &mut Ui, run: &mut Run) {
        for q in &QUICK_STARTS {
            ui.horizontal(|ui| {
                if ui
                    .add(
                        crate::components::button(RichText::new(q.label).size(text::BODY))
                            .min_size(vec2(size::FIELD_NARROW, size::CONTROL)),
                    )
                    .on_hover_text(q.help)
                    .clicked()
                {
                    *run = Some((q.command, self.session.quick_start_params(q)));
                }
                ui.add(egui::Label::new(RichText::new(q.help).color(theme::c().muted)).wrap());
            });
        }
    }

    fn library_summary_card(&mut self, ui: &mut Ui, run: &mut Run) {
        egui::Grid::new("home-library")
            .num_columns(2)
            .spacing(vec2(space::L, space::XS))
            .show(ui, |ui| {
                for c in self.session.library_summary() {
                    ui.label(
                        RichText::new(obs_engine::library_search::type_plural(c.resource_type))
                            .color(theme::c().ink),
                    );
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(c.count.to_string()).color(theme::c().ink));
                        if c.count > 0 {
                            return;
                        }
                        ui.add_space(space::L);
                        match c.get {
                            Some(f) => {
                                if ui.link(format!("Get {}…", f.label())).clicked() {
                                    *run = Some((
                                        "library.get_resources",
                                        json!({ "type": f.id().unwrap_or_default() }),
                                    ));
                                }
                            }
                            None => {
                                if ui
                                    .link("Starter library")
                                    .on_hover_text("These come with the Starter library")
                                    .clicked()
                                {
                                    *run = Some(("library.starter", json!({})));
                                }
                            }
                        }
                    });
                    ui.end_row();
                }
            });
        ui.add_space(space::S);
        let starter = self.session.starter_summary();
        // Each link on a line of its own: a link wrapped over two lines leaves an unnamed
        // AccessKit node for its second line.
        if starter.in_library == 0 {
            muted(ui, "The Starter library is not in your library.");
            if ui.link("Add the Starter library…").clicked() {
                *run = Some(("library.starter", json!({})));
            }
        } else {
            let of = match starter.offered {
                Some(n) if n != starter.in_library => format!(" of {n}"),
                _ => String::new(),
            };
            muted(
                ui,
                &format!("Starter library: {}{of} Resources", starter.in_library),
            );
            if ui.link("Starter library").clicked() {
                *run = Some(("library.starter", json!({})));
            }
        }
        if ui.link("Open the Library").clicked() {
            *run = Some(("library.open", json!({})));
        }
    }
}

/// "Open Home at launch" and Reset cards.
fn home_settings_row(ui: &mut Ui, open_at_launch: bool, run: &mut Run) {
    let mut on = open_at_launch;
    if ui
        .checkbox(&mut on, "Open Home at launch")
        .on_hover_text("Show Home at every launch, on top of the Workspace as you left it")
        .changed()
    {
        *run = Some(("home.open_at_launch", json!({ "on": on })));
    }
    if ui
        .small_button("Reset cards")
        .on_hover_text("Every card shown, in the first order")
        .clicked()
    {
        *run = Some(("home.reset_cards", json!({})));
    }
}

/// The hidden cards, each with Show.
fn hidden_row(ui: &mut Ui, hidden: &[HomeCard], run: &mut Run) {
    if hidden.is_empty() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        muted(ui, "Hidden:");
        for card in hidden {
            if ui
                .small_button(format!("Show {}", card.title()))
                .on_hover_text("Show this card on Home again")
                .clicked()
            {
                *run = Some((
                    "home.hide_card",
                    json!({ "card": card.id(), "hidden": false }),
                ));
            }
        }
    });
}

/// A small header button with an accessible name.
fn card_button(ui: &mut Ui, icon: Icon, name: &str) -> egui::Response {
    crate::components::icon_button(ui, icon, name)
}

/// One place to open: returns whether it was clicked.
fn place_row(ui: &mut Ui, icon: Option<Icon>, label: &str) -> bool {
    let text = RichText::new(label).size(text::BODY).color(theme::c().ink);
    let button = match icon {
        Some(icon) => crate::components::button((
            icons::image(ui.ctx(), icon, icons::BODY, theme::c().gilt),
            text,
        )),
        None => crate::components::button(text),
    };
    ui.add(
        button
            .frame(false)
            .min_size(vec2(ui.available_width(), size::CONTROL_COMPACT)),
    )
    .on_hover_text("Open (Ctrl/Cmd-click: in a new split)")
    .clicked()
}

fn muted(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).color(theme::c().muted));
}

fn heading(ui: &mut Ui, text: &str) {
    crate::components::section_header(ui, text);
}
