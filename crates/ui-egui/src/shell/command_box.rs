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
                        egui::ScrollArea::vertical()
                            .max_height(ROW * (MAX_ROWS as f32 + 0.5))
                            .show(ui, |ui| {
                                let c = theme::c();
                                for (i, s) in cb.suggestions.iter().enumerate() {
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
                                    let detail = painter.text(
                                        pos2(rect.right() - 8.0, rect.center().y),
                                        Align2::RIGHT_CENTER,
                                        &s.detail,
                                        font.clone(),
                                        c.muted,
                                    );
                                    let label =
                                        painter.layout_no_wrap(s.label.clone(), font, c.ink);
                                    let room = (detail.left() - rect.left() - 24.0).max(0.0);
                                    let clip = Rect::from_min_size(rect.min, vec2(room + 8.0, ROW));
                                    painter.with_clip_rect(clip).galley(
                                        pos2(
                                            rect.left() + 8.0,
                                            rect.center().y - label.size().y / 2.0,
                                        ),
                                        label,
                                        c.ink,
                                    );
                                    resp.widget_info(|| {
                                        egui::WidgetInfo::selected(
                                            egui::Role::ListBoxOption,
                                            true,
                                            i == cb.selected,
                                            format!("{} · {}", s.label, s.detail),
                                        )
                                    });
                                    if resp.clicked() {
                                        clicked = Some((i, ui.input(|i| i.modifiers.alt)));
                                    }
                                }
                            });
                    });
            });
        cb.dropdown = Some(shown.response.rect);
        clicked
    }

    /// Run suggestion `choice` for the box's text; on success the box empties and lets the
    /// keyboard go back to the Workspace.
    fn run_choice(&mut self, ui: &Ui, choice: usize, new_tab: bool) {
        let cb = &self.st.command_box;
        if choice >= cb.suggestions.len() {
            return;
        }
        let text = cb.text.trim().to_string();
        let ran = self.run(
            "command_box.run",
            &json!({ "text": text, "choice": choice, "new_tab": new_tab }),
        );
        if ran.is_some() {
            let cb = &mut self.st.command_box;
            cb.text.clear();
            cb.for_text = None;
            cb.dropdown = None;
            cb.suggestions.clear();
            ui.memory_mut(|m| m.surrender_focus(egui::Id::unique(BOX_ID)));
        }
    }
}
