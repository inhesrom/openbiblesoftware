# Settings (#213)

**Settings** (Ctrl+, — Cmd+, on macOS — or View → Settings) is one Tab with every preference,
grouped in sections, with a search box in the Tab's header. It adds no store and no second way
to change anything: each row reads the value from the Session and writes it by running the
command the View menu, the toolbar, Home or Search already runs, so the change applies at once
and is saved in the User database as before.

| Crate | File | What |
|---|---|---|
| `obs-engine` | `settings.rs` | `SETTINGS` (each `Setting`: id, `SettingsSection`, label, help, search keywords, `SettingKind`), `settings_matching` / `setting_matches` (the search), `Session::setting_is_default`, command `settings.reset {setting}` |
| `obs-engine` | `workspace_commands.rs` | `app.settings` (Ctrl+Comma, first in the View menu) opens `TabKind::Settings` (single instance, `HeaderBox::Filter`, no link chip) |
| `obs-engine` | `bible_search.rs` | `search.word_forms {value}` takes no `tab` from Settings: every open Search Tab searches again |
| `obs-ui-egui` | `shell/settings_tab.rs` | the Tab: sections, rows, Reset links, the confirmation of Privacy and data actions |
| `obs-ui-egui` | `shell/keys_tab.rs` | `key_binding_rows`, shared with the Keyboard shortcuts Tab and embedded in Settings |

## Sections and Settings

| Section | Setting | Changed by | Reset runs |
|---|---|---|---|
| Text and fonts | Text size | `view.text_smaller`, `view.text_larger` | `view.text_reset` |
| | Latin, Greek, Hebrew text font; Interface font | View → Fonts (`view.fonts`, `view.font`) | `view.font {slot}` (bundled) |
| Interface size | Interface size | `view.interface_size` | `{value: default}` |
| Theme and Page tone | Theme, Page tone | `view.theme`, `view.page_tone` | `{value: system}`, `{value: auto}` |
| Reading layout defaults | Line spacing, Verse numbers | `view.line_spacing`, `view.verse_numbers` | `{value: normal}`, `{value: superscript}` |
| | Red letter, Show Highlights | `view.red_letter`, `view.highlights` (toggles) | the toggle, when off |
| | Highlight styles (#181) | a button: View → Highlight styles… (`view.highlight_styles`) | none (an action; each style edit is undone with Ctrl+Z) |
| | Notebooks in text | `notebook.show_in_text {id, show}` | `show: true` for each hidden one |
| | Open Home at launch | `home.open_at_launch {on}` | `on: false` |
| | Home cards, Guide sections | arranged on Home and in each Guide | `home.reset_cards`, `guide.reset_arrangement {guide}` for each arranged Guide |
| Toolbar | Toolbar position, Toolbar buttons | `view.toolbar_position`, `toolbar.labels` | `{value: top}`, `{value: both}` |
| | Auto-hide toolbar | `view.toolbar_auto_hide` | the toggle, when on |
| | Launchers | Customize toolbar… (`view.toolbar_customize`) | `toolbar.move_launcher` and `toolbar.show_launcher` for each, in default order |
| Keyboard shortcuts | Keyboard shortcuts (the list itself) | `keys.set`, `keys.clear`, `keys.reset` | `keys.reset_all` |
| Library folders and downloads | Library folder (shown; `OBS_LIBRARY`) | — | — |
| | Get Resources, Starter library | `library.get_resources`, `library.starter` | — (actions) |
| Search | Word forms | `search.word_forms {value}` | `value: false` |
| Privacy and data | Clear History, Clear Search history | `history.clear`, `search.clear_history`, after a confirmation | — (actions) |

Under the Page tone choices a line says which tone the pages are in now (#240): "Showing Dark
pages now: Auto follows the theme." for Auto, "Showing Sepia pages in every theme. Choose Auto to
follow the theme." for a chosen tone (`shell/page_tone.rs`, `effective_text`). The first time the
theme turns dark over a chosen Paper or Sepia, a one-time hint offers Auto (Use Auto runs
`view.page_tone {value: auto}`); `Session::page_tone_hint_due` remembers it was offered in the
User database (preference `page-tone-hint`), so it never comes back, even after a restart.

Actions and the library folder are not values, so they have no Reset; every other row has one,
greyed out while the Setting is at its default.

## Search

Every word typed must be in the Setting's label, section title, help line, keywords or (for a
choice) the names of its choices, in any case: "sepia" finds Page tone, "zoom" Interface size,
"hotkeys" Keyboard shortcuts. The Keyboard shortcuts section also shows when only a binding
matches ("larger text" lists the Larger text key under it). The search box has the keyboard
whenever the Tab becomes the focused one, so Ctrl+, then typing searches.

## Adding a preference

Add a `Setting` to `SETTINGS` (and, for a new kind of control, a `SettingKind` with its
`setting_is_default` and `settings.reset` arms and its row in `settings_tab.rs`). The engine
tests `every_preference_command_is_a_setting` and
`every_stored_view_and_toolbar_field_has_a_setting` fail when a View menu item or a stored View
or toolbar field has no Setting.

## Tests

- Engine (`settings_tests.rs`): every preference command and stored field is a Setting; the
  search finds each Setting by its label, section and plain words; Reset restores each default,
  saved across a restart, leaving the others alone; Ctrl+, opens the one Settings Tab.
- UI (`settings_tests.rs`, `a11y_audit_tests.rs`): Ctrl+, opens Settings with the search box
  ready; each Setting is found; changes from the rows apply at once (dark chrome, zoom, text
  size) and each Reset link restores the default after a restart; the embedded Keyboard
  shortcuts; Clear History asks first; keyboard-only use and AccessKit names
  (`keyboard_only_settings`, `every_tab_kind_names_its_interactive_nodes`).
- Page tone (`page_tone_tests.rs`, #240): an open BSB Tab is on the Dark page the frame after
  Theme → Dark (and Follow system with the OS turning dark) and on Paper the frame after
  Light; every reading surface (Bible and Resource panes, Text Comparison, Passage Guide, Notes
  cards, the reference popover, interlinear lines, footnote markers) in Light and Dark with
  `UPDATE_PREVIEWS` snapshots; a chosen Paper is kept and its hint appears once; Settings shows
  the tone in effect. Engine: `the_page_tone_hint_is_due_once_per_user_database_for_a_chosen_light_tone`.
