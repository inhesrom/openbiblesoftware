//! Get Resources → Books: the Christian Open Library (#347, ADR 0024) through the shell's
//! real-event headless harness, against a mock of COL's GitHub repository. No network.

use obs_engine::col_get::SessionSecret;
use obs_engine::testing::MockCol;
use obs_model::ResourceType;

use super::*;

const TOKEN: &str = "github_pat_TEST0123456789";

fn setup(name: &str) -> (Harness, Arc<MockCol>, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("obs-shell-col-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("library")).unwrap();
    let col = Arc::new(MockCol::new(TOKEN));
    let mut h = Harness::new();
    let s = &mut h.shell.st.session;
    s.set_fetch(col.clone());
    s.col.set_api(MockCol::API);
    s.col.set_secrets(Arc::new(SessionSecret::default()));
    s.library.set_directory(root.join("library"));
    h.frames(2);
    (h, col, root)
}

fn settle(h: &mut Harness) {
    for _ in 0..20 {
        h.shell.st.session.jobs().pump(64);
        h.frames(2);
    }
}

fn open_books(h: &mut Harness) {
    h.shell
        .run("library.get_resources", &json!({ "type": "books" }))
        .unwrap();
    settle(h);
}

fn type_text(h: &mut Harness, text: &str) {
    h.events.push(Event::Text(text.into()));
    h.frames(2);
}

/// The control labelled `label` on the row whose title is `title`.
fn on_row(h: &Harness, title: &str, label: &str) -> Pos2 {
    let row = h
        .shapes
        .iter()
        .find(|(_, t)| t == title)
        .map(|(r, _)| *r)
        .unwrap_or_else(|| panic!("{title:?} not on screen: {:?}", h.texts()));
    // The row's own control is the one nearest its title (the next row's is a row away).
    h.shapes
        .iter()
        .filter(|(_, t)| t == label)
        .map(|(r, _)| r.center())
        .min_by(|a, b| {
            (a.y - row.center().y)
                .abs()
                .total_cmp(&(b.y - row.center().y).abs())
        })
        .filter(|p| (p.y - row.center().y).abs() < crate::tokens::size::ROW_DOUBLE / 2.0)
        .unwrap_or_else(|| panic!("no {label:?} on {title:?}: {:?}", h.texts()))
}

#[test]
fn books_connects_with_a_token_lists_col_and_one_click_downloads_a_work_into_the_library() {
    let (mut h, col, root) = setup("download");
    open_books(&mut h);
    // Not connected: how to get access, and nothing asked of GitHub yet.
    for t in [
        "Christian Open Library",
        "Ask for access…",
        "Connect",
        "Use a local copy…",
    ] {
        assert!(h.texts().contains(&t), "{t}: {:?}", h.texts());
    }
    // Get Resources reads the SWORD repositories' catalog; COL is asked nothing.
    let asked_col = col
        .seen
        .lock()
        .unwrap()
        .iter()
        .any(|(u, _)| u.starts_with(MockCol::API) || u.contains("Christian-open-library"));
    assert!(!asked_col, "no request to COL before Connect");

    // Paste the token and connect.
    h.click_text("GitHub token");
    type_text(&mut h, TOKEN);
    h.click_text("Connect");
    settle(&mut h);
    assert!(
        h.shell.st.session.col.error().is_none(),
        "{:?}",
        h.shell.st.session.col.error()
    );
    for t in [
        "Of the Mortification of Sin",
        "The Westminster Confession of Faith",
        "Showing 5 of 5 works · 0 chosen (0 KB)",
    ] {
        assert!(h.texts().contains(&t), "{t}: {:?}", h.texts());
    }
    assert!(
        h.texts()
            .iter()
            .any(|t| t.starts_with("John Owen · 1656 · book · Puritan · EEBO-TCP")),
        "{:?}",
        h.texts()
    );
    // The token is in no field on screen and nowhere but the secret store.
    assert!(!h.texts().iter().any(|t| t.contains(TOKEN)));

    // One click downloads one work; it lands in the Library as a Book.
    let p = on_row(&h, "Of the Mortification of Sin", "Download");
    h.click(p);
    settle(&mut h);
    let books: Vec<String> = h
        .shell
        .st
        .session
        .library
        .of_type(ResourceType::Book)
        .map(|e| e.manifest.title.clone())
        .collect();
    assert_eq!(books, ["Of the Mortification of Sin"]);
    let id = obs_engine::col::col_resource_id("EEBO-TCP/John Owen/Owen - Mortification.md");
    let entry = h.shell.st.session.library.get(&id).unwrap();
    assert!(
        entry
            .manifest
            .attribution
            .contains("Christian Open Library")
    );
    // Its row says so and opens it.
    open_books(&mut h);
    let open = on_row(&h, "Of the Mortification of Sin", "Open");
    assert!(h.texts().contains(&"In your Library"));
    h.click(open);
    settle(&mut h);
    assert!(
        h.shell
            .st
            .session
            .workspace
            .tabs()
            .any(|t| t.kind == TabKind::Resource(id.clone()))
    );

    // A filter, then Download all shown.
    open_books(&mut h);
    h.shell
        .run("col.filter", &json!({ "collection": "Creeds" }))
        .unwrap();
    h.frames(2);
    assert!(
        h.texts()
            .contains(&"Showing 2 of 5 works · 0 chosen (0 KB)")
    );
    h.click_text("Download all 2 shown");
    settle(&mut h);
    assert_eq!(
        h.shell
            .st
            .session
            .library
            .of_type(ResourceType::Book)
            .count(),
        3
    );

    h.shell.run("col.filter", &json!({})).unwrap();

    // COL changes Owen: Check for new books offers Update; Update brings it in place.
    col.put(
        "EEBO-TCP/John Owen/Owen - Mortification — Part 2.md",
        "## Chapter II\n\nThe second <!-- p. 2 --> chapter, corrected.\n",
    );
    open_books(&mut h);
    h.click_text("Check for new books");
    settle(&mut h);
    assert!(
        h.texts().contains(&"Of your 3 COL books: 1 changed."),
        "{:?}",
        h.texts()
    );
    assert!(h.texts().contains(&"Changed in COL"));
    let revision = h
        .shell
        .st
        .session
        .library
        .get(&id)
        .unwrap()
        .manifest
        .revision;
    h.click_text("Update 1");
    settle(&mut h);
    assert_eq!(
        h.shell
            .st
            .session
            .library
            .get(&id)
            .unwrap()
            .manifest
            .revision,
        revision + 1
    );
    open_books(&mut h);
    assert!(
        h.texts().contains(&"Your 3 COL books are up to date."),
        "{:?}",
        h.texts()
    );

    // Disconnect forgets the token.
    h.click_text("Disconnect");
    settle(&mut h);
    assert!(h.shell.st.session.col.access().is_none());
    assert!(h.texts().contains(&"Connect"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_refused_token_says_so_plainly_and_asks_for_another() {
    let (mut h, col, root) = setup("refused");
    *col.refuse.lock().unwrap() = Some(obs_engine::sword_repo::FetchError::NotFound);
    open_books(&mut h);
    h.shell
        .run("col.connect_token", &json!({ "token": TOKEN }))
        .unwrap();
    settle(&mut h);
    assert!(
        h.texts().iter().any(|t| t
            .starts_with("This GitHub account cannot see the Christian Open Library's books yet.")),
        "{:?}",
        h.texts()
    );
    *col.refuse.lock().unwrap() = Some(obs_engine::sword_repo::FetchError::Unauthorized);
    h.shell.run("col.refresh", &json!({})).unwrap();
    settle(&mut h);
    assert!(
        h.texts()
            .iter()
            .any(|t| t.starts_with("GitHub did not accept this token.")),
        "{:?}",
        h.texts()
    );
    assert!(
        h.shell.st.session.col.access().is_none(),
        "a refused token is forgotten"
    );
    assert!(h.texts().contains(&"Connect"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn the_catalogue_can_be_browsed_before_connecting() {
    let (mut h, _col, root) = setup("public");
    open_books(&mut h);
    h.click_text("Browse the catalogue");
    settle(&mut h);
    for t in [
        "THE CATALOGUE · 5",
        "The Heidelberg Catechism",
        "Connect to download any of them.",
    ] {
        assert!(h.texts().contains(&t), "{t}: {:?}", h.texts());
    }
    let _ = std::fs::remove_dir_all(root);
}
