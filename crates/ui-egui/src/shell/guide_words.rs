//! The Passage Guide's Key words and People & places sections (#54): drawing the engine's
//! pure data ([`obs_engine::KeyWords`], [`obs_engine::PersonOrPlace`]) and opening what a
//! row links to. A Key word opens a Word Study at its Strong's number; a Dictionary link
//! opens that Dictionary at the article. Both reuse an open Tab of the kind unless Ctrl/Cmd
//! is held, and leave the Guide and its Link set where they are.

use egui::{FontId, Layout, Rect, RichText, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use obs_engine::{ArticleLink, Guide, GuideSection, KeyWords, PersonOrPlace, TabId, TabKind};
use obs_layout::ColorRole;
use obs_model::Key;
use obs_scripture::{Passage, RefStyle, Standard, StrongsNumber, format_passage};
use serde_json::json;

use super::State;
use crate::theme;

/// Rows shown before "Show all".
pub(super) const ROWS_SHOWN: usize = 12;
/// Width of the Lemma column of a Key word row.
const LEMMA_W: f32 = 112.0;
/// Values of one fact shown before "+N".
const FACT_VALUES: usize = 5;

/// What a click in these sections asks for.
pub(super) enum WordsAction {
    WordStudy(StrongsNumber, bool),
    Article(ArticleLink, bool),
    /// A person or place's Factbook (#149), by People & places id.
    Factbook(String, bool),
}

impl State {
    pub(super) fn expanded(&self, tab: TabId, section: GuideSection) -> bool {
        self.tabs
            .get(&tab)
            .is_some_and(|t| t.guide_expanded.contains(&section))
    }

    /// "Show all N" / "Show fewer" under a long section.
    pub(super) fn show_all(
        &mut self,
        ui: &mut Ui,
        tab: TabId,
        section: GuideSection,
        total: usize,
    ) {
        if total <= ROWS_SHOWN {
            return;
        }
        let expanded = self.expanded(tab, section);
        let label = if expanded {
            "Show fewer".to_string()
        } else {
            format!("Show all {total}")
        };
        if ui.small_button(label).clicked()
            && let Some(t) = self.tabs.get_mut(&tab)
            && !t.guide_expanded.remove(&section)
        {
            t.guide_expanded.insert(section);
        }
    }

    /// Key words: Lemma, transliteration, Gloss, and how often the word is in the Passage
    /// and in the Bible; rarest first. Click: Word Study.
    pub(super) fn key_words_section(
        &mut self,
        ui: &mut Ui,
        tab: TabId,
        k: &KeyWords,
        new_tab: bool,
    ) -> Option<WordsAction> {
        let mut action = None;
        let shown = if self.expanded(tab, GuideSection::KeyWords) {
            k.words.len()
        } else {
            k.words.len().min(ROWS_SHOWN)
        };
        let settings = self.session.text;
        let row_h = (settings.size * settings.line_spacing).max(22.0) + 4.0;
        for w in &k.words[..shown] {
            // One line in fixed columns: Lemma | transliteration and Gloss | counts.
            let width = ui.available_width();
            let (rect, _) = ui.allocate_exact_size(vec2(width, row_h), Sense::hover());
            let c = theme::c();
            let counts = ui.painter().layout_no_wrap(
                format!(
                    "{}× · {} in {}",
                    w.in_passage, w.in_bible, k.bible_abbreviation
                ),
                FontId::proportional(11.5),
                c.muted,
            );
            let counts_at = pos2(
                rect.right() - counts.size().x - 12.0,
                rect.center().y - counts.size().y / 2.0,
            );
            ui.painter().galley(counts_at, counts, c.muted);
            let lemma_rect = Rect::from_min_size(rect.min, vec2(LEMMA_W.min(width * 0.4), row_h));
            let centered = Layout::left_to_right(egui::Align::Center);
            ui.scope_builder(
                UiBuilder::new().max_rect(lemma_rect).layout(centered),
                |ui| match &w.lemma {
                    Some(lemma) => self.guide_snippet(ui, lemma, ColorRole::Ink, w.rtl(), 1),
                    None => {
                        ui.add(
                            egui::Label::new(RichText::new(w.strongs.to_string()).color(c.ink))
                                .selectable(false),
                        );
                    }
                },
            );
            let middle = Rect::from_min_max(
                pos2(lemma_rect.right() + 6.0, rect.top()),
                pos2(counts_at.x - 8.0, rect.bottom()),
            );
            if middle.width() > 16.0 {
                ui.scope_builder(UiBuilder::new().max_rect(middle).layout(centered), |ui| {
                    ui.set_clip_rect(middle.intersect(ui.clip_rect()));
                    if let Some(t) = &w.transliteration {
                        ui.add(
                            egui::Label::new(RichText::new(t).italics().color(c.ink))
                                .selectable(false),
                        );
                    }
                    if let Some(g) = &w.gloss {
                        ui.add(
                            egui::Label::new(RichText::new(g).color(c.muted))
                                .truncate()
                                .selectable(false),
                        );
                    }
                });
            }
            let r = ui
                .interact(
                    rect,
                    ui.scope_id().with(("key-word", tab, w.strongs)),
                    Sense::click(),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            let label = format!("Word Study on {}", w.strongs);
            r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Link, true, &label));
            if r.hovered() {
                ui.painter().rect_stroke(
                    rect.expand(2.0),
                    4.0,
                    Stroke::new(1.0, theme::c().hover_rule),
                    egui::StrokeKind::Outside,
                );
            }
            let r = r.on_hover_ui(|ui| {
                let mut head = w.strongs.to_string();
                if let Some(p) = w.part_of_speech {
                    head.push_str(&format!(" · {p}"));
                }
                ui.label(RichText::new(head).strong());
                if let Some(g) = &w.gloss {
                    ui.label(g);
                }
                ui.label(
                    RichText::new("Click for a Word Study · Ctrl/Cmd-click opens a new Tab")
                        .small()
                        .color(theme::c().muted),
                );
            });
            if r.clicked() {
                action = Some(WordsAction::WordStudy(w.strongs, new_tab));
            }
        }
        self.show_all(ui, tab, GuideSection::KeyWords, k.words.len());
        if k.function_words > 0 {
            ui.label(
                RichText::new(format!(
                    "{} function word{} left out (articles, conjunctions, prepositions, \
                     particles, pronouns)",
                    k.function_words,
                    if k.function_words == 1 { "" } else { "s" }
                ))
                .small()
                .color(theme::c().muted),
            );
        }
        action
    }

    /// People & places: name as the Passage gives it, kind and region, relations, the Verses
    /// that mention them (for a range) and Dictionary article links.
    pub(super) fn people_section(
        &mut self,
        ui: &mut Ui,
        tab: TabId,
        rows: &[PersonOrPlace],
        passage: Passage,
        new_tab: bool,
    ) -> Option<WordsAction> {
        let mut action = None;
        let shown = if self.expanded(tab, GuideSection::PeopleAndPlaces) {
            rows.len()
        } else {
            rows.len().min(ROWS_SHOWN)
        };
        for row in &rows[..shown] {
            ui.horizontal_wrapped(|ui| {
                let usual = row.usual_name.as_deref().unwrap_or(&row.name);
                let name = ui
                    .add(
                        egui::Label::new(RichText::new(&row.name).strong().color(theme::c().lapis))
                            .sense(Sense::click()),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(format!(
                        "Open the Factbook on {usual} · Ctrl/Cmd-click opens a new Tab"
                    ));
                let label = format!("Factbook: {usual}");
                name.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Link, true, &label));
                if name.clicked() {
                    action = Some(WordsAction::Factbook(row.entity.clone(), new_tab));
                }
                if let Some(u) = &row.usual_name {
                    ui.label(RichText::new(format!("({u})")).color(theme::c().ink));
                }
                ui.label(RichText::new(row.summary()).small().color(theme::c().muted));
            });
            if !row.facts.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for (i, f) in row.facts.iter().enumerate() {
                        if i > 0 {
                            ui.label(RichText::new("·").color(theme::c().muted));
                        }
                        ui.label(RichText::new(f.label).small().color(theme::c().muted));
                        let mut values = f
                            .values
                            .iter()
                            .take(FACT_VALUES)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ");
                        if f.values.len() > FACT_VALUES {
                            values.push_str(&format!(" +{}", f.values.len() - FACT_VALUES));
                        }
                        ui.label(RichText::new(values).color(theme::c().ink))
                            .on_hover_text(f.values.join(", "));
                    }
                });
            }
            if !passage.is_single_verse() && !row.verses.is_empty() {
                let verses: Vec<String> = row
                    .verses
                    .iter()
                    .map(|v| format_passage(Passage::verse(*v), &Standard, RefStyle::Short))
                    .collect();
                ui.label(
                    RichText::new(format!("Mentioned in {}", verses.join(", ")))
                        .small()
                        .color(theme::c().muted),
                );
            }
            ui.horizontal_wrapped(|ui| {
                if row.articles.is_empty() {
                    ui.label(
                        RichText::new("No Dictionary article")
                            .small()
                            .color(theme::c().muted),
                    );
                }
                for a in &row.articles {
                    let r = ui
                        .link(format!("{}: {}", a.abbreviation, a.headword))
                        .on_hover_text(format!(
                            "Open {} at “{}” · Ctrl/Cmd-click opens a new Tab",
                            a.title, a.headword
                        ));
                    if r.clicked() {
                        action = Some(WordsAction::Article(a.clone(), new_tab));
                    }
                }
            });
            ui.add_space(6.0);
        }
        self.show_all(ui, tab, GuideSection::PeopleAndPlaces, rows.len());
        action
    }

    pub(super) fn words_action(&mut self, guide: TabId, action: WordsAction) {
        match action {
            WordsAction::WordStudy(n, new_tab) => self.open_word_study(guide, n, new_tab),
            WordsAction::Article(a, new_tab) => self.open_article(guide, &a, new_tab),
            WordsAction::Factbook(entity, new_tab) => {
                self.session.workspace.focus(guide);
                self.run(
                    "guide.factbook",
                    &json!({ "entity": entity, "new_tab": new_tab }),
                );
            }
        }
    }

    /// A Word Study at `number`: the open Word Study Tab moves there (Ctrl/Cmd: a new one).
    fn open_word_study(&mut self, guide: TabId, number: StrongsNumber, new_tab: bool) {
        let kind = TabKind::Guide(Guide::WordStudy);
        let existing = (!new_tab)
            .then(|| {
                let ws = &self.session.workspace;
                ws.tabs().filter(|t| t.kind == kind).map(|t| t.id).max()
            })
            .flatten();
        // Both ways go through the command, so whatever Word Study adds to it applies to a
        // reused Tab too.
        let params = match existing {
            Some(tab) => json!({ "strongs": number.to_string(), "tab": tab.0 }),
            None => {
                self.session.workspace.focus(guide);
                json!({ "strongs": number.to_string() })
            }
        };
        self.run("guide.word_study", &params);
    }

    /// A Dictionary at an article: its open Tab moves there (Ctrl/Cmd: always a new Tab).
    pub(super) fn open_article(&mut self, guide: TabId, a: &ArticleLink, new_tab: bool) {
        let kind = TabKind::Resource(a.resource_id.clone());
        let key = Key::Headword(a.headword.clone());
        let existing = (!new_tab)
            .then(|| {
                let ws = &self.session.workspace;
                ws.tabs().filter(|t| t.kind == kind).map(|t| t.id).max()
            })
            .flatten();
        match existing {
            Some(tab) => {
                self.session.workspace.navigate_entry(tab, key);
                self.session.workspace.focus(tab);
            }
            None => {
                self.session.workspace.focus(guide);
                if let Some(out) = self.run("resource.open", &json!({ "id": a.resource_id }))
                    && let Some(tab) = out["tab"].as_u64()
                {
                    // A new Tab opens at its entry history's current Key.
                    self.session.workspace.navigate_entry(TabId(tab), key);
                }
            }
        }
    }
}
