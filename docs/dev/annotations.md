# Annotation service, storage and undo (#61)

Notebooks, Notes and Highlights live in the User database (`obs_store::UserStore`, schema
v5). The engine reaches them only through the `UserData` trait; `crates/engine/src/
annotations.rs` holds the service on `Session` and the `annotation.*`, `notebook.*` and
`edit.*` commands. The origin Bible's UI is #62 and other Bibles #63 (both below); the Notes
Tool (#64) and re-anchoring (#65) are still to come.

## Tables

| Table | Holds |
|---|---|
| `notebooks` | `id` (UUIDv7), `name`, `is_default` (exactly one row, partial unique index), timestamps (ms), `show_in_text` (schema v5) |
| `annotations` | `id`, `notebook_id` (deferred FK), `kind` `note`/`highlight`, `style` (Highlights), `body` (Notes), timestamps |
| `anchors` | at most one per Annotation. Passage anchors: Standard start/end as canonical book number (1–66), chapter, verse. Text range (a Text-range anchor, or a Passage anchor's origin): `resource_id`, `unit_id`, `start_byte`, `end_byte`, `quote`, `prefix`, `suffix`, `content_hash`, `native_ref`, `whole_verses` (schema v5) |
| `notes_fts` | FTS5 over Note bodies in normalized-word form (the Search index's form), keyed by `annotation_id` |

A CHECK makes a stored Text range without its quote, context or content hash impossible
(ADR 0006). The migration and every launch make the default Notebook if it is missing.

## Every edit is a list of row Changes

`obs_store::Change { before, after }` describes one row (`Row::Notebook` or
`Row::Annotation`, the latter with its Anchor). `UserStore::apply_changes` writes a list in
one transaction: it checks each row is still `before` (else `StoreError::Conflict`, nothing
written), writes `after`, and rewrites the row's `notes_fts` entry when the Note text
changed. The default Notebook cannot be deleted or demoted (`StoreError::Rejected`).

Each command builds its list, applies it and pushes it on `Session`'s `UndoHistory` as one
step. Undo applies `inverse(changes)` (each Change swapped, in reverse order); redo applies
the list again. Because rows carry their timestamps, both land on byte-identical rows. If a
step no longer matches the database (something outside the history changed it), nothing is
written and the history is cleared. `Session::annotations_revision()` changes with every
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

## Commands

| Command | Params |
|---|---|
| `annotation.highlight` | `resource`, `start_unit`, `start`, `end_unit`?, `end`, `style`? (`yellow` default, `green`, `blue`, `pink`, `red_underline`), `notebook`? |
| `edit.undo` / `edit.redo` | Ctrl+Z / Ctrl+Shift+Z; enabled only when there is something to undo or redo |
| `annotation.note` | `body`?, then a selection (as above), or `passage` (one Standard Passage), or neither (no Anchor); `notebook`? |
| `annotation.edit` / `annotation.style` / `annotation.move` / `annotation.delete` | `id` and `body` / `style` / `notebook` |
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
  toolbar above it: 5 style swatches, ✏ Note, Copy and Search this (disabled until #59).
  Right-click shows the same actions as a menu (on the selection under the pointer, else on
  the clicked word). Keys: Ctrl+1…5 (styles, in `HighlightStyle::ALL` order), Ctrl+Shift+N
  (Note), Ctrl+Z / Ctrl+Shift+Z (undo, redo). A Highlight clears the selection so its colour
  shows at once.
- **Keys and text fields.** The Shell runs command shortcuts before widgets see the key, so
  while a text field has focus (`egui::Context::text_edit_focused`) it skips the keys text
  fields use themselves (Ctrl+Z, Ctrl+Shift+Z, Esc, Alt+←/→) and the annotation keys.
  Like egui, a shortcut still matches with an extra Shift or Alt held, because some layouts
  need them for the key (Ctrl+Plus is Ctrl+Shift+= on a US keyboard; digits need Shift on
  AZERTY). The exception is a key bound more than once with different modifiers: then Shift
  and Alt must match exactly (`shell::extra_held`, `shell::consume_key`). So Ctrl+Shift+Z
  never undoes, even when there is nothing to redo. The style keys Ctrl+1…5 accept an
  extra Shift but not Alt, because Ctrl+Alt is AltGr on Windows.
- **The Note editor** is a popup anchored under the selection, with a Notebook picker that
  starts on the last used Notebook. It saves (as `annotation.note` with the picked
  `notebook`) on a click outside it, Esc, Ctrl+Enter or quitting; an empty Note is not
  saved; Discard closes it without saving. If the save fails, the editor stays open with its
  text and the error. Starting another Note does not replace it, and closing the window is
  cancelled until the Note is saved or discarded. Open in Notes is disabled until #64.
- **Drawing them.** `MarkCache` reloads the Annotations when `Session::annotations_revision`
  moves and turns those whose Text range is in a Resource into `Mark`s over document
  positions. A range whose text no longer reads as its quote is not drawn over other words:
  a Passage anchor falls back to its whole Verse(s) in this Bible (ADR 0006); a bare Text
  range is skipped until #65 re-anchors it. Reloading happens only when the revision changes,
  synchronously on the UI thread (fine at today's scale; large Notebooks may want the job
  queue or incremental updates later). The pane computes their rectangles from the laid-out clusters each frame
  (`selection::range_rects`, one pass for all Marks), so they follow scrolling, resizing and
  text-size changes like the selection. Backgrounds go under the glyph mesh in creation order
  (the newest on top); the red underline and Note markers go over it. Colours come from the
  Page tone's palette when painting (#21), so a theme switch never re-lays anything out.
- **Note markers** sit just past the end of the Note's range, raised like a superscript;
  hovering one previews the Note and its Notebook. Clicking it will open the Notes Tool
  (#64).

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
  margin mark never implies that words are marked. When re-anchoring (#65) drops a Text
  range, the flag goes with it and the Highlight shows as whole Verses everywhere, as
  ADR 0006 says.
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
  Tool (#64) will show the same toggle per Notebook. Toggling either moves
  `annotations_revision`, so `MarkCache` reloads.
- **Colours** come from the Page tone's palette: each tone has a margin-mark colour per style
  (`Palette::marks`), at least 3:1 against its page (tested with the other palette pairs).
- Text Comparison does not draw Annotations in v0.1; its rows carry the diff tints.
