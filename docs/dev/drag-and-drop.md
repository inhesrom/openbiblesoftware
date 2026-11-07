# Drag and drop (#216)

Things are moved around the Workspace with the pointer, as in other study apps, in our own
look. Every drag has a keyboard or click equivalent (listed last).

## User-facing

- **Tabs.** Drag a Tab's title onto another Panel's tab bar to move it there, or onto a
  Panel's edge to split it off as a new Panel (egui_tiles; its preview shows where it docks).
  Dragged out of its window it floats (#152).
- **A Bible reference** in any text (a Commentary's "Rom 8:28", a Lexicon's citation, a Bible's
  cross-reference): drag it instead of clicking it.
- **Selected text**: a drag that starts on selected text carries it (a word, or a reference
  written plainly). A drag that starts anywhere else, the margin beside it included, selects,
  as before.
- **Resources**: drag a row out of the Library launcher's pane, or a row of the Library Tab
  (by the row itself; its ≡ handle still reorders the Preferred order).
- **Note cards**: drag a Note card's grip (its Note icon) onto a Resource's text. A **Move Note** box asks
  "Move this Note from Genesis 30:1 (TB) to Genesis 30:5 (TB)?"; Move re-anchors it (Edit →
  Undo moves it back), Cancel or Esc leaves it.

### Drop targets

While dragging, the Drop target under the pointer is washed in lapis and a label follows the
pointer. For a reference or text:

| Dropped on | What happens |
|---|---|
| a Tab's text | that Tab goes there: a Bible, Commentary, Passage Guide or other Passage Tab to the Passage (a jump its Link set follows; a Guide shows a chapter or range whole); a Lexicon, Dictionary, Word Study, Factbook or Topic Guide looks the word up as if typed in its box; a Search Tab searches for it |
| a Panel's tab bar | a new Tab in that Panel |
| a Panel's right quarter | a new Panel beside it |

A new Tab for a Passage is the Bible the drag came from, else the ★ Bible; for a word, a
Search for it. Text counts as a Bible reference when it parses as one and has a chapter or
verse number ("Rom 8:28", "Ps 23"); a bare book name ("Job", "Mark") is a word. Dropped back
on the Tab it came from, nothing happens; Esc cancels any drag.

Selected text is read where it was written (CONTEXT.md, Bible reference): "Genesis 32:1"
selected in a Bible numbered the Hebrew way is that Bible's 32:1, the Standard Genesis 31:55,
so an English Bible it is dropped on goes to 31:55. Selected anywhere else it is Standard.

A Note drops only on a Resource's text, under the pointer: on the selection when dropped
inside it, else on the Verse under the pointer (a Bible) or the word (any other Resource). The
Tab's header, its Contents, the pane's margins and the space after the text are no target.

Resources keep the Library launcher's targets (#140): a Panel (its tab bar or a Tab's text: a
Tab there) or its right quarter (a new Panel). A row dropped back on its own Library Tab opens
nothing. Every drag, a Library Tab row's too, drops in the window it started in: a floating
window's Library opens Resources among that window's Panels.

Files dragged onto the window from the system (a `.obr`) still join the library and open
(`App::files_dropped`); drags inside the Workspace never carry files.

## Contract

- Engine: `workspace.drop` (`crates/engine/src/drag_drop.rs`) takes `passage` (a Standard
  reference) or `text` (with `from`, the Tab it was dragged from: a Bible reference in a
  Bible's text is parsed in its Versification and translated through its Verse map, a range
  to the Standard range covering every Verse it maps to, even when the map reorders them; an
  Unmapped first verse fails), an optional `bible`, and where: `tab`, `panel` (its tab bar) or
  `beside` (its right edge); none, a new Tab where its kind opens. A new Bible Tab gets
  `Effect::NavigateStandard`, so it moves once its Verse map is loaded.
- UI, `pane.rs`: a drag whose press was on a Scripture reference, or on a glyph inside a
  selection with text (the cluster hit, not the nearest caret), reports `PaneOutput::drag_out` and sets `ReadingView::dragging_out`, which keeps the
  selection as it is for the rest of the drag.
- UI, `shell/drag_drop.rs`: `DragDropUi` holds the drag and the OS window it started in.
  After each window's dock is drawn, `workspace_drag` finds the Drop target under the pointer
  (`drop_panels`: that window's Panels and front Tabs), highlights it and drops on release. A
  reference's Passage comes from the reference target cache (the same Standard targets a
  click uses); a drop made before the worker has them waits in `pending`.
- Notes: a Tab takes a Note only where `ReadingView::text_under` finds text (inside the pane
  last drawn, `pane_rect`, on a laid-out line, never clamped as `pos_at` does). Then
  `annotation.reattach` with the selection params of where it was dropped, after the Move
  Note box (`reanchor_prompt`, also opened by the card menu's Move to selected text), drawn
  in the window of the Tab it was dropped on (a floating OS window draws its own, as the Note
  editor).
- Library Tab rows sense click-and-drag; a drag of the row is a `Dragged::Resource` in
  `DragDropUi`, so it drops in the window it started in (`toolbar.open_resource` with `panel`
  or `beside`). The Library launcher's pane, only in the main window, keeps its own drag
  (`ToolbarUi::dragging`, `Shell::library_drag`).

## Keyboard and click equivalents

| Drag | Instead |
|---|---|
| a reference onto a Tab | click it (its Link set moves), or type it in the Tab's box |
| a reference onto an edge | Ctrl/Cmd-click it (a new Tab) |
| a word onto a Lexicon or Search | type it in the box, or the right-click menu's Search this / Look up in Dictionary |
| a Resource | Enter, double-click or Ctrl/Cmd-click in the Library |
| a Note card | select the text, then the card's menu (… or right-click), Move to selected text (the same Move Note box); an Orphaned card's Re-attach here |
| a Tab | Float in new window / Move to main window (docking itself has no keyboard way yet) |

## Tests

- `crates/engine/src/drag_drop_tests.rs`: every Drop target for a Passage and for text.
- `crates/ui-egui/src/drag_drop_tests.rs` (real pointer events): Rom 8:28 from a Commentary
  onto a Bible Tab and onto its Panel's edge; a selected word onto a tab bar; Esc; a drag
  inside and outside the selection; a Library Tab row onto an edge; a Note card moved after
  confirming (Cancel, Move, Undo); where a Note can drop.
- `floating_windows_tests.rs` (native windows, each with its own input): a Library Tab row
  dragged onto Panels of the same floating window; the Move Note box in a floating Bible's
  window, Esc there.
- `tabs_can_be_dragged_to_split_dock_and_resize` (`shell_tests.rs`): Tab docking.
- `a_resource_file_dropped_on_the_window_from_the_system_joins_the_library_and_opens`
  (`tests.rs`): system file drop.
