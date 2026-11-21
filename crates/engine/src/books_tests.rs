//! Books (ADR 0023, #344): a folder of Markdown books imported book by book under one Import
//! report; a Book Tab follows no Link set and goes to a printed page; Library search finds
//! a Book's words.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use obs_model::{KeyScheme, ResourceType, UnitId};
use serde_json::json;

use crate::books::BATCH_PREFIX;
use crate::resource_navigation::{PageMark, ResourceNavigation};
use crate::*;

struct Lab {
    root: PathBuf,
}

impl Lab {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("obs-books-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("library")).unwrap();
        Self { root }
    }

    fn write(&self, rel: &str, text: impl AsRef<[u8]>) {
        let path = self.root.join("books").join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn session(&self) -> Session {
        let mut s = Session::new(JobMode::Inline);
        s.library.scan(&self.root.join("library"));
        let (user, _) = obs_store::UserStore::open(&self.root.join("user.db")).unwrap();
        s.attach_user(Box::new(user));
        s
    }

    fn books(&self) -> PathBuf {
        self.root.join("books")
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A COL-like folder: a split work, a book in a subfolder, an empty one that cannot be
/// imported, one that is not UTF-8, and an Obsidian settings folder that is never read.
fn col_folder(lab: &Lab) {
    lab.write(
        "EEBO-TCP/John Owen/Owen - Mortification.md",
        "---\ntitle: Of the Mortification of Sin\nauthor: John Owen\ndate: 1656\n\
         license: public domain\n---\n\n# Of the Mortification of Sin\n\n- [Part 1](x)\n",
    );
    lab.write(
        "EEBO-TCP/John Owen/Owen - Mortification — Part 1.md",
        "<!-- p. 1 -->\n\n## Chapter I\n\nMortify the deeds of the body, Rom. 8. 13.\n\n\
         ## Chapter II\n\nThe second <!-- p. 2 --> chapter.\n",
    );
    lab.write(
        "Creeds/Westminster Confession.md",
        "---\ntitle: The Westminster Confession of Faith\ndate: 1646\nlicense: public domain\n---\n\n\
         ## Chapter I. Of the Holy Scripture\n\nThe light of nature.\n\n\
         ## Chapter II. Of God\n\nThere is but one only living and true God.\n",
    );
    lab.write(
        "Contributed/Empty.md",
        "---\ntitle: Empty\n---\n\n<!-- nothing -->\n",
    );
    lab.write(
        "Contributed/Latin-1.md",
        b"Caf\xe9 \xe0 la carte.\n".as_slice(),
    );
    lab.write(".obsidian/workspace.md", "# not a book\n");
}

fn start(s: &mut Session, path: &Path) -> String {
    s.run("import.start", &json!({ "path": path })).unwrap()["id"]
        .as_str()
        .unwrap()
        .into()
}

fn finish(s: &mut Session) {
    for _ in 0..100 {
        s.jobs().pump(64);
        assert!(s.poll_imports().is_empty(), "no stray events");
        if !s.imports.is_running() {
            return;
        }
    }
    panic!("the batch never finished");
}

#[test]
fn a_folder_of_books_imports_each_book_under_one_report_and_lists_what_failed() {
    let lab = Lab::new("folder");
    col_folder(&lab);
    let mut s = lab.session();
    let id = start(&mut s, &lab.books());
    assert!(id.starts_with(BATCH_PREFIX), "{id}");
    // One report Tab, named after the folder.
    let tabs: Vec<_> = s
        .workspace
        .tabs()
        .filter(|t| matches!(t.kind, TabKind::ImportReport(_)))
        .map(|t| t.title.clone())
        .collect();
    assert_eq!(tabs, ["Import report · books"]);
    finish(&mut s);

    let record = s.imports.report(&id).unwrap();
    assert_eq!(record.phase, ImportPhase::Complete);
    let books = record.books.as_ref().unwrap();
    let mut titles: Vec<&str> = books.imported.iter().map(|b| b.title.as_str()).collect();
    titles.sort_unstable();
    assert_eq!(
        titles,
        [
            "Of the Mortification of Sin",
            "The Westminster Confession of Faith"
        ]
    );
    let failed: Vec<(&str, &str)> = books
        .failed
        .iter()
        .map(|f| (f.name.as_str(), f.error.as_str()))
        .collect();
    assert_eq!(failed.len(), 2, "{failed:?}");
    assert!(
        failed
            .iter()
            .any(|(n, e)| *n == "Contributed/Empty.md" && e.contains("no text found"))
    );
    assert!(
        failed
            .iter()
            .any(|(n, e)| *n == "Contributed/Latin-1.md" && e.contains("must be UTF-8"))
    );
    assert!(!books.imported.iter().any(|b| b.name.contains(".obsidian")));

    // Each book is a Book in the Library with its own Import record.
    for book in &books.imported {
        let entry = s.library.get(&book.resource_id).unwrap();
        assert_eq!(entry.manifest.resource_type, ResourceType::Book);
        assert!(entry.path.exists());
        let own = s.imports.report(&book.resource_id).unwrap();
        assert_eq!(own.phase, ImportPhase::Complete);
        assert_eq!(
            own.source,
            format!("{} in {}", book.name, lab.books().display())
        );
    }
    let owen = books
        .imported
        .iter()
        .find(|b| b.name.ends_with("Owen - Mortification.md"))
        .unwrap();
    assert!(owen.resource_id.starts_with("owen-mortification-"));
    let report = s.imports.report(&owen.resource_id).unwrap();
    assert_eq!(report.report.as_ref().unwrap().source_files.len(), 2);
    assert!(s.workspace.take_effects().contains(&Effect::Message(
        "Imported 2 books; 2 could not be imported".into()
    )));
}

#[test]
fn a_cancelled_batch_keeps_the_books_already_in() {
    let lab = Lab::new("cancel");
    for n in 1..=4 {
        lab.write(
            &format!("Book {n}.md"),
            format!("## One\n\nBook {n}.\n\n## Two\n\nMore.\n"),
        );
    }
    let mut s = lab.session();
    let id = start(&mut s, &lab.books());
    s.jobs().pump(1);
    s.poll_imports();
    assert_eq!(s.library.of_type(ResourceType::Book).count(), 1);
    s.run("import.cancel", &json!({ "id": id })).unwrap();
    finish(&mut s);
    let record = s.imports.report(&id).unwrap();
    assert_eq!(record.phase, ImportPhase::Failed);
    assert_eq!(record.error.as_deref(), Some("Import cancelled"));
    assert_eq!(record.books.as_ref().unwrap().imported.len(), 1);
    assert_eq!(s.library.of_type(ResourceType::Book).count(), 1);
}

#[test]
fn a_book_tab_follows_no_link_set_and_goes_to_a_printed_page() {
    let mut s = Session::new(JobMode::Inline);
    let mut bible = obs_model::Manifest::new("kjv", "KJV", "KJV", ResourceType::Bible, "en");
    bible.key_schemes = vec![KeyScheme::Passage];
    s.library.add(LibraryEntry {
        manifest: bible,
        path: "/kjv.obr".into(),
    });
    s.library.add(LibraryEntry {
        manifest: obs_model::Manifest::new("owen", "Owen", "OWEN", ResourceType::Book, "en"),
        path: "/owen.obr".into(),
    });
    let bible = TabId(
        s.run("resource.open", &json!({"id": "kjv"})).unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    s.workspace.set_link(bible, Some('A'));
    s.workspace.focus(bible);
    let book = TabId(
        s.run("resource.open", &json!({"id": "owen"})).unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    let t = s.workspace.tab(book).unwrap();
    assert!(t.key_schemes.is_empty());
    assert_eq!(t.header_box(), HeaderBox::Page);
    assert!(!t.has_link_chip());
    assert_eq!(t.link, None, "a Book never joins its opener's Link set");
    assert!(
        s.run("tab.link", &json!({"tab": book.0, "set": "A"}))
            .is_err()
    );

    // Go to page, as the reading view reports the Book's pages.
    let unit = |id: &str| UnitId::new(id).unwrap();
    let page = |label: &str, u: &str, byte| PageMark {
        label: label.into(),
        unit: unit(u),
        byte,
    };
    let nav = ResourceNavigation::new(Vec::new()).with_pages(
        vec![
            page("iv", "div.1", 0),
            page("1", "div.2", 0),
            page("2", "div.2", 120),
        ],
        [unit("div.1"), unit("div.2")],
    );
    assert_eq!(nav.page_at(&unit("div.2"), 119).unwrap().label, "1");
    assert_eq!(nav.page_at(&unit("div.2"), 120).unwrap().label, "2");
    assert_eq!(nav.page_at(&unit("div.1"), 5).unwrap().label, "iv");
    s.workspace.tab_mut(book).unwrap().navigation = Some(Arc::new(nav));
    s.workspace.take_effects();
    for typed in ["2", "p. 2", "Page 2"] {
        let out = s
            .run("tab.go", &json!({"tab": book.0, "text": typed}))
            .unwrap();
        assert_eq!(out["page"], "2");
        assert!(s.workspace.take_effects().contains(&Effect::NavigateText {
            tab: book,
            unit: unit("div.2"),
            range: 120..120,
            key: None,
        }));
    }
    let err = s
        .run("tab.go", &json!({"tab": book.0, "text": "99"}))
        .unwrap_err();
    assert!(err.to_string().contains("No page 99 in this Book"), "{err}");
    // The Bible's Link set never moved the Book.
    assert_eq!(s.workspace.tab(book).unwrap().history.current(), None);
}

#[test]
fn library_search_finds_a_books_words() {
    let lab = Lab::new("search");
    col_folder(&lab);
    let mut s = lab.session();
    s.library.set_directory(lab.root.join("library"));
    s.set_search_cache_dir(lab.root.join("cache"));
    let id = start(&mut s, &lab.books());
    finish(&mut s);
    assert!(s.imports.report(&id).is_some());
    for _ in 0..200 {
        s.jobs().pump(64);
        s.poll_imports();
        if !s.search.is_building() {
            break;
        }
    }
    let out = s
        .run(
            "search.run",
            &json!({"text": "\"only living and true God\"", "mode": "library"}),
        )
        .unwrap();
    for _ in 0..200 {
        s.jobs().pump(64);
        s.poll_imports();
    }
    let tab = TabId(out["tab"].as_u64().unwrap());
    let results = s
        .search_tab(tab)
        .unwrap()
        .library_results
        .clone()
        .expect("library results");
    let groups: Vec<&str> = results.groups.iter().map(|g| g.title.as_str()).collect();
    assert_eq!(groups, ["The Westminster Confession of Faith"]);
}

/// Ask for a Guide section until it is no longer Loading, pumping between asks.
fn section(s: &mut Session, key: &obs_model::Key, section: GuideSection) -> SectionStatus {
    for _ in 0..100 {
        let status = s.guide_section(key, section);
        if status != SectionStatus::Loading {
            return status;
        }
        s.jobs().pump(8);
        s.poll_imports();
    }
    panic!("{section:?} never finished");
}

#[test]
fn cited_in_lists_the_books_that_cite_a_passage_earliest_first() {
    use crate::cited_in::CitingBook;
    use obs_scripture::{Book, Passage, Verse};
    let lab = Lab::new("cited");
    lab.write(
        "Owen.md",
        "---\ntitle: Of the Mortification of Sin\nauthor: John Owen\ndate: 1656\n---\n\n\
         ## Chapter I\n\nIf ye through the Spirit do mortify the deeds of the body, Rom. 8. 13, \
         ye shall live.\n\nAgain Rom. viii. 13 and Col. 3. 5.\n\n## Chapter II\n\nOn John 3:16.\n",
    );
    lab.write(
        "Spurgeon.md",
        "---\ntitle: Sermons\nauthor: C. H. Spurgeon\ndate: 1896\n---\n\n\
         ## Sermon\n\nThe Spirit helps us mortify sin (Romans 8:12-14).\n",
    );
    lab.write(
        "Calvin.md",
        "---\ntitle: Institutes\nauthor: John Calvin\ndate: 1559\n---\n\n## Book III\n\nNothing on it.\n",
    );
    let mut s = lab.session();
    start(&mut s, &lab.books());
    finish(&mut s);
    let rom = |v| Verse::new(Book::Romans, 8, v);
    let key = obs_model::Key::Passage(Passage::verse(rom(13)));
    let SectionStatus::Ready(data) = section(&mut s, &key, GuideSection::CitedIn) else {
        panic!("not ready");
    };
    let SectionData::CitedIn(books) = &*data else {
        panic!("{data:?}");
    };
    let shown: Vec<(&str, Option<u16>, usize)> = books
        .iter()
        .map(|b: &CitingBook| (b.title.as_str(), b.year, b.count))
        .collect();
    // Owen (1656) before Spurgeon (1896); Calvin does not cite it.
    assert_eq!(
        shown,
        [
            ("Of the Mortification of Sin", Some(1656), 2),
            ("Sermons", Some(1896), 1)
        ]
    );
    let owen = &books[0];
    assert_eq!(owen.author.as_deref(), Some("John Owen"));
    assert_eq!(owen.first.division.as_deref(), Some("Chapter I"));
    let (a, b) = owen.first.mark;
    assert_eq!(&owen.first.excerpt[a..b], "Rom. 8. 13");
    assert!(owen.first.excerpt.starts_with("If ye through the Spirit"));

    // A Passage no Book cites: the empty state says so.
    let key = obs_model::Key::Passage(Passage::verse(Verse::new(Book::Genesis, 1, 1)));
    let SectionStatus::Ready(data) = section(&mut s, &key, GuideSection::CitedIn) else {
        panic!("not ready");
    };
    assert!(data.is_empty());
    assert_eq!(
        s.guide_empty_state(&key, GuideSection::CitedIn).message,
        "None in your library: no Book cites this Passage."
    );
    // The Passage Guide lists the section last.
    assert_eq!(
        Guide::PassageGuide.sections().last(),
        Some(&GuideSection::CitedIn)
    );
}
