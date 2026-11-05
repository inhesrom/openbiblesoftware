//! The Notes Tool and Notes search (#64): cards, following anchors, Show in text, editing as
//! one undo step, Notebook deletion and searching words typed a moment ago.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use obs_scripture::{Book, Passage, Verse};
use serde_json::{Value, json};

use crate::testing::bibles::{kjv, web, write_library};
use crate::*;

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("obs-notes-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        write_library(&root.join("library"), &[kjv(), web()]);
        Self { root }
    }

    fn session(&self) -> Session {
        let mut s = Session::new(JobMode::Inline);
        assert!(s.library.scan(&self.root.join("library")).is_empty());
        let (store, _) = obs_store::UserStore::open(&self.root.join("user.db")).unwrap();
        s.attach_user(Box::new(store));
        s
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn v(book: Book, c: u16, n: u16) -> Verse {
    Verse::new(book, c, n)
}

/// A Note anchored on a Standard Passage, in `notebook` (default: the last used one).
fn note(s: &mut Session, passage: &str, body: &str, notebook: Option<&str>) -> String {
    let mut p = json!({ "body": body });
    if !passage.is_empty() {
        p["passage"] = json!(passage);
    }
    if let Some(n) = notebook {
        p["notebook"] = json!(n);
    }
    let id = s.run("annotation.note", &p).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    tick();
    id
}

/// Let the millisecond clock move on, so changes made one after another have distinct
/// modification times ("newest first" orders are then deterministic).
fn tick() {
    std::thread::sleep(std::time::Duration::from_millis(3));
}

/// A Highlight of the first occurrence of `word` in a KJV book.
fn highlight(s: &mut Session, book: &str, word: &str) -> String {
    let unit = kjv()
        .units
        .into_iter()
        .find(|u| u.id.as_str() == book)
        .unwrap();
    let at = unit.text.find(word).unwrap();
    s.run(
        "annotation.highlight",
        &json!({
            "resource": "kjv", "start_unit": book, "start": at, "end": at + word.len(),
            "style": "green",
        }),
    )
    .unwrap()["ids"][0]
        .as_str()
        .unwrap()
        .to_string()
}

fn card_ids(s: &Session, filter: &str) -> Vec<String> {
    s.note_cards(filter)
        .unwrap()
        .into_iter()
        .map(|c| c.annotation.id)
        .collect()
}

#[test]
fn cards_follow_the_notebook_choice_in_bible_order_or_last_modified() {
    let f = Fixture::new("cards");
    let mut s = f.session();
    let study = s
        .run("notebook.create", &json!({ "name": "Study" }))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let romans = note(&mut s, "Rom 13:10", "Love fulfils the law", None);
    let free = note(&mut s, "", "A thought with no anchor", None);
    let mark = highlight(&mut s, "MAT", "neighbour");
    // (New Annotations go into the Notebook last used: this one last.)
    let lev = note(&mut s, "Lev 19:18", "The neighbour command", Some(&study));

    // All, in Bible order: Leviticus, Matthew, Romans, then the Note without an anchor.
    assert_eq!(
        card_ids(&s, ""),
        [lev.as_str(), mark.as_str(), romans.as_str(), free.as_str()]
    );
    let cards = s.note_cards("").unwrap();
    assert_eq!(cards[0].reference, "Leviticus 19:18");
    assert_eq!(cards[0].notebook, "Study");
    assert_eq!(cards[1].reference, "Matthew 5:43 (KJV)");
    assert_eq!(cards[1].quote.as_deref(), Some("neighbour"));
    assert!(cards[1].followable());
    assert_eq!(cards[3].reference, "No anchor");
    assert!(!cards[3].followable());

    // Last modified: newest change first; an edit moves a card to the top.
    s.run("notes.order", &json!({ "value": "modified" }))
        .unwrap();
    assert_eq!(
        card_ids(&s, ""),
        [lev.as_str(), mark.as_str(), free.as_str(), romans.as_str()]
    );
    s.run(
        "annotation.edit",
        &json!({ "id": romans, "body": "Love is the fulfilling of the law" }),
    )
    .unwrap();
    assert_eq!(card_ids(&s, "")[0], romans);

    // One Notebook; the header filter narrows by body, quote, reference or Notebook.
    s.run("notes.notebook", &json!({ "value": study })).unwrap();
    assert_eq!(card_ids(&s, ""), [lev.as_str()]);
    s.run("notes.notebook", &json!({ "value": "all" })).unwrap();
    assert_eq!(card_ids(&s, "LAW"), [romans.as_str()]);
    assert_eq!(card_ids(&s, "matthew"), [mark.as_str()]);
    assert_eq!(card_ids(&s, "study neighbour"), [lev.as_str()]);
    assert!(
        s.run("notes.notebook", &json!({ "value": "nope" }))
            .is_err()
    );
    assert!(s.run("notes.order", &json!({ "value": "nope" })).is_err());

    // A deleted Notebook's choice falls back to All.
    s.run("notes.notebook", &json!({ "value": study })).unwrap();
    s.run(
        "notebook.delete",
        &json!({ "id": study, "annotations": "move" }),
    )
    .unwrap();
    assert_eq!(card_ids(&s, "").len(), 4);
}

#[test]
fn opening_in_notes_reveals_the_card_and_switches_to_all_when_needed() {
    let f = Fixture::new("open");
    let mut s = f.session();
    let study = s
        .run("notebook.create", &json!({ "name": "Study" }))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let id = note(&mut s, "Rom 13:10", "x", None);
    s.run("notes.notebook", &json!({ "value": study })).unwrap();
    let tab = s.run("notes.open", &json!({ "id": id })).unwrap()["tab"]
        .as_u64()
        .unwrap();
    assert_eq!(
        s.workspace.tab(TabId(tab)).unwrap().kind,
        TabKind::Tool(Tool::Notes)
    );
    assert_eq!(s.notes.filter, NotesFilter::All);
    assert_eq!(s.notes.reveal, Some((id.clone(), true)));
    // Single instance: opening again focuses the same Tab.
    let again = s.run("notes.open", &json!({})).unwrap()["tab"]
        .as_u64()
        .unwrap();
    assert_eq!(again, tab);
    assert!(s.run("notes.open", &json!({ "id": "nope" })).is_err());
}

#[test]
fn following_a_card_goes_to_the_most_recent_bible_preferring_its_origin() {
    let f = Fixture::new("follow");
    let mut s = f.session();
    let passage = note(&mut s, "Rom 13:10", "x", None);
    let mark = highlight(&mut s, "MAT", "neighbour");
    let free = note(&mut s, "", "y", None);
    s.workspace.take_effects();

    // No Bible open: a new Tab, which goes to the Verse once its Verse map is loaded.
    let out = s.run("notes.follow", &json!({ "id": passage })).unwrap();
    assert_eq!(out["via"], "new");
    let first = TabId(out["tab"].as_u64().unwrap());
    let effects = s.workspace.take_effects();
    assert!(
        effects.contains(&Effect::NavigateStandard {
            tab: first,
            verse: v(Book::Romans, 13, 10)
        }),
        "{effects:?}"
    );

    // WEB opened later is the most recently active Bible.
    let web = TabId(
        s.run("resource.open", &json!({ "id": "web" })).unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    let kjv = if s.workspace.tab(first).unwrap().kind == TabKind::Resource("kjv".into()) {
        first
    } else {
        TabId(
            s.run("resource.open", &json!({ "id": "kjv" })).unwrap()["tab"]
                .as_u64()
                .unwrap(),
        )
    };
    s.workspace.focus(web);
    s.workspace.take_effects();
    // A Passage anchor without an origin: the most recent Bible.
    let out = s.run("notes.follow", &json!({ "id": passage })).unwrap();
    assert_eq!(
        (out["via"].as_str(), out["tab"].as_u64()),
        (Some("tab"), Some(web.0))
    );
    let effects = s.workspace.take_effects();
    assert!(effects.contains(&Effect::Navigate {
        tab: web,
        verse: v(Book::Romans, 13, 10)
    }));
    // The Highlight was made in the KJV: its origin wins although WEB is more recent.
    let out = s.run("notes.follow", &json!({ "id": mark })).unwrap();
    assert_eq!(out["tab"].as_u64(), Some(kjv.0));
    let effects = s.workspace.take_effects();
    assert!(effects.contains(&Effect::Navigate {
        tab: kjv,
        verse: v(Book::Matthew, 5, 43)
    }));
    // Ctrl/Cmd: always a new Tab.
    let tabs = s.workspace.tabs().count();
    let out = s
        .run("notes.follow", &json!({ "id": mark, "split": true }))
        .unwrap();
    assert_eq!(out["via"], "new");
    assert_eq!(s.workspace.tabs().count(), tabs + 1);
    // Nothing to follow.
    assert!(s.run("notes.follow", &json!({ "id": free })).is_err());
}

/// A Bible reference in a Note body: `notes.show_passage` shows it like a Passage anchor.
#[test]
fn a_body_reference_shows_its_passage_through_the_registry() {
    let f = Fixture::new("show-passage");
    let mut s = f.session();
    // No Bible open: a new Tab, which goes to the Verse once its Verse map is loaded.
    let out = s
        .run("notes.show_passage", &json!({ "passage": "Rom 13:10" }))
        .unwrap();
    assert_eq!(out["via"], "new");
    let first = TabId(out["tab"].as_u64().unwrap());
    assert!(
        s.workspace
            .take_effects()
            .contains(&Effect::NavigateStandard {
                tab: first,
                verse: v(Book::Romans, 13, 10)
            })
    );
    // Then in that Tab.
    let out = s
        .run("notes.show_passage", &json!({ "passage": "Matt 5:43" }))
        .unwrap();
    assert_eq!(
        (out["via"].as_str(), out["tab"].as_u64()),
        (Some("tab"), Some(first.0))
    );
    assert!(s.workspace.take_effects().contains(&Effect::Navigate {
        tab: first,
        verse: v(Book::Matthew, 5, 43)
    }));
    let err = s
        .run("notes.show_passage", &json!({ "passage": "Hezekiah 1:1" }))
        .unwrap_err();
    assert!(err.to_string().contains("Hezekiah"), "{err}");
}

#[test]
fn following_a_text_range_anchor_reveals_it_in_its_resource() {
    let f = Fixture::new("follow-range");
    let mut s = f.session();
    // A Text-range anchor (as on an Unmapped verse or a Commentary), made directly.
    let now = obs_store::annotation::now_ms();
    let unit = kjv()
        .units
        .into_iter()
        .find(|u| u.id.as_str() == "ROM")
        .unwrap();
    let at = unit.text.find("fulfilling").unwrap();
    let range = TextRange {
        resource_id: "kjv".into(),
        unit: unit.id.clone(),
        start: at as u32,
        end: (at + 10) as u32,
        quote: "fulfilling".into(),
        prefix: String::new(),
        suffix: String::new(),
        content_hash: "sha256:x".into(),
        native_ref: None,
        whole_verses: false,
        orphaned: false,
    };
    let a = Annotation {
        id: obs_store::annotation::new_id_at(now),
        notebook_id: s.default_notebook().unwrap().id,
        content: Content::Highlight {
            style: HighlightStyle::Blue,
        },
        anchor: Some(Anchor::TextRange { range }),
        created_at: now,
        modified_at: now,
    };
    s.user
        .as_ref()
        .unwrap()
        .apply_changes(&[Change::insert(Row::Annotation(a.clone()))])
        .unwrap();
    let card = s.note_cards("").unwrap().remove(0);
    assert_eq!(card.reference, "KJV, ROM");
    let out = s.run("notes.follow", &json!({ "id": a.id })).unwrap();
    assert_eq!(out["via"], "new");
    let tab = TabId(out["tab"].as_u64().unwrap());
    assert!(s.workspace.take_effects().contains(&Effect::RevealText {
        tab,
        unit: unit.id.clone(),
        byte: at
    }));
    // Its Resource leaves the library: the card says so and cannot be followed.
    s.library.remove("kjv");
    let card = s.note_cards("").unwrap().remove(0);
    assert_eq!(card.missing.as_deref(), Some("KJV is not in the library"));
    assert!(!card.followable());
    assert!(s.run("notes.follow", &json!({ "id": a.id })).is_err());
}

/// Show in text as the Notes Tool's 👁 uses it: #63's `notebook.show_in_text` (`show`), read
/// back by `hidden_in_text`; hidden Notebooks still list as cards.
#[test]
fn show_in_text_is_kept_per_notebook_in_the_user_database() {
    let f = Fixture::new("shown");
    let mut s = f.session();
    let default = s.default_notebook().unwrap().id;
    let id = note(&mut s, "Rom 13:10", "Love", None);
    assert!(s.hidden_in_text().is_empty());
    assert_eq!(s.annotations_in_text().unwrap().len(), 1);
    let revision = s.annotations_revision();
    // No `show`: the other way round.
    let out = s
        .run("notebook.show_in_text", &json!({ "id": default }))
        .unwrap();
    assert_eq!(out["notebook"]["id"], default.as_str());
    assert_eq!(out["notebook"]["show_in_text"], false);
    assert!(s.hidden_in_text().contains(&default));
    assert!(
        s.annotations_in_text().unwrap().is_empty(),
        "hidden in text"
    );
    assert_eq!(
        s.note_cards("").unwrap().len(),
        1,
        "still in the Notes Tool"
    );
    assert!(s.annotations_revision() > revision, "views reload");
    assert!(s.is_enabled("edit.undo"), "one undo step");
    assert!(
        s.run("notebook.show_in_text", &json!({ "id": "nope" }))
            .is_err()
    );
    drop(s);
    let mut s = f.session();
    assert!(
        s.hidden_in_text().contains(&default),
        "kept across launches"
    );
    let out = s
        .run(
            "notebook.show_in_text",
            &json!({ "id": default, "show": true }),
        )
        .unwrap();
    assert_eq!(out["notebook"]["show_in_text"], true);
    assert!(s.hidden_in_text().is_empty());
    assert_eq!(s.annotations_in_text().unwrap()[0].id, id);
    // Asking for what it already is changes nothing.
    let revision = s.annotations_revision();
    s.set_show_in_text(&default, Some(true)).unwrap();
    assert_eq!(s.annotations_revision(), revision);
}

#[test]
fn saving_while_typing_is_one_undo_step() {
    let f = Fixture::new("continue");
    let mut s = f.session();
    let id = note(&mut s, "Rom 13:10", "Love", None);
    for (i, body) in ["Love w", "Love wor", "Love worketh"].iter().enumerate() {
        s.run(
            "annotation.edit",
            &json!({ "id": id, "body": body, "continue": i > 0 }),
        )
        .unwrap();
    }
    assert_eq!(s.annotation(&id).unwrap().body(), "Love worketh");
    assert_eq!(s.undo_history().undo_label(), Some("Edit Note"));
    s.run("edit.undo", &Value::Null).unwrap();
    assert_eq!(s.annotation(&id).unwrap().body(), "Love", "the whole pass");
    s.run("edit.redo", &Value::Null).unwrap();
    assert_eq!(s.annotation(&id).unwrap().body(), "Love worketh");
    // A new pass after something else is its own step, even when asked to continue.
    s.run("notebook.create", &json!({ "name": "Other" }))
        .unwrap();
    s.run(
        "annotation.edit",
        &json!({ "id": id, "body": "Love worketh no ill", "continue": true }),
    )
    .unwrap();
    s.run("edit.undo", &Value::Null).unwrap();
    assert_eq!(s.annotation(&id).unwrap().body(), "Love worketh");
}

#[test]
fn deleting_a_notebook_moves_its_annotations_by_default_and_undo_restores_it() {
    let f = Fixture::new("delete");
    let mut s = f.session();
    let study = s
        .run("notebook.create", &json!({ "name": "Study" }))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let a = note(&mut s, "Rom 13:10", "a", Some(&study));
    let b = note(&mut s, "Lev 19:18", "b", Some(&study));
    let before = s.user.as_ref().unwrap().annotation_dump().unwrap();
    // The pre-selected choice: move to the default Notebook.
    s.run("notebook.delete", &json!({ "id": study })).unwrap();
    let default = s.default_notebook().unwrap().id;
    for id in [&a, &b] {
        assert_eq!(s.annotation(id).unwrap().notebook_id, default);
    }
    s.run("edit.undo", &Value::Null).unwrap();
    assert_eq!(s.user.as_ref().unwrap().annotation_dump().unwrap(), before);
    // Deleting them instead, also undoable.
    s.run(
        "notebook.delete",
        &json!({ "id": study, "annotations": "delete" }),
    )
    .unwrap();
    assert!(s.annotations(None).unwrap().is_empty());
    s.run("edit.undo", &Value::Null).unwrap();
    assert_eq!(s.user.as_ref().unwrap().annotation_dump().unwrap(), before);
}

fn notes_search(s: &mut Session, params: Value) -> (TabId, Vec<String>) {
    let mut p = params;
    p["mode"] = json!("notes");
    let out = s.run("search.run", &p).unwrap();
    assert!(out["error"].is_null(), "{out}");
    let tab = TabId(out["tab"].as_u64().unwrap());
    let hits = s
        .search_tab(tab)
        .unwrap()
        .notes
        .as_ref()
        .unwrap()
        .hits
        .iter()
        .map(|h| h.id.clone())
        .collect();
    (tab, hits)
}

#[test]
fn notes_search_finds_a_word_typed_a_moment_ago_newest_first_within_its_scope() {
    let f = Fixture::new("search");
    let mut s = f.session();
    let study = s
        .run("notebook.create", &json!({ "name": "Study" }))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let romans = note(&mut s, "Rom 13:10", "Love is the law", None);
    highlight(&mut s, "MAT", "neighbour");
    let free = note(&mut s, "", "Unanchored love", None);
    let lev = note(&mut s, "Lev 19:18", "Love thy neighbour", Some(&study));

    // Typed a moment ago, as a card saves while typing: found at once.
    tick();
    s.run(
        "annotation.edit",
        &json!({ "id": romans, "body": "Love is the law of Zerubbabel" }),
    )
    .unwrap();
    let (tab, hits) = notes_search(&mut s, json!({ "text": "zerubbabel" }));
    assert_eq!(hits, [romans.as_str()]);
    let hit = &s.search_tab(tab).unwrap().notes.as_ref().unwrap().hits[0];
    assert_eq!(hit.reference, "Romans 13:10");
    assert_eq!(
        &hit.snippet.text[hit.snippet.highlights[0].clone()],
        "Zerubbabel"
    );
    assert_eq!(
        s.search_tab(tab).unwrap().notes.as_ref().unwrap().summary(),
        "1 note"
    );

    // Newest change first; Highlights have no text and are not matched.
    let (_, hits) = notes_search(&mut s, json!({ "text": "love", "tab": tab.0 }));
    assert_eq!(hits, [romans.as_str(), lev.as_str(), free.as_str()]);
    // The #16 grammar: phrases, prefixes, exclusions.
    let (_, hits) = notes_search(&mut s, json!({ "text": "\"thy neighbour\"", "tab": tab.0 }));
    assert_eq!(hits, [lev.as_str()]);
    let (_, hits) = notes_search(&mut s, json!({ "text": "zerub* OR unanch*", "tab": tab.0 }));
    assert_eq!(hits, [romans.as_str(), free.as_str()]);
    let (_, hits) = notes_search(&mut s, json!({ "text": "love -law", "tab": tab.0 }));
    assert_eq!(hits, [lev.as_str(), free.as_str()]);

    // A Passage list leaves out other Passages and Notes without one.
    let (_, hits) = notes_search(
        &mut s,
        json!({ "text": "love", "tab": tab.0, "scope": "Lev; Matt" }),
    );
    assert_eq!(hits, [lev.as_str()]);
    let (_, hits) = notes_search(&mut s, json!({ "text": "love", "tab": tab.0, "scope": "" }));
    assert_eq!(hits.len(), 3);
    // The Notebook filter.
    let out = s
        .run("search.notebook", &json!({ "tab": tab.0, "value": study }))
        .unwrap();
    assert!(out["error"].is_null());
    let st = s.search_tab(tab).unwrap();
    assert_eq!(
        st.notes
            .as_ref()
            .unwrap()
            .hits
            .iter()
            .map(|h| &h.id)
            .collect::<Vec<_>>(),
        [&lev]
    );
    assert!(
        s.run("search.notebook", &json!({ "tab": tab.0, "value": "nope" }))
            .is_err()
    );
    s.run("search.notebook", &json!({ "tab": tab.0, "value": "all" }))
        .unwrap();
    assert_eq!(
        s.search_tab(tab)
            .unwrap()
            .notes
            .as_ref()
            .unwrap()
            .hits
            .len(),
        3
    );
    // Errors stay inline.
    let out = s
        .run("search.run", &json!({ "text": "-love", "tab": tab.0 }))
        .unwrap();
    assert_eq!(out["error"]["field"], "query");
    // Recent queries are kept for Notes mode.
    assert_eq!(s.recent_searches(SearchMode::Notes)[0], "love");
}

#[test]
fn long_bodies_are_cut_around_the_first_hit() {
    let f = Fixture::new("snippet");
    let mut s = f.session();
    let body = format!("{} needle {}", "word ".repeat(200), "after ".repeat(100));
    note(&mut s, "", &body, None);
    let (tab, _) = notes_search(&mut s, json!({ "text": "needle" }));
    let hit = &s.search_tab(tab).unwrap().notes.as_ref().unwrap().hits[0];
    let snip = &hit.snippet;
    assert!(snip.text.starts_with('…') && snip.text.ends_with('…'));
    assert!(snip.text.chars().count() <= crate::notes::SNIPPET_CHARS + 2);
    assert_eq!(&snip.text[snip.highlights[0].clone()], "needle");
}

#[test]
fn bible_references_in_bodies_are_standard_passages() {
    let found = body_references("See Rom 13:10 and Lev 19:18; Mt 5:43.");
    let passages: Vec<Vec<Passage>> = found.iter().map(|(_, l)| l.passages().to_vec()).collect();
    assert_eq!(
        passages,
        vec![
            vec![Passage::verse(v(Book::Romans, 13, 10))],
            vec![
                Passage::verse(v(Book::Leviticus, 19, 18)),
                Passage::verse(v(Book::Matthew, 5, 43))
            ],
        ]
    );
}
