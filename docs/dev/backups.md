# Back up, restore and export all user data (#217)

Everything the user makes or chooses is in the User database (#17), so a **Backup** is a copy of
it and a restore puts one back. ADR 0013 records why. **Tools → Back up & restore…**
(`data.backups`), or Backups in Settings → Privacy and data (#213), opens a keyless,
single-instance Tab with all of it.

| Layer | Piece | Role |
|---|---|---|
| `obs-store` | `backup.rs`, `backup_store.rs` | `UserStore::back_up`, `open_backup`, `backup_contents`, `restore`, `refuse_own_file`, `backups`, `contents`, `preferences`, `local_time`; `BackupKind`, `BackupFile`, `BackupContents`. |
| `obs-engine` | `backups.rs`, `backup_check.rs` | `UserData`'s backup methods; the check of what the engine keeps inside rows (`unreadable_records`, `JSON_SETTINGS`); the restore preview (`restore_lines`); reloading the Session after a restore; Undo restore; Export everything; commands `data.back_up`, `data.restore`, `data.undo_restore`, `data.export`. |
| `obs-ui-egui` | `shell/backups_tab.rs` | The Tab: native save/open dialogs on their own thread, the preview panel, the kept Backups list. |

## The Backup file

- `UserStore::back_up(dest)` copies the database with SQLite's online backup API into
  a new scratch file beside `dest` (`.<name>.<pid>-<n>.partial`, created with `create_new` and
  never one of the database's own files, so a file already there is never removed), switches
  the copy to a rollback journal (so it is one file, with no `-wal` or `-shm`) and renames it
  over `dest`. A failed write leaves no half file.
- Its format version is the database's own: `PRAGMA application_id` (`OBSU`) says it is a
  User database, `PRAGMA user_version` which schema it has. There is no other header.
- `back_up` (and Export everything) first calls `UserStore::refuse_own_file(dest)`, which
  refuses the live database and its `-wal`, `-shm` and `-journal` files by any path naming
  them: `.`/`..`, symlinks and hard links (the `same-file` crate: device and inode on Unix, volume
  and file index on Windows), and another case on Windows and macOS, whose disks ignore case by default. SQLite names its
  files beside the database's real file, so for a database opened through a symlink the
  sidecars beside the link's target (from SQLite's own filename for the connection and the
  canonical path) are refused as well as those beside the link. Writing there would destroy
  the data being saved.
- The launch backups (`backups/user-000006.db`) and the before-upgrade copy
  (`user.v8-<time>.bak`) are written the same way, so they are Backups too.

## Restore

`UserStore::restore(path)`:

1. `open_backup` reads the file read-only into an in-memory database, refuses a file that is not
   a User database (wrong `application_id`, or none) or is newer than this build
   (`StoreError::NewerFormat`), and runs the same forward-only migrations a launch runs. It then
   checks the result is whole, so a Backup the app could not use is refused here rather than
   failing after it replaced the user's data: SQLite's `integrity_check` passes; no row points
   at one that is not there (`foreign_key_check`, which the integrity check skips: an
   Annotation of a Notebook the file lacks would belong to none, #258); every table,
   column, index and trigger of a database freshly migrated to this build's version is there;
   every row of every table can be read; the Notes search index passes its own check; and
   every read the app makes of a User database (Notebooks, Annotations with their Anchors,
   Layouts, Highlight palettes and styles with their colours, Favorites, History, Reading
   plans, preferences...) succeeds. It makes the default
   Notebook if there is none. Nothing has been written yet.
2. A **Safety backup** of the live database goes to `backups/user-safety-000001.db` (the last
   `SAFETY_BACKUPS` = 5 are kept; their numbering is separate from the launch backups').
3. The migrated copy's pages are copied over the live connection with the backup API, in one
   step. The live database stays in WAL mode. Pages copy only between databases of one page
   size: a copy with another is first rebuilt in a new scratch file
   `backups/.user-restoring.<pid>-<n>.db` with the live
   page size (`VACUUM`), then deleted, however the restore ends (a `ScratchFile` guard).

Before any of it, `Session::restore_backup(path)` checks what the engine keeps inside rows,
which the store cannot read (`backup_check.rs`, #258), on the Backup read into memory
(`UserData::open_backup`): every Layout's Workspace snapshot and every Favorite's and Visit's
location must read, and so must every setting the app keeps as JSON (`JSON_SETTINGS`: View
settings, Custom theme, toolbar, keys, Update check settings, First-run tour, Home, Guide arrangements, Saved searches, Search history, recent
searches and Command box entries, Copy verses, Collections, Resource tags, Visual filters,
the Starter library state, Import reports, kept Attributions), each as the type its loader
reads; a Layout must not hold two Tabs with one id (restoring it would keep only one of them).
Settings the loader keeps only part of are checked as it reads them: a field of View,
Toolbar, Update check settings or the First-run tour must read; a View Text size must not be above the maximum nor
a Line spacing outside 1.0–2.0, which the loader would clamp (a Text size below the minimum is
read: the app resets it with a notice, #241); a Search history must not hold more than
`SEARCH_HISTORY_LIMIT` entries, which the loader would cut; a Home arrangement or Toolbar must hold each card or
Launcher this version knows once, a Guide arrangement each section of its Guide once (filtered to
a Collection only where it can be); an Import report must be the report of the Resource its key
names. The app reads each of those leniently at launch (an unreadable one left out or at its
default, a repeated or unknown card or section dropped), so without this a restore would drop or default them silently once the live data is
gone. A Backup with any of them is refused, naming each ("the Layout “Broken”", "the setting
`search.saved` (Saved searches)"), with nothing changed. Plain-text settings (Log level, the
last Notebook used...) are not checked: any text is one. A test
(`every_setting_the_engine_keeps_is_known_to_the_restore_check`) fails when a new `*_PREF`
constant is in neither `JSON_SETTINGS` nor its plain-text list.

`Session::restore_backup(path)` then sets the preferences `attach_user` only overwrites when the
database has them (View settings and fonts, toolbar, Guide arrangements, Copy verses, Import
reports) back to their defaults (`forget_user_preferences`) and calls `attach_user` again, so a
preference the Backup lacks ends at its default, as a launch on the Backup would, and everything the Session keeps
of the User database reloads as at launch (View settings, toolbar, Preferred order, Favorites,
History, Reading plans, Home, Copy presets, Collections, Saved searches, keys, the Starter
library state) and `annotations_revision` moves, so reading panes reload their Marks. The
annotation undo history is cleared (its rows no longer exist). The Workspace on screen is kept,
and the Last session cache is forgotten so the next save writes it over the Backup's. A Notes
Tool showing a Notebook the Backup lacks goes back to All. Search Tabs whose results come from
the User database (Notes mode, and Library mode, whose Search scope names Collections) drop
their results and run their searches again over the restored data (`rerun_searches_of_user_data`,
as a Tab restored from a Workspace snapshot does: no Visit, no Search history entry), keeping
their query.

A restore that fails before step 2 changes nothing at all: the Session keeps its User database,
undo history included, and no Safety backup is taken.

**Undo restore** (`data.undo_restore`, enabled after a restore in this run) restores the last
restore's Safety backup, taking a Safety backup of its own. It skips the check of what the
engine keeps inside rows: the Safety backup is the data as it was in use, and is put back as
it was.

Before the Tab runs either, it saves the Note being written and the Notes card being edited
(`settle_note_editors`, as closing the window does), so text typed before the restore is in
the Safety backup and no later autosave or retry writes it over the restored Note. A save the
database refuses holds the restore back: its card is shown with why, and the preview stays
open. After a restore, the Notes Tab forgets what it was in the middle of (a Notebook name
being typed, Delete Notebook's question, a Re-attach).

## The preview

`Session::restore_preview(path)` runs the same check as `restore_backup` (a Backup it refuses
shows why instead of a preview), then compares `UserData::contents()` (now) with the contents
of `open_backup(path)` (the migrated copy) as `RestoreLine`s: Notebooks, Notes, Highlights,
Layouts, Reading plans, Favorites, History visits and Settings (preference rows), each with
both counts, and for the named ones what comes back and what goes ("brings back “Romans”").
`older_format` says the Backup is upgraded as it is restored. Previewing only reads the file.

## Export everything

`Session::export_everything(dest)` writes a zip (deflate, the `zip` crate obs-formats already
uses):

| File | What |
|---|---|
| `manifest.json` | App and version, `export_version` (`DATA_EXPORT_VERSION`), the User database version, when. |
| `README.md` | What each file is, and that the app does not read the zip back. |
| `notes/<Notebook>.md` | Each Notebook's Notes export (Markdown, Bible order, Attributions). |
| `annotations.json` | Every Notebook, Highlight palette and style (#181) and Annotation with its Anchor, as `obs_store` serializes them, and `resource_names`: the remembered abbreviation and title of every Resource seen (`resource_id`, `abbreviation`, `title`), so Annotations on a since-removed Resource still name it. |
| `layouts.json`, `last-session.json` | Layouts by name with their Workspace snapshots; the Last session. |
| `favorites.json`, `history.json`, `reading-plans.json` | With their locations as JSON. |
| `preferred-order.json`, `settings.json` | Resource ids best first; every preference, JSON values parsed. |

## Tests

- `crates/store/tests/user_backup.rs`: one-file Backups; restore brings back a deleted
  Notebook and its Safety backup undoes it; an older Backup is migrated; newer, foreign and
  missing files are refused with no Safety backup; a Backup missing a table, index or column,
  with an unreadable row or with an Annotation of a missing Notebook is refused by preview and restore with nothing changed; Back up now
  refuses the database and its sidecar files by any path; the kept list; another page size;
  no scratch file left by a restore that fails.
- `crates/engine/src/backups_tests.rs`: the #217 acceptance through commands (back up, delete a
  Notebook, preview, restore, Undo restore, restart); reloading settings and Layouts and
  writing the Workspace after a restore; every preference the Backup lacks back at its
  default (compared with a Session launched on the Backup), font choices included; Back up now
  and Export everything refusing the User database; a refused restore keeping undo; Export
  everything's files, remembered Resource names of a removed Resource included; a Backup with
  an unreadable Layout, Favorite, Visit or JSON setting refused naming each, a Layout with two
  Tabs of one id refused naming it, and one whose
  View values the loader would clamp or whose Search history it would cut refused too, while one of
  everything the app writes passes; every `*_PREF` constant known to the check; open Notes
  searches run again after a restore and after Undo restore.
- `crates/store/tests/user_db.rs` (`a_refused_database_is_not_changed`): a newer or foreign
  database is refused at launch byte for byte unchanged (its journal mode is read, not set to
  WAL, until it is accepted).
- `crates/ui-egui/src/backups_tests.rs`: the Tab from the Tools menu, with injected dialogs:
  Back up now, preview, Restore, Undo restore, a kept Backup's Restore…, Export everything and
  a cancelled dialog. `UPDATE_PREVIEWS=1` writes `backups-*.ppm` to `target/previews/`.
- `crates/ui-egui/src/notes_tests.rs`
  (`a_card_being_edited_is_saved_before_a_restore_and_never_written_over_it`): a card whose
  save is refused holds back Restore and Undo restore; once saved, its words are in the
  Safety backup and no retry writes them over the restored Note.
