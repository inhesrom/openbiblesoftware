# Get Resources (#91, #132)

**Library → Get Resources…** lists the Bibles, Lexicons, Commentaries and Dictionaries that SWORD repositories offer. One click on **Download** fetches a module from its own repository, runs SWORD Import, and puts the Resource in the Library. **Library → Get Bibles…** opens the same Tab with its type filter set to Bibles.

The code keeps its #91 names (`get_bibles` module, `TabKind::GetBibles`, `getbibles.*` commands), because saved Layouts and scripts use them.

What the Tab shows:

- **Type filter** (Bibles · Lexicons · Commentaries · Dictionaries · All), next to the search and the language filter. All three filters apply together to the "All" list. `library.get_resources` takes `type` (`bibles`, `lexicons`, `commentaries`, `dictionaries`, `all`) so other features can open the Tab already filtered.
- **Featured** sections, one per type. With All, a type that has no Featured module offered is left out.
- **Type.** A SWORD Dictionary module whose conf has `Feature=GreekDef` or `HebrewDef` is listed as a Lexicon. General books and locked modules are not listed.
- **Used by.** Each row says which feature the Resource feeds, for example "Used by Passage Guide › Commentaries".
- **Not supported yet.** A module SWORD Import refuses from its conf alone (Commentary numbering other than KJV/KJVA, `HREFCom`, daily devotionals) is still listed. Its Download is disabled, and the reason shows on hover (`obs_formats::sword_import_type`).

## How it fits together

| Layer | Piece | Role |
|---|---|---|
| `apps/openbible` | `fetch::HttpFetch` | The only network client (`ureq`). It streams with progress and stops when the progress callback returns `false`. `ftp://` and `file://` use the SWORD crate's built-in transport. |
| `obs-engine` | `Session::set_fetch` | Holds the app's `Fetch`. The default `LocalFetch` answers only `file://`, so the engine never opens a connection by itself. |
| `obs-engine` | `get_bibles` | Fetches the catalog (master list → each repository's catalog → unlocked Bibles, Lexicons, Commentaries and Dictionaries), removes duplicates, caches to `sword-catalog.json`, and tracks each listing's status. |
| `obs-engine` | `import` | `ImportSource::Repository`: Download → Parse → Index on one job. A cancel or failure removes the `.obr` and the SWORD copy. |
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

- **Offline, in CI:** `crates/engine/src/get_bibles_tests.rs` and the `get_bibles` UI tests use `obs_engine::testing::MockRepos`, which builds `file://` repositories from the open-text SWORD fixtures (feature `test-support`). In the mock, "NASB1995" is the public-domain KJV excerpt under another name and "StrongsGreek" is the open test Greek lexicon. The mock also has a Commentary (`ObsNotes`), a Dictionary (`ObsTopics`) and two Commentaries that Import refuses (`ObsHebrewNotes`, MT numbering, and `ObsLinks`, `HREFCom`).
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
