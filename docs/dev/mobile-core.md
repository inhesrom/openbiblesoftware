# Mobile core (ADR 0021)

The Mobile app (CONTEXT.md § Mobile) is a native shell (SwiftUI on iOS) over Rust. Its Rust
side is two crates and a tool. What each screen shows and what each action does lives in
`obs-mobile`; the shell lays out, draws and forwards input (ADR 0021 rule 3). The plan they
follow is `docs/plans/mobile-ios.md` (A1–A3). This page has the scaffold that S02 built. S13
fills in the screens.

## Contract

- **`obs-mobile`** (`crates/mobile`, L5). The Mobile app's screen models and actions as plain
  Rust types. It depends on `obs-engine` (and on `obs-reading` once that crate exists). It
  has no FFI crate (`cargo xtask layers`), compiles for wasm32 (`cargo xtask wasm`), keeps
  `unsafe_code = "forbid"`, and is tested by `cargo xtask test` on every CI OS. Today it
  holds `Paths` (the folders the shell picks under Application Support) and a `MobileApp`
  placeholder.
- **`obs-ffi`** (`apps/obs-ffi`, an app). The one UniFFI crate Swift links, in proc-macro
  mode (`uniffi::setup_scaffolding!()`, `#[uniffi::export]`, no UDL). Its crate types are
  `staticlib` (iOS), `cdylib` (the host library bindgen reads) and `lib` (Rust tests). It
  only converts and forwards: every rule stays in `obs-mobile`. The types that cross are
  mirror Records and Enums defined here with `From<obs_mobile::…>` (D9), so `obs-mobile`
  never needs `uniffi`. Today it exports `engine_version() -> String`. Its
  `uniffi.toml` names the Swift module `ObsFFI`, so the bindings are `ObsFFI.swift`, with
  `ObsFFIFFI.h` and `ObsFFIFFI.modulemap` for UniFFI's C module `ObsFFIFFI`.
- **`uniffi-bindgen`** (`apps/uniffi-bindgen`). UniFFI's `uniffi_bindgen_main()` at the
  version `obs-ffi` uses, as the UniFFI manual describes. It is a crate of its own so that
  the `cli` and `cargo-metadata` features (clap, askama, goblin …) stay out of `obs-ffi`'s
  graph, which is what ships. Nothing is installed globally.
- **Layers.** `FFI_CRATES = ["uniffi"]` in `xtask/src/layers.rs`. No library may depend on
  it, directly or transitively. Only apps may, as with networking crates.
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
| `ios build [--sim-only]` | Xcode with the iOS SDKs | Probes `xcrun --sdk iphoneos --show-sdk-path` (and `iphonesimulator`). If one fails, it stops with "`cargo xtask ios build` needs Xcode: …", builds nothing and exits non-zero. Otherwise it runs `ios bindgen`, then for each slice `cargo rustc -p obs-ffi --lib --profile ios --target <t> --crate-type staticlib`, puts the header and module map (renamed `module.modulemap`) in `target/ios/headers`, and runs `xcodebuild -create-xcframework` into `target/ios/ObsFFI.xcframework`. The slices are `aarch64-apple-ios` and `aarch64-apple-ios-sim`, or the simulator's alone with `--sim-only`. |

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
- **Presentation-only decisions accepted in Swift** (A10): none yet.

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
