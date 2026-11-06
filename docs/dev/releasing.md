# Releasing (#184)

Pushing a `v*` tag runs the **Release** workflow ([`.github/workflows/release.yml`](../../.github/workflows/release.yml)). It builds the app on Linux, macOS and Windows, packs each build with the Starter library into a **Release archive**, checks that the Linux archive launches with its Starter library, and publishes everything to a GitHub release. The app keeps its working name (`openbible`, "Open Bible Software") until the product is named (#1).

## What a release publishes

| Asset | From | Used by |
|---|---|---|
| `openbible-<tag>-x86_64-unknown-linux-gnu.tar.gz` | `build` (ubuntu-latest) + `package` | Linux users |
| `openbible-<tag>-aarch64-apple-darwin.tar.gz` | `build` (macos-latest, Apple silicon) + `package` | macOS users |
| `openbible-<tag>-x86_64-pc-windows-msvc.zip` | `build` (windows-latest) + `package` | Windows users |
| `<id>.obr` for every Starter Resource, `starter-manifest.json` | the `starter` workflow | **Download Starter library** (`releases/latest/download/`, [Starter library](starter-library.md)) |
| `CHANGES-<id>.md` for every CC BY source | the `starter` workflow | the About Tab's change record link ([Credits](credits.md)) |
| `starter-build-report.md` | the `starter` workflow | reviewers; also the release notes |
| `SHA256SUMS` | `package` | checking a download by hand |

The release notes are a short header and the Starter build report.

## What is in a Release archive

One folder, named like the archive:

```text
openbible-<tag>-<target>/
  openbible            (openbible.exe on Windows; mode 0755 in the .tar.gz)
  LICENSE-APACHE
  LICENSE-MIT
  starter/             the whole of dist/starter/: starter-manifest.json, every .obr,
                       search/, aliases/, CHANGES-<id>.md, starter-build-report.md
```

`starter/` beside the executable is the second place the app looks for a bundled Starter library ([Starter library](starter-library.md#finding-the-bundle)), so an unpacked archive needs no setup: on the first launch the app copies every Starter Resource, its prebuilt Search index caches and alias caches into the user's Library. The archive does not depend on where it is unpacked.

`cargo xtask release-archive` writes it. Before packing it checks that `starter-manifest.json` parses and that every `.obr` it lists is present with the `file_sha256` it names, so a broken artifact never ships.

## How the workflow fits together

| Job | Runs on | Does |
|---|---|---|
| `starter` | ubuntu-latest | Calls [`starter.yml`](../../.github/workflows/starter.yml) (`workflow_call`): `cargo xtask starter --check-determinism`, diffed against the previous release's manifest, uploaded as the `starter-library` artifact. The Starter library is built once and reused, never rebuilt per platform. |
| `build` | ubuntu, macos, windows | `cargo build --release --locked -p openbible`, uploaded as `app-<target>`. Runs alongside `starter`. |
| `package` | ubuntu-latest | Downloads `starter-library` and the three `app-*` artifacts; `cargo xtask release-archive` for each target; `xvfb-run cargo xtask release-smoke` on the Linux archive; copies the Starter assets beside the archives, writes `SHA256SUMS`, and uploads them all as the `release-assets` artifact (plus `release-smoke-screenshot`). |
| `publish` | ubuntu-latest | Only for a pushed `refs/tags/v*` (`github.event_name == 'push'`); a `workflow_dispatch` on a tag stays a dry run. Downloads `release-assets` and creates the GitHub release (`--verify-tag`), or uploads with `--clobber` if the release already exists. The only job with `contents: write`. |

The `starter` workflow no longer runs on tags or publishes anything itself: it still runs on pull requests that touch the lock or the pipeline, and the `release` workflow calls it.

### The launch check

`cargo xtask release-smoke <archive>` unpacks the archive into `target/release-smoke/`, then runs its `openbible` with every data path in a fresh folder (`OBS_LIBRARY`, `OBS_USER_DB`, the Search and alias caches, the SWORD folders, `HOME` and `XDG_*`), no `OBS_STARTER_DIR`, and a Starter download source that does not exist, so the Library can only come from `starter/` beside the binary. `OBS_SHOT` makes the app save its 60th frame and close. The check fails unless the app exited cleanly, the screenshot has a size, and every Resource in the archive's `starter-manifest.json` is in the new Library byte for byte with each of its prebuilt Search index caches. In CI it runs on Linux under `xvfb-run`; it works on a desktop with a display too:

```sh
cargo xtask starter                                   # or download the starter-library artifact into dist/starter
cargo build --release -p openbible
cargo xtask release-archive --bin target/release/openbible --name openbible-dev-local
cargo xtask release-smoke dist/release/openbible-dev-local.tar.gz --shot /tmp/first-launch.ppm
```

## Dry runs

**Actions → Release → Run workflow** (`gh workflow run release.yml --ref <branch>`) runs everything except `publish`, even when `--ref` is a `v*` tag (only the `push` event publishes); the archives are named `openbible-dev-<sha7>-<target>` and land in the run's `release-assets` artifact with the Starter assets. Pull requests that change `release.yml`, `starter.yml` or `xtask/src/release.rs` run the same dry run.

## Cutting a release

1. Make sure `master` is green, including a Release dry run on it.
2. Bump `version` in the workspace `Cargo.toml` if it should match the tag.
3. `git tag -a vX.Y.Z -m "vX.Y.Z" && git push origin vX.Y.Z` (a maintainer does this; the release workflow publishes from the tag).
4. Check the release page: three archives, every `.obr`, `starter-manifest.json`, the `CHANGES-*.md`, `starter-build-report.md` and `SHA256SUMS`. `releases/latest/download/starter-manifest.json` now resolves, so **Download Starter library** works for source builds.

A tag that fails part way can be re-run (`gh run rerun`); `publish` uploads over an existing release with `--clobber`. The next release's `starter` job diffs against this release's `starter-manifest.json`, so a removed text-unit id fails the build (#20 decision 2).

## Not yet

These wait for the packaging decisions in #1 (marked `TODO(#1)` in the workflow and in `xtask/src/release.rs`):

- **Signing.** No code signing on any platform. macOS Gatekeeper refuses an unsigned, quarantined binary downloaded by a browser: remove the quarantine with `xattr -dr com.apple.quarantine openbible-<tag>-aarch64-apple-darwin` or open it with right-click → Open. Windows SmartScreen warns on first run.
- **Notarization** for macOS, and an `.app` bundle (`Contents/Resources/starter/` is already a place the app looks).
- **Installers**: msi, dmg, AppImage/deb. They must bundle the whole of `dist/starter/` ([ADR 0007](../adr/0007-starter-library-is-copied-into-the-library-from-a-bundle-or-the-release.md)).
- **More targets**: Intel macOS (`x86_64-apple-darwin`), Linux and Windows on ARM.
