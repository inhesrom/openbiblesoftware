# iOS project (`apps/ios`)

The Mobile app's iOS shell: a SwiftUI app over the Rust core (`docs/dev/mobile-core.md`,
ADR 0021). The plan is `docs/plans/mobile-ios.md` (A2, A6). S06 built the scaffold described
here: the project builds and tests from the command line, links `obs-ffi`, boots the core over
the Bundled set and shows the engine's version and the library. There is no reader yet (S07,
S13, S14a).

## Layout

| Path | Committed | What |
|---|---|---|
| `apps/ios/project.yml` | yes | The Xcode project, for XcodeGen 2.46. `xcodegen generate` writes `OpenBible.xcodeproj` from it. |
| `apps/ios/OpenBible/` | yes | The app's Swift sources and `PrivacyInfo.xcprivacy`. Info.plist keys are build settings in `project.yml` (`GENERATE_INFOPLIST_FILE`). |
| `apps/ios/OpenBibleTests/` | yes | XCTest unit tests, hosted in the app, against the real core. |
| `apps/ios/OpenBibleUITests/` | yes | XCUITest launch smoke with a screenshot attachment. |
| `apps/ios/Packages/OpenBibleCore/Package.swift` | yes | The local Swift package the app links: a binary target `ObsFFI` (`ObsFFI.xcframework`) and the Swift target `OpenBibleCore` (`Sources/Generated/ObsFFI.swift`, which imports the C module `ObsFFIFFI`). Swift code says `import OpenBibleCore`. |
| `apps/ios/Packages/OpenBibleCore/ObsFFI.xcframework/`, `Sources/Generated/` | no | Copied there by `cargo xtask ios build`. |
| `apps/ios/Bundled/starter/` | no | The Bundled set, copied by `cargo xtask ios bundled-set` and checked against `apps/ios/bundled-set.lock` (`docs/dev/starter-library.md`, "Mobile sets"). The app bundles it as the folder `starter`. |
| `apps/ios/OpenBible.xcodeproj` | no | Written by `xcodegen generate`. |

**Gitignore policy:** the project, the XCFramework, the generated Swift and the Bundled set are
build artefacts and are rebuilt by the commands below; `.gitignore` lists each. Only the
project spec, the Swift sources, `Package.swift`, the privacy manifest and the lock are
committed. `apps/ios` has no `Cargo.toml`; the workspace excludes it (`Cargo.toml`
`exclude = ["apps/ios"]`).

## Build, test, screenshot

From the repository root, with Xcode selected (`xcode-select -p`) and `xcodegen` installed:

```sh
cargo xtask ios bundled-set            # once, and after a Starter build: needs dist/starter
cargo xtask ios build --sim-only       # the simulator slice; without --sim-only, the device's too
cd apps/ios
xcodegen generate
xcodebuild build -scheme OpenBible \
  -destination 'platform=iOS Simulator,name=iPhone 17' \
  -derivedDataPath ../../target/ios/xcode
xcodebuild test -scheme OpenBible \
  -destination 'platform=iOS Simulator,name=iPhone 17' \
  -derivedDataPath ../../target/ios/xcode
```

`xcodebuild test` runs both test targets (the scheme's test action lists `OpenBibleTests` and
`OpenBibleUITests`). Pick the simulator name from `simctl list devices available`. `-derivedDataPath`
keeps Xcode's output under the ignored `target/`. The package must hold the XCFramework and
`ObsFFI.swift` before Xcode resolves it, which is why `ios build` comes before the first
`xcodebuild`.

A screenshot for review (ADR 0020) goes in `docs/previews/ios/<step>-<surface>.png`:

```sh
xcrun simctl boot 'iPhone 17'
xcrun simctl install booted target/ios/xcode/Build/Products/Debug-iphonesimulator/OpenBible.app
xcrun simctl launch booted io.github.inhesrom.openbible.dev
xcrun simctl io booted screenshot docs/previews/ios/s06-hello.png
```

The UI test also attaches a screenshot (`s06-hello`) to the test results
(`-resultBundlePath`).

## The run-script phase

The app target's first build phase, "Rust core and Bundled set", runs
`cargo xtask ios build` (with `--sim-only` when `PLATFORM_NAME` is `iphonesimulator`) and then
`cargo xtask ios bundled-set --verify`, so a build from Xcode picks up Rust changes and refuses
a Bundled set that does not match the lock.

- **Clean environment.** Xcode's build PATH has no `~/.cargo/bin`, and its `SDKROOT` and
  deployment settings would reach cargo's host build scripts. The phase runs
  `${CARGO:-$HOME/.cargo/bin/cargo}` under `env -i` with `HOME`, `USER`, `TMPDIR`, a PATH that
  starts with that cargo's own folder and then `~/.cargo/bin`, and `DEVELOPER_DIR` (so `xcrun`
  finds the same Xcode). It also
  passes `RUSTUP_HOME`, `CARGO_HOME`, `RUSTUP_TOOLCHAIN` and the proxy variables when they are
  set. The phase runs under `/bin/bash`, because it uses arrays and `pipefail`. Where Rust is not
  under `~/.cargo` (GitHub's macOS runners), set `CARGO` (and `CARGO_HOME`/`RUSTUP_HOME`) in the
  environment of `xcodebuild`, as `ios.yml` does.
- **Before the first device build or archive, run `cargo xtask ios build` (no `--sim-only`).** A
  simulator build leaves only the simulator slice in the package. Xcode reads the XCFramework
  before this phase runs, so a device build straight after a simulator build has no device
  slice, and the phase that would add it never gets to run.
- **One build late, so it fails instead.** Xcode copies the package's XCFramework into the
  build products (`ProcessXCFramework`) and compiles `OpenBibleCore` before any target's
  phases run, so a library this phase rebuilds would only be linked by the next build. The
  phase therefore fails with "the Rust core changed after Xcode had read it; build again"
  whenever it changed a file in the package, and the next build is consistent. Building with
  `cargo xtask ios build` before `xcodebuild` (as above) never meets it.
- **Unchanged means untouched.** `ios build` copies into the package only the files whose bytes
  differ (`mirror` in `xtask/src/ios.rs`), so an unchanged core costs Xcode nothing. The
  XCFramework's root `Info.plist` (no other file) counts as unchanged when only the order of
  its lines differs:
  `xcodebuild -create-xcframework` lists the two slices in no fixed order, which made every
  other device build fail "build again" until S07. The phase
  takes about 3–4 s when nothing changed (bindgen, `cargo rustc` with nothing to do,
  `xcodebuild -create-xcframework`).
- `ENABLE_USER_SCRIPT_SANDBOXING` is off for the app target: the phase writes under `target/`
  and the package, outside any declared output.
- There is no debug build of the Rust core for the simulator: `--sim-only` uses
  `[profile.ios]` (fat LTO). A change in a core crate costs about a minute; a debug variant
  can be added when the edit-run loop needs one.

## CI

`.github/workflows/ios.yml` (plan A9, S32) runs the commands above on GitHub's `macos-latest`
(arm64) for every push to master and pull request that changes `crates/**`, `apps/**`,
`xtask/**`, `Cargo.lock`, `Cargo.toml`, `rust-toolchain.toml` or the workflow, and on demand
(`workflow_dispatch`). `ci.yml` and `cargo xtask ci` are unchanged: nothing there needs macOS
or Xcode. No device build, signing or upload (S33, S35). Its steps:

1. **Select Xcode.** The workflow's `XCODE_VERSION` is matched against the
   `CFBundleShortVersionString` of every `/Applications/Xcode*.app` the runner has (the version
   itself or any `version.x`; the newest wins), and `DEVELOPER_DIR` is set to it for every later
   step and for the run-script phase. The step lists what it found, and fails with "this runner
   image has no Xcode …" when none matches. **It is `26`, not S05's 27.2:** GitHub has no macOS 27
   image yet (`macos-latest` was `macos-26-arm64` with Xcode 26.0–26.6 on 2026-11-21), so CI
   builds and tests with the newest Xcode 26.x, the previous SDK. Local work and release archives
   (S34) stay on 27.2. **When a macOS 27 image ships,** set `runs-on` to it and `XCODE_VERSION`
   to `27`.
2. **Toolchain:** `rust-toolchain.toml`'s, plus `aarch64-apple-ios-sim` (CI builds the simulator
   slice only); `Swatinem/rust-cache`; `brew install xcodegen`.
3. **Starter library, then the Bundled set.** It downloads the `starter-library` artifact of
   master's latest successful `starter.yml` run (`gh run download`, about 160 MB) into
   `dist/starter` and runs `cargo xtask ios bundled-set`. That is cheaper than building the
   Starter library on the runner (`starter.yml`'s two builds take about 8 minutes on Linux).
   But the artifact is master's, so its Bundled set matches the lock only while the branch
   changes neither the Starter build nor the lock. When it does not match, or when no artifact
   can be downloaded (artifacts expire after 90 days), the job says so in a notice and builds
   the Starter library from the branch (`cargo xtask starter --previous <the latest release's
   manifest>`, as `starter.yml` does, with its own cache of the source downloads), then runs
   `bundled-set` again, which must match the lock.
4. `cargo xtask ios build --sim-only`, `xcodegen generate`.
5. **Simulator:** the workflow's `SIMULATOR` (`iPhone 17`) on the runtime of the selected SDK
   (iOS 27.2), else the first iPhone on it with a warning; it is booted before the tests and
   passed to `xcodebuild` by its udid.
6. **`xcodebuild test`** of both targets, with `-derivedDataPath` and `-resultBundlePath`
   under the runner's temporary folder (outside `target/`, which rust-cache prunes). When
   `dist/starter` holds every Resource its manifest puts in the Study set, the tests get
   `TEST_RUNNER_OBS_STUDY_SET_DIR=dist/starter`, so S07's
   `testATaggedWordShowsItsStrongsNumber` runs; otherwise only that test is skipped
   (`-skip-testing:`), and a notice in the log says so.
7. **On failure:** the `ios-test-results` artifact holds `OpenBible.xcresult`, the screenshots
   the tests attached (exported with `xcresulttool export attachments`, named as the tests name
   them) and a screenshot of the simulator at the end.

**Lock check on Linux.** `starter.yml` runs `cargo xtask ios bundled-set --out
target/bundled-set` after it uploads the Starter library, so a Starter change that moves the
Bundled set fails there too, and a pass on Linux shows that `apps/ios/bundled-set.lock` does not
depend on the host. `starter.yml` also runs when the lock alone changes; it skips the check
when `release.yml` calls it, so a desktop release never waits on the iOS lock. A stale lock can be
rewritten from that run's artifact: `cargo xtask ios bundled-set --from <the artifact>
--update-lock`.

**Known cost.** The run-script phase runs cargo under `env -i`, so rust-cache's
`CARGO_INCREMENTAL=0` does not reach it, and `ios build`'s host build of `obs-ffi` (for
`bindgen`) is compiled again inside `xcodebuild` (about 20 s on the owner's Mac; the
`[profile.ios]` library itself is up to date).

**Timings on the owner's Mac (dry run of the job's steps, 2026-11-21):** the artifact download
and a `bundled-set` attempt 1 min 29 s; `ios build --sim-only` in a fresh worktree 3 min 24 s;
`xcodebuild test` (build and both targets, 6 tests) 1 min 39 s. The runner's go in the plan's
S32 Outcome after its first run.

## Minimum iOS, devices, orientation

iOS 17 (A6), iPhone and iPad (`TARGETED_DEVICE_FAMILY = 1,2`), portrait and landscape (iPad
also upside down). `cargo xtask ios build` builds the staticlibs with
`IPHONEOS_DEPLOYMENT_TARGET=17.0` (`ios::IOS_DEPLOYMENT_TARGET`), so rustc and SQLite's C build
for the same minimum as the app; the xtask test
`ios::tests::the_libraries_are_built_for_the_apps_minimum_ios` checks that `project.yml` and
`Package.swift` say the same. The linker gives no deployment-target warning.

## Bundle id and signing

- **Dev bundle id:** `io.github.inhesrom.openbible.dev`, the project setting `OBS_BUNDLE_ID`
  (override with `xcodebuild … OBS_BUNDLE_ID=…`). The test bundles append `.tests` and
  `.uitests`. The final id (default `io.github.inhesrom.openbible`, #328) is applied only by
  S33's follow-up PR. Display name "Open Bible" (working name, #324).
- **Signing:** automatic, no team in the project. Simulator builds sign to run locally and
  need no Apple ID. A device run (S07/S08) needs the owner's Personal Team, chosen on the
  command line (`DEVELOPMENT_TEAM=…`) or in Xcode, not committed.
- **Does a Personal Team registration of an id block a later paid-team registration?**
  **Unverified.** S06 could not reach Apple's documentation (no network to
  developer.apple.com from the build session). What the plan assumed (A6): automatic signing
  registers the App ID under the team that signs, and an explicit App ID is unique across all
  teams, so registering `io.github.inhesrom.openbible` under the free Personal Team could stop
  the paid team from registering it later. Until someone checks Apple's App ID and free
  provisioning pages, treat the `.dev` suffix as **required**, not belt-and-braces: never
  sign the final id with the Personal Team. The check is an owner or S33 action.

## Privacy manifest

`OpenBible/PrivacyInfo.xcprivacy`: no tracking, no tracking domains, no collected data types,
and one required-reason API category, `NSPrivacyAccessedAPICategoryFileTimestamp` with reason
`C617.1` (files inside the app's container): Rust's `std::fs` and SQLite call `stat`, `fstat`,
`fstatat` and `lstat`. It is copied into the app bundle.

What the linked core calls, from `nm -u` on the simulator's `libobs_ffi.a` (S06), for S34's
audit: `stat`, `fstat`, `fstatat`, `lstat` (declared); `statfs` and `fstatfs` (SQLite's
locking-style probe on Apple platforms; Apple lists these under the disk-space category);
`futimes`, `utimes`, `clock_gettime`, `sysctlbyname`. Reason `3B52.1` (files the user chose)
belongs with Import (S26) and the Sync folder (S30).

## Agent shells on the owner's Mac

Besides the quirks S05 recorded (`xcrun simctl` is killed; use
`/Library/Developer/PrivateFrameworks/CoreSimulator.framework/Versions/A/Resources/bin/simctl`):

- **The run-script phase can be killed.** Xcode writes the phase to
  `target/ios/xcode/Build/Intermediates.noindex/OpenBible.build/<config>/OpenBible.build/Script-*.sh`
  and runs it with `/bin/sh -c <file>`. In an agent shell, executing that file is sometimes
  killed (exit 137, the build fails "without specifying any errors"), while a copy of it
  runs. Deleting the file, so that Xcode writes it again, made the next build pass. The
  owner's Terminal and Xcode are not affected.
- **Macro plug-ins once failed to load.** The first `xcodebuild build` failed on `@Observable`
  and `@State` ("library load denied by system policy" for `libSwiftUIMacros.dylib`); the
  same command passed on the next run with nothing changed.

## Measurements (S06, 2026-11-21)

Owner's Mac (Apple M6, macOS 27.0.1, Xcode 27.2, iOS 27.2 simulator, iPhone 17), with other
lanes building:

- `cargo xtask ios build --sim-only`, first time in this worktree: 2 min 1 s;
  `libobs_ffi.a` is now 27.2 MB (S02 measured 7.5 MB with only `engine_version`; S03's spike
  brought in layout, rendering and the store).
- `xcodebuild build` with nothing to do in Rust: about 4 s. `xcodebuild test` (both targets,
  the app already built): about 1 min.
- Boot over the Bundled set into an empty folder (copying its 43.1 MB, opening the BSB as
  reader 0), in the simulator from the unit test: 1.8–1.9 s over two runs; a second boot over
  the same folder: 1.6–1.8 s, so the copy is not most of it (S03 measured `open_reader` of the
  whole BSB at 0.5–1.2 s on the host). Under the plan's 3 s.
