# Menus and commands (#226)

The menu bar is **File · Edit · View · Go · Tools · Window · Help**, left of the toolbar's
Launchers (#140, when the toolbar is at the Top) and the Command box (#139). The menus are
`obs_engine::MENUS` (`crates/engine/src/workspace_commands.rs`); the Launchers keep their own
lists (`Launcher::items`, `crates/engine/src/toolbar.rs`), so the Library, Guides, Tools and
Layouts drop-downs are what they were before the audit.

| Menu | What is in it |
|---|---|
| File | the Library and getting Resources (Import…, Get Resources…, Get Bibles…, Starter library), saving and resetting the Layout, Back up & restore… |
| Edit | Undo and Redo, Copy verses…, Find in Resource with Find next and Find previous, Add to Favorites |
| View | Settings, the look of the app and of Resource text, Highlights and Notebooks in text, the Toolbar |
| Go | moving in the focused Tab (Back, Forward, chapter, entry, parallel resource), Home, the Command box |
| Tools | the Guides, then the Tools |
| Window | Reopen closed Tab and Recently closed, Close Tab and Close Panel, moving between Tabs, Panels and areas, Floating windows |
| Help | All commands, Keyboard shortcuts, Report a problem… and Open log folder, Credits, About Open Bible |

Rarely used items are grouped: View's choices (Theme, Page tone …) and Window → Go to Panel
(Panels 1–9) are submenus. A menu command that needs a Tab (Close Tab, Back, Find in
Resource …) runs on the focused Tab, and is greyed out with no Tab open.

## Where every command is

Every registered command is in exactly one of these places (the test
`every_command_is_in_a_menu_the_command_box_or_its_own_place`):

- **A menu**, once (`no_command_is_in_the_menus_twice_and_no_two_listed_commands_share_a_label`).
- **The Command box.** It lists every command that needs no parameter but the focused Tab
  (`obs_engine::is_listed`, `Session::listed_commands`): typing a command's name (three
  letters or more) suggests it among the other suggestions, and **Help → All commands**
  (`app.commands`, Ctrl+Shift+P) puts the keyboard in the box with `>` typed, which lists
  only commands, all of them, each with its Keyboard shortcut ("Command · Ctrl+W"); typing
  after `>` filters them and Enter runs one, on the focused Tab if it needs one. A `>` text is
  not kept as a recent entry. Menu commands come first, in menu order.
- **Its own place**, for a command that needs a parameter only that place has: a Resource's
  id (`resource.open` from the Library), a Passage, a selection (`annotation.note`), a drop
  (`workspace.drop`), a file from a window (`highlight_palette.import`). The test finds the
  command's id in the UI's sources. `obs_engine::IN_PLACE_COMMANDS` lists the ones that need
  no parameter but are still left out of the Command box.
- **A Setting**, for a choice only Settings offers (`app.log_level`, Log level): its row in
  the Settings Tab runs the command with the value picked (`SettingKind::Choice` in
  `obs_engine::SETTINGS`).
- **Scripting only** (`obs_engine::SCRIPTING_COMMANDS`): commands that answer
  `obs-cli run …` with data and change nothing on screen (`reference.parse`,
  `annotation.list`, `command_box.suggest` …). No menu offers them.

Keyboard shortcuts are shown wherever a command is listed: in the menus (each item's key on the
right), in the Command box, in the Tools Launcher and in Help → Keyboard shortcuts, whose groups
follow the menus (`every_menu_item_shows_its_label_and_shortcut`).

## Naming rules

Checked for every command's label and every menu, choice and submenu title
(`labels_follow_the_naming_rules`):

- **Sentence case.** The first word starts with a capital; a later word does only when it is
  part of a CONTEXT.md term (Close **Tab**, Go to **Panel** 1, Open in **Notes**) or a name
  (Open Bible, Library, Greek, CSV).
- **CONTEXT.md terms keep their casing.** The terms the app capitalises mid-sentence (Tab,
  Panel, Layout, Resource, Notebook, Highlight style, Keyboard shortcut, Text size, Today's
  reading …, the test's `PROPER_TERMS`) are written so in labels: Save **Layout** as…, Reset
  **Text size**, Reset all **Keyboard shortcuts**, Open **Today's reading**. Other terms are
  lower case mid-label (Compare a parallel passage).
- **Ellipses.** "…" is one character, at the end, after a word, and only on a label that
  starts with a verb for a command that asks for something before it acts (a name, a file, a
  choice in a window or Tab, a confirmation): Import…, Get Bibles…, Save Layout as…, Rename
  Layout…, Remove…, Copy verses…, Customize toolbar…, Back up & restore…, Report a problem…
  (the report is shown before anything is sent). A command named for what it opens has
  none: Library, Starter library, Settings, Fonts, Highlight styles, Keyboard shortcuts.
- **No two listed commands share a label** (Word Info's own Word Study button is "Word Study
  of this word", beside "Search this word").

## What the audit changed

- Menu bar: Library, Guides, Tools, Layouts (Launchers), View, Window, Help → File, Edit,
  View, Go, Tools, Window, Help, drawn before the Launchers. 32 commands that only had a key
  or a ⋯ menu entry (Close Tab, Back, Forward, Find in Resource, Undo, Redo, Ctrl+1…9, F6 …)
  are now in a menu.
- Help → List commands did nothing on screen; it is **All commands** and lists them in the
  Command box.
- The Command box suggested only menu commands and the Launchers; it now runs every listed
  command, giving a Tab command the focused Tab.
- Labels: Starter library…, Fonts…, Highlight styles… lost their ellipsis; Save/Restore/Rename/
  Delete/Reset layout → Layout; Reset text size → Text size; Open today's reading → Today's
  reading; Set/Clear/Reset (all) keyboard shortcut(s) → Keyboard shortcut(s); Word Info's
  Word Study → Word Study of this word.

## The menu map

<!-- menu-map:start (written by menu_audit_tests; OBS_BLESS=1 rewrites it) -->

### File

| Item | Command | Keyboard shortcut |
|---|---|---|
| Library | `library.open` | Ctrl+Shift+L |
| Import… | `library.import` |  |
| Get Resources… | `library.get_resources` |  |
| Get Bibles… | `library.get_bibles` |  |
| Starter library | `library.starter` |  |
| — | | |
| Save Layout as… | `layout.save` |  |
| Reset Layout | `layout.reset` |  |
| — | | |
| Back up & restore… | `data.backups` |  |

### Edit

| Item | Command | Keyboard shortcut |
|---|---|---|
| Undo | `edit.undo` | Ctrl+Z |
| Redo | `edit.redo` | Ctrl+Shift+Z |
| — | | |
| Copy verses… | `tool.copy_verses` |  |
| — | | |
| Find in Resource | `tab.find` | Ctrl+F |
| Find next | `tab.find_next` |  |
| Find previous | `tab.find_previous` |  |
| — | | |
| Add to Favorites | `favorite.add` | Ctrl+D |

### View

| Item | Command | Keyboard shortcut |
|---|---|---|
| Settings | `app.settings` | Ctrl+Comma |
| — | | |
| Theme ▸ | `view.theme` (one per choice) | |
| Page tone ▸ | `view.page_tone` (one per choice) | |
| Interface size ▸ | `view.interface_size` (one per choice) | |
| — | | |
| Larger text | `view.text_larger` | Ctrl+Plus |
| Smaller text | `view.text_smaller` | Ctrl+Minus |
| Reset Text size | `view.text_reset` | Ctrl+0 |
| Line spacing ▸ | `view.line_spacing` (one per choice) | |
| Verse numbers ▸ | `view.verse_numbers` (one per choice) | |
| Red letter | `view.red_letter` |  |
| Footnote markers | `view.footnote_markers` |  |
| Fonts | `view.fonts` |  |
| — | | |
| Show Highlights | `view.highlights` |  |
| Highlight styles | `view.highlight_styles` |  |
| Visual filters | `view.visual_filters` |  |
| Notebooks in text ▸ | `notebook.show_in_text` (one per Notebook) | |
| — | | |
| Toolbar position ▸ | `view.toolbar_position` (one per choice) | |
| Auto-hide toolbar | `view.toolbar_auto_hide` |  |
| Customize toolbar… | `view.toolbar_customize` |  |

### Go

| Item | Command | Keyboard shortcut |
|---|---|---|
| Back | `tab.back` | Alt+Left |
| Forward | `tab.forward` | Alt+Right |
| — | | |
| Previous chapter | `tab.previous_chapter` | Ctrl+PageUp |
| Next chapter | `tab.next_chapter` | Ctrl+PageDown |
| Previous entry | `tab.previous_entry` |  |
| Next entry | `tab.next_entry` |  |
| Previous parallel resource | `tab.parallel_previous` | Ctrl+[ |
| Next parallel resource | `tab.parallel_next` | Ctrl+] |
| — | | |
| Home | `home.open` | Alt+H |
| Command box | `command_box.focus` | Ctrl+L |

### Tools

| Item | Command | Keyboard shortcut |
|---|---|---|
| Passage Guide | `guide.passage_guide` |  |
| Exegetical Guide | `guide.exegetical_guide` |  |
| Word Study | `guide.word_study` |  |
| Factbook | `guide.factbook` |  |
| Topic Guide | `guide.topic_guide` |  |
| — | | |
| Search | `tool.search` | Ctrl+Shift+F |
| Text Comparison | `tool.text_comparison` |  |
| Notes | `tool.notes` |  |
| Word Info | `tool.word_info` | Ctrl+Shift+I |
| Cross-references | `tool.cross_references` |  |
| Favorites | `tool.favorites` |  |
| History | `tool.history` |  |
| Reading plans | `tool.reading_plans` |  |
| Atlas | `tool.atlas` |  |
| Timeline | `tool.timeline` |  |

### Window

| Item | Command | Keyboard shortcut |
|---|---|---|
| Reopen closed Tab | `tab.reopen_closed` | Ctrl+Shift+T |
| Recently closed ▸ | `tab.reopen_closed` (one per entry) | |
| — | | |
| Close Tab | `tab.close` | Ctrl+W |
| Close Panel | `panel.close` |  |
| — | | |
| Next Tab in Panel | `tab.next_in_panel` | Ctrl+Tab |
| Previous Tab in Panel | `tab.previous_in_panel` | Ctrl+Shift+Tab |
| Go to Panel ▸ | | |
| ↳ Go to Panel 1 | `panel.focus_1` | Ctrl+1 |
| ↳ Go to Panel 2 | `panel.focus_2` | Ctrl+2 |
| ↳ Go to Panel 3 | `panel.focus_3` | Ctrl+3 |
| ↳ Go to Panel 4 | `panel.focus_4` | Ctrl+4 |
| ↳ Go to Panel 5 | `panel.focus_5` | Ctrl+5 |
| ↳ Go to Panel 6 | `panel.focus_6` | Ctrl+6 |
| ↳ Go to Panel 7 | `panel.focus_7` | Ctrl+7 |
| ↳ Go to Panel 8 | `panel.focus_8` | Ctrl+8 |
| ↳ Go to Panel 9 | `panel.focus_9` | Ctrl+9 |
| Next area of the window | `window.next_area` | F6 |
| Previous area of the window | `window.previous_area` | Shift+F6 |
| — | | |
| Float in new window | `tab.float` |  |
| Move to main window | `tab.dock` |  |

### Help

| Item | Command | Keyboard shortcut |
|---|---|---|
| All commands | `app.commands` | Ctrl+Shift+P |
| Keyboard shortcuts | `help.keyboard_shortcuts` |  |
| — | | |
| Report a problem… | `help.report_problem` |  |
| Open log folder | `help.open_log_folder` |  |
| — | | |
| Credits | `help.credits` |  |
| About Open Bible | `help.about` |  |

<!-- menu-map:end -->
