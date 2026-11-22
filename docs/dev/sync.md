# Sync: the Sync folder, the merge and the lifecycle (ADR 0022)

The Rust side of Sync, from plan steps S28a (the file layer), S28b-1 and S28b-2 (the merge,
[below](#merging)) and S28c (snapshots, trimming, device ids, restores, when sync runs:
[the contract](#contract)). The Settings → Sync screens are S29 (desktop) and S30 (iOS); they
render the states below and hold no rule. Terms are in [CONTEXT.md](../../CONTEXT.md):
**Sync folder**, **Synced device**, **Synced data**, **Sync conflict**.

| Layer | Piece | Role |
|---|---|---|
| `obs-store` | `hlc.rs` | `Hlc`: the hybrid logical clock, its text form, `Hlc::next`, `MAX_FORWARD_SKEW_MS`. Compiles everywhere. |
| `obs-store` | `sync_clock.rs` | From schema 14, `apply_changes` gives each `Change` the next clock and writes it into the row, item or tombstone; while this device records ops, keeps each `Change` with its clock in `op_outbox`. `device_id`, `observe_clock`, `start_recording_ops`, `records_ops`, `reserve_op_files`, `op_file_written`. |
| `obs-store` | `sync_schema.rs` | Migration 13 → 14 (`item_clock`, `op_outbox`), 14 → 15 (`tombstone.covers`, `item_clock.position`), 15 → 16 (`annotations.conflict_of`, `tombstone.row` with `row_covers` and `row_clock`, `sync_held`) and 16 → 17 (`former_device`, `device.restore_choice`); `ensure_device` at `open` and `open_backup` (a missing or damaged device id gets a new one). |
| `obs-store` | `sync_device.rs` | ADR 0022 rule 7: `own_device_ids` (this id and the former ones), `take_new_device_id` (a copied database), `restore_choice_pending`, `clear_restore_choice`, `device_name`, `set_device_name`, `merged_devices`, `last_reserved_op_file`; `renew_device_id`, which `UserStore::restore_from` (`backup_store.rs`) runs after every restore. |
| `obs-store` | `snapshot_store.rs` | ADR 0022 rule 5: `snapshot_bytes`, `open_snapshot`, `snapshot_contents` (`SnapshotContents`), `merge_snapshot`, `merged_up_to`, `has_pending_ops`, `join_sync`, `restamp_for_replace`, `MergeTx::keep_base_positions`; `SNAPSHOT_EMPTIED`. |
| `obs-store` | `merge_store.rs` | `UserStore::merge_op_file`: one op file merged in one transaction with its `applied_up_to`; `UserStore::merge_held`: the held ops tried again in one transaction; `MergeTx` (`state`, `write_merged`, `delete_merged`, `note_delete`, `note_item_op`, `note_list_op`, `write_kept`, `record`, `begin_op` / `end_op`, `hold` / `unhold` / `held`, `list_base`, `list_in_shape`, `write_list_base`, `tombstone_row`, `renamed_for_clash`); `row_state`, `applied_up_to`, `sync_held`; `UserStore::sync_conflicts` (`annotation_store.rs`). |
| `obs-engine` | `sync/folder.rs` | `SyncFolder` (`list`, `read`, `write_new`, `remove`, `ensure_available`), `FsSyncFolder` (`std::fs`, the desktop's), `MemSyncFolder` (tests, feature `test-support`). The Mobile app implements `SyncFolder` in Swift (S30). |
| `obs-engine` | `sync/clock.rs` | Re-exports `obs_store::Hlc`: the store stamps rows itself (its own writes, such as `save_layout`, plan their Changes), so the clock lives there. |
| `obs-engine` | `sync/ops.rs` | `Op`, `encode_op_file`, `decode_op_file`, `read_ops_after`, `write_pending_ops`. |
| `obs-engine` | `sync/merge.rs` | The merge rules (`merge_op`) and `merge_folder`, which tries the held ops again, merges the snapshots it needs and every other device's op files not merged yet. |
| `obs-engine` | `sync/snapshot.rs` | Snapshot names, `write_snapshot`, `read_snapshot`, `snapshot_ops`, `merge_snapshot`, `snapshot_due` (`SnapshotPolicy`), `trim_own_op_files`. |
| `obs-engine` | `sync/lifecycle.rs` | `sync_now`, `needs_restore_choice`, `restore_choice` (`RestoreChoice`), `SyncOptions`, `SyncReport`, `SyncState`; the copied-database check. |
| `obs-engine` | `sync/schedule.rs` | `SyncSchedule` and `next_sync_due` (ADR 0022 rule 8). Compiles everywhere. |
| `obs-engine` | `sync/mod.rs` | Paths, `sync.json` (`ensure_sync_json`), `device.json` (`write_device_json`, `read_devices`), `SyncError`. |

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

`merge_folder(user, folder)` first merges the [snapshots](#snapshots) it needs (of a device
not met yet, or whose op files after the last merged here are gone), then every other
device's op files after its `applied_up_to`, in order (`read_ops_after`). Each file is merged by
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
| Delete (of another version of that name) | This device's own Collection, Copy preset or Saved search, which the delete had not seen (its `before_clock` is not this row's clock, its `before` JSON another) | Kept: the delete noted, this device's item recorded again under its own name, after what the tombstone covers (edit beats delete, [list items of one name](#two-list-items-of-one-name)) (`EditKept`) |
| Write | A list item at or before its list's base (its newest whole-value write) | Nothing (`Superseded`) ([a list's whole value](#a-lists-whole-value)) |
| Write, losing (`Superseded`, `Covered`) | An op of this device's former id, against a version not this device's own | Its Note text recorded as a copy, its list item as "`name` (2)" or again under its name ([restores and copies](#restores-and-copies-new-device-ids)) |
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
item's list base), and into the outbox while recording, written at the next sync. Each op is
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
"`name` (2)", "`name` (3)" … among the list's items here, at its old position, every other
field of its JSON as it was (`preference_items::renamed_item`). The device whose item won does
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
"`name` (2)"; safe, and every device converges on it.

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
(`merge_tests::of_a_rows_held_writes_only_the_newest_is_kept`). `merge_folder` skips the
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
| Every synced table with its clocks, `anchors`, `notes_fts`; `annotations.conflict_of` | `history`, `last_session`, `resource_names` |
| `tombstone` whole: `covers`, a parent's `row` with `row_covers` and `row_clock` | `op_outbox` (written as op files first), `former_device` |
| `item_clock`: items' clocks and positions, the lists' bases (`'preference'`, key) | Every preference that is not Synced data (display preferences, Search history and recent searches, the Command box's recent entries …) |
| `device` (the writer's id; `restore_choice` 0), `applied_up_to`: **the op files it covers**, for each device the last merged, for its own id the last written | |
| `sync_held`: ops of files it covers that it could not apply (a joiner starting from it skips those files, so they would be lost otherwise; S28c corrected S28b-2's plan to leave it out) | |

A device writes one **only with an empty outbox**, right after `write_pending_ops`, so it
covers exactly its own op files up to `seq`. Its first is the join's (`seq` 0 when it joins
with nothing written); later ones when `snapshot_due` says (`SnapshotPolicy`: 100 own op files
since its newest, or the first of them 30 days old). Writing one deletes its older snapshots.

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
snapshot covers, so a new device replays only the op files after it. Each list's
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
wrote its files (`sync_device.rs`, migration 17):

- **A restore** (`UserStore::restore_from`, so also Undo restore and every restore the app
  does) always gives the device a new UUIDv7. **A copied database**: `sync_now` finds op files
  or a snapshot under its id numbered above both the last of its own op files written and the
  last reserved (`last_reserved_op_file`: after a stop between writing a file and noting it,
  the file is above the first but still reserved), or `write_pending_ops` finds a reserved own
  file there with other ops (`OwnFileDiffers`: the copy wrote first); it takes a new id
  (`take_new_device_id`) and catches up, with no question asked.
- **The ids it had** go to `former_device` (a restore adds the Backup's id and the one it had
  before), and every "own" check of the merge counts a clock naming any of them as this
  device's own (`own_device_ids`, `MergeTx::is_own`): a text written under the old id that a
  remote edit never saw is still kept as a copy. Two devices claiming one id (another device's
  Backup restored, a copy) make the same copy (its id is derived) and the same "(2)", which
  converge. A former id's op files are this device's own history, so `merge_folder` merges them
  before every other device's; and **an op of a former id that loses here** (superseded by a
  version that is not this device's own, or covered by a delete of another version) keeps its
  Note text as a copy (`conflict_id` of the Note and the op's clock) or its list item as
  "Name (2)", or under its own name against the delete (`merge::former_own_lost`): the device
  that wrote it would have when that version replaced it, and that device is this one now. The
  cost: a copy no one needed when the other device had in fact seen it (safe).
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
  - **Replace**: `restamp_for_replace` first gives every synced row, item clock, list base and
    the tombstone of every row not here one new clock, after every clock the data holds, even
    one far ahead (which the device's own clock does not follow), then it joins. Its snapshot
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
3. A copied database takes a new id ([above](#restores-and-copies-new-device-ids)).
4. A device not recording ops joins ([above](#joining)).
5. Its pending ops are written (`write_pending_ops`), then its snapshot if one is due.
6. `merge_folder`: held ops tried again, snapshots, every other device's op files, held ops
   again. What the merge recorded (copies, renames, demotions, restored parents, items kept or
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
  starts recording, and a second join changes nothing. `sync_clock_db.rs`: a restore keeps
  every table but gives a new id (the Backup's and the old one former), empties the outbox and
  asks no question on a device that did not sync; migration 17 adds empty `former_device`.
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
  it, also when that edit is merged before the old id's op. `schedule.rs`: launch, resume and request; 30 s after the first edit; the desktop's
  5 minutes; a failed sync.
- `merge_props.rs` (S28c): every device joins through the real join (its default Notebook and
  what it made before only in its snapshot), a third of them late, at their first sync; a
  quarter of the syncs run `sync_now` at a small policy (a snapshot every 1–3 own files or
  1–21 s, own files trimmed after 1–31 s), so devices merge each other's snapshots for files
  trimmed; devices take a Backup and later restore it, choosing catch up or replace; D3's
  sequence of a child's maker leaving it runs now and then. The invariants read an archive of
  every op file and snapshot ever written (`snapshot_ops` for the snapshots): convergence, rows
  exactly when not covered, every Note text and Collection kept unless an op that had it
  replaced it, or a replace discarded it on purpose (what the restoring device held that its
  Backup did not), held only while damaged. The run must reach a late join, a snapshot merged,
  one merged for files trimmed, a file trimmed, a catch-up and a replace. Mutation checks: the
  former ids not counted as own (the props, two unit tests), the replace's clock not after
  every clock held (the props), a device met through another's snapshot still waiting for its
  own (a unit test), a former id's losing op not kept (a unit test: the props merge former ids
  first, which leaves the case to partial views). 1,000 cases take about 10 s (was 4: each join writes a snapshot and every
  other device reads it), 10,000 about 95 s.

## For later steps

- **S29, S30:** Needs attention lists `UserStore::sync_conflicts()` (a copy's `conflict_of`
  names the Note in place) and `UserStore::sync_held()` (each with why); sync must not be turned
  on for users before Needs attention shows the copies, which until then look like ordinary
  Notes.
- **S29, S30:** render `SyncState` (`needs_newer_app` per device, `FolderNewer`,
  `NeedsRestoreChoice` with Catch up as the default and Replace), let the user name the device
  (`set_device_name`), drive `SyncSchedule`, and implement `SyncFolder`. A restore on a
  syncing device runs `sync_now` first when the folder is reachable, so edits never written
  reach it before the restore replaces them.
