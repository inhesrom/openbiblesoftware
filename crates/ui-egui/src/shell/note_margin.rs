//! Note cards in the margin (#210): a Resource Tab whose Note display is Margin shows each Note
//! in its text as a card in the column beside it (`crate::note_margin` sets the column and
//! stacks the cards), drawn after the reading pane.
//!
//! - **Aligned**: a card starts on the line of its Note's marker, the marker the pane drew this
//!   frame, so cards scroll with the text. Cards that would overlap stack in text order, and
//!   each has a leader line from the text column's edge on its marker's line to the card.
//! - **Collapsed** to the first lines of the Note's text (three rows); clicking a
//!   card, or its chevron, expands it to the whole text, and the chevron collapses it again.
//!   Which cards are expanded is the Tab's, for as long as the app runs.
//! - **Editing in place**: clicking an expanded card's text edits it with the Note editor
//!   (#186), as a Notes Tool card does: it saves as you type, and when the editor loses the
//!   keys (a click elsewhere, Esc) or on Ctrl+Enter. One Note is edited at a time, here or in
//!   the Notes Tool ([`super::notes_tab::EditPlace`]).
//! - Cards are on the page, so they take the Page tone's colours.

use std::collections::{BTreeSet, HashMap};

use egui::text::{LayoutJob, TextWrapping};
use egui::{Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use obs_engine::TabId;
use obs_model::NoteLineKind;

use super::State;
use super::notes_tab::{EditPlace, card_editor_id};
use crate::icons::{self, Icon};
use crate::note_margin::stack;
use crate::pane::PaneOutput;
use crate::tokens::note_margin::{
    BORDER_MIX, ESTIMATE, ESTIMATE_ROW, FILL_MIX, LEADER, LEADER_HOVER, LIFT,
};
use crate::tokens::{alpha, inset, radius, space, stroke, text};

/// A collapsed card shows this many rows of the Note's first lines.
const COLLAPSED_ROWS: usize = 3;

/// A Tab's Note margin: each card's height when last drawn and the cards expanded.
#[derive(Default)]
pub(super) struct MarginUi {
    heights: HashMap<String, f32>,
    pub(super) expanded: BTreeSet<String>,
    /// The cards drawn last frame, top to bottom.
    pub(super) drawn: Vec<DrawnCard>,
}

/// A card as drawn last frame (tests, and the margin's own hover).
#[derive(Debug, Clone)]
#[cfg_attr(not(test), allow(dead_code))]
pub(super) struct DrawnCard {
    pub(super) id: String,
    pub(super) rect: Rect,
    /// Its Note's marker in the text.
    pub(super) marker: Rect,
    /// Its leader line, from the text column's edge to the card.
    pub(super) leader: [Pos2; 3],
    pub(super) expanded: bool,
    pub(super) editing: bool,
}

/// What a card was asked to do this frame.
enum CardAction {
    Expand,
    Collapse,
    Edit,
}

impl State {
    /// After a reading pane in Margin mode drew: its Note cards and their leader lines.
    pub(super) fn note_margin(&mut self, ui: &mut Ui, tab: TabId, out: &PaneOutput) {
        let Some(room) = out.note_margin else {
            if let Some(t) = self.tabs.get_mut(&tab) {
                t.margin.drawn.clear();
            }
            return;
        };
        let Some(pane) = out.response.as_ref().map(|r| r.rect) else {
            return;
        };
        // One card per Note, in text order: by line, then the Notes beside the verse number
        // (oldest first), then those in the line's text in reading order.
        let mut anchors: Vec<(String, Rect)> = Vec::new();
        let mut order: Vec<&crate::pane::NoteMarker> = out.note_markers.iter().collect();
        let line = |m: &crate::pane::NoteMarker| (m.rect.top() * 2.0).round() as i64;
        let along = |m: &crate::pane::NoteMarker| {
            if m.at_verse_number {
                f32::NEG_INFINITY
            } else if room.rtl {
                -m.rect.left()
            } else {
                m.rect.left()
            }
        };
        order.sort_by(|a, b| line(a).cmp(&line(b)).then(along(a).total_cmp(&along(b))));
        for m in order {
            if !anchors.iter().any(|(a, _)| *a == m.id) {
                anchors.push((m.id.clone(), m.rect));
            }
        }
        let notes: Vec<(String, Rect, String, String)> = anchors
            .into_iter()
            .filter_map(|(id, marker)| {
                let a = self.marks.annotation(&id)?;
                let notebook = self.marks.notebook_name(&a.notebook_id).unwrap_or_default();
                Some((id, marker, a.body().to_string(), notebook))
            })
            .collect();
        let Some(t) = self.tabs.get_mut(&tab) else {
            return;
        };
        let estimate =
            |body: &str| ESTIMATE + ESTIMATE_ROW * body.lines().count().min(COLLAPSED_ROWS) as f32;
        let heights: Vec<f32> = notes
            .iter()
            .map(|(id, _, body, _)| t.margin.heights.get(id).copied().unwrap_or(estimate(body)))
            .collect();
        let wanted: Vec<f32> = notes.iter().map(|(_, m, _, _)| m.top() - LIFT).collect();
        let tops = stack(&wanted, &heights);
        let expanded = t.margin.expanded.clone();
        let palette = self.palette;
        let muted = icons::page_muted(&palette);
        let colors = CardColors {
            ink: palette.ink,
            muted,
            link: palette.link,
        };
        let fill = mix(palette.page, palette.note_marker, FILL_MIX);
        let border = mix(palette.page, palette.note_marker, BORDER_MIX);
        let clip = room.rect.intersect(ui.clip_rect());
        let painter = ui.painter_at(pane);
        let mut drawn = Vec::new();
        let mut measured: Vec<(String, f32)> = Vec::new();
        let mut actions: Vec<(String, CardAction)> = Vec::new();
        let mut over_card = false;
        for ((id, marker, body, notebook), top) in notes.iter().zip(&tops) {
            let editing = (self.notes_ui.editing.as_ref())
                .is_some_and(|e| e.id == *id && e.place == EditPlace::Margin(tab));
            let open = editing || expanded.contains(id);
            let at = Rect::from_min_size(
                pos2(room.rect.left(), *top),
                vec2(room.rect.width(), f32::INFINITY),
            );
            if at.top() > room.rect.bottom() {
                // Below the pane: nothing more to draw, and its height is kept from before.
                continue;
            }
            let mut card = ui.new_child(UiBuilder::new().max_rect(at).id_salt((
                "note-margin-card",
                tab,
                id.as_str(),
            )));
            card.set_clip_rect(clip);
            card.style_mut().interaction.selectable_labels = false;
            let mut body_rect = Rect::NOTHING;
            let frame = crate::components::card_frame()
                .fill(fill)
                .stroke(Stroke::new(stroke::HAIR, border))
                .inner_margin(inset::POPOVER)
                .show(&mut card, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.add(icons::image(ui.ctx(), Icon::Note, icons::SMALL, muted));
                        ui.label(RichText::new(notebook).small().color(muted));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if editing {
                                return;
                            }
                            let (icon, name) = if open {
                                (Icon::ChevronUp, "Collapse Note")
                            } else {
                                (Icon::ChevronDown, "Expand Note")
                            };
                            let r = icons::button(ui, icon, icons::SMALL, muted, name)
                                .on_hover_text(name);
                            if r.clicked() {
                                let action = if open {
                                    CardAction::Collapse
                                } else {
                                    CardAction::Expand
                                };
                                actions.push((id.clone(), action));
                            }
                        });
                    });
                    if editing {
                        body_rect = self.margin_editor(ui, id);
                        return;
                    }
                    let top = ui.cursor().top();
                    card_lines(ui, body, open, colors);
                    body_rect = Rect::from_min_max(
                        pos2(ui.min_rect().left(), top),
                        pos2(ui.min_rect().right(), ui.cursor().top()),
                    );
                });
            let rect = frame.response.rect;
            measured.push((id.clone(), rect.height()));
            if !editing && body_rect.is_positive() {
                let what = if open {
                    "Click to edit this Note"
                } else {
                    "Click to show the whole Note"
                };
                let r = card
                    .interact(body_rect, card.scope_id().with("body"), Sense::click())
                    .on_hover_text(what);
                let first = body.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
                r.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Note: {first}"))
                });
                let r = if open {
                    r.on_hover_cursor(egui::CursorIcon::Text)
                } else {
                    r
                };
                if r.clicked() {
                    let action = if open {
                        CardAction::Edit
                    } else {
                        CardAction::Expand
                    };
                    actions.push((id.clone(), action));
                }
            }
            let hovered = ui.rect_contains_pointer(rect.intersect(clip));
            over_card |= hovered;
            // The leader: from the text's edge on the marker's line, across the gutter, to
            // the card beside its first text row.
            let y = marker.center().y;
            let out_of_text = if room.rtl { -space::XS } else { space::XS };
            let card_edge = if room.rtl { rect.right() } else { rect.left() };
            let start = pos2(room.text_edge + out_of_text, y);
            let bend = pos2((start.x + card_edge) / 2.0, y);
            let end = pos2(
                card_edge,
                (rect.top() + LIFT + marker.height() / 2.0).max(y),
            );
            let leader = [start, bend, end];
            let line = if hovered {
                Stroke::new(
                    stroke::MEDIUM,
                    palette.note_marker.gamma_multiply(LEADER_HOVER),
                )
            } else {
                Stroke::new(stroke::HAIR, palette.note_marker.gamma_multiply(LEADER))
            };
            painter.line(leader.to_vec(), line);
            if hovered {
                painter.rect_stroke(
                    marker.expand(space::XXS),
                    radius::XS,
                    Stroke::new(stroke::HAIR, palette.note_marker),
                    egui::StrokeKind::Outside,
                );
            }
            drawn.push(DrawnCard {
                id: id.clone(),
                rect,
                marker: *marker,
                leader,
                expanded: open,
                editing,
            });
        }
        // The wheel over a card scrolls the text, as it does over the rest of the pane.
        let pane_hovered = out.response.as_ref().is_some_and(|r| r.hovered());
        if over_card && !pane_hovered {
            let dy = ui.input(|i| i.smooth_scroll_delta.y);
            if dy != 0.0
                && let Some(view) = self.tabs.get_mut(&tab).and_then(|t| t.view.as_mut())
            {
                view.scroll_by(-dy);
                ui.ctx().request_repaint();
            }
        }
        let Some(t) = self.tabs.get_mut(&tab) else {
            return;
        };
        for (id, h) in measured {
            let old = t.margin.heights.insert(id, h);
            if old.is_none_or(|o| (o - h).abs() > space::PX / 2.0) {
                ui.ctx().request_repaint();
            }
        }
        t.margin.drawn = drawn;
        for (id, action) in actions {
            match action {
                CardAction::Expand => {
                    t.margin.expanded.insert(id);
                }
                CardAction::Collapse => {
                    t.margin.expanded.remove(&id);
                }
                CardAction::Edit => {
                    let body = notes
                        .iter()
                        .find(|n| n.0 == id)
                        .map(|n| n.2.clone())
                        .unwrap_or_default();
                    self.start_card_edit(&id, &body, EditPlace::Margin(tab));
                    // Its editor draws from next frame: the pass goes on until then.
                    self.notes_ui.margin_shown = true;
                    return;
                }
            }
        }
    }

    /// The Note editor in a margin card for the Note being edited (`id`): it saves as you
    /// type (the Shell's autosave), and when the keys leave it or on Ctrl+Enter.
    fn margin_editor(&mut self, ui: &mut Ui, id: &str) -> Rect {
        let now = self.now;
        self.notes_ui.margin_shown = true;
        let Some(ed) = self.notes_ui.editing.as_mut() else {
            return Rect::NOTHING;
        };
        let out =
            super::note_editor::note_editor(ui, card_editor_id(id).with("margin"), &mut ed.text, 4);
        let r = out.response;
        if ed.focus {
            r.request_focus();
            ed.focus = false;
        }
        if r.changed() {
            ed.changed_at = now;
        }
        let mut finished = out.left;
        if r.has_focus()
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter))
        {
            finished = true;
        }
        if let Some(e) = &ed.error {
            ui.label(
                RichText::new(format!("Not saved: {e}. Trying again…"))
                    .small()
                    .color(crate::theme::c().warn),
            );
        }
        if finished {
            self.save_card_edit(true);
        }
        r.rect
    }
}

/// How one byte of a card's line is drawn.
#[derive(Clone, Copy, PartialEq)]
struct Look {
    bold: bool,
    italics: bool,
    link: bool,
}

/// A card's text: the Note's lines with their Note formatting (#186): a heading in the
/// display face, a list item after its bullet or number, a quote in quotation marks, bold,
/// italics, and links and Bible references in the link colour. Collapsed, only its first
/// [`COLLAPSED_ROWS`] rows, with "…" where it is cut.
fn card_lines(ui: &mut Ui, body: &str, open: bool, colors: CardColors) {
    let lines = obs_engine::note_body(body);
    let mut rows = if open { usize::MAX } else { COLLAPSED_ROWS };
    let mut shown = false;
    let size = ui.style().text_styles[&egui::TextStyle::Body].size;
    for bl in lines
        .iter()
        .skip_while(|l| l.line.kind == NoteLineKind::Blank)
    {
        let line = &bl.line;
        if rows == 0 {
            ui.label(RichText::new("…").color(colors.muted));
            break;
        }
        let indent = |level: u8| "    ".repeat(usize::from(level));
        let (prefix, suffix, heading) = match line.kind {
            NoteLineKind::Blank => {
                ui.add_space(space::XS);
                continue;
            }
            NoteLineKind::Heading(_) => (String::new(), "", true),
            NoteLineKind::Bullet { level } => (format!("{}• ", indent(level)), "", false),
            NoteLineKind::Numbered { level, number } => {
                (format!("{}{number}. ", indent(level)), "", false)
            }
            NoteLineKind::Quote => ("“".to_string(), "”", false),
            NoteLineKind::Text => (String::new(), "", false),
        };
        shown = true;
        let plain = Look {
            bold: heading,
            italics: false,
            link: false,
        };
        let mut looks = vec![plain; line.text.len()];
        for m in &line.marks {
            for l in looks.get_mut(m.range.clone()).into_iter().flatten() {
                match m.kind {
                    obs_model::NoteMarkKind::Bold => l.bold = true,
                    obs_model::NoteMarkKind::Italic => l.italics = true,
                    obs_model::NoteMarkKind::Link(_) => l.link = true,
                }
            }
        }
        for (r, _) in &bl.references {
            for l in looks.get_mut(r.clone()).into_iter().flatten() {
                l.link = true;
            }
        }
        let format = |look: Look| {
            let font = if look.bold {
                crate::theme::display(size)
            } else {
                FontId::proportional(size)
            };
            egui::TextFormat {
                font_id: font,
                color: if look.link { colors.link } else { colors.ink },
                italics: look.italics,
                underline: if look.link {
                    Stroke::new(stroke::HAIR, colors.link.gamma_multiply(alpha::SOFT))
                } else {
                    Stroke::NONE
                },
                ..Default::default()
            }
        };
        let mut job = LayoutJob::default();
        job.append(&prefix, 0.0, format(plain));
        let mut at = 0;
        while at < line.text.len() {
            let look = looks[at];
            let mut end = at + 1;
            while end < line.text.len() && (looks[end] == look || !line.text.is_char_boundary(end))
            {
                end += 1;
            }
            job.append(&line.text[at..end], 0.0, format(look));
            at = end;
        }
        job.append(suffix, 0.0, format(plain));
        job.wrap = TextWrapping {
            max_width: ui.available_width(),
            max_rows: rows,
            break_anywhere: false,
            overflow_character: Some('…'),
        };
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        rows = rows.saturating_sub(galley.rows.len());
        let elided = galley.elided;
        ui.add(egui::Label::new(galley));
        if elided {
            // Cut mid-line: its "…" says there is more.
            break;
        }
    }
    if !shown {
        ui.label(
            RichText::new("Empty Note")
                .italics()
                .font(FontId::proportional(text::SMALL))
                .color(colors.muted),
        );
    }
}

/// A card's colours, from the Page tone.
#[derive(Clone, Copy)]
struct CardColors {
    ink: Color32,
    muted: Color32,
    link: Color32,
}

/// `a` with `t` of `b` mixed in.
fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}
