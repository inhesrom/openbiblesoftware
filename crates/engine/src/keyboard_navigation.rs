//! Keyboard navigation across Panels and Tabs (#215). Each is a command with a default key, so
//! Help → Keyboard shortcuts lists it and the user can rebind it (#177):
//!
//! - `tab.next_in_panel` (Ctrl+Tab) and `tab.previous_in_panel` (Ctrl+Shift+Tab) go down or
//!   up the Panel's Tab switcher list: its Tabs, the Tab first, then the others most recently
//!   used first. One press goes back to the Tab used before. The UI shows the list while Ctrl
//!   is held and runs `tab.next_in_panel` with the `steps` picked once Ctrl is let go.
//! - `panel.focus_1` … `panel.focus_9` (Ctrl+1…9) put the keyboard in the Nth Panel, in the
//!   order the screen shows them ([`Workspace::panels_in_order`](crate::Workspace)). The
//!   Highlight Selection keys moved from Ctrl+1…9 to Alt+1…9 to make room.
//! - `tab.back` and `tab.forward` (Alt+Left, Alt+Right) are the Tab's Back and Forward.
//! - `tab.previous_chapter` and `tab.next_chapter` (Ctrl+Page Up, Ctrl+Page Down) go to the
//!   previous or next chapter in the Tab's own numbering; in a Commentary to its previous or
//!   next section, in a Lexicon or Dictionary to its previous or next entry.
//! - `window.next_area` (F6) and `window.previous_area` (Shift+F6) move the keyboard between
//!   the areas of the window the keys were typed in: the menu bar, the Toolbar and each Panel.

use obs_scripture::{Standard, Verse, Versification};
use serde_json::{Value, json};

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::session::Session;
use crate::workspace::{Effect, Tab, TabId};
use crate::workspace_commands::tab_param;

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

/// The Tab switcher's list for `tab` (#215); see [`crate::Workspace::tab_switcher_order`].
/// `steps` down it (up it without `forward`), wrapping round: the Tab to show.
pub fn tab_switcher_pick(order: &[TabId], steps: i64, forward: bool) -> Option<TabId> {
    let n = i64::try_from(order.len()).ok().filter(|n| *n > 0)?;
    // Reduced before going up the list: negating `i64::MIN` would overflow.
    let down = steps.rem_euclid(n);
    let at = if forward { down } else { (n - down) % n };
    order.get(usize::try_from(at).ok()?).copied()
}

fn switch_tab(s: &mut Session, p: Params<'_>, forward: bool) -> Result<Value, CommandError> {
    let id = if forward {
        "tab.next_in_panel"
    } else {
        "tab.previous_in_panel"
    };
    let tab = tab_param(id, s, &p)?;
    let order = s.workspace.tab_switcher_order(tab);
    let to = tab_switcher_pick(&order, p.i64("steps").unwrap_or(1), forward)
        .ok_or_else(|| failed(id, "the Tab is in no Panel"))?;
    s.workspace.focus(to);
    s.workspace.push_effect(Effect::KeyboardToTab(to));
    Ok(json!({ "tab": to.0 }))
}

/// Whether there is an `n`th Panel (1 first).
fn has_panel(s: &Session, n: usize) -> bool {
    s.workspace.panels_in_order().len() >= n
}

fn focus_panel(s: &mut Session, id: &'static str, n: usize) -> Result<Value, CommandError> {
    let panel = (s.workspace.panels_in_order().get(n - 1).copied())
        .ok_or_else(|| failed(id, format!("there is no Panel {n}")))?;
    let tab = (s.workspace.active_in(panel)).ok_or_else(|| failed(id, "the Panel is empty"))?;
    s.workspace.focus(tab);
    s.workspace.push_effect(Effect::KeyboardToTab(tab));
    Ok(json!({ "panel": panel.0, "tab": tab.0 }))
}

/// The first Verse of the chapter after (or before) the one `at` is in.
fn chapter_step<V: Versification + ?Sized>(v: &V, at: Verse, forward: bool) -> Option<Verse> {
    if forward {
        v.next_verse(v.last_verse_of(at.book, at.chapter)?)
    } else {
        let before = v.prev_verse(v.first_verse(at.book, at.chapter)?)?;
        v.first_verse(before.book, before.chapter)
    }
}

/// Where Ctrl+Page Down (`forward`) or Ctrl+Page Up takes `t`: a Verse, or an entry Key.
enum PageTo {
    Verse(Verse),
    Entry(obs_model::Key),
}

fn page_target(t: &Tab, forward: bool) -> Option<PageTo> {
    if t.keeps_entries() {
        let at = t.entry_history.current()?;
        let next = t.navigation.as_ref()?.adjacent(&at, forward)?;
        return Some(PageTo::Entry(next.key.clone()));
    }
    let at = t.history.current()?;
    if let Some(nav) = &t.navigation {
        return nav.adjacent_section(at, forward).map(PageTo::Verse);
    }
    let to = match &t.versification {
        Some(v) => chapter_step(v.as_ref(), at, forward),
        None => chapter_step(&Standard, at, forward),
    };
    to.map(PageTo::Verse)
}

/// Whether the Tab shows something with chapters, sections or entries to page through.
fn pages(t: &Tab) -> bool {
    if t.keeps_entries() {
        t.navigation.is_some() && t.entry_history.current().is_some()
    } else {
        t.history.current().is_some()
    }
}

/// The chapter keys are left alone unless the focused Tab can page (with no focused Tab, as
/// from `obs-cli`, they run and report what is wrong).
fn focused_pages(s: &Session) -> bool {
    (s.workspace.focused().and_then(|t| s.workspace.tab(t))).is_none_or(pages)
}

fn page(s: &mut Session, p: Params<'_>, forward: bool) -> Result<Value, CommandError> {
    let id = if forward {
        "tab.next_chapter"
    } else {
        "tab.previous_chapter"
    };
    let tab = tab_param(id, s, &p)?;
    let to = (s.workspace.tab(tab)).and_then(|t| page_target(t, forward));
    match to {
        Some(PageTo::Verse(v)) => {
            s.workspace.navigate(tab, v);
            Ok(json!({ "verse": v.osis() }))
        }
        Some(PageTo::Entry(key)) => {
            s.workspace.navigate_entry(tab, key.clone());
            Ok(json!({ "key": key }))
        }
        None => Err(failed(
            id,
            if forward {
                "This is the last chapter, section or entry"
            } else {
                "This is the first chapter, section or entry"
            },
        )),
    }
}

const TAB: ParamSpec = ParamSpec {
    name: "tab",
    kind: ParamKind::Integer,
    required: true,
    help: "The Tab",
};

const STEPS: ParamSpec = ParamSpec {
    name: "steps",
    kind: ParamKind::Integer,
    required: false,
    help: "How many Tabs along the Tab switcher's list (default 1)",
};

macro_rules! focus_panel {
    ($n:literal, $id:literal, $label:literal, $key:literal) => {
        CommandSpec {
            id: $id,
            label: $label,
            shortcut: Some($key),
            params: &[],
            enabled: |s| has_panel(s, $n),
            run: |s, _| focus_panel(s, $id, $n),
        }
    };
}

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "tab.next_in_panel",
        label: "Next Tab in Panel",
        shortcut: Some("Ctrl+Tab"),
        params: &[TAB, STEPS],
        enabled: always,
        run: |s, p| switch_tab(s, p, true),
    },
    CommandSpec {
        id: "tab.previous_in_panel",
        label: "Previous Tab in Panel",
        shortcut: Some("Ctrl+Shift+Tab"),
        params: &[TAB, STEPS],
        enabled: always,
        run: |s, p| switch_tab(s, p, false),
    },
    focus_panel!(1, "panel.focus_1", "Go to Panel 1", "Ctrl+1"),
    focus_panel!(2, "panel.focus_2", "Go to Panel 2", "Ctrl+2"),
    focus_panel!(3, "panel.focus_3", "Go to Panel 3", "Ctrl+3"),
    focus_panel!(4, "panel.focus_4", "Go to Panel 4", "Ctrl+4"),
    focus_panel!(5, "panel.focus_5", "Go to Panel 5", "Ctrl+5"),
    focus_panel!(6, "panel.focus_6", "Go to Panel 6", "Ctrl+6"),
    focus_panel!(7, "panel.focus_7", "Go to Panel 7", "Ctrl+7"),
    focus_panel!(8, "panel.focus_8", "Go to Panel 8", "Ctrl+8"),
    focus_panel!(9, "panel.focus_9", "Go to Panel 9", "Ctrl+9"),
    CommandSpec {
        id: "tab.previous_chapter",
        label: "Previous chapter",
        shortcut: Some("Ctrl+PageUp"),
        params: &[TAB],
        enabled: focused_pages,
        run: |s, p| page(s, p, false),
    },
    CommandSpec {
        id: "tab.next_chapter",
        label: "Next chapter",
        shortcut: Some("Ctrl+PageDown"),
        params: &[TAB],
        enabled: focused_pages,
        run: |s, p| page(s, p, true),
    },
    CommandSpec {
        id: "window.next_area",
        label: "Next area of the window",
        shortcut: Some("F6"),
        params: &[],
        enabled: always,
        run: |s, _| {
            s.workspace.push_effect(Effect::NextArea { forward: true });
            Ok(Value::Null)
        },
    },
    CommandSpec {
        id: "window.previous_area",
        label: "Previous area of the window",
        shortcut: Some("Shift+F6"),
        params: &[],
        enabled: always,
        run: |s, _| {
            s.workspace.push_effect(Effect::NextArea { forward: false });
            Ok(Value::Null)
        },
    },
];
