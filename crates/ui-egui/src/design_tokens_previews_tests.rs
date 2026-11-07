//! Visual evidence for the design-token pass (#221): `target/previews/tokens/<surface>.ppm`
//! when `UPDATE_PREVIEWS` is set, one per Tab kind, pane, window, menu and popover, each in a
//! fresh window. The PR's kittest before and after images are this test run on master and on the
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
    Run(fn(&mut Harness, &Path)),
}

/// The first Resource Tab: the Test Bible.
fn bible_tab(h: &Harness) -> obs_engine::TabId {
    h.shell
        .st
        .session
        .workspace
        .tabs()
        .find(|t| matches!(t.kind, TabKind::Resource(_)))
        .map(|t| t.id)
        .expect("a Resource Tab")
}

/// The Test Bible's Contents pane beside its text.
fn contents(h: &mut Harness, _: &Path) {
    let tab = bible_tab(h);
    h.shell
        .run("tab.contents", &json!({ "tab": tab.0, "on": true }));
    settle_all(h);
}

/// An Import report: a USFM folder with markup the importer drops.
fn import_report(h: &mut Harness, dir: &Path) {
    let source = dir.join("usfm").join("WEB");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(
        source.join("GEN.usfm"),
        "\\id GEN\n\\c 1\n\\p\n\\v 1 In the beginning God created the heavens and the earth.\n\\ip An introduction the report lists as dropped.\n",
    )
    .unwrap();
    let path = source.to_string_lossy().to_string();
    // Into this window's own folder, not the user's library.
    h.shell
        .st
        .session
        .library
        .set_directory(dir.join("library"));
    h.shell.run("import.start", &json!({ "path": path }));
    // An inline job runs when pumped: the Import, then its Search index.
    for _ in 0..4 {
        h.shell.st.session.jobs().pump(1);
        settle_all(h);
    }
}

/// A word of the Test Bible's text on screen.
fn word(h: &Harness) -> egui::Pos2 {
    super::word_info::word_spot(h, bible_tab(h), "witness")
}

/// The right-click menu on a word.
fn right_click(h: &mut Harness, _: &Path) {
    let p = word(h);
    h.events.push(Event::PointerMoved(p));
    h.frame();
    for pressed in [true, false] {
        h.events.push(Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.frame();
    }
}

/// The selection toolbar over a double-clicked word.
fn selection_toolbar(h: &mut Harness, _: &Path) {
    let p = word(h);
    super::annotation::double_click(h, p);
}

/// The popup Note editor on a double-clicked word.
fn note_popup(h: &mut Harness, dir: &Path) {
    selection_toolbar(h, dir);
    key(h, egui::Key::N, Modifiers::COMMAND | Modifiers::SHIFT);
    h.events
        .push(Event::Text("Laban's heap: a witness.".into()));
    h.frames(2);
}

/// The Notes Tool with a Note card.
fn notes_with_a_card(h: &mut Harness, _: &Path) {
    h.shell.run(
        "annotation.note",
        &json!({ "passage": "Gen 31:48", "body": "**Galeed**: the heap of witness.\n\n- Laban\n- Jacob" }),
    );
    h.shell.run("tool.notes", &json!({}));
    settle_all(h);
}

/// The Tab switcher, Ctrl held after Ctrl+Tab.
fn tab_switcher(h: &mut Harness, _: &Path) {
    for id in ["tool.search", "tool.history"] {
        h.shell.run(id, &json!({}));
    }
    settle_all(h);
    let ctrl = Modifiers {
        ctrl: true,
        command: true,
        ..Modifiers::NONE
    };
    h.modifiers = ctrl;
    for pressed in [true, false] {
        h.events.push(Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: ctrl,
        });
    }
    h.frames(3);
}

/// The New Collection… window over the Library Tab.
fn new_collection(h: &mut Harness, _: &Path) {
    h.shell.run("library.open", &json!({}));
    settle_all(h);
    h.shell.st.collection_editor = Some(Default::default());
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
    (
        "home",
        &[
            ("reading_plan.create", r#"{"template":"nt-90-days"}"#),
            ("home.open", "{}"),
        ],
        Then::Nothing,
    ),
    ("import-report", &[], Then::Run(import_report)),
    ("word-info", &[("tool.word_info", "{}")], Then::Nothing),
    ("contents", &[], Then::Run(contents)),
    ("library-pane", &[], Then::Key(egui::Key::L, Modifiers::ALT)),
    (
        "layouts-pane",
        &[("layout.save", r#"{"name":"Study desk"}"#)],
        Then::Key(egui::Key::Y, Modifiers::ALT),
    ),
    ("help-menu", &[], Then::Menu("Help")),
    (
        "customize-toolbar",
        &[("view.toolbar_customize", "{}")],
        Then::Nothing,
    ),
    ("right-click-menu", &[], Then::Run(right_click)),
    ("selection-toolbar", &[], Then::Run(selection_toolbar)),
    ("note-popup", &[], Then::Run(note_popup)),
    ("notes-with-a-card", &[], Then::Run(notes_with_a_card)),
    ("tab-switcher", &[], Then::Run(tab_switcher)),
    ("new-collection", &[], Then::Run(new_collection)),
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
            Then::Run(f) => f(&mut h, &dir),
        }
        h.frames(10);
        h.screenshot(&out.join(format!("{name}.ppm")));
        drop(h);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
