# Accessibility audit of the new surfaces (#182)

#68 gave Resource text an AccessKit tree. This audit covers the chrome added since #131:
toolbar Launchers and panes, the Command box, Get Resources, the Starter library, the Guides
(Passage Guide, Word Study, Factbook, Exegetical Guide, Topic Guide), interlinear lines, the Search views,
Reading plans, Home, Favorites and History, the Atlas (#150), the Timeline (#185), Collections, Copy verses, Fonts, Customize toolbar and the
right-click menu. It checks three things, and all three run in `cargo xtask ci` (gate `a11y`, and again in
`test`):

```sh
cargo xtask a11y   # cargo test -p obs-ui-egui --lib a11y_audit
```

The tests are in `crates/ui-egui/src/a11y_audit_tests.rs` and use the shell's real-event
headless harness with AccessKit on (the same harness every UI test uses; no second test
dependency, as decided in `resource-tabs.md`).

## 1. Every control has a role and a name

`every_tab_kind_names_its_interactive_nodes` opens every Tab kind (the match in
`every_tab_kind` has no catch-all, so a new Tab kind does not build until the audit opens
it), each over the default layout, with the Starter library's test Resources and again with
an empty Library (the Guides' get-the-Resource buttons). The Search Tab is checked in each of
its Verses, Grid, Chart and Analysis views and in Library mode (the Resource and Collection
choosers) and Notes mode; the Bible Tab with interlinear lines on, and again with its
Contents pane (#176) open.
`every_pane_menu_and_window_names_its_interactive_nodes` does the same for the four toolbar
panes (Library also while searching), the Command box's suggestions, the View and Help menus,
Customize toolbar…, Fonts…, Copy verses…, the New Collection window and the right-click menu,
and checks Esc closes each.

A node is **interactive** when AccessKit can click or focus it. Its **name** is its label,
else the names of the nodes that label it (`labelled_by`), and for a `Label` its value. ARIA
asks no name of a window or a scroll bar, so those are left out. A tooltip names a widget that
has no name of its own (egui does this), so icon buttons with a tooltip pass.

## 2. Everything works from the keyboard, with a visible focus ring

One `keyboard_only_*` test per surface, keyboard events only (no clicks, no `shell.run`
except to set up data): open it, use it, close it. `tab_to` presses Tab until the named
control has the keyboard and then checks that a ring in the focus colour is drawn round it.

| Surface | Opens with | Uses | Closes with |
|---|---|---|---|
| Starter library (#131) | Ctrl+L `starter library` | Tab to *Open KJV (test)*, Enter | Ctrl+W |
| Get Resources (#132, #133, #136) | Ctrl+L `get resources` | Tab to *Lexicons*, Enter | Ctrl+W |
| Empty states (#135) | Ctrl+L `G26` (empty Library) | Tab to *Get Lexicons…*, Enter | Ctrl+W |
| Fonts (#138) | Ctrl+L `fonts` | Tab to *Latin text font*, Space opens the list | Esc, Esc |
| Command box (#139) | Ctrl+L | `gen 1:3`, Enter | Esc |
| Toolbar pane (#140), History (#155) | Alt+T `history`, Enter | Tab to the *Genesis 1:2 · KJV* Visit, Enter (the Bible goes back there) | Ctrl+W |
| Customize toolbar (#141) | Ctrl+L `customize toolbar` | Tab to *Labels only*, Space | Esc |
| Right-click menu (#142) | Shift+F10 | Tab to *Verse: Gen 1:1*, *Passage Guide*, Enter | Esc / Ctrl+W |
| Interlinear lines (#143) | Alt+L `tagnt`, Enter | Tab to *Interlinear*, Space | Ctrl+W |
| Exegetical Guide (#144) | Ctrl+L `eg Lk 1:30` | Tab to a word, Enter (Word Study) | Ctrl+W |
| Search views (#146) | Ctrl+Shift+F | query, Enter; Tab to *Grid*, Enter | Ctrl+W |
| Reading plans (#147) | Ctrl+L `reading plans` | Tab to *Start plan*, Enter | Ctrl+W |
| Home (#178) | Alt+H | Tab to *Open Home at launch*, Space; Tab to *Move Today's reading down*, Enter; Tab to *Word Study*, Enter (a Word Study) | Ctrl+W |
| Factbook (#149) | Alt+G, Tab to the Factbook box, `Abraham`, Enter | Tab to the *Terah* link, Enter (his Factbook) | Ctrl+W |
| Topic Guide (#148) | Ctrl+L `topic forgiveness` | Tab to *Topic Guide: Forgiveness*, Enter; Tab to *Search your library for “Forgiveness”*, Enter (Search) | Ctrl+W |
| Parallel resources (#151) | Ctrl+] | Tab to *Previous of your Bibles*, Enter | — |
| Collections (#153) | Tab to *New Collection…*, Enter | *Name*, *Rules*, Tab to *Save*, Enter | Esc |
| Copy verses (#154) | Ctrl+L `copy verses` | Tab to *Copy plain text*, Enter | Esc |
| Favorites (#155) | Ctrl+D, Ctrl+L `favorites` | Tab to the Favorite, Enter | Ctrl+W |
| Keyboard shortcuts (#177) | Ctrl+L `keyboard shortcuts` | *Filter* `larger`; Tab to *Larger text shortcut: Ctrl+Plus*, Enter, Ctrl+↑; Tab to *Reset the Larger text shortcut*, Enter | Ctrl+W |
| Settings (#213) | Ctrl+, (the search box has the keyboard) | `page tone`; Tab to *Page tone: Sepia*, Space; Tab to *Reset Page tone*, Enter; *Search settings* `red letter`; Tab to *Red letter*, Space; Tab to *Reset Red letter*, Enter | Ctrl+W |
| Timeline (#185) | Ctrl+L `timeline` | Tab to *Fit*, Enter; Tab to *Era: Patriarchs, …*, Enter (the view fits it); Tab to *Event: Birth of Abraham, …*, Enter (the Link set moves to Gen 11:26); Tab to *Lifespan: Abraham, …*, Enter (his Factbook) | Ctrl+W |
| Panel separator | Tab | → → widens the left Panel by 48 points | — |
| Floating window (#152), an OS window of its own | `tab.float` on a Search Tab, a Bible split beside it (set-up) | In that window: Tab to *Search for*, query, Enter; Tab to *Grid*, Enter; Tab to *Resize Panels*, → → | — |

The floating-window test (`keyboard_only_floating_window`) draws the window as eframe does,
a viewport with its own input and its own AccessKit tree, and also checks that tree has no
unnamed interactive node. Each OS window has its own keyboard focus, so the focus ring, the
scroll into view and the separators' names and arrow keys are applied per window.

## 3. Contrast meets WCAG AA in every theme × Page tone

`every_surface_meets_wcag_aa_contrast_in_every_theme_and_page_tone` draws every surface of
section 1 in Light and Dark with each Page tone (Auto, Paper, Sepia, Dark): 8 combinations. For
each run of text the chrome paints it finds what the run is painted on (the solid fills drawn
before it, in paint order and within their clip, composited under the middle of the row) and
requires 4.5:1, or 3:1 for large text (24 pt, or 18.66 pt bold) and for symbols that are not
words (★, ≡, ×, ■), which WCAG counts as graphics. Text inside a disabled control is exempt, as
WCAG exempts inactive components. Each surface is drawn for 15 frames first so fades have
settled. Resource text itself (glyph quads, not text shapes) is covered by
`every_ink_on_every_highlight_meets_wcag_aa_in_every_tone` (#21).

`chrome_text_colours_meet_wcag_aa_on_every_chrome_background` checks the chrome palette for
states the rendered check does not reach (hover, a selected row): ink, muted, lapis, verdigris
and warn on shell, surface, field, hover and selection, the ★ and the focus ring at 3:1, the
text on every Link set chip and badge, and the Grid view's heat cells at every heat.

## Findings and fixes

| Finding | Fix |
|---|---|
| Header boxes (reference, word, name, Lexicon, headword, filter), the Command box and the fields and drop-downs in Get Resources, Search, Fonts, Copy verses, Notes, the Note editor, Reading plans, Favorites, the Collection editor and the toolbar panes had no name (a hint is not a name) | `accessible_name`, mostly the hint or the visible label beside it |
| The Command box's suggestions, the panes' footer links, Credits' linked text were unnamed | Named, with roles (list option, link); a focused Credits text opens its first URL with Enter |
| egui_tiles' Panel separators and tab-bar backgrounds were unnamed Tab stops with no focus | `shell/tile_handles.rs` names them, in the main window and in floating windows (*Resize Panels*, a splitter; *Panel tab bar*); ←/→ (↑/↓) move a focused separator 24 points |
| Many custom widgets (Guide headers, word links, Factbook cards, Layout cards, the reading pane) and egui buttons showed focus by colour alone, or not at all | One focus ring (2 pt, lapis) round whatever has the keyboard; text fields keep their own border |
| A link wrapped across two rows (Import a file…; the Topic Guide's Passage and *see* links, e.g. *Matt 19:11*) is two widgets in egui; the second had no name | It starts a new row when it does not fit (`shell::unbroken_link`, also used for the Factbook's Passage and family links) |
| Tab moved the keyboard to widgets scrolled out of sight without scrolling, so the focus ring was out of sight (the Topic Guide's Search button) | A widget the keyboard has just moved to is scrolled into view, through AccessKit's Scroll into view as a screen reader would ask |
| Word Study's disclosure arrow was a second Tab stop with no name for the entry title's action | Mouse only; the title is the stop |
| Ambiguous names: *…*, *+*, *▶*, *◀*, *A*, *Open* ×10, *Download* ×n | *Tab menu*, *Open a Tab in this Panel*, *Next/Previous of your Bibles*, *Link set A*, *Open KJV (test)*, *Download …*; menu targets read *Verse: Gen 1:1* |
| Shift+F10 opened nothing without the pointer over the text and no selection, so the menu had no keyboard-only route | It selects the Verse at the top of the view and acts on that; moving onto a target with Tab chooses it, as hovering does, and a pointer resting on another target does not take it back until it moves (arrow keys also move between the columns) |
| Fonts, Copy verses and Customize toolbar could only be closed with the mouse or by tabbing to their × | Esc closes them (after first closing a drop-down inside), one at a time: the window with the keyboard, or with the keyboard in the Panels or none, the one in front; none while the keyboard is in another overlay. Esc in the Note editor saves it and closes nothing behind it |
| Hints and shortcuts (egui's faded ink) were 4.16:1 on fields | `weak_text_color` is the theme's muted ink |
| Light: muted on the selection 4.37:1, verdigris on the selection 4.13:1, the ★ (gilt) 2.66:1 | Light muted, verdigris and gilt darkened |
| Dark: lapis on the selection 3.35:1 and on hover 3.95:1, muted on hover 4.45:1, verdigris on hover 4.19:1 | Dark selection deepened; lapis, muted and verdigris lightened |
| White text on the Link set chips (Dark: 3.00:1; unlinked: 2.60:1) and on the update badge (3.16:1) | `theme::text_on` picks white or near-black for the fill (black if neither meets AA, as in mid-heat Grid cells) |
| Highlight swatches (■) in menus were the pale highlight fills, 1.0–1.5:1 on the menu | They use the style's margin-mark colour for the chrome's theme (designed for 3:1) |

## Gaps left

- **Moving Tabs and Panels by keyboard.** egui_tiles' drag-to-dock has no keyboard equivalent,
  and the tab-bar background (drag to move the whole Panel) is still a Tab stop that does
  nothing from the keyboard; it is named so a screen reader says what it is.
  The other drags (#216: references, selected text, Resources, Note cards) each have a click
  or keyboard way to do the same; `drag-and-drop.md` lists them. A Note card's ✏ grip is a
  Tab stop named *Drag to move this Note* that does nothing from the keyboard.
- **egui_tiles' tab close buttons** are named *Close*, not *Close KJV*.
- **Reading pane caret.** There is no keyboard caret or Shift+arrow selection in Resource text,
  so the keyboard-only right-click menu acts on the Verse at the top of the view (and on
  whatever a screen reader selected through AccessKit). Word-level actions need the pointer
  or a screen reader's selection.
- **Library rows** reorder by drag or the row's right-click menu; the rows themselves have no
  Shift+F10 menu, and each row's drag handle is a Tab stop.
- **Tab order is long** (toolbar, Command box, then each Panel's tab bar and header) and there
  is no shortcut to move between Panels (F6 in many apps).
- **What is checked** is text drawn as text shapes against solid fills. Launcher icons and
  other line drawings are checked only through the chrome colour test, not as drawn.
- **Surfaces not built yet**: the Atlas (#150) and the Home Tab (#178) are open tickets.
  Because `every_tab_kind` matches `TabKind` without a catch-all, the first
  of them to add a Tab kind has to add it to the audit.
- **No screen reader was run.** The checks read the AccessKit tree the app publishes; nobody
  has tried NVDA, VoiceOver or Orca against it on this host (no display, no screen reader).

## Previews

Rendered by `a11y_audit::previews` (`UPDATE_PREVIEWS=1`) from the harness, converted with
ffmpeg:

- [Focus ring on Get Resources' Lexicons chip, reached with Tab](../previews/a11y-focus-ring-get-resources-kittest.png)
- [The right-click menu from Shift+F10 alone: the top Verse selected, its target focused](../previews/a11y-keyboard-menu-kittest.png)
- [Dark theme: the Tools pane's selected row and the Link set chip's dark text](../previews/a11y-dark-tools-pane-kittest.png)
