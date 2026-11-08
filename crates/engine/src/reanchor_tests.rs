//! Re-anchoring (#65, ADR 0006) through the Session: fixture revisions written as real
//! `.obr` files, a real User database, and the library-load job.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use obs_model::{
    Manifest, ParaStyle, Resource, ResourceBuilder, ResourceType, SpanKind, UnitId, UnitKind,
};
use obs_scripture::Book;
use serde_json::json;

use crate::testing::bibles::{bible, v};
use crate::*;

/// A library folder and a User database in a temp folder.
struct Lib {
    root: PathBuf,
}

impl Lib {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("obs-reanchor-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("library")).unwrap();
        Self { root }
    }

    fn path(&self, r: &Resource) -> PathBuf {
        self.root
            .join("library")
            .join(format!("{}.obr", r.manifest.resource_id))
    }

    /// Put (or replace) a Resource revision in the library folder.
    fn write(&self, r: &Resource) {
        obs_store::write_obr(r, &self.path(r)).unwrap();
    }

    fn remove(&self, r: &Resource) {
        std::fs::remove_file(self.path(r)).unwrap();
    }

    /// Launch: scan the library, open the User database (which re-anchors, in the
    /// background), and finish the background work.
    fn launch(&self) -> Session {
        let mut s = self.launch_pending();
        settle(&mut s);
        s
    }

    /// Launch without running the background work yet.
    fn launch_pending(&self) -> Session {
        let mut s = Session::new(JobMode::Inline);
        assert!(s.library.scan(&self.root.join("library")).is_empty());
        let (store, _) = obs_store::UserStore::open(&self.root.join("user.db")).unwrap();
        s.attach_user(Box::new(store));
        s
    }
}

impl Drop for Lib {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Run queued jobs and apply their results.
fn settle(s: &mut Session) {
    for _ in 0..20 {
        s.jobs().pump(64);
        let _ = s.poll_imports();
        if !s.reanchoring.is_busy() {
            return;
        }
    }
    panic!("re-anchoring did not finish");
}

const GEN_1: [&str; 3] = [
    "In the beginnning God created the heaven and the earth.",
    "And the earth was without form, and void; and darkness was upon the face of the deep.",
    "And God said, Let there be light: and there was light.",
];

/// A Bible revision with Genesis 1:1-3 (open text).
fn tb(verses: [&str; 3], revision: u32) -> Resource {
    let list: Vec<_> = (1..=3)
        .map(|n| (v(Book::Genesis, 1, n), verses[n as usize - 1]))
        .collect();
    let mut r = bible("tb", "TB", &list, None, &[]);
    r.manifest.revision = revision;
    r
}

/// A Commentary revision: one unit per `(id, text)`, with `aliases` (old → new unit id).
fn notes_on(units: &[(&str, &str)], aliases: &[(&str, &str)], revision: u32) -> Resource {
    let mut m = Manifest::new(
        "tcm",
        "Test Commentary",
        "TCM",
        ResourceType::Commentary,
        "en",
    );
    m.license = "CC0-1.0".into();
    m.revision = revision;
    let mut b = ResourceBuilder::new(m);
    for (id, text) in units {
        let unit = UnitId::new(*id).unwrap();
        b.add_unit(unit.clone(), UnitKind::Section, None, (*text).to_string());
        b.add_span(
            &unit,
            0..text.len(),
            SpanKind::Para {
                style: ParaStyle::Prose,
            },
        );
    }
    for (old, new) in aliases {
        b.add_unit_alias(UnitId::new(*old).unwrap(), UnitId::new(*new).unwrap());
    }
    b.build().unwrap()
}

fn unit_text(r: &Resource, unit: &str) -> String {
    r.units
        .iter()
        .find(|u| u.id.as_str() == unit)
        .unwrap()
        .text
        .clone()
}

/// Highlight the `nth` occurrence of `quote` in a unit; returns its id.
fn highlight(s: &mut Session, r: &Resource, unit: &str, quote: &str, nth: usize) -> String {
    let text = unit_text(r, unit);
    let at = text.match_indices(quote).nth(nth).unwrap().0;
    let out = s
        .run(
            "annotation.highlight",
            &json!({
                "resource": r.manifest.resource_id,
                "start_unit": unit,
                "start": at,
                "end": at + quote.len(),
            }),
        )
        .unwrap();
    out["ids"][0].as_str().unwrap().to_string()
}

/// A Note on the first `quote` in a unit; returns its id.
fn note(s: &mut Session, r: &Resource, unit: &str, quote: &str, body: &str) -> String {
    let text = unit_text(r, unit);
    let at = text.find(quote).unwrap();
    let out = s
        .run(
            "annotation.note",
            &json!({
                "resource": r.manifest.resource_id,
                "start_unit": unit,
                "start": at,
                "end": at + quote.len(),
                "body": body,
            }),
        )
        .unwrap();
    out["id"].as_str().unwrap().to_string()
}

fn range(s: &Session, id: &str) -> Option<TextRange> {
    s.annotation(id)
        .unwrap()
        .anchor
        .and_then(|a| a.text_range().cloned())
}

/// The text a stored range points at in a Resource revision.
fn pointed(r: &Resource, range: &TextRange) -> String {
    unit_text(r, range.unit.as_str())[range.start as usize..range.end as usize].to_string()
}

#[test]
fn a_typo_fixed_elsewhere_moves_only_the_annotations_after_it() {
    let lib = Lib::new("typo");
    let old = tb(GEN_1, 1);
    lib.write(&old);
    let (before, after, later) = {
        let mut s = lib.launch();
        (
            highlight(&mut s, &old, "GEN", "In the", 0),
            highlight(&mut s, &old, "GEN", "God created", 0),
            highlight(&mut s, &old, "GEN", "darkness", 0),
        )
    };
    let mut fixed = GEN_1;
    fixed[0] = "In the beginning God created the heaven and the earth.";
    let new = tb(fixed, 2);
    lib.write(&new);
    let s = lib.launch();
    let report = s.reanchoring.report("tb").unwrap();
    assert_eq!(
        (
            report.unchanged,
            report.moved,
            report.orphaned,
            report.whole_verses
        ),
        (1, 2, 0, 0),
        "{report:?}"
    );
    assert!(report.message().unwrap().contains("2 Annotations moved"));
    let r = range(&s, &before).unwrap();
    assert_eq!((r.start, pointed(&new, &r).as_str()), (0, "In the"));
    for (id, quote) in [(&after, "God created"), (&later, "darkness")] {
        let r = range(&s, id).unwrap();
        let was = range_at(&old, quote);
        assert_eq!(r.start + 1, was, "{quote} moved back one byte");
        assert_eq!(pointed(&new, &r), quote);
        assert_eq!(r.content_hash, new.manifest.content_hash);
        assert!(!r.orphaned);
    }
    // Still a Passage anchor on the same Standard Verse.
    let a = s.annotation(&after).unwrap();
    assert_eq!(
        a.anchor.unwrap().passage().unwrap().start(),
        v(Book::Genesis, 1, 1)
    );
}

fn range_at(r: &Resource, quote: &str) -> u32 {
    unit_text(r, "GEN").find(quote).unwrap() as u32
}

#[test]
fn an_inserted_sentence_moves_the_annotation_with_its_text() {
    let lib = Lib::new("insert");
    let text = "Moses wrote this book. It begins with the creation of the world.";
    let old = notes_on(&[("intro", text)], &[], 1);
    lib.write(&old);
    let id = {
        let mut s = lib.launch();
        note(
            &mut s,
            &old,
            "intro",
            "begins with the creation",
            "Creation first",
        )
    };
    let inserted = format!("Some date it late; we follow tradition here. {text}");
    let new = notes_on(&[("intro", &inserted)], &[], 2);
    lib.write(&new);
    let s = lib.launch();
    let report = s.reanchoring.report("tcm").unwrap();
    assert_eq!((report.moved, report.orphaned), (1, 0), "{report:?}");
    let r = range(&s, &id).unwrap();
    assert_eq!(pointed(&new, &r), "begins with the creation");
    assert_eq!(
        r.start as usize,
        inserted.find("begins with the creation").unwrap()
    );
    assert_eq!(s.annotation(&id).unwrap().body(), "Creation first");
}

#[test]
fn a_deleted_quote_orphans_a_text_range_and_shows_a_passage_anchor_on_whole_verses() {
    let lib = Lib::new("delete");
    let old_tb = tb(GEN_1, 1);
    let old_tcm = notes_on(
        &[(
            "intro",
            "Moses wrote this book. It begins with the creation of the world.",
        )],
        &[],
        1,
    );
    lib.write(&old_tb);
    lib.write(&old_tcm);
    let (bible_hl, commentary_note) = {
        let mut s = lib.launch();
        (
            highlight(&mut s, &old_tb, "GEN", "and void", 0),
            note(&mut s, &old_tcm, "intro", "Moses wrote", "Authorship"),
        )
    };
    let mut edited = GEN_1;
    edited[1] = "And the earth was without form; and darkness was upon the face of the deep.";
    lib.write(&tb(edited, 2));
    lib.write(&notes_on(
        &[("intro", "It begins with the creation of the world.")],
        &[],
        2,
    ));
    let s = lib.launch();
    // The Passage anchor keeps its Verse and shows as whole Verses; its Text range is kept
    // (quote, context, Resource, hash), marked not found, never dropped (ADR 0006).
    let a = s.annotation(&bible_hl).unwrap();
    match a.anchor.as_ref().unwrap() {
        Anchor::Passage { passage, origin } => {
            assert_eq!(passage.start(), v(Book::Genesis, 1, 2));
            let origin = origin.as_ref().expect("the origin is kept");
            assert!(origin.orphaned);
            assert_eq!(origin.quote, "and void");
            assert_eq!(origin.resource_id, "tb");
            assert_eq!(origin.content_hash, old_tb.manifest.content_hash);
        }
        other => panic!("{other:?}"),
    }
    assert!(!a.is_orphaned(), "not listed under Needs attention");
    let card = s
        .note_cards("")
        .unwrap()
        .into_iter()
        .find(|c| c.annotation.id == bible_hl)
        .unwrap();
    assert_eq!(
        card.quote.as_deref(),
        Some("and void"),
        "the card keeps its quote"
    );
    assert!(card.attention.is_none() && card.followable());
    assert_eq!(s.reanchoring.report("tb").unwrap().whole_verses, 1);
    // The Text-range anchor is orphaned, keeping its quote.
    let n = s.annotation(&commentary_note).unwrap();
    assert!(n.is_orphaned());
    let r = range(&s, &commentary_note).unwrap();
    assert_eq!(r.quote, "Moses wrote");
    assert_eq!(n.body(), "Authorship");
    let report = s.reanchoring.report("tcm").unwrap();
    assert_eq!(report.orphaned, 1);
    assert!(
        report.message().unwrap().contains("1 needs attention"),
        "{report:?}"
    );
    assert_eq!(s.orphaned_annotations().unwrap().len(), 1);
    // Nothing is planned again at the next launch.
    drop(s);
    let s = lib.launch_pending();
    assert!(!s.reanchoring.is_busy());
    drop(s);
    // The old revision comes back: the kept range is found again, exact to the word.
    lib.write(&old_tb);
    let s = lib.launch();
    let report = s.reanchoring.report("tb").unwrap();
    assert_eq!((report.moved, report.whole_verses), (1, 0), "{report:?}");
    let r = range(&s, &bible_hl).unwrap();
    assert!(!r.orphaned);
    assert_eq!(pointed(&old_tb, &r), "and void");
    assert_eq!(r.content_hash, old_tb.manifest.content_hash);
}

#[test]
fn a_moved_range_gets_its_coverage_and_own_reference_worked_out_again() {
    let lib = Lib::new("coverage");
    let esth = |v4: &str, v5: &str, revision: u32| {
        let list = [
            (
                v(Book::Esther, 10, 3),
                "For Mardocheus was next unto the king.",
            ),
            (v(Book::Esther, 10, 4), v4),
            (v(Book::Esther, 10, 5), v5),
        ];
        // 10:4 and 10:5 are Unmapped (as in the LXX's additions).
        let standard = [Some(v(Book::Esther, 10, 3)), None, None];
        let mut r = bible("lxx", "LXX", &list, Some(&standard), &[]);
        r.manifest.revision = revision;
        r
    };
    let old_lxx = esth(
        "Then Mardocheus said, God hath done these things.",
        "For I remember a dream.",
        1,
    );
    let old_tb = tb(GEN_1, 1);
    lib.write(&old_lxx);
    lib.write(&old_tb);
    let unit = old_lxx.units[0].id.as_str().to_string();
    let (unmapped, whole) = {
        let mut s = lib.launch();
        (
            highlight(&mut s, &old_lxx, &unit, "God hath done these things", 0),
            highlight(&mut s, &old_tb, "GEN", GEN_1[2], 0),
        )
    };
    let mut s = lib.launch();
    assert_eq!(
        range(&s, &unmapped).unwrap().native_ref.as_deref(),
        Some("Esth.10.4")
    );
    assert!(range(&s, &whole).unwrap().whole_verses);
    drop(s);
    // The words move to 10:5; Genesis 1:3 grows, so the Highlight covers only part of it.
    let new_lxx = esth(
        "Then Mardocheus said, I remember.",
        "For God hath done these things, and a dream.",
        2,
    );
    let mut grown = GEN_1;
    grown[2] = "And God said, Let there be light: and there was light. And it was so.";
    let new_tb = tb(grown, 2);
    lib.write(&new_lxx);
    lib.write(&new_tb);
    s = lib.launch();
    let r = range(&s, &unmapped).unwrap();
    assert_eq!(pointed(&new_lxx, &r), "God hath done these things");
    assert_eq!(
        r.native_ref.as_deref(),
        Some("Esth.10.5"),
        "label and Bible order"
    );
    let card = s
        .note_cards("")
        .unwrap()
        .into_iter()
        .find(|c| c.annotation.id == unmapped)
        .unwrap();
    assert!(card.reference.contains("10:5"), "{}", card.reference);
    let r = range(&s, &whole).unwrap();
    assert_eq!(pointed(&new_tb, &r), GEN_1[2]);
    assert!(
        !r.whole_verses,
        "other Bibles show a margin mark, not the whole Verse"
    );
}

#[test]
fn undo_bringing_back_a_deleted_annotation_reanchors_it() {
    let lib = Lib::new("undo-delete");
    let old = tb(GEN_1, 1);
    lib.write(&old);
    let mut s = lib.launch();
    let id = highlight(&mut s, &old, "GEN", "darkness", 0);
    let kept = s
        .run("notebook.create", &json!({ "name": "Study" }))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let other = highlight(&mut s, &old, "GEN", "light", 0);
    s.run("annotation.move", &json!({ "id": other, "notebook": kept }))
        .unwrap();
    s.run("annotation.delete", &json!({ "id": id })).unwrap();
    s.run(
        "notebook.delete",
        &json!({ "id": kept, "annotations": "delete" }),
    )
    .unwrap();
    assert!(s.annotations(None).unwrap().is_empty());
    // A new revision arrives while they are deleted: nothing to re-anchor.
    let mut fixed = GEN_1;
    fixed[0] = "In the beginning God created the heaven and the earth.";
    let new = tb(fixed, 2);
    lib.write(&new);
    s.library.add_file(&lib.path(&new)).unwrap();
    assert!(!s.reanchor_resource("tb"));
    // Undo the Notebook delete, then the Annotation delete: both come back made against the
    // old revision, and are re-anchored; until then their Marks wait.
    for (label, id, quote) in [
        ("Delete Notebook", &other, "light"),
        ("Delete Highlight", &id, "darkness"),
    ] {
        assert_eq!(s.undo().unwrap().as_deref(), Some(label));
        assert!(s.is_reanchoring("tb"), "{label}: re-anchoring queued");
        settle(&mut s);
        let r = range(&s, id).unwrap();
        assert_eq!(r.content_hash, new.manifest.content_hash, "{label}");
        assert_eq!(pointed(&new, &r), quote, "{label}");
    }
    // Redo and undo still land on the moved rows.
    assert_eq!(s.redo().unwrap().as_deref(), Some("Delete Highlight"));
    assert!(s.annotation(&id).is_err());
    assert!(s.undo().unwrap().is_some());
    assert!(!s.is_reanchoring("tb"), "already up to date");
    assert_eq!(pointed(&new, &range(&s, &id).unwrap()), "darkness");
}

#[test]
fn a_renamed_unit_is_followed_through_its_unit_alias() {
    let lib = Lib::new("alias");
    let old = notes_on(
        &[
            ("gen-intro", "Genesis is the book of beginnings."),
            ("exo-intro", "Exodus tells of the way out of Egypt."),
        ],
        &[],
        1,
    );
    lib.write(&old);
    let id = {
        let mut s = lib.launch();
        highlight(&mut s, &old, "gen-intro", "book of beginnings", 0)
    };
    let new = notes_on(
        &[
            ("GEN.intro", "Genesis is the book of beginnings."),
            ("EXO.intro", "Exodus tells of the way out of Egypt."),
        ],
        &[("gen-intro", "GEN.intro"), ("exo-intro", "EXO.intro")],
        2,
    );
    lib.write(&new);
    let s = lib.launch();
    let r = range(&s, &id).unwrap();
    assert_eq!(r.unit.as_str(), "GEN.intro");
    assert_eq!(pointed(&new, &r), "book of beginnings");
    assert!(!r.orphaned);
    assert_eq!(s.reanchoring.report("tcm").unwrap().moved, 1);
}

#[test]
fn a_renamed_unit_without_an_alias_orphans_its_annotations() {
    let lib = Lib::new("no-alias");
    let old = notes_on(
        &[("gen-intro", "Genesis is the book of beginnings.")],
        &[],
        1,
    );
    lib.write(&old);
    let id = {
        let mut s = lib.launch();
        highlight(&mut s, &old, "gen-intro", "beginnings", 0)
    };
    lib.write(&notes_on(
        &[("GEN.intro", "Genesis is the book of beginnings.")],
        &[],
        2,
    ));
    let s = lib.launch();
    assert!(s.annotation(&id).unwrap().is_orphaned());
}

#[test]
fn spacing_and_punctuation_changes_are_matched_ignoring_them() {
    let lib = Lib::new("loose");
    let old = tb(GEN_1, 1);
    lib.write(&old);
    let id = {
        let mut s = lib.launch();
        highlight(&mut s, &old, "GEN", "Let there be light: and", 0)
    };
    let mut edited = GEN_1;
    edited[2] = "And God said, “Let there be light,” and there was light.";
    let new = tb(edited, 2);
    lib.write(&new);
    let s = lib.launch();
    let r = range(&s, &id).unwrap();
    assert_eq!(pointed(&new, &r), "Let there be light,” and");
    assert_eq!(s.reanchoring.report("tb").unwrap().moved, 1);
}

#[test]
fn marks_wait_for_reanchoring_and_unchanged_resources_queue_nothing() {
    let lib = Lib::new("gate");
    let old = tb(GEN_1, 1);
    lib.write(&old);
    lib.write(&notes_on(&[("intro", "Unchanged text.")], &[], 1));
    {
        let mut s = lib.launch();
        highlight(&mut s, &old, "GEN", "God", 0);
        let tcm = notes_on(&[("intro", "Unchanged text.")], &[], 1);
        highlight(&mut s, &tcm, "intro", "Unchanged", 0);
    }
    let mut fixed = GEN_1;
    fixed[0] = "In the beginning God created the heaven and the earth.";
    lib.write(&tb(fixed, 2));
    let mut s = lib.launch_pending();
    assert!(s.is_reanchoring("tb"), "TB waits for its job");
    assert!(!s.is_reanchoring("tcm"), "TCM did not change");
    let revision = s.annotations_revision();
    settle(&mut s);
    assert!(!s.is_reanchoring("tb"));
    assert!(s.annotations_revision() > revision, "views reload");
    assert!(s.reanchoring.report("tcm").is_none());
}

#[test]
fn a_removed_resource_keeps_its_annotations_dormant_and_a_reimport_brings_them_back() {
    let lib = Lib::new("removed");
    let old = notes_on(
        &[("intro", "Moses wrote this book. It begins with creation.")],
        &[],
        1,
    );
    lib.write(&old);
    let id = {
        let mut s = lib.launch();
        note(&mut s, &old, "intro", "begins with creation", "Dormant")
    };
    lib.remove(&old);
    {
        let mut s = lib.launch();
        let a = s.annotation(&id).unwrap();
        assert!(!a.is_orphaned(), "dormant, not orphaned");
        assert_eq!(
            range(&s, &id).unwrap().content_hash,
            old.manifest.content_hash
        );
        // Named as the User database last knew it, though it left before this launch.
        let card = s.note_cards("").unwrap().remove(0);
        assert_eq!(card.missing.as_deref(), Some("TCM is not in the library"));
        assert!(card.attention.is_none());
        assert!(card.reference.contains("TCM"), "{}", card.reference);
        assert!(!card.followable());
        let e = s.follow_annotation(&id, false).unwrap_err();
        assert_eq!(e, "TCM is not in the library");
    }
    // Re-imported as a new revision with a sentence inserted.
    let new = notes_on(
        &[(
            "intro",
            "Preface. Moses wrote this book. It begins with creation.",
        )],
        &[],
        2,
    );
    lib.write(&new);
    let s = lib.launch();
    let r = range(&s, &id).unwrap();
    assert_eq!(pointed(&new, &r), "begins with creation");
    let card = s.note_cards("").unwrap().remove(0);
    assert!(card.missing.is_none() && card.attention.is_none());
}

/// #259 AUD-ENGINE-DATA-2: an Orphaned annotation is looked for again in every new revision,
/// not only when its own revision comes back, and only once per revision.
#[test]
fn an_orphan_is_found_again_in_a_later_revision() {
    let lib = Lib::new("third");
    let v1 = notes_on(&[("intro", "grace and peace")], &[], 1);
    let old_tb = tb(GEN_1, 1);
    lib.write(&v1);
    lib.write(&old_tb);
    let (hl, passage_hl) = {
        let mut s = lib.launch();
        (
            highlight(&mut s, &v1, "intro", "grace", 0),
            highlight(&mut s, &old_tb, "GEN", "and void", 0),
        )
    };
    // Revision 2 loses both quotes.
    lib.write(&notes_on(&[("intro", "peace alone")], &[], 2));
    let mut edited = GEN_1;
    edited[1] = "And the earth was without form; and darkness was upon the face of the deep.";
    lib.write(&tb(edited, 2));
    {
        let s = lib.launch();
        assert!(s.annotation(&hl).unwrap().is_orphaned());
        assert!(range(&s, &passage_hl).unwrap().orphaned);
    }
    // Nothing is looked for again while the revision stays the one already searched.
    {
        let mut s = lib.launch_pending();
        assert!(!s.reanchoring.is_busy(), "revision 2 was searched already");
        let out = s
            .run("annotation.reanchor", &json!({ "resource": "tcm" }))
            .unwrap();
        assert_eq!(out["queued"], json!([]));
    }
    // Revision 3 has both quotes again: found at launch, and no longer orphaned.
    let v3 = notes_on(&[("intro", "renewed grace and peace")], &[], 3);
    lib.write(&v3);
    let mut back = GEN_1;
    back[1] = "And the earth was without form, and void; and darkness was over the deep.";
    let tb3 = tb(back, 3);
    lib.write(&tb3);
    let s = lib.launch();
    assert!(!s.annotation(&hl).unwrap().is_orphaned(), "found again");
    let r = range(&s, &hl).unwrap();
    assert_eq!(pointed(&v3, &r), "grace");
    assert_eq!(r.content_hash, v3.manifest.content_hash);
    let r = range(&s, &passage_hl).unwrap();
    assert!(!r.orphaned);
    assert_eq!(pointed(&tb3, &r), "and void");
    assert_eq!(s.reanchoring.report("tcm").unwrap().moved, 1);
}

/// #259: an explicit re-anchoring of a Resource at a revision its orphan was not yet looked
/// for in queues a job.
#[test]
fn asking_to_reanchor_looks_for_an_orphan_in_a_new_revision() {
    let lib = Lib::new("manual");
    let v1 = notes_on(&[("intro", "grace and peace")], &[], 1);
    lib.write(&v1);
    let hl = {
        let mut s = lib.launch();
        highlight(&mut s, &v1, "intro", "grace", 0)
    };
    lib.write(&notes_on(&[("intro", "peace alone")], &[], 2));
    drop(lib.launch());
    let v3 = notes_on(&[("intro", "renewed grace and peace")], &[], 3);
    lib.write(&v3);
    let mut s = Session::new(JobMode::Inline);
    let (store, _) = obs_store::UserStore::open(&lib.root.join("user.db")).unwrap();
    s.attach_user(Box::new(store));
    // The library is scanned after the User database: no load-time job ran for it.
    assert!(s.library.scan(&lib.root.join("library")).is_empty());
    let out = s
        .run("annotation.reanchor", &json!({ "resource": "tcm" }))
        .unwrap();
    assert_eq!(out["queued"], json!(["tcm"]), "manual retry queues");
    settle(&mut s);
    assert!(!s.annotation(&hl).unwrap().is_orphaned());
}

#[test]
fn removing_in_session_names_the_resource_on_its_cards() {
    let lib = Lib::new("departed");
    let old = notes_on(&[("intro", "Moses wrote this book.")], &[], 1);
    lib.write(&old);
    let mut s = lib.launch();
    note(&mut s, &old, "intro", "Moses", "x");
    s.library.remove("tcm");
    let card = s.note_cards("").unwrap().remove(0);
    assert_eq!(card.missing.as_deref(), Some("TCM is not in the library"));
}

#[test]
fn reattach_and_keep_without_anchor_clear_the_warning_and_undo() {
    let lib = Lib::new("reattach");
    let old = notes_on(&[("intro", "Moses wrote this book.")], &[], 1);
    lib.write(&old);
    let (n, h) = {
        let mut s = lib.launch();
        (
            note(&mut s, &old, "intro", "Moses", "Who wrote it"),
            highlight(&mut s, &old, "intro", "book", 0),
        )
    };
    let new = notes_on(&[("intro", "The Pentateuch was written down.")], &[], 2);
    lib.write(&new);
    let mut s = lib.launch();
    assert_eq!(s.orphaned_annotations().unwrap().len(), 2);
    s.run("notes.notebook", &json!({ "value": "attention" }))
        .unwrap();
    assert_eq!(s.note_cards("").unwrap().len(), 2);
    assert!(
        s.note_cards("").unwrap()[0]
            .attention
            .as_deref()
            .unwrap()
            .contains("TCM")
    );
    let before = s.annotation(&h).unwrap();
    // Its old offsets point at other words now: it is not followed there.
    for card in s.note_cards("").unwrap() {
        assert!(
            !card.followable(),
            "an Orphaned annotation is re-attached, not followed"
        );
    }
    let e = s.follow_annotation(&h, false).unwrap_err();
    assert!(e.contains("not found in TCM"), "{e}");
    assert!(s.run("notes.follow", &json!({ "id": h })).is_err());

    // Re-attach the Highlight on "Pentateuch".
    let at = "The Pentateuch was written down."
        .find("Pentateuch")
        .unwrap();
    s.run(
        "annotation.reattach",
        &json!({ "id": h, "resource": "tcm", "start_unit": "intro", "start": at, "end": at + 10 }),
    )
    .unwrap();
    let r = range(&s, &h).unwrap();
    assert_eq!(pointed(&new, &r), "Pentateuch");
    assert!(!s.annotation(&h).unwrap().is_orphaned());
    assert_eq!(s.note_cards("").unwrap().len(), 1, "one left to attend to");
    // A Highlight cannot be kept without its text.
    assert!(s.run("annotation.detach", &json!({ "id": h })).is_err());
    // Keep the Note without an anchor.
    s.run("annotation.detach", &json!({ "id": n })).unwrap();
    let detached = s.annotation(&n).unwrap();
    assert!(detached.anchor.is_none() && !detached.is_orphaned());
    assert_eq!(detached.body(), "Who wrote it");
    assert!(s.orphaned_annotations().unwrap().is_empty());
    // Both are undo steps.
    assert_eq!(s.undo().unwrap().as_deref(), Some("Keep without anchor"));
    assert!(s.annotation(&n).unwrap().is_orphaned());
    assert_eq!(s.undo().unwrap().as_deref(), Some("Re-attach"));
    assert_eq!(s.annotation(&h).unwrap(), before);
    assert_eq!(s.orphaned_annotations().unwrap().len(), 2);
}

#[test]
fn undo_after_a_reimport_lands_on_the_moved_anchor() {
    let lib = Lib::new("rebase");
    let old = tb(GEN_1, 1);
    lib.write(&old);
    let mut s = lib.launch();
    let id = highlight(&mut s, &old, "GEN", "darkness", 0);
    // The Resource is replaced while the app runs (an Import of a new revision).
    let mut fixed = GEN_1;
    fixed[0] = "In the beginning God created the heaven and the earth.";
    let new = tb(fixed, 2);
    lib.write(&new);
    s.library.add_file(&lib.path(&new)).unwrap();
    assert!(s.reanchor_resource("tb"));
    settle(&mut s);
    assert_eq!(pointed(&new, &range(&s, &id).unwrap()), "darkness");
    // Undo the Highlight: it lands although its anchor moved since it was made.
    assert_eq!(s.undo().unwrap().as_deref(), Some("Highlight"));
    assert!(s.annotations(None).unwrap().is_empty());
    assert_eq!(s.redo().unwrap().as_deref(), Some("Highlight"));
    assert_eq!(pointed(&new, &range(&s, &id).unwrap()), "darkness");
}

#[test]
fn a_row_edited_while_the_job_ran_is_planned_again() {
    let lib = Lib::new("conflict");
    let old = notes_on(&[("intro", "Moses wrote this book.")], &[], 1);
    lib.write(&old);
    let id = {
        let mut s = lib.launch();
        note(&mut s, &old, "intro", "this book", "first")
    };
    let new = notes_on(&[("intro", "Indeed Moses wrote this book.")], &[], 2);
    lib.write(&new);
    let mut s = lib.launch_pending();
    // The job plans against "first"; the user edits the Note before it is applied.
    s.jobs().pump(64);
    s.run("annotation.edit", &json!({ "id": id, "body": "second" }))
        .unwrap();
    settle(&mut s);
    let a = s.annotation(&id).unwrap();
    assert_eq!(a.body(), "second", "the edit is kept");
    assert_eq!(pointed(&new, &range(&s, &id).unwrap()), "this book");
}

/// A tiny deterministic generator for the property test.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const WORDS: &[&str] = &[
    "grace", "peace", "light", "word", "the", "and", "of", "faith", "hope", "love", "truth",
    "life", "way", "spirit", "law", "promise",
];

fn sentence(rng: &mut Rng) -> String {
    let n = 3 + rng.below(8);
    let mut words: Vec<&str> = (0..n).map(|_| WORDS[rng.below(WORDS.len())]).collect();
    let first = words[0];
    let capital = format!("{}{}", first[..1].to_uppercase(), &first[1..]);
    words[0] = &capital;
    let mut s = words.join(" ");
    s.push(['.', ';', ',', '!'][rng.below(4)]);
    s
}

/// One random edit: insert, delete or replace a sentence, change punctuation or spacing.
fn edit(rng: &mut Rng, text: &str) -> String {
    let mut parts: Vec<String> = text.split(' ').map(str::to_string).collect();
    match rng.below(5) {
        0 => {
            let at = rng.below(parts.len() + 1);
            parts.insert(at, sentence(rng));
        }
        1 if parts.len() > 4 => {
            let at = rng.below(parts.len() - 3);
            parts.drain(at..at + 1 + rng.below(3));
        }
        2 => {
            let at = rng.below(parts.len());
            parts[at] = WORDS[rng.below(WORDS.len())].to_string();
        }
        3 => {
            let at = rng.below(parts.len());
            parts[at].push(',');
        }
        _ => {
            let at = rng.below(parts.len());
            parts[at].push(' ');
        }
    }
    parts.join(" ")
}

/// Property: whatever the edits, re-anchoring never deletes an Annotation, never changes
/// what one says, and leaves each either pointing at its quote (found) or kept with it
/// (orphaned, or a Passage anchor on whole Verses).
#[test]
fn reanchoring_never_deletes_an_annotation() {
    let lib = Lib::new("property");
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut seen = ReanchorReport::default();
    for round in 0..24 {
        let text: String = (0..12)
            .map(|_| sentence(&mut rng))
            .collect::<Vec<_>>()
            .join(" ");
        let verses: [String; 3] = [sentence(&mut rng), text.clone(), sentence(&mut rng)];
        let old_tcm = notes_on(&[("intro", &text)], &[], 1);
        let old_tb = tb([verses[0].as_str(), &verses[1], &verses[2]], 1);
        lib.write(&old_tcm);
        lib.write(&old_tb);
        let _ = std::fs::remove_file(lib.root.join("user.db"));
        let made = {
            let mut s = lib.launch();
            for i in 0..10 {
                let (r, unit) = if i % 2 == 0 {
                    (&old_tcm, "intro")
                } else {
                    (&old_tb, "GEN")
                };
                let t = unit_text(r, unit);
                let words: Vec<(usize, &str)> = t
                    .split_word_bound_indices_simple()
                    .into_iter()
                    .filter(|(_, w)| w.chars().all(char::is_alphabetic))
                    .collect();
                let a = rng.below(words.len());
                let b = (a + rng.below(4)).min(words.len() - 1);
                let start = words[a].0;
                let end = words[b].0 + words[b].1.len();
                let mut params = json!({
                    "resource": r.manifest.resource_id,
                    "start_unit": unit,
                    "start": start,
                    "end": end,
                });
                let cmd = if i % 3 == 0 {
                    params["body"] = json!(format!("note {i}"));
                    "annotation.note"
                } else {
                    "annotation.highlight"
                };
                s.run(cmd, &params).unwrap();
            }
            s.annotations(None).unwrap()
        };
        assert_eq!(made.len(), 10);
        let mut new_text = text.clone();
        for _ in 0..1 + rng.below(4) {
            new_text = edit(&mut rng, &new_text);
        }
        let new_tcm = notes_on(&[("intro", &new_text)], &[], round + 2);
        let new_tb = tb([verses[0].as_str(), &new_text, &verses[2]], round + 2);
        lib.write(&new_tcm);
        lib.write(&new_tb);
        let s = lib.launch();
        let after = s.annotations(None).unwrap();
        assert_eq!(
            after.len(),
            made.len(),
            "round {round}: no Annotation deleted"
        );
        let total: usize = ["tb", "tcm"]
            .iter()
            .filter_map(|r| s.reanchoring.report(r))
            .map(ReanchorReport::total)
            .sum();
        assert_eq!(total, made.len(), "round {round}: every one accounted for");
        for r in ["tb", "tcm"].iter().filter_map(|r| s.reanchoring.report(r)) {
            seen.unchanged += r.unchanged;
            seen.moved += r.moved;
            seen.whole_verses += r.whole_verses;
            seen.orphaned += r.orphaned;
        }
        for (b, a) in made.iter().zip(&after) {
            assert_eq!(a.id, b.id);
            assert_eq!(a.content, b.content, "what it says is kept");
            assert_eq!(a.notebook_id, b.notebook_id);
            let new = if a.anchor.as_ref().unwrap().passage().is_some() {
                &new_tb
            } else {
                &new_tcm
            };
            match a.anchor.as_ref().unwrap().text_range() {
                Some(r) if r.orphaned => {
                    assert_eq!(
                        Some(&r.quote),
                        b.anchor.as_ref().unwrap().text_range().map(|o| &o.quote)
                    );
                }
                Some(r) => {
                    let quote = &b.anchor.as_ref().unwrap().text_range().unwrap().quote;
                    let at = pointed(new, r);
                    let letters = |s: &str| {
                        s.chars()
                            .filter(|c| c.is_alphanumeric())
                            .collect::<String>()
                    };
                    assert_eq!(letters(&at), letters(quote), "round {round}");
                    assert_eq!(r.content_hash, new.manifest.content_hash);
                }
                // A Passage anchor showing whole Verses.
                None => assert!(a.anchor.as_ref().unwrap().passage().is_some()),
            }
        }
    }
    // The edits exercised every outcome.
    assert!(
        seen.unchanged > 0 && seen.moved > 0 && seen.whole_verses > 0 && seen.orphaned > 0,
        "{seen:?}"
    );
}

/// Word boundaries without a Unicode segmentation dependency: runs of letters and the rest.
trait SimpleWords {
    fn split_word_bound_indices_simple(&self) -> Vec<(usize, &str)>;
}

impl SimpleWords for str {
    fn split_word_bound_indices_simple(&self) -> Vec<(usize, &str)> {
        let mut out = Vec::new();
        let mut start = None;
        for (i, c) in self.char_indices() {
            match (c.is_alphabetic(), start) {
                (true, None) => start = Some(i),
                (false, Some(s)) => {
                    out.push((s, &self[s..i]));
                    start = None;
                }
                _ => {}
            }
        }
        if let Some(s) = start {
            out.push((s, &self[s..]));
        }
        out
    }
}

/// Timing on a real Starter Bible (local only: skipped when the library is absent). 1,000
/// Highlights made against "another revision" of the KJV re-anchored in one plan.
/// `cargo test -p obs-engine --release -- --ignored plan_on_the_starter_kjv --nocapture`
#[test]
#[ignore]
fn plan_on_the_starter_kjv() {
    use obs_store::ResourceStore;
    let Some(dir) = crate::default_library_dir() else {
        return;
    };
    let path = dir.join("kjv-strongs.obr");
    if !path.exists() {
        eprintln!("skipped: {} is absent", path.display());
        return;
    }
    let reader = obs_store::ObrReader::open(&path).unwrap();
    let units = reader.unit_ids().unwrap();
    let mut made = Vec::new();
    let mut rng = Rng(7);
    while made.len() < 1000 {
        let unit = &units[rng.below(units.len())];
        let Some((_, text)) = reader.unit_text(unit).unwrap() else {
            continue;
        };
        let words = text.split_word_bound_indices_simple();
        if words.len() < 10 {
            continue;
        }
        let a = rng.below(words.len() - 5);
        let (start, end) = (words[a].0, words[a + 3].0 + words[a + 3].1.len());
        let (prefix, quote, suffix) =
            obs_store::annotation::quote_with_context(&text, start, end).unwrap();
        // Shifted offsets: every one has to be searched for.
        made.push(Annotation {
            id: format!("{}", made.len()),
            notebook_id: "n".into(),
            content: Content::Highlight {
                style: "yellow".into(),
            },
            anchor: Some(Anchor::TextRange {
                range: TextRange {
                    resource_id: "kjv-strongs".into(),
                    unit: unit.clone(),
                    start: start as u32 + 7,
                    end: end as u32 + 7,
                    quote: quote.into(),
                    prefix: prefix.into(),
                    suffix: suffix.into(),
                    content_hash: "sha256:older".into(),
                    native_ref: None,
                    whole_verses: false,
                    orphaned: false,
                    checked_hash: None,
                },
            }),
            created_at: 0,
            modified_at: 0,
        });
    }
    let t = std::time::Instant::now();
    let plan = crate::reanchor::plan(&reader, &made).unwrap();
    let took = t.elapsed();
    eprintln!("{:?} in {took:?}", plan.report);
    assert_eq!(plan.report.total(), 1000);
    assert_eq!(plan.report.moved, 1000);
}
