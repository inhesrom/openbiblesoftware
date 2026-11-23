# Which GPUI, and which component kit (#373)

Which GPUI build should the desktop app depend on, now that its UI is moving to GPUI
(ADR 0025)? And should we build Panels, inputs and menus ourselves, or take a component kit?

This note answers both. It draws on two sources, both from late November 2026:
- **Primary sources:** Zed's repository (source at `main`, PRs and issues), the crates.io API,
  [longbridge/gpui-kit](https://github.com/longbridge/gpui-kit),
  [longbridge/gpui-fast](https://github.com/longbridge/gpui-fast) and
  [gpui-archipelago/gpui-unofficial](https://github.com/gpui-archipelago/gpui-unofficial).
- **Dependency checks** run on this machine: `cargo metadata`, `cargo tree` and `cargo check`
  on one probe crate per candidate. There were no full builds.

**Not confirmed** marks a claim no primary source settled.

## Recommendation

1. **Depend on `gpui-kit` 0.10 with `default-features = false`, pinned exactly (`=0.10.x`).**
   - That gives GPUI (the `gpui-pre` snapshot the kit pins, re-exported) plus `gpui-base`.
     `gpui-base` holds the kit's unstyled behaviour: the dock, text input and text area with
     IME, popovers, dialogs, scroll bars, focus traps, reduce motion, and AccessKit roles and
     actions.
   - It is the only candidate with a serializable dock, finished text inputs and a UI test
     harness that queries AccessKit. Building those ourselves on plain GPUI would add weeks
     to G1–G3.
   - Our own look goes on top, from our design tokens and shared components (ADR 0015). We
     don't take the styled `gpui-component` layer: the look is ours.
2. **Upgrade on purpose, not weekly.**
   - `gpui-kit` and `gpui-pre` both release every Monday, and any release may break the API.
   - We bump when we need a fix, and otherwise about once a month.
   - Each bump is its own pull request, treated as a UI change under ADR 0020 rule 4, with the
     gallery diff.
3. **Leave `gpui-fast` off.** It is Longbridge's experimental fork of GPUI. It claims 73–88 %
   less main-thread CPU than gpui-pre 0.3.7
   ([README](https://github.com/longbridge/gpui-fast)), but its accessibility code is its own
   and unverified. Measure it later if frame time needs it.
4. **Keep an exit path.** `gpui-base` is Apache-2.0, and `gpui-pre` is an unmodified snapshot
   of Zed `main`. If Longbridge stops publishing, we can vendor `gpui-base` and point it at
   `gpui-unofficial` or a pinned Zed revision. Both carry the same upstream API.
5. **ADR 0025's stop point is not triggered.** AccessKit is built into every 2026 GPUI build on
   macOS, Windows and Linux, and is on by default. Two known bugs are open:
   - on Windows, the window's content is only reachable after focus leaves and returns;
   - views drawn from cache drop their accessibility nodes.

   These are reasons to check early, not to stop: #377 should include a screen-reader check
   on each OS, instead of waiting for #398.
6. **The gates need no exceptions.**
   - The 2026 builds bring no HTTP transport: the client is a trait, with a null client by
     default. The networking exception allowed by ADR 0025 rule 6 is not needed.
   - No licence falls outside `about.toml` once MPL-2.0 is accepted.
   - wasm32 is not required (ADR 0025 rule 7).

## The candidates

| | `gpui` 0.2.2 | `gpui-pre` 0.3.x | Zed git revision | **`gpui-kit` 0.10** | `gpui-unofficial` 1.29.x |
|---|---|---|---|---|---|
| Publisher | Zed | Longbridge (weekly snapshot of Zed `main`) | Zed | Longbridge | gpui-archipelago (third party) |
| Latest | 0.2.2, 2025-10-22 | 0.3.14, 2026-11-23 | any commit | 0.10.0, 2026-11-16, pins `gpui-pre =0.3.12` | 1.29.3, 2026-11-20 |
| Releases in 2026 | none | weekly since 2026-09-03 (15) | every commit | weekly (16); 0.7 → 0.10 in seven weeks | 71, following Zed's release tags |
| AccessKit | no | yes, always on | yes, from `1d029c5` | yes, plus roles and names on its widgets | yes |
| Docking | no | no | no | yes: splits, tabs, drag, zoom, JSON state | no |
| Text input widgets | IME bridge only | IME bridge only | IME bridge only | Input, Textarea, Editor: IME, undo, clipboard | IME bridge only |
| Bidi caret in input | no | no | no | layout mirrored; no caret inside RTL runs | no |
| Multiple OS windows | yes | yes | yes | yes, one dock per window | yes |
| Headless pixels | no | macOS (Metal), Linux (wgpu) | same | same, with a ready test harness | same |
| HTTP transport forced | yes (`zed-reqwest`, `hyper`, `tokio`) | no (trait only) | no | no | no |
| Renderer on Linux / macOS / Windows | blade / Metal / Direct3D | wgpu 30 / Metal / Direct3D 11 | same | same | same (wgpu patched to 30.0.1) |
| wasm32 | no | yes, one window | yes | yes (default features need nightly) | yes (nightly) |
| Licence | Apache-2.0 | Apache-2.0 | Apache-2.0 | Apache-2.0 | Apache-2.0 |
| Distinct packages, Linux ¹ | 535 | 462 | – | 544 (488 without default features) | 466 |

¹ Normal dependencies only, counted with `cargo metadata --filter-platform
x86_64-unknown-linux-gnu`. The 2026 builds are measured together with their platform crate.
Counted the same way, the egui app has 369. The earlier figures of 361 and 569 counted crates
compiled, build scripts and proc macros included.

**`gpui` 0.2.2 is a dead end.** It is still the newest `gpui` on crates.io, and Zed has not
published it since ([crates.io](https://crates.io/api/v1/crates/gpui)). Everything we need
landed on Zed `main` after it:
- AccessKit ([#56065](https://github.com/zed-industries/zed/pull/56065), May 2026);
- the wgpu renderer on Linux ([#46758](https://github.com/zed-industries/zed/pull/46758));
- the split into platform crates ([#49277](https://github.com/zed-industries/zed/pull/49277));
- the web backend ([#50228](https://github.com/zed-industries/zed/pull/50228));
- the Linux headless renderer ([#64718](https://github.com/zed-industries/zed/pull/64718)).

Its HTTP client pulls in `zed-reqwest` and cannot be turned off
([deps](https://crates.io/api/v1/crates/gpui_http_client/0.2.2/dependencies)).

**`gpui-pre`, a Zed revision and `gpui-unofficial` are the same code on different schedules.**
- `gpui-pre` is published weekly by Huacnlee of Longbridge, as "unmodified snapshots" of Zed
  `main` ([gpui-kit FAQ](https://github.com/longbridge/gpui-kit/blob/main/website/docs/faq.md)).
- `gpui-unofficial` builds each Zed release tag. It patches `gpui_wgpu` for crates.io wgpu
  30.0.1 and drops the `inspector` feature
  ([upstream patches](https://github.com/gpui-archipelago/gpui-unofficial/blob/main/docs/upstream-patches.md)).
- A git dependency on Zed brings the whole Zed workspace into resolution, and Zed's own
  `[patch]` entries don't carry over.

None of the three offers docking or finished text inputs: in Zed, those live in GPL crates
that are not published.

**`gpui-kit` adds the parts an app needs on top of GPUI**
([README](https://github.com/longbridge/gpui-kit)). It powers
[Longbridge Pro](https://longbridge.com/desktop), and its showcase lists over 100 apps. Which
GPUI build Longbridge Pro ships on is not confirmed.

## Accessibility

- **Merged and always present.** AccessKit was merged into GPUI on 2026-05-27
  ([#56065](https://github.com/zed-industries/zed/pull/56065)). It is a non-optional dependency
  of `gpui` ([Cargo.toml](https://raw.githubusercontent.com/zed-industries/zed/main/crates/gpui/Cargo.toml)),
  of the macOS and Windows platform crates, and of the Linux one through its `x11` and
  `wayland` features. An app can turn it off at run time (`Application::new_inaccessible`).
  Zed does so in its own builds, because AccessKit panicked in nightlies
  ([#57954](https://github.com/zed-industries/zed/pull/57954)).
- **Reports in the PR thread, per platform:**
  - **macOS (VoiceOver):** works. The early "only the title bar is reachable" problem was fixed
    before the merge.
  - **Windows (NVDA 2026.1.1, Narrator):** the content is reachable only after focus leaves the
    window and comes back. This was reported after the merge, and no fix was found. JAWS
    reaches the controls, but gives nothing the first focus.
  - **Linux (Wayland):** the nodes are visible over AT-SPI. Orca was not tested.
  - **Linux (X11):** checked by a NixOS integration test.
- **Open bug.** Views drawn from cache (`Entity::cached`) lose their nodes, and focus falls
  back to the window ([#66690](https://github.com/zed-industries/zed/issues/66690), November
  2026). Whether gpui-kit's 0.10 dock still draws panels through `.cached` is not confirmed.
- **Exposing our own painted text.** A custom `Element` can give itself a role and add
  synthetic children through `a11y_synthetic_children`
  ([_accessibility.rs](https://raw.githubusercontent.com/zed-industries/zed/main/crates/gpui/src/_accessibility.rs)).
  Synthetic children are always leaves
  ([window/a11y.rs](https://raw.githubusercontent.com/zed-industries/zed/main/crates/gpui/src/window/a11y.rs)).
  That fits #19's rule of one AccessKit node per laid-out block:
  - each paragraph is a real element with `Role::Paragraph`;
  - its Tagged words and references are synthetic `Role::Link` children, with their bounds
    set by hand.

  Which coordinate space those bounds use is not confirmed; #372 and #380 should check it with
  a screen reader.

## Docking, text input and windows (`gpui-base`)

- **Dock**
  ([source](https://github.com/longbridge/gpui-kit/blob/main/crates/base/src/dock/mod.rs),
  [state](https://github.com/longbridge/gpui-kit/blob/main/crates/base/src/dock/state.rs),
  [docs](https://gpui-kit.com/base/dock)):
  - `DockArea` has a centre region and left, right and bottom docks. Each region is a tree of
    splits and tab groups.
  - Dropping a tab in the middle of a group merges it; at an edge, it splits; on the strip, it
    inserts.
  - Docks resize and collapse, and any node can be zoomed.
  - `dump` and `load` round-trip a serde `DockAreaState`. Panels are rebuilt through a
    registry by name, and a `LayoutChanged` event fires on every edit.
  - Compared with egui_tiles, it does what we use today and adds edge docks and zoom. Our
    Workspace model stays our own type in the engine, converted to and from the dock state, as
    with egui_tiles (ADR 0005).
  - The dock was rewritten between 0.7 and 0.10 (`TabPanel` and `StackPanel` became
    `PaneTree`, `TabGroup` and `DockLayout`). Expect more change.
- **Text input:**
  - Input, Textarea and Editor support IME composition, history (undo), clipboard and Unicode
    input, and are covered by the kit's tests
    ([TESTING.md](https://docs.rs/crate/gpui-kit/latest/source/TESTING.md)).
  - 0.10 added a layout direction that mirrors the layout
    ([#3886](https://github.com/longbridge/gpui-kit/pull/3886)). A caret or hit test inside a
    right-to-left run still waits on GPUI
    ([#31102](https://github.com/zed-industries/zed/issues/31102), with the partial fix
    [#60115](https://github.com/zed-industries/zed/pull/60115) still open).
  - This matches ADR 0004: typing Hebrew in text inputs stays unsupported.
- **Windows:** one `App` owns every entity, and windows open with `cx.open_window`. Floating a
  Tab into its own OS window while it shares the app's state is native to GPUI (#388). Moving a
  dock panel from one window's `DockArea` to another's is not documented; #388 should try it
  early.

## Testing and screenshots (for #374)

- **What the GPUI builds give:**
  - `TestAppContext` draws with a mocked platform.
  - A visual test context draws real macOS windows off-screen
    ([8c0b088](https://github.com/zed-industries/zed/commit/8c0b088366545e5050711066a4be666266f28110)).
  - A Linux wgpu headless renderer, added in September 2026, reads frames back as RGBA. It
    refuses software adapters unless asked, and it sits behind `test-support`
    ([#64718](https://github.com/zed-industries/zed/pull/64718)).
  - **Windows has no headless renderer.**
- **What gpui-kit adds:** its test harness drives headless windows with pointer, keyboard,
  scroll and drag input. It finds elements by their AccessKit label or role, and checks focus
  and IME. Its rendering tests check real off-screen pixels on macOS and on Linux, including
  under Mesa lavapipe, and skip Windows
  ([TESTING.md](https://docs.rs/crate/gpui-kit/latest/source/TESTING.md)).
- **What follows for #374:** text is rasterized by a different system on each OS (cosmic-text,
  Core Text, DirectWrite). So goldens cannot be shared across systems as ADR 0019's are today:
  they need to be per OS, with Windows drawn on a GPU runner or not at all. #374 decides.

## Gates

- **Networking (ADR 0025 rule 6):**
  - In every 2026 build the HTTP client is a trait. Its dependencies are `anyhow`, `bytes`,
    `futures`, `http`, `http-body`, `serde`, `url` and a few more, with no transport.
  - The app gets a `NullHttpClient` unless it installs one with
    `Application::with_http_client`
    ([gpui-pre-http-client](https://crates.io/api/v1/crates/gpui-pre-http-client/0.3.14/dependencies);
    sources read). Images loaded from a URL need an installed client; local images don't.
  - The only HTTP-related crate left in the tree is `http`, which holds types and is not on
    `xtask layers`' networking list. So **no exception is needed**: `ui-gpui` must simply never
    install an HTTP client. A review rule is enough; add a check if one ever appears.
- **Licences:** with MPL-2.0 accepted (`option-ext`, `cbindgen`, and `dwrote` on Windows),
  nothing on any candidate or target falls outside `about.toml`. `self_cell` and `r-efi` are
  taken under their MIT or Apache alternatives.
- **wasm32:** `ui-gpui` is exempt. For the record:
  - `gpui-kit` 0.10 with default features fails on stable Rust: its web backend's
    `multithreaded` feature needs nightly.
  - `gpui-pre` with `gpui-pre-web` and no default features passes `cargo check --target
    wasm32-unknown-unknown`.

## Building on Linux

`cargo check` of `gpui-pre` and `gpui-kit` 0.10 passed in 26–30 s on this machine. The
`xattr` crate that broke `gpui` 0.2.2 is gone.

- **fontconfig:** linked at build time unless `RUST_FONTCONFIG_DLOPEN=1` is set, which makes
  `yeslogic-fontconfig-sys` load it at run time instead.
- **xkbcommon, xkbcommon-x11 and xcb:** linked through `#[link]` attributes, so linking needs
  their `-dev` packages. Not confirmed with a full link.
- **Wayland:** loaded at run time.
- **wgpu:** on Linux it needs Vulkan, or EGL and GLES, at run time.

gpui-kit's CI installs this set on Ubuntu 24.04
([install-linux.sh](https://github.com/longbridge/gpui-kit/blob/main/script/install-linux.sh)):
`libfontconfig-dev libwayland-dev libxkbcommon-x11-dev libx11-xcb-dev libvulkan1`. Add
`mesa-vulkan-drivers` for headless rendering on CI. gpui-kit turns on both the X11 and Wayland
backends, and we cannot choose only one without a `[patch]`.

## What this changes for other tickets

- **#377 (scaffold):** use `gpui-kit = { version = "=0.10.x", default-features = false }`, and
  `gpui_kit::application()` as the entry point (GPUI's `gpui_platform::application()`
  underneath). Add the Linux packages above to CI. No
  networking exception is needed. Add a screen-reader check: a labelled button, reached on
  macOS, Windows and Linux.
- **#372 (painting our text):**
  - The prototype's numbers came from `gpui` 0.2.2, which draws with blade on Linux. The 2026
    builds use wgpu on Linux (Metal and Direct3D 11 on the others), so measure again on the
    chosen build.
  - The accessibility design above (a paragraph element with link leaves) is the one to build.
- **#374 (headless tests):** headless pixels exist for macOS and Linux only, and text is
  rasterized per OS. Goldens must be per OS, or limited to Linux.
- **#385 (Workspace):** build on `gpui-base`'s `DockArea`, with our Workspace model converted
  to and from its state.
- **#388 (Floating windows):** check moving a panel between two windows' docks first.
- **#398 (accessibility):** check whether the Windows focus-on-launch bug and #66690 are fixed
  in the pinned build.

## Not confirmed

- Whether the Windows focus-on-launch problem has been fixed on Zed `main` since May 2026.
- Whether gpui-kit 0.10's dock still draws panels through `.cached` (which would hit #66690).
- The coordinate space for the bounds of synthetic accessibility nodes.
- Moving a dock panel between windows.
- Which GPUI build Longbridge Pro ships on.
- Whether linking needs the `-dev` packages for xkbcommon and xcb.
