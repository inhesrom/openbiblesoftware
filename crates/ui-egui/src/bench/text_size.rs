//! The Text size scenario (#299): Ctrl+Plus and Ctrl+Minus with a whole Bible in two Tabs and
//! a whole-Bible Commentary in a third, all in view. Each press is timed from its frame
//! starting to the end of the first frame in which every one of the three Tabs draws the text
//! in view at the new size, every block laid out (no placeholders): the time until the change
//! is visible. Then Ctrl+Plus is held for ten steps, as key repeat sends them, and the last
//! step is timed the same way: earlier steps' layouts must not queue up ahead of it.
//!
//! `text-size-tools` times the same presses with a Text Comparison Tab and a Search Tab.
//!
//! `resource-text-size` (#292) times a Resource's own Text size slider the same way: each
//! step until both Tabs of the Bible draw at it, then a fast drag (a step every frame) until
//! they draw at its last.
//!
//! Between presses the timed Tab (the first Bible; the Text Comparison) is scrolled, so its
//! ordinary frames are held to #19's 2 ms synchronous-work budget. The frames from a press
//! until its text shows are exempt, as #19 exempts the block a jump lands in, and held to the
//! time budget instead.

use egui::{Event, Key, Modifiers};
use obs_engine::TabKind;
use obs_scripture::Book;
use serde_json::json;

use super::tools::{ActionWork, ShellBench, built};
use super::{BenchConfig, Samples, library, scale};

/// Ctrl, as Linux and Windows report it (the shortcut is Command: ⌘ on macOS).
const CTRL: Modifiers = Modifiers {
    ctrl: true,
    command: true,
    ..Modifiers::NONE
};

/// How many steps holding Ctrl+Plus sends.
pub(crate) const HELD_STEPS: usize = 10;

/// Frames between two key-repeat steps at 60 Hz (key repeat is about 30 a second).
const REPEAT_FRAMES: usize = 2;

/// A Text size key press: Ctrl+Plus (`larger`) or Ctrl+Minus, `repeat` when held.
fn press(larger: bool, repeat: bool) -> Event {
    Event::Key {
        key: if larger { Key::Plus } else { Key::Minus },
        physical_key: None,
        pressed: true,
        repeat,
        modifiers: CTRL,
    }
}

/// Two whole Bibles: verses of 15–35 words, as long as a real English Bible's; the first with
/// Strong's numbers, as the KJV.
fn whole_bibles() -> Result<[obs_model::Resource; 2], String> {
    Ok([
        built(
            library::english_bible("bench-a", "BA", &Book::ALL, 15..35, true, 1),
            "whole Bible",
        )?,
        built(
            library::english_bible("bench-b", "BB", &Book::ALL, 15..35, false, 2),
            "whole Bible",
        )?,
    ])
}

/// The two whole Bibles and a whole-Bible Commentary (about JFB's size), opened side by side
/// at Psalm 119.
pub(crate) fn text_size(cfg: &BenchConfig) -> Result<Samples, String> {
    let [a, b] = whole_bibles()?;
    let resources = [
        a,
        b,
        built(scale::commentary("bench-commentary", 3), "Commentary")?,
    ];
    let mut b = ShellBench::new(cfg, "text-size", &resources, |_, _| {})?;
    let first = b
        .tab_of(&TabKind::Resource("bench-a".into()))
        .ok_or("no Bible Tab")?;
    let second = b.open("resource.open", &json!({ "id": "bench-b", "split": true }))?;
    let commentary = b.open(
        "resource.open",
        &json!({ "id": "bench-commentary", "split": true }),
    )?;
    let tabs = [first, second, commentary];
    for t in tabs {
        b.shell
            .run("tab.go", &json!({ "tab": t.0, "text": "Ps 119:105" }));
    }
    b.time_tab(first);
    presses(cfg, &mut b, first, move |b, size| {
        tabs.iter().all(|&t| b.shell.reading_text_drawn_at(t, size))
    })
}

/// The Text Comparison and Search Tabs (#299, as fast as the reading Tabs): a Text Comparison
/// of the two whole Bibles over Psalm 119 (176 Verses) beside the Verses of a whole-Bible
/// search, timed as [`text_size`] times its reading Tabs.
pub(crate) fn text_size_tools(cfg: &BenchConfig) -> Result<Samples, String> {
    let resources = whole_bibles()?;
    let ids = ["bench-a", "bench-b"];
    let mut b = ShellBench::new(cfg, "text-size-tools", &resources, |s, root| {
        s.set_search_cache_dir(root.join("cache"));
        s.refresh_search_indexes();
    })?;
    b.wait("the Search indexes", |b| {
        let s = b.shell.session();
        !s.search.is_building() && ids.iter().all(|id| s.search.is_ready(id))
    })?;
    let search = b.open(
        "search.run",
        &json!({
            "text": "LORD",
            "bibles": ids.join(","),
            "scope": "Whole Bible",
            "split": true,
        }),
    )?;
    let comparison = b.open(
        "tool.text_comparison",
        &json!({ "passage": "Ps 119:105", "split": true }),
    )?;
    b.time_tab(comparison);
    presses(cfg, &mut b, comparison, move |b, size| {
        [comparison, search]
            .iter()
            .all(|&t| b.shell.tool_text_drawn_at(t, size))
    })
}

/// A Resource's own Text size (#292): the slider in a Tab's menu dragged for the first whole
/// Bible, open in two Tabs, beside the second Bible, which keeps the global size. Each slider
/// step is timed from its frame starting to the end of the first frame in which both Tabs of
/// the Bible draw their text at it (the second Bible at the global size), as [`text_size`]
/// times a key press. Then the slider is dragged ten steps, one a frame, and the last step is
/// timed the same way: the slider must feel live, its earlier steps' layouts not queued ahead.
pub(crate) fn resource_text_size(cfg: &BenchConfig) -> Result<Samples, String> {
    let resources = whole_bibles()?;
    let mut b = ShellBench::new(cfg, "resource-text-size", &resources, |_, _| {})?;
    let first = b
        .tab_of(&TabKind::Resource("bench-a".into()))
        .ok_or("no Bible Tab")?;
    let again = b.open("resource.open", &json!({ "id": "bench-a", "split": true }))?;
    let other = b.open("resource.open", &json!({ "id": "bench-b", "split": true }))?;
    for t in [first, again, other] {
        b.shell
            .run("tab.go", &json!({ "tab": t.0, "text": "Ps 119:105" }));
    }
    b.time_tab(first);
    let own = |b: &ShellBench| b.shell.session().text_size_of("bench-a");
    let shown = move |b: &ShellBench, size: f32| {
        let global = b.shell.session().text.size;
        [first, again]
            .iter()
            .all(|&t| b.shell.reading_text_drawn_at(t, size))
            && b.shell.reading_text_drawn_at(other, global)
    };
    // The slider's command, as the Text size row runs it for each step.
    let slide = |size: f32| {
        move |shell: &mut crate::Shell| {
            shell.run(
                "view.resource_text_size",
                &json!({ "resource": "bench-a", "value": size as i64 }),
            );
        }
    };
    b.wait("every Tab's text", |b| shown(b, own(b)))?;
    let pane = b.pane(first).ok_or("the Bible's Tab is not drawn")?;
    let over = pane.center();
    let mut s = Samples::default();
    b.wheel(&mut s, over, -40.0, cfg.tool_frames / 4);
    // Single steps, up and down in turn.
    for k in 0..cfg.tool_steps {
        let to = own(&b) + if k % 2 == 0 { 1.0 } else { -1.0 };
        b.settle_timed(&mut s, ActionWork::Exempt, slide(to), |b| {
            own(b) == to && shown(b, to)
        });
        b.wheel(&mut s, over, -40.0, cfg.tool_frames / 4);
    }
    // A fast drag: a step every frame; only the last is timed.
    let before = own(&b);
    for step in 1..HELD_STEPS {
        slide(before + step as f32)(&mut b.shell);
        b.exempt_frame(&mut s);
    }
    let to = before + HELD_STEPS as f32;
    b.settle_timed(&mut s, ActionWork::Exempt, slide(to), |b| {
        own(b) == to && shown(b, to)
    });
    if own(&b) != to {
        return Err(format!(
            "dragging the slider {HELD_STEPS} steps from {before} pt reached {} pt",
            own(&b)
        ));
    }
    b.wheel(&mut s, over, -40.0, cfg.tool_frames / 4);
    b.measured(s)
}

/// Wait until `shown` at the Text size, then time single presses and a held one, scrolling
/// Tab `scrolled` between them.
fn presses(
    cfg: &BenchConfig,
    b: &mut ShellBench,
    scrolled: obs_engine::TabId,
    shown: impl Fn(&ShellBench, f32) -> bool + Copy,
) -> Result<Samples, String> {
    let size = |b: &ShellBench| b.shell.session().text.size;
    b.wait("every Tab's text", |b| shown(b, size(b)))?;
    let pane = b.pane(scrolled).ok_or("the scrolled Tab is not drawn")?;
    let over = pane.center();
    let mut s = Samples::default();
    b.wheel(&mut s, over, -40.0, cfg.tool_frames / 4);
    // Single presses, larger and smaller in turn.
    for k in 0..cfg.tool_steps {
        let before = size(b);
        b.events.push(press(k % 2 == 0, false));
        b.settle_timed(
            &mut s,
            ActionWork::Exempt,
            |_| {},
            |b| size(b) != before && shown(b, size(b)),
        );
        b.wheel(&mut s, over, -40.0, cfg.tool_frames / 4);
    }
    // Held: the first press, then key repeats, a step every other frame. Only the last is
    // timed: until the text shows at the final size.
    let before = size(b);
    for step in 0..HELD_STEPS - 1 {
        b.events.push(press(true, step > 0));
        for _ in 0..REPEAT_FRAMES {
            b.exempt_frame(&mut s);
        }
    }
    b.events.push(press(true, true));
    let held = before + HELD_STEPS as f32 * obs_engine::appearance::TEXT_SIZE_STEP;
    b.settle_timed(
        &mut s,
        ActionWork::Exempt,
        |_| {},
        |b| size(b) == held && shown(b, held),
    );
    if size(b) != held {
        return Err(format!(
            "holding Ctrl+Plus for {HELD_STEPS} steps from {before} pt reached {} pt",
            size(b)
        ));
    }
    b.wheel(&mut s, over, -40.0, cfg.tool_frames / 4);
    b.measured(s)
}
