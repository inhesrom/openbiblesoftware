//! The toolbar's preferences (#141): Toolbar position, Auto-hide and the Launchers shown, kept
//! in the User database across launches, never in a Layout; hidden Launchers still reachable.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::toolbar_settings::TOOLBAR_PREF;
use crate::*;

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "obs-toolbar-settings-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn db(&self) -> PathBuf {
        self.0.join("user.db")
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn launch(db: &Path) -> Session {
    let mut s = Session::new(JobMode::Inline);
    let (store, _) = obs_store::UserStore::open(db).unwrap();
    s.attach_user(Box::new(store));
    s
}

#[test]
fn every_position_survives_a_restart() {
    let t = Temp::new("positions");
    assert_eq!(launch(&t.db()).toolbar.position, ToolbarPosition::Top);
    for (position, id, _) in ToolbarPosition::ALL {
        let mut s = launch(&t.db());
        s.run("view.toolbar_position", &json!({ "value": id }))
            .unwrap();
        assert_eq!(s.toolbar.position, position);
        drop(s);
        assert_eq!(launch(&t.db()).toolbar.position, position, "{id}");
    }
    let mut s = launch(&t.db());
    let err = s
        .run("view.toolbar_position", &json!({ "value": "middle" }))
        .unwrap_err();
    assert!(err.to_string().contains("middle"), "{err}");
}

#[test]
fn customization_and_auto_hide_survive_a_restart() {
    let t = Temp::new("customize");
    let mut s = launch(&t.db());
    s.run(
        "toolbar.show_launcher",
        &json!({ "launcher": "guides", "shown": false }),
    )
    .unwrap();
    s.run(
        "toolbar.move_launcher",
        &json!({ "launcher": "layouts", "to": 0 }),
    )
    .unwrap();
    s.run("toolbar.labels", &json!({ "value": "icons" }))
        .unwrap();
    s.run("view.toolbar_auto_hide", &json!({})).unwrap();
    drop(s);

    let mut s = launch(&t.db());
    let tb = &s.toolbar;
    assert_eq!(
        tb.launchers(),
        [
            (Launcher::Layouts, true),
            (Launcher::Library, true),
            (Launcher::Guides, false),
            (Launcher::Tools, true),
        ]
    );
    assert_eq!(
        tb.shown(),
        [Launcher::Layouts, Launcher::Library, Launcher::Tools]
    );
    assert_eq!(tb.labels, LauncherLabels::IconsOnly);
    assert!(tb.auto_hide);
    assert!(tb.shows_icons() && !tb.shows_labels());

    s.run("toolbar.reset", &json!({})).unwrap();
    drop(s);
    assert_eq!(launch(&t.db()).toolbar, ToolbarSettings::default());
}

#[test]
fn vertical_positions_show_icons_only() {
    let mut tb = ToolbarSettings::default();
    tb.labels = LauncherLabels::LabelsOnly;
    assert!(!tb.shows_icons() && tb.shows_labels());
    for position in [ToolbarPosition::Left, ToolbarPosition::Right] {
        tb.position = position;
        assert!(tb.shows_icons() && !tb.shows_labels(), "{position:?}");
    }
    tb.position = ToolbarPosition::Bottom;
    tb.labels = LauncherLabels::IconsAndLabels;
    assert!(tb.shows_icons() && tb.shows_labels());
}

#[test]
fn the_toolbar_is_a_preference_not_part_of_a_layout() {
    let t = Temp::new("layout");
    let mut s = launch(&t.db());
    s.run("view.toolbar_position", &json!({ "value": "left" }))
        .unwrap();
    s.run("layout.save", &json!({ "name": "Desk" })).unwrap();
    let snapshot = s.workspace.snapshot().to_json();
    assert!(!snapshot.contains("toolbar"), "{snapshot}");
    s.run("view.toolbar_position", &json!({ "value": "bottom" }))
        .unwrap();
    s.run("layout.restore", &json!({ "name": "Desk" })).unwrap();
    assert_eq!(s.toolbar.position, ToolbarPosition::Bottom);
    let saved = s.user.as_ref().unwrap().preference(TOOLBAR_PREF).unwrap();
    let saved: Value = serde_json::from_str(&saved.unwrap()).unwrap();
    assert_eq!(saved["position"], "bottom");
}

#[test]
fn a_saved_record_with_unknown_or_missing_launchers_keeps_every_launcher_once() {
    let v = json!({
        "position": "right",
        "labels": "nonsense",
        "launchers": [
            { "id": "tools", "shown": false },
            { "id": "ribbon" },
            { "id": "tools", "shown": true },
            { "id": "library" },
        ],
    });
    let tb = ToolbarSettings::from_json(&v);
    assert_eq!(tb.position, ToolbarPosition::Right);
    assert_eq!(tb.labels, LauncherLabels::IconsAndLabels);
    assert_eq!(
        tb.launchers(),
        [
            (Launcher::Tools, false),
            (Launcher::Library, true),
            (Launcher::Guides, true),
            (Launcher::Layouts, true),
        ]
    );
    assert_eq!(ToolbarSettings::from_json(&tb.to_json()), tb);
}

#[test]
fn hidden_launchers_stay_reachable_through_the_command_box_and_the_command_list() {
    let mut s = Session::new(JobMode::Inline);
    for which in Launcher::ALL {
        s.run(
            "toolbar.show_launcher",
            &json!({ "launcher": which.id(), "shown": false }),
        )
        .unwrap();
    }
    assert!(s.toolbar.shown().is_empty());
    // The command list (Help → List commands, `obs-cli run app.commands`).
    let list = s.run("app.commands", &json!({})).unwrap();
    let listed: Vec<&str> = (list.as_array().unwrap().iter())
        .filter_map(|c| c["id"].as_str())
        .collect();
    for which in Launcher::ALL {
        assert!(listed.contains(&which.command()), "{}", which.command());
        assert!(s.is_enabled(which.command()));
    }
    // The Command box: its title finds its Launcher, which runs and drops it down.
    for (typed, which) in [
        ("library launcher", Launcher::Library),
        ("guides", Launcher::Guides),
        ("tools launcher", Launcher::Tools),
        ("layouts", Launcher::Layouts),
    ] {
        let found = s
            .command_box_suggestions(typed)
            .into_iter()
            .find(|g| g.action == CommandBoxAction::Command(which.command()))
            .unwrap_or_else(|| panic!("{typed:?} does not suggest {}", which.command()));
        s.workspace.take_effects();
        s.run_command_box_action(&found.action, false).unwrap();
        let effects = s.workspace.take_effects();
        assert!(
            effects.contains(&Effect::OpenLauncher(which)),
            "{typed:?}: {effects:?}"
        );
    }
}

#[test]
fn the_view_menu_offers_position_auto_hide_and_customize() {
    let view = MENUS.iter().find(|m| m.title == "View").unwrap();
    assert!(view.items.contains(&MenuItem::Choice {
        title: "Toolbar position",
        command: "view.toolbar_position",
    }));
    assert!(
        view.items
            .contains(&MenuItem::Command("view.toolbar_auto_hide"))
    );
    assert!(
        view.items
            .contains(&MenuItem::Command("view.toolbar_customize"))
    );
    let mut s = Session::new(JobMode::Inline);
    let (options, current) = choices(&s, "view.toolbar_position");
    assert_eq!(
        options,
        [
            ("top", "Top"),
            ("bottom", "Bottom"),
            ("left", "Left"),
            ("right", "Right")
        ]
    );
    assert_eq!(current, "top");
    s.run("view.toolbar_customize", &json!({})).unwrap();
    assert_eq!(s.workspace.take_effects(), [Effect::CustomizeToolbar]);
}
