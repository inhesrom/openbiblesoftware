# The Notes Tool and Notes search (#64)

The Notes Tool is a single-instance Tab (`tool.notes`, Tools → Notes) for browsing, editing and
managing every Annotation (#18 rule 7). The Search Tool's Notes mode searches Note bodies
(#16 rules 10 and 13). Both build on the annotation service and `notes_fts` described in
[annotations.md](annotations.md).

| Layer | Piece | Role |
|---|---|---|
| `obs-search` | `compile_notes` | The #16 query grammar compiled for `notes_fts` (one `body` column). A Strong's number or Lemma matches itself as written in a Note. |
| `obs-formats` | `scripture_references` | Bible references in typed text, with byte ranges (Note bodies). English words that are also book spellings need a period. |
| `obs-layout` | `styled_snippet_paragraph` | An inline snippet with some byte ranges in their own colour role (references as links). |
| `obs-engine` | `notes.rs` | Cards, following anchors, Show in text, Notes search; commands `notes.notebook`, `notes.order`, `notes.open`, `notes.follow`, `notes.show_passage`, `search.notebook`. |
| `obs-ui-egui` | `shell/notes_tab.rs`, `shell/search_tab.rs` | The Notebook column, the virtualized card list, the delete dialog, and the Notes results. |

## The Notebook column

- **All**, then each Notebook (default first, then by name) with its count and its 👁
  **Show in text** toggle, then **+ New notebook** (type a name, Enter).
- Double-click a Notebook (or right-click → Rename…) to rename it in place: Enter or a click
  elsewhere saves, Esc cancels.
- Right-click → **Delete…** asks what happens to its Annotations: **Move them to the default
  Notebook** (pre-selected) or **Delete them** (#17 rule 19). The default Notebook can be
  renamed but not deleted. Either way it is one undo step: Ctrl+Z brings the Notebook and its
  Annotations back exactly.
- **⚠ Needs attention** (#65, #17 rule 12) lists the Orphaned annotations of every
  Notebook, with their count. It shows only when there are some, or while it is chosen
  (then, once all are dealt with: "Nothing needs attention"). `notes.notebook` with
  `value: attention` chooses it.

### Orphaned cards

An Orphaned annotation's card (in Needs attention and in its Notebook) says "⚠ This text was
not found in *TB* after it changed", shows its quote, and offers:

- **Re-attach here**: anchors it on the text selected in the most recently used reading pane
  (`annotation.reattach`), exactly as a new Highlight or Note there would be; the selection
  clears so it shows at once, and the warning goes. Disabled, with a hint, until some text is
  selected.
- **Keep without anchor** (Notes only, `annotation.detach`): the Note stays in its Notebook
  with no anchor.
- **Delete**.

Each is one undo step. Re-anchoring itself is described in [annotations.md](annotations.md).

### Show in text

The 👁 toggle is #63's Show in text ([annotations.md](annotations.md), "In other Bibles"): it
runs `notebook.show_in_text` (`id`, `show`), which sets the Notebook's `show_in_text` column as
one undo step and moves `annotations_revision`, so every reading pane reloads its Marks from
`Session::annotations_in_text()`. The column reads `Session::hidden_in_text()` (the Notebooks
with it off). The View menu's "Notebooks in text" submenu sets the same flag. A hidden
Notebook's Annotations still list in the Notes Tool.

## Cards

`Session::note_cards(filter)` returns the chosen Notebook's Annotations (or all) as `Card`s:
the Annotation, its Notebook's name, its reference label, its quote, for a Text-range
anchor whose Resource left the library "*WEB* is not in the library" (#17 rule 13; the
abbreviation while the app remembers it, else the id), and for an Orphaned annotation its
warning.

- **Order** (`notes.order`): **Bible order** (the default): Passage anchors by Standard
  Passage; Text-range anchors in a Bible by their own reference (`native_ref`) among them;
  then Text ranges in other Resources by Resource and position; then Notes without an Anchor
  by creation. **Last modified**: newest change first.
- **Filter field** (the Tab header's): every word must appear in the body, quote, reference or
  Notebook name, ignoring case.
- **Reference labels**: `Genesis 30:1 (TB)` (Passage anchor with its origin Bible),
  `Leviticus 19:18` (Passage only), `Esth 10:4 (LXX)` (Unmapped verse), `MHC, GEN.1`
  (another Resource), `No anchor`.
- **Drawing**: the list is virtualized: only the cards in view are drawn and measured
  (`HeightIndex`, estimates until measured), inside an egui scroll area; with 300 Notes about
  ten cards are laid out a frame. The list reloads when the annotation revision, the Notebook,
  the order, the filter or the library (each Resource's id, abbreviation and revision) changes,
  so swapping one Resource for another updates labels and "not in the library". Quotes and bodies
  are drawn by the text pipeline (glyph atlas, inline-snippet layouts from the workers).
- **Note cards are edited in place**: a click on the body opens an editor there. It saves a
  moment (1 s) after the last keystroke, and when focus leaves it, on Esc or Ctrl+Enter, and on
  quitting. All saves of one editing pass are one undo step (`annotation.edit` with
  `continue: true` merges into the previous step when that step edited this same Note's text).
  Every save moves `annotations_revision`, so the marker preview in the text shows the new
  words at once. The Shell checks the autosave after every frame (`notes_after_frame`), not
  the Notes Tab, and in any frame the Notes Tab is not drawn (Search or another Tab opened over
  it, the Tab closed) it ends the editing pass and saves at once: a hidden editor never loses
  focus, so it would otherwise stay unsaved, and unsearchable, until Notes was shown again.
- **While a card is edited the list holds still**: refreshes keep the cards' order and members
  (fresh contents; Annotations deleted meanwhile drop out, new ones join at the end). Otherwise
  each save would move the card to the top in Last modified order, or out of the filter, the
  scroll offset would no longer show it, and egui would take the keys from the unseen editor.
  The list re-sorts when editing ends.
- **Bible references in a body** are found when shown (`notes::body_references`, Standard
  versification per ADR 0002), once per (Note, body) and cached in the Tab's UI state: a
  capitalized book name or abbreviation and chapter:verse, optionally continuing as a list
  (`Jn 3:16-18; Rom 5:8`, `1 Cor 13:4, 7`). They are drawn in the link colour, underlined;
  hovering one shows the reference popup (#101), clicking it runs `notes.show_passage`
  (`passage`, `bible`?, `split`?), which shows it like a Passage anchor (Ctrl/Cmd: a new Tab).
- **False links, the trade-off**: Note prose is not edited text with known reference
  spellings, so matching is conservative. A bare chapter (`Romans 8`, "Job 5 times") stays
  text, since book names double as words; so does a lowercase word ("I am 3:15"). English
  words that are also book spellings (`Am`, `Is`, `So`, `Re`, `Ex`, `He`, `Ho`, `Ac`, `Act`,
  `Ob`) name a book only with a period (`Is. 53:5`): "Is 2:30 OK?" and "So 3:15 then" are
  times. A list stops before such a word ("Rom 5:8; So 3 of us" links Rom 5:8). The cost:
  `Is 53:5` without a period is not a link; the app's own abbreviations (`Isa`, `Song`, `Rev`,
  `Amos`) always are. Names that are also books (`Mark 4:30`, `John 3:16`) still link.
- **Card menu** (… and right-click): Go to text, Go to text in a new Tab, Move to Notebook,
  Highlight style, Delete. All undoable.

## Following a card (#18 rule 10)

`notes.follow` (`id`, `split`): click a card's reference.

- **Passage anchor**: the most recently active Bible Tab, preferring the origin Bible when it
  is open, goes to the Passage (`navigate_standard`), and its Link set follows. With no Bible
  open, a new Tab of the origin Bible (else the ★ Bible) opens; `Effect::NavigateStandard`
  moves it once the UI has loaded its Verse map.
- **Text-range anchor**: the most recently active Tab of its Resource, else a new one;
  `Effect::RevealText` scrolls to the unit and byte and flashes the Verse there.
- **Ctrl/Cmd-click** always opens a new Tab. A Note without an Anchor, or whose Resource is
  gone, has no link.

## Opening a Note in Notes

`notes.open` (`id`?) opens or focuses the Notes Tab, switches the column to All if the
Annotation is in another Notebook, clears a filter that hides it, scrolls to its card and, for
a Note, starts editing it. It is run by a click on a Note marker in the text, by the Note
editor's **Open in Notes** (which saves the Note first) and by Notes search results.

## Notes search (Search Tool, Notes mode)

- **Query**: the #16 grammar, compiled by `compile_notes` against `notes_fts` (normalized
  words, the Search index's form). Errors show inline under the box.
- **Scope**: the Notebook filter (`search.notebook`: a Notebook id or `all`) and the optional
  Passage list of the Scope field. With a Passage list, only Notes whose Passage anchor
  overlaps it are found (Notes without one, and Text-range anchors, need the Whole Bible).
- **Results**: newest change first, one row per Note: its reference and Notebook, then a
  snippet (white space collapsed, cut to about 220 characters around the first hit) drawn by
  the pipeline with the hits marked. Highlights have no text and are not searched.
- **Opening a hit**: click (or Enter) opens the Note in the Notes Tool; Ctrl/Cmd-click goes to
  its text in a new Tab.
- **Freshness**: the search runs synchronously against the User database, whose `notes_fts`
  row is written in the same transaction as each edit, and cards save as the user types; so a
  word typed seconds earlier is found.
