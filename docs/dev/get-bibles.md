# Get Resources (#91, #132, #136)

**Library → Get Resources…** lists the Bibles, Lexicons, Commentaries and Dictionaries that SWORD repositories offer. One click on **Download** fetches a module from its own repository, runs SWORD Import, and puts the Resource in the Library. **Library → Get Bibles…** opens the same Tab with its type filter set to Bibles.

The code keeps its #91 names (`get_bibles` module, `TabKind::GetBibles`, `getbibles.*` commands), because saved Layouts and scripts use them.

What the Tab shows:

- **Type filter** (Bibles · Lexicons · Commentaries · Dictionaries · All), next to the search and the language filter. All three filters apply together to the "All" list. `library.get_resources` takes `type` (`bibles`, `lexicons`, `commentaries`, `dictionaries`, `all`) so other features can open the Tab already filtered.
- **Featured** sections, one per type. With All, a type that has no Featured module offered is left out.
- **Type.** A SWORD Dictionary module whose conf has `Feature=GreekDef` or `HebrewDef` is listed as a Lexicon. General books and locked modules are not listed.
- **Used by.** Each row says which feature the Resource feeds, for example "Used by Passage Guide › Commentaries".
- **Not supported yet.** A module SWORD Import refuses from its conf alone (Commentary numbering other than KJV/KJVA, `HREFCom`, daily devotionals) is still listed. Its Download is disabled, and the reason shows on hover (`obs_formats::sword_import_type`).
- **Update available** (#136). A downloaded Resource whose repository now lists a newer `Version` of its module shows "Update available" with **Update** and **Open**; hovering it gives both versions ("Version 1.1 is available (you have 1.0)"). Above the list, "N updates available: …" has **Update all**, and the Library menu shows the count ("Library (2)", "Get Resources… (2 updates)").

## Updates and removal (#136)

- **What counts as newer.** Each download's Import record (`RepoOrigin`, in the User database) keeps the conf `Version` it was imported at. A row offers an update when the catalog's `Version` is newer by SWORD's version order (`sword_repo::compare_versions`; a missing `Version` is 0). Downloads made before #136 have no version on their record; it is read once from their SWORD copy (`Session::fill_installed_versions`, at startup and after each catalog fetch). A live check on 2026-11-05: all 428 CrossWire confs and all 4 Lockman confs carry a numeric dotted `Version`.
- **When it is checked.** Only when the catalog is fetched: when the Tab opens and the catalog is older than six hours, or on Refresh. Nothing downloads until the user clicks Update or Update all (`getbibles.update`, `getbibles.update_all`).
- **What an update does.** It downloads the module into the SWORD folder again (replacing the SWORD copy) and imports it under the **same Resource id** as the next Resource revision (`manifest.revision + 1`). The new `.obr` is written beside the file the library read the Resource from (`<file>.update`, whatever that file is named: the library does not tie file names to ids), indexed (the revision before it keeps its Search index cache until the new revision is in the library), then renamed over that exact file, so a failed or cancelled update leaves the revision before it untouched and the row offers the update again. The Library entry keeps its place in the Preferred order, Annotations are re-anchored (#65) and open Resource and About Tabs read the new revision (`Effect::Replaced`). No Import report Tab opens; the report is still kept.
- **No false "up to date".** The Tab says "Your downloaded Resources are up to date (checked …)" only when a fetch succeeded in this run of the app, every downloaded Resource's repository answered and still lists its module, and none has an update (`UpdatesSummary::up_to_date`). Offline it says the updates could not be checked. After a restart, a catalog saved by an earlier run (still fresh, so not fetched again) only lets it say when updates were last checked (`UpdateCheck::Saved`), until Refresh fetches it; while a fetch runs it says nothing about them. A Resource being updated counts as downloaded with its old revision (`UpdatesSummary::updating`), so no up-to-date claim is made until its update is in.
- **Removing** a downloaded Resource (Library → Remove…, after confirmation) deletes its `.obr`, its Search index cache and its SWORD copy (`Session::sword_copy_of`; kept if another Resource in the Library came from the same module). The confirmation names the SWORD module. Remove is refused while the Resource is being updated, since the update would write its file and SWORD copy back.
- **Starter library** Resources are not downloads, so they never show here; they are updated with the Starter library.

## How it fits together

| Layer | Piece | Role |
|---|---|---|
| `apps/openbible` | `fetch::HttpFetch` | The only network client (`ureq`). It streams with progress and stops when the progress callback returns `false`. `ftp://` and `file://` use the SWORD crate's built-in transport. |
| `obs-engine` | `Session::set_fetch` | Holds the app's `Fetch`. The default `LocalFetch` answers only `file://`, so the engine never opens a connection by itself. |
| `obs-engine` | `get_bibles` | Fetches the catalog (master list → each repository's catalog → unlocked Bibles, Lexicons, Commentaries and Dictionaries), removes duplicates, caches to `sword-catalog.json`, tracks each listing's status, and works out updates (`Session::resource_updates`). |
| `obs-engine` | `import` | `ImportSource::Repository`: Download → Parse → Index on one job. A cancel or failure removes the `.obr` and the SWORD copy. An update (`Session::start_update`) writes the next Resource revision of an existing Resource. |
| `obs-formats` | `sword_repo` | `Fetch` trait, plus a bridge to the `sword-modules` installer. Built with feature `install`, never `https`. |
| `obs-ui-egui` | `get_bibles_tab` | Type filter, Featured sections, then the All list (search and language filter), licence badges, "Used by", Download / progress / Open / Retry. |

`cargo xtask layers` fails if any library crate depends on a networking crate (`ureq`, `reqwest`, `hyper`, `rustls`, …), directly or transitively.

## Data and folders

| What | Where | Override |
|---|---|---|
| Featured Bibles (order, repository) | `crates/engine/data/featured-bibles.toml` | edit the file |
| Featured Lexicons, Commentaries, Dictionaries | `crates/engine/data/featured-resources.toml` (checked against the live catalogs: [docs/research/get-resources-catalog.md](../research/get-resources-catalog.md)) | edit the file |
| Catalog cache (opens offline) | `<data>/OpenBible/sword-catalog.json` | `OBS_SWORD_CATALOG` |
| Downloaded SWORD modules | `<data>/OpenBible/sword/` | `OBS_SWORD_DIR` |
| Imported Resources | `<data>/OpenBible/library/` | `OBS_LIBRARY` |

`<data>` is `~/.local/share` on Linux, `~/Library/Application Support` on macOS, and `%APPDATA%` on Windows.

When a module is in several repositories, Get Resources shows it once, even when the copies are of different types (a SWORD folder holds one module of a name). It takes the repository a Featured list names, otherwise CrossWire, otherwise the first one listed. A catalog older than six hours is refreshed in the background when the Tab opens. So is a catalog cached by an older version of the app (`CATALOG_VERSION`; v0.1 caches list only Bibles), but it is still shown while that runs, and offline.

## Tests

- **Offline, in CI:** `crates/engine/src/get_bibles_tests.rs` and the `get_bibles` UI tests use `obs_engine::testing::MockRepos`, which builds `file://` repositories from the open-text SWORD fixtures (feature `test-support`). In the mock, "NASB1995" is the public-domain KJV excerpt under another name and "StrongsGreek" is the open test Greek lexicon. `MockRepos::publish` bumps a module's `Version` (and can edit a RawText module's text) to test updates. The mock also has a Commentary (`ObsNotes`), a Dictionary (`ObsTopics`) and two Commentaries that Import refuses (`ObsHebrewNotes`, MT numbering, and `ObsLinks`, `HREFCom`).
- **Live, by hand only:** these tests need the network and download copyrighted modules, all into a temp folder that is deleted afterwards. CI never runs them.

  ```sh
  # HTTPS through the app's transport: catalog, NASB 1995 size and download, every Featured
  # Bible, every Featured Lexicon/Commentary/Dictionary, and StrongsGreek → Word Study G26.
  cargo test -p openbible --release -- --ignored --nocapture --test-threads=1 live_
  # The Tab itself, clicked from Download to reading NASB 1995, then offline from the cache.
  # Uses the repositories' FTP sources; screenshots go to target/previews/ (never commit them).
  cargo test -p obs-ui-egui --release -- --ignored --nocapture live_get_bibles
  ```

## Copyright

Downloads go only to the user's own data folder, straight from the repository that is allowed to distribute them. The rules in [local-copyrighted-modules.md](local-copyrighted-modules.md) still apply: never commit, bundle or cache in CI a copyrighted module, `.obr` or Search index.
