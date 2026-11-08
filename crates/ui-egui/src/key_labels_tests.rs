//! #262: shortcuts read as the platform writes them (⌘ ⌥ ⇧ on macOS, key names rather than
//! egui's "Comma"/"Plus"/"Minus" everywhere), and their symbols and the interface's arrows
//! draw without a missing-glyph box.

use egui::{FontFamily, FontId, Key, MissingGlyphPolicy};

use super::*;

fn mac() -> Harness {
    let mut h = Harness::new();
    h.ctx.set_os(egui::os::OperatingSystem::Mac);
    h.frames(2);
    h
}

fn key(h: &mut Harness, key: Key, modifiers: Modifiers) {
    h.modifiers = modifiers;
    h.events.push(Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    });
    h.frame();
    h.events.push(Event::Key {
        key,
        physical_key: None,
        pressed: false,
        repeat: false,
        modifiers,
    });
    h.modifiers = Modifiers::NONE;
    h.frames(2);
}

fn hover(h: &mut Harness, p: Pos2) {
    h.events.push(Event::PointerMoved(p));
    // Tooltips show after a short rest.
    for _ in 0..40 {
        h.frame();
    }
}

/// egui key names and the portable "Ctrl" that must not be shown on macOS.
fn assert_no_raw_keys(h: &Harness, mac: bool) {
    for t in h.texts() {
        for raw in [
            "Comma",
            "Ctrl+Plus",
            "Ctrl+Minus",
            "Alt+Left",
            "Alt+Right",
            "PageUp",
        ] {
            assert!(!t.contains(raw), "{t:?} shows {raw}");
        }
        if mac {
            assert!(
                !t.contains("Ctrl+") && !t.contains("Alt+") && !t.contains("Shift+"),
                "{t:?}"
            );
        }
    }
}

/// WORKSPACE-MAC-10 on macOS: the View menu, a Tab's ⋯ menu, the Tools Launcher, the Command
/// box, a tooltip and the Keyboard shortcuts Tab show ⌘ ⌥ ⇧ and the keys' own signs.
#[test]
fn macos_shows_shortcuts_with_modifier_symbols_everywhere() {
    // The Command box.
    let mut h = mac();
    assert!(
        h.find("Go to, study, open or search (⌘L)").is_some(),
        "{:?}",
        h.texts()
    );
    assert_no_raw_keys(&h, true);

    // The View menu.
    h.open_menu("View");
    for want in ["⌘,", "⌘+", "⌘−", "⌘0"] {
        assert!(h.find(want).is_some(), "{want} not in {:?}", h.texts());
    }
    assert_no_raw_keys(&h, true);

    // The reading Tab's ⋯ menu.
    let mut h = mac();
    let place = h.at("Genesis 30:1");
    let more = (h.shapes.iter())
        .filter(|(r, t)| t == "[ellipsis]" && (r.center().y - place.y).abs() < 12.0)
        .map(|(r, _)| r.center())
        .min_by(|a, b| (a.x - place.x).abs().total_cmp(&(b.x - place.x).abs()))
        .expect("the Tab's ⋯ menu");
    h.click(more);
    for want in ["Close Tab", "⌘W", "⌘F"] {
        assert!(h.find(want).is_some(), "{want} not in {:?}", h.texts());
    }
    assert_no_raw_keys(&h, true);

    // The Tools Launcher (Alt+T): Search is Ctrl+Shift+F.
    let mut h = mac();
    key(&mut h, Key::T, Modifiers::ALT);
    assert!(h.find("⇧⌘F").is_some(), "{:?}", h.texts());
    assert_no_raw_keys(&h, true);

    // Tooltips: Back (once there is somewhere to go back to) and Next of your Bibles.
    let mut h = mac();
    let tb = h.tab_id("TB");
    h.shell
        .run("tab.go", &json!({ "tab": tb.0, "text": "Gen 30:12" }));
    h.frames(3);
    let back = h.at("[arrow-left]");
    hover(&mut h, back);
    assert!(h.find("Back (⌥←)").is_some(), "{:?}", h.texts());
    assert_no_raw_keys(&h, true);
    let next = h.at("[chevron-right]");
    hover(&mut h, next);
    assert!(
        h.find("Next of your Bibles (⌘])").is_some(),
        "{:?}",
        h.texts()
    );

    // Help → Keyboard shortcuts.
    let mut h = mac();
    h.shell.run("help.keyboard_shortcuts", &json!({}));
    h.frames(3);
    for want in ["⇧⌘L", "⇧⌘F", "⇧⌘I"] {
        assert!(h.find(want).is_some(), "{want} not in {:?}", h.texts());
    }
    assert!(
        !h.texts().iter().any(|t| t.contains("Ctrl")),
        "{:?}",
        h.texts()
    );
}

/// WORKSPACE-MAC-10 on Windows and Linux: Ctrl stays, but keys show their own signs.
#[test]
fn windows_and_linux_show_key_signs_not_egui_key_names() {
    let mut h = Harness::new();
    assert!(
        h.find("Go to, study, open or search (Ctrl+L)").is_some(),
        "{:?}",
        h.texts()
    );
    h.open_menu("View");
    for want in ["Ctrl+,", "Ctrl++", "Ctrl+−", "Ctrl+0"] {
        assert!(h.find(want).is_some(), "{want} not in {:?}", h.texts());
    }
    assert_no_raw_keys(&h, false);
}

/// WORKSPACE-MAC-9: Help lists only items that do something on screen; List commands (JSON
/// for scripts) is not one of them, and Ctrl+Shift+P runs nothing.
#[test]
fn help_has_no_list_commands_item_that_does_nothing() {
    let mut h = Harness::new();
    h.open_menu("Help");
    assert!(h.find("Keyboard shortcuts").is_some(), "{:?}", h.texts());
    assert!(h.find("List commands").is_none(), "{:?}", h.texts());
    key(&mut h, Key::Escape, Modifiers::NONE);
    let before: Vec<String> = h.texts().iter().map(ToString::to_string).collect();
    key(&mut h, Key::P, Modifiers::COMMAND | Modifiers::SHIFT);
    assert!(h.shell.st.toast.is_none());
    assert_eq!(h.texts(), before);
}

/// WORKSPACE-MAC-8: the interface fonts have → (and the other arrows of interface text), and
/// ⌘ ⌥ ⇧ ⌃ draw, in every family the chrome uses, with no missing-glyph box.
#[test]
fn arrows_and_modifier_symbols_draw_without_a_missing_glyph_box() {
    let h = mac();
    let families = [
        FontFamily::Proportional,
        FontFamily::Monospace,
        FontFamily::Name("display".into()),
    ];
    h.ctx.fonts_mut(|f| {
        for family in &families {
            let font = FontId::new(14.0, family.clone());
            assert!(f.has_glyphs(&font, "←↑→↓−"), "{family:?}");
        }
    });
    h.ctx.set_missing_glyph_policy(MissingGlyphPolicy::Panic);
    let texts = [
        "Find the inflected forms of a word (love → loved)",
        "Edit → Undo brings it back (⌘Z).",
        "Library → Import…",
        "⇧⌘N ⌥← ⌃Tab ⌘− ⌘, ⌥⇧⌘X",
    ];
    h.ctx.fonts_mut(|f| {
        for family in &families {
            for size in [11.5, 12.0, 14.0, 21.0] {
                for text in texts {
                    let galley = f.layout_no_wrap(
                        text.to_string(),
                        FontId::new(size, family.clone()),
                        egui::Color32::WHITE,
                    );
                    assert!(galley.size().x > 0.0);
                }
            }
        }
    });
    h.ctx.set_missing_glyph_policy(MissingGlyphPolicy::Tofu);
}

/// The modifier symbols are drawn from the icon set: real ink, on the baseline, and each one
/// as wide as its drawing.
#[test]
fn modifier_symbols_are_drawn_from_the_icon_set() {
    for (sym, icon) in [
        ("⌘", crate::icons::Icon::CommandKey),
        ("⌥", crate::icons::Icon::OptionKey),
        ("⇧", crate::icons::Icon::ShiftKey),
        ("⌃", crate::icons::Icon::ChevronUp),
    ] {
        let rasterizer = crate::key_labels::modifier_rasterizer();
        let glyph = (rasterizer.rasterize)(&egui::GlyphRasterizerRequest {
            cluster: sym,
            family: &FontFamily::Proportional,
            font_size_px: 14.0,
            subpixel_offset_px: 0.0,
        })
        .unwrap_or_else(|| panic!("{sym} not drawn"));
        let img = &glyph.bitmap.image;
        assert_eq!(img.size, [13, 13], "{sym} ({icon:?})");
        assert!(
            img.pixels.iter().filter(|p| p.a() > 0).count() > 12,
            "{sym}"
        );
        // Its foot is on the baseline: the drawing's bottom (21 of 24) at y = 0.
        assert!((glyph.bitmap.offset_px.y + 11.0).abs() <= 1.0, "{sym}");
        assert!(glyph.advance_px >= 10.0, "{sym}");
    }
    let rasterizer = crate::key_labels::modifier_rasterizer();
    assert!(
        (rasterizer.rasterize)(&egui::GlyphRasterizerRequest {
            cluster: "A",
            family: &FontFamily::Proportional,
            font_size_px: 14.0,
            subpixel_offset_px: 0.0,
        })
        .is_none(),
        "letters are the fonts'"
    );
}
