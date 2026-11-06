# Floating windows (#152)

A Tab can leave the main window for an OS window of its own (a Floating window), so a Bible
can sit on one monitor and commentaries and notes on another. See ADR 0008 for why these
are immediate egui viewports drawn by the one `Shell`.

## User-facing

- Right-click a Tab's title (or its header's … menu) → **Float in new window**. A Tab in a
  Floating window offers **Move to main window** instead (and Float in new window too while
  its window holds other Tabs).
- Dragging a Tab out of its window (released outside it, or the pointer leaving the window
  mid-drag) floats it where the pointer left. A drag that ends inside the window docks as
  before. The only Tab of a Floating window is not torn out again: the window just stays.
- A Floating window has its own dock: drag Tabs within it to split and stack Panels, and
  its + opens new Tabs there. Tabs opened from one of its Tabs follow the normal placement
  rules (#18 rule 2), so they land in the Panel last used for that kind of Tab, which may be
  in the Floating window.
- Closing a Floating window moves its Panels, as they were, to the right of the main window.
  Nothing is closed.
- Link sets, focus, keyboard shortcuts and the Last session work across windows.

## Contract

- Engine (`floating_windows.rs`, `workspace.rs`): `WindowId`, `FloatingWindow`
  (`arrangement` + `placement`), `WindowPlacement` (outer top-left and inner size in egui
  points on the desktop, and the `ScreenRect` of the monitor it was on).
  `Workspace::float(tab, placement)`, `dock(tab)`, `dock_window(window)`, `window_of(tab)`,
  `window_tabs(window)`; effects `Effect::Float`, `Effect::Dock`, `Effect::DockWindow`.
  Commands `tab.float {tab}` and `tab.dock {tab}`.
- `Workspace::floating` is kept up to date by the UI after every frame's changes, like
  `Workspace::arrangement` for the main window. `WorkspaceSnapshot::windows` saves each one
  (omitted when empty, so older snapshots load and snapshots without floating windows are
  byte-for-byte what they were; `SNAPSHOT_VERSION` stays 1).
- `WindowPlacement::on_screen(screens)` moves a window too little of whose title bar is on
  any connected monitor onto the first one (the main window's), keeping its offset from its
  old monitor's corner as far as it fits and shrinking it to fit. With no monitors known it
  is left as saved.
- UI (`shell/floating.rs`): one `FloatingDock` per window (its `egui_tiles::Tree`, its
  Tabs-container → Panel map and its place). `Shell::show_floating_windows` shows each as
  `show_viewport_immediate`; the dock's behaviour is the same `State` as the main window's,
  with `st.panels` swapped for the window's map while it is drawn. The window's place is
  read from its `ViewportInfo` each frame. The app calls `Shell::set_screens` with the
  connected monitors (from winit) while `Shell::wants_screens()`.

## Tests

- Engine: `floating_windows_tests.rs` (float, dock, close a window, a floating Bible
  following its Link set, Layout save and restore with places, old snapshots, a window whose
  monitor is gone).
- UI: `floating_windows_tests.rs` through the headless harness (right-click → Float in new
  window and Link sets across windows, dragging a Tab out, closing a window, a Layout and
  the Last session, a monitor gone at restore, no idle text work in either window).
