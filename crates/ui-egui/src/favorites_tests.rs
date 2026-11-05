//! Favorites and History (#155) through the headless harness: a real `.obr` BSB fixture and a
//! real User database in a temp folder, the ☆ in a Tab header clicked, the Favorites and
//! History Tools opened from the Tools menu, their rows clicked, and the app quit and
//! relaunched in between.

use std::path::{Path, PathBuf};

use obs_engine::TabId;
use obs_engine::testing::bibles::{bible, v};
use obs_scripture::{Book, Verse};

use super::*;

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("obs-favorites-ui-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A BSB fixture of Gen 1:1-31, Jn 3:1-36 and Rom 8:1-39, so each place is far from the others.
fn write_bsb(dir: &Path) {
    let mut text: Vec<(Verse, String)> = Vec::new();
    for (book, chapter, count) in [
        (Book::Genesis, 1, 31),
        (Book::John, 3, 36),
        (Book::Romans, 8, 39),
    ] {
        for n in 1..=count {
            text.push((
                v(book, chapter, n),
                format!(
                    "{book:?} {chapter}:{n}. And we know that God works all things together for \
                     the good of those who love Him, who are called according to His purpose."
                ),
            ));
        }
    }
    let verses: Vec<(Verse, &str)> = text.iter().map(|(v, t)| (*v, t.as_str())).collect();
    obs_engine::testing::bibles::write_library(dir, &[bible("bsb", "BSB", &verses, None, &[])]);
}

/// Launch on `dir`'s User database (restoring the Last session) with BSB in the library.
fn launch(dir: &Path) -> Harness {
    let mut h = Harness::launch_full(Some(&dir.join("user.db")), JobMode::Inline, Some(dir));
    h.shell
        .st
        .session
        .library
        .add_file(&dir.join("bsb.obr"))
        .unwrap();
    settle_all(&mut h);
    h
}

fn open_bsb(h: &mut Harness) -> TabId {
    let out = h
        .shell
        .run("resource.open", &json!({ "id": "bsb", "split": true }))
        .unwrap();
    settle_all(h);
    TabId(out["tab"].as_u64().unwrap())
}

fn go(h: &mut Harness, tab: TabId, text: &str) {
    h.shell
        .run("tab.go", &json!({ "tab": tab.0, "text": text }));
    settle_all(h);
}

fn bsb_tabs(h: &Harness) -> Vec<TabId> {
    h.shell
        .st
        .session
        .workspace
        .tabs()
        .filter(|t| t.kind == TabKind::Resource("bsb".into()))
        .map(|t| t.id)
        .collect()
}

/// Visual evidence for `docs/previews/<name>.png` when `UPDATE_PREVIEWS` is set.
fn preview(h: &Harness, name: &str) {
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
        std::fs::create_dir_all(&dir).unwrap();
        h.screenshot(&dir.join(format!("{name}.ppm")));
    }
}

/// Open a Tool from the Tools menu (its menu item is the topmost-painted text of that name).
fn open_tool(h: &mut Harness, title: &str) {
    h.open_menu("Tools");
    h.click_text(title);
    settle_all(h);
}

#[test]
fn starring_rom_8_28_in_bsb_then_opening_it_from_favorites_after_a_restart_lands_there() {
    let dir = folder("restart");
    write_bsb(&dir);
    let mut h = launch(&dir);
    let bsb = open_bsb(&mut h);
    go(&mut h, bsb, "Rom 8:28");
    assert_eq!(location(&mut h, bsb), Some(v(Book::Romans, 8, 28)));
    // The ☆ in BSB's header stars it.
    let star = h.shell.st.tabs[&bsb].favorite_star.expect("BSB has a ☆");
    h.click(star.center());
    assert!(h.shell.st.session.favorite_of(bsb).is_some());
    // The ⋯ menu's "Add to Favorites folder" puts another into a new folder.
    go(&mut h, bsb, "Jn 3:16");
    h.shell
        .run("favorite.add", &json!({ "tab": bsb.0, "folder": "Sermon" }));
    let favorites: Vec<String> = (h.shell.st.session.favorites().iter())
        .map(|f| format!("{}/{}", f.folder, f.label))
        .collect();
    assert_eq!(favorites, ["/Romans 8:28 · BSB", "Sermon/John 3:16 · BSB"]);
    // Close BSB and quit.
    h.shell.run("tab.close", &json!({ "tab": bsb.0 }));
    h.frames(2);
    h.shell.save_session(true);
    drop(h);

    let mut h = launch(&dir);
    assert!(bsb_tabs(&h).is_empty());
    open_tool(&mut h, "Favorites");
    h.click_text("Romans 8:28 · BSB");
    settle_all(&mut h);
    let opened = bsb_tabs(&h);
    assert_eq!(opened.len(), 1);
    let bsb = opened[0];
    assert_eq!(h.shell.st.session.workspace.focused(), Some(bsb));
    // The reading pane shows Rom 8:28, and its header box says so.
    assert_eq!(location(&mut h, bsb), Some(v(Book::Romans, 8, 28)));
    assert_eq!(h.shell.st.location_text(bsb), "Romans 8:28");
    assert!(h.find("SERMON").is_some(), "{:?}", h.texts());
    preview(&h, "favorites-opened-after-restart");
    // Its ☆ is now a ★: clicking it removes the Favorite.
    let star = h.shell.st.tabs[&bsb].favorite_star.unwrap();
    h.click(star.center());
    assert_eq!(h.shell.st.session.favorites().len(), 1);
    assert!(h.find("Romans 8:28 · BSB").is_none());
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_history_tool_shows_the_last_sessions_locations_in_order() {
    let dir = folder("history");
    write_bsb(&dir);
    let mut h = launch(&dir);
    let bsb = open_bsb(&mut h);
    for place in ["Gen 1:1", "Jn 3:16", "Rom 8:28"] {
        go(&mut h, bsb, place);
    }
    h.shell.save_session(true);
    drop(h);

    let mut h = launch(&dir);
    open_tool(&mut h, "History");
    assert!(h.find("Today").is_some(), "{:?}", h.texts());
    let y = |h: &Harness, label: &str| h.at(label).y;
    let (rom, john, gen1) = (
        y(&h, "Romans 8:28 · BSB"),
        y(&h, "John 3:16 · BSB"),
        y(&h, "Genesis 1:1 · BSB"),
    );
    assert!(h.at("Today").y < rom, "the day heading comes first");
    assert!(
        rom < john && john < gen1,
        "newest first: {rom} {john} {gen1}"
    );
    preview(&h, "history-tool");
    // The header's filter searches History.
    let history = h.tab_id("History");
    h.shell.st.tabs.get_mut(&history).unwrap().filter = "john".into();
    h.frames(2);
    assert!(h.find("John 3:16 · BSB").is_some());
    assert!(h.find("Romans 8:28 · BSB").is_none());
    h.shell.st.tabs.get_mut(&history).unwrap().filter.clear();
    h.frames(2);

    // A row goes back there.
    h.click_text("John 3:16 · BSB");
    settle_all(&mut h);
    let bsb = bsb_tabs(&h)[0];
    assert_eq!(location(&mut h, bsb), Some(v(Book::John, 3, 16)));

    // Clear History… asks first; then History is empty, after a restart too.
    h.shell.run("tool.history", &json!({}));
    h.frames(2);
    h.click_text("Clear History…");
    h.click_text("Clear");
    assert!(h.shell.st.session.history().is_empty());
    assert!(
        h.texts().iter().any(|t| t.starts_with("History is empty")),
        "{:?}",
        h.texts()
    );
    drop(h);
    let h = launch(&dir);
    assert!(h.shell.st.session.history().is_empty());
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}
