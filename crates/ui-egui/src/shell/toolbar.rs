//! The toolbar Launchers (#140): Library, Guides, Tools and Layouts as icon-and-label buttons
//! left of the View and Help menus, each dropping down a pane anchored to it. What the
//! panes list comes from the engine ([`obs_engine::toolbar`]); how they look is
//! `toolbar_look.rs`.
//!
//! Keyboard first: Alt+L, Alt+G, Alt+T and Alt+Y open them (the `toolbar.*` commands). The
//! keyboard goes to the pane's field: typing filters (searches, in the Library), ↑/↓ pick a row
//! (in the Layout cards, while the filter is empty, the card above or below, and ←/→ too),
//! Enter runs it (Ctrl/Cmd+Enter: a Resource in a new Tab), Esc closes. Enter is resolved
//! after the field takes this frame's typing, so text and Enter in one frame run what was typed. In the Layouts pane F2 renames the picked saved Layout
//! and Delete deletes it. A click outside closes the pane.
//!
//! - **Library:** click or Enter opens a Resource in the focused Panel (bringing its Tab to the
//!   front if that Panel has one); Ctrl/Cmd-click always opens a new Tab, split off beside it;
//!   dragging a row onto a Panel docks it there, onto a Panel's right edge in a new Panel
//!   beside it (`toolbar.open_resource`).
//! - **Guides:** each Guide with its own input box; Enter opens the Guide at what was typed.
//! - **Tools:** the Tools menu's commands.
//! - **Layouts:** saved and Built-in Layouts as cards with a thumbnail of their tiling; one
//!   click loads one; Rename and Delete in place.
//!
//! The toolbar can sit on any window edge (#141, `toolbar_bar.rs`): a pane drops down under its
//! button at the Top, up from it at the Bottom, and out to the side at Left and Right. A
//! Launcher hidden in Customize toolbar… (or in the » button of a small window) drops its pane
//! from the toolbar's start (or the » button).

use std::collections::{BTreeMap, HashMap};

use egui::{Key, Modifiers, Rect, Sense, Ui, pos2, vec2};
use obs_engine::{Launcher, LayoutCard, LibraryLauncherSection, MenuItem};
use serde_json::{Value, json};

use super::Shell;
use super::toolbar_look as look;

/// The Library pane's search field.
pub(crate) const SEARCH_HINT: &str = "Find a Resource: title, abbreviation, type, language";
/// The Tools and Layouts panes' filter fields.
pub(crate) const TOOLS_HINT: &str = "Filter tools";
pub(crate) const LAYOUTS_HINT: &str = "Filter layouts";

/// How far into a Panel's right edge a dropped Resource splits off a new Panel, as a share
/// of its width.
const DROP_EDGE: f32 = 0.25;

/// The toolbar's state between frames.
#[derive(Default)]
pub(crate) struct ToolbarUi {
    pub(crate) open: Option<Launcher>,
    /// The open pane's search or filter text.
    pub(crate) query: String,
    /// The picked row (or card), in the order the pane lists them.
    pub(crate) selected: usize,
    /// Put the keyboard in the pane's field next frame.
    focus: bool,
    /// The keyboard moved the pick: scroll it into view.
    scroll: bool,
    /// Guides: what is typed in each Guide's box, by command id.
    pub(crate) guide_inputs: BTreeMap<&'static str, String>,
    /// Layouts: the cards, read from the User database when the pane opens.
    cards: Option<Vec<LayoutCard>>,
    /// A saved Layout being renamed in place, and the name being typed.
    pub(crate) renaming: Option<(String, String)>,
    /// Where each launcher and the open pane were drawn last frame.
    pub(crate) buttons: HashMap<Launcher, Rect>,
    pub(crate) pane: Option<Rect>,
    /// A Resource being dragged out of the Library pane: its id and abbreviation.
    pub(crate) dragging: Option<(String, String)>,
    /// Where the toolbar's Launchers were drawn this frame (none: not drawn, Auto-hide), and
    /// the » button holding those a small window has no room for (#141).
    pub(crate) bar: Option<Rect>,
    pub(crate) overflow: Option<Rect>,
    /// The bottom of the menu bar, where the Workspace starts.
    pub(crate) menus_bottom: f32,
    /// Top position: the menu bar's width besides the Launchers (View, Help, the Command box …),
    /// measured last frame, so the Launchers know their room.
    pub(crate) reserve: f32,
    /// The edge the toolbar's grip is being dragged to.
    pub(crate) grip_target: Option<obs_engine::ToolbarPosition>,
    /// Where the toolbar was drawn last frame.
    pub(crate) last_bar: Option<Rect>,
    /// Auto-hide: the toolbar is showing.
    pub(crate) revealed: bool,
    /// A menu of the toolbar (right-click, ») is open this frame, and was last frame.
    pub(crate) menu_open: bool,
    pub(crate) menu_was_open: bool,
    /// The Customize toolbar window is open.
    pub(crate) customizing: bool,
    /// Where the open pane is placed (its corner `pivot` at `at`), and how tall it may grow.
    pub(crate) pane_at: Option<(egui::Pos2, egui::Align2)>,
    pub(crate) pane_span: f32,
}

impl ToolbarUi {
    /// Open `which`, or close it if it is open.
    pub(crate) fn toggle(&mut self, which: Launcher) {
        if self.open == Some(which) {
            self.close();
        } else {
            self.query.clear();
            self.selected = 0;
            self.focus = true;
            self.scroll = true;
            self.cards = None;
            self.renaming = None;
            self.open = Some(which);
        }
    }

    /// Forget where the toolbar was drawn: each frame draws it again.
    pub(crate) fn begin_frame(&mut self) {
        self.buttons.clear();
        self.last_bar = self.bar.take();
        self.overflow = None;
        self.menu_was_open = std::mem::take(&mut self.menu_open);
    }

    /// Where the open pane goes: its corner `pivot` at a point.
    fn pane_place(&self) -> (egui::Pos2, egui::Align2) {
        self.pane_at
            .unwrap_or((egui::Pos2::ZERO, egui::Align2::LEFT_TOP))
    }

    pub(crate) fn close(&mut self) {
        self.open = None;
        self.pane = None;
        self.renaming = None;
    }
}

/// Something a pane lists that Enter or a click runs.
#[derive(Debug, Clone, PartialEq)]
enum Item {
    Resource(String),
    Favorite(i64),
    /// A command with no parameters (a Tool, a Guide, a footer link).
    Command(&'static str),
    /// A Layout card, by index into the cards.
    Layout(usize),
}

/// What the user did in a pane this frame.
enum Action {
    Run(Item, bool),
    /// Enter (`true`: with Ctrl/Cmd), before this frame's typing is in: resolved to the
    /// picked row once the pane's field has taken it (see `Shell::submit`).
    Submit(bool),
    Close,
    Rename(String, String),
    Delete(String),
    Drag(String, String),
}

/// One Library pane row, as drawn.
struct ResourceRow {
    item: Item,
    abbreviation: String,
    title: String,
    detail: String,
}

impl Shell {
    /// The open launcher's pane, by its button; and a Resource being dragged out of it.
    pub(super) fn launcher_pane(&mut self, ui: &mut Ui) {
        self.library_drag(ui);
        let Some(which) = self.st.toolbar.open else {
            return;
        };
        // A press outside the pane and the launchers closes it.
        let tb = &self.st.toolbar;
        let outside = ui.input(|i| {
            i.pointer.any_pressed()
                && i.pointer.press_origin().is_some_and(|p| {
                    !tb.pane.is_some_and(|r| r.contains(p))
                        && !tb.buttons.values().any(|r| r.contains(p))
                })
        });
        if outside {
            self.st.toolbar.close();
            return;
        }
        let anchor = self.pane_anchor(ui, which);
        self.place_pane(ui, anchor);
        let action = match which {
            Launcher::Library => self.library_pane(ui),
            Launcher::Guides => self.guides_pane(ui),
            Launcher::Tools => self.tools_pane(ui),
            Launcher::Layouts => self.layouts_pane(ui),
        };
        match action {
            Some(Action::Run(item, new_tab)) => self.run_item(item, new_tab),
            Some(Action::Close) => self.st.toolbar.close(),
            // Each pane resolves its own Enter; one left over had no row to run.
            Some(Action::Submit(_)) => {}
            Some(Action::Rename(name, to)) => {
                let renamed = self.run("layout.rename", &json!({ "name": name, "to": to }));
                if renamed.is_some() {
                    self.st.toolbar.renaming = None;
                    self.st.toolbar.cards = None;
                    self.st.toolbar.focus = true;
                }
            }
            Some(Action::Delete(name)) => {
                self.run("layout.delete", &json!({ "name": name }));
                let tb = &mut self.st.toolbar;
                tb.cards = None;
                tb.selected = tb.selected.saturating_sub(1);
                tb.focus = true;
            }
            Some(Action::Drag(id, abbreviation)) => {
                self.st.toolbar.dragging = Some((id, abbreviation));
            }
            None => {}
        }
    }

    /// Run what a pane's row stands for, then close the pane.
    fn run_item(&mut self, item: Item, new_tab: bool) {
        let which = self.st.toolbar.open;
        self.st.toolbar.close();
        match item {
            Item::Resource(id) => {
                self.run(
                    "toolbar.open_resource",
                    &json!({ "id": id, "new_tab": new_tab }),
                );
            }
            Item::Favorite(id) => {
                self.run("favorite.open", &json!({ "id": id, "split": new_tab }));
            }
            Item::Command(id) if which == Some(Launcher::Guides) => {
                let text = self
                    .st
                    .toolbar
                    .guide_inputs
                    .get(id)
                    .map(|t| t.trim().to_string())
                    .unwrap_or_default();
                if id == "guide.word_study" && !text.is_empty() {
                    self.word_study_at(&text);
                } else {
                    let (command, params) = guide_with(id, &text);
                    self.run(command, &params);
                }
                if let Some(t) = self.st.toolbar.guide_inputs.get_mut(id) {
                    t.clear();
                }
            }
            Item::Command(id) => {
                let splits = (self.st.session.commands().get(id))
                    .is_some_and(|c| c.params.iter().any(|p| p.name == "split"));
                let params = if new_tab && splits {
                    json!({ "split": true })
                } else {
                    json!({})
                };
                self.run(id, &params);
            }
            Item::Layout(i) => {
                let card = self
                    .st
                    .toolbar
                    .cards
                    .as_ref()
                    .and_then(|c| c.get(i))
                    .cloned();
                self.st.toolbar.cards = None;
                let Some(card) = card else {
                    return;
                };
                if let Some(why) = card.unavailable {
                    self.st.toast(format!("{}: {why}", card.name));
                } else if card.builtin {
                    self.run("layout.builtin", &json!({ "name": card.name }));
                } else {
                    self.run("layout.restore", &json!({ "name": card.name }));
                }
            }
        }
    }

    /// Word Study at what was typed: a Strong's number, a Lemma or its transliteration, or
    /// English words (the chooser), read as the Command box reads `ws …`.
    fn word_study_at(&mut self, text: &str) {
        let s = &mut self.st.session;
        let best = s
            .command_box_suggestions(&format!("ws {text}"))
            .into_iter()
            .find(|g| matches!(g.action.kind(), "word_study" | "word_study_words"));
        let ran = match best {
            Some(g) => s.run_command_box_action(&g.action, false).map(|_| ()),
            None => s
                .run("word_study.choose", &json!({ "words": text }))
                .map(|_| ()),
        };
        if let Err(e) = ran {
            self.st.toast(e.to_string());
        }
        self.apply_effects();
    }

    /// Keys every pane shares: ↑/↓ (in `grid`, rows of item indices, to the row above or
    /// below; else one item), ←/→ if `arrows_lr`, Enter and Esc. Returns the action for
    /// Enter ([`Action::Submit`], for the pane to resolve after its field) or Esc. They are
    /// taken before the pane's field so it does not see them.
    fn pane_keys(
        &mut self,
        ui: &Ui,
        len: usize,
        grid: Option<&[Vec<usize>]>,
        arrows_lr: bool,
    ) -> Option<Action> {
        let tb = &mut self.st.toolbar;
        let last = len.saturating_sub(1);
        let mut action = None;
        ui.input_mut(|i| {
            let before = tb.selected;
            if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                tb.selected = match grid {
                    Some(rows) => grid_step(rows, tb.selected, true),
                    None => (tb.selected + 1).min(last),
                };
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                tb.selected = match grid {
                    Some(rows) => grid_step(rows, tb.selected, false),
                    None => tb.selected.saturating_sub(1),
                };
            }
            if arrows_lr {
                if i.consume_key(Modifiers::NONE, Key::ArrowRight) {
                    tb.selected = (tb.selected + 1).min(last);
                }
                if i.consume_key(Modifiers::NONE, Key::ArrowLeft) {
                    tb.selected = tb.selected.saturating_sub(1);
                }
            }
            if tb.selected != before {
                tb.scroll = true;
            }
            if i.consume_key(Modifiers::COMMAND, Key::Enter) {
                action = Some(Action::Submit(true));
            } else if i.consume_key(Modifiers::NONE, Key::Enter) {
                action = Some(Action::Submit(false));
            }
            if i.consume_key(Modifiers::NONE, Key::Escape) {
                action = Some(Action::Close);
            }
        });
        if tb.selected > last {
            tb.selected = last;
        }
        action
    }

    /// Enter resolved against `items`, the pane's rows as they are after this frame's
    /// typing (text and Enter can arrive in one frame): the picked row to run.
    fn submit(&mut self, action: Option<Action>, items: &[Item]) -> Option<Action> {
        match action {
            Some(Action::Submit(new_tab)) => {
                let tb = &mut self.st.toolbar;
                tb.selected = tb.selected.min(items.len().saturating_sub(1));
                (items.get(tb.selected)).map(|it| Action::Run(it.clone(), new_tab))
            }
            other => other,
        }
    }

    /// The pane's search or filter field; typing in it starts the pick again at the top.
    fn pane_field(&mut self, ui: &mut Ui, hint: &str, id: &str) {
        let tb = &mut self.st.toolbar;
        let before = tb.query.clone();
        let field_id = egui::Id::unique(id);
        let resp = ui
            .add(
                egui::TextEdit::singleline(&mut tb.query)
                    .id(field_id)
                    .hint_text(hint)
                    .desired_width(f32::INFINITY)
                    .margin(egui::Margin::symmetric(8, 5)),
            )
            .accessible_name(hint);
        if std::mem::take(&mut tb.focus) {
            resp.request_focus();
        }
        if tb.query != before {
            tb.selected = 0;
        }
    }

    /// The Library pane's sections for `query`: search results or Recent and Preferred rows,
    /// and the former Library menu's Favorites with nothing typed.
    fn library_sections(&self, query: &str) -> Vec<(String, Vec<ResourceRow>)> {
        let s = &self.st.session;
        let mut sections: Vec<(String, Vec<ResourceRow>)> = s
            .library_launcher(query)
            .into_iter()
            .map(|(section, rows)| {
                let heading = section.heading().to_string();
                let rows = rows
                    .into_iter()
                    .map(|r| ResourceRow {
                        detail: match section {
                            LibraryLauncherSection::Preferred(_) => r.language.clone(),
                            _ => r.detail(),
                        },
                        item: Item::Resource(r.resource_id),
                        abbreviation: r.abbreviation,
                        title: r.title,
                    })
                    .collect();
                (heading, rows)
            })
            .collect();
        let favorites: Vec<ResourceRow> = if query.trim().is_empty() {
            s.favorites()
                .iter()
                .take(4)
                .map(|f| ResourceRow {
                    item: Item::Favorite(f.id),
                    abbreviation: "★".into(),
                    title: f.label.clone(),
                    detail: f.folder.clone(),
                })
                .collect()
        } else {
            Vec::new()
        };
        if !favorites.is_empty() {
            let at = usize::from(sections.first().is_some_and(|(h, _)| h == "Recent"));
            sections.insert(at, ("Favorites".into(), favorites));
        }
        sections
    }

    /// The Library pane: search, Recent and Preferred rows, Favorites, footer links.
    fn library_pane(&mut self, ui: &mut Ui) -> Option<Action> {
        let span = self.st.toolbar.pane_span;
        let query = self.st.toolbar.query.clone();
        let sections = self.library_sections(&query);
        let footer = self.footer(Launcher::Library);
        let library_items = |sections: &[(String, Vec<ResourceRow>)]| -> Vec<Item> {
            (sections.iter())
                .flat_map(|(_, rows)| rows.iter().map(|r| r.item.clone()))
                .chain(footer.iter().map(|(id, _, _)| Item::Command(id)))
                .collect()
        };
        let items = library_items(&sections);
        let mut action = self.pane_keys(ui, items.len(), None, false);
        let tb_id = egui::Id::unique("launcher-pane");
        let shown = egui::Area::new(tb_id)
            .order(egui::Order::Foreground)
            .pivot(self.st.toolbar.pane_place().1)
            .fixed_pos(self.st.toolbar.pane_place().0)
            .fade_in(false)
            .show(ui.ctx(), |ui| {
                look::dropdown_frame(ui).show(ui, |ui| {
                    ui.set_min_width(look::LIBRARY_WIDTH);
                    ui.set_max_width(look::LIBRARY_WIDTH);
                    self.pane_field(ui, SEARCH_HINT, "launcher-search");
                    let tb = &mut self.st.toolbar;
                    let selected = tb.selected;
                    let scroll = std::mem::take(&mut tb.scroll);
                    let mut index = 0;
                    egui::ScrollArea::vertical()
                        .id_salt("library-rows")
                        .max_height(look::LIBRARY_LIST.min(list_room(ui, look::ROW * 2.0, span)))
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            if sections.is_empty() {
                                ui.add_space(6.0);
                                ui.label(
                                    egui::RichText::new(format!(
                                        "No Resource matches “{}”",
                                        query.trim()
                                    ))
                                    .color(look::look().muted),
                                );
                            }
                            for (heading, rows) in &sections {
                                look::heading(ui, heading);
                                for row in rows {
                                    let (rect, resp) = ui.allocate_exact_size(
                                        vec2(ui.available_width(), look::ROW),
                                        Sense::click_and_drag(),
                                    );
                                    let name = format!("{} — {}", row.abbreviation, row.title);
                                    resp.widget_info(|| {
                                        egui::WidgetInfo::labeled(egui::Role::Button, true, &name)
                                    });
                                    look::row_background(
                                        ui,
                                        rect,
                                        index == selected,
                                        resp.hovered(),
                                    );
                                    look::resource_row(
                                        ui,
                                        rect,
                                        &row.abbreviation,
                                        &row.title,
                                        &row.detail,
                                    );
                                    if index == selected && scroll {
                                        resp.scroll_to_me(None);
                                    }
                                    if resp.clicked() {
                                        let ctrl = ui.input(|i| i.modifiers.command);
                                        action = Some(Action::Run(row.item.clone(), ctrl));
                                    }
                                    if resp.drag_started()
                                        && let Item::Resource(id) = &row.item
                                    {
                                        action = Some(Action::Drag(
                                            id.clone(),
                                            row.abbreviation.clone(),
                                        ));
                                    }
                                    index += 1;
                                }
                            }
                        });
                    ui.add_space(4.0);
                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        for (id, label, enabled) in &footer {
                            let r = ui.add_enabled_ui(*enabled, |ui| {
                                look::footer_link(ui, label, index == selected)
                            });
                            if r.inner.clicked() {
                                action = Some(Action::Run(Item::Command(id), false));
                            }
                            index += 1;
                        }
                    });
                });
            });
        self.st.toolbar.pane = Some(shown.response.rect);
        if matches!(action, Some(Action::Submit(_))) {
            let query = self.st.toolbar.query.clone();
            let items = library_items(&self.library_sections(&query));
            action = self.submit(action, &items);
        }
        action
    }

    /// A pane's footer links: the menu's commands that are not rows of their own, labelled
    /// as the menu labels them.
    fn footer(&self, which: Launcher) -> Vec<(&'static str, String, bool)> {
        let s = &self.st.session;
        which
            .menu()
            .items
            .iter()
            .filter_map(|i| match i {
                MenuItem::Command(id) => Some(*id),
                _ => None,
            })
            .filter_map(|id| {
                let spec = s.commands().get(id)?;
                let label = match id {
                    "library.open" => "Open full Library".to_string(),
                    "library.get_resources" => match s.resource_updates().updates.len() {
                        0 => spec.label.to_string(),
                        1 => format!("{} (1 update)", spec.label),
                        n => format!("{} ({n} updates)", spec.label),
                    },
                    _ => spec.label.to_string(),
                };
                Some((id, label, s.is_enabled(id)))
            })
            .collect()
    }

    /// The Tools menu's commands whose label holds `query`: id, label, shortcut, enabled.
    fn tool_rows(&self, query: &str) -> Vec<(&'static str, &'static str, &'static str, bool)> {
        let needle = query.trim().to_lowercase();
        self.footer(Launcher::Tools)
            .into_iter()
            .filter_map(|(id, _, enabled)| {
                let spec = self.st.session.commands().get(id)?;
                Some((id, spec.label, spec.shortcut.unwrap_or_default(), enabled))
            })
            .filter(|(_, label, _, _)| needle.is_empty() || label.to_lowercase().contains(&needle))
            .collect()
    }

    /// The Tools pane: a filter and the Tools menu's commands with their shortcuts.
    fn tools_pane(&mut self, ui: &mut Ui) -> Option<Action> {
        let rows = self.tool_rows(&self.st.toolbar.query);
        let items: Vec<Item> = rows.iter().map(|(id, ..)| Item::Command(id)).collect();
        let mut action = self.pane_keys(ui, items.len(), None, false);
        let shown = egui::Area::new(egui::Id::unique("launcher-pane"))
            .order(egui::Order::Foreground)
            .pivot(self.st.toolbar.pane_place().1)
            .fixed_pos(self.st.toolbar.pane_place().0)
            .fade_in(false)
            .show(ui.ctx(), |ui| {
                look::dropdown_frame(ui).show(ui, |ui| {
                    ui.set_min_width(look::TOOLS_WIDTH);
                    ui.set_max_width(look::TOOLS_WIDTH);
                    self.pane_field(ui, TOOLS_HINT, "launcher-filter");
                    let selected = self.st.toolbar.selected;
                    ui.add_space(4.0);
                    if rows.is_empty() {
                        ui.label(egui::RichText::new("No tool matches").color(look::look().muted));
                    }
                    for (i, (id, label, shortcut, enabled)) in rows.iter().enumerate() {
                        let (rect, resp) = ui.allocate_exact_size(
                            vec2(ui.available_width(), look::ROW),
                            Sense::click(),
                        );
                        resp.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::Role::Button, *enabled, *label)
                        });
                        look::row_background(ui, rect, i == selected, resp.hovered());
                        look::plain_row(ui, rect, label, shortcut, *enabled);
                        if resp.clicked() && *enabled {
                            let ctrl = ui.input(|i| i.modifiers.command);
                            action = Some(Action::Run(Item::Command(id), ctrl));
                        }
                    }
                });
            });
        self.st.toolbar.pane = Some(shown.response.rect);
        if matches!(action, Some(Action::Submit(_))) {
            let rows = self.tool_rows(&self.st.toolbar.query);
            let items: Vec<Item> = rows.iter().map(|(id, ..)| Item::Command(id)).collect();
            action = self.submit(action, &items);
        }
        action
    }

    /// The Guides pane: each Guide with an input box for its Key.
    fn guides_pane(&mut self, ui: &mut Ui) -> Option<Action> {
        let rows: Vec<(&'static str, String)> = self
            .footer(Launcher::Guides)
            .into_iter()
            .map(|(id, label, _)| (id, label))
            .collect();
        let items: Vec<Item> = rows.iter().map(|(id, _)| Item::Command(id)).collect();
        let before = self.st.toolbar.selected;
        let mut action = self.pane_keys(ui, items.len(), None, false);
        if self.st.toolbar.selected != before {
            self.st.toolbar.focus = true;
        }
        let shown = egui::Area::new(egui::Id::unique("launcher-pane"))
            .order(egui::Order::Foreground)
            .pivot(self.st.toolbar.pane_place().1)
            .fixed_pos(self.st.toolbar.pane_place().0)
            .fade_in(false)
            .show(ui.ctx(), |ui| {
                look::dropdown_frame(ui).show(ui, |ui| {
                    ui.set_min_width(look::GUIDES_WIDTH);
                    ui.set_max_width(look::GUIDES_WIDTH);
                    let tb = &mut self.st.toolbar;
                    let focus = std::mem::take(&mut tb.focus);
                    for (i, (id, label)) in rows.iter().enumerate() {
                        let selected = i == tb.selected;
                        let (rect, _) = ui.allocate_exact_size(
                            vec2(ui.available_width(), look::ROW + 8.0),
                            Sense::hover(),
                        );
                        let label_rect = Rect::from_min_size(rect.min, vec2(150.0, rect.height()));
                        let resp = ui.interact(
                            label_rect,
                            ui.scope_id().with(("guide", *id)),
                            Sense::click(),
                        );
                        resp.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::Role::Button, true, label)
                        });
                        look::row_background(ui, rect, selected, resp.hovered());
                        look::plain_row(ui, label_rect, label, "", true);
                        let input = tb.guide_inputs.entry(id).or_default();
                        let field = Rect::from_min_max(
                            pos2(label_rect.right(), rect.top() + 5.0),
                            pos2(rect.right() - 8.0, rect.bottom() - 5.0),
                        );
                        let edit = ui
                            .put(
                                field,
                                egui::TextEdit::singleline(input)
                                    .id(egui::Id::unique(("guide-input", *id)))
                                    .hint_text(guide_hint(id))
                                    .margin(egui::Margin::symmetric(8, 4)),
                            )
                            .accessible_name(format!("{label}: {}", guide_hint(id)));
                        if selected && focus {
                            edit.request_focus();
                        }
                        if edit.gained_focus() {
                            tb.selected = i;
                        }
                        if resp.clicked() {
                            action = Some(Action::Run(Item::Command(id), false));
                        }
                    }
                    ui.add_space(2.0);
                    ui.label(
                        egui::RichText::new("Enter opens the Guide at what you typed")
                            .small()
                            .color(look::look().muted),
                    );
                });
            });
        self.st.toolbar.pane = Some(shown.response.rect);
        // The Guide's input is read when it runs, so this frame's typing is in.
        self.submit(action, &items)
    }

    /// The Layouts pane: saved and Built-in Layouts as cards, then Save and Reset.
    fn layouts_pane(&mut self, ui: &mut Ui) -> Option<Action> {
        let span = self.st.toolbar.pane_span;
        if self.st.toolbar.cards.is_none() {
            self.st.toolbar.cards = Some(self.st.session.layout_cards());
        }
        let needle = self.st.toolbar.query.trim().to_lowercase();
        let cards = self.st.toolbar.cards.clone().unwrap_or_default();
        let footer = self.footer(Launcher::Layouts);
        let shown_cards = cards_matching(&cards, &needle);
        let items = layout_items(&shown_cards, &footer);
        let renaming = self.st.toolbar.renaming.is_some();
        let mut action = None;
        // Enter while renaming: the new name, read once this frame's typing is in.
        let mut rename = false;
        if renaming {
            ui.input_mut(|i| {
                if i.consume_key(Modifiers::NONE, Key::Escape) {
                    // Esc while renaming only stops renaming.
                    self.st.toolbar.renaming = None;
                    self.st.toolbar.focus = true;
                }
                rename = i.consume_key(Modifiers::NONE, Key::Enter);
            });
        } else {
            action = if needle.is_empty() {
                // ↑/↓ keep the column across the rows of cards as drawn.
                let grid = layout_grid(&cards, &shown_cards, footer.len());
                self.pane_keys(ui, items.len(), Some(&grid), true)
            } else {
                // ←/→ move in the filter text, so ↑/↓ walk every match one at a time.
                self.pane_keys(ui, items.len(), None, false)
            };
            let picked = items.get(self.st.toolbar.selected).cloned();
            if let Some(Item::Layout(i)) = picked
                && !cards[i].builtin
            {
                let name = cards[i].name.clone();
                ui.input_mut(|inp| {
                    if inp.consume_key(Modifiers::NONE, Key::F2) {
                        self.st.toolbar.renaming = Some((name.clone(), name.clone()));
                    }
                    // Delete edits the filter while it has text, typed, pasted or composed
                    // (IME) this frame too.
                    let typing = inp.events.iter().any(|e| {
                        matches!(
                            e,
                            egui::Event::Text(_) | egui::Event::Paste(_) | egui::Event::Ime(_)
                        )
                    });
                    if needle.is_empty() && !typing && inp.consume_key(Modifiers::NONE, Key::Delete)
                    {
                        action = Some(Action::Delete(name.clone()));
                    }
                });
            }
        }
        let shown = egui::Area::new(egui::Id::unique("launcher-pane"))
            .order(egui::Order::Foreground)
            .pivot(self.st.toolbar.pane_place().1)
            .fixed_pos(self.st.toolbar.pane_place().0)
            .fade_in(false)
            .show(ui.ctx(), |ui| {
                look::dropdown_frame(ui).show(ui, |ui| {
                    let width = look::CARDS_ACROSS as f32 * (look::CARD.x + 8.0) - 8.0;
                    ui.set_min_width(width);
                    ui.set_max_width(width);
                    if !renaming {
                        self.pane_field(ui, LAYOUTS_HINT, "launcher-filter");
                    }
                    let selected = self.st.toolbar.selected;
                    let scroll = std::mem::take(&mut self.st.toolbar.scroll);
                    let (saved, builtin): (Vec<_>, Vec<_>) = shown_cards
                        .iter()
                        .copied()
                        .enumerate()
                        .partition(|(_, c)| !cards[*c].builtin);
                    // Saved Layouts can be many: the cards scroll, the footer stays in view.
                    egui::ScrollArea::vertical()
                        .id_salt("layout-cards")
                        .max_height(list_room(ui, look::CARD.y, span))
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            look::heading(ui, "Saved layouts");
                            if saved.is_empty() {
                                ui.label(
                                    egui::RichText::new(if needle.is_empty() {
                                        "None yet: Save layout as… keeps the Workspace as it is now"
                                    } else {
                                        "No saved layout matches"
                                    })
                                    .color(look::look().muted),
                                );
                            }
                            if let Some(a) = self.card_grid(ui, &cards, &saved, selected, scroll) {
                                action = Some(a);
                            }
                            look::heading(ui, "Built-in");
                            if let Some(a) = self.card_grid(ui, &cards, &builtin, selected, scroll)
                            {
                                action = Some(a);
                            }
                        });
                    ui.add_space(4.0);
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        for (n, (id, label, enabled)) in footer.iter().enumerate() {
                            let index = shown_cards.len() + n;
                            let r = ui.add_enabled_ui(*enabled, |ui| {
                                look::footer_link(ui, label, index == selected)
                            });
                            if r.inner.clicked() {
                                action = Some(Action::Run(Item::Command(id), false));
                            }
                        }
                    });
                });
            });
        self.st.toolbar.pane = Some(shown.response.rect);
        if rename && let Some((name, to)) = self.st.toolbar.renaming.clone() {
            action = Some(Action::Rename(name, to.trim().to_string()));
        }
        if matches!(action, Some(Action::Submit(_))) {
            let needle = self.st.toolbar.query.trim().to_lowercase();
            let items = layout_items(&cards_matching(&cards, &needle), &footer);
            action = self.submit(action, &items);
        }
        action
    }

    /// One grid of Layout cards: `(position in the pane's list, card index)` each. `scroll`:
    /// the keyboard moved the pick, so scroll its card into view.
    fn card_grid(
        &mut self,
        ui: &mut Ui,
        cards: &[LayoutCard],
        shown: &[(usize, usize)],
        selected: usize,
        scroll: bool,
    ) -> Option<Action> {
        let mut action = None;
        for chunk in shown.chunks(look::CARDS_ACROSS) {
            ui.horizontal(|ui| {
                for (pos, i) in chunk {
                    let card = &cards[*i];
                    let (rect, resp) = ui.allocate_exact_size(look::CARD, Sense::click());
                    let name = format!("Layout {}", card.name);
                    resp.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::Role::Button,
                            card.unavailable.is_none(),
                            &name,
                        )
                    });
                    let resp = match card.unavailable {
                        Some(why) => resp.on_hover_text(why),
                        None => resp,
                    };
                    let is_selected = *pos == selected;
                    if is_selected && scroll {
                        resp.scroll_to_me(None);
                    }
                    let slot = look::layout_card(
                        ui,
                        rect,
                        card.thumbnail.as_ref(),
                        is_selected,
                        resp.hovered(),
                        card.unavailable.is_some(),
                    );
                    let tb = &mut self.st.toolbar;
                    let editing = tb
                        .renaming
                        .as_mut()
                        .filter(|(from, _)| *from == card.name && !card.builtin);
                    if let Some((_, text)) = editing {
                        let field = Rect::from_min_size(slot.min, vec2(slot.width(), 22.0));
                        let edit = ui
                            .put(
                                field,
                                egui::TextEdit::singleline(text)
                                    .id(egui::Id::unique("layout-rename"))
                                    .margin(egui::Margin::symmetric(4, 2)),
                            )
                            .accessible_name("Layout name");
                        if !edit.has_focus() && !edit.lost_focus() {
                            edit.request_focus();
                        }
                        continue;
                    }
                    look::card_name(ui, slot, &card.name, card.unavailable.is_some());
                    if let Some(why) = card.unavailable {
                        look::card_note(ui, slot, why);
                    }
                    if resp.clicked() {
                        action = Some(Action::Run(Item::Layout(*i), false));
                    }
                    // Rename and Delete in place, on the hovered or picked saved card.
                    if !card.builtin
                        && (is_selected || resp.hovered() || ui.rect_contains_pointer(rect))
                    {
                        let mut x = slot.right();
                        for (text, what) in [("Delete", 1), ("Rename", 0)] {
                            let w = look::small_width(ui, text) + 8.0;
                            x -= w;
                            let r =
                                Rect::from_min_size(pos2(x, slot.bottom() - 16.0), vec2(w, 16.0));
                            x -= 2.0;
                            if look::card_action(ui, r, text).clicked() {
                                if what == 0 {
                                    tb.renaming = Some((card.name.clone(), card.name.clone()));
                                } else {
                                    action = Some(Action::Delete(card.name.clone()));
                                }
                            }
                        }
                    }
                }
            });
            ui.add_space(4.0);
        }
        action
    }

    /// A Resource dragged out of the Library pane: its ghost follows the pointer, the Panel
    /// under it is marked, and the release docks it there.
    fn library_drag(&mut self, ui: &Ui) {
        let Some((id, abbreviation)) = self.st.toolbar.dragging.clone() else {
            return;
        };
        let pos = ui.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos()));
        let released = ui.input(|i| i.pointer.any_released() || !i.pointer.any_down());
        let over_pane = pos.is_some_and(|p| self.st.toolbar.pane.is_some_and(|r| r.contains(p)));
        let target = pos.filter(|_| !over_pane).and_then(|p| self.drop_target(p));
        if released {
            self.st.toolbar.dragging = None;
            if let Some((panel, rect, beside)) = target {
                let _ = rect;
                self.st.toolbar.close();
                let params = if beside {
                    json!({ "id": id, "beside": panel.0 })
                } else {
                    json!({ "id": id, "panel": panel.0 })
                };
                self.run("toolbar.open_resource", &params);
            }
            return;
        }
        if let Some(p) = pos {
            look::drag_ghost(ui.ctx(), p, &abbreviation);
        }
        if let Some((_, rect, _)) = target {
            look::drop_target(ui.ctx(), rect);
        }
        ui.ctx().request_repaint();
    }

    /// The Panel under `p`, the area a drop there would fill, and whether that is a new
    /// Panel beside it (its right edge).
    fn drop_target(&self, p: egui::Pos2) -> Option<(obs_engine::PanelId, Rect, bool)> {
        self.st.panels.iter().find_map(|(tile, panel)| {
            let r = self.tree.tiles.rect(*tile)?;
            if !r.contains(p) {
                return None;
            }
            let edge = r.right() - r.width() * DROP_EDGE;
            Some(if p.x >= edge {
                (*panel, Rect::from_min_max(pos2(edge, r.top()), r.max), true)
            } else {
                (*panel, r, false)
            })
        })
    }
}

/// What a launcher offers, for its tooltip.
pub(super) fn hint(which: Launcher) -> &'static str {
    match which {
        Launcher::Library => "Find and open a Resource",
        Launcher::Guides => "Passage Guide, Word Study, Factbook, Topic Guide",
        Launcher::Tools => "Search, Text Comparison, Notes and more",
        Launcher::Layouts => "Saved and built-in Layouts",
    }
}

/// What a Guide's box asks for.
fn guide_hint(id: &str) -> &'static str {
    match id {
        "guide.passage_guide" => "Passage, e.g. Jn 3:16",
        "guide.exegetical_guide" => "Passage, e.g. Ps 23",
        "guide.word_study" => "G26, ἀγάπη, agape or love",
        "guide.factbook" => "A person or place, e.g. Abraham",
        "guide.topic_guide" => "A topic, e.g. Forgiveness",
        _ => "",
    }
}

/// The command and parameters that open Guide `id` at `text` (none typed: as its menu item).
fn guide_with(id: &'static str, text: &str) -> (&'static str, Value) {
    if text.is_empty() {
        return (id, json!({}));
    }
    match id {
        "guide.passage_guide" | "guide.exegetical_guide" => (id, json!({ "passage": text })),
        "guide.factbook" => (id, json!({ "name": text })),
        "guide.topic_guide" => (id, json!({ "text": text })),
        _ => (id, json!({})),
    }
}

/// The item above (`down` false) or below `at` in `rows`, rows of item indices as a pane draws
/// them: the same column, or the row's last item when that row is shorter. `at` itself at
/// the top or bottom row.
fn grid_step(rows: &[Vec<usize>], at: usize, down: bool) -> usize {
    let Some((row, col)) = rows
        .iter()
        .enumerate()
        .find_map(|(r, items)| items.iter().position(|i| *i == at).map(|c| (r, c)))
    else {
        return at;
    };
    let to = if down {
        row.checked_add(1)
    } else {
        row.checked_sub(1)
    };
    to.and_then(|r| rows.get(r))
        .and_then(|items| items.get(col.min(items.len().saturating_sub(1))))
        .copied()
        .unwrap_or(at)
}

/// The Layout cards whose name holds `needle` (lower-case), by index.
fn cards_matching(cards: &[LayoutCard], needle: &str) -> Vec<usize> {
    (0..cards.len())
        .filter(|i| needle.is_empty() || cards[*i].name.to_lowercase().contains(needle))
        .collect()
}

/// The Layouts pane's items in order: the `shown` cards, then the footer links.
fn layout_items(shown: &[usize], footer: &[(&'static str, String, bool)]) -> Vec<Item> {
    (shown.iter().map(|i| Item::Layout(*i)))
        .chain(footer.iter().map(|(id, _, _)| Item::Command(id)))
        .collect()
}

/// The Layouts pane's items as drawn, as rows of item indices: the saved cards, the Built-in
/// cards, then the footer links.
fn layout_grid(cards: &[LayoutCard], shown: &[usize], footer: usize) -> Vec<Vec<usize>> {
    let (saved, builtin): (Vec<usize>, Vec<usize>) =
        (0..shown.len()).partition(|p| !cards[shown[*p]].builtin);
    let mut rows: Vec<Vec<usize>> = (saved.chunks(look::CARDS_ACROSS))
        .chain(builtin.chunks(look::CARDS_ACROSS))
        .map(<[usize]>::to_vec)
        .collect();
    if footer > 0 {
        rows.push((shown.len()..shown.len() + footer).collect());
    }
    rows
}

/// The height left for a pane's scrolling list from where `ui` is now in the pane, when the
/// whole pane may be `span` tall, keeping room for the footer links under it; never less than
/// `least`.
fn list_room(ui: &Ui, least: f32, span: f32) -> f32 {
    let above = ui.cursor().top() - ui.min_rect().top();
    (span - above - look::BELOW_LIST).max(least)
}
