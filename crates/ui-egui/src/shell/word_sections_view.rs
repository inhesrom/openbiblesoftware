//! The Word Study's Translated as, Occurrences and Forms sections (#56; decision #15). The
//! data is the engine's ([`obs_engine::word_sections`]); this file only draws it.
//!
//! - **Translated as:** each rendering with its count; a click opens Search for the Verses
//!   with that rendering (`search.run`, `strong:G5485 "favour"`).
//! - **Occurrences:** a bar per Bible book, then the first [`OCCURRENCE_ROWS`] Verses, each
//!   with an English preview through the inline-snippet pipeline and the word highlighted;
//!   "Show all in Search" runs `strong:G5485`. A click opens the Verse in its Bible.
//! - **Forms:** each form with its count; a click filters Occurrences (again: clears it).
//!   The filter is per Tab and resets when the Word Study moves to another number.

use egui::{Rect, RichText, Sense, Ui, pos2, vec2};
use obs_engine::{Forms, OCCURRENCE_ROWS, OccurrenceList, TabId, TranslatedAs};
use obs_layout::{ColorRole, TextSettings};
use obs_model::Key;
use obs_scripture::{Passage, RefStyle, Standard, Verse, format_passage};

use super::State;
use super::guide_tab::Action;
use crate::compare_view::range_rects;
use crate::theme;
use crate::tokens::{alpha, chart, radius, scale, size, space, text};

/// Renderings shown before Show all.
const RENDERINGS_SHOWN: usize = 16;
/// Width of a book's label in the bar list.
const BOOK_LABEL: f32 = chart::CELL.x;
/// Width of a count beside its bar.
const COUNT_WIDTH: f32 = size::TEXT_MIN;
/// Height of one bar row.
const BAR_ROW: f32 = chart::ROW;
/// Lines of a Verse preview.
const PREVIEW_LINES: usize = 4;

fn verse_reference(v: Verse) -> String {
    format_passage(Passage::verse(v), &Standard, RefStyle::Short)
}

impl State {
    /// Translated as: the renderings as links, most frequent first.
    pub(super) fn translated_as_section(
        &mut self,
        ui: &mut Ui,
        tab: TabId,
        t: &TranslatedAs,
        number: obs_scripture::StrongsNumber,
        new_tab: bool,
    ) -> Option<Action> {
        let all = self.tabs.get(&tab).is_some_and(|t| t.renderings_all);
        let mut action = None;
        let counted = t
            .counted
            .filter(|c| *c != number)
            .map(|c| format!(" (as {c})"))
            .unwrap_or_default();
        ui.label(crate::components::hint_text(format!(
            "How the {}{counted} translates it, {} times:",
            t.bible_abbreviation,
            t.total()
        )));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = space::L;
            let shown = if all {
                t.renderings.len()
            } else {
                RENDERINGS_SHOWN
            };
            for r in t.renderings.iter().take(shown) {
                let link = crate::components::link(
                    ui,
                    RichText::new(format!("{} ×{}", r.rendering, r.count)).color(theme::c().lapis),
                )
                .on_hover_text(format!(
                    "Search the {} for {} translated “{}” · Ctrl/Cmd-click: a new Search Tab",
                    t.bible_abbreviation,
                    t.counted.unwrap_or(number),
                    r.rendering
                ));
                if link.clicked()
                    && let Some(text) = t.search_query(&r.rendering)
                {
                    action = Some(Action::Search {
                        text,
                        bibles: t.bible_id.clone(),
                        new_tab,
                    });
                }
            }
            if t.renderings.len() > RENDERINGS_SHOWN {
                let label = if all {
                    "Show fewer".to_string()
                } else {
                    format!("Show all {}", t.renderings.len())
                };
                if ui.small_button(label).clicked() {
                    action = Some(Action::ShowAllRenderings(!all));
                }
            }
        });
        action
    }

    /// Occurrences: per-book bars, then the first Verses with their previews, as the Tab's
    /// form filter leaves them.
    pub(super) fn occurrences_section(
        &mut self,
        ui: &mut Ui,
        tab: TabId,
        o: &OccurrenceList,
        number: obs_scripture::StrongsNumber,
        new_tab: bool,
    ) -> Option<Action> {
        let form = self.tabs.get(&tab).and_then(|t| t.occurrence_form.clone());
        let filtered = o.filtered(form.as_deref());
        let mut action = None;
        let verses_word = |n: usize| if n == 1 { "Verse" } else { "Verses" };
        let counted = o
            .counted
            .filter(|c| *c != number)
            .map(|c| format!(" (as {c})"))
            .unwrap_or_default();
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!(
                    "{} in {} {} of the {}{counted}",
                    if filtered.count == 1 {
                        "1 occurrence".to_string()
                    } else {
                        format!("{} occurrences", filtered.count)
                    },
                    filtered.verses.len(),
                    verses_word(filtered.verses.len()),
                    o.bible_abbreviation
                ))
                .color(theme::c().muted),
            );
            if let Some(f) = &form {
                ui.label(
                    RichText::new(format!("Form: {f}"))
                        .strong()
                        .color(theme::c().ink),
                );
                if ui
                    .small_button("Clear")
                    .on_hover_text("Show every form again")
                    .clicked()
                {
                    action = Some(Action::FilterForm(None));
                }
            }
        });
        ui.add_space(space::XS);
        // One bar per Bible book, scaled to the largest.
        let max = filtered
            .books
            .iter()
            .map(|(_, n)| *n)
            .max()
            .unwrap_or(1)
            .max(1);
        let c = theme::c();
        for (book, n) in &filtered.books {
            let width = ui.available_width();
            let (rect, response) = ui.allocate_exact_size(vec2(width, BAR_ROW), Sense::hover());
            let painter = ui.painter_at(rect);
            painter.text(
                pos2(rect.left(), rect.center().y),
                egui::Align2::LEFT_CENTER,
                book.abbrev(),
                egui::FontId::proportional(text::LABEL),
                c.ink,
            );
            let track = (width - BOOK_LABEL - COUNT_WIDTH).max(space::XXL);
            let bar = track * (*n as f32 / max as f32);
            painter.rect_filled(
                Rect::from_min_size(
                    pos2(rect.left() + BOOK_LABEL, rect.top() + chart::BAR_INSET),
                    vec2(bar.max(space::XXS), BAR_ROW - 2.0 * chart::BAR_INSET),
                ),
                radius::XS,
                c.lapis.gamma_multiply(alpha::STRONG),
            );
            painter.text(
                pos2(rect.left() + BOOK_LABEL + bar + space::S, rect.center().y),
                egui::Align2::LEFT_CENTER,
                n.to_string(),
                egui::FontId::proportional(text::SMALL),
                c.muted,
            );
            let label = format!("{}: {n}", book.name());
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &label));
        }
        ui.add_space(space::M);
        // The first Verses, each with its preview.
        let shown: Vec<&obs_engine::OccurrenceVerse> = filtered
            .verses
            .iter()
            .take(OCCURRENCE_ROWS)
            .copied()
            .collect();
        let previews = match &o.preview {
            Some(bible) => {
                let verses: Vec<Verse> = shown
                    .iter()
                    .filter(|r| !r.unmapped)
                    .map(|r| r.verse)
                    .collect();
                let got = self.session.occurrence_previews(bible, &verses);
                verses.into_iter().zip(got).collect::<Vec<_>>()
            }
            None => Vec::new(),
        };
        let rtl = o.preview.as_ref().is_some_and(|p| p.rtl);
        let preview_abbreviation = o
            .preview
            .as_ref()
            .map_or(o.bible_abbreviation.as_str(), |p| p.abbreviation.as_str());
        for row in shown {
            let top = ui.cursor().min;
            let preview = previews
                .iter()
                .find(|(v, _)| !row.unmapped && *v == row.verse)
                .and_then(|(_, p)| p.clone());
            let reference = if row.unmapped {
                format!("{} ({})", verse_reference(row.native), o.bible_abbreviation)
            } else {
                verse_reference(row.verse)
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new(&reference).strong().color(c.lapis));
                if row.forms.len() > 1 {
                    ui.label(
                        RichText::new(format!("×{}", row.forms.len()))
                            .small()
                            .color(c.muted),
                    );
                }
            });
            match &preview {
                Some(p) if !p.text.is_empty() => {
                    self.highlighted_snippet(ui, &p.text, &p.highlights, rtl, PREVIEW_LINES);
                }
                Some(p) if p.error.is_some() => {
                    let e = p.error.as_deref().unwrap_or_default();
                    ui.label(
                        RichText::new(format!("Could not read the {preview_abbreviation}: {e}"))
                            .small()
                            .color(c.warn),
                    );
                }
                Some(_) => {
                    ui.label(
                        RichText::new(format!("Not in the {preview_abbreviation}"))
                            .small()
                            .color(c.muted),
                    );
                }
                None if row.unmapped => {}
                None => {
                    self.placeholder_line(ui);
                }
            }
            let rect = Rect::from_min_max(top, pos2(ui.max_rect().right(), ui.cursor().top()));
            let r = ui
                .interact(
                    rect,
                    ui.scope_id()
                        .with(("occurrence", tab, row.verse, row.unmapped)),
                    Sense::click(),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("Click to open this Verse · Ctrl/Cmd-click opens a new Tab");
            let label = format!("Open {reference}");
            r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Link, true, &label));
            if r.hovered() {
                crate::components::hover_outline(ui, rect);
            }
            if r.clicked() {
                // In the preview Bible where it has the Verse, else in the counted Bible.
                let (bible, verse) = match (&o.preview, preview.as_ref().and_then(|p| p.native)) {
                    (Some(b), Some(native)) => (b.id.clone(), native),
                    _ => (o.bible_id.clone(), row.native),
                };
                action = Some(Action::OpenVerse {
                    bible,
                    verse,
                    new_tab,
                });
            }
            ui.add_space(space::S);
        }
        ui.horizontal_wrapped(|ui| {
            if filtered.verses.len() > OCCURRENCE_ROWS {
                ui.label(
                    RichText::new(format!(
                        "Showing the first {OCCURRENCE_ROWS} of {} Verses.",
                        filtered.verses.len()
                    ))
                    .color(c.muted),
                );
            }
            let hint = if form.is_some() {
                "Every occurrence of every form, in Search · Ctrl/Cmd-click: a new Search Tab"
            } else {
                "Every occurrence, in Search · Ctrl/Cmd-click: a new Search Tab"
            };
            if ui
                .button("Show all in Search")
                .on_hover_text(hint)
                .clicked()
            {
                action = Some(Action::Search {
                    text: o.search_query(number),
                    bibles: o.search_bibles(number),
                    new_tab,
                });
            }
        });
        action
    }

    /// Forms: each form with its count; a click filters Occurrences.
    pub(super) fn forms_section(&mut self, ui: &mut Ui, tab: TabId, f: &Forms) -> Option<Action> {
        let selected = self.tabs.get(&tab).and_then(|t| t.occurrence_form.clone());
        let mut action = None;
        ui.label(crate::components::hint_text(format!(
            "In the {}. Click a form to list only its occurrences.",
            f.bible_abbreviation
        )));
        for form in &f.forms {
            let on = selected.as_deref() == Some(form.label.as_str());
            let text = RichText::new(format!("{} ×{}", form.label, form.count));
            let text = if on {
                text.strong().color(theme::c().ink)
            } else {
                text.color(theme::c().lapis)
            };
            let r = ui.selectable_label(on, text).on_hover_text(format!(
                "{} ({}) · Click to {}",
                form.long,
                form.codes.join(", "),
                if on {
                    "show every form again"
                } else {
                    "list only these occurrences"
                }
            ));
            if r.clicked() {
                action = Some(Action::FilterForm((!on).then(|| form.label.clone())));
            }
        }
        if f.without_form > 0 {
            ui.label(crate::components::hint_text(format!(
                "{} without Morphology",
                f.without_form
            )));
        }
        action
    }

    /// Resource text with highlighted ranges, through the inline-snippet pipeline, at most
    /// `lines` lines (a placeholder bar until a worker has laid it out).
    fn highlighted_snippet(
        &mut self,
        ui: &mut Ui,
        text: &str,
        highlights: &[std::ops::Range<usize>],
        rtl: bool,
        lines: usize,
    ) {
        let settings: TextSettings = self.session.text;
        let width = ui.available_width().max(size::TEXT_MIN);
        let Some(laid) = self
            .snippets
            .get(text, ColorRole::Ink, rtl, width, &settings)
        else {
            self.placeholder_line(ui);
            return;
        };
        let height = lines
            .checked_sub(1)
            .and_then(|i| laid.lines.get(i))
            .map_or(laid.height, |l| l.bottom)
            .min(laid.height);
        let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text));
        if !ui.is_rect_visible(rect) {
            return;
        }
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        painter.rect_filled(rect, 0.0, self.palette.page);
        for r in highlights {
            for [x0, y0, x1, y1] in range_rects(&laid, r) {
                painter.rect_filled(
                    Rect::from_min_max(
                        pos2(rect.left() + x0, rect.top() + y0),
                        pos2(rect.left() + x1, rect.top() + y1),
                    ),
                    radius::XS,
                    self.palette.search_hit,
                );
            }
        }
        self.atlas.paint(&painter, &laid, rect.min, &self.palette);
    }

    /// One line's placeholder while text is read or laid out.
    fn placeholder_line(&mut self, ui: &mut Ui) {
        let settings: TextSettings = self.session.text;
        let width = ui.available_width().max(size::TEXT_MIN);
        let line = settings.size * settings.line_spacing;
        let (rect, _) = ui.allocate_exact_size(vec2(width, line), Sense::hover());
        crate::components::text_placeholder(
            ui.painter(),
            rect.min,
            width.min(settings.size * scale::PLACEHOLDER_EMS),
            line,
            theme::c().rule,
        );
        ui.ctx().request_repaint();
    }

    /// The Strong's number a Word Study section is for.
    pub(super) fn section_number(key: &Key) -> Option<obs_scripture::StrongsNumber> {
        match key {
            Key::StrongsNumber(n) => Some(*n),
            _ => None,
        }
    }
}
