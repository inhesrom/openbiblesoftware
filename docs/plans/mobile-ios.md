# Mobile app for iOS: build plan

Status: draft for handoff (written 2026-11-14 at master `8aea4b0`). One step per executor session.
Owner request: "start building out the iOS app in native swift"; "a multi-step plan … I can handoff
to Claude Opus 5.5 later on".

## Summary

- **What:** the Mobile app (CONTEXT.md § Mobile) for iPhone and iPad: a SwiftUI shell over the Rust
  engine through UniFFI, Resource text drawn by our own pipeline (ADR 0004) in a hosted view.
  v1 scope is #330 + #333 + ADR 0022; Android, the #330 deferred list, F-Droid, accounts and
  servers stay out.
- **Routed through the map (#324):** ADR 0021 makes SwiftUI *provisional* until the UI stack
  prototype (#331) proves the hosted text view, and the screen anatomy (#335) is decided by a
  clickable prototype. So M1 *is* those prototypes, each with go/no-go criteria, an ADR, and the
  map bookkeeping. Flutter / Compose Multiplatform is the named fallback branch (S09-F).
- **Architecture (decided by ADR 0021, shaped here):** three new crates. `obs-reading` (L5) lifts
  the egui-free reading core out of `crates/ui-egui` (`doc.rs`, `view.rs`, `interlinear.rs`,
  `selection.rs`, `marks.rs`, `heights.rs`, `workers.rs`, the colour derivation of `palette.rs`
  ≈ 3,400 lines) so desktop and mobile share
  it; `obs-mobile` (L5, wasm-clean, no FFI) holds every Mobile screen model and action; `obs-ffi`
  under `apps/` is the one UniFFI crate Swift links. Rules stay in Rust; Swift lays out, draws and
  forwards input (ADR 0021 rule 3). Every Xcode step's PR carries a **Shell audit** (A10) and
  follows the UI step standard (see "Milestones and steps").
- **Exploits the no-Xcode window:** M0 (S01–S04) is host-only Rust + Swift-bindings work. Xcode
  is the first HITL step (S05). Steps marked "Xcode: y" wait for it.
- **Order of value:** reader → Study sheet → Annotations → Search/Guides/Plans → Library →
  Sync (ADR 0022, its own ticket set) → App Store readiness. HITL tickets #334 (identifiers),
  #336 (accounts), #337 (trademark) and the privacy policy URL are scheduled (S33) *before* the
  first App Store Connect upload (S35, HITL), because a bundle id is permanent; dev builds use a
  distinct `.dev` bundle id until then.
- **Size:** 8 milestones, 39 steps + 1 fallback step. 6 are HITL, 2 mixed (S09, S12). 25 need
  Xcode.

## Step table

| ID | Step | AFK/HITL | Xcode | Depends on | Key deliverable | Status |
|---|---|---|---|---|---|---|
| S01 | Lift the reading core into `obs-reading` (L5) | AFK | n | — | `crates/reading`, desktop unchanged (gallery byte-identical) | done (PR #359) |
| S02 | Scaffold `obs-mobile` (L5) + `apps/obs-ffi` (UniFFI) + gates + `cargo xtask ios` | AFK | n | — (∥ S01) | crates, xtask commands, `docs/dev/mobile-core.md`, D21 (UniFFI licence) | done (PR #357) |
| S03 | "Hello engine" through the FFI on the macOS host | AFK | n | S01, S02 | host test: open BSB → lay out Gen 1 → tile PNG; `swiftc` smoke | done (PR #360) |
| S04 | Bundled set + Study set in the Starter build | AFK | n | — (∥ S01–S03) | `cargo xtask ios bundled-set`, `apps/ios/bundled-set.lock`, `mobile_set` manifest field, workspace `exclude` | done (PR #358) |
| S05 | Install Xcode, simulators, iOS Rust targets, XcodeGen; Personal Team | HITL | y | — | owner checklist done; `xcodebuild -version` works | done (2026-11-20; Personal Team + device pending) |
| S06 | iOS project scaffold under `apps/ios/` | AFK | y | S03, S05 | `project.yml`, local Swift package, XCFramework, app shows engine version, test targets | done (branch mobile/s06-ios-project, PR pending) |
| S07 | #331 UI stack prototype: hosted text view on device | AFK | y | S04, S06 | prototype build + measurements + `docs/dev/mobile-text-view.md` | todo |
| S08 | #331 owner judgement on a real iPhone | HITL | y | S07 | go/no-go, Android-half decision | todo |
| S09 | ADR 0023 (UI stack, hosting option, performance targets, accessibility bar) + #331 close + map line | AFK + HITL | n | S08 | `docs/adr/0023-*.md`, #324 Decisions line, #331 resolution | todo |
| S09-F | Fallback branch: Flutter / Compose Multiplatform spike (only if S08 = no-go) | AFK | y | S08 | re-plan of M1–M7; ADR 0023 records the fallback | todo |
| S10 | #335 screen anatomy clickable prototype (phone + iPad) | AFK | y | S09 (∥ S13) | prototype app, flows, screenshots | todo |
| S11 | #335 owner review | HITL | y | S10 | decisions on navigation, sheet, split, reference entry | todo |
| S12 | ADR 0024 (anatomy) + `docs/dev/mobile-anatomy.md` + #335 close + min iOS + mobile extras + i18n/string-location decision | AFK + HITL | n | S11 | ADR, contract, map line | todo |
| S13 | `obs-mobile` core: boot, paths, Bundled set install, readers, navigation, History, FFI surface | AFK | n | S03, S04, S09 | `MobileApp` object, snapshots, events, tests | todo |
| S14a | iOS reader: real text view, grid + typed references + keys, Back/History, Last session | AFK | y | S12, S13 | reader screens, previews | todo |
| S14b | iOS reader: footnotes, Settings, Dynamic Type, Reduce Motion / Increase Contrast, VoiceOver | AFK | y | S14a | Settings screen, a11y pass, previews | todo |
| S15 | Parallel resources + Split view with Follow | AFK | y | S14b | Rust Follow via Link sets; stacked/side-by-side readers | todo |
| S16 | Study sheet | AFK | y | S14b | `StudySheet` model; bottom sheet / side pane | todo |
| S17 | Highlights (active palette, Recently used) + Undo toast | AFK | y | S16 | selection → Highlight; Undo | todo |
| S18 | Notes (CommonMark editor, keyboard bar, live links) + Notebooks | AFK | y | S16 | shared formatting rules in `obs-model`, editor, Notebook picker | todo |
| S19 | Notes Tool (lighter) + Needs attention + Notes export + Copy/Share | AFK | y | S17, S18 | screens, share sheet | todo |
| S20 | Search (Bible + Notes, scopes, full syntax) | AFK | y | S14b | Search screen, index status | todo |
| S21 | Passage Guide + Word Study (lighter), synced arrangement followed | AFK | y | S16 | Guide screens | todo |
| S22 | Reading plans + optional local reminder | AFK | y | S14b | plans screens, `UNUserNotificationCenter` | todo |
| S23 | Library Rust side: install-from-file seam, manifest merge, Study set, Open catalogue, storage accounting | AFK | n | S13 (∥ S14–S22) | engine/mobile APIs, tests, `docs/dev/mobile-library.md` | todo |
| S24 | iOS downloads: background `URLSession`, Wi-Fi-only, first-launch Study set offer, Library screens | AFK | y | S23 | Library → Starter library / Get Resources | todo |
| S25 | On-device index builds + `BGProcessingTask` + Storage + backup exclusion | AFK | y | S24 | Storage screen, index status | todo |
| S26 | Import: document picker, Open In, SWORD zip, Import report | AFK | y | S23 | UTType declarations, `ImportSource::SwordZip` (engine-only on desktop) | todo |
| S27a | Sync 1a (Rust): User database migration 13 — clocks, tombstones, device id, `applied_up_to`, Layout ids | AFK | n | — (∥ M2–M5) | `obs-store` migration, tests | done (PR #356) |
| S27b | Sync 1b (Rust): `Row` variants for every synced table; every synced write via `apply_changes` | AFK | n | S27a | engine/store changes, tests | todo |
| S28a | Sync 2a (Rust): `SyncFolder` trait, HLC, op file writer/reader, `sync.json`, device files | AFK | n | S27b | `obs-engine::sync` skeleton, `docs/dev/sync.md` draft | todo |
| S28b | Sync 2b (Rust): merge rules + property tests | AFK | n | S28a | merge module, 10k-case property tests | todo |
| S28c | Sync 2c (Rust): snapshots, trimming, format versioning, device-id rules, restore choice, scheduling | AFK | n | S28b | `sync_now`, `next_sync_due`, `restore_choice`, `docs/dev/sync.md` final | todo |
| S29 | Sync 3 (desktop): Settings → Sync, status, Needs attention Sync conflicts, restore prompt | AFK | n | S28c | egui UI (ui-review) | todo |
| S30 | Sync 4 (iOS): folder picker, bookmarks, `NSFileCoordinator`, placeholders, scheduling, Settings → Sync | AFK | y | S28c, S19 | Swift `SyncFolder`, screens | todo |
| S31 | Sync 5: iPhone ↔ Mac iCloud Drive end-to-end + provider picker check | HITL | y | S29, S30 | test report, folder list for onboarding | todo |
| S32 | CI: iOS job on a macOS runner | AFK | y (runner) | S06 | `.github/workflows/ios.yml`, caching | todo |
| S33 | Before anything permanent: #334, #336, #337, privacy policy URL, release cadence | HITL | n | S12 | decisions recorded, tickets closed, map lines, final bundle id applied | todo |
| S34 | App Store readiness: `PrivacyInfo.xcprivacy`, icon, Credits, accessibility audit, size, archive + export + validate | AFK | y | S19–S26, S30, S32, S33 | validated archive, review notes, `docs/dev/mobile-release.md` | todo |
| S35 | App Store Connect record, first upload, TestFlight, submission | HITL | y | S34 | TestFlight build; review submitted | todo |

Parallel lanes (separate worktrees): S01 ∥ S02 ∥ S04; S10 ∥ S13; S23 ∥ S14–S22; S27a–S28c ∥
anything (they do not need S01); S32 any time after S06. HITL steps need no worktree.

## How to run a step

**Start of session, read in this order:** this file (summary, the step, its "read first" list);
`CONTEXT.md` § Mobile and § Sync (lines ~697–740); ADR 0021 and 0022; the tickets the step
names (`gh issue view N -R inhesrom/openbiblesoftware --comments`); the code paths the step cites.
Verify every claim this plan makes about code before relying on it; the plan was written at
`8aea4b0` and the tree moves.

**Branch and worktree:** one Paseo worktree per step, branched from `master`, branch name
`mobile/<step-id>-<slug>`. One PR per step. Commit style as `git log`: `feat(mobile): …`,
`fix(mobile): …`, `docs(mobile): …`, `refactor(reading): …`, `chore(ci): …`; the PR title is the
squash subject and GitHub appends `(#N)`.

**Gates to keep green:** `cargo xtask ci` on the host (fmt, layers, assets, licenses, clippy,
a11y, test, wasm — `xtask/src/main.rs` `GATES`). Rust-only steps: that is all. Xcode steps also run
`cargo xtask ios build` and `xcodebuild test` (S06 defines the exact commands) and attach
simulator screenshots. No step may make `cargo xtask ci` need macOS or Xcode. On the owner's Mac,
`xcrun simctl` and `brew` are killed in agent shells; S05's Outcome lists the workarounds.

**UI review (ADR 0020):** any PR that changes what a user sees (iOS screens, desktop Settings →
Sync, Highlight colours) gets the `ui-review` label, before/after PNGs under `docs/previews/ios/`
(simulator: `xcrun simctl io booted screenshot <file>`) and waits for the owner. Never merge or
enable auto-merge on one. Agent review may add `agent-approved`. ADR 0020 rule 4: when in doubt,
it is a UI change (S01's refactor of shared painting code and S18's editor lift are labelled too).

**Shell audit (A10):** every PR that touches `apps/ios` has a **Shell audit** section in its body:
each new Swift decision point, and why it is presentation, not a rule. A missing section is a
review blocker.

**Status bookkeeping:** in the same PR, set the step's row in the table to `in progress (PR #N)`,
then `done (PR #N)`, and add an **Outcome** paragraph under the step (what shipped, what was
deferred, measurements, decisions the owner made). If a step uncovers that a later step's
assumption is wrong, edit that later step too and say so in the PR.

**Stop-and-ask:** each step lists triggers. Hitting one means: stop, write the question and the
options with a recommendation in the PR (or to the owner), do not guess. Nothing in this plan
authorises a GitHub issue edit, close, label or comment on #324–#337; those are owner actions
listed as HITL.

**Kickoff prompt (copy-paste to a fresh Opus 5.5 session):**

```text
Execute step <ID> of docs/plans/mobile-ios.md in this repository (a Paseo worktree branched
from master). Read the plan's "How to run a step" and the step's full entry first, then
every file and ticket in its "read first" list. Verify the plan's claims against the code
before relying on them. Do only what the step's "do" says; respect its "don't" fences; stop
and ask at any listed stop-and-ask trigger. Keep `cargo xtask ci` green (and the iOS gates
the step names). Deliver the step's deliverables, update the step's status row and add an
Outcome paragraph in docs/plans/mobile-ios.md in the same PR. Commit style: feat(mobile): …
(see git log). UI changes get the ui-review label and screenshots; never merge them yourself.
If the step touches apps/ios, the PR body must have a "Shell audit" section (plan A10) listing
every new Swift decision point and why it is presentation, not a rule.
Do not edit, comment on or close GitHub issues; list any owner HITL actions in the PR body.
```

## Architecture

Where an ADR decides, this plan follows it. Where it does not, the choice below is a
**recommendation**; "owner may override" marks the real choices, and S09/S12/S33 are the points
to override at.

### A1. Crate split

| Crate | Where | Layer | Allows unsafe | wasm gate | Holds |
|---|---|---|---|---|---|
| `obs-reading` | `crates/reading` | L5 | no | yes | the reading core lifted from `crates/ui-egui`: `ReadingDoc`, `ReadingView`, `InterlinearSource`, `HeightIndex`, `Selection`/`DocPos`, Marks, `LayoutClient` workers, `TonePalette` (over `Rgb`: tone constants, `derive`, `check`, `contrast`, `paint`), reading-page tokens, a CPU tile compositor |
| `obs-mobile` | `crates/mobile` | L5 | no | yes | the Mobile app: paths, boot, readers (≤2), Split view Follow, Study sheet model, Highlights/Notes/Notebooks actions, Search/Guides/Plans screen models, Library and download state machine, sync scheduling. Plain Rust types; unit-tested on all three CI OSes |
| `obs-ffi` | `apps/obs-ffi` | app (not in `LAYERS`) | only if needed (see below) | no (apps are excluded, `xtask/src/layers.rs` `wasm_crates`) | UniFFI object `MobileApp` + Records mirroring `obs-mobile` snapshots; `staticlib` + `cdylib`(host) |

Rationale. `xtask/src/layers.rs` requires every crate outside `apps/` and `xtask` to be in `LAYERS`,
and `xtask wasm` checks every `LAYERS` crate for `wasm32-unknown-unknown` (`wasm_crates`). A UniFFI
crate under `crates/` would have to compile for wasm and obey the "no networking crate" rule for
libraries; under `apps/` it is `Kind::App`, exempt from both, and `apps/` is already where the
one network transport lives (`apps/openbible/src/fetch.rs`). Clippy still covers it
(`cargo clippy --workspace --all-targets`). Why two L5 crates instead of one: `obs-reading` is
desktop code that `obs-ui-egui` keeps using (`crates/ui-egui/src/lib.rs` re-exports it), while
`obs-mobile` is new and must never be a dependency of the desktop. Owner may override the names
(`obs-app`, `obs-core` were floated).

`LAYERS` gains `("obs-reading", 5), ("obs-mobile", 5)`; `README.md` layer table gains L5;
`obs-ui-egui` stays L6. `UI_CRATES`/`NET_CRATES` unchanged; add `"uniffi"` to a new
`FFI_CRATES` list that only apps may depend on (same shape as `NET_CRATES`), so `uniffi` can never
leak below `apps/`.

**Lints.** `[workspace.lints.rust] unsafe_code = "forbid"` (`Cargo.toml`) is inherited by every crate
through `[lints] workspace = true`. Cargo cannot partially override an inherited table, so if the
FFI crate needs an exception it declares its own table: `[lints.rust] unsafe_code = "deny"` plus
the three clippy denies copied verbatim, with a comment citing ADR 0021. **Try `workspace = true`
first** (S02): UniFFI proc-macro scaffolding is generated code and rustc may not report
`unsafe_code` inside external macro expansions; if the crate compiles under `forbid`, keep it and
record "no exception needed" in `docs/dev/mobile-core.md`. If not, the exception is scoped to
`apps/obs-ffi` only. Either way, hand-written `unsafe` in `obs-ffi` is a stop-and-ask.

**Types across the FFI: mirror types.** `obs-ffi` defines `#[derive(uniffi::Record)]` /
`#[derive(uniffi::Enum)]` DTOs and `From<obs_mobile::X>` conversions. UniFFI's remote-type
mechanism would let `obs-mobile` types cross directly but needs `uniffi` as a dependency of the
L5 crate (breaking the wasm gate) or attribute-heavy remote declarations in `obs-ffi` anyway.
Mirror types keep L5 free of uniffi and make the FFI surface an explicit, reviewable file. Cost:
duplication; `obs-mobile` snapshot types are already flat DTOs, so it is small. Kotlin stays
possible because the same `obs-ffi` library generates Kotlin bindings (`--language kotlin`); no
iOS-specific code goes in Rust.

**Gate changes (S02):** `layers` (two L5 entries, `FFI_CRATES`), `wasm` (automatic: new L5
crates are checked — so native-only engine calls in `obs-mobile` are `#[cfg(not(target_arch = "wasm32"))]`,
see S13), `clippy` (automatic), `test` (automatic), `licenses` (today reads
`apps/openbible/Cargo.toml` only, `xtask/src/licenses.rs` `APP_MANIFEST`; S02 adds a second
listing over the `obs-ffi` graph so a licence problem surfaces at S02, not S34 — see D21),
`assets` (any new `assets/` dir needs `ATTRIBUTION.toml`). **Licence check (D21):** UniFFI is
MPL-2.0 (weak copyleft; executor confirms in the `uniffi` crate's `Cargo.toml`), and `about.toml`
`accepted` holds only permissive licences by design. The owner accepted MPL-2.0 for the iOS
binary on 2026-11-20 (Credits text + source link satisfy it); S02 adds it to `accepted` with a
comment. The design-token lint (ADR 0015 rule 3) is extended in S01 to scan
`crates/reading/src` and later `crates/mobile/src`.

### A2. UniFFI usage

- **Proc-macro mode** (`#[uniffi::export]`, `#[derive(uniffi::Record)]`, `uniffi::setup_scaffolding!()`),
  no UDL. Executor verifies against the UniFFI manual for the version in `Cargo.lock`
  (https://mozilla.github.io/uniffi-rs/latest/), including edition-2024 compatibility of the
  macros and the `unsafe(no_mangle)` attribute form.
- **Bindgen in library mode** from xtask: build `obs-ffi` as `cdylib` for the host, then
  `uniffi-bindgen generate --library <lib> --language swift --out-dir target/ios/bindings`. The
  bindgen binary is a tiny separate workspace crate `apps/uniffi-bindgen` (`uniffi = { features = ["cli"] }`,
  `uniffi::uniffi_bindgen_main()`), the shape the UniFFI manual documents, so the `cli` feature and
  its deps never enter `obs-ffi`'s graph (which cargo-about lists for iOS Credits). No global
  install is needed (verify in the manual's "Foreign language bindings"
  → Swift pages; also evaluate `uniffi-bindgen-swift` for XCFramework-ready modulemaps).
- **Targets:** `aarch64-apple-ios`, `aarch64-apple-ios-sim` (`rustup target add`, works without
  Xcode; *building* them does not: `rusqlite`'s bundled SQLite compiles C with the iOS SDK via
  `cc`/`xcrun`, so the staticlib build is "Xcode: y"). `aarch64-apple-darwin` cdylib for host
  tests and bindgen.
- **XCFramework:** `xcodebuild -create-xcframework -library libobs_ffi.a -headers <dir>` per slice
  → `target/ios/ObsFFI.xcframework`, wrapped by a local Swift package `apps/ios/Packages/OpenBibleCore`
  (binary target + the generated `.swift`). Generated bindings, the XCFramework and the
  `.xcodeproj` are gitignored and rebuilt by `cargo xtask ios build`.
- **Profile:** a `[profile.ios]` inheriting `release` with `lto = "fat"`, `codegen-units = 1`,
  `opt-level = "s"`, `strip = "debuginfo"`. **Keep `panic = "unwind"`:** the job queue isolates
  panicking jobs with `catch_unwind` (`crates/engine/src/jobs.rs:276`); `panic = "abort"` would turn a
  job panic into an app crash. Measure size in S07 and S34.

### A3. Threading, async and observation

- `Session::run(&mut self, …)` (`crates/engine/src/session.rs:436`) is the only command entry
  point and needs `&mut`. `obs-ffi` holds `Arc<Mutex<obs_mobile::MobileApp>>` behind one UniFFI
  object; every FFI method is **synchronous and short** (lock, mutate, return a snapshot). No UniFFI
  async in v1 (it needs a foreign executor/runtime; revisit if a call proves slow).
- **`Session` must be `Send`.** `UserData` has no `Send` bound (`crates/engine/src/persist.rs:266`),
  so `Box<dyn UserData>` makes `Session` `!Send`. S02 adds `: Send` to the trait (the only impl is
  `obs_store::UserStore`, which wraps a `rusqlite::Connection`, which is `Send`) and a compile-time
  `fn assert_send<T: Send>()` test. Fallback if something else is `!Send`: an engine actor thread
  with a command channel.
- **Background work** stays on the engine's thread job queue (`JobMode::Threads(n)`,
  `crates/engine/src/session.rs:17`; the queue is `crates/engine/src/jobs.rs`) and `obs-reading`'s layout workers. **The `JobQueue` event receiver
  is single-consumer** (`events_rx`, `jobs.rs:241`) and `Session` must drain it itself:
  `Session::poll_imports` (`import.rs:256–271`) routes finished jobs to `poll_starter`,
  `poll_search_indexes`, `poll_guides`, … (Starter installs land there via `JobEventKind::Finished`
  downcasts, `starter.rs:1020–1030`). So `obs-ffi` runs one Rust "pump" thread that **never reads
  `JobQueue` events**: it wakes on a timer (e.g. 50 ms while jobs are running, idle otherwise) or a
  signal from `obs-mobile`, briefly locks the `Mutex<MobileApp>`, calls `session.poll_imports()`
  plus the layout-worker drain, translates the resulting `Effect`s into `Changed` values, unlocks,
  then calls a Swift-implemented `#[uniffi::export(with_foreign)] trait Observer { fn changed(&self, what: Changed) }`.
  Swift hops to `@MainActor`, pulls the snapshots it needs, and updates `@Observable` view models.
  Snapshots are value types; Swift never holds Rust references across awaits.
- Desktop pumps `Effect`s each frame (`crates/ui-egui/src/lib.rs` → `shell.apply_effects()`);
  mobile has no frame loop, so `obs-mobile` owns an `Effect` → `Changed` translation (S13).

### A4. Hosting Resource text (ADR 0004)

Decision for the prototype (S07), recorded in ADR 0023 (S09):

- **B first: CPU tiles.** `obs-render` gains `composite(quads, atlas, colours: &RoleColours, tile: Rect, scale) -> Vec<u8>`
  (RGBA8, premultiplied): pure, wasm-clean, L3 (it already owns the atlas and quads,
  `crates/render/src/atlas.rs` `GlyphAtlas::quads_from`). `obs-reading` composes a tile: page colour,
  Highlight backgrounds and underlines (Marks via `TonePalette::paint`), then glyphs. Swift makes a
  `CGImage` from the bytes and shows tiles in a `UIScrollView` (one `UIView` per tile, or
  `CATiledLayer`). No GPU lifecycle, no raw handles, no unsafe.
- **A fallback, written in Swift, not Rust:** if B misses the frame budget, keep Rust returning
  `GlyphQuad`s plus the atlas coverage bytes and dirty rect (`take_dirty`, `stats().generation`), and
  draw in Swift with Metal (`MTKView`, R8 atlas texture, one quad vertex buffer per block). This
  avoids wgpu raw-handle `unsafe` in Rust entirely. Prototype A only if B fails.
- **C rejected** (native `AttributedString`): gives up our shaping, justification, bidi and
  identical rendering with desktop (ADR 0004).
- **Selection with handles:** a custom `UIView` adopting `UITextInput` + `UITextInteraction`,
  whose positions/ranges map to `DocPos` and whose `selectionRects(for:)`/`caretRect(for:)` come from
  Rust (`obs_reading::selection::{range_rects, selection_rects, word_at}`) and hit-testing
  (`LaidBlock::hit`, `crates/layout/src/layout.rs:1449`). Granularity (word/Verse) is Rust's.
- **VoiceOver:** `UIAccessibilityElement` per Verse (Bibles) / per block (others) with label text
  and frames from Rust (`accessibility_items(frame) -> Vec<{label, rect, unit}>` in `obs-reading`);
  rotor "Verses"; Dynamic Type mapping `UIContentSizeCategory → Text size` as a Rust table in
  `obs-mobile` so Android's font scale uses the same bounds.
- **RTL Hebrew:** no shell work; bidi is in `obs-layout`; `has_rtl`/`starts_rtl` drive paragraph
  alignment already.
- **Tap → Study sheet:** `hit` → `UnitPos` → `obs-mobile::tap(reader, pos)` decides word vs Verse
  (Rust), returns a `StudySheet` snapshot (S16).

### A5. Colours and tokens without rules in Swift

`crates/ui-egui/src/palette.rs` is the only place colour roles become colours and derives Highlight
colours per Page tone (ADR 0012). Every `Palette` field is `egui::Color32` (`palette.rs:66–90`) and
there are 130+ field uses across `pane.rs`, `shell/*`, `compare_view.rs`, `search_view.rs` and
tests, so the type itself cannot move without touching them. S01 therefore moves only the
**derivation logic** into `obs-reading` as `TonePalette` over `obs_engine::Rgb`
(`crates/store/src/highlight_style.rs:25`) / `[u8; 4]`: the six tone constants, `derive`, `check`,
`contrast`, `text_on`, `paint` (→ `StylePaint` over `Rgb`), `color`, `overlays`, `inks`.
`obs_ui_egui::Palette` keeps its `Color32` fields and every method name, built by one
`impl From<TonePalette> for Palette`, each method delegating through `to_rgb`/`to_color`
(`palette.rs:47–52`). Call sites compile unchanged. The reading-page
tokens `view.rs` uses (`space::XXL`, `size::TEXT_MIN`, `motion::*`, `mark::FOOTNOTE_BOX`) move with it
as a `reading_tokens` module that `obs_ui_egui::tokens` re-exports. **ADR 0015 rules 1 and 3 keep
holding:** the design-token lint (`design_tokens_tests.rs`; `TOKEN_FILES` lists `palette.rs`,
`ALLOWED` has `view.rs` entries) is extended to scan `crates/reading/src` too, with allowlist
entries keyed by crate-relative path; S01 records the amendment in `docs/dev/design-tokens.md`
(and a one-line ADR 0015 "Amended by" note), and S13 adds `crates/mobile/src` to the scan. The
iOS shell gets page colours, inks, overlays,
Highlight swatches and accent from Rust snapshots (`PaletteSnapshot`), and uses system SwiftUI
materials, Dynamic Type and HIG spacing for chrome — layout tokens are look, not study rules.
Theme (Light/Dark/System/High contrast) and Page tone choices remain `obs_engine::appearance`.

### A6. iOS project shape

- **Location:** `apps/ios/` — `project.yml` (XcodeGen, committed), `OpenBible/` (Swift sources,
  assets, `Info.plist` keys in `project.yml`, `PrivacyInfo.xcprivacy`), `OpenBibleTests/` (XCTest or
  Swift Testing, simulator), `OpenBibleUITests/` (XCUITest smoke), `Packages/OpenBibleCore/`
  (local SwiftPM package: binary target → XCFramework + generated Swift), `bundled-set.lock`.
  **Workspace glob:** `Cargo.toml` has `members = ["crates/*", "apps/*", "xtask"]`; a directory
  matching `apps/*` without a `Cargo.toml` makes every cargo command fail with
  `failed to load manifest for workspace member` (confirmed with a throwaway workspace). S04, the
  first step to create `apps/ios/`, adds `exclude = ["apps/ios"]` to `[workspace]` and verifies
  with `cargo metadata`.
  `.gitignore` adds `/apps/ios/*.xcodeproj`, `/apps/ios/Packages/OpenBibleCore/{Sources/Generated,ObsFFI.xcframework}`,
  `/apps/ios/Bundled/`.
- **Why XcodeGen** (recommended; owner may override to Tuist or a committed `.xcodeproj` with
  synchronized folders): agents cannot click the GUI, YAML diffs review well, and the project is a
  build artefact. Executor verifies syntax against XcodeGen's `Docs/ProjectSpec.md` for the
  installed version.
- **Minimum iOS: 17** (recommended): `@Observable` needs iOS 17; `NavigationSplitView` iOS 16;
  `UITextInteraction` iOS 13. Floor hardware is then A12 (iPhone XS/XR, 2018). Owner may raise to 18
  at S12.
- **Bundle id:** dev and Personal Team builds use a **distinct dev id**,
  `io.github.inhesrom.openbible.dev` (a `project.yml` setting, overridable), because automatic
  signing registers the App ID under whichever team signs it and App IDs are global — registering
  the real id under the free Personal Team could block the paid/organisation team later (executor
  checks Apple's App ID and free-provisioning docs in S06 and records the answer). The final id
  (default `io.github.inhesrom.openbible`, #328) is applied only by S33's follow-up PR after #334 and
  #336 decide. Display name "Open Bible" (working, per #324) until #334/#337. Nothing is uploaded to
  App Store Connect before S33 and S35.
- **Signing:** free Personal Team for device runs (automatic signing; 7-day profiles; no TestFlight,
  no App Groups; iCloud Drive through the document picker needs no iCloud entitlement). The
  Developer Program (#336) is needed for TestFlight and the store (S33/S35).
- **Tests:** unit tests for view models against the FFI; XCUITest smoke (launch, open Genesis 1,
  tap a word, Study sheet appears); `performAccessibilityAudit()` in XCUITest (Xcode 15+; verify).
  Screenshots via `xcrun simctl io booted screenshot` into `docs/previews/ios/<step>-<surface>.png`
  for ADR 0020 review. No pixel goldens for iOS in v1 (simulator rendering drifts between Xcode
  versions); revisit after launch.

### A7. Library on device (#333)

- **Bundled set** = Starter Resources `bsb`, `openbible-xrefs`, `strongs-hebrew`, `strongs-greek`
  (`starter/sources.toml`), with their prebuilt Search index caches (`dist/starter/search/`) and
  alias caches, laid out exactly as a Starter bundle (`starter-manifest.json` + `.obr` + `search/` +
  `aliases/` + `CHANGES-*.md`, `docs/dev/starter-library.md`). `cargo xtask ios bundled-set` (S04)
  copies that subset from `dist/starter/` into `apps/ios/Bundled/starter/` and checks each sha256
  against the committed `apps/ios/bundled-set.lock`; an Xcode run-script phase fails if the lock
  and the files disagree. The app passes `Bundle.main.resourceURL/starter` to Rust.
- **First launch: copy (ADR 0007), not open in place.** `Session::sync_starter_library(bundle)`
  (`crates/engine/src/starter.rs:758`) already stages, verifies and renames `.obr` files into the
  library folder and handles revisions. 43 MB (the Bundled set with its Search indexes, measured in
  S04) copied once is cheap; opening in place would need a
  second code path for revisions and Remove. Owner may override. **Manifest mismatch to fix (S23):**
  the engine treats the bundled and the released Starter manifest as either/or —
  `StarterLibrary::manifest()` returns the bundle's first (`starter.rs:248–251`) and `add_starter`
  installs from the bundle whenever one is set, rejecting ids outside it ("is not part of the
  Starter library", `starter.rs:880–900`). With a 4-item bundle manifest, Library → Starter library
  would show 4 rows and Study set downloads would fail. S23 makes the bundle serve install and
  revision only, and the rows/wanted set come from the released manifest (merged with the bundle's
  entries while offline).
- **Paths from Swift.** `obs_engine::default_library_dir`/`default_user_db` (`crates/engine/src/library.rs:402–420`)
  use `$HOME` and `cfg!(target_os = "macos")`; on iOS they would fall to the XDG branch. `obs-mobile`
  takes `Paths { library, user_db, search_cache, alias_cache, logs, downloads }` (all under
  Application Support, `FileManager.urls(for: .applicationSupportDirectory)`) and calls the existing
  setters (`set_search_cache_dir`, `set_alias_cache_dir`, `set_log_dir`, `Library::scan`). Swift sets
  `isExcludedFromBackup = true` on `library/`, `search/` and `aliases/`; `user.db` stays backed up.
- **Study set** = the #333 list (`kjv-strongs`, `tagnt`, `tahot`, `tbesg`, `tbesh`, `bdb`,
  `abbott-smith`, `easton`, `smith`, `hitchcock`, `jfb`, `tipnr`, `theographic`, `openbible-geo`,
  `bsb-parallels`). S04 adds `mobile_set = "bundled" | "study" | "more"` to `starter/sources.toml` and
  the manifest; `StarterItem` ignores unknown fields (`starter.rs:49–52`), so desktop is unaffected.
- **Downloads meet the `Fetch` seam without a Rust networking crate.** Two paths: (1) small
  JSON (Starter manifest, Open catalogue) through a Swift-implemented UniFFI callback
  `Transport { get(url) -> bytes }` wrapped as `obs_formats::sword_repo::Fetch`
  (`crates/formats/src/sword/repo.rs:51`) and installed with `Session::set_fetch`; it is only called
  from job threads. (2) `.obr` and catalogue files by background `URLSession` download tasks
  (Swift) to `downloads/`, then `MobileApp::install_downloaded(resource_id, path)` → engine verifies
  sha256 and stages through `starter::install` (`starter.rs:405`, today `pub(crate)` with a `read`
  closure — S23 exposes an entry taking a file path) or `OpenSource::check_download` + `import`
  (`crates/engine/src/open_catalogue.rs:225–255`). Rust owns manifests, pins, states, "Wi-Fi only" and
  "ask first over cellular" rules; Swift owns the transfer.
- **Indexes** build on device through the existing `SearchIndexes`/`CacheIndexer`
  (`crates/engine/src/search.rs`) on the job queue; `BGProcessingTask` continues them when the app is
  closed (S25). Prebuilt indexes ship only for the Bundled set.
- **Import** (S26): `UIDocumentPickerViewController` and `CFBundleDocumentTypes`/`UTExportedTypeDeclarations`
  for `.usfm`, `.sfm`, `.zip`, `.obr`; files are copied into Application Support, then
  `Session::start_import(ImportSource)` (`crates/engine/src/import.rs:575`). Gap: `ImportSource`
  has no local zip variant (`import.rs:465–483`); S26 adds `SwordZip`/`UsfmZip` (unpack with the
  `zip` crate already in `obs-engine`, then `Sword { path }` / `usfm_zip_books`). The engine change
  is shared; the desktop picker is not changed in this plan (a separate ticket if wanted).

### A8. Sync (ADR 0022)

- **Its own ticket set.** This is a cross-cutting change to the desktop User database and both apps;
  it deserves a child wayfinder map (or a ticket set under #324) with the steps below as its
  spine. S27a–S29 are Rust/desktop, need nothing from M0–M5 (not even S01) and can start at once;
  nothing in M2–M5 waits on them.
- **Folder I/O through a Rust trait implemented in Swift: yes (recommended).**
  `obs_engine::sync::SyncFolder { list(dir), read(path), write_new(path, bytes) /*temp+rename*/, remove(path), ensure_available(path) }`.
  Desktop implements it with `std::fs`; iOS implements it in Swift with
  `startAccessingSecurityScopedResource`, `NSFileCoordinator`, and
  `startDownloadingUbiquitousItem` for iCloud placeholders (`docs/research/mobile-folder-sync.md`
  "iOS" bullets). Every rule — file names, hash lines, clocks, merge, trimming — stays in Rust and is
  tested with an in-memory `SyncFolder`.
- **Scheduling** is Rust (`next_sync_due()` per ADR 0022 rule 8); the shells only call `sync_now()`
  at launch, on resume, from a timer and on demand.
- **Reader first:** M6 sits after M5 in value order but is parallelisable; S30 depends on S19
  (Needs attention) because conflicts surface there.

### A9. CI

One new job `ios` on `macos-latest` (arm64) in a new `.github/workflows/ios.yml` (S32), run on
every PR that touches `crates/**`, `apps/**`, `Cargo.lock` or the workflow (FFI-visible breakage
comes from engine/store/render/layout too): rustup +
targets, `Swatinem/rust-cache`, `cargo xtask ios build --sim-only`, `xcodegen generate`,
`xcodebuild test -scheme OpenBible -destination 'platform=iOS Simulator,…'`, upload screenshots.
`DEVELOPER_DIR` is pinned to the Xcode version S05 recorded (not a number fixed here).
`cargo xtask ci` and `.github/workflows/ci.yml` stay unchanged for ubuntu/windows; the Rust
tests of `obs-reading`, `obs-mobile` and `obs-ffi` (host) run inside `xtask test` on all three OSes.
Device builds and archives are not in CI until signing exists (S33) and the first manual upload
has happened (S35); then a separate `ios-release.yml` with App Store Connect API key secrets.

### A10. Keeping "a rule in Swift is a bug" enforceable

Every PR that touches `apps/ios` includes a **Shell audit** section (see "How to run a step" and
the kickoff prompt; every Xcode step's Done when requires it) listing each new Swift decision
point and why it is presentation. Reviewers grep `apps/ios` for: reference parsing, versification, book names,
colour math, Highlight ordering, search syntax, sorting of Resources, file naming, sha256,
merge rules, "default" values, and any `switch` over domain enums that chooses *behaviour* rather
than a view. Each such finding either moves to `obs-mobile` or is justified as pure presentation.
`docs/dev/mobile-core.md` keeps the running list of accepted presentation-only decisions.

## Milestones and steps

Field key: **Type** AFK (agent alone) / HITL (owner does the listed actions). **Xcode** y/n.
**Size** S (< 1 session of light work), M (one session), L (one full session; split if it overruns).

**UI step standard** (every "Xcode: y" step from S14a on; a step saying "UI standard" means all of
this in addition to its own lines):

- *Verify:* `cargo xtask ci`; `cargo xtask ios build --sim-only`; `cd apps/ios && xcodegen generate`;
  `xcodebuild test -scheme OpenBible -destination 'platform=iOS Simulator,name=<from simctl list>'`
  (iPhone and iPad destinations); `xcrun simctl io booted screenshot docs/previews/ios/sNN-<surface>.png`
  per surface in Light/Dark.
- *Done when:* `cargo xtask ci` green; `xcodebuild test` green on both destinations; previews under
  `docs/previews/ios/sNN-*.png`; `ui-review` label; the PR body has a **Shell audit** (A10);
  XCUITest `performAccessibilityAudit()` passes on every new screen; VoiceOver is checked by
  XCUITest assertions on accessibility labels and element order (not by the Accessibility
  Inspector GUI).
- *Stop-and-ask:* a rule wants to live in Swift (anything in A10's grep list); an Apple API the step
  names does not behave as documented; a `docs/dev/mobile-*.md` contract has to change.

### M0 — Foundations (host-only; no Xcode)

#### S01 — Lift the reading core into `obs-reading` (L5)

- **Goal:** desktop and mobile share one Rust reading core; `ui-egui` keeps only egui painting.
- **Type:** AFK. **Xcode:** n. **Size:** L. **Parallel:** with S02, S04.
- **Depends on:** —.
- **Read first:** ADR 0004, 0005, `docs/adr/0012-highlight-styles-are-user-rows-and-tone-colours-are-derived.md`,
  `docs/adr/0015-the-interface-look-is-design-tokens-and-shared-components.md`, 0021;
  `crates/ui-egui/src/lib.rs` (module list and
  re-exports); `doc.rs`, `view.rs`, `interlinear.rs`, `selection.rs`, `marks.rs`, `heights.rs`,
  `workers.rs`, `palette.rs`, `tokens.rs` (`space`, `size`, `motion`, `mark`), `design_tokens_tests.rs`;
  `xtask/src/layers.rs`; `docs/dev/design-tokens.md`; `docs/dev/screenshot-gallery.md`.
- **Do:** create `crates/reading` (`obs-reading`, L5, `[lints] workspace = true`). Move `doc.rs`,
  `view.rs`, `interlinear.rs` (`view.rs:16` imports `InterlinearSource`; the file is egui-free —
  Interlinear lines stay deferred on mobile, the type just has to come along), `selection.rs`,
  `marks.rs`, `heights.rs`, `workers.rs`, plus the reading-page token constants they use. Palette per
  A5: new `obs_reading::TonePalette` over `Rgb` holds the constants and derivation; `obs_ui_egui::Palette`
  keeps its `Color32` fields and API, built by `From<TonePalette>`, so the 130+ call sites compile
  unchanged. Keep every other
  public name; `obs_ui_egui` re-exports them (`pub use obs_reading::…`) so `shell/*` and tests
  compile unchanged apart from imports. Add `obs-reading` to `LAYERS` at 5 and to
  `[workspace.dependencies]`; update `README.md`'s layer table. Move the tests that belong to the
  moved modules. Extend `design_tokens_tests.rs` to scan `crates/reading/src` as well (allowlist
  entries keyed `reading/view.rs` etc.; `TOKEN_FILES` gains the `reading_tokens` module); record the
  scope change in `docs/dev/design-tokens.md` and an "Amended by" line in ADR 0015. Add a short
  `docs/dev/reading-core.md` contract (what lives there, what stays in `ui-egui`).
- **Don't:** change behaviour, numbers or rendering; touch `pane.rs` beyond imports; change
  `Palette`'s field types or any call site; move
  `filters.rs`/`find.rs`/`references.rs` (they use egui); add `uniffi` or mobile code here; drop a
  design-token allowlist entry.
- **Deliverables:** `crates/reading/`, edited `crates/ui-egui`, `xtask/src/layers.rs`, `README.md`,
  `docs/dev/reading-core.md`, `docs/dev/design-tokens.md` note, ADR 0015 amendment line. Label
  `ui-review` (ADR 0020 rule 4: shared painting code moved).
- **Done when:** `cargo xtask ci` passes on the host; `cargo xtask gallery` reports **zero** changed
  goldens (no `--bless`); `cargo xtask bench` within thresholds; `crates/reading` has no `egui`
  dependency (`cargo tree -p obs-reading -i egui` fails to find it); the design-token lint still
  flags a planted literal in `crates/reading/src/view.rs`.
- **Verify:** `cargo xtask ci`; `cargo xtask gallery`; `cargo xtask bench`;
  `cargo tree -p obs-reading --edges normal | grep -c egui` → `0`.
- **Stop-and-ask:** a moved function needs egui to stay correct; any golden changes; the bench
  regresses; a public re-export cannot keep a name; the step overruns (then split the palette
  work into S01b, which must still land before S03).
- **Outcome (2026-11-21):**
  - **Shipped:** `crates/reading` (`obs-reading`, L5, `[lints] workspace = true`, no egui:
    `cargo tree -p obs-reading --edges normal | grep -c egui` → `0`) with `doc`, `view`,
    `interlinear`, `selection`, `marks`, `heights`, `workers` moved by `git mv` and unchanged
    apart from paths and visibility; `palette` (`TonePalette` over `Rgb`: the six tone constants,
    `for_theme`/`for_tone`/`for_style_tone`, `check`, `derive`, `text_on`, `paint` → `StylePaint`
    over `Rgb`, `color`, `overlays`, `inks`, `contrast`, `ContrastShort` and the three contrast
    constants); `reading_tokens` (`space` and `motion` whole, `size::TEXT_MIN`,
    `mark::FOOTNOTE_BOX`), re-exported by `obs_ui_egui::tokens`. `obs_ui_egui::Palette` keeps its
    `Color32` fields and every method; its constants are `const` conversions of `TonePalette`'s,
    `From` goes both ways, and each method converts and asks `TonePalette`. `obs_ui_egui/lib.rs`
    brings the moved modules in by name (`use obs_reading::{doc, view, …}`), so no call site or
    `crate::view::…` path changed and no `pane.rs` line was touched. `LAYERS` gains
    `("obs-reading", 5)`; README layer table gains L5; `docs/dev/reading-core.md`; design-tokens
    note and ADR 0015 "Amended by" line; path fixes in `docs/dev/annotations.md` and
    `interlinear.md`.
  - **Beyond the letter of "Do", each forced by the move:** `impl ComparedBible`/`CopiedBible for
    ReadingDoc` moved from `compare_view.rs` to `reading/doc.rs` (orphan rule); `pub(crate)` items
    the painting code uses became `pub` (`ReadingView::{text_under, top_block_laid,
    drew_all_text}` and four fields, `selection::{text_range_rects, block_range_rects}`); the
    workers' test-only hold/release hooks are behind `cfg(any(test, feature = "test-support"))`,
    a feature `obs-ui-egui` enables in its dev-dependencies only; `unicode-segmentation` moved to
    `obs-ui-egui`'s dev-dependencies (only its tests still use it). The emoji/chrome-font lint
    (`icons_tests.rs`) scans `crates/reading/src` too, so its coverage did not shrink.
  - **Lint:** `design_tokens_tests.rs` reads `source_roots()` (ui-egui's `src`, and
    `crates/reading/src` named `reading/…`); `TOKEN_FILES` gains `reading_tokens.rs`; the eight
    `view.rs`/`selection.rs` allowlist entries are now keyed `reading/view.rs`,
    `reading/selection.rs` (none dropped, all still used). A literal planted in
    `crates/reading/src/view.rs` (`space::XXL + 6.0`) fails it as
    `reading/view.rs:38: float literal 6.0`; reverted. S13 adds `crates/mobile/src` as a third
    `source_roots()` entry.
  - **Tests:** the inline tests of `doc.rs` and `heights.rs` moved with them (2 pass in
    `obs-reading`). The ReadingView and selection tests in `crates/ui-egui/src/tests.rs` stay there:
    they share the `bible()` fixtures and the bench's synthetic Bibles with the egui tests, and
    test `obs-reading` through the re-exports. New `tests::palette_is_the_tone_palette_in_egui_colours`
    checks each tone constant round-trips and every built-in style paints and checks the same.
  - **Measurements:** `cargo xtask ci` all gates pass (780 `obs-ui-egui` tests). `cargo xtask
    gallery` (no `--bless`): 15/15, zero changed goldens (`target/gallery/new` empty).
    `cargo xtask bench` fails one threshold on this Mac under load: `search-preview` action max
    675 ms and 795 ms (max 150). The base commit `055d47d`, benched the same way on the same
    machine, fails it too (678 ms, plus `atlas`, `exegetical-guide` and `text-size-tools`), so it
    is the machine, not this step; steady/fling frame CPU p50 0.89/0.94 ms on the branch vs
    1.12/1.41 ms at base. Worth re-running the bench on an idle machine or CI before merge.
    **Idle re-check (supervisor, 2026-11-21, load ≤ 3):** two interleaved runs each of base `055d47d`
    and branch `14700fa` exceed the same thresholds by similar amounts: `search-preview` action max
    704/619 ms base vs 695/842 ms branch (limit 150), `text-size-tools` 103–121 ms on both, and
    `atlas`/`exegetical-guide` sync p99 ≈ 2.0–2.8 ms on both. `jumps` placeholder frames 21/21 base vs
    20/20 branch, and steady p50 1.10–1.11 vs 1.13–1.14 ms. No regression from S01. The bench
    fails at `master` on this Mac (it is not a `cargo xtask ci` gate), which needs its own issue.
  - **For later steps:** S03 can call `obs_reading::TonePalette` and `reading_tokens` directly;
    the CPU tile compositor of A1 is not here yet (S03 adds it). The PR carries `ui-review`
    (ADR 0020 rule 4: shared painting code moved); no screenshots change.

#### S02 — Scaffold `obs-mobile`, `apps/obs-ffi`, gates, `cargo xtask ios`

- **Goal:** the crate split and the build commands exist and are gated, before any feature.
- **Type:** AFK. **Xcode:** n. **Size:** M. **Parallel:** with S01, S04.
- **Depends on:** —.
- **Read first:** ADR 0021; A1–A3 above; `Cargo.toml` (lints, profiles); `xtask/src/layers.rs`,
  `xtask/src/main.rs` (`GATES`, `USAGE`), `xtask/src/licenses.rs`, `about.toml`; `crates/engine/src/persist.rs:266`;
  UniFFI manual for the version chosen (proc-macro mode, Swift bindings, library-mode bindgen,
  `uniffi_bindgen_main`) and the `uniffi` crate's `Cargo.toml` `license` field; `.gitignore`.
- **Do:** **first, D21 (decided 2026-11-20: MPL-2.0 accepted):** confirm UniFFI's licence field
  really is MPL-2.0. If it is, add `MPL-2.0` to `about.toml` `accepted` with a comment that it is
  accepted for the UniFFI runtime in the iOS binary only. If it is anything else, stop and ask.
  `crates/mobile` (`obs-mobile`, L5, depends on `obs-engine`, `obs-reading` once S01 merges —
  until then on `obs-engine` only) with a `MobileApp` placeholder and `Paths`. `apps/obs-ffi` with
  `uniffi` (proc-macro), `crate-type = ["staticlib", "cdylib", "lib"]`, one
  exported `fn engine_version() -> String` (`obs_engine::updates::APP_VERSION`), `[lints] workspace = true`
  first (see A1; fall back to a crate-local table only if `forbid` breaks the build). `apps/uniffi-bindgen`
  crate (A2). Add
  `FFI_CRATES = ["uniffi"]` to `layers.rs` with the apps-only rule and tests. `[profile.ios]` per A2.
  `UserData: Send` + `assert_send::<Session>()` test. xtask: `ios bindgen` (host cdylib → Swift
  bindings in `target/ios/bindings`), `ios build [--sim-only]` (staticlibs + XCFramework; prints
  "needs Xcode" cleanly when `xcrun --sdk iphoneos --show-sdk-path` fails), `ios check` (host-only
  `cargo check -p obs-ffi` + bindgen). `cargo xtask licenses` gains a second listing over
  `apps/obs-ffi/Cargo.toml` (host target is enough to resolve the graph; written to
  `crates/engine/data/third-party-licenses-ios.json`, checked by the `licenses` gate like the
  desktop file); `about.toml` `accepted` gains `MPL-2.0` (D21), with a comment.
  Document in `docs/dev/mobile-core.md` (Contract / Data /
  Evidence) and `xtask` `USAGE`.
- **Don't:** add screen logic; add Swift files; change `cargo xtask ci`'s gate list or make any gate
  need macOS; write hand `unsafe`.
- **Deliverables:** `crates/mobile/`, `apps/obs-ffi/`, `apps/uniffi-bindgen/`, `xtask/src/ios.rs`,
  `layers.rs` and `licenses.rs` changes, `docs/dev/mobile-core.md`, `.gitignore` entries,
  `about.toml` (D21 outcome noted in a comment).
- **Done when:** `cargo xtask ci` green on the host (incl. the `obs-ffi` licence listing);
  `cargo xtask ios bindgen` writes
  `ObsFFI.swift` + modulemap + header; `cargo xtask wasm` includes `obs-mobile`; the
  `cargo xtask layers` test fails on a fake L5 crate depending on `uniffi`.
- **Verify:** `cargo xtask ci`; `cargo xtask ios bindgen && ls target/ios/bindings`;
  `cargo xtask ios build` (expected: clean "needs Xcode" message, exit non-zero).
- **Stop-and-ask:** UniFFI's licence is not MPL-2.0 (D21 covers MPL-2.0 only); `forbid` fails and the fix needs more than a
  crate-local lint table; `uniffi`
  does not build on edition 2024 / rust 1.95 (`rust-version`); `Session` is `!Send` for a reason
  other than `UserData`.
- **Outcome (2026-11-21):**
  - **Shipped:** `crates/mobile` (`obs-mobile`, L5, on `obs-engine` only until S01 merges:
    `Paths`, `MobileApp` placeholder); `apps/obs-ffi` (UniFFI **0.32.2**, proc-macro mode,
    `staticlib`/`cdylib`/`lib`, `engine_version()`, `uniffi.toml` names the Swift module
    `ObsFFI`); `apps/uniffi-bindgen`; `[profile.ios]` (D20); `UserData: Send` plus
    `session::tests::session_is_send` (nothing else made `Session` `!Send`); `FFI_CRATES` with
    tests; `cargo xtask ios bindgen | check | build [--sim-only]`; the second licence listing
    `crates/engine/data/third-party-licenses-ios.json` (cargo-about `--target aarch64-apple-ios
    --target aarch64-apple-ios-sim`, not the host target: it reads metadata only, so any host
    works); `MPL-2.0` last in `about.toml` `accepted`, with a test that only `uniffi*` crates,
    and nothing in the desktop listing, are taken under it; `docs/dev/mobile-core.md`.
  - **D21 confirmed:** every `uniffi*` crate's `Cargo.toml` says `license = "MPL-2.0"`.
    **No lint exception:** `obs-ffi` builds and passes clippy under `[lints] workspace = true`
    (`forbid`). `rust-version` stays 1.95, but that floor is unverified for UniFFI: 0.32.2
    declares no `rust-version` and was built with stable rustc 1.99.1 only.
  - **Changed circumstance (Xcode 27.2 installed in S05):** the Verify line's "needs Xcode" is
    no longer what `cargo xtask ios build` does on this Mac. It **builds both staticlibs with
    `[profile.ios]` and assembles `target/ios/ObsFFI.xcframework`** (headers dir with
    `ObsFFIFFI.h` and `module.modulemap`). The SDK check stays, takes the probe as a parameter
    and is unit-tested. A real run with `DEVELOPER_DIR=/Library/Developer/CommandLineTools`
    printed the "needs Xcode" message and exited 1. Proof the framework works: a one-line
    Swift `main` compiled with `ObsFFI.swift` against the simulator slice
    (`xcrun --sdk iphonesimulator swiftc -target arm64-apple-ios17.0-simulator …`) printed
    `engine 0.2.0` under `simctl spawn` on an iPhone 17e (iOS 27.2).
  - **Measurements:** first `ios build` about 66 s for the device slice and 35 s for the
    simulator slice (warm host build), then about 9 s; `libobs_ffi.a` 7.5 MB per slice
    (fat LTO over a one-function surface). iOS listing: 119 crates, `uniffi_bindgen` and
    `clap` not among them.
  - **Surprises:** `target/ios/` is also cargo's host-artifact folder for `--profile ios`
    cross builds; our outputs (`bindings/`, `headers/`, `ObsFFI.xcframework/`) sit beside it
    under names cargo does not use. The stock `uniffi-bindgen` writes the module map as
    `ObsFFIFFI.modulemap`, and `ios build` renames it `module.modulemap` for the XCFramework,
    so `uniffi-bindgen-swift` was not needed. For S03's `swiftc` smoke against
    `target/ios/bindings`, clang finds that file only through
    `-Xcc -fmodule-map-file=target/ios/bindings/ObsFFIFFI.modulemap` (or use
    `target/ios/headers` after `ios build`). `ios bindgen` passes `--no-format` so every host
    writes the same bytes.
  - **Known limit:** the `ios` commands assume cargo's output is in `target/` and ignore
    `CARGO_TARGET_DIR` and `build.target-dir`, as `update_e2e.rs` does; with either set,
    `ios bindgen` and `ios build` fail on a missing file.
  - **Deferred:** `IPHONEOS_DEPLOYMENT_TARGET` is not set (rustc's default minimum applies);
    S06 sets it to the app's minimum if Xcode warns. A debug simulator variant for S06's
    edit-run loop is not built: `--sim-only` uses `[profile.ios]` too. **Later steps
    edited:** S06 (`ios build` already builds and assembles the XCFramework).

#### S03 — "Hello engine" through the FFI on the macOS host

- **Goal:** prove the whole chain without Xcode: Swift → UniFFI → `obs-mobile` → engine → layout →
  tile bytes.
- **Type:** AFK. **Xcode:** n. **Size:** M.
- **Depends on:** S01, S02.
- **Read first:** `crates/ui-egui/src/lib.rs:80–150` (host wiring order: library scan, caches, user
  db, Starter bundle); `crates/reading` (`ReadingDoc`, `ReadingView`, `LayoutClient`);
  `crates/render/src/atlas.rs` (`quads_from`, `coverage`, `take_dirty`); `docs/dev/starter-library.md`
  "Finding the bundle"; `docs/dev/bench.md`.
- **Do:** `obs-render::composite` (A4) with tests (a known glyph covers expected pixels; role
  colours applied; premultiplied). `obs-mobile::MobileApp::boot(paths, bundle_dir)`: `Session::new(JobMode::Threads(n))`,
  scan, caches, open/create `user.db`, `sync_starter_library(Some(bundle))`, restore or default
  reader; `open_reader(resource_id, reference)`, `layout_tile(reader, tile_rect, scale) -> Tile{w,h,rgba}`.
  `obs-ffi` exposes them. Tests in two tiers (CI never builds `dist/starter`, `ci.yml:17–35`, and
  `cargo xtask test` runs `nextest --workspace`): (1) CI-run tests use `obs_engine::testing`
  fixtures (`bsb_john_1`, `bsb_psalm_3`, `tahot`, `crates/engine/src/testing.rs`) — lay out, composite,
  assert pixels; (2) a `#[ignore = "needs dist/starter from `cargo xtask starter`"]` integration
  test under `apps/obs-ffi/tests` (the repo convention, `crates/engine/src/starter_tests.rs:921`)
  boots against `dist/starter` (or the S04 subset) and writes `target/ios/gen1.png` for eyeballing.
  A Swift smoke: `apps/obs-ffi/swift-smoke/main.swift` compiled with the CLT
  `swiftc -I target/ios/bindings -L target/debug -lobs_ffi` (verify flags), printing the engine version
  and the tile size; wired as `cargo xtask ios smoke`.
- **Don't:** build for iOS targets; write any UIKit; design the final FFI (this is a spike surface,
  marked `#[doc(hidden)]`/`// S03 spike` to be replaced in S13).
- **Deliverables:** `obs-render::composite`, `obs-mobile` boot + tile API, FFI test, Swift smoke,
  `cargo xtask ios smoke`, Evidence section in `docs/dev/mobile-core.md`.
- **Done when:** test produces a readable PNG of Genesis 1 in BSB at 3×; `cargo xtask ios smoke`
  prints the version and `tile 1170x768`; `cargo xtask ci` green.
- **Verify:** `cargo xtask ci` (fixture tests run here); `cargo xtask starter` (once; long) or
  `cargo xtask ios bundled-set` if S04 merged, then
  `cargo test -p obs-ffi --test hello_engine -- --ignored`; `cargo xtask ios smoke`.
- **Stop-and-ask:** compositing a full-width 3× tile takes > 8 ms on the host (then B is in doubt;
  note it for S07 and continue); `swiftc` from the CLT cannot import the generated module.
- **Outcome (2026-11-21, branch `mobile/s03-hello-engine`):**
  - **Shipped:** `obs_render::{composite, composite_over, tile_size, RoleColours}`
    (`crates/render/src/composite.rs`: RGBA8 premultiplied, background fill then each quad
    in its role's colour with atlas coverage as alpha, source over; a quad whose uv lies
    outside the atlas is skipped, so no index can panic, but a quad made before the atlas
    grew or started over is not detected, so callers composite each part before
    `start_over`; a tile's pixel size is `round(x1·scale) − round(x0·scale)` (and so for y),
    so tiles at fractional offsets meet without a seam; sizes whose bytes overflow give an
    empty tile); `obs_reading::tile` (`compose_tile`,
    `Tile`, `text_column`: lays out the band a tile shows, places glyphs with `quads_from`
    and starts the atlas over between parts as the desktop pane does, text column centred
    between `view::MARGIN`s with no scrollbar) and `TonePalette::role_colours`; `obs-mobile`
    now depends on `obs-reading`, `obs-render`, `obs-store` (and `obs-model`,
    `obs-scripture`) and has `MobileApp::boot(paths, bundle)` (folders, `Session` with
    `JobMode::default_for_host()`, scan, Search index/alias/log folders, `user.db`,
    `sync_starter_library`, first Bible in Preferred order as reader 0; native only),
    `open_reader(resource_id, reference)`, `layout_tile(reader, top, width, height, scale)`
    (refuses a tile over `MAX_TILE_PX` = 8192 device px on a side),
    `problems`, `resource_ids`, `reader_resource`, `MobileError`, `ReaderId`; `obs-ffi`
    mirrors them (Object `MobileApp` over a `Mutex`, Records `Paths`/`Tile`, Error
    `MobileError.Failed { message }`). Every spike item is `#[doc(hidden)]` and marked
    `S03 spike`. `apps/obs-ffi/swift-smoke/main.swift` and `cargo xtask ios smoke
    [--bundle DIR]`; Contract, Commands and Evidence in `docs/dev/mobile-core.md`; `tile` row
    in `docs/dev/reading-core.md`.
  - **Tests:** CI-run over `obs_engine::testing` fixtures (`bsb_john_1`, `tahot`): 8 in
    `obs-render` (a known glyph's pixels equal its atlas coverage in the ink and nothing
    outside it; role colours; premultiplied; clipping; tiles at fractional offsets stack into
    the whole; an overflowing size; a wrong-length buffer), 4 in `obs-reading` (a 1170 × 768
    tile: margins page-coloured, ink in the column, Paper and Dark; two half tiles equal the
    whole, and so do tiles at fractional tops), 4 in `obs-mobile` (boot over a fixture Bundled
    set and again; readers; refusals, tiles over the cap among them; boot with nothing), 1 in
    `apps/obs-ffi/tests/hello_engine.rs` (the same through the FFI), 3 in xtask (`smoke` options, its pass check, the missing
    Bundled set message). `#[ignore]`d: `hello_engine::genesis_1_in_the_bsb_at_3x_from_the_real_bundled_set`
    (`apps/ios/Bundled/starter`, else `dist/starter`) writes `target/ios/gen1.png`.
  - **The PNG** (1170 × 2304, 390 × 768 pt at 3×, Paper) reads as Genesis 1 in the BSB:
    the large accent-blue chapter numeral 1, verses 1–2, the "The First Day" heading, verses
    3–5 with gold verse numbers and green Footnote callers a and b, "The Second Day" and the
    start of verse 6, sharp Gentium Plus.
  - **Measurements** (load average ~14 from other lanes): release, a warm full-width 3×
    tile (1170 × 768; laid out and rastered already, so fill, quads and composite) 0.42–0.48
    ms median, 0.75 ms max (20 tiles × 3 runs); the first 1170 × 2304 tile including layout
    and raster 2.5–2.9 ms. Test profile (`opt-level = 1`): 2.5–3.4 ms median, 7.9 ms max.
    **Far under the 8 ms stop line: option B stands for S07.** `boot` into an empty folder
    over the Bundled set (copying 43.1 MB) 0.55–1.4 s; `open_reader` of the whole BSB
    (`ReadingDoc::open`) 0.50–1.2 s, worth watching on a phone in S07/S13.
    `cargo xtask ios smoke` prints `engine 0.2.0` and `tile 1170x768` (swiftc ~1.5 s, run
    ~1.3 s). The iOS licence listing did not change (119 crates).
  - **Flags verified:** `-I target/ios/bindings` alone does not import the module
    ("cannot find 'uniffi_obs_ffi_fn_func_engine_version' in scope"); the smoke passes
    `-Xcc -fmodule-map-file=target/ios/bindings/ObsFFIFFI.modulemap` (no `-I` needed).
    `-L target/debug -lobs_ffi` links the host cdylib (`libobs_ffi.dylib` beats the `.a`),
    so an rpath to `target/debug` is added. The Command Line Tools' own `swiftc` (Swift 6.4,
    `DEVELOPER_DIR=/Library/Developer/CommandLineTools`) builds and runs it too, so neither
    stop-and-ask fired.
  - **Deferred:** restoring a reader at boot (the Mobile Last session is S13's own JSON;
    restoring the desktop Workspace snapshot would be the wrong format), so boot always
    opens the first Bible at its start; Highlights, other Marks, selection and Note-marker
    room in tiles (S13/S17); "Follow system" dark (the spike resolves Auto as a light Theme;
    S13/S14b take the system appearance); the pump thread and `Changed` events (S13); the
    tile size cap's final value (S03 added 8192 px a side; S13); the cost of a tile growing
    with `top` (S07's tile cache). `cargo xtask ios build` was not run (the step's "don't
    build for iOS targets"); `obs-ffi` now also pulls `obs-reading`/`obs-render`, pure Rust
    already in its graph, so S06's first `ios build` is the check. No later step's
    assumption proved wrong; S13's Do and Read first and S07's Do now name these deferrals.
    S13's Swift smoke check means updating `main.swift` to the designed surface.

#### S04 — Bundled set and Study set in the Starter build

- **Goal:** a pinned, reproducible Bundled set directory for the app, and set membership in the
  manifest.
- **Type:** AFK. **Xcode:** n. **Size:** M. **Parallel:** with S01–S03.
- **Depends on:** —.
- **Read first:** #333 resolution; CONTEXT.md **Bundled set**, **Study set**; `starter/sources.toml`;
  `xtask/src/starter/mod.rs` (outputs, determinism check, size budget), `xtask/src/starter/lock.rs`
  (`Source` is `#[serde(deny_unknown_fields)]`, `lock.rs:22–23`, so `mobile_set` must be added
  there too); `crates/engine/src/starter.rs`
  (`StarterItem`, `StarterManifest::parse`, `MANIFEST_FORMAT`); `Cargo.toml` `[workspace]`;
  `.github/workflows/starter.yml`; `docs/dev/releasing.md`.
- **Do:** add `mobile_set` to `[[source]]` entries in `sources.toml` (bundled: `bsb`,
  `openbible-xrefs`, `strongs-hebrew`, `strongs-greek`; study: the fifteen in A7; more: `asv`, `web`,
  `mhc`) and write it into `starter-manifest.json` (format stays 1; additive). `StarterItem` gains
  `mobile_set: Option<String>`. `cargo xtask ios bundled-set [--from dist/starter] [--out apps/ios/Bundled/starter]`
  copies the bundled items' `.obr`, `search/<id>*`, `aliases/*` involving them, `CHANGES-<id>.md`
  and a filtered manifest; writes/verifies `apps/ios/bundled-set.lock` (ids, revisions, sha256,
  bytes; `--update-lock` to refresh); fails if the directory exceeds 50 MB (raised from 40 MB at S04;
  see its Outcome). Record sizes in the
  starter build report. Add `exclude = ["apps/ios"]` to `[workspace]` in `Cargo.toml` (A6: the
  `apps/*` member glob otherwise fails on a directory without a manifest).
- **Don't:** change any Resource's content or id; ship the Study set in the app; touch the release
  workflow.
- **Deliverables:** `sources.toml` + xtask changes, `apps/ios/bundled-set.lock`,
  `cargo xtask ios bundled-set`, a "Mobile sets" section in `docs/dev/starter-library.md`.
- **Done when:** `cargo xtask starter --check-determinism` still passes; the lock matches a fresh
  build; the subset dir is ≤ 50 MB and `sync_starter_library` accepts it as a bundle (an
  `#[ignore]`d engine test against the subset, plus a CI-run test over a synthetic 2-item bundle
  built from `obs_engine::testing` fixtures); `cargo metadata` succeeds with `apps/ios/` present.
- **Verify:** `cargo xtask starter --check-determinism`; `cargo xtask ios bundled-set`;
  `du -sh apps/ios/Bundled/starter`; `cargo test -p obs-engine starter`;
  `cargo test -p obs-engine starter -- --ignored`; `cargo metadata --format-version 1 >/dev/null`.
- **Stop-and-ask:** the bundled subset exceeds 50 MB (was 40 MB; see the Outcome); a Resource in
  #333's lists is not in `sources.toml`.
- **Outcome (2026-11-21, branch `mobile/s04-mobile-sets`):** shipped `mobile_set` on all 22
  `[[source]]` entries (`bundled`: `bsb`, `openbible-xrefs`, `strongs-hebrew`, `strongs-greek`;
  `study`: the fifteen of A7, every one already in `sources.toml`; `more`: `asv`, `web`, `mhc`),
  parsed as `lock::MobileSet` and written into `starter-manifest.json` (format 1, field omitted
  when unset); `StarterItem.mobile_set: Option<String>`; a **Mobile sets** table in the build
  report; `cargo xtask ios bundled-set [--from] [--out] [--update-lock | --verify]`
  (`xtask/src/starter/bundled_set.rs`; `--verify` checks an already-copied folder without
  `dist/starter`, as S06's run-script phase expects); `apps/ios/bundled-set.lock` (every file's
  path, sha256 and bytes, plus each Resource's id, revision, `.obr` sha256 and bytes);
  `exclude = ["apps/ios"]` (without it `cargo metadata` fails with "failed to load manifest for
  workspace member `…/apps/ios`"; with it, it passes); `/apps/ios/Bundled/` in `.gitignore`; a
  "Mobile sets" section in `docs/dev/starter-library.md`. Tests: a CI-run xtask test checks every
  lock entry against #333's lists; another builds a 3-Resource Starter library, copies its
  2-Resource Bundled set (lock written, matched, a changed lock, a changed file and the cap
  refused) and adds it through `sync_starter_library`; a CI-run engine test adds a 2-Resource
  Bundled set built from `obs_engine::testing` fixtures; an `#[ignore]`d engine test adds the real
  subset (4 Resources in 0.12 s, prebuilt caches used, G26 from Strong's Greek). **Measured:** the
  Bundled set is 43.1 MB (43,116,439 bytes; `du -sh` 41M): 34.46 MB of `.obr` + 8.31 MB of
  prebuilt Search indexes + 0.34 MB manifest + 1 KB change record, no alias caches (no People &
  places index or Dictionary in it). #333's "about 31 MB" counted `.obr` files only. **Decision
  (parent session, at the stop-and-ask):** the cap is 50 MB (`bundled_set::MAX_BYTES`): it exists to
  catch a Study set Resource bundled by mistake, not to squeeze the Bundled set; all four `.obr`
  and all four prebuilt indexes ship as #333 says. Other sizes: Study set 136.6 MB of `.obr`
  (+ 47.1 MB indexes, built on the device; #333 said about 125 MB), `more` 136.4 MB. The four
  bundled `.obr` files are byte-identical to the latest release's (built on Linux), so their lock
  lines hold across hosts (the Search index files were not compared across hosts). `dist/starter` (397.8 MB, 22 Resources) is left in this worktree for S03.
  Deferred, as planned: S23's manifest merge (with only the Bundled set's manifest, the Starter
  library rows see 4 Resources). Gates: `cargo xtask starter --check-determinism` passes (two
  builds byte-identical, ~28 s with a warm download cache); `cargo xtask ci` passed every gate but
  one test under four parallel lane builds: `obs-ui-egui`
  `find_bar::text_comparison_rows_are_found_and_stepped_through` ("Enter #126: []", no hit painted
  yet), a crate S04 does not touch, which passes on its own (24.8 s): a load flake.

### M1 — Prototypes (#331, #335)

#### S05 — Install Xcode and the iOS toolchain (HITL)

- **Goal:** the Mac can build and run iOS apps; the agent can use `xcodebuild` and `simctl`.
- **Type:** HITL. **Xcode:** y. **Size:** S (owner time ~1–2 h incl. download).
- **Depends on:** —. Can happen any time; blocks S06.
- **Owner checklist:** install the current Mac App Store Xcode that supports macOS 27 and meets
  App Store Connect's current SDK requirement (check Apple's "Upcoming requirements" page; the
  requirement moves yearly — `docs/research/mobile-store-policy-and-builds.md` recorded "Xcode 26 /
  iOS 26 SDK" in 2026); record the exact version in the Outcome, S32 pins `DEVELOPER_DIR` to it;
  open it once, accept the licence (`sudo xcodebuild -license accept`), install the iOS platform
  and one iPhone + one iPad simulator; `sudo xcode-select -s /Applications/Xcode.app`;
  `brew install xcodegen`; `rustup target add aarch64-apple-ios aarch64-apple-ios-sim`; sign in to
  Xcode with an Apple ID (free Personal Team) under Settings → Accounts; plug in the iPhone, enable
  Developer Mode, trust the Mac. **Do not enrol in the Developer Program yet:** enrolment fixes the
  account type and seller name, which #336 decides (individual vs organisation, S33 item 3).
  Starting S33's #336 interview early is fine.
- **Done when (agent verifies next session):** `xcodebuild -version`, `xcrun simctl list devices available`,
  `xcrun --sdk iphoneos --show-sdk-path`, `xcodegen --version`, `rustup target list --installed`
  all succeed; `xcrun xctrace list devices` shows the iPhone.
- **Stop-and-ask:** n/a (owner step). Record Xcode/SDK/simulator versions in the Outcome.
- **Outcome (2026-11-20):**
  - **Installed:** Xcode 27.2 (build 27B52) at `/Applications/Xcode-27.2.0.app` (installed with
    `xcodes`, so the name carries the version; S32 pins `DEVELOPER_DIR` to this path or its CI
    equivalent); iPhoneOS and iPhoneSimulator SDK 27.2; Swift 6.4.2; iOS 27.2 Simulator runtime
    (24B8117, plus 24B91016s for the newest devices); simulators include iPhone 17, iPhone 18 Pro,
    iPad Pro 11-inch (M5), iPad Air 11-inch (M4). `rustup` targets `aarch64-apple-ios` and
    `aarch64-apple-ios-sim`; `xcodegen` 2.46.0, `mas` 7.0.0, `xcodes` 2.1.0, `aria2` from Homebrew.
  - **Checked:** `xcodebuild -version`, `-checkFirstLaunchStatus`, both SDKs; simulator boot and
    `io … screenshot` work; `codesign --verify --strict` of the Xcode bundle passes.
  - **Not done yet (needed only from S07/S08):** Apple ID in Xcode → Settings → Accounts
    (Personal Team); iPhone in Developer Mode and trusting this Mac. Developer Program not
    joined, by design (S33).
  - **Agent-shell quirks on this Mac (executors read this):** in the agent's shell, executing some
    files directly is killed with SIGKILL (exit 137). Known cases: `/opt/homebrew/bin/brew`,
    `actionlint`, and Xcode 27.2's `usr/bin/simctl`, which is a bash wrapper around the real
    binary, so `xcrun simctl` dies too. Copies of the same files run, and the owner's own Terminal
    is not affected. Workarounds: run Homebrew as `HOMEBREW_NO_AUTO_UPDATE=1 bash /opt/homebrew/bin/brew …`;
    call simctl as
    `/Library/Developer/PrivateFrameworks/CoreSimulator.framework/Versions/A/Resources/bin/simctl`
    (or `bash "$(xcrun -f simctl)" …`). Use the same form in `cargo xtask ios` helpers rather than
    `xcrun simctl`, or try both. `xcodebuild` itself is unaffected. Cause not found; the macOS
    security log shows nothing for these kills.

#### S06 — iOS project scaffold

- **Goal:** `apps/ios` builds, runs in the simulator, links the XCFramework and shows the engine
  version; tests and screenshots work from the command line.
- **Type:** AFK. **Xcode:** y. **Size:** M.
- **Depends on:** S03, S05.
- **Read first:** A2, A6; S03 outcome; XcodeGen `ProjectSpec.md`; UniFFI Swift/Xcode integration
  page; `docs/research/mobile-store-policy-and-builds.md` (required-reason APIs,
  `PrivacyInfo.xcprivacy`).
- **Do:** `cargo xtask ios build [--sim-only]` already builds `aarch64-apple-ios(-sim)` staticlibs
  with `[profile.ios]` and creates `target/ios/ObsFFI.xcframework` (S02 Outcome); S06 adds a
  debug variant for the simulator if the edit-run loop needs one, and wires the framework in.
  `apps/ios/project.yml`: app target `OpenBible` (dev bundle id `io.github.inhesrom.openbible.dev`
  per A6, iOS 17, iPhone + iPad,
  portrait + landscape), `OpenBibleTests`, `OpenBibleUITests`, local package `OpenBibleCore`, a
  run-script phase that calls `cargo xtask ios build` and `cargo xtask ios bundled-set --verify`
  (Xcode's build PATH lacks `~/.cargo/bin`: the script sources `$HOME/.cargo/env` or uses
  `${CARGO:-$HOME/.cargo/bin/cargo}`; verify),
  a resources folder reference to `Bundled/starter`. Check Apple's App ID / free-provisioning docs
  for whether a Personal Team registration of an id blocks a later paid-team registration; record
  the answer in `docs/dev/ios-project.md` (it decides whether the `.dev` suffix is belt-and-braces or
  required). `PrivacyInfo.xcprivacy` with the file-timestamp
  reason for `std::fs`/SQLite. Minimal SwiftUI `App` showing `engineVersion()` and the Bundled set
  count after `boot`. One XCTest, one XCUITest that launches and screenshots. `docs/dev/ios-project.md`:
  how to generate, build, test, screenshot, and the gitignore policy.
- **Don't:** any reader UI; any rule in Swift; commit `.xcodeproj`, bindings or the XCFramework;
  upload anything; use the final bundle id.
- **Deliverables:** `apps/ios/**`, xtask changes, `docs/dev/ios-project.md`,
  `docs/previews/ios/s06-hello.png`.
- **Done when:** `xcodegen generate && xcodebuild build -scheme OpenBible -destination 'platform=iOS Simulator,name=<booted>'`
  succeeds; `xcodebuild test` runs both test targets; the screenshot shows the version; the
  Bundled set appears in the app's library listing (count = 4); `cargo xtask ci` green; PR body has
  a Shell audit.
- **Verify:** `cargo xtask ios build --sim-only`; `cd apps/ios && xcodegen generate`;
  `xcodebuild test -scheme OpenBible -destination 'platform=iOS Simulator,name=iPhone 17'`
  (adjust name to `simctl list`); `xcrun simctl io booted screenshot docs/previews/ios/s06-hello.png`.
- **Stop-and-ask:** `rusqlite` bundled SQLite fails to cross-compile; linking needs a framework or
  flag not documented; the Bundled set copy at first launch takes > 3 s in the simulator.
- **Outcome (2026-11-21, branch `mobile/s06-ios-project`):**
  - **Shipped:** `apps/ios/project.yml` (XcodeGen 2.46: app `OpenBible`, dev bundle id
    `io.github.inhesrom.openbible.dev` as the overridable setting `OBS_BUNDLE_ID`, iOS 17,
    iPhone + iPad, portrait + landscape, Info.plist keys as build settings, display name "Open
    Bible", Swift 6; `OpenBibleTests` hosted XCTest; `OpenBibleUITests` XCUITest; local package
    `OpenBibleCore`; the run-script phase "Rust core and Bundled set"; `Bundled/starter` as a
    folder reference, bundled as `starter`); `apps/ios/Packages/OpenBibleCore/Package.swift`
    (binary target `ObsFFI` + Swift target `OpenBibleCore` over the generated `ObsFFI.swift`);
    `PrivacyInfo.xcprivacy` (no tracking, nothing collected, FileTimestamp `C617.1`); a SwiftUI
    hello screen (`HelloModel`, `HelloView`) that boots the S03 spike off the main thread under
    `Application Support/OpenBible` and lists the library: engine 0.2.0, Resources 4 (`bsb`,
    `openbible-xrefs`, `strongs-hebrew`, `strongs-greek`); `docs/dev/ios-project.md`;
    `docs/previews/ios/s06-hello.png` (**UI change: the PR needs `ui-review`**). xtask:
    `ios build` builds the staticlibs with `IPHONEOS_DEPLOYMENT_TARGET=17.0` and copies the
    XCFramework and `ObsFFI.swift` into the package, writing only changed files (S02's
    deferred deployment target, done); tests for that copy and for `project.yml` /
    `Package.swift` naming the same minimum iOS. The Shell audit is in the PR body;
    `docs/dev/mobile-core.md` lists the accepted presentation-only decisions.
  - **Run-script phase:** runs `cargo xtask ios build [--sim-only]` and `ios bundled-set
    --verify` under `env -i` (Xcode's PATH lacks `~/.cargo/bin`; `${CARGO:-$HOME/.cargo/bin/cargo}`
    works). **Surprise:** Xcode copies the package's XCFramework (`ProcessXCFramework`) and
    compiles `OpenBibleCore` before any target's phases, so a library the phase rebuilds
    would be linked one build late; the phase fails with "the Rust core changed after Xcode
    had read it; build again" when it changed the package, and the command-line order
    (`ios build` first) never meets it. No debug simulator variant: not needed yet.
  - **Checks:** `xcodegen generate`, `xcodebuild build` and `xcodebuild test` (2 unit tests:
    boot over the Bundled set with 4 Resources and again, a failed boot's message; 1 UI test:
    launch, version, `Resources, 4`, screenshot attachment) pass on an iPhone 17 simulator
    (iOS 27.2). No stop-and-ask fired: SQLite cross-compiles, linking needs no extra flag or
    framework, and boot over the Bundled set takes 1.8–1.9 s in the simulator (a second boot
    1.6–1.8 s, so mostly opening the BSB, not the copy).
  - **Measurements:** `libobs_ffi.a` (simulator) 27.2 MB (S02: 7.5 MB before S03's surface);
    first `ios build --sim-only` 2 min, an Xcode build with no Rust change about 4 s.
  - **Not verified:** whether a Personal Team registration of an App ID blocks a later
    paid-team registration (Apple's site unreachable from the session); recorded as
    unverified in `docs/dev/ios-project.md`, so the `.dev` id is treated as required. Nothing
    was signed for a device and no Apple ID was added.
  - **Found for later steps (none edited):** `performAccessibilityAudit()` (A6) fails on the
    system List's secondary text ("Contrast nearly passed"), so the UI test does not run it
    yet (S14b / ADR 0023's bar). `nm` on the core shows `statfs`/`fstatfs` (disk-space
    required-reason category, from SQLite) besides the timestamp calls, for S34's privacy
    audit. The app's `MARKETING_VERSION` is `0.0.0` until S34 sets a scheme. Agent shells
    sometimes SIGKILL Xcode's generated run-script file and once failed to load the SwiftUI
    macro plug-ins; both workarounds are in `docs/dev/ios-project.md`.

#### S07 — #331 UI stack prototype: the hosted text view

- **Goal:** a throwaway-quality but measurable build that answers #331's questions on device.
- **Type:** AFK (owner installs on the phone in S08). **Xcode:** y. **Size:** L.
- **Depends on:** S04, S06.
- **Read first:** #331, #325 resolution, ADR 0021 rule 2, `docs/research/mobile-rust-ui.md`
  "Hosting our Resource-text pipeline" and "Open questions"; A4; `crates/reading` (`ReadingView`
  scroll anchoring, `SYNC_BUDGET`, `Selection`, `range_rects`); `crates/ui-egui/src/pane.rs`
  (`reading_pane`, how desktop pairs quads with the texture — for parity, not to copy egui code);
  Apple docs: `UITextInput`, `UITextInteraction`, `UIAccessibilityElement`, `CATiledLayer`
  (executor fetches; the research notes these pages were JS-blocked).
- **Do:** in `obs-mobile`: a `ReaderFrame` API (visible tiles for a scroll offset, tile cache keyed
  by block + scale, Highlight/selection rects, accessibility items, hit → `DocPos`, word/Verse
  expansion, a `tap → StudySheetStub {word, lemma?, verse, cross_ref_count}` using
  `obs_engine::word_info`/`cross_references`). Tile cost grows with `top` in the S03 spike
  (`compose_tile` lays out from the Current location down); the tile cache keyed by block +
  scale must bound it. In Swift: `ResourceTextView` (UIKit, in `UIViewRepresentable`) scrolling one whole Bible (BSB) with B tiles; `UITextInput` +
  `UITextInteraction` selection with handles; `UIAccessibilityElement` per Verse; a reference text
  field (`reference.parse` command) and book→chapter grid stub; Dynamic Type → Text size; Page tone
  switch; a bottom-sheet stub on tap. Instrument: tile composite time, frames dropped during a
  fast fling (`CADisplayLink` or Instruments' Animation Hitches), cold start to first text, RSS
  (simulator numbers are indicative only; the device numbers in S08 decide). If B drops > 1 % of
  frames on the simulator at 3×, implement the Metal path in Swift (A4) behind a switch and measure
  both. Write `docs/dev/mobile-text-view.md` (Contract / Data / Evidence) with the measurements table.
- **Don't:** polish chrome; implement Notes/Highlights persistence; study rules in Swift (the
  word/Verse decision, Verse labels, colours all come from Rust); touch desktop behaviour.
- **Deliverables:** prototype build (installable `.app` via Xcode to the owner's phone), Rust
  APIs (clearly `// prototype` where throwaway), `docs/dev/mobile-text-view.md`, previews
  `docs/previews/ios/s07-*.png` (light/dark/sepia, RTL Hebrew if a Hebrew Resource is at hand:
  `tahot` is Study set, so use the Study set dir from `dist/starter` in debug via `OBS_STARTER_DIR`-like
  override in `Paths`).
- **Done when (go criteria, measured on the simulator; S08 repeats on device):** continuous
  fling through Genesis–Exodus with ≤ 1 % dropped frames; a tile composites in ≤ 4 ms at 3× on the
  host; cold start to first text ≤ 1.5 s; selection handles, loupe and the system edit menu appear
  on long-press; XCUITest asserts one accessibility element per Verse, labelled with the Verse's
  reference and text, in reading order, and `performAccessibilityAudit()` passes (the Accessibility
  Inspector is a GUI the executor cannot drive; the owner uses VoiceOver itself in S08); typed
  "Jn 3:16" navigates; tapping a word opens the stub sheet showing the Verse reference and its
  cross-reference count (Word Info on BSB needs the Study set, #333; with the debug Study set
  override the sheet also shows a Strong's number); RTL Hebrew paragraph renders right-aligned;
  PR body has a Shell audit.
- **Verify:** `xcodebuild test`; XCUITest `prototypeSmoke` (scroll, long-press, type reference,
  tap word); `xcrun simctl io booted screenshot …`; numbers recorded in `mobile-text-view.md`.
- **Stop-and-ask:** both B and A miss the budget; `UITextInteraction` cannot be attached to a
  non-`UITextView` view in the installed iOS (then propose the fallback of custom handles and
  stop); the Rust staticlib exceeds 40 MB.

#### S08 — #331 owner judgement on a real iPhone (HITL)

- **Goal:** the go/no-go on SwiftUI + hosted text view, made where only a human can.
- **Type:** HITL. **Xcode:** y. **Size:** S.
- **Depends on:** S07.
- **Owner checklist:** `cd apps/ios && xcodegen generate`, open `OpenBible.xcodeproj`, select the
  `OpenBible` target → Signing & Capabilities → pick your Personal Team (bundle id stays the `.dev`
  one), choose the iPhone as the run destination, Run; on the phone, trust the developer
  (Settings → General → VPN & Device Management) and launch again; repeat on an iPad if available.
  Judge against #331's criteria and ADR 0021 rule 2: (1) feel — fling,
  rubber-band, tap latency; (2) input — long-press selection, handles, loupe, keyboard in the
  reference field, Hebrew keyboard if present; (3) accessibility — VoiceOver through a chapter,
  Dynamic Type at the largest accessibility size; (4) performance — note any hitch, battery heat
  after 10 minutes of reading; (5) cost — read `docs/dev/mobile-text-view.md` measurements and the
  Swift line count. Decide **go / no-go** and, separately, **the Android half of #331**: #331's text
  asks for an Android phone too; ADR 0021 moved Android to a later release. Default recommendation:
  judge iOS now, record "Android half deferred to the Android release; Compose over the same
  `obs-ffi`" in ADR 0023, so #331 can close. Owner may instead keep #331 open for Android.
- **Done when:** the owner writes the verdict (go/no-go, B or A, Android decision, perf notes)
  as a comment on the S07 PR or a note the S09 executor can quote.

#### S09 — ADR 0023 and the #331 bookkeeping

- **Goal:** record the stack decision, the hosting option, and resolve the "Not yet specified"
  fog items that are now answerable: performance targets + floor hardware, accessibility bar.
- **Type:** AFK (write) + HITL (GitHub). **Xcode:** n. **Size:** S–M.
- **Depends on:** S08 (go). If no-go → S09-F instead.
- **Read first:** S08 verdict; ADR template (`docs/adr/0021-*.md` shape: context, decided list,
  Considered Options, Consequences); #324 "Decisions so far" format (one line per closed ticket);
  `docs/research/mobile-rust-ui.md` "Open questions" 1–6; `docs/dev/accessibility.md`.
- **Do:** write `docs/adr/0023-mobile-shell-is-swiftui-and-resource-text-is-hosted-as-rust-tiles.md`
  (title to match the verdict; check `ls docs/adr` for the next free number — the series has had
  collisions, e.g. two 0008s). Decide in it: hosting option (B or A-in-Swift), selection via
  `UITextInteraction`, VoiceOver via `UIAccessibilityElement`s, **performance targets** (proposed:
  floor hardware A12 / iOS 17 devices; scroll at the display's refresh with ≤ 1 % dropped frames;
  cold start to text ≤ 1.5 s; chapter open ≤ 100 ms warm; reading RSS ≤ 150 MB; no sustained CPU
  > 10 % while idle), **accessibility bar** (VoiceOver on reader and every screen, Dynamic Type
  through the accessibility sizes, 44 pt targets, Reduce Motion/Increase Contrast honoured;
  claim the App Store accessibility labels at 1.0), and the Android-half decision. Add to
  `docs/dev/mobile-text-view.md` the targets as Evidence to keep. Update CONTEXT.md only if a new
  term appeared. Draft the #331 resolution comment and the #324 "Decisions so far" line in the PR
  body for the owner to paste.
- **Owner HITL:** post the resolution comment on #331, close it, add the Decisions line to #324,
  strike "Performance targets…" and "Accessibility on mobile…" from #324's "Not yet specified".
- **Deliverables:** ADR 0023, edits to `mobile-text-view.md`, PR body with the two texts.
- **Don't:** edit GitHub; soften a target without the owner's say; decide the Android half alone.
- **Done when:** ADR merged; owner confirms the GitHub edits (record in Outcome).
- **Verify:** `cargo xtask ci` (docs only); `ls docs/adr | grep 0023` shows one file.
- **Stop-and-ask:** the verdict is "go, but…" with conditions the plan does not cover.

#### S09-F — Fallback branch (only if S08 = no-go)

- **Goal:** keep ADR 0021's fallback honest: one cross-platform shell (Flutter or Compose
  Multiplatform) hosting the same `obs-ffi`.
- **Type:** AFK. **Xcode:** y. **Size:** L.
- **Depends on:** S08 (no-go).
- **Read first:** S08 verdict; ADR 0021 Considered Options; `docs/research/mobile-rust-ui.md`
  (Flutter / Compose MP rows); A1–A4.
- **Do:** ADR 0023 records the no-go and why; spike the hosted text view in the fallback stack
  (Flutter: `Texture`/`dart:ffi` with `uniffi-dart` or a C header; Compose MP on iOS: Kotlin/Native +
  UniFFI Kotlin bindings — verify both toolchains' iOS support); same go criteria as S07; then
  rewrite M1–M7's Swift-specific steps in this plan for the chosen stack. The Rust side
  (`obs-reading`, `obs-mobile`, `obs-ffi`) is unchanged — that is the point of A1.
- **Don't:** change `obs-mobile`'s surface to suit the new shell; keep both shells.
- **Deliverables:** ADR 0023 (no-go), spike under `apps/<stack>/`, rewritten steps in this plan.
- **Done when:** the spike meets S07's go criteria on the simulator and the owner repeats S08 on it.
- **Verify:** the stack's own test command; `cargo xtask ci`.
- **Stop-and-ask:** before choosing between Flutter and Compose MP (owner decision).

#### S10 — #335 screen anatomy clickable prototype

- **Goal:** decide navigation, where reference entry lives, how Study sheet / Guides / Notes /
  Search are reached, and phone-vs-tablet layout, by clicking, not arguing.
- **Type:** AFK. **Xcode:** y. **Size:** L. **Parallel:** with S13.
- **Depends on:** S09.
- **Read first:** #335, #329 resolution, `docs/research/mobile-bible-app-anatomy.md`, #330 "In v1"
  (Split view, Study sheet, Settings), CONTEXT.md **Study sheet**, **Split view**; Apple HIG
  (navigation, sheets, split views, iPad multitasking); S07's reader.
- **Do:** a SwiftUI prototype over the S07 reader with stubbed data from `obs-mobile` (real
  library listing, real Verse text; Guides/Notes/Search as static mocks): phone — reader with a
  compact top bar (Resource title → Parallel resources, reference → grid/typed entry), a toolbar
  (Library, Search, Notes, Plans, Settings) as a tab bar *or* a "more" sheet (build both toggles);
  Study sheet as a resizable bottom sheet with detents; Split view stacked. iPad — `NavigationSplitView`
  with the Study sheet as a side pane; Split view side by side; Stage Manager/Slide Over size
  classes. Include History/Back, Undo toast placement, download banner placement, and the
  Highlight palette row in the selection menu. Produce a flow diagram (Mermaid in the doc) and
  screenshots per surface for phone and iPad in Light/Dark.
- **Don't:** persist anything; implement real Guides/Search; add rules in Swift (titles, ordering,
  what a sheet shows come from `obs-mobile` even when mocked).
- **Deliverables:** prototype in `apps/ios` behind a `PROTOTYPE_ANATOMY` scheme flag,
  `docs/dev/mobile-anatomy.md` (draft: options with screenshots), `docs/previews/ios/s10-*.png`.
- **Done when:** both toggle variants run on iPhone and iPad simulators; every v1 surface is
  reachable in ≤ 2 taps from the reader; screenshots and the flow diagram are in the PR; PR body has
  a Shell audit.
- **Verify:** `xcodebuild build` for both destinations; XCUITest `anatomyFlows` (each surface
  reachable); screenshots.
- **Stop-and-ask:** a v1 surface cannot be reached without a third navigation level; iPad side
  pane and Split view together leave < 320 pt for text at the floor iPad width.

#### S11 — #335 owner review (HITL)

- **Type:** HITL. **Xcode:** y. **Size:** S. **Depends on:** S10.
- **Goal:** the anatomy decisions only a person clicking a phone can make.
- **Owner checklist:** run the prototype on iPhone (and iPad if available); choose: tab bar vs
  "more" sheet; sheet detents; where typed reference entry lives; Split view gestures (swipe to
  switch Resource vs tap title); Study sheet vs side pane widths on iPad; whether Settings is in the
  tab bar. Also decide **minimum iOS** (default 17), the **mobile-only extras** fog item (default:
  defer share extension, deep links and widgets to v1.1; keep only "Open in" from #333) and
  **i18n / string location** (D16 default below). Write the
  decisions as a PR comment for S12.
- **Done when:** the comment lists a choice for every item above.

#### S12 — ADR 0024 (anatomy), `docs/dev/mobile-anatomy.md`, #335 bookkeeping, string rule

- **Type:** AFK + HITL. **Xcode:** n. **Size:** S–M. **Depends on:** S11.
- **Goal:** the contract every M2–M5 UI step cites, plus the i18n/string-location rule before any
  UI string is written.
- **Read first:** S11's comment; `docs/dev/mobile-anatomy.md` draft; ADR 0021 shape; #324 fog list.
- **Do:** ADR 0024 "The Mobile app is one reader with a Study sheet, and a side pane on tablets"
  (or as decided): navigation model, reference entry, Split view behaviour, sheet/pane, min iOS,
  extras deferred. **String rule (D16):** domain and behaviour strings (Resource titles, Verse
  labels, action names, toast/banner/guidance text, errors, Library row text) come from Rust —
  English in v1, and any later i18n lives in Rust so Android shares it; only chrome-only labels
  (tab names, "Settings", "Done") go in `Localizable.xcstrings`. Hebrew/Greek *input* via system
  keyboards (#330), no transliteration keyboard. Finalise `docs/dev/mobile-anatomy.md` as the
  contract every M2–M5 step cites, with the string rule in it.
  CONTEXT.md: add terms only if new ones appeared (e.g. "Reader bar"). Remove the prototype scheme
  flag or leave it disabled. PR body: #335 resolution comment + #324 Decisions line + strike
  "Mobile-only extras" and "i18n and Hebrew text input" from Not yet specified.
- **Owner HITL:** post, close #335, edit #324.
- **Don't:** add anatomy the prototype did not test; put strings rules anywhere but the contract.
- **Deliverables:** ADR 0024, `docs/dev/mobile-anatomy.md`, PR body texts.
- **Done when:** ADR and contract merged; owner confirms GitHub edits.
- **Verify:** `cargo xtask ci` (docs only; keeps the tree honest).
- **Stop-and-ask:** S11's comment leaves an item undecided.

### M2 — Reader

#### S13 — `obs-mobile` core and the real FFI surface

- **Goal:** the Mobile app's state machine in Rust, tested on all CI OSes, with a stable FFI.
- **Type:** AFK. **Xcode:** n. **Size:** L. **Parallel:** with S10.
- **Depends on:** S03, S04, S09.
- **Read first:** A3, A7 (paths), ADR 0022 rule 4 (what stays per device); `crates/engine/src/session.rs`
  (`attach_user`, `save_last_session`, `restore_last_session`, `run`), `workspace.rs` (`Tab`,
  `History`, `TabKind`, `Effect`), `persist.rs` (`WorkspaceSnapshot`, `UserData`),
  `resource_navigation.rs`, `crates/scripture/src/reference.rs` (`parse_reference`),
  `crates/engine/src/favorites.rs` (History rows), `docs/dev/favorites-history.md`, `docs/dev/settings.md`;
  S03 Outcome.
- **Do:** replace the S03 spike with the designed surface. **Decision (recommended):** `obs-mobile`
  drives the engine's `Workspace` internally with one Panel and ≤ 2 reading `Tab`s (every command
  takes a `TabId`; `tab.go`, `tab.back`, `tab.parallel*`, `tab.link` already exist), but exposes
  only Mobile vocabulary (`Reader`, `Split view`, `Follow`). Last session: the Mobile snapshot is
  stored through `UserData::save_last_session` as its own JSON (per device; never synced). Boot per
  A7 with `Paths`; `isExcludedFromBackup` is Swift's job but Rust returns the folder list.
  `MobileApp` methods: `boot`, `library()`, `open(reader, target)`, `navigate(reader, Reference|Key)`,
  `parse_reference(text) -> Result`, `books(reader)`/`chapters(reader, book)`/`verses(...)` for the
  grid (from the Resource's own versification via `ResourceNavigation`), `back(reader)`,
  `history()`, `frame(reader, viewport, offset, scale)`, `tile(...)`, `hit(...)`, `footnote(reader, marker)`,
  `settings()`/`set_theme|page_tone|text_size|reading_font|reduce_motion`, `dynamic_type(category)`,
  `save_session()`, `on_resume()`/`on_background()`, `poll_changed()`. Events per A3: the pump is
  `MobileApp::pump() -> Vec<Changed>` (locks, `session.poll_imports()`, layout-worker drain,
  `Effect` → `Changed`); `obs-ffi`'s thread calls it on a timer/signal and never touches `JobQueue`
  events itself. **wasm gate:** `sync_starter_library`, `install_from_bundle`, `add_starter` are
  `#[cfg(not(target_arch = "wasm32"))]` (`starter.rs:757, 845, 866`), so `obs-mobile`'s boot and
  install paths are cfg-gated the same way `App::with_file` is (`crates/ui-egui/src/lib.rs:84–150`);
  the wasm build keeps the pure state machine only. Every method
  has a unit test; the `JobMode::Inline` test harness uses `obs_engine::testing` fixtures
  (CI-run) plus `#[ignore]`d tests over the S04 subset from `dist/starter` (run locally, command in
  Verify). Extend the design-token lint scan to `crates/mobile/src` (A5).
  S03 deferrals: Auto Page tone takes the shell's system appearance as an input to
  `tile`/`frame`; a tile pixel-size cap (S03 added 8192); Note-marker room in tiles; FFI calls
  that are infallible in the spike (`problems`, `resource_ids`, `reader_resource`) become
  fallible so a poisoned lock is visible.
  Update `docs/dev/mobile-core.md`.
- **Don't:** Swift; Study sheet/Annotations/Search (later steps add methods); leak `TabId`/`Panel`
  into the FFI; put Theme/Page tone colour math anywhere but `obs-reading`.
- **Deliverables:** `crates/mobile` modules (`app.rs`, `paths.rs`, `readers.rs`, `navigation.rs`,
  `session.rs`, `settings.rs`, `events.rs`), `apps/obs-ffi/src/lib.rs` Records/Enums/Object,
  `docs/dev/mobile-core.md`.
- **Done when:** `cargo xtask ci` green on the host; `cargo xtask wasm` still passes with
  `obs-mobile` in it; `cargo xtask ios bindgen` regenerates
  cleanly; the Swift smoke (S03) still builds against the new surface; fixture tests (run on all CI
  OSes) cover: first launch installs a bundle and opens the Bible's first chapter; relaunch restores
  the reader; `Jn 3:16` and
  `1 Co 13` parse; back/forward; Hebrew Resource starts RTL; Dynamic Type map bounds; the
  `#[ignore]`d test does the same against the real Bundled set.
- **Verify:** `cargo xtask ci`; `cargo xtask ios bindgen`; `cargo xtask ios smoke`;
  `cargo test -p obs-mobile -- --ignored` (after `cargo xtask ios bundled-set`).
- **Stop-and-ask:** reusing `Workspace` needs a change to desktop-visible behaviour; `Effect`
  variants exist that have no mobile meaning and cannot be ignored safely.

#### S14a — iOS reader: text view, navigation, History, Last session

- **Goal:** the real reader for any Resource: hosted text view, book→chapter→verse grid, key list for
  Dictionaries/Lexicons, typed references, Back/History, Last session restore.
- **Type:** AFK. **Xcode:** y. **Size:** L.
- **Depends on:** S12, S13.
- **Read first:** ADR 0023/0024, `docs/dev/mobile-anatomy.md`, `docs/dev/mobile-text-view.md`,
  #330 "Reader", "Passage navigation"; `docs/dev/reading-layout.md`;
  `crates/engine/src/resource_navigation.rs` (`entries`, `lookup`, `parse_entry`, `adjacent` for
  keyed Resources).
- **Do:** productionise S07's `ResourceTextView` (tile cache, prefetch, scroll anchoring from
  `ReadingView`), reader bar per ADR 0024, book→chapter→verse grid (data from Rust), headword/key
  list with type-ahead for Dictionaries and Lexicons (#330 "any Resource"; data and matching from
  `ResourceNavigation` through S13's `navigate(reader, Key)`), typed reference
  field with Rust validation and suggestions, Back + History list, Last session restore, save
  session on background. Previews on iPhone and iPad.
- **Don't:** footnotes, Settings, Study sheet, Split view, Annotations (next steps); any book-name
  table, versification, key ordering or reference grammar in Swift; a Swift colour literal for
  page/ink.
- **Deliverables:** `apps/ios/OpenBible/Reader/**`; `docs/dev/mobile-reader.md`;
  `docs/previews/ios/s14a-*.png`; XCUITests (navigate by grid, by key, by typing; back; relaunch
  restores).
- **Done when:** UI standard; all #330 Reader + Passage navigation bullets demonstrably work in the
  simulator for a Bible and a Lexicon (Study set debug override); targets from ADR 0023 met on the
  simulator.
- **Verify:** UI standard.
- **Stop-and-ask:** UI standard; any ADR 0023 target missed by > 20 %.

#### S14b — iOS reader: footnotes, Settings, Dynamic Type, accessibility

- **Goal:** the reader is complete and accessible: footnote popovers, Settings, Dynamic Type,
  Reduce Motion / Increase Contrast, VoiceOver.
- **Type:** AFK. **Xcode:** y. **Size:** M.
- **Depends on:** S14a.
- **Read first:** #330 "Settings"; `docs/dev/footnotes.md`, `docs/dev/settings.md`,
  `docs/dev/accessibility.md`; `crates/engine/src/appearance.rs` (Theme ids, `view.reduce_motion`);
  ADR 0018; ADR 0023 accessibility bar.
- **Do:** footnote marker tap → popover with the note's text (Rust supplies the laid note);
  Settings screen (Theme Light/Dark/System, Page tone, Text size slider with Dynamic Type default,
  Reading font among the bundled faces — see A6/Risks for system fonts);
  `UIAccessibility.isReduceMotionEnabled` → `view.reduce_motion`, `isDarkerSystemColorsEnabled` →
  High contrast theme pair (ADR 0018); VoiceOver elements per Verse with rotor, 44 pt targets.
  Previews for each surface in Light/Dark/Sepia on iPhone and iPad.
- **Don't:** Study sheet, Split view, Annotations; Theme/tone colour math in Swift.
- **Deliverables:** `apps/ios/OpenBible/Settings/**`, footnote popover; `docs/dev/mobile-reader.md`
  section; `docs/previews/ios/s14b-*.png`; XCUITests (footnote, theme, Dynamic Type at the largest
  accessibility size, VoiceOver order).
- **Done when:** UI standard; all #330 Settings bullets work; `performAccessibilityAudit()` passes on
  reader and Settings at the largest Dynamic Type size.
- **Verify:** UI standard.
- **Stop-and-ask:** UI standard; Reading font from system fonts needs file access iOS does not
  grant (then ship bundled faces only and note it).

#### S15 — Parallel resources and Split view with Follow

- **Goal:** two readers at most, Follow by Link set, Parallel resources by swipe/title.
- **Type:** AFK. **Xcode:** y. **Size:** M. **Depends on:** S14b.
- **Read first:** #330 "Parallel reading", CONTEXT.md **Split view**, `docs/dev/parallel-resources.md`,
  `crates/engine/src/workspace.rs` (Link sets, `tab.link`, `tab.parallel_next/previous`),
  `crates/engine/src/resource_navigation.rs:71–91` (`section`, `adjacent_section` — "nearest earlier
  section").
- **Do:** Rust: `set_split(on)`, `second_reader()`, `follow(on)`; Follow = both Tabs in one Link set
  (engine semantics, so a Commentary follows a Bible to the nearest earlier section as on desktop);
  navigating the second reader on its own leaves the set; `Follow` rejoins. Parallel resources: tap
  title → list of Resources of the same type at the Current location; swipe (per ADR 0024) →
  `tab.parallel_next/previous`. Swift: stacked readers on compact height/width, side by side on
  regular; a Follow chip. Kept in the Mobile session snapshot.
- **Don't:** more than two readers; Layouts; any "nearest section" logic in Swift.
- **Deliverables:** Rust methods + tests (Bible+Commentary follow; break and rejoin), Swift views,
  previews, `docs/dev/mobile-reader.md` section.
- **Done when:** UI standard; XCUITest: open BSB, split with a Commentary (Study set in debug),
  navigate to
  Psalm 23 → Commentary at its section; navigate the Commentary alone → chip shows "Follow";
  tap → rejoined.
- **Verify:** UI standard; `cargo test -p obs-mobile follow`.
- **Stop-and-ask:** UI standard; Link-set semantics need changing for mobile.

#### S16 — Study sheet

- **Goal:** the tap sheet (#330): Word Info, cross-references with text, Notes, Highlights, actions.
- **Type:** AFK. **Xcode:** y. **Size:** L. **Depends on:** S14b.
- **Read first:** CONTEXT.md **Study sheet**, #330 "Study sheet", #333 "Before the Study set is
  downloaded"; `crates/engine/src/word_info.rs` (`ShownWord`, `lexicon_gloss`, `lexicon_excerpt`),
  `cross_references.rs` (`group_references`, `bible_preview`, `TOP_REFERENCES`), `annotations.rs`
  (annotations at a Passage), `docs/dev/word-info.md` if present else `docs/dev/passage-guide.md`
  §Word Info; ADR 0024 (sheet vs pane).
- **Do:** Rust: `study_sheet(reader, hit) -> StudySheet { subject: Word|Verse, word_info?: {lemma, gloss, morphology, strongs, lexicon_excerpt}, cross_refs: [{passage, text}], notes: [...], highlights: [...], actions: [...], missing: Option<"Study set" offer> }`
  computed on the job queue where desktop does (Guides indexes), with the "never an empty error"
  rule from #333. Actions are an enum the shell renders as buttons; running one calls back into Rust
  (`run_action`). Swift: bottom sheet with detents (phone) / side pane (iPad) per ADR 0024; tap a
  cross-reference → navigate or open in the second reader (Rust decides which by ADR 0024).
- **Don't:** Highlight/Note editing (S17/S18) beyond the action buttons that open them; any
  ordering of cross-references or lexicon choice in Swift.
- **Deliverables:** Rust model + tests (BSB word with tagging via Study set; BSB without Study set
  → offer), Swift sheet/pane, previews, `docs/dev/mobile-study.md`.
- **Done when:** UI standard; tapping a word in BSB shows cross-references with text within 150 ms
  warm; with
  the Study set installed, Word Info shows Strong's + gloss + lexicon excerpt; XCUITest asserts the
  sheet's accessibility labels and order (subject, Word Info, cross-references, Notes, actions).
- **Verify:** UI standard; `cargo test -p obs-mobile study`.
- **Stop-and-ask:** UI standard; Word Info needs a Bible-specific tagging path not in `word_info.rs`.

### M3 — Annotations

#### S17 — Highlights with the active palette, Recently used, Undo toast

- **Goal:** Highlights from a selection with desktop-identical colours; Undo for Annotation changes.
- **Type:** AFK. **Xcode:** y. **Size:** M. **Depends on:** S16.
- **Read first:** #330 "Highlights", "Undo"; ADR 0012; `crates/engine/src/highlight_styles.rs`
  (`palette_styles`, active palette), `annotations.rs` (`annotation.highlight`, `edit.undo`,
  `UndoHistory`), `obs_reading::marks`, `obs_reading::palette` (`paint`); `docs/dev/annotations.md`.
- **Do:** Rust: `highlight_menu() -> {styles in active palette order with swatch colours for the
  current tone, recently_used}` ("Recently used" is a new per-device preference `mobile.recent-styles`,
  not synced per ADR 0022 rule 4), `highlight(selection, style_id)`, `delete_annotation(id)`,
  `undo()`/`redo()` with toast text from Rust. Marks drawn in tiles (backgrounds under glyphs,
  underlines/boxes over). Swift: palette row in the selection edit menu (`UIEditMenuInteraction`
  or the `UITextInteraction` menu; verify), Undo toast with a 5 s timer (duration is presentation).
- **Don't:** style/palette editing; colour math in Swift.
- **Deliverables:** Rust methods + tests, Swift palette row and toast, `docs/dev/mobile-annotations.md`
  (Contract / Data / Evidence), `docs/previews/ios/s17-*.png`.
- **Done when:** UI standard; highlight a Verse → persists across relaunch → shows in another Bible
  (Passage
  anchor) per `marks.rs` rules; delete → Undo restores; swatches match desktop's for the same tone
  (compare `TonePalette::paint` output in a Rust test shared with desktop).
- **Verify:** UI standard; `cargo test -p obs-mobile highlight`.
- **Stop-and-ask:** UI standard; the `UITextInteraction` menu cannot host a custom palette row.

#### S18 — Notes as CommonMark text, keyboard bar, live links; Notebooks

- **Goal:** Notes typed on the phone are the exact text desktop's editor would produce, because both
  call one Rust rule set.
- **Type:** AFK. **Xcode:** y. **Size:** L. **Depends on:** S16.
- **Read first:** ADR 0010 (note text is CommonMark), #330 "Notes", "Notebooks", "Undo";
  `crates/model/src/note_text.rs` (`note_spans`, `note_shown`, `note_outline`, `note_unformattable`),
  `crates/engine/src/notes.rs` (`note_body`), `annotations.rs` (`annotation.note`, `annotation.edit`,
  `notebook.*`, `edit.undo`), `crates/ui-egui/src/shell/note_editor.rs:294–841` (`list_continuation`,
  `apply_format`, `format_blocked`, `line_format_blocked` — `pub(super)`, egui-free, ~550 lines) and
  `note_editor_corpus_tests.rs`; `docs/dev/annotations.md`, `docs/dev/note-margin.md`.
- **Do:** **first, a lift** (its own commit, `refactor(model): …`, carries `ui-review` per ADR 0020
  rule 4 because it touches ui-egui): move the egui-free CommonMark edit rules (`Format`,
  `apply_format`, `format_blocked`, `list_continuation`, `line_format_blocked`) and the corpus tests
  from `note_editor.rs` into `obs-model` next to `note_text.rs` (e.g. `note_edit.rs`); `note_editor.rs`
  calls them. Then Rust: `note_editor(annotation?) -> {text, outline, link_ranges}`,
  `apply_format(text, selection, Bold|Italic|List|ReferenceLink{passage})` over the shared rules,
  `save_note(...)` (an edit is undoable: the S17 Undo toast appears after save and after delete),
  `notebooks()`, `create/rename_notebook`,
  `set_show_in_text`, reference detection for live links (`scripture_references_in`). Swift: a
  `UITextView` editor (plain text, monospaced off), an input accessory keyboard bar (B, I, list,
  reference link), tap on a reference in a displayed Note → navigate; Notebook picker when creating.
- **Don't:** a rich-text/WYSIWYG editor; any Markdown parsing or edit rule in Swift; a second copy
  of the formatting functions.
- **Deliverables:** `crates/model/src/note_edit.rs` (+ corpus tests moved), `note_editor.rs` slimmed,
  Rust mobile methods + tests, Swift editor + bar + picker, `docs/dev/mobile-annotations.md` section,
  `docs/previews/ios/s18-*.png`.
- **Done when:** UI standard; a Note typed with `**bold**` shows bold in the Study sheet and in
  desktop (same
  `user.db` copied over by hand once); keyboard bar edits produce exactly the text desktop's editor
  would (one corpus test, run by both callers); desktop gallery unchanged after the lift; Notebook
  create/rename/show-in-text work; Undo restores a Note edit and a deleted Note; VoiceOver labels on
  the bar.
- **Verify:** UI standard; `cargo test -p obs-model note_edit`; `cargo xtask gallery` (zero changes).
- **Stop-and-ask:** UI standard; a formatting function needs egui's cursor types to move (then
  design a plain `Range<usize>` seam and ask).

#### S19 — Notes Tool (lighter), Needs attention, Notes export, Copy/Share

- **Goal:** the Notes list, orphan handling, export and Copy/Share with desktop-identical output.
- **Type:** AFK. **Xcode:** y. **Size:** M. **Depends on:** S17, S18.
- **Read first:** #330 "Notes Tool", "Copy and Share", ADR 0006; `crates/engine/src/notes.rs`,
  `reanchor.rs` (Orphaned, keep without anchor, delete), `notes_export.rs` (`note_markdown`),
  `copy_verses.rs` (default Copy preset); #333 "Resources on one device but not another";
  `docs/dev/notes-tool.md`, `docs/dev/copy-export.md`.
- **Do:** Rust: `notes_tool(notebook?) -> cards`, `needs_attention() -> [{annotation, reason: Orphaned|SyncConflict(later)|MissingResource}]`,
  `keep_without_anchor`, `export_notes(notebook?) -> {files}` written to a temp dir; `copy_text(selection|verse) -> {plain, html}`.
  Swift: Notes Tool screen with Notebook filter, Needs attention section (keep/delete only — re-attach
  deferred), share sheet for export (`UIActivityViewController`) and for Copy/Share verses
  (`UIPasteboard` with plain + HTML).
- **Don't:** re-attach Orphaned Annotations (deferred, #330); Copy preset editing; Markdown or
  HTML generation in Swift.
- **Deliverables:** Rust methods + tests, Swift screens and share sheet wiring,
  `docs/dev/mobile-annotations.md` section, `docs/previews/ios/s19-*.png`.
- **Done when:** UI standard; export of a Notebook produces the same Markdown desktop produces
  (Rust test);
  "Get *Resource* to see it in place" shows for a Text-range Note whose Resource is absent.
- **Verify:** UI standard; `cargo test -p obs-mobile notes_tool`.
- **Stop-and-ask:** UI standard; `UIActivityViewController` needs a file type the engine does not
  export.

### M4 — Search, Guides, Reading plans

#### S20 — Search (Bible + Notes), scopes, full syntax

- **Goal:** Search with the full desktop syntax and desktop-identical hits.
- **Type:** AFK. **Xcode:** y. **Size:** M. **Depends on:** S14b.
- **Read first:** #330 "Search"; `docs/dev/search-tool.md` (syntax incl. `strong:`/`lemma:`, scopes),
  `docs/dev/search-index.md`; `crates/engine/src/bible_search.rs`, `search.rs` (`SearchRequest`,
  index status), `annotations.rs` (`annotation.search_notes`); `text.normalize` command.
- **Do:** Rust: `search(query, mode: Bible|Notes, scope) -> results|error|indexing(resource)`,
  suggestions, open-hit → navigate. Swift: search screen with mode/scope pickers, results list,
  "still being indexed" row; system keyboards for Hebrew/Greek (no custom keyboard; D16, decided in
  S12).
- **Don't:** query parsing, scope rules or result ranking in Swift; Search Library mode (deferred).
- **Deliverables:** Rust methods + tests, Swift Search screen, `docs/dev/mobile-search.md`,
  `docs/previews/ios/s20-*.png`.
- **Done when:** UI standard; `strong:G26 @noun`, `lemma:`, phrase and scope queries return
  desktop-identical
  hit counts (Rust test); Notes mode finds a Note; a hit opens in the reader.
- **Verify:** UI standard; `cargo test -p obs-mobile search`.
- **Stop-and-ask:** UI standard; a syntax feature needs an index the Bundled set does not ship.

#### S21 — Passage Guide and Word Study (lighter); synced arrangement followed

- **Goal:** the two Guides #330 keeps, read-only arrangement from the synced row.
- **Type:** AFK. **Xcode:** y. **Size:** L. **Depends on:** S16.
- **Read first:** #330 "Passage Guide", "Word Study", "Guide arrangement"; `crates/engine/src/guide.rs`
  (`GuideSection`, `GuideArrangement`, `LibrarySections`), `word_study.rs`, `key_words.rs`,
  `people_places.rs`; `docs/dev/passage-guide.md`, `docs/dev/word-study.md`.
- **Do:** Rust: `passage_guide(passage) -> sections [Commentaries, Cross-references, Key words, People & places]`
  in the synced arrangement's order/hidden state (read-only), per-device collapsed state;
  `word_study(strongs) -> {header, lexicon entries, translated as, occurrences}` (no Forms).
  Swift: Guide screens with collapsible sections; occurrences open in the reader.
- **Don't:** arrangement editing; Forms; Exegetical/Topic Guide, Factbook (deferred); section order
  or hidden-state logic in Swift.
- **Deliverables:** Rust methods + tests, Swift Guide screens, `docs/dev/mobile-guides.md`,
  `docs/previews/ios/s21-*.png`.
- **Done when:** UI standard; section order follows a desktop-saved arrangement in a copied
  `user.db`; hidden
  sections stay hidden; results match desktop counts for Psalm 23 / G26 (Rust tests).
- **Verify:** UI standard; `cargo test -p obs-mobile guide`.
- **Stop-and-ask:** UI standard; a Guide section needs a Resource type the Study set lacks.

#### S22 — Reading plans with an optional local reminder

- **Goal:** Reading plans and progress (synced) with a device-local daily reminder.
- **Type:** AFK. **Xcode:** y. **Size:** M. **Depends on:** S14b.
- **Read first:** #330 "Reading plans"; `crates/engine/src/reading_plans.rs` (`PlanDate`, today,
  Behind, mark), `docs/dev/reading-plans.md`; ADR 0022 (progress syncs; reminder is a device
  preference).
- **Do:** Rust: `plans()`, `today(plan)`, `mark_read`, `reminder() -> {enabled, time}` (per-device
  pref, off by default), `reminder_text(today)`. Swift: plans screen; `UNUserNotificationCenter`
  daily trigger scheduled from Rust's `{enabled, time, text}`; permission asked only when toggled on.
- **Don't:** any push/remote notification; plan templates in Swift.
- **Deliverables:** Rust methods + tests, Swift plans screen + scheduler wrapper,
  `docs/dev/mobile-plans.md`, `docs/previews/ios/s22-*.png`.
- **Done when:** UI standard; marking a Reading on iOS shows done on desktop via a copied `user.db`;
  the
  reminder schedules and cancels (unit test on the scheduler wrapper).
- **Verify:** UI standard; `cargo test -p obs-mobile plans`.
- **Stop-and-ask:** UI standard; the notification permission flow needs an entitlement.

### M5 — Library

#### S23 — Library Rust side: install-from-file seam, manifest merge, Study set, Open catalogue, storage

- **Goal:** every Library rule (manifests, sets, sha256, states, cellular) in Rust, driven by a Swift
  transport.
- **Type:** AFK. **Xcode:** n. **Size:** L. **Parallel:** with S14–S22. **Depends on:** S13.
- **Read first:** #333 (all), A7 (incl. the bundled/released manifest mismatch); `crates/engine/src/starter.rs`
  (`manifest`, `install`, `starter_rows`,
  `starter_wanted`, `add_starter`, `install_from_bundle`, `MANIFEST_FORMAT`, `check_starter_release`), `open_catalogue.rs`
  (`OpenSource`, `check_download`, `import`, licence filter), `get_bibles.rs` (catalogue cache),
  `crates/formats/src/sword/repo.rs:51` (`Fetch`), `apps/openbible/src/fetch.rs` (status/size
  semantics to mirror in the Swift transport); `docs/dev/starter-library.md`, `docs/dev/get-bibles.md`.
- **Do:** engine, first the manifest fix: `StarterLibrary` keeps the bundle manifest for
  install/revision only; rows and the wanted set come from the released manifest once fetched,
  merged with the bundle's entries before that (`manifest()` returns the merge; `add_starter` goes
  to the bundle for ids it holds and to the download path for the rest). Then
  `pub fn add_starter_from_files(&mut self, items, files: &[(id, PathBuf)])` wrapping
  `install` with `read = fs::read`; `OpenSource` install from a file path; "manifest of the release
  whose format matches" (#333) — verify how `MANIFEST_FORMAT` is applied today and add the
  selection if missing; `wanted_set(Study)`; storage accounting per Resource (`.obr` + index +
  alias bytes), "won't fit" check given free bytes passed in by the shell. `obs-mobile`: download
  state machine `{queued, downloading{done,total}, verifying, installing, indexing, done, failed{retry}}`
  per Resource, Wi-Fi-only rule and the "ask first over cellular with size" rule (inputs: `is_wifi`,
  `allow_cellular`), banner text, Remove/Add back, Removed stays removed. FFI: `Transport` callback
  trait (A7) adapter to `Fetch`; `install_downloaded(id, path, origin)`. Tests with `file://`
  bases (as `starter_tests.rs` does).
- **Don't:** Swift; any networking crate (check `cargo xtask layers`); prebuilt index downloads.
- **Deliverables:** engine + mobile APIs, tests, `docs/dev/mobile-library.md` (Contract / Data /
  Evidence).
- **Done when:** engine test: a 4-item bundle installed, then a Study set item added through the
  release path from a `file://` base, Library → Starter library listing 22 rows throughout; a Study
  set "download" from a `file://` base through the state machine installs
  and the Library lists 19 Resources; a sha256 mismatch deletes the file and offers Retry; cellular
  prompt rule tests pass. All of these use fixture bundles (CI-run); `#[ignore]`d variants use
  `dist/starter`.
- **Verify:** `cargo xtask ci`; `cargo test -p obs-engine starter`; `cargo test -p obs-mobile library`;
  `cargo test -p obs-engine starter -- --ignored` after `cargo xtask starter`.
- **Stop-and-ask:** the manifest merge changes a desktop-visible Starter library behaviour; a
  `MANIFEST_FORMAT` selection rule is missing upstream and #333's wording is ambiguous for it.

#### S24 — iOS downloads and Library screens

- **Goal:** background, resumable, sha256-checked downloads (#333) with Rust owning every rule.
- **Type:** AFK. **Xcode:** y. **Size:** L. **Depends on:** S23.
- **Read first:** #333 "Downloads", `docs/dev/mobile-library.md`; Apple: `URLSessionConfiguration.background`,
  `URLSessionDownloadTask`, `allowsCellularAccess`, `handleEventsForBackgroundURLSession`,
  `NWPathMonitor` (verify APIs).
- **Do:** Swift `Transport` impl (data tasks, off-main, status → `FetchError` mapping as
  `fetch.rs` does), background download session with resume data persisted, progress → Rust,
  completion → `install_downloaded`; first-launch Study set offer sheet (size, Wi-Fi state, "Not
  now"); Library → Starter library rows (size, licence, what it powers — strings from Rust), Get
  Resources (Open catalogue with the same licence filter from Rust), banner, Cancel removes
  partial files.
- **Don't:** SWORD repository browsing; any manifest parsing or sha256 in Swift.
- **Deliverables:** Swift `Transport`, download session, Library screens, `docs/dev/mobile-library.md`
  Evidence section, `docs/previews/ios/s24-*.png`.
- **Done when:** UI standard; in the simulator against a local `file://`/`http://localhost` mirror,
  the Study
  set downloads, survives app suspension (simulate), verifies and installs; a bad hash shows Retry;
  Wi-Fi-only blocks on a simulated cellular path (`NWPathMonitor` injected).
- **Verify:** UI standard; XCUITest `libraryDownload` against the local mirror.
- **Stop-and-ask:** UI standard; background `URLSession` resume data proves unusable for
  GitHub Releases URLs (redirects).

#### S25 — On-device index builds, `BGProcessingTask`, Storage, backup exclusion

- **Goal:** indexes finish in the background; Storage shows and removes; data folders stay out of
  backups.
- **Type:** AFK. **Xcode:** y. **Size:** M. **Depends on:** S24.
- **Read first:** #333 "Storage", `crates/engine/src/search.rs` (`refresh_search_indexes`, `is_building`),
  `aliases.rs`; Apple `BGTaskScheduler`, `isExcludedFromBackup` (verify).
- **Do:** Rust: `pending_background_work() -> bool`, `step_background(deadline)` pumping index/alias
  jobs; `storage() -> rows`. Swift: register a `BGProcessingTask` id in `project.yml`
  (`BGTaskSchedulerPermittedIdentifiers`), schedule when work is pending, `isExcludedFromBackup` on
  `library/`, `search/`, `aliases/` after boot, Storage screen with Remove, low-storage check before
  download (`volumeAvailableCapacityForImportantUsage`).
- **Don't:** storage maths or "won't fit" rules in Swift (Rust takes free bytes as input).
- **Deliverables:** Rust methods + tests, Swift task registration + Storage screen,
  `docs/dev/mobile-library.md` section, `docs/previews/ios/s25-*.png`.
- **Done when:** UI standard; after installing a Resource and backgrounding, the index completes
  under the
  task (simulate with `e -l objc -- (void)[[BGTaskScheduler sharedScheduler] _simulateLaunchForTaskWithIdentifier:@"…"]`
  in lldb; verify the incantation); Search says "still being indexed" until then;
  `isExcludedFromBackup` is set on the three folders (XCTest reads the resource value back).
- **Verify:** UI standard; `cargo test -p obs-mobile storage`.
- **Stop-and-ask:** UI standard; the task is never granted in the simulator (then document the
  device-only check for S31/S35).

#### S26 — Import: document picker, Open In, SWORD zip, Import report

- **Goal:** Import from Files and "Open in" through the existing importers; the zip variants are an
  engine change only.
- **Type:** AFK. **Xcode:** y. **Size:** M. **Depends on:** S23.
- **Read first:** #333 "Import"; `crates/engine/src/import.rs` (`ImportSource`, `start_import`,
  `ImportRecord`, `failure_message`), `crates/formats/src/usfm_zip.rs`, `sword/mod.rs` (`SwordSource::open`),
  `docs/dev/local-copyrighted-modules.md`; Apple `UIDocumentPickerViewController`, `UTType`
  declarations, `CFBundleDocumentTypes`, `LSSupportsOpeningDocumentsInPlace = false`.
- **Do:** engine: `ImportSource::SwordZip(path)` (unpack with `zip` into a temp dir under the
  library's `imports/`, then `Sword { path }`), `ImportSource::UsfmZip(path)` via `usfm_zip_books`;
  `.obr` copy-in as today's `add_file`. Swift: Import button → picker (`.item` types limited to our
  UTTypes), "Open in Open Bible" registration, copy to Application Support, call Rust, Import
  report screen.
- **Don't:** change the desktop Import picker or its UI (engine-only on desktop; a separate ticket if
  wanted); format sniffing in Swift beyond the UTType.
- **Deliverables:** engine `ImportSource` variants + tests, Swift picker + Open In + report,
  `docs/dev/mobile-library.md` Import section, `docs/previews/ios/s26-*.png`.
- **Done when:** UI standard; a USFM zip, a SWORD module zip and an `.obr` import in the simulator
  (drag files
  into the simulator's Files app) and show reports; the engine imports the same zips in a Rust test
  (`cargo test -p obs-engine import`); desktop gallery unchanged.
- **Verify:** UI standard; `cargo test -p obs-engine import`; `cargo xtask gallery` (zero changes).
- **Stop-and-ask:** UI standard; a zip layout the importers cannot place (nested folders, mixed
  modules).

### M6 — Sync (ADR 0022) — recommend its own ticket set

#### S27a — Sync 1a (Rust): User database migration 13 — clocks, tombstones, device, ids

- **Goal:** the schema ADR 0022 needs, with no behaviour change yet.
- **Type:** AFK. **Xcode:** n. **Size:** L. **Parallel:** with M0–M5 (needs nothing from them).
  **Depends on:** —.
- **Read first:** ADR 0022 (all, esp. Consequences), #332 "What changes on desktop", ADR 0006,
  ADR 0013; `crates/store/src/user.rs` (`USER_SCHEMA_VERSION = 12`, `MIGRATIONS`),
  `annotation.rs:216` (`Change`, `Row`, `RowKey`), `clock.rs`, `persist.rs`
  (`UserData` — layouts by name, favorites, plans, preferences); `docs/dev/backups.md`.
- **Do:** migration 12 → 13: `clock` column on every synced table, `tombstone` table, `device`
  table (UUIDv7 id, name), `applied_up_to` table; Layouts get ids; the `preference` table splits
  into synced vs device keys by an explicit list. Backups (ADR 0013) keep working. Tests: migration
  from a v12 fixture with a pre-migration copy taken (`OpenReport.migration_backup`); every synced
  table has a clock; a Layout keeps its name and gains an id.
- **Don't:** change any write path yet (S27b); write sync files; UI; change ADR 0006 semantics.
- **Deliverables:** `obs-store` migration + tests, `docs/dev/backups.md` note on the new tables.
- **Done when:** `cargo xtask ci` green; the v12 fixture migrates; `cargo xtask gallery` unchanged.
- **Verify:** `cargo xtask ci`; `cargo test -p obs-store migration`; `cargo xtask gallery`.
- **Stop-and-ask:** a table cannot get a stable per-row id without data loss.
- **Outcome (2026-11-20, branch `mobile/s27a-sync-schema`):** `USER_SCHEMA_VERSION` 13;
  `crates/store/src/sync_schema.rs` holds migration 12 → 13 and the lists. Shipped: a nullable
  `clock TEXT` (encoding left to S28a; `NULL` = before Sync, older than any clock) on every table
  in `SYNCED_TABLES` (`notebooks`, `annotations`, `highlight_palettes`, `highlight_styles`,
  `reading_plan`, `plan_reading`, `favorite`, `layout`, `preferred_order`, `preference`);
  `anchors` rides on its Annotation's clock; `last_session`, `history`, `resource_names` and
  `notes_fts` stay local. `tombstone (row_table, row_id, clock)`, one-row
  `device (id = 1, device_id UUIDv7 made by the migration, name '')`,
  `applied_up_to (device_id, seq)`. `layout` rebuilt with `id TEXT PRIMARY KEY` (UUIDv7) and
  `name UNIQUE COLLATE NOCASE`, so every name-keyed call is unchanged; `save_layout` supplies an
  id on insert and keeps it on conflict. **Owner decisions (via supervisor):** the preference
  split is one `preference` table with the clock column and explicit lists, no rows moved
  (`SYNCED_PREFERENCES`: `highlights.palette`, `guide-arrangement/` prefix, `copy.presets`,
  `search.saved`, `library.collections`, `library.tags`, `search.word_forms`;
  `DEVICE_PREFERENCES`: every other key the engine writes, incl. `copy.options`,
  `annotations.last_notebook` and the Visual filters; an unlisted key stays on the device), and
  `favorite.uid` / `reading_plan.uid` (UUIDv7, unique, backfilled; new rows get one) because
  their integer ids collide across devices. The integer ids are untouched. No other write path
  moved; no sync files; no UI. A test fails when an engine `*_PREF`/`*_KEY` constant is in
  neither list. Four older store tests wrote Layouts/Favorites through today's API on a v2–v7
  database; they now insert the old rows with `execute_unchecked`. Measured: `cargo xtask ci`
  green (12 m 42 s), `cargo xtask gallery` unchanged. **For later steps:** a restore copies the
  Backup's `device` row, so the restored device has the Backup's id until S28c applies rule 7. S27b's
  entry gained an "Added by S27a" bullet (uids, per-item merge requirement, Visual filters).

#### S27b — Sync 1b (Rust): `Row` for every synced table; every synced write via `apply_changes`

- **Goal:** one write path for synced data, so S28a has a single stream to record.
- **Type:** AFK. **Xcode:** n. **Size:** L. **Depends on:** S27a.
- **Read first:** ADR 0022 rules 3–4 and Consequences; `annotation_store.rs` (`apply_changes`),
  `favorites_store.rs`, `reading_plans_store.rs`, `highlight_style_store.rs`; `crates/engine/src/`
  callers of each store (`rg "user\.\w+\(" crates/engine/src`).
- **Do:** `Row` (today Notebook, Annotation, HighlightStyle, HighlightPalette, `annotation.rs:202–209`)
  gains variants for Favorites, Collections, Resource tags, Copy presets, Saved searches, Guide
  arrangements, Preferred order, Layouts, **Reading plans and Reading done-marks** (ADR 0022 rule 3
  merges done-marks per Reading, rule 4 lists plans and progress), synced preferences; hard deletes
  become tombstones; every write to synced data goes through `apply_changes` (today only the
  annotation tables do). Tests: every synced write emits a `Change` (a table-driven test over all
  variants); `notes_fts` rebuilt after `apply_changes`.
- **Don't:** sync files; UI; desktop-visible behaviour changes.
- **Deliverables:** store + engine changes, tests.
- **Done when:** `cargo xtask ci` green; the "every write emits a `Change`" test covers every `Row`
  variant; `cargo xtask gallery` unchanged.
- **Verify:** `cargo xtask ci`; `cargo test -p obs-store`; `cargo test -p obs-engine`;
  `cargo xtask gallery`.
- **Stop-and-ask:** a write cannot be expressed as a `Row` before/after pair.
- **Added by S27a (owner decisions):** favorites and Reading plans already carry a UUIDv7
  `uid` (S27a), the id other devices know them by. **Requirement:**
  `library.collections`, `library.tags`, `copy.presets` and `search.saved` are each one JSON
  list per key, and `guide-arrangement/<guide>` values hold a list of sections; whole-value
  last-writer-wins would drop one device's concurrent addition (ADR 0006), so S27b must split
  each into per-item rows or give it a per-item merge. **Open question, decide here:** Visual
  filters (`reading.visual_filters`, `_next_id`) are user-made but stay device-only for now,
  because their numeric per-device ids would collide (Layouts switch filters by id); they need
  ids that are the same on every device before they can sync. **Also from the S27a review:**
  Preferred order is one value (ADR 0022 rule 3), so make it one `Row` holding the whole list and
  one clock. `save_preferred_order` replaces its rows, so the per-row `clock` column holds that
  one clock, and replacing the order writes no tombstones.

#### S28a — Sync 2a (Rust): `SyncFolder`, HLC, op files, device files

- **Goal:** the file layer of ADR 0022, rule-complete and tested against an in-memory folder, with
  no merging yet.
- **Type:** AFK. **Xcode:** n. **Size:** M. **Depends on:** S27b.
- **Read first:** ADR 0022 rules 1–2, 5–6; #326 resolution; `docs/research/mobile-folder-sync.md`
  §2 "Platform mechanics" and §4 "Candidate designs"; A8.
- **Added by the S27a review:** the HLC needs this device's `device` row, but a v13 database
  or Backup can lack it: `make_device` runs only in migration 12→13, and `check_whole` checks
  tables, not rows. Add `ensure_device()` at open, the way `ensure_default_notebook` repairs the
  default Notebook.
- **Do:** `obs_engine::sync`: `SyncFolder` trait (A8) + `FsSyncFolder` + `MemSyncFolder` (tests);
  HLC (`wall_ms`, `counter`, `device_id`); `sync.json` with a format version;
  `devices/<id>/device.json`; `ops/<seq>.jsonl` writer (temp + rename; trailing length-and-hash
  line) and tolerant reader (skip bad/partial, retry next time); recording local `Change`s as ops.
- **Don't:** merge; snapshots; UI.
- **Deliverables:** `crates/engine/src/sync/{mod,folder,clock,ops}.rs`, tests, `docs/dev/sync.md`
  draft (file format tables).
- **Done when:** `cargo xtask ci` green; a truncated op file is skipped and read whole next time;
  ops round-trip byte-identically through `FsSyncFolder`.
- **Verify:** `cargo xtask ci`; `cargo test -p obs-engine sync`.
- **Stop-and-ask:** the trailing hash line conflicts with a provider's partial-write behaviour the
  research documents.

#### S28b — Sync 2b (Rust): merge rules and property tests

- **Goal:** ADR 0022's merge rules, proven by property tests.
- **Type:** AFK. **Xcode:** n. **Size:** L. **Depends on:** S28a.
- **Read first:** ADR 0022 rule 3 (every sub-rule), ADR 0006; S28a's module.
- **Do:** merge: LWW per row, Note-text Sync conflict → second Note + Needs attention, edit beats
  delete (incl. Notebook resurrection), field-wise "absent field untouched", Reading done-marks per
  Reading, Preferred order LWW, Layout name clash "(2)", two default Notebooks. Property tests: N
  `MemSyncFolder` devices, random edit/delete interleavings, invariants "no user-made row
  disappears" (ADR 0006) and convergence.
- **Don't:** schedule or trim; UI.
- **Deliverables:** `sync/merge.rs`, property tests (no property-test crate is in the workspace
  today; adding `proptest` as a dev-dependency is a stop-and-ask — a seeded hand-rolled generator
  over `rand` is the default), `docs/dev/sync.md` merge table.
- **Done when:** property tests pass 10k cases; one unit test per ADR 0022 rule-3 bullet.
- **Verify:** `cargo xtask ci`; `cargo test -p obs-engine sync -- --include-ignored` (long cases
  may be `#[ignore]`d with a smaller CI case count; record both).
- **Stop-and-ask:** a merge rule in ADR 0022 is ambiguous for a real case (write the case down).

#### S28c — Sync 2c (Rust): snapshots, trimming, versioning, device-id rules, restore, scheduling

- **Goal:** the lifecycle rules: snapshots, 90-day trimming, format versioning, copied database,
  restore choice, when sync runs.
- **Type:** AFK. **Xcode:** n. **Size:** M. **Depends on:** S28b.
- **Read first:** ADR 0022 rules 5–8 and Consequences ("catch up or replace"); ADR 0013;
  `crates/engine/src/backups.rs` (`restore_preview`, `RestorePreview`, the restore path).
- **Added by S27a (ADR 0022 rule 7, first clause):** `restore_from` copies a v13 Backup's
  `device` row as is, so a restored device keeps the Backup's `device_id`. Restoring must give
  the device a new id. Decide what happens to the restored `applied_up_to` rows (keep them as
  the restored state's view of the other devices, or clear them so everything replays), and
  test restoring the device's own older Backup.
- **Do:** snapshots in Backup format with covered clocks; 90-day trimming of own ops; newer-format
  files left alone + "update the app" state; copied-database detection → new device id; **after a
  Backup restore on a syncing device:** `restore_choice(CatchUp | Replace)` — catch up (default)
  pulls the folder's ops; replace writes a snapshot with new clocks; the engine exposes
  `needs_restore_choice()` for the shells; `next_sync_due()` per rule 8 (launch, resume, 5 min,
  30 s after an edit); `sync_now()`.
- **Don't:** UI (S29/S30 render the states).
- **Deliverables:** `sync/{snapshot,schedule}.rs`, tests, `docs/dev/sync.md` final (Contract /
  Data / Evidence).
- **Done when:** two `FsSyncFolder` devices on one disk converge end to end; a restored database
  with `Replace` makes a third device converge to the restored state; trimming never removes ops a
  known device has not applied.
- **Verify:** `cargo xtask ci`; `cargo test -p obs-engine sync`.
- **Stop-and-ask:** a "replace" case where ADR 0006 and rule 7 disagree on what survives.

#### S29 — Sync 3 (desktop): Settings → Sync, Needs attention conflicts, restore prompt

- **Goal:** desktop can join a Sync folder and show its state.
- **Type:** AFK. **Xcode:** n. **Size:** M. **Depends on:** S28c.
- **Read first:** #332 "When it runs, and the UX"; `docs/dev/settings.md`, `docs/dev/notes-tool.md`,
  `docs/dev/backups.md`; `crates/ui-egui/src/shell/settings_tab.rs`, `notes_tab.rs`, the Backup
  restore flow; ADR 0020.
- **Do:** Settings → Sync (choose folder via `rfd`, device name, Sync now, Last synced, Synced
  devices with last seen, Stop syncing, onboarding copy per platform), status indicator, Sync
  conflicts in Needs attention; the shell calls `sync_now` when `next_sync_due` says so; after a
  Backup restore on a syncing device, a prompt "Catch up / Replace" (ADR 0022 rule 7) calling
  `restore_choice`. Gallery goldens for the new surfaces; `ui-review`.
- **Don't:** any rule in the shell (texts and states come from `obs_engine::sync`).
- **Deliverables:** egui surfaces, goldens, `docs/dev/settings.md` and `docs/dev/sync.md` sections.
- **Done when:** two desktop instances with `OBS_USER_DB` set to different files sync through a
  temp folder (an `update-e2e`-style scripted test); restore prompt appears and both choices work in
  that test; gallery blessed only for new surfaces.
- **Verify:** `cargo xtask ci`; `cargo xtask gallery`; the scripted two-instance test.
- **Stop-and-ask:** the restore prompt needs a change to the Backup restore flow's contract.

#### S30 — Sync 4 (iOS): folder picker, bookmarks, coordination, scheduling, Settings → Sync

- **Goal:** the iPhone joins a Sync folder through the document picker; every rule stays in Rust.
- **Type:** AFK. **Xcode:** y. **Size:** L. **Depends on:** S28c, S19.
- **Read first:** A8; `docs/research/mobile-folder-sync.md` iOS bullets; Apple: "Providing access
  to directories", `startAccessingSecurityScopedResource`, `NSFileCoordinator`,
  `startDownloadingUbiquitousItem(at:)`, `URLResourceKey.ubiquitousItemDownloadingStatusKey`
  (verify); `BGAppRefreshTask`.
- **Do:** Swift `SyncFolder` impl (bookmark stored in `user.db` via a device preference as
  base64 — Rust stores, Swift resolves; stale bookmark → "choose the folder again" state from
  Rust); coordinated reads/writes; placeholder download before read; `sync_now` on launch, resume,
  Rust-timed after edits, on demand; optional `BGAppRefreshTask`. Settings → Sync per #332 with the
  iOS folder guidance text from Rust; Needs attention shows Sync conflicts (S19 screen); the
  restore-choice prompt (S28c) if Backup restore ever reaches iOS (deferred in #330, so a state
  check only).
- **Don't:** iCloud entitlement/ubiquity container (not needed for a picked iCloud Drive folder;
  stop-and-ask if it turns out to be); any file-name or merge logic in Swift.
- **Deliverables:** Swift `SyncFolder`, Settings → Sync screen, Needs attention wiring,
  `docs/dev/sync.md` iOS section, `docs/previews/ios/s30-*.png`.
- **Done when:** UI standard; in the simulator, two app instances (two simulators sharing a host
  folder via
  the picker — or one simulator + desktop) converge; a Note edited on both becomes a Sync
  conflict listed in Needs attention; a stale bookmark shows the re-pick state.
- **Verify:** UI standard; XCUITest `syncTwoDevices` (host folder injected via launch argument).
- **Stop-and-ask:** UI standard; the ubiquity container turns out to be required; `NSFileCoordinator`
  blocks the main thread for more than a frame on a cold placeholder.

#### S31 — Sync 5: iPhone ↔ Mac over iCloud Drive (HITL)

- **Goal:** the only test that proves the provider behaviour the research could not: a real iCloud
  Drive round trip.
- **Type:** HITL. **Xcode:** y. **Size:** S–M. **Depends on:** S29, S30.
- **Owner checklist:** pick `Open Bible Sync/` in iCloud Drive on the Mac desktop app and on the
  iPhone; make Highlights/Notes on each; wait; confirm convergence and a deliberate conflict; note
  latency; then (per #332 "Validation") check whether Dropbox, OneDrive and Google Drive folders
  can be picked in the iOS picker at all. Report back; the executor updates the supported-folders
  copy (Rust strings) and `docs/dev/sync.md`.
- **Done when:** the report names latency, the conflict outcome and the pickable providers; the
  follow-up PR with the Rust strings is merged.

### M7 — Release

#### S32 — CI: iOS job on a macOS runner

- **Goal:** iOS builds and tests run on every relevant PR without touching `cargo xtask ci`.
- **Type:** AFK. **Xcode:** y (runner). **Size:** M. **Depends on:** S06.
- **Read first:** `.github/workflows/ci.yml`, `starter.yml` (artefact pattern for the Starter
  library), A9; GitHub `macos-latest` image contents (Xcode versions; verify at run time with
  `ls /Applications | grep Xcode`); S05 Outcome (the Xcode version in use).
- **Do:** `.github/workflows/ios.yml` on push/PR when `crates/**`, `apps/**`, `Cargo.lock`,
  `Cargo.toml` or the workflow change (engine/store/render/layout changes break the FFI too):
  toolchain + iOS targets,
  `Swatinem/rust-cache`, `brew install xcodegen`, download the `starter-library` artefact or run
  `cargo xtask ios bundled-set` from a cached `dist/starter` (pick the cheaper; record), `cargo xtask ios build --sim-only`,
  `xcodegen generate`, `xcodebuild test` on a named simulator, upload screenshots and `xcresult` on
  failure, `DEVELOPER_DIR` pinned to the Xcode version S05 recorded (bump it together with S05's
  Outcome). Keep `ci.yml` untouched.
- **Lock check:** the iOS job (or `starter.yml`) runs `cargo xtask ios bundled-set --out
  target/bundled-set` after the Starter build, so a stale `apps/ios/bundled-set.lock` fails CI;
  on Linux this also proves the lock is host-independent.
- **Don't:** device builds, signing, uploads; any change to `ci.yml` or `cargo xtask ci`.
- **Deliverables:** `.github/workflows/ios.yml`, `docs/dev/ios-project.md` CI section.
- **Done when:** the job passes on the PR; a deliberate Swift test failure fails it; a deliberate
  `crates/engine` change triggers it; total time recorded in the Outcome.
- **Verify:** `gh run watch` on the PR; `gh run view --log` for timings.
- **Stop-and-ask:** the job exceeds 30 minutes (then cache DerivedData or split); the runner image
  lacks the pinned Xcode.

#### S33 — Before anything permanent: identifiers, accounts, trademark, privacy URL, cadence (HITL)

- **Goal:** every permanent choice is made before the first upload ties the bundle id.
- **Type:** HITL (with an AFK follow-up PR). **Xcode:** n. **Size:** M (owner weeks of lead time).
- **Depends on:** S12. Must finish before S35; start the interviews early (S05 note), but enrol
  (item 3) only after the individual/organisation decision.
- **Owner checklist:** (1) **#337** — get a trademark clearance opinion (US + EU) on "Open Bible
  Software" and a fallback shortlist; (2) **#334** — fix the bundle id (default
  `io.github.inhesrom.openbible`; if a domain is bought first, use it *before* the first upload),
  store name/subtitle, seller name; decide whether desktop ids follow; (3) **#336** — enrol in the
  Apple Developer Program (individual vs organisation — organisation needs a D-U-N-S number;
  seller name shows in the store), note Android key custody is deferred with Android; (4) **privacy
  policy + website** — default: GitHub Pages `https://inhesrom.github.io/openbiblesoftware/privacy`
  stating "Data Not Collected", no telemetry, crash reports stay on device (ADR 0016);
  (5) **release cadence** — default: mobile versions independent of desktop, tags `ios-vX.Y.Z`,
  TestFlight per merged milestone, store release when M7 is done; CI builds/tests on PRs, archives
  only on tags with ASC API key secrets. Post resolutions on #334/#336/#337, close them, add
  Decisions lines to #324, strike "Release cadence…" and "Project website…" from the fog.
- **AFK follow-up (same step):** apply the final bundle id (replacing the `.dev` id, A6) and names
  to `project.yml`, `about`/Credits strings,
  CONTEXT.md if the name changes; add the privacy page source under `docs/site/` (or wherever
  the owner chose) — a separate small PR.
- **Done when:** the four decisions are recorded in this plan's Outcome and the GitHub tickets; the
  follow-up PR is merged and `project.yml` carries the final id.

#### S34 — App Store readiness

- **Goal:** a validated, exportable archive and everything App Review asks for — without uploading.
- **Type:** AFK. **Xcode:** y. **Size:** L. **Depends on:** S19–S26, S30, S32, S33.
- **Read first:** `docs/research/mobile-store-policy-and-builds.md` (App Review rows, required-reason
  APIs, 200 MB), #327 resolution, ADR 0016, ADR 0019/0020; `xtask/src/licenses.rs`, `about.toml`,
  `docs/dev/credits.md`; Apple: App Store Connect accessibility labels, `xcodebuild archive`/
  `-exportArchive`, `xcrun altool --validate-app` vs ASC API (verify).
- **Do:** `PrivacyInfo.xcprivacy` final (tracking: none; collected data: none; required-reason
  APIs with reasons); app icon + launch screen (ADR 0012 line-icon set rasterised; `ui-review`);
  Credits screen fed by the `obs-ffi` cargo-about listing from S02, now resolved for
  `aarch64-apple-ios` (`cargo xtask licenses --ios`; check whether the graph differs from the host
  one and record) plus font/data attributions
  from `ATTRIBUTION.toml`; OpenBible.info credit wording per #328 ("data from OpenBible.info, CC BY
  4.0"); accessibility audit pass on every screen (XCUITest `performAccessibilityAudit`); size:
  `.ipa` ≤ 200 MB (expect ≈ 43 MB Bundled set, measured in S04, + Rust ≈ 20–40 MB; record); first-launch offline path
  (4.2.3(ii)); review notes (what the app does without download, how Import works, no accounts);
  `xcodebuild archive` + `-exportArchive` (App Store method) with the Developer Program certificate
  (owner-provided via Xcode Accounts; agent runs the command) + local validation of the `.ipa`
  (`altool --validate-app` or the ASC API equivalent; verify) — **no upload**: the first upload
  binds the bundle id to the App Store Connect record and is the owner's action in S35.
  `docs/dev/mobile-release.md` (incl. the later `ios-release.yml` design: archives on `ios-v*` tags,
  uploads via ASC API key only after S35's first manual upload).
- **Don't:** upload to App Store Connect or TestFlight; create an ASC record; add ASC secrets.
- **Deliverables:** final `PrivacyInfo.xcprivacy`, icon/launch assets, Credits screen, review
  notes, `docs/dev/mobile-release.md`, `docs/previews/ios/s34-*.png`.
- **Done when:** UI standard; archive and export succeed and the `.ipa` validates locally; size
  recorded; audits pass; review notes drafted; Credits list
  every third-party crate in the iOS binary.
- **Verify:** UI standard; `xcodebuild archive … && xcodebuild -exportArchive …`; the validate
  command; `du -sh` on the `.ipa`; `cargo xtask licenses --ios`.
- **Stop-and-ask:** UI standard; size > 200 MB; a required-reason API we cannot justify;
  cargo-about rejects a licence in the iOS graph (`about.toml` `accepted`); anything in the
  export asks for an upload or a sign-in.

#### S35 — App Store Connect record, first upload, TestFlight, submission (HITL)

- **Goal:** the permanent actions, done by the owner in order.
- **Type:** HITL. **Xcode:** y. **Size:** S–M. **Depends on:** S34.
- **Owner checklist:** (1) create the App Store Connect record with the S33 bundle id and name;
  (2) fill privacy ("Data Not Collected"), accessibility labels, age rating, category; (3) **first
  upload** of the S34 `.ipa` from Xcode Organizer or Transporter (or, if preferred, generate an ASC
  API key, store it as a repo secret and let the agent run the upload command once the record
  exists); (4) internal TestFlight on the owner's devices for a week; external
  TestFlight optional; (5) submit for review with the S34 notes; on rejection, open a ticket quoting
  the reviewer and loop to the responsible step. After (3), an AFK follow-up adds `ios-release.yml`
  per S34's design.
- **Done when:** a TestFlight build is installable; the review is submitted; the Outcome records
  the build number and the ASC record id.

## Owner decisions needed

| # | Decision | Recommended default | Blocks |
|---|---|---|---|
| D1 | Android half of #331 | Defer to the Android release; record in ADR 0023; close #331 on the iOS verdict | S09 |
| D2 | Hosting option | B (CPU tiles) first; A in Swift/Metal only if B misses the budget; never C | S07/S09 |
| D3 | Performance targets + floor hardware | A12 / iOS 17 devices; ≤ 1 % dropped frames; cold start ≤ 1.5 s; chapter ≤ 100 ms warm; RSS ≤ 150 MB | S09 |
| D4 | Accessibility bar | VoiceOver everywhere, Dynamic Type through accessibility sizes, 44 pt targets, Reduce Motion/Increase Contrast; claim labels at 1.0 | S09, S14b |
| D5 | Minimum iOS | 17 | S12, S06 (placeholder 17 until then) |
| D6 | Mobile-only extras | Defer share extension, deep links, widgets to v1.1; "Open in" stays (it is Import) | S12 |
| D7 | Anatomy choices (tab bar vs more-sheet, detents, reference entry, Split gestures) | Decided by clicking in S11 | S12 |
| D8 | Crate names and placement | `obs-reading`, `obs-mobile` (L5), `apps/obs-ffi` | S01, S02 |
| D9 | Types across the FFI | Mirror types in `obs-ffi` | S02, S13 |
| D10 | Xcode project generation | XcodeGen `project.yml` committed; `.xcodeproj` gitignored | S06 |
| D11 | Bundled set install | Copy into the library folder at first launch (ADR 0007) | S13 |
| D12 | Sync folder I/O | Rust `SyncFolder` trait, Swift implementation on iOS | S28a, S30 |
| D13 | Sync as its own ticket set / child map | Yes; S27a–S31 are its spine | S27a |
| D14 | iOS screenshot evidence | Owner-reviewed PNGs under `docs/previews/ios/`; no pixel goldens in v1 | S06+ |
| D15 | Reading font on iOS | Bundled faces + system fonts only if font files are readable; else bundled only | S14b |
| D16 | i18n / string location / Hebrew input | Domain and behaviour strings from Rust (English in v1; later i18n lives in Rust); chrome-only labels in `.xcstrings`; system keyboards for Hebrew/Greek | S12 (before S14a) |
| D17 | Identifiers (#334), accounts (#336), trademark (#337) | `io.github.inhesrom.openbible` unless a domain is bought first (dev builds use `…openbible.dev` until S33); individual Developer Program, enrolled only after the #336 decision; lawyer's opinion before upload | S33 → S35 |
| D18 | Privacy policy URL + website | GitHub Pages page in this repo | S33 |
| D19 | Release cadence + CI/signing shape | Independent `ios-v*` tags; TestFlight per milestone; first upload manual (S35), then archives/uploads on tags with ASC API key | S33, S34, S35 |
| D20 | Release profile | `[profile.ios]`: fat LTO, `opt-level = "s"`, keep `panic = "unwind"` | S02 |
| D21 | UniFFI licence (MPL-2.0, weak copyleft; `about.toml` accepts only permissive licences today) | **Decided 2026-11-20: accept MPL-2.0** for the iOS binary (Credits text + source link), added to `accepted` with a comment. Any other licence goes back to the owner | S02 |

## Risks, early warnings, fallbacks

| Risk | Early warning | Fallback |
|---|---|---|
| CPU tiles (B) too slow for 120 Hz fling | S03 host composite > 8 ms/tile; S07 simulator drops > 1 % | A in Swift/Metal (S07 switch); then S09-F |
| `UITextInteraction` on a custom `UITextInput` view misbehaves (handles, loupe, menu) | S07 handles do not appear or jump | Custom handle views + `UIEditMenuInteraction`; record in ADR 0023 |
| `unsafe_code = "forbid"` conflicts with UniFFI macros | S02 build error under `forbid` | Crate-local `[lints]` in `apps/obs-ffi` only (ADR 0021 Consequences) |
| UniFFI vs edition 2024 / rust 1.95 | S02 compile errors | Pin a UniFFI version that supports it; raise `rust-version` only with owner approval |
| `rusqlite` bundled SQLite cross-compile for iOS | S06 `cc` errors | Set `IPHONEOS_DEPLOYMENT_TARGET`, `SDKROOT` via `xcrun`; as a last resort the `rusqlite` `bundled` features for iOS documented upstream |
| `Session` is `!Send` beyond `UserData` | S02 `assert_send` fails | Engine actor thread + channel (A3) |
| Desktop regression from the S01 lift | Gallery goldens change; bench regresses | Revert to re-exports with zero moves of logic; bisect |
| Reading font: iOS system font files unreadable | S14b `kCTFontURLAttribute` denied | Bundled faces only (D15) |
| Study set URL = GitHub Releases latest | Format mismatch after a desktop release | #333's "manifest of the matching format" (S23) |
| Bundled vs released Starter manifest either/or (`starter.rs:248`, `:880`) | S23 test: 4 rows after bundle install | Manifest merge in S23; bundle for install/revision only |
| App Review 4.2.3 / 2.5.2 | Reviewer asks about downloads | Review notes (S34): one Bible bundled, `.obr` is data |
| Trademark (#337) blocks the name | Lawyer's opinion negative | Fallback shortlist from #337; rename before the first upload (S33) |
| Personal Team limits (7-day profiles, 3 apps/week) slow S08/S11 | Install failures on device | Re-run from Xcode weekly; bring the #336 decision forward (S33 item 3), then enrol — never before it |
| Personal Team App ID registration blocks the final id for the paid team | Apple docs say App IDs are team-scoped-but-global | Dev builds already use the `.dev` id (A6); final id first registered by the Developer Program team in S33/S35 |
| UniFFI's licence turns out not to be MPL-2.0 (D21 accepted MPL-2.0 only) | S02 licence check | Back to the owner; alternatives are a hand-written C ABI via `cbindgen` or `swift-bridge`, decided before S02 writes code |
| iCloud placeholders / stale bookmarks | S31 reads fail after idle | `startDownloadingUbiquitousItem`, re-pick flow from Rust state (S30) |
| Sync data loss | Property test failure; owner loses a Note in S31 | ADR 0006 invariant tests block the merge; conflicts never auto-merge |
| App size > 200 MB | S34 archive size | Strip debuginfo, `opt-level = "s"`, drop unused features; move `openbible-xrefs` to the Study set (owner decision) |
| Xcode/SDK churn (App Store SDK requirement moves yearly) | `xcodebuild` warnings; Apple "Upcoming requirements" | Pin `DEVELOPER_DIR` in CI to S05's recorded version; bump S05 Outcome and S32 together |
| Agent cannot judge "feel" | — | S08, S11, S31, S35 are HITL by design |

## Out of scope (v1)

- Android shell (Compose), Play, the signed APK, NDK/16 KB pages, Android key custody — next
  release over the same `obs-ffi` (ADR 0021).
- The #330 deferred list: Exegetical/Topic Guide, Factbook, Atlas, Timeline, Text Comparison,
  Interlinear, Visual filters, Reading layout options, Link set UI, Layouts, Command box, Favorites,
  Home / Verse of the day, Search Library mode, Saved searches, Morphology criteria, Proximity
  search, editing Highlight styles/palettes, re-attaching Orphaned annotations, Backup/restore UI
  and Data export, Theme editor / Custom theme, Crash report UI, Update check.
- F-Droid; accounts; any server of ours; licensed modern translations; web/WASM build (#324).
- Folder encryption, shared Sync folders, CRDTs, line-merging Notes (ADR 0022).
- SWORD repository browsing on mobile (#333); prebuilt index downloads (#333).
- Pixel-golden screenshot tests for iOS (D14), share extension / deep links / widgets (D6).

## What this plan verified, and what it could not

Checked in the tree at `8aea4b0`: `xtask/src/layers.rs` (`LAYERS`, `UI_CRATES`, `NET_CRATES`,
`wasm_crates` includes every `LAYERS` crate — also L6), `xtask/src/main.rs` (`GATES`, `wasm`,
`licenses` via `apps/openbible/Cargo.toml`), `Cargo.toml` lints/profiles/`members` glob (a
manifest-less `apps/ios` dir breaks `cargo metadata`; reproduced in a scratch workspace),
`about.toml` `accepted`, `crates/ui-egui/src/*`
(egui-free modules listed in A1 incl. `interlinear.rs`; `palette.rs` fields are `egui::Color32`
with 130+ uses; `design_tokens_tests.rs` `TOKEN_FILES`/`ALLOWED`; `shell/note_editor.rs` formatting
functions are egui-free `pub(super)`), `crates/render/src/atlas.rs`
(`GlyphQuad`, `quads_from`, `coverage`, `take_dirty`), `crates/layout/src/lib.rs` exports,
`crates/engine/src/{session,jobs,persist,appearance,starter,import,open_catalogue,search,workspace,library,backups,testing}.rs`
(APIs cited inline; `UserData` lacks `Send`; `catch_unwind` in jobs; `JobQueue` events are
single-consumer and drained by `poll_imports`; `install` is `pub(crate)`; `sync_starter_library`,
`install_from_bundle`, `add_starter` are not wasm; `manifest()` prefers the bundle;
`ImportSource` has no zip variant; `data_dir` is macOS/XDG only; `testing.rs` fixtures
`bsb_john_1`/`bsb_psalm_3`/`tahot`; `starter_tests.rs:921` `#[ignore]` convention),
`crates/store/src/{user,annotation,clock}.rs`
(`USER_SCHEMA_VERSION = 12`, `Change`, `Row` has four variants), `crates/formats/src/sword/repo.rs` (`Fetch`),
`apps/openbible/src/fetch.rs`, `crates/fonts/src/face.rs` (fonts are `include_bytes!`),
`starter/sources.toml` ids, `xtask/src/starter/lock.rs` (`deny_unknown_fields`),
`.github/workflows/{ci,starter}.yml`, `docs/dev/{starter-library,releasing,atlas}.md`,
`docs/research/mobile-folder-sync.md` section numbers,
ADRs 0012/0015/0019/0020/0021/0022, CONTEXT.md § Mobile/§ Sync, GitHub labels
(`ui-review`, `agent-approved`, `owner-approved`).

Not verifiable here (no Xcode, no network to Apple docs): UniFFI version features (proc-macro
remote types, library-mode bindgen flags, `with_foreign` traits, edition-2024 behaviour of
`unsafe_code` under its macros) and its licence field (expected MPL-2.0; `uniffi` is not in
`Cargo.lock` yet), Apple's App ID rules for Personal vs paid teams, XcodeGen `project.yml` keys,
`xcodebuild -create-xcframework`
flags, `UITextInteraction` on custom views, `performAccessibilityAudit`, `BGProcessingTask`
simulation command, `URLSession` background semantics, `kCTFontURLAttribute` readability on iOS,
simulator device names, GitHub macOS runner Xcode versions, `altool --validate-app` current
status. Each is flagged "verify" at the step
that first touches it.
