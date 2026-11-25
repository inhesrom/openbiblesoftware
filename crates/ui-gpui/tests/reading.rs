//! The reading pane on GPUI (#380), on GPUI's TestPlatform: what the egui app's reading-pane
//! tests check about scrolling, motion, jumps, Text sizes and the Reading layout, on the same
//! `ReadingView`, driven through the GPUI pane's own input, plus its paragraph nodes. Each test
//! names the egui test it ports.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, Entity, Modifiers, Role, ScrollDelta, ScrollEvent,
    TestAppContext, TouchPhase, point, px, size,
};
use obs_engine::Session;
use obs_scripture::{Book, Verse};
use obs_ui_gpui::Shell;
use obs_ui_gpui::reading::{self, ARROW_STEP, LINE_STEP, PAGE_SHARE, ReadingPane};
use serde_json::json;
use support::Driver;

struct Open {
    handle: AnyWindowHandle,
    shell: Entity<Shell>,
    pane: Entity<ReadingPane>,
    lab: support::Lab,
}

/// The Shell over the fixtures' Session (plus the reading fixtures), showing `resource` at
/// `reference` in its reading pane, with motion as asked.
fn open(
    cx: &mut TestAppContext,
    name: &str,
    resource: &str,
    reference: &str,
    reduce: bool,
) -> Open {
    let (mut session, lab) = support::session(name);
    for r in [
        support::hebrew_genesis_1(),
        support::greek_john_1(),
        support::long_psalm(),
    ] {
        support::add_resource(&mut session, &lab, &r);
    }
    session.appearance.reduce_motion = reduce;
    open_session(cx, session, lab, resource, reference)
}

fn open_session(
    cx: &mut TestAppContext,
    session: Session,
    lab: support::Lab,
    resource: &str,
    reference: &str,
) -> Open {
    let (handle, shell) = cx.update(|cx| {
        obs_ui_gpui::install(cx);
        let bounds = Bounds {
            origin: point(px(0.), px(0.)),
            size: size(
                px(obs_ui_gpui::tokens::WINDOW_WIDTH),
                px(obs_ui_gpui::tokens::WINDOW_HEIGHT),
            ),
        };
        let options = obs_ui_gpui::main_window_options(Some(bounds), cx);
        obs_ui_gpui::open_main_window(session, options, cx).unwrap()
    });
    let pane = cx
        .update_window(handle, |_, window, cx| {
            window.frame(cx);
            shell.update(cx, |s, cx| {
                s.open_reading(resource, Some(reference), window, cx)
                    .unwrap_or_else(|e| panic!("{resource} at {reference}: {e}"))
            })
        })
        .unwrap();
    frames(cx, handle, 2);
    Open {
        handle,
        shell,
        pane,
        lab,
    }
}

fn close(cx: &mut TestAppContext, o: Open) {
    let Open {
        handle,
        shell,
        pane,
        lab,
    } = o;
    drop(pane);
    drop(shell);
    cx.update_window(handle, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    drop(lab);
}

fn frames(cx: &mut TestAppContext, handle: AnyWindowHandle, n: usize) {
    for _ in 0..n {
        cx.update_window(handle, |_, window, cx| window.frame(cx))
            .unwrap();
    }
}

/// Frames until the pane's layouts and glides have settled (workers deliver between frames).
fn settle(cx: &mut TestAppContext, o: &Open) {
    for _ in 0..200 {
        cx.run_until_parked();
        frames(cx, o.handle, 1);
        let busy = o
            .pane
            .read_with(cx, |p, _| p.view().is_gliding() || p.view().has_pending());
        if !busy {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn position(cx: &mut TestAppContext, o: &Open) -> f64 {
    o.pane.update(cx, |p, _| p.view_mut().position().0)
}

fn shown(cx: &mut TestAppContext, o: &Open) -> f64 {
    o.pane.update(cx, |p, _| p.view_mut().shown_position())
}

fn location(cx: &mut TestAppContext, o: &Open) -> Option<Verse> {
    o.pane.update(cx, |p, _| p.view_mut().current_location())
}

fn set_scroll_speed(cx: &mut TestAppContext, o: &Open, speed: f32) {
    o.shell.update(cx, |s, cx| {
        assert!(
            s.run("view.scroll_speed", &json!({ "value": speed }))
                .is_some(),
            "view.scroll_speed {speed}"
        );
        cx.notify();
    });
    frames(cx, o.handle, 1);
}

fn press(cx: &mut TestAppContext, o: &Open, key: &str) {
    cx.update_window(o.handle, |_, window, cx| window.press(key, cx))
        .unwrap();
}

/// A wheel or trackpad event over the pane's middle.
fn wheel(cx: &mut TestAppContext, o: &Open, delta: ScrollDelta, modifiers: Modifiers) {
    let at = o.pane.read_with(cx, |p, _| p.bounds().center());
    cx.update_window(o.handle, |_, window, cx| {
        window.pointer_move(at, cx);
        window.dispatch_event(
            gpui_kit::InputEvent::to_platform_input(ScrollEvent {
                position: at,
                delta,
                modifiers,
                touch_phase: TouchPhase::Moved,
                source: Default::default(),
            }),
            cx,
        );
        window.frame(cx);
    })
    .unwrap();
}

/// How far a wheel event moves the text, in points.
fn wheeled(cx: &mut TestAppContext, o: &Open, delta: ScrollDelta) -> f64 {
    let before = position(cx, o);
    wheel(cx, o, delta, Modifiers::default());
    position(cx, o) - before
}

fn lines(n: f32) -> ScrollDelta {
    // A wheel turned towards the user scrolls down the text: a negative delta.
    ScrollDelta::Lines(point(0.0, -n))
}

fn points(n: f32) -> ScrollDelta {
    ScrollDelta::Pixels(point(px(0.0), px(-n)))
}

/// egui `motion_tests::scroll_speed_scales_wheel_lines_and_trackpad_points_by_its_factor`:
/// wheel lines and trackpad points scroll by the Scroll speed's factor (#293), and back up as
/// far; with Reduce motion the whole scaled step comes in its frame.
#[gpui_kit::test]
fn scroll_speed_scales_wheel_lines_and_trackpad_points_by_its_factor(cx: &mut TestAppContext) {
    for reduce in [true, false] {
        let o = open(cx, "wheel", "long-psalm", "Ps 119:1", reduce);
        for delta in [lines(3.0), points(90.0)] {
            set_scroll_speed(cx, &o, 1.0);
            let one = wheeled(cx, &o, delta);
            assert!(one > 50.0, "reduce {reduce}, {delta:?}: {one}");
            for speed in [0.5f32, 2.0] {
                set_scroll_speed(cx, &o, speed);
                let moved = wheeled(cx, &o, delta);
                assert!(
                    (moved - one * f64::from(speed)).abs() < 0.5,
                    "reduce {reduce}, {delta:?} at {speed}×: {moved}, at 1×: {one}"
                );
                let back = wheeled(
                    cx,
                    &o,
                    match delta {
                        ScrollDelta::Lines(d) => ScrollDelta::Lines(point(0.0, -d.y)),
                        ScrollDelta::Pixels(d) => ScrollDelta::Pixels(point(px(0.0), -d.y)),
                    },
                );
                assert!((back + moved).abs() < 0.5, "{back} after {moved}");
            }
        }
        // A wheel line at 1× is the egui app's line step.
        set_scroll_speed(cx, &o, 1.0);
        let line = wheeled(cx, &o, lines(1.0));
        assert!((line - f64::from(LINE_STEP)).abs() < 0.5, "{line}");
        close(cx, o);
    }
}

/// egui `motion_tests::ctrl_wheel_zoom_is_the_same_at_every_scroll_speed` (its scrolling
/// half): Ctrl+wheel (Cmd+wheel on macOS) never scrolls the text.
#[gpui_kit::test]
fn ctrl_or_cmd_with_the_wheel_never_scrolls(cx: &mut TestAppContext) {
    let o = open(cx, "ctrl-wheel", "long-psalm", "Ps 119:1", true);
    for modifiers in [Modifiers::control(), Modifiers::command()] {
        let before = position(cx, &o);
        wheel(cx, &o, lines(2.0), modifiers);
        assert_eq!(position(cx, &o), before, "{modifiers:?}");
    }
    close(cx, o);
}

/// egui `motion_tests::arrow_keys_follow_the_scroll_speed_and_page_down_moves_one_page`
/// (#293): an arrow key's step follows the Scroll speed; Page Down still moves one page.
#[gpui_kit::test]
fn arrow_keys_follow_the_scroll_speed_and_page_down_moves_one_page(cx: &mut TestAppContext) {
    let o = open(cx, "keys", "long-psalm", "Ps 119:1", true);
    let moved = |cx: &mut TestAppContext, key: &str| {
        let before = position(cx, &o);
        press(cx, &o, key);
        position(cx, &o) - before
    };
    let arrow = f64::from(ARROW_STEP);
    let mut pages = Vec::new();
    for speed in [1.0f32, 0.5, 2.0] {
        set_scroll_speed(cx, &o, speed);
        let down = moved(cx, "down");
        assert!(
            (down - arrow * f64::from(speed)).abs() < 0.5,
            "{speed}×: {down}"
        );
        let up = moved(cx, "up");
        assert!(
            (up + arrow * f64::from(speed)).abs() < 0.5,
            "{speed}×: {up}"
        );
        pages.push(moved(cx, "pagedown"));
    }
    let height = o
        .pane
        .read_with(cx, |p, _| f32::from(p.bounds().size.height));
    assert!(pages[0] > 100.0, "{pages:?}");
    assert!(
        (pages[0] - f64::from(height * PAGE_SHARE)).abs() < 0.5,
        "a page is {PAGE_SHARE} of the pane: {pages:?}, pane {height}"
    );
    assert!(
        pages.iter().all(|p| (p - pages[0]).abs() < 0.5),
        "a page at every Scroll speed: {pages:?}"
    );
    // Space pages too, and Page Up comes back.
    let space = moved(cx, "space");
    assert!((space - pages[0]).abs() < 0.5, "{space}");
    let up = moved(cx, "pageup");
    assert!((up + pages[0]).abs() < 0.5, "{up}");
    close(cx, o);
}

/// egui `motion_tests::with_motion_keyboard_scrolling_eases_and_the_wheel_is_smoothed` (its
/// keyboard half) and `motion_tests::reduce_motion_makes_every_change_instant` (its jump and
/// Page Down parts): with motion, Page Down and a jump ease into place; with Reduce motion they
/// land in their frame.
#[gpui_kit::test]
fn keyboard_scrolling_and_jumps_ease_unless_motion_is_reduced(cx: &mut TestAppContext) {
    for reduce in [false, true] {
        let o = open(cx, "motion", "long-psalm", "Ps 119:1", reduce);
        settle(cx, &o);
        let before = position(cx, &o);
        press(cx, &o, "pagedown");
        let to = position(cx, &o);
        assert!(to > before + 100.0, "reduce {reduce}");
        let gliding = o.pane.read_with(cx, |p, _| p.view().is_gliding());
        assert_eq!(
            gliding, !reduce,
            "reduce {reduce}: Page Down glides only with motion"
        );
        if !reduce {
            assert!(
                shown(cx, &o) < to - 1.0,
                "drawn short of its place at first"
            );
            std::thread::sleep(std::time::Duration::from_secs_f64(
                obs_reading::reading_tokens::motion::SCROLL + 0.05,
            ));
            settle(cx, &o);
        }
        assert_eq!(shown(cx, &o), position(cx, &o), "reduce {reduce}: it lands");

        // A jump.
        let jumped = o.pane.update(cx, |p, _| p.go("Ps 119:100")).unwrap();
        assert_eq!(jumped, Verse::new(Book::Psalms, 119, 100));
        let gliding = o.pane.read_with(cx, |p, _| p.view().is_gliding());
        assert_eq!(
            gliding, !reduce,
            "reduce {reduce}: a jump glides only with motion"
        );
        if !reduce {
            std::thread::sleep(std::time::Duration::from_secs_f64(
                obs_reading::reading_tokens::motion::JUMP + 0.05,
            ));
        }
        settle(cx, &o);
        assert_eq!(location(cx, &o), Some(jumped), "reduce {reduce}");
        close(cx, o);
    }
}

/// A jump into a long paragraph lays that block out at once (#19, ADR 0004's #372 amendment):
/// the first frame after it draws the target's text, never a placeholder that a worker fills
/// in on a later frame.
#[gpui_kit::test]
fn a_jump_into_a_long_paragraph_is_drawn_in_its_first_frame(cx: &mut TestAppContext) {
    let o = open(cx, "long-jump", "long-psalm", "Ps 119:1", true);
    settle(cx, &o);
    // The jump: the block is far longer than one frame's budget would lay out.
    let bytes = o.pane.read_with(cx, |p, _| {
        p.view()
            .doc
            .blocks
            .iter()
            .map(|b| b.source.range.len())
            .max()
            .unwrap()
    });
    assert!(bytes > 20_000, "one long paragraph ({bytes} bytes)");
    // Start from a pane whose layouts were dropped (as a new width or Text size does).
    o.pane.update(cx, |p, _| p.view_mut().set_width(321.0));
    let target = Verse::new(Book::Psalms, 119, 150);
    assert!(o.pane.update(cx, |p, _| p.jump_to(target)));
    frames(cx, o.handle, 1);
    let (placeholders, laid) = o.pane.read_with(cx, |p, _| {
        (
            p.last_frame().map(|f| f.placeholders),
            p.view().top_block_laid(),
        )
    });
    assert!(laid, "the jump's block is laid out in its first frame");
    assert_eq!(placeholders, Some(0), "no placeholder in the first frame");
    assert_eq!(location(cx, &o), Some(target));
    let drawn = reading::observed::paragraphs();
    assert!(
        drawn.iter().any(|p| p.glyphs > 0),
        "the first frame painted the target's glyphs"
    );
    close(cx, o);
}

/// egui `text_size_tests::holding_ctrl_plus_keeps_every_tabs_current_location` and
/// `resource_text_size_tests::every_tab_of_the_resource_keeps_its_current_location`: a new
/// Text size, or the Resource's own text size, lays the text out again at the new size and
/// keeps the Current location.
#[gpui_kit::test]
fn a_new_text_size_or_resource_text_size_keeps_the_current_location(cx: &mut TestAppContext) {
    let o = open(cx, "text-size", "long-psalm", "Ps 119:40", true);
    settle(cx, &o);
    // Scrolled by hand a little: no longer on the jump's target line.
    wheel(cx, &o, points(45.0), Modifiers::default());
    settle(cx, &o);
    let before = location(cx, &o);
    assert!(before.is_some());
    let default = obs_layout::TextSettings::default().size;
    // Ten steps up, as holding Ctrl+Plus does, then back below the default.
    for (command, steps) in [("view.text_larger", 10), ("view.text_smaller", 12)] {
        o.shell.update(cx, |s, cx| {
            for _ in 0..steps {
                assert!(s.run(command, &json!({})).is_some(), "{command}");
            }
            cx.notify();
        });
        settle(cx, &o);
        let size = o.shell.read_with(cx, |s, _| s.session().text.size);
        assert_ne!(size, default, "{command}");
        let (now, drew) = o.pane.read_with(cx, |p, _| {
            (p.view().settings().size, p.view().drew_all_text())
        });
        assert_eq!(now, size, "the pane draws at the Text size");
        assert!(drew, "all its text drawn at {size}");
        assert_eq!(location(cx, &o), before, "at {size}");
    }
    // The Resource's own size wins over the Text size, and keeps the location too.
    o.shell.update(cx, |s, cx| {
        let out = s.run(
            "view.resource_text_size",
            &json!({ "resource": "long-psalm", "value": 30 }),
        );
        assert!(out.is_some(), "view.resource_text_size");
        cx.notify();
    });
    settle(cx, &o);
    let now = o.pane.read_with(cx, |p, _| p.view().settings().size);
    assert_eq!(now, 30.0, "the Resource's own text size");
    assert_eq!(location(cx, &o), before);
    close(cx, o);
}

/// egui `reading_layout_tests::hebrew_stays_right_to_left_and_is_never_hyphenated`, through the
/// pane: with every Reading layout option on (#209), Hebrew lines run right to left and no
/// Hebrew word is hyphenated, while narrow justified English is.
#[gpui_kit::test]
fn hebrew_stays_right_to_left_and_is_never_hyphenated(cx: &mut TestAppContext) {
    let everything = obs_layout::ReadingLayout {
        flow: obs_layout::TextFlow::VersePerLine,
        large_chapter_numbers: true,
        section_headings: false,
        column: obs_layout::ColumnWidth::Narrow,
        justified: true,
    };
    let hyphens = |cx: &mut TestAppContext, resource: &str, at: &str| {
        let o = open(cx, "layout", resource, at, true);
        o.shell.update(cx, |s, cx| {
            s.session_mut().text.layout = everything;
            cx.notify();
        });
        settle(cx, &o);
        let blocks = o.pane.update(cx, |p, _| {
            assert_eq!(
                p.view().settings().layout,
                everything,
                "the pane draws the Reading layout"
            );
            p.view_mut().visible(2000.0)
        });
        let mut n = 0;
        for b in &blocks {
            for line in &b.laid.lines {
                n += line.items.iter().filter(|i| i.bytes.is_empty()).count();
                if b.laid.rtl {
                    let first = line.items.iter().min_by_key(|i| i.bytes.start).unwrap();
                    assert!(line.items.iter().all(|i| i.x <= first.x));
                }
            }
        }
        let rtl = blocks.iter().any(|b| b.laid.rtl);
        close(cx, o);
        (n, rtl)
    };
    let (n, rtl) = hyphens(cx, "tahot-test", "Gen 1:1");
    assert!(rtl, "the Hebrew Bible reads right to left");
    assert_eq!(n, 0, "a Hebrew word was hyphenated");
    let (n, rtl) = hyphens(cx, "long-psalm", "Ps 119:1");
    assert!(!rtl);
    assert!(n > 0, "narrow justified English is hyphenated");
}

/// Greek breaks between words, never inside one (GPUI's own text wrapper breaks Greek
/// mid-word; the pane draws our layout's lines).
#[gpui_kit::test]
fn greek_lines_break_between_words(cx: &mut TestAppContext) {
    let o = open(cx, "greek", "tagnt-test", "John 1:1", true);
    settle(cx, &o);
    let blocks = o.pane.update(cx, |p, _| p.view_mut().visible(2000.0));
    let mut lines = 0;
    for b in &blocks {
        let text = &b.laid.text;
        for line in &b.laid.lines {
            lines += 1;
            let end = line.bytes.end;
            if end >= text.len() {
                continue;
            }
            let before = text[..end].chars().next_back();
            let after = text[end..].chars().next();
            assert!(
                before.is_some_and(char::is_whitespace) || after.is_some_and(char::is_whitespace),
                "a line breaks inside a word at {end}: {:?}|{:?}",
                &text[end.saturating_sub(12)..end],
                &text[end..(end + 12).min(text.len())]
            );
        }
    }
    assert!(lines > 1, "the paragraph wraps ({lines} lines)");
    close(cx, o);
}

/// The paragraph nodes (#19, ADR 0004's #372 amendment): each laid block in view is one
/// paragraph with its text, and its Tagged words are link children.
#[gpui_kit::test]
fn every_laid_block_in_view_is_a_paragraph_node_with_its_links(cx: &mut TestAppContext) {
    let o = open(cx, "nodes", "kjv-strongs", "Gen 1:1", true);
    settle(cx, &o);
    let paragraphs = reading::observed::paragraphs();
    assert!(!paragraphs.is_empty(), "the pane exposes its blocks");
    assert!(paragraphs.iter().all(|p| p.role == Role::Paragraph));
    let first = paragraphs
        .iter()
        .find(|p| {
            p.value
                .contains("In the beginning God created the heaven and the earth.")
        })
        .unwrap_or_else(|| panic!("Gen 1:1's paragraph: {paragraphs:?}"));
    assert!(first.glyphs > 0, "its glyphs are painted");
    assert!(!first.rtl);
    // Hebrew paragraphs say they read right to left.
    close(cx, o);
    let o = open(cx, "nodes-hebrew", "tahot-test", "Gen 1:1", true);
    settle(cx, &o);
    let paragraphs = reading::observed::paragraphs();
    assert!(
        paragraphs
            .iter()
            .any(|p| p.rtl && p.value.contains("בְּרֵאשִׁ֖ית"))
    );
    close(cx, o);
    // A tagged Bible's words are links.
    let mut session_tagged = support::session("nodes-tagged");
    support::add_resource(
        &mut session_tagged.0,
        &session_tagged.1,
        &obs_engine::testing::tagnt_grace(),
    );
    let o = open_session(
        cx,
        session_tagged.0,
        session_tagged.1,
        "tagnt-grace",
        "John 1:14",
    );
    settle(cx, &o);
    let paragraphs = reading::observed::paragraphs();
    let links: Vec<&str> = paragraphs
        .iter()
        .flat_map(|p| p.links.iter().map(|l| l.label.as_str()))
        .collect();
    assert!(
        links.contains(&"χάριτος"),
        "Tagged words are link children: {links:?}"
    );
    for p in &paragraphs {
        for l in &p.links {
            let (x0, y0, x1, y1) = l.rect;
            assert!(x1 > x0 && y1 > y0, "{l:?} has a box");
        }
    }
    close(cx, o);
}
