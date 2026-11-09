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
| `SHA256SUMS` | `package` | Update now (#276) checks the archive against it before installing; checking a download by hand |
| `SHA256SUMS.minisig` | `package`, only once the `MINISIGN_SECRET_KEY` secret exists | Update now checks it with the public key compiled into the app ([Release signing](#release-signing)) |

The release notes are a short header and the Starter build report.

## What is in a Release archive

One folder, named like the archive:

```text
openbible-<tag>-<target>/
  openbible            (openbible.exe on Windows; mode 0755 in the .tar.gz)
  LICENSE-APACHE
  LICENSE-MIT
  openbible-install.json   {"kind": "archive"}: Update now may replace this folder in place (#276)
  starter/             the whole of dist/starter/: starter-manifest.json, every .obr,
                       search/, aliases/, CHANGES-<id>.md, starter-build-report.md
```

`starter/` beside the executable is the second place the app looks for a bundled Starter library ([Starter library](starter-library.md#finding-the-bundle)), so an unpacked archive needs no setup: on the first launch the app copies every Starter Resource, its prebuilt Search index caches and alias caches into the user's Library. The archive does not depend on where it is unpacked.

`cargo xtask release-archive` writes it. An unpacked archive updates itself: Help → Check for updates (and, with the setting on, every launch) asks the GitHub releases API for the latest release, and Update now downloads the archive for its platform, checks it against `SHA256SUMS`, backs up the User database and renames the new files into place ([ADR 0017](../adr/0017-the-app-may-ask-github-for-its-latest-release-and-replace-its-own-release-archive.md), [Update check and Update now](updates.md)). So the archive names must stay `openbible-<tag>-<target>.<tar.gz|zip>`.

Before packing it checks that `starter-manifest.json` parses and that every `.obr` it lists is present with the `file_sha256` it names, so a broken artifact never ships.

## How the workflow fits together

| Job | Runs on | Does |
|---|---|---|
| `starter` | ubuntu-latest | Calls [`starter.yml`](../../.github/workflows/starter.yml) (`workflow_call`): `cargo xtask starter --check-determinism`, diffed against the previous release's manifest, uploaded as the `starter-library` artifact. The Starter library is built once and reused, never rebuilt per platform. |
| `build` | ubuntu, macos, windows | `cargo build --release --locked -p openbible`, uploaded as `app-<target>`. Runs alongside `starter`. |
| `package` | ubuntu-latest | For a pushed tag, fails unless the tag is `v` + the workspace version (the app compares release tags with its own version). Downloads `starter-library` and the three `app-*` artifacts; `cargo xtask release-archive` for each target; `xvfb-run cargo xtask release-smoke` on the Linux archive; copies the Starter assets beside the archives, writes `SHA256SUMS`, signs it when the signing secret exists, and uploads them all as the `release-assets` artifact (plus `release-smoke-screenshot`). |
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

### The update check end to end

`cargo xtask update-e2e` (CI job `update-e2e` on all three OSes; Linux under `xvfb-run`) builds the app as this version and as the next patch version (`OBS_BUILD_VERSION`), packs both as Release archives with a one-Bible Starter library, installs the first, serves the second from a mock release source on localhost (`OBS_UPDATE_URL`), and runs Check for updates, Update now and Restart now. It passes when the next version runs from the same folder with the User database and the Library intact and a Before-update backup taken. See [Update check and Update now](updates.md).

## Release signing

The app verifies a detached [minisign](https://jedisct1.github.io/minisign/) signature of `SHA256SUMS` before it trusts any checksum in it, with the public key compiled into it ([ADR 0017](../adr/0017-the-app-may-ask-github-for-its-latest-release-and-replace-its-own-release-archive.md)). Until the key exists, releases have `SHA256SUMS` only, the app checks that alone, and the update notice says "Release signing is not set up yet". To set it up (a maintainer, once, on a trusted computer; minisign 0.11 or later: `brew install minisign`, `apt install minisign` or `winget install minisign`):

```sh
# 1. The key pair. -W: no password, because the secret key lives in an Actions secret; keep
#    the file itself offline (a password manager), never in the repository.
minisign -G -W -p openbible-release.pub -s openbible-release.key

# 2. The secret key for the release workflow (it signs SHA256SUMS in the `package` job).
gh secret set MINISIGN_SECRET_KEY -R inhesrom/openbiblesoftware < openbible-release.key

# 3. The public key into the app: the second line of openbible-release.pub (it starts `RW`)
#    becomes RELEASE_PUBLIC_KEY in crates/engine/src/updates.rs:
#      pub const RELEASE_PUBLIC_KEY: Option<&str> = Some("RW...");
#    Update the test that asserts it is None (updates_tests.rs), the update notice's
#    CHECKSUM_NOTE and the ADR's "until then", and commit the public key (it is public).
tail -n 1 openbible-release.pub

# 4. Check a signed release by hand.
minisign -Vm SHA256SUMS -p openbible-release.pub
```

The first release after step 3 must be signed: an app with the key refuses an unsigned `SHA256SUMS`. Apps released before it keep checking checksums only, so publish the key in a release before relying on it. Rotating the key means a release signed with the old key that carries the new public key.

A pull request from a fork has no secrets, so its dry run skips the signing step; so does every run until the secret exists.

## Dry runs

**Actions → Release → Run workflow** (`gh workflow run release.yml --ref <branch>`) runs everything except `publish`, even when `--ref` is a `v*` tag (only the `push` event publishes); the archives are named `openbible-dev-<sha7>-<target>` and land in the run's `release-assets` artifact with the Starter assets. Pull requests that change `release.yml`, `starter.yml` or `xtask/src/release.rs` run the same dry run.

## Cutting a release

1. Make sure `master` is green, including a Release dry run on it.
2. Bump `version` in the workspace `Cargo.toml` to the tag's version: the `package` job fails a tag that does not match it, since the app compares release tags with its own version.
3. `git tag -a vX.Y.Z -m "vX.Y.Z" && git push origin vX.Y.Z` (a maintainer does this; the release workflow publishes from the tag).
4. Check the release page: three archives, every `.obr`, `starter-manifest.json`, the `CHANGES-*.md`, `starter-build-report.md` and `SHA256SUMS`. `releases/latest/download/starter-manifest.json` now resolves, so **Download Starter library** works for source builds.

A tag that fails part way can be re-run (`gh run rerun`); `publish` uploads over an existing release with `--clobber`. The next release's `starter` job diffs against this release's `starter-manifest.json`, so a removed text-unit id fails the build (#20 decision 2).

## Not yet

These wait for the packaging decisions in #1 (marked `TODO(#1)` in the workflow and in `xtask/src/release.rs`):

- **Code signing.** No code signing on any platform (release signing of `SHA256SUMS` is [above](#release-signing)). macOS Gatekeeper refuses an unsigned, quarantined binary downloaded by a browser: remove the quarantine with `xattr -dr com.apple.quarantine openbible-<tag>-aarch64-apple-darwin` or open it with right-click → Open. Windows SmartScreen warns on first run.
- **Notarization** for macOS, and an `.app` bundle (`Contents/Resources/starter/` is already a place the app looks).
- **Installers**: msi, dmg, AppImage/deb. They must bundle the whole of `dist/starter/` ([ADR 0007](../adr/0007-starter-library-is-copied-into-the-library-from-a-bundle-or-the-release.md)). Each becomes a new `kind` of the install marker that Update now knows how to replace (a whole `.app` bundle, a quiet `msiexec`, the AppImage file; [ADR 0017](../adr/0017-the-app-may-ask-github-for-its-latest-release-and-replace-its-own-release-archive.md)); until then Update now offers only the download page for them.
- **More targets**: Intel macOS (`x86_64-apple-darwin`), Linux and Windows on ARM.
