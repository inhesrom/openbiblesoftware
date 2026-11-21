# Mobile core (ADR 0021)

The Mobile app (CONTEXT.md § Mobile) is a native shell (SwiftUI on iOS) over Rust. Its Rust
side is two crates and a tool. What each screen shows and what each action does lives in
`obs-mobile`; the shell lays out, draws and forwards input (ADR 0021 rule 3). The plan they
follow is `docs/plans/mobile-ios.md` (A1–A3). This page has the scaffold that S02 built and
the "Hello engine" spike of S03. S13 replaces the spike and fills in the screens.

## Contract

- **`obs-mobile`** (`crates/mobile`, L5). The Mobile app's screen models and actions as plain
  Rust types. It depends on `obs-engine`, `obs-model`, `obs-reading`, `obs-render`,
  `obs-scripture` and `obs-store`. It has no FFI crate (`cargo xtask layers`), compiles for
  wasm32 (`cargo xtask wasm`; `boot` and `open_reader` read files and are native only), keeps
  `unsafe_code = "forbid"`, and is tested by `cargo xtask test` on every CI OS. Today it holds `Paths` (the folders the shell
  picks under Application Support) and the S03 spike of `MobileApp` (below).
- **`obs-ffi`** (`apps/obs-ffi`, an app). The one UniFFI crate Swift links, in proc-macro
  mode (`uniffi::setup_scaffolding!()`, `#[uniffi::export]`, no UDL). Its crate types are
  `staticlib` (iOS), `cdylib` (the host library bindgen reads) and `lib` (Rust tests). It
  only converts and forwards: every rule stays in `obs-mobile`. The types that cross are
  mirror Records and Enums defined here with `From<obs_mobile::…>` (D9), so `obs-mobile`
  never needs `uniffi`. Today it exports `engine_version() -> String` and the S03 spike. Its
  `uniffi.toml` names the Swift module `ObsFFI`, so the bindings are `ObsFFI.swift`, with
  `ObsFFIFFI.h` and `ObsFFIFFI.modulemap` for UniFFI's C module `ObsFFIFFI`.
- **`uniffi-bindgen`** (`apps/uniffi-bindgen`). UniFFI's `uniffi_bindgen_main()` at the
  version `obs-ffi` uses, as the UniFFI manual describes. It is a crate of its own so that
  the `cli` and `cargo-metadata` features (clap, askama, goblin …) stay out of `obs-ffi`'s
  graph, which is what ships. Nothing is installed globally.
- **Layers.** `FFI_CRATES = ["uniffi"]` in `xtask/src/layers.rs`. No library may depend on
  it, directly or transitively. Only apps may, as with networking crates.
- **The S03 spike** (`#[doc(hidden)]`, marked `S03 spike`; S13 replaces it with the designed
  surface). `obs_mobile::MobileApp::boot(paths, bundle)` starts as the desktop does
  (`obs_ui_egui::App::with_file`): it makes the `Paths` folders, creates a
  `Session` with `JobMode::default_for_host()` (threads), scans the library, sets the Search
  index, alias and log folders, opens or creates the User database, calls
  `sync_starter_library(bundle)`, and opens the first Bible in Preferred order as reader 0.
  It restores nothing: the Mobile Last session is S13's. `open_reader(resource_id,
  reference)` opens a `ReadingView` at the reference's first Verse (read in the Resource's
  own numbering). `layout_tile(reader, top, width, height, scale)` draws a full-width tile
  of the reader's page, `top` points below its Current location, with
  `obs_reading::compose_tile` in the Page tone of the User database (Auto as in a light
  Theme), and refuses a tile over `MAX_TILE_PX` (8192) device pixels on a side, since a
  failed allocation aborts the app. `obs-ffi` mirrors it as the Object `MobileApp` (a `Mutex` around
  `obs_mobile::MobileApp`; constructor `boot`, then `problems`, `resourceIds`,
  `readerResource`, `openReader`, `layoutTile`), the Records `Paths` and `Tile { width,
  height, rgba }` (Swift `Data`, premultiplied RGBA8), and the Error `MobileError.Failed {
  message }`.
- **Tiles (plan A4, option B).** `obs_render::composite(quads, atlas, colours, tile, scale)`
  fills a tile with `RoleColours::background` and draws glyph quads over it, each in its
  role's colour with the atlas coverage as alpha (premultiplied, source over);
  `composite_over` draws more quads into the same pixels. A tile's pixel size
  (`tile_size`) runs from the device pixel its top-left edge rounds to up to the one its
  bottom-right edge rounds to, so tiles that meet at a fractional offset share no pixel row
  and leave none out; a size whose bytes overflow a `usize` gives an empty tile.
  `obs_reading::compose_tile` lays
  out what the tile shows, places the glyphs in the atlas (starting it over between parts
  when it fills, as the desktop pane does) and composites them in
  `TonePalette::role_colours()`. The text column is centred between `view::MARGIN`s as on
  the desktop, with no scrollbar. Not drawn yet: Highlights and other Marks, the selection,
  Note markers' room.
- **`Session` is `Send`.** `obs_engine::UserData: Send`, so `Box<dyn UserData>` no longer
  makes the Session `!Send`. `session::tests::session_is_send` checks this at compile time.
  `obs-ffi` holds the Session behind a `Mutex` (A3), so it must stay `Send`.

## Commands

`cargo xtask ios …` (`xtask/src/ios.rs`). None of these is a gate of `cargo xtask ci`, which
never needs macOS or Xcode.

| Command | Needs | Does |
|---|---|---|
| `ios bindgen` | a host toolchain | `cargo build -p obs-ffi --lib` (host cdylib), then `cargo run -p uniffi-bindgen -- generate <lib> --language swift --no-format --out-dir target/ios/bindings` (library mode). It clears the folder first and fails if any of the three files is missing. |
| `ios check` | a host toolchain (Linux too) | `cargo check -p obs-ffi`, then `ios bindgen`. |
| `ios smoke [--bundle DIR]` | `swiftc` (Xcode or the Command Line Tools) and a Bundled set | Fails, saying to run `ios bundled-set`, unless `DIR` (default `apps/ios/Bundled/starter`) has a `starter-manifest.json`. Runs `ios bindgen`, then `swiftc -Xcc -fmodule-map-file=target/ios/bindings/ObsFFIFFI.modulemap -L target/debug -lobs_ffi -Xlinker -rpath -Xlinker target/debug` over `apps/obs-ffi/swift-smoke/main.swift` and `ObsFFI.swift` into `target/ios/swift-smoke/swift-smoke`, and runs it with `DIR` and a fresh `target/ios/swift-smoke/run/`. The smoke boots, opens the BSB at Genesis 1 and lays out a 390 × 256 pt tile at 3×; the command fails unless it prints `engine <version>` and `tile 1170x768`. |
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
- **Lints.** `obs-ffi` keeps `[lints] workspace = true`, so `unsafe_code = "forbid"` and the
  three clippy denies still apply. UniFFI 0.32's proc-macro scaffolding compiles under
  `forbid` (edition 2024, rust-version 1.95), so **no exception is needed**. Hand-written
  `unsafe` in `obs-ffi` needs the owner's approval first.
- **Presentation-only decisions accepted in Swift** (A10). The Swift smoke is a host test
  program, not app code. In the app (`apps/ios`, `docs/dev/ios-project.md`), from S06:
  - `HelloModel.State` (starting, ready, failed) and the hello screen's `switch` over it:
    which section the screen shows while the core boots, once it has, or when it failed.
  - The hello screen shows the count of `resourceIds()` and the ids in the order the core
    returns them, a Problems section only when `problems()` has any, and a failed boot's
    error text as the core worded it.
  - `boot` runs off the main thread (`Task.detached`), so the screen keeps drawing.
  - `HelloModel.paths(in:)` names the core's files under `Application Support/OpenBible`
    (`library`, `user.db`, `search`, `aliases`, `logs`, `downloads`, as the Swift smoke
    does), because plan A7 has the shell pass all six `Paths`. It is file naming, on A10's
    list: accepted for the S03 spike surface; S13 should decide whether the designed surface
    takes one root folder and names the rest in `obs-mobile`.
- **Tests.** CI-run, over `obs_engine::testing` fixtures: `obs-render` `composite::tests`
  (a known glyph's pixels are its atlas coverage in the ink, nothing outside it; each role's
  colour; premultiplied; clipping and tiles lower down; tiles at fractional offsets stack
  into the whole; an overflowing size; a wrong-length buffer left alone), `obs-reading`
  `tile::tests` (a 1170 × 768 phone tile is page colour in the margins with ink in the
  column, in Paper and Dark; two half tiles are the whole, and so are tiles at fractional
  tops), `obs-mobile` `tests` (boot over a 2-Resource Bundled set and again, readers,
  refusals including tiles over `MAX_TILE_PX`, boot with nothing) and
  `apps/obs-ffi/tests/hello_engine.rs` (the same through the FFI types). By hand, after
  `cargo xtask ios bundled-set --from dist/starter`:
  `cargo test -p obs-ffi --test hello_engine -- --ignored --nocapture` boots over the real
  Bundled set, writes `target/ios/gen1.png` (Genesis 1 in the BSB, 390 × 768 pt at 3×) and
  prints timings.

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
