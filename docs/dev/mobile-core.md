# Mobile core (ADR 0021)

The Mobile app (CONTEXT.md § Mobile) is a native shell (SwiftUI on iOS) over Rust. Its Rust
side is two crates and a tool. What each screen shows and what each action does lives in
`obs-mobile`; the shell lays out, draws and forwards input (ADR 0021 rule 3). The plan they
follow is `docs/plans/mobile-ios.md` (A1–A3). S02 built the scaffold, S03 the "Hello engine"
spike, S07 the text view prototype; S13 replaced both with the designed surface below. Later
steps add methods (the Study sheet, Annotations, Search, Guides, the Library).

## Contract

- **`obs-mobile`** (`crates/mobile`, L5). The Mobile app's screen models and actions as plain
  Rust types. It depends on `obs-engine`, `obs-layout`, `obs-model`, `obs-reading`,
  `obs-render`, `obs-scripture`, `obs-store`, `serde` and `serde_json` (command parameters
  and the Mobile Last session). It has no FFI crate (`cargo xtask layers`), compiles for
  wasm32 (`cargo xtask wasm`; `boot` reads files and is native only, and a reader there reads
  only documents already open), keeps `unsafe_code = "forbid"`, and is tested by `cargo xtask
  test` on every CI OS. Its modules:
  - `paths`: `Paths::under(root)` names everything under the one folder the shell picks (on
    iOS `Application Support/OpenBible`): `library`, `user.db`, `search`, `aliases`
    (`obs_engine::aliases::ALIAS_DIR`), `logs`, `downloads`; `backup_excluded()` lists
    `library`, `search` and `aliases` for the shell's `isExcludedFromBackup` (the User
    database is backed up); `study_set` is the Debug-only Study set override.
  - `app`: `MobileApp::boot(paths, bundle)` starts as the desktop does
    (`obs_ui_egui::App::with_file`): makes the folders, a `Session` with
    `JobMode::default_for_host()` (threads; `boot_with` takes `Inline` for tests), scans the
    library, sets the Search index, alias and log folders, opens or creates the User
    database, calls `sync_starter_library(bundle)`, adds the Study set override, then opens
    the readers of the Mobile Last session, else the first Bible in Preferred order at its
    start. `library()`, `problems()`, `engine_version()`. One `MobileApp` per process: every
    scene of the shell shares it.
  - `readers`: at most `MAX_READERS` (2: the reader and the Split view's second) readers,
    `ReaderId(0)` then `ReaderId(1)`. Each is one reading `Tab` of the engine's Workspace (in
    its one Panel) and the `ReadingView` of its Resource; Tab and Panel ids never leave the
    crate. `open(reader, resource, at)` opens the next reader (`resource.open`), switches a
    reader to another Resource of its type as Parallel resources do (`tab.parallel`: same
    place, a Back step) or to one of another type afresh. Loading a view is the desktop
    pane's `load` cut to one Panel: the Tab learns the numbering, navigation and Key schemes
    and the view goes to the arrival, the Tab's own location or the Resource's start.
    `reader(r)`/`readers()` are `ReaderSnapshot`s (Resource, type, right to left, keyed,
    location label, can go back/forward). Room for Note markers (#305) is made from the
    User database's Notes in the Resource (`marks_for`, `set_note_ends`) at each annotation
    revision; the markers themselves are S18's.
  - `navigation`: `navigate(reader, text)` (`tab.go`: a Bible reference in the Resource's
    own numbering with the Current location as context, or for a Lexicon or Dictionary an
    entry Key), `parse_reference` (the same, without going), `back`/`forward` (`tab.back`,
    `tab.forward`), the grid (`books`, `chapters`, `verses` from the Resource's own Verses, or
    a Commentary's sections; `go_to_verse`), `history()` (days of Visits) and
    `open_visit(reader, id)`. Every jump is a Visit, as on the desktop.
  - `frame` and `text_input`: the hosted text view (`docs/dev/mobile-text-view.md`):
    `frame(reader, offset, Viewport, SystemAppearance)`, `tile(reader, key, scale)`, `hit`
    (the Verse with its text and Cross-reference count, a Tagged word as Word Info shows
    it, a Footnote marker), `footnote(reader, note)`, and the `UITextInput` and accessibility
    answers. The system appearance is an input of every frame (Follow system and Auto
    follow it); a tile is refused over `MAX_TILE_PX` = 4096 device pixels a side (the widest
    page, a 13-inch iPad in landscape at 2×, is 2752).
  - `session`: the Mobile Last session, `{"mobile":1,"readers":[{"resource","verse"|"key"}]}`
    through `UserData::save_last_session` (per device, never synced, ADR 0022 rule 4); a
    snapshot of another format (the desktop's) is ignored and replaced. `save_session()`
    also records History; `on_background()` saves; `on_resume()` refreshes History's day.
  - `settings`: `settings()` and `set_theme`, `set_page_tone`, `set_reduce_motion`,
    `set_reading_font` through the View menu's commands (saved, as Settings saves on the
    desktop); `set_text_size` (saved with the View preferences); `dynamic_type(category)`
    sets the Text size for UIKit's content size category for the run (the system keeps the
    choice; `tokens::DYNAMIC_TYPE`, 12–40 pt, Large = 20 pt); `parallel_resources(reader)`.
    Whether an explicit Text size outlives Dynamic Type is S14b's Settings decision.
  - `library`: Library → Starter library, the Open catalogue, the downloads and their
    states, Remove and Add back, Storage ([mobile-library](mobile-library.md)). `boot` sets
    the engine's released-leads switch before `sync_starter_library`.
  - `events`: `pump()` lets the engine route finished jobs (`Session::poll_imports`, the
    job queue's one reader), applies the Workspace's effects to the readers, records
    History, moves the downloads on and returns each `Changed` once (`Library`, `Reader(id)`,
    `History`, `Message`, `Downloads`); `busy()` says jobs are running. Effects that place
    Tabs in Panels or open desktop windows and menus come from commands the Mobile app never
    runs and are dropped (the module docs list which mean something).
- **`obs-ffi`** (`apps/obs-ffi`, an app). The one UniFFI crate Swift links, in proc-macro
  mode (`uniffi::setup_scaffolding!()`, `#[uniffi::export]`, no UDL). Its crate types are
  `staticlib` (iOS), `cdylib` (the host library bindgen reads) and `lib` (Rust tests). It
  only converts and forwards: every rule stays in `obs-mobile`. The types that cross are
  mirror Records and Enums defined here with `From<obs_mobile::…>` (D9), so `obs-mobile`
  never needs `uniffi`. The Object `MobileApp` (constructor `boot(root, bundle, studySet)`)
  holds the app behind one `Mutex`; every method returns `Result`, and a lock poisoned by a
  panic is `MobileError.Stopped` (S03's `problems`, `resource_ids`, `reader_resource` and
  S07's `books`, `page_tones`, `set_page_tone`, `set_content_size`, `bibles` answered
  nothing instead). Errors: `Reference { message }` (a field's error, the parser's words),
  `Stopped`, `Failed`. `set_observer(Observer)` starts the pump thread: it waits on a
  channel that every changing call signals, else 50 ms while jobs run and 1 s otherwise,
  locks, calls `pump()`, unlocks, then calls the Swift `Observer.changed` for each change;
  it never reads the job queue's events, and stops when the app or its observer goes.
  `poll_changed()` is the same poll by hand. `set_transport(Transport)` takes the shell's
  foreign `Transport { get(url) -> bytes }` for small JSON and hands it to the engine as its
  `Fetch` (`src/library.rs`, [mobile-library](mobile-library.md)). Its `uniffi.toml` names the Swift module
  `ObsFFI`, so the bindings are `ObsFFI.swift`, with `ObsFFIFFI.h` and
  `ObsFFIFFI.modulemap` for UniFFI's C module `ObsFFIFFI`.
- **Tile pixels: the bulk byte path.** UniFFI 0.32 lowers a `Vec<u8>` one byte at a time
  (`uniffi_core`'s `Lower for Vec<T>`); on the Swift side the bytes type is already one
  read into `Data`. `obs_ffi::TilePixels(Vec<u8>)` crosses as that same bytes type with one
  copy: it is the crate's one `unsafe impl` (`FfiConverter`, an unsafe trait; the impl does
  no unsafe operation), with the same `TYPE_ID_META` as `Vec<u8>` and a byte-identical wire
  format (a big-endian `i32` count, then the bytes). Approved in S13 (ADR 0021 foresaw an
  FFI-only exception). `tests::tile_pixels_cross_as_vec_u8_does` checks the wire bytes
  against `Vec<u8>`'s (a real 3× tile, an empty one, one byte) so a UniFFI upgrade that
  changes the format fails; `tests/mobile_app.rs` checks a 3× tile lowers in a fraction of
  the per-item time; `cargo xtask ios bindgen` fails unless Swift still reads `Tile.rgba`
  with `FfiConverterData`; `cargo xtask layers` fails on any other `unsafe` (or
  `allow(unsafe_code)`) in `apps/obs-ffi`.
- **`uniffi-bindgen`** (`apps/uniffi-bindgen`). UniFFI's `uniffi_bindgen_main()` at the
  version `obs-ffi` uses, as the UniFFI manual describes. It is a crate of its own so that
  the `cli` and `cargo-metadata` features (clap, askama, goblin …) stay out of `obs-ffi`'s
  graph, which is what ships. Nothing is installed globally.
- **Layers.** `FFI_CRATES = ["uniffi"]` in `xtask/src/layers.rs`. No library may depend on
  it, directly or transitively. Only apps may, as with networking crates.
- **Tiles (plan A4, option B).** `obs_render::composite(quads, atlas, colours, tile, scale)`
  fills a tile with `RoleColours::background` and draws glyph quads over it, each in its
  role's colour with the atlas coverage as alpha (premultiplied, source over);
  `composite_over` draws more quads into the same pixels. A tile's pixel size
  (`tile_size`) runs from the device pixel its top-left edge rounds to up to the one its
  bottom-right edge rounds to, so tiles that meet at a fractional offset share no pixel row
  and leave none out; a size whose bytes overflow a `usize` gives an empty tile.
  `obs_reading::compose_block_tile` lays out one block, places its glyphs in the atlas
  (starting it over between parts when it fills, as the desktop pane does) and composites
  a band of it in `TonePalette::role_colours()`. The text column is centred between
  `view::MARGIN`s as on the desktop, with no scrollbar. Not drawn yet: Highlights and other
  Marks (S17), the selection (the shell draws it), Note markers (S18; their room is made).
- **Cold start.** A Bible's document opens faster for everyone (the `.obr` reader asks each
  unit's id once per run of Verses or Keys, not once per row; `ReadingDoc::build` maps
  Verses to units through a map), and the Mobile app measures the reading view's height
  estimates on up to four threads (`ReadingView::with_estimate_threads`; the desktop keeps
  one, and the estimates are the same either way).
- **`Session` is `Send`.** `obs_engine::UserData: Send`, so `Box<dyn UserData>` no longer
  makes the Session `!Send`. `session::tests::session_is_send` checks this at compile time.
  `obs-ffi` holds the Session behind a `Mutex` (A3), so it must stay `Send`.

## Commands

`cargo xtask ios …` (`xtask/src/ios.rs`). None of these is a gate of `cargo xtask ci`, which
never needs macOS or Xcode.

| Command | Needs | Does |
|---|---|---|
| `ios bindgen` | a host toolchain | `cargo build -p obs-ffi --lib` (host cdylib), then `cargo run -p uniffi-bindgen -- generate <lib> --language swift --no-format --out-dir target/ios/bindings` (library mode). It clears the folder first and fails if any of the three files is missing, or if `ObsFFI.swift` no longer reads `Tile.rgba` as `Data` (the bulk byte path). |
| `ios check` | a host toolchain (Linux too) | `cargo check -p obs-ffi`, then `ios bindgen`. |
| `ios smoke [--bundle DIR]` | `swiftc` (Xcode or the Command Line Tools) and a Bundled set | Fails, saying to run `ios bundled-set`, unless `DIR` (default `apps/ios/Bundled/starter`) has a `starter-manifest.json`. Runs `ios bindgen`, then `swiftc -Xcc -fmodule-map-file=target/ios/bindings/ObsFFIFFI.modulemap -L target/debug -lobs_ffi -Xlinker -rpath -Xlinker target/debug` over `apps/obs-ffi/swift-smoke/main.swift` and `ObsFFI.swift` into `target/ios/swift-smoke/swift-smoke`, and runs it with `DIR` and a fresh `target/ios/swift-smoke/run/`. The smoke boots in that folder, goes to Genesis 1, takes the largest Dynamic Type size, asks for a frame of a 390 × 700 pt phone at 3× and draws a whole 256 pt band; the command fails unless it prints `engine <version>` and `tile 1170x768`. |
| `ios build [--sim-only]` | Xcode with the iOS SDKs | Probes `xcrun --sdk iphoneos --show-sdk-path` (and `iphonesimulator`). If one fails, it stops with "`cargo xtask ios build` needs Xcode: …", builds nothing and exits non-zero. Otherwise it runs `ios bindgen`, then for each slice `cargo rustc -p obs-ffi --lib --profile ios --target <t> --crate-type staticlib` with `IPHONEOS_DEPLOYMENT_TARGET=17.0` (the app's minimum), puts the header and module map (renamed `module.modulemap`) in `target/ios/headers`, and runs `xcodebuild -create-xcframework` into `target/ios/ObsFFI.xcframework`. Then it copies the XCFramework and `ObsFFI.swift` into the app's package `apps/ios/Packages/OpenBibleCore` (only the files that changed; `docs/dev/ios-project.md`). The slices are `aarch64-apple-ios` and `aarch64-apple-ios-sim`, or the simulator's alone with `--sim-only`. |

The staticlibs are built with `--crate-type staticlib`, so no iOS cdylib is linked.
`rusqlite`'s bundled SQLite is C code that `cc` compiles with the iOS SDK. That is why
`build` needs Xcode and `check` does not. The SDK check takes the probe as a parameter, so its
tests run on every OS without `xcrun`.

`target/ios/` is also where cargo puts the host build scripts and proc-macros of a
`--profile ios` cross build (`build/`, `deps/`, `.fingerprint/` …). Our outputs (`bindings/`,
`headers/`, `ObsFFI.xcframework/`) sit beside those under names cargo does not use. All of
it is under the ignored `/target`.

## Data

- **`[profile.ios]`** (`Cargo.toml`, D20) inherits `release` with `lto = "fat"`,
  `codegen-units = 1`, `opt-level = "s"` and `strip = "debuginfo"`. It keeps
  `panic = "unwind"`, because the job queue catches a panicking job
  (`crates/engine/src/jobs.rs`) and `abort` would turn that into an app crash. The xtask test
  `ios::tests::the_ios_profile_unwinds` checks this.
- **Licences (D21).** `cargo xtask licenses` writes a second listing,
  `crates/engine/data/third-party-licenses-ios.json`. It covers `apps/obs-ffi/Cargo.toml`
  for `aarch64-apple-ios` and `aarch64-apple-ios-sim` (cargo-about `--target`, which reads
  only metadata, so any host works), and the `licenses` gate checks it as it checks the
  desktop file. `about.toml` accepts `MPL-2.0`, last in the list, for UniFFI's runtime only.
  The test `licenses::tests::only_uniffi_is_taken_under_mpl_2_0_and_only_on_ios` fails if any
  other crate, or anything in the desktop listing, is taken under MPL-2.0. The engine does
  not embed the iOS listing yet; the iOS Credits screen will (S34).
- **Lints.** Every crate but `obs-ffi` inherits `[lints] workspace = true`
  (`unsafe_code = "forbid"` and the three clippy denies). UniFFI 0.32's proc-macro
  scaffolding compiles under `forbid` (edition 2024, rust-version 1.95); S02 to S07 needed no
  exception. **S13 took the one exception** for the tiles' bulk byte path (`TilePixels`,
  above), approved by the supervisor: `obs-ffi` copies the workspace's table with
  `unsafe_code = "deny"` (Cargo cannot loosen one inherited lint) and the three clippy
  denies, and the only `#[allow(unsafe_code)]` is on that impl, which `cargo xtask layers`
  enforces. Any other hand-written `unsafe` needs the owner's approval first.
- **Presentation-only decisions accepted in Swift** (A10). The Swift smoke is a host test
  program, not app code. In the app (`apps/ios`, `docs/dev/ios-project.md`), from S06:
  - From S06, now on S13's surface: `AppCore.State` (starting, ready, failed) and the
    screens' `switch` over it: what shows while the core boots, once it has, or when it
    failed. The About sheet (`AboutView`, S06's hello screen) shows the count of `library()`
    and the ids in the order the core returns them, a Problems section only when
    `problems()` has any, and a failed boot's error text as the core worded it. `boot` runs
    off the main thread (`Task.detached`), so the screen keeps drawing.
  - From S13 (`AppCore`): the core boots once per process and every scene shares it (an
    iPad can open several windows); Swift passes one folder, `Application
    Support/OpenBible`, and the core names everything in it (S06's `HelloModel.paths(in:)`
    is gone); Swift sets `isExcludedFromBackup` on exactly the folders `backupExcluded()`
    lists; the core's observer is called on the pump thread and hops to the main actor;
    the app's scene phase `background` calls `onBackground()` and `active` calls
    `onResume()`; `AppCore.message` shows an error's words as the core wrote them. On the
    reader screen (`ReaderModel`): it shows reader 0; a `Changed.reader` for it redraws the
    text view, `Changed.library` reloads the core's choices, `Changed.message` is logged
    (no toast before S14a); a tap opens the sheet stub when the core's hit has a Verse (a
    Footnote marker's popover is S14b); the grid passes the core's book id back; the tile
    renderer times the FFI call, the image and its preparation apart (instrumentation).
  - From S07 (the text view prototype, `docs/dev/mobile-text-view.md`): the scroll view
    clamps its offset to the page before sending it (rubber-banding stays UIKit's) and moves
    to the position the core reports; a missing tile in view is drawn at once and the others
    on a serial queue; tiles are shown 1:1 (`contentsGravity .topLeft`, pixel size ÷ scale);
    the page colour, Page tone and Bible choices and their labels, Verse labels, location,
    sheet text and counts are the core's; `UITextGranularity` is renamed 1:1 to the core's
    `TextUnitKind` (the core decides, in `rangeEnclosingPosition`, that a word is a word and a
    sentence, line or paragraph a Verse); a tap clears a selection before asking the core
    what it opens; `.header` when the core marks a heading;
    the system's dark appearance and content size category are passed to the core, which
    resolves Auto and the Text size; Return in the reference field means Go; 44 pt hit areas
    and HIG spacing; and, as a test harness only, debug launch arguments, the
    `OBS_STUDY_SET_DIR` variable (Debug builds), the fling bench stopping when the core's
    location label starts with the book it was given, and frame-drop counting from
    `CADisplayLink`.
  - From S26 (Import, `docs/dev/mobile-library.md` § Import): the picker's types are the
    three `project.yml` declares (`ImportTypes`), and what a file holds is the core's to
    tell; Swift copies a picked or shared file to the path `import_destination` names,
    inside its security-scoped access and a coordinated read, and deletes a file the
    system put in `Documents/Inbox` for "Open in" once the copy is done, worked or not
    (Apple's folder, the app's copy; both paths compared with symlinks resolved, `/var` and
    `/private/var`); a file shared before the core booted waits for it; a copy that failed is handed
    to the core (`import_copy_failed`), whose report says so; the report sheet's `switch`
    over `ImportState` chooses the view (progress and Cancel, the status, or the failure
    and Details); sections fold where the core's `collapsed` says; a warning line is
    semibold; section titles are a list's first row in `.headline` and the file name is
    `.headline` (a grouped list's header and secondary text fail the audit's contrast,
    S06); Done, Open, Cancel and Details are Swift's labels, like the menu's "Import…";
    Open shows the core's Resource in reader 0. As a test harness only, the Debug
    `OBS_TEST_APP_FOLDER` variable boots in a fresh folder of that name.
  - Accepted S07 prototype gaps, where Swift decides more than presentation
    (`docs/dev/mobile-text-view.md` "Decided in Swift"; S14a closes the first two):
    - `CoreTokenizer` overrides only `rangeEnclosingPosition`; the tokenizer's boundary
      answers are UIKit's `UITextInputStringTokenizer`, so word snapping during a handle drag
      is UIKit's, not the core's.
    - Text direction: `position(from:in:offset:)` maps left and up to a negative offset and
      the writing direction answers are `.natural`, decided in Swift, not by the core's
      `doc.direction()`, so they are wrong for right-to-left text (the TAHOT).
    - Swift-authored labels ("Word", "Strong's", "Lemma", "Gloss", "Verse", "No Bible in the
      library", the menu, placeholder and accessibility labels) and chrome colours (`.red`
      for the reference error, `.secondarySystemBackground` for the reference field).
- **Tests.** CI-run, over `obs_engine::testing` fixtures: `obs-render` `composite::tests`
  (a known glyph's pixels are its atlas coverage in the ink, nothing outside it; each role's
  colour; premultiplied; clipping and tiles lower down; tiles at fractional offsets stack
  into the whole; an overflowing size; a wrong-length buffer left alone), `obs-reading`
  `tile::tests` (a 1170 × 768 phone tile is page colour in the margins with ink in the
  column, in Paper and Dark; two half tiles are the whole, and so are tiles at fractional
  tops) and `view::tests` (estimates on several threads equal those on one), `obs-mobile`
  `tests` and `paths`/`settings` tests (jobs inline, over a Bundled set of the BSB's Isaiah 40
  and John 1, a WEB excerpt, a TAHOT excerpt and a Greek Lexicon: the first launch installs
  it and opens the first Bible at its start; a relaunch restores both readers; a snapshot of
  another format opens the default reader; `Jn 3:16` and `1 Co 13` parse in the reader's
  numbering; Back and Forward; the TAHOT starts right to left; a parallel switch and Back
  across it; Lexicon Keys; the grid; History and its Visits; settings through the View
  commands and kept; Dynamic Type's bounds and place; a dark system; frames, tiles, the tile
  cap; hits on a Verse, a Tagged word and a Footnote marker; room for a Note's marker; the
  text input and accessibility answers; refusals; the pump), `apps/obs-ffi` (the bulk byte
  path's wire bytes; `tests/mobile_app.rs`: the surface through the FFI types, a 3× tile's
  bulk crossing, the pump thread reaching an `Observer`) and xtask (`layers`' unsafe check,
  `ios`' `Data` check). By hand, after `cargo xtask ios bundled-set --from dist/starter`:
  `cargo test -p obs-mobile -- --ignored --nocapture` (the same over the real Bundled set,
  and the cold-start timing against its target) and
  `cargo test -p obs-ffi --test mobile_app -- --ignored --nocapture` (writes
  `target/ios/gen1.png`, Genesis 1 in the BSB, 390 × 768 pt at 3×, from the frame's tiles).

## Evidence

Measured on 2026-11-21 on the owner's Mac (Apple M6, macOS 27.0.1, rustc 1.99.1, Xcode 27.2):

- UniFFI **0.32.2**. Every `uniffi*` crate's `Cargo.toml` says `license = "MPL-2.0"`, and its
  parser dependency `weedle2` is MIT. UniFFI 0.32.2 declares no `rust-version`, so cargo's
  resolver had nothing to check; it was built with stable rustc 1.99.1 only. Whether it
  builds on the workspace's `rust-version = "1.95"` floor is unverified.
- `cargo xtask ios bindgen` writes `ObsFFI.swift` (19.8 kB), `ObsFFIFFI.h` and
  `ObsFFIFFI.modulemap`.
- `cargo xtask ios build`, the first time: about 66 s for the device slice and 35 s for the
  simulator slice, from a warm host build. Each `libobs_ffi.a` is 7.5 MB, with fat LTO over
  today's one-function surface. After that, `ios build` takes about 9 s.
- With the Command Line Tools selected instead of Xcode
  (`DEVELOPER_DIR=/Library/Developer/CommandLineTools`), `cargo xtask ios build` prints the
  "needs Xcode" message naming both `xcrun` probes and exits 1.
- A link check (not committed): `xcrun --sdk iphonesimulator swiftc -target
  arm64-apple-ios17.0-simulator` compiled a one-line `main.swift` with `ObsFFI.swift`
  against the XCFramework's simulator slice. Run with `simctl spawn` on an iPhone 17e
  simulator (iOS 27.2), it printed `engine 0.2.0`.
- The iOS licence listing has 119 crates. MPL-2.0 covers `uniffi`, `uniffi_core`,
  `uniffi_internal_macros`, `uniffi_macros`, `uniffi_meta` and `uniffi_pipeline` only. Neither
  `uniffi_bindgen` nor `clap` is in it. The desktop listing did not change.

S03 ("Hello engine"), measured on 2026-11-21 on the same Mac, with three other lanes building
(load average about 14):

- `target/ios/gen1.png` (1170 × 2304) reads as Genesis 1 in the BSB on the Paper tone: the
  large accent-coloured chapter numeral 1 and verses 1–2, the "The First Day" heading in the
  accent colour, verses 3–5 with their verse numbers in the verse-number colour and Footnote
  callers a and b, "The Second Day" and the start of verse 6, in Gentium Plus, sharp at 3×.
- Release (`cargo test --release`): a warm full-width 3× tile (390 × 256 pt, 1170 × 768
  px; already laid out and rastered, so background fill, quads and the composite) takes
  0.42–0.48 ms median and at most 0.75 ms over 20 tiles, three runs. The first
  1170 × 2304 tile, laying out and rastering Genesis 1:1–6 too, takes 2.5–2.9 ms. Far under
  the plan's 8 ms per tile, so option B stands for S07.
- The test profile (`opt-level = 1` for workspace crates): the warm tile 2.5–3.4 ms median,
  at most 7.9 ms; the first tile 8.8–13.4 ms.
- `boot` over the Bundled set into an empty folder (copying its 43.1 MB): 0.55–1.4 s.
  Opening the whole BSB as a `ReadingDoc` (`open_reader`): 0.50–1.2 s, which S07 and S13
  should watch on a phone.
- `cargo xtask ios smoke`: `swiftc` takes about 1.5 s, the run about 1.3 s, printing
  `engine 0.2.0`, `resources bsb openbible-xrefs strongs-hebrew strongs-greek` and
  `tile 1170x768`. The Command Line Tools' own `swiftc` (Swift 6.4,
  `DEVELOPER_DIR=/Library/Developer/CommandLineTools`) builds and runs it the same way.
  `-I target/ios/bindings` alone does not find the module (`cannot find
  'uniffi_obs_ffi_fn_func_engine_version' in scope`), so the command names the module map.
  `-lobs_ffi` links the host `libobs_ffi.dylib` (the cdylib), hence the rpath.
- The iOS licence listing did not change (still 119 crates): `obs-render` and
  `obs-reading`'s dependencies were already in `obs-ffi`'s graph.

S13 (the designed surface), measured on 2026-11-21 on the same Mac (load average 4–11 from
other lanes):

- **Cold start, host.** Before S13, `boot` of a second launch took 0.73 s (release): the
  whole BSB's `ReadingDoc::open` about 0.45 s, `ReadingView::new`'s height estimates about
  0.28 s. Opening the BSB's document now takes 60 ms instead of 220 ms (release; the per-row
  unit-id queries were 80 ms, the Verse-to-unit search about 90 ms) and its estimates 70 ms
  on four threads instead of 200 ms on one. A second launch's boot and first frame:
  0.14–0.15 s in release, 0.22 s in the test profile, against the 0.4 s target the ignored
  test `a_second_launch_boots_and_draws_its_first_frame_within_the_target` writes down (the
  first launch, copying the Bundled set: 0.17 s release, 0.25 s test profile).
- **Cold start, simulator** (iPhone 17, iOS 27.2, Release, the fling bench's launch
  argument, four launches): cold start to first text 594–605 ms after the first launch
  (999 ms, which copies the Bundled set), against S07's 1.53–1.95 s; `start()` at 262–275
  ms after process start, booted at 533–545 ms. `AppCoreTests` (Debug): first boot over the
  Bundled set 0.36 s, second 0.35 s (S06: 1.8–1.9 s and 1.6–1.8 s).
- **Tile bytes.** Lowering a 1170 × 768 tile (3.6 MB): `Vec<u8>` per item 1.3–1.5 ms
  (release) and 5.7 ms (test profile); `TilePixels` 76 µs (test profile), with the same
  bytes. In the simulator's fling bench (8000 pt/s, Genesis–Exodus, 2778 tiles a run) the
  Swift side of a tile, split as the S07 review asked: the FFI call (the core's drawing and
  the pixels' crossing) p50 1.3–2.0 ms, p95 6.2–6.5 ms, of which the core's drawing is
  p50 0.47–0.70 ms, p95 2.6–2.8 ms; making the `CGImage` p50 5–10 µs; preparing it for
  display p50 0.26–0.38 ms, p95 1.0–1.1 ms. S07 timed all three as one span: 3.6–5 ms p50.
  Frames dropped 0–0.11 %. (The core's drawing ran slower than S07's 0.17–0.3 ms under this
  run's load; S14a measures on the device.)
- `cargo xtask ios smoke` prints `engine 0.2.0`, `resources bsb openbible-xrefs
  strongs-hebrew strongs-greek`, `reader bsb at Genesis 1:1` and `tile 1170x768`.
- `libobs_ffi.a` (simulator) 27.9 MB (S07: 27.4 MB). The generated `ObsFFI.swift` is
  134.6 kB.
