# Back up, restore and export all user data (#217)

Everything the user makes or chooses is in the User database (#17), so a **Backup** is a copy of
it and a restore puts one back. ADR 0012 records why. **Tools → Back up & restore…**
(`data.backups`), or Backups in Settings → Privacy and data (#213), opens a keyless,
single-instance Tab with all of it.

| Layer | Piece | Role |
|---|---|---|
| `obs-store` | `backup.rs`, `backup_store.rs` | `UserStore::back_up`, `open_backup`, `backup_contents`, `restore`, `backups`, `contents`, `preferences`, `local_time`; `BackupKind`, `BackupFile`, `BackupContents`. |
| `obs-engine` | `backups.rs` | `UserData`'s backup methods; the restore preview (`restore_lines`); reloading the Session after a restore; Undo restore; Export everything; commands `data.back_up`, `data.restore`, `data.undo_restore`, `data.export`. |
| `obs-ui-egui` | `shell/backups_tab.rs` | The Tab: native save/open dialogs on their own thread, the preview panel, the kept Backups list. |

## The Backup file

- `UserStore::back_up(dest)` copies the database with SQLite's online backup API into
  `.<name>.partial` beside `dest`, switches the copy to a rollback journal (so it is one file,
  with no `-wal` or `-shm`) and renames it over `dest`. A failed write leaves no half file.
- Its format version is the database's own: `PRAGMA application_id` (`OBSU`) says it is a
  User database, `PRAGMA user_version` which schema it has. There is no other header.
- The launch backups (`backups/user-000006.db`) and the before-upgrade copy
  (`user.v8-<time>.bak`) are written the same way, so they are Backups too.

## Restore

`UserStore::restore(path)`:

1. `open_backup` reads the file read-only into an in-memory database, refuses a file that is not
   a User database (wrong `application_id`, or none) or is newer than this build
   (`StoreError::NewerFormat`), runs the same forward-only migrations a launch runs, and makes
   the default Notebook if there is none. Nothing has been written yet.
2. A **Safety backup** of the live database goes to `backups/user-safety-000001.db` (the last
   `SAFETY_BACKUPS` = 5 are kept; their numbering is separate from the launch backups').
3. The migrated copy's pages are copied over the live connection with the backup API, in one
   step. The live database stays in WAL mode. Pages copy only between databases of one page
   size: a copy with another is first rebuilt in `backups/.user-restoring.db` with the live
   page size (`VACUUM`), then deleted.

`Session::restore_backup(path)` then calls `attach_user` again, so everything the Session keeps
of the User database reloads as at launch (View settings, toolbar, Preferred order, Favorites,
History, Reading plans, Home, Copy presets, Collections, Saved searches, keys, the Starter
library state) and `annotations_revision` moves, so reading panes reload their Marks. The
annotation undo history is cleared (its rows no longer exist). The Workspace on screen is kept,
and the Last session cache is forgotten so the next save writes it over the Backup's. A Notes
Tool showing a Notebook the Backup lacks goes back to All.

A restore that fails before step 2 changes nothing at all: the Session keeps its User database,
undo history included, and no Safety backup is taken.

**Undo restore** (`data.undo_restore`, enabled after a restore in this run) restores the last
restore's Safety backup, taking a Safety backup of its own.

## The preview

`Session::restore_preview(path)` compares `UserData::contents()` (now) with
`backup_contents(path)` (the migrated copy) as `RestoreLine`s: Notebooks, Notes, Highlights,
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
| `annotations.json` | Every Notebook and Annotation with its Anchor, as `obs_store` serializes them. |
| `layouts.json`, `last-session.json` | Layouts by name with their Workspace snapshots; the Last session. |
| `favorites.json`, `history.json`, `reading-plans.json` | With their locations as JSON. |
| `preferred-order.json`, `settings.json` | Resource ids best first; every preference, JSON values parsed. |

## Tests

- `crates/store/tests/user_backup.rs`: one-file Backups; restore brings back a deleted
  Notebook and its Safety backup undoes it; an older Backup is migrated; newer, foreign and
  missing files are refused with no Safety backup; the kept list; another page size.
- `crates/engine/src/backups_tests.rs`: the #217 acceptance through commands (back up, delete a
  Notebook, preview, restore, Undo restore, restart); reloading settings and Layouts and
  writing the Workspace after a restore; a refused restore keeping undo; Export everything's
  files.
- `crates/ui-egui/src/backups_tests.rs`: the Tab from the Tools menu, with injected dialogs:
  Back up now, preview, Restore, Undo restore, a kept Backup's Restore…, Export everything and
  a cancelled dialog. `UPDATE_PREVIEWS=1` writes `backups-*.ppm` to `target/previews/`.
