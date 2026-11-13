# Folder sync for a SQLite User database (#326 research)

How the one User database (Notebooks, Annotations and Anchors, Highlight styles and palettes,
preferences, Layouts, Favorites, History, Reading plans: `crates/store/src/user.rs`) could
stay in step between the desktop app (macOS, Windows, Linux) and the planned **Open Bible**
phone and tablet apps, through a cloud folder the user picks (iCloud Drive, Google Drive,
Dropbox, OneDrive, Syncthing). We run no server. Every claim below names the source that owns
it. Checks ran from this project's build host on **2026-11-10** (GitHub and crates.io API
timestamps match that date). Where a source is a forum post or Q&A answer and not the vendor's
own documentation, it is marked *(secondary)*.

## 1. Why the live User database cannot sit in the synced folder

| Fact | Source |
|---|---|
| WAL needs every process on one host: "All processes using a database must be on the same host computer; WAL does not work over a network filesystem", because the wal-index is in shared memory. The User database opens in WAL (`user.rs`, `journal_mode = WAL`). | [sqlite.org/wal.html](https://www.sqlite.org/wal.html) §"Advantages and disadvantages", §"Concurrency" |
| A copy taken mid-transaction "might contain some old and some new content, and thus be corrupt". A sync client copying the file whenever it changes does exactly this. | [howtocorrupt.html §1.2](https://www.sqlite.org/howtocorrupt.html) |
| The `-wal` / `-journal` file must travel with the database. "Copying a database file without also copying its journal" and "overwriting a database file with another without also deleting any hot journal" both corrupt. A sync client moves files one at a time, in no guaranteed order. | [howtocorrupt.html §1.3–1.4](https://www.sqlite.org/howtocorrupt.html) |
| SQLite relies on file locks, and network filesystems' locks "have been known to operate incorrectly … This has led to database corruption." Sync clients take no SQLite lock at all. | [useovernet.html](https://www.sqlite.org/useovernet.html), [howtocorrupt.html §2.1](https://www.sqlite.org/howtocorrupt.html) |
| Safe ways to copy a live database: the backup API, `VACUUM INTO`, or `sqlite3_rsync`. ADR 0013's Backups already use the backup API. | [howtocorrupt.html §1.2](https://www.sqlite.org/howtocorrupt.html), ADR 0013 |
| Zotero, another SQLite-backed app, says a data directory in Dropbox, Google Drive or OneDrive is "extremely likely to corrupt your database". The sync client may pull a change into part of the file while the app has it open. "Even if a cloud storage tool did honor file locks, you would simply end up with two conflicted copies that were impossible to merge." | [Zotero KB](https://www.zotero.org/support/kb/data_directory_in_cloud_storage_folder) |

**So:** the live `.sqlite` file (and its `-wal`/`-shm`) stays in app-private storage on every
device. Only files that are written whole and then left alone go into the shared folder.

## 2. How each provider reaches the app, per OS

| Provider | macOS | Windows | Linux | iOS / iPadOS | Android |
|---|---|---|---|---|---|
| **iCloud Drive** | Native folder. Files can be offloaded: Obsidian advises **Keep Downloaded** (macOS 15+) or turning off *Optimize Mac Storage* ([Obsidian help](https://github.com/obsidianmd/obsidian-help/blob/master/en/Getting%20started/Sync%20your%20notes%20across%20devices.md)) | iCloud for Windows exists. Obsidian warns it "may lead to file duplication or corruption" (same source) | No Apple client found *(gap)* | Folder picker works for iCloud Drive and "On My iPhone" (see row below) | No client found *(gap)* |
| **Google Drive** | Drive for desktop, macOS 13+; a *virtual drive* that **streams** (placeholders) or **mirrors** ([Google support 2375082](https://support.google.com/drive/answer/2375082?hl=en), [10838124](https://support.google.com/drive/answer/10838124?hl=en)) | Drive for desktop, Win 10+ 64-bit (same sources) | "The desktop version isn't available" ([2375082](https://support.google.com/drive/answer/2375082?hl=en)) | Obsidian lists it as not supported on iOS (Obsidian help) | The Drive app does not mirror a folder locally. Obsidian sends users to third-party two-way sync apps (Obsidian help) |
| **Dropbox** | Desktop app; **online-only** placeholders are possible ([Dropbox help](https://help.dropbox.com/sync/make-files-online-only)) | Same | Official Linux app: Ubuntu 18.04+ / Fedora 28+, x86-64 only, "doesn't support ARM processors for Linux" ([requirements](https://help.dropbox.com/installs/system-requirements)) | Not supported on iOS for Obsidian. The Dropbox picker "doesn't support opening folders" (Dropbox staff reply, community forum, *secondary*, search snippet only) | Has a SAF provider, but Obsidian recommends Dropsync/FolderSync instead (Obsidian help) |
| **OneDrive** | Sync app, macOS 15.8+; Files On-Demand placeholders ([MS system reqs](https://support.microsoft.com/en-us/onedrive/onedrive-system-requirements), [release notes](https://learn.microsoft.com/en-us/sharepoint/sync-release-notes)) | Sync app, Windows 10 22H2+ (same) | No Linux client listed (same) | "isn't officially supported" for Obsidian vaults on iOS (Obsidian help) | "may not function well for Android syncing" (Obsidian help) |
| **Syncthing** | Official binaries | Official | Official | No official app. Obsidian lists it as unsupported on iOS | The official app is **discontinued**: last release 1.28.1 on 2024-12-03, repo archived ([README](https://github.com/syncthing/syncthing-android/blob/main/README.md)). The community **Syncthing-Fork** continues (Obsidian help) |

### Platform mechanics

- **iOS: picking a folder.** `UIDocumentPickerViewController` with the folder type returns a
  *security-scoped URL*. That URL lets the app "recursively access the directory and all of
  its contents", and the app can save it as a bookmark to reopen next launch. Every access
  must sit between `startAccessingSecurityScopedResource()` and `stop…`, and reads and writes
  should go through `NSFileCoordinator`. Apple warns that access can fail later, "especially
  … when saving and resolving bookmarks", and that a bookmark can come back *stale*
  ([Providing access to directories](https://developer.apple.com/documentation/uikit/providing-access-to-directories)).
  An unbalanced `start…` call "leaks kernel resources" until the app is relaunched
  ([startAccessingSecurityScopedResource](https://developer.apple.com/documentation/foundation/nsurl/startaccessingsecurityscopedresource())).
- **iOS: third-party providers in the folder picker.** Developers report that Dropbox, Google
  Drive and other third-party File Provider roots are **greyed out** when picking a folder;
  only iCloud Drive and On My iPhone can be chosen
  ([Apple Dev Forums 691738](https://forums.developer.apple.com/forums/thread/691738),
  [Stack Overflow 58262986](https://stackoverflow.com/questions/58262986), both *secondary*,
  search snippets only). Obsidian's and Logseq's guidance agrees: Logseq says "Most 3rd-party
  sync services cannot be accessed by Logseq"
  ([Logseq FAQ](https://discuss.logseq.com/t/im-using-logseq-with-icloud-but-experiencing-data-loss-or-file-conflict-errors-whats-going-on/13393)).
- **iOS: iCloud placeholders.** A file may not be on the device yet.
  `FileManager.startDownloadingUbiquitousItem(at:)` "starts the download process" for an item
  not yet downloaded
  ([Apple](https://developer.apple.com/documentation/foundation/filemanager/startdownloadingubiquitousitem(at:))).
  Logseq reports that iCloud sometimes "doesn't notify apps on file updated" and can get stuck
  uploading small files (Logseq FAQ, *secondary*).
- **Android: picking a folder.** `ACTION_OPEN_DOCUMENT_TREE` grants a whole directory tree,
  and `takePersistableUriPermission` keeps the grant across restarts. Since Android 11 the
  storage root, `Download/` and `Android/data|obb` cannot be picked
  ([Android docs](https://developer.android.com/training/data-storage/shared/documents-files)).
  A provider's root shows up in the tree picker only if it supports `FLAG_SUPPORTS_IS_CHILD`
  ([DocumentsContract.Root](https://developer.android.com/reference/android/provider/DocumentsContract.Root)).
  Google Drive's root reportedly does not
  ([Stack Overflow 56539920](https://stackoverflow.com/questions/56539920), *secondary*).
  Cloud roots may set `FLAG_LIMITED_FUNCTIONALITY_WHEN_OFFLINE`, so contents that are not
  local cannot be read offline (same reference).
- **Android: no file paths.** SAF hands out `content://` URIs and file descriptors
  (`openFileDescriptor`), not paths (Android docs above). `MANAGE_EXTERNAL_STORAGE`
  ("All files access") is allowed on Play only for file managers, backup, antivirus and
  similar apps, and "only … when your app cannot effectively make use of" SAF
  ([Play policy](https://support.google.com/googleplay/android-developer/answer/10467955)).
  *Inference:* rusqlite opens paths, so sync files must be read and written as whole byte
  streams through SAF, with SQLite kept on app-private storage. That fits a design of whole
  files that are never changed after they are written.

## 3. Provider behaviour: latency, partial files, conflict copies

| Provider | Latency (documented) | Partial / placeholder files | Conflicting edits to one file |
|---|---|---|---|
| Syncthing | Watcher batches changes for 10 s (`fsWatcherDelayS`); deletes wait 1 min more; full rescan hourly ([docs](https://docs.syncthing.net/users/syncing)) | Writes to `.syncthing.<name>.tmp` (`~syncthing~…` on Windows), then renames into place, so readers never see half a file (same) | Renames the older one to `<name>.sync-conflict-<date>-<time>-<device>.<ext>`, which then syncs everywhere as an ordinary file (same) |
| Dropbox | Not documented | Online-only placeholders; opening one downloads it ([help](https://help.dropbox.com/sync/make-files-online-only)) | `… (<user>'s conflicted copy <date>)`. It happens on simultaneous edits, offline edits, and "a file … left open … auto-save" ([help](https://help.dropbox.com/organize/conflicted-copy)) |
| OneDrive | Not documented | Files On-Demand placeholders (MS release notes) | Duplicate with the **computer name** appended ([MS support](https://support.microsoft.com/en-us/onedrive/duplicate-files-in-onedrive)) |
| Google Drive | Not documented | Streaming mode keeps files online until opened; the virtual drive is FAT-formatted ([2375082](https://support.google.com/drive/answer/2375082?hl=en)) | Not found in Google's help *(gap)* |
| iCloud Drive | Not documented; reports of stuck uploads and missed notifications (Logseq FAQ, *secondary*) | Placeholders until downloaded (Apple API above) | Duplicated files reported (Logseq FAQ, *secondary*) |

What every row has in common: **two devices writing the same file is what creates conflict
copies**. Every sync client syncs whole files, and none of them merges.

## 4. Candidate designs

| Design | How it works | Rust / wasm / mobile | Fit |
|---|---|---|---|
| **A. Live DB in the folder** | Point SQLite at the synced file | — | **Rejected** (§1) |
| **B. Snapshot + three-way merge** (KOReader) | Each device uploads a whole copy of the DB and keeps a cached copy of the last one it synced. On sync it `ATTACH`es the remote copy and the cached copy, then works out inserts and deletes per table in SQL. | Plain SQLite. Snapshot with the backup API or `VACUUM INTO` | Simple, and works with any provider. But one shared file means two devices uploading cross-wise produces a conflict copy. Every table needs hand-written merge SQL. Uploads the whole DB each time |
| **C. Per-device append-only op log** | Each device writes **only its own** files: `devices/<device-id>/<seq>.ops`, never rewritten. It reads every other device's files and replays ops it has not applied, resolving per row. Periodic per-device snapshots let a new device start fast and let old logs be trimmed. | Pure Rust plus serde. The op type could be the existing `Change { before, after }` (§6), which already compiles for wasm | No file is ever written by two devices, so no conflict copies by design. Half-written or missing files are skipped and retried. The merge rules are ours to define |
| **C′. Op log as SQLite session changesets** | Record changes with the session extension and ship `changeset` blobs. Applying one calls a conflict handler for `DATA` / `NOTFOUND` / `CONFLICT` / `CONSTRAINT` / `FOREIGN_KEY` | rusqlite feature `session` (needs `SQLITE_ENABLE_SESSION`; rusqlite 0.41's `Cargo.toml`) with safe wrappers `Session`, `Changeset`, `Connection::apply` ([src/session.rs](https://github.com/rusqlite/rusqlite/blob/master/src/session.rs)) | Needs a declared PRIMARY KEY on every table and does not capture virtual tables, so `notes_fts` is skipped (it can be rebuilt). Changesets apply only to "a database with the same schema and compatible starting data" ([sessionintro](https://www.sqlite.org/sessionintro.html)). Changesets carry no clock or device, so ordering stays ours. The format is SQLite's own binary |
| **D. cr-sqlite** | Tables become "conflict-free replicated relations". Peers exchange rows from the `crsql_changes` virtual table, with column versions and `site_id` ([README](https://github.com/vlcn-io/cr-sqlite)) | C and Rust **loadable extension**; MIT; 3.8k stars. Last release **v0.16.3, 2024-01-17**; last push 2026-08-10 ([GitHub API](https://api.github.com/repos/vlcn-io/cr-sqlite/releases/latest)). Loading it from rusqlite goes through `unsafe` `LoadExtensionGuard` / `load_extension` ([rusqlite](https://github.com/rusqlite/rusqlite/blob/master/src/load_extension_guard.rs)), which the workspace's `unsafe_code = "forbid"` rejects. Static linking means a custom SQLite build. README says inserts are about 2.5× slower | Strong merge semantics, but stale releases, an `unsafe` integration and its own schema-change ritual (`crsql_begin_alter`) |
| **E. Automerge** | A JSON-like CRDT document; history syncs as binary changes | `automerge` crate **0.12.0** (2026-09-16), MIT. Rust core compiled to wasm, plus a C API. The README says the Rust API "is low level and not well documented" ([README](https://github.com/automerge/automerge), [crates.io](https://crates.io/crates/automerge)) | Would replace SQLite as the source of truth, or shadow it. Its strength is concurrent text, which Notes might want |
| **F. Loro** | CRDT library: LWW map, movable list and tree, Fugue text | `loro` **1.17.1** (2026-10-22), MIT. Rust, JS via wasm, and Swift; other bindings in `loro-ffi` ([README](https://github.com/loro-dev/loro), [crates.io](https://crates.io/crates/loro)) | Same trade-off as E. Its LWW map fits row-per-key data well |

## 5. Prior art

| App | Storage | How it syncs through a folder | Conflicts |
|---|---|---|---|
| **Zotero** | SQLite | Refuses a data directory in a cloud folder and pushes its own Zotero Sync instead ([KB](https://www.zotero.org/support/kb/data_directory_in_cloud_storage_folder)) | — |
| **Joplin** | SQLite locally; on the sync target, one file per item | The "File system" target works with any folder, including one inside Dropbox. Each item is uploaded "within a few seconds" of a change, and the client polls "every few minutes" ([sync spec](https://joplinapp.org/help/dev/spec/sync), [apps/sync](https://joplinapp.org/help/apps/sync/)). Lock files named `<lockType>_<clientType>_<clientId>.json`, refreshed on a timeout. An EXCLUSIVE lock covers target upgrades ([sync_lock](https://joplinapp.org/help/dev/spec/sync_lock)) | Line-based three-way auto-merge from a stored base. When that fails, the local copy goes to a **Conflicts** notebook ([auto-merge spec](https://joplinapp.org/help/dev/spec/conflict_auto_merge/), [conflict.md](https://github.com/laurent22/joplin/blob/dev/readme/apps/conflict.md)) |
| **KOReader** (statistics, vocabulary) | SQLite (WAL where possible) | Uploads the whole DB to Dropbox, WebDAV or FTP and merges on sync: `onSync(local_path, cached_path, income_path)` `ATTACH`es the remote and cached DBs and computes deletes and inserts in SQL ([PR #9709](https://github.com/koreader/koreader/pull/9709), [statistics main.lua](https://github.com/koreader/koreader/blob/master/plugins/statistics.koplugin/main.lua)). Moved to the Cloud storage plugin in 2026 ([PR #15210](https://github.com/koreader/koreader/pull/15210)) | Rows matched on a natural key (title, authors, md5). Merged durations take `MAX` |
| **Obsidian** | Plain Markdown files | Leaves sync to the user's tool, but warns: keep files downloaded, iCloud on Windows is risky, third-party clouds are unsupported on iOS, and never mix two sync services (Obsidian help) | Whatever the provider does (conflict copies) |
| **Logseq** (file graphs) | Markdown files plus an in-app DB | iCloud and others: duplicated files and missed updates. Logseq built its own Logseq Sync and asks users to move graphs **out** of synced folders ([FAQ](https://discuss.logseq.com/t/im-using-logseq-with-icloud-but-experiencing-data-loss-or-file-conflict-errors-whats-going-on/13393), [Logseq Sync post](https://blog.logseq.com/how-to-setup-and-use-logseq-sync/)) | A `logseq/bak` folder of overwritten versions |

The lesson across all five: the apps that get this right either send **many small per-item
files with their own merge rules** (Joplin), or **merge whole SQLite snapshots themselves**
(KOReader). The apps that left the sync client to cope with a live file (Zotero's users,
Logseq) lost data.

## 6. What the User database already gives (code, read 2026-11-10)

- Every annotation edit is a list of `Change { before: Option<Row>, after: Option<Row> }`,
  applied in one transaction "after checking that each row is still what the Change expects"
  (`crates/store/src/annotation_store.rs`, `annotation.rs`). `Change` is serde-serializable
  and "compile[s] everywhere (wasm included)". It is close to an op-log record already, and
  its `before` gives the base that conflict detection needs.
- Ids are UUIDv7 TEXT primary keys made in-crate (RFC 9562 method 1) for Notebooks,
  Annotations and Highlight styles (`annotation.rs::new_id`). Notebooks and annotations
  carry `modified_at`.
- Things a sync design must change: deletes are **hard deletes** with
  `ON DELETE CASCADE`, so there are no tombstones. `notes_fts` is an FTS5 virtual table.
  `layout` is keyed by its name (`COLLATE NOCASE`), and `preference` and `preferred_order` by
  natural keys, so two devices can create the "same" row independently (`user.rs` migrations).
  The default-Notebook rule ("exactly one", via a partial unique index) is a cross-device
  invariant the merge has to keep.

## Implications for Open Bible mobile

1. **The live DB stays private on every device.** WAL, locking and journal pairing all rule
   out the synced folder (§1). Mobile needs this anyway: SAF gives no paths, and iOS gives
   only scoped, coordinated access (§2).
2. **Design C, a per-device append-only op log, fits the constraints best.** Each device
   writes only under its own `devices/<id>/`. Files are written once (temp file, then rename,
   or write and never touch again), carry a length and a hash, and are skipped until
   complete. Conflict copies then cannot arise for our files (§3). Any provider that mirrors
   a folder works, and a missing file only delays a merge. Build it on the existing `Change`
   type, not session changesets: it is ours, versioned, and already compiles for wasm.
   C′ (session changesets) is a fallback for tables outside `apply_changes`.
3. **Merge rules have to follow ADR 0006 ("never silently dropped").** Use last-writer-wins
   per row for styles, preferences and Layouts. For a Note edited on two devices from the same
   `before`, keep both, Joplin-style, in a conflict Notebook or as a second Note. That needs
   tombstones for deletes, a device id plus a hybrid or Lamport clock (`modified_at`
   wall-clock alone is not safe), and rules for natural-key rows and the one default
   Notebook.
4. **Periodic per-device snapshots** (backup API, the same file format as ADR 0013 Backups)
   bound how much log is replayed and let a new phone bootstrap. KOReader shows SQL
   `ATTACH`-merge of snapshots is workable, but as the *only* mechanism it brings back
   shared-file conflicts.
5. **CRDT libraries are not worth it yet.** cr-sqlite is stale, needs `unsafe` and costs
   insert speed. Automerge and Loro would move the source of truth off SQLite. Revisit only
   if collaborative Note editing becomes a goal.
6. **Supported folders differ by platform, and the onboarding copy must say so.** On iOS,
   expect iCloud Drive (and "On My iPhone" folders fed by a Syncthing-style app); third-party
   clouds probably cannot be picked as folders. On Android, SAF trees work for local storage
   and Syncthing-Fork folders, while Google Drive likely needs a third-party mirroring app.
   On Linux there is no Google Drive, OneDrive or iCloud client, so Dropbox or Syncthing.
   Tell users to mark the folder **always keep on device**.
7. **Sync is periodic, not live.** Even Syncthing batches for 10 s or more, and others
   publish no latency figures. Run sync at launch, on resume, after edits (debounced) and on
   demand, and show when the last merge happened.

## Open questions for the decision ticket

- Which providers do we promise per platform? Do we need to test on real devices whether
  Dropbox, Google Drive and OneDrive File Providers on iOS allow folder picking today, and
  whether their Android SAF roots allow tree picking?
- Should the app's own iCloud container (ubiquity container) be offered on Apple platforms,
  instead of or as well as a user-picked folder?
- Op format: JSON lines of `Change` (readable, larger) or a binary format? How long are logs
  kept before compaction into a snapshot, and what happens to a device that has been away
  longer than that?
- Clock: a hybrid logical clock per device, or a Lamport counter plus device id? Where does
  the device id live, and what happens if a User database is restored from a Backup
  (ADR 0013) and so goes back in time?
- Which tables sync? Annotations, Notebooks, Highlight styles and palettes, Reading plans and
  Favorites, surely. History, Last session and per-device preferences (window layout on
  desktop versus phone) probably not.
- Note conflict UX: a conflict Notebook, an inline "two versions" card, or a line-based
  three-way merge first (Joplin)?
- Schema versions across devices: an older app meets ops from a newer schema. Refuse and
  prompt to update (Joplin's sync-target version), or skip unknown fields?
- Encrypt files in the cloud folder? There is no account, so it would be a user passphrase.
- Is Syncthing-Fork, the community Android app, an acceptable recommendation now that the
  official app is archived?

## Sources

Kept (checked 2026-11-10):

- SQLite: [wal.html](https://www.sqlite.org/wal.html), [howtocorrupt.html](https://www.sqlite.org/howtocorrupt.html), [useovernet.html](https://www.sqlite.org/useovernet.html), [sessionintro.html](https://www.sqlite.org/sessionintro.html): why the live file breaks, and the changeset model.
- Apple: [Providing access to directories](https://developer.apple.com/documentation/uikit/providing-access-to-directories), [startAccessingSecurityScopedResource](https://developer.apple.com/documentation/foundation/nsurl/startaccessingsecurityscopedresource()), [startDownloadingUbiquitousItem](https://developer.apple.com/documentation/foundation/filemanager/startdownloadingubiquitousitem(at:)) (read via the DocC JSON endpoints): iOS folder access.
- Android: [documents-files](https://developer.android.com/training/data-storage/shared/documents-files), [document-provider](https://developer.android.com/guide/topics/providers/document-provider), [DocumentsContract.Root](https://developer.android.com/reference/android/provider/DocumentsContract.Root); Play [All files access policy](https://support.google.com/googleplay/android-developer/answer/10467955).
- Providers: [Syncthing syncing](https://docs.syncthing.net/users/syncing), [syncthing-android README](https://github.com/syncthing/syncthing-android/blob/main/README.md), Dropbox [conflicted copy](https://help.dropbox.com/organize/conflicted-copy), [online-only](https://help.dropbox.com/sync/make-files-online-only), [system requirements](https://help.dropbox.com/installs/system-requirements), Microsoft [duplicate files](https://support.microsoft.com/en-us/onedrive/duplicate-files-in-onedrive), [OneDrive system requirements](https://support.microsoft.com/en-us/onedrive/onedrive-system-requirements), Google [Drive for desktop compatibility](https://support.google.com/drive/answer/2375082?hl=en).
- Libraries: rusqlite `Cargo.toml`, `src/session.rs`, `src/load_extension_guard.rs` (master, 0.41.0); cr-sqlite repo and releases (GitHub API); Automerge and Loro READMEs plus crates.io API.
- Prior art: Zotero KB; Joplin sync, sync_lock and auto-merge specs; KOReader PRs #9709 and #15210 and the statistics `onSync` source; the Obsidian help source (GitHub); Logseq FAQ and Sync post.
- This repo: `crates/store/src/{user.rs, annotation.rs, annotation_store.rs}`, ADRs 0006 and 0013, the workspace `Cargo.toml`.

Secondary, used only where marked: Apple Developer Forums 691738, Stack Overflow 58262986 and
56539920, the Dropbox community thread 374480 (search snippets; the pages could not be
fetched), and the Logseq forum FAQ.

Dropped: SEO explainers on cloud conflicts (digitalcitizen.life, kagemusha.app,
technobezz.com), which only repeat vendor pages; Reddit threads; university KB copies of
Microsoft's OneDrive notes.

## Gaps

- No vendor documents sync latency for iCloud, Google Drive, Dropbox or OneDrive. It needs
  measuring on devices.
- Folder picking of third-party providers on iOS and Android tree support for Drive, Dropbox
  and OneDrive rest on secondary reports. They need a device spike.
- Google Drive's conflict behaviour and whether any iCloud or Google client exists for Linux
  were not confirmed from the vendors.
- Whether rusqlite's `session` feature builds with its wasm backend (`sqlite-wasm-rs`) was
  not checked.
