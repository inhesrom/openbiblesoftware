//! The Settings Tab (#213) through the shell's real-event headless harness: Ctrl+, opens it
//! with the search box ready, the search finds every Setting, a change made there applies at
//! once (as from the View menu), and each Setting's Reset link restores its default.

use egui::Key;
use obs_engine::{
    HeaderBox, PageTone, SETTINGS, Setting, SettingKind, TabId, Theme, setting_by_id,
};

use super::*;

/// Ctrl as Linux and Windows report it.
const CTRL: Modifiers = Modifiers {
    ctrl: true,
    command: true,
    ..Modifiers::NONE
};

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

fn preview(h: &Harness, name: &str) {
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
        std::fs::create_dir_all(&dir).unwrap();
        h.screenshot(&dir.join(format!("{name}.ppm")));
    }
}

fn pane_rect(h: &Harness, tab: TabId) -> Rect {
    let tiles = &h.shell.tree.tiles;
    tiles.rect(tiles.find_pane(&tab).unwrap()).unwrap()
}

/// Where `text` is painted in the Tab's body (under its header, whose search box may hold
/// the same words).
fn in_tab(h: &Harness, tab: TabId, text: &str) -> Option<Rect> {
    let mut r = pane_rect(h, tab);
    // The header's More menu button is the lowest thing above the body.
    if let Some(menu) = (h.shapes.iter()).find(|(s, t)| t == "[ellipsis]" && r.contains(s.center()))
    {
        r.min.y = menu.0.bottom() + 2.0;
    }
    (h.shapes.iter())
        .find(|(s, t)| t == text && r.contains(s.center()))
        .map(|(s, _)| *s)
}

fn click_in_tab(h: &mut Harness, tab: TabId, text: &str) {
    let p = in_tab(h, tab, text)
        .unwrap_or_else(|| panic!("{text:?} is not in the Tab: {:?}", h.texts()))
        .center();
    h.click(p);
}

/// Where `text` is painted on the same line as `anchor` (the same words may be elsewhere, in
/// the embedded Keyboard shortcuts).
fn beside(h: &Harness, tab: TabId, anchor: &str, text: &str) -> Pos2 {
    let row = in_tab(h, tab, anchor)
        .unwrap_or_else(|| panic!("{anchor:?} is not in the Tab: {:?}", h.texts()));
    (h.shapes.iter())
        .find(|(s, t)| t == text && (s.center().y - row.center().y).abs() < 6.0)
        .map(|(s, _)| s.center())
        .unwrap_or_else(|| panic!("no {text:?} beside {anchor:?}: {:?}", h.texts()))
}

/// The Reset link on the same line as the Setting named `label`.
fn reset_of(h: &Harness, tab: TabId, label: &str) -> Pos2 {
    beside(h, tab, label, "Reset")
}

/// Ctrl+, opens (or focuses) Settings.
fn open_settings(h: &mut Harness) -> TabId {
    key(h, Key::Comma, CTRL);
    let tab = h.tab_id("Settings");
    let ws = &h.shell.st.session.workspace;
    assert_eq!(ws.focused(), Some(tab));
    let t = ws.tab(tab).unwrap();
    assert_eq!(t.kind, TabKind::Settings);
    assert_eq!(t.header_box(), HeaderBox::Filter);
    tab
}

fn search(h: &mut Harness, tab: TabId, query: &str) {
    h.shell.st.tabs.get_mut(&tab).unwrap().filter = query.to_string();
    h.frames(2);
}

/// Acceptance: Ctrl+, opens Settings with the search box ready to type, and the search
/// filters the Settings.
#[test]
fn ctrl_comma_opens_settings_ready_to_search() {
    let mut h = Harness::new();
    let tab = open_settings(&mut h);
    // Every section is listed, the first ones in view.
    for title in ["TEXT AND FONTS", "INTERFACE SIZE"] {
        assert!(in_tab(&h, tab, title).is_some(), "{title}: {:?}", h.texts());
    }
    preview(&h, "settings-tab");
    let body = pane_rect(&h, tab).center();
    h.events.push(Event::PointerMoved(body));
    h.events.push(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, -200.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: Modifiers::NONE,
    });
    h.frames(3);
    assert!(
        in_tab(&h, tab, "THEME AND PAGE TONE").is_some(),
        "{:?}",
        h.texts()
    );
    // Typed straight into the search box.
    h.events.push(Event::Text("sepia".into()));
    h.frames(2);
    assert_eq!(h.shell.st.tabs[&tab].filter, "sepia");
    assert!(in_tab(&h, tab, "Page tone").is_some());
    assert!(in_tab(&h, tab, "Sepia").is_some());
    assert!(in_tab(&h, tab, "Text size").is_none());
    assert!(in_tab(&h, tab, "Theme").is_none());
    preview(&h, "settings-search-sepia");
    // Ctrl+, again keeps the one Tab.
    key(&mut h, Key::Comma, CTRL);
    let count = (h.shell.st.session.workspace.tabs())
        .filter(|t| t.kind == TabKind::Settings)
        .count();
    assert_eq!(count, 1);
    search(&mut h, tab, "no such setting");
    assert!(in_tab(&h, tab, "No setting matches “no such setting”.").is_some());
}

/// Acceptance: every Setting is found by searching for it, and is drawn with its control.
#[test]
fn search_finds_every_setting() {
    let db = temp_db("settings-search");
    let mut h = Harness::launch(Some(&db));
    let tab = open_settings(&mut h);
    for setting in SETTINGS {
        search(&mut h, tab, setting.label);
        let label = match setting.kind {
            // An action's row is its button.
            SettingKind::Action { confirm: true, .. } => {
                format!("{}…", setting.label)
            }
            SettingKind::Action { command, .. } => h
                .shell
                .st
                .session
                .commands()
                .get(command)
                .unwrap()
                .label
                .to_string(),
            _ => setting.label.to_string(),
        };
        assert!(
            in_tab(&h, tab, &label).is_some(),
            "{:?} does not show {label:?}: {:?}",
            setting.label,
            h.texts()
        );
        // Its own row (the line saying what it does), not only a shortcut so named.
        assert!(
            in_tab(&h, tab, setting.help).is_some(),
            "{:?} does not show its row: {:?}",
            setting.label,
            h.texts()
        );
    }
}

/// Change a Setting from its row, as a user would.
fn change_from_row(h: &mut Harness, tab: TabId, setting: &Setting) {
    search(h, tab, setting.label);
    match setting.kind {
        SettingKind::Choice { command, default } => {
            let (choices, _) = obs_engine::choices(&h.shell.st.session, command);
            let other = choices.iter().find(|c| c.0 != default).unwrap().1;
            // The choice under this Setting's name (another row may have one so named).
            let row = in_tab(h, tab, setting.label).unwrap();
            let p = (h.shapes.iter())
                .filter(|(s, t)| t == other && s.top() > row.bottom())
                .min_by(|a, b| a.0.top().total_cmp(&b.0.top()))
                .unwrap()
                .0
                .center();
            h.click(p);
        }
        SettingKind::Toggle { .. } => click_in_tab(h, tab, setting.label),
        SettingKind::TextSize => click_in_tab(h, tab, "[plus]"),
        SettingKind::NotebooksInText => click_in_tab(h, tab, obs_store::DEFAULT_NOTEBOOK_NAME),
        _ => {}
    }
}

/// Acceptance: a change made in Settings applies at once (the window turns dark, text grows,
/// the toolbar moves), and Reset restores each default; both are saved.
#[test]
fn changes_apply_live_and_reset_restores_each_default() {
    let db = temp_db("settings-live");
    let mut h = Harness::launch(Some(&db));
    let tab = open_settings(&mut h);
    let changed: Vec<&Setting> = (SETTINGS.iter())
        .filter(|s| {
            matches!(
                s.kind,
                SettingKind::Choice { .. }
                    | SettingKind::Toggle { .. }
                    | SettingKind::TextSize
                    | SettingKind::NotebooksInText
            )
        })
        .collect();
    let size = h.shell.st.session.text.size;
    for setting in &changed {
        change_from_row(&mut h, tab, setting);
        assert_eq!(
            h.shell.st.session.setting_is_default(setting.id),
            Some(false),
            "{} did not change from its row",
            setting.id
        );
    }
    // Live: the chrome is dark, Resource text is a step larger, the toolbar is elsewhere.
    let s = &h.shell.st.session;
    assert_ne!(s.appearance.theme, Theme::System);
    assert_ne!(s.appearance.page_tone, PageTone::Auto);
    assert_eq!(s.text.size, size + 1.0);
    assert_ne!(s.toolbar.position, obs_engine::ToolbarPosition::Top);
    assert_ne!(
        h.ctx.zoom_factor(),
        1.0,
        "the Interface size applies at once"
    );
    search(&mut h, tab, "theme");
    let row = in_tab(&h, tab, "Theme").unwrap();
    let dark = (h.shapes.iter())
        .filter(|(r, t)| t == "Dark" && r.top() > row.bottom())
        .min_by(|a, b| a.0.top().total_cmp(&b.0.top()))
        .unwrap()
        .0
        .center();
    h.click(dark);
    assert_eq!(h.shell.st.session.appearance.theme, Theme::Dark);
    assert!(
        h.ctx.global_style().visuals.dark_mode,
        "the chrome turns dark at once"
    );
    search(&mut h, tab, "");
    preview(&h, "settings-changed");
    for setting in &changed {
        search(&mut h, tab, setting.label);
        let p = reset_of(&h, tab, setting.label);
        h.click(p);
        assert_eq!(
            h.shell.st.session.setting_is_default(setting.id),
            Some(true),
            "{}'s Reset did not restore it",
            setting.id
        );
    }
    let s = &h.shell.st.session;
    assert_eq!(s.text.size, size);
    assert_eq!(s.appearance, obs_engine::Appearance::default());
    drop(h);
    let mut h = Harness::launch(Some(&db));
    for setting in &changed {
        assert_eq!(
            h.shell.st.session.setting_is_default(setting.id),
            Some(true),
            "{} after a restart",
            setting.id
        );
    }
}

/// The rows that open a window or a Tab: Fonts, Customize toolbar, Home, Get Resources.
#[test]
fn rows_open_the_windows_and_tabs_that_change_them() {
    let mut h = Harness::new();
    let tab = open_settings(&mut h);
    search(&mut h, tab, "Greek text font");
    click_in_tab(&mut h, tab, "Choose…");
    assert!(h.shell.st.fonts_ui.open);
    key(&mut h, Key::Escape, Modifiers::NONE);
    h.shell.st.fonts_ui.open = false;
    h.frames(2);
    h.click(h.tab_at("Settings"));
    search(&mut h, tab, "Launchers");
    click_in_tab(&mut h, tab, "Customize toolbar…");
    assert!(h.shell.st.toolbar.customizing);
    h.shell.st.toolbar.customizing = false;
    h.frames(2);
    search(&mut h, tab, "Get Resources");
    let get = setting_by_id("get_resources").unwrap();
    let p = beside(&h, tab, get.help, "Get Resources…");
    h.click(p);
    let ws = &h.shell.st.session.workspace;
    let focused = ws.focused().and_then(|t| ws.tab(t)).map(|t| t.kind.clone());
    assert_eq!(focused, Some(TabKind::GetBibles));
}

/// Keyboard shortcuts are embedded: the search finds a binding by its name, and Reset beside
/// the section restores every key.
#[test]
fn keyboard_shortcuts_are_embedded_and_reset_together() {
    let db = temp_db("settings-keys");
    let mut h = Harness::launch(Some(&db));
    h.shell.run(
        "keys.set",
        &json!({ "command": "view.text_larger", "key": "Ctrl+Up" }),
    );
    let tab = open_settings(&mut h);
    search(&mut h, tab, "larger text");
    assert!(
        in_tab(&h, tab, "Keyboard shortcuts").is_some(),
        "{:?}",
        h.texts()
    );
    assert!(in_tab(&h, tab, "Ctrl+↑").is_some(), "{:?}", h.texts());
    assert!(in_tab(&h, tab, "Smaller text").is_none());
    let p = reset_of(&h, tab, "Keyboard shortcuts");
    h.click(p);
    let s = &h.shell.st.session;
    assert_eq!(s.shortcut("view.text_larger"), Some("Ctrl+Plus"));
    assert!(setting_by_id("keyboard_shortcuts").unwrap().has_reset());
}

/// Privacy and data asks before it clears History.
#[test]
fn clearing_history_asks_first() {
    let db = temp_db("settings-history");
    let mut h = Harness::launch(Some(&db));
    let bible = (h.shell.st.session.workspace.tabs())
        .find(|t| matches!(t.kind, TabKind::Resource(_)))
        .unwrap()
        .id;
    for place in ["Gen 1:1", "Gen 1:5"] {
        h.shell
            .run("tab.go", &json!({ "tab": bible.0, "text": place }));
        h.frames(2);
    }
    h.shell.save_session(true);
    assert!(!h.shell.st.session.history().is_empty());
    let tab = open_settings(&mut h);
    search(&mut h, tab, "Clear History");
    click_in_tab(&mut h, tab, "Clear History…");
    assert!(in_tab(&h, tab, "Clear History? This cannot be undone.").is_some());
    click_in_tab(&mut h, tab, "Cancel");
    assert!(in_tab(&h, tab, "Clear History…").is_some());
    click_in_tab(&mut h, tab, "Clear History…");
    let p = beside(&h, tab, "Clear History? This cannot be undone.", "Clear");
    h.click(p);
    assert!(h.shell.st.session.history().is_empty());
}

/// `text` typed into the Command box and Enter.
fn run_from_the_command_box(h: &mut Harness, text: &str) {
    h.shell.run("command_box.focus", &json!({}));
    h.frames(2);
    h.events.push(Event::Text(text.into()));
    h.frames(2);
    key(h, Key::Enter, Modifiers::NONE);
}

/// Clear History and Clear Search history run from the Command box (#226) ask first, as
/// their Settings buttons do: Settings shows the row asking Clear or Cancel; Cancel keeps
/// everything, Clear clears that one only.
#[test]
fn clearing_history_from_the_command_box_asks_first() {
    let db = temp_db("settings-command-box-history");
    let mut h = Harness::launch(Some(&db));
    let bible = (h.shell.st.session.workspace.tabs())
        .find(|t| matches!(t.kind, TabKind::Resource(_)))
        .unwrap()
        .id;
    for place in ["Gen 1:1", "Gen 1:5"] {
        h.shell
            .run("tab.go", &json!({ "tab": bible.0, "text": place }));
        h.frames(2);
    }
    h.shell.run("search.run", &json!({ "text": "light" }));
    h.frames(2);
    h.shell.save_session(true);
    let visits = h.shell.st.session.history().len();
    let searches = h.shell.st.session.search_history().len();
    assert!(visits > 0 && searches > 0);

    for (typed, asking, cleared) in [
        (">clear history", "Clear History? This cannot be undone.", 0),
        (
            ">clear search history",
            "Clear Search history? This cannot be undone.",
            1,
        ),
    ] {
        let counts = |h: &Harness| {
            let s = &h.shell.st.session;
            [s.history().len(), s.search_history().len()]
        };
        let before = counts(&h);
        // Picking it only asks.
        run_from_the_command_box(&mut h, typed);
        let tab = h.tab_id("Settings");
        assert_eq!(h.shell.st.session.workspace.focused(), Some(tab));
        assert!(
            in_tab(&h, tab, asking).is_some(),
            "{typed}: {:?}",
            h.texts()
        );
        assert_eq!(counts(&h), before, "{typed}: nothing cleared yet");
        // Cancel keeps everything.
        let p = beside(&h, tab, asking, "Cancel");
        h.click(p);
        assert!(in_tab(&h, tab, asking).is_none());
        assert_eq!(counts(&h), before, "{typed}: Cancel keeps it");
        // Asked again, Clear clears it.
        run_from_the_command_box(&mut h, typed);
        let p = beside(&h, tab, asking, "Clear");
        h.click(p);
        let after = counts(&h);
        assert_eq!(after[cleared], 0, "{typed}");
        assert_eq!(
            after[1 - cleared],
            before[1 - cleared],
            "{typed}: only that one"
        );
    }
}

/// Privacy and data opens Back up & restore (#217) at once: opening a Tab needs no
/// confirmation.
#[test]
fn privacy_and_data_opens_back_up_and_restore() {
    let db = temp_db("settings-backups");
    let mut h = Harness::launch(Some(&db));
    let tab = open_settings(&mut h);
    search(&mut h, tab, "backup");
    assert!(in_tab(&h, tab, "This cannot be undone.").is_none());
    preview(&h, "settings-backups-kittest");
    click_in_tab(&mut h, tab, "Back up & restore…");
    let backups = h.tab_id(obs_engine::BACKUPS_TITLE);
    assert_eq!(
        h.shell.st.session.workspace.tab(backups).unwrap().kind,
        TabKind::Backups
    );
    assert_eq!(h.shell.st.session.workspace.focused(), Some(backups));
}

/// The Scroll speed slider's track, from the AccessKit tree.
fn scroll_speed_slider(h: &Harness) -> Rect {
    let tree = h.tree.as_ref().expect("the AccessKit tree");
    let node = (tree.nodes.iter().map(|(_, n)| n))
        .find(|n| n.role() == egui::accesskit::Role::Slider && n.label() == Some("Scroll speed"))
        .unwrap_or_else(|| panic!("no Scroll speed slider: {:?}", h.texts()));
    let b = node.bounds().unwrap();
    // Points: the harness draws at one pixel a point.
    #[allow(clippy::cast_possible_truncation)]
    let r = Rect::from_min_max(
        pos2(b.x0 as f32, b.y0 as f32),
        pos2(b.x1 as f32, b.y1 as f32),
    );
    r
}

/// How far one mouse-wheel line over the Settings Tab scrolls it: how far `anchor`'s text
/// moves up.
fn wheel_settings(h: &mut Harness, tab: TabId, anchor: &str) -> f32 {
    let top = |h: &Harness| {
        in_tab(h, tab, anchor)
            .unwrap_or_else(|| panic!("{anchor:?} is not in the Tab: {:?}", h.texts()))
            .top()
    };
    let before = top(h);
    h.events
        .push(Event::PointerMoved(pane_rect(h, tab).center()));
    h.events.push(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: vec2(0.0, -1.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: Modifiers::NONE,
    });
    h.frames(3);
    before - top(h)
}

/// #293 acceptance: Settings → Motion has a Scroll speed slider at 1×, found by "scroll",
/// "wheel" and "speed". Moving it applies at once (the Settings Tab itself then scrolls at the
/// new speed: its live preview), it is kept across a restart, and Reset puts 1× back.
#[test]
fn the_scroll_speed_slider_applies_at_once_and_resets_to_1x() {
    let db = temp_db("settings-scroll-speed");
    let mut h = Harness::launch(Some(&db));
    h.ctx.enable_accesskit();
    let tab = open_settings(&mut h);
    for query in ["scroll", "wheel", "speed", "trackpad"] {
        search(&mut h, tab, query);
        assert!(in_tab(&h, tab, "Scroll speed").is_some(), "{query:?}");
        assert!(in_tab(&h, tab, "1×").is_some(), "{query:?}: at 1×");
    }
    // Under Motion, beside Reduce motion.
    search(&mut h, tab, "motion");
    let (motion, reduce, speed) = (
        in_tab(&h, tab, "MOTION").expect("the Motion section"),
        in_tab(&h, tab, "Reduce motion").unwrap(),
        in_tab(&h, tab, "Scroll speed").unwrap(),
    );
    assert!(motion.top() < reduce.top() && reduce.top() < speed.top());
    preview(&h, "settings-scroll-speed");

    // The whole Tab at 1×: a wheel line moves it this far.
    search(&mut h, tab, "");
    // A help line well inside the first screen of the Tab.
    let anchor = setting_by_id("font_interface").unwrap().help;
    let at_one = wheel_settings(&mut h, tab, anchor);
    assert!(at_one > 10.0, "{at_one}");

    // A click near the slider's right end: about 3× (the slider is logarithmic).
    search(&mut h, tab, "scroll speed");
    let track = scroll_speed_slider(&h);
    h.click(pos2(track.left() + track.width() * 0.9, track.center().y));
    let speed = h.shell.st.session.appearance.scroll_speed;
    assert!((2.0..4.0).contains(&speed), "{speed}");
    assert_eq!(
        h.shell.st.session.setting_is_default("scroll_speed"),
        Some(false)
    );
    let shown = obs_engine::appearance::scroll_speed_label(f64::from(speed));
    assert!(
        in_tab(&h, tab, &shown).is_some(),
        "{shown}: {:?}",
        h.texts()
    );

    // Live: the Settings Tab itself scrolls at the new speed from the next frame.
    search(&mut h, tab, "");
    let moved = wheel_settings(&mut h, tab, anchor);
    assert!(
        (moved - at_one * speed).abs() < 1.0,
        "{moved} at {speed}×, {at_one} at 1×"
    );
    drop(h);

    // Kept across a restart; Reset puts 1× back.
    let mut h = Harness::launch(Some(&db));
    assert_eq!(h.shell.st.session.appearance.scroll_speed, speed);
    let tab = open_settings(&mut h);
    search(&mut h, tab, "scroll speed");
    let p = reset_of(&h, tab, "Scroll speed");
    h.click(p);
    assert_eq!(h.shell.st.session.appearance.scroll_speed, 1.0);
    assert!(in_tab(&h, tab, "1×").is_some());
}
