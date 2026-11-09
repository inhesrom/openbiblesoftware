//! The Timeline Tool (#185): Eras, Lifespans and Events on a zoomable horizontal axis.
//!
//! Everything shown is engine data ([`obs_engine::timeline`]); this file lays it out and turns
//! pointer input into a [`TimelineView`] change or a click. The axis is drawn with egui's
//! painter: year ticks on top, the Eras as a band of named periods (shading the axis beneath
//! them), then rows of Lifespans and of Events, each placed in the first row where its bar or
//! marker and its label fit. What the Passage emphasises is drawn in full colour, the focus
//! ringed in gilt; the rest is quieter. Drag pans, the wheel or a pinch zooms. A click on an
//! Event opens its Passage, on a Lifespan its person's Factbook, on an Era fits the view to it.
//! Each entry on the axis is a button of its own, so Tab reaches it and Enter opens it, and
//! `Earlier`, `Later`, Zoom out, Zoom in and `Fit` move the view without a pointer.

use std::sync::Arc;

use egui::{Color32, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, pos2, vec2};
use obs_engine::TabId;
use obs_engine::timeline::{Emphasis, Timeline, TimelineEntry, TimelineView};
use obs_model::EventKind;
use obs_scripture::{Passage, RefStyle, Standard, format_passage};
use serde_json::json;

use super::State;
use crate::theme;
use crate::tokens::{chart, radius, share, size, space, stroke, text};

/// A Timeline Tab's view: where it looks and what it was fitted to.
#[derive(Default)]
pub(super) struct TimelineUi {
    view: Option<TimelineView>,
    /// The Passage (or none: the whole Timeline) the view was last fitted to.
    fitted: Option<Option<Passage>>,
    /// The axis width when fitted; until the user moves the view, a new width fits it again.
    fitted_width: Option<f32>,
    moved: bool,
}

#[cfg(test)]
impl TimelineUi {
    /// Where the Tab looked last frame.
    pub(super) fn view(&self) -> Option<TimelineView> {
        self.view
    }
}

/// Years shown around a short focus Event: its reign and the decades either side.
const FOCUS_YEARS: f64 = 160.0;
/// Heights of the parts of the axis, in points.
const TICKS_H: f32 = chart::TICKS;
const ERA_H: f32 = chart::BAND;
const ROW_H: f32 = chart::ROW;
/// A point Event's diamond, half its width.
const DIAMOND: f32 = chart::DIAMOND;
/// Horizontal room left between neighbours in a row.
const GAP: f32 = space::M;
/// Most rows of Lifespans; the rest go to Events.
const LIFESPAN_ROWS: usize = 3;
/// How far `Earlier` and `Later` move the view: a third of the axis.
const PAN_SHARE: f64 = 1.0 / 3.0;
/// Room left at the axis's ends when it fits a span of years.
const FIT_PAD: f64 = space::XXL as f64;

/// `a` blended `t` of the way towards `b`.
fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| {
        // A blend of two bytes is a byte.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let v = (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
        v
    };
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// One entry laid out on the axis this frame.
struct Placed {
    index: usize,
    /// The bar (a span) or the diamond (a point), on screen.
    shape: Rect,
    /// Where its label goes, if there is room for one.
    label: Option<(Pos2, Arc<egui::Galley>, Color32)>,
    fill: Color32,
    strong: bool,
    focus: bool,
}

impl State {
    pub(super) fn timeline_body(&mut self, ui: &mut Ui, tab: TabId) {
        let shown = self
            .session
            .workspace
            .tab(tab)
            .and_then(|t| t.shown_passage());
        let timeline: Arc<Timeline> = match self.references.timeline(&self.session.library) {
            None => {
                crate::components::loading_state(ui, "Reading the Timeline…");
                return;
            }
            Some(Err(e)) => {
                if crate::components::error_state(ui, "The Timeline could not be read.", &e) {
                    self.retry_failed();
                }
                return;
            }
            Some(Ok(t)) => t,
        };
        if timeline.is_empty() {
            let empty = self.session.timeline_empty_state();
            if let Some((id, params)) = self.missing_resource_ui(ui, empty.message, &empty.missing)
            {
                self.run(id, &params);
            }
            return;
        }
        let Some(bounds) = timeline.bounds() else {
            return;
        };
        let emphasis = shown.map(|p| timeline.emphasis(p)).unwrap_or_default();
        let mut state = self
            .tabs
            .get_mut(&tab)
            .and_then(|t| t.timeline.take())
            .unwrap_or_default();
        let mut refit = state.fitted != Some(shown);
        let mut zoom: Option<f64> = None;
        // Fit was pressed: fit the whole Timeline once the axis's width is known.
        let mut fit_all = false;
        // A pan by this share of the axis's width (positive: earlier years).
        let mut pan: Option<f64> = None;
        let mut go: Option<(Passage, bool)> = None;
        let mut factbook: Option<(String, bool)> = None;
        let focus = emphasis.focus.map(|i| &timeline.entries[i]);

        // Controls and status.
        ui.horizontal(|ui| {
            if ui
                .small_button("Earlier")
                .on_hover_text("Show earlier years")
                .clicked()
            {
                pan = Some(PAN_SHARE);
            }
            if ui
                .small_button("Later")
                .on_hover_text("Show later years")
                .clicked()
            {
                pan = Some(-PAN_SHARE);
            }
            if crate::icons::small_button(ui, crate::icons::Icon::ZoomOut, "Zoom out").clicked() {
                zoom = Some(0.5);
            }
            if crate::icons::small_button(ui, crate::icons::Icon::ZoomIn, "Zoom in").clicked() {
                zoom = Some(2.0);
            }
            if ui
                .small_button("Fit")
                .on_hover_text("Show the whole Timeline")
                .clicked()
            {
                fit_all = true;
                refit = false;
            }
            if let Some(f) = focus
                && ui
                    .small_button("Passage")
                    .on_hover_text(format!("Show {} again", f.title))
                    .clicked()
            {
                refit = true;
            }
            let status = match (shown, focus) {
                (Some(p), Some(_)) => {
                    let n = timeline.events_in(&emphasis).len();
                    format!(
                        "{} · {}",
                        format_passage(emphasis.widened.unwrap_or(p), &Standard, RefStyle::Long),
                        if n == 1 {
                            "1 event".to_string()
                        } else {
                            format!("{n} events")
                        }
                    )
                }
                (Some(p), None) => format!(
                    "{} · no event on the Timeline",
                    format_passage(p, &Standard, RefStyle::Long)
                ),
                (None, _) => {
                    "Enter a Bible reference or join a Link set to bring its events out".to_string()
                }
            };
            ui.add(egui::Label::new(RichText::new(status).color(theme::c().muted)).truncate());
        });

        // The axis, and the list of the Passage's events under it.
        let area = ui.available_rect_before_wrap().intersect(ui.clip_rect());
        let listed = timeline.events_in(&emphasis);
        let caption_h = size::CONTROL_COMPACT;
        let list_h = if listed.is_empty() {
            0.0
        } else {
            (area.height() * share::LIST).clamp(size::ROW_DOUBLE, size::SIDE_LIST)
        };
        let axis_rect = Rect::from_min_max(
            area.min,
            pos2(
                area.max.x,
                (area.max.y - list_h - caption_h).max(area.min.y + chart::AXIS_MIN),
            ),
        );
        let width = f64::from(axis_rect.width());
        let resized = state.fitted_width != Some(axis_rect.width());
        if refit || state.view.is_none() || (resized && !state.moved) {
            state.view = Some(match focus {
                Some(f) => TimelineView::around(f, FOCUS_YEARS, width),
                None => TimelineView::fit(bounds.0, bounds.1, width, FIT_PAD),
            });
            state.fitted = Some(shown);
            state.fitted_width = Some(axis_rect.width());
            state.moved = false;
        }
        if fit_all {
            state.view = Some(TimelineView::fit(bounds.0, bounds.1, width, FIT_PAD));
            state.moved = true;
        }
        let mut view = state
            .view
            .unwrap_or_else(|| TimelineView::fit(bounds.0, bounds.1, width, FIT_PAD));
        if let Some(factor) = zoom {
            state.moved = true;
            view.zoom_about(factor, width / 2.0, width, bounds);
        }
        if let Some(share) = pan {
            state.moved = true;
            view.pan(share * width, bounds);
        }
        let clicked = axis(ui, axis_rect, &mut view, &timeline, &emphasis, bounds);
        state.moved |= clicked.moved;
        if let Some((i, new_tab)) = clicked.entry {
            let e = &timeline.entries[i];
            match e.kind {
                EventKind::Event => go = e.passages.first().map(|p| (*p, new_tab)),
                EventKind::Lifespan => match e.person().and_then(|p| p.entity.clone()) {
                    Some(id) => factbook = Some((id, new_tab)),
                    None => self.toast(format!(
                        "{} is in no People & places index in your library",
                        e.title
                    )),
                },
                EventKind::Era => {
                    view = TimelineView::fit(e.from, e.to, width, FIT_PAD);
                    state.moved = true;
                }
            }
        }
        state.view = Some(view);

        // The Passage's events, under the axis.
        let below = Rect::from_min_max(pos2(area.min.x, axis_rect.max.y + space::XS), area.max);
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(below)
                .id_salt(("timeline-list", tab)),
        );
        if !listed.is_empty() {
            let heading = match (shown, emphasis.widened) {
                (Some(p), Some(chapter)) => format!(
                    "In {} (no event tells {})",
                    format_passage(chapter, &Standard, RefStyle::Long),
                    format_passage(p, &Standard, RefStyle::Short)
                ),
                (Some(p), None) => format!("In {}", format_passage(p, &Standard, RefStyle::Long)),
                (None, _) => String::new(),
            };
            child.label(RichText::new(heading).strong());
            let new_tab = child.input(|i| i.modifiers.command || i.modifiers.ctrl);
            egui::ScrollArea::vertical()
                .id_salt(("timeline-events", tab, shown))
                .max_height(list_h - size::CONTROL_COMPACT)
                .auto_shrink([false, true])
                .show(&mut child, |ui| {
                    for e in &listed {
                        let is_focus =
                            focus.is_some_and(|f| f.id == e.id && f.index_id == e.index_id);
                        if let Some(action) = event_row(ui, e, is_focus) {
                            match action {
                                RowAction::Show => {
                                    view = TimelineView::around(e, FOCUS_YEARS, width);
                                    state.view = Some(view);
                                    state.moved = true;
                                }
                                RowAction::Passage(p) => go = Some((p, new_tab)),
                                RowAction::Factbook(id) => factbook = Some((id, new_tab)),
                            }
                        }
                    }
                });
        }
        child.label(crate::components::hint_text(format!(
            "Dates as {} gives them; “c.” marks a date the source gives as approximate.",
            timeline.sources.join(", ")
        )));

        if let Some(t) = self.tabs.get_mut(&tab) {
            t.timeline = Some(state);
        }
        if let Some((passage, new_tab)) = go {
            self.reference_navigate(tab, passage, new_tab);
        }
        if let Some((entity, new_tab)) = factbook {
            self.run(
                "guide.factbook",
                &json!({ "entity": entity, "new_tab": new_tab }),
            );
        }
    }
}

enum RowAction {
    Show,
    Passage(Passage),
    Factbook(String),
}

/// One of the Passage's events: its title (shows it on the axis), dates, Passages (open them)
/// and the people it names (open their Factbooks).
fn event_row(ui: &mut Ui, e: &TimelineEntry, focus: bool) -> Option<RowAction> {
    let mut action = None;
    ui.horizontal_wrapped(|ui| {
        // Whole widgets move to the next row; none is split across two (a split link is two
        // widgets, and screen readers get no name for the second, #182).
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        let mut title = RichText::new(&e.title).strong();
        if focus {
            title = title.color(theme::c().lapis);
        }
        if ui
            .selectable_label(focus, title)
            .on_hover_text("Show it on the Timeline")
            .clicked()
        {
            action = Some(RowAction::Show);
        }
        ui.label(RichText::new(&e.dates).color(theme::c().muted));
        for p in e.passages.iter().take(4) {
            let label = format_passage(*p, &Standard, RefStyle::Short);
            if crate::components::link(ui, label)
                .on_hover_text("Open this Passage")
                .clicked()
            {
                action = Some(RowAction::Passage(*p));
            }
        }
        for person in e.people.iter().filter(|p| p.entity.is_some()).take(6) {
            if crate::icons::onward(ui, &person.name)
                .on_hover_text(format!("Open {}'s Factbook", person.name))
                .clicked()
            {
                action = person.entity.clone().map(RowAction::Factbook);
            }
        }
    });
    action
}

/// What happened on the axis this frame.
#[derive(Default)]
struct AxisInput {
    /// An entry clicked, and whether Ctrl/Cmd was held.
    entry: Option<(usize, bool)>,
    moved: bool,
}

/// Draw the axis in `rect` and handle panning, zooming, hovering and clicks.
fn axis(
    ui: &mut Ui,
    rect: Rect,
    view: &mut TimelineView,
    timeline: &Timeline,
    emphasis: &Emphasis,
    bounds: (f64, f64),
) -> AxisInput {
    // Drag only: the entries on it take clicks and the keyboard, `Earlier` and `Later` pan.
    let response = ui.allocate_rect(rect, Sense::DRAG);
    let width = f64::from(rect.width());
    let before = *view;
    if response.dragged() {
        view.pan(f64::from(response.drag_delta().x), bounds);
    }
    // Over the axis or an entry on it (the entries are widgets on top of the axis).
    let pointer = (ui.ctx().pointer_hover_pos()).filter(|_| ui.rect_contains_pointer(rect));
    if let Some(p) = pointer {
        let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
        let factor = f64::from(pinch) * (f64::from(scroll.y) / 300.0).exp();
        if (factor - 1.0).abs() > 1e-6 {
            view.zoom_about(factor, f64::from(p.x - rect.min.x), width, bounds);
        }
        if scroll.x.abs() > 0.0 {
            view.pan(f64::from(scroll.x), bounds);
        }
    }
    let v = *view;
    let (from, to) = v.visible(width);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Image,
            true,
            format!(
                "Timeline from {} to {}",
                obs_engine::timeline::year_label(from.round() as i32),
                obs_engine::timeline::year_label(to.round() as i32)
            ),
        )
    });
    let x = |year: f64| -> f32 {
        // Screen points fit in f32.
        #[allow(clippy::cast_possible_truncation)]
        let p = rect.min.x + v.x(year, width) as f32;
        p
    };
    let ch = theme::c();
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, ch.field);
    let font = FontId::proportional(text::SMALL);

    // Eras: shading down the axis, then the band of names.
    let era_top = rect.min.y + TICKS_H;
    let era_band = Rect::from_min_max(pos2(rect.min.x, era_top), pos2(rect.max.x, era_top + ERA_H));
    let mut hits: Vec<(Rect, usize)> = Vec::new();
    let eras: Vec<(usize, usize, f32, f32)> = timeline
        .of_kind(EventKind::Era)
        .enumerate()
        .map(|(n, (i, era))| (n, i, x(era.from), x(era.to)))
        .filter(|&(_, _, x0, x1)| x1 >= rect.min.x && x0 <= rect.max.x)
        .collect();
    let era_tint = |n: usize| {
        if n.is_multiple_of(2) {
            ch.lapis
        } else {
            ch.verdigris
        }
    };
    for &(n, i, x0, x1) in &eras {
        let strong = emphasis.entries.contains(&i);
        let shade = mix(
            ch.field,
            era_tint(n),
            if strong {
                chart::SHADE_STRONG
            } else {
                chart::SHADE
            },
        );
        painter.rect_filled(
            Rect::from_min_max(pos2(x0, era_band.max.y), pos2(x1, rect.max.y)),
            0.0,
            shade,
        );
    }

    // Ticks, under the Eras' names.
    for (year, label) in v.ticks(width, f64::from(chart::TICK_SPACING)) {
        let tx = x(year);
        painter.line_segment(
            [
                pos2(tx, rect.min.y + TICKS_H - chart::TICK),
                pos2(tx, rect.max.y),
            ],
            Stroke::new(stroke::HAIR, mix(ch.field, ch.rule, chart::GRID)),
        );
        let galley = painter.layout_no_wrap(label, FontId::proportional(text::CAPTION), ch.muted);
        let at = pos2(tx - galley.size().x / 2.0, rect.min.y + space::XXS);
        if at.x >= rect.min.x && at.x + galley.size().x <= rect.max.x {
            painter.galley(at, galley, ch.muted);
        }
    }

    for &(n, i, x0, x1) in &eras {
        let era = &timeline.entries[i];
        let strong = emphasis.entries.contains(&i);
        let fill = mix(
            ch.field,
            era_tint(n),
            if strong {
                chart::TINT_STRONG
            } else {
                chart::TINT
            },
        );
        let band = Rect::from_min_max(pos2(x0, era_band.min.y), pos2(x1, era_band.max.y));
        let visible = band.intersect(rect);
        painter.rect_filled(visible, 0.0, fill);
        hits.push((visible, i));
        let galley = painter.layout_no_wrap(era.title.clone(), font.clone(), ch.ink);
        if galley.size().x + space::M <= visible.width() {
            let at = pos2(
                visible.center().x - galley.size().x / 2.0,
                band.center().y - galley.size().y / 2.0,
            );
            painter.galley(at, galley, ch.ink);
        }
    }

    // Rows of Lifespans, then of Events.
    let rows_top = era_band.max.y + space::S;
    // The axis is a few hundred points tall: a few dozen rows.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let all_rows = (((rect.max.y - rows_top) / ROW_H).floor() as usize).max(2);
    let (lifespans, life_rows) = place_rows(
        &painter,
        timeline,
        emphasis,
        EventKind::Lifespan,
        rows_top,
        LIFESPAN_ROWS.min(all_rows / 3).max(1),
        rect,
        &x,
    );
    let events_top = if life_rows == 0 {
        rows_top
    } else {
        rows_top + life_rows as f32 * ROW_H + space::XS
    };
    let (events, _) = place_rows(
        &painter,
        timeline,
        emphasis,
        EventKind::Event,
        events_top,
        all_rows.saturating_sub(life_rows).max(1),
        rect,
        &x,
    );
    for p in lifespans.iter().chain(&events) {
        let e = &timeline.entries[p.index];
        if e.is_span() {
            painter.rect_filled(p.shape, radius::S, p.fill);
        } else {
            let c = p.shape.center();
            painter.add(Shape::convex_polygon(
                vec![
                    c + vec2(0.0, -DIAMOND),
                    c + vec2(DIAMOND, 0.0),
                    c + vec2(0.0, DIAMOND),
                    c + vec2(-DIAMOND, 0.0),
                ],
                p.fill,
                Stroke::NONE,
            ));
        }
        if p.focus {
            painter.rect_stroke(
                p.shape.expand(space::XXS),
                radius::S,
                Stroke::new(stroke::THICK, ch.gilt),
                egui::StrokeKind::Outside,
            );
        }
        if let Some((at, galley, color)) = &p.label {
            painter.galley(*at, galley.clone(), *color);
        }
        let mut hit = p.shape.expand2(vec2(0.0, space::XXS));
        if let Some((at, galley, _)) = &p.label {
            hit = hit.union(Rect::from_min_size(*at, galley.size()));
        }
        hits.push((hit.intersect(rect), p.index));
    }

    // Each entry is a button on top of the axis, in drawing order, so the last drawn under the
    // pointer takes the click; Tab reaches each one and Enter or Space opens it, and a screen
    // reader names it (#182).
    let mut input = AxisInput {
        moved: *view != before,
        ..AxisInput::default()
    };
    for (hit, i) in hits {
        if !hit.is_positive() {
            continue;
        }
        let e = &timeline.entries[i];
        // The same id every frame, so the keyboard stays on an entry as the view moves.
        let id = ui.make_persistent_id(("timeline-entry", i));
        let entry = ui.interact(hit, id, Sense::click());
        entry.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, entry_name(e)));
        if entry.clicked() {
            let new_tab = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
            input.entry = Some((i, new_tab));
        }
        entry.on_hover_text_at_pointer(entry_tooltip(e));
    }
    input
}

/// An entry's accessible name: what it is, its title and its dates.
fn entry_name(e: &TimelineEntry) -> String {
    let kind = match e.kind {
        EventKind::Era => "Era",
        EventKind::Lifespan => "Lifespan",
        EventKind::Event => "Event",
    };
    format!("{kind}: {}, {}", e.title, e.dates)
}

/// What hovering an entry says: title, dates, people (of an Event), Passages and what a click
/// does.
fn entry_tooltip(e: &TimelineEntry) -> String {
    let mut text = format!("{}\n{}", e.title, e.dates);
    let people: Vec<&str> = e.people.iter().map(|p| p.name.as_str()).collect();
    if !people.is_empty() && e.kind == EventKind::Event {
        text.push_str(&format!("\n{}", people.join(", ")));
    }
    if !e.passages.is_empty() {
        let refs: Vec<String> = e
            .passages
            .iter()
            .take(3)
            .map(|p| format_passage(*p, &Standard, RefStyle::Short))
            .collect();
        text.push_str(&format!("\n{}", refs.join("; ")));
    }
    text.push_str(match e.kind {
        EventKind::Event => "\nClick to open its Passage",
        EventKind::Lifespan => "\nClick for their Factbook",
        EventKind::Era => "\nClick to show the whole Era",
    });
    text
}

/// `text` on one line, cut short with "…" to fit `width`.
fn elided(
    painter: &egui::Painter,
    text: &str,
    font: &FontId,
    color: Color32,
    width: f32,
) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_string(),
        egui::TextFormat::simple(font.clone(), color),
    );
    job.wrap = egui::text::TextWrapping {
        max_width: width,
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    painter.layout_job(job)
}

/// Lay out the entries of `kind` in view into at most `rows` rows from `top`: emphasised ones
/// first, so they get their labels, each in the first row where it (and its label) fits.
/// Returns them and how many rows they took.
#[allow(clippy::too_many_arguments)]
fn place_rows(
    painter: &egui::Painter,
    timeline: &Timeline,
    emphasis: &Emphasis,
    kind: EventKind,
    top: f32,
    rows: usize,
    rect: Rect,
    x: &dyn Fn(f64) -> f32,
) -> (Vec<Placed>, usize) {
    let ch = theme::c();
    let font = FontId::proportional(text::SMALL);
    let (bar, strong_bar) = match kind {
        EventKind::Lifespan => (
            mix(ch.field, ch.verdigris, chart::TINT_STRONG),
            ch.verdigris,
        ),
        _ => (mix(ch.field, ch.lapis, chart::TINT_STRONG), ch.lapis),
    };
    let point = (mix(ch.field, ch.muted, chart::POINT), ch.lapis);
    let mut items: Vec<(usize, &TimelineEntry, bool)> = timeline
        .of_kind(kind)
        .filter(|(_, e)| x(e.to) >= rect.min.x - space::XS && x(e.from) <= rect.max.x + space::XS)
        .map(|(i, e)| (i, e, emphasis.entries.contains(&i)))
        .collect();
    items.sort_by(|a, b| b.2.cmp(&a.2).then(a.1.from.total_cmp(&b.1.from)));
    let mut taken: Vec<Vec<(f32, f32)>> = vec![Vec::new(); rows.max(1)];
    let mut out = Vec::new();
    for (index, e, strong) in items {
        let focus = emphasis.focus == Some(index);
        let (x0, x1) = (x(e.from), x(e.to));
        let span = e.is_span() && x1 - x0 >= chart::BAR_INSET;
        let (start, end) = if span {
            (x0, x1)
        } else {
            (x0 - DIAMOND, x0 + DIAMOND)
        };
        let fill = match (span, strong) {
            (true, true) => strong_bar,
            (true, false) => bar,
            (false, true) => point.1,
            (false, false) => point.0,
        };
        // The label inside a bar wide enough for it (on the bar's colour), else after it, or
        // before it when there is no room after it on the axis.
        let text_color = if strong { ch.ink } else { ch.muted };
        let mut galley = painter.layout_no_wrap(e.title.clone(), font.clone(), text_color);
        let visible_start = start.max(rect.min.x);
        let inside = span && end.min(rect.max.x) - visible_start >= galley.size().x + space::M;
        let after = end.max(visible_start) + space::XS;
        let (room_after, room_before) = (rect.max.x - after, start - space::XS - rect.min.x);
        // With no room either side, the side with more, the label cut short to fit it.
        let before = !inside && galley.size().x > room_after && room_before > room_after;
        let room = if before { room_before } else { room_after };
        if !inside && galley.size().x > room && room > space::XXL {
            galley = elided(painter, &e.title, &font, text_color, room);
        }
        let w = galley.size().x;
        let label_x = if inside {
            visible_start + space::XS
        } else if before {
            start - space::XS - w
        } else {
            after
        };
        let wanted = if before {
            (label_x - GAP, end + GAP)
        } else if inside {
            (start, end + GAP)
        } else {
            (start, after + w + GAP)
        };
        let row = taken
            .iter()
            .position(|r| r.iter().all(|&(a, b)| wanted.1 <= a || wanted.0 >= b));
        let (row, labelled) = match row {
            Some(r) => (r, true),
            // No room: in the last row, unlabelled.
            None => (taken.len() - 1, false),
        };
        let occupied = if labelled {
            wanted
        } else {
            (start, end + space::XXS)
        };
        taken[row].push(occupied);
        let y = top + row as f32 * ROW_H;
        let shape = if span {
            Rect::from_min_max(
                pos2(x0, y + chart::BAR_INSET),
                pos2(x1.max(x0 + chart::BAR_INSET), y + ROW_H - chart::BAR_INSET),
            )
        } else {
            Rect::from_center_size(
                pos2(x0, y + ROW_H / 2.0),
                vec2(DIAMOND * 2.0, DIAMOND * 2.0),
            )
        };
        let label = labelled.then(|| {
            let ty = y + (ROW_H - galley.size().y) / 2.0;
            if inside {
                let on = theme::text_on(fill);
                let g = painter.layout_no_wrap(e.title.clone(), font.clone(), on);
                (pos2(label_x, ty), g, on)
            } else {
                (pos2(label_x, ty), galley, text_color)
            }
        });
        out.push(Placed {
            index,
            shape,
            label,
            fill,
            strong,
            focus,
        });
    }
    // Quieter entries first, so emphasised ones are drawn over them.
    out.sort_by_key(|p| (p.strong, p.focus));
    let used = taken
        .iter()
        .rposition(|r| !r.is_empty())
        .map_or(0, |r| r + 1);
    (out, used)
}
