# Screenshot goldens are PNG files in the repository, drawn headlessly and compared with a tolerance

Amended by #374 (below): in the GPUI UI (ADR 0025) the goldens are drawn on Linux by GPUI's headless renderer, and macOS and Windows get smoke screenshots instead of goldens.

The design-token pass (#221) changed the look of every surface at once, and nothing would have
said so if a later change moved a colour or a spacing by accident. The Screenshot gallery (#222)
draws every Tab kind, toolbar pane, menu, window, dialog and popover in Light, Dark and Sepia at
1× and 2× and compares each image with a golden. We decided:

1. **The goldens are lossless PNG files committed under `docs/previews/gallery/`**, one per
   surface × Look × scale (102 surfaces, 612 images, about 145 MB at the default 1400 × 900-point
   window), and
   `docs/previews/gallery.md` shows them all on one page. `cargo xtask gallery --bless` writes
   only the goldens that are new or off beyond the tolerance, so a re-bless commits what
   changed and nothing else.
2. **They are drawn by the headless harness's own rasterizer**, not a GPU: the shell's real
   frames tessellated by egui and filled on the CPU, premultiplied alpha, nearest texels, with
   the bundled fonts only. The same code runs on Linux, macOS and Windows, so CI compares every
   platform with the same goldens.
3. **A small tolerance absorbs rounding, not changes**: a pixel matches when no channel is more
   than 2 levels off, and an image matches when no more than 0.01 % of its pixels (126 at 1×)
   differ beyond that. A colour token changed by a few levels moves thousands of pixels.
4. **The fixtures are deterministic**: the accessibility audit's Starter subset plus the BSB's
   Genesis 1, a fresh launch for each surface and Look, and a fixed clock. The clock is
   `obs_store::clock`: the system clock, or with the `test-support` feature a time fixed for
   one thread, under which local time is UTC. The User database stamps and shows times by it,
   and the engine reads it for today.

## Considered Options

- **Git LFS for the goldens:** keeps clones small, but every CI checkout on three platforms
  would download them against GitHub's LFS bandwidth quota, and contributors need LFS set up.
- **Hashes instead of images:** tiny, but a hash cannot carry a tolerance across platforms and a
  failure cannot show what changed.
- **`egui_kittest` snapshots through wgpu:** a GPU or a software adapter on every CI runner, a
  large new dependency, and images that differ by driver; our harness already rasterizes for the
  preview images of earlier PRs.
- **Fewer images (one scale, or Sepia only where a page shows):** smaller, but 2× is where
  glyph and icon rasterization differ, and the default layout shows a Resource page behind every
  surface.

## Consequences

- The repository grows by about 145 MB, and by the size of the changed images each time goldens
  are re-blessed. A change that touches a colour used everywhere re-blesses most of them.
- A pull request that changes the look must bless and commit the goldens. Two such pull
  requests conflict on the same golden; the second rebases and blesses again.
- Something that differs by computer cannot be in the gallery as it is: the Import report
  shows the folder imported, so the gallery imports from a relative path; Report a problem
  shows the operating system and the log, so it is left out; so is a font family's style
  chooser, shown only for installed fonts.
- The app version is drawn in About Open Bible and Credits, so a version bump re-blesses them.
- The fixed clock is a thread-local behind `test-support`; a job run on a worker thread reads
  the system clock, so the gallery runs its jobs inline.

## Amendment (#374): GPUI goldens are drawn on Linux; macOS and Windows get smoke screenshots

The desktop UI moves to GPUI (ADR 0025). GPUI rasterizes text with each system's own text
stack: cosmic-text on Linux, Core Text on macOS and DirectWrite on Windows. So one set of
goldens can no longer serve all three systems, as rule 2 says. A throwaway prototype (#374,
branch `prototype/374-gpui-headless`) drew one GPUI surface with `gpui-kit` 0.10 on Linux,
on GitHub's Linux, macOS and Windows runners:
- **Linux:** GPUI's headless renderer runs on Mesa's software Vulkan driver (lavapipe), with
  no display and no GPU. Three fresh captures were byte-identical at 1× and 2×, in light and
  dark. Changing one label moved 198 pixels, well over rule 3's tolerance.
- **macOS:** headless Metal frames on the hosted runner were byte-identical run to run. The
  same surface was 4.1 % of its pixels away from the Linux golden; the tolerance allows
  0.01 %.
- **Windows:** GPUI has no headless renderer. Reading back a window that was never shown gave
  a 1 × 1 image, so Windows has no usable capture yet.
- **Interaction tests passed on all three systems:** clicks, typing, IME composition, and
  finding controls by their accessible label and role.

For the GPUI UI:

1. **The goldens are Linux frames.** GPUI's headless renderer draws them on Mesa lavapipe,
   on the CI image's pinned Mesa, with the bundled fonts. They replace rule 2's CPU
   tessellation. The files, the gallery page, `--bless` and rule 4's fixtures stay as they
   are.
2. **The tolerance is unchanged.** Rule 3 still applies: ±2 per channel, and at most 0.01 % of
   the pixels. Upgrading Mesa on CI means a deliberate re-bless, reviewed like any other
   change (ADR 0020).
3. **macOS and Windows have no goldens.**
   - **macOS:** CI draws a fixed set of smoke surfaces with headless Metal. Each surface is
     captured three times and must be identical and not blank. The images are kept as CI
     artifacts for review.
   - **Windows:** runs the interaction and accessibility tests only, until a capture works.
     #379 tries a window shown off-screen.
   - Interaction and accessibility tests run on all three systems.
4. **Two small GPUI changes for the test contexts**, carried as a `[patch]` on the pinned
   GPUI until they are merged upstream:
   - setting a headless window's scale, since gpui-pre 0.3.12 draws headless windows at 2×
     only;
   - letting a test window turn accessibility on, so tests can check the shape of the
     AccessKit tree and not only each node.

### Considered options

- **Committed goldens per system:** three times the repository weight. The macOS goldens
  could be blessed only on macOS runners, and Windows has none to bless.
- **Goldens drawn on a GPU runner:** frames vary with the driver version, and hosted runners
  have no GPU.
- **Keeping egui-style CPU rasterization for the gallery:** it would test a renderer the app
  no longer ships.

