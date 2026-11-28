//! Driving one reading pane (#381): open a Resource in the Shell, find where a word is drawn,
//! and click, drag, right-click and type there. Shared by `tests/selection.rs`,
//! `tests/find.rs` and `tests/context_menu.rs`.

#![allow(dead_code)] // Each test binary uses part of it.

use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, Entity, InputEvent as _, Modifiers, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, TestAppContext, point, px, size,
};
use obs_engine::{Session, TabId};
use obs_model::SpanKind;
use obs_ui_gpui::Shell;
use obs_ui_gpui::reading::ReadingPane;

use super::Driver;

pub struct Open {
    pub handle: AnyWindowHandle,
    pub shell: Entity<Shell>,
    pub pane: Entity<ReadingPane>,
    pub tab: TabId,
    pub lab: super::Lab,
}

/// The Shell over the fixtures' Session plus `extra`, showing `resource` at `reference` in its
/// reading pane, with motion reduced.
pub fn open(
    cx: &mut TestAppContext,
    name: &str,
    extra: &[obs_model::Resource],
    resource: &str,
    reference: &str,
) -> Open {
    let (mut session, lab) = super::session(name);
    for r in extra {
        super::add_resource(&mut session, &lab, r);
    }
    session.appearance.reduce_motion = true;
    open_session(cx, session, lab, resource, reference)
}

fn open_session(
    cx: &mut TestAppContext,
    session: Session,
    lab: super::Lab,
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
    let tab = shell.read_with(cx, |s, _| {
        s.session()
            .workspace
            .tabs()
            .find(|t| s.pane(t.id).is_some_and(|p| p == &pane))
            .map(|t| t.id)
            .unwrap()
    });
    let o = Open {
        handle,
        shell,
        pane,
        tab,
        lab,
    };
    settle(cx, &o);
    o
}

pub fn close(cx: &mut TestAppContext, o: Open) {
    let Open {
        handle,
        shell,
        pane,
        lab,
        ..
    } = o;
    drop(pane);
    drop(shell);
    cx.update_window(handle, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    drop(lab);
}

pub fn frames(cx: &mut TestAppContext, handle: AnyWindowHandle, n: usize) {
    for _ in 0..n {
        cx.update_window(handle, |_, window, cx| window.frame(cx))
            .unwrap();
    }
}

/// Frames until the pane's layouts, glides and Find worker have settled.
pub fn settle(cx: &mut TestAppContext, o: &Open) {
    for _ in 0..400 {
        cx.run_until_parked();
        frames(cx, o.handle, 1);
        let busy = o.pane.read_with(cx, |p, _| {
            p.view().is_gliding()
                || p.view().has_pending()
                || p.find().is_some_and(|f| f.is_finding())
        });
        if !busy {
            frames(cx, o.handle, 1);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("the pane never settled");
}

/// Where the glyphs of bytes `range` of unit `unit` are drawn: the middle of its first cluster,
/// in window points.
pub fn spot_of(
    cx: &mut TestAppContext,
    o: &Open,
    unit: usize,
    range: std::ops::Range<usize>,
) -> Point<Pixels> {
    o.pane.read_with(cx, |p, _| {
        let view = p.view();
        let [ox, oy] = view.text_origin;
        let height = f32::from(p.bounds().size.height);
        for (index, top, laid) in view.laid_in_view(height) {
            if view.doc.blocks[index].unit != unit {
                continue;
            }
            for line in &laid.lines {
                for item in &line.items {
                    for (bytes, x0, x1) in item.clusters() {
                        if let Some(obs_layout::UnitPos::Text(b)) = laid.to_unit(bytes.start)
                            && range.contains(&b)
                        {
                            return point(
                                px(ox + (x0 + x1) / 2.0),
                                px(oy + top + (line.top + line.bottom) / 2.0),
                            );
                        }
                    }
                }
            }
        }
        panic!("{range:?} of unit {unit} is not laid out in view");
    })
}

/// The unit and bytes of the `nth` (from 0) `word` in the pane's text.
pub fn find_word(
    cx: &mut TestAppContext,
    o: &Open,
    word: &str,
    nth: usize,
) -> (usize, std::ops::Range<usize>) {
    o.pane.read_with(cx, |p, _| {
        let doc = &p.view().doc;
        doc.units
            .iter()
            .enumerate()
            .flat_map(|(u, unit)| {
                unit.text
                    .match_indices(word)
                    .map(move |(b, w)| (u, b..b + w.len()))
                    .collect::<Vec<_>>()
            })
            .nth(nth)
            .unwrap_or_else(|| panic!("no {word:?} #{nth}"))
    })
}

/// Where the `nth` `word` is drawn.
pub fn word_spot(cx: &mut TestAppContext, o: &Open, word: &str, nth: usize) -> Point<Pixels> {
    let (unit, range) = find_word(cx, o, word, nth);
    spot_of(cx, o, unit, range)
}

/// The first Tagged word tagged with Strong's number `strongs`: its unit and bytes.
pub fn tagged(cx: &mut TestAppContext, o: &Open, strongs: &str) -> (usize, std::ops::Range<usize>) {
    let wanted: obs_scripture::StrongsNumber = strongs.parse().unwrap();
    o.pane.read_with(cx, |p, _| {
        let doc = &p.view().doc;
        for (u, unit) in doc.units.iter().enumerate() {
            for span in &unit.spans {
                if let SpanKind::TaggedWord { parts } = &span.kind
                    && parts.iter().any(|m| m.strongs == wanted)
                {
                    return (u, span.start as usize..span.end as usize);
                }
            }
        }
        panic!("no word tagged {strongs}");
    })
}

/// Where the first word tagged `strongs` is drawn.
pub fn tagged_spot(cx: &mut TestAppContext, o: &Open, strongs: &str) -> Point<Pixels> {
    let (unit, range) = tagged(cx, o, strongs);
    spot_of(cx, o, unit, range)
}

pub fn pointer_move(cx: &mut TestAppContext, o: &Open, at: Point<Pixels>) {
    cx.update_window(o.handle, |_, window, cx| window.pointer_move(at, cx))
        .unwrap();
}

/// The `count`th click in a row of `button` at `at` (down and up), the pointer moved there
/// first, with `modifiers` held.
pub fn click_with(
    cx: &mut TestAppContext,
    o: &Open,
    at: Point<Pixels>,
    button: MouseButton,
    count: usize,
    modifiers: Modifiers,
) {
    cx.update_window(o.handle, |_, window, cx| {
        window.dispatch_event(
            MouseMoveEvent {
                position: at,
                pressed_button: None,
                modifiers,
            }
            .to_platform_input(),
            cx,
        );
        window.frame(cx);
        window.dispatch_event(
            MouseDownEvent {
                button,
                position: at,
                modifiers,
                click_count: count,
                first_mouse: false,
            }
            .to_platform_input(),
            cx,
        );
        window.frame(cx);
        window.dispatch_event(
            MouseUpEvent {
                button,
                position: at,
                modifiers,
                click_count: count,
            }
            .to_platform_input(),
            cx,
        );
        window.frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    frames(cx, o.handle, 1);
}

/// A left click at `at`.
pub fn click(cx: &mut TestAppContext, o: &Open, at: Point<Pixels>) {
    click_with(cx, o, at, MouseButton::Left, 1, Modifiers::default());
}

/// A double-click (`count` 2) or triple-click (3) at `at`, each click counted as a platform
/// does.
pub fn multi_click(cx: &mut TestAppContext, o: &Open, at: Point<Pixels>, count: usize) {
    for n in 1..=count {
        click_with(cx, o, at, MouseButton::Left, n, Modifiers::default());
    }
}

pub fn right_click(cx: &mut TestAppContext, o: &Open, at: Point<Pixels>) {
    click_with(cx, o, at, MouseButton::Right, 1, Modifiers::default());
}

/// A drag with the left button from `from` to `to`, in `steps` moves.
pub fn drag(cx: &mut TestAppContext, o: &Open, from: Point<Pixels>, to: Point<Pixels>) {
    cx.update_window(o.handle, |_, window, cx| {
        window.pointer_move(from, cx);
        window.dispatch_event(
            MouseDownEvent {
                button: MouseButton::Left,
                position: from,
                modifiers: Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            }
            .to_platform_input(),
            cx,
        );
        window.frame(cx);
        for k in 1..=8 {
            let t = k as f32 / 8.0;
            let at = point(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t);
            window.dispatch_event(
                MouseMoveEvent {
                    position: at,
                    pressed_button: Some(MouseButton::Left),
                    modifiers: Modifiers::default(),
                }
                .to_platform_input(),
                cx,
            );
            window.frame(cx);
        }
        window.dispatch_event(
            MouseUpEvent {
                button: MouseButton::Left,
                position: to,
                modifiers: Modifiers::default(),
                click_count: 1,
            }
            .to_platform_input(),
            cx,
        );
        window.frame(cx);
    })
    .unwrap();
}

pub fn press(cx: &mut TestAppContext, o: &Open, key: &str) {
    cx.update_window(o.handle, |_, window, cx| window.press(key, cx))
        .unwrap();
    cx.run_until_parked();
    frames(cx, o.handle, 1);
}

pub fn type_text(cx: &mut TestAppContext, o: &Open, text: &str) {
    cx.update_window(o.handle, |_, window, cx| window.type_text(text, cx))
        .unwrap();
    cx.run_until_parked();
    frames(cx, o.handle, 1);
}

/// Click the element labelled `label`.
pub fn click_label(cx: &mut TestAppContext, o: &Open, label: &str) {
    cx.update_window(o.handle, |_, window, cx| window.click_label(label, cx))
        .unwrap();
    cx.run_until_parked();
    frames(cx, o.handle, 1);
}

/// Whether an element labelled `label` is in the last frame.
pub fn shows(cx: &mut TestAppContext, o: &Open, label: &str) -> bool {
    cx.update_window(o.handle, |_, window, cx| {
        window.frame(cx);
        window.elements().iter().any(|e| e.label() == Some(label))
    })
    .unwrap()
}

/// Every accessible label in the last frame (for failure messages).
pub fn labels(cx: &mut TestAppContext, o: &Open) -> Vec<String> {
    cx.update_window(o.handle, |_, window, _| {
        window
            .elements()
            .iter()
            .filter_map(|e| e.label().map(str::to_string))
            .collect()
    })
    .unwrap()
}

/// The text on the clipboard.
pub fn clipboard(cx: &mut TestAppContext) -> Option<String> {
    cx.read_from_clipboard().and_then(|c| c.text())
}

/// Run an engine command in the Shell, then draw.
pub fn run(cx: &mut TestAppContext, o: &Open, id: &str, params: serde_json::Value) {
    o.shell.update(cx, |s, cx| {
        assert!(s.run(id, &params).is_some(), "{id} {params}");
        cx.notify();
    });
    cx.run_until_parked();
    frames(cx, o.handle, 2);
}
