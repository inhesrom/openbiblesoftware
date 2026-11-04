# Annotation service, storage and undo (#61)

Notebooks, Notes and Highlights live in the User database (`obs_store::UserStore`, schema
v4). The engine reaches them only through the `UserData` trait; `crates/engine/src/
annotations.rs` holds the service on `Session` and the `annotation.*`, `notebook.*` and
`edit.*` commands. There is no UI yet (#62–#64) and no re-anchoring (#65).

## Tables

| Table | Holds |
|---|---|
| `notebooks` | `id` (UUIDv7), `name`, `is_default` (exactly one row, partial unique index), timestamps (ms) |
| `annotations` | `id`, `notebook_id` (deferred FK), `kind` `note`/`highlight`, `style` (Highlights), `body` (Notes), timestamps |
| `anchors` | at most one per Annotation. Passage anchors: Standard start/end as canonical book number (1–66), chapter, verse. Text range (a Text-range anchor, or a Passage anchor's origin): `resource_id`, `unit_id`, `start_byte`, `end_byte`, `quote`, `prefix`, `suffix`, `content_hash`, `native_ref` |
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
| `annotation.note` | `body`?, then a selection (as above), or `passage` (one Standard Passage), or neither (no Anchor); `notebook`? |
| `annotation.edit` / `annotation.style` / `annotation.move` / `annotation.delete` | `id` and `body` / `style` / `notebook` |
| `annotation.get`, `annotation.list` (`notebook`?), `annotation.search_notes` (`text`) | read only |
| `notebook.create` (`name`), `notebook.rename` (`id`, `name`), `notebook.delete` (`id`, `annotations`: `move` default or `delete`), `notebook.list` | |
| `edit.undo`, `edit.redo` | enabled only when there is something to undo or redo |

No default shortcuts yet: the Shell runs a shortcut's command before text fields see the
key, so #62 binds Ctrl+Z / Ctrl+Shift+Z / Ctrl+1…5 / Ctrl+Shift+N with focus in mind.
