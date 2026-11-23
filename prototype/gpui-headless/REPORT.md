# Headless GPUI tests and screenshots (#374): report

> PROTOTYPE, throwaway. Branch `prototype/374-gpui-headless` only. Question from #374: can
> GPUI surfaces be driven and drawn headlessly, so `ui-egui`'s tests and the Screenshot gallery
> (ADR 0019) carry over? If not, where are goldens drawn, and with what tolerance?

## Answer

**Yes on Linux and macOS; Windows can draw only from a real, hidden window.**
ADR 0025's stop point ("GPUI surfaces cannot be tested and screenshotted headlessly") is
**not triggered**. Three things change from ADR 0019:
- goldens must be **per OS**;
- Linux is the one place goldens are drawn with no GPU at all (Mesa lavapipe);
- 1× needs a small change to GPUI (a 19-line patch here, an upstream pull request later).

## What works on Linux

Measured on this machine: Ryzen 7 7840HS, Mesa lavapipe (`llvmpipe (LLVM 21.1.8, 256 bits)`,
Vulkan), with `DISPLAY` and `WAYLAND_DISPLAY` unset. Release build, `gpui-kit =0.10.0` with
`gpui-pre =0.3.12`. `./run.sh` passes in 112 s wall; most of that is building the patched
copy of gpui-pre.

**Interaction** (`tests/interaction.rs`): `#[gpui_kit::test]` on GPUI's `TestPlatform`, with
no display, GPU or screen reader. 6 tests pass in 0.02 s in total; one click test takes
2–4 ms.

| Test | Shows |
|---|---|
| `runs_without_a_display_server` | `run.sh` unsets `DISPLAY` and `WAYLAND_DISPLAY`, and the test asserts both are unset |
| `a_pointer_click_on_the_labelled_button_changes_state` | the Button is found by its label "Save" with `Role::Button`; a pointer down and up at its centre (real hit testing) plus `click("save")` make two saves |
| `clicking_a_list_row_selects_it` | `click(("book", 2))` selects the third row |
| `typing_into_the_input_changes_its_value` | a pointer click focuses the Input; `input("John 3:16x")` and `press("backspace")` give the value "John 3:16", read back as the field's accessible value; the field reports it has focus |
| `ime_composition_marks_then_commits_text` | `TestInput` over the Input's `ElementInputHandler` composes "ש", then "שָׁ" (marked ranges 0..1, then 0..3 in UTF-16), then commits "שָׁלוֹם", after which nothing is marked |
| `controls_are_found_by_accessible_label_and_role` | `find_all_by_role(ListItem)` gives Genesis, Psalms, John; the field is `TextInput` "Reference" with its placeholder; `elements()` lists every exposed node |

What the accessibility queries read:
- `ElementSnapshot` builds each observed element's `accesskit::Node` with the same
  `write_a11y_info` that feeds the real tree: role, label, value, placeholder,
  checked/selected/expanded, disabled, focus.
- **The full AccessKit `TreeUpdate` cannot be read on the test platform.** GPUI builds it only
  after a platform adapter activates accessibility, and `TestWindow::a11y_init` does nothing,
  so `Window::debug_a11y_tree_json()` returns `None`. The test asserts this. Only gpui-kit's
  GPUI Fast build has a switch for it (`GPUI_A11Y_ACTIVE=1`, used by its `a11y_text` test).
- In practice: node by node, we can check what a screen reader would be told. The tree's
  shape (parents, order, synthetic children such as the paragraph link nodes #372 plans) is
  not checkable without upstream help.

**Rendering** (`tests/rendering.rs`, its own harness on the main thread, as gpui-kit's own
`rendering` target): `HeadlessAppContext` with the platform's real text system
(cosmic-text) and `gpui_platform::current_headless_renderer` (wgpu, zed-industries/zed#64718).

| Check | Result |
|---|---|
| A frame as RGBA, saved as PNG | 960×520 px at 2×; 480×260 px at 1× with `./scale.sh` |
| Three captures from fresh contexts, light and dark, 1× and 2× | **byte-identical** every time, and across separate processes |
| Committed goldens (`goldens/linux/`) with ADR 0019's tolerance | match: 0 pixels off, maximum channel difference 0 |
| A seeded change: one click on Save redraws "Save (0)" as "Save (1)" | 198 of 499,200 pixels differ, up to 205 levels: caught (the tolerance allows 49) |
| Time per screenshot, warm | context about 5 ms, open plus one frame 45–60 ms, capture 4–12 ms |
| Time per screenshot, first in a process | context 20–40 ms, frame 120–340 ms (shader and device setup), capture 20–210 ms |

At about 70 ms a surface, the gallery's 102 surfaces × 3 Looks × 2 scales (612 images) would
take about 45 s of rendering on one core of a CI runner with lavapipe.

**1× needs a GPUI change.** In gpui-pre 0.3.12, `TestWindow` starts at scale 2.0. Only
`TestAppContext` can change it, and that context cannot take a renderer. `HeadlessAppContext`
can take a renderer but cannot change the scale. `patches/gpui-pre-headless-scale.patch`
(19 lines) adds `HeadlessAppContext::simulate_window_scale_factor_change`, which forwards to
the existing `TestWindow::simulate_scale_factor_change`. `scale.sh` builds against a patched
copy, and 1× then works with the same determinism. That patch is a natural upstream pull
request to Zed.

**Not available on this machine:**
- A real GPU: the user is not in the `render` group. So lavapipe against hardware Vulkan
  (RADV) was not compared.
- wgpu's GL backend headless: with Vulkan hidden, `No usable headless GPU adapter found`.

## macOS

- **Works on GitHub's hosted runners.** gpui-kit's CI (`.github/workflows/ci.yml` in
  longbridge/gpui-kit, job "Test", `aarch64-apple-darwin` on `macos-latest`) runs `cargo test
  ... --test rendering` as a required step.
- **It passes there.** Run 45096575353 on 2026-11-23 logged `rendering: 16 passed (real
  offscreen renderer)`. That's Metal on the runner's virtualized Apple GPU, through
  `MetalHeadlessRenderer`.
- **The same `HeadlessAppContext` code** in `tests/rendering.rs` selects it through
  `current_headless_renderer`.
- **A real hidden window can also be captured** (`MacWindow::render_to_image`; see
  `src/bin/capture.rs`).
- **Not run by me:** the workflow file here would show this prototype's numbers on macOS.

## Windows

- **No headless renderer.** `current_headless_renderer` returns `Ok(None)` on Windows, in
  gpui-pre-platform 0.3.12 and 0.3.14 alike. gpui-kit's CI skips its rendering target there
  ("Windows has no headless renderer").
- **There is a real-window path.** `gpui_windows`' Direct3D 11 renderer implements
  `render_to_image` under `test-support`: it draws the scene into the window's render target
  without presenting, copies it to a staging texture, and reads it back.
  - Its own comment calls this "the Windows analogue" of the macOS path, so the window "need
    never be shown".
  - `src/bin/capture.rs` uses it: `gpui_kit::application()`, a window opened with
    `show: false`, one drawn frame, then `Window::render_to_image()`.
  - That's the real platform (real DirectWrite text, real scheduler), not the deterministic
    test platform.
- **Compile status:** the binary compiles on Linux, where `render_to_image` reports "not
  implemented for this platform" on X11 and headless, as the source says. It was not
  compiled for or run on Windows here.
- **The workflow** runs it 3 times on `windows-latest` and compares the captures. GitHub's
  Windows runners have no GPU, so Direct3D would fall back to WARP (Microsoft Basic Render
  Driver); unverified.
- **Interaction tests** (TestPlatform) are platform-independent and should pass on Windows,
  as gpui-kit's do in its Windows CI job.

## How far apart text rasterizers are

GPUI rasterizes glyphs with each system's text stack:
- **Linux:** cosmic-text with swash, in `gpui_wgpu::CosmicTextSystem`;
- **macOS:** Core Text, in `gpui_macos`;
- **Windows:** DirectWrite, in `gpui_windows::direct_write`.

Each has its own anti-aliasing, gamma, hinting and subpixel positioning.

**Measured here as a proxy:** the English line from the 2× Linux golden, against the same
string, font (Gentium Plus) and pixel size (32 px) drawn by FreeType (ImageMagick 7.1.2,
FreeType 2.14.2). Pictures are in `evidence/`.

| Crop | Pixels off by more than 2 levels |
|---|---|
| The stem of "I" (8×32) | 6 of 256 (2.3 %) |
| "In" (30×32) | 137 of 960 (14 %) |
| The whole line (722×32) | 7,866 of 23,104 (34 %); the lines are also 722 and 734 px wide |

- **ADR 0019's tolerance** allows 0.01 % of a 960×520 window, which is 49 pixels. A single
  word drawn by another rasterizer exceeds it.
- **Not measured:** Core Text and DirectWrite themselves. The workflow's `cross-os` job
  compares macOS and Windows frames with the Linux golden when the branch is pushed. Expect
  thousands of pixels off.
- **GPU against lavapipe** on the same OS is unmeasured. Glyphs are rasterized on the CPU, so
  the atlas is identical. Quads, rounded corners, shadows and blending run in shaders, where
  floating-point results may differ by a level or two between drivers. That's the case the
  ±2 tolerance exists for.

## Recommendation

**For #379 (the test driver):**
- **Build it on gpui-kit's test API.** `#[gpui_kit::test]`, `TestWindowExt`, `TestEventExt`,
  `TestQueryExt` and `TestInput` already cover everything `ui-egui`'s harness does today:
  commands, pointer and keyboard input, IME, finding controls by label or role, and focus.
  Its frames run headless on all three OSes.
- **Give every control an id and `.test_support()`**, and put the label on the element
  that owns the semantics (for an Input, through `InputAccessibility`).
- **Checks on the accessibility tree's shape** (one AccessKit paragraph node per laid-out
  block with link children, #19) need GPUI to let a test activate accessibility. A small
  upstream change: a `TestWindow` that calls its activation callback, or an
  `accessibility_force_enabled` switch beside the existing `accessibility_force_disabled`.
  Until then, check per-element snapshots.

**For goldens (amends ADR 0019):**
1. **Linux goldens are the gallery.** Draw them headlessly on CI with Mesa lavapipe, through
   `HeadlessAppContext` and `current_headless_renderer`, in 3 Looks at 1× and 2×.
   - Keep ADR 0019's tolerance: ±2 per channel, at most 0.01 % of pixels.
   - On lavapipe the frames are byte-identical, so the tolerance only matters if a runner's
     Mesa version changes. Pin the CI image, and re-bless deliberately when Mesa is upgraded.
2. **No committed macOS or Windows goldens.** Their text rasterizers differ from Linux's by
   far more than the tolerance, so shared goldens are impossible. Three more golden sets
   (about 145 MB each at today's size) would triple the repository's weight for little gain.
   Instead, each OS gets **smoke screenshots**:
   - macOS: headless Metal; Windows: a hidden real window.
   - Draw a fixed handful of surfaces (the reading pane in Hebrew, Greek and English, the
     Workspace, a dialog). Check each is deterministic (3 captures, identical) and not blank,
     and keep the images as CI artifacts for the owner's review (ADR 0020).
   - The interaction tests run on all three OSes, as today.
3. **Upstream two small GPUI changes** before #379 builds on them: a scale setter on
   `HeadlessAppContext` (the patch here), and a way to activate accessibility in test
   windows. If either is refused, carry it as a `[patch]` on a vendored gpui-pre, which the
   pinned-version policy from #373 makes cheap.

## Draft ADR text

As an amendment at the end of `docs/adr/0019-screenshot-goldens-are-png-files-in-the-repository-compared-with-a-tolerance.md`:

> ## Amendment (#374): GPUI goldens are drawn on Linux; macOS and Windows get smoke screenshots
>
> The desktop UI moves to GPUI (ADR 0025), which rasterizes text with each system's own text
> stack: cosmic-text on Linux, Core Text on macOS, DirectWrite on Windows. A word drawn by
> another rasterizer differs from a golden by far more than rule 3's tolerance (14 % of the
> pixels of "In" between swash and FreeType, measured in #374), so one set of goldens can no
> longer serve all three systems as rule 2 says. A prototype (#374) drew a GPUI surface with
> GPUI's headless renderer on Mesa's software Vulkan driver (lavapipe), with no display and
> no GPU. Three fresh captures were byte-identical, at 1× and 2× and in light and dark, and
> one changed label moved 198 pixels. For the GPUI UI:
>
> 1. **The goldens are Linux frames.** They are drawn by GPUI's headless renderer (`wgpu`
>    through `HeadlessAppContext`) on Mesa lavapipe, on the CI image's pinned Mesa, with the
>    bundled fonts and the platform text system. They replace rule 2's CPU tessellation; the
>    files, the gallery page, `--bless` and rule 4's fixtures stay as they are.
> 2. **The tolerance is unchanged:** rule 3, ±2 per channel and at most 0.01 % of pixels.
>    Upgrading Mesa on CI is a deliberate re-bless, reviewed like any other (ADR 0020).
> 3. **macOS and Windows have no goldens.** CI draws a fixed set of smoke surfaces on each:
>    - macOS: headless Metal; Windows: a never-shown window read back from Direct3D, as GPUI
>      has no headless renderer there;
>    - each surface is captured three times and must be identical and not blank;
>    - the images are kept as CI artifacts for review.
>
>    Interaction and accessibility tests run on all three systems.
> 4. **The test contexts need two small GPUI changes,** carried as a `[patch]` on the pinned
>    GPUI until they are merged upstream: setting a headless window's scale (for 1×), and
>    letting a test window activate accessibility, so the AccessKit tree's shape can be
>    checked and not only each node.
>
> Considered: committed goldens per system (three times the repository weight, and the macOS
> and Windows ones could only be blessed on those runners); goldens drawn on a GPU runner
> (not deterministic across driver versions, and hosted runners have no GPU); and keeping
> egui-style CPU rasterization for the gallery (it would test a renderer the app no longer
> ships).

## Not confirmed

- The prototype's numbers on macOS and Windows runners: the workflow is ready but not pushed.
- Whether Windows' hidden-window capture works on GitHub's runners (WARP), and whether it is
  deterministic.
- How far lavapipe frames are from a hardware GPU's on Linux.
- The exact difference Core Text and DirectWrite make against the Linux golden: the
  `cross-os` job measures it.
