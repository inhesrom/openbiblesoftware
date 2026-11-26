//! Headless rendering and goldens for GPUI surfaces (#379, ADR 0019 as amended by #374).
//!
//! Its own harness, because GPUI's headless renderer runs on the main thread (as GPUI Kit's
//! own rendering tests do). `cargo test -p obs-ui-gpui --test rendering`:
//! - **Linux:** frames from GPUI's headless renderer (wgpu, Mesa lavapipe on a computer with no
//!   GPU) are compared with the PNG goldens in `goldens/linux/` under ADR 0019's tolerance.
//!   `UPDATE_GOLDEN=1` writes goldens that are missing or off; without it a missing golden
//!   fails.
//! - **macOS:** no goldens (its text rasterizer differs from Linux's): each surface is drawn
//!   three times and must be identical and not blank.
//! - **Windows:** skipped; GPUI has no headless renderer there.
//!
//! With no usable headless renderer (Linux without Mesa's Vulkan drivers), the test prints why
//! and passes, unless `OBS_GPUI_REQUIRE_RENDERER=1` (CI) makes that a failure.
//!
//! Frames go to `ui-gpui-frames/` in the target folder (`CARGO_TARGET_DIR`, else the
//! workspace's `target/`) for review. Only 2× for now: gpui-pre 0.3.12's headless windows are
//! fixed at 2× (docs/dev/ui-gpui.md).
//!
//! Test runners call the binary with libtest's arguments: it lists itself as one test,
//! `rendering`, for `--list` (nextest), and honours a name filter, `--exact` and `--skip`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

/// The one test this harness runs, by the name test runners list and filter it by.
const TEST_NAME: &str = "rendering";

/// What the command line asks of the harness, in the libtest arguments test runners pass:
/// cargo test (`[FILTER...] [--exact] [--skip FILTER]`) and nextest (`--list --format terse`
/// to list, `rendering --exact --nocapture` to run).
#[derive(Debug, PartialEq, Eq)]
enum Request {
    /// `--list`: print the test, as `rendering: test` (none with `--ignored`; it is not ignored).
    List { ignored_only: bool },
    /// Run the test.
    Run,
    /// The filters, `--skip` or `--ignored` leave nothing to run.
    Nothing,
}

fn request(args: &[String]) -> Request {
    // libtest options that take a value, so it is not read as a filter.
    const VALUED: [&str; 7] = [
        "--format",
        "--test-threads",
        "--skip",
        "--color",
        "--logfile",
        "--shuffle-seed",
        "-Z",
    ];
    let mut filters = Vec::new();
    let mut skips = Vec::new();
    let (mut list, mut ignored_only, mut exact) = (false, false, false);
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--list" => list = true,
            "--ignored" => ignored_only = true,
            "--exact" => exact = true,
            "--skip" => skips.extend(args.next().cloned()),
            a if VALUED.contains(&a) => {
                args.next();
            }
            a if a.starts_with('-') => {}
            a => filters.push(a.to_owned()),
        }
    }
    if list {
        return Request::List { ignored_only };
    }
    let matches = |f: &String| {
        if exact {
            f == TEST_NAME
        } else {
            TEST_NAME.contains(f.as_str())
        }
    };
    let selected = filters.is_empty() || filters.iter().any(matches);
    if ignored_only || !selected || skips.iter().any(matches) {
        Request::Nothing
    } else {
        Request::Run
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match request(&args) {
        Request::List { ignored_only } => {
            if !ignored_only {
                println!("{TEST_NAME}: test");
            }
        }
        Request::Nothing => println!("{TEST_NAME}: filtered out"),
        Request::Run => run(),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn run() {
    println!("rendering: skipped; GPUI has no headless renderer on this platform (ADR 0019, #374)");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod support;

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn run() {
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

    /// The Resource Tabs drawn (#380, #385): golden name, and each Panel's Resource and
    /// reference, left to right.
    const READING: [(&str, &[(&str, &str)]); 5] = [
        ("reading-kjv-gen-1-light-2x", &[("kjv-strongs", "Gen 1:1")]),
        (
            "reading-hebrew-gen-1-light-2x",
            &[("tahot-test", "Gen 1:1")],
        ),
        (
            "reading-greek-john-1-light-2x",
            &[("tagnt-test", "John 1:1")],
        ),
        (
            "workspace-two-panels-light-2x",
            &[("kjv-strongs", "Gen 1:1"), ("bsb", "Gen 1:1")],
        ),
        (
            "workspace-kjv-hebrew-light-2x",
            &[("kjv-strongs", "Gen 1:1"), ("tahot-test", "Gen 1:1")],
        ),
    ];

    /// A fresh context and window in the Light look showing each of `panels`' Resources at its
    /// reference in a Resource Tab, each Tab in its own Panel at the right of the one before;
    /// their layouts settled; one capture.
    fn shoot_reading(panels: &[(&str, &str)]) -> RgbaImage {
        let mut cx = context();
        let (mut session, lab) = support::session(&format!("render-reading-{}", panels.len()));
        for r in [support::hebrew_genesis_1(), support::greek_john_1()] {
            support::add_resource(&mut session, &lab, &r);
        }
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
        let tabs = cx
            .update_window(handle, |_, window, cx| {
                window.frame(cx);
                shell.update(cx, |s, cx| {
                    assert!(s.run("view.theme", &json!({ "value": "light" })).is_some());
                    assert!(
                        s.run("view.reduce_motion", &json!({ "on": true }))
                            .is_some()
                    );
                    let mut tabs = Vec::new();
                    for (i, (resource, reference)) in panels.iter().enumerate() {
                        // Each after the first in a new Panel at the right.
                        let tab = s
                            .run("resource.open", &json!({ "id": resource, "split": i > 0 }))
                            .and_then(|v| v["tab"].as_u64())
                            .unwrap_or_else(|| panic!("{resource} did not open"));
                        s.run("tab.go", &json!({ "tab": tab, "text": reference }))
                            .unwrap_or_else(|| panic!("{resource} at {reference}"));
                        tabs.push(obs_engine::TabId(tab));
                    }
                    cx.notify();
                    tabs
                })
            })
            .unwrap_or_else(|e| panic!("{e}"));
        // One frame opens the panes and puts them at their references.
        let _ = frame(&mut cx, handle);
        let panes: Vec<_> = cx.update(|cx| {
            tabs.iter()
                .map(|t| shell.read(cx).pane(*t).expect("a reading pane").clone())
                .collect()
        });
        // Every block in view laid out (the workers deliver between frames), then one frame.
        for _ in 0..200 {
            let _ = cx.update_window(handle, |_, window, cx| window.frame(cx));
            let busy = cx.update(|cx| panes.iter().any(|p| p.read(cx).view().has_pending()));
            if !busy {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let image = frame(&mut cx, handle);
        drop(panes);
        close(cx, shell, lab);
        image
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
        let (handle, shell, lab) = open(&mut cx, look);
        let image = frame(&mut cx, handle);
        close(cx, shell, lab);
        image
    }

    /// Shut the app down, so the Session closes its User database, before the Lab removes the
    /// scratch folder (Windows cannot remove a file that is open).
    fn close(cx: HeadlessAppContext, shell: Entity<Shell>, lab: support::Lab) {
        drop(shell);
        drop(cx);
        let root = lab.root.clone();
        drop(lab);
        assert!(!root.exists(), "{} is left behind", root.display());
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
        // The reading pane (#380): Latin, Hebrew (right to left, with points and
        // cantillation) and Greek, each through our layout and GPUI's glyph painter.
        for (name, panels) in READING {
            let shots: Vec<RgbaImage> = (0..3).map(|_| shoot_reading(panels)).collect();
            let first = &shots[0];
            save(first, &support::frame_dir().join(format!("{name}.png")));
            if !support::not_blank(first) {
                failures.push(format!("{name}: the frame is blank"));
            }
            let identical = shots.iter().all(|s| s.as_raw() == first.as_raw());
            if !identical {
                failures.push(format!("{name}: the three captures differ"));
            }
            println!(
                "determinism: {name}: 3 captures, {}x{} px, identical: {identical}",
                first.width(),
                first.height()
            );
            if GOLDENS && let Some(failure) = check_golden(name, first) {
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

    /// Compare `frame` with its golden; `UPDATE_GOLDEN=1` writes it when it is missing or off.
    /// A missing golden fails otherwise, so a golden left out of a commit is noticed.
    fn check_golden(name: &str, frame: &RgbaImage) -> Option<String> {
        let dir = support::golden_dir();
        let path = dir.join(format!("{name}.png"));
        if !path.exists() {
            if support::bless() {
                std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
                save(frame, &path);
                println!("golden: wrote {} (new)", path.display());
                return None;
            }
            save(
                frame,
                &support::frame_dir().join(format!("{name}-actual.png")),
            );
            return Some(format!(
                "{name} has no golden ({}); rerun with UPDATE_GOLDEN=1 and review",
                path.display()
            ));
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
        let golden = match image::open(&golden_path) {
            Ok(golden) => golden.to_rgba8(),
            Err(e) => {
                return Some(format!(
                    "seeded change: no Light golden to compare with ({}: {e})",
                    golden_path.display()
                ));
            }
        };
        let mut cx = context();
        let (handle, shell, lab) = open(&mut cx, "light");
        let mut seeded = obs_ui_gpui::tokens::LIGHT;
        // `surface` (244, 246, 244) to (240, 242, 240): 4 levels, past the ±2 channel tolerance.
        seeded.surface = obs_engine::Rgb::new(240, 242, 240);
        cx.update(|cx| {
            shell.update(cx, |s, cx| {
                s.override_chrome(Some(seeded));
                cx.notify();
            })
        });
        let changed = frame(&mut cx, handle);
        close(cx, shell, lab);
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
