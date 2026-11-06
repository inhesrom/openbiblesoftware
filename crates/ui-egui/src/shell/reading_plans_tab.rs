//! Tools → Reading plans (#147), and Today's reading on the toolbar.
//!
//! The Tool lists each Reading plan with today's Readings to tick off (each Passage opens in
//! the plan's Bible), the Readings missed, and a "Behind by N days" bar with Catch up and
//! Reschedule. Its New plan form makes a plan from a template with a start day, a Bible and
//! the days of the week. The toolbar's Today's reading opens the first plan's reading in one
//! click.

use egui::{RichText, Ui};
use obs_engine::{PlanDate, ReadingPlan, TabId, WEEKDAY_NAMES, Weekdays, reading_plans};
use obs_model::ResourceType;
use obs_scripture::{RefStyle, Standard, format_passage};
use serde_json::{Value, json};

use super::State;
use crate::theme;

/// The New plan form's fields.
pub(super) struct PlansUi {
    /// The form is open (it always shows when there are no plans).
    form_open: bool,
    /// A template id, or [`reading_plans::CUSTOM`].
    template: String,
    /// `YYYY-MM-DD`; empty: today.
    start: String,
    /// A Bible's resource id; none: the ★ Bible.
    bible: Option<String>,
    weekdays: Weekdays,
    /// A custom plan's books and number of days.
    books: String,
    days: String,
    /// The plan whose Delete is being confirmed.
    confirm_remove: Option<i64>,
}

impl Default for PlansUi {
    fn default() -> Self {
        Self {
            form_open: false,
            template: "bible-canonical".into(),
            start: String::new(),
            bible: None,
            weekdays: Weekdays::ALL,
            books: String::new(),
            days: "30".into(),
            confirm_remove: None,
        }
    }
}

/// One Reading as the Tool lists it.
struct Row {
    position: u32,
    read: bool,
    day: PlanDate,
    /// Each Passage, as it is written.
    passages: Vec<String>,
}

fn rows<'a>(readings: impl IntoIterator<Item = &'a obs_engine::Reading>) -> Vec<Row> {
    readings
        .into_iter()
        .map(|r| Row {
            position: r.position,
            read: r.read,
            day: r.day,
            passages: r
                .passages
                .iter()
                .map(|p| format_passage(*p, &Standard, RefStyle::Short))
                .collect(),
        })
        .collect()
}

/// What the Tool shows of one plan.
struct PlanView {
    id: i64,
    name: String,
    summary: String,
    behind: usize,
    today: Vec<Row>,
    missed: Vec<Row>,
    next: Option<(PlanDate, Vec<Row>)>,
    finished: bool,
}

/// Missed Readings listed under the plan (the rest are counted).
const MISSED_SHOWN: usize = 7;

impl State {
    fn plan_view(&self, plan: &ReadingPlan, today: PlanDate) -> PlanView {
        let bible = self
            .session
            .library
            .abbreviation_of(&plan.bible_id)
            .map_or_else(|| "★ Bible".to_string(), str::to_string);
        let summary = format!(
            "{bible} · {} · {} of {} read · ends {}",
            plan.weekdays.label(),
            plan.read_count(),
            plan.readings.len(),
            plan.end().label_with_year()
        );
        let next = plan
            .readings
            .iter()
            .filter(|r| r.day > today)
            .map(|r| r.day)
            .min()
            .map(|d| (d, rows(plan.due_on(d))));
        PlanView {
            id: plan.id,
            name: plan.name.clone(),
            summary,
            behind: plan.behind_days(today),
            today: rows(plan.due_on(today)),
            missed: rows(plan.overdue(today)),
            next,
            finished: plan.is_finished(),
        }
    }

    pub(super) fn reading_plans_body(&mut self, ui: &mut Ui, _tab: TabId) {
        let today = self.session.plan_today();
        let views: Vec<PlanView> = self
            .session
            .reading_plans()
            .iter()
            .map(|p| self.plan_view(p, today))
            .collect();
        let split = self.split_held;
        let mut run: Option<(&'static str, Value)> = None;
        let mut form_open = self.plans_ui.form_open || views.is_empty();
        let mut confirm = self.plans_ui.confirm_remove;
        egui::Frame::new()
            .fill(theme::c().field)
            .inner_margin(egui::Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                if self.session.user.is_none() {
                    muted(
                        ui,
                        "Reading plans are kept in the User database, which is not open.",
                    );
                    return;
                }
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("Today is {}", today.long_label()))
                                    .color(theme::c().muted),
                            );
                            if !views.is_empty() && !form_open && ui.button("New plan…").clicked()
                            {
                                form_open = true;
                            }
                        });
                        if form_open {
                            ui.add_space(6.0);
                            if let Some(r) = self.plan_form(ui, &mut form_open, views.is_empty()) {
                                run = Some(r);
                            }
                        }
                        for v in &views {
                            ui.add_space(10.0);
                            plan_card(ui, v, split, &mut run, &mut confirm);
                        }
                    });
            });
        self.plans_ui.form_open = form_open;
        self.plans_ui.confirm_remove = confirm;
        if let Some((id, params)) = run {
            let created = id == "reading_plan.create";
            if self.run(id, &params).is_some() && created {
                self.plans_ui = PlansUi::default();
            }
        }
    }

    /// The New plan form; returns `reading_plan.create` once Start plan is clicked.
    fn plan_form(
        &mut self,
        ui: &mut Ui,
        open: &mut bool,
        first: bool,
    ) -> Option<(&'static str, Value)> {
        let bibles: Vec<(String, String)> = self
            .session
            .library
            .of_type(ResourceType::Bible)
            .map(|e| {
                (
                    e.manifest.resource_id.clone(),
                    format!("{} — {}", e.manifest.abbreviation, e.manifest.title),
                )
            })
            .collect();
        let f = &mut self.plans_ui;
        let mut out = None;
        egui::Frame::new()
            .fill(theme::c().surface)
            .stroke(egui::Stroke::new(1.0, theme::c().rule))
            .corner_radius(6.0)
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.label(RichText::new("New Reading plan").strong().size(15.0));
                ui.add_space(4.0);
                heading(ui, "Plan");
                for t in reading_plans::templates() {
                    ui.radio_value(&mut f.template, t.id.clone(), &t.name)
                        .on_hover_text(&t.about);
                }
                ui.radio_value(
                    &mut f.template,
                    reading_plans::CUSTOM.to_string(),
                    "Custom: chosen books over N days",
                );
                if let Some(t) = reading_plans::template(&f.template) {
                    muted(ui, &t.about);
                }
                if f.template == reading_plans::CUSTOM {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Books");
                        ui.add(
                            egui::TextEdit::singleline(&mut f.books)
                                .hint_text("e.g. Rom-Gal; Jas")
                                .desired_width(180.0),
                        )
                        .accessible_name("Books");
                        ui.label("over");
                        ui.add(egui::TextEdit::singleline(&mut f.days).desired_width(44.0))
                            .accessible_name("Over how many days");
                        ui.label("days");
                    });
                }
                ui.add_space(6.0);
                heading(ui, "Start");
                ui.add(
                    egui::TextEdit::singleline(&mut f.start)
                        .hint_text("Today (or YYYY-MM-DD)")
                        .desired_width(180.0),
                )
                .accessible_name("Start");
                ui.add_space(6.0);
                heading(ui, "Bible");
                let shown = f
                    .bible
                    .as_ref()
                    .and_then(|b| bibles.iter().find(|(id, _)| id == b))
                    .map_or("★ Bible (first in your Preferred order)", |(_, l)| l);
                egui::ComboBox::from_id_salt("reading-plan-bible")
                    .selected_text(shown)
                    .width(260.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut f.bible, None, "★ Bible");
                        for (id, label) in &bibles {
                            ui.selectable_value(&mut f.bible, Some(id.clone()), label);
                        }
                    })
                    .response
                    .accessible_name("Bible");
                ui.add_space(6.0);
                heading(ui, "Read on");
                ui.horizontal_wrapped(|ui| {
                    for (i, name) in WEEKDAY_NAMES.iter().enumerate() {
                        let on = f.weekdays.has(i);
                        if ui.selectable_label(on, &name[..3]).clicked() {
                            f.weekdays = f.weekdays.with(i, !on);
                        }
                    }
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let ok = !f.weekdays.is_empty()
                        && (f.template != reading_plans::CUSTOM || !f.books.trim().is_empty());
                    if ui
                        .add_enabled(ok, egui::Button::new("Start plan"))
                        .clicked()
                    {
                        let mut p = json!({
                            "template": f.template,
                            "start": f.start.trim(),
                            "weekdays": weekday_param(f.weekdays),
                        });
                        if let Some(b) = &f.bible {
                            p["bible"] = json!(b);
                        }
                        if f.template == reading_plans::CUSTOM {
                            p["books"] = json!(f.books.trim());
                            p["days"] = json!(f.days.trim().parse::<i64>().unwrap_or(0));
                        }
                        out = Some(("reading_plan.create", p));
                    }
                    if !first && ui.button("Cancel").clicked() {
                        *open = false;
                    }
                });
            });
        out
    }

    /// The toolbar's Today's reading: the first plan with something to read today (or
    /// missed), one click from opening it; "Behind by N days" opens the Tool. Returns whether
    /// it ran a command.
    pub(super) fn todays_reading_button(&mut self, ui: &mut Ui) -> bool {
        let today = self.session.plan_today();
        let Some(plan) = self.session.toolbar_plan() else {
            return false;
        };
        let id = plan.id;
        let behind = plan.behind_days(today);
        let due = plan.due_on(today);
        let unread: Vec<&str> = due
            .iter()
            .filter(|r| !r.read)
            .map(|r| r.label.as_str())
            .collect();
        let (label, open) = if !unread.is_empty() {
            (format!("Today: {}", unread.join("; ")), true)
        } else if !due.is_empty() {
            ("✓ Today's reading".to_string(), false)
        } else if behind > 0 {
            (
                format!(
                    "Next: {}",
                    plan.todays_reading(today).map_or("", |r| &r.label)
                ),
                true,
            )
        } else {
            let next = plan.next_day(today).map(PlanDate::short_label);
            (format!("Next reading: {}", next.unwrap_or_default()), false)
        };
        let hover = format!("{} — open in its Bible", plan.name);
        let mut run: Option<(&'static str, Value)> = None;
        if behind > 0 {
            let r = ui.add(
                egui::Button::new(
                    RichText::new(behind_text(behind))
                        .size(12.5)
                        .color(theme::c().warn),
                )
                .frame(false),
            );
            if r.on_hover_text("Catch up or Reschedule in Reading plans")
                .clicked()
            {
                run = Some(("tool.reading_plans", json!({})));
            }
        }
        let r = ui.add(egui::Button::new(RichText::new(label).size(12.5)));
        let r = if open {
            r.on_hover_text(hover)
        } else {
            r.on_hover_text("Open Reading plans")
        };
        if r.clicked() {
            run = Some(if open {
                (
                    "reading_plan.open",
                    json!({ "plan": id, "split": self.split_held }),
                )
            } else {
                ("tool.reading_plans", json!({}))
            });
        }
        let Some((cmd, p)) = run else {
            return false;
        };
        self.run(cmd, &p);
        true
    }
}

/// "mon,tue,…" for `reading_plan.create`.
fn weekday_param(w: Weekdays) -> String {
    (0..7)
        .filter(|&i| w.has(i))
        .map(|i| WEEKDAY_NAMES[i][..3].to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(",")
}

fn behind_text(days: usize) -> String {
    if days == 1 {
        "Behind by 1 day".into()
    } else {
        format!("Behind by {days} days")
    }
}

fn plan_card(
    ui: &mut Ui,
    v: &PlanView,
    split: bool,
    run: &mut Option<(&'static str, Value)>,
    confirm: &mut Option<i64>,
) {
    egui::Frame::new()
        .fill(theme::c().surface)
        .stroke(egui::Stroke::new(1.0, theme::c().rule))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&v.name)
                        .strong()
                        .size(15.0)
                        .color(theme::c().ink),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if *confirm == Some(v.id) {
                        if ui.button("Cancel").clicked() {
                            *confirm = None;
                        }
                        if ui.button("Delete").clicked() {
                            *run = Some(("reading_plan.remove", json!({ "plan": v.id })));
                            *confirm = None;
                        }
                        ui.label("Delete this plan and its progress?");
                    } else if ui
                        .add(
                            egui::Button::new(RichText::new("Delete…").color(theme::c().muted))
                                .frame(false),
                        )
                        .clicked()
                    {
                        *confirm = Some(v.id);
                    }
                });
            });
            muted(ui, &v.summary);
            if v.behind > 0 {
                ui.add_space(6.0);
                egui::Frame::new()
                    .fill(theme::c().hover)
                    .corner_radius(4.0)
                    .inner_margin(egui::Margin::symmetric(8, 6))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(behind_text(v.behind))
                                    .strong()
                                    .color(theme::c().warn),
                            );
                            if ui
                                .button("Catch up")
                                .on_hover_text(
                                    "Read the first missed Reading today and move the rest on; \
                                     the plan ends later",
                                )
                                .clicked()
                            {
                                *run = Some(("reading_plan.catch_up", json!({ "plan": v.id })));
                            }
                            if ui
                                .button("Reschedule")
                                .on_hover_text(
                                    "Keep the end day and spread the Readings left over the \
                                     days left",
                                )
                                .clicked()
                            {
                                *run = Some(("reading_plan.reschedule", json!({ "plan": v.id })));
                            }
                        });
                    });
            }
            ui.add_space(6.0);
            if v.finished {
                ui.label(
                    RichText::new("Finished: every Reading is read.").color(theme::c().verdigris),
                );
            }
            heading(ui, "Today");
            if v.today.is_empty() {
                muted(ui, "Nothing to read today.");
            }
            for r in &v.today {
                reading_row(ui, v.id, r, false, split, run);
            }
            if !v.missed.is_empty() {
                ui.add_space(4.0);
                heading(ui, "Missed");
                for r in v.missed.iter().take(MISSED_SHOWN) {
                    reading_row(ui, v.id, r, true, split, run);
                }
                if v.missed.len() > MISSED_SHOWN {
                    muted(ui, &format!("and {} more", v.missed.len() - MISSED_SHOWN));
                }
            }
            if let Some((day, next)) = &v.next {
                ui.add_space(4.0);
                heading(ui, &format!("Next · {}", day.short_label()));
                for r in next {
                    reading_row(ui, v.id, r, false, split, run);
                }
            }
        });
}

/// A Reading's tick box and its Passages, each one click from opening.
fn reading_row(
    ui: &mut Ui,
    plan: i64,
    r: &Row,
    dated: bool,
    split: bool,
    run: &mut Option<(&'static str, Value)>,
) {
    ui.horizontal(|ui| {
        let mut read = r.read;
        let tick = ui.checkbox(&mut read, "").on_hover_text(if r.read {
            "Mark unread"
        } else {
            "Mark read"
        });
        if tick.changed() {
            *run = Some((
                "reading_plan.mark",
                json!({ "plan": plan, "position": r.position, "read": read }),
            ));
        }
        if dated {
            ui.label(
                RichText::new(r.day.short_label())
                    .size(12.0)
                    .color(theme::c().muted),
            );
        }
        for (i, p) in r.passages.iter().enumerate() {
            let mut text = RichText::new(p).size(14.0).color(theme::c().lapis);
            if r.read {
                text = text.color(theme::c().muted);
            }
            let b = ui.add(egui::Button::new(text).frame(false));
            if b.on_hover_text("Open in the plan's Bible (Ctrl/Cmd-click: in a new split)")
                .clicked()
            {
                *run = Some((
                    "reading_plan.open",
                    json!({ "plan": plan, "position": r.position, "passage": i, "split": split }),
                ));
            }
        }
    });
}

fn muted(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).color(theme::c().muted));
}

fn heading(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text.to_uppercase())
            .size(10.5)
            .strong()
            .color(theme::c().muted),
    );
}
