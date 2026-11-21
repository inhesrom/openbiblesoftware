# Sync: the Sync folder's files and the merge (ADR 0022) — draft

Draft from plan steps S28a (the file layer) and S28b-1 (the merge, [below](#merging)). Sync
conflicts of Note text and the rules for deleted parents (S28b-2), snapshots, trimming,
device-id rules, restore and scheduling (S28c), and the Settings → Sync screens (S29, S30)
come later and finish this page. Terms are in
[CONTEXT.md](../../CONTEXT.md): **Sync folder**, **Synced device**, **Synced data**,
**Sync conflict**.

| Layer | Piece | Role |
|---|---|---|
| `obs-store` | `hlc.rs` | `Hlc`: the hybrid logical clock, its text form, `Hlc::next`, `MAX_FORWARD_SKEW_MS`. Compiles everywhere. |
| `obs-store` | `sync_clock.rs` | From schema 14, `apply_changes` gives each `Change` the next clock and writes it into the row, item or tombstone; while this device records ops, keeps each `Change` with its clock in `op_outbox`. `device_id`, `observe_clock`, `start_recording_ops`, `records_ops`, `reserve_op_files`, `op_file_written`. |
| `obs-store` | `sync_schema.rs` | Migration 13 → 14 (`item_clock`, `op_outbox`) and 14 → 15 (`tombstone.covers`, `item_clock.position`); `ensure_device` at `open` and `open_backup` (a missing or damaged device id gets a new one). |
| `obs-store` | `merge_store.rs` | `UserStore::merge_op_file`: one op file merged in one transaction with its `applied_up_to`; `MergeTx` (`state`, `write_merged`, `note_delete`, `note_item_op`, `write_kept`, `record`); `row_state`, `applied_up_to`. |
| `obs-engine` | `sync/folder.rs` | `SyncFolder` (`list`, `read`, `write_new`, `remove`, `ensure_available`), `FsSyncFolder` (`std::fs`, the desktop's), `MemSyncFolder` (tests, feature `test-support`). The Mobile app implements `SyncFolder` in Swift (S30). |
| `obs-engine` | `sync/clock.rs` | Re-exports `obs_store::Hlc`: the store stamps rows itself (its own writes, such as `save_layout`, plan their Changes), so the clock lives there. |
| `obs-engine` | `sync/ops.rs` | `Op`, `encode_op_file`, `decode_op_file`, `read_ops_after`, `write_pending_ops`. |
| `obs-engine` | `sync/merge.rs` | The merge rules (`merge_op`) and `merge_folder`, which merges every other device's op files not merged yet. |
| `obs-engine` | `sync/mod.rs` | Paths, `sync.json` (`ensure_sync_json`), `device.json` (`write_device_json`, `read_devices`), `SyncError`. |

## The folder

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
| `Open Bible Sync/devices/<device-id>/device.json` | That device | Yes, whole (temp + rename), by that device only, when its name, platform or app version changes; the only file any device writes again | `{"format":1,"device":"<id>","name":"…","platform":"macos","app_version":"0.x.y"}` |
| `Open Bible Sync/devices/<device-id>/ops/<seq>.jsonl` | That device | Never | One batch of its ops ([below](#op-files)) |
| `Open Bible Sync/devices/<device-id>/snapshots/` | That device | — | S28c |

No file is written by two devices, so providers never make conflict copies of ours.
`write_new` writes to a temporary name in the same folder that starts with `.`
(`.<name>.<uuid>.tmp` on desktop) and renames it into place, so a reader never sees part of a
file under its real name. `FsSyncFolder` flushes the file (`sync_all`) before the rename and,
on unix, the folder after it, so a file noted written survives a power loss. JSON files are compact JSON followed by one newline. Readers ignore
fields they do not know (`device.json` from a later app still reads).

A `sync.json` whose `format` is newer than this build's (`SYNC_FORMAT`, 1) is left alone and
`ensure_sync_json` returns `FolderFormat::Newer`: the app asks the user to update (ADR 0022
rule 6). One that is not JSON is left alone too (`SyncError::Damaged`). One listed but only
in the cloud is neither read nor written over: `ensure_available` starts its download and
`ensure_sync_json` returns `SyncError::NotDownloaded`, for the next sync to read it.

## Clocks

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

## Recording ops

A device records ops while `applied_up_to` has a row for its own device id; that row's `seq`
is the last of its own op files written. `start_recording_ops` adds it with `seq` 0 (S28c's
join calls it after writing its snapshot). While recording, `apply_changes` writes each
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
   write stops with `SyncError::Damaged`, noting nothing written (an op file never changes once
   written).
3. `op_file_written(seq)` (one transaction): its rows leave the outbox and the own `seq`
   moves on. Only a reserved `seq` at most one after the own `seq` is accepted
   (`StoreError::Rejected` otherwise), so the own files never skip a number.

A stop between 2 and 3 (the app quits, the folder fails) leaves the file reserved; the next
try finds it there with the same bytes and notes it written, and edits made in between go in
the next file (`sync::tests::a_reserved_op_file_already_written_after_a_stop_is_kept_as_it_is`).

## Op files

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

## Merging

`merge_folder(user, folder)` merges every other device's op files after its
`applied_up_to`, in order (`read_ops_after`). Each file is merged by
`UserStore::merge_op_file` in one transaction that also moves `applied_up_to` on: all of it
or none. A file with an op that needs a row not here yet (another device's file brings it:
an Annotation's Notebook, a Highlight's new style, a style's palette, a Reading's plan)
**waits**: it is rolled back, the other devices' files are merged, and it is tried again, pass
after pass while any file merges, then at the next sync (`MergeReport::waiting`, `retried`).
No two files can wait for each other: a file only needs rows from files written before it.
Every op's clock is observed (`observe_clock`), so an edit made here afterwards is later than
anything merged; one more than a day ahead is not observed (`MergeReport::not_observed`) and
its row merges as any other.

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
| Delete | A Notebook with Annotations, a palette with styles, a style with Highlights, a plan with Readings | Kept, so nothing is lost (`ChildrenKept`); S28b-2 settles these (D3) |
| Delete | This device's default Notebook | Waits for the older default Notebook that replaces it |

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

**Edits the merge records.** The demotion of an own default Notebook and a Layout rename are
applied with `MergeTx::record` like any local edit: a new clock, after every clock merged, and
into the outbox while recording, written at the next sync.

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
  observed, its row kept; edit beats delete in every arrival order (both ends and a third
  device each way); a delete that saw the newest edit deletes it in every order; a delete's
  tombstone and op carry what it covers; fields an op does not carry kept; Reading done-marks
  per Reading; the Preferred order whole; two default Notebooks (named after the device, or
  "My Notebook (2)"); two Layouts of one name; the four-device Layout sequence above; a file
  needing another device's Notebook waits and merges after it; a parent delete meeting live
  children is skipped; an own op file or one out of order is refused, and a failed merge
  notes nothing.
- `crates/engine/src/sync/merge_props.rs`: two or three in-memory devices make random edits
  (Notebooks, Notes, Highlights, Favorites, Layouts with clashing names, the Preferred order,
  Reading done-marks, a synced preference, Collections) and deletes, and sync at random; once
  settled, every device holds the same `synced_dump` (rows, clocks, tombstones, item clocks,
  lists in order), and every row is there exactly when its newest op write is later than what
  its deletes cover, holding that write. 1,000 cases in CI (about 2.5 s), 10,000 under
  `--include-ignored` (about 22 s); the run prints how often each rule was reached.
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

## For later steps

- **S28b-2:** Note-text Sync conflicts (D2: a `conflict_of` Note with a deterministic id, made
  only by the device whose own edit lost, as an ordinary recorded Change; in a rare
  three-device chain, where a device gets the third device's edit before the second's, it can
  make a copy no one needed, which is safe: nothing is lost); deleted parents (D3, the deletes
  `ChildrenKept` skips today must be settled so every device converges); two list items of
  one name (D6); a list's whole value against its items (D7, `sync_held`). A whole-value
  `Row::Preference` of a list key leaves that list's `item_clock` clocks as they were and
  clears their positions.
- **S28c:** join = write the snapshot, then `start_recording_ops`; a restore gives the device a
  new id (rule 7), and the outbox rows written under the old id must be re-homed or cleared;
  `applied_up_to`'s own row is the copied-database check: op files under this id above
  `max(own seq, MAX(op_outbox.file_seq))` were not written here (a file above the own `seq`
  but still reserved was written here and not yet noted written). From S28b-1: `sync_now`
  is `write_pending_ops`, then `merge_folder`; the merge can record edits of its own (a
  default Notebook's demotion, a Layout rename), which go out at the next write, so a sync
  that recorded any should write again. A snapshot (and merging one, rule 5) must carry
  tombstones with `covers` and item clocks with `position`, and merge rows by the same table
  as ops; a device's own default Notebook reaches the others through it (the property tests
  stand in for it by writing each device's default as an op when it joins).
