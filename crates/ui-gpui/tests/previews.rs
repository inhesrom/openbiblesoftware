//! Catalogue previews (#378): the component catalogue drawn by GPUI's headless renderer (Linux:
//! wgpu on Mesa's lavapipe, as ADR 0019's #374 amendment decided) in each Look, saved under
//! `crates/ui-gpui/previews/catalogue-<look>-2x.png`. gpui-pre 0.3.12 draws headless windows at
//! 2× only. Run with `OBS_GPUI_PREVIEWS=1 cargo test -p obs-ui-gpui --test previews`; without
//! the variable the test only checks that a frame can be drawn.
//!
//! Also from the headless renderer: the keyboard focus rings' pixels, where a ring could be
//! drawn in the colour it sits on.
//!
//! With no headless renderer the tests say why and pass, unless `OBS_GPUI_REQUIRE_RENDERER=1`
//! (CI) makes that a failure, as in `tests/rendering.rs`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[cfg(target_os = "linux")]
mod support;

/// Whether GPUI can draw headlessly here. Without a renderer this panics when
/// `OBS_GPUI_REQUIRE_RENDERER=1`, and otherwise says why the test is skipped.
#[cfg(target_os = "linux")]
fn renderer_available(test: &str) -> bool {
    let why = match gpui_kit::platform::current_headless_renderer() {
        Ok(Some(_)) => return true,
        Ok(None) => "none on this platform".to_string(),
        Err(e) => e.to_string(),
    };
    let message = format!(
        "{test}: no headless renderer ({why}); on Linux install Mesa's Vulkan drivers \
         (mesa-vulkan-drivers)"
    );
    assert!(
        std::env::var("OBS_GPUI_REQUIRE_RENDERER").as_deref() != Ok("1"),
        "{message}"
    );
    eprintln!("{message}: skipped");
    false
}

#[cfg(target_os = "linux")]
fn headless_context() -> gpui_kit::HeadlessAppContext {
    let mut cx = gpui_kit::HeadlessAppContext::with_platform(
        gpui_kit::platform::current_platform(true).text_system(),
        std::sync::Arc::new(obs_ui_gpui::icons::Assets),
        gpui_kit::platform::current_headless_renderer,
    );
    cx.update(obs_ui_gpui::install);
    cx
}

#[cfg(target_os = "linux")]
fn hidden_window(width: f32, height: f32) -> gpui_kit::WindowOptions {
    use gpui_kit::{Bounds, WindowBounds, WindowOptions, point, px, size};
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.), px(0.)),
            size: size(px(width), px(height)),
        })),
        focus: false,
        show: false,
        ..Default::default()
    }
}

#[cfg(target_os = "linux")]
#[test]
fn catalogue_previews() {
    use gpui_kit::AppContext as _;
    use obs_ui_gpui::catalogue::{Catalogue, LOOKS, look_named};

    use crate::support::Driver as _;

    if !renderer_available("previews") {
        return;
    }
    let save = std::env::var_os("OBS_GPUI_PREVIEWS").is_some();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("previews");
    for (name, _, _) in LOOKS {
        let mut cx = headless_context();
        cx.update(|cx| obs_ui_gpui::theme::set_look(look_named(name, 1.0).unwrap(), cx));
        let (handle, _) = cx
            .update(|cx| {
                gpui_kit::open_window(hidden_window(1280., 1000.), cx, |window, cx| {
                    cx.new(|cx| Catalogue::new(window, cx))
                })
            })
            .expect("open window");
        cx.update_window(handle, |_, window, cx| window.frame(cx))
            .unwrap();
        let image = cx
            .capture_screenshot(handle)
            .unwrap_or_else(|e| panic!("previews: capture: {e}"));
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

/// A control built in a Look, for the focus ring tests.
#[cfg(target_os = "linux")]
type Control = fn(&obs_ui_gpui::theme::Look) -> gpui_kit::AnyElement;

/// One control on the shell's ground, for the focus ring tests.
#[cfg(target_os = "linux")]
struct OneControl(Control);

#[cfg(target_os = "linux")]
impl gpui_kit::Render for OneControl {
    fn render(
        &mut self,
        _: &mut gpui_kit::Window,
        cx: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        use gpui_kit::{ParentElement as _, Styled as _, div, px};
        let look = obs_ui_gpui::theme::look(cx);
        div()
            .size_full()
            .p(px(40.))
            .bg(obs_ui_gpui::tokens::rgba(look.chrome.shell))
            .child((self.0)(&look))
    }
}

/// The control labelled `label` in a window of its own, in the catalogue's Look `look_name`,
/// given the keyboard focus (so its focus-visible style is drawn); the frame, and the
/// control's bounds in the frame's pixels.
#[cfg(target_os = "linux")]
fn focused_frame(look_name: &str, label: &str, control: Control) -> (image::RgbaImage, [f32; 4]) {
    use gpui_kit::AppContext as _;

    use crate::support::Driver as _;

    let mut cx = headless_context();
    cx.update(|cx| {
        let look = obs_ui_gpui::catalogue::look_named(look_name, 1.0).unwrap();
        obs_ui_gpui::theme::set_look(look, cx);
    });
    let (handle, _) = cx
        .update(|cx| {
            gpui_kit::open_window(hidden_window(400., 200.), cx, |_, cx| {
                cx.new(|_| OneControl(control))
            })
        })
        .expect("open window");
    let bounds = cx
        .update_window(handle, |_, window, cx| {
            window.frame(cx);
            window.focus_next(cx);
            // A key press: focus is drawn for the keyboard only. Tab comes back to the one
            // control.
            window.press("tab", cx);
            let found = window.find_by_label(label);
            assert_eq!(
                found.focused(),
                Some(true),
                "{label} has the keyboard focus"
            );
            let b = found.bounds();
            [
                f32::from(b.origin.x),
                f32::from(b.origin.y),
                f32::from(b.size.width),
                f32::from(b.size.height),
            ]
        })
        .unwrap();
    let image = cx
        .capture_screenshot(handle)
        .unwrap_or_else(|e| panic!("capture: {e}"));
    let scale = image.width() as f32 / 400.;
    (image, bounds.map(|v| v * scale))
}

#[cfg(target_os = "linux")]
fn pixel(image: &image::RgbaImage, x: f32, y: f32) -> [u8; 3] {
    let p = image.get_pixel(x.round() as u32, y.round() as u32);
    [p[0], p[1], p[2]]
}

#[cfg(target_os = "linux")]
fn rgb(c: obs_engine::Rgb) -> [u8; 3] {
    c.0
}

/// Whether two colours are within the goldens' channel tolerance.
#[cfg(target_os = "linux")]
fn near(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter()
        .zip(b)
        .all(|(x, y)| x.abs_diff(y) <= support::CHANNEL_TOLERANCE)
}

#[cfg(target_os = "linux")]
#[test]
fn the_focused_primary_button_has_a_ring_unlike_its_fill() {
    use gpui_kit::IntoElement as _;
    use obs_ui_gpui::components as c;

    if !renderer_available("focus rings") {
        return;
    }
    for look_name in ["Light", "Dark", "High contrast"] {
        let look = obs_ui_gpui::catalogue::look_named(look_name, 1.0).unwrap();
        let (image, [x, y, _, h]) = focused_frame(look_name, "Open", |look| {
            c::primary_button("open", "Open", look).into_any_element()
        });
        let scale = image.width() as f32 / 400.;
        let ring = look.focus_width() * scale;
        let mid = y + h / 2.0;
        let border = pixel(&image, x + ring / 2.0, mid);
        let fill = pixel(&image, x + ring + 2.0 * scale, mid);
        let outside = pixel(&image, x - ring / 2.0, mid);
        assert!(
            near(fill, rgb(look.chrome.lapis)),
            "{look_name}: the fill is lapis: {fill:?}"
        );
        assert!(
            !near(border, fill),
            "{look_name}: the focused border {border:?} is the fill's colour"
        );
        assert!(
            near(border, rgb(c::on_lapis(&look))),
            "{look_name}: the inner ring is the label's colour: {border:?}"
        );
        assert!(
            near(outside, rgb(look.chrome.lapis)),
            "{look_name}: the outer ring is lapis: {outside:?}"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn the_chips_close_button_and_a_menu_item_draw_a_focus_ring() {
    use gpui_kit::IntoElement as _;
    use obs_ui_gpui::components as c;

    if !renderer_available("focus rings") {
        return;
    }
    let look = obs_ui_gpui::catalogue::look_named("Light", 1.0).unwrap();
    let controls: [(&str, Control); 2] = [
        ("Remove KJV", |look| {
            c::chip("kjv", "KJV", look, Some(Box::new(|_, _, _| {}))).into_any_element()
        }),
        ("Float", |look| {
            c::menu_item("float", "Float", None, look).into_any_element()
        }),
    ];
    for (label, control) in controls {
        let (image, [x, y, _, h]) = focused_frame("Light", label, control);
        let scale = image.width() as f32 / 400.;
        let ring = look.focus_width() * scale;
        let edge = pixel(&image, x + ring / 2.0, y + h / 2.0);
        assert!(
            near(edge, rgb(look.chrome.lapis)),
            "{label}: the focused edge is the lapis ring: {edge:?}"
        );
    }
}
