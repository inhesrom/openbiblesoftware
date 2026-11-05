//! Text size keys and Interface size (#137), through the headless harness with key events as
//! egui-winit reports them on real keyboards: the active key is the logical key (what the key
//! types with the held Shift), else the physical key (AZERTY's à key under Ctrl is Num0).

use egui::Key;

use super::*;

/// Ctrl as Linux and Windows report it, and ⌘ as macOS does.
const CTRL: Modifiers = Modifiers {
    ctrl: true,
    command: true,
    ..Modifiers::NONE
};
const MAC_CMD: Modifiers = Modifiers {
    mac_cmd: true,
    command: true,
    ..Modifiers::NONE
};

/// One key press as egui-winit reports it: the active key, the physical key and Shift.
#[derive(Clone, Copy, Debug)]
struct Report {
    what: &'static str,
    key: Key,
    physical: Key,
    shift: bool,
}

const fn report(what: &'static str, key: Key, physical: Key, shift: bool) -> Report {
    Report {
        what,
        key,
        physical,
        shift,
    }
}

/// A keyboard layout's reports for Larger text, Smaller text and Reset text size.
struct Layout {
    name: &'static str,
    larger: &'static [Report],
    smaller: &'static [Report],
    reset: &'static [Report],
}

/// The numpad's + and − report Plus and Minus on every layout.
const NUMPAD_PLUS: Report = report("numpad +", Key::Plus, Key::Plus, false);
const NUMPAD_MINUS: Report = report("numpad −", Key::Minus, Key::Minus, false);

const LAYOUTS: &[Layout] = &[
    // US and UK: + is Shift+=, − and 0 are unshifted.
    Layout {
        name: "US",
        larger: &[
            report("Ctrl+=", Key::Equals, Key::Equals, false),
            report("Ctrl+Shift+= (+)", Key::Plus, Key::Equals, true),
            NUMPAD_PLUS,
        ],
        smaller: &[
            report("Ctrl+−", Key::Minus, Key::Minus, false),
            report("Ctrl+−", Key::Minus, Key::Minus, false),
            NUMPAD_MINUS,
        ],
        reset: &[report("Ctrl+0", Key::Num0, Key::Num0, false)],
    },
    Layout {
        name: "UK",
        larger: &[
            report("Ctrl+=", Key::Equals, Key::Equals, false),
            report("Ctrl+Shift+= (+)", Key::Plus, Key::Equals, true),
            NUMPAD_PLUS,
        ],
        smaller: &[
            NUMPAD_MINUS,
            report("Ctrl+−", Key::Minus, Key::Minus, false),
            report("Ctrl+−", Key::Minus, Key::Minus, false),
        ],
        reset: &[report("Ctrl+0", Key::Num0, Key::Num0, false)],
    },
    // French AZERTY: = and + share the key right of ), − is the unshifted 6 key, and 0 is
    // Shift+à. Ctrl+à types no Latin key, so egui-winit reports the physical Num0.
    Layout {
        name: "AZERTY",
        larger: &[
            report("Ctrl+=", Key::Equals, Key::Equals, false),
            report("Ctrl+Shift+= (+)", Key::Plus, Key::Equals, true),
            NUMPAD_PLUS,
        ],
        smaller: &[
            report("Ctrl+− (6 key)", Key::Minus, Key::Num6, false),
            NUMPAD_MINUS,
            report("Ctrl+− (6 key)", Key::Minus, Key::Num6, false),
        ],
        reset: &[
            report("Ctrl+à", Key::Num0, Key::Num0, false),
            report("Ctrl+Shift+à (0)", Key::Num0, Key::Num0, true),
        ],
    },
    // German QWERTZ: + is its own unshifted key (Ctrl++).
    Layout {
        name: "QWERTZ",
        larger: &[
            report("Ctrl++", Key::Plus, Key::CloseBracket, false),
            report("Ctrl++", Key::Plus, Key::CloseBracket, false),
            NUMPAD_PLUS,
        ],
        smaller: &[
            report("Ctrl+−", Key::Minus, Key::Slash, false),
            NUMPAD_MINUS,
            report("Ctrl+−", Key::Minus, Key::Slash, false),
        ],
        reset: &[report("Ctrl+0", Key::Num0, Key::Num0, false)],
    },
];

fn press(h: &mut Harness, r: Report, held: Modifiers) {
    let modifiers = Modifiers {
        shift: r.shift,
        ..held
    };
    h.modifiers = modifiers;
    h.events.push(Event::Key {
        key: r.key,
        physical_key: Some(r.physical),
        pressed: true,
        repeat: false,
        modifiers,
    });
    h.frame();
    h.events.push(Event::Key {
        key: r.key,
        physical_key: Some(r.physical),
        pressed: false,
        repeat: false,
        modifiers,
    });
    h.modifiers = Modifiers::NONE;
    h.frame();
}

/// Press each layout's Larger, Smaller and Reset reports with `held` (Ctrl, or ⌘ on macOS):
/// Text size steps up and back down exactly, Ctrl+0 restores the default, the size shows
/// briefly, and the window's zoom never moves.
fn text_size_keys_with(held: Modifiers, layouts: &[Layout]) {
    let default = obs_layout::TextSettings::default().size;
    let mut h = Harness::new();
    h.frames(2);
    for layout in layouts {
        let size = |h: &Harness| h.shell.st.session.text.size;
        assert_eq!(size(&h), default, "{}", layout.name);
        for (n, r) in layout.larger.iter().enumerate() {
            press(&mut h, *r, held);
            let want = default + (n + 1) as f32;
            assert_eq!(size(&h), want, "{} {} is Larger text", layout.name, r.what);
            assert!(
                h.find(&format!("Text {want} pt")).is_some(),
                "{} {}: no toast in {:?}",
                layout.name,
                r.what,
                h.texts()
            );
        }
        for (n, r) in layout.smaller.iter().enumerate() {
            press(&mut h, *r, held);
            let want = default + (layout.larger.len() - n - 1) as f32;
            assert_eq!(size(&h), want, "{} {} is Smaller text", layout.name, r.what);
        }
        for r in layout.reset {
            press(&mut h, layout.larger[0], held);
            press(&mut h, layout.larger[0], held);
            assert_eq!(size(&h), default + 2.0);
            press(&mut h, *r, held);
            assert_eq!(size(&h), default, "{} {} resets", layout.name, r.what);
            assert!(h.find(&format!("Text {default} pt")).is_some());
        }
        // Not egui's whole-window zoom: chrome stays at the Interface size.
        assert_eq!(h.ctx.zoom_factor(), 1.0, "{}", layout.name);
        assert_eq!(h.ctx.pixels_per_point(), 1.0, "{}", layout.name);
    }
}

#[test]
fn ctrl_text_size_keys_on_us_uk_azerty_and_qwertz_keyboards_never_zoom_the_window() {
    text_size_keys_with(CTRL, LAYOUTS);
}

#[test]
fn cmd_text_size_keys_on_macos_never_zoom_the_window() {
    // macOS can report ⌘+Shift+= as the = key with Shift held.
    const MAC: &[Layout] = &[Layout {
        name: "macOS US",
        larger: &[
            report("⌘=", Key::Equals, Key::Equals, false),
            report("⌘+Shift+= (+)", Key::Plus, Key::Equals, true),
            report("⌘+Shift+= as =", Key::Equals, Key::Equals, true),
            NUMPAD_PLUS,
        ],
        smaller: &[
            report("⌘−", Key::Minus, Key::Minus, false),
            NUMPAD_MINUS,
            report("⌘−", Key::Minus, Key::Minus, false),
            report("⌘−", Key::Minus, Key::Minus, false),
        ],
        reset: &[report("⌘0", Key::Num0, Key::Num0, false)],
    }];
    text_size_keys_with(MAC_CMD, MAC);
    text_size_keys_with(MAC_CMD, LAYOUTS);
}

#[test]
fn interface_size_scales_the_chrome_survives_a_restart_and_text_keys_never_change_it() {
    let db = temp_db("interface-size");
    let mut h = Harness::launch(Some(&db));
    assert_eq!(h.ctx.zoom_factor(), 1.0);
    h.open_menu("View");
    h.click_text("Interface size");
    h.click_text("Larger");
    h.frames(2);
    assert_eq!(
        h.shell.st.session.appearance.interface_size,
        obs_engine::InterfaceSize::Larger
    );
    assert_eq!(h.ctx.zoom_factor(), 1.25);
    // Every point of chrome is drawn 1.25 pixels wide.
    assert_eq!(h.ctx.pixels_per_point(), 1.25);

    // Text size keys leave it alone.
    let size = h.shell.st.session.text.size;
    let us = &LAYOUTS[0];
    for r in us.larger.iter().chain(us.smaller).chain(us.reset) {
        press(&mut h, *r, CTRL);
        assert_eq!(h.ctx.zoom_factor(), 1.25, "{}", r.what);
    }
    assert_eq!(h.shell.st.session.text.size, size);
    drop(h);

    // After a restart; a stale zoom from anywhere else is put back to the Interface size.
    let mut h = Harness::launch(Some(&db));
    assert_eq!(h.ctx.zoom_factor(), 1.25);
    h.ctx.set_zoom_factor(1.6);
    h.frames(3);
    assert_eq!(h.ctx.zoom_factor(), 1.25);
    h.open_menu("View");
    h.click_text("Interface size");
    h.click_text("Default");
    h.frames(2);
    assert_eq!(h.ctx.zoom_factor(), 1.0);
    drop(h);
    let h = Harness::launch(Some(&db));
    assert_eq!(h.ctx.zoom_factor(), 1.0);
    drop(h);
    std::fs::remove_dir_all(db.parent().unwrap()).unwrap();
}

#[test]
fn a_text_size_that_cannot_be_saved_shows_the_error_not_the_size() {
    let db = temp_db("text-size-unsaved");
    let mut h = Harness::new();
    let (store, _) = obs_store::UserStore::open(&db).unwrap();
    h.shell
        .st
        .session
        .attach_user(Box::new(obs_engine::testing::PreferenceWritesFail(store)));
    let default = obs_layout::TextSettings::default().size;
    press(&mut h, LAYOUTS[0].larger[0], CTRL);
    let texts = h.texts();
    assert!(
        texts.iter().any(|t| t.contains("disk full")),
        "no save error in {texts:?}"
    );
    let size = format!("Text {} pt", default + 1.0);
    assert!(!texts.contains(&size.as_str()), "{size} hides the error");
    drop(h);
    std::fs::remove_dir_all(db.parent().unwrap()).unwrap();
}
