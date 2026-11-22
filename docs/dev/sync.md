# Sync: the Sync folder, the merge and the lifecycle (ADR 0022)

The Rust side of Sync, from plan steps S28a (the file layer), S28b-1 and S28b-2 (the merge,
[below](#merging)) and S28c (snapshots, trimming, device ids, restores, when sync runs:
[the contract](#contract)), the desktop's Settings → Sync, status indicator, restore
prompt and Needs attention from S29 ([below](#the-desktop-s29)), and sync on a worker off the
UI thread from S29b ([below](#off-the-ui-thread-s29b)); S30 is the iOS screens. The
shells render the states and words of the engine and hold no rule. Terms are in [CONTEXT.md](../../CONTEXT.md):
**Sync folder**, **Synced device**, **Synced data**, **Sync conflict**.

| Layer | Piece | Role |
|---|---|---|
| `obs-store` | `hlc.rs` | `Hlc`: the hybrid logical clock, its text form, `Hlc::next`, `MAX_FORWARD_SKEW_MS`. Compiles everywhere. |
| `obs-store` | `user.rs` | `UserStore::second_connection` (`SecondConnection`: `open`, `refuse_writes`), `BUSY_TIMEOUT`, `UserStore::write_tx` (`BEGIN IMMEDIATE`) and `WriteTurns` (S29b: [off the UI thread](#off-the-ui-thread-s29b)). |
| `obs-store` | `sync_clock.rs` | From schema 14, `apply_changes` gives each `Change` the next clock and writes it into the row, item or tombstone; while this device records ops, keeps each `Change` with its clock in `op_outbox`. `device_id`, `observe_clock`, `start_recording_ops`, `records_ops`, `reserve_op_files`, `op_file_written`. |
| `obs-store` | `sync_schema.rs` | Migration 13 → 14 (`item_clock`, `op_outbox`), 14 → 15 (`tombstone.covers`, `item_clock.position`), 15 → 16 (`annotations.conflict_of`, `tombstone.row` with `row_covers` and `row_clock`, `sync_held`) 16 → 17 (`former_device`, `device.restore_choice`) and 17 → 18 (`former_device.seq`); `ensure_device` at `open` and `open_backup` (a missing or damaged device id gets a new one). |
| `obs-store` | `sync_device.rs` | ADR 0022 rule 7: `own_device_ids` (this id and the former ones), `take_new_device_id` (a copied database), `restore_choice_pending`, `clear_restore_choice`, `device_name`, `set_device_name`, `merged_devices`, `last_reserved_op_file`; `renew_device_id`, which `UserStore::restore_from` (`backup_store.rs`) runs after every restore. |
| `obs-store` | `snapshot_store.rs` | ADR 0022 rule 5: `snapshot_bytes`, `open_snapshot`, `snapshot_contents` (`SnapshotContents`), `merge_snapshot`, `merged_up_to`, `has_pending_ops`, `join_sync`, `restamp_for_replace`, `MergeTx::keep_base_positions`; `SNAPSHOT_EMPTIED`. |
| `obs-store` | `merge_store.rs` | `UserStore::merge_op_file`: one op file merged in one transaction with its `applied_up_to`; `UserStore::merge_held`: the held ops tried again in one transaction; `MergeTx` (`state`, `write_merged`, `delete_merged`, `note_delete`, `note_item_op`, `note_list_op`, `write_kept`, `record`, `begin_op` / `end_op`, `hold` / `unhold` / `held`, `list_base`, `list_in_shape`, `write_list_base`, `tombstone_row`, `renamed_for_clash`); `row_state`, `applied_up_to`, `sync_held`; `UserStore::sync_conflicts` (`annotation_store.rs`). |
| `obs-engine` | `sync/folder.rs` | `SyncFolder` (`list`, `read`, `write_new`, `remove`, `ensure_available`), `FsSyncFolder` (`std::fs`, the desktop's), `MemSyncFolder` (tests, feature `test-support`). The Mobile app implements `SyncFolder` in Swift (S30). |
| `obs-engine` | `sync/clock.rs` | Re-exports `obs_store::Hlc`: the store stamps rows itself (its own writes, such as `save_layout`, plan their Changes), so the clock lives there. |
| `obs-engine` | `sync/ops.rs` | `Op`, `encode_op_file`, `decode_op_file`, `read_ops_after`, `write_pending_ops`. |
| `obs-engine` | `sync/merge.rs` | The merge rules (`merge_op`) and `merge_folder`, which tries the held ops again, merges the snapshots it needs and every other device's op files not merged yet. |
| `obs-engine` | `sync/snapshot.rs` | Snapshot names, `write_snapshot` (`write_snapshot_unless_edited`), `read_snapshot`, `snapshot_ops`, `merge_snapshot`, `snapshot_due` (`SnapshotPolicy`), `trim_own_op_files`. |
| `obs-engine` | `sync/lifecycle.rs` | `sync_now`, `needs_restore_choice`, `restore_choice` (`RestoreChoice`), `SyncOptions`, `SyncReport`, `SyncState`; the copied-database check. |
| `obs-engine` | `sync/schedule.rs` | `SyncSchedule` and `next_sync_due` (ADR 0022 rule 8). Compiles everywhere. |
| `obs-engine` | `sync/mod.rs` | Paths, `sync.json` (`ensure_sync_json`), `device.json` (`write_device_json`, `read_devices`), `SyncError`. |
| `obs-engine` | `sync/texts.rs` | Every word the shells show of Sync (S29, owner-approved through the supervisor): onboarding, folder guidance per platform, status lines, device rows, the restore prompt, the Sync conflict's text and actions, held ops' lines. Compiles everywhere; S30 shows the same. |
| `obs-engine` | `sync_worker.rs` | S29b: `SyncWorker` (a sync or a restore's choice on a thread of its own, on a second connection), `RESTORE_SYNC_WAIT`, `STOP_SYNC_WAIT`, the folder it refuses once abandoned. |
| `obs-engine` | `sync_session.rs` | The desktop Session's sync (S29): the Sync folder (`SYNC_FOLDER_PREF`), the `SyncSchedule`, `Session::sync_tick`, `sync_panel`, `sync_indicator`, `sync_restore_pending`, `sync_held_lines`, `resolve_sync_conflict` (`ConflictChoice`), the restore's hooks, commands `sync.*`. |
| `obs-ui-egui` | `shell/sync_ui.rs` | Settings → Sync, the menu bar's status indicator, the restore prompt, the frame's tick; Needs attention's parts are in `shell/notes_tab.rs`. |

## Contract

What the shells call, and what they show (S29, S30). Every rule is in Rust; a shell implements
`SyncFolder` for its platform, keeps a `SyncSchedule`, and renders the states.

- **`sync_now(user, folder, &SyncOptions::new(platform))`** runs one sync
  ([below](#a-sync-sync_now)) and returns a `SyncState`:
  - `Synced(report)`: `report.merge.needs_newer_app()` names the devices whose op files or
    snapshot a newer app wrote (left alone; the app asks the user to update to sync with
    them, rule 6, and keeps writing its own ops); `report.new_device_id` is set when this
    database was found to be a copy and took a new id; `joined` the first sync of this device.
  - `FolderNewer { format }`: a newer app set up the Sync folder; nothing is read or written.
  - `NeedsRestoreChoice`: a Backup was restored while this device synced; nothing syncs until
    the user answers.
- **`needs_restore_choice(user)`** and **`restore_choice(user, folder, choice, &options)`**:
  after a restore on a syncing device the shell asks **Catch up** (`RestoreChoice::CatchUp`,
  the default) or **Replace** (`RestoreChoice::Replace`), then syncs
  ([restores](#restores-and-copies-new-device-ids)). The question survives a restart.
- **`SyncSchedule`** ([when sync runs](#when-sync-runs)): `at_launch(periodic)`, `resumed`,
  `requested`, `edited(now)`, `synced(started, finished)`, `sync_failed(now)`,
  `next_sync_due(now)`, `is_due(now)`; `periodic` on the desktop only.
- **The device's name** others show (`device.json`): `UserStore::set_device_name`.
- Needs attention lists `UserStore::sync_conflicts()` and `UserStore::sync_held()`.

## The desktop (S29)

The desktop app syncs through a folder the user picks in Settings → Sync
([`settings.md`](settings.md#sync)); until then nothing of Sync shows or runs, and the app
behaves as before (the gallery's goldens are unchanged).

- **The Sync folder** is the device preference `sync.folder` (its path; empty: none), in
  `DEVICE_PREFERENCES`, so it never syncs. `sync.choose_folder {path}` sets it (a folder that
  is there) and makes a sync due at once, which joins. A folder no longer there (a disk not
  mounted) is "Could not reach the Sync folder", tried again later; nothing is made at its
  path. The platform `device.json` names is `std::env::consts::OS` (`Session::set_sync_platform`
  lets tests and the gallery draw one platform's words on every computer).
- **When it runs:** `Session::sync_tick(now)` every frame keeps the `SyncSchedule`
  (`at_launch(true)`: at launch, every 5 minutes, 30 seconds after the first edit not yet
  synced, which it notes when the outbox has ops, and on Sync now). A due sync starts **on
  its worker** ([off the UI thread](#off-the-ui-thread-s29b), S29b; S29 ran it on the UI
  thread, supervisor decision D3, 136–141 ms for a 1,000-file catch-up) and shows "Syncing…"
  until a later tick takes its result (the tick asks for the next frame in 50 ms meanwhile).
- **After a sync** (at the tick that takes its result) that merged anything, the Session reloads what it keeps of Synced data, as
  at launch (Preferred order, Guide arrangements, Favorites, Reading plans, Copy presets,
  Collections, Saved searches, Word forms), moves `annotations_revision`, runs Notes searches
  again and re-anchors as at launch (`reload_synced_data`); preferences that stay on the device
  are not touched. The undo history is kept: a step whose rows a merge changed is refused as
  stale and the history cleared, as before.
- **The status indicator** (`Session::sync_indicator`) and **Settings → Sync**
  (`Session::sync_panel`): the status line, the error behind it (Details), `FolderNewer`, each
  device's `needs_newer_app`, the Synced devices from their `device.json` (this one first,
  "(this device)"; a restored or copied device lists its old id as another device), the name.
  Help → Keyboard shortcuts lists Sync now only once a folder is chosen.
- **Stop syncing** (`sync.stop {confirm: true}`, asked first; while a sync runs it waits for
  it, "Syncing…" meanwhile, as choosing a folder does, but at most `STOP_SYNC_WAIT`, 10 s:
  then the sync is abandoned, writing nothing more, [below](#off-the-ui-thread-s29b), and it
  stops) gives the device a new id
  (`take_new_device_id`): it records no more ops, its outbox is emptied, a restore's question
  is dropped, its files stay in the folder; then it forgets the folder (in that order, so a
  failure leaves it syncing rather than recording into an outbox nothing reads). Choosing a folder again joins as
  a new Synced device.
- **A restore** (`data.restore`, `data.undo_restore`) on a device with a Sync folder syncs first
  when the folder is reachable (`Session::sync_before_restore`; a failure does not hold the
  restore back: the Safety backup has the edits). That sync runs on its worker, after any sync
  already running, while the restore **waits on the UI thread** (a restore blocks its frame
  anyway; supervisor decision for S29b), for both at most `RESTORE_SYNC_WAIT` (30 s): then the
  restore goes ahead, the sync left running is abandoned (it writes nothing more, [below](#off-the-ui-thread-s29b)),
  and Settings → Sync's Details say "The sync before the restore did not finish in time…"
  (`texts::RESTORE_SYNC_UNFINISHED`; when the restore then fails, nothing replaced,
  `texts::RESTORE_FAILED_SYNC_UNFINISHED`). Choose folder or Stop syncing waiting for a sync
  when the restore starts applies: before it when that sync ends in the wait (a device that
  stopped runs no sync first), after it when the restore abandons that sync. The restore puts
  this device's `sync.folder` (as those left it) back
  after it, whatever the Backup held ([`backups.md`](backups.md#restore)), so a restore never
  starts, stops or moves sync. On a device that was syncing the prompt "Restored from a
  Backup" then shows over the window until **Catch up** (the default, Enter) or **Replace**
  runs `sync.restore_choice {choice}` (`restore_choice`, then a sync, on the worker: the prompt
  goes, "Syncing…" shows, and the prompt comes back if the folder could not be reached); it
  comes back after a restart.
- **Needs attention** (`notes-tool.md`) lists the Sync conflicts' copies (with "Sync conflict:
  this text and the Note's at … were changed on two devices…", and **Keep this version**,
  **Keep the other**, **Keep both**, **Combine**: `sync.resolve_conflict {id, keep}`, each one
  undo step that syncs like any edit; Combine puts the Note in place's text, a blank line and
  the copy's text in the Note in place and opens it to edit; a copy whose Note another device
  deleted says so and offers **Keep this text** and **Let it go** instead) and, under
  **Changes waiting**,
  the held ops, each "A change from <device> to <what> is waiting: <why>. It is tried again at
  every sync."

## Off the UI thread (S29b)

Every sync (and a restore's Catch up or Replace) runs on a thread of its own
(`sync_worker.rs`, one at a time), so no frame waits for it and no read of the Sync folder (an
iCloud placeholder's download can block for seconds) is on the UI thread: the worker makes the
folder, runs `sync_now` or `restore_choice`, reads the devices' `device.json`, and sends the
outcome over a channel. The tick that takes it updates the schedule and status and reloads
what was merged; a worker that stopped without an outcome (it panicked) is a failed sync.

- **A second connection.** The worker opens the User database again
  (`UserStore::second_connection().open()`: the same file, at the schema the first had,
  nothing migrated or backed up). No schema change.
- **One clock.** Both connections share one `ClockState` (`Arc<Mutex<…>>`): they hand out one
  increasing sequence of clocks, no clock twice, and a clock the merge observes is passed by
  the next edit's. The device id a clock names is read in the transaction that stamps it, so a
  new id the worker takes (a copied database) is the next edit's at once.
- **Writes take turns.** Every write transaction is `BEGIN IMMEDIATE` (`UserStore::write_tx`):
  a deferred one that read before the other connection committed could not write at all
  (`SQLITE_BUSY_SNAPSHOT`), refusing a user's edit. A write waits for the other's transaction
  (`BUSY_TIMEOUT`, 10 s), and the first connection (the UI's) goes first (`WriteTurns`): its
  busy handler retries every 100 µs and marks that it waits, and the worker starts no write
  transaction within 20 ms of that mark. Without it SQLite's busy wait, which backs off further
  at each try, left an edit waiting 125 ms behind a 1,000-file merge's transactions one after
  another; with it, 1–5 ms.
- **Edits during a sync** are stamped after every clock merged so far, go in the outbox (the
  outbox rows the sync reserved before them are its; theirs go in the next file) and are synced
  with the next sync, due 30 seconds after the edit as any; a merge transaction reads the rows
  as they are, so an edit committed meanwhile is a local edit to it (last writer wins, a Sync
  conflict's copy of Note text). An edit made during a sync is never lost. It is refused, to be
  made again, only when the same row changed on another device and the merge wrote that
  meanwhile (the edit's `before`, read outside its transaction, is not the row then:
  `StoreError::Conflict`); the Note editor turns that case into a Sync conflict's copy (its
  base-text rule), and a one-shot edit (a Notebook renamed, a Highlight style, a list
  preference) says it failed. Waiting for the worker's transaction widens that window, which
  was there before S29b. An edit while a snapshot is due makes it not due
  ([snapshots](#snapshots)).
- **Backups during a sync.** SQLite's backup API, which Back up now and the Before-update and
  Safety backups use (`backup_to`), as the snapshot's copy does (`copy_in_memory`), starts its
  copy again whenever the other connection commits. A Backup taken while a merge writes
  therefore restarts at each of the worker's commits and finishes, on the UI thread, once the
  merge's writes pause; the worker's snapshot copy restarts at each edit the same way.
- **Stop syncing abandons a sync** still running after `STOP_SYNC_WAIT` (10 s; a folder call
  that does not come back, such as a hung network disk or an iCloud download), as a restore
  does, then stops; that worker's thread ends whenever its call returns.
- **An abandoned sync** (a restore that did not wait for it, above) has its writes refused:
  its connection's commit hook rolls back every transaction from then (a commit let through
  holds the write lock the restore needs, so it is done before the restore copies), and its
  folder refuses every call. Either alone keeps the restored data as it is.
- **Measured** (`measure_syncs_off_the_ui_thread`, three runs on the owner's Mac, dev
  profile, a local folder, frames of 16 ms; each frame's sync work is `sync_tick` and the
  status the shell draws): catching up with 1,000 op files (the sync 128–159 ms), 9–11
  frames, sync work at most 0.09–0.19 ms a frame, the result's frame (with the reload)
  0.07–0.10 ms, no frame's over 1.2 ms; the same with an edit at every frame, the longest edit
  1.4–4.7 ms (0.2–0.4 ms with no sync running); a new device joining that folder, edits at most
  4–8 ms; a new device joining a library of 10,000 Notes, the result's frame 4.5–7.6 ms, but
  **an edit made during it waits 0.5–0.9 s** for the one merge transaction (10,000 ops in one
  op file, or the snapshot: [for later steps](#for-later-steps)). The frame after the
  result's, where the views read again what moved `annotations_revision` (`MarkCache`'s
  Annotations in text, Notebooks and Highlight styles, the Notes Tool's cards, Needs
  attention's count), measured from S29b's review (three more runs): 0.9–1.2 ms after the
  1,000-file catch-up, **17–24 ms after a 10,000-Note join** (the same reads as after any
  Annotation edit there; [for later steps](#for-later-steps)); in those runs the 10,000-Note
  join's edits waited 0.74–0.85 s.
- **The worker needs only `SyncFolder: Send + Sync`** (as the trait is), made on its thread
  (`sync_worker::FolderMaker`, crate-private; the desktop's is `FsSyncFolder` at the chosen
  path, checked to be there).

## Data


### The folder

Everything of ours is under `Open Bible Sync/` in the folder the user picked. Paths given to
a `SyncFolder` are relative to the picked folder, with `/` between parts, and never hold an
empty part, `.` or `..`, nor a part with `\`, `:` (a Windows drive) or a control character.
`FsSyncFolder` also refuses (`BadPath`) a path that goes through a symbolic link inside the
folder (a sync client can bring one in), so it reads and writes nothing outside it; the folder
the user picked may itself be a link.

`list` reports a file the provider keeps only in the cloud under its real name, so readers ask
`ensure_available` before reading it. `FsSyncFolder` lists iCloud Drive's `.<name>.icloud`
(how `std::fs` sees an evicted file before macOS 14) as `<name>`, and `ensure_available` says
`false` for it; `std::fs` cannot start its download.

| Path | Written by | Changed after it is written? | Holds |
|---|---|---|---|
| `Open Bible Sync/sync.json` | The first device to join, only when there is none | Never | `{"format":1}` |
| `Open Bible Sync/devices/<device-id>/device.json` | That device | Yes, whole (temp + rename), by that device only, when it changes (at each sync); the only file any device writes again | `{"format":1,"device":"<id>","name":"…","platform":"macos","app_version":"0.x.y","applied":{"<other id>":12},"synced_at":1800000000000}` |
| `Open Bible Sync/devices/<device-id>/ops/<seq>.jsonl` | That device | Never | One batch of its ops ([below](#op-files)) |
| `Open Bible Sync/devices/<device-id>/snapshots/<seq>-<sha256>.obsbackup` | That device | Never; deleted by that device once it writes a newer one | A snapshot ([below](#snapshots)) |

No file is written by two devices, so providers never make conflict copies of ours.
`write_new` writes to a temporary name in the same folder that starts with `.`
(`.<name>.<uuid>.tmp` on desktop) and renames it into place, so a reader never sees part of a
file under its real name. `FsSyncFolder` flushes the file (`sync_all`) before the rename and,
on unix, the folder after it, so a file noted written survives a power loss. JSON files are compact JSON followed by one newline. Readers ignore
fields they do not know (`device.json` from a later app still reads). `device.json`'s
`applied` (the last of each other device's op files merged there) and `synced_at` (its last
sync, its own wall clock in Unix milliseconds) were added by S28c within format 1; one without
them reads as having merged nothing and never synced.

A `sync.json` whose `format` is newer than this build's (`SYNC_FORMAT`, 1) is left alone and
`ensure_sync_json` returns `FolderFormat::Newer`: the app asks the user to update (ADR 0022
rule 6). One that is not JSON is left alone too (`SyncError::Damaged`). One listed but only
in the cloud is neither read nor written over: `ensure_available` starts its download and
`ensure_sync_json` returns `SyncError::NotDownloaded`, for the next sync to read it.

### Clocks

Every change of Synced data has a hybrid logical clock (`obs_store::Hlc`): `wall_ms` (Unix
milliseconds, 48 bits), `counter` (16 bits) and the device's id. Its one text form, in the
User database (`clock` columns, `tombstone.clock`, `item_clock.clock`, `op_outbox.clock`)
and in op files alike:

```text
{wall_ms:012x}-{counter:04x}-{device_id}
0193a1b2c3d4-002a-0193a1b2-c3d4-7000-8000-000000000001
```

Lowercase hexadecimal of fixed width, then the device's UUIDv7 (lowercase, hyphenated), 54
characters. Because every part has a fixed width, comparing two clocks as text (SQLite's
default `BINARY` collation, `ORDER BY clock`, `MAX(clock)`) compares them by wall time, then
counter, then device id; two devices tying on both are ordered by id
(`hlc.rs`: `clocks_sorted_as_text_sort_by_wall_then_counter_then_device`).

A row's `NULL` clock (last changed before schema 14) and a tombstone's empty clock (`''`,
written at schema 13, `synced_store::NO_CLOCK`) both mean "changed before this device recorded
clocks"; both sort before every clock. Existing rows keep them; every write from schema 14
writes a clock.

**Handing out clocks.** `Hlc::next(last, now, device)`: the wall time when it is later than
the last clock's, else the last clock's with the counter counted on; a counter at 65535 moves
to the next millisecond. It is always above `last`; after the largest clock there is (wall
`MAX_WALL_MS`, counter 65535) there is none, and it is an error (`StoreError::Rejected`,
which `apply_changes` returns, writing nothing). `Hlc::encode` refuses a clock with no text
form (a wall time above `MAX_WALL_MS`, a device id that is not a UUID) rather than cut it.
`UserStore` keeps the last clock in memory, seeded at the first change after opening (or
restoring) the database from the largest clock it holds (every synced table, `tombstone`,
`item_clock`, `op_outbox`, one `MAX(clock)` each), and never below a clock it handed out
before. So the clock does not go back across a restart, a restore or a wall clock stepping
back. Each `Change` in an `apply_changes` call gets its own clock, in order.

The seed only reads clocks that parse (`clock GLOB` the text form, so a damaged value sorting
above every clock hides none below it) and that are at most `MAX_FORWARD_SKEW_MS` (24 h)
ahead of the wall clock. A clock further ahead (a computer whose clock was once set years
ahead, a damaged or crafted Backup) would pin every later clock at its wall time, so it is not
taken in; its row is kept as it is (ADR 0006), and only the clocks handed out here do not
follow it.

The seed runs once per open or restore, so it does not see a clock merged in during a session.
**The merge (S28b) must call `UserStore::observe_clock(&clock)` for every clock it takes in
from another device** (the HLC receive rule, `last = max(last, remote)`), so a local edit made
after seeing a remote change gets a later clock and wins LWW (ADR 0022 rule 2). It takes in
none more than `MAX_FORWARD_SKEW_MS` ahead (and says so, returning `false`); the merged row
keeps that clock all the same.

**Where each clock is written.**

| `Row` | Its clock |
|---|---|
| Notebook, Annotation (its Anchor goes with it), Highlight style, Highlight palette, Favorite, Reading plan, Reading, Layout, Preference | The row's `clock` column |
| Preferred order | `item_clock ('preferred_order', '')`, kept when the order is empty (it has no rows then, and is never deleted, so no tombstone); every `preferred_order` row's `clock` takes the same clock |
| Collection, Copy preset, Saved search, Guide section, Resource tags (list-preference items) | `item_clock (row_table, row_id)`, keyed as the item's tombstone, with the item's `position` (schema 15; none for Resource tags); the list's `preference.clock` is the newest clock of any op on its items |
| Any of them deleted | `tombstone.clock`, and `tombstone.covers` the clock of the version deleted (an item's `item_clock` row goes) |

### Recording ops

A device records ops while `applied_up_to` has a row for its own device id; that row's `seq`
is the last of its own op files written. `start_recording_ops` adds it with `seq` 0, and so
does the join (`UserStore::join_sync`, [below](#joining)). While recording, `apply_changes` writes each
`Change`, with its clock and the clock its `before` row had (`before_clock`, inside the
`change` JSON), into `op_outbox` in its own transaction, so an edit made just before
the app stops is not lost. A device that does not sync records nothing (its clocks are still
stamped), so its User database and Backups do not grow.

`write_pending_ops(user, folder)`:

1. `reserve_op_files` (one transaction): every outbox row not yet in a file is given the next
   number (`max(own seq, largest reserved) + 1`); every reserved, unwritten file comes back
   with its ops in the order applied.
2. Each is encoded (`encode_op_file`; the same ops always give the same bytes, and an op that
   does not serialize fails the whole file) and written with `write_new`, unless the file is
   there already: with the same bytes it is kept; with others it is left as it is and the
   write stops with `SyncError::OwnFileDiffers`, noting nothing written (an op file never
   changes once written; another copy of this database wrote it, and `sync_now` takes a new
   id, [below](#restores-and-copies-new-device-ids)).
3. `op_file_written(seq)` (one transaction): its rows leave the outbox and the own `seq`
   moves on. Only a reserved `seq` at most one after the own `seq` is accepted
   (`StoreError::Rejected` otherwise), so the own files never skip a number.

A stop between 2 and 3 (the app quits, the folder fails) leaves the file reserved; the next
try finds it there with the same bytes and notes it written, and edits made in between go in
the next file (`sync::tests::a_reserved_op_file_already_written_after_a_stop_is_kept_as_it_is`).

### Op files

`devices/<device-id>/ops/<seq>.jsonl`: `seq` in decimal from 1, without leading zeros. Every
other name in `ops/` (temporary files, `02.jsonl`, a provider's conflict copy, an iCloud
`.icloud` stub) is ignored. UTF-8, one JSON value per line, each line ending in `\n`:

| Line | Fields, in this order |
|---|---|
| Each op | `device` (the device whose file it is), `clock` (text form above), `before`, `after`: a `Change`, each side a serialized `Row` (tagged by `row`) or `null`; then `before_clock`, the clock `before` had on that device (left out when there is no `before` or it had no clock; S28b-1 added it within format 1, and readers ignore fields they do not know) |
| Last: the trailer | `format` (1, `OP_FORMAT`), `ops` (how many op lines), `bytes` (the byte length of everything before the trailer), `sha256` (of those bytes, lowercase hex) |

```text
{"device":"0193…0001","clock":"0193a1b2c3d4-0000-0193…0001","before":null,"after":{"row":"notebook","id":"…","name":"Romans",…}}
{"device":"0193…0001","clock":"0193a1b2c3d4-0001-0193…0001","before":{…},"after":{…},"before_clock":"0193a1b2c3d4-0000-0193…0001"}
{"format":1,"ops":2,"bytes":412,"sha256":"9f2c…"}
```

One file is everything the outbox held when it was written, in clock order (several
`apply_changes` calls, each a transaction, may share a file). The trailer's `format` field is
in every format to come.

**Every top-level field of a `Row` is written, `null` when it has no value, never left out**
(`skip_serializing_if` is not used on them). An op line without one is then always one from
an app that does not know the field, and the merge keeps the value here
([below](#merging), "Fields an op does not carry"). A field left out when `None` would not be
seen as missing from an older app's line (`Op::absent` lists only the fields this build would
write), so the merge would write `None` over the value here. `Annotation.conflict_of`
(S28b-2) is the first such field; every new top-level `Row` field follows the rule. It is
also in every Annotation the engine's commands return (`annotations.rs`, `"conflict_of": null`
on all but a Sync conflict's copy) and in the Data export's `annotations.json`: both additive,
approved with S28b-2's review. Nested fields (a Text range's
`native_ref` …) are not compared and may be left out when empty, as they always were.

**Reading** (`read_ops_after(folder, device, applied)`): the device's op files numbered after
`applied` (S28b passes its `applied_up_to`), in order, each read only after `ensure_available`
says it is here. A device's ops are applied up to one number, so reading stops at the first
file that cannot be read, saying why (`OpsStop`), and tries it again next time:

| The file | `decode_op_file` | Reading |
|---|---|---|
| Whole: ends in `\n`, its trailer's length and hash match, every line is an `Op`, as many as the trailer says | `Whole(ops)` | Taken, reading goes on, if every op names the device whose folder it is in |
| Whole, but an op names another device (its `device` or its clock's): a file copied or mis-homed | `Whole(ops)` | Stops (`NotWhole`); read again next time |
| A newer `format` in its last line | `Newer { format }` | Stops; the file is left alone; `needs_newer_app()`: the app asks the user to update to sync with that device, and keeps writing its own ops |
| Cut anywhere (no final newline, the trailer missing or cut), a byte changed, a line not an `Op` | `NotWhole(why)` | Stops; read again next time |
| Listed but a placeholder (iCloud, Files On-Demand) | — | Stops (`NotDownloaded`); `ensure_available` started its download |
| A later number is listed but not this one | — | Stops (`Missing`): it has not arrived yet |

This fits what the providers do (#326 research §2–3): Syncthing writes to a temporary name and
renames, so a reader sees no part file at all; Dropbox, OneDrive and iCloud keep placeholders,
which `ensure_available` downloads; any other part file fails its length or hash.

### Merging

`merge_folder(user, folder)` first merges this device's former ids' op files (its own
history, the id it gave up last first, each after its snapshot when it is met through one:
[restores and copies](#restores-and-copies-new-device-ids)),
then the other [snapshots](#snapshots) it needs (of a device not met yet, or whose op files after the
last merged here are gone), then every other device's op files after its `applied_up_to`, in
order (`read_ops_after`). Each file is merged by
`UserStore::merge_op_file` in one transaction that also moves `applied_up_to` on: all of it
or none. A file with an op that needs a row not here yet (another device's file brings it:
an Annotation's Notebook, a Highlight's new style, a style's palette, a Reading's plan)
**waits**: it is rolled back, the other devices' files are merged, and it is tried again, pass
after pass while any file merges, then at the next sync (`MergeReport::waiting`, `retried`).
No two files can wait for each other: a file only needs rows from files written before it.
Every op's clock is observed (`observe_clock`), so an edit made here afterwards is later than
anything merged; one more than a day ahead is not observed (`MergeReport::not_observed`) and
its row merges as any other. A local edit of such a row (or a row made again whose tombstone
covers such a clock) would then get a clock before the one it replaces, and every other
device would keep the far version (`Superseded`) while this one kept the edit, for good. So a
local write is stamped after what it replaces: the next clock, or, when the row's clock (or
what its tombstone covers) is at or after that, the clock just after it
(`UserStore::next_clock_after`). The device's own clock does not follow it: only that write
passes the far clock, and the next write of another row gets an ordinary clock again
(`merge_tests::a_clock_more_than_a_day_ahead_is_not_observed_but_its_row_merges`: a device
edits the far row and renames a far Layout for a clash, and a third device holds the same
rows and clocks).

Per op, against the row's state here (`MergeTx::state`: the row, its clock, its tombstone):

| Op | Here | Result (`Outcome`) |
|---|---|---|
| Write | The row, with a clock at or after the op's | Nothing: last writer wins (`Superseded`) |
| Write | The row, older | The op's row, **with the op's clock**, not a new one; fields the op does not carry kept from the row here (`Written`) |
| Write | No row; a tombstone covering at or after the op's clock | Nothing: the delete saw this version or a later one (`Covered`) |
| Write | No row; no tombstone, or one covering less | The op's row, with its clock: an edit the delete had not seen brings the row back (`Written`) |
| Delete | The row, at or before what the delete covers | Deleted, its tombstone the op's clock and covers (`Written`) |
| Delete | The row, later than what the delete covers | Kept (**edit beats delete**); the tombstone notes the delete (`EditKept`) |
| Delete | No row | The tombstone notes the delete (`DeleteNoted`) |
| Delete | A Notebook with Annotations, a palette with styles, a style with Highlights, a plan with Readings (at or before what the delete covers) | Kept, the delete noted, and the parent recorded again with the rows its op file deleted with it ([deleted parents](#deleted-parents)) (`ParentKept`) |
| Delete | This device's default Notebook | Waits for the older default Notebook that replaces it |
| Write | A child (an Annotation, a Reading, a style, a Highlight's new style) whose parent is deleted here | The parent restored from its tombstone's row and recorded, then the child as above ([deleted parents](#deleted-parents)) |
| Write or delete | This device's own Note (its clock is this device's), whose text the op had not seen (its `before` text is another, or it has none) and would lose | A [Sync conflict](#sync-conflicts)'s copy recorded first, then the op as above |
| Write (an add, or an edit of another version) | This device's own Collection, Copy preset or Saved search of that name, other JSON, older, which the op had not seen | The op's as above; this device's recorded again as "`name` (2)" ([list items of one name](#two-list-items-of-one-name)) |
| Delete (of another version of that name) | This device's own Collection, Copy preset or Saved search, which the delete had not seen (its `before_clock` is not this row's clock, its `before` JSON another; a delete with no `before`, a snapshot's tombstone, only by its `before_clock`) | Kept: the delete noted, this device's item recorded again under its own name, after what the tombstone covers (edit beats delete, [list items of one name](#two-list-items-of-one-name)) (`EditKept`) |
| Write | A list item at or before its list's base (its newest whole-value write) | Nothing (`Superseded`) ([a list's whole value](#a-lists-whole-value)) |
| Write, losing (`Superseded`, `Covered`) | An op of this device's former id, against a version that did not come after it in this device's own history (not of its id now, the op's id, or a former id given up after the op's) | Its Note text recorded as a copy, its list item as "`name` (2)" or again under its name ([restores and copies](#restores-and-copies-new-device-ids)) |
| Write | A list item of a list whose value here is not in its list's shape | Held (`Held`) ([held ops](#ops-held-back)) |
| Any | An op this build refuses (a synced preference a later app added …) | Held (`Held`); the rest of its file merges |

**What a delete covers.** A delete's op carries `before_clock`, the clock of the version it
removed; its tombstone keeps the newest delete clock (`clock`) and the newest version any
delete of the row covered (`covers`, migration 15). A row is there exactly when its newest
write is later than `covers`, whatever order the writes and deletes arrive in: a phone's edit
made offline at 10:00 beats the laptop's delete at 10:05 that never saw it, and a delete made
after seeing the newest edit deletes it even on a device that gets the delete first
(`merge_tests::edit_beats_delete_in_every_arrival_order`,
`a_delete_that_saw_the_newest_edit_deletes_it_in_every_arrival_order`). From schema 15 a
tombstone is **kept for good**: a row made again (an undo, a later edit from elsewhere) lives
beside it, later than what it covers; before schema 15 an undo took the tombstone away. A
tombstone from before schema 15 has no `covers` (`NULL`): it covers up to its own clock. `''`
covers only a version from before clocks.

**Fields an op does not carry** (an older app's, which does not know a field a later one
added) are taken from the row here: `decode_op_file` notes the top-level fields of `after` a
line lacks (`Op::absent`), and the merge fills them in. Nested fields (a Text range's) are not
compared: they are left out when empty in every version. This is per arrival: two versions of
the app editing one row at once converge only on the fields both know.

**Reading done-marks** are a row per Reading and the **Preferred order** one row
(`item_clock ('preferred_order', '')`), so both merge by the rules above: two devices marking
two Readings keep both marks; two orders do not mix.

**List-preference items** (Collections, Copy presets, Saved searches, Guide sections,
Resource tags) merge item by item as above. A list's stored value holds only its items'
order, which would depend on the order their ops were merged in, so `item_clock.position`
(migration 15) keeps each item's position as its last write gave it, and the list is composed
by (stored position, name): every device builds the same order from the same last writes
(`synced_store::stored_positions`). `NULL` (an item written before schema 15, or a list last
written whole) is its index. The cost: after a merge two items can share a position, and the
next local edit of the list renumbers them, so it writes a few more item ops than it changed.
The list's `preference.clock` is the newest clock of any op on its items, and once any op on
an item is merged the list has a value (empty if none of its items is here), so every device
holds the same (`MergeTx::note_item_op`).

**Two default Notebooks** (devices that joined separately): the older by (`created_at`, id)
is the default everywhere. Whether a Notebook is the default is settled on each device, never
taken from an op: a new Notebook its device made its default becomes ordinary here when this
device's default is older, else this device's default becomes ordinary (`write_kept`: its
clock stays, as every device rewrites it the same). When that default is this device's own
(its clock is this device's, or it has none), the device also records the change as its own
edit, renamed to the device's name (`device.name`) if the two Notebooks' names clash, or
"`name` (2)" when the device has no name. A delete of this device's default Notebook (another
device deleted it as an ordinary one) waits until the older default is here.

**Two Layouts of one name** (names are unique ignoring case) both stay: the later, the one
with the larger id (a UUIDv7, so the one made later, and it never changes), is renamed
"`name` (2)" (or the first of (3), (4) … that is free) by the device that finds the clash, and
that rename is recorded as the device's own edit, so it reaches every device by last writer
wins, even one that never meets the clash itself
(`merge_tests::a_layout_renamed_for_a_clash_its_owner_never_saw_still_converges`). Two devices
recording the same rename converge on it. The cost: the recorded rename is a whole-row write,
so it can overwrite a re-save of that Layout made at the same time on another device
(rule 3's per-row last writer wins).

**Edits the merge records.** The demotion of an own default Notebook, a Layout rename, a Sync
conflict's copy, a restored parent (and the rows deleted with it) and a list item renamed for
a clash are applied with `MergeTx::record` like any local edit: a new clock, after every clock
merged and after the version it replaces (a far-ahead one included, as above, and a list
item's list base), and into the outbox while recording, written at the next sync. An own list
item recorded again (as "`name` (2)", or under its own name against a delete) is also stamped
after the version it keeps (`MergeTx::record_after`), so a list base later than the re-add is
later than that version too ([below](#two-list-items-of-one-name)). Each op is
merged in a savepoint (`MergeTx::begin_op`, `end_op`), so one that fails leaves nothing of
itself behind; what an op recorded is counted in `MergeReport::recorded` only when its
savepoint is kept.

#### Sync conflicts

A Note's text edited on two devices before either saw the other's edit (ADR 0022 rule 3, plan
S28b-2 D2): the later edit stays in place, by last writer wins, and **the device whose own text
lost** records that text as a second Note, the **Sync conflict**'s copy. "Own" is a clock naming
this device's id or one of its former ids (`MergeTx::is_own`,
[below](#restores-and-copies-new-device-ids)); the same goes for every "own" check here (a list
item renamed or kept, a default Notebook's demotion). A device's text lost
when another device's op replaces or deletes its own Note (the row's clock is this device's)
and that op had not seen its text: the op's `before` is a Note with other text, or there is no
`before` (an insert, such as an undo bringing the Note back). A delete counts too: a device
that saw a third device's later text and deleted the Note had not seen this one (it covered a
later version, so the Note is gone here), and the text must survive
(`merge_tests::a_delete_that_never_saw_this_devices_note_text_keeps_it_as_a_conflict_copy`, in
both arrival orders). No copy is made when the op's text is this device's own.

The copy has the losing text, the Anchor and Notebook of the Note in place (of this device's
Note when the op deleted it), the Note's `created_at`, and `conflict_of` (migration 16,
`annotations.conflict_of`) naming the Note. Its id is a UUID-shaped hash (version 8) of the
Note's id and the clock of the version that lost (`merge::conflict_id`), so it is the same
wherever it is made; one already here, or deleted, is not made again. The device whose edit
won makes none: its own text is in place. `UserStore::sync_conflicts()` lists the copies for
Needs attention (S19, S29); keeping one, combining them or keeping both is the UI's (S29, S30).

A rare three-device chain can make a copy no one needed: when this device's text was seen by
a second device, and a third device's later edit (which had not seen it) arrives here before
the second's, this device copies a text the second device had replaced on purpose. That is
safe (nothing is lost; the user deletes the copy) and every device converges on it, as on any
recorded edit.

#### Deleted parents

A Notebook, Reading plan, Highlight palette or Highlight style deleted on one device while
another device adds a child to it (an Annotation, a Reading or its done-mark, a style, a
Highlight) comes back, with the rows the deleting device deleted with it (its Annotations,
Readings or styles: the deletes in the same op file), so nothing is lost (ADR 0022 rule 3, plan
S28b-2 D3):

- **A parent's tombstone keeps the deleted row** (migration 16, `tombstone.row`, a serialized
  `Row`): the row as the deleting device had it, with that delete's own `covers` and clock
  (`tombstone.row_covers`, `row_clock`). A later delete's row replaces it when that delete
  covers a newer version than the kept row's delete, or the same one and is the later delete
  (`synced_store::tombstone_row_wins`). It is compared with the kept row's delete, not with the
  tombstone's `covers` and `clock` (the newest of every delete), so every device keeps the same
  row whatever order the deletes came in
  (`merge_tests::a_parents_tombstone_keeps_the_same_row_in_every_order_of_its_deletes`). A
  tombstone from before schema 16 keeps no row, and the first delete with one is kept.
- **A child arriving at a deleted parent** restores the parent from that row (its own parent
  first: a style's palette) and records it, then the child merges. Nothing waits for the
  child's maker: its own record of the parent is in a later file of its own, after the child.
- **A parent's delete meeting a live child here** (an Annotation the delete had not seen) keeps
  the parent, notes the delete in the tombstone, records the parent again (a clock after what
  the tombstone covers), and records again each row its op file deleted with it that is not
  here (`ParentKept`), except one this device's user deleted too, later: its tombstone here
  has a clock other than the file's delete
  (`merge_tests::a_kept_parent_does_not_bring_back_a_row_this_devices_user_deleted_too`). A
  Sync conflict's copy made for a Note deleted with its Notebook is in that Notebook, so the
  Notebook comes back with every Note its delete took (S29's Needs attention should expect it).
- **Every device that restores a parent records it**, not only the child's maker: the maker may
  move or delete its child before it meets the delete, so it would delete the parent while the
  devices that restored it for the child kept it
  (`merge_tests::a_parent_restored_for_a_child_its_maker_then_moved_or_deleted_still_converges`).
  Several devices may record the same restore; last writer wins makes them one. A restore is an
  ordinary write, so the rows-exist-iff-newer-than-covered rule above holds after it.
- A tombstone written before schema 16 keeps no row. A child arriving at such a parent is
  [held](#ops-held-back) and tried again at every sync, until another device (one that keeps
  the parent for that child) brings it back; that device's tombstone also differs from the
  others' in its `row` alone.

#### Two list items of one name

Collections, Copy presets and Saved searches are keyed by name. Two devices adding an item of
one name with different JSON (plan S28b-2 D6): last writer wins keeps the later under the name,
and the device whose own item lost (its item's clock is its own, and the op had not seen it:
an add, with no `before`, or a write whose `before_clock` is not this row's clock and whose
`before` JSON is not this device's) records its item again under the first free name of
"`name` (2)", "`name` (3)" … among the list's items here and the items held here (one held
while its list was damaged comes back under its own name; S28c's property runs found a
"(2)" taking the name of an own "(2)" held then, which lost it), at its old position, every
other field of its JSON as it was (`preference_items::renamed_item`), stamped after the clock
of the version it keeps as well as the list's base (`MergeTx::record_after`; the sync merge
fix: with only the next clock, a far-ahead edit's "(2)" got an ordinary clock, and a whole value
merged afterwards, later than that clock but earlier than the edit, superseded it, so the edit,
written after that base, was lost on every device). The device whose item won does
nothing. The edit case: G adds "Greek", D adds its own "Greek" later, and G then edits its own
before either syncs; G's edit replaces D's item having seen only G's, and D's would be lost on
every device (`merge_tests::an_edit_of_an_item_that_never_saw_this_devices_item_of_its_name_keeps_both`).
The cost, accepted (the list-item version of a [Sync conflict](#sync-conflicts)): two devices
editing one shared item before either syncs keep both, the earlier edit as "`name` (2)".

A delete is the same case as a Sync conflict's delete (Q1): X adds "Read", Y adds its own
"Read" later, and Z, having Y's, deletes "Read"; X meeting Z's delete before Y's add would lose
its item, which no device saw. So a remote delete of this device's own item whose `before` is
another version (its `before_clock` not this row's clock, its JSON another) does not remove it:
the delete is noted in the tombstone, and this device records its item again under its own name
(the delete freed it), stamped after what the tombstone covers, so it beats the delete on every
device by the ordinary rule (edit beats delete; `MergeReport::recorded.items_kept`). Met in the
other order, Y's add renames X's item first, as above
(`merge_tests::a_delete_that_never_saw_this_devices_list_item_of_its_name_keeps_it`, both
orders). As with D2's three-device chain, a copy no one needed can come of it: a device whose
own item the re-record replaces before it meets the delete of that item keeps it as
"`name` (2)"; safe, and every device converges on it. A delete with no `before` (a snapshot's
tombstone, merged by key) is unseen only when its `before_clock` is not this row's clock: one
that covers exactly this version had it, and deletes it (the sync merge fix: counted unseen, a
restored device met such a delete in a snapshot, recorded the item its old id had deleted
again at a new clock, and that beat the old id's later item of its name;
`merge_tests::a_delete_with_no_before_covering_this_devices_own_item_deletes_it`).

#### A list's whole value

A list preference whose value is not in its list's shape (damaged, edited by hand) is written
whole, as one `Row::Preference` (S27b). Its items are other rows, so plain last writer wins
would depend on arrival order. Instead the **newest whole-value write of a list is its base**
(plan S28b-2 D7), kept as `item_clock ('preference', <key>)`:

- an item write at or before the base is superseded; one after it goes on top of the base;
- a merged whole value later than the base becomes the base: the items here written after it
  are written again on top of it, the items deleted after it by a delete that covered the base's
  version go from it, and when it is not in its list's shape, the items written after it are
  [held](#ops-held-back) until a value in shape comes (`MergeTx::write_list_base`);
- the base keeps its own items: each gets the base's clock and its index in the base's value
  as its position (an item's clock is at least its list's base, `read_row_clock`), and the item
  clocks at or before the base go;
- the list's `preference.clock` stays the newest clock of any op on it, whole or item;
- a local write of an item is stamped after its list's base (`UserStore::next_clock_after`),
  so every device takes it.

So every device holds the base and the item ops after it, whatever order they arrived in
(`merge_tests::a_damaged_whole_list_value_holds_later_items_on_every_device_until_it_is_in_shape`,
`an_item_written_after_a_whole_value_goes_on_top_of_it_in_any_arrival_order`). An item op
meeting a value not in shape is held (D7: "keep a damaged value as is, do not apply the item,
and hold the op"); a delete then only notes its tombstone, which the value in shape to come
obeys. Deleting a list preference's whole value is handled as a base with no value; the app
writes one only as the undo of the first whole write of a list, which the property tests do not
generate.

#### Ops held back

An op this device cannot apply goes to `sync_held` (migration 16: `row_table`, `row_id`,
`clock`, and `change`, the op's `Change` with its `before_clock`, the fields its line did not
carry and why it is held), and the rest of its file merges: the stream never stalls. Held are an
item of a list whose value is not in shape (D7), a synced preference this build does not know
(a later app's, which `apply_changes` refuses as "not Synced data"), a child of a parent deleted
before schema 16, a child of a parent whose own op is held here (rather than waiting for it,
which would stall its device's stream; the held ops are tried again in clock order, so the
parent first: `merge_tests::a_child_of_a_held_parent_is_held_too_and_applies_after_it`), and
any other op this build refuses (`StoreError::Rejected` or `Conflict`).
`merge_folder` tries every held op again at its start and end, and a whole value that puts a
list in shape tries that list's at once; one that applies, or that a later write supersedes
(it lost by last writer wins), leaves `sync_held`. Of a row's held writes only the newest is
kept (`MergeTx::hold`: it supersedes the older once it applies, so a later app writing its own
setting again and again holds one op, not one per write); held deletes are all kept, and so is a
write whose line lacked fields
(`merge_tests::of_a_rows_held_writes_only_the_newest_is_kept`), and so is an older write of
this device's own (by any of its ids) when the newer is neither of its id nor of an id this
device took after it (`MergeTx::held_beside`, by `is_own_after`): tried again in clock order, it
applies before the newer one, which then finds it here, so the own checks keep it (the sync
merge fix: an own Collection held when the device merged a damaged base older than it, then
another device's later item of its name held too, was dropped by that hold, and when a value in
shape came the other's applied with no own item to rename "`name` (2)": lost on every device;
`merge_tests::an_own_list_item_held_while_its_list_is_damaged_survives_a_later_item_of_its_name`).
A newer write of its own history replaces it by last writer wins with nothing to keep, so it
goes as another device's would: after a copy, the original's writes under the id the copy gave
up are held one per row, not one per write, and the copy's own item held beside the original's
older write of it is no "`name` (2)" (`merge_tests::of_a_former_ids_held_writes_only_the_newest_is_kept`,
`a_copys_originals_older_write_is_not_held_beside_this_devices_own_item`). `merge_folder` skips the
retries when nothing is held (`UserStore::has_sync_held`, one `EXISTS`). `UserStore::sync_held()` lists them for
Needs attention (S29, S30). The held ops are this device's own record, not Synced data: two
devices can hold different ops for the same state (one that merged an item before a damaged
base and one after hold the same; one that merged a delete of it too holds none). Their clocks
are seeded like every other clock.

### Snapshots

A **snapshot** (ADR 0022 rule 5) is a Backup (ADR 0013: the User database file, `OBSU` and
its schema as `user_version`) holding only Synced data and what merging it needs, written by
its device to `devices/<device-id>/snapshots/<seq>-<sha256>.obsbackup`: `seq` the last of its
own op files it covers (decimal, no leading zeros), `sha256` the lowercase hash of its bytes.
The Backup format has no hash of its own, so a reader checks the name's: a file that is
incomplete or changed is skipped (`OpsStop::SnapshotNotWhole`) and read again next time. Every
other name in `snapshots/` is ignored. A snapshot whose `user_version` is newer than this
build's is left alone (`OpsStop::SnapshotNewer`, `MergeReport::needs_newer_app`, rule 6).

**What it holds** (`UserStore::snapshot_bytes`: a copy in memory, emptied, `VACUUM`ed, so no
freed page keeps deleted data, and marked as a rollback-journal file):

| Kept | Emptied (stays on the device, rule 4) |
|---|---|
| Every synced table with its clocks, `anchors`; `annotations.conflict_of` | `history`, `last_session`, `resource_names`; `notes_fts`, the Notes search index (emptied and merged into nothing with `optimize` before the `VACUUM`: its segments keep the words of deleted Notes; `delete-all` is only for an index without content of its own) |
| `tombstone` whole: `covers`, a parent's `row` with `row_covers` and `row_clock` | `op_outbox` (written as op files first), `former_device` |
| `item_clock`: items' clocks and positions, the lists' bases (`'preference'`, key) | Every preference that is not Synced data (display preferences, Search history and recent searches, the Command box's recent entries …) |
| `device` (the writer's id; `restore_choice` 0), `applied_up_to`: **the op files it covers**, for each device the last merged, for its own id the last written | |
| `sync_held`: ops of files it covers that it could not apply (a joiner starting from it skips those files, so they would be lost otherwise; S28c corrected S28b-2's plan to leave it out) | |

A device writes one **only with an empty outbox**, right after `write_pending_ops`, so it
covers exactly its own op files up to `seq`. An edit made while the sync runs (S29b) that puts
an op in the outbox makes it not due: `write_snapshot_unless_edited` writes none then (it
checks again after its copy, which may hold the edit), and the sync goes on; one is due again
after the next op file. Its first is the join's (`seq` 0 when it joins
with nothing written); later ones when `snapshot_due` says (`SnapshotPolicy`: 100 own op files
since its newest, or the first of them 30 days old), and whenever its newest in the folder is
not whole to `read_snapshot` (partly written, changed, damaged), so a device writing few ops or
none is still met by one joining (each sync reads its own newest snapshot for that). Writing
one deletes its older snapshots, and replaces a damaged file of its own name.

**Which a device merges** (`merge_folder`, per other device X):

| Here | X's files | Merged |
|---|---|---|
| X not met (no `applied_up_to` row) | a snapshot | X's newest snapshot, then X's op files after it |
| X not met | op files, no snapshot yet | Nothing of X (`OpsStop::NoSnapshot`): its first snapshot brings the rows it made before it recorded ops, so none of its files goes before it. A device met through another's snapshot (which covers its files) needs it no more. |
| X met up to `a` | `a + 1` is in the folder | Its op files, as before |
| X met up to `a` | `a + 1` is gone (trimmed), a snapshot covering more | X's newest snapshot, then its op files after it |

Merging a snapshot (`snapshot::merge_snapshot`, one transaction through
`UserStore::merge_snapshot`) turns it into the ops it stands for (`snapshot_ops`) and merges
them by the same rules and the same code as op files (`merge_keyed`), parents first: each row a
write at its clock; each list's base a whole-value write at the base's clock, holding the
base's items still at that clock (their stored positions kept, `MergeTx::keep_base_positions`),
then the items written after it; each tombstone a delete at its newest clock covering the
newest version covered, after (a parent's) the delete whose row it keeps, at that delete's
clock and covers; each held op as it was. Every clock is observed. Then X is merged up to the
snapshot's `seq`, and every other device up to the larger of what is noted here and what the
snapshot covers, so a new device replays only the op files after it; but never this device's
own ids, current or former: a former id's files are this device's own history and merge op by
op, so its own checks see each op (S28c review: a device met after a Backup, whose snapshot
covered the old id's file of a text, else made the restored device skip that file, and the
text was lost). Each list's
`preference.clock` is raised to the snapshot's. Rows with no clock are left out: the join
gives every row one ([below](#joining)).

A snapshot does not say what each version replaced (an op's `before`), and a version later by
its clock may have been written before its device merged this one's (its clock running
ahead). So **its versions count as not having seen this device's own**: this device's own
Note text or list item that a snapshot's version replaces is kept as a Sync conflict's copy or
"Name (2)", even when the other device had in fact seen it. That copy no one needed is the
cost, and it only comes up when a snapshot is merged (a join, files trimmed, a replace); the
property tests found the alternative ("seen when the snapshot covers every file of mine")
losing an item.

### Joining

A device starts syncing at its first `sync_now`: `UserStore::join_sync` gives every row of
Synced data with no clock yet (rows from before schema 14, the built-in Highlight styles, a
pre-sync Preferred order and list items) one clock of this device's, and records ops from
then on; the same sync writes its first snapshot before any op file. Plan S28a's "write the
snapshot, then start recording" became "start recording, then the snapshot", so an app
stopped between the two never records without a snapshot to follow: a device recording ops
with no snapshot in the folder writes one at its next sync (`snapshot_due`). The others merge
it (they have not met the device), which brings them its default Notebook and everything it
made before.

### Trimming

`trim_own_op_files` deletes an own op file when all three hold (rule 5; tombstones are never
trimmed, they are kept for good in every database and snapshot):

1. this device's newest snapshot in the folder covers it (`seq` at most the snapshot's);
2. its last op is older than `SnapshotPolicy::trim_after_ms` (90 days, `TRIM_AFTER_MS`);
3. every other device **seen** within those 90 days (its `device.json`'s `synced_at`) has
   merged it (its `applied`). A device whose `device.json` cannot be read holds every file
   back. One silent for longer catches up from snapshots, as the table above says, and loses
   nothing.

Files are in time order, so trimming stops at the first one too young.

### Restores and copies: new device ids

ADR 0022 rule 7. A Synced device's id changes when its database stops being the one that
wrote its files (`sync_device.rs`, migrations 17 and 18):

- **A restore** (`UserStore::restore_from`, so also Undo restore and every restore the app
  does) always gives the device a new UUIDv7, in one step: the Backup is staged in a copy, given
  the new id, former ids, empty outbox and pending choice there, then copied page by page in
  one transaction, so a stop never leaves the Backup's data under its old id with no choice
  waiting. **A copied database**: `sync_now` finds op files
  or a snapshot under its id numbered above both the last of its own op files written and the
  last reserved (`last_reserved_op_file`: after a stop between writing a file and noting it,
  the file is above the first but still reserved), or, on a device not syncing yet, any op file
  or snapshot under its id (a new id never has files: the database was copied before either
  copy joined), or `write_pending_ops` finds a reserved own
  file there with other ops (`OwnFileDiffers`: the copy wrote first); it takes a new id
  (`take_new_device_id`), before it joins, and catches up, with no question asked.
- **The ids it had** go to `former_device`: a restore keeps the device's own ids (the one it
  had before and its former ones) and adds the Backup's id, with the Backup's former ones, only
  when that id is one of them (supervisor decision on the S28c review): a Backup of another
  device's, restored on a new computer while that device still syncs, does not make that
  device's ops this one's own, which would make Sync conflict copies of them no one needs for
  the life of the database. Every "own" check of the merge counts a clock naming any of them as this
  device's own (`own_device_ids`, `MergeTx::is_own`): a text written under the old id that a
  remote edit never saw is still kept as a copy. Two devices claiming one id (another device's
  Backup restored, a copy) make the same copy (its id is derived) and the same "(2)", which
  converge. `former_device.seq` (migration 18) keeps the order the database gave its former
  ids up, from 1: a restore numbers them oldest first (the Backup's this device no longer has,
  then its own former ones, then the id it had), and a copy detection gives the id it gives up
  the next number; migration 18 numbers a v17 database's in id order, and a Backup carries the
  column (`own_device_ids` lists the former ids the last given up first). A former id's op
  files are this device's own history, so `merge_folder` merges them before every other
  snapshot and every other device's, the last given up first, each after its own snapshot when
  this database never met it or its files were trimmed (a Backup from before the id was
  taken) (the sync merge fix: a snapshot merged first, or an older former id's later files,
  which after a copy are the other copy's, made the merge record again at a new clock a Sync
  conflict's copy its last id had made and then edited, and that record beat the edit). What
  is left: a former id's snapshot or file that **waits** (for a row only another device's
  snapshot or file brings) is tried again after the other snapshots, so those can come first
  and the same re-made copy can beat its edit. No test reaches it, and the merge before the
  fix lost it too. **An op of a former id that loses here** (superseded by a
  version that did not come after it in this device's own history, or covered by a delete of
  another version) keeps its Note text as a copy (`conflict_id` of the Note and the op's clock)
  or its list item as "Name (2)", or under its own name against the delete
  (`merge::former_own_lost`): the device that wrote it would have when that version replaced
  it, and that device is this one now. A version came after the op when it is of this device's
  id now, of the op's own id, or of a former id given up after the op's (`MergeTx::is_own_after`,
  by `seq`; the sync merge fix): an older former id's version can be another device's, as a
  copied database's original keeps the id the copy gave up. The cost: a copy no one needed when
  the other device had in fact seen it, or when an older former id's version did come after
  (only with a clock running ahead): safe.
- **`applied_up_to` stays**: the other devices' rows are what the restored data has merged of
  them, so catching up replays their files after those; the old own id's row (the last of its
  own files written) is now what it has merged of that id, another device's from now on.
- **The outbox is emptied**: its ops name the old id, and what they changed is in the rows,
  which the new id's join snapshot carries. `sync_held` stays and is tried again.
- The new id records nothing until it joins. **After a restore on a device that was syncing**
  (it recorded ops, or a restore's choice was already pending) `device.restore_choice` is set:
  `sync_now` returns `NeedsRestoreChoice` until `restore_choice` (on a device not syncing: a
  new id and no question):
  - **Catch up** (the default): it joins; its snapshot brings the restored rows (with their old
    clocks, so every later change in the folder wins, and rows deleted since stay deleted),
    and it merges everything in the folder after what the restored data had merged, its own
    old id's later files included.
  - **Replace**: `restamp_for_replace` first gives every synced row, item clock, the base of a
    list whose value is not in its shape, and the tombstone of every row not here one new
    clock, after every clock the data holds, even one far ahead (which the device's own clock
    does not follow), then it joins. The base of a list in its shape keeps its clock (S28c
    review): the re-stamped items go on top of it, and another device's item made since (later
    than the base) is a row the restored data never knew, kept; re-stamped, the base superseded
    every such item with no copy. The cost: a whole value not in its shape written after the
    Backup (on any device) stays the base, so the list stays damaged, its items (the restored
    ones too) held, until a value in shape is written; nothing is lost. Its snapshot
    wins everywhere: rows the restored data holds replace every other version, rows it had
    deleted are deleted everywhere, rows it never knew (made since on any device) are kept
    (ADR 0006 and rule 7 together), and another device's own Note text or list item it replaces
    or deletes stays there as a Sync conflict's copy or "Name (2)". The cost: an edit the
    restored data never saw, from a device whose clock runs ahead of the restorer's, can still
    win. The property tests found the "after every clock held" rule: with only the next clock,
    a snapshot covering a far-ahead device's files held rows older than that device's, and the
    devices that met it through the snapshot never got them.
- `Session::restore_backup` cannot reach the folder: edits made before a restore and not yet
  written are only in the Safety backup. S29's restore runs `sync_now` first when the folder is
  reachable.

### A sync (`sync_now`)

1. `sync.json` is read (written by the first device); one of a newer format stops here
   (`FolderNewer`).
2. A restore waiting for the user's choice stops here (`NeedsRestoreChoice`).
3. A copied database takes a new id, before it joins ([above](#restores-and-copies-new-device-ids)).
4. A device not recording ops joins ([above](#joining)).
5. Its pending ops are written (`write_pending_ops`), then its snapshot if one is due (none
   when an edit made meanwhile has an op in the outbox: [snapshots](#snapshots)).
6. `merge_folder`: held ops tried again, the former ids' snapshots (those it needs) and op
   files (the last given up first), the other snapshots, every other device's op files, held ops again. What the merge recorded (copies, renames, demotions, restored parents, items kept or
   renamed) is in the outbox and is written at once.
7. A snapshot if one is now due; its own op files that may go are trimmed; its `device.json`
   (name, platform, app version, `applied`, `synced_at`) is written.

### When sync runs

Rule 8 (`schedule.rs`): at launch, on resume, about 30 seconds after the first edit not yet
synced (`EDIT_DELAY_MS`; later edits do not push it back, so typing never holds a sync off),
every 5 minutes while the desktop app is open (`DESKTOP_PERIOD_MS`), and on demand. An edit
made while a sync ran (on another connection) may have missed it, so `synced(started,
finished)` keeps a sync due 30 seconds after the first edit since the sync started (after its
start, when an edit before it came first). A failed
sync is due again 30 seconds on if edits wait, else at the next period, resume or request. The
shells tell a `SyncSchedule` what happened and call `sync_now` when `next_sync_due` comes; the
Mobile app has no period (`at_launch(false)`).

## Evidence

- `crates/store/src/hlc.rs` (unit tests): the text form round-trips and refuses near misses;
  a clock with no text form is refused, not cut to fit; sorting the text sorts by (wall,
  counter, device), with the counter's overflow and two devices tying; the next clock passes
  the last when the wall clock steps back, and never comes at or below it at the largest wall
  time.
- `crates/store/tests/sync_clock_db.rs`: migration 13 → 14 from a database and from a Backup
  loses nothing; a Backup carries `item_clock` and `op_outbox`; `ensure_device` at `open` and
  `open_backup`, replacing a damaged device id; the clock passes every clock held after a
  reopen (wall clock stepped back, another device's clock in a tombstone a day ahead) and
  after a restore (one behind), but takes in no Backup's clocks a year ahead, no damaged
  clock and none more than a day ahead, keeping their rows; a clock merged in is observed
  within the session; the outbox only while recording, a reserved file coming back the same,
  only the next reserved file noted written, numbers going on across a reopen.
- `crates/store/tests/synced_writes_db.rs`
  (`every_write_of_synced_data_stamps_its_rows_with_clocks_and_records_its_ops`): every `Row`
  variant's write (an emptied Preferred order included) and its undo stamp the row, item or
  tombstone with the op's clock, and the outbox is exactly the stream of applied Changes.
- `crates/store/tests/sync_clock_db.rs` (S28b-1): migration 15's item positions and what
  each tombstone covers round-trip through a Backup; a v14 Backup restores with both unknown
  (each item at its index, each tombstone covering up to its own clock).
- `crates/store/tests/synced_writes_db.rs`
  (`a_delete_leaves_a_tombstone_kept_for_good_and_undoing_it_brings_the_row_back`).
- `crates/engine/src/sync/merge_tests.rs` (one per rule): last writer wins and keeps the op's
  clock; every merged clock observed, so a later local edit wins; one two days ahead not
  observed, its row kept, and a local edit of that row (and a Layout rename for a clash with
  it) stamped just after its clock, so a third device holds the same rows and clocks; edit
  beats delete in every arrival order (both ends and a third device each way); a delete that
  saw the newest edit deletes it in every order; a delete's tombstone and op carry what it
  covers; fields an op does not carry kept; Reading done-marks per Reading; a Collection
  renamed on one device and edited on another keeps both, in one order and with one clock;
  the Preferred order whole; two default Notebooks (named after the device, or "My Notebook
  (2)"); two Layouts of one name; the four-device Layout sequence above; a file needing
  another device's Notebook waits and merges after it; an own op file or one out of order is
  refused, and a failed merge notes nothing.
- `crates/store/tests/sync_clock_db.rs` (S28b-2): migration 16's Sync conflict copies,
  parents' tombstone rows, held ops and list bases round-trip through a Backup; a v15 Backup
  restores with none of them. `crates/store/src/preference_items.rs`: a renamed item keeps its
  other fields in order.
- `crates/engine/src/sync/merge_tests.rs` (S28b-2, one per decision): a Note's text edited on
  two devices keeps both, the loser's as a copy only its device makes; a delete that never saw
  this device's text keeps it as a copy, in both arrival orders; an older app's op without
  `conflict_of` keeps it; a deleted Notebook comes back with its Notes for a Note added
  meanwhile (on the deleter, the adder and a third device, nothing waiting); a parent restored
  for a child its maker then moved or deleted converges; a deleted plan and palette come back
  for a done-mark and a style; a child of a parent deleted before schema 16 is held until the
  parent comes back; two Collections of one name both stay; a damaged whole value holds later
  items on every device until one in shape puts them on top; an item after a whole value goes on
  top in any order; an op file with an unknown synced preference between two good ops holds it
  and merges the rest, and the next file too. From the S28b-2 review: a Collections save on a device with no tags
  keeps another device's earlier tags; an edit of an item that never saw this device's item of
  its name keeps both; a delete that never saw this device's list item of its name keeps it, in
  both orders; a parent's tombstone keeps the same row in every order of its deletes; a child of
  a held parent is held too and applies after it; a kept parent does not bring back a row this
  device's user deleted too; of a row's held writes only the newest is kept.
  `crates/store/tests/synced_writes_db.rs`: an empty list written where there was none changes
  nothing.
- `crates/engine/src/sync/merge_props.rs`: two or three in-memory devices make random edits
  (Notebooks, Notes, Highlights, Favorites, Layouts with clashing names, the Preferred order,
  Reading done-marks, a synced preference, Collections changed, added, removed and moved)
  and deletes, undo their own last change (a deleted row back with its old id), and sync at
  random. Each case runs under a fixed clock that moves on by random steps, each device's
  offset from it by its own amount, sometimes more than a day ahead (`MAX_FORWARD_SKEW_MS`),
  and with seeded ids, so a seed makes the same case on every run
  (`a_seed_makes_the_same_case_on_every_run` compares two runs' `synced_dump`). Once settled:
  every device holds the same `synced_dump` (rows, clocks, tombstones, item clocks, lists in
  order); every row is there exactly when its newest op write is later than what its deletes
  cover, holding that write; no row is there that no op wrote (or the devices began with);
  every tombstone holds the op stream's newest delete clock and newest version covered; each
  device has one default Notebook, the oldest (`created_at`, id) the devices began with. From
  S28b-2 they also delete Notebooks with their Annotations (or moving them to the default),
  Reading plans with their Readings, Highlight styles and palettes with Highlights on them,
  edit Note text on several devices at once, add Collections of one name on two devices, and
  write the Collections' whole value damaged and in shape again; and check that every Note text
  an op wrote is a live Note's (in place or a copy) unless an op that had it deleted its Note or
  wrote other text over it (a move does not count), that each copy as first written holds a text
  an op wrote to the Note its `conflict_of` names with the id `conflict_id` gives for that op's
  clock, that every Collection an add op wrote is live under some name unless an op that had it
  replaced or deleted it or the base supersedes it, that a Collection follows its list's base,
  and that an op is held only while its list is damaged. From the S28b-2 review they also save
  the Resource tags with the Collections as `Session::save_collections` does (`"{}"` where there
  are none), damage and mend the Collections more often, sync seeing only part of the folder (some
  devices' files, or their first few), and run the review's Q1 sequence (a Note's text edited on
  two devices, a third deleting it, the first meeting the delete first) and D7 sequence (a list
  damaged and mended, an item after the mend from a device whose clock runs ahead, a third device
  meeting the item first). The run must reach every
  outcome (`ParentKept` and `Held` included), a file retried, a Layout renamed, a default
  demoted, a moved item, an undo, a clock not observed, a Sync conflict's copy, a parent restored
  for a child, a row restored with its parent, a list item renamed, a held op applied, a damaged
  and a mended whole value, and a parent back after its child's maker moved or deleted the
  child, and both review sequences. 1,000 cases in CI (about 4 s), 10,000 under
  `--include-ignored` (about 30 s); the run prints how often each was reached. Mutation checks:
  the property tests find a missing copy (D2), a parent kept without being recorded again (D3),
  an item not held (D7), the copy on a delete missing (Q1), held items not put back on a merged
  base, the empty-list whole write (P1-1), D6 firing only on adds (P1-2) and an own list item
  deleted by a delete that never saw it; removing `restore_deleted_with` fails their "a row
  restored with its parent" reach check (no invariant can see it: a parent the child's maker
  left comes back without its rows) and two unit tests; a tombstone row compared with the
  tombstone's newest of all deletes, or with the tie-break flipped (still the same in every
  order), a held parent's child left waiting, and a kept parent bringing back a row deleted here
  too are found by their unit tests only.
- `crates/engine/src/sync/tests.rs`: ops round-trip byte for byte through `FsSyncFolder`;
  a truncated op file is skipped and read whole next time; a damaged one fails its length or
  hash; a newer one is left alone and asks for an update; temporary and other names are
  skipped and a gap stops reading; an op file naming another device is not read; a
  placeholder is read once downloaded; a reserved file already written after a stop is kept,
  and one there with other bytes is left and not noted written; a device not recording writes
  nothing; `sync.json` once, a newer one asking for an update, one still downloading neither
  read nor written over; `device.json` rewritten only by its own device; `FsSyncFolder`
  refuses bad paths (`:`, control characters …) and symbolic links in the folder, and lists
  iCloud placeholders by their real names.

- S28c: `crates/store/tests/snapshot_db.rs`: a snapshot holds no History, Last session,
  recent entries, device-only or unlisted preferences, remembered Resource names, outbox or
  former ids (no text of them anywhere in it), and its Synced data, device and `applied_up_to`;
  it reads back as a Backup with every row clocked, parents first; a join clocks every row and
  starts recording, and a second join changes nothing; no local string, nor a word of a Note
  deleted before the join, is anywhere in a snapshot's bytes. `sync_clock_db.rs`: a restore
  keeps every table but gives a new id (the one it had former, not another database's Backup's),
  empties the outbox and asks no question on a device that did not sync; on a syncing device
  the new id, former ids, empty outbox and pending choice arrive together, and a restore whose
  new-id step fails leaves the database as it was; the Backup's id is a former one only when it
  is this device's own; a v16 Backup restores into 17 (now 18); a name held for a damaged list is not
  free for a clash rename; migration 17 adds empty `former_device`.
- `crates/engine/src/sync/lifecycle_tests.rs`: two devices with `FsSyncFolder` on one disk
  converge end to end through `sync_now`, each with what it made before joining, and a third
  joins late from their snapshots; a late joiner from snapshots alone (every file they cover
  deleted) holds a parent's tombstone row, a list base, a Sync conflict's copy and a held op; a
  device's op files wait for its first snapshot; trimming deletes only files covered, 90 days
  old and merged by every device seen within 90 days (a device seen lately holds back what it
  has not merged; silent longer, it no longer does and catches up from the snapshot); a newer
  snapshot is left alone and asks for an update, a changed one is skipped, a newer folder is
  neither read nor written; a copied database takes a new id and catches up, and when the copy
  writes first the original does; a restore gives a new id, asks no question on a device not
  syncing, waits for the choice across a restart on a syncing one, and catches up; a restored
  database with Replace makes a third device converge to the restored state (rows it held back,
  rows it deleted deleted, a row made since kept) and keeps another device's own later text as
  a Sync conflict's copy; a text written under the old id survives a remote edit that never saw
  it, also when that edit is merged before the old id's op. From the S28c review: a former id's
  files merge op by op when the snapshot of a device met after the Backup covers them (its
  text kept); a Replace over a list base keeps another device's Collection made after the
  Backup everywhere; a database copied before either copy joins takes a new id, converges and
  trims none of the other copy's files; an idle device's damaged snapshot is written again
  (under the same name too). `schedule.rs`: launch, resume and request; 30 s after the first
  edit; an edit made while a sync ran still due; the desktop's 5 minutes; a failed sync.
- `merge_props.rs` (S28c): every device joins through the real join (its default Notebook and
  what it made before only in its snapshot), a third of them late, at their first sync; a
  quarter of the syncs run `sync_now` at a small policy (a snapshot every 1–3 own files or
  1–21 s, own files trimmed after 1–31 s), so devices merge each other's snapshots for files
  trimmed; devices take a Backup and later restore it, choosing catch up or replace; D3's
  sequence of a child's maker leaving it runs now and then. The invariants read an archive of
  every op file and snapshot ever written (`snapshot_ops` for the snapshots): convergence, rows
  exactly when not covered, every Note text and Collection kept unless an op that had it
  replaced it, or a replace discarded it on purpose (what the restoring device held that its
  Backup did not, of what its own ids' ops wrote after the Backup: another device's text or
  item must stay as a copy or "(2)"), held only while damaged. From the S28c review: a
  Collection another joined device held that a replace's Backup never knew (no row, no
  tombstone) stays, unless an op that had it, or a whole value an op or a join (not a
  replace's base in shape) wrote at or after it, took it, or the list ends damaged (held); and
  four sequences: a Backup taken before a device joins late, that device writing over the
  Backup's device's later text and then a snapshot covering it, the Backup restored catching
  up (`late_backup_step`); a database copied, joined or not, the first copy writing and the
  other taking a new id (`copy_step`); a Backup taken with the Collections on a base; an own
  snapshot damaged in the folder, whole again after its device's next sync (`mend_step`). The
  run must reach a late join, a snapshot merged, one merged for files trimmed, a file
  trimmed, a catch-up, a replace and each of those four. Mutation checks: the former ids not
  counted as own (the props, two unit tests), the replace's clock not after every clock held
  (the props), a device met through another's snapshot still waiting for its own (a unit
  test), a former id's losing op not kept (a unit test: the props merge former ids first,
  which leaves the case to partial views); from the review, a snapshot moving a former id's
  `applied_up_to` (the props' late Backup, a unit test), Replace re-stamping a base in shape
  (the props' never-known check, a unit test), the copied-database check only on a recording
  device (the props' copy, a unit test), a damaged own snapshot not written again (the props'
  mend, a unit test), a clash rename taking a held name (a 10k seed, a store test). 1,000
  cases take about 15–23 s on a loaded Mac (16.6 s before the review's sequences on the same
  load; the copies add devices, and each `sync_now` reads its own newest snapshot), 10,000
  about 145–190 s.

- **The sync merge fix** (after S29b; the merge property test lost a Collection at
  `OBS_MERGE_SEED=20266453`, there since S28c): six distinct losses of user data, each shrunk
  to a test that failed before its fix. `merge_tests`:
  `an_own_list_item_held_while_its_list_is_damaged_survives_a_later_item_of_its_name` (20266453:
  `hold` dropped an own held write for a newer one),
  `an_own_items_re_add_comes_after_the_version_it_keeps_so_a_later_base_keeps_it` (126745: a
  far-ahead edit's "(2)" stamped below a base merged later),
  `a_delete_with_no_before_covering_this_devices_own_item_deletes_it` (3146456: a snapshot's
  delete counted unseen). `lifecycle_tests`:
  `a_snapshots_delete_covering_this_devices_own_item_deletes_it` (3146456 whole, a restore),
  `a_conflict_copy_edited_before_a_restore_keeps_its_edit_when_catching_up` (5007792: a snapshot
  merged before the former id's files re-made a copy that beat its edit),
  `a_copys_edited_conflict_copy_survives_a_restore_catching_up` and
  `…_when_the_originals_id_sorts_last` (7744: an older former id's files, a copy's original's,
  merged first; the second holds the order by `seq`, not by id),
  `a_copys_text_survives_a_restore_when_the_original_wrote_over_it` (891: an older former id's
  version counted as after the op). `sync_clock_db.rs`:
  `former_ids_keep_the_order_they_were_given_up_through_migration_18_restores_and_backups`.
  `merge_props.rs` runs the first four sequences now and then
  (`held_own_item_step`, `far_base_after_re_add_step`, `snapshot_delete_step`,
  `edited_copy_step`; per 1,000 cases about 196, 194, 86 and 48) and must reach each. Mutation
  checks: undoing the held-write fix, the re-add's stamp or the former-ids-first order fails the
  1,000-case run; the delete with no `before` is caught by its unit test only (former ids first
  now keeps the restore sequence from reaching it). After the fixes 10,000-case runs from 20
  seeds in a row (1, 7, 42, 99, 777, 1234, 4321, 65536, 100000, 2000000, 3000000, 7000000,
  8000000, 9000000, 10000000, 11111111, 22222222, 33333333, 44444444, 20261122) found nothing;
  1,000 cases take about 19 s, 10,000 about 250 s on the owner's Mac with four runs at once.

- `crates/engine/src/sync_session_tests.rs` (S29): without a Sync folder nothing syncs, shows
  or is recorded, and a restore leaves it so; two Sessions converge through one folder and list
  each other; an edit makes a sync due 30 seconds on, else the period; a restore on a syncing
  device syncs first, keeps its folder (a Backup from before it synced) and Catch up brings the
  edit back, across a restart; Replace makes the restored data every device's; a restore never
  takes the Backup's folder; Stop syncing records nothing more and choosing again joins as a new
  device; a Sync conflict's copy is in Needs attention with its four actions, each undone in
  one step, the other device taking Keep this version, Keep both and Combine; a copy whose
  Note another device deleted offers Keep this text and Let it go; a Note editor's save after
  a merge changed its Note keeps both texts (into a new copy, or the merge's copy of its
  text); the restore prompt comes back after a restart until it is answered; held ops listed with why; a folder not
  there and one a newer app set up. S29b: no call of the Sync folder is on the UI thread
  (through a restore and its Catch up); edits made while a sync runs (it held reading another
  device's op file) are kept and reach the other device; choosing a folder and Stop syncing
  wait for the sync running, "Syncing…" meanwhile; a worker that panics is a failed sync and
  the next syncs; a restore whose sync first is held goes ahead after its wait, says so, and
  that sync, let go, changes neither the restored data nor the folder (failing without both
  the commit hook and the folder's refusal). From S29b's review: Stop syncing or Choose
  folder waiting for a held sync when a restore starts stays applied, whether the sync ends in
  the restore's wait or is abandoned; Stop syncing abandons a held sync after its wait and
  stops, the sync let go writing nothing; an edit made while `snapshot_due` reads the device's
  snapshot skips the snapshot, not the sync (failing before with "a snapshot is written only
  once every op is in an op file"); a worker keeps a fixed clock of the Session's thread.
  `measure_syncs_off_the_ui_thread` (ignored) measures, the frame after the result's too.
- `crates/store/tests/sync_clock_db.rs` (S29b): two connections on two threads, the wall
  clock standing still, hand out 400 clocks once each, increasing on each, all in the outbox,
  the first's next passing a clock the second observed; a new id taken on the second is the
  first's next clock's; a second connection is refused at another schema and where there is
  no database; once its writes are refused it commits nothing (an `apply_changes`, one
  statement, a new id, a connection opened after), the first writing as before; a write on the
  first waits only for the second's transaction running (failing with SQLite's own busy
  wait).
- `crates/ui-egui/src/sync_tests.rs` (S29): the same through the harness (Choose Sync folder…
  with the folder dialog injected, the indicator opening Settings on Sync, the prompt's two
  buttons, Needs attention's buttons, an autosave after a merge keeping both texts and typing
  on in the copy, Combine clicked while typing in the copy); the gallery's Sync goldens.
- `cargo xtask sync-e2e` (S29, `xtask/src/sync_e2e.rs`): the real app twice, each with its own
  `OBS_USER_DB`, through one temporary folder: they converge, and a restored Backup's prompt
  takes Catch up and Replace, photographed. Steps that follow frame-numbered commands wait on a
  file (`after_file`: A's Backup; for B, a file the xtask writes once B's Word forms op is in
  the folder), so a slow software renderer cannot reach them first; B must stop with nothing
  waiting to sync.

## For later steps

- **Split the largest merge transactions** (found by S29b): a snapshot, or an op file of
  thousands of ops (a device that made 10,000 changes between syncs), merges in one
  transaction, 0.5–0.9 s for 10,000 Notes on the owner's Mac; an edit the user makes
  meanwhile waits for it on the UI thread. Past `BUSY_TIMEOUT` (10 s) the edit is refused
  ("database is locked") rather than waiting longer: a larger library or an iPhone could get
  there, and the restore's copy into the live database waits up to that long for an abandoned
  worker's transaction still running. Merging them in bounded transactions keeps edits under a
  frame and far from that limit, but touches the merge's all-or-none contract
  (`merge_snapshot`, `merge_op_file` with its `applied_up_to`).
- **The views' reads after a large merge** (found by S29b's review): the frame after a sync
  that merged anything re-reads every Annotation in text and the Notes Tool's cards, 17–24 ms
  at 10,000 Notes (dev profile), as after any Annotation edit; reading only what changed would
  keep it under a frame.
- **S30:** Needs attention lists the Sync conflicts and held ops as the desktop does
  (`texts`, `Session::sync_held_lines`, `resolve_sync_conflict`); render `SyncState`
  (`needs_newer_app` per device, `FolderNewer`, `NeedsRestoreChoice` with Catch up as the
  default and Replace), let the user name the device, drive `SyncSchedule` (`at_launch(false)`),
  and implement `SyncFolder` in Swift, on S29b's off-thread path: the Swift folder (an
  `NSFileCoordinator`-backed `SyncFolder`, `Send + Sync`) is given to the worker in place of
  `FsSyncFolder` (S30 adds the public Session hook that sets the worker's `FolderMaker`; today
  it is crate-private, the desktop's path and tests' folder), and `obs-mobile` calls
  `Session::sync_tick` as the desktop's frames do, so no coordinated read or placeholder
  download is on the main thread. The words are `texts`'s.
