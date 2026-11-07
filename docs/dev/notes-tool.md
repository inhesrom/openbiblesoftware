# The Notes Tool and Notes search (#64)

The Notes Tool is a single-instance Tab (`tool.notes`, Tools → Notes) for browsing, editing and
managing every Annotation (#18 rule 7). The Search Tool's Notes mode searches Note bodies
(#16 rules 10 and 13). Both build on the annotation service and `notes_fts` described in
[annotations.md](annotations.md).

| Layer | Piece | Role |
|---|---|---|
| `obs-search` | `compile_notes` | The #16 query grammar compiled for `notes_fts` (one `body` column). A Strong's number or Lemma matches itself as written in a Note. |
| `obs-formats` | `scripture_references` | Bible references in typed text, with byte ranges (Note bodies). English words that are also book spellings need a period. |
| `obs-model` | `parse_note_text`, `escape_note_text`, `note_plain_text` | Note formatting (#186): a Note's CommonMark as shown, line by line, with its marks; plain text escaped to show as typed. |
| `obs-layout` | `styled_snippet_paragraph`, `formatted_snippet_paragraph` | An inline snippet with some byte ranges in their own colour role (references as links), or bold, italic and at a heading's scale. |
| `obs-engine` | `notes.rs` | Cards, following anchors, Show in text, Notes search; commands `notes.notebook`, `notes.order`, `notes.open`, `notes.follow`, `notes.show_passage`, `search.notebook`. |
| `obs-ui-egui` | `shell/notes_tab.rs`, `shell/note_editor.rs`, `shell/search_tab.rs` | The Notebook column, the virtualized card list, the Note editor, the delete dialog, and the Notes results. |

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
abbreviation the User database last knew, also after a restart), and for an Orphaned
annotation its warning.

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

## Note formatting (#186)

A Note's text is CommonMark, stored as typed in `annotations.body`
([ADR 0010](../adr/0010-note-text-is-commonmark.md)). `obs_model::parse_note_text` reads it
with pulldown-cmark (no extensions) and gives each line of the text what it shows; everything
else goes through it.

| Typed | Shown |
|---|---|
| `# `, `## `, `### ` (to `###### `), or a setext underline | A heading: bold, larger (×1.35, ×1.2, ×1.08), in the heading colour |
| `- `, `* ` or `+ ` | A bulleted item (•, then ○ and – for nested items) |
| `1. ` or `1) ` | A numbered item, numbered as CommonMark numbers it (on from the list's first number) |
| the marker under an item's text (two columns under `- `, three under `1. `) | One level deeper (18 px) |
| `>` or `> ` | A block-quote line, beside a rule |
| `**bold**`, `__bold__` | Bold (the reading font's bold face) |
| `*italic*`, `_italic_` | Italic; CommonMark's flanking rules, so `snake_case` and `2 * 3` stay as typed |
| `[text](<Rom 5:8>)`, `[text](<Rom 5>)` | A link to the Passage: hover previews it, click shows it, as for a reference |
| `[text](https://…)`, `<https://…>` | A link to the web address: hover shows it, click opens the browser |
| `\` before ASCII punctuation, `&amp;` | The character itself |
| `` `code` ``, code blocks, raw HTML | Their text, as typed (HTML is never run) |

Every line break is a line break where the Note is shown (a soft line break inside a paragraph
is drawn as one), and each blank line is a gap (`0.8 ×` the text size), as for plain Notes
before. Bold or a link that runs over a line break is marked on each line. A line that
continues a list item's text is drawn as text. Bible references in a line's shown text are
links wherever they are (bold, in a list item, in a heading), except inside a link, whose
target decides where it goes (`notes::note_body`). Each line is laid out by the pipeline with
`obs_layout::formatted_snippet_paragraph` (bold, italic and link-colour byte ranges, which may
overlap, at a heading's scale); a plain line is laid out exactly as before (`a_plain_formatted_snippet_is_the_styled_snippet_it_replaces`).

- **Old Notes**: User database migration 9 → 10 escapes each Note written before that would
  not show exactly as typed: on each line, what CommonMark would read as markup gets a `\`
  (`# not a heading` becomes `\# not a heading`) and leading or trailing spaces become `&#32;`,
  so every old Note shows exactly as it did; Notes that read as plain text are untouched.
- **Search and the filter field** read the text a Note shows (`note_plain_text`): `notes_fts`
  indexes it, so `**grace** alone` is found by the phrase `"grace alone"`, and link targets are
  not words of the Note. Notes search snippets show no markup.
- **The marker preview** in the text shows the lines as shown, a heading strong, items after
  their bullet or number.

### The Note editor (`shell/note_editor.rs`)

Used by the cards and by the popup Note editor. Above the field a toolbar: **B**, *I*, **H**
(cycles `#`, `##`, `###`, text), •, 1., “” and 🔗 (inserts `[selection](<>)` with the cursor
between `<` and `>`, so a Bible reference's spaces are part of the target; off, with a hover
saying why, while the selection spans lines). Ctrl+B and Ctrl+I (Cmd on macOS) bold and
italicise the selection, or take it off when it is already so (written with `*` or `_`):
the toolbar never scans for markers itself, it takes off the delimiters of the span
pulldown-cmark found (`note_spans`), and Enter continues the list item or quote the
parse found. Each selected line gets its own
marks, after its heading, list or quote marker, empty lines skipped (a span cannot run over a
blank line or a list item's marker); bold or italic already running over those lines is cut
into one span per line first. A code span, link, image or autolink is opaque: one the
selection takes in whole is copied as typed, but while the selection (or the cursor) is partly
in one every button is off ("Selection partly covers a code span or link"), and while a change
would cut one at a line break (a mark on each line, a list item or quote of its own) that
button is off; Ctrl+B and Ctrl+I then leave the text as it is, byte for byte. A code span over
lines shows its whole text (line breaks read as spaces) on the line it starts on: no position
inside a code span is worked out. Typing `- `, `1. ` or `> `
at a line's start starts a list or quote; Enter at the end of an item continues it (with the
next number) and Enter in an empty item ends it. List, quote and heading changes apply to
every line the selection touches, each read in its place in the Note's parse: numbering goes
per nesting level (on from a numbered item just above), a nested item is written under its
parent's text (re-indented when the parent's marker changes width), and the nested items of a
line that stops being an item move out a level rather than turn into text.

The field shows the Markdown as typed, coloured: headings in the display face and larger,
bold text strong, italics slanted, markup dimmed, links and Bible references in the link colour
and underlined. Each toolbar or shortcut change is one step of the field's own undo (Ctrl+Z,
Ctrl+Shift+Z), and pressing the toolbar does not end the card's editing pass (the field gets
the keys back). The saves of an editing pass are still one `annotation.edit` undo step.

Every save (a card's editing pass, the popup Note editor, undo and redo) stores the field's
text byte for byte: white space is CommonMark (four spaces start a code block), so nothing is
trimmed or normalized; only a Note that is all white space is not made.

Exports keep the formatting: the Markdown is the text as typed, the HTML pulldown-cmark's;
see [copy-export.md](copy-export.md) and `notes_export.rs` (`note_markdown`, `note_html`).

## Following a card (#18 rule 10)

`notes.follow` (`id`, `split`): click a card's reference.

- **Passage anchor**: the most recently active Bible Tab, preferring the origin Bible when it
  is open, goes to the Passage (`navigate_standard`), and its Link set follows. With no Bible
  open, a new Tab of the origin Bible (else the ★ Bible) opens; `Effect::NavigateStandard`
  moves it once the UI has loaded its Verse map.
- **Text-range anchor**: the most recently active Tab of its Resource, else a new one;
  `Effect::RevealText` scrolls to the unit and byte and flashes the Verse there.
- **Ctrl/Cmd-click** always opens a new Tab. A Note without an Anchor, one whose Resource is
  gone, and an Orphaned annotation (its offsets point at whatever is there now: it is
  re-attached instead) have no link; `notes.follow` on them fails with "*TB* is not in the
  library" or "This text was not found in *TB* after it changed: re-attach it".

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
