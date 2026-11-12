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
- Link sets, focus, keyboard shortcuts and the Last session work across windows. Keys typed
  in a window act on that window's Tab (the one last focused there), and switching to a
  window with Alt-Tab or Cmd-Tab makes its Tab the focused one. Ctrl/Cmd held in a Floating
  window (Ctrl/Cmd-double-click in its Library, say) opens in a new Panel as in the main one,
  and a Note started on a selection in a Floating window is written in that window.

## Contract

- Engine (`floating_windows.rs`, `workspace.rs`): `WindowId`, `FloatingWindow`
  (`arrangement` + `placement`), `WindowPlacement` (outer top-left in desktop units, inner
  size in egui points, and the `ScreenRect` of the monitor it was on). Desktop units are the
  OS's own window coordinates, the same across monitors: pixels on Windows and Linux, points
  on macOS. egui points are not, when monitors have different scale factors (a window's
  points are its own pixels over its own scale), so each `ScreenRect` also keeps its
  `scale`, desktop units per egui point on it (1 in snapshots saved before it existed).
  `Workspace::float(tab, placement)`, `dock(tab)`, `dock_window(window)`, `window_of(tab)`,
  `window_tabs(window)`; effects `Effect::Float`, `Effect::Dock`, `Effect::DockWindow`.
  Commands `tab.float {tab}` and `tab.dock {tab}`.
- `Workspace::floating` is kept up to date by the UI after every frame's changes, like
  `Workspace::arrangement` for the main window. `WorkspaceSnapshot::windows` saves each one
  (omitted when empty, so older snapshots load and snapshots without floating windows are
  byte-for-byte what they were; `SNAPSHOT_VERSION` stays 1).
- `WindowPlacement::on_screen(screens)` moves a window too little of whose title bar is on
  any connected monitor onto the first one (the main window's), keeping its offset in points
  from its old monitor's corner as far as it fits and shrinking it to fit. With no monitors
  known it is left as saved.
- UI (`shell/floating.rs`): one `FloatingDock` per window (its `egui_tiles::Tree`, its
  Tabs-container → Panel map and its place). `Shell::show_floating_windows` shows each as
  `show_viewport_immediate`; the dock's behaviour is the same `State` as the main window's,
  with `st.panels` swapped for the window's map while it is drawn. The window's place is
  read from its `ViewportInfo` each frame and turned into desktop units at that window's own
  scale (`DesktopUnits::per_point`); a window opens at its desktop place over its screen's
  scale and, if the OS opened it at another scale, is moved there in its own points (at most
  three times). The app calls `Shell::set_screens` with the connected monitors from winit
  (`DesktopUnits::screen`) while `Shell::wants_screens()`.
- Per window, not shared: the keyboard (`Shell::focus_in`, the focused Tab if it is in that
  window, else the one last focused there; `Event::WindowFocused(true)` makes it the focused
  Tab), the Ctrl/Cmd split modifier (`State::split_held`, set from the floating window's
  input while it is drawn) and the Note editor (drawn by the window whose Tab it is on).
  Floating windows embedded in the main one (web, the default headless harness) take the main
  window's keys.
- A Tab dragged out keeps the pointer's last desktop position while dragged, so a pointer
  that leaves the window (`PointerGone`) still places the new window where it left.
- macOS title bar (#297): with `Shell::set_unified_title_bar`, a Floating window's viewport is
  built with `unified_title_bar` too (its title, the Panels' active Tabs, is kept for the
  Window menu and VoiceOver but not shown), and its top tab bars are its title bar: the dock
  starts at the window's top edge (centred on the traffic lights as near as the tab bar's
  height allows), and the tab bar under the traffic lights leaves them their room
  (`State::traffic_lights`, `Behavior::tab_bar_left_ui`). That room moves the window when
  dragged and zooms or minimizes it when double-clicked, as the main window's menu bar does;
  the tab bar's own empty space keeps its own meaning (it moves its Panel when the window
  holds more than one). In full screen the room collapses.
  See [menus.md](menus.md#the-macos-title-bar-297).

## Tests

- Engine: `floating_windows_tests.rs` (float, dock, close a window, a floating Bible
  following its Link set, Layout save and restore with places, old snapshots, a window whose
  monitor is gone).
- UI: `floating_windows_tests.rs` through the headless harness (right-click → Float in new
  window and Link sets across windows, dragging a Tab out, also as the pointer leaves the
  window, closing a window, a Layout and the Last session, a monitor gone at restore, no idle
  text work in either window). `Harness::native_windows` draws floating windows as eframe
  does, as immediate viewports with input of their own: Alt-Tab then Ctrl+W in each window,
  Ctrl held only in a floating Library, a Note written and saved in a floating window, and a
  window on a 2× monitor beside a 1× one (placed, read back, moved when opened at the wrong
  scale, restored onto the 1× monitor).
