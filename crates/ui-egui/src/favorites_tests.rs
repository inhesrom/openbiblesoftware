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

#[test]
fn opening_the_first_bible_is_in_history_after_a_restart_without_a_jump() {
    let dir = folder("first-open");
    write_bsb(&dir);
    let mut h = launch(&dir);
    // A Workspace with nothing open, so BSB has no Link set or other Bible to start beside.
    let open: Vec<TabId> = h.shell.st.session.workspace.tabs().map(|t| t.id).collect();
    for t in open {
        h.shell.run("tab.close", &json!({ "tab": t.0 }));
    }
    settle_all(&mut h);
    h.shell.run("history.clear", &json!({}));
    // BSB gets its location only once its text is laid out, a frame after it opens.
    let bsb = open_bsb(&mut h);
    assert_eq!(location(&mut h, bsb), Some(v(Book::Genesis, 1, 1)));
    h.shell.save_session(true);
    drop(h);

    let h = launch(&dir);
    let labels: Vec<&str> = (h.shell.st.session.history().iter())
        .map(|v| v.label.as_str())
        .collect();
    // The oldest Visit (the relaunch's own Tabs are newer).
    assert_eq!(labels.last(), Some(&"Genesis 1:1 · BSB"), "{labels:?}");
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A Favorite or History Visit opening a new Resource Tab is a jump: the Link set the Tab
/// joins follows it, translated into each member's numbering. HEB numbers Ps 51 the Hebrew
/// way (51:5 is Standard 51:3); BSB numbers it like the Standard.
#[test]
fn opening_a_favorite_or_visit_in_a_new_tab_moves_its_link_set() {
    let dir = folder("link-set");
    let long = "Have mercy upon me, O God, according to thy lovingkindness: according unto the \
                multitude of thy tender mercies blot out my transgressions.";
    let ps = |n| v(Book::Psalms, 51, n);
    let mut bsb_text: Vec<(Verse, String)> = (1..=31)
        .map(|n| (v(Book::Genesis, 1, n), format!("Genesis 1:{n}. {long}")))
        .collect();
    bsb_text.extend((1..=19).map(|n| (ps(n), format!("Psalm 51:{n}. {long}"))));
    let heb_text: Vec<(Verse, String)> = (1..=21)
        .map(|n| (ps(n), format!("Psalm 51:{n} (Hebrew). {long}")))
        .collect();
    let heb_standard: Vec<Option<Verse>> = (1..=21).map(|n| Some(ps(n.max(2) - 2))).collect();
    let bsb_verses: Vec<(Verse, &str)> = bsb_text.iter().map(|(v, t)| (*v, t.as_str())).collect();
    let heb_verses: Vec<(Verse, &str)> = heb_text.iter().map(|(v, t)| (*v, t.as_str())).collect();
    obs_engine::testing::bibles::write_library(
        &dir,
        &[
            bible("bsb", "BSB", &bsb_verses, None, &[]),
            bible("heb", "HEB", &heb_verses, Some(&heb_standard), &[]),
        ],
    );
    let mut h = Harness::launch_full(Some(&dir.join("user.db")), JobMode::Inline, Some(&dir));
    for id in ["bsb", "heb"] {
        let lib = &mut h.shell.st.session.library;
        lib.add_file(&dir.join(format!("{id}.obr"))).unwrap();
    }
    let open: Vec<TabId> = h.shell.st.session.workspace.tabs().map(|t| t.id).collect();
    for t in open {
        h.shell.run("tab.close", &json!({ "tab": t.0 }));
    }
    settle_all(&mut h);
    let open_resource = |h: &mut Harness, id: &str| {
        let out = h
            .shell
            .run("resource.open", &json!({ "id": id, "split": true }))
            .unwrap();
        settle_all(h);
        TabId(out["tab"].as_u64().unwrap())
    };

    // Star HEB at its Ps 51:5, then close it.
    let heb = open_resource(&mut h, "heb");
    go(&mut h, heb, "Ps 51:5");
    let favorite = h
        .shell
        .run("favorite.add", &json!({ "tab": heb.0 }))
        .unwrap()["id"]
        .clone();
    h.shell.run("tab.close", &json!({ "tab": heb.0 }));
    settle_all(&mut h);

    // BSB at Gen 1:1, in Link set A; the Favorite opens HEB from it, so HEB joins A.
    let bsb = open_resource(&mut h, "bsb");
    h.shell
        .run("tab.link", &json!({ "tab": bsb.0, "set": "A" }));
    go(&mut h, bsb, "Gen 1:1");
    assert_eq!(location(&mut h, bsb), Some(v(Book::Genesis, 1, 1)));
    let out = h
        .shell
        .run("favorite.open", &json!({ "id": favorite }))
        .unwrap();
    settle_all(&mut h);
    let heb = TabId(out["tab"].as_u64().unwrap());
    let ws = &h.shell.st.session.workspace;
    assert_eq!(ws.tab(heb).unwrap().link, Some('A'));
    assert_eq!(location(&mut h, heb), Some(ps(5)));
    assert_eq!(
        location(&mut h, bsb),
        Some(ps(3)),
        "BSB follows, in its numbering"
    );
    assert_eq!(h.shell.st.location_text(bsb), "Psalms 51:3");

    // The same from History: BSB back at Gen 1:1, HEB closed, its Visit opened from BSB.
    go(&mut h, bsb, "Gen 1:1");
    h.shell.run("tab.close", &json!({ "tab": heb.0 }));
    h.shell.st.session.workspace.user_focused(bsb);
    settle_all(&mut h);
    assert_eq!(location(&mut h, bsb), Some(v(Book::Genesis, 1, 1)));
    let visit = (h.shell.st.session.history().iter())
        .find(|v| v.label.ends_with("· HEB"))
        .expect("HEB's Visit")
        .id;
    let out = h
        .shell
        .run("history.open", &json!({ "id": visit }))
        .unwrap();
    settle_all(&mut h);
    let heb = TabId(out["tab"].as_u64().unwrap());
    assert_eq!(location(&mut h, heb), Some(ps(5)));
    assert_eq!(location(&mut h, bsb), Some(ps(3)));
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}
