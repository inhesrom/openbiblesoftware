//! Word Study (#55): the box, the English-word chooser, the header, the Lexicon entries
//! section in Preferred order, and the ways in (Word Info, Link sets, history).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use obs_model::{Key, MorphemePart};
use obs_scripture::StrongsNumber;
use serde_json::json;

use crate::word_study::{Chooser, WordStudyHeader, parse_input};
use crate::*;

fn n(text: &str) -> StrongsNumber {
    text.parse().unwrap()
}

struct Temp(PathBuf);
impl Temp {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("obs-word-study-{name}-{}", std::process::id()));
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

fn write(dir: &Path, resource: &obs_model::Resource) {
    let path = dir.join(format!("{}.obr", resource.manifest.resource_id));
    obs_store::write_obr(resource, &path).unwrap();
}

/// A library of the KJV-style test Bible, the Hebrew test Bible (when `hebrew`) and the
/// Starter Lexicon excerpts, scanned from disk in file-name order (bdb, strongs-hebrew,
/// tbesg, tbesh) as the app scans the Starter library.
fn session(t: &Temp, hebrew: bool) -> Session {
    write(&t.0, &testing::kjv_love());
    if hebrew {
        write(&t.0, &testing::hebrew_bara());
    }
    for lexicon in testing::starter_lexicon_excerpts() {
        write(&t.0, &lexicon);
    }
    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(&t.0).is_empty());
    s
}

fn pump(s: &mut Session) {
    s.jobs().pump(16);
    assert!(
        s.poll_imports().is_empty(),
        "Word Study events are applied, not returned"
    );
}

fn header(s: &mut Session, number: &str) -> Arc<WordStudyHeader> {
    for _ in 0..50 {
        match s.word_study_header(n(number)) {
            WordStatus::Ready(h) => return h,
            WordStatus::Failed(e) => panic!("{e}"),
            WordStatus::Loading => pump(s),
        }
    }
    panic!("the header never finished");
}

fn chooser(s: &mut Session, words: &str) -> Arc<Chooser> {
    for _ in 0..50 {
        match s.word_chooser(words) {
            WordStatus::Ready(c) => return c,
            WordStatus::Failed(e) => panic!("{e}"),
            WordStatus::Loading => pump(s),
        }
    }
    panic!("the chooser never finished");
}

fn lexicon_entries(s: &mut Session, number: &str) -> Vec<LexiconEntry> {
    let key = Key::StrongsNumber(n(number));
    for _ in 0..50 {
        match s.guide_section(&key, GuideSection::LexiconEntries) {
            SectionStatus::Ready(d) => match &*d {
                SectionData::LexiconEntries(e) => return e.clone(),
                other => panic!("{other:?}"),
            },
            SectionStatus::Failed(e) => panic!("{e}"),
            SectionStatus::Loading => pump(s),
        }
    }
    panic!("Lexicon entries never finished");
}

fn open_word_study(s: &mut Session) -> TabId {
    let out = s.run("guide.word_study", &json!({})).unwrap();
    TabId(out["tab"].as_u64().unwrap())
}

fn key_of(s: &Session, tab: TabId) -> Option<Key> {
    s.workspace.tab(tab).unwrap().entry_history.current()
}

#[test]
fn the_box_reads_strongs_numbers_or_english_words() {
    assert_eq!(
        parse_input("G5485"),
        Ok(WordStudyInput::Strongs(n("G5485")))
    );
    assert_eq!(
        parse_input(" h1254a "),
        Ok(WordStudyInput::Strongs(n("H1254A")))
    );
    assert_eq!(
        parse_input("H1254"),
        Ok(WordStudyInput::Strongs(n("H1254")))
    );
    assert_eq!(
        parse_input("love"),
        Ok(WordStudyInput::Words("love".into()))
    );
    assert_eq!(
        parse_input("everlasting life"),
        Ok(WordStudyInput::Words("everlasting life".into()))
    );
    // Hebrew and Greek are words too; a mistyped number is not.
    assert_eq!(
        parse_input("Holy"),
        Ok(WordStudyInput::Words("Holy".into()))
    );
    assert!(parse_input("G5485AB").is_err());
    assert!(parse_input("  ").is_err());
}

#[test]
fn typing_love_offers_g25_g26_and_g5368_with_counts() {
    let t = Temp::new("love");
    let mut s = session(&t, true);
    let tab = open_word_study(&mut s);
    let out = s
        .run("tab.go", &json!({ "tab": tab.0, "text": "love" }))
        .unwrap();
    assert_eq!(out["choose"], "love");
    assert_eq!(s.word_study.choosing(tab), Some("love"));
    let c = chooser(&mut s, "love");
    let offered: Vec<(String, u32)> = c
        .choices
        .iter()
        .map(|c| (c.strongs.to_string(), c.count))
        .collect();
    assert_eq!(
        offered,
        [("G25".into(), 3), ("G26".into(), 2), ("G5368".into(), 2)]
    );
    // Counts are from the tagged English Bible; the Hebrew Bible has no English renderings.
    assert_eq!(c.bible, Some(("kjv-love".into(), "KJV".into())));
    // Each choice carries the first Lexicon's Lemma and gloss when one has it.
    let agape = &c.choices[1];
    assert_eq!(agape.lemma.as_deref(), Some("ἀγάπη"));
    assert!(agape.gloss.as_deref().is_some_and(|g| g.contains("love")));
    // Several numbers: the Guide waits for the user's choice.
    assert_eq!(s.word_study.choosing(tab), Some("love"));
    assert_eq!(key_of(&s, tab), None);
    // Choosing one is typing its number: the chooser closes and the Guide goes there.
    s.run("tab.go", &json!({ "tab": tab.0, "text": "G26" }))
        .unwrap();
    assert_eq!(s.word_study.choosing(tab), None);
    assert_eq!(key_of(&s, tab), Some(Key::StrongsNumber(n("G26"))));
    // Case and inflection: "Love" is "love"; "loved" is its own rendering.
    assert_eq!(chooser(&mut s, "Love").choices.len(), 3);
    let loved = chooser(&mut s, "loved");
    assert_eq!(loved.choices.len(), 1);
    assert_eq!(loved.choices[0].count, 1);
    drop(s);
}

#[test]
fn words_with_one_number_go_straight_to_it_and_unknown_words_say_so() {
    let t = Temp::new("one");
    let mut s = session(&t, false);
    let tab = open_word_study(&mut s);
    s.run("tab.go", &json!({ "tab": tab.0, "text": "created" }))
        .unwrap();
    for _ in 0..10 {
        pump(&mut s);
        let _ = s.word_chooser("created");
    }
    assert_eq!(key_of(&s, tab), Some(Key::StrongsNumber(n("H1254"))));
    assert_eq!(s.word_study.choosing(tab), None);

    s.run("tab.go", &json!({ "tab": tab.0, "text": "charity" }))
        .unwrap();
    let c = chooser(&mut s, "charity");
    assert!(c.choices.is_empty());
    assert!(c.bible.is_none());
    assert!(c.has_tagged_bible);
    // The Guide stays where it was, still choosing.
    assert_eq!(key_of(&s, tab), Some(Key::StrongsNumber(n("H1254"))));
    assert_eq!(s.word_study.choosing(tab), Some("charity"));
    // Back closes the chooser too.
    s.run("tab.back", &json!({ "tab": tab.0 })).unwrap();
    assert_eq!(s.word_study.choosing(tab), None);
    drop(s);
}

#[test]
fn h1254a_lists_tbesh_then_bdb_then_strongs() {
    // The Starter Preferred order for Lexicons (decision #15: TBESH/TBESG, BDB, Abbott-Smith,
    // Strong's) applies although the files scan as bdb, strongs-hebrew, tbesg, tbesh.
    let t = Temp::new("order");
    let mut s = session(&t, true);
    let lexicons: Vec<&str> = s
        .library
        .of_type(obs_model::ResourceType::Lexicon)
        .map(|e| e.manifest.resource_id.as_str())
        .collect();
    assert_eq!(lexicons, ["tbesh", "tbesg", "bdb", "strongs-hebrew"]);
    let entries = lexicon_entries(&mut s, "H1254A");
    let found: Vec<(&str, String)> = entries
        .iter()
        .map(|e| (e.resource_id.as_str(), e.key.to_string()))
        .collect();
    // TBESH and BDB have the extended number; Strong's answers with its base number.
    assert_eq!(
        found,
        [
            ("tbesh", "H1254A".to_string()),
            ("bdb", "H1254A".to_string()),
            ("strongs-hebrew", "H1254".to_string()),
        ]
    );
    assert_eq!(entries[0].headword.as_deref(), Some("בָּרָא"));
    assert!(entries[0].text.contains("to create"), "{}", entries[0].text);

    // A base number finds every homograph TBESH lists under it.
    let base: Vec<&str> = lexicon_entries(&mut s, "H1254")
        .iter()
        .filter(|e| e.resource_id == "tbesh")
        .map(|e| e.unit.as_str())
        .collect::<Vec<_>>()
        .into_iter()
        .map(|u| if u.ends_with('A') { "A" } else { "other" })
        .collect();
    assert!(base.contains(&"A") && base.len() >= 2, "{base:?}");

    // The user's own order wins.
    s.run("library.move", &json!({ "id": "strongs-hebrew", "to": 0 }))
        .unwrap();
    let first = lexicon_entries(&mut s, "H1254A")[0].resource_id.clone();
    assert_eq!(first, "strongs-hebrew");

    // Nothing for a number no Lexicon has; no Lexicon at all names the kind it needs.
    assert!(lexicon_entries(&mut s, "G9999").is_empty());
    assert_eq!(
        GuideSection::LexiconEntries.empty_hint(&Library::new()),
        "None in your library. Add a Lexicon to read its entry for this word."
    );
    drop(s);
}

#[test]
fn the_header_shows_lemma_transliteration_gloss_part_of_speech_and_occurrences() {
    let t = Temp::new("header");
    let mut s = session(&t, true);
    let h = header(&mut s, "H1254A");
    assert_eq!(h.strongs, n("H1254A"));
    assert_eq!(h.lemma.as_deref(), Some("בָּרָא"));
    assert_eq!(h.transliteration.as_deref(), Some("bara"));
    let gloss = h.gloss.as_ref().unwrap();
    assert_eq!(gloss.abbreviation, "TBESH");
    assert_eq!(gloss.text, "to create");
    assert_eq!(h.part_of_speech.as_deref(), Some("verb"));
    // Counted in the Hebrew Bible, the number's original language, before the KJV.
    let o = h.occurrences.as_ref().unwrap();
    assert_eq!((o.count, o.abbreviation.as_str()), (3, "HOT"));
    assert_eq!(o.counted, n("H1254A"));

    // A Greek number with only the English Bible tagging it: its count there.
    let g = header(&mut s, "G26");
    assert_eq!(g.lemma.as_deref(), Some("ἀγάπη"));
    assert_eq!(g.transliteration.as_deref(), Some("agape"));
    let o = g.occurrences.as_ref().unwrap();
    assert_eq!((o.count, o.abbreviation.as_str()), (2, "KJV"));
    assert_eq!(g.part_of_speech, None);
    drop(s);

    // Without the Hebrew Bible, H1254A is counted by its base number in the KJV, and the
    // KJV's unreadable codes give no part of speech.
    let t = Temp::new("header-kjv");
    let mut s = session(&t, false);
    let h = header(&mut s, "H1254A");
    let o = h.occurrences.as_ref().unwrap();
    assert_eq!((o.count, o.counted), (1, n("H1254")));
    assert_eq!(h.part_of_speech, None);
    // A number nothing has: just the number.
    let none = header(&mut s, "G9999");
    assert_eq!(none.lemma, None);
    assert_eq!(none.occurrences, None);
    drop(s);
}

#[test]
fn word_info_opens_the_shown_word_in_a_word_study() {
    let t = Temp::new("word-info");
    let mut s = session(&t, false);
    assert!(!s.is_enabled("word_info.word_study"));
    let mut part = MorphemePart::new(n("G26"));
    part.gloss = Some("love".into());
    let word = ShownWord::new("love", false, "John 15:13 (KJV)".into(), &[part], |_| None).unwrap();
    s.word_info.pin(word);
    assert!(s.is_enabled("word_info.word_study"));
    let out = s.run("word_info.word_study", &json!({})).unwrap();
    let tab = TabId(out["tab"].as_u64().unwrap());
    assert_eq!(
        s.workspace.tab(tab).unwrap().kind,
        TabKind::Guide(Guide::WordStudy)
    );
    assert_eq!(key_of(&s, tab), Some(Key::StrongsNumber(n("G26"))));
    // Again with another number: the same Word Study moves (history keeps the old one).
    let out = s
        .run("word_study.open", &json!({ "strongs": "G25" }))
        .unwrap();
    assert_eq!(out["tab"].as_u64(), Some(tab.0));
    assert_eq!(key_of(&s, tab), Some(Key::StrongsNumber(n("G25"))));
    s.run("tab.back", &json!({ "tab": tab.0 })).unwrap();
    assert_eq!(key_of(&s, tab), Some(Key::StrongsNumber(n("G26"))));
    s.run("tab.forward", &json!({ "tab": tab.0 })).unwrap();
    assert_eq!(key_of(&s, tab), Some(Key::StrongsNumber(n("G25"))));
    // Ctrl/Cmd-click: a new Word Study.
    let out = s
        .run(
            "word_study.open",
            &json!({ "strongs": "H1254A", "new_tab": true }),
        )
        .unwrap();
    assert_ne!(out["tab"].as_u64(), Some(tab.0));
    assert!(
        s.run("word_study.open", &json!({ "strongs": "love" }))
            .is_err()
    );
    // The Guides menu starts a new Word Study at the word Word Info shows.
    let fresh = open_word_study(&mut s);
    assert_ne!(fresh, tab);
    assert_eq!(key_of(&s, fresh), Some(Key::StrongsNumber(n("G26"))));
    drop(s);
}

#[test]
fn linked_word_studies_follow_each_other() {
    let mut s = Session::new(JobMode::Inline);
    let a = open_word_study(&mut s);
    let b = open_word_study(&mut s);
    s.run("tab.link", &json!({ "tab": a.0, "set": "B" }))
        .unwrap();
    s.run("tab.link", &json!({ "tab": b.0, "set": "B" }))
        .unwrap();
    s.run("tab.go", &json!({ "tab": a.0, "text": "G5485" }))
        .unwrap();
    assert_eq!(key_of(&s, b), Some(Key::StrongsNumber(n("G5485"))));
    // A Word Study opened from a linked one joins its set and moves it.
    let c = s
        .run(
            "word_study.open",
            &json!({ "strongs": "G1", "new_tab": true }),
        )
        .unwrap();
    let c = TabId(c["tab"].as_u64().unwrap());
    assert_eq!(s.workspace.tab(c).unwrap().link, Some('B'));
    assert_eq!(key_of(&s, a), Some(Key::StrongsNumber(n("G1"))));
    // One joining a set later starts at the set's number.
    s.run("tab.link", &json!({ "tab": c.0, "set": "" }))
        .unwrap();
    s.run("tab.go", &json!({ "tab": c.0, "text": "G2" }))
        .unwrap();
    assert_eq!(key_of(&s, a), Some(Key::StrongsNumber(n("G1"))));
    s.run("tab.link", &json!({ "tab": c.0, "set": "B" }))
        .unwrap();
    assert_eq!(key_of(&s, c), Some(Key::StrongsNumber(n("G1"))));
}

/// The installed Starter library, when this machine has one.
fn starter_library() -> Option<PathBuf> {
    let dir = default_library_dir()?;
    dir.join("kjv-strongs.obr").is_file().then_some(dir)
}

#[test]
#[ignore = "needs the installed Starter library"]
fn starter_library_love_and_h1254a() {
    let Some(dir) = starter_library() else {
        eprintln!("no Starter library; skipped");
        return;
    };
    let mut s = Session::new(JobMode::Inline);
    s.library.scan(&dir);
    let started = std::time::Instant::now();
    let c = chooser(&mut s, "love");
    eprintln!("chooser for “love” in {:?}: {c:#?}", started.elapsed());
    let offered: Vec<String> = c.choices.iter().map(|c| c.strongs.to_string()).collect();
    for wanted in ["G25", "G26", "G5368"] {
        assert!(offered.contains(&wanted.to_string()), "{offered:?}");
    }
    assert!(c.choices.iter().all(|c| c.count > 0));
    let started = std::time::Instant::now();
    let warm = chooser(&mut s, "beloved");
    eprintln!(
        "warm chooser in {:?}: {} numbers",
        started.elapsed(),
        warm.choices.len()
    );

    let started = std::time::Instant::now();
    let entries = lexicon_entries(&mut s, "H1254A");
    eprintln!("Lexicon entries in {:?}", started.elapsed());
    let order: Vec<&str> = entries.iter().map(|e| e.resource_id.as_str()).collect();
    eprintln!("H1254A Lexicon entries: {order:?}");
    let at = |id: &str| order.iter().position(|o| *o == id).unwrap();
    assert_eq!(order[0], "tbesh");
    assert!(
        at("tbesh") < at("bdb") && at("bdb") < at("strongs-hebrew"),
        "{order:?}"
    );
    let h = header(&mut s, "H1254A");
    eprintln!("H1254A header: {h:#?}");
    assert_eq!(h.transliteration.as_deref(), Some("bara"));
    assert!(h.occurrences.as_ref().is_some_and(|o| o.count > 0));
    drop(s);
}

#[test]
fn a_word_study_comes_back_at_its_number() {
    let mut s = Session::new(JobMode::Inline);
    let tab = open_word_study(&mut s);
    s.run("tab.go", &json!({ "tab": tab.0, "text": "H1254A" }))
        .unwrap();
    let json = s.workspace.snapshot().to_json();
    let mut restored = Session::new(JobMode::Inline);
    restored
        .workspace
        .restore(WorkspaceSnapshot::from_json(&json).unwrap());
    assert_eq!(
        key_of(&restored, tab),
        Some(Key::StrongsNumber(n("H1254A")))
    );
}
