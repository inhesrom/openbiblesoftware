//! The Command box (#139): the toolbar box on the right of the menus. The engine reads the
//! text and ranks the suggestions ([`obs_engine::Session::command_box_suggestions`]); this
//! draws them as a dropdown under the box and runs the one picked through `command_box.run`.
//!
//! The dropdown is a results panel (#295): min(640 pt, half the window) wide, at most 60% of
//! the window tall, never off-screen; the suggestions under headings with their counts, and
//! under the Search row the Search preview (the ★ Bible's first Verses for the text, searched
//! in the background once the typing stops). When its rows do not all fit, a scroll bar shows
//! and the bottom edge fades with "N more". A Verse's text is Resource text: it is laid out and
//! drawn by the text pipeline (ADR 0004), right to left for a right-to-left Bible, in the row
//! and in its hover popup alike.
//!
//! Keyboard first: Ctrl+L (the `command_box.focus` command) puts the keyboard in the box and
//! selects its text; ↑/↓, PgUp/PgDn and Home/End move the highlight through every row; Enter
//! runs it; Alt+Enter runs it in a new Tab; Esc leaves the box. An empty box lists the recent
//! entries; `>` lists every command it can run (Help → All commands, Ctrl+Shift+P, #226). A
//! click on a row runs it (Alt held: in a new Tab).

use std::sync::Arc;

use egui::text::{LayoutJob, TextWrapping};
use egui::{Align2, Key, Modifiers, Rect, Sense, Ui, pos2, vec2};
use obs_engine::{CommandBoxAction, SearchPreview, Suggestion};
use obs_layout::{ColorRole, TextSettings};
use serde_json::json;

use super::Shell;
use crate::icons::{self, Icon};
use crate::palette::Palette;
use crate::pane::AtlasTexture;
use crate::theme;
use crate::tokens::{inset, radius, share, size, space, text};
use crate::workers::SnippetCache;

/// The box's widget id.
pub(crate) const BOX_ID: &str = "command-box";

/// Placeholder shown in the empty box, followed by the box's key (#177).
pub(crate) const HINT: &str = "Go to, study, open or search";

/// Height of a one-line row.
const ROW: f32 = size::ROW;

/// Height of a heading.
const HEADING: f32 = size::ROW_COMPACT;

/// Height of a Search preview Verse: its reference, then its text.
const HIT_ROW: f32 = size::ROW + text::LABEL_LINE;

/// The dropdown's scrolling list, by which its scroll is forgotten when it is made again.
const LIST: &str = "command_box_list";

/// The row that offers Get Bibles when the library has no Bible to preview a search in.
pub(crate) const ADD_A_BIBLE: &str = "Add a Bible to search as you type";

/// One row of the dropdown.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Row {
    /// A heading over the rows that follow, with how many there are.
    Heading { title: String, count: usize },
    /// Suggestion `i` of [`CommandBoxUi::suggestions`].
    Suggestion(usize),
    /// Verse `i` of the Search preview.
    Hit(usize),
    /// Show all the Search preview's results in the Search Tool: the box's Search suggestion,
    /// `i`, with how many Verses match.
    ShowAll { search: usize, total: usize },
    /// Get Bibles: the library has no Bible to search.
    AddBible,
    /// A line that cannot be picked: the ★ Bible being indexed, or no Verse matching.
    Note(String),
}

impl Row {
    /// Whether the highlight can rest on it and Enter run it.
    fn is_pickable(&self) -> bool {
        !matches!(self, Row::Heading { .. } | Row::Note(_))
    }

    fn height(&self) -> f32 {
        match self {
            Row::Heading { .. } => HEADING,
            Row::Hit(_) => HIT_ROW,
            _ => ROW,
        }
    }
}

/// The dropdown's rows: the suggestions under their headings, and under the Search row the
/// Search preview.
pub(crate) fn rows(suggestions: &[Suggestion], preview: &SearchPreview) -> Vec<Row> {
    let mut out = Vec::new();
    for g in obs_engine::suggestion_groups(suggestions) {
        out.push(Row::Heading {
            title: g.heading.to_string(),
            count: g.suggestions.len(),
        });
        out.extend(g.suggestions.iter().map(|&i| Row::Suggestion(i)));
        let search = (g.suggestions.iter())
            .find(|&&i| matches!(suggestions[i].action, CommandBoxAction::Search(_)));
        if let Some(&search) = search {
            preview_rows(search, preview, &mut out);
        }
    }
    out
}

/// The Search preview's rows, under Search suggestion `search`.
fn preview_rows(search: usize, preview: &SearchPreview, out: &mut Vec<Row>) {
    match preview {
        SearchPreview::Off => {}
        SearchPreview::NoBible => out.push(Row::AddBible),
        SearchPreview::Indexing(abbreviation) => out.push(Row::Note(format!(
            "Indexing {abbreviation}… results will appear here"
        ))),
        SearchPreview::Hits(h) if h.hits.is_empty() => {
            out.push(Row::Note(format!("No verses in {} match", h.bible)));
        }
        SearchPreview::Hits(h) => {
            out.push(Row::Heading {
                title: format!("Verses in {}", h.bible),
                count: h.total,
            });
            out.extend((0..h.hits.len()).map(Row::Hit));
            out.push(Row::ShowAll {
                search,
                total: h.total,
            });
        }
    }
}

/// "Show all 12 results in Search".
pub(crate) fn show_all_label(total: usize) -> String {
    let results = if total == 1 { "result" } else { "results" };
    format!("Show all {total} {results} in Search")
}

/// The Command box's text and dropdown.
#[derive(Default)]
pub(crate) struct CommandBoxUi {
    pub(crate) text: String,
    pub(crate) suggestions: Vec<Suggestion>,
    /// The text `suggestions` were made for; `None` makes them again.
    for_text: Option<String>,
    /// The dropdown's rows as last drawn.
    pub(crate) rows: Vec<Row>,
    /// The highlighted row, counting only the rows that can be picked.
    pub(crate) selected: usize,
    /// The highlight moved: scroll it into view.
    scroll_to_selected: bool,
    /// The suggestions were made again: the list shows from its first row, the highlighted
    /// one (it may still be scrolled from the last time it showed).
    from_the_top: bool,
    /// Ctrl+L asked for the keyboard.
    pub(crate) focus: bool,
    /// The keyboard goes in with the caret after the text instead of the text selected (All
    /// commands, #226).
    caret_at_end: bool,
    /// Where the dropdown was drawn last frame, so a click on it is not lost to the box
    /// losing focus at the press.
    pub(super) dropdown: Option<Rect>,
    /// Pickable rows a page (PgUp/PgDn) moves by: those that fitted last frame.
    page: usize,
    /// Pickable rows below the list's bottom edge last frame ("N more"); `None` when every
    /// row fitted and there is no scroll bar.
    pub(crate) more: Option<usize>,
    /// The Search preview's Verse texts, laid out off the UI thread by the text pipeline.
    snippets: SnippetCache,
    /// The Search preview's Verses whose text was drawn last frame (the others still wait for
    /// their layout).
    pub(crate) verses_drawn: usize,
}

impl CommandBoxUi {
    /// Help → All commands (Ctrl+Shift+P, #226): the keyboard in the box, which lists every
    /// command it can run; typing filters them.
    pub(crate) fn list_commands(&mut self) {
        self.text = obs_engine::COMMANDS_PREFIX.to_string();
        self.focus = true;
        self.caret_at_end = true;
    }

    /// The rows that can be picked, in order.
    fn picks(&self) -> Vec<&Row> {
        self.rows.iter().filter(|r| r.is_pickable()).collect()
    }

    /// The highlighted row.
    #[cfg(test)]
    pub(crate) fn selected_row(&self) -> Option<&Row> {
        self.picks().get(self.selected).copied()
    }

    /// The text the Search preview previews: the Search suggestion's.
    fn search_text(&self) -> Option<&str> {
        self.suggestions.iter().find_map(|s| match &s.action {
            CommandBoxAction::Search(t) => Some(t.as_str()),
            _ => None,
        })
    }
}

/// What the keys asked of the dropdown this frame.
#[derive(Default)]
struct Keys {
    run: Option<bool>,
    leave: bool,
}

impl Shell {
    /// Draw the Command box (in a right-to-left layout) and its dropdown; run what was picked.
    pub(super) fn command_box(&mut self, ui: &mut Ui) {
        let id = egui::Id::unique(BOX_ID);
        let had_focus = ui.memory(|m| m.has_focus(id));
        let mut keys = Keys::default();
        if had_focus {
            keys = self.command_box_keys(ui);
        }
        let mut picked: Option<(usize, bool)> =
            keys.run.map(|alt| (self.st.command_box.selected, alt));
        let cb = &mut self.st.command_box;
        let mut out = egui::TextEdit::singleline(&mut cb.text)
            .id(id)
            .hint_text(super::with_key(&self.st.session, HINT, "command_box.focus"))
            .desired_width(size::FIELD_WIDE)
            .margin(inset::BAR)
            .show(ui);
        let r = egui::Response::clone(&out.response).accessible_name("Command box");
        if std::mem::take(&mut cb.focus) {
            r.request_focus();
            let range = if std::mem::take(&mut cb.caret_at_end) {
                egui::text::CCursorRange::one(egui::text::CCursor::new(cb.text.chars().count()))
            } else {
                egui::text::CCursorRange::select_all_str(&cb.text)
            };
            out.state.cursor.set_char_range(Some(range));
            out.state.store(ui.ctx(), id);
            cb.for_text = None;
        }
        let focused = r.has_focus() || ui.memory(|m| m.has_focus(id));
        if focused && (cb.for_text.as_deref() != Some(cb.text.as_str()) || r.gained_focus()) {
            cb.suggestions = self.st.session.command_box_suggestions(&cb.text);
            cb.for_text = Some(cb.text.clone());
            cb.selected = 0;
            cb.from_the_top = true;
        }
        if keys.leave {
            ui.memory_mut(|m| m.surrender_focus(id));
        }
        // Keep drawing the dropdown while a press on it completes into a click.
        let pressing_on_dropdown = cb.dropdown.is_some_and(|d| {
            ui.input(|i| {
                (i.pointer.any_down() || i.pointer.any_released())
                    && i.pointer.interact_pos().is_some_and(|p| d.contains(p))
            })
        });
        let shown = !keys.leave && (focused || pressing_on_dropdown) && !cb.suggestions.is_empty();
        // The Search preview searches the text once the typing stops, off this thread.
        let now = ui.input(|i| i.time);
        let preview_text = shown
            .then(|| cb.search_text().map(str::to_string))
            .flatten();
        let session = &mut self.st.session;
        session.update_search_preview(preview_text.as_deref(), now);
        if let Some(due) = session.search_preview_due_in(now) {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(due));
        }
        let cb = &mut self.st.command_box;
        cb.rows = rows(&cb.suggestions, session.search_preview());
        cb.selected = cb.selected.min(cb.picks().len().saturating_sub(1));
        if shown {
            if let Some(i) = self.dropdown(ui, r.rect) {
                picked = Some(i);
            }
        } else {
            let cb = &mut self.st.command_box;
            cb.dropdown = None;
            cb.more = None;
        }
        if let Some((pick, new_tab)) = picked {
            self.run_pick(ui, pick, new_tab);
        }
    }

    /// Take the dropdown's keys from the box: the highlight's moves, Enter, Esc. Returns
    /// whether to run the highlighted row (and with Alt) and whether to leave the box.
    fn command_box_keys(&mut self, ui: &Ui) -> Keys {
        let cb = &mut self.st.command_box;
        let last = cb.picks().len().saturating_sub(1);
        let page = cb.page.max(1);
        cb.scroll_to_selected = false;
        let mut keys = Keys::default();
        ui.input_mut(|i| {
            let at = cb.selected;
            let moves = [
                (Key::ArrowDown, at + 1),
                (Key::ArrowUp, at.saturating_sub(1)),
                (Key::PageDown, at + page),
                (Key::PageUp, at.saturating_sub(page)),
                (Key::End, last),
                (Key::Home, 0),
            ];
            for (key, to) in moves {
                if i.consume_key(Modifiers::NONE, key) {
                    cb.selected = to.min(last);
                    cb.scroll_to_selected = true;
                }
            }
            if i.consume_key(Modifiers::ALT, Key::Enter) {
                keys.run = Some(true);
            } else if i.consume_key(Modifiers::NONE, Key::Enter) {
                keys.run = Some(false);
            }
            keys.leave = i.consume_key(Modifiers::NONE, Key::Escape);
        });
        keys
    }

    /// The results panel under the box, right edges aligned, inside the window. Returns the
    /// pickable row clicked (and whether Alt was held).
    fn dropdown(&mut self, ui: &Ui, field: Rect) -> Option<(usize, bool)> {
        let st = &mut self.st;
        let session = &st.session;
        let cb = &mut st.command_box;
        let mut verses = VerseText {
            snippets: &mut cb.snippets,
            atlas: &mut st.atlas,
            settings: preview_settings(session.text),
            palette: hit_palette(&theme::c()),
        };
        let details: Vec<String> = (cb.suggestions.iter())
            .map(|s| shown_detail(session, s))
            .collect();
        let preview = match session.search_preview() {
            SearchPreview::Hits(h) => Some(Arc::clone(h)),
            _ => None,
        };
        let rtl = preview.as_ref().is_some_and(|p| p.rtl);
        let screen = ui.ctx().content_rect();
        let frame = crate::components::popover_frame().inner_margin(inset::BAR);
        let margin = frame.total_margin();
        // Wider than the box, at most half the window; never off its left edge.
        let width = (size::RESULTS.min(screen.width() * share::RESULTS_WIDTH))
            .max(field.width())
            .min(screen.width() - 2.0 * space::M);
        let right = field.right().max(screen.left() + space::M + width);
        let top = field.bottom() + space::XS;
        let tallest = (screen.height() * share::RESULTS_HEIGHT)
            .min(screen.bottom() - space::M - top)
            .max(ROW);
        let list_height = tallest - margin.sum().y;
        let inner_width = width - margin.sum().x;
        let picks = cb.rows.iter().filter(|r| r.is_pickable()).count();
        let mut clicked = None;
        let mut more = None;
        let mut fitted = 0;
        let mut verses_drawn = 0;
        let shown = egui::Area::new(egui::Id::unique(BOX_ID).with("dropdown"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(right, top))
            .pivot(Align2::RIGHT_TOP)
            .fade_in(false)
            .show(ui.ctx(), |ui| {
                frame.show(ui, |ui| {
                    ui.set_width(inner_width);
                    // A solid bar, shown whenever the rows do not all fit, hovered or not, in
                    // the app's scroll thumb grey.
                    let solid = egui::style::ScrollStyle::solid();
                    ui.spacing_mut().scroll = egui::style::ScrollStyle {
                        bar_width: size::SCROLLBAR,
                        dormant_handle_opacity: solid.interact_handle_opacity,
                        active_handle_opacity: solid.interact_handle_opacity,
                        ..solid
                    };
                    let c = theme::c();
                    let w = &mut ui.visuals_mut().widgets;
                    w.inactive.bg_fill = c.muted.gamma_multiply(crate::tokens::alpha::HALF);
                    w.hovered.bg_fill = c.muted;
                    w.active.bg_fill = c.ink;
                    // Made again, it forgets where it was scrolled to, and any scroll
                    // to a highlight still under way.
                    if cb.from_the_top {
                        let list = ui.make_persistent_id(egui::IdSalt::new(LIST));
                        ui.data_mut(|d| d.remove::<egui::scroll_area::State>(list));
                    }
                    let list = egui::ScrollArea::vertical()
                        .id_salt(LIST)
                        .auto_shrink([false, true])
                        .max_height(list_height)
                        .min_scrolled_height(list_height)
                        .scroll_bar_visibility(
                            egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
                        );
                    let out = list.show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        let view = ui.clip_rect();
                        let mut pick = 0;
                        let mut below = 0;
                        let mut first: Option<Rect> = None;
                        for row in &cb.rows {
                            let (rect, resp) = ui.allocate_exact_size(
                                vec2(ui.available_width(), row.height()),
                                if row.is_pickable() {
                                    Sense::click()
                                } else {
                                    Sense::hover()
                                },
                            );
                            let first = *first.get_or_insert(rect);
                            if !row.is_pickable() {
                                paint_note(ui, rect, row);
                                continue;
                            }
                            if rect.center().y > view.bottom() {
                                below += 1;
                            } else if rect.top() >= view.top() {
                                fitted += 1;
                            }
                            let selected = pick == cb.selected;
                            if selected && cb.scroll_to_selected {
                                // The first row shows its heading too.
                                let to = if pick == 0 { rect.union(first) } else { rect };
                                ui.scroll_to_rect(to, None);
                            }
                            let painter = ui.painter();
                            if selected {
                                crate::components::selected_row(painter, rect, radius::S);
                            } else if resp.hovered() {
                                painter.rect_filled(rect, radius::S, theme::c().hover);
                            }
                            let (name, resp) = match row {
                                Row::Suggestion(i) => {
                                    let s = &cb.suggestions[*i];
                                    let detail = &details[*i];
                                    let elided = paint_line(ui, rect, &s.label, detail);
                                    let resp = if elided {
                                        resp.on_hover_text(s.label.clone())
                                    } else {
                                        resp
                                    };
                                    (format!("{} · {detail}", s.label), resp)
                                }
                                Row::Hit(k) => {
                                    let hit = preview.as_ref().and_then(|p| p.hits.get(*k));
                                    let Some(hit) = hit else {
                                        continue;
                                    };
                                    let drawn = paint_hit(ui, rect, hit, rtl, &mut verses);
                                    verses_drawn += usize::from(drawn.is_some());
                                    // The whole Verse, through the text pipeline too.
                                    let resp = if drawn == Some(true) {
                                        resp.on_hover_ui(|ui| {
                                            paint_whole_hit(ui, hit, rtl, &mut verses);
                                        })
                                    } else {
                                        resp
                                    };
                                    let name = format!("{} · {}", hit.reference, hit.snippet.text);
                                    (name, resp)
                                }
                                Row::ShowAll { total, .. } => {
                                    let label = show_all_label(*total);
                                    paint_line(ui, rect, &label, "Search");
                                    (label, resp)
                                }
                                Row::AddBible => {
                                    paint_line(ui, rect, ADD_A_BIBLE, "Get Bibles");
                                    (format!("{ADD_A_BIBLE} · Get Bibles"), resp)
                                }
                                Row::Heading { .. } | Row::Note(_) => continue,
                            };
                            let at = pick;
                            resp.widget_info(|| {
                                egui::WidgetInfo::selected(
                                    egui::Role::ListBoxOption,
                                    true,
                                    selected,
                                    format!("{name}, {} of {picks}", at + 1),
                                )
                            });
                            if resp.clicked() {
                                clicked = Some((pick, ui.input(|i| i.modifiers.alt)));
                            }
                            pick += 1;
                        }
                        below
                    });
                    let overflows = out.content_size.y > out.inner_rect.height();
                    if overflows {
                        more = Some(out.inner);
                        if out.inner > 0 {
                            paint_more(ui, out.inner_rect, out.inner);
                        }
                    }
                });
            });
        cb.from_the_top = false;
        cb.verses_drawn = verses_drawn;
        cb.dropdown = Some(shown.response.rect);
        cb.more = more;
        cb.page = fitted.max(1);
        clicked
    }

    /// Run pickable row `pick`: a suggestion through `command_box.run`, a Search preview
    /// Verse, Show all, or Get Bibles. On success the box empties and lets the keyboard go
    /// back to the Workspace, unless the command put it back in the box (All commands,
    /// Command box, #226). On failure the text stays, to be corrected.
    fn run_pick(&mut self, ui: &Ui, pick: usize, new_tab: bool) {
        let Some(row) = self.st.command_box.picks().get(pick).map(|r| (*r).clone()) else {
            return;
        };
        let ran = match row {
            Row::Suggestion(i) | Row::ShowAll { search: i, .. } => {
                return self.run_choice(ui, i, new_tab);
            }
            Row::Hit(k) => self.run(
                "command_box.open_hit",
                &json!({ "hit": k, "new_tab": new_tab }),
            ),
            Row::AddBible => self.run("library.get_bibles", &json!({})),
            Row::Heading { .. } | Row::Note(_) => None,
        };
        if ran.is_some() {
            let cb = &mut self.st.command_box;
            cb.text.clear();
            cb.suggestions.clear();
            cb.for_text = None;
            self.left_the_box(ui);
        }
    }

    /// Run suggestion `choice` for the box's text; see [`Self::run_pick`].
    fn run_choice(&mut self, ui: &Ui, choice: usize, new_tab: bool) {
        let cb = &mut self.st.command_box;
        if choice >= cb.suggestions.len() {
            return;
        }
        // Emptied before the command runs, so what the command puts in the box stays.
        let typed = std::mem::take(&mut cb.text);
        let suggestions = std::mem::take(&mut cb.suggestions);
        let for_text = cb.for_text.take();
        let mut params = json!({ "text": typed.trim(), "choice": choice, "new_tab": new_tab });
        // The command shown runs, not whichever the suggestions found again put at `choice`:
        // they leave out disabled commands, so one becoming possible (an Undo) shifts them.
        if let obs_engine::CommandBoxAction::Command(id) = suggestions[choice].action {
            params["command"] = json!(id);
        }
        let ran = self.run("command_box.run", &params);
        let cb = &mut self.st.command_box;
        if ran.is_none() {
            cb.text = typed;
            cb.suggestions = suggestions;
            cb.for_text = for_text;
            return;
        }
        self.left_the_box(ui);
    }

    /// After a row ran: the dropdown closes and the keyboard goes back to the Workspace,
    /// unless the command put it back in the box.
    fn left_the_box(&mut self, ui: &Ui) {
        let cb = &mut self.st.command_box;
        cb.dropdown = None;
        cb.more = None;
        if !cb.focus {
            ui.memory_mut(|m| m.surrender_focus(egui::Id::unique(BOX_ID)));
        }
    }
}

/// `text` on one line in `font` and `color`, cut with "…" to `width`.
fn one_line(
    ui: &Ui,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
    width: f32,
) -> Arc<egui::Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap = TextWrapping::truncate_at_width(width.max(0.0));
    ui.fonts_mut(|f| f.layout_job(job))
}

/// A one-line row: `label` on the left, `detail` on the right; the label is cut with "…"
/// where it would reach the detail. Returns whether it was cut.
fn paint_line(ui: &Ui, rect: Rect, label: &str, detail: &str) -> bool {
    let c = theme::c();
    let painter = ui.painter();
    let font = egui::TextStyle::Body.resolve(ui.style());
    let detail_rect = painter.text(
        pos2(rect.right() - space::M, rect.center().y),
        Align2::RIGHT_CENTER,
        detail,
        font.clone(),
        c.muted,
    );
    let room = detail_rect.left() - rect.left() - space::M - space::XXL;
    let galley = one_line(ui, label, font, c.ink, room);
    let elided = galley.elided;
    let at = pos2(
        rect.left() + space::M,
        rect.center().y - galley.size().y / 2.0,
    );
    painter.galley(at, galley, c.ink);
    elided
}

/// The Page palette the Search preview's Verses are drawn in: as dark (and as High contrast)
/// as the Theme, never the Page tone's own, so its ink and Search hit tint read on the chrome
/// whatever Page tone sits beside it.
pub(crate) fn hit_palette(c: &crate::tokens::Chrome) -> Palette {
    let tone = if c.dark {
        obs_engine::PageTone::Dark
    } else {
        obs_engine::PageTone::Paper
    };
    Palette::for_theme(tone, c.high_contrast)
}

/// The Search preview's Verse texts in Body size: Resource text in the Reading fonts, sized to
/// the chrome around it.
fn preview_settings(reading: TextSettings) -> TextSettings {
    TextSettings {
        size: text::BODY,
        ..reading
    }
}

/// What the Search preview's Verses are laid out and drawn with: the text pipeline's snippet
/// layouts and glyph atlas, in [`preview_settings`] and [`hit_palette`].
struct VerseText<'a> {
    snippets: &'a mut SnippetCache,
    atlas: &'a mut AtlasTexture,
    settings: TextSettings,
    palette: Palette,
}

impl VerseText<'_> {
    /// `hit`'s text laid out `width` wide, or `None` (and a frame asked for) while a worker
    /// lays it out.
    fn laid(
        &mut self,
        ui: &Ui,
        hit: &obs_engine::PreviewHit,
        rtl: bool,
        width: f32,
    ) -> Option<Arc<obs_layout::LaidBlock>> {
        let text = &hit.snippet.text;
        let laid = (self.snippets).get(text, ColorRole::Ink, rtl, width, &self.settings);
        if laid.is_none() {
            ui.ctx().request_repaint();
        }
        laid
    }

    /// Draw `laid` at `origin` as far as `painter`'s clip rect shows it, `hit`'s matches
    /// tinted behind it.
    fn paint(
        &mut self,
        painter: &egui::Painter,
        laid: &obs_layout::LaidBlock,
        hit: &obs_engine::PreviewHit,
        origin: egui::Pos2,
    ) {
        for r in &hit.snippet.highlights {
            for [x0, y0, x1, y1] in crate::compare_view::range_rects(laid, r) {
                let tint = Rect::from_min_max(origin + vec2(x0, y0), origin + vec2(x1, y1));
                painter.rect_filled(tint, 0.0, self.palette.search_hit);
            }
        }
        self.atlas.paint(painter, laid, origin, &self.palette);
    }

    /// A grey bar where the text will be.
    fn placeholder(&self, painter: &egui::Painter, origin: egui::Pos2, width: f32) {
        let line = self.settings.size * self.settings.line_spacing;
        let color = theme::c().rule;
        crate::components::text_placeholder(painter, origin, width, line, color);
    }
}

/// A Search preview Verse: its reference, then the first line of its text, flush with the
/// Bible's direction, the matches tinted and a cut line ended with "…". Returns whether the
/// text was cut, or `None` while it waits for its layout.
fn paint_hit(
    ui: &Ui,
    rect: Rect,
    hit: &obs_engine::PreviewHit,
    rtl: bool,
    verses: &mut VerseText<'_>,
) -> Option<bool> {
    let c = theme::c();
    let painter = ui.painter();
    let body = egui::TextStyle::Body.resolve(ui.style());
    let width = rect.width() - 2.0 * space::M;
    let reference = one_line(ui, &hit.reference, body.clone(), c.ink, width);
    let left = rect.left() + space::M;
    let top = rect.top() + space::XS + reference.size().y;
    painter.galley(pos2(left, rect.top() + space::XS), reference, c.ink);
    let ink = verses.palette.ink;
    let ellipsis = painter.layout_no_wrap("…".to_string(), body, ink);
    // Room for the "…" at the line's end: its right in left-to-right text, its left in
    // right-to-left text.
    let room = ellipsis.size().x + space::XS;
    let text_width = (width - room).max(size::TEXT_MIN);
    let origin = pos2(if rtl { left + room } else { left }, top);
    let Some(laid) = verses.laid(ui, hit, rtl, text_width) else {
        verses.placeholder(painter, origin, text_width.min(size::POPOVER_TEXT));
        return None;
    };
    let Some(first) = laid.lines.first() else {
        return Some(false);
    };
    let line = Rect::from_min_max(
        pos2(rect.left(), top + first.top),
        pos2(rect.right(), top + first.bottom),
    );
    let clipped = painter.with_clip_rect(line.intersect(painter.clip_rect()));
    verses.paint(&clipped, &laid, hit, origin);
    let cut = laid.lines.len() > 1;
    if cut {
        let (start, end) = (first.items.iter()).fold((f32::MAX, f32::MIN), |(a, b), item| {
            (a.min(item.x), b.max(item.x + item.width))
        });
        let (x, align) = if rtl {
            (origin.x + start - space::XXS, Align2::RIGHT_CENTER)
        } else {
            (origin.x + end + space::XXS, Align2::LEFT_CENTER)
        };
        let at = align.align_size_within_rect(
            ellipsis.size(),
            Rect::from_x_y_ranges(x..=x, line.y_range()),
        );
        painter.galley(at.min, ellipsis, ink);
    }
    Some(cut)
}

/// A cut Verse whole, for its hover popup: the text pipeline's layout of it, as wide as a
/// popover's text.
fn paint_whole_hit(
    ui: &mut Ui,
    hit: &obs_engine::PreviewHit,
    rtl: bool,
    verses: &mut VerseText<'_>,
) {
    let width = size::POPOVER_TEXT;
    let Some(laid) = verses.laid(ui, hit, rtl, width) else {
        let line = verses.settings.size * verses.settings.line_spacing;
        let (rect, _) = ui.allocate_exact_size(vec2(width, line), Sense::hover());
        verses.placeholder(ui.painter(), rect.min, width);
        return;
    };
    let (rect, resp) = ui.allocate_exact_size(vec2(width, laid.height), Sense::hover());
    let text = &hit.snippet.text;
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text));
    let painter = ui
        .painter()
        .with_clip_rect(rect.expand(space::XS).intersect(ui.clip_rect()));
    verses.paint(&painter, &laid, hit, rect.min);
}

/// A heading (its title and count) or a note that cannot be picked.
fn paint_note(ui: &Ui, rect: Rect, row: &Row) {
    let c = theme::c();
    let painter = ui.painter();
    match row {
        Row::Heading { title, count } => {
            let font = egui::TextStyle::Small.resolve(ui.style());
            let y = rect.center().y;
            painter.text(
                pos2(rect.left() + space::M, y),
                Align2::LEFT_CENTER,
                title,
                font.clone(),
                c.muted,
            );
            painter.text(
                pos2(rect.right() - space::M, y),
                Align2::RIGHT_CENTER,
                count.to_string(),
                font,
                c.muted,
            );
        }
        Row::Note(note) => {
            let font = egui::TextStyle::Body.resolve(ui.style());
            let galley = one_line(ui, note, font, c.muted, rect.width() - 2.0 * space::M);
            let at = pos2(
                rect.left() + space::M,
                rect.center().y - galley.size().y / 2.0,
            );
            painter.galley(at, galley, c.muted);
        }
        _ => {}
    }
}

/// The list's bottom edge when rows follow below it: a fade into the panel, and a "N more"
/// pill with a chevron down at its right, over the rows.
fn paint_more(ui: &Ui, view: Rect, n: usize) {
    let c = theme::c();
    let painter = ui.painter();
    let band = Rect::from_min_max(pos2(view.left(), view.bottom() - ROW), view.right_bottom());
    let mut fade = egui::Mesh::default();
    // Into the popover's fill.
    let clear = egui::Color32::TRANSPARENT;
    fade.colored_vertex(band.left_top(), clear);
    fade.colored_vertex(band.right_top(), clear);
    fade.colored_vertex(band.right_bottom(), c.field);
    fade.colored_vertex(band.left_bottom(), c.field);
    fade.add_triangle(0, 1, 2);
    fade.add_triangle(0, 2, 3);
    painter.add(fade);
    let font = egui::TextStyle::Small.resolve(ui.style());
    let label = painter.layout_no_wrap(format!("{n} more"), font, c.ink);
    let pill_size = vec2(
        label.size().x + size::ICON_SMALL + space::XS + 2.0 * space::M,
        size::CHIP,
    );
    let pill = Rect::from_min_size(
        pos2(
            view.right() - space::XS - pill_size.x,
            band.center().y - pill_size.y / 2.0,
        ),
        pill_size,
    );
    painter.rect(
        pill,
        radius::PILL,
        c.field,
        egui::Stroke::new(crate::tokens::stroke::HAIR, c.rule),
        egui::StrokeKind::Inside,
    );
    let text_at = pos2(
        pill.left() + space::M,
        pill.center().y - label.size().y / 2.0,
    );
    painter.galley(text_at, label, c.ink);
    let chevron = Rect::from_center_size(
        pos2(
            pill.right() - space::M - size::ICON_SMALL / 2.0,
            pill.center().y,
        ),
        vec2(size::ICON_SMALL, size::ICON_SMALL),
    );
    icons::paint(painter, Icon::ChevronDown, chevron, size::ICON_SMALL, c.ink);
}

/// `s`'s detail as shown: a command's key as the platform writes it (`⌘W` on macOS, #262).
fn shown_detail(session: &obs_engine::Session, s: &Suggestion) -> String {
    if let obs_engine::CommandBoxAction::Command(id) = s.action
        && let Some(key) = session.shortcut(id)
        && let Some(head) = s.detail.strip_suffix(key)
    {
        return format!("{head}{}", crate::key_labels::label(key));
    }
    s.detail.clone()
}
