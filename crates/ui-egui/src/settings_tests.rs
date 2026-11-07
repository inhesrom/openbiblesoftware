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
    // Every section is listed.
    for title in ["TEXT AND FONTS", "INTERFACE SIZE", "THEME AND PAGE TONE"] {
        assert!(in_tab(&h, tab, title).is_some(), "{title}: {:?}", h.texts());
    }
    preview(&h, "settings-tab");
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
            SettingKind::Action { .. } if setting.id.starts_with("clear") => {
                format!("{}…", setting.label)
            }
            SettingKind::Action { command } => h
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
    assert!(in_tab(&h, tab, "Ctrl+Up").is_some(), "{:?}", h.texts());
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
