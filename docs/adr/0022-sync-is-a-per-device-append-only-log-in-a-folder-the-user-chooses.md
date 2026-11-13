# Sync is a per-device append-only log in a folder the user chooses

The Mobile app and the desktop app keep one person's study data in step with no account and no
server of ours (#324), through a cloud folder the user already has. The research (#326) showed
that a live SQLite file in a synced folder corrupts (WAL, locks and journals do not survive a
sync client) and that every sync client makes conflict copies when two devices write one file.
ADR 0006 still holds: nothing the user made is ever dropped silently. We decided:

1. **The User database stays private on every device.** Only files written whole and never
   changed go into the Sync folder, under `Open Bible Sync/`:
   `sync.json` (format version), and per device `devices/<device-id>/device.json` (name,
   platform, app version), `ops/<seq>.jsonl` and `snapshots/`.
2. **Each device writes only its own files.** An op file is one batch of the engine's
   `Change { before, after }` records as JSON lines, each with the device id and a hybrid
   logical clock (wall-clock milliseconds, a counter and the device id), and ends with a
   length and hash line. It is written to a temporary name and renamed, so a file that is
   incomplete or fails its hash is skipped and read again next time. No file is written by two
   devices, so the providers never make conflict copies of ours.
3. **Every synced row carries the clock of its last change, and deletes leave tombstones**
   (the row's id and clock, kept for good). Rows merge last-writer-wins by clock, except:
   - **A Note's text edited on two devices before either saw the other** is a **Sync conflict**:
     the later edit stays in place and the other becomes a second Note on the same Anchor in
     the same Notebook, listed in Needs attention until the user keeps, merges by hand or
     deletes one. No automatic line merge in v1.
   - **Edit beats delete:** a row deleted on one device and changed on another comes back, as
     do a deleted Notebook and its Annotations if another device added to it.
   - **Fields an op does not carry are left as they are**, so an older app's edit does not wipe
     a field it does not know.
   - **Reading done-marks** merge per Reading; **Preferred order** is one value, last writer
     wins.
   - **Rows keyed by a name** (Layouts) get ids; two with the same name both stay, the later
     renamed "(2)". **Two default Notebooks** from devices that joined separately: the older
     stays the default; the other becomes an ordinary Notebook, named after its device if the
     names clash.
4. **What syncs:** Notebooks, Annotations and their Anchors, Highlight styles and palettes and
   the active palette, Reading plans and progress, Favorites, Collections, Resource tags, Copy
   presets, Saved searches, Guide arrangements, Preferred order, Layouts (the Mobile app keeps
   but does not use them) and a short list of study preferences. **What does not:** History,
   the Last session, the Search history, the Command box's recent entries, Removed Starter
   Resources, which Resources are installed, and every display preference (Theme, Page tone,
   Text size, fonts, Keyboard shortcuts, window sizes). A preference syncs only if it is listed
   as one that does.
5. **Snapshots bound the log.** Each device sometimes writes a snapshot of its synced rows (the
   Backup format, ADR 0013) with the clocks it covers, and deletes its own op files older than
   90 days that a snapshot covers. A new device starts from the newest snapshot and replays
   the ops after it; a device away longer than 90 days merges others' snapshots row by row,
   by the same rules, and then writes its own pending changes as ops, so it loses nothing.
6. **Format versions:** an op file or snapshot with a newer format than the app reads is left
   alone, and the app asks the user to update to sync with that device; it keeps writing its
   own ops meanwhile.
7. **Device id:** a UUIDv7 in the User database. Restoring a Backup gives the device a new id.
   A device that finds ops under its own id that it did not write (a copied database) takes a
   new id too. After a restore the user chooses: the restored data **catches up** with the
   other devices (the default), or **replaces** what is in the Sync folder (a snapshot with new
   clocks that wins everywhere).
8. **Sync is periodic, not live:** at launch, on resume, about 30 seconds after an edit, every
   5 minutes while open on the desktop, and on demand. The folder is not encrypted in v1; the
   folder belongs to the user's own cloud account.

## Considered Options

- **The live User database in the folder:** corrupts (#326 §1).
- **Whole-database snapshots with a three-way merge (KOReader):** one shared file, so two
  devices uploading at once make conflict copies, and every table needs hand-written merge SQL.
- **SQLite session changesets:** SQLite's own binary format with no clock or device, needs a
  primary key on every table and the same starting data on every device.
- **CRDT libraries (cr-sqlite, Automerge, Loro):** cr-sqlite is stale and needs `unsafe`;
  Automerge and Loro would move the source of truth off SQLite. Revisit only if collaborative
  Note editing becomes a goal.
- **A three-way line merge of Notes (Joplin):** better when it works, but a wrong automatic
  merge in a Note is worse than two clear versions. Later, if Sync conflicts turn out common.
- **Encrypting the folder with a passphrase:** no account means a forgotten passphrase loses
  the synced copy; deferred.

## Consequences

- A User database migration adds a clock column to every synced table, a tombstone table, the
  device id and a per-device "applied up to" table; hard deletes become tombstones. Layouts
  move from name keys to ids. `notes_fts` stays local and is rebuilt from merged rows.
- Every change to synced data goes through `Change`, including the tables outside
  `apply_changes` today (preferences, Layouts, Favorites, Reading plans).
- Backups (ADR 0013) keep working on their own; a restore on a syncing device asks catch up
  or replace.
- The desktop app gets sync between the user's own computers too, without the Mobile app.
- One person's devices only: a Sync folder shared by two people is not supported.
