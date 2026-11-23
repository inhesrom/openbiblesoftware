# PROTOTYPE — headless GPUI tests and screenshots (#374, throwaway)

> Throwaway prototype for the decision ticket #374 on the GPUI map #371 (ADR 0025). It lives
> only on the branch `prototype/374-gpui-headless` and is never merged. The findings are in
> [REPORT.md](REPORT.md).

A tiny surface (`src/lib.rs`), drawn with `gpui-kit =0.10.0` (default features off, so GPUI
plus `gpui-base`): a header row with a labelled Button and a text Input, a three-row list,
and a paragraph of English and Hebrew in the app's bundled fonts.

```sh
./run.sh                 # everything, with DISPLAY and WAYLAND_DISPLAY unset
cargo test --release --test interaction   # clicks, typing, IME, accessibility queries
cargo test --release --test rendering     # off-screen frames at 2x, light and dark
./scale.sh               # the same at 1x and 2x, against a 19-line patched gpui-pre
OBS_PROTO_BLESS=1 cargo test --release --test rendering   # rewrite goldens/<os>/
cargo run --release --bin capture -- out.png   # real hidden window (Windows, macOS only)
```

Frames go to `out/` (ignored); goldens to `goldens/<os>/`. Linux needs a Vulkan driver;
Mesa's lavapipe (`mesa-vulkan-drivers`) is enough. `.github/workflows/proto-374-gpui-headless.yml`
runs it all on ubuntu-latest, macos-latest and windows-latest when this branch is pushed.

Prototype-only hacks: `build.rs` and `.cargo/config.toml` let it build without the `-dev`
packages (as in `prototype/gpui-shell`); `scale.sh` patches a copy of gpui-pre in `target/`.
