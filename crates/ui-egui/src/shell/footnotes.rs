//! Footnote popovers (#208). Resting the pointer on a Footnote marker for Word Info's dwell
//! (or reaching it with Tab) shows the Footnote beside it; clicking the marker (or Enter)
//! pins it until Esc, its ×, a click elsewhere or another click on the marker. The note is
//! Resource text, drawn with the text pipeline; each Bible reference in it is a link whose
//! hover (or keyboard focus) previews the Passage and whose click goes there.

use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::Arc;

use egui::{Mesh, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, pos2, vec2};
use obs_model::{Direction, Span, SpanKind, TextUnit};
use obs_scripture::{
    Book, Passage, PassageList, RefStyle, Standard, Versification, format_passage,
};

use super::references::{Targets, standard_targets};
use super::{HOVER_DWELL_SECS, State, TabId};
use crate::doc::ReadingDoc;
use crate::pane::{FootnoteHit, PaneOutput};
use crate::theme;
use crate::workers::SnippetCache;

/// The popover's text column, in points.
const WIDTH: f32 = 340.0;
/// The most the popover's text shows at once, in points: a longer note scrolls.
const MAX_BODY_HEIGHT: f32 = 360.0;

/// A Footnote as its popover shows it: the note's text unit, with each Bible reference in it
/// a link to its Standard Passages, and the line naming it ("Footnote a · John 1:5").
#[derive(Debug)]
pub(crate) struct FootnoteBody {
    pub(crate) unit: TextUnit,
    pub(crate) links: Vec<(Range<usize>, Targets)>,
    pub(crate) header: String,
    direction: Direction,
}

/// Where a Tab's Footnote popover stands.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) enum FootnotePhase {
    #[default]
    Closed,
    /// The pointer (or keyboard focus) rests on the marker; its popover shows once Word
    /// Info's dwell has passed since `since`.
    Dwelling { marker: FootnoteHit, since: f64 },
    /// Shown on hover: it closes once the pointer and focus leave the marker and popover.
    Open { marker: FootnoteHit },
    /// Clicked (or Enter): it stays until Esc, its ×, a click elsewhere or another click on
    /// the marker.
    Pinned { marker: FootnoteHit },
}

impl FootnotePhase {
    pub(crate) fn marker(&self) -> Option<&FootnoteHit> {
        match self {
            Self::Closed => None,
            Self::Dwelling { marker, .. } | Self::Open { marker } | Self::Pinned { marker } => {
                Some(marker)
            }
        }
    }

    fn marker_mut(&mut self) -> Option<&mut FootnoteHit> {
        match self {
            Self::Closed => None,
            Self::Dwelling { marker, .. } | Self::Open { marker } | Self::Pinned { marker } => {
                Some(marker)
            }
        }
    }

    /// Whether `hit` is this phase's marker.
    fn on(&self, hit: &FootnoteHit) -> bool {
        self.marker().is_some_and(|m| same_marker(m, hit))
    }

    pub(crate) fn shown(&self) -> bool {
        matches!(self, Self::Open { .. } | Self::Pinned { .. })
    }

    pub(crate) fn pinned(&self) -> bool {
        matches!(self, Self::Pinned { .. })
    }
}

/// A Tab's Footnote popover: its phase, and the markers it was dismissed from. Every change
/// goes through [`next`].
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct FootnoteState {
    pub(crate) phase: FootnotePhase,
    /// Markers whose popover was dismissed (Esc, another click on the marker, a click
    /// elsewhere, its × or a link followed): none shows for them while the pointer or focus
    /// stays on them, so the dwell cannot bring one back, even after another marker's
    /// popover has shown meanwhile. A click (Enter) on one pins it again. At most two in
    /// practice: one under the pointer, one with the keyboard focus.
    pub(crate) dismissed: Vec<FootnoteHit>,
}

impl FootnoteState {
    /// The markers it follows: its phase's and the dismissed ones.
    pub(crate) fn markers(&self) -> impl Iterator<Item = &FootnoteHit> {
        self.phase.marker().into_iter().chain(&self.dismissed)
    }

    fn markers_mut(&mut self) -> impl Iterator<Item = &mut FootnoteHit> {
        self.phase
            .marker_mut()
            .into_iter()
            .chain(&mut self.dismissed)
    }

    fn dismiss(&mut self, marker: FootnoteHit) {
        if !self.dismissed.iter().any(|m| same_marker(m, &marker)) {
            self.dismissed.push(marker);
        }
    }

    fn forget(&mut self, marker: &FootnoteHit) {
        self.dismissed.retain(|m| !same_marker(m, marker));
    }

    fn is_dismissed(&self, marker: &FootnoteHit) -> bool {
        self.dismissed.iter().any(|m| same_marker(m, marker))
    }
}

/// What a reading pane tells its Footnote popover.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FootnoteEvent {
    /// The pointer (or keyboard focus) is on a marker, at time `at`; sent every frame it is.
    PointerEnter {
        marker: FootnoteHit,
        at: f64,
    },
    /// The pointer and focus have left the marker (and its popover, when one is shown).
    PointerLeave(FootnoteHit),
    /// Word Info's dwell has passed since the pointer came to rest.
    DwellElapsed,
    /// A click on a marker, or Enter (Space, a screen reader's click) on the focused one.
    Click(FootnoteHit),
    /// A click outside the marker and its popover.
    ClickAway,
    Esc,
    /// The popover's × clicked, or one of its links followed.
    Close,
    /// The marker is no longer in the Tab's text (scrolled away, markers hidden).
    MarkerGone(FootnoteHit),
    TabClosed,
    /// The Tab now shows another Resource.
    ResourceChanged,
}

/// A Tab's Footnote popover after `event`.
pub(crate) fn next(mut state: FootnoteState, event: FootnoteEvent) -> FootnoteState {
    use FootnoteEvent as E;
    use FootnotePhase as P;
    let pinned = state.phase.pinned();
    match event {
        E::TabClosed | E::ResourceChanged => return FootnoteState::default(),
        E::MarkerGone(marker) => {
            state.forget(&marker);
            if state.phase.on(&marker) {
                state.phase = P::Closed;
            }
        }
        E::Click(marker) => {
            if pinned && state.phase.on(&marker) {
                state.phase = P::Closed;
                state.dismiss(marker);
            } else {
                state.forget(&marker);
                state.phase = P::Pinned { marker };
            }
        }
        E::PointerEnter { marker, at } => {
            if pinned || state.phase.on(&marker) {
                return state;
            }
            state.phase = if state.is_dismissed(&marker) {
                // Resting on a dismissed marker: another marker's popover gives way to it.
                P::Closed
            } else {
                P::Dwelling { marker, since: at }
            };
        }
        E::PointerLeave(marker) => {
            state.forget(&marker);
            if !pinned && state.phase.on(&marker) {
                state.phase = P::Closed;
            }
        }
        E::DwellElapsed => {
            if let P::Dwelling { marker, .. } = state.phase {
                state.phase = P::Open { marker };
            }
        }
        E::ClickAway | E::Esc | E::Close => {
            let phase = std::mem::take(&mut state.phase);
            if let Some(marker) = phase.marker() {
                state.dismiss(marker.clone());
            }
        }
    }
    state
}

/// A Tab's Footnote popover: its state and what it drew last frame.
#[derive(Debug, Default)]
pub(crate) struct TabFootnote {
    pub(crate) state: FootnoteState,
    /// Pinned from the keyboard: focus moves into it once it is drawn. Kept until a link (or
    /// its ×) has been given focus, which waits for the note's text to be laid out.
    focus_inside: bool,
    /// Where it was drawn last frame: the pointer may rest on it without closing it.
    pub(crate) rect: Option<Rect>,
    /// Where its links were drawn last frame, in its body's `links` order.
    pub(crate) link_rects: Vec<Rect>,
    /// The pass (of its window's viewport) it was last drawn in: only a popover on screen
    /// takes Esc ahead of the keyboard shortcuts.
    drawn_in: Option<u64>,
    /// The note built for a marker: used only while that marker (of that Resource) is the
    /// phase's, so a hover's note serves its pin and never another Resource's marker.
    body: Option<(FootnoteHit, Arc<FootnoteBody>)>,
}

impl TabFootnote {
    pub(crate) fn body(&self) -> Option<&Arc<FootnoteBody>> {
        let (hit, body) = self.body.as_ref()?;
        self.state.phase.on(hit).then_some(body)
    }
}

#[derive(Debug, Default)]
pub(crate) struct Footnotes {
    pub(crate) tabs: BTreeMap<TabId, TabFootnote>,
    snippets: SnippetCache,
}

impl Footnotes {
    /// Send `event` to `tab`'s popover. One popover is pinned at a time: pinning one closes
    /// every other Tab's, and while one is pinned a marker in another Tab shows none on
    /// hover (only a click replaces it).
    pub(crate) fn send(&mut self, tab: TabId, event: FootnoteEvent) {
        let pinned_elsewhere = self
            .tabs
            .iter()
            .any(|(t, f)| *t != tab && f.state.phase.pinned());
        if pinned_elsewhere && matches!(event, FootnoteEvent::PointerEnter { .. }) {
            return;
        }
        let closed = event == FootnoteEvent::TabClosed;
        let f = self.tabs.entry(tab).or_default();
        f.state = next(std::mem::take(&mut f.state), event);
        let pinned = f.state.phase.pinned();
        if !f.state.phase.shown() {
            f.rect = None;
            f.link_rects.clear();
            f.focus_inside = false;
            f.drawn_in = None;
        }
        if closed {
            self.tabs.remove(&tab);
        }
        if pinned {
            for (_, f) in self.tabs.iter_mut().filter(|(t, _)| **t != tab) {
                if f.state.phase != FootnotePhase::Closed {
                    // Its dismissed markers stay dismissed.
                    let dismissed = std::mem::take(&mut f.state.dismissed);
                    *f = TabFootnote::default();
                    f.state.dismissed = dismissed;
                }
            }
        }
    }

    /// A Tab closed: its popover goes with it (pinned, it would otherwise keep every other
    /// marker from showing one).
    pub(crate) fn tab_closed(&mut self, tab: TabId) {
        self.send(tab, FootnoteEvent::TabClosed);
    }

    /// Only these Tabs are left (a Layout loaded, the Workspace rebuilt).
    pub(crate) fn retain_tabs(&mut self, live: &[TabId]) {
        self.tabs.retain(|t, _| live.contains(t));
    }

    /// Whether a popover shown on a Tab `here` was drawn in the last pass of `ctx`'s viewport:
    /// Esc is for it before any command bound to Esc.
    pub(crate) fn on_screen(&self, ctx: &egui::Context, here: impl Fn(TabId) -> bool) -> bool {
        let pass = ctx.cumulative_pass_nr();
        self.tabs.iter().any(|(t, f)| {
            f.state.phase.shown() && f.drawn_in.is_some_and(|d| d + 1 >= pass) && here(*t)
        })
    }

    /// The popover drawn (open on hover or pinned), with its Tab.
    #[cfg(test)]
    pub(crate) fn shown(&self) -> Option<(TabId, &TabFootnote)> {
        self.tabs
            .iter()
            .find(|(_, f)| f.state.phase.shown())
            .map(|(t, f)| (*t, f))
    }

    /// The marker whose popover shows or waits for its dwell, in any Tab.
    #[cfg(test)]
    pub(crate) fn pending(&self) -> Option<(TabId, &FootnoteHit)> {
        self.tabs.iter().find_map(|(t, f)| match &f.state.phase {
            FootnotePhase::Dwelling { marker, .. }
            | FootnotePhase::Open { marker }
            | FootnotePhase::Pinned { marker } => Some((*t, marker)),
            _ => None,
        })
    }
}
/// A Bible's own numbering for the books it has, the Standard for the others: how a
/// reference written in its Footnote is read, item by item, so a list naming both ("Ps 3:9;
/// Rom 3:4" in a Hebrew Psalter) keeps every item.
struct OwnElseStandard<'a>(&'a dyn Versification);

impl OwnElseStandard<'_> {
    fn has(&self, book: Book) -> bool {
        self.0.chapter_count(book) > 0
    }
}

impl Versification for OwnElseStandard<'_> {
    fn chapter_count(&self, book: Book) -> u16 {
        if self.has(book) {
            self.0.chapter_count(book)
        } else {
            Standard.chapter_count(book)
        }
    }

    fn last_verse(&self, book: Book, chapter: u16) -> Option<u16> {
        if self.has(book) {
            self.0.last_verse(book, chapter)
        } else {
            Standard.last_verse(book, chapter)
        }
    }

    fn has_title(&self, book: Book, chapter: u16) -> bool {
        if self.has(book) {
            self.0.has_title(book, chapter)
        } else {
            Standard.has_title(book, chapter)
        }
    }
}

/// A Footnote's text with its Bible references as links. References the importer marked are
/// kept; the rest of the text is searched for written references ("Cited in Hebrews 4:4"),
/// which are read in the Bible's own numbering and go through its Verse map, like a
/// reference in its text (ADR 0002). A reference to a book the Bible does not have (an
/// Old Testament verse cited in a New Testament) has no own numbering: it is read in the
/// Standard, item by item within a list that also names the Bible's books.
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
    let numbering = OwnElseStandard(&doc.versification);
    let mut found = Vec::new();
    for (range, list) in obs_engine::body_references_in(&unit.text, &numbering) {
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
        let targets = list
            .iter()
            .flat_map(|p| {
                let native = numbering.has(p.start().book);
                standard_targets(doc, &vec![*p].into(), native)
            })
            .collect();
        links.push((range, targets));
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
    a.resource == b.resource && a.unit == b.unit && a.anchor == b.anchor && a.note == b.note
}

impl State {
    /// Follow a reading pane's Footnote markers this frame: turn what the pointer, keyboard
    /// and text did into events for its Tab's popover, and draw the popover.
    pub(super) fn footnote_frame(&mut self, ui: &mut Ui, tab: TabId, out: &PaneOutput) {
        // A shown popover takes the Esc that closes it: nothing behind it (a floating window,
        // the Note editor) closes too.
        let shown = (self.footnotes.tabs.get(&tab)).is_some_and(|f| f.state.phase.shown());
        let (pointer, pressed, escape) = ui.input_mut(|i| {
            (
                i.pointer.hover_pos(),
                i.pointer.any_pressed(),
                if shown {
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
                } else {
                    i.key_pressed(egui::Key::Escape)
                },
            )
        });
        let now = self.now;
        self.footnote_follow(tab, out);
        let send = |s: &mut Self, e| s.footnotes.send(tab, e);
        if let Some(hit) = &out.clicked_footnote {
            send(self, FootnoteEvent::Click(hit.clone()));
            if let Some(f) = self.footnotes.tabs.get_mut(&tab)
                && f.state.phase.pinned()
            {
                f.focus_inside = out.footnote_by_keyboard;
            }
        } else if let Some(hit) = out
            .hovered_footnote
            .as_ref()
            .or(out.focused_footnote.as_ref())
        {
            send(
                self,
                FootnoteEvent::PointerEnter {
                    marker: hit.clone(),
                    at: now,
                },
            );
        }
        // Each marker it follows that neither the pointer nor the focus is on any more (nor,
        // for a shown popover's, the pointer on the popover) has been left.
        if let Some(f) = self.footnotes.tabs.get(&tab) {
            let left: Vec<FootnoteHit> = f
                .state
                .markers()
                .filter(|m| {
                    let pointed = pointer.is_some_and(|at| {
                        m.rect.expand(6.0).contains(at)
                            || (f.state.phase.on(m)
                                && f.rect.is_some_and(|r| r.expand(6.0).contains(at)))
                    });
                    let resting = [&out.hovered_footnote, &out.focused_footnote]
                        .into_iter()
                        .flatten()
                        .any(|h| same_marker(h, m));
                    !pointed && !resting
                })
                .cloned()
                .collect();
            for m in left {
                send(self, FootnoteEvent::PointerLeave(m));
            }
        }
        if out.clicked_footnote.is_none()
            && let Some(f) = self.footnotes.tabs.get(&tab)
            && let Some(m) = f.state.phase.marker()
        {
            if escape {
                // Focus goes back to the marker a pinned popover closes from.
                if f.state.phase.pinned() {
                    let marker = m.widget;
                    ui.ctx().memory_mut(|mem| mem.request_focus(marker));
                }
                send(self, FootnoteEvent::Esc);
            } else if pressed
                && pointer.is_some_and(|at| {
                    !m.rect.expand(3.0).contains(at) && !f.rect.is_some_and(|r| r.contains(at))
                })
            {
                send(self, FootnoteEvent::ClickAway);
            }
        }
        if let Some(FootnotePhase::Dwelling { since, .. }) =
            self.footnotes.tabs.get(&tab).map(|f| &f.state.phase)
        {
            let rested = now - since;
            if rested < HOVER_DWELL_SECS {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_secs_f64(
                        HOVER_DWELL_SECS - rested,
                    ));
                return;
            }
            send(self, FootnoteEvent::DwellElapsed);
        }
        let Some(f) = self.footnotes.tabs.get_mut(&tab) else {
            return;
        };
        let Some(marker) = f.state.phase.marker().filter(|_| f.state.phase.shown()) else {
            return;
        };
        if f.body().is_none() {
            let Some(doc) = self
                .tabs
                .get(&tab)
                .and_then(|t| t.view.as_ref())
                .map(|v| &v.doc)
            else {
                return;
            };
            f.body = footnote_body(doc, marker).map(|b| (marker.clone(), Arc::new(b)));
        }
        self.footnote_popover(ui, tab);
    }

    /// The text may have moved under the markers the popover follows (its own and the
    /// dismissed ones): each follows its place, and goes with it.
    fn footnote_follow(&mut self, tab: TabId, out: &PaneOutput) {
        let Some(f) = self.footnotes.tabs.get_mut(&tab) else {
            return;
        };
        if f.state.markers().next().is_none() {
            return;
        }
        let height = out.response.as_ref().map_or(0.0, |r| r.rect.height());
        let view = self.tabs.get(&tab).and_then(|t| t.view.as_ref());
        let in_view = view.map(|v| (v, v.footnote_markers(height)));
        let mut gone = Vec::new();
        for m in f.state.markers_mut() {
            let moved = in_view
                .as_ref()
                .filter(|(view, _)| view.doc.manifest.resource_id == m.resource)
                .and_then(|(view, markers)| {
                    let [ox, oy] = view.text_origin;
                    markers
                        .iter()
                        .find(|n| n.unit == m.unit && n.anchor == m.anchor && n.note == m.note)
                        .map(|n| {
                            let [x0, y0, x1, y1] = n.rect;
                            Rect::from_min_max(pos2(ox + x0, oy + y0), pos2(ox + x1, oy + y1))
                        })
                });
            match moved {
                Some(r) => m.rect = r,
                None => gone.push(m.clone()),
            }
        }
        for m in gone {
            self.footnotes.send(tab, FootnoteEvent::MarkerGone(m));
        }
    }

    /// Draw the popover beside its marker.
    fn footnote_popover(&mut self, ui: &Ui, tab: TabId) {
        let Some(f) = self.footnotes.tabs.get(&tab) else {
            return;
        };
        let (Some(m), Some(body)) = (f.state.phase.marker(), f.body().cloned()) else {
            return;
        };
        let (marker, widget, pinned, focus_inside) =
            (m.rect, m.widget, f.state.phase.pinned(), f.focus_inside);
        // A long note scrolls inside the popover, which never grows past the window.
        let screen = ui.ctx().content_rect();
        let max_body = (screen.height() - 120.0).clamp(60.0, MAX_BODY_HEIGHT);
        let mut close = false;
        let mut close_id = None;
        let mut go: Option<(Passage, bool)> = None;
        let mut links = Vec::new();
        let mut focus = LinkFocus::NotLaidOut;
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
                                    let r = crate::icons::button(
                                        ui,
                                        crate::icons::Icon::Close,
                                        crate::icons::SMALL,
                                        theme::c().muted,
                                        "Close footnote",
                                    )
                                    .on_hover_text("Close (Esc)");
                                    close_id = Some(r.id);
                                    close |= r.clicked();
                                },
                            );
                        }
                    });
                    egui::ScrollArea::vertical()
                        .id_salt(("footnote-body", widget))
                        .max_height(max_body)
                        .show(ui, |ui| {
                            (go, links, focus) = self.footnote_text(ui, &body, focus_inside);
                        });
                    if !pinned {
                        ui.label(
                            RichText::new("Click the marker to keep this open")
                                .small()
                                .color(theme::c().muted),
                        );
                    }
                });
            });
        // No link to focus once the text is drawn: the × takes focus instead.
        let focus_moved = match focus {
            LinkFocus::NotLaidOut => false,
            LinkFocus::Link => true,
            LinkFocus::NoLink => {
                if focus_inside && let Some(id) = close_id {
                    ui.ctx().memory_mut(|m| m.request_focus(id));
                }
                true
            }
        };
        if let Some(f) = self.footnotes.tabs.get_mut(&tab) {
            f.rect = Some(shown.response.rect);
            f.drawn_in = Some(ui.ctx().cumulative_pass_nr());
            f.link_rects = links;
            if focus_moved {
                f.focus_inside = false;
            }
        }
        if close || go.is_some() {
            self.footnotes.send(tab, FootnoteEvent::Close);
        }
        if let Some((passage, new_tab)) = go {
            self.reference_navigate(tab, passage, new_tab);
        }
    }

    /// The Footnote's text through the text pipeline, its references underlined as links.
    /// Returns the Passage of a link clicked (and whether Ctrl/Cmd was held), where each
    /// link is, and whether a link could take focus (`focus_first` gives it to the first).
    fn footnote_text(
        &mut self,
        ui: &mut Ui,
        body: &FootnoteBody,
        focus_first: bool,
    ) -> (Option<(Passage, bool)>, Vec<Rect>, LinkFocus) {
        let settings = self.session.text;
        let laid = self
            .footnotes
            .snippets
            .get_unit(&body.unit, body.direction, WIDTH, &settings);
        let Some(laid) = laid else {
            ui.allocate_space(vec2(WIDTH, settings.size * 1.6));
            ui.ctx().request_repaint();
            return (None, Vec::new(), LinkFocus::NotLaidOut);
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
        let mut focus = LinkFocus::NoLink;
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
            // One widget per line the link wraps over (only the first takes focus), so a
            // wrapped link's bounding box never covers the links beside it.
            let id = ui.scope_id().with(("footnote-link", i));
            let first = rects.first().copied().unwrap_or(area);
            let link = ui.interact(first, id, Sense::click());
            link.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Link, true, &written));
            if link.gained_focus() {
                ui.scroll_to_rect(area, None);
            }
            if focus == LinkFocus::NoLink {
                focus = LinkFocus::Link;
                if focus_first {
                    link.request_focus();
                }
            }
            let mut pointed = link.hovered();
            let mut clicked = link.clicked();
            for (j, r) in rects.iter().enumerate().skip(1) {
                let line = ui.interact(*r, id.with(j), Sense::CLICK);
                pointed |= line.hovered();
                clicked |= line.clicked();
            }
            if link.has_focus() {
                for r in &rects {
                    painter.rect_stroke(
                        r.expand(1.0),
                        2.0,
                        ui.visuals().selection.stroke,
                        egui::StrokeKind::Outside,
                    );
                }
            }
            if pointed || link.has_focus() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                self.reference_popup(ui, targets);
            }
            if clicked && let Some(passage) = targets.iter().flat_map(PassageList::iter).next() {
                let new_tab = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
                go = Some((*passage, new_tab));
            }
        }
        (go, drawn, focus)
    }
}

/// Whether the popover's text has a link that can take keyboard focus this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinkFocus {
    /// The text is still with the layout worker: nothing to focus yet.
    NotLaidOut,
    NoLink,
    Link,
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use obs_model::{Direction, NoteKind, TextUnit, UnitId, UnitKind};

    use super::{
        FootnoteBody, FootnoteEvent as E, FootnoteHit, FootnotePhase as P, FootnoteState as S,
        Footnotes, TabFootnote, TabId, next,
    };

    /// Marker `a` and `b` of one Resource, and `A`: `a`'s place in another Resource.
    fn hit(name: char) -> FootnoteHit {
        let (resource, note) = match name {
            'a' => ("bsb", 1),
            'b' => ("bsb", 2),
            'A' => ("bsb-copy", 1),
            _ => unreachable!(),
        };
        FootnoteHit {
            resource: resource.into(),
            unit: 3,
            anchor: 10 * note as usize,
            note: UnitId::note(note),
            kind: NoteKind::Footnote,
            label: name.to_string(),
            rect: egui::Rect::NOTHING,
            widget: egui::Id::unique(name),
        }
    }

    /// Every event, a pointer arriving at 2.0.
    fn event(name: &str) -> E {
        let marker = |n: &str| hit(n.chars().last().unwrap());
        match name {
            "Enter a" | "Enter b" | "Enter A" => E::PointerEnter {
                marker: marker(name),
                at: 2.0,
            },
            "Leave a" | "Leave b" => E::PointerLeave(marker(name)),
            "DwellElapsed" => E::DwellElapsed,
            "Click a" | "Click b" | "Click A" => E::Click(marker(name)),
            "ClickAway" => E::ClickAway,
            "Esc" => E::Esc,
            "Close" => E::Close,
            "Gone a" | "Gone b" => E::MarkerGone(marker(name)),
            "TabClosed" => E::TabClosed,
            "ResourceChanged" => E::ResourceChanged,
            _ => unreachable!(),
        }
    }

    /// A state, written "Closed", "Dwelling a 1", "Open a", "Pinned b"…, followed by
    /// "/ a b" for the markers dismissed: "Closed / a", "Open b / a".
    fn want(text: &str) -> S {
        let (phase, dismissed) = text.split_once(" / ").unwrap_or((text, ""));
        let words: Vec<&str> = phase.split(' ').collect();
        let marker = || hit(words[1].chars().next().unwrap());
        let phase = match words[0] {
            "Closed" => P::Closed,
            "Dwelling" => P::Dwelling {
                marker: marker(),
                since: words[2].parse().unwrap(),
            },
            "Open" => P::Open { marker: marker() },
            "Pinned" => P::Pinned { marker: marker() },
            _ => unreachable!(),
        };
        let dismissed = dismissed
            .split(' ')
            .filter(|w| !w.is_empty())
            .map(|w| hit(w.chars().next().unwrap()))
            .collect();
        S { phase, dismissed }
    }

    const EVENTS: [&str; 16] = [
        "Enter a",
        "Enter b",
        "Enter A",
        "Leave a",
        "Leave b",
        "DwellElapsed",
        "Click a",
        "Click b",
        "Click A",
        "ClickAway",
        "Esc",
        "Close",
        "Gone a",
        "Gone b",
        "TabClosed",
        "ResourceChanged",
    ];

    /// Each state's next state for each event, in `EVENTS` order: every phase, a marker
    /// dismissed with none shown, and a dismissed marker kept while another one's shows.
    const TABLE: [(&str, [&str; 16]); 6] = [
        (
            "Closed",
            [
                "Dwelling a 2",
                "Dwelling b 2",
                "Dwelling A 2",
                "Closed",
                "Closed",
                "Closed",
                "Pinned a",
                "Pinned b",
                "Pinned A",
                "Closed",
                "Closed",
                "Closed",
                "Closed",
                "Closed",
                "Closed",
                "Closed",
            ],
        ),
        (
            "Dwelling a 1",
            [
                "Dwelling a 1",
                "Dwelling b 2",
                "Dwelling A 2",
                "Closed",
                "Dwelling a 1",
                "Open a",
                "Pinned a",
                "Pinned b",
                "Pinned A",
                "Closed / a",
                "Closed / a",
                "Closed / a",
                "Closed",
                "Dwelling a 1",
                "Closed",
                "Closed",
            ],
        ),
        (
            "Open a",
            [
                "Open a",
                "Dwelling b 2",
                "Dwelling A 2",
                "Closed",
                "Open a",
                "Open a",
                "Pinned a",
                "Pinned b",
                "Pinned A",
                "Closed / a",
                "Closed / a",
                "Closed / a",
                "Closed",
                "Open a",
                "Closed",
                "Closed",
            ],
        ),
        (
            "Pinned a",
            [
                "Pinned a",
                "Pinned a",
                "Pinned a",
                "Pinned a",
                "Pinned a",
                "Pinned a",
                "Closed / a",
                "Pinned b",
                "Pinned A",
                "Closed / a",
                "Closed / a",
                "Closed / a",
                "Closed",
                "Pinned a",
                "Closed",
                "Closed",
            ],
        ),
        (
            "Closed / a",
            [
                "Closed / a",
                "Dwelling b 2 / a",
                "Dwelling A 2 / a",
                "Closed",
                "Closed / a",
                "Closed / a",
                "Pinned a",
                "Pinned b / a",
                "Pinned A / a",
                "Closed / a",
                "Closed / a",
                "Closed / a",
                "Closed",
                "Closed / a",
                "Closed",
                "Closed",
            ],
        ),
        (
            "Open b / a",
            [
                "Closed / a",
                "Open b / a",
                "Dwelling A 2 / a",
                "Open b",
                "Closed / a",
                "Open b / a",
                "Pinned a",
                "Pinned b / a",
                "Pinned A / a",
                "Closed / a b",
                "Closed / a b",
                "Closed / a b",
                "Open b",
                "Closed / a",
                "Closed",
                "Closed",
            ],
        ),
    ];

    #[test]
    fn next_covers_every_phase_and_event() {
        for (state, row) in TABLE {
            for (name, expected) in EVENTS.iter().zip(row) {
                assert_eq!(
                    next(want(state), event(name)),
                    want(expected),
                    "{state} + {name}"
                );
            }
        }
    }

    /// Run `events` from `state`.
    fn run(state: S, events: impl IntoIterator<Item = E>) -> S {
        events.into_iter().fold(state, next)
    }

    fn rest_on(marker: char, frames: usize) -> Vec<E> {
        (0..frames)
            .flat_map(|i| {
                [
                    E::PointerEnter {
                        marker: hit(marker),
                        at: 10.0 + i as f64,
                    },
                    E::DwellElapsed,
                ]
            })
            .collect()
    }

    /// Review repro: a second click on the marker closes its popover, and the pointer left
    /// resting on the marker does not bring it back after the dwell; a further click does.
    #[test]
    fn a_second_click_keeps_the_popover_closed_past_the_dwell() {
        let mut events = vec![E::Click(hit('a')), E::Click(hit('a'))];
        events.extend(rest_on('a', 30));
        let phase = run(S::default(), events);
        assert_eq!(phase, want("Closed / a"));
        assert_eq!(next(phase, E::Click(hit('a'))), want("Pinned a"));
    }

    /// Esc on a hover popover (or a pinned one) keeps it closed while the pointer or focus
    /// rests on the marker; once they leave, the marker dwells again.
    #[test]
    fn esc_keeps_the_popover_closed_until_the_marker_is_left() {
        for opened in [E::DwellElapsed, E::Click(hit('a'))] {
            let mut events = rest_on('a', 1);
            events.extend([opened, E::Esc]);
            events.extend(rest_on('a', 30));
            let phase = run(S::default(), events);
            assert_eq!(phase, want("Closed / a"));
            let phase = run(
                phase,
                [
                    E::PointerLeave(hit('a')),
                    E::PointerEnter {
                        marker: hit('a'),
                        at: 50.0,
                    },
                ],
            );
            assert_eq!(phase, want("Dwelling a 50"));
        }
    }

    /// Review repro: a marker dismissed while it keeps the keyboard focus stays dismissed
    /// after the pointer has shown another marker's popover and left it for the text (the
    /// focused marker is where the pane rests again); it dwells again only once the focus
    /// leaves it too.
    #[test]
    fn a_dismissed_marker_stays_dismissed_while_focused_after_hovering_another() {
        let mut events = rest_on('a', 1);
        events.extend([E::DwellElapsed, E::Esc]);
        // The pointer onto `b`, whose popover shows: `a` keeps the focus.
        events.extend(rest_on('b', 3));
        let state = run(S::default(), events);
        assert_eq!(state, want("Open b / a"));
        // The pointer leaves `b` for the text; the focused `a` is where the pane rests.
        let mut events = vec![E::PointerLeave(hit('b'))];
        events.extend(rest_on('a', 30));
        let state = run(state, events);
        assert_eq!(state, want("Closed / a"), "no dwell brings it back");
        let state = run(state, [E::PointerLeave(hit('a')), event("Enter a")]);
        assert_eq!(state, want("Dwelling a 2"));
    }

    /// Pinning in one Tab closes another Tab's popover, and a pinned popover keeps a marker
    /// in another Tab from opening one on hover; once its Tab closes, that marker dwells.
    #[test]
    fn a_pinned_popover_is_the_only_one_until_its_tab_closes() {
        let (one, two) = (TabId(1), TabId(2));
        let mut f = Footnotes::default();
        f.send(two, event("Enter b"));
        f.send(two, E::DwellElapsed);
        f.send(one, E::Click(hit('a')));
        assert_eq!(f.tabs[&one].state, want("Pinned a"));
        assert_eq!(f.tabs[&two].state.phase, P::Closed, "replaced by the pin");
        f.send(two, event("Enter b"));
        assert_eq!(f.tabs[&two].state.phase, P::Closed, "no hover beside a pin");
        f.tab_closed(one);
        assert!(!f.tabs.contains_key(&one), "gone with its Tab");
        f.send(two, event("Enter b"));
        assert_eq!(f.tabs[&two].state, want("Dwelling b 2"));
        f.send(two, E::Click(hit('A')));
        f.retain_tabs(&[one]);
        assert!(f.tabs.is_empty(), "gone when the Tabs are rebuilt");
    }

    /// Two panes' Tabs, step by step: a marker focused in the first (`a`) pinned with Space
    /// and dismissed with Esc, while the pointer goes over the second's text and markers
    /// (`b`), its popover shows, is clicked, dismissed. Each step: the Tab, the event, then
    /// the first Tab's state and the second's.
    const PANES: [(u64, &str, &str, &str); 16] = [
        (1, "Enter a", "Dwelling a 2", "Closed"),
        (1, "Click a", "Pinned a", "Closed"),
        // A pin leaves no hover beside it, in any pane.
        (2, "Enter b", "Pinned a", "Closed"),
        (2, "ClickAway", "Pinned a", "Closed"),
        (1, "Esc", "Closed / a", "Closed"),
        (2, "Enter b", "Closed / a", "Dwelling b 2"),
        (2, "DwellElapsed", "Closed / a", "Open b"),
        // The first pane rests on its focused marker: it stays dismissed.
        (1, "Enter a", "Closed / a", "Open b"),
        (2, "Leave b", "Closed / a", "Closed"),
        (1, "Enter a", "Closed / a", "Closed"),
        (1, "DwellElapsed", "Closed / a", "Closed"),
        // Pinning in the second pane keeps the first's dismissal.
        (2, "Click b", "Closed / a", "Pinned b"),
        (2, "Esc", "Closed / a", "Closed / b"),
        // Focus and pointer off `a`: it dwells again.
        (1, "Leave a", "Closed", "Closed / b"),
        (1, "Enter a", "Dwelling a 2", "Closed / b"),
        (2, "Leave b", "Dwelling a 2", "Closed"),
    ];

    #[test]
    fn markers_in_two_panes_keep_their_dismissals() {
        let mut f = Footnotes::default();
        let state = |f: &Footnotes, t| f.tabs.get(&TabId(t)).map(|f| f.state.clone());
        for (i, (tab, name, one, two)) in PANES.into_iter().enumerate() {
            f.send(TabId(tab), event(name));
            let got = (state(&f, 1), state(&f, 2));
            let want = (want(one), want(two));
            assert_eq!(
                (got.0.unwrap_or_default(), got.1.unwrap_or_default()),
                want,
                "step {i}: Tab {tab} + {name}"
            );
        }
    }

    /// A note built for a marker serves only that marker of that Resource: the same note
    /// id under the same marker in another Resource is built again.
    #[test]
    fn a_popover_body_belongs_to_its_markers_resource() {
        let unit = TextUnit {
            id: UnitId::note(1),
            kind: UnitKind::Note,
            title: None,
            text: "Or comprehended".into(),
            spans: Vec::new(),
        };
        let body = Arc::new(FootnoteBody {
            unit,
            links: Vec::new(),
            header: "Footnote a".into(),
            direction: Direction::Ltr,
        });
        let mut f = TabFootnote {
            state: want("Open a"),
            body: Some((hit('a'), body)),
            ..TabFootnote::default()
        };
        assert!(f.body().is_some());
        f.state = next(std::mem::take(&mut f.state), E::Click(hit('a')));
        assert!(f.body().is_some(), "the hover's note serves its pin");
        f.state = next(std::mem::take(&mut f.state), E::Click(hit('A')));
        assert!(f.body().is_none(), "not another Resource's marker");
        f.state = next(std::mem::take(&mut f.state), E::ResourceChanged);
        assert!(f.body().is_none());
    }
}
