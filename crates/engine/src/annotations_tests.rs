//! The annotation service (#61): selection → Anchor rules, Notebooks, notes_fts, and undo/redo
//! restoring the User database exactly.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

use obs_model::{
    Key, KeyScheme, Resource, ResourceBuilder, ResourceType, TextAccumulator, UnitId, UnitKind,
};
use obs_scripture::{Book, Passage, Standard, Verse, VerseMap, VerseMapEntry};
use serde_json::{Value, json};

use crate::annotations::{Segment, note_anchor};
use crate::*;

/// A temporary library with the fixture Bible and Commentary, and a User database.
struct Fixture {
    root: PathBuf,
    bible: Resource,
    commentary: Resource,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("obs-annotations-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("library")).unwrap();
        let bible = test_bible();
        let commentary = test_commentary();
        for r in [&bible, &commentary] {
            let path = root
                .join("library")
                .join(format!("{}.obr", r.manifest.resource_id));
            obs_store::write_obr(r, &path).unwrap();
        }
        Self {
            root,
            bible,
            commentary,
        }
    }

    fn session(&self) -> Session {
        let mut s = Session::new(JobMode::Inline);
        assert!(s.library.scan(&self.root.join("library")).is_empty());
        let (store, _) = obs_store::UserStore::open(&self.root.join("user.db")).unwrap();
        s.attach_user(Box::new(store));
        s
    }

    fn text(&self, resource: &str, unit: &str) -> String {
        let r = if resource == "test-lxx" {
            &self.bible
        } else {
            &self.commentary
        };
        r.units
            .iter()
            .find(|u| u.id.as_str() == unit)
            .unwrap()
            .text
            .clone()
    }

    /// Byte offset of `needle` in a unit (its end with `after`).
    fn at(&self, resource: &str, unit: &str, needle: &str, after: bool) -> usize {
        let text = self.text(resource, unit);
        let i = text
            .find(needle)
            .unwrap_or_else(|| panic!("{needle} in {unit}"));
        if after { i + needle.len() } else { i }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn manifest(id: &str, ty: ResourceType) -> obs_model::Manifest {
    let mut m = obs_store::sample_resource().manifest;
    m.resource_id = id.into();
    m.title = format!("Test {id}");
    m.abbreviation = id.to_uppercase();
    m.resource_type = ty;
    m
}

/// A Bible numbered partly like the Greek and Hebrew traditions:
/// - Esther 10:4 is a Greek addition: an Unmapped verse.
/// - Psalm 51 uses Hebrew numbering: 51:1-2 are the title (Standard 51:0), 51:3 is
///   Standard 51:1.
/// - John 3 starts with a heading outside any Verse; a footnote body sits between John and
///   Romans.
fn test_bible() -> Resource {
    let mut b = ResourceBuilder::new(manifest("test-lxx", ResourceType::Bible));
    let mut map = Vec::new();
    // (own chapter:verse, Standard chapter:verse, text); no own number: outside any Verse.
    type Piece<'a> = (Option<(u16, u16)>, Option<(u16, u16)>, &'a str);
    let mut add = |b: &mut ResourceBuilder, book: Book, pieces: &[Piece<'_>]| {
        let unit = UnitId::book(book);
        let mut t = TextAccumulator::new();
        let mut verses = Vec::new();
        for (native, standard, text) in pieces {
            let r = t.push(text);
            t.push(" ");
            if let Some((c, v)) = native {
                verses.push((r, Verse::new(book, *c, *v), *standard));
            }
        }
        b.add_unit(
            unit.clone(),
            UnitKind::BibleBook,
            Some(book.name().into()),
            t.into_string(),
        );
        for (r, verse, standard) in verses {
            b.add_verse(&unit, r, verse, verse.verse);
            map.push(VerseMapEntry {
                verse,
                last: verse.verse,
                standard: standard.map(|(c, v)| Passage::verse(Verse::new(book, c, v))),
            });
        }
    };
    add(
        &mut b,
        Book::Esther,
        &[
            (
                Some((10, 3)),
                Some((10, 3)),
                "For Mordecai the Jew was next unto king Ahasuerus, and great among the Jews.",
            ),
            (
                Some((10, 4)),
                None,
                "Then Mardocheus said, God hath done these things.",
            ),
        ],
    );
    add(
        &mut b,
        Book::Psalms,
        &[
            (
                Some((51, 1)),
                Some((51, 0)),
                "To the chief Musician, A Psalm of David,",
            ),
            (
                Some((51, 2)),
                Some((51, 0)),
                "when Nathan the prophet came unto him.",
            ),
            (
                Some((51, 3)),
                Some((51, 1)),
                "Have mercy upon me, O God, according to thy lovingkindness.",
            ),
            (
                Some((51, 4)),
                Some((51, 2)),
                "Wash me throughly from mine iniquity, and cleanse me from my sin.",
            ),
        ],
    );
    add(
        &mut b,
        Book::John,
        &[
            (None, None, "Jesus and Nicodemus"),
            (
                Some((3, 16)),
                Some((3, 16)),
                "For God so loved the world, that he gave his only begotten Son.",
            ),
            (
                Some((3, 17)),
                Some((3, 17)),
                "For God sent not his Son into the world to condemn the world.",
            ),
        ],
    );
    b.add_unit(UnitId::note(1), UnitKind::Note, None, "Or, only.".into());
    add(
        &mut b,
        Book::Romans,
        &[(
            Some((1, 1)),
            Some((1, 1)),
            "Paul, a servant of Jesus Christ, called to be an apostle.",
        )],
    );
    b.set_verse_map(VerseMap::new(map, Vec::new(), &Standard));
    b.build().unwrap()
}

fn test_commentary() -> Resource {
    let mut b = ResourceBuilder::new({
        let mut m = manifest("test-notes", ResourceType::Commentary);
        m.key_schemes = vec![KeyScheme::Passage];
        m
    });
    for (id, text) in [
        (
            "JHN.3.16",
            "The love of God is the source of salvation, given freely to all who believe.",
        ),
        ("JHN.3.17", "Christ came to save, not to condemn."),
    ] {
        let unit = UnitId::new(id).unwrap();
        b.add_unit(unit.clone(), UnitKind::Section, None, text.into());
        b.add_key(
            Key::Passage(Passage::verse(Verse::new(Book::John, 3, 16))),
            &unit,
            None,
            false,
        );
    }
    b.build().unwrap()
}

fn sel(resource: &str, unit: &str, start: usize, end_unit: &str, end: usize) -> Value {
    json!({
        "resource": resource,
        "start_unit": unit,
        "start": start,
        "end_unit": end_unit,
        "end": end,
    })
}

fn with(mut a: Value, b: Value) -> Value {
    for (k, v) in b.as_object().unwrap() {
        a[k] = v.clone();
    }
    a
}

fn dump(s: &Session) -> String {
    s.user.as_ref().unwrap().annotation_dump().unwrap()
}

fn std_verse(book: Book, c: u16, v: u16) -> Verse {
    Verse::new(book, c, v)
}

/// Check one stored Text range against the Resource text: quote at the offsets, up to 32
/// characters of context each side (fewer only at the unit's edge), and the content hash.
fn assert_captured(f: &Fixture, r: &TextRange) {
    let text = f.text(&r.resource_id, r.unit.as_str());
    let (s, e) = (r.start as usize, r.end as usize);
    assert!(s < e, "empty range {r:?}");
    assert_eq!(r.quote, text[s..e]);
    let before: Vec<char> = text[..s].chars().collect();
    let want_prefix: String = before[before.len().saturating_sub(32)..].iter().collect();
    let want_suffix: String = text[e..].chars().take(32).collect();
    assert_eq!(r.prefix, want_prefix, "prefix of {r:?}");
    assert_eq!(r.suffix, want_suffix, "suffix of {r:?}");
    let hash = if r.resource_id == "test-lxx" {
        &f.bible.manifest.content_hash
    } else {
        &f.commentary.manifest.content_hash
    };
    assert_eq!(&r.content_hash, hash);
    assert!(r.content_hash.starts_with("sha256:"));
}

fn highlight(s: &mut Session, params: Value) -> Vec<Annotation> {
    let out = s.run("annotation.highlight", &params).unwrap();
    out["ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| s.annotation(id.as_str().unwrap()).unwrap())
        .collect()
}

#[test]
fn a_selection_in_mapped_verses_gets_a_standard_passage_anchor_with_its_origin_range() {
    let f = Fixture::new("mapped");
    let mut s = f.session();
    // "mercy upon me" in Hebrew-numbered Ps 51:3 is Standard Ps 51:1.
    let start = f.at("test-lxx", "PSA", "mercy", false);
    let end = f.at("test-lxx", "PSA", "upon me", true);
    let made = highlight(
        &mut s,
        with(
            sel("test-lxx", "PSA", start, "PSA", end),
            json!({ "style": "green" }),
        ),
    );
    assert_eq!(made.len(), 1);
    let Some(Anchor::Passage { passage, origin }) = &made[0].anchor else {
        panic!("{:?}", made[0].anchor);
    };
    assert_eq!(*passage, Passage::verse(std_verse(Book::Psalms, 51, 1)));
    let origin = origin.as_ref().unwrap();
    assert_eq!(origin.quote, "mercy upon me");
    assert_eq!(origin.native_ref, None);
    assert_captured(&f, origin);
    assert_eq!(
        made[0].content,
        Content::Highlight {
            style: HighlightStyle::Green
        }
    );
    // The title verses 51:1-2 together are the Standard title slot.
    let start = f.at("test-lxx", "PSA", "To the chief", false);
    let end = f.at("test-lxx", "PSA", "unto him.", true);
    let made = highlight(&mut s, sel("test-lxx", "PSA", start, "PSA", end));
    assert_eq!(
        made[0].anchor.as_ref().unwrap().passage(),
        Some(Passage::verse(std_verse(Book::Psalms, 51, 0)))
    );
    // Reversed ends and surrounding whitespace make the same anchor.
    let a = f.at("test-lxx", "JHN", "For God so", false);
    let b = f.at("test-lxx", "JHN", "begotten Son.", true);
    let made = highlight(&mut s, sel("test-lxx", "JHN", b + 1, "JHN", a - 1));
    let range = made[0]
        .anchor
        .as_ref()
        .unwrap()
        .text_range()
        .unwrap()
        .clone();
    assert_eq!((range.start as usize, range.end as usize), (a, b));
    assert_eq!(
        made[0].anchor.as_ref().unwrap().passage(),
        Some(Passage::verse(std_verse(Book::John, 3, 16)))
    );
    drop(s);
}

#[test]
fn an_unmapped_verse_selection_gets_a_text_range_anchor_with_its_own_reference() {
    let f = Fixture::new("unmapped");
    let mut s = f.session();
    let start = f.at("test-lxx", "EST", "God hath", false);
    let end = f.at("test-lxx", "EST", "these things", true);
    let made = highlight(&mut s, sel("test-lxx", "EST", start, "EST", end));
    let Some(Anchor::TextRange { range }) = &made[0].anchor else {
        panic!("{:?}", made[0].anchor);
    };
    assert_eq!(range.native_ref.as_deref(), Some("Esth.10.4"));
    assert_eq!(range.quote, "God hath done these things");
    assert_captured(&f, range);
    // A Note there too.
    let out = s
        .run(
            "annotation.note",
            &with(
                sel("test-lxx", "EST", start, "EST", end),
                json!({ "body": "LXX" }),
            ),
        )
        .unwrap();
    let note = s.annotation(out["id"].as_str().unwrap()).unwrap();
    assert!(matches!(note.anchor, Some(Anchor::TextRange { .. })));
    // Text outside any Verse (a heading) is a Text-range anchor without a reference.
    let end = f.at("test-lxx", "JHN", "Nicodemus", true);
    let made = highlight(&mut s, sel("test-lxx", "JHN", 0, "JHN", end));
    let Some(Anchor::TextRange { range }) = &made[0].anchor else {
        panic!("{:?}", made[0].anchor);
    };
    assert_eq!(range.native_ref, None);
    // Across mapped and Unmapped verses, the mapped ones give the Passage.
    let start = f.at("test-lxx", "EST", "the Jews.", false);
    let end = f.at("test-lxx", "EST", "Mardocheus", true);
    let made = highlight(&mut s, sel("test-lxx", "EST", start, "EST", end));
    assert_eq!(
        made[0].anchor.as_ref().unwrap().passage(),
        Some(Passage::verse(std_verse(Book::Esther, 10, 3)))
    );
    drop(s);
}

#[test]
fn a_cross_book_selection_makes_one_highlight_per_book_in_one_undo_step() {
    let f = Fixture::new("crossbook");
    let mut s = f.session();
    let before = dump(&s);
    let start = f.at("test-lxx", "PSA", "Wash me", false);
    let end = f.at("test-lxx", "ROM", "Paul, a servant", true);
    let made = highlight(&mut s, sel("test-lxx", "PSA", start, "ROM", end));
    // Psalms, John and Romans; the footnote body between John and Romans is skipped.
    let passages: Vec<_> = made
        .iter()
        .map(|a| a.anchor.as_ref().unwrap().passage().unwrap())
        .collect();
    assert_eq!(
        passages,
        [
            Passage::verse(std_verse(Book::Psalms, 51, 2)),
            Passage::new(std_verse(Book::John, 3, 16), std_verse(Book::John, 3, 17)).unwrap(),
            Passage::verse(std_verse(Book::Romans, 1, 1)),
        ]
    );
    for a in &made {
        assert_captured(&f, a.anchor.as_ref().unwrap().text_range().unwrap());
    }
    assert_eq!(s.undo_history().undo_label(), Some("Highlights"));
    let after = dump(&s);
    s.run("edit.undo", &Value::Null).unwrap();
    assert_eq!(dump(&s), before);
    s.run("edit.redo", &Value::Null).unwrap();
    assert_eq!(dump(&s), after);

    // A Note on the same selection: one Passage anchor over it all, no Text range.
    let out = s
        .run(
            "annotation.note",
            &with(
                sel("test-lxx", "PSA", start, "ROM", end),
                json!({ "body": "x" }),
            ),
        )
        .unwrap();
    let note = s.annotation(out["id"].as_str().unwrap()).unwrap();
    assert_eq!(
        note.anchor,
        Some(Anchor::Passage {
            passage: Passage::new(
                std_verse(Book::Psalms, 51, 2),
                std_verse(Book::Romans, 1, 1)
            )
            .unwrap(),
            origin: None,
        })
    );
    drop(s);
}

#[test]
fn a_note_across_units_with_nothing_mapped_keeps_the_first_text_range() {
    let range = |unit: &str| TextRange {
        resource_id: "r".into(),
        unit: UnitId::new(unit).unwrap(),
        start: 0,
        end: 1,
        quote: "a".into(),
        prefix: String::new(),
        suffix: String::new(),
        content_hash: "sha256:0".into(),
        native_ref: None,
        whole_verses: false,
        orphaned: false,
    };
    let parts = [
        Segment {
            range: range("A"),
            standard: None,
            in_bible: false,
        },
        Segment {
            range: range("B"),
            standard: None,
            in_bible: false,
        },
    ];
    assert_eq!(
        note_anchor(&parts),
        Some(Anchor::TextRange { range: range("A") })
    );
    assert_eq!(note_anchor(&[]), None);
}

#[test]
fn non_bible_resources_get_text_range_anchors_and_count_as_referring() {
    let f = Fixture::new("commentary");
    let mut s = f.session();
    let start = f.at("test-notes", "JHN.3.16", "source", false);
    let end = f.at("test-notes", "JHN.3.17", "Christ came", true);
    let made = highlight(
        &mut s,
        sel("test-notes", "JHN.3.16", start, "JHN.3.17", end),
    );
    assert_eq!(made.len(), 2, "one Highlight per section");
    for a in &made {
        let Some(Anchor::TextRange { range }) = &a.anchor else {
            panic!("{:?}", a.anchor);
        };
        assert_captured(&f, range);
    }
    assert_eq!(s.annotations_referring("test-notes"), 2);
    let a = f.at("test-lxx", "JHN", "loved", false);
    highlight(&mut s, sel("test-lxx", "JHN", a, "JHN", a + 5));
    assert_eq!(s.annotations_referring("test-lxx"), 1);
    // The Remove confirmation states it.
    let out = s
        .run("library.remove", &json!({ "id": "test-notes" }))
        .unwrap();
    assert!(
        out["confirm"]
            .as_str()
            .unwrap()
            .contains("2 Annotations refer"),
        "{out}"
    );
    drop(s);
}

#[test]
fn notes_on_a_passage_or_on_nothing() {
    let f = Fixture::new("notes");
    let mut s = f.session();
    let out = s
        .run(
            "annotation.note",
            &json!({ "passage": "Jn 3:16-18", "body": "Gospel in a nutshell" }),
        )
        .unwrap();
    let note = s.annotation(out["id"].as_str().unwrap()).unwrap();
    assert_eq!(
        note.anchor,
        Some(Anchor::Passage {
            passage: Passage::new(std_verse(Book::John, 3, 16), std_verse(Book::John, 3, 18))
                .unwrap(),
            origin: None,
        })
    );
    let out = s
        .run("annotation.note", &json!({ "body": "Free thought" }))
        .unwrap();
    assert_eq!(out["annotation"]["anchor"], Value::Null);
    assert!(
        s.run("annotation.note", &json!({ "passage": "Jn 3:16; Rom 5:8" }))
            .is_err()
    );
    assert!(
        s.run(
            "annotation.note",
            &json!({ "passage": "Jn 3:16", "resource": "test-lxx" })
        )
        .is_err()
    );
    // Editing a Highlight's text or a Note's style is refused.
    let note_id = out["id"].as_str().unwrap();
    assert!(
        s.run(
            "annotation.style",
            &json!({ "id": note_id, "style": "blue" })
        )
        .is_err()
    );
    drop(s);
}

#[test]
fn notes_fts_matches_body_edits_immediately() {
    let f = Fixture::new("fts");
    let mut s = f.session();
    let out = s
        .run(
            "annotation.note",
            &json!({ "passage": "Jn 3:16", "body": "God’s Agápē love" }),
        )
        .unwrap();
    let id = out["id"].as_str().unwrap().to_string();
    let search = |s: &mut Session, text: &str| -> Vec<String> {
        let out = s
            .run("annotation.search_notes", &json!({ "text": text }))
            .unwrap();
        serde_json::from_value(out["ids"].clone()).unwrap()
    };
    assert_eq!(search(&mut s, "agape"), std::slice::from_ref(&id));
    assert_eq!(search(&mut s, "LOVE god’s"), std::slice::from_ref(&id));
    s.run(
        "annotation.edit",
        &json!({ "id": id, "body": "Mercy and grace" }),
    )
    .unwrap();
    assert!(search(&mut s, "agape").is_empty());
    assert_eq!(search(&mut s, "grace"), std::slice::from_ref(&id));
    s.run("edit.undo", &Value::Null).unwrap();
    assert_eq!(search(&mut s, "agape"), std::slice::from_ref(&id));
    assert!(search(&mut s, "grace").is_empty());
    s.run("edit.redo", &Value::Null).unwrap();
    assert_eq!(search(&mut s, "grace"), std::slice::from_ref(&id));
    // Words that are not FTS syntax stay words.
    assert!(search(&mut s, "\" OR NEAR( *").is_empty());
    assert!(search(&mut s, "   ").is_empty());
    s.run("annotation.delete", &json!({ "id": id })).unwrap();
    assert!(search(&mut s, "grace").is_empty());
    drop(s);
}

#[test]
fn the_default_notebook_stays_and_deleting_another_moves_or_deletes_its_annotations() {
    let f = Fixture::new("notebooks");
    let mut s = f.session();
    let default = s.default_notebook().unwrap();
    assert!(
        s.run("notebook.delete", &json!({ "id": default.id }))
            .is_err()
    );
    // The default can be renamed.
    s.run(
        "notebook.rename",
        &json!({ "id": default.id, "name": "Inbox" }),
    )
    .unwrap();
    let study = s
        .run("notebook.create", &json!({ "name": "Study" }))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        s.run("notebook.create", &json!({ "name": " study " }))
            .is_err(),
        "names are unique, ignoring case"
    );
    assert!(s.run("notebook.create", &json!({ "name": "  " })).is_err());
    let a = f.at("test-lxx", "JHN", "loved", false);
    for _ in 0..2 {
        highlight(
            &mut s,
            with(
                sel("test-lxx", "JHN", a, "JHN", a + 5),
                json!({ "notebook": study }),
            ),
        );
    }
    let list = s.run("notebook.list", &Value::Null).unwrap();
    assert_eq!(list[0]["name"], "Inbox");
    assert_eq!(list[0]["default"], true);
    assert_eq!(list[1]["count"], 2);

    // Moving to the default is the pre-selected choice.
    let before = dump(&s);
    let out = s.run("notebook.delete", &json!({ "id": study })).unwrap();
    assert_eq!(out["moved"], 2);
    assert_eq!(s.annotations(Some(&default.id)).unwrap().len(), 2);
    s.run("edit.undo", &Value::Null).unwrap();
    assert_eq!(dump(&s), before);
    let out = s
        .run(
            "notebook.delete",
            &json!({ "id": study, "annotations": "delete" }),
        )
        .unwrap();
    assert_eq!(out["deleted_annotations"], 2);
    assert!(s.annotations(None).unwrap().is_empty());
    assert_eq!(s.notebooks().unwrap().len(), 1);
    s.run("edit.undo", &Value::Null).unwrap();
    assert_eq!(dump(&s), before);
    assert!(
        s.run(
            "notebook.delete",
            &json!({ "id": study, "annotations": "keep" })
        )
        .is_err()
    );
    drop(s);
}

/// #18 rule 8: new Annotations go into the Notebook last used, remembered in the User
/// database across launches; a deleted one falls back to the default.
#[test]
fn new_annotations_go_into_the_notebook_last_used_across_launches() {
    let f = Fixture::new("last-notebook");
    let mut s = f.session();
    let default = s.default_notebook().unwrap();
    assert_eq!(s.last_notebook().unwrap(), default);
    let study = s
        .run("notebook.create", &json!({ "name": "Study" }))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let a = f.at("test-lxx", "JHN", "loved", false);
    // A rejected Highlight or Note does not change the last used Notebook.
    let missing = with(
        sel("no-such-resource", "JHN", a, "JHN", a + 5),
        json!({ "notebook": study, "style": "yellow" }),
    );
    assert!(s.run("annotation.highlight", &missing).is_err());
    let mut missing_note = missing.clone();
    missing_note["body"] = json!("x");
    assert!(s.run("annotation.note", &missing_note).is_err());
    assert_eq!(s.last_notebook().unwrap(), default);
    let picked = highlight(
        &mut s,
        with(
            sel("test-lxx", "JHN", a, "JHN", a + 5),
            json!({ "notebook": study }),
        ),
    );
    assert_eq!(picked[0].notebook_id, study);
    // Without a Notebook: the last used one, for Highlights and Notes alike.
    let next = highlight(&mut s, sel("test-lxx", "JHN", a, "JHN", a + 5));
    assert_eq!(next[0].notebook_id, study);
    let note = s.run("annotation.note", &json!({ "body": "x" })).unwrap();
    assert_eq!(note["annotation"]["notebook_id"], study.as_str());
    drop(s);
    // Remembered by the User database.
    let mut s = f.session();
    assert_eq!(s.last_notebook().unwrap().id, study);
    s.run(
        "notebook.delete",
        &json!({ "id": study, "annotations": "delete" }),
    )
    .unwrap();
    assert_eq!(s.last_notebook().unwrap().id, default.id);
    let note = s.run("annotation.note", &json!({ "body": "y" })).unwrap();
    assert_eq!(note["annotation"]["notebook_id"], default.id.as_str());
    drop(s);
}

/// Undo keys are bound; the Shell leaves them to a focused text field (#62).
#[test]
fn undo_and_redo_have_their_usual_shortcuts() {
    let s = Session::new(JobMode::Inline);
    let key = |id: &str| s.commands().get(id).unwrap().shortcut;
    assert_eq!(key("edit.undo"), Some("Ctrl+Z"));
    assert_eq!(key("edit.redo"), Some("Ctrl+Shift+Z"));
}

#[test]
fn undo_refuses_rows_that_changed_underneath_and_clears_the_history() {
    let f = Fixture::new("conflict");
    let mut s = f.session();
    let out = s.run("annotation.note", &json!({ "body": "one" })).unwrap();
    let id = out["id"].as_str().unwrap().to_string();
    // Something outside the history (later: re-anchoring) edits the row.
    let before = s.annotation(&id).unwrap();
    let after = Annotation {
        content: Content::Note { body: "two".into() },
        ..before.clone()
    };
    s.user
        .as_ref()
        .unwrap()
        .apply_changes(&[Change::update(
            Row::Annotation(before),
            Row::Annotation(after.clone()),
        )])
        .unwrap();
    let err = s.run("edit.undo", &Value::Null).unwrap_err();
    assert!(err.to_string().contains("changed since"), "{err}");
    assert_eq!(s.annotation(&id).unwrap(), after);
    assert!(!s.is_enabled("edit.undo"));
    assert!(!s.is_enabled("edit.redo"));
    drop(s);
}

/// A small deterministic generator, so failures reproduce.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
}

/// A random selection: ends on character boundaries, in one or two units.
fn random_selection(f: &Fixture, rng: &mut Rng) -> Value {
    let (resource, units): (&str, &[&str]) = if rng.below(4) == 0 {
        ("test-notes", &["JHN.3.16", "JHN.3.17"])
    } else {
        ("test-lxx", &["EST", "PSA", "JHN", "ROM"])
    };
    let a = rng.below(units.len());
    let b = (a + rng.below(2)).min(units.len() - 1);
    let pos = |rng: &mut Rng, unit: &str| {
        let text = f.text(resource, unit);
        let bounds: Vec<usize> = text
            .char_indices()
            .map(|(i, _)| i)
            .chain([text.len()])
            .collect();
        *rng.pick(&bounds)
    };
    let (s, e) = (pos(rng, units[a]), pos(rng, units[b]));
    sel(resource, units[a], s, units[b], e)
}

/// Every command is undoable and redoable to an identical database: a long random run of
/// every annotation and Notebook command, checking after each that Undo restores the dump
/// from before it and Redo the dump from after it; then undoing everything restores the
/// empty database and redoing everything the final one. Every Text range made along the way
/// has its quote and context.
#[test]
fn every_command_is_undoable_and_redoable_to_an_identical_database() {
    let f = Fixture::new("undo");
    let mut s = f.session();
    let mut rng = Rng(0x5EED_0061);
    let initial = dump(&s);
    let words = ["grace", "mercy", "love", "Agápē", "", "faith hope"];
    let styles = ["yellow", "green", "blue", "pink", "red_underline"];
    let mut steps = 0;
    let mut tried: std::collections::BTreeSet<&str> = Default::default();
    for _ in 0..400 {
        let notebooks: Vec<String> = s.notebooks().unwrap().into_iter().map(|n| n.id).collect();
        let annotations: Vec<Annotation> = s.annotations(None).unwrap();
        let some_annotation = |rng: &mut Rng| {
            if annotations.is_empty() {
                "none".to_string()
            } else {
                rng.pick(&annotations).id.clone()
            }
        };
        let (id, params) = match rng.below(11) {
            0 => (
                "notebook.create",
                json!({ "name": format!("Notebook {}", rng.below(1000)) }),
            ),
            1 => (
                "notebook.rename",
                json!({ "id": rng.pick(&notebooks), "name": format!("Renamed {}", rng.below(1000)) }),
            ),
            2 => (
                "notebook.delete",
                json!({ "id": rng.pick(&notebooks), "annotations": *rng.pick(&["move", "delete"]) }),
            ),
            3 | 4 => (
                "annotation.highlight",
                with(
                    random_selection(&f, &mut rng),
                    json!({ "style": rng.pick(&styles), "notebook": rng.pick(&notebooks) }),
                ),
            ),
            5 => (
                "annotation.note",
                match rng.below(3) {
                    0 => json!({ "body": rng.pick(&words), "passage": "Ps 51:1-3" }),
                    1 => json!({ "body": rng.pick(&words) }),
                    _ => with(
                        random_selection(&f, &mut rng),
                        json!({ "body": rng.pick(&words) }),
                    ),
                },
            ),
            6 => (
                "annotation.edit",
                json!({ "id": some_annotation(&mut rng), "body": rng.pick(&words) }),
            ),
            7 => (
                "annotation.style",
                json!({ "id": some_annotation(&mut rng), "style": rng.pick(&styles) }),
            ),
            8 => (
                "annotation.move",
                json!({ "id": some_annotation(&mut rng), "notebook": rng.pick(&notebooks) }),
            ),
            9 => (
                "annotation.delete",
                json!({ "id": some_annotation(&mut rng) }),
            ),
            _ => (
                "notebook.show_in_text",
                match rng.below(3) {
                    0 => json!({ "id": rng.pick(&notebooks) }),
                    1 => json!({ "id": rng.pick(&notebooks), "show": true }),
                    _ => json!({ "id": rng.pick(&notebooks), "show": false }),
                },
            ),
        };
        let before = dump(&s);
        let label_before = s.undo_history().undo_label().map(str::to_string);
        let ok = s.run(id, &params).is_ok();
        let after = dump(&s);
        if !ok || after == before {
            // Refused or a no-op: nothing written and no undo step added.
            assert_eq!(after, before, "{id} {params} failed but wrote");
            assert_eq!(
                s.undo_history().undo_label().map(str::to_string),
                label_before
            );
            continue;
        }
        tried.insert(id);
        steps += 1;
        s.run("edit.undo", &Value::Null).unwrap();
        assert_eq!(dump(&s), before, "undo of {id} {params}");
        s.run("edit.redo", &Value::Null).unwrap();
        assert_eq!(dump(&s), after, "redo of {id} {params}");
    }
    for a in s.annotations(None).unwrap() {
        if let Some(r) = a.anchor.as_ref().and_then(Anchor::text_range) {
            assert_captured(&f, r);
        }
        assert_eq!(a.problem(), None);
    }
    let last = dump(&s);
    assert!(steps > 150, "only {steps} changing steps");
    for id in [
        "notebook.create",
        "notebook.rename",
        "notebook.delete",
        "annotation.highlight",
        "annotation.note",
        "annotation.edit",
        "annotation.style",
        "annotation.move",
        "annotation.delete",
        "notebook.show_in_text",
    ] {
        assert!(tried.contains(id), "{id} never changed anything");
    }
    let mut undone = 0;
    while s.is_enabled("edit.undo") {
        s.run("edit.undo", &Value::Null).unwrap();
        undone += 1;
    }
    assert_eq!(undone, steps);
    assert_eq!(dump(&s), initial);
    while s.is_enabled("edit.redo") {
        s.run("edit.redo", &Value::Null).unwrap();
    }
    assert_eq!(dump(&s), last);
    drop(s);
}

/// The hostile-params test again, now with a User database and Resources, so the annotation
/// commands get past "no User database" and must reject junk without panicking or leaving
/// the tables inconsistent.
#[test]
fn hostile_params_never_panic_with_a_user_database() {
    let f = Fixture::new("hostile");
    let mut s = f.session();
    s.run("annotation.note", &json!({ "body": "seed" }))
        .unwrap();
    let junk = vec![
        Value::Null,
        json!(true),
        json!(-1),
        json!(i64::MAX),
        json!(""),
        json!("\u{0}"),
        json!("test-lxx"),
        json!("JHN"),
        json!("Jn 3:16"),
        json!("\u{202E}\u{FEFF}\u{10FFFF}"),
        json!("a".repeat(5000)),
        json!([1]),
        json!({}),
    ];
    let ids: Vec<&'static str> = s
        .commands()
        .iter()
        .map(|c| c.id)
        .filter(|id| {
            id.starts_with("annotation.") || id.starts_with("notebook.") || id.starts_with("edit.")
        })
        .collect();
    let mut runs = 0;
    for id in ids {
        let spec = *s.commands().get(id).unwrap();
        let mut cases = junk.clone();
        for v in &junk {
            for p in spec.params {
                cases.push(json!({ p.name: v }));
            }
            let all: serde_json::Map<String, Value> = spec
                .params
                .iter()
                .map(|q| (q.name.to_string(), v.clone()))
                .collect();
            cases.push(Value::Object(all));
        }
        // Selections with every byte offset kind of wrong.
        for (start, end) in [(0, 0), (1, 2), (-1, 3), (i64::MAX, 0), (3, 100_000)] {
            cases.push(json!({
                "resource": "test-lxx", "start_unit": "EST", "start": start,
                "end_unit": "EST", "end": end, "id": "x", "name": "n",
            }));
        }
        for params in cases {
            let result = catch_unwind(AssertUnwindSafe(|| s.run(id, &params)));
            assert!(result.is_ok(), "`{id}` panicked with {params}");
            runs += 1;
        }
    }
    assert!(runs > 300, "only {runs} runs");
    // Whatever was accepted left consistent tables, and the history still works.
    dump(&s);
    while s.is_enabled("edit.undo") {
        s.run("edit.undo", &Value::Null).unwrap();
    }
    assert!(s.annotations(None).unwrap().is_empty());
    drop(s);
}

#[test]
fn empty_or_unknown_selections_are_refused() {
    let f = Fixture::new("bytes");
    let mut s = f.session();
    let err = s
        .run(
            "annotation.highlight",
            &sel("test-lxx", "JHN", 0, "NOPE", 3),
        )
        .unwrap_err();
    assert!(err.to_string().contains("no text unit"), "{err}");
    let err = s
        .run("annotation.highlight", &sel("test-lxx", "JHN", 5, "JHN", 5))
        .unwrap_err();
    assert!(err.to_string().contains("Select some text"), "{err}");
    drop(s);
}

/// Timing on the real Starter library (skipped when it is absent): a word Highlight and a
/// cross-book Highlight in the KJV, and undo. Run with
/// `cargo test -p obs-engine --release real_library -- --ignored --nocapture`.
#[test]
#[ignore = "needs the Starter library in the platform data folder"]
fn real_library_highlight_timing() {
    let Some(lib) = default_library_dir().filter(|d| d.join("kjv-strongs.obr").is_file()) else {
        eprintln!("no Starter library; skipped");
        return;
    };
    let root = std::env::temp_dir().join(format!("obs-annotations-real-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let mut s = Session::new(JobMode::Inline);
    s.library.scan(&lib);
    let (store, _) = obs_store::UserStore::open(&root.join("user.db")).unwrap();
    s.attach_user(Box::new(store));
    let t = std::time::Instant::now();
    let out = s
        .run(
            "annotation.highlight",
            &sel("kjv-strongs", "JHN", 1000, "JHN", 1040),
        )
        .unwrap();
    eprintln!(
        "word Highlight: {:?} {}",
        t.elapsed(),
        out["annotations"][0]["anchor"]
    );
    let t = std::time::Instant::now();
    let out = s
        .run(
            "annotation.highlight",
            &sel("kjv-strongs", "MAL", 100, "MAT", 100),
        )
        .unwrap();
    eprintln!(
        "cross-book Highlight: {:?} ({} Highlights)",
        t.elapsed(),
        out["ids"].as_array().unwrap().len()
    );
    let t = std::time::Instant::now();
    s.run("edit.undo", &Value::Null).unwrap();
    eprintln!("undo: {:?}", t.elapsed());
    drop(s);
    let _ = std::fs::remove_dir_all(&root);
}

/// #63: whether a Bible Text range covers its Verses whole decides how other Bibles show it
/// (Verses highlighted, or only a margin mark). White space and punctuation at a Verse's
/// ends do not count; a Text-range anchor never covers Verses.
#[test]
fn a_bible_text_range_records_whether_it_covers_its_verses_whole() {
    let f = Fixture::new("whole");
    let mut s = f.session();
    let whole = |s: &mut Session, unit: &str, from: &str, to: &str| {
        let a = f.at("test-lxx", unit, from, false);
        let b = f.at("test-lxx", unit, to, true);
        let made = highlight(s, sel("test-lxx", unit, a, unit, b));
        assert_eq!(made.len(), 1);
        made[0]
            .anchor
            .as_ref()
            .unwrap()
            .text_range()
            .unwrap()
            .whole_verses
    };
    // Jn 3:16 from its first word to its last, with or without the full stop.
    assert!(whole(&mut s, "JHN", "For God so", "begotten Son."));
    assert!(whole(&mut s, "JHN", "For God so", "begotten Son"));
    // Jn 3:16-17 whole, and the same selection one word short at either end.
    assert!(whole(&mut s, "JHN", "For God so", "condemn the world."));
    assert!(!whole(&mut s, "JHN", "God so loved", "condemn the world."));
    assert!(!whole(&mut s, "JHN", "For God so", "condemn the"));
    // One word of Hebrew-numbered Ps 51:3.
    assert!(!whole(&mut s, "PSA", "mercy", "mercy"));
    // A Text-range anchor (the Unmapped Esth 10:4) never counts as whole Verses.
    assert!(!whole(
        &mut s,
        "EST",
        "Then Mardocheus",
        "done these things."
    ));
    drop(s);
}

/// #63: each Notebook's Show in text, and View → Show Highlights, decide what
/// `annotations_in_text` hands the reading panes. Both are kept in the User database; Show
/// in text is undoable like any Notebook change, and every change moves the revision views
/// reload on.
#[test]
fn show_in_text_and_show_highlights_filter_what_is_drawn_and_persist() {
    let f = Fixture::new("show");
    let mut s = f.session();
    let study = s.create_notebook("Study").unwrap();
    let a = f.at("test-lxx", "JHN", "For God", false);
    let b = f.at("test-lxx", "JHN", "loved", true);
    let lit = highlight(&mut s, sel("test-lxx", "JHN", a, "JHN", b));
    let note = s
        .run(
            "annotation.note",
            &json!({ "body": "hidden later", "passage": "Jn 3:16", "notebook": study.id }),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let ids = |s: &Session| -> Vec<String> {
        s.annotations_in_text()
            .unwrap()
            .into_iter()
            .map(|a| a.id)
            .collect()
    };
    assert_eq!(ids(&s), vec![lit[0].id.clone(), note.clone()]);

    // Hide the Study Notebook: its Note is no longer drawn, but it is still listed.
    let rev = s.annotations_revision();
    let out = s
        .run("notebook.show_in_text", &json!({ "id": study.id }))
        .unwrap();
    assert_eq!(out["notebook"]["show_in_text"], json!(false));
    assert!(s.annotations_revision() > rev);
    assert_eq!(ids(&s), vec![lit[0].id.clone()]);
    assert_eq!(s.annotations(None).unwrap().len(), 2);
    let listed = s.run("notebook.list", &Value::Null).unwrap();
    let row = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == json!(study.id))
        .unwrap()
        .clone();
    assert_eq!(row["show_in_text"], json!(false));
    // An explicit value that is already set changes nothing and adds no undo step.
    s.run(
        "notebook.show_in_text",
        &json!({ "id": study.id, "show": false }),
    )
    .unwrap();
    assert_eq!(s.undo_history().undo_label(), Some("Hide in text"));
    // Undo shows it again; redo hides it.
    s.run("edit.undo", &Value::Null).unwrap();
    assert_eq!(ids(&s).len(), 2);
    s.run("edit.redo", &Value::Null).unwrap();
    assert_eq!(ids(&s), vec![lit[0].id.clone()]);

    // View → Show Highlights off: Highlights go, Notes in shown Notebooks stay.
    s.run(
        "notebook.show_in_text",
        &json!({ "id": study.id, "show": true }),
    )
    .unwrap();
    let rev = s.annotations_revision();
    s.run("view.highlights", &Value::Null).unwrap();
    assert!(!s.appearance.show_highlights);
    assert!(s.annotations_revision() > rev);
    assert_eq!(ids(&s), vec![note.clone()]);
    s.run(
        "notebook.show_in_text",
        &json!({ "id": study.id, "show": false }),
    )
    .unwrap();
    drop(s);

    // Both are remembered by the User database.
    let s = f.session();
    assert!(!s.appearance.show_highlights);
    assert!(
        !s.notebooks()
            .unwrap()
            .iter()
            .find(|n| n.id == study.id)
            .unwrap()
            .show_in_text
    );
    assert!(ids(&s).is_empty());
    drop(s);
}
