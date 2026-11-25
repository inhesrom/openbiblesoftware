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
transport and the OS language.

## The Workspace (#385)

`crates/ui-gpui/src/workspace.rs` draws the engine's Panels and Tabs in `gpui-base`'s
`DockArea` (docs/research/gpui-stack.md), kept in step as the egui Shell keeps `egui_tiles`:

- **Engine → dock:** each frame applies the engine's Effects: `Opened` puts the Tab where its
  `Placement` says (into a Panel, or a new Panel on a side of another, `levels` out), `Focus`
  and `Activate` show it, `Closed` removes it, `Rebuild` builds the dock from
  `Workspace::arrangement` (a Layout or the Last session), `ResetLayout` makes the default
  arrangement, `PromptLayoutName` asks for a Layout's name, `Message` shows a toast. Moving
  views and Tools' windows come with their tickets.
- **Floating windows** come with #388. Until then the engine docks them (`dock_window`) at
  the right of the main window: the Panels of an egui Last session's or Layout's floating
  windows, and a Tab floated by `tab.float`. A Tab is never closed because the dock left it
  out: a Tab the arrangement misses goes into its Panel (`workspace::with_every_tab`).
- **Dock → engine:** after every change of the dock (a drag, a split, a ×) the Shell tells the
  engine each Panel's Tabs (`sync_panels`: a Tab no Panel shows is closed and goes to
  Recently closed, #214) and records the arrangement for Layouts and the Last session. A dock
  tab group is an engine Panel (`DockMap::groups`); a dock panel is a `TabView`, one per Tab.
- **A Tab:** its header strip (link chip, ‹ ›, the reference or Key box running `tab.go`,
  title, Aa, ⋯ with Close Tab and Reopen closed Tab) over `workspace::tab_content`: the hook the
  reading pane (#380) and the Tools plug into, one `match` arm per Tab kind. A Tool shows a
  placeholder until its ticket.
- **A Resource Tab** has one reading pane (`workspace::ResourcePane`), read when the Tab
  opens and kept, across frames and drags, until it closes; a Layout restored reads every
  pane again, as egui does. As the egui Shell's `load`, the Tab learns the Resource's
  numbering and Keys, and the pane starts at the Tab's location (else its Link set's, else
  another Bible's, else the first Verse).
  - The Tab's navigation moves the pane: the engine's `Navigate` Effect (`tab.go`, the header
    box, Back, Forward) jumps it; `NavigateStandard` and `NavigateOpened` go through the
    engine first. A parallel Resource switched in, or a Resource file replaced, is read again.
  - Where the pane is scrolled is the Tab's location (`Workspace::scrolled`), so the header
    box and Back follow it. The Link set does not follow yet (#386).
  - A Resource with no text (a Cross-reference set, a People & places index) says where its
    data shows, with one button that goes there. A Resource that cannot be read says so, with
    Retry and Details; the error shows only after Details (#223).
- **Overlays** (the + menu, the ⋯ menu, the Layout-name dialog) occlude what is under them,
  so a click on them never reaches a reading pane.
- **The Panel's +** opens a Resource or the Library in that Panel.
- **Layouts** are engine data: save (the name dialog), restore, rename and delete run the
  `layout.*` commands; a Layout saved by the egui app restores in GPUI, in its proportions.
  A split's shares are its slots' sizes scaled to average 1, as egui_tiles' are.
- **The Last session:** the GPUI app restores it at launch, but writes it only with
  `OBS_GPUI_SAVE_SESSION=1` (`Shell::set_save_session`). While egui ships, trying the GPUI app
  must not overwrite the egui app's Last session with a Workspace whose Tools are
  placeholders.

The system fonts, system motion and rich clipboard are handed to the egui Shell only: their
traits live in `obs-ui-egui` and move down when a GPUI surface needs them.

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
- **Component tests** (`tests/components.rs`, #378): the shared components and the
  catalogue, on the same driver and TestPlatform. The IME is driven through the field's own
  `ElementInputHandler`.
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
- **Surfaces with goldens so far:**
  - the window with an empty Workspace, in Light and Dark;
  - a two-Panel Workspace in Light: KJV and BSB (#385), and KJV beside the Hebrew fixture
    (`workspace-kjv-hebrew`, #380), each Bible at Gen 1:1 in its Resource Tab;
  - a Resource Tab's reading pane (#380) in Light: `reading-kjv-gen-1` (the Starter KJV), `reading-hebrew-gen-1`
    (Gen 1:1-5 with points and cantillation, one right-to-left paragraph) and
    `reading-greek-john-1` (John 1:1-5). Each is drawn three times from a fresh context and must
    be identical. The egui app's counterparts are the gallery's reading surfaces in
    `docs/previews/gallery/`.

  The headless context has no asset source, so icon buttons draw without their icons in these
  frames.
- **Workspace tests** (`tests/workspace.rs`, #385): on the TestPlatform, ported from the egui
  tests' behaviour.
- **Resource Tab tests** (`tests/resource_tabs.rs`): the header box, Back, Forward and
  `tab.go` move the Tab's one pane; scrolling moves the Tab's location; a closed Tab's pane
  goes; and the egui `pane_states_tests` behaviours of a Resource Tab (nothing to read, and an
  unreadable Resource with Retry and Details).
- **Reading pane tests** (`tests/reading.rs`, #380), on the TestPlatform. Each names the egui
  test it ports: Scroll speed for wheel lines and trackpad points, Ctrl+wheel, the arrow and page
  keys, motion against Reduce motion, the Text size and Resource text size keeping the Current
  location, and Hebrew right to left and never hyphenated. Also: Greek breaks between words, a
  jump into a long paragraph is drawn in its first frame, and the paragraph nodes.
  - GPUI's test windows never turn accessibility on, so each block records the node it writes
    (`reading::observed`, under `test-support`), and the tests read that.
- **Fonts:** the Shell draws its chrome in the default Interface font (egui's Ubuntu Light,
  asked for by its Light weight, so an installed Ubuntu Regular is not drawn instead) and its
  wordmark in Gentium Plus. Both are bundled, so frames are the same on every computer.

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

## The look and the shared components (#378)

- `tokens.rs`: the design tokens (ADR 0015), copied from the egui app's `tokens.rs` and
  `Palette`: spacing, type scale, radii, strokes, insets, elevations, sizes, motion durations,
  the chrome colour roles (Light, Dark, High contrast light and dark) and the page colours of
  each Page tone. `tests/tokens.rs` fails if any value differs from the egui one.
- `theme.rs`: the `Look`, a GPUI global that holds the chrome, page, Interface font, Interface
  size and the app's own Reduce motion setting. It is resolved from the Session's appearance
  and Custom theme (ADR 0018) as the egui app's `theme.rs` does. `set_look` replaces it and
  refreshes every window, so a theme change shows in the next frame. It also sets gpui-base's
  reduce motion preference from the app's setting alone: Reduce when the app asks, else
  System (the operating system's setting, which gpui-base keeps current). Code that animates
  reads `cx.reduce_motion()`, not the Look. The Shell makes the Session's Look current when the Session's appearance
  changes (a command such as `view.theme`, or Follow system when the window's appearance
  changes); the catalogue keeps the Look its buttons choose.
- `icons.rs`: the Lucide set from `crates/ui-egui/assets/icons`, compiled in with the egui
  app's names and drawn as SVG masks in a theme role's colour. `icons::Assets` is the app's
  asset source.
- `components.rs`: buttons, icon buttons, text and search fields, switches, radios, chips,
  section headers, the Tab page, cards, notices, menus and menu items, popovers, tooltips,
  toasts, dialogs, the empty, loading and error states, and scroll bars.
  - Controls are built on gpui-base's primitives (`Button`, `Switch`, `Radio`, and
    `InputBase` over `InputState`). Those give them activation, keyboard focus, IME, undo and
    the clipboard, plus their AccessKit roles.
  - Every component has an accessible name, and every control has a visible keyboard focus
    ring (heavy in High contrast). A primary button's ring is two rings, the label's colour
    inside and lapis around it, as lapis alone would not show on its lapis fill. Switches,
    radios, menu items and a chip's Close button keep a transparent border the ring's width,
    so focus does not move them.
  - An icon button shows its name in a tooltip while the pointer rests on it.
  - Containers (menus, dialogs, notices, states) get their role and name through a stateful
    element. Call `.test_support()` on the finished element to make it findable by name in
    headless tests.

### The component catalogue

`OBS_GPUI_CATALOGUE=<Look> cargo run -p openbible --features gpui` opens the catalogue instead
of the Workspace:
- the Look is `Light`, `Dark`, `Sepia`, `High contrast` or `High contrast dark`;
- `OBS_GPUI_ZOOM=1.25` sets the Interface size;
- buttons at the catalogue's top switch the Look while it runs.

`OBS_GPUI_PREVIEWS=1 cargo test -p obs-ui-gpui --test previews` draws the catalogue in every
Look with GPUI's headless renderer, at 2×, into `crates/ui-gpui/previews/`. The same test file
checks the pixels of the focus rings a colour could hide (the primary button's, a chip's Close
button's, a menu item's). Without a headless renderer these tests are skipped, unless
`OBS_GPUI_REQUIRE_RENDERER=1`.

## The reading pane (#380)

`reading.rs`: a `ReadingPane` reads one Resource through `obs-reading`'s `ReadingView`, the same
reading core the egui app and the mobile app use. ADR 0004's #372 amendment decides how it paints:
- **Fonts:** the bundled reading fonts (Gentium Plus, Ezra SIL) are registered with GPUI's text
  system by `install`. `ReadingFonts` maps each `obs_fonts::FaceId` to its GPUI `FontId` once.
- **Painting:** every glyph our pipeline laid out (harfrust shaping, unicode-bidi, our line
  breaking) is painted with `Window::paint_glyph`, at our position, in the Page tone's colour for
  its role. GPUI's own text system never lays out Resource text.
- **Layout:** in the pane element's prepaint, where its size is known: the Reading column (#209)
  centred as in the egui pane, then `ReadingView::frame` within `SYNC_BUDGET`. Other blocks go to
  the layout workers and show as faint bars until they arrive. The block a jump lands in is laid
  out at once. While a glide or a worker layout is pending, the pane asks for another frame.
- **Input:**
  - Wheel lines (40 points each) and trackpad points scroll at the Scroll speed (#293). Ctrl or
    Cmd with the wheel never scrolls.
  - Page Down, Space and Page Up move 0.9 of the pane; the arrows move 40 points at the Scroll
    speed. Keys and jumps glide unless motion is reduced (#225).
  - The scroll bar's thumb follows the position and can be dragged.
  - Unlike egui, the wheel is not smoothed over frames.
- **Settings:** each frame the Shell hands the pane the Resource's Text settings (the Text size,
  or the Resource's own text size, and the Reading layout), the Page tone and the Scroll speed. A
  change lays the text out again and keeps the Current location.
- **Accessibility:** each laid block in view is one element with `Role::Paragraph`, its text and
  its direction. Its Tagged words and references are synthetic `Role::Link` children, with bounds
  relative to the window in device pixels.
- **Not yet:** marks and highlights (#382), selection, Find and Word Info hover (#381),
  footnotes, the Note margin and the jump flash (#383). Also faces loaded from the computer
  (#138): `obs-fonts` does not expose a loaded face's file yet, so their glyphs are not drawn.

`OBS_GPUI_READ` opens a Resource in a Resource Tab at launch:

```sh
OBS_GPUI_READ=kjv-strongs cargo run -p openbible --features gpui              # at its start
OBS_GPUI_READ='tahot@Gen 1:1' cargo run -p openbible --features gpui          # at a reference
```

The reference is typed as into the header box (`tab.go`), in the Resource's own numbering.
The pane opens with the keyboard.

