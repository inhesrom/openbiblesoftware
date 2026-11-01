# Upstream egui from crates.io with egui_tiles; no ArtCraft egui fork or craft-libs dependency

We build on upstream `egui`/`eframe` 0.37 from crates.io and use `egui_tiles` (MIT/Apache, already on egui 0.37) for docked Panels and Tabs. We do not depend on storytold's egui/winit forks or on craft-libs. The main reason to take the fork was bidi/RTL for Hebrew. Since ADR 0004 moved all Resource text to our own pipeline, egui now draws only chrome, and that reason is gone. What remains is a fork pinned by rev that is still changing (its PATCHES.md lists uncommitted local repairs) and is not on crates.io. Every egui-based craft-libs crate requires it, including the `craft-ui` docking we would otherwise want. We copy useful WordCraft and craft-libs code with attribution (MIT OR Apache-2.0) instead of depending on it.

## Considered Options

- **storytold fork + craft-libs (craft-ui docking, i18n, keymap):** most features out of the box (IME fixes, Wayland clipboard, shortcuts on non-Latin keyboard layouts, docking). But we inherit the fork's churn, and only one egui can resolve in the build.
- **Upstream egui + egui_dock:** egui_dock 0.21 still targets egui 0.36, so it lags behind.
- **Upstream egui + egui_tiles (chosen):** current with egui 0.37, serde-serializable trees (good for Layouts and Last session), and proven in the Rerun viewer. Floating windows are not supported, but the v0.1 feature cut already deferred them.

## Consequences

- In v0.1 we accept stock egui's IME, Wayland clipboard and non-Latin shortcut gaps. All egui crates come in through `[workspace.dependencies]`, so moving to the fork later is a single `[patch.crates-io]` block. That switch is cheap as long as we haven't adopted craft-libs UI crates.
- Docking UI follows egui_tiles' model (tiles, tabs, splits). The Workspace model in the engine stays our own type and converts to and from the egui_tiles tree in `ui-egui`.
