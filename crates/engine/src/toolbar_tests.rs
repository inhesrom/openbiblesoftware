//! The toolbar Launchers' engine side (#140): the Library drop-down's rows and search,
//! `toolbar.open_resource`, Layout cards with thumbnails and the Built-in Layouts.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use obs_model::{Manifest, ResourceType};
use obs_scripture::{Book, Verse};
use serde_json::json;

use crate::toolbar::{LayoutThumbnail, LibraryLauncherSection};
use crate::*;

fn entry(id: &str, abbrev: &str, title: &str, ty: ResourceType, lang: &str) -> LibraryEntry {
    LibraryEntry {
        manifest: Manifest::new(id, title, abbrev, ty, lang),
        path: format!("/library/{id}.obr").into(),
    }
}

fn session() -> Session {
    let mut s = Session::new(JobMode::Inline);
    let all = [
        (
            "kjv",
            "KJV",
            "King James Version",
            ResourceType::Bible,
            "en",
        ),
        (
            "web",
            "WEB",
            "World English Bible",
            ResourceType::Bible,
            "en",
        ),
        (
            "bsb",
            "BSB",
            "Berean Standard Bible",
            ResourceType::Bible,
            "en",
        ),
        ("lxx", "LXX", "Septuagint", ResourceType::Bible, "grc"),
        (
            "mhc",
            "MHC",
            "Matthew Henry Concise",
            ResourceType::Commentary,
            "en",
        ),
        (
            "abbott",
            "Abbott",
            "Abbott-Smith Lexicon",
            ResourceType::Lexicon,
            "en",
        ),
        (
            "easton",
            "Easton",
            "Easton's Bible Dictionary",
            ResourceType::Dictionary,
            "en",
        ),
    ];
    for (id, abbrev, title, ty, lang) in all {
        s.library.add(entry(id, abbrev, title, ty, lang));
    }
    s
}

fn temp_db(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("obs-toolbar-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("user.db")
}

fn with_user(name: &str) -> Session {
    let mut s = session();
    let (store, _) = obs_store::UserStore::open(&temp_db(name)).unwrap();
    s.attach_user(Box::new(store));
    s
}

fn tab(v: &serde_json::Value) -> TabId {
    TabId(v["tab"].as_u64().unwrap())
}

fn ids(rows: &[LibraryLauncherRow]) -> Vec<&str> {
    rows.iter().map(|r| r.resource_id.as_str()).collect()
}

fn section<'a>(
    list: &'a [(LibraryLauncherSection, Vec<LibraryLauncherRow>)],
    which: &LibraryLauncherSection,
) -> Option<&'a [LibraryLauncherRow]> {
    list.iter()
        .find(|(s, _)| s == which)
        .map(|(_, r)| r.as_slice())
}

#[test]
fn with_nothing_typed_the_library_launcher_lists_recent_and_the_top_of_each_type() {
    let mut s = session();
    let list = s.library_launcher("");
    assert_eq!(
        section(&list, &LibraryLauncherSection::Recent),
        None,
        "nothing opened yet"
    );
    let bibles = section(
        &list,
        &LibraryLauncherSection::Preferred(ResourceType::Bible),
    )
    .unwrap();
    assert_eq!(
        ids(bibles),
        ["kjv", "web", "bsb"],
        "the top three in Preferred order"
    );
    let headings: Vec<&str> = list.iter().map(|(s, _)| s.heading()).collect();
    assert_eq!(
        headings,
        ["Bibles", "Commentaries", "Lexicons", "Dictionaries"]
    );

    s.run("resource.open", &json!({ "id": "mhc" })).unwrap();
    s.run("resource.open", &json!({ "id": "lxx" })).unwrap();
    let list = s.library_launcher("");
    assert_eq!(list[0].0, LibraryLauncherSection::Recent);
    assert_eq!(
        ids(&list[0].1),
        ["lxx", "mhc"],
        "most recently active first"
    );
    let lxx = &list[0].1[0];
    assert_eq!(
        (lxx.abbreviation.as_str(), lxx.language.as_str()),
        ("LXX", "Greek")
    );
}

#[test]
fn the_library_launcher_searches_title_abbreviation_type_and_language_best_first() {
    let s = session();
    let found = |q: &str| -> Vec<String> {
        s.library_launcher(q)
            .into_iter()
            .flat_map(|(sec, rows)| {
                assert_eq!(sec, LibraryLauncherSection::Matches);
                rows
            })
            .map(|r| r.resource_id)
            .collect()
    };
    // An exact abbreviation first, then what it starts, then the rest.
    assert_eq!(found("web")[0], "web");
    assert_eq!(found("kjv"), ["kjv"]);
    assert_eq!(found("king james"), ["kjv"]);
    // A type and a language.
    assert_eq!(found("commentary"), ["mhc"]);
    assert_eq!(found("lexicons"), ["abbott"]);
    assert_eq!(found("greek"), ["lxx"]);
    assert_eq!(found("grc"), ["lxx"]);
    // Words together: every word must match.
    assert_eq!(found("english dictionary"), ["easton"]);
    assert_eq!(found("bible greek"), ["lxx"]);
    // Titles contain: "Bible" in a Dictionary's title still finds it, after the Bibles.
    let bible = found("bible");
    assert!(bible.contains(&"easton".to_string()), "{bible:?}");
    assert!(found("zzz").is_empty());
}

#[test]
fn open_resource_uses_the_focused_panel_or_splits_off_a_new_tab() {
    let mut s = session();
    let kjv = tab(&s.run("resource.open", &json!({ "id": "kjv" })).unwrap());
    let panel = s.workspace.panel_of(kjv).unwrap();
    let notes = tab(&s.run("tool.notes", &json!({ "split": true })).unwrap());
    let notes_panel = s.workspace.panel_of(notes).unwrap();
    assert_ne!(panel, notes_panel);

    // Click: in the focused Panel (here the Notes one), not where Bibles were last.
    let web = tab(&s
        .run("toolbar.open_resource", &json!({ "id": "web" }))
        .unwrap());
    assert_eq!(s.workspace.panel_of(web), Some(notes_panel));
    assert_eq!(s.workspace.focused(), Some(web));
    // Again: the Tab already there comes to the front.
    s.workspace.focus(notes);
    let again = tab(&s
        .run("toolbar.open_resource", &json!({ "id": "web" }))
        .unwrap());
    assert_eq!(again, web);
    assert_eq!(s.workspace.focused(), Some(web));

    // Ctrl/Cmd-click: always a new Tab, in a new Panel beside the focused one.
    let second = tab(&s
        .run(
            "toolbar.open_resource",
            &json!({ "id": "web", "new_tab": true }),
        )
        .unwrap());
    assert_ne!(second, web);
    let new_panel = s.workspace.panel_of(second).unwrap();
    assert!(![panel, notes_panel].contains(&new_panel));

    // Dropped on a Panel: a Tab there; on its edge: a new Panel beside it.
    let dropped = tab(&s
        .run(
            "toolbar.open_resource",
            &json!({ "id": "mhc", "panel": panel.0 }),
        )
        .unwrap());
    assert_eq!(s.workspace.panel_of(dropped), Some(panel));
    let beside = tab(&s
        .run(
            "toolbar.open_resource",
            &json!({ "id": "abbott", "beside": panel.0 }),
        )
        .unwrap());
    let placed = s
        .workspace
        .take_effects()
        .into_iter()
        .rev()
        .find_map(|e| match e {
            Effect::Opened { tab, placement } if tab == beside => Some(placement),
            _ => None,
        });
    assert!(
        matches!(placed, Some(Placement::NewPanel { beside: Some(p), .. }) if p == panel),
        "{placed:?}"
    );
    assert!(
        s.run("toolbar.open_resource", &json!({ "id": "nope" }))
            .is_err()
    );
}

#[test]
fn each_launcher_has_a_shortcut_command() {
    let mut s = session();
    for (launcher, key) in Launcher::ALL
        .into_iter()
        .zip(["Alt+L", "Alt+G", "Alt+T", "Alt+Y"])
    {
        let spec = s.commands().get(launcher.command()).unwrap();
        assert_eq!(spec.shortcut, Some(key));
        assert_eq!(launcher.menu().title, launcher.title());
        s.workspace.take_effects();
        s.run(launcher.command(), &json!({})).unwrap();
        assert_eq!(s.workspace.take_effects(), [Effect::OpenLauncher(launcher)]);
    }
}

#[test]
fn layout_cards_show_saved_layouts_and_built_ins_with_thumbnails_from_the_arrangement() {
    let mut s = with_user("cards");
    let kjv = tab(&s.run("resource.open", &json!({ "id": "kjv" })).unwrap());
    s.run("tab.link", &json!({ "tab": kjv.0, "set": "A" }))
        .unwrap();
    let web = tab(&s
        .run("resource.open", &json!({ "id": "web", "split": true }))
        .unwrap());
    let (pk, pw) = (
        s.workspace.panel_of(kjv).unwrap(),
        s.workspace.panel_of(web).unwrap(),
    );
    // The UI records the arrangement it shows.
    s.workspace.arrangement = Some(LayoutNode::Row {
        children: vec![
            (
                LayoutNode::Panel {
                    panel: pk.0,
                    tabs: vec![kjv.0],
                    active: Some(kjv.0),
                },
                2.0,
            ),
            (
                LayoutNode::Panel {
                    panel: pw.0,
                    tabs: vec![web.0],
                    active: Some(web.0),
                },
                1.0,
            ),
        ],
    });
    s.run("layout.save", &json!({ "name": "Two Bibles" }))
        .unwrap();

    let cards = s.layout_cards();
    let names: Vec<&str> = cards.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Two Bibles",
            "Bible + Commentary",
            "Word study desk",
            "Parallel Bibles",
            "Reading only"
        ]
    );
    let saved = &cards[0];
    assert!(!saved.builtin);
    let Some(LayoutThumbnail::Row(children)) = &saved.thumbnail else {
        panic!("{:?}", saved.thumbnail);
    };
    assert_eq!(children.len(), 2);
    assert_eq!(children[0].1, 2.0, "shares come from the stored tree");
    assert!(matches!(
        &children[0].0,
        LayoutThumbnail::Panel { title, link: Some('A'), tabs: 1 } if title == "KJV"
    ));
    // Built-ins: from the library's Preferred order.
    let thumb = |name: &str| {
        cards
            .iter()
            .find(|c| c.name == name)
            .and_then(|c| c.thumbnail.clone())
            .unwrap()
    };
    assert_eq!(thumb("Bible + Commentary").titles(), ["KJV", "MHC"]);
    assert_eq!(
        thumb("Word study desk").titles(),
        ["KJV", "Word Study", "Word Info"]
    );
    assert_eq!(thumb("Parallel Bibles").titles(), ["KJV", "WEB"]);
    assert_eq!(thumb("Reading only").titles(), ["KJV"]);
    assert!(cards.iter().all(|c| c.unavailable.is_none()));

    // Without a Commentary, that one says what it needs.
    let mut bare = Session::new(JobMode::Inline);
    bare.library
        .add(entry("kjv", "KJV", "KJV", ResourceType::Bible, "en"));
    let cards = bare.layout_cards();
    let names: Vec<(&str, Option<&str>)> = cards
        .iter()
        .map(|c| (c.name.as_str(), c.unavailable))
        .collect();
    assert_eq!(
        names,
        [
            (
                "Bible + Commentary",
                Some("Needs a Commentary in the library")
            ),
            ("Word study desk", None),
            ("Parallel Bibles", Some("Needs two Bibles in the library")),
            ("Reading only", None),
        ]
    );
    assert!(
        bare.run("layout.builtin", &json!({ "name": "Parallel Bibles" }))
            .is_err()
    );
}

#[test]
fn a_built_in_layout_replaces_the_workspace_and_opens_at_the_current_passage() {
    let mut s = session();
    let kjv = tab(&s.run("resource.open", &json!({ "id": "kjv" })).unwrap());
    let jn = Verse::new(Book::John, 3, 16);
    s.workspace.navigate(kjv, jn);
    s.workspace.take_effects();
    s.run("layout.builtin", &json!({ "name": "Bible + Commentary" }))
        .unwrap();
    let tabs: Vec<(TabKind, Option<char>)> = s
        .workspace
        .tabs()
        .map(|t| (t.kind.clone(), t.link))
        .collect();
    assert_eq!(
        tabs,
        [
            (TabKind::Resource("kjv".into()), Some('A')),
            (TabKind::Resource("mhc".into()), Some('A')),
        ]
    );
    assert!(
        s.workspace.tabs().all(|t| t.id.0 > kjv.0),
        "fresh Tab ids, so History never names the wrong Tab"
    );
    let bible = s
        .workspace
        .tabs()
        .find(|t| t.kind == TabKind::Resource("kjv".into()))
        .unwrap()
        .id;
    assert_eq!(s.workspace.focused(), Some(bible));
    let effects = s.workspace.take_effects();
    assert!(effects.contains(&Effect::Rebuild), "{effects:?}");
    assert!(
        effects.contains(&Effect::NavigateStandard {
            tab: bible,
            verse: jn
        }),
        "{effects:?}"
    );
    // Two Panels side by side, each with its Tab.
    let Some(LayoutNode::Row { children }) = &s.workspace.arrangement else {
        panic!("{:?}", s.workspace.arrangement);
    };
    assert_eq!(children.len(), 2);

    s.run("layout.builtin", &json!({ "name": "Word study desk" }))
        .unwrap();
    let kinds: Vec<TabKind> = s.workspace.tabs().map(|t| t.kind.clone()).collect();
    assert_eq!(
        kinds,
        [
            TabKind::Resource("kjv".into()),
            TabKind::Guide(Guide::WordStudy),
            TabKind::Tool(Tool::WordInfo),
        ]
    );
    assert!(s.run("layout.builtin", &json!({ "name": "Nope" })).is_err());
}
