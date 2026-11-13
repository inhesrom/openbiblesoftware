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
| | Resources with their own Text size (#292): each Resource with its size and Reset | a Tab's Text size row (`view.resource_text_size`, `view.resource_text_larger`, `view.resource_text_smaller`) | one: `settings.reset {setting: resource_text_sizes, resource}`; Reset all: `settings.reset {setting: resource_text_sizes}` |
| | Latin, Greek, Hebrew text font; Interface font | View → Fonts (`view.fonts`, `view.font`) | `view.font {slot}` (bundled) |
| Interface size | Interface size | `view.interface_size` | `{value: default}` |
| Theme and Page tone | Theme, Page tone | `view.theme`, `view.page_tone` | `{value: system}`, `{value: auto}` |
| Motion (#225) | Reduce motion | `view.reduce_motion {on}` | `on: false` |
| | Scroll speed (#293): a logarithmic slider, 0.25× to 4× | `view.scroll_speed {value}` (also from the Command box: `scroll speed 2`) | `value: 1` |
| Reading layout defaults | Line spacing, Verse numbers | `view.line_spacing`, `view.verse_numbers` | `{value: normal}`, `{value: superscript}` |
| | Paragraphs or verse per line, Reading column width (#209) | `view.text_flow`, `view.column_width` | `{value: paragraphs}`, `{value: medium}` |
| | Large chapter numbers, Section headings, Justified text (#209) | `view.large_chapter_numbers {on}`, `view.section_headings {on}`, `view.justified {on}` | `on: false`, `on: true`, `on: false` |
| | Red letter, Show Highlights | `view.red_letter`, `view.highlights` (toggles) | the toggle, when off |
| | Highlight styles (#181) | a button: View → Highlight styles (`view.highlight_styles`) | none (an action; each style edit is undone with Ctrl+Z) |
| | Notebooks in text | `notebook.show_in_text {id, show}` | `show: true` for each hidden one |
| | Open Home at launch | `home.open_at_launch {on}` | `on: false` |
| | Home cards, Guide sections | arranged on Home and in each Guide | `home.reset_cards`, `guide.reset_arrangement {guide}` for each arranged Guide |
| Toolbar | Toolbar position, Toolbar buttons | `view.toolbar_position`, `toolbar.labels` | `{value: top}`, `{value: both}` |
| | Auto-hide toolbar | `view.toolbar_auto_hide` | the toggle, when on |
| | Expanded toolbar (#291): Left and Right only | `view.toolbar_expanded {on}` | `on: false` |
| | Launchers | Customize toolbar… (`view.toolbar_customize`) | `toolbar.move_launcher` and `toolbar.show_launcher` for each, in default order |
| Keyboard shortcuts | Keyboard shortcuts (the list itself) | `keys.set`, `keys.clear`, `keys.reset` | `keys.reset_all` |
| Library folders and downloads | Library folder (shown; `OBS_LIBRARY`) | — | — |
| | Get Resources, Starter library | `library.get_resources`, `library.starter` | — (actions) |
| Search | Word forms | `search.word_forms {value}` | `value: false` |
| Privacy and data | Clear History, Clear Search history | `history.clear`, `search.clear_history`, after a confirmation | — (actions) |
| Updates (#276) | Check for updates automatically (on), Include pre-releases (off) | `updates.auto_check {on}`, `updates.prereleases {on}` | `on: true`, `on: false` |
| | Check for updates | a button: Help → Check for updates (`help.check_for_updates`) | — (an action) |

Under the Page tone choices a line says which tone the pages are in now (#240): "Showing Dark
pages now: Auto follows the theme." for Auto, "Showing Sepia pages in every theme. Choose Auto to
follow the theme." for a chosen tone (`shell/page_tone.rs`, `effective_text`). The first time the
theme turns dark over a chosen Paper or Sepia, a one-time hint offers Auto (Use Auto runs
`view.page_tone {value: auto}`); `Session::page_tone_hint_due` remembers it was offered in the
User database (preference `page-tone-hint`), so it never comes back, even after a restart.

## Scroll speed (#293)

One multiplier for the mouse wheel and the trackpad (`Appearance::scroll_speed`, saved with the
View preferences as `scroll_speed`, never in a Layout). The UI applies it once a frame in every
viewport (`motion::scroll_input`, called by `Shell::ui` and by each Floating window's own pass):
it scales egui's `smooth_scroll_delta`, which every scrollable area reads (egui's ScrollAreas,
reading panes, Text Comparison, Search results, the Note margin), so mouse-wheel lines and
trackpad points are scaled alike, and egui's smoothing of a big wheel step keeps working. With
Reduce motion on it scales the instant whole-step delta instead. It does not scale
`input_options.line_scroll_speed`: egui 0.37 feeds that into Ctrl+wheel zoom too. Ctrl+wheel
never becomes a scroll delta, so zoom is untouched; the Atlas and the Timeline zoom by
`motion::unscaled_scroll_delta`, the wheel's own amount (the Timeline's sideways swipe still
scrolls at the Scroll speed; drag-panning is not a wheel). An arrow key in a reading pane
scrolls `pane::ARROW_STEP` × the Scroll speed; Page Up/Down and Space still move one page.
Scrolling the Settings Tab uses a new speed from the next frame, the slider's live preview.

Actions and the library folder are not values, so they have no Reset; every other row has one,
greyed out while the Setting is at its default.

## A Resource's own Text size (#292)

A Resource can have an Own Text size (CONTEXT.md) over the global Text size: absolute, in the
same 12–40 pt range, for every Tab of the Resource. The global Larger and Smaller text never
move it; Reset makes the Resource follow the global size again.

| Crate | File | What |
|---|---|---|
| `obs-engine` | `resource_text_size.rs` | `ResourceTextSizes` (resource id → points), kept in the User database preference `view.resource_text_sizes` (JSON object; a size out of range is clamped on load, so a Backup holding one is refused by the trial restore); `Session::text_size_of`, `text_settings_of`, `text_size_resource(tab)`; the commands below |
| `obs-engine` | `workspace.rs` | `Tab::clicked_resource`, set by `Workspace::clicked_into`: the Resource last clicked into in a Text Comparison or Search Tab (not saved) |
| `obs-engine` | `backup_check.rs` | the trial restore compares `view.resource_text_sizes` like every other setting |
| `obs-ui-egui` | `shell/text_size_controls.rs` | the Text size row (label and "own size" or "follows Text size", A−, slider, A+, the size, Reset) at the top of a Tab's ⋯ menu, and the header's **Aa** button that opens the row alone |
| `obs-ui-egui` | `shell.rs`, `compare_view.rs`, `search_view.rs` | a Resource Tab's view takes `text_settings_of` its Resource; Text Comparison rows and Search hits each take their Resource's size (`set_own_sizes`, `row_settings`, `settings_of`); a click or right-click on a row reports its Resource (`clicked_into`) |
| `obs-ui-egui` | `shell/settings_tab.rs` | the Setting's list: each Resource with its size and Reset; the Setting's Reset link reads Reset all |

Commands, all acting on the Tab's **Text size Resource** (`Session::text_size_resource`): a
Resource Tab's Resource; in a Text Comparison or Search Tab the one last clicked into while it
is still shown, else the first row's; none in other Tabs (the key commands are then disabled
and leave their keys alone). `tab` (default: the focused Tab) or `resource` name another.

| Command | Key | What |
|---|---|---|
| `view.resource_text_larger` | Ctrl+Alt+Plus (also Ctrl+Alt+= and numpad +) | one point up from the size the Resource is drawn at |
| `view.resource_text_smaller` | Ctrl+Alt+Minus | one point down |
| `view.resource_text_reset` | Ctrl+Alt+0 | follow the global Text size again |
| `view.resource_text_size {value}` | — | the slider's form: an absolute size (no message; the row shows it) |

The three keys are Ctrl+Plus, Ctrl+Minus and Ctrl+0 with Alt. Ctrl+Plus and Ctrl+Alt+Plus are
told apart by Alt alone, so both may still be pressed with Shift (+ is Shift+= on a US
keyboard; `shell::extra_held`). Ctrl+Alt is AltGr on Windows, which types ~ \ } with these keys
on some layouts, so the keys are left to a focused text field (`annotate::TEXT_FIELD_KEYS`).

Every change runs a command at once and the views take the new size on the next frame through
#299's fast path (scaled height estimates, stale layouts skipped), so the text follows the
slider while it is dragged. The ⋯ menu and the Aa popover close only on a click outside
(`components::menu_staying_open`); each of the ⋯ menu's items closes it itself.

Tests: engine `resource_text_size_tests.rs` (shared by a Resource's Tabs, the global size not
moving an own one, the range, a restart and Layout loads, Backup and restore, the Text
Comparison Bible clicked into, keys and the Command box, Settings); UI
`resource_text_size_tests.rs` (the ⋯ row live while dragging with the menu open, the Aa button
and the keyboard on the slider with its accessible name, Ctrl+Alt keys and the global keys,
every Tab keeping its Current location, Text Comparison rows, Settings' list).

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
