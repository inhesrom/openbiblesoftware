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

## Screen readers

GPUI's AccessKit is on by default. The title bar's **Quit** button (labelled "Quit Open Bible
Software") is there so a screen reader has something to reach; check it with VoiceOver
(macOS), NVDA (Windows) and Orca (Linux). A known upstream bug on Windows: the window's content
is reachable only after focus leaves the window and comes back.
