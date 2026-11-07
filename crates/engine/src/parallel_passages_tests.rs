//! Parallel passages (#212): the Passage Guide section from the BSB's `\r` sections, read both
//! ways with the narrowest evidence winning, and `compare.parallel` opening Text Comparison
//! on a parallel without moving the Guide.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use obs_model::Key;
use obs_scripture::{Book, Passage, RefStyle, Standard, Verse, format_passage};
use serde_json::json;

use crate::parallel_passages::{ParallelPassages, parallels_for};
use crate::*;

fn passage(book: Book, chapter: u16, start: u16, end: u16) -> Passage {
    Passage::new(
        Verse::new(book, chapter, start),
        Verse::new(book, chapter, end),
    )
    .unwrap()
}

fn sets() -> Vec<(String, String, Vec<obs_model::ParallelSection>)> {
    let r = testing::bsb_parallels();
    vec![(
        r.manifest.resource_id.clone(),
        r.manifest.title.clone(),
        r.parallel_sections,
    )]
}

fn short(p: Passage) -> String {
    format_passage(p, &Standard, RefStyle::Short)
}

/// Each row as `part: parallel (heading)`.
fn rows(found: &ParallelPassages) -> Vec<String> {
    found
        .groups
        .iter()
        .flat_map(|g| {
            g.rows
                .iter()
                .map(move |r| format!("{}: {} ({})", short(g.passage), short(r.passage), r.title))
        })
        .collect()
}

/// Acceptance: Mark 1:9-11 lists Matthew 3:13-17 and Luke 3:21-22. Mark's own section is
/// 1:1-11 (with Matthew 3:1-17 and Luke 3:1-22), but Matthew's and Luke's baptism sections
/// name Mark 1:9-11 exactly, so the narrower evidence wins.
#[test]
fn mark_1_9_11_lists_matthew_3_13_17_and_luke_3_21_22() {
    let found = parallels_for(passage(Book::Mark, 1, 9, 11), &sets());
    assert_eq!(
        rows(&found),
        [
            "Mark 1:9-11: Matt 3:13-17 (The Baptism of Jesus)",
            "Mark 1:9-11: Luke 3:21-22 (The Baptism of Jesus)",
            "Mark 1:9-11: John 1:29-34 (Jesus the Lamb of God)",
        ]
    );
    assert_eq!(found.count(), 3);
    assert_eq!(found.rows().next().unwrap().source_id, "bsb-parallels");
    // A single Verse of it finds the same accounts.
    let verse = parallels_for(Passage::verse(Verse::new(Book::Mark, 1, 10)), &sets());
    assert_eq!(
        verse.rows().map(|r| r.passage).collect::<Vec<_>>(),
        [
            passage(Book::Matthew, 3, 13, 17),
            passage(Book::Luke, 3, 21, 22),
            passage(Book::John, 1, 29, 34)
        ]
    );
}

/// Acceptance: 2 Samuel 24 lists 1 Chronicles 21, section by section and as one Passage in
/// all; the census offering of Exodus 30 is listed beside the census.
#[test]
fn second_samuel_24_lists_first_chronicles_21() {
    let chapter = Passage::chapter(&Standard, Book::SecondSamuel, 24).unwrap();
    let found = parallels_for(chapter, &sets());
    assert_eq!(
        rows(&found),
        [
            "2 Sam 24:1-9: Exod 30:11-16 (David’s Military Census)",
            "2 Sam 24:1-9: 1 Chr 21:1-6 (David’s Military Census)",
            "2 Sam 24:10-14: 1 Chr 21:7-13 (Judgment for David’s Sin)",
            "2 Sam 24:15-17: 1 Chr 21:14-17 (A Plague on Israel)",
            "2 Sam 24:18-25: 1 Chr 21:18-30 (David Builds an Altar)",
        ]
    );
    assert_eq!(
        found.all.iter().map(|p| short(*p)).collect::<Vec<_>>(),
        ["Exod 30:11-16", "1 Chr 21"]
    );
    // And back: 1 Chronicles 21 lists 2 Samuel 24.
    let back = parallels_for(
        Passage::chapter(&Standard, Book::FirstChronicles, 21).unwrap(),
        &sets(),
    );
    assert!(back.all.iter().any(|p| short(*p) == "2 Sam 24"), "{back:?}");
}

/// A Passage covering several sections lists each part's accounts, and a coarse parallel gives
/// way to the narrower ones inside it (Matthew 3:1-17 to 3:1-12 and 3:13-17).
#[test]
fn a_wider_passage_lists_each_part_and_drops_coarse_parallels() {
    let found = parallels_for(passage(Book::Mark, 1, 1, 11), &sets());
    let listed: Vec<String> = found.rows().map(|r| short(r.passage)).collect();
    for expected in [
        "Isa 40:1-5",
        "Matt 3:1-12",
        "Matt 3:13-17",
        "Luke 3:1-20",
        "Luke 3:21-22",
        "John 1:19-28",
        "John 1:29-34",
    ] {
        assert!(
            listed.contains(&expected.to_string()),
            "{expected}: {listed:?}"
        );
    }
    for coarse in ["Matt 3:1-17", "Luke 3:1-22", "John 1:19-34"] {
        assert!(
            !listed.contains(&coarse.to_string()),
            "{coarse}: {listed:?}"
        );
    }
    assert_eq!(
        found
            .groups
            .iter()
            .map(|g| short(g.passage))
            .collect::<Vec<_>>(),
        ["Mark 1:1-8", "Mark 1:9-11"]
    );
    // Nothing is parallel to a Passage no section covers, and a Passage is never its own
    // parallel.
    assert!(parallels_for(passage(Book::Revelation, 22, 1, 5), &sets()).is_empty());
    assert!(
        found
            .rows()
            .all(|r| !r.passage.overlaps(passage(Book::Mark, 1, 1, 11)))
    );
}

struct Temp(PathBuf);
impl Temp {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("obs-parallels-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn session(dir: &std::path::Path, resources: &[obs_model::Resource]) -> Session {
    for r in resources {
        let path = dir.join(format!("{}.obr", r.manifest.resource_id));
        obs_store::write_obr(r, &path).unwrap();
    }
    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(dir).is_empty());
    s
}

fn settle(s: &mut Session, key: &Key, section: GuideSection) -> std::sync::Arc<SectionData> {
    for _ in 0..50 {
        match s.guide_section(key, section) {
            SectionStatus::Loading => {
                s.jobs().pump(4);
                s.poll_imports();
            }
            SectionStatus::Ready(d) => return d,
            SectionStatus::Failed(e) => panic!("{section:?} failed: {e}"),
        }
    }
    panic!("{section:?} never finished");
}

/// The Passage Guide has the section after Cross-references, reading every Cross-reference
/// set; without the parallels set it names it and the Starter library offers it.
#[test]
fn the_passage_guide_section_reads_the_library_and_names_what_is_missing() {
    assert_eq!(
        Guide::PassageGuide.sections(),
        [
            GuideSection::Commentaries,
            GuideSection::CrossReferences,
            GuideSection::ParallelPassages,
            GuideSection::KeyWords,
            GuideSection::PeopleAndPlaces,
        ]
    );
    assert_eq!(GuideSection::ParallelPassages.id(), "parallel_passages");
    assert_eq!(GuideSection::ParallelPassages.title(), "Parallel passages");
    // An arrangement saved before the section existed shows it after Cross-references.
    let saved = r#"{"sections":[{"id":"key_words"},{"id":"cross_references","collapsed":true},{"id":"commentaries"},{"id":"people_and_places"}]}"#;
    let order: Vec<GuideSection> = GuideArrangement::from_json(Guide::PassageGuide, saved)
        .sections
        .iter()
        .map(|p| p.section)
        .collect();
    assert_eq!(
        order,
        [
            GuideSection::KeyWords,
            GuideSection::CrossReferences,
            GuideSection::ParallelPassages,
            GuideSection::Commentaries,
            GuideSection::PeopleAndPlaces,
        ]
    );

    let t = Temp::new("section");
    let mut s = session(&t.0, &[testing::bsb_parallels()]);
    let key = Key::Passage(passage(Book::Mark, 1, 9, 11));
    let data = settle(&mut s, &key, GuideSection::ParallelPassages);
    let SectionData::ParallelPassages(found) = &*data else {
        panic!("{data:?}")
    };
    assert_eq!(data.count(), 3);
    assert_eq!(
        found.rows().next().unwrap().passage,
        passage(Book::Matthew, 3, 13, 17)
    );
    // The parallels set has no Cross-references: that section still asks for a set.
    let xrefs = s.guide_empty_state(&key, GuideSection::CrossReferences);
    assert!(
        xrefs.message.contains("Add a Cross-reference set"),
        "{xrefs:?}"
    );
    let none = s.guide_empty_state(
        &Key::Passage(passage(Book::Revelation, 22, 1, 5)),
        GuideSection::ParallelPassages,
    );
    assert_eq!(
        none.message,
        "None in your library: no parallel passage is listed for this Passage."
    );

    // OpenBible.info's Cross-references stay the first Cross-reference set of a Starter
    // library, ahead of the parallels set its file name sorts before.
    let (xrefs, _) = obs_formats::import_openbible_cross_references(
        include_str!("../../formats/tests/assets/openbible/cross-references-excerpt.txt"),
        &obs_formats::DatasetOptions {
            resource_id: "openbible-xrefs".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let both = Temp::new("order");
    let s = session(&both.0, &[testing::bsb_parallels(), xrefs]);
    let order: Vec<&str> = s
        .library
        .of_type(obs_model::ResourceType::CrossReferenceSet)
        .map(|e| e.manifest.resource_id.as_str())
        .collect();
    assert_eq!(order, ["openbible-xrefs", "bsb-parallels"]);

    let empty = Session::new(JobMode::Inline);
    let missing = empty.guide_empty_state(&key, GuideSection::ParallelPassages);
    assert!(
        missing.message.contains("BSB Parallel Passages"),
        "{missing:?}"
    );
    assert!(missing.missing.offers_starter());
}

fn open(s: &mut Session, id: &str, params: serde_json::Value) -> TabId {
    TabId(s.run(id, &params).unwrap()["tab"].as_u64().unwrap())
}

/// Acceptance: clicking a parallel opens Text Comparison aligned on it — the parallel's
/// Verses, all compared Bibles in rows — and the Guide (and its Link set) stays put. A second
/// click reuses that Text Comparison; Ctrl/Cmd-click opens another.
#[test]
fn compare_parallel_opens_text_comparison_on_the_parallel_without_moving_the_guide() {
    let t = Temp::new("compare");
    let mut s = session(&t.0, &[testing::bsb_parallels()]);
    let guide = open(
        &mut s,
        "guide.passage_guide",
        json!({ "passage": "Mark 1:9-11" }),
    );
    s.run("tab.link", &json!({ "tab": guide.0, "set": "A" }))
        .unwrap();
    // A Text Comparison already in the Guide's Link set is left alone (it would move the Guide).
    let linked = open(&mut s, "tool.text_comparison", json!({}));
    s.run("tab.link", &json!({ "tab": linked.0, "set": "A" }))
        .unwrap();
    s.workspace.focus(guide);
    let shown = |s: &Session, tab: TabId| s.workspace.tab(tab).unwrap().shown_passage();
    let before = shown(&s, guide);

    let out = s
        .run("compare.parallel", &json!({ "passage": "Matt 3:13-17" }))
        .unwrap();
    let tc = TabId(out["tab"].as_u64().unwrap());
    assert_ne!(tc, linked);
    let tab = s.workspace.tab(tc).unwrap();
    assert_eq!(tab.kind, TabKind::Tool(Tool::TextComparison));
    assert_eq!(tab.link, None);
    assert_eq!(
        tab.history.current(),
        Some(Verse::new(Book::Matthew, 3, 13))
    );
    assert_eq!(
        tab.comparison.range,
        Some(passage(Book::Matthew, 3, 13, 17))
    );
    assert_eq!(out["range"], "Matt 3:13-17");
    assert_eq!(s.workspace.focused(), Some(tc));
    assert_ne!(
        s.workspace.panel_of(tc),
        s.workspace.panel_of(guide),
        "beside the Guide, not over it"
    );
    assert_eq!(shown(&s, guide), before, "the Guide stays at Mark 1:9-11");

    // The next parallel moves the same Text Comparison; Ctrl/Cmd-click opens another.
    s.workspace.focus(guide);
    let again = s
        .run("compare.parallel", &json!({ "passage": "Luke 3:21-22" }))
        .unwrap();
    assert_eq!(again["tab"].as_u64(), Some(tc.0));
    assert_eq!(
        s.workspace.tab(tc).unwrap().comparison.range,
        Some(passage(Book::Luke, 3, 21, 22))
    );
    let fresh = s
        .run(
            "compare.parallel",
            &json!({ "passage": "1 Chr 21", "new_tab": true }),
        )
        .unwrap();
    let fresh = TabId(fresh["tab"].as_u64().unwrap());
    assert_ne!(fresh, tc);
    // A whole chapter needs no range chip: the Tool shows the chapter.
    let tab = s.workspace.tab(fresh).unwrap();
    assert_eq!(tab.comparison.range, None);
    assert_eq!(
        tab.history.current(),
        Some(Verse::new(Book::FirstChronicles, 21, 1))
    );
    assert!(
        s.run("compare.parallel", &json!({ "passage": "nowhere" }))
            .is_err()
    );
}

/// Manual check against the built Starter set (`cargo xtask starter` writes `dist/starter`;
/// skips when it is absent): the acceptance Passages with the whole BSB, and the timing.
#[test]
#[ignore = "reads dist/starter/bsb-parallels.obr from a Starter build"]
fn starter_bsb_parallels_for_the_acceptance_passages() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../dist/starter/bsb-parallels.obr");
    if !path.exists() {
        eprintln!("no Starter build; skipped");
        return;
    }
    let mut library = Library::new();
    library.add_file(&path).unwrap();
    for (passage, expected) in [
        (
            passage(Book::Mark, 1, 9, 11),
            vec!["Matt 3:13-17", "Luke 3:21-22"],
        ),
        (
            Passage::chapter(&Standard, Book::SecondSamuel, 24).unwrap(),
            vec!["1 Chr 21"],
        ),
    ] {
        let t = std::time::Instant::now();
        let found = parallel_passages::library_parallels(&library, passage).unwrap();
        let elapsed = t.elapsed();
        println!("{} ({elapsed:?}):", short(passage));
        for line in rows(&found) {
            println!("  {line}");
        }
        let all: Vec<String> = found.all.iter().map(|p| short(*p)).collect();
        println!("  in all: {}", all.join("; "));
        let listed: Vec<String> = found.rows().map(|r| short(r.passage)).collect();
        for e in expected {
            assert!(
                listed.iter().chain(&all).any(|l| l == e),
                "{e}: {listed:?} {all:?}"
            );
        }
    }
}
