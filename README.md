# Open Bible Software

An offline desktop Bible study app for the serious lay student, written in Rust with egui.
Domain language lives in [`CONTEXT.md`](CONTEXT.md), decisions in [`docs/adr/`](docs/adr/), and
the v0.1 spec and implementation tickets in [issue #1](https://github.com/inhesrom/openbiblesoftware/issues/1).

## Workspace

| Layer | Crates |
|---|---|
| L0 | `obs-scripture` |
| L1 | `obs-model`, `obs-fonts` |
| L2 | `obs-formats`, `obs-layout`, `obs-search`, `obs-store` (the only SQLite user) |
| L3 | `obs-render` |
| L4 | `obs-engine` |
| L6 | `obs-ui-egui` (the only egui/eframe user) |
| apps | `openbible` (desktop), `obs-cli` |

A crate may depend only on its own or lower layers. Everything below L6 compiles for
`wasm32-unknown-unknown`. `obs-` is a working prefix until the product is named.

## Development

```sh
cargo run -p openbible   # desktop app
cargo xtask ci           # every gate CI runs: fmt, layers, assets, clippy, test, wasm
cargo xtask help         # run a single gate
```

Needs the toolchain from `rust-toolchain.toml` (stable, with the `wasm32-unknown-unknown` target)
and, for tests, [`cargo-nextest`](https://nexte.st/) (`cargo xtask test` falls back to `cargo test`
locally). Bundled fonts and data files need an entry in [`ATTRIBUTION.toml`](ATTRIBUTION.toml).

## Licence

Code is dual-licensed under MIT ([`LICENSE-MIT`](LICENSE-MIT)) or Apache-2.0 ([`LICENSE-APACHE`](LICENSE-APACHE)). Bundled content keeps its own licence; see
`ATTRIBUTION.toml` and each Resource's About view.
