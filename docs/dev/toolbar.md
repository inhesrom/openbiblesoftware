# The toolbar Launchers (#140)

The top bar is the app's name, the menus (File · Edit · View · Go · Tools · Window · Help,
#226, `docs/dev/menus.md`), four **Launchers** (Library, Guides, Tools, Layouts), and the
Command box (#139) on the right. The Launchers can move to another edge
of the window, auto-hide and be customized (#141, below). Each Launcher is an icon-and-label
button that drops down a pane anchored under it; the panes replaced the Library, Guides, Tools
and Layouts text menus, and keep their own lists (`Launcher::items`) since the menu audit
(#226) brought those commands back into File, Edit and Tools.

| Launcher | Shortcut | Pane |
|---|---|---|
| Library | Alt+L | Search field (title, abbreviation, Resource type, language). With nothing typed: Recent, Favorites (up to four), then the top three of each type's Preferred order. Footer: Open full Library, Import…, Get Resources… (with its update count), Get Bibles…, Starter library |
| Guides | Alt+G | Passage Guide, Word Study and Factbook, each with an input box |
| Tools | Alt+T | Filter field, then the Tools Launcher's commands with their shortcuts |
| Layouts | Alt+Y | Filter field, saved Layouts and Built-in Layouts as cards with a thumbnail of their Panel tiling; Rename and Delete on a saved card; footer: Save Layout as…, Reset Layout |

The Library launcher shows the number of downloaded Resources with an update as a badge, as
the Library menu's title did (#136).

## Keyboard

Every pane is usable without the mouse:

- Alt+L / Alt+G / Alt+T / Alt+Y open the pane (again: close it). The keyboard goes to the
  pane's field (in Guides, the first Guide's box).
- Typing searches (Library) or filters (Tools, Layouts). The pick goes back to the top.
- ↑/↓ move the pick through the rows and on into the footer links. In Layouts, while the
  filter is empty, ↑/↓ move to the card above or below as drawn (same column, saved cards,
  then Built-in, then the footer links) and ←/→ one card; while it has text the field keeps
  ←/→ and ↑/↓ step through every match and footer link one at a time. In Guides ↑/↓ move
  between the Guides' boxes. A pick the keyboard moves out of view scrolls into it.
- Enter runs the pick. Ctrl/Cmd+Enter opens a Resource in a new Tab. Text typed in the same
  frame as Enter counts: the pick is resolved after the field has taken it.
- The Library rows and the Layout cards scroll inside the pane, which stays inside the
  window; the footer links stay in view.
- Layouts: F2 renames the picked saved Layout in place (Enter keeps the name, Esc stops), and
  Delete deletes it.
- Esc closes the pane. So does a click anywhere outside it.

Opening any Resource is two interactions: the Library launcher (Alt+L or a click), then type
and Enter.

## Library: where a Resource opens

`toolbar.open_resource` (engine, `crates/engine/src/toolbar.rs`):

- Click / Enter: in the focused Panel. If that Panel already has a Tab of the Resource, that
  Tab comes to the front instead of a second one opening.
- Ctrl/Cmd-click / Ctrl/Cmd+Enter: always a new Tab, split off as a new Panel beside the
  focused one, as Ctrl/Cmd does for every other way of opening a Tab.
- Drag a row out of the pane onto the Workspace: dropped on a Panel, it becomes a Tab there;
  dropped on the right quarter of a Panel, it opens in a new Panel beside it. The target is
  marked while dragging. A row of the Library Tab drags the same way (#216, see
  `drag-and-drop.md`).

Search ranking (`Session::library_launcher`): every word typed must match the abbreviation or
title (anywhere), or start a word of the Resource type's name or the language's name or tag.
Best first: an exact abbreviation or title, then abbreviations and titles the text starts,
then the rest; ties in type order and Preferred order. At most twelve rows.

Recent (`Session::recent_resources`): the Resources of History's Visits, newest first, then
those of open Tabs, most recently active first.

## Guides: what the boxes take

| Guide | Box | Runs |
|---|---|---|
| Passage Guide | a Bible reference (Standard versification) | `guide.passage_guide {passage}` |
| Word Study | a Strong's number, Lemma, transliteration or English words | read as the Command box reads `ws …`: `word_study.open`, else `word_study.choose` |
| Factbook | a name | `guide.factbook {name}` |

An empty box opens the Guide as its menu item did.

## Layouts: thumbnails and Built-in Layouts

`Session::layout_cards` reads each saved Layout's stored snapshot and turns its arrangement
(`LayoutNode`: rows, columns and Panels with their shares) into a `LayoutThumbnail`; nothing is
screenshotted. Each Panel tile is titled with its active Tab (and "+n" for more Tabs) and
topped with its Link set's colour.

The Built-in Layouts (`layout.builtin {name}`) are made from the library's Preferred order when
loaded, with fresh Tab ids, and open at the Standard Verse the focused (else most recent
Passage-keyed) Tab shows; their Bible's Link set A carries the others there:

| Built-in Layout | Panels |
|---|---|
| Bible + Commentary | ★ Bible \| ★ Commentary (both in Link set A) |
| Word study desk | ★ Bible \| Word Study over Word Info (3 : 2) |
| Parallel Bibles | first \| second Bible in Preferred order (Link set A) |
| Reading only | ★ Bible |

A Built-in Layout whose Resources the library lacks shows its card greyed with what it needs
("Needs a Commentary in the library"); clicking it says so and changes nothing.

## Position and customization (#141)

The Launchers can sit on any edge of the window. The engine keeps where and what
(`ToolbarSettings`, `crates/engine/src/toolbar_settings.rs`); the UI draws it
(`crates/ui-egui/src/shell/toolbar_bar.rs`). The settings are an app preference in the User
database (the `toolbar` preference), read when the User database is attached; they are not in
the Workspace snapshot, so loading a Layout never moves the toolbar.

| Position | Where | Buttons | A pane opens |
|---|---|---|---|
| Top (default) | in the menu bar, after the app's name | icons and labels (or as customized) | under its button |
| Bottom | a bar along the window's bottom | icons and labels (or as customized) | over its button |
| Left | a column at the left, under the menu bar | Collapsed: icons only, labels in the tooltips; Expanded: icon, label and detail line | to its right |
| Right | a column at the right, under the menu bar | Collapsed: icons only, labels in the tooltips; Expanded: icon, label and detail line | to its left |

The menu bar keeps the app's name, View, Help, Home (#178), Today's reading and the Command box wherever the
Launchers are, so View → Toolbar position is always reachable.

Ways to change it:

- **View → Toolbar position ▸ Top / Bottom / Left / Right** (`view.toolbar_position {value}`).
  From the keyboard: Tab to View, Enter, Tab to Toolbar position, Enter, Tab to the edge, Enter.
- **Right-click the toolbar** (a Launcher, the grip, the pop-out control or the bar between
  them): Position ▸, Expanded and Collapsed (Left and Right only), Auto-hide, Customize….
- **Drag the grip** (the dots at the toolbar's start): the window edge nearest the pointer is
  marked while dragging; letting go moves the toolbar there. The grip is not a Tab stop.
- **View → Auto-hide toolbar** (`view.toolbar_auto_hide`): the Launchers leave their place and
  are drawn over the Workspace, at their edge, only while the pointer is at that window edge
  (within 6 points) or on the toolbar, Alt is held, or one of its panes or menus is open. At the
  Top they show under the menu bar when the pointer reaches the window's top edge.
- **View → Customize toolbar…** (`view.toolbar_customize`): a checkbox per Launcher to show or
  hide it (`toolbar.show_launcher`), Move up / Move down to order them
  (`toolbar.move_launcher`), Buttons show: Icons and labels, Icons only, Labels only
  (`toolbar.labels`), At the left or right: Collapsed or Expanded (`view.toolbar_expanded`),
  Position, Auto-hide, and Reset (`toolbar.reset`).

A hidden Launcher keeps its shortcut (Alt+L, Alt+G, Alt+T, Alt+Y) and is listed by Help → List
commands; the Command box suggests it by name ("Guides launcher", or just "guides"). Its pane
drops down from the toolbar's start.

**Small windows.** The Launchers that do not fit along the toolbar (beside the View and Help
menus and the Command box at the Top; the window's height at Left and Right) go into a »
button at the toolbar's end, in toolbar order; choosing one there drops its pane down from ».
Nothing is clipped.

## Expanded and Collapsed (#291)

At the Left and Right the toolbar is **Collapsed** (icons only, the labels in the tooltips,
as above) or **Expanded**: a wider column where each Launcher's row shows its icon, its label
and beside it, muted, a detail line: its shortcut as the platform writes it and its status
("Library ⌥L · 2 updates" on macOS; the Collapsed column shows the count as a badge on the
icon). The rows are as tall as the Collapsed buttons, so the pop-out control stays under the
pointer: clicking it again at once undoes the click. Top and Bottom are unchanged and ignore
the choice.

- **The pop-out control** is the last button of a side toolbar, after the Launchers and the »
  button: one click flips Collapsed ↔ Expanded. Its double chevron points the way the
  toolbar's inner edge moves: » on the Left and « on the Right while Collapsed, the other way
  (with the word "Collapse") while Expanded. It is a Tab stop: F6 puts the keyboard on the
  first Launcher, Tab reaches the control, and Enter or Space flips it. Its accessible name
  states the state ("Expand toolbar (now collapsed)", "Collapse toolbar (now expanded)") and it
  reads as a toggle that is on while Expanded.
- **Elsewhere:** right-click the toolbar (Expanded, Collapsed), Customize toolbar… (At the left
  or right), Settings → Toolbar → Expanded toolbar, and the Command box ("Expanded toolbar"),
  all running `view.toolbar_expanded` (`{on: true|false}`; without `on` it flips). Its
  Keyboard shortcut is Ctrl+Shift+B (⇧⌘B on macOS). Hovering never expands it.
- **Width:** fixed, not resizable: as wide as the widest row (label, detail line and the
  pop-out control's "Collapse") at the current Interface size, up to a maximum (a 180-point
  text column, `toolbar_look::expanded_width`); longer text ends in "…".
- **Room:** the Expanded column pushes the Workspace over, and panes open beside the wider
  column. With Auto-hide it is drawn over the Workspace like the Collapsed one. A short window
  moves the Launchers it has no room for into », as Collapsed; the pop-out control always keeps
  its place at the end.
- Expanded is the `expanded` field of the `toolbar` preference, kept across launches and never
  part of a Layout.

## Restyling

The look lives in one file, `crates/ui-egui/src/shell/toolbar_look.rs`: sizes, the colour
roles (taken from the chrome theme, so Light and Dark follow), the launcher button (icons,
labels or both), the Expanded rows and the pop-out control (#291), the grip, the » button, the docked bar's frame and the drop-edge mark, the four
icons (our own line drawings: three books on a shelf, a compass, a two-by-two grid, a tiled
window), rows, headings, footer links, Layout cards and thumbnails, and the drag ghost and drop
target. `toolbar.rs` only decides what is listed and what happens.

Page tone (Paper, Sepia, …) colours Resource text only; the toolbar is chrome and follows the
Theme (Light, Dark), like the rest of the chrome.

## Previews

Rendered headlessly by `toolbar_tests::previews` (`UPDATE_PREVIEWS=1`), light and dark:
`docs/previews/toolbar-{library,library-search,guides,tools,layouts}-{light,dark}.png`.

Positions and customization (#141), by `toolbar_position_tests` (`UPDATE_PREVIEWS=1`):
`docs/previews/toolbar-{bottom,left,right}.png` (each with the Library pane open),
`toolbar-{left,right}-expanded-{light,dark}.png` (Expanded, #291, the Library pane open),
`toolbar-view-menu.png`, `toolbar-customize.png` and `toolbar-overflow.png` (a 760-point window).
The real app (macOS, `OBS_SCRIPT` with `view.toolbar_position` then `toolbar.library`,
`toolbar.layouts` or `toolbar.guides`): `toolbar-real-left.png`, `toolbar-real-bottom-dark.png`,
`toolbar-real-right.png`; Expanded (#291, then `view.toolbar_expanded`):
`toolbar-real-left-expanded.png`, `toolbar-real-right-expanded-dark.png`, and
`toolbar-real-left-expanded-restart-shortcut.png` (a relaunch keeps it Expanded; ⇧⌘B collapses
it).
