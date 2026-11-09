//! The Theme editor (#224) through the shell's real-event headless harness: it opens from
//! Settings, starts from a built-in theme, changes a colour role from its hex field with the
//! whole window as the live preview, warns as soon as a colour does not read, and exports and
//! imports the Custom theme as its file dialogs would, across a restart.

use egui::Key;
use obs_engine::{CustomTheme, Theme, ThemeRole};

use super::a11y_audit::{key, tab_to};
use super::*;
use crate::theme::{self, Chrome};

/// Ctrl as Linux and Windows report it.
const CTRL: Modifiers = Modifiers {
    ctrl: true,
    command: true,
    ..Modifiers::NONE
};

fn folder(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("obs-theme-editor-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn launch(dir: &std::path::Path) -> Harness {
    let mut h = Harness::launch(Some(&dir.join("user.db")));
    h.ctx.enable_accesskit();
    h.frames(2);
    h
}

/// Open the Theme editor from the Settings Tab's Theme editor button, found by its search.
fn open_from_settings(h: &mut Harness) {
    key(h, Key::Comma, CTRL);
    h.events.push(Event::Text("theme editor".into()));
    h.frames(2);
    // The Setting's button, above the Keyboard shortcuts row of the same name.
    let button = (h.shapes.iter())
        .find(|(_, t)| t == "Theme editor")
        .map(|(r, _)| r.center())
        .expect("the Theme editor Setting");
    h.click(button);
    h.frames(2);
    assert!(h.shell.st.theme_editor.open, "{:?}", h.texts());
}

/// Type `text` into the hex field named `name`, replacing what it shows, and press Enter.
fn type_hex(h: &mut Harness, name: &str, text: &str) {
    tab_to(h, name);
    key(h, Key::A, CTRL);
    h.events.push(Event::Text(text.into()));
    h.frames(2);
    key(h, Key::Enter, Modifiers::NONE);
    h.frames(2);
}

/// Press the Theme editor's Start from button for theme `label`.
fn start_from(h: &mut Harness, label: &str) {
    tab_to(h, &format!("Start from {label}"));
    key(h, Key::Enter, Modifiers::NONE);
    h.frames(2);
}

/// Press the button named `name` from the keyboard.
fn press(h: &mut Harness, name: &str) {
    tab_to(h, name);
    key(h, Key::Enter, Modifiers::NONE);
    h.frames(2);
}

fn warnings(h: &Harness) -> Vec<String> {
    (h.texts().into_iter())
        .filter(|t| t.starts_with("Low contrast: "))
        .map(String::from)
        .collect()
}

/// Start from High contrast dark, then change the Text colour in its hex field: the Custom
/// theme becomes the Theme and the whole window draws in it on the next frame; a colour that
/// does not read is warned of at once, with its ratio and what it needs (AAA, as the base is
/// High contrast); Reset puts the base's colour back and the warnings go.
#[test]
fn the_editor_previews_live_and_warns_as_the_user_edits() {
    let dir = folder("live");
    let mut h = launch(&dir);
    open_from_settings(&mut h);
    assert_eq!(h.shell.st.session.appearance.theme, Theme::System);
    assert!(h.find("Your Theme is not the Custom theme now.").is_some());
    assert!(
        h.find("Contrast: every text colour reads at WCAG AA on every background.")
            .is_some(),
        "{:?}",
        h.texts()
    );

    start_from(&mut h, "High contrast dark");
    assert_eq!(h.shell.st.session.appearance.theme, Theme::Custom);
    assert_eq!(theme::c(), theme::HIGH_CONTRAST_DARK, "used at once");
    assert!(
        h.find("Contrast: every text colour reads at WCAG AAA on every background.")
            .is_some()
    );

    type_hex(&mut h, "Text hex colour", "#5a5a5a");
    let ink = egui::Color32::from_rgb(0x5a, 0x5a, 0x5a);
    assert_eq!(
        h.shell.st.session.custom_theme.colors[&ThemeRole::Ink],
        "#5a5a5a".parse().unwrap()
    );
    assert_eq!(theme::c().ink, ink, "the whole window is the preview");
    assert_eq!(
        h.ctx
            .global_style()
            .visuals
            .widgets
            .inactive
            .fg_stroke
            .color,
        ink
    );
    let shown = warnings(&h);
    assert!(
        shown.iter().any(
            |w| w.starts_with("Low contrast: Text on Window background:")
                && w.ends_with("needs 7.0:1")
        ),
        "{shown:?}"
    );
    assert!(
        h.texts()
            .iter()
            .any(|t| t.starts_with("Contrast warnings (") && t.ends_with("WCAG AAA")),
        "{:?}",
        h.texts()
    );

    // The Text row's Reset.
    tab_to(&mut h, "Reset Text");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert!(h.shell.st.session.custom_theme.colors.is_empty());
    assert_eq!(theme::c(), theme::HIGH_CONTRAST_DARK);
    assert!(warnings(&h).is_empty(), "{:?}", warnings(&h));

    // The preview is a named picture.
    let tree = h.tree.as_ref().unwrap();
    assert!(tree.nodes.iter().any(|(_, n)| {
        n.role() == egui::accesskit::Role::Image && n.label() == Some("Preview of the Custom theme")
    }));
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Acceptance (#224): a custom theme round-trips through export and import, and survives a
/// restart. Made in the window, exported and imported as its file dialogs would, then the app
/// is launched again on the same User database and draws the same chrome.
#[test]
fn a_custom_theme_round_trips_through_export_and_import_and_survives_a_restart() {
    let dir = folder("round-trip");
    let file = dir.join("custom.theme.json");
    let mut h = launch(&dir);
    open_from_settings(&mut h);
    start_from(&mut h, "Dark");
    type_hex(&mut h, "Window background hex colour", "#10141c");
    type_hex(&mut h, "Links, focus and Link set A hex colour", "#9cc4ff");
    let made: CustomTheme = h.shell.st.session.custom_theme.clone();
    let chrome: Chrome = theme::c();
    assert_eq!(made.base, Theme::Dark);
    assert_eq!(made.colors.len(), 2);
    assert_eq!(chrome.shell, egui::Color32::from_rgb(0x10, 0x14, 0x1c));

    h.shell.st.theme_file_chosen(true, &file);
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("\"base\": \"dark\""), "{text}");
    assert!(text.contains("\"shell\": \"#10141c\""), "{text}");

    // Somewhere else entirely (replacing the changed colours, as asked), then the file back.
    start_from(&mut h, "Light");
    assert_eq!(h.shell.st.session.custom_theme, made, "asked first");
    press(&mut h, "Replace");
    assert_eq!(h.shell.st.session.custom_theme.base, Theme::Light);
    h.shell.run("view.theme", &json!({ "value": "light" }));
    h.frames(2);
    assert_eq!(theme::c(), theme::LIGHT);
    h.shell.st.theme_file_chosen(false, &file);
    h.frames(2);
    assert_eq!(h.shell.st.session.custom_theme, made);
    assert_eq!(h.shell.st.session.appearance.theme, Theme::Custom);
    assert_eq!(theme::c(), chrome);

    drop(h);
    let h = launch(&dir);
    assert_eq!(h.shell.st.session.custom_theme, made, "after a restart");
    assert_eq!(h.shell.st.session.appearance.theme, Theme::Custom);
    assert_eq!(theme::c(), chrome, "drawn in it after a restart");
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Start from never throws changed colours away unasked: with some changed it asks, and Keep
/// my colours keeps them; with none changed it starts at once. Escape in a hex field drops
/// what was typed rather than applying it.
#[test]
fn start_from_asks_before_replacing_changed_colours_and_escape_cancels_a_hex_field() {
    let dir = folder("start-from");
    let mut h = launch(&dir);
    open_from_settings(&mut h);
    start_from(&mut h, "Dark");
    assert_eq!(h.shell.st.session.custom_theme.base, Theme::Dark);
    type_hex(&mut h, "Text hex colour", "#e0e0e0");
    let made = h.shell.st.session.custom_theme.clone();
    assert_eq!(made.colors.len(), 1);

    start_from(&mut h, "Light");
    assert_eq!(h.shell.st.session.custom_theme, made, "asked first");
    assert!(
        h.find("Start from Light? Your 1 changed colour will be lost.")
            .is_some(),
        "{:?}",
        h.texts()
    );
    press(&mut h, "Keep my colours");
    assert_eq!(h.shell.st.session.custom_theme, made);
    assert!(!h.texts().iter().any(|t| t.starts_with("Start from Light?")));

    // Typed, then Escape: nothing applied (and the window closes, as Escape does).
    tab_to(&mut h, "Window background hex colour");
    key(&mut h, Key::A, CTRL);
    h.events.push(Event::Text("#ff0000".into()));
    h.frames(2);
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(h.shell.st.session.custom_theme, made, "Escape cancels");
    assert!(!h.shell.st.theme_editor.open);

    open_from_settings(&mut h);
    tab_to(&mut h, "Reset Text");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert!(h.shell.st.session.custom_theme.colors.is_empty());
    start_from(&mut h, "Light");
    assert_eq!(h.shell.st.session.custom_theme, CustomTheme::default());
    assert!(
        !h.texts().iter().any(|t| t.starts_with("Start from Light?")),
        "none changed: not asked"
    );
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Writes `target/previews/theme-*.ppm` when `UPDATE_PREVIEWS` is set: Settings in High
/// contrast light and dark, and the Theme editor with a warning.
#[test]
fn previews() {
    if std::env::var_os("UPDATE_PREVIEWS").is_none() {
        return;
    }
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
    std::fs::create_dir_all(&out).unwrap();
    let dir = folder("previews");
    let mut h = launch(&dir);
    key(&mut h, Key::Comma, CTRL);
    for id in ["high_contrast_light", "high_contrast_dark"] {
        h.shell.run("view.theme", &json!({ "value": id }));
        h.frames(15);
        h.screenshot(&out.join(format!("theme-{id}-settings.ppm")));
    }
    h.shell.run("view.theme", &json!({ "value": "light" }));
    open_from_settings(&mut h);
    start_from(&mut h, "Dark");
    type_hex(&mut h, "Secondary text hex colour", "#505860");
    h.frames(15);
    h.screenshot(&out.join("theme-editor-warning.ppm"));
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}
