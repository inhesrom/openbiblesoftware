//! Word Study Translated as, Occurrences and Forms (#56), from open fixtures: KJV verses
//! (public domain) where G5485 is "grace" and "favour", the same verses as TAGNT tags them
//! (CC BY), and the Hebrew test Bible for an extended number.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use obs_model::Key;
use obs_scripture::{Book, StrongsNumber, Verse};
use serde_json::json;

use crate::word_sections::form_of;
use crate::*;

fn n(text: &str) -> StrongsNumber {
    text.parse().unwrap()
}

struct Temp(PathBuf);
impl Temp {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("obs-word-sections-{name}-{}", std::process::id()));
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

fn session(dir: &Path, resources: &[obs_model::Resource]) -> Session {
    for r in resources {
        let path = dir.join(format!("{}.obr", r.manifest.resource_id));
        obs_store::write_obr(r, &path).unwrap();
    }
    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(dir).is_empty());
    s
}

fn grace_session(t: &Temp) -> Session {
    session(&t.0, &[testing::kjv_grace(), testing::tagnt_grace()])
}

fn pump(s: &mut Session) {
    s.jobs().pump(16);
    assert!(s.poll_imports().is_empty());
}

fn section(s: &mut Session, number: &str, section: GuideSection) -> std::sync::Arc<SectionData> {
    let key = Key::StrongsNumber(n(number));
    for _ in 0..50 {
        match s.guide_section(&key, section) {
            SectionStatus::Ready(d) => return d,
            SectionStatus::Failed(e) => panic!("{e}"),
            SectionStatus::Loading => pump(s),
        }
    }
    panic!("{} never finished", section.title());
}

fn translated_as(s: &mut Session, number: &str) -> TranslatedAs {
    match &*section(s, number, GuideSection::TranslatedAs) {
        SectionData::TranslatedAs(t) => t.clone(),
        other => panic!("{other:?}"),
    }
}

fn occurrences(s: &mut Session, number: &str) -> OccurrenceList {
    match &*section(s, number, GuideSection::Occurrences) {
        SectionData::Occurrences(o) => o.clone(),
        other => panic!("{other:?}"),
    }
}

fn forms(s: &mut Session, number: &str) -> Forms {
    match &*section(s, number, GuideSection::Forms) {
        SectionData::Forms(f) => f.clone(),
        other => panic!("{other:?}"),
    }
}

fn v(book: Book, chapter: u16, verse: u16) -> Verse {
    Verse::new(book, chapter, verse)
}

fn renderings(t: &TranslatedAs) -> Vec<(&str, u32)> {
    t.renderings
        .iter()
        .map(|r| (r.rendering.as_str(), r.count))
        .collect()
}

#[test]
fn g5485_is_translated_grace_and_favour_in_the_kjv() {
    let t = Temp::new("translated");
    let mut s = grace_session(&t);
    let tr = translated_as(&mut s, "G5485");
    assert_eq!(renderings(&tr), [("grace", 6), ("favour", 3)]);
    assert_eq!(
        (tr.bible_id.as_str(), tr.bible_abbreviation.as_str()),
        ("kjv-grace", "KJV")
    );
    assert_eq!(tr.total(), 9);
    // Clicking a rendering opens Search for those Verses (docs/dev/search-tool.md).
    assert_eq!(
        tr.search_query("favour").as_deref(),
        Some("strong:G5485 \"favour\"")
    );
    // The section header counts the renderings.
    assert_eq!(
        SectionData::TranslatedAs(tr.clone()).count(),
        2,
        "two renderings"
    );
    // A number no English Bible renders: nothing, and the hint says why.
    assert!(translated_as(&mut s, "G9999").renderings.is_empty());
    assert_eq!(
        GuideSection::TranslatedAs.empty_hint(&s.library, &s.guides.indexes()),
        "None in your library: no English Bible with Strong's numbers translates this word."
    );
    drop(s);
}

#[test]
fn translated_as_follows_the_choosers_bible_and_an_extended_numbers_base() {
    // H1254A: the Hebrew Bible has no English, the KJV tags only H1254.
    let t = Temp::new("translated-base");
    let mut s = session(
        &t.0,
        &[
            testing::hebrew_bara(),
            testing::kjv_love(),
            testing::kjv_grace(),
        ],
    );
    let tr = translated_as(&mut s, "H1254A");
    assert_eq!(renderings(&tr), [("created", 1)]);
    assert_eq!(tr.counted, Some(n("H1254")));
    assert_eq!(tr.bible_id, "kjv-love");
    assert_eq!(
        tr.search_query("created").as_deref(),
        Some("strong:H1254 \"created\"")
    );
    // G5485 is rendered only by the grace Bible, even though the love Bible renders more
    // numbers and comes first.
    let tr = translated_as(&mut s, "G5485");
    assert_eq!(tr.bible_id, "kjv-grace");
    drop(s);
}

#[test]
fn occurrences_come_from_tagnt_per_book_with_forms_and_kjv_previews() {
    let t = Temp::new("occurrences");
    let mut s = grace_session(&t);
    let o = occurrences(&mut s, "G5485");
    assert_eq!(
        (o.bible_id.as_str(), o.bible_abbreviation.as_str()),
        ("tagnt-grace", "TAGNT")
    );
    assert_eq!(o.count(), 9);
    let all = o.filtered(None);
    assert_eq!(
        all.books,
        [
            (Book::Luke, 3),
            (Book::John, 3),
            (Book::Acts, 1),
            (Book::Romans, 1),
            (Book::Ephesians, 1)
        ]
    );
    let verses: Vec<Verse> = all.verses.iter().map(|r| r.verse).collect();
    assert_eq!(
        verses,
        [
            v(Book::Luke, 1, 30),
            v(Book::Luke, 2, 40),
            v(Book::Luke, 2, 52),
            v(Book::John, 1, 14),
            v(Book::John, 1, 16),
            v(Book::Acts, 2, 47),
            v(Book::Romans, 1, 5),
            v(Book::Ephesians, 2, 8)
        ]
    );
    // John 1:16 has two forms: "grace for grace".
    assert_eq!(all.verses[4].forms.len(), 2);
    // Filtering by the dative leaves Luke 2:52 and Ephesians 2:8.
    let dative = form_of("N-DSF").0;
    let only = o.filtered(Some(&dative));
    let verses: Vec<Verse> = only.verses.iter().map(|r| r.verse).collect();
    assert_eq!(verses, [v(Book::Luke, 2, 52), v(Book::Ephesians, 2, 8)]);
    assert_eq!(only.books, [(Book::Luke, 1), (Book::Ephesians, 1)]);
    assert_eq!(only.count, 2);
    assert!(o.filtered(Some("no such form")).verses.is_empty());
    // Show all in Search: the `strong:` query in the preview Bible, then TAGNT.
    assert_eq!(o.search_query(n("G5485")), "strong:G5485");
    assert_eq!(o.search_bibles(n("G5485")), "kjv-grace,tagnt-grace");

    // Previews come from the KJV with the word highlighted, read only when asked for.
    let bible = o.preview.clone().unwrap();
    assert_eq!(
        (bible.id.as_str(), bible.highlight),
        ("kjv-grace", n("G5485"))
    );
    let wanted = [v(Book::Luke, 1, 30), v(Book::John, 1, 16)];
    assert!(
        s.occurrence_previews(&bible, &wanted)
            .iter()
            .all(Option::is_none)
    );
    pump(&mut s);
    let got = s.occurrence_previews(&bible, &wanted);
    let lk = got[0].as_ref().unwrap();
    assert!(lk.text.starts_with("And the angel said unto her"), "{lk:?}");
    let marked: Vec<&str> = lk.highlights.iter().map(|r| &lk.text[r.clone()]).collect();
    assert_eq!(marked, ["favour"]);
    assert_eq!(lk.native, Some(v(Book::Luke, 1, 30)));
    let jn = got[1].as_ref().unwrap();
    let marked: Vec<&str> = jn.highlights.iter().map(|r| &jn.text[r.clone()]).collect();
    assert_eq!(marked, ["grace", "grace"]);
    drop(s);
}

#[test]
fn forms_are_counted_in_plain_english_and_collapsed_by_default() {
    let t = Temp::new("forms");
    let mut s = grace_session(&t);
    let f = forms(&mut s, "G5485");
    assert_eq!(f.bible_abbreviation, "TAGNT");
    let got: Vec<(String, u32)> = f.forms.iter().map(|f| (f.label.clone(), f.count)).collect();
    let label = |code: &str| form_of(code).0;
    assert_eq!(
        got,
        [
            (label("N-ASF"), 4),
            (label("N-DSF"), 2),
            (label("N-GSF"), 2),
            (label("N-NSF"), 1)
        ]
    );
    // Plain English, not the code; the code is kept for the tooltip.
    assert!(!got[0].0.contains("N-ASF"), "{got:?}");
    assert_eq!(got[0].0, "noun, acc. fem. sing.");
    assert_eq!(f.forms[0].codes, ["N-ASF"]);
    // The labels are the ones Occurrences rows carry, so a form filters them.
    let o = occurrences(&mut s, "G5485");
    for form in &f.forms {
        assert_eq!(o.filtered(Some(&form.label)).count, form.count);
    }
    // Forms starts collapsed; the other Word Study sections expanded, in #15's order.
    let a = s.guides.arrangement(Guide::WordStudy);
    let places: Vec<(&str, bool)> = a
        .sections
        .iter()
        .map(|p| (p.section.id(), p.collapsed))
        .collect();
    assert_eq!(
        places,
        [
            ("lexicon_entries", false),
            ("translated_as", false),
            ("occurrences", false),
            ("forms", true)
        ]
    );
    // An arrangement saved before #56 gains the new sections at their default places.
    let old = GuideArrangement::from_json(
        Guide::WordStudy,
        r#"{"sections":[{"id":"lexicon_entries","collapsed":true}]}"#,
    );
    assert_eq!(old, {
        let mut d = GuideArrangement::default_for(Guide::WordStudy);
        d.sections[0].collapsed = true;
        d
    });
    s.run(
        "guide.collapse_section",
        &json!({ "guide": "word_study", "section": "forms" }),
    )
    .unwrap();
    assert!(!s.guides.arrangement(Guide::WordStudy).sections[3].collapsed);
    drop(s);
}

#[test]
fn an_extended_number_counts_in_the_hebrew_bible_with_its_forms() {
    let t = Temp::new("hebrew");
    let mut s = session(&t.0, &[testing::kjv_love(), testing::hebrew_bara()]);
    let o = occurrences(&mut s, "H1254A");
    assert_eq!(o.bible_abbreviation, "HOT");
    assert_eq!(o.count(), 3);
    // Previews from the KJV, which tags the base number.
    let bible = o.preview.clone().unwrap();
    assert_eq!(
        (bible.id.as_str(), bible.highlight),
        ("kjv-love", n("H1254"))
    );
    let f = forms(&mut s, "H1254A");
    let got: Vec<(String, u32)> = f.forms.iter().map(|f| (f.label.clone(), f.count)).collect();
    assert_eq!(got, [(form_of("HVqw3ms").0, 2), (form_of("HVqp3ms").0, 1)]);
    assert!(got[1].0.starts_with("Qal perfect"), "{got:?}");
    drop(s);

    // Without an original-language Bible, Occurrences counts in the KJV and Forms is empty
    // with a hint naming what it needs.
    let t = Temp::new("kjv-only");
    let mut s = session(&t.0, &[testing::kjv_grace()]);
    let o = occurrences(&mut s, "G5485");
    assert_eq!((o.bible_id.as_str(), o.count()), ("kjv-grace", 9));
    assert!(o.forms.is_empty());
    assert_eq!(o.search_bibles(n("G5485")), "kjv-grace");
    assert!(forms(&mut s, "G5485").forms.is_empty());
    assert_eq!(
        GuideSection::Forms.empty_hint(&s.library, &s.guides.indexes()),
        "None in your library. Add a Hebrew or Greek Bible with Morphology to see this \
         word's forms: TAHOT and TAGNT come with the Starter library, and more are in \
         Get Resources › Hebrew & Greek."
    );
    assert!(occurrences(&mut s, "G9999").verses.is_empty());
    drop(s);
}

#[test]
fn an_occurrence_opens_its_bible_at_the_verse() {
    let t = Temp::new("open");
    let mut s = grace_session(&t);
    let out = s
        .run(
            "word_study.open_verse",
            &json!({ "bible": "kjv-grace", "verse": "Lk 2:40" }),
        )
        .unwrap();
    let tab = TabId(out["tab"].as_u64().unwrap());
    let shown = s.workspace.tab(tab).unwrap();
    assert_eq!(shown.kind, TabKind::Resource("kjv-grace".into()));
    assert_eq!(shown.history.current(), Some(v(Book::Luke, 2, 40)));
    // Again: the same Tab moves; Ctrl/Cmd-click opens another.
    let again = s
        .run(
            "word_study.open_verse",
            &json!({ "bible": "kjv-grace", "verse": "Acts 2:47" }),
        )
        .unwrap();
    assert_eq!(again["tab"].as_u64(), Some(tab.0));
    let other = s
        .run(
            "word_study.open_verse",
            &json!({ "bible": "kjv-grace", "verse": "Acts 2:47", "new_tab": true }),
        )
        .unwrap();
    assert_ne!(other["tab"].as_u64(), Some(tab.0));
    assert!(
        s.run(
            "word_study.open_verse",
            &json!({ "bible": "nope", "verse": "Acts 2:47" })
        )
        .is_err()
    );
    drop(s);
}

/// Psalm 3 numbered as the Hebrew Bible numbers it: the superscription is verse 1, so the
/// last verse is 9 (the Standard's 8).
fn hebrew_psalm_3() -> obs_model::Resource {
    let texts = [
        "A Psalm of David, when he fled from Absalom his son.",
        "LORD, how are they increased that trouble me!",
        "Many there be which say of my soul,",
        "But thou, O LORD, art a shield for me;",
        "I cried unto the LORD with my voice,",
        "I laid me down and slept;",
        "I will not be afraid of ten thousands of people,",
        "Arise, O LORD; save me, O my God:",
        "Salvation belongeth unto the LORD: thy blessing is upon thy people. Selah.",
    ];
    let verses: Vec<(Verse, &str)> = texts
        .iter()
        .enumerate()
        .map(|(i, t)| (v(Book::Psalms, 3, i as u16 + 1), *t))
        .collect();
    let standard: Vec<Option<Verse>> = (0..texts.len())
        .map(|i| Some(v(Book::Psalms, 3, i as u16)))
        .collect();
    testing::bibles::bible(
        "heb-ps",
        "HPS",
        &verses,
        Some(&standard),
        &[("LORD", "H3068")],
    )
}

#[test]
fn an_occurrence_opens_at_a_verse_the_standard_does_not_have() {
    // Ps 3:9 in Hebrew numbering is Standard Ps 3:8; Standard Ps 3 has no verse 9, so the
    // reference is read in the Bible's own numbering.
    let t = Temp::new("open-native");
    let mut s = session(&t.0, &[hebrew_psalm_3()]);
    let out = s
        .run(
            "word_study.open_verse",
            &json!({ "bible": "heb-ps", "verse": "Ps 3:9" }),
        )
        .unwrap();
    let tab = TabId(out["tab"].as_u64().unwrap());
    assert_eq!(
        s.workspace.tab(tab).unwrap().history.current(),
        Some(v(Book::Psalms, 3, 9))
    );
    // A Verse the Bible itself lacks is still refused.
    assert!(
        s.run(
            "word_study.open_verse",
            &json!({ "bible": "heb-ps", "verse": "Ps 3:10" })
        )
        .is_err()
    );
    drop(s);
}

#[test]
fn show_all_in_search_looks_for_the_number_each_bible_tags() {
    // With the Hebrew Bible, H1254A is counted there; the KJV tags only H1254, so Search
    // for `strong:H1254A` would find nothing in it and it is left out.
    let t = Temp::new("search-extended");
    let mut s = session(&t.0, &[testing::kjv_love(), testing::hebrew_bara()]);
    let number = n("H1254A");
    let o = occurrences(&mut s, "H1254A");
    assert_eq!(o.search_query(number), "strong:H1254A");
    assert_eq!(o.search_bibles(number), "heb-bara");
    drop(s);

    // KJV only: H1254A is counted as H1254, so Search looks for H1254 in the KJV.
    let t = Temp::new("search-base");
    let mut s = session(&t.0, &[testing::kjv_love()]);
    let o = occurrences(&mut s, "H1254A");
    assert_eq!(o.counted, Some(n("H1254")));
    assert_eq!(o.search_query(number), "strong:H1254");
    assert_eq!(o.search_bibles(number), "kjv-love");
    drop(s);
}

#[test]
fn a_preview_that_cannot_be_read_says_so() {
    let t = Temp::new("preview-failed");
    let mut s = grace_session(&t);
    let o = occurrences(&mut s, "G5485");
    let bible = o.preview.clone().unwrap();
    assert_eq!(bible.id, "kjv-grace");
    // The preview Bible's file goes away before its previews are read.
    std::fs::remove_file(t.0.join("kjv-grace.obr")).unwrap();
    let wanted = [v(Book::Luke, 1, 30)];
    s.occurrence_previews(&bible, &wanted);
    pump(&mut s);
    let got = s.occurrence_previews(&bible, &wanted);
    let p = got[0].as_ref().unwrap();
    assert!(p.text.is_empty());
    assert!(p.error.is_some(), "{p:?}");
    drop(s);
}

#[test]
fn empty_hints_ask_for_a_bible_with_strongs_numbers() {
    // Only TAGNT: no English Bible with Strong's numbers, so Translated as suggests one.
    let t = Temp::new("hint-tagnt");
    let mut s = session(&t.0, &[testing::tagnt_grace()]);
    assert!(translated_as(&mut s, "G5485").renderings.is_empty());
    assert_eq!(
        GuideSection::TranslatedAs.empty_hint(&s.library, &s.guides.indexes()),
        "None in your library. Add an English Bible with Strong's numbers (the KJV with \
         Strong's) to see how this word is translated."
    );
    // TAGNT has Strong's numbers, so an Occurrences miss says nothing has the word.
    assert!(occurrences(&mut s, "G9999").verses.is_empty());
    assert_eq!(
        GuideSection::Occurrences.empty_hint(&s.library, &s.guides.indexes()),
        "None in your library: no Bible with Strong's numbers has this word."
    );
    drop(s);

    // Only an untagged English Bible: both sections suggest adding a tagged one.
    let t = Temp::new("hint-untagged");
    let web = testing::bibles::bible(
        "web",
        "WEB",
        &[(v(Book::Luke, 1, 30), "you have found favor with God.")],
        None,
        &[],
    );
    let mut s = session(&t.0, &[web]);
    assert!(translated_as(&mut s, "G5485").renderings.is_empty());
    assert!(occurrences(&mut s, "G5485").verses.is_empty());
    assert_eq!(
        GuideSection::TranslatedAs.empty_hint(&s.library, &s.guides.indexes()),
        "None in your library. Add an English Bible with Strong's numbers (the KJV with \
         Strong's) to see how this word is translated."
    );
    assert_eq!(
        GuideSection::Occurrences.empty_hint(&s.library, &s.guides.indexes()),
        "None in your library. Add a Bible with Strong's numbers to see where this word \
         occurs."
    );
    drop(s);
}

#[test]
fn a_verse_map_that_does_not_line_up_is_read_verse_by_verse() {
    use obs_scripture::{Passage, VerseMap, VerseMapEntry};
    let unit = obs_model::UnitId::book(Book::Psalms);
    let verse = |verse: Verse, start: u32| obs_model::BibleVerse {
        verse,
        last: verse.verse,
        unit: unit.clone(),
        start,
        end: start + 1,
    };
    let verses = [
        verse(v(Book::Psalms, 2, 12), 0),
        verse(v(Book::Psalms, 3, 2), 2),
        verse(v(Book::Psalms, 3, 9), 4),
    ];
    // Entries for the last two Verses only: by position they would land on the wrong ones.
    let entry = |native: Verse, standard: Verse| VerseMapEntry {
        verse: native,
        last: native.verse,
        standard: Some(Passage::verse(standard)),
    };
    let map = VerseMap::new(
        vec![
            entry(v(Book::Psalms, 3, 2), v(Book::Psalms, 3, 1)),
            entry(v(Book::Psalms, 3, 9), v(Book::Psalms, 3, 8)),
        ],
        Vec::new(),
        &obs_scripture::Standard,
    );
    assert_eq!(
        crate::word_sections::standard_verses(&verses, Some(&map)),
        [
            (v(Book::Psalms, 2, 12), Some(v(Book::Psalms, 2, 12))),
            (v(Book::Psalms, 3, 2), Some(v(Book::Psalms, 3, 1))),
            (v(Book::Psalms, 3, 9), Some(v(Book::Psalms, 3, 8))),
        ]
    );
    // Lined up, the map is read by position: an Unmapped verse stays Unmapped.
    let unmapped = VerseMap::new(
        vec![
            VerseMapEntry {
                verse: v(Book::Psalms, 2, 12),
                last: 12,
                standard: None,
            },
            entry(v(Book::Psalms, 3, 2), v(Book::Psalms, 3, 1)),
            entry(v(Book::Psalms, 3, 9), v(Book::Psalms, 3, 8)),
        ],
        Vec::new(),
        &obs_scripture::Standard,
    );
    assert_eq!(
        crate::word_sections::standard_verses(&verses, Some(&unmapped))[0],
        (v(Book::Psalms, 2, 12), None)
    );
}

fn starter_library() -> Option<PathBuf> {
    let dir = default_library_dir()?;
    (dir.join("kjv-strongs.obr").is_file() && dir.join("tagnt.obr").is_file()).then_some(dir)
}

/// Real Starter check (manual): `cargo test --release -p obs-engine --lib
/// word_sections_tests::starter_library -- --ignored --nocapture`.
#[test]
#[ignore = "needs the installed Starter library"]
fn starter_library_g5485_and_h1254a() {
    let Some(dir) = starter_library() else {
        eprintln!("no Starter library; skipped");
        return;
    };
    // The Starter library only: a locally imported Bible (NASB) is left out so the KJV is
    // the tagged English Bible, as in a fresh install.
    let mut s = Session::new(JobMode::Inline);
    s.library.scan(&dir);
    s.library.remove("nasb1995");
    let started = std::time::Instant::now();
    let tr = translated_as(&mut s, "G5485");
    eprintln!(
        "G5485 Translated as in {:?} from {}: {:?}",
        started.elapsed(),
        tr.bible_abbreviation,
        renderings(&tr)
    );
    let top: Vec<&str> = tr
        .renderings
        .iter()
        .take(2)
        .map(|r| r.rendering.as_str())
        .collect();
    assert_eq!(tr.bible_id, "kjv-strongs");
    assert_eq!(top, ["grace", "favour"]);
    let started = std::time::Instant::now();
    let o = occurrences(&mut s, "G5485");
    eprintln!(
        "G5485 Occurrences in {:?}: {} in {} verses of {}; books {:?}; preview {:?}",
        started.elapsed(),
        o.count(),
        o.verses.len(),
        o.bible_abbreviation,
        o.filtered(None).books,
        o.preview
    );
    assert_eq!(o.bible_id, "tagnt");
    let started = std::time::Instant::now();
    let f = forms(&mut s, "G5485");
    eprintln!("G5485 Forms in {:?}:", started.elapsed());
    for form in &f.forms {
        eprintln!("  {} ×{} {:?}", form.label, form.count, form.codes);
    }
    let bible = o.preview.clone().unwrap();
    let verses: Vec<Verse> = o
        .verses
        .iter()
        .take(OCCURRENCE_ROWS)
        .map(|r| r.verse)
        .collect();
    let started = std::time::Instant::now();
    s.occurrence_previews(&bible, &verses);
    pump(&mut s);
    let previews = s.occurrence_previews(&bible, &verses);
    eprintln!("50 previews in {:?}", started.elapsed());
    let first = previews[0].as_ref().unwrap();
    eprintln!("first preview: {first:?}");
    assert!(!first.highlights.is_empty());

    let started = std::time::Instant::now();
    let o = occurrences(&mut s, "H1254A");
    eprintln!(
        "H1254A Occurrences in {:?}: {} in {}",
        started.elapsed(),
        o.count(),
        o.bible_abbreviation
    );
    let f = forms(&mut s, "H1254A");
    for form in &f.forms {
        eprintln!("  {} ×{} {:?}", form.label, form.count, form.codes);
    }
    let tr = translated_as(&mut s, "H1254A");
    eprintln!(
        "H1254A Translated as ({}): {:?}",
        tr.bible_abbreviation,
        renderings(&tr)
    );
    let started = std::time::Instant::now();
    let o = occurrences(&mut s, "H3068");
    eprintln!(
        "H3068 Occurrences (warm TAHOT) in {:?}: {} in {} verses",
        started.elapsed(),
        o.count(),
        o.verses.len()
    );
    drop(s);
}
