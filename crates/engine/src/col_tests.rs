#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use super::*;

fn blob(path: &str, sha: char, size: u64) -> String {
    format!(
        r#"{{"path":"{path}","mode":"100644","type":"blob","sha":"{}","size":{size}}}"#,
        sha.to_string().repeat(40)
    )
}

/// A tree like COL's: catalogue files, a split work, a book, a hidden file, a link, a
/// path escape, a non-Markdown file and an oversized file.
fn tree_json() -> String {
    let entries = [
        blob("catalog_full.csv", 'a', 900),
        blob("authors.csv", 'b', 300),
        r#"{"path":"EEBO-TCP","mode":"040000","type":"tree","sha":"cccccccccccccccccccccccccccccccccccccccc"}"#.to_string(),
        blob("EEBO-TCP/John Owen/Owen - Mortification.md", 'c', 200),
        blob("EEBO-TCP/John Owen/Owen - Mortification — Part 1.md", 'd', 50_000),
        blob("EEBO-TCP/John Owen/Owen - Mortification — Part 2.md", 'e', 40_000),
        blob("Creeds/Westminster Confession.md", 'f', 30_000),
        blob(".github/notes.md", '1', 10),
        r#"{"path":"Links/evil.md","mode":"120000","type":"blob","sha":"2222222222222222222222222222222222222222","size":10}"#.to_string(),
        blob("../escape.md", '3', 10),
        blob("Images/cover.png", '4', 10),
        blob("Huge/huge.md", '5', MAX_FILE_BYTES + 1),
    ];
    format!(
        r#"{{"sha":"x","truncated":false,"tree":[{}]}}"#,
        entries.join(",")
    )
}

const CATALOG: &str = "File,Author,Title,Full title,Year,Type,License,Source\n\
EEBO-TCP/John Owen/Owen - Mortification.md,John Owen,\"Of the Mortification of Sin, in Believers\",,1656,book,public domain,https://quod.lib.umich.edu/x\n\
Creeds/Westminster Confession.md,Westminster Assembly,The Westminster Confession of Faith,,1646,confession,public domain,\n";

const AUTHORS: &str = "Author,Dates,Tradition,Note\nJohn Owen,1616–1683,Puritan,English Puritan\n";

#[test]
fn the_tree_keeps_only_safe_markdown_and_catalogue_files() {
    let tree = ColTree::from_github("abc123", tree_json().as_bytes()).unwrap();
    // Byte order, as git's: a book's Parts (" — Part") before its main file (".md").
    let paths: Vec<&str> = tree.files.keys().map(String::as_str).collect();
    assert_eq!(
        paths,
        [
            "Creeds/Westminster Confession.md",
            "EEBO-TCP/John Owen/Owen - Mortification — Part 1.md",
            "EEBO-TCP/John Owen/Owen - Mortification — Part 2.md",
            "EEBO-TCP/John Owen/Owen - Mortification.md",
            "authors.csv",
            "catalog_full.csv",
        ]
    );
    // The link, the escape and the oversized file are refused; the PNG and the hidden
    // file are simply not wanted.
    assert_eq!(tree.refused, 3);
    assert_eq!(tree.commit, "abc123");
    let truncated = r#"{"truncated":true,"tree":[]}"#;
    assert_eq!(
        ColTree::from_github("x", truncated.as_bytes()),
        Err(TreeError::Truncated)
    );
    assert!(ColTree::from_github("x", b"not json").is_err());
}

#[test]
fn unsafe_paths_are_refused() {
    for bad in [
        "",
        "/abs.md",
        "a/../b.md",
        "./a.md",
        "a//b.md",
        ".git/config",
        "a/.GIT/x.md",
        "a\\b.md",
        "a\u{0}.md",
    ] {
        assert!(!safe_path(bad), "{bad:?}");
    }
    assert!(safe_path("EEBO-TCP/John Owen/Owen — Part 2.md"));
    assert!(!safe_path(&"a/".repeat(300)));
}

#[test]
fn works_join_the_tree_and_catalogue_and_each_has_a_stable_resource_id() {
    let tree = ColTree::from_github("abc", tree_json().as_bytes()).unwrap();
    let works = works(&tree, Some(CATALOG), Some(AUTHORS));
    assert_eq!(works.len(), 2);
    let owen = works.iter().find(|w| w.author == "John Owen").unwrap();
    assert_eq!(owen.title, "Of the Mortification of Sin, in Believers");
    assert_eq!(owen.year, Some(1656));
    assert_eq!(owen.kind, "book");
    assert_eq!(owen.collection, "EEBO-TCP");
    assert_eq!(owen.tradition, "Puritan");
    assert_eq!(owen.parts.len(), 2);
    assert_eq!(owen.size, 90_200);
    let id = owen.resource_id();
    assert!(id.starts_with("col-owen-mortification-"), "{id}");
    assert!(
        id.len() <= 64 && obs_model::is_valid_resource_id(&id),
        "{id}"
    );
    assert_eq!(
        id,
        col_resource_id("EEBO-TCP/John Owen/Owen - Mortification.md")
    );
    // Another folder with the same file name is another book.
    assert_ne!(id, col_resource_id("Other/Owen - Mortification.md"));
    // A Part's path gives its book's id.
    assert_eq!(
        id,
        col_resource_id("EEBO-TCP/John Owen/Owen - Mortification — Part 1.md")
    );
    let long = format!("X/{}.md", "word ".repeat(40));
    assert!(col_resource_id(&long).len() <= 64);
    assert!(obs_model::is_valid_resource_id(&col_resource_id(&long)));

    // Without a catalogue, a book is still offered, by its file name.
    let bare = super::works(&tree, None, None);
    assert!(bare.iter().any(|w| w.title == "Westminster Confession"));
}

#[test]
fn filters_match_type_author_years_tradition_collection_and_old_spellings() {
    let tree = ColTree::from_github("abc", tree_json().as_bytes()).unwrap();
    let works = works(&tree, Some(CATALOG), Some(AUTHORS));
    let shown = |f: ColFilter| -> Vec<String> {
        works
            .iter()
            .filter(|w| f.accepts(w))
            .map(|w| w.author.clone())
            .collect()
    };
    assert_eq!(shown(ColFilter::default()).len(), 2);
    let f = |text: &str| ColFilter {
        text: text.into(),
        ..ColFilter::default()
    };
    assert_eq!(shown(f("mortif owen")), ["John Owen"]);
    // Old spellings: u/v, i/j and ſ fold ("belieuers" finds "Believers"); other
    // spellings ("Beleevers") do not.
    assert_eq!(shown(f("Beleevers")), Vec::<String>::new());
    assert_eq!(shown(f("belieuers")), ["John Owen"]);
    assert_eq!(shown(f("Belieuerſ")), ["John Owen"]);
    assert_eq!(shown(f("westminster")), ["Westminster Assembly"]);
    assert_eq!(
        shown(ColFilter {
            kind: Some("confession".into()),
            ..ColFilter::default()
        }),
        ["Westminster Assembly"]
    );
    assert_eq!(
        shown(ColFilter {
            tradition: Some("puritan".into()),
            ..ColFilter::default()
        }),
        ["John Owen"]
    );
    assert_eq!(
        shown(ColFilter {
            collection: Some("Creeds".into()),
            ..ColFilter::default()
        }),
        ["Westminster Assembly"]
    );
    assert_eq!(
        shown(ColFilter {
            year_from: Some(1650),
            year_to: Some(1700),
            ..ColFilter::default()
        }),
        ["John Owen"]
    );
    let kinds = facet(&works, |w| &w.kind);
    assert_eq!(kinds, [("book".into(), 1), ("confession".into(), 1)]);
}

#[test]
fn csv_reads_quotes_commas_line_breaks_and_a_bom() {
    let rows = csv_rows(
        "\u{feff}A,B\r\n\"x, y\",\"say \"\"hi\"\"\"\n\"two\nlines\",z\n\n,last\n",
        100,
    );
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0]["A"], "x, y");
    assert_eq!(rows[0]["B"], "say \"hi\"");
    assert_eq!(rows[1]["A"], "two\nlines");
    assert_eq!(rows[2]["B"], "last");
    assert_eq!(csv_rows("A\n1\n2\n3\n", 2).len(), 2);
    assert!(csv_rows("", 10).is_empty());
    // Hostile: unbalanced quotes and huge fields never panic.
    let _ = csv_rows(&format!("A\n\"{}", "x".repeat(100_000)), 10);
}

#[test]
fn the_public_catalogue_lists_every_work_without_paths() {
    let csv = "Author,Title,Year,Type,Parts,Folder\n\
               John Owen,Of Communion with God,1657,book,,EEBO-TCP/John Owen\n\
               ,,,,,\n";
    let works = public_works(csv);
    assert_eq!(works.len(), 1);
    assert_eq!(works[0].year, Some(1657));
    assert_eq!(works[0].folder, "EEBO-TCP/John Owen");
}

#[test]
fn git_blob_shas_match_git() {
    // `printf 'hello\n' | git hash-object --stdin`
    assert_eq!(
        git_blob_sha(b"hello\n"),
        "ce013625030ba8dba906f756967f9e9ca394464a"
    );
    assert_eq!(
        git_blob_sha(b""),
        "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391"
    );
}

#[test]
fn check_for_new_books_finds_new_changed_removed_and_current() {
    let tree = ColTree::from_github("abc", tree_json().as_bytes()).unwrap();
    let works = works(&tree, Some(CATALOG), Some(AUTHORS));
    let origin = |path: &str, blobs: &[(&str, char)]| ColOrigin {
        via: "github".into(),
        path: path.into(),
        commit: "old".into(),
        blobs: blobs
            .iter()
            .map(|(p, c)| (p.to_string(), c.to_string().repeat(40)))
            .collect(),
    };
    let owen = "EEBO-TCP/John Owen/Owen - Mortification.md";
    let mut installed = BTreeMap::new();
    installed.insert(
        "owen".to_string(),
        origin(
            owen,
            &[
                (owen, 'c'),
                ("EEBO-TCP/John Owen/Owen - Mortification — Part 1.md", 'd'),
                ("EEBO-TCP/John Owen/Owen - Mortification — Part 2.md", '9'),
            ],
        ),
    );
    installed.insert(
        "gone".to_string(),
        origin("Old/Gone.md", &[("Old/Gone.md", '7')]),
    );
    let changes = compare(&installed, &tree, &works);
    assert_eq!(changes.changed, [("owen".to_string(), owen.to_string())]);
    assert_eq!(changes.removed, ["gone"]);
    assert_eq!(changes.new, ["Creeds/Westminster Confession.md"]);
    assert_eq!(changes.current, 0);
    // Up to date once its blobs match.
    installed.insert(
        "owen".to_string(),
        origin(
            owen,
            &[
                (owen, 'c'),
                ("EEBO-TCP/John Owen/Owen - Mortification — Part 1.md", 'd'),
                ("EEBO-TCP/John Owen/Owen - Mortification — Part 2.md", 'e'),
            ],
        ),
    );
    let changes = compare(&installed, &tree, &works);
    assert!(changes.changed.is_empty());
    assert_eq!(changes.current, 1);
}

#[test]
fn sizes_read_in_words() {
    assert_eq!(size_text(0), "0 KB");
    assert_eq!(size_text(1), "1 KB");
    assert_eq!(size_text(64_200), "65 KB");
    assert_eq!(size_text(4_472_092), "4.5 MB");
    assert_eq!(size_text(2_900_000_000), "2.9 GB");
}
