# Keyboard shortcuts (#177)

**Help → Keyboard shortcuts** opens one Keyboard shortcuts Tab: every Keyboard shortcut,
grouped by menu, with a Filter box in the Tab's header. Clicking a shortcut records the next
key pressed; Clear leaves the command without a key; Reset restores its default; Reset all
restores every default. The user's own keys are kept in the User database and apply at once:
the menus, tooltips, the Launchers and the key handling all read them.

## What is listed

| Group | Bindings |
|---|---|
| Each menu (Library, Guides, Tools, Layouts, View, Help), in menu order | the menu's commands that a key can run |
| Selection | the Selection keys (`obs_engine::SELECTION_KEYS`): Note and the 5 Highlight styles |
| Other commands | every other command a key can run (Back, Forward, Find, Close Tab, the Launchers …), in registration order |

A command can run from a key when it needs no parameter except its Tab
(`key_bindings::runs_from_a_key`); the Shell gives such a command the focused Tab. The
Selection keys are not registered commands: only the Shell knows the reading-pane selection
(#62). They have stable ids (`selection.note`, `selection.highlight_yellow` …) so they are
stored and rebound like commands.

## The engine side (`crates/engine/src/key_bindings.rs`)

- `Session::shortcut(id)` is the key a binding has now: the user's own, else its
  `CommandSpec::shortcut` (or the Selection key's default). `Session::key_bindings()` lists
  them; `key_conflict(id, key)` names the binding that already has a key;
  `key_conflicts()` finds bindings sharing a key (a newer version's default can meet a key the
  user chose; the Tab flags both rows).
- Commands: `help.keyboard_shortcuts`; `keys.set {command, key, swap}`,
  `keys.clear {command}`, `keys.reset {command, swap}`, `keys.reset_all`. A key in use is
  refused, naming the other binding, unless `swap` is true: then the other binding takes this
  one's old key (none if it had none). Reset to a default another binding has now is the
  same conflict. A swap never moves a refused key off the binding it is the default of
  (`Session::swap_refused`): rebinding Unpin Word Info to Ctrl+F with swap is refused, since
  Find in Resource would get Esc.
- Keys are written in egui notation and stored one way (`canonical_key`): Ctrl, Shift, Alt in
  that order, then egui's key name (`Ctrl+Up`, `Ctrl+OpenBracket`). Ctrl+Plus drops Shift and
  is the same key as Ctrl+= (#137), so they conflict.
- Refused keys (`not_bindable`): Esc (closes and cancels; Esc also cancels recording), Enter
  (text fields use Enter, Shift+, Ctrl+ and Alt+Enter), Tab (moves the keyboard focus), and
  any key without Ctrl or Alt except F1–F35 (a key alone or with Shift types text or moves
  in a pane). Defaults are not checked: Unpin Word Info keeps Esc.
- The User database preference `key_bindings` is a JSON object, command id → key, or `null`
  for a cleared key. A key equal to the default is not stored, so a later default change
  still applies. A saved key that no longer reads is dropped.

## The UI side (`crates/ui-egui/src/shell/keys_tab.rs`)

- While a key is being recorded, the Shell runs no shortcut and no Selection key
  (`KeysUi::is_recording`), and the recorder takes every key event of the frame. Modifier
  keys alone are ignored. Recording stops on Esc, on a click elsewhere, and when the Tab is
  not drawn (another Tab chosen, the Tab closed).
- A recorded key in use shows "Ctrl+F is already the key of “Find in Resource”." with
  **Swap keys** and **Cancel**; nothing is saved until one is chosen. When the other binding
  may not take the old key (`swap_refused`), the row says why and Swap keys is disabled.
- Selection keys match like commands' keys: each is also pressed as its `pressed_as`
  reports, so a Selection key on Ctrl+Plus runs on Ctrl+= too.
- Every shortcut's button is named "*Command* shortcut: *key*", and Clear and Reset are named
  for their command, so screen readers and `tab_to` tell rows apart.

## Tests

- Engine: `key_bindings_tests.rs` (rebinding survives a restart, Reset, conflicts and swap,
  reserved keys, one spelling per key, every binding listed once with no conflicting
  defaults, flagged conflicts).
- UI: `keyboard_shortcuts_tests.rs` (the acceptance criteria with real key events and a
  restart, and Swap keys never handing Esc away),
  `annotation_tests::a_rebound_selection_key_highlights_and_its_old_key_does_not` and
  `a_selection_key_on_ctrl_plus_also_runs_on_ctrl_equals`,
  and the accessibility audit (`keyboard_only_keyboard_shortcuts`, every Tab kind named).
