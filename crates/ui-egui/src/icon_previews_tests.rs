//! Visual evidence for the icon set (#220): `target/previews/icons/*.ppm` when
//! `UPDATE_PREVIEWS` is set. The PR's before and after screenshots are this test run on
//! master and on the branch: the toolbar and a Tab header, the Tools and Guides panes, the
//! View menu, the Notes Tool, and the dark theme over a sepia page.

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

#[test]
fn icon_previews() {
    if std::env::var_os("UPDATE_PREVIEWS").is_none() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("obs-icon-previews-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews/icons");
    let mut h = Harness::launch_full(Some(&dir.join("user.db")), JobMode::Inline, Some(&dir));
    settle_all(&mut h);
    h.frames(5);
    h.screenshot(&out.join("workspace.ppm"));
    key(&mut h, egui::Key::T, Modifiers::ALT);
    h.screenshot(&out.join("tools-pane.ppm"));
    key(&mut h, egui::Key::Escape, Modifiers::NONE);
    key(&mut h, egui::Key::G, Modifiers::ALT);
    h.screenshot(&out.join("guides-pane.ppm"));
    key(&mut h, egui::Key::Escape, Modifiers::NONE);
    h.open_menu("View");
    h.frames(20);
    h.screenshot(&out.join("view-menu.ppm"));
    key(&mut h, egui::Key::Escape, Modifiers::NONE);
    h.shell.run("tool.notes", &json!({}));
    settle_all(&mut h);
    h.frames(5);
    h.screenshot(&out.join("notes.ppm"));
    h.shell.run("view.theme", &json!({ "value": "dark" }));
    h.shell.run("view.page_tone", &json!({ "value": "sepia" }));
    h.frames(5);
    h.screenshot(&out.join("dark-sepia.ppm"));
    drop(h);
    let _ = std::fs::remove_dir_all(&dir);
}
