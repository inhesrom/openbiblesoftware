# The Starter library in the Library (#131)

`cargo xtask starter` builds the Starter library into `dist/starter/` and the `release` workflow publishes it ([releasing](releasing.md)). This page covers the other half: how the app finds it, puts it in the user's Library, and keeps it there. The decision is recorded in [ADR 0007](../adr/0007-starter-library-is-copied-into-the-library-from-a-bundle-or-the-release.md).

## How it fits together

| Layer | Piece | Role |
|---|---|---|
| `xtask` | `starter` | Writes `starter-manifest.json`: per Resource its revision, content hash, `file_sha256`, `bytes`, and (since #131) title, abbreviation, Resource type, language and SPDX licence, so rows show before anything is downloaded. |
| `obs-engine` | `starter` | Finds the bundle, the first-run sync, removals, revisions, Add back and the verified download. The download goes through the app's `Fetch` (`Session::set_fetch`); the engine opens no connection itself. |
| `obs-engine` | `Session::added_to_library` | The Library entry path shared with Import: `Library::add` (so `STARTER_PREFERRED_ORDER` applies, and a revision keeps its place), save the Preferred order, remember names, Search index, re-anchoring. |
| `obs-ui-egui` | `App::with_file` | Calls `sync_starter_library` at launch, after the library scan and the User database. With no bundle and nothing set up, opens **Library → Starter library…** on the first frame. |
| `obs-ui-egui` | `starter_tab` | "Not in your Library" first, then "In your Library". One row per Starter Resource: type, abbreviation, size, licence badge, what it powers, and In Library / Add back / Download / Update. Download progress, Cancel and Retry. |
| `apps/openbible` | `fetch::HttpFetch` | The HTTPS transport (unchanged from Get Bibles). |

## Finding the bundle

The first folder that holds `starter-manifest.json`, in this order:

1. `$OBS_STARTER_DIR`. When set it is the only place looked at, so pointing it at an empty folder means "no bundle".
2. `starter/` beside the executable (Windows and Linux installers, portable zips).
3. `../Resources/starter/` from the executable (a macOS app bundle: `Contents/MacOS/openbible` → `Contents/Resources/starter/`).
4. Debug builds only: `dist/starter/` in the workspace, so `cargo run -p openbible` works after `cargo xtask starter`.

**What an installer must bundle:** the whole of `dist/starter/` (the manifest, every `.obr`, `search/`, `aliases/` and the `CHANGES-<id>.md` records) in one of those places. The Release archives each release publishes put it in `starter/` beside the executable ([releasing](releasing.md)).

## What happens at launch

`Session::sync_starter_library`, with a bundle:

| Starter Resource is… | First run (no state in the User database) | Later launches |
|---|---|---|
| not in the Library, never removed | added | added only if it is new to this bundle; a known one whose file was deleted outside the app is offered, not added |
| removed by the user | — | stays removed |
| in the Library at a lower revision | replaced | replaced |
| in the Library | kept | kept |

Adding copies `<id>.obr` into the library folder through a `<id>.obr.part` file. Every file's sha256 is checked against the manifest before any of them is renamed into place. Putting them in place is all or nothing too: an earlier revision is first moved aside to `<id>.obr.replaced`, and if any rename fails, the files already in place are taken back and the earlier revisions restored. A failure leaves nothing half-added. The prebuilt Search index caches go to `<library>/search/` (found there as "beside the `.obr`"), the alias caches to `<library>/aliases/`, and the change record next to the `.obr` (About opens the local copy).

Without a bundle, nothing is added. On the very first launch the Starter library Tab opens once to offer the download.

## The User database

Preference `starter-library` holds `{ "known": [...], "removed": [...], "offered": bool }`. Its absence means the Library has never been set up. **Remove…** on a Resource whose manifest says `origin = starter` adds it to `removed`; **Add back** (or any add) takes it out again. A Resource that comes back joins the bottom of its type, like any re-Import; the user's saved Preferred order is not rearranged.

## Downloads

**Download Starter library (≈N MB)** fetches `<base>starter-manifest.json` first (when the Tab opens), then `<base><id>.obr` for every Starter Resource that is missing or outdated and not removed. `<base>` is `$OBS_STARTER_URL`, else `https://github.com/inhesrom/openbiblesoftware/releases/latest/download/`. `file://` bases work too (tests, local mirrors). Every manifest `resource_id` must be a valid Resource id (lowercase letters, digits and `-`; it names a file in the library folder) and listed once; a manifest that breaks this is refused before anything is read or written. One job downloads everything; progress is the sum of the files' sizes. A sha256 mismatch, a network error or Cancel removes every file the job wrote, shows the error with **Retry**, and adds nothing. **Retry** (`starter.retry`) repeats the same attempt: after a failed **Add back** it adds back that Resource again, not "every missing one". A Resource in the download in flight (a newer revision being fetched) cannot be removed until the download ends or is cancelled; `library.remove` says so. Search indexes for downloaded Resources are built here in the background; the prebuilt caches are not downloaded.

## Revisions

A Starter `.obr` with the same `resource_id` and a higher `revision` replaces the older file in place. The Library entry keeps its Preferred-order position, and `reanchor_resource` moves the Resource's Annotations to the new text (ADR 0006), or orphans the ones it cannot find. The engine then emits `Effect::Replaced(id)` (an Import over an existing Resource does too): the app drops the text it loaded from the old file and its cached Marks, and loads the open Tabs of that Resource (and Text Comparisons) again, where they were.

## The Tab after a relaunch

A Starter library Tab restored with the Last session (or a Layout) has no manifest yet. Each frame, once the app's transport is attached, `Session::load_starter_manifest_for_open_tab` fetches the released one if the Tab is open, none is being fetched, and the last attempt did not fail (that failure shows with **Retry** instead).

## Trying it by hand

```sh
cargo xtask starter                       # builds dist/starter/
OBS_LIBRARY=/tmp/obs/library OBS_USER_DB=/tmp/obs/user.db cargo run -p openbible
# No bundle, download from a local folder that looks like the release assets:
OBS_STARTER_DIR=/nonexistent OBS_STARTER_URL=file:///path/to/dist/starter/ \
  OBS_LIBRARY=/tmp/obs2/library OBS_USER_DB=/tmp/obs2/user.db cargo run -p openbible
```

## Tests

- `crates/engine/src/starter_tests.rs`: first run (Preferred order, caches, records, Word Study G26 Lexicon entries), removals and Add back, files deleted outside the app, Resources new to a bundle, revisions with Annotations, the download (success, sha256 mismatch, cancel, retry, released revision), and old manifests.
- The real `dist/starter/`, by hand after `cargo xtask starter`: `cargo test -p obs-engine --release -- --ignored --nocapture built_starter_library` (first run into an empty folder, every prebuilt cache used, Word Study G26 Lexicon entries from TBESG, Abbott-Smith and Strong's).
- `crates/engine/src/starter_tests.rs` also covers a rename failure while putting files in place (fresh and over an earlier revision), unsafe or repeated ids, Retry after a failed or cancelled Add back, and removing a Resource while its revision downloads.
- `crates/ui-egui/src/starter_library_tests.rs`: the Tab's rows, Add back, the one-click download, the mismatch error with Retry, Retry after a failed Add back, the Tab restored on relaunch, and a revision downloaded under an open Tab with a Highlight (and after close and reopen). `UPDATE_PREVIEWS=1` writes screenshots to `target/previews/`.
