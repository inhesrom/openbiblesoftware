# Release archive visual evidence (#184)

A real `openbible` window, captured on macOS (Apple silicon) by `cargo xtask release-smoke`
with the app's `OBS_SHOT` framebuffer callback (1280 × 800). Only conversion was PPM → PNG
with ffmpeg; no compositing or retouching. **No screen recording (no recorder on this host).**

| Preview | State checked |
| --- | --- |
| [First launch of an unpacked Release archive](release-archive-first-launch.png) | `cargo xtask release-archive --bin target/release/openbible --name openbible-dev-local-aarch64-apple-darwin`, then `cargo xtask release-smoke` on that `.tar.gz`: unpacked into a fresh folder and launched with an empty Library and User database, no `OBS_STARTER_DIR` and an unreachable download source. The Library Tab lists the Starter Resources copied from `starter/` beside the binary; the check reported "19 Starter Resources and 19 prebuilt Search index caches in the Library". |

The Starter library inside the archive is the `starter-library` artifact of a `starter`
workflow run on `master` (the same artifact the Release workflow packs), not a new build.
The Linux archive gets the same check in CI under `xvfb-run` (`release` workflow, `package` job).
