# Keyboard navigation across Panels and Tabs (#215)

Every key here is a registered command with a default key, so Help → Keyboard shortcuts
lists it under *Other commands* and the user can rebind, clear or reset it (#177,
`keyboard-shortcuts.md`). The engine side is `crates/engine/src/keyboard_navigation.rs`
(commands) and `crates/engine/src/workspace/panel_order.rs` (orders); the UI side is
`crates/ui-egui/src/shell/keyboard_nav.rs`.

| Key | Command | What it does |
|---|---|---|
| Ctrl+Tab | `tab.next_in_panel {tab, steps}` | Tab switcher: down the Panel's list |
| Ctrl+Shift+Tab | `tab.previous_in_panel {tab, steps}` | Tab switcher: up the list |
| Ctrl+1 … Ctrl+9 | `panel.focus_1` … `panel.focus_9` | the keyboard to the Nth Panel |
| Alt+← / Alt+→ | `tab.back` / `tab.forward` | the Tab's Back and Forward (unchanged) |
| Ctrl+Page Up / Ctrl+Page Down | `tab.previous_chapter` / `tab.next_chapter` | previous or next chapter, section or entry |
| F6 / Shift+F6 | `window.next_area` / `window.previous_area` | the keyboard to the next or previous area |

## The Tab switcher

`Workspace::tab_switcher_order(tab)` is the Tab's Panel: the Tab first, then the Panel's other
Tabs most recently active first (never-active ones in the Panel's order). `tab.next_in_panel`
with `steps` n shows the Tab n places down that list (wrapping); `tab.previous_in_panel` n
places up it. One step is the Tab used before, so a quick Ctrl+Tab flips between two Tabs.

In the Shell a switcher key does not run at once: the first press opens the switcher
(`TabSwitcher`) with a snapshot of the list and the second entry picked, each further press
moves the pick, and the list is drawn over the Panel. While a modifier of the key that opened
it (Ctrl, Cmd or Alt) is held it stays up; once let go, the Shell runs `tab.next_in_panel`
with the steps to the picked Tab, so the engine's activity order only changes on that one
switch. Esc (with Ctrl still held) closes it without switching; clicking a row picks it. A
key rebound to something that cannot be held (F7 alone) switches at once.

On macOS Ctrl means Cmd, but Cmd+Tab is the system's app switcher and never reaches the app:
`shell::pressed_as` also takes Ctrl+Tab as the Control key with Tab there (and the Control key
is Ctrl on Windows and Linux anyway).

## Panels in screen order (Ctrl+1…9)

`Workspace::panels_in_order()` reads `Workspace::arrangement` (the main window's Panel tree,
which the UI keeps up to date) depth first: a row's children left to right, a column's top to
bottom. Then each Floating window's arrangement, oldest window first, then any Panel the UI has
not placed yet, by id. `panel.focus_N` is disabled when there is no Nth Panel, so the key is
left alone.

The keyboard lands on the Panel's active Tab: the Workspace focuses it (its keys act there)
and queues `Effect::KeyboardToTab`, and the Shell has that Tab's button in the Panel's tab bar
take the keyboard when it is next drawn (`State::tab_button_drawn`), where the focus ring
(#182) shows it. Enter or Space on it does nothing new; Tab moves on into the Tab's header and
body.

### The Highlight keys moved

The Highlight Selection keys were Ctrl+1…9 (#62, #181). They are Alt+1…9 now. Ids are
unchanged (`selection.highlight_1` …), so a key the user set for a slot is kept; only the
defaults moved. On macOS Alt is Option: Option+digit types ¡ ™ £ …, but egui falls back to
the physical digit key for a character it has no key for, so the binding still matches, and
the reading pane takes no text.

`shell::extra_held` now compares only bindings with the same Ctrl/Cmd, so Ctrl+1 and Alt+1
on the same digit do not force each other to match Shift exactly (Ctrl+1 still arrives as
Ctrl+Shift+1 on AZERTY). Ctrl+digit never takes an extra Alt: Ctrl+Alt is AltGr on Windows,
which types { [ ] } on digits on some layouts, even in a text field.

## Ctrl+Page Up / Page Down

On the Tab's Current location: a Bible moves by chapter in its own Versification (Malachi 3
to Matthew 1 in a Bible that numbers Malachi with three chapters); a Commentary (a Resource
with Passage-keyed sections) to the next or previous section start
(`ResourceNavigation::adjacent_section`); a Lexicon or Dictionary to the next or previous
entry (as Next entry / Previous entry); a Passage-keyed Guide or Tool by chapter in the
Standard versification. Each is a jump (`Workspace::navigate`), so Back returns and the Link
set follows. Ctrl+Page Up mid-chapter goes to the chapter before, not to this one's start.
The commands are disabled while the focused Tab has nothing to page through (Library, Home),
and run otherwise even at the ends, where they say there is no further chapter.

## F6 and Shift+F6

The engine queues `Effect::NextArea { forward }`; the Shell, reading the keys of one window,
lists that window's stops as last drawn: in the main window the menu bar (its first menu) and
the Toolbar (its first Launcher; none while Auto-hide hides it), then the window's Panels in
`panels_in_order`. The current stop is the smallest one round the widget with the keyboard
(the Toolbar at the Top is inside the menu bar), else the focused Tab's Panel; F6 goes to the
next, wrapping, Shift+F6 to the previous. A Panel stop focuses its active Tab and puts the
keyboard on its tab, as Ctrl+1…9 do. In a Floating window that is an OS window of its own the
stops are its Panels.

The issue names a *status area*: the app has none (messages are toasts that fade). Nothing
else is drawn there for F6 to visit, so the menu bar takes that place in the round, giving
the keyboard a way to the menus.

## Tests

- Engine: `keyboard_navigation_tests.rs`: the switcher's order and steps, staying in the
  Panel, Panels in screen order (rows, columns, Floating windows), chapters in a Bible's own
  numbering, sections and entries, F6's effects, and every key listed with no conflicts.
- UI: `keyboard_navigation_tests.rs`: real key events for the switcher held and let go
  (Ctrl+Shift+Tab, Esc, the macOS Control key), Ctrl+1…9 with the focus ring (AZERTY Shift,
  AltGr), Ctrl+Page keys and Alt+arrows, F6 / Shift+F6 round the areas with a ring at each
  stop, and the keys as Help → Keyboard shortcuts names them. The Highlight key tests in
  `annotation_tests.rs` and `highlight_styles_tests.rs` press Alt+digit now.
