# Back up, restore and export all user data (#217)

Everything the user makes or chooses is in the User database (#17), so a **Backup** is a copy of
it and a restore puts one back. ADR 0013 records why. **Tools → Back up & restore…**
(`data.backups`), or Backups in Settings → Privacy and data (#213), opens a keyless,
single-instance Tab with all of it.

| Layer | Piece | Role |
|---|---|---|
| `obs-store` | `backup.rs`, `backup_store.rs` | `UserStore::back_up`, `open_backup`, `backup_contents`, `restore`, `refuse_own_file`, `backups`, `contents`, `preferences`, `local_time`; `BackupKind`, `BackupFile`, `BackupContents`. |
| `obs-engine` | `backups.rs`, `backup_check.rs` | `UserData`'s backup methods; the trial restore of what the engine keeps inside rows (`Session::trial_restore`, `unkept_records`, `held_settings`); the restore preview (`restore_lines`); reloading the Session after a restore; Undo restore; Export everything; commands `data.back_up`, `data.restore`, `data.undo_restore`, `data.export`. |
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

### What Sync added (schemas 13 to 18, ADR 0022)

Migrations 12 → 13, 13 → 14, 14 → 15, 15 → 16, 16 → 17 and 17 → 18 (`crates/store/src/sync_schema.rs`) add what Sync needs; a
Backup holds it like every other table, and an older Backup gains it as it is restored (with
ids made then). Every write of Synced data is a `Change` through `UserStore::apply_changes`
(S27b, `crates/store/src/synced.rs`), which writes the tombstones and, from schema 14, stamps
each row, item and tombstone with its hybrid logical clock (S28a, `sync_clock.rs`; the
encoding is in [`sync.md`](sync.md#clocks)); schemas 15 and 16 add what the merge needs
(S28b-1, S28b-2, [`sync.md`](sync.md#merging)), 17 what a restore or a copy needs (S28c,
[`sync.md`](sync.md#restores-and-copies-new-device-ids)), and 18 the order the former ids were
given up (the sync merge fix).

| Table or column | What |
|---|---|
| `clock` on every table in `SYNCED_TABLES` | The hybrid logical clock of the row's last change ([`sync.md`](sync.md#clocks)); `NULL` for a row last changed before schema 14, which sorts before every clock. `anchors` has none (an Anchor goes with its Annotation's clock); in `preference` only the keys in `SYNCED_PREFERENCES` are Synced data, every other key (`DEVICE_PREFERENCES`, or unlisted) stays on the device. |
| `layout.id` | Layouts are keyed by a UUIDv7; the name stays unique ignoring case, so every Layout keeps its name and is still found by it. Saving over a Layout keeps its id. |
| `favorite.uid`, `reading_plan.uid` | A UUIDv7 each, unique, beside the numbers the app names them by (which another device also hands out). |
| `tombstone` | A deleted synced row, kept for good: `row_table` is its kind (the `row` tag of a serialized `Row`: `notebook`, `annotation`, `favorite`, `layout`, `reading`, `collection`, `guide_section` …), `row_id` its id (a Reading's `<plan uid>/<position>`, a Guide section's `<guide>/<section>`, a list item's name), `clock` its delete's clock. A tombstone written at schema 13 has empty text (`synced_store::NO_CLOCK`), which, like a row's `NULL` clock, means "before this device recorded clocks" and sorts before any clock ([`sync.md`](sync.md#clocks)); from schema 14 every delete writes its clock. `covers` (15): the clock of the version the delete removed (`''`: one from before clocks; `NULL`, every tombstone written before schema 15: up to its own clock), so another device's edit it did not see brings the row back. From schema 15 a tombstone is kept beside a row made again (an undone delete, such an edit), with the newest delete clock and the newest version covered; before it, a row made again took its tombstone away. Replacing the Preferred order leaves none. `row` (16): on a deleted Notebook, Reading plan, Highlight palette or style, the row as its delete removed it (a serialized `Row`), which a child arriving from another device restores ([`sync.md`](sync.md#deleted-parents)); `NULL` on every other tombstone and every one written before schema 16. |
| `device` | One row: this Synced device's id (UUIDv7, made by the migration) and the name the user gives it. A restore copies the Backup's row like any other, then gives the device a new id (ADR 0022 rule 7, below). `restore_choice` (17): 1 after a restore on a device that was syncing, until the user chooses to catch up or replace. |
| `former_device` (17; `seq`, 18) | The Synced device ids this database had before (after a restore, the one it had and its former ones, plus the Backup's only when that is one of them; after a copy, the copy's): the merge counts a clock naming any of them as this device's own. `seq` is the order they were given up (a v17 Backup's numbered by id); the merge merges their op files the last given up first. |
| `applied_up_to` | For each other Synced device, the last of its op files applied here; for this device's own id, the last of its own op files written. This device records ops (keeps them in `op_outbox`) only while its own id has a row. |
| `item_clock` (14) | The clock of each item of a list preference (a Collection, Copy preset, Saved search, Guide section, Resource tags), keyed as its tombstone is; `preference.clock` of the list's key is the newest clock of any op on its items. `position` (15): the item's position as its last write gave it, so every device composes the list in the same order (`NULL`, an item written before schema 15 or a list last written whole: its index). A deleted item's row goes; its tombstone has the clock. The Preferred order's clock is here too (`preferred_order`, `''`), kept when the order is empty. From schema 16 a list written whole has its base's clock here (`preference`, its key), and the base's items the base's clock and their index ([`sync.md`](sync.md#a-lists-whole-value)). |
| `op_outbox` (14) | While this device records ops: each `Change` applied, with its clock, until it is in an op file of the Sync folder (`file_seq`, once reserved). Empty on a device that does not sync. |
| `annotations.conflict_of` (16) | On a Sync conflict's copy, the id of the Note whose text won ([`sync.md`](sync.md#sync-conflicts)); `NULL` on every other Annotation. |
| `sync_held` (16) | Other devices' ops this device could not apply yet, each with its row, clock and `change` JSON, tried again at every sync ([`sync.md`](sync.md#ops-held-back)). Empty on a device that does not sync. |

A **snapshot** in the Sync folder (S28c, [`sync.md`](sync.md#snapshots)) is a Backup in this
format holding only the Synced data and what merging it needs: History, the Last session,
remembered Resource names, the outbox, the former ids, the Notes search index (`notes_fts`) and
every preference that is not Synced data are emptied, and it is vacuumed. It is read with the same checks as a Backup.

The restore checks treat them as any table: a Backup missing one is refused, and the trial
restore compares only what the engine loads, so none of them changes what a restore keeps.

S27b changed no table: the synced preferences that hold a list (`library.collections`,
`library.tags`, `copy.presets`, `search.saved`, `guide-arrangement/<guide>`) are still one
JSON value per key, written byte for byte as before. Their `Change`s are item by item
(`Row::Collection`, `Row::ResourceTags`, `Row::CopyPreset`, `Row::SavedSearch`,
`Row::GuideSection`; `preference_items.rs`), and a value not in its list's shape is written
whole (`Row::Preference`), so nothing in it is lost. Their items' clocks are in
`item_clock` (14), since `preference.clock` is one per key.

## Restore

`UserStore::restore(path)` (`open_backup`, then `restore_from` the copy it read):

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
3. The migrated copy is staged in a copy (in memory, or the page-size rebuild below), which
   first gets (ADR 0022 rule 7, `sync_device.rs`) a new Synced device id, the live database's
   id and former ids kept as its former ones (the Backup's only when it is one of them), and an
   empty outbox (ops under the old id); on a device that was syncing, `device.restore_choice`
   is set there. Its pages are then copied over the live connection with the backup API, in
   one step, so all of it arrives together, and Sync waits until the user chooses to catch up
   or replace
   ([`sync.md`](sync.md#restores-and-copies-new-device-ids)). The restore's checks and the
   trial restore compare what the app keeps, which this does not change. The live database stays in WAL mode. Pages copy only between databases of one page
   size: a copy with another is first rebuilt in a new scratch file
   `backups/.user-restoring.<pid>-<n>.db` with the live
   page size (`VACUUM`), then deleted, however the restore ends (a `ScratchFile` guard).

`Session::restore_backup(path)` reads the Backup into memory once (`UserData::open_backup`),
loads that copy in a **trial restore**, and hands the same copy to `UserData::restore`
(`UserStore::restore_from`): the file is not read again, so one changed meanwhile cannot put
back anything the trial did not load (#258). The trial (`backup_check.rs`) is how the app
knows a restore would keep everything the engine keeps *inside* rows, which the store cannot
read. Rather than a check per kind of record (which kept drifting from the loaders, review
rounds of #258), it runs the loaders themselves:

1. `StoredRecords::read` takes what the Backup holds before anything loads it (loading writes:
   a Text size too small to read is reset, the library's Attributions are confirmed).
2. `UserData::copy_in_memory` copies the Backup page for page, and a Session of its own, with
   this Session's library, attaches the copy exactly as a launch does (`attach_user`: every
   loader), restores every Layout with `layout.restore`, and reads what is read on demand
   (recent searches, Command box entries, Word forms, kept Attributions). This Session, the
   Backup in memory and the file are not changed. The process's Log level, which loading
   sets, is put back.
3. `Session::unkept_records` compares what that Session holds with what the Backup held: every
   Favorite and Visit (by id, each location kept), every Reading plan and each of its Readings
   (by position), every Layout whole (below), and every setting a loader reads
   (`held_settings`: the value the app would write for it now). A setting is kept when its
   value now holds every value the stored one holds (`holds`): the same number (a whole
   number the app writes, such as an id or counter, only as that whole number written as one,
   so `16.0` or `1.6e1` that an integer loader falls back from is not; one with a fraction as
   written or as the app keeps it, `f32`), text and flag, each field of an object (one missing now only if
   it carried nothing: `null`, `false`, `0`, empty), and each item of a list, matched once in
   any order, so a repeated item kept once, an unknown id set back to its default, a clamped or
   cut value, a wrong-typed field and a field this version does not know (with a value) all
   count as lost. Records from an older version are first put in today's shape by their
   loader's own upgrade (`appearance::upgrade_record`: #37's `dark_page` and boolean
   `verse_numbers`, a Text size saved while it was the default; `KeyBindings::upgrade_record`:
   old Selection key ids, unless the record holds the new id too, when the loader would keep
   only one).
   A Layout is compared twice with the same rule: its stored JSON against what
   `WorkspaceSnapshot::from_json` read of it (so a field this version does not know, anywhere
   in it, counts as lost), then what was read against the Workspace `layout.restore` gave
   (every Tab by id with all its fields, the arrangement and its shares, Floating windows,
   remembered Panels; a remembered Panel under a Tab kind this version does not know is
   dropped by the restore, so lost). Two exceptions, as at launch: Tabs of Resources not in
   the library are left out (`without_missing`), and a focused Tab, remembered Panel or last
   Panel naming one the Layout does not have falls back (`without_dangling`; the app writes
   remembered Panels it has since closed). Tabs the arrangement does not place (all of them
   in a Layout saved without the UI) get a Panel of their own on the right, which the
   arrangement may hold besides the stored one.
4. A panic while loading (a record no loader expected) is caught (`catch_unwind`) and refuses
   the Backup as damaged ("it is damaged (the app stopped reading it: …)"). The loaders the
   reviews found panicking now refuse instead: `PlanDate::parse` reads only years 1–9999 and
   days 1–31 before any arithmetic, and a Reading's number is counted in `u64`.

A Backup with anything the trial did not keep is refused, naming each ("the Layout “Broken”",
"the Favorite “John 3:16”", "2 History visits", "the Reading plan “Gospel”", "Reading 3 of the
Reading plan “Gospel”", "the setting `search.saved` (Saved searches)"), before a Safety backup
is taken, with nothing changed. Two settings are not compared, because they only name another
record and fall back to the default when it is gone, as at launch: the last Notebook used and
the active Highlight palette. A setting no loader reads (a Guide arrangement of a Guide this
version does not have, the Import report of a Resource not in the library) is restored as it
is; nothing reads or overwrites it. The Last session is not compared either: a restore keeps
the open Workspace, and the next save writes it over the Backup's (ADR 0013). Two tests keep the
trial complete: `every_setting_the_engine_keeps_is_compared_by_the_trial_restore` fails when a
`*_PREF`, `*_PREF_PREFIX` or `*_KEY` constant is neither compared nor listed as a pointer, and
`a_backup_of_what_the_app_writes_passes_the_restore_check` fails when a setting the app writes
is not compared or not kept.

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
as a Tab restored from a Workspace snapshot does: no Visit, no Search history entry and no
Recent searches entry, so the Backup's own Recent searches come back as they were), keeping
their query.

A restore that fails before step 2 changes nothing at all: the Session keeps its User database,
undo history included, and no Safety backup is taken.

**On a device with a Sync folder** (plan S29, supervisor decision D5): `restore_backup` and
Undo restore first sync when the folder is reachable (`Session::sync_before_restore`), so edits
not yet written reach the folder before the restore replaces them; a sync that fails does not
hold the restore back (the Safety backup has the edits). After `UserStore::restore`, the
device's own Sync folder preference (`sync.folder`, or none) is written back over whatever the
Backup held, before the Session reloads: **a restore never starts, stops or moves sync** (a
Backup from before this device synced, or another computer's, would otherwise drop its folder
or point it at another), which makes `sync.folder` the one setting a restore does not take
from the Backup. The trial restore does not compare it (`NOT_COMPARED` in `backups_tests.rs`).
On a device that was syncing, the restore then waits for Catch up or Replace
([`sync.md`](sync.md#the-desktop-s29)).

**Undo restore** (`data.undo_restore`, enabled after a restore in this run) restores the last
restore's Safety backup, taking a Safety backup of its own. It skips the trial restore: the
Safety backup is the data as it was in use, and is put back as it was.

Before the Tab runs either, it saves the Note being written and the Notes card being edited
(`settle_note_editors`, as closing the window does), so text typed before the restore is in
the Safety backup and no later autosave or retry writes it over the restored Note. A save the
database refuses holds the restore back: its card is shown with why, and the preview stays
open. After a restore, the Notes Tab forgets what it was in the middle of (a Notebook name
being typed, Delete Notebook's question, a Re-attach).

## The preview

`Session::restore_preview(path)` runs the same trial restore as `restore_backup` (a Backup it
refuses shows why instead of a preview), then compares `UserData::contents()` (now) with the contents
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
  an unreadable Layout, Favorite, Visit or setting refused naming each, a Layout with two
  Tabs of one id, a Layout with a field this version does not know (in a Tab's Reading layout
  or on the Layout) or a remembered Panel of an unknown Tab kind refused while one with a Tab
  of a missing Resource and dangling Panel references restores as stored, a Reading plan with an unreadable start day or Reading, dates far outside
  the calendar and a Reading at position `u32::MAX` refused naming each, settings whose loaders
  would default, drop, clamp or cut a field refused while older records still restore, a
  Visual filters counter the loader would raise by one past `f32` precision refused, a
  loader's panic refused as damaged, and one of everything the app writes passing;
  `every_change_to_a_backup_is_refused_cleanly_or_restored_whole` changes each value of such a
  Backup in turn (about 380 cases: huge and negative numbers, wrong types, unknown ids,
  impossible dates, repeated items and ids, a shortcut under its old and new id, text that is
  not JSON) and checks each is refused by preview and restore alike with the live data and
  kept Backups unchanged, or restored with nothing changed by the restore and every loader of
  the live Session keeping all of it (the Layout also checked apart from the trial: its stored
  JSON itself against the Workspace `layout.restore` gives), and that nothing panics; every `*_PREF` constant compared
  by the trial; open Notes
  searches run again after a restore and after Undo restore.
- `crates/store/tests/sync_schema_db.rs`: a v12 database migrates to 13 with every row kept
  and a copy taken first; every synced table has a clock; Layouts keep their names and gain
  ids (kept when saved over or renamed); new Favorites and Reading plans get ids; a v13 Backup
  round-trips and a v12 one gains the new tables as it is restored; every preference the
  engine keeps is listed once as synced or staying on the device.
- `crates/store/tests/sync_clock_db.rs`: a v13 database and a v13 Backup migrate to 18
  losing nothing (a restore giving a new id and emptying the outbox); migration 16's columns and tables (a copy's `conflict_of`, a parent's
  tombstone row, a held op, a list's base) round-trip through a Backup, and a v15 Backup restores
  with none; a Backup carries the item clocks and the outbox; item positions and what
  each tombstone covers round-trip through a Backup, and a v14 Backup restores with both
  `NULL` (an item at its index, a tombstone covering up to its own clock); every database has a
  device id after `open` and `open_backup`; the clock passes every clock held after a reopen
  (with the wall clock stepped back) and after a restore; the outbox keeps ops only while
  recording and a reserved op file comes back with the same ops.
  `synced_writes_db.rs` (`every_write_of_synced_data_stamps_its_rows_with_clocks_and_records_its_ops`)
  checks every `Row` variant's write stamps its row, item or tombstone.
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
