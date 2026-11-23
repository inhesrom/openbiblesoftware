//! PROTOTYPE (#374): draw the surface off-screen with GPUI's headless renderer (wgpu on Linux,
//! Metal on macOS) and compare frames with ADR 0019's tolerance. Its own harness, because the
//! renderer must run on the main thread (as gpui-kit's own `rendering` target does).
//!
//! `cargo test --release --test rendering` writes PNGs to `out/` and compares them with the
//! goldens in `goldens/<os>/`; `OBS_PROTO_BLESS=1` rewrites the goldens.

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn main() {
    println!("rendering: skipped; GPUI has no headless renderer on this platform");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn main() {
    rendering::run();
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod rendering {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        AnyWindowHandle, AppContext as _, Bounds, HeadlessAppContext, WindowBounds,
        WindowOptions, point, px, size,
    };
    use image::RgbaImage;
    use obs_prototype_gpui_headless::{Surface, register_fonts};

    /// The window size in points, the same for every capture.
    const WINDOW: (f32, f32) = (480., 260.);

    /// ADR 0019: a pixel matches when no channel is more than 2 levels off; an image matches
    /// when at most 0.01 % of its pixels differ beyond that.
    const CHANNEL_TOLERANCE: u8 = 2;
    const PIXEL_SHARE: f64 = 0.0001;

    fn os() -> &'static str {
        std::env::consts::OS
    }

    fn out_dir() -> PathBuf {
        let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("out");
        std::fs::create_dir_all(&d).expect("out dir");
        d
    }

    fn golden_dir() -> PathBuf {
        let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("goldens").join(os());
        std::fs::create_dir_all(&d).expect("golden dir");
        d
    }

    struct Shot {
        image: RgbaImage,
        /// Creating the context: platform text system, wgpu device (shared after the first).
        setup: Duration,
        /// Opening the window and laying it out, then drawing one frame.
        frame: Duration,
        /// Rasterizing the scene and reading it back as RGBA.
        capture: Duration,
    }

    fn context() -> HeadlessAppContext {
        let mut cx = HeadlessAppContext::with_platform(
            gpui_kit::platform::current_platform(true).text_system(),
            Arc::new(()),
            gpui_kit::platform::current_headless_renderer,
        );
        cx.update(|cx| {
            gpui_kit::init(cx);
            register_fonts(cx);
        });
        cx
    }

    fn open(cx: &mut HeadlessAppContext, dark: bool) -> AnyWindowHandle {
        let (handle, _) = cx
            .update(|cx| {
                gpui_kit::open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds {
                            origin: point(px(0.), px(0.)),
                            size: size(px(WINDOW.0), px(WINDOW.1)),
                        })),
                        focus: false,
                        show: false,
                        ..Default::default()
                    },
                    cx,
                    |window, cx| cx.new(|cx| Surface::new(dark, window, cx)),
                )
            })
            .expect("open window");
        handle
    }

    /// A fresh context and window, one frame, one capture.
    fn shoot(dark: bool, scale: Option<f32>) -> Shot {
        let t = Instant::now();
        let mut cx = context();
        let setup = t.elapsed();
        let t = Instant::now();
        let handle = open(&mut cx, dark);
        if let Some(scale) = scale {
            set_scale(&mut cx, handle, scale);
        }
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
        let frame = t.elapsed();
        let t = Instant::now();
        let image = cx
            .capture_screenshot(handle)
            .expect("an off-screen renderer must be available");
        let capture = t.elapsed();
        Shot {
            image,
            setup,
            frame,
            capture,
        }
    }

    #[cfg(feature = "scale")]
    fn set_scale(cx: &mut HeadlessAppContext, handle: AnyWindowHandle, scale: f32) {
        cx.simulate_window_scale_factor_change(handle, scale);
    }

    #[cfg(not(feature = "scale"))]
    fn set_scale(_: &mut HeadlessAppContext, _: AnyWindowHandle, scale: f32) {
        assert_eq!(
            scale, 2.0,
            "gpui-pre 0.3.12's headless windows are fixed at 2x; build with --features scale"
        );
    }

    /// Pixels differing beyond the channel tolerance, and whether that is within ADR 0019.
    fn compare(a: &RgbaImage, b: &RgbaImage) -> (usize, usize, u8, bool) {
        assert_eq!(a.dimensions(), b.dimensions(), "sizes differ");
        let mut off = 0;
        let mut max = 0u8;
        for (p, q) in a.pixels().zip(b.pixels()) {
            let d = (0..4).map(|i| p[i].abs_diff(q[i])).max().unwrap_or(0);
            max = max.max(d);
            if d > CHANNEL_TOLERANCE {
                off += 1;
            }
        }
        let total = (a.width() * a.height()) as usize;
        (off, total, max, (off as f64) <= PIXEL_SHARE * total as f64)
    }

    fn name(dark: bool, scale: f32) -> String {
        format!(
            "surface-{}-{}x",
            if dark { "dark" } else { "light" },
            scale as u32
        )
    }

    pub fn run() {
        let _ = env_logger::try_init();
        let bless = std::env::var("OBS_PROTO_BLESS").as_deref() == Ok("1");
        let scales: &[f32] = if cfg!(feature = "scale") {
            &[1.0, 2.0]
        } else {
            &[2.0]
        };
        let mut failures = Vec::new();
        for &scale in scales {
            for dark in [false, true] {
                let n = name(dark, scale);
                // Three captures from fresh contexts: the pixels must be identical.
                let shots: Vec<Shot> = (0..3).map(|_| shoot(dark, Some(scale))).collect();
                for (i, s) in shots.iter().enumerate() {
                    println!(
                        "timing: {n} run {}: setup {:.1} ms, frame {:.1} ms, capture {:.1} ms ({}x{} px)",
                        i + 1,
                        s.setup.as_secs_f64() * 1000.0,
                        s.frame.as_secs_f64() * 1000.0,
                        s.capture.as_secs_f64() * 1000.0,
                        s.image.width(),
                        s.image.height()
                    );
                }
                let first = &shots[0].image;
                first.save(out_dir().join(format!("{n}.png"))).unwrap();
                for (i, s) in shots.iter().enumerate().skip(1) {
                    let identical = s.image.as_raw() == first.as_raw();
                    println!("determinism: {n} run {} identical to run 1: {identical}", i + 1);
                    if !identical {
                        let (off, total, max, ok) = compare(first, &s.image);
                        println!("  {off} of {total} pixels off by > {CHANNEL_TOLERANCE}, max {max}, within tolerance {ok}");
                        failures.push(format!("{n}: run {} differs from run 1", i + 1));
                    }
                }
                // The golden for this OS.
                let golden = golden_dir().join(format!("{n}.png"));
                if bless || !golden.exists() {
                    first.save(&golden).unwrap();
                    println!("golden: wrote {}", golden.display());
                } else {
                    let g = image::open(&golden).unwrap().to_rgba8();
                    let (off, total, max, ok) = compare(&g, first);
                    println!(
                        "golden: {n}: {off} of {total} pixels off by > {CHANNEL_TOLERANCE} (max {max}): {}",
                        if ok { "match" } else { "MISMATCH" }
                    );
                    if !ok {
                        first.save(out_dir().join(format!("{n}-actual.png"))).unwrap();
                        failures.push(format!("{n}: does not match its golden"));
                    }
                }
            }
        }
        // The comparison catches a change: one more click on Save redraws "Save (1)", and a
        // dark page instead of a light one changes almost everything.
        {
            let mut cx = context();
            let handle = open(&mut cx, false);
            cx.update_window(handle, |_, window, cx| window.render_frame(cx))
                .unwrap();
            let before = cx.capture_screenshot(handle).unwrap();
            cx.update_window(handle, |_, window, cx| window.click("save", cx))
                .unwrap();
            cx.update_window(handle, |_, window, cx| window.render_frame(cx))
                .unwrap();
            let after = cx.capture_screenshot(handle).unwrap();
            after.save(out_dir().join("surface-light-after-click.png")).unwrap();
            let (off, total, max, ok) = compare(&before, &after);
            println!("seeded change (one click on Save): {off} of {total} pixels differ (max {max}); within tolerance: {ok}");
            if ok {
                failures.push("a click on Save went unnoticed by the comparison".into());
            }
        }
        if failures.is_empty() {
            println!("rendering: passed (os {}, scales {scales:?})", os());
        } else {
            for f in &failures {
                eprintln!("FAIL: {f}");
            }
            std::process::exit(1);
        }
    }
}
