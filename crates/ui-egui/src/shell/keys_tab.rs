//! Help → Keyboard shortcuts (#177): every binding, grouped by menu, with the filter in the
//! Tab's header. Clicking a shortcut records the next key pressed (Esc cancels); Clear leaves
//! the binding without a key and Reset restores its default. A key another binding has is not
//! saved until the user swaps the two keys or cancels. The bindings and their rules are the
//! engine's ([`obs_engine::key_bindings`]); this Tab changes them only through `keys.*`.

use egui::{Align, Layout, RichText, Ui, vec2};
use obs_engine::key_bindings::{canonical_key, key_binding_groups, not_bindable};
use obs_engine::{KeyBinding, KeyConflict, TabId};
use serde_json::{Value, json};

use super::State;
use super::library_tab::section;
use crate::theme;

/// Widest the command column gets.
const LABEL_W: f32 = 300.0;
/// Narrowest command column; in a Panel too narrow for it each name is above its buttons.
const MIN_LABEL_W: f32 = 120.0;
/// Width of the shortcut buttons.
const KEY_W: f32 = 130.0;
/// Room for Clear and Reset, the gaps between the row's parts and the scroll bar.
const REST_W: f32 = 160.0;

/// A key another binding has, waiting for Swap keys or Cancel.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pending {
    id: &'static str,
    key: String,
    /// Reset asked for it (the binding's default), not a recorded key.
    reset: bool,
    conflict: KeyConflict,
}

/// The Keyboard shortcuts Tab's state across frames.
#[derive(Debug, Default)]
pub(crate) struct KeysUi {
    /// The binding whose new key is being recorded: no other key runs meanwhile.
    recording: Option<&'static str>,
    /// The OS window (egui viewport) the binding is being recorded in: input in another one,
    /// or this one losing focus, stops recording.
    recording_in: egui::ViewportId,
    pending: Option<Pending>,
    /// Why the last key pressed for a binding was refused.
    message: Option<(&'static str, String)>,
    /// The Tab was drawn since the last frame began; recording stops once it is not.
    drawn: bool,
}

impl KeysUi {
    /// A frame begins: recording stops if the Tab went out of sight.
    pub(super) fn begin_frame(&mut self) {
        if !std::mem::take(&mut self.drawn) {
            self.recording = None;
        }
    }

    /// Whether a key is being recorded in the OS window `ui` draws, so the keys typed there
    /// run nothing else. Recording stops first if that window lost focus, or the user went on
    /// in another window (focused it, clicked or pressed a key there): a key typed elsewhere
    /// is not the key being recorded.
    pub(super) fn is_recording_in(&mut self, ui: &Ui) -> bool {
        if self.recording.is_none() {
            return false;
        }
        let here = ui.ctx().viewport_id() == self.recording_in;
        let left = ui.input(|i| {
            let focus = |f: bool| i.events.contains(&egui::Event::WindowFocused(f));
            if here {
                focus(false)
            } else {
                focus(true)
                    || i.pointer.any_pressed()
                    || (i.events.iter())
                        .any(|e| matches!(e, egui::Event::Key { pressed: true, .. }))
            }
        });
        if left {
            self.recording = None;
        }
        here && !left
    }

    /// Whether a key is being recorded, so the keys typed run nothing else.
    #[cfg(test)]
    pub(super) fn is_recording(&self) -> bool {
        self.recording.is_some()
    }
}

/// A modifier pressed alone, which is never a key of its own.
fn is_modifier(key: egui::Key) -> bool {
    use egui::Key::*;
    matches!(
        key,
        ShiftLeft
            | ShiftRight
            | ControlLeft
            | ControlRight
            | AltLeft
            | AltRight
            | SuperLeft
            | SuperRight
    )
}

/// A key press written as a shortcut (`Ctrl+Shift+N`). Ctrl is Cmd on macOS.
fn key_text(key: egui::Key, m: egui::Modifiers) -> String {
    let mut parts = Vec::new();
    if m.command || m.ctrl {
        parts.push("Ctrl");
    }
    if m.shift {
        parts.push("Shift");
    }
    if m.alt {
        parts.push("Alt");
    }
    parts.push(key.name());
    parts.join("+")
}

/// Whether every word of `filter` (lowercase) is in the binding's name, key or group.
fn matches(b: &KeyBinding, filter: &str) -> bool {
    let text = format!(
        "{} {} {}",
        b.label,
        b.key.as_deref().unwrap_or_default(),
        b.group
    )
    .to_lowercase();
    filter.split_whitespace().all(|w| text.contains(w))
}

impl State {
    pub(super) fn keys_body(&mut self, ui: &mut Ui, tab: TabId) {
        self.keys_ui.drawn = true;
        self.record_key(ui);
        let filter = (self.tabs.get(&tab))
            .map(|t| t.filter.trim().to_lowercase())
            .unwrap_or_default();
        let bindings = self.session.key_bindings();
        let conflicts = self.session.key_conflicts();
        let mut act: Option<(&'static str, Value)> = None;
        egui::Frame::new()
            .fill(theme::c().field)
            .inner_margin(egui::Margin::symmetric(18, 12))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                egui::ScrollArea::vertical()
                    .id_salt(("keys", tab))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("Keyboard shortcuts")
                                .font(theme::display(20.0))
                                .color(theme::c().ink),
                        );
                        ui.label(
                            RichText::new(
                                "Click a shortcut, then press the new key (Esc cancels). Ctrl \
                                 is ⌘ on macOS. Your keys are kept in the User database.",
                            )
                            .color(theme::c().muted),
                        );
                        let any_custom = bindings.iter().any(KeyBinding::is_custom);
                        if ui
                            .add_enabled(any_custom, egui::Button::new("Reset all"))
                            .on_hover_text("Every shortcut back to its default")
                            .clicked()
                        {
                            act = Some(("keys.reset_all", json!({})));
                        }
                        let mut shown = 0;
                        // The command column narrows in a narrow Panel, then goes above.
                        let room = ui.available_width() - KEY_W - REST_W;
                        let label_w = (room >= MIN_LABEL_W).then_some(room.min(LABEL_W));
                        for group in key_binding_groups() {
                            let rows: Vec<&KeyBinding> = (bindings.iter())
                                .filter(|b| b.group == group && matches(b, &filter))
                                .collect();
                            if rows.is_empty() {
                                continue;
                            }
                            ui.add_space(14.0);
                            section(ui, group);
                            for b in rows {
                                shown += 1;
                                if let Some(a) = self.key_row(ui, b, conflicts.get(b.id), label_w) {
                                    act = Some(a);
                                }
                            }
                        }
                        if shown == 0 {
                            ui.add_space(14.0);
                            ui.label(
                                RichText::new(format!("No shortcut matches “{filter}”."))
                                    .color(theme::c().muted),
                            );
                        }
                    });
            });
        if let Some((id, params)) = act {
            self.keys_ui.message = None;
            self.run(id, &params);
        }
    }

    /// One binding: its name, its shortcut (click to record), Clear and Reset, and under it
    /// a refused key, a conflict to resolve or another binding with the same key.
    fn key_row(
        &mut self,
        ui: &mut Ui,
        b: &KeyBinding,
        clash: Option<&KeyConflict>,
        label_w: Option<f32>,
    ) -> Option<(&'static str, Value)> {
        let mut act = None;
        let name = |ui: &mut Ui| {
            ui.add(egui::Label::new(RichText::new(b.label).color(theme::c().ink)).truncate());
        };
        if label_w.is_none() {
            ui.add_space(4.0);
            name(ui);
        }
        ui.horizontal(|ui| {
            if let Some(w) = label_w {
                ui.allocate_ui_with_layout(
                    vec2(w, ui.spacing().interact_size.y),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.set_width(w);
                        name(ui);
                    },
                );
            }
            act = self.key_buttons(ui, b);
        });
        if let Some((_, why)) = self.keys_ui.message.as_ref().filter(|m| m.0 == b.id) {
            let why = why.clone();
            ui.indent(("key-message", b.id), |ui| {
                ui.colored_label(theme::c().warn, why);
            });
        }
        if let Some(p) = self.keys_ui.pending.clone().filter(|p| p.id == b.id) {
            ui.indent(("key-conflict", b.id), |ui| {
                ui.colored_label(
                    theme::c().warn,
                    format!("{} is already the key of “{}”.", p.key, p.conflict.label),
                );
                let swap_tip = match self.session.shortcut(b.id) {
                    Some(old) => format!("“{}” gets {old}", p.conflict.label),
                    None => format!("“{}” is left without a key", p.conflict.label),
                };
                // A reserved key (Esc) stays with the binding it is the default of.
                let refused = self.session.swap_refused(p.id, p.conflict.id);
                if let Some(why) = &refused {
                    ui.colored_label(theme::c().warn, format!("{why}; pick another key."));
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(refused.is_none(), egui::Button::new("Swap keys"))
                        .on_hover_text(swap_tip)
                        .clicked()
                    {
                        self.keys_ui.pending = None;
                        act = Some(if p.reset {
                            ("keys.reset", json!({ "command": p.id, "swap": true }))
                        } else {
                            (
                                "keys.set",
                                json!({ "command": p.id, "key": p.key, "swap": true }),
                            )
                        });
                    }
                    if ui.button("Cancel").clicked() {
                        self.keys_ui.pending = None;
                    }
                });
            });
        } else if let Some(c) = clash {
            ui.indent(("key-clash", b.id), |ui| {
                ui.colored_label(
                    theme::c().warn,
                    format!("{} is also the key of “{}”.", c.key, c.label),
                );
            });
        }
        act
    }

    /// A binding's shortcut button (click to record), Clear and Reset.
    fn key_buttons(&mut self, ui: &mut Ui, b: &KeyBinding) -> Option<(&'static str, Value)> {
        let mut act = None;
        let recording = self.keys_ui.recording == Some(b.id);
        let shown = if recording {
            "Press a key…".to_string()
        } else {
            b.key.clone().unwrap_or_else(|| "None".into())
        };
        let text = if b.key.is_some() || recording {
            RichText::new(&shown).monospace().color(theme::c().ink)
        } else {
            RichText::new(&shown).color(theme::c().muted)
        };
        let r = ui
            .add(
                egui::Button::new(text)
                    .selected(recording)
                    .min_size(vec2(KEY_W, 0.0)),
            )
            .accessible_name(format!("{} shortcut: {shown}", b.label))
            .on_hover_text("Click, then press the new key");
        if recording {
            r.request_focus();
            // A click anywhere else stops recording.
            if ui.input(|i| i.pointer.any_pressed()) && !r.hovered() {
                self.keys_ui.recording = None;
            }
        }
        if r.clicked() {
            self.keys_ui.recording = Some(b.id);
            self.keys_ui.recording_in = ui.ctx().viewport_id();
            self.keys_ui.pending = None;
            self.keys_ui.message = None;
        }
        if ui
            .add_enabled(b.key.is_some(), egui::Button::new("Clear"))
            .accessible_name(format!("Clear the {} shortcut", b.label))
            .on_hover_text("No key for this command")
            .clicked()
        {
            self.keys_ui.recording = None;
            self.keys_ui.pending = None;
            act = Some(("keys.clear", json!({ "command": b.id })));
        }
        let default = b.default.unwrap_or("none");
        if ui
            .add_enabled(b.is_custom(), egui::Button::new("Reset"))
            .accessible_name(format!("Reset the {} shortcut", b.label))
            .on_hover_text(format!("Back to the default key ({default})"))
            .clicked()
        {
            self.keys_ui.recording = None;
            self.keys_ui.pending = None;
            let conflict = b.default.and_then(|d| self.session.key_conflict(b.id, d));
            match (b.default, conflict) {
                (Some(key), Some(conflict)) => {
                    self.keys_ui.pending = Some(Pending {
                        id: b.id,
                        key: key.to_string(),
                        reset: true,
                        conflict,
                    });
                }
                _ => act = Some(("keys.reset", json!({ "command": b.id }))),
            }
        }
        act
    }

    /// While a binding is recording, the next key pressed (none of the frame's keys reach
    /// anything else): Esc cancels; a key that cannot be bound says why; a key another binding
    /// has waits for Swap keys or Cancel; any other key is bound at once.
    fn record_key(&mut self, ui: &Ui) {
        let Some(id) = self.keys_ui.recording else {
            return;
        };
        let mut pressed = None;
        ui.input_mut(|i| {
            i.events.retain(|e| {
                let egui::Event::Key {
                    key,
                    pressed: down,
                    modifiers,
                    ..
                } = e
                else {
                    return true;
                };
                if *down && pressed.is_none() && !is_modifier(*key) {
                    pressed = Some((*key, *modifiers));
                }
                false
            });
        });
        let Some((key, modifiers)) = pressed else {
            return;
        };
        self.keys_ui.recording = None;
        if key == egui::Key::Escape && modifiers.is_none() {
            return;
        }
        let key = canonical_key(&key_text(key, modifiers))
            .and_then(|k| not_bindable(&k).map_or(Ok(k), Err));
        match key {
            Err(why) => self.keys_ui.message = Some((id, why)),
            Ok(key) => match self.session.key_conflict(id, &key) {
                Some(conflict) => {
                    self.keys_ui.pending = Some(Pending {
                        id,
                        key,
                        reset: false,
                        conflict,
                    });
                }
                None => {
                    self.run("keys.set", &json!({ "command": id, "key": key }));
                }
            },
        }
    }
}
