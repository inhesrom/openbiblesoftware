//! The Command box (#139): the toolbar box on the right of the menus. The engine reads the
//! text and ranks the suggestions ([`obs_engine::Session::command_box_suggestions`]); this
//! draws them as a dropdown under the box and runs the one picked through `command_box.run`.
//!
//! Keyboard first: Ctrl+L (the `command_box.focus` command) puts the keyboard in the box and
//! selects its text; ↑/↓ pick a suggestion; Enter runs it; Alt+Enter runs it in a new Tab; Esc
//! leaves the box. An empty box lists the recent entries; `>` lists every command it can run
//! (Help → All commands, Ctrl+Shift+P, #226). A click on a suggestion runs it (Alt held: in
//! a new Tab).

use egui::{Align2, Key, Modifiers, Rect, Sense, Ui, pos2, vec2};
use obs_engine::Suggestion;
use serde_json::json;

use super::Shell;
use crate::theme;

/// The box's widget id.
pub(crate) const BOX_ID: &str = "command-box";

/// Placeholder shown in the empty box, followed by the box's key (#177).
pub(crate) const HINT: &str = "Go to, study, open or search";

/// Height of one suggestion row.
const ROW: f32 = 26.0;

/// Rows the dropdown shows before it scrolls.
const MAX_ROWS: usize = 12;

/// The dropdown's scrolling list, by which its scroll is forgotten when it is made again.
const LIST: &str = "command_box_list";

/// The Command box's text and dropdown.
#[derive(Default)]
pub(crate) struct CommandBoxUi {
    pub(crate) text: String,
    pub(crate) suggestions: Vec<Suggestion>,
    /// The text `suggestions` were made for; `None` makes them again.
    for_text: Option<String>,
    /// The highlighted suggestion.
    pub(crate) selected: usize,
    /// ↑/↓ moved the highlight: scroll it into view.
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
}

impl CommandBoxUi {
    /// Help → All commands (Ctrl+Shift+P, #226): the keyboard in the box, which lists every
    /// command it can run; typing filters them.
    pub(crate) fn list_commands(&mut self) {
        self.text = obs_engine::COMMANDS_PREFIX.to_string();
        self.focus = true;
        self.caret_at_end = true;
    }
}

impl Shell {
    /// Draw the Command box (in a right-to-left layout) and its dropdown; run what was picked.
    pub(super) fn command_box(&mut self, ui: &mut Ui) {
        let id = egui::Id::unique(BOX_ID);
        let had_focus = ui.memory(|m| m.has_focus(id));
        let mut picked: Option<(usize, bool)> = None;
        let mut leave = false;
        if had_focus {
            let cb = &mut self.st.command_box;
            let last = cb.suggestions.len().saturating_sub(1);
            cb.scroll_to_selected = false;
            ui.input_mut(|i| {
                if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                    cb.selected = (cb.selected + 1).min(last);
                    cb.scroll_to_selected = true;
                }
                if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                    cb.selected = cb.selected.saturating_sub(1);
                    cb.scroll_to_selected = true;
                }
                if i.consume_key(Modifiers::ALT, Key::Enter) {
                    picked = Some((cb.selected, true));
                } else if i.consume_key(Modifiers::NONE, Key::Enter) {
                    picked = Some((cb.selected, false));
                }
                leave = i.consume_key(Modifiers::NONE, Key::Escape);
            });
        }
        let cb = &mut self.st.command_box;
        let mut out = egui::TextEdit::singleline(&mut cb.text)
            .id(id)
            .hint_text(super::with_key(&self.st.session, HINT, "command_box.focus"))
            .desired_width(280.0)
            .margin(egui::Margin::symmetric(8, 4))
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
        if leave {
            ui.memory_mut(|m| m.surrender_focus(id));
        }
        // Keep drawing the dropdown while a press on it completes into a click.
        let pressing_on_dropdown = cb.dropdown.is_some_and(|d| {
            ui.input(|i| {
                (i.pointer.any_down() || i.pointer.any_released())
                    && i.pointer.interact_pos().is_some_and(|p| d.contains(p))
            })
        });
        if !leave && (focused || pressing_on_dropdown) && !cb.suggestions.is_empty() {
            if let Some(i) = self.dropdown(ui, r.rect) {
                picked = Some(i);
            }
        } else {
            self.st.command_box.dropdown = None;
        }
        if let Some((choice, new_tab)) = picked {
            self.run_choice(ui, choice, new_tab);
        }
    }

    /// The suggestions under the box, right edges aligned. Returns the one clicked (and
    /// whether Alt was held).
    fn dropdown(&mut self, ui: &Ui, field: Rect) -> Option<(usize, bool)> {
        let cb = &mut self.st.command_box;
        let details: Vec<String> = (cb.suggestions.iter())
            .map(|s| shown_detail(&self.st.session, s))
            .collect();
        let width = field.width().max(380.0);
        let mut clicked = None;
        let shown = egui::Area::new(egui::Id::unique(BOX_ID).with("dropdown"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(field.right(), field.bottom() + 4.0))
            .pivot(Align2::RIGHT_TOP)
            .fade_in(false)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style())
                    .inner_margin(egui::Margin::same(4))
                    .show(ui, |ui| {
                        // All commands (#226) can be more rows than fit: they scroll, keeping
                        // the highlighted one in view.
                        // Made again, it forgets where it was scrolled to, and any scroll
                        // to a highlight still under way.
                        if cb.from_the_top {
                            let list = ui.make_persistent_id(egui::IdSalt::new(LIST));
                            ui.data_mut(|d| d.remove::<egui::scroll_area::State>(list));
                        }
                        let list = egui::ScrollArea::vertical()
                            .id_salt(LIST)
                            .max_height(ROW * (MAX_ROWS as f32 + 0.5));
                        list.show(ui, |ui| {
                            let c = theme::c();
                            for (i, (s, detail)) in cb.suggestions.iter().zip(&details).enumerate()
                            {
                                let (rect, resp) =
                                    ui.allocate_exact_size(vec2(width, ROW), Sense::click());
                                if i == cb.selected && cb.scroll_to_selected {
                                    ui.scroll_to_rect(rect, None);
                                }
                                let painter = ui.painter();
                                if i == cb.selected {
                                    painter.rect_filled(rect, 4.0, c.selection);
                                } else if resp.hovered() {
                                    painter.rect_filled(rect, 4.0, c.hover);
                                }
                                let font = egui::TextStyle::Body.resolve(ui.style());
                                let detail_rect = painter.text(
                                    pos2(rect.right() - 8.0, rect.center().y),
                                    Align2::RIGHT_CENTER,
                                    detail,
                                    font.clone(),
                                    c.muted,
                                );
                                let label = painter.layout_no_wrap(s.label.clone(), font, c.ink);
                                let room = (detail_rect.left() - rect.left() - 24.0).max(0.0);
                                let clip = Rect::from_min_size(rect.min, vec2(room + 8.0, ROW));
                                painter.with_clip_rect(clip).galley(
                                    pos2(rect.left() + 8.0, rect.center().y - label.size().y / 2.0),
                                    label,
                                    c.ink,
                                );
                                resp.widget_info(|| {
                                    egui::WidgetInfo::selected(
                                        egui::Role::ListBoxOption,
                                        true,
                                        i == cb.selected,
                                        format!("{} · {detail}", s.label),
                                    )
                                });
                                if resp.clicked() {
                                    clicked = Some((i, ui.input(|i| i.modifiers.alt)));
                                }
                            }
                        });
                    });
            });
        cb.from_the_top = false;
        cb.dropdown = Some(shown.response.rect);
        clicked
    }

    /// Run suggestion `choice` for the box's text; on success the box empties and lets the
    /// keyboard go back to the Workspace, unless the command put it back in the box (All
    /// commands, Command box, #226). On failure the text stays, to be corrected.
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
        cb.dropdown = None;
        if !cb.focus {
            ui.memory_mut(|m| m.surrender_focus(egui::Id::unique(BOX_ID)));
        }
    }
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
