//! The Exegetical Guide's sections (#144): drawing the engine's pure data
//! ([`obs_engine::WordByWord`], [`obs_engine::LexiconSummary`], [`obs_engine::GrammarNote`]).
//! Original-language text goes through the text pipeline (`State::guide_snippet`), as in Key
//! words. A word or a Lexicon summary opens a Word Study at its Strong's number; a summary's
//! Lexicon name opens the Lexicon at the entry. Ctrl/Cmd opens a new Tab.

use egui::{FontId, Layout, Rect, RichText, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use obs_engine::{GrammarNote, GuideSection, LexiconSummary, TabId, WordByWord};
use obs_layout::ColorRole;
use obs_scripture::{Passage, RefStyle, Standard, format_passage};

use super::State;
use super::guide_tab::Action;
use super::guide_words::{ROWS_SHOWN, WordsAction};
use crate::theme;

/// Width of the column holding the word as written (and its Lemma under it).
const WORD_W: f32 = 130.0;

impl State {
    /// Word by word: each Verse's reference in the Bible read (with its Standard numbering
    /// when the Bible numbers it differently), then one two-line row per word: the word,
    /// transliteration and Gloss; its Lemma, Strong's number and Morphology in plain English.
    pub(super) fn word_by_word_section(
        &mut self,
        ui: &mut Ui,
        tab: TabId,
        w: &WordByWord,
        new_tab: bool,
    ) -> Option<Action> {
        let mut action = None;
        let settings = self.session.text;
        let row_h = (settings.size * settings.line_spacing).max(22.0) + 2.0;
        let c = theme::c();
        for verse in &w.verses {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(verse.reference()).strong().color(c.lapis));
                if verse.renumbered() {
                    let standard =
                        format_passage(Passage::verse(verse.standard), &Standard, RefStyle::Short);
                    ui.label(
                        RichText::new(format!("{standard} in English Bibles"))
                            .small()
                            .color(c.muted),
                    )
                    .on_hover_text(
                        "This Bible numbers the verse differently; its Verse map leads here \
                         from the Standard versification.",
                    );
                }
            });
            for (i, word) in verse.words.iter().enumerate() {
                let width = ui.available_width();
                let (rect, _) = ui.allocate_exact_size(vec2(width, row_h * 2.0), Sense::hover());
                let top = Rect::from_min_size(rect.min, vec2(width, row_h));
                let bottom =
                    Rect::from_min_size(pos2(rect.left(), top.bottom()), vec2(width, row_h));
                let column = WORD_W.min(width * 0.4);
                let centered = Layout::left_to_right(egui::Align::Center);
                let word_rect = Rect::from_min_size(top.min, vec2(column, row_h));
                ui.scope_builder(
                    UiBuilder::new().max_rect(word_rect).layout(centered),
                    |ui| self.guide_snippet(ui, &word.written, ColorRole::Ink, word.rtl, 1),
                );
                let middle = Rect::from_min_max(
                    pos2(word_rect.right() + 8.0, top.top()),
                    pos2(top.right() - 8.0, top.bottom()),
                );
                if middle.width() > 16.0 {
                    ui.scope_builder(UiBuilder::new().max_rect(middle).layout(centered), |ui| {
                        ui.set_clip_rect(middle.intersect(ui.clip_rect()));
                        if let Some(t) = &word.transliteration {
                            ui.add(
                                egui::Label::new(RichText::new(t).italics().color(c.ink))
                                    .selectable(false),
                            );
                        }
                        if let Some(g) = &word.gloss {
                            ui.add(
                                egui::Label::new(RichText::new(g).color(c.ink))
                                    .truncate()
                                    .selectable(false),
                            );
                        }
                    });
                }
                let lemma_rect = Rect::from_min_size(
                    pos2(bottom.left() + 14.0, bottom.top()),
                    vec2((column - 14.0).max(20.0), row_h),
                );
                ui.scope_builder(
                    UiBuilder::new().max_rect(lemma_rect).layout(centered),
                    |ui| match &word.lemma {
                        Some(l) => self.guide_snippet(ui, l, ColorRole::Accent, word.rtl, 1),
                        None => {
                            ui.add(
                                egui::Label::new(RichText::new("—").color(c.muted))
                                    .selectable(false),
                            );
                        }
                    },
                );
                let grammar = Rect::from_min_max(
                    pos2(lemma_rect.right() + 8.0, bottom.top()),
                    pos2(bottom.right() - 8.0, bottom.bottom()),
                );
                if grammar.width() > 16.0 {
                    ui.scope_builder(UiBuilder::new().max_rect(grammar).layout(centered), |ui| {
                        ui.set_clip_rect(grammar.intersect(ui.clip_rect()));
                        let mut line = word.strongs.to_string();
                        if let Some(m) = &word.morphology {
                            line.push_str(" · ");
                            line.push_str(m);
                        }
                        ui.add(
                            egui::Label::new(RichText::new(line).small().color(c.muted))
                                .truncate()
                                .selectable(false),
                        );
                    });
                }
                let r = ui
                    .interact(
                        rect,
                        ui.scope_id()
                            .with(("exegetical-word", tab, &verse.native, i)),
                        Sense::click(),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                let label = format!("Word Study on {} ({})", word.strongs, word.written);
                r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Link, true, &label));
                if r.hovered() {
                    ui.painter().rect_stroke(
                        rect.expand(2.0),
                        4.0,
                        Stroke::new(1.0, c.hover_rule),
                        egui::StrokeKind::Outside,
                    );
                }
                let r = r.on_hover_ui(|ui| {
                    ui.label(RichText::new(word.strongs.to_string()).strong());
                    if let Some(m) = &word.morphology {
                        ui.label(m);
                    }
                    if let Some(code) = &word.morph_code {
                        ui.label(
                            RichText::new(format!("Code: {code}"))
                                .small()
                                .color(theme::c().muted),
                        );
                    }
                    ui.label(
                        RichText::new("Click for a Word Study · Ctrl/Cmd-click opens a new Tab")
                            .small()
                            .color(theme::c().muted),
                    );
                });
                if r.clicked() {
                    action = Some(Action::Words(WordsAction::WordStudy(word.strongs, new_tab)));
                }
            }
            ui.add_space(6.0);
        }
        action
    }

    /// Lexicon summaries: Lemma, transliteration and Strong's number, the Lexicon and its
    /// short entry, in order of first use; the first 12 with Show all.
    pub(super) fn lexicon_summaries_section(
        &mut self,
        ui: &mut Ui,
        tab: TabId,
        rows: &[LexiconSummary],
        new_tab: bool,
    ) -> Option<Action> {
        let mut action = None;
        let shown = if self.expanded(tab, GuideSection::LexiconSummaries) {
            rows.len()
        } else {
            rows.len().min(ROWS_SHOWN)
        };
        let c = theme::c();
        for row in &rows[..shown] {
            let settings = self.session.text;
            let row_h = (settings.size * settings.line_spacing).max(22.0) + 2.0;
            let width = ui.available_width();
            let (rect, _) = ui.allocate_exact_size(vec2(width, row_h), Sense::hover());
            let column = WORD_W.min(width * 0.4);
            let centered = Layout::left_to_right(egui::Align::Center);
            let lemma_rect = Rect::from_min_size(rect.min, vec2(column, row_h));
            ui.scope_builder(
                UiBuilder::new().max_rect(lemma_rect).layout(centered),
                |ui| match &row.lemma {
                    Some(l) => self.guide_snippet(ui, l, ColorRole::Ink, row.rtl(), 1),
                    None => {
                        ui.add(
                            egui::Label::new(RichText::new(row.strongs.to_string()).color(c.ink))
                                .selectable(false),
                        );
                    }
                },
            );
            let middle = Rect::from_min_max(
                pos2(lemma_rect.right() + 8.0, rect.top()),
                pos2(rect.right() - 8.0, rect.bottom()),
            );
            ui.scope_builder(UiBuilder::new().max_rect(middle).layout(centered), |ui| {
                ui.set_clip_rect(middle.intersect(ui.clip_rect()));
                if let Some(t) = &row.transliteration {
                    ui.add(
                        egui::Label::new(RichText::new(t).italics().color(c.ink)).selectable(false),
                    );
                }
                let study = ui
                    .link(RichText::new(row.strongs.to_string()).small())
                    .on_hover_text("Word Study on this word · Ctrl/Cmd-click opens a new Tab");
                if study.clicked() {
                    action = Some(Action::Words(WordsAction::WordStudy(row.strongs, new_tab)));
                }
                let lexicon = ui
                    .link(RichText::new(&row.entry.abbreviation).small())
                    .on_hover_text(format!(
                        "Open {} at this entry · Ctrl/Cmd-click opens a new Tab",
                        row.entry.title
                    ));
                if lexicon.clicked() {
                    action = Some(Action::OpenLexicon(row.entry.clone(), new_tab));
                }
                if row.in_passage > 1 {
                    ui.label(
                        RichText::new(format!("{}× here", row.in_passage))
                            .small()
                            .color(c.muted),
                    );
                }
            });
            let rtl = row.entry.direction == obs_model::Direction::Rtl;
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                ui.vertical(|ui| self.guide_snippet(ui, &row.summary, ColorRole::Ink, rtl, 3));
            });
            ui.add_space(6.0);
        }
        self.show_all(ui, tab, GuideSection::LexiconSummaries, rows.len());
        action
    }

    /// Grammatical notes: each term with what kind of term it is, its explanation and the
    /// words that use it.
    pub(super) fn grammatical_notes_section(&mut self, ui: &mut Ui, notes: &[GrammarNote]) {
        let c = theme::c();
        for note in notes {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(note.term.name).strong().color(c.ink));
                ui.label(RichText::new(note.label).small().color(c.muted));
            });
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(note.explanation).color(c.ink));
                    let words = note.words.join("  ");
                    let rtl = note.term.language != obs_model::MorphLanguage::Greek;
                    let font = FontId::proportional(11.5);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("In:").font(font).color(c.muted));
                        ui.vertical(|ui| self.guide_snippet(ui, &words, ColorRole::Ink, rtl, 2));
                    });
                });
            });
            ui.add_space(6.0);
        }
    }
}
