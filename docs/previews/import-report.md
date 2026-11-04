# Import report visual evidence (#47)

Real `openbible` windows, captured on macOS with the app's `OBS_SHOT` framebuffer
callback (1280 × 800), plus two frames rasterized by the headless shell test harness
(1400 × 900). Only conversion was PPM → PNG with ffmpeg; no compositing or retouching.
**No screen recording (no recorder on this host).**

| Preview | State checked |
| --- | --- |
| [Finished report (real window)](import-report.png) | `OBS_SCRIPT="10 import.start {path: .preview-data/WEB}"`: the five-file WEB test folder imported through the engine job. Parse complete, Search index deferred to #58, summary with Unmapped/Absent counts, licence warning, and the Dropped markup / Warnings / Source files sections above Open · About · Remove import…. |
| [About after relaunch (real window)](import-about-relaunched.png) | The app exited and launched again with the same User database. About · WEB-1 shows Origin "Imported", the USFM source folder, and the **Latest Import report** action; the report Tab itself was restored with the Last session. |
| [In-progress report (harness)](import-report-in-progress-kittest.png) | Frame rasterized while the worker is held inside the Index phase: Parse complete, `Search index: 1 / 3` live from the worker's own progress events. The test around it asserts twenty paced frames each complete well under the budget and the Library menu still answers. |
| [Finished report (harness)](import-report-kittest.png) | The same report with Dropped markup (`\ip ×1`) and Source files expanded by clicks. |

The headless shell harness drives Library → Import…, hands the chooser a path the way
the `rfd` thread does, and checks phases, counts, warnings, expansion, Open, About,
Latest Import report and Remove import. These real-window previews complement those
assertions; they are not kittest image-diff baselines.

## Fixtures

An isolated `.preview-data/library/` and `.preview-data/user.db` were used, never the
Mac user's library or User database. These are tiny test Resources, not new Starter
sources or release artifacts:

- KJV: `obs-cli write-sample .preview-data/library/kjv.obr` (the repository's public-domain
  `store::sample_resource`, unchanged).
- WEB: `.preview-data/WEB/` is a copy of `crates/formats/tests/assets/usfm/web/` (five
  existing USFM test fixtures), imported from the running app.
