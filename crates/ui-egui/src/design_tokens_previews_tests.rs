//! Visual evidence for the design-token pass (#221): `target/previews/tokens/<surface>.ppm`
//! when `UPDATE_PREVIEWS` is set, one per Tab kind, pane, window and menu, each in a fresh
//! window. The PR's kittest before and after images are this test run on master and on the
//! branch.

use std::path::Path;

use super::*;

fn key(h: &mut Harness, key: egui::Key, modifiers: Modifiers) {
    for pressed in [true, false] {
        h.events.push(Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        });
        h.frame();
    }
    h.frames(3);
}

/// A surface: its file name, the commands that open it, and what to do after them.
type Surface = (&'static str, &'static [(&'static str, &'static str)], Then);

#[derive(Clone, Copy)]
enum Then {
    Nothing,
    Key(egui::Key, Modifiers),
    Menu(&'static str),
}

const SURFACES: &[Surface] = &[
    ("workspace", &[], Then::Nothing),
    (
        "workspace-dark",
        &[("view.theme", r#"{"value":"dark"}"#)],
        Then::Nothing,
    ),
    (
        "bible-dark-sepia",
        &[
            ("view.theme", r#"{"value":"dark"}"#),
            ("view.page_tone", r#"{"value":"sepia"}"#),
        ],
        Then::Nothing,
    ),
    ("tools-pane", &[], Then::Key(egui::Key::T, Modifiers::ALT)),
    ("guides-pane", &[], Then::Key(egui::Key::G, Modifiers::ALT)),
    ("view-menu", &[], Then::Menu("View")),
    ("library-menu", &[], Then::Menu("Library")),
    ("library", &[("library.open", "{}")], Then::Nothing),
    (
        "get-resources",
        &[("library.get_resources", "{}")],
        Then::Nothing,
    ),
    (
        "starter-library",
        &[("library.starter", "{}")],
        Then::Nothing,
    ),
    (
        "about",
        &[("resource.about", r#"{"id":"test-bible"}"#)],
        Then::Nothing,
    ),
    ("credits", &[("help.credits", "{}")], Then::Nothing),
    (
        "keyboard-shortcuts",
        &[("help.keyboard_shortcuts", "{}")],
        Then::Nothing,
    ),
    ("settings", &[("app.settings", "{}")], Then::Nothing),
    (
        "passage-guide",
        &[("guide.passage_guide", r#"{"passage":"Jn 3:16"}"#)],
        Then::Nothing,
    ),
    (
        "exegetical-guide",
        &[("guide.exegetical_guide", r#"{"passage":"Jn 3:16"}"#)],
        Then::Nothing,
    ),
    (
        "word-study",
        &[("word_study.open", r#"{"strongs":"G26"}"#)],
        Then::Nothing,
    ),
    (
        "factbook",
        &[("guide.factbook", r#"{"name":"Abraham"}"#)],
        Then::Nothing,
    ),
    (
        "topic-guide",
        &[("guide.topic_guide", r#"{"text":"faith"}"#)],
        Then::Nothing,
    ),
    (
        "search",
        &[("search.run", r#"{"text":"love"}"#)],
        Then::Nothing,
    ),
    (
        "text-comparison",
        &[("tool.text_comparison", r#"{"passage":"Jn 3:16"}"#)],
        Then::Nothing,
    ),
    (
        "cross-references",
        &[("tool.cross_references", r#"{"passage":"Jn 3:16"}"#)],
        Then::Nothing,
    ),
    ("notes", &[("tool.notes", "{}")], Then::Nothing),
    ("favorites", &[("tool.favorites", "{}")], Then::Nothing),
    ("history", &[("tool.history", "{}")], Then::Nothing),
    (
        "reading-plans",
        &[("tool.reading_plans", "{}")],
        Then::Nothing,
    ),
    (
        "atlas",
        &[("tool.atlas", r#"{"passage":"Acts 13"}"#)],
        Then::Nothing,
    ),
    (
        "timeline",
        &[("tool.timeline", r#"{"passage":"Gen 12"}"#)],
        Then::Nothing,
    ),
    ("backups", &[("data.backups", "{}")], Then::Nothing),
    ("fonts-window", &[("view.fonts", "{}")], Then::Nothing),
    (
        "highlight-styles",
        &[("view.highlight_styles", "{}")],
        Then::Nothing,
    ),
    (
        "copy-verses",
        &[("tool.copy_verses", r#"{"passages":"Jn 3:16-18"}"#)],
        Then::Nothing,
    ),
    ("find-bar", &[], Then::Key(egui::Key::F, Modifiers::COMMAND)),
    ("command-box", &[("command_box.focus", "{}")], Then::Nothing),
];

#[test]
fn design_token_previews() {
    if std::env::var_os("UPDATE_PREVIEWS").is_none() {
        return;
    }
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews/tokens");
    std::fs::create_dir_all(&out).unwrap();
    for (name, commands, then) in SURFACES {
        let dir = std::env::temp_dir().join(format!("obs-token-previews-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut h = Harness::launch_full(Some(&dir.join("user.db")), JobMode::Inline, Some(&dir));
        settle_all(&mut h);
        for (command, params) in *commands {
            let params: serde_json::Value = serde_json::from_str(params).unwrap();
            let reply = h.shell.run(command, &params);
            eprintln!("{name}: {command}: {reply:?}");
            settle_all(&mut h);
        }
        match then {
            Then::Nothing => {}
            Then::Key(k, m) => key(&mut h, *k, *m),
            Then::Menu(title) => {
                h.open_menu(title);
                h.frames(20);
            }
        }
        h.frames(10);
        h.screenshot(&out.join(format!("{name}.ppm")));
        drop(h);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
