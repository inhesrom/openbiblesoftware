//! Footnote popovers (#208). Resting the pointer on a Footnote marker for Word Info's dwell
//! (or reaching it with Tab) shows the Footnote beside it; clicking the marker (or Enter)
//! pins it until Esc, its ×, a click elsewhere or another click on the marker. The note is
//! Resource text, drawn with the text pipeline; each Bible reference in it is a link whose
//! hover (or keyboard focus) previews the Passage and whose click goes there.

use std::ops::Range;
use std::sync::Arc;

use egui::{Mesh, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, pos2, vec2};
use obs_model::{Direction, Span, SpanKind, TextUnit};
use obs_scripture::{Passage, PassageList, RefStyle, format_passage};

use super::references::{Targets, standard_targets};
use super::{HOVER_DWELL_SECS, State, TabId};
use crate::doc::ReadingDoc;
use crate::pane::{FootnoteHit, PaneOutput};
use crate::theme;
use crate::workers::SnippetCache;

/// The popover's text column, in points.
const WIDTH: f32 = 340.0;

/// A Footnote as its popover shows it: the note's text unit, with each Bible reference in it
/// a link to its Standard Passages, and the line naming it ("Footnote a · John 1:5").
#[derive(Debug)]
pub(crate) struct FootnoteBody {
    pub(crate) unit: TextUnit,
    pub(crate) links: Vec<(Range<usize>, Targets)>,
    pub(crate) header: String,
    direction: Direction,
}

/// The one Footnote popover, shown or waiting for its dwell.
#[derive(Debug)]
pub(crate) struct FootnotePopover {
    pub(crate) tab: TabId,
    pub(crate) hit: FootnoteHit,
    since: f64,
    pub(crate) shown: bool,
    pub(crate) pinned: bool,
    /// Pinned from the keyboard: focus moves into it once it is drawn.
    focus_inside: bool,
    /// Where it was drawn last frame: the pointer may rest on it without closing it.
    pub(crate) rect: Option<Rect>,
    /// Where its links were drawn last frame, in `body.links` order.
    pub(crate) link_rects: Vec<Rect>,
    pub(crate) body: Option<Arc<FootnoteBody>>,
}

#[derive(Debug, Default)]
pub(crate) struct Footnotes {
    pub(crate) popover: Option<FootnotePopover>,
    snippets: SnippetCache,
}

/// A Footnote's text with its Bible references as links. References the importer marked are
/// kept; the rest of the text is searched for written references ("Cited in Hebrews 4:4"),
/// which are read in the Bible's own numbering, like a reference in its text (ADR 0002).
pub(crate) fn footnote_body(doc: &ReadingDoc, hit: &FootnoteHit) -> Option<FootnoteBody> {
    let mut unit = doc.footnote(&hit.note)?.clone();
    let mut links: Vec<(Range<usize>, Targets)> = Vec::new();
    for s in &unit.spans {
        let range = s.range();
        match &s.kind {
            SpanKind::ScriptureRef { targets } if !range.is_empty() => {
                links.push((range, standard_targets(doc, targets, true)));
            }
            SpanKind::Xref { targets } if !range.is_empty() => {
                links.push((range, standard_targets(doc, targets, false)));
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    for (range, list) in obs_engine::body_references(&unit.text) {
        if links
            .iter()
            .any(|(r, _)| r.start < range.end && range.start < r.end)
        {
            continue;
        }
        found.push(Span {
            start: u32::try_from(range.start).ok()?,
            end: u32::try_from(range.end).ok()?,
            kind: SpanKind::ScriptureRef {
                targets: list.clone(),
            },
        });
        links.push((range, standard_targets(doc, &list, true)));
    }
    if !found.is_empty() {
        unit.spans.extend(found);
        unit.spans
            .sort_by_key(|s| (s.start, std::cmp::Reverse(s.end)));
    }
    links.sort_by_key(|(r, _)| r.start);
    let place = doc
        .verse_containing(hit.unit, hit.anchor)
        .map(|v| format_passage(Passage::verse(v), &doc.versification, RefStyle::Short));
    let header = match place {
        Some(place) => format!("{} · {place}", hit.name()),
        None => hit.name(),
    };
    Some(FootnoteBody {
        unit,
        links,
        header,
        direction: doc.direction(),
    })
}

fn same_marker(a: &FootnoteHit, b: &FootnoteHit) -> bool {
    a.unit == b.unit && a.anchor == b.anchor && a.note == b.note
}

impl State {
    /// Follow a reading pane's Footnote markers this frame: start, continue or end the dwell,
    /// pin on a click, close a pinned popover on Esc or a click elsewhere, and draw it.
    pub(super) fn footnote_frame(&mut self, ui: &mut Ui, tab: TabId, out: &PaneOutput) {
        let (pointer, pressed, escape) = ui.input(|i| {
            (
                i.pointer.hover_pos(),
                i.pointer.any_pressed(),
                i.key_pressed(egui::Key::Escape),
            )
        });
        let now = self.now;
        let fresh = |hit: &FootnoteHit, pinned: bool| FootnotePopover {
            tab,
            hit: hit.clone(),
            since: now,
            shown: pinned,
            pinned,
            focus_inside: false,
            rect: None,
            link_rects: Vec::new(),
            body: None,
        };
        let popover = &mut self.footnotes.popover;
        if let Some(hit) = &out.clicked_footnote {
            let again = popover
                .as_ref()
                .is_some_and(|p| p.tab == tab && p.pinned && same_marker(&p.hit, hit));
            if again {
                *popover = None;
            } else {
                let keyboard = pointer.is_none_or(|p| !hit.rect.expand(3.0).contains(p));
                let mut p = fresh(hit, true);
                if let Some(old) = popover.take()
                    && same_marker(&old.hit, hit)
                {
                    p.body = old.body;
                }
                p.focus_inside = keyboard;
                *popover = Some(p);
            }
        } else if let Some(hit) = &out.hovered_footnote {
            let keep = popover
                .as_ref()
                .is_some_and(|p| p.tab == tab && (p.pinned || same_marker(&p.hit, hit)));
            if !keep {
                *popover = Some(fresh(hit, false));
            }
        } else if let Some(p) = popover.as_ref()
            && p.tab == tab
            && !p.pinned
        {
            let on = pointer.is_some_and(|at| {
                p.hit.rect.expand(6.0).contains(at)
                    || p.rect.is_some_and(|r| r.expand(6.0).contains(at))
            });
            if !on {
                *popover = None;
            }
        }
        if let Some(p) = popover.as_ref()
            && p.tab == tab
            && p.pinned
            && out.clicked_footnote.is_none()
        {
            let outside = pointer.is_some_and(|at| {
                !p.hit.rect.expand(3.0).contains(at) && !p.rect.is_some_and(|r| r.contains(at))
            });
            if escape || (pressed && outside) {
                if escape {
                    let marker = p.hit.widget;
                    ui.ctx().memory_mut(|m| m.request_focus(marker));
                }
                *popover = None;
            }
        }
        // The text may have moved under it: it follows its marker, and goes with it.
        if let Some(p) = popover.as_mut().filter(|p| p.tab == tab) {
            let height = out.response.as_ref().map_or(0.0, |r| r.rect.height());
            let moved = self
                .tabs
                .get(&tab)
                .and_then(|t| t.view.as_ref())
                .and_then(|view| {
                    let [ox, oy] = view.text_origin;
                    view.footnote_markers(height)
                        .into_iter()
                        .find(|m| {
                            m.unit == p.hit.unit && m.anchor == p.hit.anchor && m.note == p.hit.note
                        })
                        .map(|m| {
                            let [x0, y0, x1, y1] = m.rect;
                            Rect::from_min_max(pos2(ox + x0, oy + y0), pos2(ox + x1, oy + y1))
                        })
                });
            match moved {
                Some(r) => p.hit.rect = r,
                None => *popover = None,
            }
        }
        let Some(p) = popover.as_mut().filter(|p| p.tab == tab) else {
            return;
        };
        if !p.shown {
            let rested = now - p.since;
            if rested < HOVER_DWELL_SECS {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_secs_f64(
                        HOVER_DWELL_SECS - rested,
                    ));
                return;
            }
            p.shown = true;
        }
        if p.body.is_none() {
            let Some(doc) = self
                .tabs
                .get(&tab)
                .and_then(|t| t.view.as_ref())
                .map(|v| &v.doc)
            else {
                return;
            };
            p.body = footnote_body(doc, &p.hit).map(Arc::new);
        }
        self.footnote_popover(ui, tab);
    }

    /// Draw the popover beside its marker.
    fn footnote_popover(&mut self, ui: &Ui, tab: TabId) {
        let Some(p) = self.footnotes.popover.as_ref() else {
            return;
        };
        let Some(body) = p.body.clone() else {
            return;
        };
        let (marker, pinned, focus_inside) = (p.hit.rect, p.pinned, p.focus_inside);
        let mut close = false;
        let mut go: Option<(Passage, bool)> = None;
        let mut links = Vec::new();
        let shown = egui::Area::new(egui::Id::unique(("footnote-popover", tab)))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(marker.left() - 8.0, marker.bottom() + 2.0))
            .constrain(true)
            .fade_in(false)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(WIDTH);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&body.header).small().color(theme::c().muted));
                        if pinned {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let r = ui
                                        .add(
                                            egui::Button::new(RichText::new("×").small())
                                                .frame(false),
                                        )
                                        .on_hover_text("Close (Esc)");
                                    r.widget_info(|| {
                                        egui::WidgetInfo::labeled(
                                            egui::Role::Button,
                                            true,
                                            "Close footnote",
                                        )
                                    });
                                    if focus_inside && body.links.is_empty() {
                                        r.request_focus();
                                    }
                                    close |= r.clicked();
                                },
                            );
                        }
                    });
                    (go, links) = self.footnote_text(ui, &body, focus_inside);
                    if !pinned {
                        ui.label(
                            RichText::new("Click the marker to keep this open")
                                .small()
                                .color(theme::c().muted),
                        );
                    }
                });
            });
        if let Some(p) = self.footnotes.popover.as_mut() {
            p.rect = Some(shown.response.rect);
            p.link_rects = links;
            p.focus_inside = false;
        }
        if close {
            self.footnotes.popover = None;
        }
        if let Some((passage, new_tab)) = go {
            self.footnotes.popover = None;
            self.reference_navigate(tab, passage, new_tab);
        }
    }

    /// The Footnote's text through the text pipeline, its references underlined as links.
    /// Returns the Passage of a link clicked (and whether Ctrl/Cmd was held), and where each
    /// link is.
    fn footnote_text(
        &mut self,
        ui: &mut Ui,
        body: &FootnoteBody,
        focus_first: bool,
    ) -> (Option<(Passage, bool)>, Vec<Rect>) {
        let settings = self.session.text;
        let laid = self
            .footnotes
            .snippets
            .get_unit(&body.unit, body.direction, WIDTH, &settings);
        let Some(laid) = laid else {
            ui.allocate_space(vec2(WIDTH, settings.size * 1.6));
            ui.ctx().request_repaint();
            return (None, Vec::new());
        };
        let height = laid.lines.last().map_or(laid.height, |l| l.bottom);
        let (rect, response) = ui.allocate_exact_size(vec2(WIDTH, height), Sense::hover());
        let text = body.unit.text.clone();
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &text));
        let quads = self.atlas.atlas.quads(
            &laid,
            [rect.left(), rect.top()],
            ui.ctx().pixels_per_point(),
        );
        let mut mesh = Mesh::with_texture(self.atlas.sync(ui.ctx()));
        for q in quads {
            mesh.add_rect_with_uv(
                Rect::from_min_max(Pos2::from(q.min), Pos2::from(q.max)),
                Rect::from_min_max(Pos2::from(q.uv_min), Pos2::from(q.uv_max)),
                self.palette.color(q.role),
            );
        }
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        painter.rect_filled(rect.expand(4.0), 3.0, self.palette.page);
        painter.add(Shape::mesh(mesh));
        let mut go = None;
        let mut drawn = Vec::new();
        for (i, (range, targets)) in body.links.iter().enumerate() {
            let rects: Vec<Rect> = crate::compare_view::range_rects(&laid, range)
                .into_iter()
                .map(|[x0, y0, x1, y1]| {
                    Rect::from_min_max(rect.min + vec2(x0, y0), rect.min + vec2(x1, y1))
                })
                .collect();
            let Some(area) = rects.iter().copied().reduce(Rect::union) else {
                drawn.push(Rect::NOTHING);
                continue;
            };
            drawn.push(area);
            for r in &rects {
                painter.hline(
                    r.x_range(),
                    r.bottom() - 2.0,
                    Stroke::new(1.0, self.palette.link.gamma_multiply(0.6)),
                );
            }
            let written = body
                .unit
                .text
                .get(range.clone())
                .unwrap_or_default()
                .to_string();
            let link = ui.interact(
                area,
                ui.scope_id().with(("footnote-link", i)),
                Sense::click(),
            );
            link.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Link, true, &written));
            if focus_first && i == 0 {
                link.request_focus();
            }
            let pointed = link.hovered()
                && rects
                    .iter()
                    .any(|r| link.hover_pos().is_some_and(|p| r.contains(p)));
            if link.has_focus() {
                painter.rect_stroke(
                    area.expand(1.0),
                    2.0,
                    ui.visuals().selection.stroke,
                    egui::StrokeKind::Outside,
                );
            }
            if pointed || link.has_focus() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                self.reference_popup(ui, targets);
            }
            if link.clicked()
                && let Some(passage) = targets.iter().flat_map(PassageList::iter).next()
            {
                let new_tab = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
                go = Some((*passage, new_tab));
            }
        }
        (go, drawn)
    }
}
