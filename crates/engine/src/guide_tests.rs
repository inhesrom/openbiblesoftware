//! Guide framework and Passage Guide sections (#53): arrangement persistence, section data
//! and the job-queue contract.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use obs_model::Key;
use obs_scripture::{Book, Passage, Verse};
use serde_json::json;

use crate::guide::{cut, heading_and_excerpt_for_tests};
use crate::*;

fn john(c: u16, v: u16) -> Verse {
    Verse::new(Book::John, c, v)
}

fn range(a: Verse, b: Verse) -> Passage {
    Passage::new(a, b).unwrap()
}

struct Temp(PathBuf);
impl Temp {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("obs-guide-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("library")).unwrap();
        Self(root)
    }
    fn lib(&self) -> PathBuf {
        self.0.join("library")
    }
    fn db(&self) -> PathBuf {
        self.0.join("user.db")
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write(dir: &Path, resource: &obs_model::Resource) -> PathBuf {
    let path = dir.join(format!("{}.obr", resource.manifest.resource_id));
    obs_store::write_obr(resource, &path).unwrap();
    path
}

/// Two Commentaries on John 3 (Preferred order: Alpha, then Beta) and one on Genesis only.
fn commentaries(dir: &Path) {
    let alpha = crate::testing::commentary(
        "alpha",
        "Alpha Commentary",
        "ALPHA",
        &[
            (
                range(john(3, 1), john(3, 36)),
                None,
                "An introduction to the whole chapter.",
            ),
            (
                range(john(3, 1), john(3, 21)),
                Some("The Night Visit"),
                "A teacher comes by night with questions about new birth and the kingdom.",
            ),
            (
                range(john(3, 22), john(3, 36)),
                Some("The Friend of the Bridegroom"),
                "The forerunner decreases so that another may increase.",
            ),
        ],
    );
    let beta = crate::testing::commentary(
        "beta",
        "Beta Notes",
        "BETA",
        &[(
            Passage::verse(john(3, 16)),
            None,
            "16. The widest verse in the chapter: love, gift, belief and life together.",
        )],
    );
    let genesis = crate::testing::commentary(
        "gamma",
        "Gamma on Genesis",
        "GAMMA",
        &[(
            range(
                Verse::new(Book::Genesis, 1, 1),
                Verse::new(Book::Genesis, 1, 5),
            ),
            Some("Light"),
            "The first day.",
        )],
    );
    for r in [alpha, beta, genesis] {
        write(dir, &r);
    }
}

fn openbible(dir: &Path) {
    let options = obs_formats::DatasetOptions {
        resource_id: "openbible".into(),
        title: "OpenBible.info".into(),
        ..Default::default()
    };
    let (r, _) = obs_formats::import_openbible_cross_references(
        include_str!("../../formats/tests/assets/openbible/cross-references-excerpt.txt"),
        &options,
    )
    .unwrap();
    write(dir, &r);
}

fn session(t: &Temp, db: bool) -> Session {
    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(&t.lib()).is_empty());
    if db {
        let (store, _) = obs_store::UserStore::open(&t.db()).unwrap();
        s.attach_user(Box::new(store));
    }
    s
}

/// Ask for a section until it is no longer Loading, pumping the inline queue between asks
/// the way frames would.
fn settle(s: &mut Session, key: &Key, section: GuideSection) -> SectionStatus {
    for _ in 0..50 {
        let status = s.guide_section(key, section);
        if status != SectionStatus::Loading {
            return status;
        }
        s.jobs().pump(4);
        assert!(
            s.poll_imports().is_empty(),
            "Guide events are applied, not returned"
        );
    }
    panic!("{section:?} never finished");
}

fn ready(status: SectionStatus) -> Arc<SectionData> {
    match status {
        SectionStatus::Ready(d) => d,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_section_is_submitted_to_the_job_queue_and_never_computed_on_the_asking_thread() {
    let t = Temp::new("queue");
    commentaries(&t.lib());
    let mut s = session(&t, false);
    let key = Key::Passage(Passage::verse(john(3, 16)));
    // Nothing runs until the queue is pumped: asking never blocks on SQLite.
    assert_eq!(
        s.guide_section(&key, GuideSection::Commentaries),
        SectionStatus::Loading
    );
    assert!(s.guides.is_busy());
    // Asking again does not submit a second job.
    assert_eq!(
        s.guide_section(&key, GuideSection::Commentaries),
        SectionStatus::Loading
    );
    assert_eq!(s.jobs().pump(10), 1);
    s.poll_imports();
    assert!(matches!(
        s.guide_section(&key, GuideSection::Commentaries),
        SectionStatus::Ready(_)
    ));
    assert!(!s.guides.is_busy());
}

#[test]
fn commentaries_give_one_row_per_commentary_in_preferred_order_with_heading_and_lines() {
    let t = Temp::new("rows");
    commentaries(&t.lib());
    let mut s = session(&t, false);
    let key = Key::Passage(Passage::verse(john(3, 16)));
    let data = ready(settle(&mut s, &key, GuideSection::Commentaries));
    let SectionData::Commentaries(rows) = &*data else {
        panic!("{data:?}")
    };
    let summary: Vec<_> = rows
        .iter()
        .map(|r| (r.title.as_str(), r.section, r.heading.as_deref()))
        .collect();
    // The narrowest section starting at or before 3:16 wins over the chapter introduction.
    assert_eq!(
        summary,
        [
            (
                "Alpha Commentary",
                range(john(3, 1), john(3, 21)),
                Some("The Night Visit")
            ),
            ("Beta Notes", Passage::verse(john(3, 16)), None),
        ]
    );
    assert!(rows[0].excerpt.starts_with("A teacher comes by night"));
    assert!(rows[1].excerpt.starts_with("16. The widest verse"));

    // Preferred order decides the row order.
    s.run("library.move", &json!({ "id": "beta", "to": 0 }))
        .unwrap();
    let data = ready(settle(&mut s, &key, GuideSection::Commentaries));
    let SectionData::Commentaries(rows) = &*data else {
        panic!()
    };
    assert_eq!(rows[0].resource_id, "beta");

    // A range starting in a gap picks the first section inside it; a Passage outside every
    // section of a Commentary gives that Commentary no row.
    let key = Key::Passage(range(john(3, 22), john(3, 30)));
    let data = ready(settle(&mut s, &key, GuideSection::Commentaries));
    let SectionData::Commentaries(rows) = &*data else {
        panic!()
    };
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].heading.as_deref(),
        Some("The Friend of the Bridegroom")
    );
    let key = Key::Passage(Passage::verse(Verse::new(Book::Exodus, 1, 1)));
    assert!(ready(settle(&mut s, &key, GuideSection::Commentaries)).is_empty());
}

#[test]
fn an_unreadable_commentary_is_skipped_and_the_others_still_show() {
    let t = Temp::new("broken");
    commentaries(&t.lib());
    let mut s = session(&t, false);
    // Alpha goes bad after the scan: its file is no longer a readable Resource.
    std::fs::write(t.lib().join("alpha.obr"), b"not a resource").unwrap();
    let key = Key::Passage(Passage::verse(john(3, 16)));
    let data = ready(settle(&mut s, &key, GuideSection::Commentaries));
    let SectionData::Commentaries(rows) = &*data else {
        panic!("{data:?}")
    };
    let ids: Vec<_> = rows.iter().map(|r| r.resource_id.as_str()).collect();
    assert_eq!(ids, ["beta"]);

    // When every Commentary that could answer is unreadable, the section reports the error.
    let sections = LibrarySections::default();
    std::fs::write(t.lib().join("beta.obr"), b"not a resource").unwrap();
    std::fs::write(t.lib().join("gamma.obr"), b"not a resource").unwrap();
    let err = sections
        .commentary_rows(&s.library, Passage::verse(john(3, 16)))
        .unwrap_err();
    assert!(err.contains("ALPHA") && err.contains("BETA"), "{err}");
}

#[test]
fn with_no_commentaries_the_section_is_empty_and_names_the_resource_it_needs() {
    let t = Temp::new("hint");
    openbible(&t.lib());
    let mut s = session(&t, false);
    let key = Key::Passage(Passage::verse(john(3, 16)));
    assert!(ready(settle(&mut s, &key, GuideSection::Commentaries)).is_empty());
    let hint = GuideSection::Commentaries.empty_hint(&s.library, &s.guides.indexes());
    assert!(hint.starts_with("None in your library"));
    assert!(hint.contains("Add a Commentary"));
    // With Commentaries that do not cover the Passage, the hint says so instead.
    commentaries(&t.lib());
    s.library.scan(&t.lib());
    assert!(
        !GuideSection::Commentaries
            .empty_hint(&s.library, &s.guides.indexes())
            .contains("Add a Commentary")
    );
    assert!(
        GuideSection::CrossReferences
            .empty_hint(&Library::new(), &Default::default())
            .contains("Add a Cross-reference set")
    );
}

#[test]
fn cross_references_reuse_the_grouped_vote_ranked_data() {
    let t = Temp::new("xrefs");
    openbible(&t.lib());
    let mut s = session(&t, false);
    let key = Key::Passage(Passage::verse(john(3, 16)));
    let data = ready(settle(&mut s, &key, GuideSection::CrossReferences));
    let SectionData::CrossReferences(groups) = &*data else {
        panic!()
    };
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].verse, john(3, 16));
    assert_eq!(groups[0].rows[0].target.to_string(), "John 11:25-26");
    assert_eq!(data.count(), groups[0].rows.len());
}

#[test]
fn a_library_change_recomputes_sections() {
    let t = Temp::new("library-change");
    let mut s = session(&t, false);
    let key = Key::Passage(Passage::verse(john(3, 16)));
    assert!(ready(settle(&mut s, &key, GuideSection::Commentaries)).is_empty());
    commentaries(&t.lib());
    s.library.scan(&t.lib());
    assert_eq!(
        s.guide_section(&key, GuideSection::Commentaries),
        SectionStatus::Loading
    );
    assert_eq!(
        ready(settle(&mut s, &key, GuideSection::Commentaries)).count(),
        2
    );
}

#[test]
fn sections_nobody_asks_for_any_more_are_cancelled() {
    let t = Temp::new("cancel");
    commentaries(&t.lib());
    let mut s = session(&t, false);
    let a = Key::Passage(Passage::verse(john(3, 16)));
    let b = Key::Passage(Passage::verse(john(3, 17)));
    s.guide_section(&a, GuideSection::Commentaries);
    s.poll_imports();
    // The Guide moved on to 3:17: the 3:16 job is cancelled before it starts.
    s.guide_section(&b, GuideSection::Commentaries);
    s.poll_imports();
    s.jobs().pump(10);
    s.poll_imports();
    assert!(!s.guides.is_busy());
    assert_eq!(
        s.guide_section(&a, GuideSection::Commentaries),
        SectionStatus::Loading,
        "a cancelled section is asked for again, not cached"
    );
}

/// Holds Commentaries until released; everything else is computed from the library.
#[derive(Debug)]
struct Held {
    entered: Mutex<mpsc::Sender<()>>,
    release: Mutex<mpsc::Receiver<()>>,
    inner: LibrarySections,
}
impl SectionSource for Held {
    fn compute(
        &self,
        library: &Library,
        key: &Key,
        section: GuideSection,
        ctx: &JobContext,
    ) -> Result<SectionData, String> {
        if section == GuideSection::Commentaries {
            self.entered.lock().unwrap().send(()).unwrap();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(20))
                .unwrap();
        }
        self.inner.compute(library, key, section, ctx)
    }
}

#[test]
fn sections_compute_independently_and_a_held_one_does_not_hold_back_the_others() {
    let t = Temp::new("independent");
    commentaries(&t.lib());
    openbible(&t.lib());
    let mut s = Session::new(JobMode::Threads(2));
    s.library.scan(&t.lib());
    let (entered, held) = mpsc::channel();
    let (release, rx) = mpsc::channel();
    s.guides.set_source(Arc::new(Held {
        entered: Mutex::new(entered),
        release: Mutex::new(rx),
        inner: LibrarySections::default(),
    }));
    let key = Key::Passage(Passage::verse(john(3, 16)));
    let ask = |s: &mut Session| {
        (
            s.guide_section(&key, GuideSection::Commentaries),
            s.guide_section(&key, GuideSection::CrossReferences),
        )
    };
    ask(&mut s);
    held.recv_timeout(Duration::from_secs(20)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let xrefs = loop {
        s.poll_imports();
        let (c, x) = ask(&mut s);
        assert_eq!(c, SectionStatus::Loading);
        if x != SectionStatus::Loading {
            break x;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(!ready(xrefs).is_empty());
    release.send(()).unwrap();
    let commentaries = loop {
        s.poll_imports();
        let (c, _) = ask(&mut s);
        if c != SectionStatus::Loading {
            break c;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(ready(commentaries).count(), 2);
}

#[test]
fn arrangement_commands_change_every_guide_of_the_kind_and_persist_across_restarts() {
    let t = Temp::new("arrangement");
    let mut s = session(&t, true);
    let order = |s: &Session| -> Vec<(&'static str, bool, bool)> {
        s.guides
            .arrangement(Guide::PassageGuide)
            .sections
            .iter()
            .map(|p| (p.section.id(), p.collapsed, p.hidden))
            .collect()
    };
    assert_eq!(
        order(&s),
        [
            ("commentaries", false, false),
            ("cross_references", false, false),
            ("parallel_passages", false, false),
            ("key_words", false, false),
            ("people_and_places", false, false)
        ]
    );
    let g = |extra: serde_json::Value| {
        let mut p = json!({ "guide": "passage_guide", "section": "cross_references" });
        p.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        p
    };
    s.run("guide.move_section", &g(json!({ "to": 0 }))).unwrap();
    s.run("guide.collapse_section", &g(json!({}))).unwrap();
    s.run(
        "guide.hide_section",
        &json!({ "guide": "passage_guide", "section": "commentaries" }),
    )
    .unwrap();
    let changed = vec![
        ("cross_references", true, false),
        ("commentaries", false, true),
        ("parallel_passages", false, false),
        ("key_words", false, false),
        ("people_and_places", false, false),
    ];
    assert_eq!(order(&s), changed);
    // Word Study's arrangement is its own.
    let word_study = s.guides.arrangement(Guide::WordStudy);
    assert_eq!(word_study, GuideArrangement::default_for(Guide::WordStudy));
    assert_eq!(word_study.sections[0].section, GuideSection::LexiconEntries);
    assert!(
        s.run(
            "guide.move_section",
            &json!({ "guide": "word_study", "section": "commentaries", "to": 0 })
        )
        .is_err()
    );
    drop(s);

    let mut s = session(&t, true);
    assert_eq!(order(&s), changed, "restored from the User database");
    s.run("guide.collapse_section", &g(json!({}))).unwrap();
    assert_eq!(order(&s)[0], ("cross_references", false, false));
    s.run(
        "guide.reset_arrangement",
        &json!({ "guide": "passage_guide" }),
    )
    .unwrap();
    drop(s);
    let s = session(&t, true);
    assert_eq!(
        order(&s),
        [
            ("commentaries", false, false),
            ("cross_references", false, false),
            ("parallel_passages", false, false),
            ("key_words", false, false),
            ("people_and_places", false, false)
        ]
    );
}

#[test]
fn a_saved_arrangement_tolerates_unknown_missing_and_repeated_sections() {
    let a = GuideArrangement::from_json(
        Guide::PassageGuide,
        r#"{"sections":[{"id":"no_such_section","hidden":true},{"id":"cross_references","collapsed":true},{"id":"cross_references"}]}"#,
    );
    let ids: Vec<_> = a
        .sections
        .iter()
        .map(|p| (p.section.id(), p.collapsed))
        .collect();
    assert_eq!(
        ids,
        [
            ("commentaries", false),
            ("cross_references", true),
            ("parallel_passages", false),
            ("key_words", false),
            ("people_and_places", false)
        ]
    );
    // An arrangement saved before #54 added Key words and People & places (and #212 Parallel
    // passages) gets them at their default places, after Cross-references, keeping the
    // user's order.
    let a = GuideArrangement::from_json(
        Guide::PassageGuide,
        r#"{"sections":[{"id":"cross_references"},{"id":"commentaries","hidden":true}]}"#,
    );
    let ids: Vec<_> = a
        .sections
        .iter()
        .map(|p| (p.section.id(), p.hidden))
        .collect();
    assert_eq!(
        ids,
        [
            ("cross_references", false),
            ("parallel_passages", false),
            ("key_words", false),
            ("people_and_places", false),
            ("commentaries", true)
        ]
    );
    assert_eq!(
        GuideArrangement::from_json(Guide::PassageGuide, "not json"),
        GuideArrangement::default_for(Guide::PassageGuide)
    );
    let mut a = GuideArrangement::default_for(Guide::PassageGuide);
    assert!(a.move_to(GuideSection::Commentaries, 99));
    // Clamped to the end.
    assert_eq!(a.sections[4].section, GuideSection::Commentaries);
    assert!(!a.move_to(GuideSection::Commentaries, 4));
    assert_eq!(
        GuideArrangement::from_json(Guide::PassageGuide, &a.to_json()),
        a
    );
}

#[test]
fn excerpts_skip_the_heading_collapse_whitespace_and_cut_at_a_word() {
    assert_eq!(cut("one two three", 100), "one two three");
    assert_eq!(cut("one two three", 9), "one two…");
    let (heading, excerpt) = heading_and_excerpt_for_tests("Heading\n  body   text\n\nmore", 7);
    assert_eq!(heading.as_deref(), Some("Heading"));
    assert_eq!(excerpt, "body text more");
    let (heading, _) = heading_and_excerpt_for_tests("Body first", 0);
    assert_eq!(heading, None);
}

/// Manual check against the real Starter library (skips when it is absent): every section
/// for John 3:16 with timings, cold and warm.
#[test]
#[ignore = "reads the Starter library from the default library folder"]
fn starter_library_passage_guide_for_john_3_16() {
    let Some(dir) = default_library_dir().filter(|d| d.is_dir()) else {
        eprintln!("no Starter library; skipped");
        return;
    };
    let mut library = Library::new();
    library.scan(&dir);
    let sections = LibrarySections::default();
    let q = JobQueue::inline();
    let ctx_job = q.submit_fn("probe", move |ctx| {
        let key = Key::Passage(Passage::verse(john(3, 16)));
        let mut out = Vec::new();
        for round in ["cold", "warm"] {
            for section in GuideSection::ALL {
                let t = Instant::now();
                let data = sections.compute(&library, &key, section, ctx)?;
                out.push(format!(
                    "{round} {}: {} in {:?}",
                    section.title(),
                    data.count(),
                    t.elapsed()
                ));
                if let SectionData::Commentaries(rows) = &data
                    && round == "cold"
                {
                    for r in rows {
                        out.push(format!(
                            "  {} | {} | {:?} | {}",
                            r.abbreviation,
                            r.section,
                            r.heading,
                            cut(&r.excerpt, 120)
                        ));
                    }
                }
            }
        }
        Ok(out)
    });
    q.pump(1);
    for e in q.drain_events() {
        if e.id == ctx_job
            && let JobEventKind::Finished(out) = e.kind
        {
            for line in *out.downcast::<Vec<String>>().unwrap() {
                eprintln!("{line}");
            }
        }
    }
}

#[test]
fn an_unlinked_passage_guide_opens_at_the_focused_tabs_passage() {
    let mut s = Session::new(JobMode::Inline);
    let tool = s.workspace.open(
        TabKind::Tool(Tool::CrossReferences),
        "Cross-references".into(),
        OpenRequest::default(),
    );
    s.workspace.navigate(tool, john(3, 16));
    let out = s.run("guide.passage_guide", &json!({})).unwrap();
    let guide = s
        .workspace
        .tab(TabId(out["tab"].as_u64().unwrap()))
        .unwrap();
    assert_eq!(guide.link, None);
    assert_eq!(guide.history.current(), Some(john(3, 16)));
    // With nothing focused it opens empty, waiting for a typed Passage.
    let focused = s.workspace.focused().unwrap();
    s.workspace.close(focused);
    assert_eq!(s.workspace.focused(), None);
    let out = s.run("guide.passage_guide", &json!({})).unwrap();
    let guide = s
        .workspace
        .tab(TabId(out["tab"].as_u64().unwrap()))
        .unwrap();
    assert_eq!(guide.history.current(), None);
}
