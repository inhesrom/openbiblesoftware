//! PROTOTYPE (#374): capture the surface from a real, never-shown window on the real platform,
//! without GPUI's test platform. This is the only screenshot path GPUI has on Windows
//! (`gpui_windows`'s Direct3D renderer reads its render target back under `test-support`);
//! it works on macOS too (Metal). Linux's X11 and Wayland windows have no such path: there the
//! headless renderer in `tests/rendering.rs` is used instead.
//!
//!     capture <out.png> [light|dark]     draw one frame, save it, quit
//!     capture compare <a.png> <b.png>    compare two captures with ADR 0019's tolerance

use std::time::{Duration, Instant};

use gpui_kit::{AppContext as _, Bounds, WindowBounds, WindowOptions, point, px, size};
use obs_prototype_gpui_headless::{Surface, register_fonts};

const CHANNEL_TOLERANCE: u8 = 2;
const PIXEL_SHARE: f64 = 0.0001;

fn compare(a: &str, b: &str) -> bool {
    let a = image::open(a).expect("first image").to_rgba8();
    let b = image::open(b).expect("second image").to_rgba8();
    if a.dimensions() != b.dimensions() {
        println!("compare: sizes differ: {:?} and {:?}", a.dimensions(), b.dimensions());
        return false;
    }
    let mut off = 0usize;
    let mut max = 0u8;
    for (p, q) in a.pixels().zip(b.pixels()) {
        let d = (0..4).map(|i| p[i].abs_diff(q[i])).max().unwrap_or(0);
        max = max.max(d);
        if d > CHANNEL_TOLERANCE {
            off += 1;
        }
    }
    let total = (a.width() * a.height()) as usize;
    let ok = (off as f64) <= PIXEL_SHARE * total as f64;
    println!(
        "compare: {off} of {total} pixels off by > {CHANNEL_TOLERANCE} (max {max}), identical {}: {}",
        a.as_raw() == b.as_raw(),
        if ok { "match" } else { "MISMATCH" }
    );
    ok
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("compare") {
        let ok = compare(&args[1], &args[2]);
        std::process::exit(if ok { 0 } else { 1 });
    }
    let out = args.first().cloned().unwrap_or_else(|| "capture.png".into());
    let dark = args.get(1).map(String::as_str) == Some("dark");
    let started = Instant::now();
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        register_fonts(cx);
        let (handle, _) = gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: point(px(0.), px(0.)),
                    size: size(px(480.), px(260.)),
                })),
                focus: false,
                show: false,
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| Surface::new(dark, window, cx)),
        )
        .expect("open window");
        cx.spawn(async move |cx| {
            // Let the platform finish creating the window and its renderer.
            cx.background_executor()
                .timer(Duration::from_millis(300))
                .await;
            let result = cx.update_window(handle, |_, window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
                let t = Instant::now();
                let image = window.render_to_image();
                (image, t.elapsed(), window.scale_factor())
            });
            match result {
                Ok((Ok(image), took, scale)) => {
                    image.save(&out).expect("save PNG");
                    println!(
                        "capture: {out}: {}x{} px at scale {scale}, capture {:.1} ms, {:.0} ms since start",
                        image.width(),
                        image.height(),
                        took.as_secs_f64() * 1000.0,
                        started.elapsed().as_secs_f64() * 1000.0
                    );
                }
                Ok((Err(e), _, _)) => {
                    eprintln!("capture: render_to_image failed: {e:#}");
                    std::process::exit(2);
                }
                Err(e) => {
                    eprintln!("capture: window gone: {e:#}");
                    std::process::exit(2);
                }
            }
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
}
