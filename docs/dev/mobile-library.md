# Mobile library (#333, plan A7)

The Mobile app's Library: **Library → Starter library** (the Bundled set, the Study set and
the rest of the Starter library), **Get Resources**' Open catalogue, the downloads, Remove
and Add back, and **Library → Storage**. Every rule (the manifests and their merge, the sets,
sha256 pinning, each download's state, "Wi-Fi only", "ask first over cellular", "won't fit")
is in Rust; the shell owns only the transfers. S23 of [the iOS plan](../plans/mobile-ios.md)
built the Rust side; S24 builds the iOS screens and the background transfers on it. The
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
    older format. That release's asset folder (`…/releases/download/<tag>/`) becomes
    `StarterLibrary::files_base()`, so its `.obr` files come from the same release. If none
    matches (or the latest is older than this app's format) `needs_app_update()` is true and
    the error says to update the app; the rows stay the bundle's. The releases list is read
    only on a format mismatch, and only when the user opens the Starter library or starts a
    download (#333 "Network"); every request goes through the shell's transport.
  - With the switch off the desktop keeps its either/or (the bundle's manifest, else the
    released one) and its refusal of another format ("Starter library manifest format 2
    (this app reads 1)"); `starter.check` with a bundle stays a no-op for the rows.
- **Installing files the shell downloaded.**
  `Session::add_starter_from_files(items, &[(id, path)])` is the Starter download's `install`
  with each file read from disk: every file checked against its sha256, staged, then all put
  in place or none; they join the Library as a download does (a Removed one is no longer
  removed). `Session::install_open_file(entry, path)` imports an Open catalogue file as
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
    known: the space check is skipped and the cellular question says so).
  - `download_rule`: free space first (`WontFit` with both sizes when `space_needed` exceeds
    the free bytes the shell passes), then the network: on Wi-Fi, start (cellular allowed
    only with "Use cellular data"); on cellular with it on, start; with it off, `AskCellular`
    with the size, and asking again with `cellular_confirmed` starts it over cellular.
  - Download states: `Queued` → `Downloading { done, total }` (`download_progress`) →
    `Verifying` → `Installing` → `Indexing` → `Done`, or `Failed { message, retry }`
    (`download_failed` from the shell; a refused file from `install_downloaded`).
    `install_downloaded(id, path, origin)` checks and installs a Starter file before it
    returns (it holds the core's lock for the copy: call it off the main thread) and starts an
    Open catalogue import on the job queue; it also accepts a finished transfer the core does
    not know (one that completed after a relaunch). `cancel_download` forgets a queued,
    downloading or failed download and deletes its file. The pump moves Open imports and
    Search index builds on, and reports `Changed::Downloads` whenever the Starter library or a
    download changes.
  - `download_banner()`: "Downloading 4 Resources · 12.0 MB of 40.0 MB", "Building the
    Search index of 2 Resources", "1 download failed", or nothing.
  - `open_catalogue(LicenceFilter)`: the Open catalogue rows the desktop's licence filter
    admits, each with "from <publisher>", licence, Licence class, evidence URL, the library
    Resource made from it and its download.
  - `remove_resource(id, confirmed)`: without `confirmed`, the question
    (`library.remove`'s, with the Annotations that refer to it); then removed as on the
    desktop. A removed Starter Resource stays removed (on this device; ADR 0022): never added
    back by a launch, left out of the Study set. A Resource a reader shows is not removed
    (the reader would lose its Tab); the error says to show another Resource first.
  - `storage()`: `library_storage` with titles and sizes in words.
- **`obs-ffi`** (`apps/obs-ffi/src/library.rs`): the Swift-implemented `Transport { get(url)
  -> bytes }` (foreign trait; errors `TransportError.notFound | rateLimited | network`, an
  unknown Swift error arrives as `network`), wrapped as the engine's `Fetch`
  (`TransportFetch`, installed by `MobileApp.setTransport`); it is only called from job
  threads with the core unlocked. Mirror Records and Enums of everything above, and
  `Changed.downloads`. Paths cross as `String`s.

## Data

| What | Where |
|---|---|
| Downloads in flight | `<root>/downloads/<id>.obr` (Starter), `<root>/downloads/open-<entry>.download` (Open catalogue); deleted once installed, refused or cancelled |
| Starter state (known, removed) | User database preference `starter-library`, as on the desktop (per device) |
| The releases list | `https://api.github.com/repos/inhesrom/openbiblesoftware/releases?per_page=20`, only on a manifest format mismatch |
| The release's files | `https://github.com/inhesrom/openbiblesoftware/releases/latest/download/`, or the pinned `…/releases/download/<tag>/` |
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
  update-the-app state, and the desktop's refusal unchanged; storage per Resource.
  `#[ignore]`d over `dist/starter`: the real Bundled set, then TBESG from the release, 22
  rows before and after (0.3 s for TBESG, release build).
- `crates/engine/src/open_catalogue_tests.rs`: an Open catalogue file downloaded by the app,
  refused by its licence notice (deleted, Failed), then imported (no report Tab).
- `crates/mobile/src/library_tests.rs` (`cargo test -p obs-mobile library`): the rows before
  and after the release; the Study set through every state into the library; a sha256
  mismatch deletes the file and offers Retry; Cancel; the cellular and free-space rules;
  Remove, a relaunch, Add back from the bundle and by download; the Open catalogue's licence
  filter and download; Storage. `#[ignore]`d over `dist/starter`: the real Study set (15
  Resources, 136.6 MB) through the states, 19 Resources in the library and 22 rows (installed
  in 0.13 s, indexed by 3.3 s, release build; Storage 226.4 MB).
- `apps/obs-ffi/tests/library.rs`: the Study set fetched through a Rust `Transport` standing
  in for Swift's, on job threads, and installed; a transport error in words.
- S24 adds the simulator evidence (background transfers, a bad hash showing Retry, Wi-Fi only
  on a simulated cellular path).
