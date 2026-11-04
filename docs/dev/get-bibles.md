# Get Bibles (#91)

**Library → Get Bibles…** lists the Bibles that SWORD repositories offer. One click on **Download** fetches a module from its own repository, runs SWORD Import, and puts the Bible in the Library.

## How it fits together

| Layer | Piece | Role |
|---|---|---|
| `apps/openbible` | `fetch::HttpFetch` | The only network client (`ureq`). It streams with progress and stops when the progress callback returns `false`. `ftp://` and `file://` use the SWORD crate's built-in transport. |
| `obs-engine` | `Session::set_fetch` | Holds the app's `Fetch`. The default `LocalFetch` answers only `file://`, so the engine never opens a connection by itself. |
| `obs-engine` | `get_bibles` | Fetches the catalog (master list → each repository's catalog → unlocked Bibles), removes duplicates, caches to `sword-catalog.json`, and tracks each Bible's status. |
| `obs-engine` | `import` | `ImportSource::Repository`: Download → Parse → Index on one job. A cancel or failure removes the `.obr` and the SWORD copy. |
| `obs-formats` | `sword_repo` | `Fetch` trait, plus a bridge to the `sword-modules` installer. Built with feature `install`, never `https`. |
| `obs-ui-egui` | `get_bibles_tab` | Featured section, then All Bibles (search and language filter), licence badges, Download / progress / Open / Retry. |

`cargo xtask layers` fails if any library crate depends on a networking crate (`ureq`, `reqwest`, `hyper`, `rustls`, …), directly or transitively.

## Data and folders

| What | Where | Override |
|---|---|---|
| Featured list (order, repository) | `crates/engine/data/featured-bibles.toml` | edit the file |
| Catalog cache (opens offline) | `<data>/OpenBible/sword-catalog.json` | `OBS_SWORD_CATALOG` |
| Downloaded SWORD modules | `<data>/OpenBible/sword/` | `OBS_SWORD_DIR` |
| Imported Resources | `<data>/OpenBible/library/` | `OBS_LIBRARY` |

`<data>` is `~/.local/share` on Linux, `~/Library/Application Support` on macOS, and `%APPDATA%` on Windows.

When a module is in several repositories, Get Bibles shows it once. It takes the repository the Featured list names, otherwise CrossWire, otherwise the first one listed. A catalog older than six hours is refreshed in the background when the Tab opens.

## Tests

- **Offline, in CI:** `crates/engine/src/get_bibles_tests.rs` and the `get_bibles` UI tests use `obs_engine::testing::MockRepos`, which builds `file://` repositories from the open-text SWORD fixtures (feature `test-support`). In the mock, "NASB1995" is the public-domain KJV excerpt under another name.
- **Live, by hand only:** these tests need the network and download copyrighted modules, all into a temp folder that is deleted afterwards. CI never runs them.

  ```sh
  # HTTPS through the app's transport: catalog, NASB 1995 size and download, every Featured Bible.
  cargo test -p openbible --release -- --ignored --nocapture --test-threads=1 live_
  # The Tab itself, clicked from Download to reading NASB 1995, then offline from the cache.
  # Uses the repositories' FTP sources; screenshots go to target/previews/ (never commit them).
  cargo test -p obs-ui-egui --release -- --ignored --nocapture live_get_bibles
  ```

## Copyright

Downloads go only to the user's own data folder, straight from the repository that is allowed to distribute them. The rules in [local-copyrighted-modules.md](local-copyrighted-modules.md) still apply: never commit, bundle or cache in CI a copyrighted module, `.obr` or Search index.
