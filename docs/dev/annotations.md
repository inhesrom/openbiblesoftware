# Annotation service, storage and undo (#61)

Notebooks, Notes and Highlights live in the User database (`obs_store::UserStore`, schema
v10). The engine reaches them only through the `UserData` trait; `crates/engine/src/
annotations.rs` holds the service on `Session` and the `annotation.*`, `notebook.*` and
`edit.*` commands. The origin Bible's UI is #62 and other Bibles #63 (both below); the Notes
Tool and Notes search are #64 ([notes-tool.md](notes-tool.md)); re-anchoring and Orphaned
annotations are #65 (below, `crates/engine/src/reanchor.rs`).

## Tables

| Table | Holds |
|---|---|
| `notebooks` | `id` (UUIDv7), `name`, `is_default` (exactly one row, partial unique index), timestamps (ms), `show_in_text` (schema v5) |
| `annotations` | `id`, `notebook_id` (deferred FK), `kind` `note`/`highlight`, `style` (Highlights), `body` (Notes), timestamps |
| `anchors` | at most one per Annotation. Passage anchors: Standard start/end as canonical book number (1–66), chapter, verse. Text range (a Text-range anchor, or a Passage anchor's origin): `resource_id`, `unit_id`, `start_byte`, `end_byte`, `quote`, `prefix`, `suffix`, `content_hash`, `native_ref`, `whole_verses` (schema v5), `status` `ok`/`orphaned` (schema v6: the quote was not found after the Resource changed; on a Text-range anchor an Orphaned annotation, on a Passage anchor's origin a range kept while the Annotation shows as whole Verses) |
| `resource_names` | schema v6: `resource_id`, last known `abbreviation` and `title` of every Resource seen, so cards of a Resource no longer in the library still name it |
| `notes_fts` | FTS5 over Note bodies in normalized-word form (the Search index's form), keyed by `annotation_id` |
| `highlight_palettes`, `highlight_styles` | schema v10 (#181): the user's Highlight palettes and styles; see [below](#highlight-styles-and-palettes-181-adr-0012) |

A CHECK makes a stored Text range without its quote, context or content hash impossible
(ADR 0006), and since schema v9 (#115) one that covers no text too: `end_byte > start_byte`
and a non-empty quote. `Annotation::problem` refuses such a range first, as
`StoreError::Rejected`. The v9 migration rebuilds `anchors` for it (SQLite cannot change a
CHECK in place). The app never wrote such a range, but should a row exist the migration
refuses before changing anything (`refuse_text_ranges_covering_no_text`): dropping or
repairing the row would lose its quote, context and offsets, and nothing is deleted
automatically (ADR 0006). The open fails with `StoreError::Rejected` naming the Annotations,
the database stays at schema 8, and the app reports it as a problem at startup and runs
without the User database until the rows are fixed by hand. The migration and every launch
make the default Notebook if it is missing.

`notes_fts.annotation_id` is UNINDEXED, so finding one entry scans the index. Deleting an
Annotation therefore touches `notes_fts` only when it is a Note: deleting a Notebook full
of Highlights costs no index scans, whatever the number of Notes.

## Every edit is a list of row Changes

`obs_store::Change { before, after }` describes one row (`Row::Notebook` or
`Row::Annotation`, the latter with its Anchor). `UserStore::apply_changes` writes a list in
one transaction: it checks each row is still `before` (else `StoreError::Conflict`, nothing
written), writes `after`, and rewrites the row's `notes_fts` entry when the Note text
changed. The default Notebook cannot be deleted or demoted (`StoreError::Rejected`).

Each command builds its list, applies it and pushes it on `Session`'s `UndoHistory` as one
step. Undo applies `inverse(changes)` (each Change swapped, in reverse order); redo applies
the list again. Because rows carry their timestamps, both land on byte-identical rows. If a
step no longer matches the database (something outside the history changed it:
`StoreError::Conflict`) or breaks a rule (`StoreError::Rejected`), nothing is written and
the history is cleared. Any other failure (SQLite busy, disk full) may pass, so the step
goes back on its stack to try again (#115). `Session::annotations_revision()` changes with every
write, undo or redo, for views to reload.

## Selection → Anchor

`annotations::segments` splits a selection `(resource, unit, byte) … (unit, byte)` into one
part per text unit (footnote bodies between the ends are skipped; whitespace at each part's
ends is trimmed), capturing quote, ±32 characters of context within the unit, and the
Resource's content hash. In a Bible, each part also gets the Standard Passage that the
mapped Verses it touches span (`ResourceStore::verses_overlapping` reads the Verse map rows
for just those Verses).

- Bible part with mapped Verses → Passage anchor + origin Text range.
- Bible part touching only Unmapped verses → Text-range anchor with `native_ref`
  (e.g. `Esth.10.4`); text outside any Verse → Text-range anchor without one.
- Non-Bible Resource → Text-range anchor.
- Highlight over several units (Bible books) → one Highlight per unit, one undo step.
- Note over several units → one Passage anchor over the whole selection with no Text range
  (or the first part's Text range when no part maps).

**Mixed selections.** One unit's part can touch both mapped and Unmapped verses (in the
fixture LXX, from Esth 10:3 into the Greek addition 10:4). It gets a Passage anchor over the
Standard Passage of just the mapped Verses, and its origin Text range covers the whole part,
Unmapped words included; `native_ref` is not set, because the Passage says where it is. So
the origin Bible marks every selected word, and other Bibles show only the mapped Verses
(the Unmapped ones have no counterpart there). `whole_verses` is worked out over every
Verse the range touches, so a mixed part that leaves part of the Unmapped verse out shows
as a margin mark elsewhere.

**Non-Bible Notes across units** keep only the first unit's Text range: a Commentary
section is its own unit, and a Note dragged across two sections anchors to the part in the
first. Revisited after #63 and #65 (#115) and kept: a Text range lives in one unit by design
(re-anchoring matches a quote within its unit; marks are drawn per unit; the `unit_alias`
table renames one unit at a time), so a range across units would need a new anchor form,
which is a schema and ADR 0006 change not worth it for this case. A Highlight across units
already makes one Highlight per unit; a Note to one place is what the Notes Tool lists.

## Commands

| Command | Params |
|---|---|
| `annotation.highlight` | `resource`, `start_unit`, `start`, `end_unit`?, `end`, `style`? (a Highlight style id; default: the first style of the active palette), `notebook`? |
| `edit.undo` / `edit.redo` | Ctrl+Z / Ctrl+Shift+Z; enabled only when there is something to undo or redo |
| `annotation.note` | `body`?, then a selection (as above), or `passage` (one Standard Passage), or neither (no Anchor); `notebook`? |
| `annotation.edit` / `annotation.style` / `annotation.move` / `annotation.delete` | `id` and `body` / `style` / `notebook` |
| `annotation.reattach` | `id`, then a selection (as above): **Re-attach here**; anchors it as a new Annotation on that selection would be, and it is no longer orphaned. A Highlight needs a selection within one unit |
| `annotation.detach` | `id`: **Keep without anchor** (Notes only) |
| `annotation.reanchor` | `resource`?: check now (default: every Resource) and queue re-anchoring where anchors are stale |
| `annotation.get`, `annotation.list` (`notebook`?), `annotation.search_notes` (`text`) | read only |
| `notebook.create` (`name`), `notebook.rename` (`id`, `name`), `notebook.delete` (`id`, `annotations`: `move` default or `delete`), `notebook.list` | |
| `notebook.show_in_text` | `id`, `show`? (default: the other way round); undoable like every Notebook change |
| `view.highlights` | View → Show Highlights, a toggle kept with the other View preferences |

`notebook`, where a command takes it, defaults to the **Notebook last used** (#18 rule 8):
giving one records it in the User database preference `annotations.last_notebook` once the
new Annotation is stored (a rejected one leaves it alone) (`Session::last_notebook()` reads it, falling back to the default Notebook when it is unset
or deleted).

## In the origin Bible (#62)

`crates/ui-egui/src/shell/annotate.rs` is the UI around a reading pane's selection;
`crates/ui-egui/src/marks.rs` and the reading pane (`pane.rs`) draw what is stored.

- **Making them.** Releasing a selection (drag, double- or triple-click) shows a floating
  toolbar above it: the active Highlight palette's swatches, `…` (Highlight styles…), ✏ Note,
  Copy and Search this. Right-click shows the same actions as a menu (on the selection under
  the pointer, else on the clicked word). Keys: Alt+1…9 (the active palette's styles in its
  order, #181; see "Highlight styles and palettes" below), Ctrl+Shift+N
  (Note), Ctrl+Z / Ctrl+Shift+Z (undo, redo). The style and Note keys are the Selection
  keys; these are their defaults, which Help → Keyboard shortcuts changes (#177,
  `keyboard-shortcuts.md`). A Highlight clears the selection so its colour shows at once.
- **Keys and text fields.** The Shell runs command shortcuts before widgets see the key, so
  while a text field has focus (`egui::Context::text_edit_focused`) it skips the keys text
  fields use themselves (Ctrl+Z, Ctrl+Shift+Z, Esc, Alt+←/→) and the annotation keys.
  Like egui, a shortcut still matches with an extra Shift or Alt held, because some layouts
  need them for the key (Ctrl+Plus is Ctrl+Shift+= on a US keyboard; digits need Shift on
  AZERTY). The exception is a key bound more than once with the same Ctrl (or Cmd) and
  different Shift or Alt: then Shift and Alt must match exactly (`shell::extra_held`,
  `shell::consume_key`). So Ctrl+Shift+Z never undoes, even when there is nothing to redo.
  Bindings that differ in Ctrl are told apart by Ctrl itself: Ctrl+1 goes to Panel 1 and
  Alt+1 highlights (#215). The style keys Alt+1…9 accept an extra Shift, and no key on
  Ctrl+digit (Ctrl+1…9, the Panels) accepts an extra Alt, because Ctrl+Alt is AltGr on
  Windows, which types { [ ] } on the digits of some layouts. The style keys were Ctrl+1…9
  until #215 gave those to the Panels.
  A Ctrl+Plus binding is also pressed as Ctrl+= (`shell::pressed_as`), so Larger text works
  without Shift; egui's own whole-window zoom keys are off, and the window zoom is the
  Interface size only (#137).
- **The Note editor** is a popup anchored under the selection, with a Notebook picker that
  starts on the last used Notebook. It saves (as `annotation.note` with the picked
  `notebook`) on a click outside it, Esc, Ctrl+Enter or quitting; an empty Note is not
  saved; Discard closes it without saving. If the save fails, the editor stays open with its
  text and the error. Starting another Note does not replace it, and closing the window is
  cancelled until the Note is saved or discarded. **Open in Notes** saves the Note and opens
  it in the Notes Tool (#64) to go on writing there.
- **Drawing them.** `MarkCache` reloads the Annotations when `Session::annotations_revision`
  moves and turns those whose Text range is in a Resource into `Mark`s over document
  positions. Orphaned annotations are not drawn. A range whose text no longer reads as its
  quote is never drawn over other words: a Passage anchor falls back to its whole Verse(s) in
  this Bible (ADR 0006); a bare Text range is skipped. Re-anchoring (below) normally fixes
  both before the Resource shows any Marks. Reloading happens only when the revision changes,
  synchronously on the UI thread (fine at today's scale; large Notebooks may want the job
  queue or incremental updates later). The pane computes their rectangles from the laid-out clusters each frame
  (`selection::range_rects`, one pass for all Marks), so they follow scrolling, resizing and
  text-size changes like the selection. Backgrounds go under the glyph mesh in creation order
  (the newest on top); the red underline and Note markers go over it. Colours come from the
  Page tone's palette when painting (#21), so a theme switch never re-lays anything out.
- **Note markers** sit just past the end of the Note's range, raised like a superscript;
  hovering one previews the Note and its Notebook. Clicking it opens the Note in the Notes
  Tool (#64).

## In other Bibles (#63)

A Passage anchor shows in every Bible that contains its Passage (ADR 0001); `marks_for` maps
the Standard Passage through each Bible's Verse map (ADR 0002). `ReadingDoc` builds, on
first use, an index of its Verses by Standard Passage (sorted by start, with a running
maximum of ends), so finding the Verses of a Passage is two binary searches. The index is
cached from `ReadingDoc::verse_map`, so that map is set only while the document is built;
changing it after the first lookup would leave the index stale.

- **Whole or partial.** When a selection becomes a Passage anchor, the engine records on its
  origin Text range whether it covers every Verse it touches whole (`whole_verses`; white
  space and punctuation at a Verse's ends, such as a KJV `¶` or the final full stop, do not
  count). Other Bibles then highlight those Verses, in runs of adjacent Verses so headings
  between them stay clear. A partial range shows only a **margin mark**: a thin bar in the
  style's colour, because word offsets differ between translations. A Passage anchor
  without a Text range counts as whole. Rows made before schema v5 read as partial: a
  margin mark never implies that words are marked. When re-anchoring (#65) moves a Text
  range, the flag (and the Bible's own reference, `native_ref`) is worked out again for
  where it is now. When it does not find one, the range is kept marked not found and the
  Highlight shows as whole Verses everywhere, as ADR 0006 says.
- **Several Verses, partly.** A partial Highlight over several Verses (from the middle of
  3:16 to the middle of 3:18) puts a margin mark beside **each** Verse it touches, 3:17
  included, not only the first: every Verse with marked words says so. Only one mark per
  style and Verse is drawn.
- **Where marks beside a verse number go.** In the text column's margin, on the line that
  holds the verse number: left of the column in left-to-right text, right of it in
  right-to-left text (the TAHOT), laid outward from the column in creation order. Not
  right beside the number: in a paragraphed Bible (WEB, BSB) most Verses start mid-line,
  where the space before the number is the previous Verse's last word. When two Verses
  start on one line, their marks share that line's margin. Marks never leave the pane or
  go under its scrollbar (they are clamped to the pane's own room).
- **Notes** from another Bible (or with no Text range at all) show one marker, in the same
  margin on the line of their first Verse's number, outside any margin marks there;
  hovering previews the Note as in the origin Bible.
- **Text-range anchors** (non-Bible Resources, Unmapped verses) show only in their own
  Resource.
- **What is drawn.** `Session::annotations_in_text` leaves out Annotations of Notebooks
  whose **Show in text** is off, and Highlights while View → Show Highlights is off. Both
  are in the User database (the Notebook row, and the `view` preference). The View menu has
  Show Highlights and a "Notebooks in text" submenu with each Notebook's toggle; the Notes
  Tool (#64) shows the same toggle (👁) per Notebook. Toggling either moves
  `annotations_revision`, so `MarkCache` reloads.
- **Colours** come from the Page tone's palette: `Palette::paint` gives each style its colour
  and margin mark there, the mark at least 3:1 against the page (see [Highlight styles and
  palettes](#highlight-styles-and-palettes-181-adr-0012)).
- Text Comparison does not draw Annotations in v0.1; its rows carry the diff tints.

## Re-anchoring and Orphaned annotations (#65)

ADR 0006 and #17 rules 9–13; the `unit_alias` table is #20 rule 2. `crates/engine/src/
reanchor.rs` holds the matching (pure, wasm-clean: `plan` over any `ResourceStore`) and the
background job; the Notes Tool's side is in [notes-tool.md](notes-tool.md).

- **When.** `attach_user` (library load) checks every library Resource; an Import that adds or
  replaces one (and a file dropped on the window) checks that one. A Resource needs it when
  one of its Text ranges was made against another content hash, or when an Orphaned
  annotation's own hash is the current one again (a revision reverted). Then a job reads the
  `.obr` on a worker and plans every Annotation with a Text range in it; Orphaned ones are
  tried again too.
- **Gate.** Until the plan is applied, `Session::is_reanchoring` is true and `MarkCache`
  returns no Marks for that Resource, so stale ranges are never drawn over other words.
  Applying it moves `annotations_revision`, so panes reload at once.
- **Matching**, per Text range, in order: the Resource's `unit_alias` (old unit id → new);
  the exact quote at the stored offsets; the exact quote elsewhere in the unit, the
  occurrence whose stored context (±32 characters) fits best, then the nearest to the old
  offset; then letters and digits only (white space and punctuation ignored), taking in the
  punctuation the quote had at its ends. Case is not ignored and there is no approximate
  matching (v0.1). A Passage anchor's origin is looked for only within the bytes of the
  Verses its Standard Passage maps to in the new revision.
- **Outcomes.** Found: offsets, quote, context and content hash are rewritten, and
  `whole_verses` and `native_ref` are worked out again for the new place as for a new
  selection (`Unchanged` if the offsets and unit are the same, else `Moved`). Not found: a
  Passage anchor keeps its origin with `status = 'orphaned'` (`WholeVerses`: it shows on
  whole Verses everywhere and is not listed under Needs attention; its card keeps the
  quote; the range is never lost, and is found again if a later revision, or the old one,
  has it); a Text-range anchor keeps everything it had and gets `status = 'orphaned'`
  (`Orphaned`). A
  `ReanchorReport` counts each; a toast reports moves, fall-backs and new orphans
  ("WEB changed: 2 Annotations moved with their text, 1 needs attention ...").
- **Writes.** One `apply_changes` list of row updates (never an insert or delete): a row
  changed while the job ran is a Conflict, and the Resource is planned again (up to 3 times).
  If the Resource was removed or replaced meanwhile, the plan is dropped (and redone for the
  replacement). Automatic re-anchoring is not an undo step; instead `UndoHistory::rebase`
  gives every recorded row of a moved Annotation its new anchor, so Ctrl+Z on an earlier edit
  still lands. Re-attach and Keep without anchor are ordinary undo steps. Undo or Redo that
  brings rows back (a deleted Annotation, a deleted Notebook's Annotations) re-anchors their
  Resources when those rows were made against another revision (`reanchor_written`), with
  the same gate.
- **Removed Resource.** Its Annotations are left as they are (dormant, not orphaned): Passage
  anchors still show in every other Bible, Text-range cards say "*WEB* is not in the library"
  (the abbreviation the User database's `resource_names` last knew, also after a restart;
  the id only for a Resource never seen with a User database). Importing it again runs the
  same re-anchoring.

## Highlight styles and palettes (#181, ADR 0012)

`crates/engine/src/highlight_styles.rs` keeps them; `crates/ui-egui/src/palette.rs` turns a
style into colours per Page tone; `crates/ui-egui/src/shell/highlight_styles_window.rs` is
View → Highlight styles….

- **Rows.** `highlight_palettes` and `highlight_styles` (User database schema 10) are rows of
  the annotation tables: every edit is row `Change`s through `UserStore::apply_changes`, one
  undo step each. Migration 9 → 10 seeds the Standard palette with the five v0.1 styles under
  their old ids (`yellow` … `red_underline`), with v0.1's hand-tuned Sepia and Dark colours as
  their tone adjustments, so every earlier Highlight keeps its look. `annotations.style` stays
  a free id; the store refuses a Highlight whose style does not exist and the deletion of a
  style a Highlight still uses.
- **Deleting** a used style (or a palette) needs a replacement style: its Highlights move
  there and the style goes in one step, so Ctrl+Z brings back both. Nothing is dropped.
- **Colours per tone.** A style stores one colour, its look and optional `paper`, `sepia` and
  `dark` adjustments. `Palette::paint` draws an adjustment that passes `Palette::check`, else
  derives one: a background fades toward the page until every ink reads on it at 4.5:1; an
  underline or box deepens toward the ink to 3:1 against the page, a text colour to 4.5:1
  against the page and every overlay. A background's margin mark (#63) is its colour deepened
  to 3:1. The window refuses an adjustment that fails, saying why.
- **Drawing.** `marks_for` carries each style's look in the `Mark`, and the pane draws a
  background under the glyphs, a text colour on the glyphs it covers, and a line or box over
  them. A text colour is checked against what is actually painted under each glyph (a
  background Highlight, a Find match, the selection): where two styles overlap and it does
  not read there at 4.5:1, `Palette::text_on` deepens it toward the ink just enough. Style
  edits move the annotation revision, so Marks are remade.
- **The window's fields** keep a draft only while the user changes it; an untouched field
  follows the stored style, so undo, redo and a new colour (which drops the tone
  adjustments) show at once, and leaving a field saves only what was typed.
- **The active palette** is the preference `highlights.palette` (not undone). The Selection
  keys `selection.highlight_1` … `_9` are places in it; keys saved under the v0.1 ids
  (`selection.highlight_yellow` …) apply to places 1–5.
- **Export / import.** `highlight_palette.export {id, path?}` writes
  `{"format": "openbible-highlight-palette", "version": 1, "name", "styles": [{"name", "look",
  "color", "paper"?, "sepia"?, "dark"?}]}` (no ids); `highlight_palette.import {path | text}`
  always makes a new palette ("Standard (2)" when the name is taken, the name shortened so
  the number fits the 60-character limit), as one undo step.

| Command | Params |
|---|---|
| `highlight_style.list` | `palette`? (default: the active one): its styles with labels and use counts |
| `highlight_style.create` | `palette`?, `name`?, `look`? (`background`, `underline`, `box`, `text_color`), `color` (`#rrggbb`) |
| `highlight_style.edit` | `id`, `name`?, `look`?, `color`? (drops the tone adjustments), `paper`? `sepia`? `dark`? (`#rrggbb`, or empty to derive) |
| `highlight_style.move` | `id`, `position` (0 first) |
| `highlight_style.delete` | `id`, `replacement`? (needed when Highlights use it) |
| `highlight_palette.create` / `.rename` / `.delete` | `name` / `id`, `name` / `id`, `replacement`? |
| `highlight_palette.activate` | `id` |
| `highlight_palette.export` / `.import` | `id`, `path`? / `path` or `text` |
| `view.highlight_styles` | opens the window |

