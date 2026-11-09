# Update check and Update now (#276)

The decision and its limits are [ADR 0017](../adr/0017-the-app-may-ask-github-for-its-latest-release-and-replace-its-own-release-archive.md). This page is where the code is and how to test it.

| Crate | File | What |
|---|---|---|
| `obs-engine` | `updates.rs` | `Version` (semver), `parse_releases`, `newest_release`, `verify_archive` (SHA256SUMS and its minisign signature), `Install` and the install marker, `stage_update` / `swap_in` / `clean_up_update_folder`, the Session's `Updates` (settings, status, jobs) and the commands below |
| `obs-engine` | `testing.rs` (feature `test-support`) | `MockReleases`, an in-memory release source that records every request and its thread and can hold requests like a slow network; `MockRelease::with_archive`, `release_archive_bytes`, `installed_archive` |
| `obs-store` | `backup_store.rs` | `UserStore::update_backup`: the Before-update backup (`user-update-000001.db` in the backups folder, the last five kept) |
| `obs-ui-egui` | `shell/update_notice.rs` | the launch check's clock, the update notice, the first-run note, Restart now (`Relauncher`) |
| `obs-ui-egui` | `lib.rs` | `App::with_update_checks`: the release source (`OBS_UPDATE_URL`), this executable's `Install`, removing what the last update replaced, arming the launch check |
| `openbible` | `fetch.rs` | the network transport: GitHub API headers, rate limits (`FetchError::RateLimited`: HTTP 429, or 403 with `x-ratelimit-remaining: 0`) |
| `xtask` | `release.rs` | writes the install marker `openbible-install.json` into every Release archive |
| `xtask` | `update_e2e.rs` | `cargo xtask update-e2e` |

## Commands

| Command | Does |
|---|---|
| `help.check_for_updates` | Help → Check for updates…: check now and show the answer, even "No update available" |
| `update.install` | Update now (from a script before the check has answered, it waits for it) |
| `update.cancel` | Cancel the download |
| `update.later` | Later / Close: hide the notice (a release on offer stays known) |
| `update.skip` | Skip this version: remembered in the User database (preference `updates`) |
| `update.restart` | Restart now (from a script before the install has finished, it waits for it) |
| `updates.auto_check {on}` | Settings → Updates → Check for updates automatically (default on) |
| `updates.prereleases {on}` | Settings → Updates → Include pre-releases (default off) |
| `updates.explained` | the first-run note's OK |

## Flow

1. `Shell::arm_launch_update_check` (the desktop app only; tests and the bench never arm it). The first frame starts a clock; `LAUNCH_CHECK_DELAY_SECS` (3 s) later `Session::launch_update_check` submits one `Update check` job, if the setting is on. It runs once per launch.
2. The job GETs `{releases}/latest` (or `{releases}?per_page=20` with pre-releases) through the app's `Fetch` and picks `newest_release`. A launch check that fails is logged and stays quiet; a manual one shows the failure.
3. Update now (`Session::start_app_update`) checks `Install::check_updatable` (a Release archive, a published archive for this target, a writable folder) and that every asset URL is under the source's download prefix, then submits an `Update download` job: `SHA256SUMS` (and `SHA256SUMS.minisig` when a key is compiled in), the archive with progress, `verify_archive`, `stage_update` into `.openbible-update/new`.
4. On the UI thread, `install_staged` takes the Before-update backup, then `swap_in` renames the new entries into place (old ones into `.openbible-update/old`, rolled back on failure). `Effect::Restart(exe)` starts the new executable (`Relauncher`) and closes the window the usual way.
5. The next launch's `clean_up_update_folder_at_launch` (on a thread of its own) removes `.openbible-update`. Restart now starts it before the old version has closed, and on Windows the old executable cannot be deleted until then, so it tries again with backoff for up to `CLEAN_UP_PATIENCE` (30 s). Its retries remove only `old/`, never an update being unpacked into `new/`; a lock keeps them apart from `stage_update` and `swap_in`.

## Environment

| Variable | Effect |
|---|---|
| `OBS_UPDATE_URL=<base>` | a mock release source: `<base>/releases/latest`, `<base>/releases?per_page=20`, assets under `<base>/download/`, download page `<base>/releases`. The compiled-in public key still applies. |
| `OBS_UPDATE_PUBLIC_KEY=<base64>` | debug builds only: another release key, for a test that signs its mock release |
| `OBS_BUILD_VERSION=<semver>` | at build time: the version the app reports (`obs_engine::updates::APP_VERSION`), for `update-e2e` |
| `OBS_STEPS=<json>` | automation: steps that wait until a text or button is on screen (found in the frame's accessibility tree), then take a screenshot (`shot`) and/or click a button by its label (`click`, pointer events at its centre). Fields: `when`, `click`, `after_file`, `after_secs`, `skip_if`, `shot`. |
| `OBS_RELAUNCH_<NAME>` | Restart now starts the new version with this as `OBS_<NAME>`; this process's own `OBS_SCRIPT`, `OBS_INPUT`, `OBS_SHOT`, `OBS_SHOT_FRAME` and `OBS_STEPS` are never passed on |

`openbible --version` prints the version; the log's first line has it and the process id.

## Tests

- `crates/engine/src/updates_tests.rs`: semver order, drafts and pre-releases, the exact request, automatic checks off (no request) and on (one, on a worker), Skip this version across launches, offline / rate-limited / no release, Update now replacing the install and nothing else (Library files and User database settings compared byte for byte), a tampered archive, missing checksums, foreign asset URLs, Cancel, no permission, not an archive install, never a downgrade, path traversal in an archive, the minisign fixtures in `crates/engine/tests/update-signing/` (a throwaway key; its secret half was deleted).
- `crates/ui-egui/src/update_notice_tests.rs`: the launch check's timing on a worker with a held request, automatic checks off, the notice and its release notes, Skip this version and the first-run note across launches, the progress bar, Restart now, the offline and rate-limited messages with Open the download page, a read-only folder. `UPDATE_PREVIEWS=1` writes their screenshots to `target/previews/`.
- `crates/store/tests/user_backup.rs`: Before-update backups are kept, listed and readable.
- `apps/openbible/src/fetch.rs`: rate limits and the request's headers, against a server on localhost.
- `cargo xtask update-e2e` (CI job `update-e2e` on Linux under `xvfb-run`, macOS and Windows): the real app, vN installed from a Release archive, vN+1 served from localhost (the archive slowed to about 8 s so its progress shows). vN's own launch check opens the notice; `OBS_STEPS` clicks **Update now** and **Restart now** with pointer events at the buttons. When the swap is logged, while vN still runs, it checks that vN's executable was renamed aside into `.openbible-update/old/` and vN+1 put in its place, and only then lets Restart now go. vN+1 must start from the same folder, remove `.openbible-update` (the old executable with it), run its own check and show its version in Help → About; the User database and the Library must be intact. Screenshots (`1-update-available`, `2-downloading`, `3-update-installed`, `4-restarted`) are written as PNG to `target/update-e2e/shots/` and uploaded as the job's artifact.
- `cargo xtask update-e2e --real-release X.Y.Z` (workflow `Update real release`, on demand: run it from the Actions tab or label a pull request `update-real-release`): this tree built as 0.0.9, with no release key, updates itself from the real published release on GitHub. It checks the log for the asset picked for this platform and its match in the release's `SHA256SUMS`, the rename-aside swap and the release's Starter library in place, then the release's own start line after Restart now (0.1.0 has no `--version`, and logs no process id); the release closes itself after its `OBS_SHOT` screenshot. Because the 0.0.9 build is this tree, its User database is newer than 0.1.0 reads, and 0.1.0 says so instead of opening it.

To try it by hand: run `cargo xtask update-e2e` once (it leaves both archives in `target/update-e2e/dist/`), serve the newer one with a `releases/latest` JSON in the layout above (`python3 -m http.server` will do), unpack the older one, and start it with `OBS_UPDATE_URL` and its own `OBS_USER_DB` / `OBS_LIBRARY`.
