//! The Find bar (#175): Ctrl+F in a Resource or Text Comparison Tab opens it under the Tab's
//! header strip. As the user types, every match in the Resource is found (on a worker) and
//! drawn; the count reads "12 of 48". Enter / Shift+Enter (or the Next and Previous buttons)
//! move between matches, scrolling to each; Esc (or Close) closes the bar and clears them.
//! Alt+Enter ("Search all Resources") hands what is typed to the Search Tool.

use egui::{Key, Modifiers, RichText, Ui, vec2};
use obs_engine::TabId;
use obs_model::ResourceType;
use serde_json::{Value, json};

use super::State;
use crate::find::{FindBar, FindStatus};
use crate::icons;
use crate::selection::selected_text;
use crate::theme;

/// What the empty field says.
pub(crate) const FIND_HINT: &str = "Find in this Resource";

/// A selection longer than this (or over several lines) is not put in the Find bar.
const SELECTION_LIMIT: usize = 100;

/// The count beside the field.
pub(crate) fn status_text(status: FindStatus) -> String {
    match status {
        FindStatus::Empty => String::new(),
        FindStatus::Finding => "Finding…".into(),
        FindStatus::Found { total: 0, .. } => "No matches".into(),
        FindStatus::Found {
            current: Some(c),
            total,
        } => format!("{} of {total}", c + 1),
        FindStatus::Found {
            current: None,
            total,
        } => format!("{total} matches"),
    }
}

/// The field's widget id in Tab `tab`.
pub(crate) fn field_id(tab: TabId) -> egui::Id {
    egui::Id::unique(("find-field", tab.0))
}

/// Previous (a chevron up) or Next (down), named for screen readers.
fn arrow_button(ui: &mut Ui, up: bool, enabled: bool, what: &str) -> egui::Response {
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, r) = ui.allocate_exact_size(vec2(22.0, 22.0), sense);
    let c = theme::c();
    if r.hovered() && enabled {
        ui.painter().rect_filled(rect, 4.0, c.hover);
    }
    let color = if enabled { c.ink } else { c.muted };
    let icon = if up {
        icons::Icon::ChevronUp
    } else {
        icons::Icon::ChevronDown
    };
    icons::paint(ui.painter(), icon, rect, icons::BODY, color);
    let label = what.split(" (").next().unwrap_or(what).to_string();
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, &label));
    r.on_hover_text(what)
}

impl State {
    /// Ctrl+F (`tab.find`): open the Tab's Find bar with `text` (else the selection, if short,
    /// else what it had) and put the keyboard in it.
    pub(super) fn open_find(&mut self, tab: TabId, text: Option<String>) {
        if !self
            .session
            .workspace
            .tab(tab)
            .is_some_and(|t| t.kind.has_find_bar())
        {
            return;
        }
        let t = self.tabs.entry(tab).or_default();
        let selection = t
            .view
            .as_ref()
            .and_then(|v| {
                let sel = v.selection.filter(|s| !s.is_empty())?;
                Some(selected_text(&v.doc, &sel))
            })
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && !s.contains('\n') && s.chars().count() <= SELECTION_LIMIT);
        let bar = t.find.get_or_insert_with(FindBar::default);
        if let Some(text) = text.or(selection) {
            bar.text = text;
        }
        bar.focus = true;
    }

    /// `tab.find_next` / `tab.find_previous`.
    pub(super) fn step_find(&mut self, tab: TabId, forward: bool) {
        if let Some(bar) = self.tabs.get_mut(&tab).and_then(|t| t.find.as_mut()) {
            bar.step = Some(forward);
        }
    }

    /// The Find bar's row in the Tab's header, when it is open.
    pub(super) fn find_row(&mut self, ui: &mut Ui, tab: TabId) {
        let id = field_id(tab);
        let focused_tab = self.session.workspace.focused() == Some(tab);
        // A Footnote popover shown on this Tab, or on screen in this window, has first refusal
        // on Esc (#208): the bar waits for the next one. The header runs before the pane that
        // consumes that Esc, so the bar asks first.
        let footnote_shown = (self.footnotes.tabs.get(&tab)).is_some_and(|f| f.state.phase.shown())
            || self.footnotes.on_screen(ui.ctx(), |_| true);
        let Some(bar) = self.tabs.get_mut(&tab).and_then(|t| t.find.as_mut()) else {
            return;
        };
        let mut close = false;
        let mut search_all = false;
        // Keys for the field, taken before it sees them: Enter steps, it does not end input.
        if ui.memory(|m| m.has_focus(id)) {
            ui.input_mut(|i| {
                if i.consume_key(Modifiers::ALT, Key::Enter) {
                    search_all = true;
                } else if i.consume_key(Modifiers::SHIFT, Key::Enter) {
                    bar.step = Some(false);
                } else if i.consume_key(Modifiers::NONE, Key::Enter) {
                    bar.step = Some(true);
                }
            });
        }
        // Esc leaves the field before any widget runs (egui), so ask whether it had the keyboard;
        // or Esc in the focused Tab while nothing else has the keyboard.
        let escape = !footnote_shown
            && ui.input(|i| i.key_pressed(Key::Escape))
            && (ui.memory(|m| m.had_focus_last_frame(id))
                || (focused_tab && ui.memory(|m| m.focused().is_none())));
        close |= escape;
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            ui.add_space(4.0);
            let mut out = egui::TextEdit::singleline(&mut bar.text)
                .id(id)
                .hint_text(FIND_HINT)
                .desired_width(170.0)
                .return_key(None)
                .margin(egui::Margin::symmetric(8, 4))
                .show(ui);
            if std::mem::take(&mut bar.focus) {
                out.response.request_focus();
                out.state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::select_all_str(&bar.text)));
                out.state.store(ui.ctx(), id);
            }
            let status = bar.status();
            let count = status_text(status);
            ui.add_sized(
                vec2(78.0, 20.0),
                egui::Label::new(RichText::new(count).small().color(theme::c().muted)),
            );
            let any = matches!(status, FindStatus::Found { total, .. } if total > 0);
            let key = crate::key_labels::label;
            let previous = arrow_button(
                ui,
                true,
                any,
                &format!("Previous match ({})", key("Shift+Enter")),
            );
            let next = arrow_button(ui, false, any, &format!("Next match ({})", key("Enter")));
            bar.arrows = Some([previous.rect, next.rect]);
            if previous.clicked() {
                bar.step = Some(false);
            }
            if next.clicked() {
                bar.step = Some(true);
            }
            if icons::button(
                ui,
                icons::Icon::Close,
                icons::SMALL,
                theme::c().muted,
                "Close",
            )
            .on_hover_text("Close (Esc)")
            .clicked()
            {
                close = true;
            }
            let o = &mut bar.options;
            for (on, label, what) in [
                (&mut o.match_case, "Aa", "Match case"),
                (&mut o.whole_word, "Whole word", "Whole word"),
                (
                    &mut o.ignore_diacritics,
                    "Ignore diacritics",
                    "Ignore diacritics: “αγαπη” finds ἀγάπη, “agape” too",
                ),
            ] {
                if ui
                    .selectable_label(*on, RichText::new(label).small())
                    .on_hover_text(what)
                    .clicked()
                {
                    *on = !*on;
                }
            }
            if ui
                .add_enabled(
                    !bar.text.trim().is_empty(),
                    egui::Button::new(RichText::new("Search all Resources").small()),
                )
                .on_hover_text(format!("Open Search with this text ({})", key("Alt+Enter")))
                .clicked()
            {
                search_all = true;
            }
        });
        let text = bar.text.trim().to_string();
        if search_all && !text.is_empty() {
            self.search_all(tab, &text);
        }
        if close && let Some(t) = self.tabs.get_mut(&tab) {
            t.find = None;
        }
    }

    /// Hand the Find text to the Search Tool as a phrase: over every Bible from a Bible or Text
    /// Comparison Tab, else over every Commentary, Lexicon and Dictionary. The whole scope is
    /// sent, so a Search Tab narrowed earlier (to Romans, or to Dictionaries) is widened again.
    fn search_all(&mut self, tab: TabId, text: &str) {
        let phrase: String = text.chars().filter(|c| *c != '"').collect();
        let bible = match self.session.workspace.tab(tab).map(|t| &t.kind) {
            Some(obs_engine::TabKind::Resource(id)) => self
                .session
                .library
                .get(id)
                .is_some_and(|e| e.manifest.resource_type == ResourceType::Bible),
            _ => true,
        };
        let params: Value = if bible {
            let bibles: Vec<String> = self
                .session
                .library
                .of_type(ResourceType::Bible)
                .map(|e| e.manifest.resource_id.clone())
                .collect();
            json!({
                "text": format!("\"{phrase}\""),
                "mode": "bible",
                "bibles": bibles.join(","),
                // Empty is the Whole Bible, as the Scope field stores it.
                "scope": "",
            })
        } else {
            json!({
                "text": format!("\"{phrase}\""),
                "mode": "library",
                "resources": "commentary,lexicon,dictionary",
            })
        };
        self.run("search.run", &params);
    }
}
