//! The Factbook Tab (#149): a hub page for one person, people group or place. The header
//! (name, what tells it from others of that name, kind, where the Bible names it), then the
//! Guide's sections through the shared [`State::guide_page`]. A name typed into the box shows
//! the chooser instead, until one entity is chosen. Everything shown is engine data
//! ([`obs_engine::factbook`]); this file only draws it.

use std::collections::BTreeMap;

use egui::{Rect, RichText, Sense, Ui, pos2};
use obs_engine::{
    ArticleLink, DictionaryArticle, EntityCard, EntityLink, FactbookSummary, Guide, GuideSection,
    RelatedEntity, TabId, WordStatus,
};
use obs_layout::ColorRole;
use obs_model::{Direction, Key, ResourceType};
use obs_scripture::{Book, Passage, RefStyle, Standard, format_passage};
use serde_json::json;

use super::State;
use super::guide_tab::hint_page;
use super::guide_words::ROWS_SHOWN;
use crate::theme;
use crate::tokens::{size, space};

/// Lines of a Dictionary article excerpt.
const ARTICLE_LINES: usize = 4;

/// Key Passages shown before "Show all".
const PASSAGES_SHOWN: usize = 40;

/// What a click in a Factbook asks for. `bool`: Ctrl/Cmd held (a new Tab).
pub(super) enum FactbookAction {
    /// Another person or place's Factbook, by People & places id.
    Entity(String, bool),
    /// A Key Passage in a Bible.
    Passage(Passage, bool),
    /// A Dictionary at an article.
    Article(ArticleLink, bool),
    /// Close the chooser.
    StopChoosing,
    /// The no-index hint's button (#135): Get Resources, the Starter library or Import.
    Command(&'static str, serde_json::Value),
}

impl State {
    pub(super) fn factbook_body(&mut self, ui: &mut Ui, tab: TabId) {
        if let Some(name) = self.session.factbooks.choosing(tab).map(str::to_string) {
            self.factbook_chooser_page(ui, tab, &name);
            return;
        }
        let key = self
            .session
            .workspace
            .tab(tab)
            .and_then(|t| t.entry_history.current());
        let Some(Key::Headword(id)) = key else {
            hint_page(
                ui,
                "Type the name of a person or place in the box above (Abraham, John, Bethel), \
                 or click a name in a Passage Guide's People & places.",
            );
            return;
        };
        let card = self.session.factbook_card(&id);
        self.guide_page(
            ui,
            tab,
            Guide::Factbook,
            &Key::Headword(id.clone()),
            &mut |_, ui| {
                factbook_header(ui, &id, &card);
                None
            },
        );
    }

    /// The chooser: every person or place with the typed name, each told apart by its facts.
    /// Click one to show it (Ctrl/Cmd-click: a new Factbook); Cancel or Escape goes back.
    fn factbook_chooser_page(&mut self, ui: &mut Ui, tab: TabId, name: &str) {
        let status = self.session.factbook_chooser(name);
        let can_cancel = self
            .session
            .workspace
            .tab(tab)
            .is_some_and(|t| t.entry_history.current().is_some());
        let new_tab = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
        let mut action: Option<FactbookAction> = None;
        crate::components::page_frame().show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            ui.horizontal(|ui| {
                ui.label(crate::components::title_text(format!("“{name}”")));
                if can_cancel
                    && ui
                        .small_button("Cancel")
                        .on_hover_text("Back to the person or place shown before (Esc)")
                        .clicked()
                {
                    action = Some(FactbookAction::StopChoosing);
                }
            });
            let chooser = match &status {
                WordStatus::Loading => {
                    ui.label(
                        RichText::new(format!("Looking up “{name}”…")).color(theme::c().muted),
                    );
                    ui.ctx().request_repaint();
                    return;
                }
                WordStatus::Failed(e) => {
                    ui.label(
                        RichText::new(format!("Could not look up “{name}”: {e}"))
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
                    .of_type(ResourceType::PeopleAndPlaces)
                    .next()
                    .is_some()
                {
                    ui.label(
                        RichText::new(format!(
                            "No person or place is called “{name}”. Try another spelling \
                                 (Abram, Jacob, Bethlehem)."
                        ))
                        .color(theme::c().muted),
                    );
                } else {
                    // The index the Factbook's sections read is what the chooser needs
                    // too (#135).
                    let missing = GuideSection::Summary.missing_resource(
                        false,
                        None,
                        &self.session.library,
                        self.session.starter.manifest(),
                    );
                    if let Some((id, params)) = self.missing_resource_ui(
                        ui,
                        "No People & places index in your library. Add one (TIPNR) to look \
                             up people and places.",
                        &missing,
                    ) {
                        action = Some(FactbookAction::Command(id, params));
                    }
                }
                return;
            }
            ui.label(RichText::new(chooser_heading(chooser, name)).color(theme::c().muted));
            ui.add_space(space::S);
            egui::ScrollArea::vertical()
                .id_salt(("factbook-chooser", tab))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for c in &chooser.choices {
                        if card_row(ui, c, None) {
                            action = Some(FactbookAction::Entity(c.entity.clone(), new_tab));
                        }
                        ui.add_space(space::XS);
                    }
                });
        });
        if can_cancel && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            action = Some(FactbookAction::StopChoosing);
        }
        if let Some(a) = action {
            self.session.workspace.user_focused(tab);
            self.factbook_action(tab, a);
        }
    }

    /// Summary: other names, what the name means, and family (each relative a link).
    pub(super) fn factbook_summary(
        &mut self,
        ui: &mut Ui,
        sum: &FactbookSummary,
        new_tab: bool,
    ) -> Option<FactbookAction> {
        let mut action = None;
        if !sum.other_names.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.label(crate::components::hint_text("Also called"));
                ui.label(RichText::new(sum.other_names.join(", ")).color(theme::c().ink));
            });
        }
        if let Some(m) = &sum.meaning {
            ui.horizontal_wrapped(|ui| {
                ui.label(crate::components::hint_text("Name means"));
                ui.label(RichText::new(format!("“{}”", m.text)).color(theme::c().ink));
                if crate::components::link(
                    ui,
                    RichText::new(format!("— {}", m.article.abbreviation)).small(),
                )
                .on_hover_text(format!(
                    "Open {} at “{}” · Ctrl/Cmd-click opens a new Tab",
                    m.article.title, m.article.headword
                ))
                .clicked()
                {
                    action = Some(FactbookAction::Article(m.article.clone(), new_tab));
                }
            });
        }
        for fact in &sum.family {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = space::XS;
                ui.label(crate::components::hint_text(fact.label));
                comma_list(ui, &fact.people, |ui, p| {
                    let (r, clicked) = entity_link(ui, p, new_tab);
                    if clicked.is_some() {
                        action = clicked;
                    }
                    r
                });
            });
        }
        action
    }

    /// Key Passages: every Passage that mentions them, by Bible book; click one to read it.
    pub(super) fn factbook_passages(
        &mut self,
        ui: &mut Ui,
        tab: TabId,
        passages: &[Passage],
        new_tab: bool,
    ) -> Option<FactbookAction> {
        let mut by_book: BTreeMap<Book, Vec<Passage>> = BTreeMap::new();
        for p in passages {
            by_book.entry(p.start().book).or_default().push(*p);
        }
        let expanded = self.expanded(tab, GuideSection::KeyPassages);
        let mut left = if expanded {
            passages.len()
        } else {
            PASSAGES_SHOWN
        };
        let mut action = None;
        for (book, list) in &by_book {
            if left == 0 {
                break;
            }
            let list = &list[..list.len().min(left)];
            left -= list.len();
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = space::S;
                let total = by_book.get(book).map_or(0, Vec::len);
                ui.label(
                    RichText::new(format!("{} · {total}", book.name()))
                        .strong()
                        .color(theme::c().ink),
                );
                for p in list {
                    let short = format_passage(*p, &Standard, RefStyle::Short);
                    let local = in_book(*p);
                    if super::unbroken_link(ui, local)
                        .on_hover_text(format!("Read {short} · Ctrl/Cmd-click opens a new Tab"))
                        .clicked()
                    {
                        action = Some(FactbookAction::Passage(*p, new_tab));
                    }
                }
            });
            ui.add_space(space::XXS);
        }
        if passages.len() > PASSAGES_SHOWN {
            let label = if expanded {
                "Show fewer".to_string()
            } else {
                format!("Show all {}", passages.len())
            };
            if ui.small_button(label).clicked()
                && let Some(t) = self.tabs.get_mut(&tab)
                && !t.guide_expanded.remove(&GuideSection::KeyPassages)
            {
                t.guide_expanded.insert(GuideSection::KeyPassages);
            }
        }
        action
    }

    /// Dictionary articles: each Dictionary's article, its first lines (Resource text) and
    /// a link that opens it.
    pub(super) fn factbook_articles(
        &mut self,
        ui: &mut Ui,
        _tab: TabId,
        articles: &[DictionaryArticle],
        new_tab: bool,
    ) -> Option<FactbookAction> {
        let mut action = None;
        for a in articles {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&a.article.title)
                        .strong()
                        .color(theme::c().ink),
                );
                if crate::components::link(
                    ui,
                    format!("{}: {}", a.article.abbreviation, a.article.headword),
                )
                .on_hover_text(format!(
                    "Open {} at “{}” · Ctrl/Cmd-click opens a new Tab",
                    a.article.title, a.article.headword
                ))
                .clicked()
                {
                    action = Some(FactbookAction::Article(a.article.clone(), new_tab));
                }
            });
            if !a.excerpt.is_empty() {
                let rtl = a.direction == Direction::Rtl;
                self.guide_snippet(ui, &a.excerpt, ColorRole::Ink, rtl, ARTICLE_LINES);
            }
            ui.add_space(space::S);
        }
        action
    }

    /// The Map (#150): a place at every location it may have had, each with how confident
    /// that is; for a person, the places they lived in (click one for its Factbook).
    /// `subject` is the Factbook's entity: when the map shows it, it is a place's own map.
    pub(super) fn factbook_map(
        &mut self,
        ui: &mut Ui,
        places: &[obs_engine::MapPlace],
        subject: Option<&str>,
    ) -> Option<FactbookAction> {
        let one_place = matches!(places, [p] if Some(p.entity.as_str()) == subject);
        let refs: Vec<&obs_engine::MapPlace> = places.iter().collect();
        let width = ui.available_width().max(size::TEXT_MIN);
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(width, super::atlas_tab::MINI_MAP_HEIGHT),
            Sense::hover(),
        );
        let mut view =
            super::atlas_tab::fit(&refs, rect, one_place, super::atlas_tab::MINI_MAP_SPAN);
        let description = format!(
            "Map: {}",
            places
                .iter()
                .map(obs_engine::MapPlace::describe)
                .collect::<Vec<_>>()
                .join("; ")
        );
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .id_salt("factbook-map"),
        );
        let input = super::atlas_tab::map_canvas(
            &mut child,
            rect,
            &mut view,
            &super::atlas_tab::MapScene {
                places: &refs,
                selected: None,
                emphasized: &|_| false,
                all_points: one_place,
                interactive: false,
                description: &description,
            },
        );
        let mut action = input
            .clicked
            .filter(|_| !one_place)
            .map(|(entity, new_tab)| FactbookAction::Entity(entity, new_tab));
        ui.add_space(space::XS);
        for place in places {
            if one_place {
                for point in &place.points {
                    ui.label(
                        RichText::new(format!("{} · {}", point.location, point.confidence.label()))
                            .color(theme::c().muted),
                    );
                }
            } else if crate::components::link(ui, place.describe())
                .on_hover_text(format!("Open {}'s Factbook", place.name))
                .clicked()
            {
                let new_tab = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
                action = Some(FactbookAction::Entity(place.entity.clone(), new_tab));
            }
        }
        action
    }

    /// Related people and places: each as a chooser-like row, with how they relate.
    pub(super) fn factbook_related(
        &mut self,
        ui: &mut Ui,
        tab: TabId,
        rows: &[RelatedEntity],
        new_tab: bool,
    ) -> Option<FactbookAction> {
        let shown = if self.expanded(tab, GuideSection::Related) {
            rows.len()
        } else {
            rows.len().min(ROWS_SHOWN)
        };
        let mut action = None;
        for r in &rows[..shown] {
            if card_row(ui, &r.card, Some(&r.why)) {
                action = Some(FactbookAction::Entity(r.card.entity.clone(), new_tab));
            }
            ui.add_space(space::XXS);
        }
        self.show_all(ui, tab, GuideSection::Related, rows.len());
        action
    }

    pub(super) fn factbook_action(&mut self, tab: TabId, action: FactbookAction) {
        match action {
            FactbookAction::Entity(id, true) => {
                self.run("guide.factbook", &json!({ "entity": id, "new_tab": true }));
            }
            FactbookAction::Entity(id, false) => {
                self.run("tab.go", &json!({ "tab": tab.0, "text": id }));
            }
            FactbookAction::StopChoosing => self.session.stop_choosing_factbook(tab),
            FactbookAction::Article(a, new_tab) => self.open_article(tab, &a, new_tab),
            FactbookAction::Passage(p, new_tab) => self.open_key_passage(tab, p, new_tab),
            FactbookAction::Command(id, params) => {
                self.run(id, &params);
            }
        }
    }

    /// Read a Key Passage: the most recently active Bible Tab goes there (its Link set
    /// follows); with Ctrl/Cmd, or with no Bible open, the ★ Bible opens there.
    pub(super) fn open_key_passage(&mut self, factbook: TabId, passage: Passage, new_tab: bool) {
        self.reference_navigate(factbook, passage, new_tab);
    }
}

/// The header: name, what tells it apart, and its kind, region and where it is named.
fn factbook_header(ui: &mut Ui, id: &str, card: &WordStatus<EntityCard>) {
    let name = obs_engine::people_places::display_name(id);
    match card {
        WordStatus::Ready(c) => {
            ui.horizontal_wrapped(|ui| {
                ui.label(crate::components::title_text(&c.name));
                if let Some(d) = &c.distinction {
                    ui.label(RichText::new(d).color(theme::c().ink));
                }
            });
            ui.label(RichText::new(c.details()).color(theme::c().muted));
        }
        WordStatus::Loading => {
            ui.label(crate::components::title_text(name));
            ui.label(RichText::new("Loading…").color(theme::c().muted));
            ui.ctx().request_repaint();
        }
        WordStatus::Failed(e) => {
            ui.label(crate::components::title_text(name));
            ui.label(RichText::new(e).color(theme::c().warn));
        }
    }
}

/// A relative's name as a link to their Factbook (plain text when there is none).
fn entity_link(
    ui: &mut Ui,
    p: &EntityLink,
    new_tab: bool,
) -> (egui::Response, Option<FactbookAction>) {
    let text = if p.uncertain {
        format!("{} (?)", p.name)
    } else {
        p.name.clone()
    };
    if !p.known {
        return (ui.label(RichText::new(text).color(theme::c().ink)), None);
    }
    let tip = if p.uncertain {
        format!(
            "The source is unsure of this link · Open the Factbook on {} · Ctrl/Cmd-click opens a new Tab",
            p.name
        )
    } else {
        format!(
            "Open the Factbook on {} · Ctrl/Cmd-click opens a new Tab",
            p.name
        )
    };
    let r = super::unbroken_link(ui, text).on_hover_text(tip);
    let clicked = r
        .clicked()
        .then(|| FactbookAction::Entity(p.entity.clone(), new_tab));
    (r, clicked)
}

/// Items in a wrapping row, separated by commas that sit right after each item (`Nahor,
/// Haran`): the comma is painted against the item and the row's item spacing follows it, so
/// there is no space before a comma and a comma never wraps onto a line by itself (#261).
fn comma_list<T>(ui: &mut Ui, items: &[T], mut item: impl FnMut(&mut Ui, &T) -> egui::Response) {
    for (i, it) in items.iter().enumerate() {
        let r = item(ui, it);
        if i + 1 < items.len() {
            let font = egui::TextStyle::Body.resolve(ui.style());
            let comma = ui.painter().text(
                pos2(r.rect.right(), r.rect.center().y),
                egui::Align2::LEFT_CENTER,
                ",",
                font,
                theme::c().muted,
            );
            // The row put its item spacing after the item: the comma takes that place, and
            // the spacing follows it.
            ui.add_space(comma.width());
        }
    }
}

/// What the chooser lists: those called `name`, then those whose names start with it (#261).
fn chooser_heading(chooser: &obs_engine::FactbookChooser, name: &str) -> String {
    let all = chooser.choices.len();
    let more = all - chooser.exact.min(all);
    match (chooser.exact, more) {
        (_, 0) => format!("{all} people and places called “{name}”. Choose one:"),
        (0, _) => format!("{all} people and places whose names start with “{name}”. Choose one:"),
        (exact, more) => {
            format!("{exact} called “{name}”, then {more} whose names start with it. Choose one:")
        }
    }
}

/// One person or place as a clickable row: `John (son of Zebedee)`, its details, and `why`
/// (Related) or what else it is called (chooser). Returns whether it was clicked.
fn card_row(ui: &mut Ui, c: &EntityCard, why: Option<&str>) -> bool {
    let top = ui.cursor().min;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(c.title()).strong().color(theme::c().lapis));
        if let Some(why) = why {
            ui.label(RichText::new(why).color(theme::c().ink));
        }
        if let Some(also) = &c.also_called {
            ui.label(
                RichText::new(format!("also called {also}"))
                    .italics()
                    .color(theme::c().ink),
            );
        }
    });
    ui.label(crate::components::hint_text(c.details()));
    let rect = Rect::from_min_max(top, pos2(ui.max_rect().right(), ui.cursor().top()));
    let r = ui
        .interact(
            rect,
            ui.scope_id().with(("factbook-card", &c.entity)),
            Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Click to open its Factbook · Ctrl/Cmd-click opens a new Tab");
    let label = format!("Factbook: {}", c.title());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Link, true, &label));
    if r.hovered() {
        crate::components::hover_outline(ui, rect);
    }
    r.clicked()
}

/// A Passage without its book, to sit beside the book name: `Gen 12:1` → `12:1`, `1 Sam
/// 17:45` → `17:45`. A whole single-chapter book keeps its name.
fn in_book(p: Passage) -> String {
    let short = format_passage(p, &Standard, RefStyle::Short);
    let book = p.start().book.abbrev();
    short
        .strip_prefix(book)
        .and_then(|rest| rest.strip_prefix(' '))
        .map_or_else(|| short.clone(), str::to_string)
}

#[cfg(test)]
mod tests {
    use obs_scripture::Verse;

    use super::*;

    /// #261 GUIDES-17: `Nahor, Haran, Sarah` — no space before a comma, one after it.
    #[test]
    fn comma_list_puts_each_comma_right_after_its_item() {
        let ctx = egui::Context::default();
        let names = ["Nahor", "Haran", "Sarah"];
        let mut rects: Vec<Rect> = Vec::new();
        let (mut comma_width, mut haran_width) = (0.0, 0.0);
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let width = |text: &str| {
                let font = egui::TextStyle::Body.resolve(ui.style());
                (ui.painter())
                    .layout_no_wrap(text.into(), font, egui::Color32::PLACEHOLDER)
                    .size()
                    .x
            };
            comma_width = width(",");
            haran_width = width("Haran");
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = space::XS;
                comma_list(ui, &names, |ui, n| {
                    let r = ui.label(*n);
                    rects.push(r.rect);
                    r
                });
            });
        });
        out.textures_delta.clear();
        // A label in a wrapping row is laid out from the row's start: its comma is the
        // galley's last `comma_width` points.
        let commas: Vec<Rect> = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) if t.galley.text() == "," => {
                    let r = t.galley.rect.translate(t.pos.to_vec2());
                    Some(Rect::from_x_y_ranges(
                        r.right() - comma_width..=r.right(),
                        r.y_range(),
                    ))
                }
                _ => None,
            })
            .collect();
        assert_eq!((rects.len(), commas.len()), (3, 2));
        for (name, comma) in rects.iter().zip(&commas) {
            assert!(
                (comma.left() - name.right()).abs() < 0.5,
                "comma {comma:?} apart from {name:?}"
            );
        }
        let haran_left = rects[1].right() - haran_width;
        assert!(
            (haran_left - commas[0].right() - 4.0).abs() < 0.5,
            "the item spacing after the comma: {haran_left} vs {:?}",
            commas[0]
        );
    }

    /// #261 GUIDES-13: the chooser says which are called the name and which only start
    /// with it.
    #[test]
    fn the_chooser_heading_tells_exact_names_from_longer_ones() {
        let card = |name: &str| EntityCard {
            entity: format!("{name}@Gen.1.1"),
            name: name.into(),
            distinction: None,
            kind: obs_model::EntityKind::Place,
            region: None,
            passages: 1,
            first: None,
            last: None,
            also_called: None,
        };
        let chooser = |exact, names: &[&str]| obs_engine::FactbookChooser {
            query: "Beth".into(),
            choices: names.iter().map(|n| card(n)).collect(),
            exact,
        };
        assert_eq!(
            chooser_heading(&chooser(1, &["Beth-eked", "Bethel", "Bethlehem"]), "Beth"),
            "1 called “Beth”, then 2 whose names start with it. Choose one:"
        );
        assert_eq!(
            chooser_heading(&chooser(0, &["Bethel", "Bethlehem"]), "Beth"),
            "2 people and places whose names start with “Beth”. Choose one:"
        );
        assert_eq!(
            chooser_heading(&chooser(2, &["Antioch", "Antioch"]), "Antioch"),
            "2 people and places called “Antioch”. Choose one:"
        );
    }

    #[test]
    fn in_book_drops_the_whole_book_abbreviation() {
        let at = |b, c, v| in_book(Passage::verse(Verse::new(b, c, v)));
        assert_eq!(at(Book::Genesis, 12, 1), "12:1");
        assert_eq!(at(Book::FirstSamuel, 17, 45), "17:45");
        assert_eq!(at(Book::SongOfSongs, 1, 1), "1:1");
    }
}
