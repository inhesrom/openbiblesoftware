//! The Topic Guide Tab (#148): one topic (a Headword) across the library's topical indexes
//! (Nave's, Torrey's) and Dictionaries. The header names the topic, then the Guide's sections
//! through the shared [`State::guide_page`]: Topical Bible entries (subtopics with their
//! Passages, whose text shows inline on request and in a popup on hover), Dictionary
//! articles, Key Passages and Search. Text typed into the box shows the chooser (the topic,
//! longer headwords and synonyms from the topical indexes) until one topic is chosen.
//! Everything shown is engine data ([`obs_engine::topic_guide`]); this file only draws it.

use egui::{Rect, RichText, Sense, Ui, pos2, vec2};
use obs_engine::{
    ArticleLink, CitedPassage, Guide, GuideSection, Subtopic, TabId, TopicChoice, TopicalEntry,
    WordStatus,
};
use obs_model::{Key, ResourceType};
use obs_scripture::{Passage, RefStyle, Standard, format_passage};
use serde_json::json;

use super::State;
use super::guide_tab::hint_page;
use crate::theme;
use crate::tokens::space;

/// Subtopic rows of one entry shown before "Show all".
const SUBTOPICS_SHOWN: usize = 12;

/// Passages of one subtopic whose text is shown inline.
const PREVIEWS_SHOWN: usize = 8;

/// What a click in a Topic Guide asks for. `bool`: Ctrl/Cmd held (a new Tab).
pub(super) enum TopicAction {
    /// Another topic: a chooser row or a `See` link.
    Topic(String, bool),
    /// A Passage: the Link set goes there.
    Passage(Passage, bool),
    /// A topical index or Dictionary at an entry.
    Article(ArticleLink, bool),
    /// Show (or hide) a subtopic's Passages inline.
    TogglePreview(SubtopicId),
    /// Search the library for the topic.
    Search(String, bool),
    /// Close the chooser.
    StopChoosing,
    /// An empty state's button (#135).
    Command(&'static str, serde_json::Value),
}

/// One subtopic row of the Topical Bible entries: one topical index can have several entries
/// on a topic (Torrey's PRAYER and PRAYER, ANSWERS TO), so the headword is part of it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct SubtopicId {
    resource_id: String,
    headword: String,
    index: usize,
}

impl State {
    /// Forget every Topic Guide's subtopic row heights, measured in faces no longer in use
    /// (the interface font changed): rows out of view are measured again as they come in.
    pub(super) fn forget_topic_row_heights(&mut self) {
        for t in self.tabs.values_mut() {
            t.topic_rows.clear();
        }
    }

    pub(super) fn topic_guide_body(&mut self, ui: &mut Ui, tab: TabId) {
        if let Some(text) = self.session.topic_guides.choosing(tab).map(str::to_string) {
            self.topic_chooser_page(ui, tab, &text);
            return;
        }
        let key = self
            .session
            .workspace
            .tab(tab)
            .and_then(|t| t.entry_history.current());
        let Some(Key::Headword(topic)) = key else {
            hint_page(
                ui,
                "Type a topic in the box above (Forgiveness, Prayer, Faith), or put this Guide in \
                 a Dictionary's Link set to follow it.",
            );
            return;
        };
        if let Some(t) = self.tabs.get_mut(&tab)
            && t.topic_previews_for.as_deref() != Some(topic.as_str())
        {
            t.topic_previews_for = Some(topic.clone());
            t.topic_previews.clear();
            t.topic_rows.clear();
            t.guide_expanded.clear();
        }
        let title = match self.session.topic_directory() {
            WordStatus::Ready(d) => d.title(&topic).map_or_else(
                || obs_engine::topic_guide::display_topic(&topic),
                str::to_string,
            ),
            _ => obs_engine::topic_guide::display_topic(&topic),
        };
        self.guide_page(
            ui,
            tab,
            Guide::TopicGuide,
            &Key::Headword(topic.clone()),
            &mut |_, ui| {
                ui.label(crate::components::title_text(title.clone()));
                None
            },
        );
    }

    /// The chooser: the topics the typed text names. Click one to show it (Ctrl/Cmd-click: a
    /// new Topic Guide); Cancel or Escape goes back.
    fn topic_chooser_page(&mut self, ui: &mut Ui, tab: TabId, text: &str) {
        let status = self.session.topic_chooser(text);
        let can_cancel = self
            .session
            .workspace
            .tab(tab)
            .is_some_and(|t| t.entry_history.current().is_some());
        let new_tab = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
        let mut action: Option<TopicAction> = None;
        crate::components::page_frame().show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            ui.horizontal(|ui| {
                ui.label(crate::components::title_text(format!("“{text}”")));
                if can_cancel
                    && ui
                        .small_button("Cancel")
                        .on_hover_text("Back to the topic shown before (Esc)")
                        .clicked()
                {
                    action = Some(TopicAction::StopChoosing);
                }
            });
            let chooser = match &status {
                WordStatus::Loading => {
                    ui.label(
                        RichText::new(format!("Looking up “{text}”…")).color(theme::c().muted),
                    );
                    ui.ctx().request_repaint();
                    return;
                }
                WordStatus::Failed(e) => {
                    ui.label(
                        RichText::new(format!("Could not look up “{text}”: {e}"))
                            .color(theme::c().warn),
                    );
                    return;
                }
                WordStatus::Ready(c) => c,
            };
            if chooser.choices.is_empty() {
                if self
                    .session
                    .library
                    .of_type(ResourceType::Dictionary)
                    .next()
                    .is_some()
                {
                    ui.label(
                        RichText::new(format!(
                            "No topic is called “{text}”. Try a shorter word (forgive, pray)."
                        ))
                        .color(theme::c().muted),
                    );
                } else {
                    let missing = GuideSection::TopicalEntries.missing_resource(
                        false,
                        None,
                        &self.session.library,
                        self.session.starter.manifest(),
                    );
                    if let Some((id, params)) = self.missing_resource_ui(
                        ui,
                        "No Dictionary in your library. Add a topical index (Nave's Topical \
                             Bible) or a Dictionary to look up topics.",
                        &missing,
                    ) {
                        action = Some(TopicAction::Command(id, params));
                    }
                }
                return;
            }
            ui.label(
                RichText::new(format!(
                    "{} topics for “{text}”. Choose one:",
                    chooser.choices.len()
                ))
                .color(theme::c().muted),
            );
            ui.add_space(space::S);
            egui::ScrollArea::vertical()
                .id_salt(("topic-chooser", tab))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for c in &chooser.choices {
                        if choice_row(ui, c) {
                            action = Some(TopicAction::Topic(c.topic.clone(), new_tab));
                        }
                        ui.add_space(space::XS);
                    }
                });
        });
        if can_cancel && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            action = Some(TopicAction::StopChoosing);
        }
        if let Some(a) = action {
            self.session.workspace.user_focused(tab);
            self.topic_action(tab, a);
        }
    }

    /// Topical Bible entries: each topical index's entry, its subtopics with their Passages
    /// (hover one to read it; ▶ shows their text inline) and `See` links to other topics.
    pub(super) fn topical_entries_section(
        &mut self,
        ui: &mut Ui,
        tab: TabId,
        entries: &[TopicalEntry],
        new_tab: bool,
    ) -> Option<TopicAction> {
        let all = self.expanded(tab, GuideSection::TopicalEntries);
        let mut action = None;
        let mut longest = 0;
        for entry in entries {
            longest = longest.max(entry.subtopics.len());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&entry.article.title)
                        .strong()
                        .color(theme::c().ink),
                );
                if ui
                    .link(format!(
                        "{}: {}",
                        entry.article.abbreviation, entry.article.headword
                    ))
                    .on_hover_text(format!(
                        "Open {} at “{}” · Ctrl/Cmd-click opens a new Tab",
                        entry.article.title, entry.article.headword
                    ))
                    .clicked()
                {
                    action = Some(TopicAction::Article(entry.article.clone(), new_tab));
                }
            });
            let shown = if all {
                entry.subtopics.len()
            } else {
                entry.subtopics.len().min(SUBTOPICS_SHOWN)
            };
            for (i, sub) in entry.subtopics[..shown].iter().enumerate() {
                let id = SubtopicId {
                    resource_id: entry.article.resource_id.clone(),
                    headword: entry.article.headword.clone(),
                    index: i,
                };
                let open = self
                    .tabs
                    .get(&tab)
                    .is_some_and(|t| t.topic_previews.contains(&id));
                // A long topic shown in full has thousands of Passage links: a row out of view
                // keeps the height it had when last drawn at this width, without being laid
                // out (#183).
                let width = ui.available_width();
                let height = self
                    .tabs
                    .get(&tab)
                    .and_then(|t| t.topic_rows.get(&id))
                    .filter(|(w, _)| *w == width)
                    .map(|(_, h)| *h);
                if let Some(h) = height
                    && !ui.is_rect_visible(Rect::from_min_size(ui.cursor().min, vec2(width, h)))
                {
                    ui.allocate_space(vec2(width, h));
                } else {
                    let row = self.subtopic_row(ui, sub, open, &id, new_tab, &mut action);
                    if let Some(t) = self.tabs.get_mut(&tab) {
                        t.topic_rows.insert(id.clone(), (width, row.height()));
                    }
                }
                if open {
                    for p in sub.passages.iter().take(PREVIEWS_SHOWN) {
                        ui.horizontal(|ui| {
                            ui.add_space(space::XL * f32::from(sub.level));
                            ui.label(crate::components::hint_text(format_passage(
                                *p,
                                &Standard,
                                RefStyle::Short,
                            )));
                        });
                        self.preview(ui, Passage::verse(p.start()), true);
                    }
                    if sub.passages.len() > PREVIEWS_SHOWN {
                        ui.label(crate::components::hint_text(format!(
                            "{} more: hover a reference to read it",
                            sub.passages.len() - PREVIEWS_SHOWN
                        )));
                    }
                }
            }
            ui.add_space(space::S);
        }
        if longest > SUBTOPICS_SHOWN {
            let label = if all {
                "Show fewer".to_string()
            } else {
                format!("Show all {longest} subtopics")
            };
            if ui.small_button(label).clicked()
                && let Some(t) = self.tabs.get_mut(&tab)
                && !t.guide_expanded.remove(&GuideSection::TopicalEntries)
            {
                t.guide_expanded.insert(GuideSection::TopicalEntries);
            }
        }
        action
    }

    /// One subtopic row: its preview toggle, label, Passage links and See topics. Returns
    /// where it was drawn.
    fn subtopic_row(
        &mut self,
        ui: &mut Ui,
        sub: &Subtopic,
        open: bool,
        id: &SubtopicId,
        new_tab: bool,
        action: &mut Option<TopicAction>,
    ) -> Rect {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = space::S;
            ui.add_space(space::XL * f32::from(sub.level.saturating_sub(1)));
            if !sub.passages.is_empty() {
                let (arrow, what) = if open {
                    (crate::icons::Icon::ChevronDown, "Hide the Passages' text")
                } else {
                    (
                        crate::icons::Icon::ChevronRight,
                        "Show the Passages' text here",
                    )
                };
                if crate::icons::small_button(ui, arrow, what).clicked() {
                    *action = Some(TopicAction::TogglePreview(id.clone()));
                }
            }
            if !sub.label.is_empty() {
                let label = RichText::new(&sub.label).color(theme::c().ink);
                ui.label(if sub.level == 1 {
                    label.strong()
                } else {
                    label
                });
            }
            for p in &sub.passages {
                if let Some(a) = self.passage_link(ui, *p, new_tab) {
                    *action = Some(a);
                }
            }
            if let Some(via) = &sub.via {
                ui.label(crate::components::hint_text(format!("(from {via})")));
            }
            if !sub.see.is_empty() {
                ui.label(RichText::new("see").italics().color(theme::c().muted));
                for target in &sub.see {
                    let topic = see_topic(target);
                    if super::unbroken_link(ui, obs_engine::topic_guide::display_topic(target))
                        .on_hover_text(format!(
                            "Open the Topic Guide on {topic} · Ctrl/Cmd-click opens a new \
                             Tab"
                        ))
                        .clicked()
                    {
                        *action = Some(TopicAction::Topic(topic, new_tab));
                    }
                }
            }
        })
        .response
        .rect
    }

    /// Key Passages: the most cited first, each with how often and its first line of text.
    pub(super) fn topic_passages_section(
        &mut self,
        ui: &mut Ui,
        passages: &[CitedPassage],
        new_tab: bool,
    ) -> Option<TopicAction> {
        let mut action = None;
        for c in passages {
            ui.horizontal(|ui| {
                if let Some(a) = self.passage_link(ui, c.passage, new_tab) {
                    action = Some(a);
                }
                let times = if c.citations == 1 {
                    "cited once".to_string()
                } else {
                    format!("cited {}×", c.citations)
                };
                ui.label(crate::components::hint_text(times));
            });
            self.preview(ui, Passage::verse(c.passage.start()), true);
        }
        action
    }

    /// Search: hand the topic to the Search Tool's Library mode.
    pub(super) fn topic_search_section(
        &mut self,
        ui: &mut Ui,
        query: &str,
        new_tab: bool,
    ) -> Option<TopicAction> {
        ui.button(format!("Search your library for “{query}”"))
            .on_hover_text(
                "Search every Commentary, Lexicon and Dictionary in the Search Tool · Ctrl/Cmd \
                 opens it beside this Guide",
            )
            .clicked()
            .then(|| TopicAction::Search(query.to_string(), new_tab))
    }

    /// A Passage as a link: hover reads it, click moves the Link set there.
    fn passage_link(&mut self, ui: &mut Ui, p: Passage, new_tab: bool) -> Option<TopicAction> {
        let r = super::unbroken_link(ui, format_passage(p, &Standard, RefStyle::Short))
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if r.hovered() {
            self.reference_popup(ui, &[vec![p].into()]);
        }
        r.clicked().then_some(TopicAction::Passage(p, new_tab))
    }

    pub(super) fn topic_action(&mut self, tab: TabId, action: TopicAction) {
        match action {
            TopicAction::Topic(topic, true) => {
                self.run(
                    "guide.topic_guide",
                    &json!({ "topic": topic, "new_tab": true }),
                );
            }
            TopicAction::Topic(topic, false) => {
                self.session.stop_choosing_topic(tab);
                self.session
                    .workspace
                    .navigate_entry(tab, Key::Headword(topic));
            }
            // In a Link set, the set goes there (a Bible in it follows); otherwise the most
            // recent Bible Tab, or a new ★ Bible Tab.
            TopicAction::Passage(p, new_tab) => self.reference_navigate(tab, p, new_tab),
            TopicAction::Article(a, new_tab) => self.open_article(tab, &a, new_tab),
            TopicAction::TogglePreview(id) => {
                if let Some(t) = self.tabs.get_mut(&tab)
                    && !t.topic_previews.remove(&id)
                {
                    t.topic_previews.insert(id);
                }
            }
            TopicAction::Search(text, new_tab) => {
                self.run(
                    "search.run",
                    &json!({ "text": text, "mode": "library", "split": new_tab }),
                );
            }
            TopicAction::StopChoosing => self.session.stop_choosing_topic(tab),
            TopicAction::Command(id, params) => {
                self.run(id, &params);
            }
        }
    }
}

/// The topic a `See` target opens: the headword before its comma (`SIN, FORGIVENESS OF` →
/// Sin), where the subtopic is.
fn see_topic(target: &str) -> String {
    let head = target.split(',').next().unwrap_or(target);
    obs_engine::topic_guide::display_topic(head)
}

/// One chooser row: the topic, the Dictionaries that have it, and for a synonym what its
/// topical index sends the reader to. Returns whether it was clicked.
fn choice_row(ui: &mut Ui, c: &TopicChoice) -> bool {
    let top = ui.cursor().min;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(&c.topic).strong().color(theme::c().lapis));
        if !c.see.is_empty() {
            ui.label(
                RichText::new(format!("see {}", c.see.join("; ")))
                    .italics()
                    .color(theme::c().ink),
            );
        }
    });
    ui.label(crate::components::hint_text(c.sources.join(", ")));
    let rect = Rect::from_min_max(top, pos2(ui.max_rect().right(), ui.cursor().top()));
    let r = ui
        .interact(
            rect,
            ui.scope_id().with(("topic-choice", &c.topic)),
            Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Click to open its Topic Guide · Ctrl/Cmd-click opens a new Tab");
    let label = format!("Topic Guide: {}", c.topic);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Link, true, &label));
    if r.hovered() {
        crate::components::hover_outline(ui, rect);
    }
    r.clicked()
}
