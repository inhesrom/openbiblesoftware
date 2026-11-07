# Drag and drop (#216)

Things are moved around the Workspace with the pointer, as in other study apps, in our own
look. Every drag has a keyboard or click equivalent (listed last).

## User-facing

- **Tabs.** Drag a Tab's title onto another Panel's tab bar to move it there, or onto a
  Panel's edge to split it off as a new Panel (egui_tiles; its preview shows where it docks).
  Dragged out of its window it floats (#152).
- **A Bible reference** in any text (a Commentary's "Rom 8:28", a Lexicon's citation, a Bible's
  cross-reference): drag it instead of clicking it.
- **Selected text**: a drag that starts inside the selection carries the selected text (a
  word, or a reference written plainly). A drag that starts outside it selects, as before.
- **Resources**: drag a row out of the Library launcher's pane, or a row of the Library Tab
  (by the row itself; its ≡ handle still reorders the Preferred order).
- **Note cards**: drag a Note card's ✏ grip onto a Resource's text. A **Move Note** box asks
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

A Note drops only on a Resource's text: on the selection when dropped inside it, else on the
Verse under the pointer (a Bible) or the word (any other Resource).

Resources keep the Library launcher's targets (#140): a Panel (a Tab there) or its right
quarter (a new Panel).

Files dragged onto the window from the system (a `.obr`) still join the library and open
(`App::files_dropped`); drags inside the Workspace never carry files.

## Contract

- Engine: `workspace.drop` (`crates/engine/src/drag_drop.rs`) takes `passage` (a Standard
  reference) or `text`, an optional `bible`, and where: `tab`, `panel` (its tab bar) or
  `beside` (its right edge); none, a new Tab where its kind opens. A new Bible Tab gets
  `Effect::NavigateStandard`, so it moves once its Verse map is loaded.
- UI, `pane.rs`: a drag whose press was on a Scripture reference, or inside a selection with
  text, reports `PaneOutput::drag_out` and sets `ReadingView::dragging_out`, which keeps the
  selection as it is for the rest of the drag.
- UI, `shell/drag_drop.rs`: `DragDropUi` holds the drag and the OS window it started in.
  After each window's dock is drawn, `workspace_drag` finds the Drop target under the pointer
  (`drop_panels`: that window's Panels and front Tabs), highlights it and drops on release. A
  reference's Passage comes from the reference target cache (the same Standard targets a
  click uses); a drop made before the worker has them waits in `pending`.
- Notes: `annotation.reattach` with the selection params of where the Note was dropped, after
  the Move Note box (`reanchor_prompt`).
- Library Tab rows sense click-and-drag; a drag of the row sets `ToolbarUi::dragging`, the
  same Resource drag as the Library launcher's (`Shell::library_drag`).

## Keyboard and click equivalents

| Drag | Instead |
|---|---|
| a reference onto a Tab | click it (its Link set moves), or type it in the Tab's box |
| a reference onto an edge | Ctrl/Cmd-click it (a new Tab) |
| a word onto a Lexicon or Search | type it in the box, or the right-click menu's Search this / Look up in Dictionary |
| a Resource | Enter, double-click or Ctrl/Cmd-click in the Library |
| a Note card | select the text, then the card's Re-attach here (Needs attention) |
| a Tab | Float in new window / Move to main window (docking itself has no keyboard way yet) |

## Tests

- `crates/engine/src/drag_drop_tests.rs`: every Drop target for a Passage and for text.
- `crates/ui-egui/src/drag_drop_tests.rs` (real pointer events): Rom 8:28 from a Commentary
  onto a Bible Tab and onto its Panel's edge; a selected word onto a tab bar; Esc; a drag
  inside and outside the selection; a Library Tab row onto an edge; a Note card moved after
  confirming (Cancel, Move, Undo); where a Note can drop.
- `tabs_can_be_dragged_to_split_dock_and_resize` (`shell_tests.rs`): Tab docking.
- `a_resource_file_dropped_on_the_window_from_the_system_joins_the_library_and_opens`
  (`tests.rs`): system file drop.
