//! Help → User guide (#218): a page for every main feature, honest menu paths, links and
//! screenshots, search, and the Tab.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use serde_json::json;

use crate::user_guide::{
    GuideBlock, USER_GUIDE, guide_blocks, guide_images, guide_page, page_for_link,
    search_user_guide,
};
use crate::*;

fn repo() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

/// What a page shows, as one lower-case text.
fn shown(page: &GuidePage) -> String {
    let mut out = String::new();
    for block in guide_blocks(page) {
        match block {
            GuideBlock::Text(lines) => {
                for l in lines {
                    out.push_str(&l.line.text);
                    out.push('\n');
                }
            }
            GuideBlock::Image { alt, .. } => out.push_str(&alt),
        }
    }
    out.to_lowercase()
}

/// The label of each command item of `items` (submenus included) and the titles of its
/// Choices and submenus.
fn labels(s: &Session, items: &[MenuItem]) -> Vec<String> {
    let mut out = Vec::new();
    for item in items {
        match item {
            MenuItem::Command(id) => out.push(s.commands().get(id).unwrap().label.to_string()),
            MenuItem::Choice { title, .. } => out.push((*title).to_string()),
            MenuItem::Submenu { title, items } => {
                out.push((*title).to_string());
                out.extend(labels(s, items));
            }
            MenuItem::RecentlyClosed => out.push("Recently closed".into()),
            MenuItem::ReadingLayout => out.push("Reading layout".into()),
            MenuItem::Notebooks => out.push("Notebooks in text".into()),
            MenuItem::Favorites => out.push("Favorites".into()),
            _ => {}
        }
    }
    out
}

/// Acceptance (#218): every main feature has a guide page — the pages the ticket names, each
/// a Markdown file whose first line is its title.
#[test]
fn the_user_guide_has_a_page_for_every_main_feature() {
    for id in [
        "read-and-compare",
        "link-sets",
        "guides",
        "word-study",
        "search",
        "notes-and-highlights",
        "get-resources",
        "layouts",
        "keyboard",
    ] {
        let page = guide_page(id).unwrap_or_else(|| panic!("no page {id}"));
        assert_eq!(
            page.text.lines().next(),
            Some(format!("# {}", page.title).as_str()),
            "{id}.md starts with its title"
        );
        assert!(
            page.text.split_whitespace().count() > 120,
            "{id} is a page, not a stub"
        );
        // The file on disk is the page compiled in.
        let on_disk =
            std::fs::read_to_string(repo().join(format!("docs/user-guide/{id}.md"))).unwrap();
        assert_eq!(on_disk, page.text);
    }
    // No page is left out of the Tab, and none is listed twice.
    let mut files: Vec<String> = std::fs::read_dir(repo().join("docs/user-guide"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter_map(|f| f.strip_suffix(".md").map(str::to_string))
        .collect();
    files.sort();
    let mut ids: Vec<String> = USER_GUIDE.iter().map(|p| p.id.to_string()).collect();
    ids.sort();
    assert_eq!(files, ids);
}

/// Every Guide and Tool the menus and Launchers open is explained on some page, by name.
#[test]
fn every_guide_tool_and_launcher_is_named_in_the_user_guide() {
    let s = Session::new(JobMode::Inline);
    let all: String = USER_GUIDE.iter().map(shown).collect();
    let tools = MENUS.iter().find(|m| m.title == "Tools").unwrap();
    let mut names = labels(&s, tools.items);
    names.extend(Launcher::ALL.iter().map(|l| l.title().to_string()));
    names.extend(
        [
            "Home",
            "Command box",
            "Starter library",
            "Layout",
            "Link set",
        ]
        .map(String::from),
    );
    for name in names {
        assert!(
            all.contains(&name.to_lowercase()),
            "the User guide never names {name}"
        );
    }
}

/// Every "Menu → Item" the guide writes is an item of that menu (or Launcher), so the guide
/// does not send anyone to a command that has moved.
#[test]
fn every_menu_path_in_the_user_guide_names_a_real_item() {
    let s = Session::new(JobMode::Inline);
    let mut checked = 0;
    for page in USER_GUIDE {
        for line in page.text.lines() {
            for (at, _) in line.match_indices(" → ") {
                let before = line[..at].trim_end_matches('*');
                let menu = before
                    .rsplit(|c: char| !c.is_alphanumeric())
                    .next()
                    .unwrap();
                let items: Vec<String> = match menu {
                    "Guides" => labels(&s, Launcher::Guides.items()),
                    m => match MENUS.iter().find(|x| x.title == m) {
                        Some(found) => labels(&s, found.items),
                        // Not a menu path (Psalm 3:1 → …).
                        None => continue,
                    },
                };
                let after = &line[at + " → ".len()..];
                assert!(
                    items.iter().any(|label| after.starts_with(label.as_str())),
                    "{}: “{menu} → {after}” is not an item of {menu}: {items:?}",
                    page.id
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 20, "only {checked} menu paths checked");
}

/// Links between pages name pages that exist.
#[test]
fn every_link_between_user_guide_pages_names_a_page() {
    let mut links = 0;
    for page in USER_GUIDE {
        for block in guide_blocks(page) {
            let GuideBlock::Text(lines) = block else {
                continue;
            };
            for l in lines {
                for m in &l.line.marks {
                    if let obs_model::NoteMarkKind::Link(to) = &m.kind
                        && to.ends_with(".md")
                    {
                        assert!(page_for_link(to).is_some(), "{}: {to}", page.id);
                        links += 1;
                    }
                }
            }
        }
    }
    assert!(links >= 9, "the pages link to each other ({links})");
    assert_eq!(page_for_link("./search.md#scope").unwrap().id, "search");
    assert!(page_for_link("https://example.org/search.md").is_none());
}

/// Screenshots come from the screenshot gallery (`docs/previews/`), and every main page has
/// one.
#[test]
fn every_user_guide_screenshot_is_in_the_screenshot_gallery() {
    let mut pages_with = 0;
    for page in USER_GUIDE {
        let images = guide_images(page);
        pages_with += usize::from(!images.is_empty());
        for file in images {
            assert!(
                repo().join("docs/previews").join(&file).is_file(),
                "{}: docs/previews/{file} is missing",
                page.id
            );
        }
    }
    assert!(pages_with >= 8, "only {pages_with} pages have a screenshot");
}

#[test]
fn a_page_is_its_text_between_its_screenshots() {
    let page = guide_page("getting-started").unwrap();
    let blocks = guide_blocks(page);
    let GuideBlock::Text(first) = &blocks[0] else {
        panic!("{blocks:?}")
    };
    assert_eq!(first[0].line.text, "Getting started");
    assert_eq!(first[0].line.kind, obs_model::NoteLineKind::Heading(1));
    let GuideBlock::Image { file, alt } = &blocks[1] else {
        panic!("{blocks:?}")
    };
    assert_eq!(file, "user-guide-window.png");
    assert!(alt.starts_with("The window"));
    // Bible references in the text are live links, as in a Note.
    let refs: usize = (blocks.iter())
        .filter_map(|b| match b {
            GuideBlock::Text(lines) => {
                Some(lines.iter().map(|l| l.references.len()).sum::<usize>())
            }
            GuideBlock::Image { .. } => None,
        })
        .sum();
    assert!(refs >= 1, "Jn 3:16 is a link");
}

/// The Tab's search box: every word, any case, title matches first, with the lines that
/// hold a word.
#[test]
fn searching_the_user_guide_finds_the_pages_that_hold_every_word() {
    let found = search_user_guide("link SET");
    assert_eq!(found[0].page.id, "link-sets");
    assert!(found.len() > 1, "other pages mention Link sets too");
    assert!(!found[0].lines.is_empty());
    assert!(
        (found[0].lines.iter())
            .all(|l| l.to_lowercase().contains("link") || l.to_lowercase().contains("set"))
    );

    let strongs = search_user_guide("strong's lexicon");
    assert!(strongs.iter().any(|m| m.page.id == "word-study"));
    let morph = search_user_guide("@aorist");
    assert_eq!(morph.len(), 1);
    assert_eq!(morph[0].page.id, "search");
    assert!(search_user_guide("zzyzx").is_empty());
    assert!(search_user_guide("  ").is_empty());
}

/// Help → User guide opens one Tab (F1), at the page asked for; another page shows in the same
/// Tab.
#[test]
fn help_user_guide_opens_one_tab_at_the_page_asked_for() {
    let mut s = Session::new(JobMode::Inline);
    assert_eq!(crate::menu_of("help.user_guide"), Some("Help"));
    assert_eq!(s.shortcut("help.user_guide"), Some("F1"));
    let out = s.run("help.user_guide", &json!({})).unwrap();
    let tab = TabId(out["tab"].as_u64().unwrap());
    let t = s.workspace.tab(tab).unwrap();
    assert_eq!(t.kind, TabKind::UserGuide);
    assert_eq!(t.title, USER_GUIDE_TITLE);
    assert_eq!(t.header_box(), HeaderBox::Filter);
    assert!(!t.kind.has_link_chip());
    assert_eq!(s.user_guide().page().id, "getting-started");

    let again = s
        .run("help.user_guide", &json!({ "page": "search" }))
        .unwrap();
    assert_eq!(again["tab"].as_u64(), Some(tab.0));
    assert_eq!(s.user_guide().page().id, "search");
    s.run("user_guide.page", &json!({ "page": "layouts" }))
        .unwrap();
    assert_eq!(s.user_guide().page().title, "Layouts");
    let err = s
        .run("user_guide.page", &json!({ "page": "nope" }))
        .unwrap_err();
    assert!(err.to_string().contains("keyboard"), "{err}");
    assert_eq!(s.user_guide().page().id, "layouts");
}
