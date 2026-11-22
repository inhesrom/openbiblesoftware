# Mobile library (#333, plan A7)

The Mobile app's Library: **Library → Starter library** (the Bundled set, the Study set and
the rest of the Starter library), **Get Resources**' Open catalogue, the downloads, Remove
and Add back, **Library → Storage**, and **Import** (S26: a file from the document picker or
"Open in Open Bible", with its Import report). Every rule (the manifests and their merge, the sets,
sha256 pinning, each download's state, "Wi-Fi only", "ask first over cellular", "won't fit")
is in Rust; the shell owns only the transfers. S23 of [the iOS plan](../plans/mobile-ios.md)
built the Rust side; S24 built the iOS screens and the background transfers on it (§ The
shell's downloads); S25 the Search index status, the background processing task and the
backup exclusion (§ Storage and background work). The
desktop's Starter library is unchanged ([starter-library](starter-library.md)).

## Contract

- **The released manifest leads** (`obs_engine::starter`). `MobileApp::boot` calls
  `StarterLibrary::set_released_leads(true)` before `sync_starter_library`. With it, and only
  with it (the desktop never sets it):
  - `StarterLibrary::manifest()` is the released manifest merged with the Bundled set's: the
    released items in their order, then the bundle's items the release does not list. A
    released item replaces the bundle's of the same id, so a newer revision of a Bundled set
    Resource shows as Update. Before the released manifest is fetched (offline, or before
    Library → Starter library opens) the rows are the Bundled set's alone.
  - `starter_rows`, `starter_wanted` and `starter_wanted_set(MobileSet)` read that merge.
  - `add_starter` installs from the bundle what the bundle holds itself (the same revision
    and `file_sha256`, `StarterLibrary::bundle_holds`), at once, and downloads the rest through
    the engine's `Fetch` job. `sync_starter_library` still reads only the bundle (first run,
    revisions in a newer app, Resources new to the bundle).
  - **The release whose manifest format matches** (#333 "Starter updates"; ADR 0007's
    `releases/latest/download/`). `check_starter_release` fetches the latest release's
    `starter-manifest.json`. If its `format` is `MANIFEST_FORMAT` it is used. If it is newer,
    `select_release` reads GitHub's releases list (`updates::RELEASES_API`,
    `?per_page=20`, parsed by `updates::parse_releases`), newest first, skipping drafts and
    pre-releases, at most `RELEASES_SEARCHED` (20), fetching each release's
    `starter-manifest.json` asset until one has this app's format; it stops at the first
    older format. That release's asset folder becomes `StarterLibrary::files_base()`, so its
    `.obr` files come from the same release; from GitHub's list it must be
    `https://github.com/<owner>/<repo>/releases/download/<tag>/` of this app's repository
    (`release_files_base`: the owner and repository of `RELEASE_DOWNLOADS`, a tag free of
    `/`, `?` and `#`), or the check fails. A releases API set with `set_releases_api` (a
    mirror, a `file://` list in tests) is trusted for its own assets. If none matches,
    `needs_app_update()` is true and the error says to update the app; the rows stay as they
    were. If the latest release's format is older than this app's (a test build ahead of
    the releases), the error says so and that a release of this format is needed;
    `needs_app_update()` stays false. The releases list is read only on a format mismatch,
    and only when the user opens the Starter library, starts a download or hands in a file
    the manifest does not list (#333 "Network"); every request goes through the shell's
    transport.
  - **Kept on the device** (`RELEASED_PREF`, `starter-library.released`, a device-only
    preference, never synced): the released manifest found and its pinned files base. A
    launch with the switch on reads it in `sync_starter_library`, so the rows, Study set and
    `files_base()` are the last release's before any check, and a transfer that finishes
    after a relaunch is still known. The desktop never reads or writes it.
  - With the switch off the desktop keeps its either/or (the bundle's manifest, else the
    released one) and its refusal of another format ("Starter library manifest format 2
    (this app reads 1)"); `starter.check` with a bundle stays a no-op for the rows.
- **Installing files the shell downloaded.**
  `Session::add_starter_from_files(items, &[(id, path)])` submits a job running the Starter
  download's `install` with each file read from disk: every file checked against its sha256,
  staged, then all put in place or none. `StarterLibrary::file_install(job)` is its
  `FileInstallPhase` (`Verifying`, `Installing` once every file is verified, `Installed`,
  `Failed(why)`) until `forget_file_install`; when it finishes the pump's `poll_starter`
  applies `starter_added`, so they join the Library as a download does (a Removed one is no
  longer removed). While it runs, `is_starter_downloading` is true for its ids (the
  desktop's Remove refusal, `StarterStatus::Adding`). `Session::install_open_file(entry, path)` imports an Open catalogue file as
  `download_open` does (`ImportSource::OpenDownloaded`: the pinned sha256 or the pinned
  licence notice, then the importer) with no report Tab. Either deletes a file once it has
  read it, so a refused file is downloaded again for Retry.
- **Storage** (`obs_engine::library_storage`). `Session::library_storage()`: per Resource in
  Preferred order, its `.obr`, its Search index caches (the cache folder and `search/` beside
  the `.obr`) and the alias caches it is one side of (a shared cache counts for each of its
  two Resources; the total counts every file once). `space_needed(bytes)` is the free space a
  download needs before it starts; `fits(bytes, free)`.
- **`obs-mobile` `library`** (`MobileApp`):
  - `set_fetch(Fetch)`, `set_starter_source(base, releases_api)`, `check_starter()`.
  - `starter_library()`: rows (title, abbreviation, type, language, SPDX licence, bytes,
    `size` "26.0 MB", `features` as on the desktop, `set`, `status` InLibrary / Outdated /
    Removed / Missing, the row's download), `checking`, `released`, `error`,
    `needs_app_update`, and `study_set` (Resources, bytes and "15 Resources, 136.6 MB").
  - `plan_starter_download(StarterChoice::Set(Study) | One(id), Network, cellular_confirmed,
    free)` → `DownloadPlan`: Resources the Bundled set holds (an Add back) are added from it
    at once; a Resource in the library at the offered revision, or already downloading, is
    left out; then `download_rule`; `Start(requests)` marks each `Queued`. Each
    `DownloadRequest` names the URL (`files_base()<id>.obr`), the size, the file in
    `downloads/` the shell writes, and whether the transfer may use cellular data.
    `plan_open_download(entry, …)` does the same for an Open catalogue entry (its size is not
    known: it needs `OPEN_DOWNLOAD_FLOOR`, 100 MB, free, and the cellular question says so).
  - `download_rule`: free space first (`WontFit` with both sizes when `space_needed`, or the
    floor, exceeds the free bytes the shell passes less what the downloads already planned
    need), then the network: on Wi-Fi, start (cellular allowed only with "Use cellular
    data"); on cellular with it on, start; with it off, `AskCellular` with the size, and
    asking again with `cellular_confirmed` starts it over cellular. A network the shell
    does not know yet (`Network.is_wifi: None`, before its first path update) counts as not
    Wi-Fi: Download asks first, saying the network is not known yet.
  - Download states: `Queued` → `Downloading { done, total }` (`download_progress`) →
    `Verifying` → `Installing` → `Indexing` → `Done`, or `Failed { message, retry }`
    (`download_failed` from the shell; a refused file from `install_downloaded`).
    `install_downloaded(id, path, origin)` takes only the file the core named for it (in
    `downloads/`; any other path is refused and not deleted), and starts a job: a Starter
    file's `add_starter_from_files` (it returns `Verifying`), an Open catalogue import
    (`Installing`); the core's lock is not held while either reads, hashes or copies. A call
    for a download already verifying, installing, indexing or done is ignored (its state is
    returned). It also accepts a finished transfer the core does not know (one that
    completed after a relaunch): with the kept released manifest it installs at once; a
    Starter Resource that manifest does not list stays `Queued`, its file kept, while a
    released manifest check runs (it starts one), and is installed when the check lists it,
    refused (file deleted) when the check lists it not or finds no release this app reads,
    and keeps waiting after a check that failed. `download_progress`, `download_failed` and
    `install_downloaded` adopt a transfer the core does not know (one the system carried on
    across a relaunch): it gets a download at the file the core names, with the manifest's
    size, so it shows in its row and the banner, can be cancelled, shows a failure with
    Retry, and counts in the free space the downloads need. `cancel_download` forgets a queued,
    downloading or failed download and deletes its file. The pump moves Starter installs,
    Open imports and Search index builds on (a download that fails there has its file
    deleted: an Import cancelled before it read it), and reports `Changed::Downloads`
    whenever the Starter library or a download changes.
  - `download_banner()`: "Downloading 4 Resources · 12.0 MB of 40.0 MB", "Building the
    Search index of 2 Resources", "1 download failed", or nothing.
  - `open_catalogue(LicenceFilter)`: the Open catalogue rows the desktop's licence filter
    admits, each with "from <publisher>", licence, Licence class, evidence URL, the library
    Resource made from it and its download.
  - `remove_resource(id, confirmed)`: without `confirmed`, the question
    (`library.remove`'s, with the Annotations that refer to it); then removed as on the
    desktop. A removed Starter Resource stays removed (on this device; ADR 0022): never added
    back by a launch, left out of the Study set. While its Starter download is active
    (queued to installing) it is refused with the desktop's words ("… is being downloaded
    with the Starter library. Cancel the download, or wait for it to finish, then remove
    it."): the download would add it back. A reader that shows it moves on first: the second
    reader of a Split view closes (`Changed::Reader(1)`; `readers()` no longer lists it); the
    reader switches to the first other Resource of the same type in Preferred order through
    `tab.parallel`, else to the first other Resource it can show; only when there is none is
    the removal refused ("… is the only Resource the reader can show").
  - `storage()`: `library_storage` with titles and sizes in words.
  - `download_label(state)`: a download's state in words for its row ("Waiting to
    download", "Downloading · 12.0 MB of 26.0 MB", "Checking the file…", "Adding it to your
    Library…", "Building its Search index…", "In your Library", or a failure's message).
- **`obs-mobile` `download_settings`** (S24; the device-only User database preference
  `mobile.downloads`, `DOWNLOADS_PREF`, in `obs_store::DEVICE_PREFERENCES`, never synced):
  - `use_cellular()` / `set_use_cellular(on)`: "Use cellular data", off by default (#333),
    kept on this device. The shell passes it as every `Network`'s `allow_cellular`; the
    `plan_*` signatures are unchanged.
  - `study_set_offer(network)` → `Option<StudySetOffer>`: the first-launch Study set offer
    while there is one to make, every word of it (title, what it brings, `size` such as "15
    Resources, 136.6 MB", `status`, the `network` line, where to find it `later`, the
    `download` and `not_now` labels, and `retry` after a failed lookup). `None` once it was
    shown at an earlier launch on this device, when nothing of the Study set is missing
    (every Resource of it in the library or Removed, once the released list says which), or
    with the debug Study set override. Before the released list is in (the first launch) it
    has no size and its status says the Study set is being looked up ("Looking up the Study
    set…", with `looking_up` true: the sheet shows a spinner), or why the list could not be
    fetched, with `retry`; a list kept on the device gives the size at once, and a later
    failed check changes nothing in it. The `network` line is "You are on Wi-Fi.", "You are
    not on Wi-Fi." or, before the shell knows, "Checking the network.", then what Download
    does there.
  - `study_set_offer_shown()`: called as the sheet appears; recorded at once, so the offer is
    not made at a later launch even if the app is closed while it shows (as the desktop's
    First-run tour). This launch keeps answering `study_set_offer`, so the open sheet follows
    the lookup.
- **`obs-ffi`** (`apps/obs-ffi/src/library.rs`): the Swift-implemented `Transport { get(url)
  -> bytes }` (foreign trait; errors `TransportError.notFound | rateLimited | network`, an
  unknown Swift error arrives as `network`), wrapped as the engine's `Fetch`
  (`TransportFetch`, installed by `MobileApp.setTransport`); it is only called from job
  threads with the core unlocked. Mirror Records and Enums of everything above, and
  `Changed.downloads`. Paths cross as `String`s. S24 added `use_cellular`,
  `set_use_cellular`, `study_set_offer` (`StudySetOffer`), `study_set_offer_shown`, and the
  free functions `download_label(state)`, `licence_class_label(class)` (the desktop's badge
  words) and `licence_filters()` (Get Resources' choices with the desktop's labels and
  hovers, and the default, All).

## The shell's downloads (S24, #333 "Downloads")

`apps/ios`; every rule above stays in Rust, and the shell moves bytes and lays out.

- **Transport** (`HTTPTransport`): one plain GET per call, blocking on the core's job
  thread, over an ephemeral `URLSession` with no cookies, cache or credentials and the
  `User-Agent` `OpenBible/<engine version>`; a request to `https://api.github.com/` asks for
  `application/vnd.github+json` and `X-GitHub-Api-Version: 2022-11-28`. Statuses as
  `apps/openbible/src/fetch.rs` reads them (`HTTPStatus.check`): 2xx a success, 404 and 410
  `notFound`, 429 and a 403 with `x-ratelimit-remaining: 0` `rateLimited`, anything else
  `network("<url>: http status: <code>")`; a transfer error `network("<url>: <error>")`;
  `file://` read locally (missing: `notFound`); another scheme `network("<url>: unsupported
  protocol \"<scheme>\"")`. Redirects are followed (GitHub's release assets). Release
  builds take `https` only (the transport and `DownloadCenter` refuse `http` and `file://`
  with the same words); Debug builds keep them for the local mirror.
- **Background transfers** (`DownloadCenter`): for each `DownloadRequest` of a
  `DownloadPlan.start`, a `URLSessionDownloadTask` in one of two
  `URLSessionConfiguration.background` sessions, `<bundle id>.downloads.wifi`
  (`allowsCellularAccess = false`) or `.cellular` (true), as the request's `cellular` says
  (the `URLRequest` says the same), `sessionSendsLaunchEvents`, not discretionary, no
  cookies or cache. The task's `taskDescription` carries the Resource id, origin, URL and
  the core's file relative to the app folder, so a transfer is known again after a
  relaunch. Progress goes to `download_progress` at most every 0.25 s (its last update
  always; with no length known, every one is throttled); a finished file is checked for its
  HTTP status, moved to the core's path at once and handed in with `install_downloaded`; a
  failure goes to `download_failed`; a request whose file is outside the app folder fails
  in the core. A request whose file has a transfer running already from the same URL
  carries on with it (a relaunch the core forgot it in); one from another URL replaces it.
  Cancel and the hand-in of a finished file take one lock, and a file whose download was
  cancelled is deleted rather than handed in, so a Cancel tapped as a transfer finishes
  installs nothing (the core would accept a finished file it does not know). No
  entitlement, capability or background mode is needed.
- **Background relaunch:** the app delegate's
  `application(_:handleEventsForBackgroundURLSession:completionHandler:)` stores the
  handler and starts the core (`AppCore.start()`; a later `start()`, the scene's, waits
  for the same boot). The app folder is known without the core
  (`AppCore.locateAppFolder()`, which changes nothing; the Debug test folder is made
  afresh only by the boot's `appFolder()`), so before the core is up a finished file still
  goes straight to the core's path and resume data is still kept; what is handed to the
  core waits for `attach`. `urlSessionDidFinishEvents(forBackgroundURLSession:)` calls the
  handler on the main thread, unless something waits for the core: then `attach` calls it
  once everything is handed in (or the boot failed), so a suspension or termination right
  after it loses no finished file.
- **Resume data:** the `NSURLSessionDownloadTaskResumeData` the system leaves with a
  dropped transfer is kept as `<the core's file>.resume`; Retry (a new `Start` for the same
  file) resumes from it. Resume data names the URL the transfer had reached, which for
  GitHub's release assets is the signed URL the 302 led to: while it is valid, the resumed
  request asks for the rest with a byte range; once it has expired the server refuses it
  and the transfer starts afresh from the core's URL, once (`DownloadTests`, against a local
  mirror). Cancel stops the transfer and deletes the resume data; the core deletes its own
  file; the core's 24 h sweep removes any left over.
- **Network** (`NetworkMonitor`): `NWPathMonitor`; `is_wifi` is `!path.usesInterfaceType(.cellular)`
  (Wi-Fi, wired and no network at all count as not cellular, which is what
  `allowsCellularAccess = false` waits for), and not known (`nil`) until the first path
  update; `allow_cellular` the core's `use_cellular()`. When the path changes, the open
  offer is asked for again, so its network line follows.
- **Refreshes:** `Changed.library` asks the core again for everything shown, Storage
  included; `Changed.downloads` (every progress update wakes the pump) asks for the rows,
  the banner and the offer only, at most every 0.25 s. Measured (`DownloadTests`, iPhone 17
  simulator): the 15-Resource Study set at 2 MB/s a connection, 18 s, brought 306
  `downloads` changes and 83 refreshes, 0.06 s on the main thread (0.34 %).
- **Free space:** `volumeAvailableCapacityForImportantUsage` of the app folder's volume,
  passed as `free_bytes` (nil when unknown); the privacy manifest declares reason `E174.1`.
- **Screens** (where the reader's options menu reaches them until the screen anatomy, #335,
  S12/S14a, decides; S14b moves Use cellular data into Settings): Library… opens the
  Library sheet (the banner, Starter library, Get Resources, Storage with its total, Use
  cellular data), each list pushed; the reader shows the core's banner at its foot while
  there is one, and tapping it opens the Library; the Study set offer is a sheet over the
  reader at launch while `study_set_offer` answers one. The core's questions
  (`AskCellular`, `WontFit`, Remove's question) and refusals are alerts with its words.
- **Test harness (Debug builds only):** `OBS_STARTER_MIRROR` (the Starter library's base
  URL: a local mirror), `OBS_TEST_NETWORK=cellular|wifi` (the path `NWPathMonitor` would
  report), `OBS_TEST_NO_STUDY_SET_OFFER=1` (the tests that do not test the offer), and with
  `OBS_TEST_APP_FOLDER` the transfers of an earlier test are cancelled at launch.

## Storage and background work (S25, #333 "Storage")

Every rule is the core's (`obs_mobile::background`, `MobileApp::search_index_status`); the
shell registers the system's task, submits the core's request, steps until the core says the
work is done or the system takes the time back, marks the core's folders, and lays out.

- **Where files live.** Everything is under `Application Support/OpenBible` (never Caches).
  `Paths::backup_excluded()` lists `library/`, `search/` and `aliases/`: what the app can copy,
  download or build again. At every boot the shell sets `isExcludedFromBackup` on exactly
  those (`AppCore.boot`, since S13); `user.db` and the app folder stay in device backups
  (`AppCoreTests` reads all of it back). `downloads/` is not excluded: its files are deleted
  once installed, and at boot when a day old.
- **Search index status** (`search_index_status(resource)` → `SearchIndexStatus`): `state`
  `Ready`, `Building` (queued or being built), `Failed` (tried again at the next launch) or
  `Unavailable`; `label` for a Storage row ("Search index ready", "Still being indexed",
  "Search index failed", "No Search index"); `searchable` (ready, or rebuilt with its older
  index searched meanwhile); `note`, the sentence Search (S20) shows for a Resource it cannot
  search yet ("TBESG is still being indexed. Search will find it once it is ready.", "… Search
  reads its older index until the new one is ready.", the failure with its error). `None`
  for a Resource not in the library. Each `storage()` row carries it as `index`.
  `Changed::Indexes` is reported when any Resource's state changes (not its progress), and
  the shell then asks for Storage again.
- **Background work** (`background_work()` → `BackgroundWork`): a Search index build queued
  or running (`SearchIndexes::is_building`), the People & places ↔ Dictionary alias caches
  being made current (`Guides::is_refreshing_aliases`), downloaded files being verified or
  installed (their index follows), and Imports not over yet (their index follows). Transfers
  are not in it: the system's background downloads carry those on.
  `pending_background_work()` → `Option<BackgroundRequest>`: while there is any,
  `INDEX_REQUEST` (`requires_external_power: false`: the builds take seconds to a minute and
  are wanted soon after a download; `requires_network: false`: they read files on the
  device).
- **Stepping** (`step_background(deadline)` → whether work is left): with jobs on threads
  (the app), one round of the pump's work (finished jobs routed, the next index build
  started), whose `Changed`s wait for the next `pump()`; with jobs inline (tests), job steps
  one at a time until the deadline has passed, so it ends at most one job step late. The FFI's
  `step_background()` repeats that for `BACKGROUND_STEP` (0.5 s) at most, releasing the
  core's lock between rounds (it waits as the pump thread does, 50 ms), so the screens'
  calls are answered meanwhile.
- **Alias caches.** Boot makes them current after the Starter sync and the Debug Study set
  override (so the library they see is the one the app shows), and the pump does again
  whenever the library changes (a download, an Import or Remove): a new pair's table is
  built in the background, a pair gone has its file deleted.
- **Work cut short is not lost.** A build in flight carries on when the app runs again; a
  launch queues every index without a cache again (`refresh_search_indexes`), and the alias
  refresh runs at every launch.

### The shell's task

- `project.yml` `info`: `UIBackgroundModes: [processing]` and
  `BGTaskSchedulerPermittedIdentifiers: [$(PRODUCT_BUNDLE_IDENTIFIER).index]`
  (`BGTaskScheduler.h`: without the mode, or with an identifier not listed, a submission is
  `notPermitted`). No entitlement.
- `BackgroundIndexer.register()` in the app delegate's `didFinishLaunching` (the system wants
  every launch handler registered before launching ends, once). The handler starts the core
  (`AppCore.start()`, which waits for a boot already under way) and runs.
- `schedule(core)` when the scene goes to the background (`AppCore.background()`) and when a
  run ends with work left: with the core's request, a `BGProcessingTaskRequest` with its
  `requiresExternalPower` and `requiresNetworkConnectivity` (no `earliestBeginDate`), submitted
  with `BGTaskScheduler.shared.submit(_:)`; with none, nothing. A refused submission is
  logged; the work then carries on the next time the app runs.
- `run(task, core)`: sets the task's `expirationHandler` (it sets a flag), calls
  `stepBackground()` off the main thread until the core says no work is left or the flag is
  set (so an expiration stops the run within one step), asks for another task while work is
  left, then `setTaskCompleted(success:)` with whether none is left.
- **Simulator.** iOS 27.2's simulator refuses the request: `submit` fails with
  `BGTaskSchedulerErrorDomain` code 1 (`unavailable`), "BGTaskScheduler is not available on
  this platform", as `BGTaskScheduler.h` says. The lldb command
  `e -l objc -- (void)[[BGTaskScheduler sharedScheduler] _simulateLaunchForTaskWithIdentifier:@"io.github.inhesrom.openbible.dev.index"]`
  exists there (the framework logs "Simulating launch for task with identifier …") but finds
  "No task request with identifier … has been scheduled", so the system's launch of the task
  is checked on a device (`docs/dev/ios-project.md`, "Background task"; plan S31/S35).

## Import (S26, #333 "Import")

- **The shell's part** (`apps/ios`, `ImportModel`). Import… (for now in the reader's options
  menu; S12/S14a may move it) opens the system document picker (SwiftUI `fileImporter`, a
  `UIDocumentPickerViewController`) limited to the app's types; "Open in Open Bible" arrives
  through `onOpenURL`. For each file Swift asks `import_destination(file name)`, copies the
  file there inside its security-scoped access and an `NSFileCoordinator` read (the original
  stays where it was; a file the system copied into `Documents/Inbox` for "Open in" is
  deleted once the copy is done, worked or not, the paths compared with symlinks resolved),
  then calls `start_import(path)`, or `import_copy_failed(path,
  error)` when the copy failed. It shows `import_report(n)` in a sheet and asks again on
  `Changed.imports`; Cancel is `cancel_import(n)`; Open shows the new Resource in reader 0.
  Swift tells no file kind apart beyond the picker's types.
- **Types** (`apps/ios/project.yml` `info`): `CFBundleDocumentTypes` for
  `org.ubsicap.usfm` (`.usfm`, `.sfm`, an imported type declaration conforming to
  `public.plain-text`; rank Default), `public.zip-archive` (rank Alternate) and
  `io.github.inhesrom.openbible.obr` (`.obr`, exported, conforming to `public.data`; rank
  Owner); `LSSupportsOpeningDocumentsInPlace = false`, so the system copies a shared file
  into the app and nothing is opened in place. No entitlement. The privacy manifest adds
  reason `3B52.1` (metadata of files the user chose) to the file timestamp category.
- **`obs-mobile` `import`** (`MobileApp`):
  - `import_destination(file_name)`: `<library>/imports/picked-<n>/<name>` (only the name's
    last component; a folder per file, so the Import reads the name the user knows).
  - `start_import(path)`: only a path the core named (any other is refused and left
    alone). By the name's extension: `.usfm`/`.sfm` → `ImportSource::Usfm`; `.zip` →
    `ImportSource::zip` (below); `.obr` → `Session::add_obr_copy`; anything else is refused
    with `IMPORT_KINDS`. Returns the Import's number. The job is the desktop's Import job
    without its report Tab (`Session::start_import_job`); its `ImportRecord` is saved in the
    User database as on the desktop.
  - `import_report(n)` → `ImportReportView`: heading ("Import in progress", "Import
    complete", "Import failed"), the file name, `ImportState`, progress and status ("Reading
    the file…", "3 / 66 Bible books", "Search index: 4 / 10", "Search index: built"…), the
    failure in plain words (`failure_message`) with the error for Details, the Resource for
    Open, and `ReportSection`s in the desktop report's words and order: Summary (title,
    `source_line`, `summary_counts`, Unmapped and Absent verses), Licence
    (`LICENCE_MISSING`, a warning), then Warnings, Dropped markup and Source files and Bible
    books, folded (`collapsed`); the Licence line is the desktop's (`LICENCE_MISSING`, a
    warning, or `LICENCE_RECORDED`). A `.obr`'s report: its title, type and abbreviation, and
    "Added to your Library as it is.", "Replaced <abbr> in your Library with revision N." or
    "Your Library already has this file (<abbr> revision N), so nothing was changed."; a
    refused one fails with `ObrRefused::message` ("Your Library already has <abbr> revision
    N. …").
  - `cancel_import(n)`; `import_copy_failed(path, error)`.
  - The pump's `advance_imports` deletes a finished Import's copy (`picked-<n>/`) and reports
    `Changed::Imports` when a report changed. Once an Import's final report has been seen
    (failed, or complete with its Search index built, failed or unavailable) it is skipped.
    Boot deletes `<library>/imports/` whole: no Import outlives the run that started it.
- **The engine's zips** (`obs_engine::import_zip`; the desktop never makes these sources,
  `ImportSource::detect` is unchanged):
  - `ImportSource::zip(path)`: a zip holding a `mods.d/*.conf` (resource forks aside) is
    `SwordZip`, anything else `UsfmZip` (a file that is not a zip then fails as one).
  - `SwordZip`: every entry is checked before anything is written. An absolute path, a `:`
    in any component (a drive letter), a backslash, a NUL or a `..` component, or a symbolic
    link refuses the whole zip; more than `ZIP_IMPORT_ENTRIES` (10,000) entries, or more than
    `ZIP_IMPORT_LIMIT` (512 MiB) declared or actually decompressed, refuses it. `mods.d/`
    must be at the top of the zip or in one folder at its top (Files' Compress); deeper, or a
    zip of several modules, is refused (no module chooser on mobile). `__MACOSX/` and `._*`
    files are left out. The zip unpacks into `<library>/imports/<id>.sword/`, its conf's
    `DataPath` must be the module's own folder in `modules/`
    (`sword_modules::install::module_data_dir`, through `SwordSource::check_data_paths`;
    the reader would otherwise read `../…` or an absolute path from outside it), then it is
    read as `ImportSource::Sword`; that folder is removed whatever the outcome, a panic too
    (`TempFolder`, a drop guard). USFM files beside
    the module are not imported, and the report's warnings say so (`USFM_BESIDE_SWORD`).
  - `UsfmZip`: the file (at most `ZIP_IMPORT_LIMIT`, `ZIP_IMPORT_ENTRIES` entries) read by
    `usfm_zip_books` (`.usfm`/`.sfm` from any folder, its own per-file and total bounds) and
    imported as a USFM folder is; none: "This zip holds no USFM books…".
  - `failure_message` has plain words for each refusal (a file it will not unpack; too large;
    several modules; not where the app looks; a `DataPath` outside the module's folder; no
    books; not a zip). Each matches a phrase only `import_zip` writes: its not-a-zip error is
    "this file is not a zip archive" (`NOT_A_ZIP`), so the desktop Open catalogue's "archive:
    not a zip archive" (`obs_formats::usfm_zip`) keeps the generic words.
  - `Session::add_obr_copy(path)` moves a `.obr` into the library folder for good (Search
    index, remembered names, re-anchoring, as the desktop adds a dropped one). Unlike the
    desktop's drop, which adds the dropped file where it is for the session, it changes the
    library folder, so it follows the Starter sync's rule: a Resource already in the library
    is replaced only by a greater revision (`ObrCopy::Replaced`; `Effect::Replaced` reloads
    its readers). The same revision with the same content changes nothing
    (`ObrCopy::AlreadyThere`); the same or a lower revision with other content is refused
    (`ObrRefused::NotNewer`). A new one is `<id>.obr` or `<id>-2.obr`
    (`ObrCopy::Added`); a file that is not a Resource is refused (`ObrRefused::Unreadable`).
    A refused or already-there file is left where it is.

## Data

| What | Where |
|---|---|
| Downloads in flight | `<root>/downloads/<id>.obr` (Starter), `<root>/downloads/open-<entry>.download` (Open catalogue); deleted once installed, refused or cancelled; at boot, a file there older than `STALE_DOWNLOAD` (24 h) is deleted |
| Starter state (known, removed) | User database preference `starter-library`, as on the desktop (per device) |
| The released manifest last fetched and its pinned files base | User database preference `starter-library.released` (device-only, `DEVICE_PREFERENCES`) |
| The releases list | `https://api.github.com/repos/inhesrom/openbiblesoftware/releases?per_page=20`, only on a manifest format mismatch |
| The release's files | `https://github.com/inhesrom/openbiblesoftware/releases/latest/download/`, or the pinned `…/releases/download/<tag>/` |
| Resume data of a dropped transfer | `<root>/downloads/<file>.resume` (S24, the shell's); deleted when the file is installed, by Cancel, and by the 24 h sweep |
| Use cellular data, the Study set offer shown | User database preference `mobile.downloads` (device-only, `DEVICE_PREFERENCES`) |
| Picked and shared files waiting for Import | `<root>/library/imports/picked-<n>/<name>`; deleted when the Import is over, and at boot |
| A SWORD zip while it is read | `<root>/library/imports/<resource id>.sword/`; removed whatever the outcome |
| Import reports | User database preference `import-report/<id>`, as on the desktop |
| Search indexes of downloaded and imported Resources | `<root>/search/` (the cache folder; the Bundled set's prebuilt ones in `<root>/library/search/`); excluded from device backups |
| Alias caches built on the device | `<root>/aliases/`; excluded from device backups |
| The background processing task | `<bundle id>.index` (`BGTaskSchedulerPermittedIdentifiers`); nothing kept: each launch has its work again |
| Space a download needs | 2.5 × its size: the file, its staged copy, room for its Search index (the Study set's indexes are 0.34 of its files) |

## Evidence

- `crates/engine/src/starter_tests.rs`: with released leads off, `starter.check` leaves the
  desktop's rows, Add back and revisions as they were; a 2-Resource Bundled set, then a Study
  set Resource through the release path from a `file://` base, all 7 rows listed throughout
  once the release is in; the Study set as the wanted items of its set; a newer released
  revision of a bundled Resource downloaded, not copied from the bundle; files downloaded by
  the app verified, installed and deleted (a mismatch adds nothing); a newer manifest format
  looked past (drafts and pre-releases skipped) to the release this app reads, its files
  pinned; no release of this format (and an older format stopping the search): the
  update-the-app state, and the desktop's refusal unchanged; a latest release older than
  this app's format (not "update the app"); only this app's release downloads pinned from
  GitHub's list (`file://` lists keep their own); the released manifest and its pinned
  files kept for the next launch (the desktop neither reads nor writes it); a downloaded
  file's install job, Verifying then Installing; storage per Resource.
  `#[ignore]`d over `dist/starter`: the real Bundled set, then TBESG from the release, 22
  rows before and after (0.3 s for TBESG, release build).
- `crates/engine/src/open_catalogue_tests.rs`: an Open catalogue file downloaded by the app,
  refused by its licence notice (deleted, Failed), then imported (no report Tab).
- `crates/mobile/src/library_tests.rs` (`cargo test -p obs-mobile library`): the rows before
  and after the release; the Study set through every state into the library; a sha256
  mismatch deletes the file and offers Retry; Cancel; the cellular and free-space rules
  (what is already planned counts; the Open floor); Remove, a relaunch, Add back from the
  bundle and by download; Remove refused while the Resource's download is under way; a
  transfer handed in after a relaunch installed (boot → plan → drop → boot → install), and
  one the kept manifest does not list waiting for the check; a file the core did not name
  refused and left alone; a second hand-in ignored; a cancelled Import's file deleted and
  stale downloads swept at boot; removing a reader's Resource (the reader switches, the
  second reader closes, the only Resource refused); the Open catalogue's licence filter and
  download; Storage. `#[ignore]`d over `dist/starter`: the real Study set (15
  Resources, 136.6 MB) through the states, 19 Resources in the library and 22 rows (installed
  in 0.13 s, indexed by 3.3 s, release build; Storage 226.4 MB).
- `apps/obs-ffi/tests/library.rs`: the Study set fetched through a Rust `Transport` standing
  in for Swift's, on job threads, and installed; a transport error in words.
- Import: `crates/engine/src/import_zip_tests.rs` (`cargo test -p obs-engine import`): a
  SWORD zip with `mods.d/` at the top and in one wrapper folder (resource forks aside)
  imports and leaves no temp folder; too deep, several modules (also in two folders)
  refused in plain words; USFM files beside a module named in the report; a zip-slip, an
  absolute, a backslash, a drive-letter (first or later component) and a symbolic-link entry
  each refuse the whole zip with nothing written outside; a conf whose `DataPath` leaves the
  temp folder (`../`, absolute) refused though the data is there; the 512 MiB and
  10,000-entry bounds (small bounds in the test) for SWORD and USFM zips, a zip holding more
  than its central directory declares refused as it is read (no temp folder left), and a
  cancelled unpack; the temp folder removed after a panic; a module the importer refuses
  fails after unpacking and leaves no temp folder; a USFM zip from a folder; no books, not a
  zip, and the desktop Open catalogue's not-a-zip error keeping the generic words; a `.obr`
  moved in, a greater revision replacing it, the same file changing nothing, the same
  revision with other content and a lower revision refused, a taken name, a non-Resource
  refused.
  `crates/mobile/src/import_tests.rs` (`cargo test -p obs-mobile import`): a USFM zip, a
  SWORD zip (with USFM beside it), a USFM file and a `.obr` (its next revision, the same
  file again, a lower revision refused) through the named file, with their reports; a
  finished Import skipped by the pump; refusals and the copy deleted; a copy that failed; a
  path not named; Cancel; boot sweeping `imports/`. `apps/obs-ffi/tests/import.rs`: zips and
  a refusal through the FFI with jobs on threads. `apps/ios/OpenBibleUITests/ImportTests`
  (with `apps/ios/scripts/place-import-files.sh`, `docs/dev/ios-project.md`): a USFM zip, a
  SWORD zip and a `.obr` picked from Files on an iPhone and an iPad, their reports, the
  report sheet's accessibility audit, Open.
- S24, `crates/mobile/src/library_tests.rs`: Use cellular data off by default and kept; the
  Study set offer looked up, then with its size and the network line (Wi-Fi, cellular,
  cellular allowed), shown once; a failed lookup with Try again and offered again at the
  next launch; no offer once nothing of the Study set is missing; the download states in
  words; a transfer still running after a relaunch adopted (its row, the banner, the
  won't-fit rule, Cancel) and one failed after it shown with Retry; a network not known
  yet asking first. `apps/obs-ffi/tests/library.rs`: the setting, the offer (its
  `looking_up`, the unknown network), adoption and the words across the FFI.
  `crates/store/tests/sync_schema_db.rs` now reads `crates/mobile/src`'s preference keys
  too.
- S24, in the simulator (iOS 27.2, iPhone 17 and iPad Air 11-inch (M4)), against
  `MirrorServer` (`apps/ios/TestSupport`), a local HTTP mirror of `dist/starter` on
  127.0.0.1 in the test process, so no test touches the network:
  `OpenBibleTests/DownloadTests` (its Resources picked from the core's Study set rows):
  the transport's statuses, a redirect followed; Cancel during a transfer leaves no core
  file and no resume data, and nothing is handed in after; a finished file that reaches
  the delegate after Cancel is not installed; a background relaunch simulated through the
  delegate before the core is up (a finished file moved to the core's path, resume data
  kept, the completion handler held until `attach`, then the file installed and the
  failure shown with Retry); the main thread's work during the Study set's download;
  resume data across a 302 to a signed URL: while valid, the resumed request is
  `/signed/easton.obr` with `Range: bytes=524288-` and nothing more; expired (2 s), the same
  request is refused, then `/r/<id>.obr` and `/signed/<id>.obr` afresh, and both install.
  `OpenBibleTests/AppCoreTests`: a second `start()` during a boot waits for it and the
  reader ends up ready.
  `OpenBibleUITests/LibraryTests`: `testLibraryDownload` (the offer at first launch with "15
  Resources, 136.6 MB" and "You are on Wi-Fi.", its VoiceOver order and audits in Light and
  Dark; Download; the banner; the app sent home while the 15 transfers run at 512 KB/s a
  connection: about 55 MB arrived while it was away, the simulator keeping it in the
  background rather than suspending it within 20 s; back, every file verified and
  installed, the Starter library with no Study set left, About counting 19 Resources);
  `testABadHashShowsRetry` (the mirror serves the KJV corrupted: "kjv-strongs.obr failed
  verification: its sha256 is …, but the Starter library manifest says …. Nothing was
  added." with Retry; served right, Retry installs it); `testWiFiOnlyAsksFirstOnCellular`
  (`OBS_TEST_NETWORK=cellular`: the offer's network line, Download asks "Download the Study
  set (136.6 MB) over cellular data? …", Not now leaves the mirror with no `.obr` request;
  with Use cellular data on, a download starts without asking and installs);
  `testCancelStopsADownload`; `testGetResourcesAndStorage`; each screen's accessibility audit
  (`docs/dev/ios-project.md`, "Library tests"). The counts come from the offer ("N
  Resources") and the Bundled set of the served manifest, and the Resources are picked from
  that manifest by set and size, not by name. Previews `docs/previews/ios/s24-*.png`.
- S25, `crates/mobile/src/background_tests.rs` (`cargo test -p obs-mobile background`, and
  `storage`): after a download the core asks for the task (`INDEX_REQUEST`); steps whose
  deadline has passed run one job step each; Storage and `search_index_status` say "Still
  being indexed" (with Search's note) until the steps have built it, then "Search index
  ready", and the pump reports `Changed::Indexes`; three downloads take at least five steps
  past the deadline; an expired run's work is pending again after a relaunch, and once built
  a relaunch has no index to build (only its own alias refresh); a People & places index and
  a Dictionary imported mid-run get their alias cache in the background, counted in
  Storage. `apps/obs-ffi/tests/background.rs`: with jobs on threads, every
  `step_background()` ends within `BACKGROUND_STEP` + 0.4 s, another thread's calls are
  answered meanwhile (slowest under 0.3 s), and the index ends ready.
- S25, in the simulator (iPhone 17 and iPad Air 11-inch (M4), iOS 27.2):
  `OpenBibleTests/BackgroundIndexerTests` (with `TEST_RUNNER_OBS_STUDY_SET_DIR`): every
  Starter library Resource outside the Bundled set copied into the core's library without
  its index (18), the core asks for the task and Storage says "Still being indexed"; a run
  with a stand-in task builds all 18 (8.4 s on the iPhone 17, Debug) and completes it
  successfully with nothing left; a task that expires at once completes unsuccessfully, asks
  for another (refused in the simulator) and leaves the work, which the next launch has
  again; the simulator's refusal (`BGTaskSchedulerErrorDomain` code 1). `AppCoreTests`:
  exactly `library`, `search` and `aliases` are excluded from backups, read back, and the app
  folder and `user.db` are not. `OpenBibleUITests/StorageTests`: Library → Storage at launch
  over those 18 Resources read in place without their indexes, rows "Still being indexed"
  (one element: title, size, state; Remove after it), then "Search index ready"; the
  accessibility audit in Light and Dark in both states. Previews `docs/previews/ios/s25-*.png`.
