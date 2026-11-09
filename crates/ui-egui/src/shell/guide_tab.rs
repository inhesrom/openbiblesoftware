//! Guide Tabs (#53): the Guide's Key (a Passage Guide's or Exegetical Guide's Passage, a Word
//! Study's header), then its sections in the Guide arrangement's order
//! ([`State::guide_page`], shared by every kind of Guide). Sections come from the engine as pure data computed on the job queue
//! ([`obs_engine::Session::guide_section`]); a section still computing shows "Loading…" and
//! the frame goes on. Headers collapse on click and reorder by dragging; the arrangement is
//! the engine's, per kind of Guide, so every Passage Guide shows the same one.

use egui::{Align2, FontId, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use obs_engine::{CommentaryRow, Guide, GuideSection, SectionData, SectionStatus, TabId, TabKind};
use obs_layout::{ColorRole, TextSettings};
use obs_model::{Direction, Key};
use obs_scripture::{Passage, RefStyle, Standard, format_passage};
use serde_json::json;

use super::State;
use super::factbook_tab::FactbookAction;
use super::guide_words::WordsAction;
use super::topic_guide_tab::TopicAction;
use crate::icons::{self, Icon};
use crate::theme;
use crate::tokens::{radius, scale, size, space, stroke, text};

/// Height of a section header.
const HEADER_H: f32 = size::ROW;
/// Lines of a Commentary row's excerpt (#15: "the first ~3 lines").
const ROW_LINES: usize = 3;
/// Lines of excerpt in a Commentary row's hover popup.
const POPUP_LINES: usize = 14;

/// What the user did in the Guide this frame.
pub(super) enum Action {
    Toggle(GuideSection),
    Hide(GuideSection),
    Show(GuideSection),
    Move(GuideSection, usize),
    Reference(Passage, bool),
    Commentary(CommentaryRow, bool),
    /// Parallel passages (#212): compare a parallel in Text Comparison (a new one when
    /// Ctrl/Cmd is held).
    CompareParallel(Passage, bool),
    /// Key words and People & places (#54).
    Words(WordsAction),
    /// Word Study (#55): open a Strong's number (a new Tab when Ctrl/Cmd is held).
    WordStudy(obs_scripture::StrongsNumber, bool),
    /// Word Study: a chooser row; plain click moves this Guide, Ctrl/Cmd-click opens a new one.
    Choose(obs_scripture::StrongsNumber, bool),
    /// Word Study: close the English-word chooser.
    StopChoosing,
    /// Word Study: expand (true) or collapse one Lexicon entry, by [`LexiconEntry::id`].
    ///
    /// [`LexiconEntry::id`]: obs_engine::LexiconEntry::id
    ToggleEntry(String, bool),
    /// Word Study: open a Lexicon at an entry.
    OpenLexicon(obs_engine::LexiconEntry, bool),
    /// Word Study (#56): hand a query over to the Search Tool (Translated as, Show all in
    /// Search); `bibles` are resource ids, comma-separated.
    Search {
        text: String,
        bibles: String,
        new_tab: bool,
    },
    /// Word Study: filter Occurrences by a form (`None`: every form).
    FilterForm(Option<String>),
    /// Word Study: Translated as shows every rendering (true) or the first few.
    ShowAllRenderings(bool),
    /// Word Study: open an Occurrences Verse in a Bible (its own numbering).
    OpenVerse {
        bible: String,
        verse: obs_scripture::Verse,
        new_tab: bool,
    },
    /// Factbook (#149): a link, a Key Passage, an article or a chooser row.
    Factbook(FactbookAction),
    /// Topic Guide (#148): a Passage, a topic, an entry, a preview toggle or Search.
    Topic(TopicAction),
    /// An empty state's button (#135): Get Resources, the Starter library or Import.
    Command(&'static str, serde_json::Value),
}

/// One visible section as drawn this frame, for working out where a dragged header lands.
struct Drawn {
    section: GuideSection,
    rect: Rect,
}

impl State {
    /// A Passage Guide's or Exegetical Guide's page: the Passage, then the Guide's sections.
    pub(super) fn passage_keyed_guide_body(&mut self, ui: &mut Ui, tab: TabId, guide: Guide) {
        let passage = self
            .session
            .workspace
            .tab(tab)
            .and_then(|t| t.shown_passage());
        let Some(passage) = passage else {
            hint_page(
                ui,
                "Type a Passage in the box above (for example “Jn 3:16-18”), or put this Guide \
                 in a Bible's Link set to follow it.",
            );
            return;
        };
        let Some(t) = self.tabs.get_mut(&tab) else {
            return;
        };
        if t.reference_at != Some(passage) {
            t.reference_at = Some(passage);
            t.expanded_references.clear();
            t.guide_expanded.clear();
        }
        let title = format_passage(passage, &Standard, RefStyle::Long);
        self.guide_page(ui, tab, guide, &Key::Passage(passage), &mut |_, ui| {
            ui.label(crate::components::title_text(title.clone()));
            None
        });
    }

    /// A Guide's page: `head` (the Guide's Key), then its sections in the Guide arrangement's
    /// order, each computed on the job queue. Headers collapse on click, move by dragging and
    /// hide with their Close button, for every Guide of the kind.
    pub(super) fn guide_page(
        &mut self,
        ui: &mut Ui,
        tab: TabId,
        guide: Guide,
        key: &Key,
        head: &mut dyn FnMut(&mut Self, &mut Ui) -> Option<Action>,
    ) {
        let dragging = self.tabs.get(&tab).and_then(|t| t.guide_drag);
        let arrangement = self.session.guides.arrangement(guide);
        let mut action: Option<Action> = None;
        let mut drawn: Vec<Drawn> = Vec::new();
        let mut drag_start: Option<GuideSection> = None;
        let mut drag_end = false;
        let mut region = Rect::NOTHING;
        crate::components::page_frame().show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            if let Some(a) = head(self, ui) {
                action = Some(a);
            }
            ui.add_space(space::XS);
            let scroll = egui::ScrollArea::vertical()
                .id_salt((guide.id(), tab))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut any = false;
                    for place in arrangement.visible() {
                        any = true;
                        let section = place.section;
                        let top = ui.cursor().top();
                        // Ask even when collapsed, so the header can show a count.
                        let status = self.session.guide_section(key, section);
                        let header = section_header(
                            ui,
                            tab,
                            guide,
                            section,
                            place.collapsed,
                            &status,
                            dragging == Some(section),
                        );
                        if header.hide {
                            action = Some(Action::Hide(section));
                        }
                        if header.body.drag_started() {
                            drag_start = Some(section);
                        }
                        if header.body.drag_stopped() {
                            drag_end = true;
                        }
                        if header.body.clicked() {
                            action = Some(Action::Toggle(section));
                        }
                        if !place.collapsed {
                            ui.add_space(space::XS);
                            if let Some(a) = self.section_body(ui, tab, section, &status, key) {
                                action = Some(a);
                            }
                        }
                        ui.add_space(space::L);
                        let rect =
                            Rect::from_x_y_ranges(ui.max_rect().x_range(), top..=ui.cursor().top());
                        drawn.push(Drawn { section, rect });
                    }
                    if !any {
                        ui.label(RichText::new("Every section is hidden.").color(theme::c().muted));
                    }
                    let hidden: Vec<GuideSection> = arrangement
                        .sections
                        .iter()
                        .filter(|p| p.hidden)
                        .map(|p| p.section)
                        .collect();
                    if !hidden.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(crate::components::hint_text("Hidden:"));
                            for s in hidden {
                                if ui
                                    .small_button(format!("Show {}", s.title()))
                                    .on_hover_text("Show this section again")
                                    .clicked()
                                {
                                    action = Some(Action::Show(s));
                                }
                            }
                        });
                    }
                });
            region = scroll.inner_rect.intersect(ui.clip_rect());
        });

        // Dragging a header: a line marks where it would land; dropping moves it there for
        // every Guide of the kind. Escape cancels.
        let mut dragging = drag_start.or(dragging);
        if !drag_end && drag_start.is_none() && !ui.input(|i| i.pointer.any_down()) {
            dragging = None;
        }
        if dragging.is_some() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            dragging = None;
            drag_end = false;
        }
        if let (Some(section), Some(p)) = (dragging, ui.ctx().pointer_interact_pos())
            && region.expand(HEADER_H).contains(p)
        {
            let others: Vec<&Drawn> = drawn.iter().filter(|d| d.section != section).collect();
            let slot = others.iter().filter(|d| d.rect.center().y < p.y).count();
            if drag_end {
                let order: Vec<_> = arrangement
                    .sections
                    .iter()
                    .map(|p| p.section)
                    .filter(|s| *s != section)
                    .collect();
                let to = match others.get(slot) {
                    Some(d) => order.iter().position(|s| *s == d.section),
                    None => others
                        .last()
                        .and_then(|d| order.iter().position(|s| *s == d.section))
                        .map(|i| i + 1),
                }
                .unwrap_or(0);
                action = Some(Action::Move(section, to));
            } else {
                let y = match (
                    others.get(slot),
                    slot.checked_sub(1).and_then(|i| others.get(i)),
                ) {
                    (Some(d), _) => d.rect.top() - space::XS,
                    (None, Some(d)) => d.rect.bottom() - space::XS,
                    (None, None) => region.top(),
                };
                ui.painter().hline(
                    region.x_range(),
                    y,
                    Stroke::new(stroke::THICK, theme::c().lapis),
                );
            }
        }
        if let Some(t) = self.tabs.get_mut(&tab) {
            t.guide_drag = if drag_end { None } else { dragging };
        }
        if self.session.guides.is_busy() || self.session.word_study.is_busy() {
            ui.ctx().request_repaint();
        }
        let Some(action) = action else {
            return;
        };
        self.session.workspace.user_focused(tab);
        let guide = guide.id();
        match action {
            Action::Toggle(s) => {
                self.run(
                    "guide.collapse_section",
                    &json!({ "guide": guide, "section": s.id() }),
                );
            }
            Action::Hide(s) | Action::Show(s) => {
                let hidden = matches!(action, Action::Hide(_));
                self.run(
                    "guide.hide_section",
                    &json!({ "guide": guide, "section": s.id(), "hidden": hidden }),
                );
            }
            Action::Move(s, to) => {
                self.run(
                    "guide.move_section",
                    &json!({ "guide": guide, "section": s.id(), "to": to }),
                );
            }
            Action::Reference(passage, new_tab) => self.reference_navigate(tab, passage, new_tab),
            Action::Commentary(row, new_tab) => self.open_commentary(tab, &row, new_tab),
            Action::CompareParallel(passage, new_tab) => {
                self.run(
                    "compare.parallel",
                    &json!({
                        "passage": format_passage(passage, &Standard, RefStyle::Short),
                        "new_tab": new_tab,
                    }),
                );
            }
            Action::Words(a) => self.words_action(tab, a),
            Action::Factbook(a) => self.factbook_action(tab, a),
            Action::Topic(a) => self.topic_action(tab, a),
            Action::Command(id, params) => {
                self.run(id, &params);
            }
            action => self.word_study_action(tab, action),
        }
    }

    /// The Guide part of a Guide Tab's ⋯ menu: show or hide each section, and Reset
    /// arrangement. Both apply to every Guide of the kind.
    pub(super) fn guide_menu(&mut self, ui: &mut Ui, guide: Guide) {
        let arrangement = self.session.guides.arrangement(guide);
        let mut run: Option<(&'static str, serde_json::Value)> = None;
        ui.label(crate::components::hint_text("Sections"));
        for place in &arrangement.sections {
            let mut shown = !place.hidden;
            if crate::components::toggle(ui, &mut shown, place.section.title()).clicked() {
                run = Some((
                    "guide.hide_section",
                    json!({ "guide": guide.id(), "section": place.section.id(), "hidden": !shown }),
                ));
            }
        }
        if ui
            .button("Reset arrangement")
            .on_hover_text(
                "The default order, every section shown and expanded or collapsed as at first",
            )
            .clicked()
        {
            run = Some(("guide.reset_arrangement", json!({ "guide": guide.id() })));
        }
        ui.separator();
        if let Some((id, params)) = run {
            self.run(id, &params);
            ui.close();
        }
    }

    /// A section's content: loading, failed, its empty state with the buttons that would fill
    /// it (#135), or its rows.
    fn section_body(
        &mut self,
        ui: &mut Ui,
        tab: TabId,
        section: GuideSection,
        status: &SectionStatus,
        key: &Key,
    ) -> Option<Action> {
        let data = match status {
            SectionStatus::Loading => {
                ui.label(RichText::new("Loading…").color(theme::c().muted));
                ui.ctx().request_repaint();
                return None;
            }
            SectionStatus::Failed(e) => {
                ui.label(
                    RichText::new(format!("Could not load {}: {e}", section.title()))
                        .color(theme::c().warn),
                );
                return None;
            }
            SectionStatus::Ready(data) => data,
        };
        // A section filtered to a Collection (#153): the chooser, and its own empty state.
        let mut filter = None;
        if section.filterable()
            && let Some(guide) = Guide::ALL
                .into_iter()
                .find(|g| g.sections().contains(&section))
        {
            filter = self.session.guide_filter(guide, section);
            if let Some(a) =
                collection_filter(ui, &self.session.library, guide, section, filter.as_deref())
            {
                return Some(a);
            }
        }
        if data.is_empty()
            && let Some(name) = filter
        {
            ui.label(
                RichText::new(format!(
                    "Nothing in the Collection “{name}” covers this. Choose “Every Resource” to \
                     see the rest of your library."
                ))
                .color(theme::c().muted),
            );
            return None;
        }
        if data.is_empty() {
            let empty = self.session.guide_empty_state(key, section);
            return self
                .missing_resource_ui(ui, empty.message, &empty.missing)
                .map(|(id, params)| Action::Command(id, params));
        }
        let new_tab = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
        match &**data {
            SectionData::CrossReferences(groups) => self
                .reference_groups(ui, tab, groups)
                .map(|p| Action::Reference(p, new_tab)),
            SectionData::ParallelPassages(found) => {
                // Parallel passages is a Passage Guide section: its Key is a Passage.
                let Key::Passage(passage) = key else {
                    return None;
                };
                self.parallel_passages_section(ui, found, *passage)
                    .map(|p| Action::CompareParallel(p, new_tab))
            }
            SectionData::Commentaries(rows) => {
                let mut clicked = None;
                for row in rows {
                    if self.commentary_row(ui, tab, row) {
                        clicked = Some(Action::Commentary(row.clone(), new_tab));
                    }
                    ui.add_space(space::S);
                }
                clicked
            }
            SectionData::KeyWords(k) => self
                .key_words_section(ui, tab, k, new_tab)
                .map(Action::Words),
            SectionData::PeopleAndPlaces(rows) => {
                // People & places is a Passage Guide section: its Key is a Passage.
                let Key::Passage(passage) = key else {
                    return None;
                };
                self.people_section(ui, tab, rows, *passage, new_tab)
                    .map(Action::Words)
            }
            SectionData::LexiconEntries(entries) => self.lexicon_entries(ui, tab, entries),
            SectionData::TranslatedAs(t) => {
                let number = Self::section_number(key)?;
                self.translated_as_section(ui, tab, t, number, new_tab)
            }
            SectionData::Occurrences(o) => {
                let number = Self::section_number(key)?;
                self.occurrences_section(ui, tab, o, number, new_tab)
            }
            SectionData::Forms(f) => self.forms_section(ui, tab, f),
            SectionData::FactbookSummary(sum) => self
                .factbook_summary(ui, sum, new_tab)
                .map(Action::Factbook),
            SectionData::KeyPassages(passages) => self
                .factbook_passages(ui, tab, passages, new_tab)
                .map(Action::Factbook),
            SectionData::DictionaryArticles(articles) => self
                .factbook_articles(ui, tab, articles, new_tab)
                .map(Action::Factbook),
            SectionData::Related(rows) => self
                .factbook_related(ui, tab, rows, new_tab)
                .map(Action::Factbook),
            SectionData::Map(places) => {
                // A Factbook's Key is its entity's id.
                let subject = match key {
                    Key::Headword(id) => Some(id.as_str()),
                    _ => None,
                };
                self.factbook_map(ui, places, subject).map(Action::Factbook)
            }
            SectionData::WordByWord(w) => self.word_by_word_section(ui, tab, w, new_tab),
            SectionData::LexiconSummaries(rows) => {
                self.lexicon_summaries_section(ui, tab, rows, new_tab)
            }
            SectionData::GrammaticalNotes(notes) => {
                self.grammatical_notes_section(ui, notes);
                None
            }
            SectionData::TopicalEntries(entries) => self
                .topical_entries_section(ui, tab, entries, new_tab)
                .map(Action::Topic),
            SectionData::TopicPassages(passages) => self
                .topic_passages_section(ui, passages, new_tab)
                .map(Action::Topic),
            SectionData::TopicSearch(query) => self
                .topic_search_section(ui, query, new_tab)
                .map(Action::Topic),
        }
    }

    /// One Commentary row: title and section reference, the section heading and the first
    /// lines of its text (Resource text, through the pipeline). Hovering shows more of it;
    /// returns whether the row was clicked.
    fn commentary_row(&mut self, ui: &mut Ui, tab: TabId, row: &CommentaryRow) -> bool {
        let top = ui.cursor().min;
        let reference = format_passage(row.section, &Standard, RefStyle::Short);
        ui.horizontal(|ui| {
            ui.label(RichText::new(&row.title).strong().color(theme::c().lapis));
            ui.label(RichText::new(&reference).color(theme::c().muted));
        });
        let rtl = row.direction == Direction::Rtl;
        if let Some(h) = &row.heading {
            self.guide_snippet(ui, h, ColorRole::Accent, rtl, 2);
        }
        if !row.excerpt.is_empty() {
            self.guide_snippet(ui, &row.excerpt, ColorRole::Ink, rtl, ROW_LINES);
        }
        let rect = Rect::from_min_max(top, pos2(ui.max_rect().right(), ui.cursor().top()));
        let r = ui
            .interact(
                rect,
                ui.scope_id()
                    .with(("commentary-row", tab, &row.resource_id)),
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        let label = format!("Open {} at {reference}", row.title);
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Link, true, &label));
        if r.hovered() {
            crate::components::hover_outline(ui, rect);
            egui::Tooltip::always_open(
                ui.ctx().clone(),
                ui.layer_id(),
                ui.scope_id().with(("commentary-popup", &row.resource_id)),
                egui::PopupAnchor::Pointer,
            )
            .show(|ui| {
                ui.set_width(size::POPOVER);
                ui.label(RichText::new(format!("{} · {reference}", row.abbreviation)).strong());
                if let Some(h) = &row.heading {
                    self.guide_snippet(ui, h, ColorRole::Accent, rtl, 3);
                }
                self.guide_snippet(ui, &row.excerpt, ColorRole::Ink, rtl, POPUP_LINES);
                ui.label(crate::components::hint_text(
                    "Click to open this section · Ctrl/Cmd-click opens a new Tab",
                ));
            });
        }
        r.clicked()
    }

    /// Draw Resource text through the inline-snippet pipeline, at most `lines` lines (a
    /// placeholder bar until a worker has laid it out).
    pub(super) fn guide_snippet(
        &mut self,
        ui: &mut Ui,
        text: &str,
        role: ColorRole,
        rtl: bool,
        lines: usize,
    ) {
        let settings: TextSettings = self.session.text;
        let width = ui.available_width().max(size::TEXT_MIN);
        let line = settings.size * settings.line_spacing;
        let laid = self.snippets.get(text, role, rtl, width, &settings);
        let Some(laid) = laid else {
            let (rect, _) = ui.allocate_exact_size(vec2(width, line), Sense::hover());
            crate::components::text_placeholder(
                ui.painter(),
                rect.min,
                width.min(settings.size * scale::PLACEHOLDER_EMS),
                line,
                theme::c().rule,
            );
            ui.ctx().request_repaint();
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
        self.atlas.paint(&painter, &laid, rect.min, &self.palette);
    }

    /// Open a Commentary at a row's section: its Tab if one is open (Ctrl/Cmd-click: always
    /// a new Tab), otherwise a new Tab in the usual place that joins the Guide's Link set.
    /// The Guide and its Link set stay where they are.
    fn open_commentary(&mut self, guide: TabId, row: &CommentaryRow, new_tab: bool) {
        let link = self.session.workspace.tab(guide).and_then(|t| t.link);
        let kind = TabKind::Resource(row.resource_id.clone());
        let existing = (!new_tab)
            .then(|| {
                let ws = &self.session.workspace;
                ws.tabs()
                    .filter(|t| t.kind == kind)
                    .max_by_key(|t| (link.is_some() && t.link == link, t.id))
                    .map(|t| (t.id, t.link))
            })
            .flatten();
        let start = row.section.start();
        match existing {
            Some((tab, tab_link)) => {
                // A member of the Guide's own Link set already shows this section (it follows
                // the Guide's Passage); anything else jumps there.
                if tab_link.is_none() || tab_link != link {
                    self.session.workspace.navigate(tab, start);
                }
                self.session.workspace.focus(tab);
            }
            None => {
                self.session.workspace.focus(guide);
                if let Some(out) = self.run("resource.open", &json!({ "id": row.resource_id }))
                    && let Some(tab) = out["tab"].as_u64()
                {
                    // Placed by `opened` without moving the Link set.
                    self.session.workspace.scrolled(TabId(tab), start);
                }
            }
        }
    }
}

/// A section header as drawn: the header itself (click: collapse, drag: move) and whether
/// its Hide button was clicked.
struct Header {
    body: egui::Response,
    hide: bool,
}

fn section_header(
    ui: &mut Ui,
    tab: TabId,
    guide: Guide,
    section: GuideSection,
    collapsed: bool,
    status: &SectionStatus,
    dragging: bool,
) -> Header {
    let width = ui.available_width();
    let (rect, body) = ui.allocate_exact_size(vec2(width, HEADER_H), Sense::click_and_drag());
    let body = body
        .on_hover_cursor(if dragging {
            egui::CursorIcon::Grabbing
        } else {
            egui::CursorIcon::Grab
        })
        .on_hover_text(format!(
            "Click to collapse or expand · drag to move (every {})",
            guide.title()
        ));
    let c = theme::c();
    let painter = ui.painter_at(rect);
    let fill = if dragging {
        c.selection
    } else if body.hovered() {
        c.hover
    } else {
        c.surface
    };
    painter.rect_filled(rect, radius::S, fill);
    let y = rect.center().y;
    let at =
        |x: f32| Rect::from_center_size(pos2(rect.left() + x, y), egui::Vec2::splat(icons::BODY));
    // Grip, chevron and the section's icon, one icon and a gap apart, then the title.
    let step = icons::BODY + space::XS;
    let (grip_x, chevron_x) = (space::L, space::L + step);
    let icon_x = chevron_x + step;
    icons::paint(&painter, Icon::Grip, at(grip_x), icons::SMALL, c.muted);
    let chevron = if collapsed {
        Icon::ChevronRight
    } else {
        Icon::ChevronDown
    };
    icons::paint(&painter, chevron, at(chevron_x), icons::BODY, c.ink);
    icons::paint(
        &painter,
        icons::for_section(section),
        at(icon_x),
        icons::BODY,
        c.lapis,
    );
    let title = painter.text(
        pos2(rect.left() + icon_x + icons::BODY / 2.0 + space::S, y),
        Align2::LEFT_CENTER,
        section.title(),
        FontId::proportional(text::BODY),
        c.ink,
    );
    let count = match status {
        SectionStatus::Ready(d) if !d.is_empty() && section.counted() => d.count().to_string(),
        _ => String::new(),
    };
    if !count.is_empty() {
        painter.text(
            pos2(title.right() + space::M, y),
            Align2::LEFT_CENTER,
            count,
            FontId::proportional(text::SMALL),
            c.muted,
        );
    }
    body.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Button,
            true,
            format!(
                "{} section, {}",
                section.title(),
                if collapsed { "collapsed" } else { "expanded" }
            ),
        )
    });
    let hide_rect = Rect::from_center_size(
        pos2(rect.right() - space::XL, y),
        egui::Vec2::splat(size::CONTROL_COMPACT),
    );
    let hide = ui
        .interact(
            hide_rect,
            ui.scope_id().with(("guide-hide", tab, section.id())),
            Sense::click(),
        )
        .on_hover_text(format!("Hide this section in every {}", guide.title()));
    icons::paint(
        &painter,
        Icon::Close,
        hide_rect,
        icons::BODY,
        if hide.hovered() { c.ink } else { c.muted },
    );
    Header {
        body,
        hide: hide.clicked(),
    }
}

pub(super) fn hint_page(ui: &mut Ui, text: &str) {
    crate::components::page_frame().show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        ui.label(RichText::new(text).color(theme::c().muted));
    });
}

/// A filterable section's Collection chooser (#153), shown once the user has Collections:
/// "Every Resource" or one Collection, for every Guide of the kind.
fn collection_filter(
    ui: &mut Ui,
    library: &obs_engine::Library,
    guide: Guide,
    section: GuideSection,
    chosen: Option<&str>,
) -> Option<Action> {
    if library.collections().is_empty() {
        return None;
    }
    let mut picked: Option<Option<String>> = None;
    ui.horizontal(|ui| {
        ui.label(crate::components::hint_text("Collection"));
        egui::ComboBox::from_id_salt(("guide-collection", guide.id(), section.id()))
            .selected_text(chosen.unwrap_or("Every Resource"))
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(chosen.is_none(), "Every Resource")
                    .clicked()
                {
                    picked = Some(None);
                }
                for c in library.collections() {
                    let on = chosen == Some(c.name.as_str());
                    if ui.selectable_label(on, c.name.as_str()).clicked() {
                        picked = Some(Some(c.name.clone()));
                    }
                }
            })
            .response
            .on_hover_text(
                "Show only the Resources of one Collection, in every Guide of this kind",
            );
    });
    let picked = picked?;
    (picked.as_deref() != chosen).then(|| {
        Action::Command(
            "guide.filter_section",
            json!({
                "guide": guide.id(),
                "section": section.id(),
                "collection": picked.unwrap_or_default(),
            }),
        )
    })
}
