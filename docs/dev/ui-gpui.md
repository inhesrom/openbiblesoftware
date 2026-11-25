# The GPUI UI (`obs-ui-gpui`)

The desktop UI is moving from egui to GPUI (ADR 0025; the map is #371). Until the cutover
(#401), `crates/ui-gpui` (package `obs-ui-gpui`, layer 6) is built beside `crates/ui-egui`, and
the egui app is the one that ships.

## Build and run

The UI is a cargo feature of `apps/openbible` (ADR 0025, rule 2):

```sh
cargo run -p openbible                                         # the egui app (default)
cargo run -p openbible --features gpui                         # the GPUI app (egui compiled in too)
cargo run -p openbible --no-default-features --features gpui   # the GPUI app, no egui at all
cargo run -p openbible --features gpui -- --smoke              # open, draw a frame, quit
```

With both features on, the GPUI app opens. The benchmarks (`openbible --bench`) need egui.
`--smoke` prints how long after start the first frame was drawn and exits with 0, or exits
with 1 if no frame is drawn within a minute; CI runs it on Linux, macOS and Windows (the
`gpui-smoke` job).

Both apps open the same Session: `obs_engine::launch::open_session` adds the library, the
User database, the Starter library, the Search index and alias caches and the Log folder, and
`obs_engine::launch::start_update_checks` sets up the update checks. The app adds the Fetch
transport and the OS language. For now the GPUI app draws a title bar and an empty Workspace
(#377); it does not restore or save the Workspace, so it leaves the egui app's Last session
alone. The system fonts, system motion and rich clipboard are handed to the egui Shell only:
their traits live in `obs-ui-egui` and move down when a GPUI surface needs them.

### Linux build dependencies

GPUI links fontconfig, xkbcommon, xkbcommon-x11 and xcb, and draws with Vulkan:

```sh
sudo apt-get install libfontconfig-dev libwayland-dev libxkbcommon-x11-dev libx11-xcb-dev \
  libvulkan1 mesa-vulkan-drivers
```

Without the fontconfig `-dev` package, `RUST_FONTCONFIG_DLOPEN=1` loads fontconfig at run time
instead.

## Dependency

`gpui-kit = { version = "=0.10.0", default-features = false }` in the workspace: GPUI (the
`gpui-pre` snapshot of Zed `main` the kit pins) and `gpui-base`, without the styled component
layer (docs/research/gpui-stack.md, #373). It is pinned exactly, because every weekly release
may break the API; bump it on purpose, in its own pull request, as a UI change (ADR 0020).

## Gates

- **Layers:** GPUI and GPUI Kit crates (every crate named `gpui`, `gpui-…` or `gpui_…`) are UI
  crates: `cargo xtask layers` fails if one appears below layer 6.
- **Networking:** unchanged. GPUI's HTTP client is a trait with no transport, so no crate on
  the networking list is in the tree. The GPUI app must never install an HTTP client.
- **wasm32:** `cargo xtask wasm` checks every library crate except `obs-ui-gpui` (rule 7).
- **Licences:** `about.toml` accepts MPL-2.0 for unmodified dependencies (rule 8). The committed
  listing (Help → Credits) is the default build's; `cargo xtask licenses --check` also runs
  cargo-about over the GPUI-only build, so an unaccepted licence there fails the gate. At the
  cutover the GPUI build becomes the default, and so the listing's.
- **Clippy:** `cargo xtask clippy` lints the workspace, then `openbible` with `--features gpui`
  and with `--no-default-features --features gpui`.

## Tests

`cargo test -p obs-ui-gpui` runs the GPUI UI's headless tests (#379; ADR 0019 as amended by
#374). They need no display: run them with `DISPLAY` and `WAYLAND_DISPLAY` unset.

- **The driver** (`crates/ui-gpui/tests/support`):
  - **A Session over the egui tests' fixtures:** the accessibility audit's Starter subset and
    the BSB's Genesis 1, in a scratch Library with a User database, with jobs run inline and
    the clock fixed at the gallery's time (`obs_store::clock`).
  - **Commands:** `Shell::run(command, params)` runs engine commands, as the egui tests do.
  - **The `Driver` trait on `Window`:** draws frames, finds elements by accessible label or
    role, and sends pointer and keyboard input.
  - **What it reads:** the elements come from the accessibility snapshots `gpui-base` records
    under its `test-support` feature: the role, label, value, focus and bounds GPUI writes
    into each AccessKit node.
  - **Not GPUI Kit's test API:** its `test-support` feature names the kit's optional styled
    and GPUI Fast layers. GPUI Fast is in `Cargo.lock` already, as an optional dependency of
    `gpui-base`, but in no build. The feature would add the styled layer's and GPUI Fast's
    platform crates and an HTTP client, and fail `cargo xtask layers`. So the crate's dev-dependencies name GPUI (`gpui-pre`,
    `gpui-pre-platform`) and `gpui-base` directly, at the versions `gpui-kit` pins. Bump them
    together with `gpui-kit`.
  - **Not checkable yet:** the shape of the AccessKit tree. GPUI's test windows never turn
    accessibility on, so only single nodes can be checked (#374).
- **Interaction tests** (`tests/interaction.rs`): `#[gpui_kit::test]` on GPUI's
  TestPlatform, on Linux, macOS and Windows alike.
- **Rendering** (`tests/rendering.rs`): its own harness on the main thread, drawn by GPUI's
  headless renderer.
  - **Linux:** wgpu; on a computer with no GPU, Mesa's software Vulkan driver, lavapipe
    (`mesa-vulkan-drivers`). Frames are compared with the PNG goldens in
    `crates/ui-gpui/goldens/linux/`. A pixel matches when no channel is more than 2 levels
    off, and a frame matches when at most 0.01 % of its pixels are off.
  - **macOS:** Metal. There are no goldens, because Core Text draws text differently from
    Linux. Each surface is drawn three times, and the captures must be identical and not
    blank.
  - **Windows:** skipped. GPUI has no headless renderer there.
  - **Without a headless renderer** (Linux without Mesa's Vulkan drivers), the test says why
    and passes. `OBS_GPUI_REQUIRE_RENDERER=1` makes that a failure, as CI should.
  - **Blessing:** `UPDATE_GOLDEN=1 cargo test -p obs-ui-gpui --test rendering` writes goldens
    that are missing or off. Review the diff. Without it, a missing golden fails.
  - **Review copies:** frames, including `*-actual.png` for a mismatch or a missing golden,
    are written to `ui-gpui-frames/` in `CARGO_TARGET_DIR`, or in the workspace's `target/`.
  - **Test runners:** the harness lists itself as one test, `rendering`, so
    `cargo nextest run` (and `cargo xtask ci`) runs it, and it honours a test-name filter,
    `--exact` and `--skip` as `cargo test` does.
  - **The seeded change:** a test-only override (`Shell::override_chrome`, under the crate's
    `test-support` feature) changes the surface colour by 4 levels. The comparison must then
    fail, which checks that the gallery catches a token change.
- **Surfaces with goldens so far:** the scaffold's window (title bar and empty Workspace), in
  Light and Dark.
- **Fonts:** the Shell draws its text in the bundled Gentium Plus, so frames are the same on
  every computer. #378 chooses the Interface font.

**Only 2× for now.** gpui-pre 0.3.12's headless windows are fixed at 2×. 1× needs
`HeadlessAppContext::simulate_window_scale_factor_change`: a 19-line patch, on the branch
`prototype/374-gpui-headless` in `prototype/gpui-headless/patches/`, that forwards to the test
window's existing scale change. A `[patch]` can only be set in the workspace's `Cargo.toml`, so
it is not carried yet. Until it is merged upstream, it goes in as:

```toml
[patch.crates-io]
gpui-pre = { path = "vendor/gpui-pre-0.3.12" }   # gpui-pre 0.3.12 plus the headless-scale patch
```

Then the rendering test can draw 1× frames too.

## Screen readers

GPUI's AccessKit is on by default. The title bar's **Quit** button (labelled "Quit Open Bible
Software") is there so a screen reader has something to reach; check it with VoiceOver
(macOS), NVDA (Windows) and Orca (Linux). A known upstream bug on Windows: the window's content
is reachable only after focus leaves the window and comes back.
