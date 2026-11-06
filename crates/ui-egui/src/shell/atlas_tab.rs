//! The Atlas Tool (#150) and the map it draws, shared with the Factbook's Map section.
//!
//! The map is the built-in base map ([`obs_engine::base_map`]): land and lakes as triangle
//! meshes, coasts, shores and rivers as lines, drawn with egui's painter from vertex data, so
//! it needs no tiles and no network. Places are markers whose look says how confident their
//! location is (filled: high, half-filled: medium, hollow: low), each labelled where its label
//! does not cover another. Everything shown is engine data ([`obs_engine::atlas`]); this file
//! only draws it and turns pointer input into a [`MapView`] change or a click.

use std::sync::Arc;

use egui::{Color32, FontId, Mesh, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, pos2, vec2};
use obs_engine::atlas::{self, MapPlace};
use obs_engine::base_map::{self, MapLine, MapMesh};
use obs_engine::{Confidence, GeoBounds, MapView, TabId, WordStatus};
use obs_scripture::{Passage, RefStyle, Standard, Verse, format_passage};
use serde_json::json;

use super::State;
use crate::theme;

/// An Atlas Tab's map: where it looks, the Passage it was fitted to, the search box and the
/// place picked in the list or the search.
#[derive(Default)]
pub(super) struct MapUi {
    view: Option<MapView>,
    fitted: Option<Passage>,
    /// The map's size when it was fitted; until the user moves the map, a new size (the Panel
    /// resized, or laid out on its first frames) fits it again.
    fitted_size: Option<egui::Vec2>,
    /// The user panned or zoomed since the map was fitted.
    moved: bool,
    query: String,
    /// The place picked (its id), and the place itself when the Passage does not name it
    /// (a search result).
    selected: Option<String>,
    found: Option<MapPlace>,
}

/// Width of the place list beside the map, when the Tab is wide enough for it.
const LIST_WIDTH: f32 = 230.0;

/// Smallest box the Atlas opens on, in degrees of latitude: one place is not shown at full
/// zoom.
const MIN_SPAN: f64 = 3.0;

/// Smallest box of the Factbook's map: enough land around a place to see where it is.
pub(super) const MINI_MAP_SPAN: f64 = 7.0;

/// How near (screen points) the pointer must be to a marker to hover or click it.
const HIT_RADIUS: f32 = 10.0;

/// Height of the Factbook's map.
pub(super) const MINI_MAP_HEIGHT: f32 = 240.0;

/// The map's colours in the current theme.
struct MapColors {
    water: Color32,
    land: Color32,
    coast: Color32,
    river: Color32,
    halo: Color32,
}

fn colors() -> MapColors {
    if theme::c().dark {
        MapColors {
            water: Color32::from_rgb(27, 38, 50),
            land: Color32::from_rgb(55, 58, 52),
            coast: Color32::from_rgb(98, 108, 108),
            river: Color32::from_rgb(68, 106, 138),
            halo: Color32::from_rgba_unmultiplied(40, 45, 50, 210),
        }
    } else {
        MapColors {
            water: Color32::from_rgb(203, 220, 230),
            land: Color32::from_rgb(240, 235, 221),
            coast: Color32::from_rgb(140, 150, 150),
            river: Color32::from_rgb(120, 160, 192),
            halo: Color32::from_rgba_unmultiplied(255, 255, 253, 210),
        }
    }
}

/// One marker to draw: a place at one of its locations.
struct Marker<'a> {
    place: &'a MapPlace,
    /// Which of the place's points.
    point: usize,
    at: Pos2,
    /// The label: the place's name, or for a place's other locations the location's.
    label: String,
    /// Drawn larger and labelled first (the current Verse names it, or it is picked).
    strong: bool,
}

/// What happened on a map this frame.
#[derive(Default)]
pub(super) struct MapInput {
    /// A place clicked (its id), and whether Ctrl/Cmd was held.
    pub clicked: Option<(String, bool)>,
    /// The user panned or zoomed.
    pub moved: bool,
}

/// What a map shows and how it may be used.
pub(super) struct MapScene<'a> {
    pub places: &'a [&'a MapPlace],
    /// The place picked (ringed, with all its locations).
    pub selected: Option<&'a str>,
    /// Places drawn larger (the current Verse names them).
    pub emphasized: &'a dyn Fn(&MapPlace) -> bool,
    /// Every location of each place (the Factbook's map), not only its most confident one.
    pub all_points: bool,
    /// Drag pans, the wheel or a pinch zooms, double-click zooms in.
    pub interactive: bool,
    /// What a screen reader hears.
    pub description: &'a str,
}

/// Draw the base map and the scene's places in `rect`, moving `view` as the user pans and
/// zooms an interactive map.
pub(super) fn map_canvas(
    ui: &mut Ui,
    rect: Rect,
    view: &mut MapView,
    scene: &MapScene<'_>,
) -> MapInput {
    let MapScene {
        places,
        selected,
        emphasized,
        all_points,
        interactive,
        description,
    } = *scene;
    let sense = if interactive {
        Sense::click_and_drag()
    } else {
        Sense::click()
    };
    let response = ui.allocate_rect(rect, sense);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, description));
    let world = base_map::builtin().bounds;
    let size = (f64::from(rect.width()), f64::from(rect.height()));
    let before = *view;
    if interactive {
        if response.dragged() {
            let d = response.drag_delta();
            view.pan(f64::from(d.x), f64::from(d.y), world);
        }
        if response.hovered() {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = f64::from(pinch) * (f64::from(scroll) / 300.0).exp();
            if (factor - 1.0).abs() > 1e-6
                && let Some(p) = response.hover_pos()
            {
                let local = p - rect.min;
                view.zoom_about(factor, f64::from(local.x), f64::from(local.y), size, world);
            }
        }
        if response.double_clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            let local = p - rect.min;
            view.zoom_about(2.0, f64::from(local.x), f64::from(local.y), size, world);
        }
    }
    let v = *view;
    let moved = v != before;
    let to_screen = |lon: f64, lat: f64| -> Pos2 {
        let (x, y) = v.to_screen(lon, lat, size);
        // Screen points fit in f32.
        #[allow(clippy::cast_possible_truncation)]
        let p = pos2(rect.min.x + x as f32, rect.min.y + y as f32);
        p
    };
    let painter = ui.painter_at(rect);
    let c = colors();
    painter.rect_filled(rect, 0.0, c.water);
    let map = base_map::builtin();
    painter.add(Shape::mesh(mesh(&map.land, c.land, &to_screen)));
    painter.add(Shape::mesh(mesh(&map.lakes, c.water, &to_screen)));
    let visible = v.visible(size);
    let max_rank = if v.scale < 12.0 {
        4
    } else if v.scale < 40.0 {
        6
    } else {
        u8::MAX
    };
    lines(
        &painter,
        &map.coasts,
        Stroke::new(0.8, c.coast),
        visible,
        u8::MAX,
        &to_screen,
    );
    lines(
        &painter,
        &map.shores,
        Stroke::new(0.7, c.river),
        visible,
        u8::MAX,
        &to_screen,
    );
    lines(
        &painter,
        &map.rivers,
        Stroke::new(1.0, c.river),
        visible,
        max_rank,
        &to_screen,
    );

    // Markers: the strong ones last, so they are drawn on top and labelled first.
    let mut markers: Vec<Marker<'_>> = Vec::new();
    let drawn: Vec<&MapPlace> = if all_points {
        places.to_vec()
    } else {
        let owned: Vec<MapPlace> = places.iter().map(|p| (*p).clone()).collect();
        let firsts: Vec<String> = atlas::first_at_each_point(&owned)
            .into_iter()
            .map(|p| p.entity.clone())
            .collect();
        places
            .iter()
            .copied()
            .filter(|p| firsts.contains(&p.entity) || Some(p.entity.as_str()) == selected)
            .collect()
    };
    for place in drawn {
        let picked = Some(place.entity.as_str()) == selected;
        let every = all_points || picked;
        for (i, point) in place.points.iter().enumerate() {
            if i > 0 && !every {
                break;
            }
            let label = if all_points && place.points.len() > 1 || (picked && i > 0) {
                point.location.clone()
            } else {
                place.name.clone()
            };
            markers.push(Marker {
                place,
                point: i,
                at: to_screen(point.lon(), point.lat()),
                label,
                strong: picked || emphasized(place),
            });
        }
    }
    // The picked place last of all: on top of any place at the same spot.
    markers.sort_by_key(|m| {
        (
            Some(m.place.entity.as_str()) == selected,
            m.strong,
            m.point == 0,
        )
    });
    let ink = theme::c().ink;
    for m in &markers {
        let confidence = m.place.points[m.point].confidence;
        marker(
            &painter,
            m.at,
            confidence,
            m.strong,
            Some(m.place.entity.as_str()) == selected,
        );
    }
    // Labels: strongest first, each only where it covers no label already placed.
    let font = FontId::proportional(12.0);
    let mut placed: Vec<Rect> = Vec::new();
    for m in markers.iter().rev() {
        if !rect.contains(m.at) {
            continue;
        }
        let galley = painter.layout_no_wrap(m.label.clone(), font.clone(), ink);
        let size = galley.size();
        // Right of the marker, else left, above or below: inside the map, clear of others.
        let candidates = [
            vec2(8.0, -size.y / 2.0),
            vec2(-8.0 - size.x, -size.y / 2.0),
            vec2(-size.x / 2.0, -8.0 - size.y),
            vec2(-size.x / 2.0, 8.0),
        ];
        let Some(at) = candidates
            .into_iter()
            .map(|offset| Rect::from_min_size(m.at + offset, size))
            .find(|r| {
                rect.contains_rect(*r) && !placed.iter().any(|p| p.expand(1.0).intersects(*r))
            })
        else {
            continue;
        };
        painter.rect_filled(at.expand(1.5), 3.0, c.halo);
        painter.galley(at.min, galley, ink);
        placed.push(at);
    }

    // Hover and click: the nearest marker within reach; of markers at the same spot, the one
    // drawn on top (`min_by` keeps the first of equals, so walk the paint order backwards).
    let pointer = response
        .hover_pos()
        .or_else(|| response.interact_pointer_pos());
    let near = pointer.and_then(|p| {
        markers
            .iter()
            .rev()
            .map(|m| (m, m.at.distance(p)))
            .filter(|(_, d)| *d <= HIT_RADIUS)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(m, _)| m)
    });
    let mut input = MapInput {
        moved,
        ..MapInput::default()
    };
    if let Some(m) = near {
        let point = &m.place.points[m.point];
        let refs = verse_list(&m.place.verses);
        let mut text = format!(
            "{}\n{} · {}",
            m.place.name,
            point.location,
            point.confidence.label()
        );
        if m.place.points.len() > 1 {
            text.push_str(&format!(
                " ({} of {} possible locations)",
                m.point + 1,
                m.place.points.len()
            ));
        }
        if !refs.is_empty() {
            text.push_str(&format!("\n{refs}"));
        }
        text.push_str("\nClick for its Factbook");
        if response.clicked() {
            let new_tab = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
            input.clicked = Some((m.place.entity.clone(), new_tab));
        }
        response.on_hover_text_at_pointer(text);
    }
    input
}

fn mesh(source: &MapMesh, color: Color32, to_screen: &dyn Fn(f64, f64) -> Pos2) -> Mesh {
    let mut mesh = Mesh::default();
    mesh.vertices.reserve(source.points.len());
    for p in &source.points {
        mesh.colored_vertex(to_screen(f64::from(p[0]), f64::from(p[1])), color);
    }
    mesh.indices.clone_from(&source.triangles);
    mesh
}

fn lines(
    painter: &egui::Painter,
    lines: &[MapLine],
    stroke: Stroke,
    visible: GeoBounds,
    max_rank: u8,
    to_screen: &dyn Fn(f64, f64) -> Pos2,
) {
    for line in lines.iter().filter(|l| l.rank <= max_rank) {
        let touches = line
            .points
            .iter()
            .any(|p| visible.contains(f64::from(p[0]), f64::from(p[1])));
        if !touches && line.points.len() < 64 {
            continue;
        }
        let points: Vec<Pos2> = line
            .points
            .iter()
            .map(|p| to_screen(f64::from(p[0]), f64::from(p[1])))
            .collect();
        painter.add(Shape::line(points, stroke));
    }
}

/// A place marker: filled for high confidence, half-filled for medium, hollow for low; ringed
/// in gilt when picked.
fn marker(painter: &egui::Painter, at: Pos2, confidence: Confidence, strong: bool, picked: bool) {
    let ch = theme::c();
    let r = if strong { 6.5 } else { 5.0 };
    let ink = ch.lapis;
    match confidence {
        Confidence::High => {
            painter.circle(at, r, ink, Stroke::new(1.5, ch.field));
        }
        Confidence::Medium => {
            // Lower half filled.
            painter.circle_filled(at, r, ch.field);
            let half: Vec<Pos2> = (0..=16u8)
                .map(|i| {
                    let a = std::f32::consts::PI * f32::from(i) / 16.0;
                    at + vec2(a.cos(), a.sin()) * r
                })
                .collect();
            painter.add(Shape::convex_polygon(half, ink, Stroke::NONE));
            painter.circle_stroke(at, r, Stroke::new(1.5, ink));
        }
        Confidence::Low => {
            painter.circle(at, r, ch.field.gamma_multiply(0.6), Stroke::new(1.5, ink));
        }
    }
    if picked {
        painter.circle_stroke(at, r + 3.5, Stroke::new(2.0, ch.gilt));
    }
}

/// `Acts 13:1, 4` style list of the Verses that name a place.
fn verse_list(verses: &[Verse]) -> String {
    let list: obs_scripture::PassageList = verses.iter().map(|v| Passage::verse(*v)).collect();
    let merged = list.merged(&Standard);
    merged
        .passages()
        .iter()
        .map(|p| format_passage(*p, &Standard, RefStyle::Short))
        .collect::<Vec<_>>()
        .join("; ")
}

/// The view that shows `places` in a `rect`, at least `min_span` degrees of latitude tall.
pub(super) fn fit(places: &[&MapPlace], rect: Rect, all_points: bool, min_span: f64) -> MapView {
    let points = places.iter().flat_map(|p| {
        let n = if all_points { p.points.len() } else { 1 };
        p.points.iter().take(n).map(|b| (b.lon(), b.lat()))
    });
    let bounds = GeoBounds::around(points)
        .map(|b| b.at_least(min_span))
        .unwrap_or_else(default_bounds);
    MapView::fit(
        bounds,
        (f64::from(rect.width()), f64::from(rect.height())),
        28.0,
    )
}

/// The Bible lands around Jerusalem: where a map with nothing on it opens.
fn default_bounds() -> GeoBounds {
    GeoBounds {
        west: 28.0,
        south: 27.0,
        east: 46.0,
        north: 38.0,
    }
}

impl State {
    pub(super) fn atlas_body(&mut self, ui: &mut Ui, tab: TabId) {
        let Some(shown) = self
            .session
            .workspace
            .tab(tab)
            .and_then(|t| t.shown_passage())
        else {
            ui.label("Enter a Bible reference (Acts 13) or join a Link set");
            return;
        };
        let passage = atlas::atlas_passage(shown);
        // The Verse the Passage is at, when the Tab follows one: its places stand out.
        let current = (shown.start() == shown.end()).then_some(shown.start());
        let result = self.references.places(
            &self.session.library,
            self.session.guides.directory(),
            passage,
        );
        let places: Arc<Vec<MapPlace>> = match result {
            None => {
                ui.label(format!(
                    "Finding the places of {}…",
                    format_passage(passage, &Standard, RefStyle::Short)
                ));
                ui.ctx().request_repaint();
                return;
            }
            Some(Err(e)) => {
                ui.label(format!("Could not read the places: {e}"));
                return;
            }
            Some(Ok(p)) => p,
        };
        let mut state = self
            .tabs
            .get_mut(&tab)
            .and_then(|t| t.map.take())
            .unwrap_or_default();
        let mut open: Option<(String, bool)> = None;
        let mut refit = state.fitted != Some(passage);
        // The search row.
        let mut zoom: Option<f64> = None;
        ui.horizontal(|ui| {
            let edit = ui.add(
                egui::TextEdit::singleline(&mut state.query)
                    .hint_text("Find a place…")
                    .desired_width(180.0),
            );
            edit.widget_info(|| {
                egui::WidgetInfo::text_edit(true, "", &state.query, "Find a place on the map")
            });
            if ui.small_button("−").on_hover_text("Zoom out").clicked() {
                zoom = Some(0.5);
            }
            if ui.small_button("+").on_hover_text("Zoom in").clicked() {
                zoom = Some(2.0);
            }
            if ui
                .small_button("Fit")
                .on_hover_text("Show every place of the Passage")
                .clicked()
            {
                refit = true;
                state.selected = None;
                state.found = None;
            }
            let located = places.iter().filter(|p| !p.points.is_empty()).count();
            let mut status = format!(
                "{} · {}",
                format_passage(passage, &Standard, RefStyle::Short),
                match located {
                    1 => "1 place".to_string(),
                    n => format!("{n} places"),
                }
            );
            let unlocated = places.len() - located;
            if unlocated > 0 {
                status.push_str(&format!(", {unlocated} without a location"));
            }
            ui.add(egui::Label::new(RichText::new(status).color(theme::c().muted)).truncate());
        });
        // Search results, under the box.
        let query = state.query.trim().to_string();
        if !query.is_empty() {
            match self.session.atlas_search(&query) {
                WordStatus::Loading => {
                    ui.label(RichText::new("Looking up places…").color(theme::c().muted));
                    ui.ctx().request_repaint();
                }
                WordStatus::Failed(e) => {
                    ui.colored_label(theme::c().warn, e);
                }
                WordStatus::Ready(found) if found.is_empty() => {
                    ui.label(
                        RichText::new(format!("No place called “{query}” is on the map"))
                            .color(theme::c().muted),
                    );
                }
                WordStatus::Ready(found) => {
                    ui.horizontal_wrapped(|ui| {
                        for place in found.iter().take(8) {
                            let label = match place.best() {
                                Some(b) if b.location != place.name => {
                                    format!("{} ({})", place.name, b.location)
                                }
                                _ => place.name.clone(),
                            };
                            if ui.button(label).on_hover_text(place.describe()).clicked() {
                                state.selected = Some(place.entity.clone());
                                state.found = Some(place.clone());
                                state.query.clear();
                                if let (Some(view), Some(b)) = (state.view.as_mut(), place.best()) {
                                    view.center = (b.lon(), b.lat());
                                    view.scale = view.scale.max(60.0);
                                    state.moved = true;
                                }
                            }
                        }
                    });
                }
            }
        }
        // The map, and the place list beside it, or under it in a narrow Tab.
        // Within what the Panel shows, however wide the rows above grew.
        let area = ui.available_rect_before_wrap().intersect(ui.clip_rect());
        let (map_rect, list_rect) = if area.width() >= LIST_WIDTH * 2.4 {
            (
                Rect::from_min_max(area.min, pos2(area.max.x - LIST_WIDTH - 8.0, area.max.y)),
                Rect::from_min_max(pos2(area.max.x - LIST_WIDTH, area.min.y), area.max),
            )
        } else {
            let map_h = (area.height() * 0.62).max(MINI_MAP_HEIGHT.min(area.height()));
            (
                Rect::from_min_max(area.min, pos2(area.max.x, area.min.y + map_h)),
                Rect::from_min_max(pos2(area.min.x, area.min.y + map_h + 6.0), area.max),
            )
        };
        let mut shown_places: Vec<&MapPlace> = places.iter().collect();
        if let Some(found) = &state.found
            && !shown_places.iter().any(|p| p.entity == found.entity)
        {
            shown_places.push(found);
        }
        let resized = state.fitted_size != Some(map_rect.size());
        if refit || state.view.is_none() || (resized && !state.moved) {
            let located: Vec<&MapPlace> = places.iter().filter(|p| !p.points.is_empty()).collect();
            state.view = Some(fit(&located, map_rect, false, MIN_SPAN));
            state.fitted = Some(passage);
            state.fitted_size = Some(map_rect.size());
            state.moved = false;
        }
        let mut view = state
            .view
            .unwrap_or_else(|| fit(&[], map_rect, false, MIN_SPAN));
        if let Some(factor) = zoom {
            state.moved = true;
            let c = map_rect.center() - map_rect.min;
            view.zoom_about(
                factor,
                f64::from(c.x),
                f64::from(c.y),
                (f64::from(map_rect.width()), f64::from(map_rect.height())),
                base_map::builtin().bounds,
            );
        }
        let description = format!(
            "Map of {}: {}",
            format_passage(passage, &Standard, RefStyle::Long),
            places
                .iter()
                .filter(|p| !p.points.is_empty())
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        let emphasized = |p: &MapPlace| current.is_some_and(|v| p.verses.contains(&v));
        let input = map_canvas(
            ui,
            map_rect,
            &mut view,
            &MapScene {
                places: &shown_places,
                selected: state.selected.as_deref(),
                emphasized: &emphasized,
                all_points: false,
                interactive: true,
                description: &description,
            },
        );
        state.view = Some(view);
        state.moved |= input.moved;
        if input.clicked.is_some() {
            open = input.clicked;
        }
        if list_rect.height() > 20.0 {
            let mut list = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(list_rect)
                    .id_salt(("atlas-list", tab)),
            );
            egui::ScrollArea::vertical()
                .id_salt(("atlas-places", tab, passage))
                .auto_shrink([false, false])
                .show(&mut list, |ui| {
                    if let Some(action) = place_list(ui, &places, &mut state, &emphasized) {
                        open = Some(action);
                    }
                });
        }
        if let Some(t) = self.tabs.get_mut(&tab) {
            t.map = Some(state);
        }
        if let Some((entity, new_tab)) = open {
            self.run(
                "guide.factbook",
                &json!({ "entity": entity, "new_tab": new_tab }),
            );
        }
    }
}

/// The places beside the map: each with where it was and how sure that is, and the Verses
/// that name it. Clicking a name picks it on the map; `Factbook ›` opens its Factbook.
fn place_list(
    ui: &mut Ui,
    places: &[MapPlace],
    state: &mut MapUi,
    emphasized: &dyn Fn(&MapPlace) -> bool,
) -> Option<(String, bool)> {
    let mut open = None;
    let new_tab = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
    if places.is_empty() {
        ui.label(
            RichText::new(
                "No place is listed for this Passage. Places come from the library's \
                 People & places indexes.",
            )
            .color(theme::c().muted),
        );
        return None;
    }
    let (located, unlocated): (Vec<&MapPlace>, Vec<&MapPlace>) =
        places.iter().partition(|p| !p.points.is_empty());
    for place in located {
        let Some(best) = place.best() else {
            continue;
        };
        let picked = state.selected.as_deref() == Some(place.entity.as_str());
        let mut name = RichText::new(&place.name).strong();
        if picked || emphasized(place) {
            name = name.color(theme::c().lapis);
        }
        ui.horizontal(|ui| {
            if ui
                .selectable_label(picked, name)
                .on_hover_text("Show it on the map")
                .clicked()
            {
                state.selected = (!picked).then(|| place.entity.clone());
                state.found = None;
                if let Some(view) = state.view.as_mut()
                    && !picked
                {
                    view.center = (best.lon(), best.lat());
                    state.moved = true;
                }
            }
            if ui
                .small_button("Factbook ›")
                .on_hover_text(format!("Open {}'s Factbook", place.name))
                .clicked()
            {
                open = Some((place.entity.clone(), new_tab));
            }
        });
        let mut detail = String::new();
        if best.location != place.name {
            detail.push_str(&best.location);
            detail.push_str(" · ");
        }
        detail.push_str(best.confidence.label());
        if place.points.len() > 1 {
            detail.push_str(&format!(" · {} possible locations", place.points.len()));
        }
        ui.label(RichText::new(detail).small().color(theme::c().muted));
        let refs = verse_list(&place.verses);
        if !refs.is_empty() {
            ui.label(RichText::new(refs).small().color(theme::c().muted));
        }
        ui.add_space(4.0);
    }
    if !unlocated.is_empty() {
        ui.separator();
        ui.label(
            RichText::new("Not on the map (location unknown)")
                .small()
                .strong(),
        );
        for place in unlocated {
            ui.horizontal(|ui| {
                ui.label(&place.name);
                if ui.small_button("Factbook ›").clicked() {
                    open = Some((place.entity.clone(), new_tab));
                }
            });
        }
    }
    open
}
