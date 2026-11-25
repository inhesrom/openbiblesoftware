//! Catalogue previews (#378): the component catalogue drawn by GPUI's headless renderer (Linux:
//! wgpu on Mesa's lavapipe, as ADR 0019's #374 amendment decided) in each Look, saved under
//! `crates/ui-gpui/previews/catalogue-<look>-2x.png`. gpui-pre 0.3.12 draws headless windows at
//! 2× only. Run with `OBS_GPUI_PREVIEWS=1 cargo test -p obs-ui-gpui --test previews`; without
//! the variable the test only checks that a frame can be drawn.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[cfg(target_os = "linux")]
mod support;

#[cfg(target_os = "linux")]
#[test]
fn catalogue_previews() {
    use std::sync::Arc;

    use gpui_kit::{AppContext as _, Bounds, HeadlessAppContext, WindowBounds, WindowOptions};
    use gpui_kit::{point, px, size};
    use obs_ui_gpui::catalogue::{Catalogue, LOOKS, look_named};

    use crate::support::Driver as _;

    let save = std::env::var_os("OBS_GPUI_PREVIEWS").is_some();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("previews");
    for (name, _, _) in LOOKS {
        let mut cx = HeadlessAppContext::with_platform(
            gpui_kit::platform::current_platform(true).text_system(),
            Arc::new(obs_ui_gpui::icons::Assets),
            gpui_kit::platform::current_headless_renderer,
        );
        cx.update(|cx| {
            obs_ui_gpui::install(cx);
            obs_ui_gpui::theme::set_look(look_named(name, 1.0).unwrap(), cx);
        });
        let (handle, _) = cx
            .update(|cx| {
                gpui_kit::open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds {
                            origin: point(px(0.), px(0.)),
                            size: size(px(1280.), px(1000.)),
                        })),
                        focus: false,
                        show: false,
                        ..Default::default()
                    },
                    cx,
                    |window, cx| cx.new(|cx| Catalogue::new(window, cx)),
                )
            })
            .expect("open window");
        cx.update_window(handle, |_, window, cx| window.frame(cx))
            .unwrap();
        let Ok(image) = cx.capture_screenshot(handle) else {
            eprintln!("previews: no headless renderer here; skipped");
            return;
        };
        assert!(image.width() > 0 && image.height() > 0);
        if save {
            let file = format!("catalogue-{}-2x.png", name.to_lowercase().replace(' ', "-"));
            image.save(dir.join(&file)).unwrap();
            eprintln!(
                "previews: wrote {file} ({}x{})",
                image.width(),
                image.height()
            );
        }
    }
}
