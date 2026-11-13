//! Motion (#225): jumps ease into place with a flash on the target verse, keyboard scrolling
//! eases, pop-ups scale in, Panels settle into a new arrangement, loading placeholders shimmer, and with
//! Reduce motion on (in the app or in the operating system) every one of them is instant.
//!
//! The Harness runs with the system's reduce motion on (each frame shows where things land);
//! these tests turn it off to see the motion.

use egui::emath::TSTransform;
use obs_scripture::{Book, Verse};

use super::*;
use crate::tokens::motion;
use crate::view::ReadingView;

fn view(h: &Harness, tab: obs_engine::TabId) -> &ReadingView {
    h.shell.st.tabs[&tab].view.as_ref().unwrap()
}

fn view_mut(h: &mut Harness, tab: obs_engine::TabId) -> &mut ReadingView {
    h.shell
        .st
        .tabs
        .get_mut(&tab)
        .unwrap()
        .view
        .as_mut()
        .unwrap()
}

/// The strongest flash drawn last frame (0: none): the alpha of a filled rect in the flash's
/// colour.
fn flash_alpha(h: &Harness) -> u8 {
    let f = h.shell.st.palette.flash;
    let near = |c: u8, a: u8, want: u8| {
        (f32::from(c) / f32::from(a) - f32::from(want) / f32::from(f.a())).abs() < 0.04
    };
    h.rects
        .iter()
        .map(|(_, c)| *c)
        .filter(|c| {
            c.a() > 8
                && near(c.r(), c.a(), f.r())
                && near(c.g(), c.a(), f.g())
                && near(c.b(), c.a(), f.b())
        })
        .map(|c| c.a())
        .max()
        .unwrap_or(0)
}

/// The visible pop-up layers' transforms (none: drawn at their own size).
fn popup_scales(h: &Harness) -> Vec<f32> {
    let layers: Vec<egui::LayerId> = h.ctx.memory(|m| {
        m.layer_ids()
            .filter(|l| l.order == egui::Order::Foreground && m.areas().is_visible(l))
            .collect()
    });
    layers
        .into_iter()
        .filter_map(|l| h.ctx.layer_transform_to_global(l))
        .map(|t: TSTransform| t.scaling)
        .collect()
}

fn gen30(v: u16) -> Verse {
    Verse::new(Book::Genesis, 30, v)
}

fn wheel_lines(h: &mut Harness, over: Pos2, lines: f32) {
    h.events.push(Event::PointerMoved(over));
    h.events.push(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: vec2(0.0, -lines),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: Modifiers::NONE,
    });
}

fn page_down(h: &mut Harness, over: Pos2) {
    h.events.push(Event::PointerMoved(over));
    h.events.push(Event::Key {
        key: egui::Key::PageDown,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
}

/// The width of the Panel holding `title`.
fn panel_width(h: &Harness, title: &str) -> f32 {
    h.panels()
        .into_iter()
        .find(|(_, titles)| titles.iter().any(|t| t == title))
        .map(|(r, _)| r.width())
        .unwrap_or_else(|| panic!("no Panel with {title}"))
}

#[test]
fn with_motion_a_link_set_jump_eases_into_place_and_flashes_the_target_verse() {
    let (mut h, tb, hn) = linked_harness();
    h.system_motion(false);
    h.frames(30);
    assert_eq!(h.ctx.global_style().animation_time, motion::POPOVER as f32);
    let start = h
        .shell
        .st
        .tabs
        .get_mut(&tb)
        .unwrap()
        .view
        .as_mut()
        .unwrap()
        .shown_position();
    h.shell
        .run("tab.go", &json!({ "tab": tb.0, "text": "Gen 30:12" }));
    let to = view_mut(&mut h, tb).position().0;
    assert!(to > start + 20.0, "a jump down the page: {start} → {to}");
    // Its place is the new one at once; it is drawn easing there.
    assert_eq!(view_mut(&mut h, tb).current_location(), Some(gen30(12)));
    let mut shown = vec![start];
    let mut flashes = Vec::new();
    for _ in 0..40 {
        h.frame();
        shown.push(view_mut(&mut h, tb).shown_position());
        flashes.push(flash_alpha(&h));
        if !view(&h, tb).is_gliding() {
            break;
        }
    }
    // About motion::JUMP of frames (60 a second), every one closer, none past it.
    let frames = shown.len() - 1;
    let expected = (motion::JUMP * 60.0).round() as usize;
    assert!(frames.abs_diff(expected) <= 2, "{frames} frames: {shown:?}");
    assert!(shown.windows(2).all(|w| w[1] >= w[0] - 1e-3), "{shown:?}");
    assert!(shown.iter().all(|y| *y <= to + 1e-3), "{shown:?}");
    assert!(
        (shown[frames] - to).abs() < 0.5,
        "lands where it jumped: {shown:?}"
    );
    // Eased out: past halfway a quarter of the way through.
    let quarter = shown[frames / 4];
    assert!(quarter - start > (to - start) / 2.0, "{shown:?}");
    // The flash on the target verse fades out.
    assert!(flashes[0] > 0, "no flash drawn: {flashes:?}");
    assert!(flashes.last() < flashes.first(), "{flashes:?}");
    // The follower in the Link set eased too, and its flash fades the same way.
    assert!(h.shell.st.tabs[&hn].flash.is_some());
}

#[test]
fn with_motion_keyboard_scrolling_eases_and_the_wheel_is_smoothed() {
    let mut h = Harness::new();
    h.system_motion(false);
    h.frames(3);
    let tb = h.tab_id("TB");
    let over = h.panels()[0].0.center();
    let before = view_mut(&mut h, tb).position().0;
    page_down(&mut h, over);
    h.frame();
    let to = view_mut(&mut h, tb).position().0;
    assert!(to > before + 100.0);
    assert!(view(&h, tb).is_gliding());
    let first = view_mut(&mut h, tb).shown_position();
    assert!(first < to - 1.0, "drawn short of its place at first");
    h.frames((motion::SCROLL * 60.0).ceil() as usize + 2);
    assert!(!view(&h, tb).is_gliding());
    assert_eq!(view_mut(&mut h, tb).shown_position(), to);

    // A mouse-wheel step is spread over a few frames (egui's smoothing).
    let line = h.ctx.options(|o| o.input_options.line_scroll_speed);
    let before = view_mut(&mut h, tb).position().0;
    wheel_lines(&mut h, over, 3.0);
    h.frame();
    let moved = view_mut(&mut h, tb).position().0 - before;
    assert!(
        moved > 0.0 && moved < f64::from(3.0 * line) - 1.0,
        "{moved}"
    );
}

#[test]
fn with_motion_popups_scale_in_and_a_new_panel_settles_in() {
    let mut h = Harness::new();
    h.system_motion(false);
    h.frames(3);
    h.open_menu("View");
    let scales = popup_scales(&h);
    assert!(
        scales
            .iter()
            .any(|s| *s < 1.0 && *s >= motion::POPOVER_SCALE),
        "the menu scales in: {scales:?}"
    );
    h.frames((motion::POPOVER * 60.0).ceil() as usize + 1);
    assert!(popup_scales(&h).is_empty(), "then is drawn at its own size");
    // Its items are still where they are drawn, and work.
    h.click_text("Reduce motion");
    assert!(h.shell.st.session.appearance.reduce_motion);
    h.shell.run("view.reduce_motion", &json!({ "on": false }));
    h.frames(2);

    let tb = h.tab_id("TB");
    let tb_width = panel_width(&h, "TB");
    h.shell.st.session.workspace.user_focused(tb);
    h.shell
        .run("resource.open", &json!({ "id": "heb-num", "split": true }));
    h.frame();
    // Every Panel takes its final size at once (its text is laid out once, at its width) …
    let final_width = panel_width(&h, "HNUM");
    let tb_panel = h.shell.st.session.workspace.panel_of(tb).unwrap();
    let hn = h.tab_id("HNUM");
    let hn_panel = h.shell.st.session.workspace.panel_of(hn).unwrap();
    // … then settles into it: TB's outline eases from its old bounds, and HNUM fades in
    // from the room it took.
    let settling = h.shell.panel_motion.settling.clone();
    assert!(!settling[&tb_panel].new);
    assert!((settling[&tb_panel].from.width() - tb_width).abs() < 0.5);
    assert!(settling[&hn_panel].new);
    let veil = |h: &Harness| -> u8 {
        let shell = crate::theme::c().shell;
        let to = h.shell.panel_motion.settling.get(&hn_panel).map(|s| s.to);
        (h.rects.iter())
            .filter(|(r, c)| {
                let want = shell.gamma_multiply(f32::from(c.a()) / 255.0);
                Some(*r) == to && c.r().abs_diff(want.r()) <= 1 && c.b().abs_diff(want.b()) <= 1
            })
            .map(|(_, c)| c.a())
            .max()
            .unwrap_or(0)
    };
    let mut veils = vec![veil(&h)];
    for _ in 0..(motion::PANEL * 60.0).ceil() as usize + 1 {
        h.frame();
        assert_eq!(panel_width(&h, "HNUM"), final_width);
        veils.push(veil(&h));
    }
    assert!(veils[0] > 200, "{veils:?}");
    assert!(veils.windows(2).all(|w| w[1] <= w[0]), "{veils:?}");
    assert!(h.shell.panel_motion.settling.is_empty(), "settled");
    assert_eq!(*veils.last().unwrap(), 0);
}

#[test]
fn the_shimmer_sweeps_down_loading_placeholders_and_rests_with_reduce_motion() {
    // A band sweeps down once every motion::SHIMMER: the top is brightest early in the
    // sweep, the bottom later, and outside the band a placeholder is at rest.
    let early = motion::SHIMMER * 0.3;
    let late = motion::SHIMMER * 0.7;
    assert!(motion::shimmer_gain(0.1, early) > motion::shimmer_gain(0.9, early));
    assert!(motion::shimmer_gain(0.9, late) > motion::shimmer_gain(0.1, late));
    for t in 0..100 {
        let g = motion::shimmer_gain(0.5, motion::SHIMMER * f64::from(t) / 100.0);
        assert!((1.0..=1.0 + motion::SHIMMER_GAIN).contains(&g), "{g}");
    }
    assert_eq!(motion::shimmer_gain(0.95, 0.0), 1.0);

    let mut h = Harness::new();
    let area = Rect::from_min_size(Pos2::ZERO, vec2(400.0, 400.0));
    let bar = egui::Color32::from_black_alpha(30);
    let at = |h: &mut Harness| {
        h.frame();
        crate::motion::shimmer(&h.ctx, area, 200.0, bar)
    };
    // Reduced (the Harness's default): at rest every frame.
    assert!((0..30).all(|_| at(&mut h) == bar));
    h.system_motion(false);
    let drawn: Vec<egui::Color32> = (0..90).map(|_| at(&mut h)).collect();
    assert!(drawn.iter().any(|c| c.a() > bar.a()), "the band passes");
}

/// With Reduce motion on, from View → Reduce motion or from the operating system, a jump lands
/// at once with its flash at full strength until it goes, keyboard and wheel scrolling move
/// the whole step in the frame they come in, pop-ups appear at their own size, a new Panel
/// shows no settling, and egui's own transitions take no time.
#[test]
fn reduce_motion_makes_every_change_instant() {
    for from_the_app in [true, false] {
        let (mut h, tb, _hn) = linked_harness();
        if from_the_app {
            h.system_motion(false);
            h.shell.run("view.reduce_motion", &json!({ "on": true }));
        }
        h.frames(3);
        let how = if from_the_app {
            "the app"
        } else {
            "the system"
        };
        assert_eq!(h.ctx.global_style().animation_time, 0.0, "{how}");

        // A jump lands at once; its flash holds at full strength, then goes.
        h.shell
            .run("tab.go", &json!({ "tab": tb.0, "text": "Gen 30:12" }));
        h.frame();
        let v = view_mut(&mut h, tb);
        assert!(!v.is_gliding(), "{how}");
        assert_eq!(v.shown_position(), v.position().0, "{how}");
        let full = flash_alpha(&h);
        assert!(full > 0, "{how}: the target verse is flashed");
        for _ in 0..30 {
            h.idle_frame();
        }
        assert_eq!(flash_alpha(&h), full, "{how}: the flash does not fade");
        assert!(
            h.repaint_delay > std::time::Duration::from_millis(100),
            "{how}: nothing animates, so no frame is asked for at once ({:?})",
            h.repaint_delay
        );
        h.frames((motion::FLASH * 60.0) as usize);
        assert_eq!(flash_alpha(&h), 0, "{how}: then it goes at once");

        // Page Down moves a whole page in its frame.
        let over = h.panels()[0].0.center();
        let before = view_mut(&mut h, tb).position().0;
        page_down(&mut h, over);
        h.frame();
        assert!(!view(&h, tb).is_gliding(), "{how}");
        let paged = view_mut(&mut h, tb).position().0;
        assert!(paged > before + 100.0, "{how}");
        assert_eq!(view_mut(&mut h, tb).shown_position(), paged, "{how}");

        // A wheel step scrolls all of its lines in the frame it comes in, and no further.
        let line = h.ctx.options(|o| o.input_options.line_scroll_speed);
        wheel_lines(&mut h, over, 3.0);
        h.frame();
        let wheeled = view_mut(&mut h, tb).position().0;
        assert!(
            (wheeled - paged - f64::from(3.0 * line)).abs() < 0.5,
            "{how}: {paged} → {wheeled}"
        );
        h.frames(10);
        assert_eq!(view_mut(&mut h, tb).position().0, wheeled, "{how}");

        // A menu opens at its own size.
        h.events.push(Event::PointerMoved(Pos2::ZERO));
        h.open_menu("View");
        assert!(h.find("Reduce motion").is_some(), "{how}: the menu is open");
        assert!(popup_scales(&h).is_empty(), "{how}: {:?}", popup_scales(&h));
        h.events.push(Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
        h.frames(2);

        // A new Panel takes its share in its first frame.
        h.shell.st.session.workspace.user_focused(tb);
        h.shell.run(
            "resource.open",
            &json!({ "id": "test-bible", "split": true }),
        );
        h.frame();
        let first: Vec<Rect> = h.panels().into_iter().map(|(r, _)| r).collect();
        h.frames(20);
        let later: Vec<Rect> = h.panels().into_iter().map(|(r, _)| r).collect();
        assert_eq!(first, later, "{how}");
    }
}

/// A view's glide, without egui: a jump is drawn from where the view was, easing out to
/// where it jumped over motion::JUMP, only through text already laid out; scrolling during
/// it starts from where the text is drawn.
#[test]
fn a_glide_eases_through_laid_text_and_scrolling_takes_over_from_where_it_is_drawn() {
    let mut v = ReadingView::new(crate::tests::bible(), obs_layout::TextSettings::default());
    v.set_width(450.0);
    v.set_clock(0.0);
    v.jump_to(Verse::new(Book::Genesis, 30, 1));
    v.frame(600.0, None);
    let from = v.shown_position();
    assert!(v.jump_to(Verse::new(Book::Genesis, 30, 6)));
    let to = v.position().0;
    v.glide_from(from, motion::JUMP, motion::ease_out);
    assert_eq!(v.current_location(), Some(Verse::new(Book::Genesis, 30, 6)));
    assert!((v.shown_position() - from).abs() < 0.5);
    let mut last = from;
    for k in 1..=20 {
        v.set_clock(f64::from(k) / 60.0);
        let f = v.frame(600.0, None);
        assert_eq!(f.stats.placeholders, 0);
        let shown = v.shown_position();
        assert!(
            shown >= last - 1e-3 && shown <= to + 1e-3,
            "{last} → {shown}"
        );
        last = shown;
    }
    assert!(!v.is_gliding());
    assert_eq!(v.shown_position(), to);

    // A long jump glides at most a pane height (motion::JUMP_REACH), through laid text only.
    v.set_clock(1.0);
    v.frame(600.0, None);
    let from = v.shown_position();
    assert!(v.jump_to(Verse::new(Book::Genesis, 33, 20)));
    let to = v.position().0;
    v.glide_from(from, motion::JUMP, motion::ease_out);
    let offset = (v.shown_position() - to).abs();
    assert!(
        offset <= f64::from(600.0 * motion::JUMP_REACH) + 0.5,
        "{offset}"
    );
    assert!(v.frame(600.0, None).stats.placeholders == 0);

    // Scrolling mid-glide carries on from where the text is drawn.
    v.jump_to(Verse::new(Book::Genesis, 30, 1));
    v.frame(600.0, None);
    let from = v.shown_position();
    v.jump_to(Verse::new(Book::Genesis, 30, 8));
    v.glide_from(from, motion::JUMP, motion::ease_out);
    v.set_clock(1.0 + motion::JUMP / 3.0);
    v.frame(600.0, None);
    let mid = v.shown_position();
    v.scroll_by(10.0);
    assert!(!v.is_gliding());
    assert!((v.position().0 - (mid + 10.0)).abs() < 0.5);
}

/// One wheel event over `over`: `dy` mouse-wheel lines or trackpad points down the page.
fn wheel(h: &mut Harness, over: Pos2, unit: egui::MouseWheelUnit, dy: f32, modifiers: Modifiers) {
    h.events.push(Event::PointerMoved(over));
    h.events.push(Event::MouseWheel {
        unit,
        delta: vec2(0.0, -dy),
        phase: egui::TouchPhase::Move,
        source: match unit {
            egui::MouseWheelUnit::Point => egui::MouseWheelSource::Trackpad,
            _ => egui::MouseWheelSource::Wheel,
        },
        modifiers,
    });
}

fn set_scroll_speed(h: &mut Harness, speed: f64) {
    h.shell.run("view.scroll_speed", &json!({ "value": speed }));
    assert_eq!(f64::from(h.shell.st.session.appearance.scroll_speed), speed);
    h.frames(2);
}

/// How far TB scrolls for one wheel event, once it has settled (egui spreads a big step over
/// several frames when motion is not reduced).
fn wheeled(h: &mut Harness, tb: obs_engine::TabId, unit: egui::MouseWheelUnit, dy: f32) -> f64 {
    let over = h.panels()[0].0.center();
    let before = view_mut(h, tb).position().0;
    wheel(h, over, unit, dy, Modifiers::NONE);
    h.frames(60);
    view_mut(h, tb).position().0 - before
}

/// #293 acceptance: at a Scroll speed of 0.5× and 2× a mouse-wheel step (lines) and a
/// trackpad swipe (points) scroll a reading pane half and twice as far as at 1×, with Reduce
/// motion on (each step lands in its frame) and off (egui's smoothing).
#[test]
fn scroll_speed_scales_wheel_lines_and_trackpad_points_by_its_factor() {
    for reduce in [true, false] {
        let mut h = Harness::new();
        if !reduce {
            h.system_motion(false);
        }
        h.frames(3);
        assert_eq!(crate::motion::reduced(&h.ctx), reduce);
        let tb = h.tab_id("TB");
        for (unit, dy) in [
            (egui::MouseWheelUnit::Line, 3.0),
            (egui::MouseWheelUnit::Point, 90.0),
        ] {
            set_scroll_speed(&mut h, 1.0);
            let one = wheeled(&mut h, tb, unit, dy);
            assert!(one > 50.0, "reduce {reduce}, {unit:?}: {one}");
            for speed in [0.5, 2.0] {
                set_scroll_speed(&mut h, speed);
                let moved = wheeled(&mut h, tb, unit, dy);
                assert!(
                    (moved - one * speed).abs() < 0.5,
                    "reduce {reduce}, {unit:?} at {speed}×: {moved}, at 1×: {one}"
                );
                // Scrolling back up goes as far.
                let back = wheeled(&mut h, tb, unit, -dy);
                assert!((back + moved).abs() < 0.5, "{back} after {moved}");
            }
        }
        // With Reduce motion, the whole scaled step in the frame it comes in.
        if reduce {
            set_scroll_speed(&mut h, 2.0);
            let over = h.panels()[0].0.center();
            let before = view_mut(&mut h, tb).position().0;
            let line = h.ctx.options(|o| o.input_options.line_scroll_speed);
            wheel(
                &mut h,
                over,
                egui::MouseWheelUnit::Line,
                1.0,
                Modifiers::NONE,
            );
            h.frame();
            let moved = view_mut(&mut h, tb).position().0 - before;
            assert!((moved - f64::from(2.0 * line)).abs() < 0.5, "{moved}");
        }
    }
}

/// #293: Ctrl+wheel (Cmd+wheel on macOS) is a zoom, never a scroll: its zoom is the same at
/// every Scroll speed, and the text does not move.
#[test]
fn ctrl_wheel_zoom_is_the_same_at_every_scroll_speed() {
    for reduce in [true, false] {
        let mut h = Harness::new();
        if !reduce {
            h.system_motion(false);
        }
        h.frames(3);
        let tb = h.tab_id("TB");
        let over = h.panels()[0].0.center();
        let zoom_at = |h: &mut Harness, speed: f64, unit: egui::MouseWheelUnit, dy: f32| {
            set_scroll_speed(h, speed);
            let before = view_mut(h, tb).position().0;
            wheel(h, over, unit, dy, Modifiers::COMMAND);
            let mut zoom = 1.0;
            for _ in 0..60 {
                h.frame();
                zoom *= h.ctx.input(egui::InputState::zoom_delta);
            }
            assert_eq!(view_mut(h, tb).position().0, before, "no scroll");
            zoom
        };
        for (unit, dy) in [
            (egui::MouseWheelUnit::Line, 2.0),
            (egui::MouseWheelUnit::Point, 60.0),
        ] {
            let one = zoom_at(&mut h, 1.0, unit, dy);
            assert!(
                (one - 1.0).abs() > 0.05,
                "reduce {reduce}, {unit:?}: the wheel zooms ({one})"
            );
            for speed in [0.5, 2.0] {
                let zoom = zoom_at(&mut h, speed, unit, dy);
                assert!(
                    (zoom - one).abs() < 1e-3,
                    "reduce {reduce}, {unit:?} at {speed}×: {zoom}, at 1×: {one}"
                );
            }
        }
    }
}

/// #293: an arrow key's step follows the Scroll speed; Page Down still moves one page.
#[test]
fn arrow_keys_follow_the_scroll_speed_and_page_down_moves_one_page() {
    let mut h = Harness::new();
    h.frames(3);
    let tb = h.tab_id("TB");
    let over = h.panels()[0].0.center();
    let key = |h: &mut Harness, key: egui::Key| {
        let before = view_mut(h, tb).position().0;
        h.events.push(Event::PointerMoved(over));
        h.events.push(Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
        h.frames(2);
        view_mut(h, tb).position().0 - before
    };
    let arrow = f64::from(crate::pane::ARROW_STEP);
    let mut pages = Vec::new();
    for speed in [1.0, 0.5, 2.0] {
        set_scroll_speed(&mut h, speed);
        let down = key(&mut h, egui::Key::ArrowDown);
        assert!((down - arrow * speed).abs() < 0.5, "{speed}×: {down}");
        let up = key(&mut h, egui::Key::ArrowUp);
        assert!((up + arrow * speed).abs() < 0.5, "{speed}×: {up}");
        pages.push(key(&mut h, egui::Key::PageDown));
    }
    assert!(pages[0] > 100.0, "{pages:?}");
    assert!(
        pages.iter().all(|p| (p - pages[0]).abs() < 0.5),
        "a page at every Scroll speed: {pages:?}"
    );
}

/// The Atlas and the Timeline zoom by the wheel's own amount, whatever the Scroll speed.
#[test]
fn the_unscaled_scroll_delta_is_the_wheels_own_at_every_scroll_speed() {
    let mut h = Harness::new();
    h.frames(3);
    let over = h.panels()[0].0.center();
    let line = h.ctx.options(|o| o.input_options.line_scroll_speed);
    for speed in [0.5, 1.0, 2.0] {
        set_scroll_speed(&mut h, speed);
        wheel(
            &mut h,
            over,
            egui::MouseWheelUnit::Line,
            1.0,
            Modifiers::NONE,
        );
        h.frame();
        let unscaled = crate::motion::unscaled_scroll_delta(&h.ctx).y;
        assert!((unscaled + line).abs() < 1e-3, "{speed}×: {unscaled}");
    }
}
