# Screenshot goldens are PNG files in the repository, drawn headlessly and compared with a tolerance

The design-token pass (#221) changed the look of every surface at once, and nothing would have
said so if a later change moved a colour or a spacing by accident. The Screenshot gallery (#222)
draws every Tab kind, toolbar pane, menu, window, dialog and popover in Light, Dark and Sepia at
1× and 2× and compares each image with a golden. We decided:

1. **The goldens are lossless PNG files committed under `docs/previews/gallery/`**, one per
   surface × Look × scale (100 surfaces, 600 images, about 140 MB at the default 1400 × 900-point
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

- The repository grows by about 140 MB, and by the size of the changed images each time goldens
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
