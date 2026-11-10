//! The Text size scenario (#299): Ctrl+Plus and Ctrl+Minus with a whole Bible in two Tabs and
//! a whole-Bible Commentary in a third, all in view. Each press is timed from its frame
//! starting to the end of the first frame in which every one of the three Tabs draws the text
//! in view at the new size, every block laid out (no placeholders): the time until the change
//! is visible. Then Ctrl+Plus is held for ten steps, as key repeat sends them, and the last
//! step is timed the same way: earlier steps' layouts must not queue up ahead of it.
//!
//! Between presses the first Bible is scrolled, so the timed Tab's ordinary frames are held
//! to #19's 2 ms synchronous-work budget. The frames from a press until its text shows are
//! exempt, as #19 exempts the block a jump lands in, and held to the time budget instead.

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

/// Two whole Bibles (verses of 15–35 words, as long as a real English Bible's; one with
/// Strong's numbers, as the KJV) and a whole-Bible Commentary (about JFB's size), opened
/// side by side at Psalm 119.
pub(crate) fn text_size(cfg: &BenchConfig) -> Result<Samples, String> {
    let resources = [
        built(
            library::english_bible("bench-a", "BA", &Book::ALL, 15..35, true, 1),
            "whole Bible",
        )?,
        built(
            library::english_bible("bench-b", "BB", &Book::ALL, 15..35, false, 2),
            "whole Bible",
        )?,
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
    let size = |b: &ShellBench| b.shell.session().text.size;
    let shown = move |b: &ShellBench| {
        let size = size(b);
        tabs.iter()
            .all(|&t| b.shell.reading_text_drawn_at(t, size))
    };
    b.wait("every Tab's text", shown)?;
    let pane = b.pane(first).ok_or("the Bible is not drawn")?;
    let over = pane.center();
    let mut s = Samples::default();
    b.wheel(&mut s, over, -40.0, cfg.tool_frames / 4);
    // Single presses, larger and smaller in turn.
    for k in 0..cfg.tool_steps {
        let before = size(&b);
        b.events.push(press(k % 2 == 0, false));
        b.settle_timed(&mut s, ActionWork::Exempt, |_| {}, |b| {
            size(b) != before && shown(b)
        });
        b.wheel(&mut s, over, -40.0, cfg.tool_frames / 4);
    }
    // Held: the first press, then key repeats, a step every other frame. Only the last is
    // timed: until the text shows at the final size.
    let before = size(&b);
    for step in 0..HELD_STEPS - 1 {
        b.events.push(press(true, step > 0));
        for _ in 0..REPEAT_FRAMES {
            b.exempt_frame(&mut s);
        }
    }
    b.events.push(press(true, true));
    let held = before + HELD_STEPS as f32 * obs_engine::appearance::TEXT_SIZE_STEP;
    b.settle_timed(&mut s, ActionWork::Exempt, |_| {}, |b| {
        size(b) == held && shown(b)
    });
    if size(&b) != held {
        return Err(format!(
            "holding Ctrl+Plus for {HELD_STEPS} steps from {before} pt reached {} pt",
            size(&b)
        ));
    }
    b.wheel(&mut s, over, -40.0, cfg.tool_frames / 4);
    b.measured(s)
}
