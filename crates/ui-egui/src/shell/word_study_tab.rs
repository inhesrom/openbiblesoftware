//! The Word Study Tab (#55; decision #15): the header (Lemma, transliteration, Strong's
//! number, Gloss, part of speech, total occurrences), then the Guide's sections through the
//! shared [`State::guide_page`]. English words typed into the box show the chooser instead,
//! until a Strong's number is chosen. Everything shown comes from the engine as pure data
//! computed on the job queue; this file only draws it.

use egui::{Rect, RichText, Sense, Shape, Stroke, Ui, pos2, vec2};
use obs_engine::{
    Chooser, Effect, Guide, LexiconEntry, TabId, TabKind, WordStatus, WordStudyHeader,
};
use obs_layout::{ColorRole, TextSettings};
use obs_model::{Direction, Key};
use obs_scripture::{StrongsLanguage, StrongsNumber};
use serde_json::json;

use super::State;
use super::guide_tab::{Action, hint_page};
use crate::theme;

/// Width of the chooser's Lemma column.
const LEMMA_WIDTH: f32 = 120.0;

/// Most lines of one Lexicon entry paragraph.
const ENTRY_LINES: usize = 400;

impl State {
    pub(super) fn word_study_body(&mut self, ui: &mut Ui, tab: TabId) {
        if let Some(words) = self.session.word_study.choosing(tab).map(str::to_string) {
            self.chooser_page(ui, tab, &words);
            return;
        }
        let key = self
            .session
            .workspace
            .tab(tab)
            .and_then(|t| t.entry_history.current());
        let Some(Key::StrongsNumber(number)) = key else {
            hint_page(
                ui,
                "Type a Strong's number (G5485, H1254A) or an English word (love) in the box \
                 above, or click Word Study › in Word Info.",
            );
            return;
        };
        if let Some(t) = self.tabs.get_mut(&tab)
            && t.lexicon_entries_for != Some(number)
        {
            t.lexicon_entries_for = Some(number);
            t.lexicon_open.clear();
            t.occurrence_form = None;
            t.renderings_all = false;
        }
        let status = self.session.word_study_header(number);
        self.guide_page(
            ui,
            tab,
            Guide::WordStudy,
            &Key::StrongsNumber(number),
            &mut |st, ui| {
                st.word_study_header(ui, number, &status);
                None
            },
        );
    }

    /// The header: Strong's number and part of speech, the Lemma (through the pipeline), its
    /// transliteration and Gloss, and the total occurrences.
    fn word_study_header(
        &mut self,
        ui: &mut Ui,
        number: StrongsNumber,
        status: &WordStatus<WordStudyHeader>,
    ) {
        let h = match status {
            WordStatus::Ready(h) => h,
            WordStatus::Loading => {
                strongs_line(ui, number, None);
                ui.label(RichText::new("Loading…").color(theme::c().muted));
                ui.ctx().request_repaint();
                return;
            }
            WordStatus::Failed(e) => {
                strongs_line(ui, number, None);
                ui.label(RichText::new(format!("Could not load: {e}")).color(theme::c().warn));
                return;
            }
        };
        strongs_line(ui, number, h.part_of_speech.as_deref());
        if let Some(lemma) = &h.lemma {
            let settings = self.session.text;
            let big = TextSettings {
                size: settings.size * 1.9,
                ..settings
            };
            let rtl = number.language() == StrongsLanguage::Hebrew;
            let width = ui.available_width();
            self.snippet(ui, lemma, rtl, width, &big);
        }
        ui.horizontal_wrapped(|ui| {
            if let Some(t) = &h.transliteration {
                ui.label(RichText::new(t).italics().color(theme::c().ink));
            }
            if let Some(g) = &h.gloss {
                ui.label(RichText::new(format!("“{}”", g.text)).color(theme::c().ink));
                ui.label(
                    RichText::new(format!("— {}", g.abbreviation))
                        .small()
                        .color(theme::c().muted),
                );
            }
        });
        let occurrences = match &h.occurrences {
            Some(o) => {
                let times = if o.count == 1 {
                    "1 occurrence".to_string()
                } else {
                    format!("{} occurrences", o.count)
                };
                let base = if o.counted == number {
                    String::new()
                } else {
                    format!(" (as {})", o.counted)
                };
                format!("{times} in {}{base}", o.abbreviation)
            }
            None => "No occurrences in a Strong's-tagged Bible in your library".into(),
        };
        ui.label(RichText::new(occurrences).color(theme::c().muted));
    }

    /// The English-word chooser: the Strong's numbers a tagged English Bible renders with the
    /// typed words, most frequent first. Click one to study it (Ctrl/Cmd-click: a new Word
    /// Study); Cancel or Escape goes back to the number shown before.
    fn chooser_page(&mut self, ui: &mut Ui, tab: TabId, words: &str) {
        let status = self.session.word_chooser(words);
        let can_cancel = self
            .session
            .workspace
            .tab(tab)
            .is_some_and(|t| t.entry_history.current().is_some());
        let new_tab = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
        let mut action: Option<Action> = None;
        egui::Frame::new()
            .fill(theme::c().field)
            .inner_margin(egui::Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("“{words}”"))
                            .font(theme::display(20.0))
                            .color(theme::c().ink),
                    );
                    if can_cancel
                        && ui
                            .small_button("Cancel")
                            .on_hover_text("Back to the Strong's number shown before (Esc)")
                            .clicked()
                    {
                        action = Some(Action::StopChoosing);
                    }
                });
                let c = match &status {
                    WordStatus::Loading => {
                        ui.label(
                            RichText::new(format!("Looking up “{words}”…")).color(theme::c().muted),
                        );
                        ui.ctx().request_repaint();
                        return;
                    }
                    WordStatus::Failed(e) => {
                        ui.label(
                            RichText::new(format!("Could not look up “{words}”: {e}"))
                                .color(theme::c().warn),
                        );
                        return;
                    }
                    WordStatus::Ready(c) => c,
                };
                if let Some(a) = self.chooser_rows(ui, c, new_tab) {
                    action = Some(a);
                }
            });
        if can_cancel && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            action = Some(Action::StopChoosing);
        }
        if let Some(a) = action {
            self.session.workspace.user_focused(tab);
            self.word_study_action(tab, a);
        }
    }

    fn chooser_rows(&mut self, ui: &mut Ui, c: &Chooser, new_tab: bool) -> Option<Action> {
        let Some((_, bible)) = &c.bible else {
            let hint = if c.has_tagged_bible {
                format!(
                    "No Strong's number is translated “{}” in your Strong's-tagged Bibles. \
                     Try another form of the word (love, loved, loveth), or a Strong's number.",
                    c.words
                )
            } else {
                // The Bible Translated as reads is what the chooser needs too (#135).
                let missing = obs_engine::GuideSection::TranslatedAs.missing_resource(
                    false,
                    None,
                    &self.session.library,
                    self.session.starter.manifest(),
                );
                return self
                    .missing_resource_ui(
                        ui,
                        "No English Bible in your library has Strong's numbers. Add one (the \
                         KJV with Strong's) to look up English words, or type a Strong's number.",
                        &missing,
                    )
                    .map(|(id, params)| Action::Command(id, params));
            };
            ui.label(RichText::new(hint).color(theme::c().muted));
            return None;
        };
        ui.label(
            RichText::new(format!(
                "Strong's numbers translated “{}” in the {bible}. Choose one to study:",
                c.words
            ))
            .color(theme::c().muted),
        );
        ui.add_space(6.0);
        let settings = self.session.text;
        let mut chosen = None;
        egui::ScrollArea::vertical()
            .id_salt("word-chooser")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for choice in &c.choices {
                    let top = ui.cursor().min;
                    ui.horizontal(|ui| {
                        ui.add_sized(
                            [64.0, 22.0],
                            egui::Label::new(
                                RichText::new(choice.strongs.to_string())
                                    .strong()
                                    .color(theme::c().lapis),
                            ),
                        );
                        // The Lemma column keeps its width when a number has none, so the
                        // counts line up.
                        match &choice.lemma {
                            Some(lemma) => {
                                let rtl = choice.strongs.language() == StrongsLanguage::Hebrew;
                                let big = TextSettings {
                                    size: settings.size * 1.15,
                                    ..settings
                                };
                                self.snippet(ui, lemma, rtl, LEMMA_WIDTH, &big);
                            }
                            None => {
                                ui.allocate_space(vec2(LEMMA_WIDTH, 1.0));
                            }
                        }
                        ui.label(RichText::new(format!("×{}", choice.count)).color(theme::c().ink));
                        if let Some(g) = &choice.gloss {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(format!("“{g}”")).color(theme::c().muted),
                                )
                                .truncate(),
                            );
                        }
                    });
                    let rect =
                        Rect::from_min_max(top, pos2(ui.max_rect().right(), ui.cursor().top()));
                    let r = ui
                        .interact(
                            rect,
                            ui.scope_id().with(("word-choice", choice.strongs)),
                            Sense::click(),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text("Click to study this word · Ctrl/Cmd-click opens a new Tab");
                    let label = format!("Study {} ({} times)", choice.strongs, choice.count);
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Link, true, &label));
                    if r.hovered() {
                        ui.painter().rect_stroke(
                            rect.expand(2.0),
                            4.0,
                            Stroke::new(1.0, theme::c().hover_rule),
                            egui::StrokeKind::Outside,
                        );
                    }
                    if r.clicked() {
                        chosen = Some(Action::Choose(choice.strongs, new_tab));
                    }
                    ui.add_space(4.0);
                }
            });
        chosen
    }

    /// The Lexicon entries section: one block per entry in Preferred order, the first
    /// expanded and the rest collapsed until clicked. Strong's links in an entry open their
    /// Word Study.
    pub(super) fn lexicon_entries(
        &mut self,
        ui: &mut Ui,
        tab: TabId,
        entries: &[LexiconEntry],
    ) -> Option<Action> {
        let open_state = self
            .tabs
            .get(&tab)
            .map(|t| t.lexicon_open.clone())
            .unwrap_or_default();
        let new_tab = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
        let mut action = None;
        for (i, e) in entries.iter().enumerate() {
            let id = e.id();
            let open = open_state.get(&id).copied().unwrap_or(i == 0);
            let toggled = ui
                .horizontal(|ui| {
                    // Mouse only: the title is the entry's one keyboard stop and its name.
                    let (rect, arrow) = ui.allocate_exact_size(vec2(14.0, 18.0), Sense::CLICK);
                    paint_disclosure(ui, rect, open);
                    ui.label(RichText::new(e.key.to_string()).color(theme::c().muted));
                    let title = ui.add(
                        egui::Label::new(RichText::new(&e.title).strong().color(theme::c().lapis))
                            .sense(Sense::click())
                            .truncate(),
                    );
                    let tip = if open {
                        "Collapse this entry"
                    } else {
                        "Expand this entry"
                    };
                    let title = title
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(tip);
                    let label = format!(
                        "{}, {}",
                        e.title,
                        if open { "expanded" } else { "collapsed" }
                    );
                    title.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::Button, true, &label)
                    });
                    arrow.clicked() || title.clicked()
                })
                .inner;
            if toggled {
                action = Some(Action::ToggleEntry(id.clone(), !open));
            }
            if open {
                let rtl = e.direction == Direction::Rtl;
                for para in e.text.lines().map(str::trim).filter(|l| !l.is_empty()) {
                    self.guide_snippet(ui, para, ColorRole::Ink, rtl, ENTRY_LINES);
                    ui.add_space(2.0);
                }
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .small_button(format!("Open in {}", e.abbreviation))
                        .on_hover_text("Open the Lexicon at this entry · Ctrl/Cmd-click: a new Tab")
                        .clicked()
                    {
                        action = Some(Action::OpenLexicon(e.clone(), new_tab));
                    }
                    if !e.links.is_empty() {
                        ui.label(RichText::new("See").small().color(theme::c().muted));
                        for n in &e.links {
                            if ui
                                .link(n.to_string())
                                .on_hover_text("Word Study · Ctrl/Cmd-click opens a new Tab")
                                .clicked()
                            {
                                action = Some(Action::WordStudy(*n, new_tab));
                            }
                        }
                    }
                });
            }
            ui.add_space(6.0);
        }
        action
    }

    /// Apply a Word Study action from its page.
    pub(super) fn word_study_action(&mut self, tab: TabId, action: Action) {
        match action {
            Action::WordStudy(n, new_tab) | Action::Choose(n, new_tab @ true) => {
                self.run(
                    "word_study.open",
                    &json!({ "strongs": n.to_string(), "new_tab": new_tab }),
                );
            }
            Action::Choose(n, false) => {
                self.run("tab.go", &json!({ "tab": tab.0, "text": n.to_string() }));
            }
            Action::StopChoosing => self.session.stop_choosing(tab),
            Action::Command(id, params) => {
                self.run(id, &params);
            }
            Action::ToggleEntry(id, open) => {
                if let Some(t) = self.tabs.get_mut(&tab) {
                    t.lexicon_open.insert(id, open);
                }
            }
            Action::OpenLexicon(entry, new_tab) => self.open_lexicon(tab, &entry, new_tab),
            Action::Search {
                text,
                bibles,
                new_tab,
            } => {
                self.run(
                    "search.run",
                    &json!({ "text": text, "mode": "bible", "bibles": bibles, "split": new_tab }),
                );
            }
            Action::FilterForm(form) => {
                if let Some(t) = self.tabs.get_mut(&tab) {
                    t.occurrence_form = form;
                }
            }
            Action::ShowAllRenderings(all) => {
                if let Some(t) = self.tabs.get_mut(&tab) {
                    t.renderings_all = all;
                }
            }
            Action::OpenVerse {
                bible,
                verse,
                new_tab,
            } => {
                let verse = obs_scripture::format_passage(
                    obs_scripture::Passage::verse(verse),
                    &obs_scripture::Standard,
                    obs_scripture::RefStyle::Short,
                );
                self.run(
                    "word_study.open_verse",
                    &json!({ "bible": bible, "verse": verse, "new_tab": new_tab }),
                );
            }
            _ => {}
        }
    }

    /// Open a Lexicon at an entry: its open Tab if there is one (Ctrl/Cmd-click: always a
    /// new Tab), otherwise a new Tab in the usual place. The Word Study and its Link set stay
    /// where they are.
    fn open_lexicon(&mut self, guide: TabId, entry: &LexiconEntry, new_tab: bool) {
        let unit = obs_model::UnitId::new(entry.unit.clone()).ok();
        let key = self
            .doc(&entry.resource_id)
            .ok()
            .and_then(|d| {
                unit.as_ref()
                    .and_then(|u| d.navigation.entry_for_unit(u))
                    .map(|k| k.key.clone())
            })
            .unwrap_or_else(|| entry.key.clone());
        let link = self.session.workspace.tab(guide).and_then(|t| t.link);
        let kind = TabKind::Resource(entry.resource_id.clone());
        let existing = (!new_tab)
            .then(|| {
                let ws = &self.session.workspace;
                ws.tabs()
                    .filter(|t| t.kind == kind)
                    .max_by_key(|t| (link.is_some() && t.link == link, t.id))
                    .map(|t| t.id)
            })
            .flatten();
        let tab = match existing {
            Some(tab) => tab,
            None => {
                self.session.workspace.focus(guide);
                let Some(tab) = self
                    .run("resource.open", &json!({ "id": entry.resource_id }))
                    .and_then(|out| out["tab"].as_u64())
                else {
                    return;
                };
                TabId(tab)
            }
        };
        // Without moving the Link set: a base-number entry must not move the Word Study.
        let ws = &mut self.session.workspace;
        if let Some(t) = ws.tab_mut(tab) {
            t.entry_history.visit(key.clone());
        }
        ws.push_effect(Effect::NavigateEntry { tab, key });
        ws.focus(tab);
    }
}

/// `H1254A · verb`: the Strong's number, large, and the part of speech.
fn strongs_line(ui: &mut Ui, number: StrongsNumber, part_of_speech: Option<&str>) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(number.to_string())
                .font(theme::display(20.0))
                .color(theme::c().lapis),
        );
        if let Some(p) = part_of_speech {
            ui.label(RichText::new(p).color(theme::c().muted));
        }
    });
}

/// ▸ collapsed, ▾ expanded, painted (the UI font has no such glyphs).
fn paint_disclosure(ui: &Ui, rect: Rect, open: bool) {
    let tip = rect.center();
    let points = if open {
        vec![
            tip + vec2(-5.0, -3.0),
            tip + vec2(5.0, -3.0),
            tip + vec2(0.0, 4.0),
        ]
    } else {
        vec![
            tip + vec2(-3.0, -5.0),
            tip + vec2(4.0, 0.0),
            tip + vec2(-3.0, 5.0),
        ]
    };
    ui.painter().add(Shape::convex_polygon(
        points,
        theme::c().lapis,
        Stroke::NONE,
    ));
}
