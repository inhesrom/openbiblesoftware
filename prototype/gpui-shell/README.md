# PROTOTYPE — GPUI shell (throwaway, never merge)

> **This is throwaway prototype code.** It lives only on the branch
> `prototype-gpui-instead-of-egui` and answers one question: *would the app look different
> or run faster if its UI were built with [GPUI](https://www.gpui.rs) (Zed's UI framework)
> instead of egui?* The answer is in [REPORT.md](REPORT.md). Nothing here is production code:
> no tests, minimal error handling, no persistence.

It is a slice of the Open Bible Software shell drawn with GPUI 0.2.2: the menu/title bar, Panel
tab headers with the Link-set chip, reference box and chapter arrows, and four Panels side by
side reading real Bibles from your library (KJV with Strong's, WEB, Hebrew TAHOT, Greek TAGNT).
All four are in Link set A: scroll or jump one and the others follow to the same verse
(through each Bible's Verse map). Resource text goes through **GPUI's own text system**
(cosmic-text shaping and GPUI's line wrapping), not the app's harfrust/bidi pipeline.

It is a standalone crate, outside the repository's Cargo workspace (own `Cargo.lock`, empty
`[workspace]`), with path dependencies on `obs-store`, `obs-model`, `obs-scripture`,
`obs-layout` and `obs-fonts`. The workspace, its lockfile and its CI gates are untouched.

## Run it

```sh
cargo run --release --manifest-path prototype/gpui-shell/Cargo.toml
# other Bibles (resource ids in your library), 2–4 of them:
cargo run --release --manifest-path prototype/gpui-shell/Cargo.toml -- --bibles kjv-strongs,tahot
```

The library is `$OBS_LIBRARY`, else `$XDG_DATA_HOME/OpenBible/library` (by default
`~/.local/share/OpenBible/library`). Files are opened read-only.

In the window: scroll with the wheel/trackpad; click a Panel's reference box (or press `g`),
type a reference (`John 3:16`, `Ps 23`) and press Enter; `‹` `›` or `[` `]` go a chapter back
or on; PageUp/PageDown/Space/arrows scroll; the round `A` chip links or unlinks a Panel.

## Compare speed with the egui app

```sh
prototype/gpui-shell/bench.sh            # builds both (release), runs each once
RUNS=3 prototype/gpui-shell/bench.sh     # alternates gpui, egui, gpui, egui, …
```

Both apps play the same frame-by-frame script ([scenario.json](scenario.json)) in a
1400×900 window (a tiling window manager may resize both alike): jump to Genesis 1:1, a screenshot, 600 frames of steady scrolling, three
flings, eight jumps across the Bible, a screenshot at John 1:1, scrolling back. The
leader Panel is scrolled (GPUI: `ListState::scroll_by`; egui: trackpad wheel events over the
pane, applied in full the same frame) or jumped (`tab.go` in egui); linked Panels follow.
Each app writes a JSON report with frame intervals and frame work (p50/p95/p99/max, missed
frames against the display's refresh), startup times, process CPU time and peak RSS;
`compare.py` puts them side by side in `summary.md`. Screenshots are taken from outside
(X11 `import`, or `grim` on wlroots Wayland) while the app holds still.

The display's refresh rate, which decides what counts as a missed frame, is read from
Hyprland (`hyprctl`), wlroots compositors (`wlr-randr`) or X11 (`xrandr`); set
`OBS_PROTO_HZ` to override it. Keep the benchmark windows visible until the run ends: a
hidden window gets no frames, and the gap is reported as a stall, not timed.

The egui side is a prototype-only mode of the real app (`OBS_PROTO_BENCH`,
`crates/ui-egui/src/proto_bench.rs`), inert unless that variable is set. It opens only the
four Bibles, with no User database, Search indexing, Starter sync, network or update check,
so both apps do comparable work. The scenario and report code is one file
([src/bench.rs](src/bench.rs)) compiled into both.

## Hacks to get GPUI building here (prototype only)

- `patches/xattr-0.2.3`: GPUI's HTTP client pulls in `xattr 0.2.3`, which uses
  `libc::ENOATTR`, gone from current `libc` on Linux; patched to `ENODATA` (same value).
- `build.rs` + `.cargo/config.toml`: build on a Linux box with only the runtime libraries
  (no `libxkbcommon-dev`, `libxcb1-dev`, `libfontconfig-dev`): symlinks to the versioned
  `.so` files, and fontconfig loaded with `dlopen`.
- `x11_poke` in `src/main.rs`: under Xvfb with no window manager, GPUI's X11 client can miss
  the first events and never start its frame loop; a property change on its window wakes
  it. Harmless on a real desktop.
