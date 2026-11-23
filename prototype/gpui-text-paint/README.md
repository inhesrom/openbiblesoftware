# PROTOTYPE — our text pipeline painted through GPUI (#372; throwaway, never merge)

> **This is throwaway prototype code** for ticket #372 on the GPUI map #371 (ADR 0025). It
> lives only on the branch `prototype/372-gpui-text-paint`. The answer is in
> [REPORT.md](REPORT.md).

The shell slice of [`prototype/gpui-shell`](../gpui-shell) has a menu bar, Panel headers and four
linked Panels (KJV with Strong's, WEB, TAHOT, TAGNT). This prototype rebuilds it on GPUI from
`gpui-kit` 0.10.0 (default features off, which pins `gpui-pre` 0.3.12), as #373 recommends.
Resource text no longer goes through GPUI's text system:

- **Layout:** the app's own pipeline lays text out: `obs-layout` with `obs-fonts`' harfrust
  shaping, bidi, line breaking and the default `TextSettings`.
- **Painting:** each laid-out glyph is painted at our position with `Window::paint_glyph`. Its
  `FontId` comes from the same font files, registered with GPUI's text system.
- **Threads:** layout runs on worker threads into a cache keyed by Bible, block and width. The
  UI thread lays out only within a 2 ms budget per frame, or the block a jump lands in.
- **Accessibility:** each laid-out block is one GPUI element with `Role::Paragraph` and its text.
  Its Tagged words and Scripture references are synthetic `Role::Link` children.

It is a standalone crate outside the Cargo workspace, with its own `Cargo.lock`.

## Run it

```sh
cargo run --release --manifest-path prototype/gpui-text-paint/Cargo.toml
# open the first Panel at a reference (useful without input devices):
OBS_PROTO_START="Genesis 2:4" cargo run --release --manifest-path prototype/gpui-text-paint/Cargo.toml
```

The library is `$OBS_LIBRARY`, else `$XDG_DATA_HOME/OpenBible/library`. Files are opened
read-only. The keys and the reference box work as in `prototype/gpui-shell`. The mouse wheel
scrolls the Panel under the pointer.

## Compare speed with the egui app

```sh
RUNS=3 prototype/gpui-text-paint/bench.sh
```

This is the same scenario and report as `prototype/gpui-shell/bench.sh`. The egui side is the
app's `OBS_PROTO_BENCH` mode, unchanged.

- **Refresh rate:** read from Hyprland, wlroots or X11. Set `OBS_PROTO_HZ` to override it.
- **Extra section in `summary.md`:** frames that showed a block not yet laid out, UI-thread
  layout time per frame, layouts done on the UI thread and by workers, glyphs painted per frame,
  and the shape cache's hit rate.

## Hacks (prototype only)

- **`build.rs` and `.cargo/config.toml`:** symlinks to the runtime `.so` files, and fontconfig
  loaded with `dlopen`. This box has no `-dev` packages. `bench.sh` builds from this folder, so
  the config applies.
- **`x11_poke`:** wakes GPUI's X11 frame loop under Xvfb with no window manager.
- **`--x11-raise <title>`:** a helper `bench.sh` calls before each screenshot. On a display
  shared by several apps with no window manager, it brings the captured window to the top.
- **`OBS_PROTO_A11Y_DUMP=<file>`:** while an assistive technology has GPUI build its AccessKit
  tree, writes the last frame's tree as JSON.
