# A Backup is a copy of the User database, restored in place after a Safety backup

Users could not see, export or restore the rolling backups the User database kept (#17), and an offline app has to make users' own data portable (#217). Everything the user makes or chooses already lives in the one User database (Notes, Highlights, Notebooks, Layouts, Reading plans, Favorites, History, Collections, Saved searches, keys and every other setting), so we decided:

1. **A Backup is a copy of the User database file**, written with SQLite's online backup API into one self-contained file (rollback journal, no `-wal`), with the suggested extension `.obsbackup`. Its format version is the User database's own: `PRAGMA application_id` says it is ours and `PRAGMA user_version` which schema it has. A Backup from an older app is migrated as it is restored, with the same forward-only migrations a launch runs; one from a newer app is refused. The launch backups and before-upgrade copies the app already kept are Backups too, and are listed and restorable.
2. **Restore replaces the live database in place.** The Backup is read into memory, migrated and checked whole first (integrity check, every table, column, index and trigger of this build's format, every row readable), and, since #258, no row pointing at a missing one and nothing the engine keeps inside rows (Layout snapshots, Favorite and Visit locations, settings kept as JSON) that the app could not read, so an unreadable, damaged, foreign or newer file changes nothing and a restore never drops or defaults anything silently. Then a **Safety backup** of the database as it is is taken, and the Backup's pages are copied over the live connection in one step. The Session reloads everything it keeps of the User database, as at launch (a preference the Backup lacks is back at its default); the open Workspace stays as it is and is what the next Last session save writes. Undo restore restores the Safety backup; the last five Safety backups are kept.
3. **Export everything is a separate, read-only form**: a zip of Markdown (each Notebook's Notes export) and JSON (every table), with a `manifest.json` carrying its own `export_version`. The app never reads it back.

## Considered Options

- **A JSON (or zip of JSON) Backup that restore imports table by table:** readable by other tools, but every table and every future migration would need an exporter and an importer kept in step, and a restore could half-succeed. The database file already is a versioned, migratable, transactional format; Export everything covers reading the data elsewhere.
- **Swap the database file and reopen it:** the Session owns the connection behind a trait object, the file would have to be closed under a running app, and WAL files beside it would have to move with it. Copying pages into the open connection keeps one connection and one file.
- **Restore the backup's Last session too:** replaces the Workspace the user is working in, with Tabs that may name Resources this library lacks. Layouts come back, and a Layout restores a Workspace on purpose.
- **Undo restore through the annotation undo history:** a restore replaces far more than annotations, and the history is reset when the Session reloads. A Safety backup is a Backup like any other, so it also survives a restart.

## Consequences

- Nothing is ever written over the live database or its SQLite files: Back up now and Export everything refuse those paths, by any name.
- A Backup carries everything in the User database, including History and the Command box's recent entries; it is as private as the database itself.
- A Backup file is only as portable as SQLite's file format, which is stable and documented; anyone can open one with `sqlite3`.
- Restoring a Backup made before a Resource was removed brings back Annotations on it; they show as they do for any Resource not in the library.
- Pages copy only between databases of one page size, so a Backup with another one (made by some other SQLite build) is first rebuilt, in a temporary file in the backups folder, with the live database's page size.
