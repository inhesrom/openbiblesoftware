# ArtCraft architecture study (issue #4)

**Question:** How are the ArtCraft Crafting Apps structured, and what can Open Bible Software reuse?
**Date:** research run against `main` of each repo at fetch time (WordCraft's `docs/architecture.md` is stamped "Last reviewed 2026-10-28"; pinned revs cited below).
**Scope:** storytold/wordcraft (deep), photocraft, vectorcraft, gridcraft, craft-launcher (glance), storytold/craft-libs, storytold/egui and storytold/winit forks, storytold/craft-common-actions.

## TL;DR

- Every app is one Cargo workspace (`crates/*`, `apps/*`, `xtask`), edition 2024, resolver 3, MSRV 1.95, `MIT OR Apache-2.0`. Layers L0–L6 run from `geom` to `ui-egui`. `cargo xtask layers` enforces them: "Nothing below L6 depends on egui/eframe/winit/rfd." ([wordcraft Cargo.toml](https://github.com/storytold/wordcraft/blob/main/Cargo.toml), [AGENTS.md](https://github.com/storytold/wordcraft/blob/main/AGENTS.md))
- **Everything is a command.** A `CommandSpec` holds an id, label, location, shortcut, params, `enabled` and `run`, and all calls go through `Session::run`. Menus, shortcuts, CLI, the JSON control channel and MCP all dispatch by id. Undo snapshots the whole document, which is cheap because blocks are `Arc`-shared. ([engine/src/lib.rs](https://github.com/storytold/wordcraft/blob/main/crates/engine/src/lib.rs), [docs/architecture.md](https://github.com/storytold/wordcraft/blob/main/docs/architecture.md))
- The egui fork is egui 0.37.0 plus 21 patches (E1–E21), mostly bidi/RTL, IME, non-Latin shortcut keys, Wayland clipboard and wgpu surface errors. The winit fork is 0.30.13 plus W1–W27, mostly IME, DPI and Wayland. Every crate is pinned by `rev` through `[patch.crates-io]`. ([egui PATCHES.md](https://github.com/storytold/egui/blob/6d518fc799681571963842f95b55a6b68431b15a/PATCHES.md), [winit PATCHES.md](https://github.com/storytold/winit/blob/a517b3846ee8457d84ee4975110bfda6797c44dc/PATCHES.md))
- craft-libs is a MIT/Apache-2.0 monorepo of more than 25 `craft-*` crates, meant to be used as **git deps pinned by rev**. Useful candidates: `craft-i18n`, `craft-ui`, `craft-dialogs`, `craft-settings`, `craft-atomic`, `craft-keymap`, `craft-window`, `craft-menu`, `craft-quit` and `craft-font-catalog`. ([craft-libs README](https://github.com/storytold/craft-libs))
- Using craft-libs crates that depend on egui means using their egui fork too: craft-libs itself patches egui and winit to the same revs.
- `craft-update` makes network calls (`ureq`, rustls) to check for, download and install updates. That conflicts with our offline-only decision unless we leave it out or put it behind a feature gate. ([update/Cargo.toml](https://github.com/storytold/craft-libs/blob/main/crates/update/Cargo.toml))
- CI and release are thin callers of `storytold/craft-common-actions@main`. That repo produces macOS DMG builds with notarization, Windows MSI (WiX v5), AppImage/deb/rpm (nfpm), Flatpak, FreeBSD, Arch and a Trunk web build. No LICENSE file was found in it, so treat it as reference only (unverified). ([craft-common-actions](https://github.com/storytold/craft-common-actions))
- Quality gate: `cargo xtask ci` runs fmt, clippy `-D warnings`, nextest, the i18n report, doctests, the asset-attribution check, the layers check and a `wasm32` check. Lint rules: `unsafe_code = "forbid"`, plus a "never panic" `#![deny(unwrap/expect/panic…)]` on every crate root. UI tests use `egui_kittest` with its `wgpu` and `snapshot` features. ([xtask/src/main.rs](https://github.com/storytold/wordcraft/blob/main/xtask/src/main.rs))
- WASM approach: a separate `apps/<app>-web` binary built with Trunk (eframe web runner, WebGPU with WebGL2 fallback). L0–L5 and `ui-egui` must pass `cargo check --target wasm32-unknown-unknown`.
- The code is open, the branding is not: the ArtCraft name and logos are trademarks under a separate brand licence, and "Forks and modified versions must remove them." ([wordcraft README](https://github.com/storytold/wordcraft#license-and-credits))

## 1. Workspace and crate layout

WordCraft's root `Cargo.toml` declares `members = ["crates/*", "apps/*", "xtask"]` and keeps internal crates in `[workspace.dependencies]` as `wordcraft-<name> = { path = "crates/<name>" }` ([Cargo.toml](https://github.com/storytold/wordcraft/blob/main/Cargo.toml)). PhotoCraft, VectorCraft and GridCraft use the same skeleton: the same members, the same lints and the same `<app>-engine` / `<app>-ui-egui` / `<app>-cli` split ([photocraft](https://raw.githubusercontent.com/storytold/photocraft/main/Cargo.toml), [vectorcraft](https://raw.githubusercontent.com/storytold/vectorcraft/main/Cargo.toml), [gridcraft](https://raw.githubusercontent.com/storytold/gridcraft/main/Cargo.toml)). craft-launcher also has `apps/artcraft-launcher` and `apps/artcraft-launcher-web` ([craft-launcher](https://github.com/storytold/craft-launcher)).

WordCraft layers ([docs/architecture.md](https://github.com/storytold/wordcraft/blob/main/docs/architecture.md)):

| Layer | Crates | Job | Analogue for us |
|---|---|---|---|
| L0 | `geom`, `control-key` | units, rects | verse refs/versification, units |
| L1 | `doc`, `fonts`, `metafile`, `proof` | pure data model plus edit primitives; fonts and shaping | resource model (texts, notes, lexicons) |
| L2 | `layout`, `docx`, `docbin`, `formats` | layout and hit-testing; format I/O | importers (OSIS/USFM/etc.), text layout |
| L3 | `render`, `pdf` | vello_cpu raster; krilla PDF | render/export |
| L4 | `engine` | `Session`, undo, command registry, I/O dispatch | library/session/search engine |
| L5 | `mcp`, `zotero` | MCP server; integrations | optional |
| L6 | `ui-egui` | front end, "swappable" | `ui-egui` |
| apps | `wordcraft`, `wordcraft-cli`, `wordcraft-web` | desktop, CLI, browser | same three |

Dependency direction runs strictly upward. `doc` depends only on `geom`, `serde`, `thiserror`, `unicode-segmentation`, `base64` and `log` ([crates/doc/Cargo.toml](https://github.com/storytold/wordcraft/blob/main/crates/doc/Cargo.toml)). `ui-egui` depends on engine, layout, render and the craft-libs UI crates ([crates/ui-egui/Cargo.toml](https://github.com/storytold/wordcraft/blob/main/crates/ui-egui/Cargo.toml)). `wordcraft-web` depends on `engine` and `ui-egui`, and enables eframe only on `cfg(target_arch="wasm32")` ([apps/wordcraft-web/Cargo.toml](https://github.com/storytold/wordcraft/blob/main/apps/wordcraft-web/Cargo.toml)).

**How the layers check works:** `xtask` reads `cargo metadata`, classifies each crate by name into a layer and reports any violation ([xtask/src/main.rs](https://github.com/storytold/wordcraft/blob/main/xtask/src/main.rs)). The layer table itself is in `xtask/src/layers.rs`, which was not read (unverified detail).

## 2. Document model, commands and undo

- **Command record:** `CommandSpec { id, label, location, shortcut ("Mod+B"), params (JSON doc for agents), mutates, enabled: fn(&Session)->Option<&str>, run: fn(&mut Session,&Value)->CmdResult }`. Errors use `CmdError::{Unknown, Params, Disabled, Failed}` ([engine/src/lib.rs](https://github.com/storytold/wordcraft/blob/main/crates/engine/src/lib.rs)).
- **Command ids:** stable strings such as `format.bold`. `ui.*` commands live in `ui-egui`. "Programmatic calls never open dialogs." ([AGENTS.md](https://github.com/storytold/wordcraft/blob/main/AGENTS.md))
- **Undo:** `Session::run` "snapshots for undo (blocks are `Arc`-shared, so snapshots are cheap) and catches an escaped panic, restoring the document" ([docs/architecture.md](https://github.com/storytold/wordcraft/blob/main/docs/architecture.md)). `EditSnapshot` stores the document, selection, undo/redo stacks (`Vec<Arc<Undo>>`), typing-group state and dirty flag. `UNDO_LIST_MAX` caps history ([engine/src/snapshot.rs](https://github.com/storytold/wordcraft/blob/main/crates/engine/src/snapshot.rs)). So the design is **persistent-data snapshot undo, not inverse-operation commands**.
- **Parity tracking:** `catalog.rs` maps target-app features to command ids. `cargo xtask parity` generates a checklist, and a test enforces a floor that only rises.
- **Robustness:** `hostile_params_never_panic` runs every command with junk parameters ([AGENTS.md](https://github.com/storytold/wordcraft/blob/main/AGENTS.md)).

## 3. The egui and winit forks

**Why they fork:** to ship fixes for language support (RTL/bidi, IME, CJK/Indic fallback, shortcuts on non-Latin keyboard layouts), Wayland clipboard and drag-and-drop, and GPU/DPI robustness, without waiting for upstream releases. Policy: "Drop a patch when an egui release in use contains it." Apps "patch every egui-workspace crate they use to this checkout, so only one egui resolves" ([egui PATCHES.md](https://github.com/storytold/egui/blob/6d518fc799681571963842f95b55a6b68431b15a/PATCHES.md)).

| Patch group | Examples | Matters for a Bible app? |
|---|---|---|
| Bidi/RTL text (E5, E9, E12, E13, E16) | RTL shaping, visual row order, `Style::right_to_left`, grapheme-cluster caret | **Yes**: Hebrew/Greek original-language text, Strong's lexicon entries |
| IME (E6, E21; winit W3, W5–W10, W18–W27) | composition not interrupted, `DeleteSurroundingBytes` | Medium (notes in CJK and other IME languages) |
| Font fallback (E14), `LineBreaker` trait (E15), language-aware layout (E17, E20) | `FontTweak::fallback_align`, `LayoutJob::language` | Yes (mixed scripts) |
| Non-Latin layout shortcuts (E7, E19, W23, W25) | letter keys on Russian/Hebrew keyboard layouts | Yes, for international users |
| Wayland clipboard (E4, W15) | winit data device instead of smithay-clipboard | Linux polish |
| wgpu (E2+E3) | fit oversized surfaces; return surface/renderer errors instead of panicking | Robustness |
| eframe `ViewportHooks` (E1) | per-viewport native window access, raw input | Pen input; low relevance for us |

Mechanism: 15 egui crates and `winit` are patched to fixed revs (`6d518fc7…` / `a517b384…`) under `[patch.crates-io]` ([wordcraft Cargo.toml](https://github.com/storytold/wordcraft/blob/main/Cargo.toml)). Each egui patch lives on a `craft/<topic>` branch, and the branches are merged in table order onto tag `0.37.0` ([commits](https://github.com/storytold/egui/commits/6d518fc799681571963842f95b55a6b68431b15a)). PATCHES.md also lists "Local migration follow-ups (not yet committed)" whose tests "have not been run after this repair", so the fork is still moving.

## 4. craft-libs

The workspace is `MIT OR Apache-2.0` with `unsafe_code = "forbid"`, and AGENTS.md rules apply: never crash, "No app code" ([Cargo.toml](https://github.com/storytold/craft-libs/blob/main/Cargo.toml), [AGENTS.md](https://github.com/storytold/craft-libs/blob/main/AGENTS.md)). The README states how apps consume it: `craft-<name> = { git = "https://github.com/storytold/craft-libs", rev = "<commit>" }` ([README](https://github.com/storytold/craft-libs)). It is not published to crates.io; crates are pinned at `version = "0.1.0"`.

| Crate | Provides | Relevance |
|---|---|---|
| `craft-i18n` | catalog lookup, fallback (`pt-br`→`pt`→English), plurals, system locale | High. Catalogs come from a separate repo, `benandez/craft-i18n`, and are an optional build input (`CRAFT_I18N_DIR`) ([i18n README](https://github.com/storytold/craft-libs/blob/main/crates/i18n/README.md)); that repo's licence is **unverified** |
| `craft-ui` | SVG icon catalogs, icon buttons, tabs, toolbar fitting, split/resize, **docking**, theme helpers, UI font stack | High (panes/docking like Logos panels). No assets bundled ([ui README](https://github.com/storytold/craft-libs/blob/main/crates/ui/README.md)) |
| `craft-dialogs` | non-blocking native file and message dialogs over `rfd` 0.17 (xdg-portal) | High ([dialogs/Cargo.toml](https://github.com/storytold/craft-libs/blob/main/crates/dialogs/Cargo.toml)) |
| `craft-settings` | `config.toml` settings in platform folders, portable mode, atomic saves, migrations | High |
| `craft-atomic` | crash-safe atomic file writes | High (notes and user data) |
| `craft-keymap` | key chords, scopes, conflict detection, user overrides | Medium-high |
| `craft-window`, `craft-menu`, `craft-quit` | one window per document plus a Window menu; macOS menu bar; quit/close handling | Medium |
| `craft-font-catalog` | installed system fonts on every platform | Medium (Greek/Hebrew fonts) |
| `craft-text` | hyphenation | Low-medium |
| `craft-pdf`, `craft-print` | PDF via patched krilla/hayro; CUPS/Windows printing | Later (print and export) |
| `craft-update` | update checks, downloads, self-update (`ureq`, `rustls`, `semver`) | **Conflicts with offline-only** unless excluded or opt-in |
| `craft-ai-core`, `cms`, `crw`, `jpeg`, `media`, `font-store` | AI contracts, colour management, image and media codecs, Google Fonts store | Not relevant (font-store downloads fonts) |

Note that craft-libs patches its own dependencies: egui/winit to the forks above, and krilla/hayro to `vendor/` copies. A git dependency on it therefore brings in the fork.

## 5. Packaging, CI and release

- The app repos have `.github/workflows/{ci,release,packaging-lint,freebsd,windows-arm64,nix,sonarcloud,printing}.yml`. `ci.yml` is a single call to `craft-common-actions/.github/workflows/gate.yml@main` plus a list of Linux apt packages ([ci.yml](https://github.com/storytold/wordcraft/blob/main/.github/workflows/ci.yml)). `release.yml` calls the shared `release.yml@main` with Apple and Azure signing secrets ([release.yml](https://github.com/storytold/wordcraft/blob/main/.github/workflows/release.yml)).
- The shared release workflow builds: macOS universal DMG (codesigned and notarized), Windows x64/x86/ARM64 MSI (WiX v5, Azure Trusted Signing), Linux AppImage/deb/rpm/tar.gz (nfpm), Flatpak, FreeBSD, Arch, RISC-V and a Trunk web build. It also writes `SHA256SUMS.txt` and can add optional SBOM/attestations ([craft-common-actions README](https://github.com/storytold/craft-common-actions)).
- `packaging/` in each app holds per-app inputs. In WordCraft it contains `packaging/icons.sh`, which uses resvg and iconutil, and the outputs are committed ([icons.sh](https://github.com/storytold/wordcraft/blob/main/packaging/icons.sh)). Not inspected: the `nfpm.yaml`/WiX layout (no `packaging/nfpm.yaml` at the root).
- **Licensing gap:** no LICENSE file appeared in the craft-common-actions tree listing (**unverified**; check before copying any YAML).

## 6. Testing, lint and format

- `cargo xtask ci` steps: `fmt --check`, `clippy --workspace --all-targets -D warnings`, nextest (falls back to `cargo test`), i18n audit, doctests, `assets` (every asset has a row in ATTRIBUTION.md), `layers`, `wasm` ([xtask/src/main.rs](https://github.com/storytold/wordcraft/blob/main/xtask/src/main.rs)). `.cargo/config.toml` defines the alias `xtask = "run --package xtask --"`.
- Lints: `unsafe_code="forbid"`; clippy denies `cognitive_complexity`, `wildcard_imports` and `too_many_arguments`; production crate roots deny unwrap/expect/panic/todo/unreachable ([Cargo.toml](https://github.com/storytold/wordcraft/blob/main/Cargo.toml)). `rustfmt.toml` sets `max_width = 150` and `use_small_heuristics = "Max"` ([rustfmt.toml](https://github.com/storytold/wordcraft/blob/main/rustfmt.toml)).
- UI tests: `egui_kittest` with `["wgpu","snapshot"]` as a dev-dependency of `ui-egui`, plus a headless `ui_shot` example that renders the README screenshots ([ui-egui Cargo.toml](https://github.com/storytold/wordcraft/blob/main/crates/ui-egui/Cargo.toml), [README](https://github.com/storytold/wordcraft)). `craft-update` has a `snapshot` feature that renders its dialogs with kittest. Overall: 821 tests plus proptest blocks ([docs/architecture.md](https://github.com/storytold/wordcraft/blob/main/docs/architecture.md)).
- Dev profile: dependencies at `opt-level=2` with no debug info; workspace at `opt-level=1` with `debug="line-tables-only"`. Release: thin LTO.

## 7. AGENTS.md conventions

From [AGENTS.md](https://github.com/storytold/wordcraft/blob/main/AGENTS.md) (CLAUDE.md is a near-copy, [CLAUDE.md](https://github.com/storytold/wordcraft/blob/main/CLAUDE.md)):

- Start each session from `plan/STATUS.md`. `plan/` is gitignored; if it is missing, start from ROADMAP.md and `docs/gaps.md`.
- The "Never crash" rules outrank feature work.
- No Microsoft, Adobe or other vendor assets. Icons are drawn in code (`icons.rs`), fonts go in a separate `craft-fonts` repo, and every asset is attributed.
- Clean room: observe the target app black-box only, save its screenshots only to gitignored folders, never copy GPL code, and implement formats from public specs.
- "Rust only (no handwritten JS/TS). Never break wasm."
- Parallel agents each use their own `CARGO_TARGET_DIR`. Commit messages carry a task id.
- CI changes go to craft-common-actions, never into a feature PR.

## 8. WASM approach

The web app is `apps/<app>-web`, built with `Trunk.toml` and `index.html`, using eframe's wasm features `wgpu` (WebGPU with WebGL2 fallback) and `accesskit`. It depends on `web-sys` for IndexedDB, Blob and Clipboard. `web-time` replaces `Instant`, and the UI reads the time zone from the browser via `js-sys` ([wordcraft-web Cargo.toml](https://github.com/storytold/wordcraft/blob/main/apps/wordcraft-web/Cargo.toml), [ui-egui Cargo.toml](https://github.com/storytold/wordcraft/blob/main/crates/ui-egui/Cargo.toml)). Desktop-only dependencies such as `craft-update` and the `chrono` clock sit under `cfg(not(target_arch="wasm32"))`. The CI check is `cargo check` only; it does not run the wasm build.

## Implications for our decisions (facts, not choices)

- **Crate split:** our planned `engine` / `ui-egui` split matches theirs. Their extra layers (geom → doc → layout/formats → render → engine → ui) and an automated `layers` check are cheap to copy and protect WASM-clean code.
- **Undo model:** `Arc`-shared snapshot undo suits mostly read-only resources plus user notes and highlights. Command-id dispatch would also give us CLI and scripting for free, and offline-friendly MCP later.
- **Fork or upstream egui:** using their fork would give us bidi/RTL now, which matters for Hebrew and Strong's entries. The cost is tracking a moving fork with uncommitted local patches. Upstream egui 0.37 lacks E5/E12 RTL (#8577 is still open upstream). Depending on egui-based craft-libs crates effectively forces the fork.
- **craft-libs:** licence-compatible with our MIT/Apache-2.0 choice and consumable as a rev-pinned git dependency, but not on crates.io (affects any later crates.io publish). Exclude `craft-update` and `craft-font-store` for offline-only, or verify they stay inert until the user opts in.
- **Packaging/CI:** the `@main` shared workflows give no stability guarantee to outsiders, and their licence is unverified. A safer option is to write our own workflows modelled on that design (nfpm, WiX, notarization).
- **Branding and clean room:** their asset and clean-room rules map directly onto our "no Logos assets or trade dress" rule. Do not reuse ArtCraft logos or the `ai.storyteller.*` IDs.

## What we could copy / reuse / depend on

| Item | Mode | Licence | Risk / note |
|---|---|---|---|
| Workspace skeleton (`crates/*`, `apps/*`, `xtask`, lints, profiles, rustfmt) | Copy pattern | MIT/Apache | Low |
| `xtask` ci/layers/wasm/assets commands | Copy and adapt code | MIT/Apache (keep notices) | Low |
| `CommandSpec` registry + `Session::run` snapshot undo | Copy pattern | MIT/Apache | Low; re-implement for our domain |
| AGENTS.md rules (never crash, clean room, attribution, never break wasm) | Copy and adapt text | MIT/Apache | Low |
| `egui_kittest` snapshot tests + headless `ui_shot` | Copy pattern | egui MIT/Apache | Snapshot tests need the wgpu feature |
| storytold/egui + winit forks | Depend (rev pin) | MIT/Apache (upstream licences) | Medium-high: drifting fork, local uncommitted patches |
| `craft-i18n` | Depend (git rev) | MIT/Apache; catalog repo licence unverified | Medium |
| `craft-ui` (docking, tabs, icons, themes) | Depend (git rev) | MIT/Apache | Medium; pulls in the egui fork |
| `craft-dialogs`, `craft-settings`, `craft-atomic`, `craft-keymap` | Depend (git rev) | MIT/Apache | Low-medium |
| `craft-window`, `craft-menu`, `craft-quit`, `craft-font-catalog` | Depend (git rev), optional | MIT/Apache | Medium |
| `craft-update`, `craft-font-store` | Avoid / opt-in | MIT/Apache | Conflicts with offline-only |
| `craft-pdf`, `craft-print` | Later | MIT/Apache (vendored krilla/hayro) | Patches crates-io krilla |
| craft-common-actions workflows | Reference only | **No licence found (unverified)** | Tracks `@main`; needs their signing secrets |
| ArtCraft brand, `ai.storyteller.*` IDs, app icons | Do not use | Trademark | Brand licence forbids use in forks |

## Sources

- Kept: storytold/wordcraft Cargo.toml, AGENTS.md, CLAUDE.md, README, docs/architecture.md, xtask/src/main.rs, engine/src/{lib,snapshot}.rs, crates/{doc,ui-egui}/Cargo.toml, apps/wordcraft-web/Cargo.toml, rustfmt.toml, .cargo/config.toml, .github/workflows/{ci,release}.yml, packaging/icons.sh (primary source code and config).
- Kept: storytold/egui PATCHES.md and commit log at rev 6d518fc7; storytold/winit PATCHES.md and commit log at rev a517b384. These are the authoritative patch lists.
- Kept: storytold/craft-libs README, Cargo.toml, AGENTS.md, and crates/{update,dialogs}/Cargo.toml, {i18n,ui}/README.md.
- Kept: Cargo.toml of photocraft, vectorcraft and gridcraft, plus the craft-launcher tree; these confirm the pattern repeats across the suite.
- Kept: storytold/craft-common-actions README and tree (packaging pipeline).
- Dropped: getartcraft.com marketing pages, which add no technical detail. Also dropped: the egui upstream README beyond the licence badge.

## Gaps

- Not read: `xtask/src/layers.rs` (exact layer rules), the actual kittest snapshot test files, the `packaging/` nfpm/WiX/Flatpak manifests, and the `craft-i18n` data-repo licence.
- Unverified: whether craft-common-actions has a licence; whether `craft-update` stays inert when unused; how stable craft-libs is for external consumers (no semver, rev pins only).
- Next steps: read `layers.rs` and one `ui-egui/tests/*` kittest file; check the LICENSE of benandez/craft-i18n and craft-common-actions; build a spike crate that depends on `craft-ui` and `craft-i18n` at rev `92d392cf` and measure the dependency tree and wasm check.
