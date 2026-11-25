//! Headless rendering and goldens for GPUI surfaces (#379, ADR 0019 as amended by #374).
//!
//! Its own harness, because GPUI's headless renderer runs on the main thread (as GPUI Kit's
//! own rendering tests do). `cargo test -p obs-ui-gpui --test rendering`:
//! - **Linux:** frames from GPUI's headless renderer (wgpu, Mesa lavapipe on a computer with no
//!   GPU) are compared with the PNG goldens in `goldens/linux/` under ADR 0019's tolerance.
//!   `UPDATE_GOLDEN=1` writes goldens that are missing or off.
//! - **macOS:** no goldens (its text rasterizer differs from Linux's): each surface is drawn
//!   three times and must be identical and not blank.
//! - **Windows:** skipped; GPUI has no headless renderer there.
//!
//! With no usable headless renderer (Linux without Mesa's Vulkan drivers), the test prints why
//! and passes, unless `OBS_GPUI_REQUIRE_RENDERER=1` (CI) makes that a failure.
//!
//! Frames go to `target/ui-gpui-frames/` for review. Only 2× for now: gpui-pre 0.3.12's
//! headless windows are fixed at 2× (docs/dev/ui-gpui.md).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn main() {
    println!("rendering: skipped; GPUI has no headless renderer on this platform (ADR 0019, #374)");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod support;

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn main() {
    rendering::run();
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod rendering {
    use std::sync::Arc;

    use gpui_kit::{AnyWindowHandle, Bounds, Entity, HeadlessAppContext, point, px, size};
    use image::RgbaImage;
    use obs_ui_gpui::Shell;
    use serde_json::json;

    use crate::support::{self, Driver as _};

    /// The Looks drawn, by the theme ids of `view.theme`.
    const LOOKS: [&str; 2] = ["light", "dark"];

    /// Goldens are compared on Linux only (ADR 0019 as amended by #374).
    const GOLDENS: bool = cfg!(target_os = "linux");

    fn context() -> HeadlessAppContext {
        let mut cx = HeadlessAppContext::with_platform(
            gpui_kit::platform::current_platform(true).text_system(),
            Arc::new(()),
            gpui_kit::platform::current_headless_renderer,
        );
        cx.update(obs_ui_gpui::install);
        cx
    }

    /// The scaffold's main window over the fixtures' Session, in `look`.
    fn open(
        cx: &mut HeadlessAppContext,
        look: &str,
    ) -> (AnyWindowHandle, Entity<Shell>, support::Lab) {
        let (session, lab) = support::session(&format!("render-{look}"));
        let (handle, shell) = cx
            .update(|cx| {
                let bounds = Bounds {
                    origin: point(px(0.), px(0.)),
                    size: size(
                        px(obs_ui_gpui::tokens::WINDOW_WIDTH),
                        px(obs_ui_gpui::tokens::WINDOW_HEIGHT),
                    ),
                };
                let mut options = obs_ui_gpui::main_window_options(Some(bounds), cx);
                options.focus = false;
                options.show = false;
                obs_ui_gpui::open_main_window(session, options, cx)
            })
            .unwrap_or_else(|e| panic!("the window did not open: {e}"));
        cx.update(|cx| {
            shell.update(cx, |s, cx| {
                assert!(
                    s.run("view.theme", &json!({ "value": look })).is_some(),
                    "view.theme {look}"
                );
                cx.notify();
            })
        });
        (handle, shell, lab)
    }

    fn frame(cx: &mut HeadlessAppContext, handle: AnyWindowHandle) -> RgbaImage {
        cx.update_window(handle, |_, window, cx| window.frame(cx))
            .unwrap_or_else(|e| panic!("render_frame: {e}"));
        cx.capture_screenshot(handle)
            .unwrap_or_else(|e| panic!("capture: {e}"))
    }

    /// A fresh context and window in `look`, one frame, one capture.
    fn shoot(look: &str) -> RgbaImage {
        let mut cx = context();
        let (handle, _, _lab) = open(&mut cx, look);
        frame(&mut cx, handle)
    }

    pub fn run() {
        let require = std::env::var("OBS_GPUI_REQUIRE_RENDERER").as_deref() == Ok("1");
        if let Err(why) = renderer_available() {
            let message = format!(
                "rendering: no headless renderer ({why}); on Linux install Mesa's Vulkan \
                 drivers (mesa-vulkan-drivers)"
            );
            if require {
                eprintln!("FAIL: {message}");
                std::process::exit(1);
            }
            println!("{message}: skipped");
            return;
        }
        let mut failures = Vec::new();
        for look in LOOKS {
            let name = format!("workspace-empty-{look}-2x");
            // Three captures from fresh contexts: the pixels must be identical, and drawn.
            let shots: Vec<RgbaImage> = (0..3).map(|_| shoot(look)).collect();
            let first = &shots[0];
            save(first, &support::frame_dir().join(format!("{name}.png")));
            if !support::not_blank(first) {
                failures.push(format!("{name}: the frame is blank"));
            }
            for (i, shot) in shots.iter().enumerate().skip(1) {
                if shot.as_raw() != first.as_raw() {
                    let c = support::compare(first, shot);
                    failures.push(format!(
                        "{name}: capture {} differs from capture 1 ({} of {} pixels, max {})",
                        i + 1,
                        c.off,
                        c.total,
                        c.max
                    ));
                }
            }
            println!(
                "determinism: {name}: 3 captures, {}x{} px, identical: {}",
                first.width(),
                first.height(),
                shots.iter().all(|s| s.as_raw() == first.as_raw())
            );
            if GOLDENS && let Some(failure) = check_golden(&name, first) {
                failures.push(failure);
            }
        }
        if GOLDENS && let Some(failure) = seeded_change() {
            failures.push(failure);
        }
        if failures.is_empty() {
            println!(
                "rendering: passed ({}, goldens: {GOLDENS})",
                std::env::consts::OS
            );
        } else {
            for f in &failures {
                eprintln!("FAIL: {f}");
            }
            std::process::exit(1);
        }
    }

    /// Whether GPUI can draw headlessly here (a wgpu adapter on Linux, Metal on macOS).
    fn renderer_available() -> Result<(), String> {
        match gpui_kit::platform::current_headless_renderer() {
            Ok(Some(_)) => Ok(()),
            Ok(None) => Err("none on this platform".into()),
            Err(e) => Err(e.to_string()),
        }
    }

    fn save(image: &RgbaImage, path: &std::path::Path) {
        image
            .save(path)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    }

    /// Compare `frame` with its golden; `UPDATE_GOLDEN=1`, or no golden yet, writes it.
    fn check_golden(name: &str, frame: &RgbaImage) -> Option<String> {
        let dir = support::golden_dir();
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
        let path = dir.join(format!("{name}.png"));
        if !path.exists() {
            save(frame, &path);
            println!("golden: wrote {} (new)", path.display());
            return None;
        }
        let golden = image::open(&path)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
            .to_rgba8();
        let c = support::compare(&golden, frame);
        println!(
            "golden: {name}: {} of {} pixels off by more than {} (max {}): {}",
            c.off,
            c.total,
            support::CHANNEL_TOLERANCE,
            c.max,
            if c.matches { "match" } else { "MISMATCH" }
        );
        if c.matches {
            return None;
        }
        if support::bless() {
            save(frame, &path);
            println!("golden: rewrote {}", path.display());
            return None;
        }
        save(
            frame,
            &support::frame_dir().join(format!("{name}-actual.png")),
        );
        Some(format!(
            "{name} is off its golden: {} of {} pixels; rerun with UPDATE_GOLDEN=1 and review",
            c.off, c.total
        ))
    }

    /// One colour token changed on purpose (the Workspace's surface, by a few levels) must
    /// fail the comparison with the Light golden: the gallery catches a token change.
    fn seeded_change() -> Option<String> {
        let golden_path = support::golden_dir().join("workspace-empty-light-2x.png");
        let golden = image::open(&golden_path)
            .unwrap_or_else(|e| panic!("{}: {e}", golden_path.display()))
            .to_rgba8();
        let mut cx = context();
        let (handle, shell, _lab) = open(&mut cx, "light");
        let mut seeded = obs_ui_gpui::tokens::light();
        // `surface` 0xf4f6f4 to 0xf0f2f0: 4 levels, past the ±2 channel tolerance.
        seeded.surface = gpui_kit::rgb(0xf0f2f0);
        cx.update(|cx| {
            shell.update(cx, |s, cx| {
                s.override_chrome(Some(seeded));
                cx.notify();
            })
        });
        let changed = frame(&mut cx, handle);
        save(
            &changed,
            &support::frame_dir().join("workspace-empty-light-2x-seeded.png"),
        );
        let c = support::compare(&golden, &changed);
        println!(
            "seeded change (surface token, 4 levels): {} of {} pixels off (max {}); caught: {}",
            c.off, c.total, c.max, !c.matches
        );
        if c.matches {
            Some("a changed colour token went unnoticed by the golden comparison".into())
        } else {
            None
        }
    }
}
