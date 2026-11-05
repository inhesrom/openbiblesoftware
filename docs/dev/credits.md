# Credits and change records (#67)

**Help → Credits** opens one keyless Credits Tab. It lists every Starter library Resource,
the data built into the app, the bundled fonts with their licence texts, the third-party
crates with theirs, and the test data in the source repository. Each CC BY Starter Resource
also has a published Change record (`CHANGES-<id>.md`), and its About Tab links to it.
Decision #20 (point 4) asks for both.

## Where the content comes from

The facts are not written twice. `obs_engine::credits` compiles these files in and parses
them once, the first time Credits opens:

| Credits section | Source | Kept honest by |
|---|---|---|
| Starter library | `starter/sources.toml` (title, licence, Attribution, changes) | the provenance gate of `cargo xtask starter` |
| Data built into the app | `ATTRIBUTION.toml` rows outside `tests/` and outside the fonts (TVTMS), plus the OpenBible.info lock entry, whose votes rank Cross-references | `cargo xtask assets` |
| Fonts | `ATTRIBUTION.toml` font rows, plus `obs_fonts::FONT_LICENSES` (`OFL.txt`, `Licenses.txt`, verbatim) | `cargo xtask assets` |
| Third-party software | `crates/engine/data/third-party-licenses.json` | the `licenses` gate of `cargo xtask ci` |
| Test data in the source code | `ATTRIBUTION.toml` rows under `tests/` | `cargo xtask assets` |

Every `ATTRIBUTION.toml` row appears exactly once. Two tests check this against the file on
disk: the engine test `credits::tests::every_attribution_row_appears_in_credits` (name,
licence, source, copyright and changes) and the UI test
`credits::credits_tab_shows_every_attribution_row_and_every_starter_resource`, which scrolls
the real Tab.

## Third-party licences

`cargo xtask licenses` runs [cargo-about](https://github.com/EmbarkStudios/cargo-about)
0.9.3. It reads `about.toml` and covers the desktop app's dependency graph
(`apps/openbible`, for every shipped desktop target). It rewrites
`crates/engine/data/third-party-licenses.json`, which holds each crate's name, version,
declared licence and repository, plus each distinct licence text and the crates it covers.
Commit the result whenever `Cargo.lock` changes.

```sh
cargo install cargo-about --locked --features cli --version 0.9.3
cargo xtask licenses          # regenerate
cargo xtask licenses --check  # what CI runs
```

cargo-about runs `--frozen` after `cargo fetch`. It reads only the licence files packaged in
each crate and never the network, so every host and OS produces the same bytes. In CI the
`licenses` gate requires cargo-about (`taiki-e/install-action`) and fails if the committed
file is stale. When it fails, it lists the crates that were added, removed or changed. On a
machine without cargo-about the gate is skipped with a warning, as `test` falls back without
nextest.

`about.toml` accepts only permissive licences, so a crate licensed `MIT OR LGPL` or
`Apache-2.0 OR GPL-2.0-only` is always taken under its permissive option.

## Change records

`cargo xtask starter` writes `dist/starter/CHANGES-<id>.md` for every lock entry whose SPDX
expression names a CC BY licence (`obs_model::is_cc_by`). The `starter` workflow attaches
`dist/starter/*` to the release. Each record has:

- the title, the abbreviation and the `resource_id`;
- the licence with a link to its Creative Commons deed, the edition, the publisher and the
  source;
- the Attribution, verbatim;
- every pinned download with its SHA-256, so the changed edition is exact;
- the lock's "Changes we made".

The About Tab of a Starter Resource whose licence is CC BY shows **Published change record:
CHANGES-<id>.md**. The link opens the copy beside the `.obr` when there is one (the installer
bundles `dist/starter/`, so this works offline). Otherwise it opens the copy on the latest
release:
`https://github.com/inhesrom/openbiblesoftware/releases/latest/download/CHANGES-<id>.md`.
Imported Resources have no Change record, because we did not change them.

Links open in the system browser through eframe's `links` feature (the `webbrowser` crate,
in `apps/openbible` only).

## Evidence

- [Credits Tab](../previews/credits.png) and
  [About Tab with its change record link](../previews/about-change-record.png), from the
  headless harness (`UPDATE_PREVIEWS=1 cargo test -p obs-ui-egui credits`, frames written
  to `target/previews/*.ppm` and encoded to PNG with ffmpeg). Only open data is shown.
