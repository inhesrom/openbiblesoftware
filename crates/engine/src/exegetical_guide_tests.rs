//! The Exegetical Guide (#144): Word by word, Lexicon summaries and Grammatical notes from
//! open fixtures (STEPBible TAGNT John 3:16, the TAHOT Psalms excerpt), the Guide's command
//! and Link set, and an ignored check against the real Starter library.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use obs_model::{Key, TermKind};
use obs_scripture::{Book, Passage, Verse};
use serde_json::json;

use crate::testing::bibles::{bsb_psalm_3, tahot_psalms};
use crate::testing::{greek_lexicon, kjv_john, tagnt_john_3_16};
use crate::*;

fn john(c: u16, v: u16) -> Verse {
    Verse::new(Book::John, c, v)
}

fn ps3(v: u16) -> Verse {
    Verse::new(Book::Psalms, 3, v)
}

struct Temp(PathBuf);
impl Temp {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("obs-exegetical-{name}-{}", std::process::id()));
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
        obs_store::write_obr(r, &dir.join(format!("{}.obr", r.manifest.resource_id))).unwrap();
    }
    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(dir).is_empty());
    s
}

fn settle(s: &mut Session, key: &Key, section: GuideSection) -> Arc<SectionData> {
    for _ in 0..50 {
        match s.guide_section(key, section) {
            SectionStatus::Loading => {}
            SectionStatus::Ready(d) => return d,
            SectionStatus::Failed(e) => panic!("{section:?} failed: {e}"),
        }
        s.jobs().pump(4);
        s.poll_imports();
    }
    panic!("{section:?} never finished");
}

fn word_by_word(s: &mut Session, passage: Passage) -> WordByWord {
    match &*settle(s, &Key::Passage(passage), GuideSection::WordByWord) {
        SectionData::WordByWord(w) => w.clone(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_exegetical_guide_has_its_three_sections_in_order() {
    assert_eq!(Guide::ExegeticalGuide.id(), "exegetical_guide");
    assert_eq!(Guide::ExegeticalGuide.title(), "Exegetical Guide");
    assert_eq!(
        Guide::from_id("exegetical_guide"),
        Some(Guide::ExegeticalGuide)
    );
    let titles: Vec<&str> = Guide::ExegeticalGuide
        .sections()
        .iter()
        .map(|s| s.title())
        .collect();
    assert_eq!(
        titles,
        ["Word by word", "Lexicon summaries", "Grammatical notes"]
    );
    for s in Guide::ExegeticalGuide.sections() {
        assert_eq!(GuideSection::from_id(s.id()), Some(*s));
    }
    // A Passage-keyed Guide: it follows its Link set like the Passage Guide.
    assert_eq!(
        TabKind::Guide(Guide::ExegeticalGuide).default_key_schemes(),
        [obs_model::KeyScheme::Passage]
    );
    assert!(TabKind::Guide(Guide::ExegeticalGuide).has_link_chip());
    assert_eq!(
        TabKind::Guide(Guide::ExegeticalGuide).header_box(),
        HeaderBox::Reference
    );
}

/// Acceptance: the Exegetical Guide for Jn 3:16 lists each TAGNT word with Lemma,
/// Morphology and Gloss in order.
#[test]
fn john_3_16_lists_each_tagnt_word_with_lemma_morphology_and_gloss_in_order() {
    let t = Temp::new("jn316");
    // The English KJV comes first in Preferred order; the Guide still reads the Greek.
    let mut s = session(&t.0, &[kjv_john(), tagnt_john_3_16(), greek_lexicon()]);
    let w = word_by_word(&mut s, Passage::verse(john(3, 16)));
    assert_eq!(w.verses.len(), 1);
    let v = &w.verses[0];
    assert_eq!((v.standard, v.native), (john(3, 16), john(3, 16)));
    assert_eq!(v.bible_abbreviation, "TAGNT");
    assert_eq!(v.reference(), "John 3:16 (TAGNT)");
    assert!(!v.renumbered());
    let rows: Vec<(&str, &str, &str, &str, &str)> = v
        .words
        .iter()
        .map(|w| {
            (
                w.written.as_str(),
                w.lemma.as_deref().unwrap(),
                w.morph_code.as_deref().unwrap(),
                w.gloss.as_deref().unwrap(),
                w.morphology.as_deref().unwrap(),
            )
        })
        .collect();
    let expected: [(&str, &str, &str, &str); 25] = [
        ("οὕτως", "οὕτω", "ADV", "Thus"),
        ("γὰρ", "γάρ", "CONJ", "for"),
        ("ἠγάπησεν", "ἀγαπάω", "V-AAI-3S", "loved"),
        ("ὁ", "ὁ", "T-NSM", "the"),
        ("θεὸς", "θεός", "N-NSM-T", "God"),
        ("τὸν", "ὁ", "T-ASM", "the"),
        ("κόσμον", "κόσμος", "N-ASM", "world,"),
        ("ὥστε", "ὥστε", "CONJ", "that"),
        ("τὸν", "ὁ", "T-ASM", "the"),
        ("υἱὸν", "υἱός", "N-ASM", "Son"),
        ("τὸν", "ὁ", "T-ASM", "the"),
        ("μονογενῆ", "μονογενής", "A-ASM", "only begotten"),
        ("ἔδωκεν", "δίδωμι", "V-AAI-3S", "He gave,"),
        ("ἵνα", "ἵνα", "CONJ", "so that"),
        ("πᾶς", "πᾶς", "A-NSM", "everyone"),
        ("ὁ", "ὁ", "T-NSM", "who"),
        ("πιστεύων", "πιστεύω", "V-PAP-NSM", "is believing"),
        ("εἰς", "εἰς", "PREP", "in"),
        ("αὐτὸν", "αὐτός", "P-ASM", "Him"),
        ("μὴ", "μή", "PRT-N", "not"),
        ("ἀπόληται", "ἀπολλύω", "V-2AMS-3S", "may perish"),
        ("ἀλλ᾽", "ἀλλά", "CONJ", "but"),
        ("ἔχῃ", "ἔχω", "V-PAS-3S", "may have"),
        ("ζωὴν", "ζωή", "N-ASF", "life"),
        ("αἰώνιον", "αἰώνιος", "A-ASF", "eternal."),
    ];
    assert_eq!(rows.len(), expected.len());
    for (got, want) in rows.iter().zip(expected) {
        assert_eq!((got.0, got.1, got.2, got.3), want);
    }
    // Morphology in plain English, not the code.
    let plain: Vec<&str> = rows.iter().map(|r| r.4).take(5).collect();
    assert_eq!(
        plain,
        [
            "Adverb",
            "Conjunction",
            "Verb, aorist active indicative, third person singular",
            "Article, definite, nominative masculine singular",
            "Noun, title, nominative masculine singular",
        ]
    );
    let first = &v.words[2];
    assert_eq!(first.strongs.to_string(), "G25");
    assert_eq!(first.transliteration.as_deref(), Some("egapesen"));
    assert!(!first.rtl);
}

/// Acceptance: Ps 3:1 (English) resolves to the right Hebrew verse through the Verse map:
/// TAHOT numbers the superscription as verse 1, so Standard Ps 3:1 is its Ps 3:2.
#[test]
fn ps_3_1_in_english_reads_tahots_ps_3_2_through_the_verse_map() {
    let t = Temp::new("ps31");
    let mut s = session(&t.0, &[bsb_psalm_3(), tahot_psalms()]);
    let w = word_by_word(&mut s, Passage::verse(ps3(1)));
    assert_eq!(w.verses.len(), 1, "{w:?}");
    let v = &w.verses[0];
    assert_eq!(v.bible_abbreviation, "TAHOT");
    assert_eq!((v.standard, v.native), (ps3(1), ps3(2)));
    assert!(v.renumbered());
    assert_eq!(v.reference(), "Psalms 3:2 (TAHOT)");
    let words: Vec<(String, &str)> = v
        .words
        .iter()
        .map(|w| (w.strongs.to_string(), w.gloss.as_deref().unwrap_or("")))
        .collect();
    assert_eq!(
        words,
        [
            ("H3068G".to_string(), "O Yahweh"),
            ("H4100".into(), "how!"),
            ("H7231".into(), "they are many"),
            ("H6862C".into(), "opponents my"),
            ("H7227A".into(), "many [people]"),
            ("H6965J".into(), "[are] rising up"),
            ("H5921A".into(), "on me"),
        ]
    );
    let yhwh = &v.words[0];
    assert!(yhwh.rtl);
    assert_eq!(yhwh.lemma.as_deref(), Some("יהוה"));
    // A word with a suffix: the main part's Lemma, both parts' Morphology.
    let enemies = &v.words[3];
    assert_eq!(enemies.lemma.as_deref(), Some("צַר"));
    assert_eq!(enemies.morph_code.as_deref(), Some("HNcmpc/HSp1bs"));
    assert_eq!(
        enemies.morphology.as_deref(),
        Some(
            "Noun, masculine plural construct + Suffix, pronominal, first person common \
             singular"
        )
    );

    // The superscription is the Standard title slot, TAHOT's Ps 3:1; the whole chapter lists
    // every TAHOT Verse once, in order.
    let title = word_by_word(&mut s, Passage::verse(Verse::new(Book::Psalms, 3, 0)));
    assert_eq!(title.verses.len(), 1);
    assert_eq!(title.verses[0].native, ps3(1));
    assert_eq!(title.verses[0].words[0].strongs.to_string(), "H4210");
    let chapter = Passage::new(Verse::new(Book::Psalms, 3, 0), ps3(8)).unwrap();
    let all = word_by_word(&mut s, chapter);
    let natives: Vec<u16> = all.verses.iter().map(|v| v.native.verse).collect();
    assert_eq!(natives, [1, 2, 3, 9]);
}

#[test]
fn lexicon_summaries_give_the_preferred_lexicons_short_entry_in_order_of_first_use() {
    let t = Temp::new("lex");
    let mut s = session(&t.0, &[tagnt_john_3_16(), greek_lexicon()]);
    let key = Key::Passage(Passage::verse(john(3, 16)));
    let SectionData::LexiconSummaries(rows) =
        &*settle(&mut s, &key, GuideSection::LexiconSummaries)
    else {
        panic!()
    };
    let got: Vec<(String, u32, &str, &str)> = rows
        .iter()
        .map(|r| {
            (
                r.strongs.to_string(),
                r.in_passage,
                r.lemma.as_deref().unwrap(),
                r.summary.as_str(),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            ("G25".to_string(), 1, "ἀγαπάω", "to love"),
            ("G3588".into(), 5, "ὁ", "the (definite article)"),
            ("G3439".into(), 1, "μονογενής", "only, unique"),
        ]
    );
    assert_eq!(rows[0].transliteration.as_deref(), Some("agapao"));
    assert_eq!(rows[0].entry.abbreviation, "LEX");
    assert_eq!(rows[0].entry.unit, "G25");
}

#[test]
fn short_entries_skip_the_headword_line_and_are_cut_at_a_word() {
    use crate::exegetical_guide::{SUMMARY_CHARS, short_entry};
    assert_eq!(
        short_entry("ἀγαπάω agapao\nto love\n  KJV: love"),
        "to love KJV: love"
    );
    assert_eq!(short_entry("only line"), "only line");
    let long = format!("head\n{}", "word ".repeat(200));
    let cut = short_entry(&long);
    assert!(cut.ends_with('…'));
    assert!(cut.chars().count() <= SUMMARY_CHARS + 1);
}

#[test]
fn grammatical_notes_explain_each_term_once_with_the_words_that_use_it() {
    let t = Temp::new("notes");
    let mut s = session(&t.0, &[tagnt_john_3_16()]);
    let key = Key::Passage(Passage::verse(john(3, 16)));
    let SectionData::GrammaticalNotes(notes) =
        &*settle(&mut s, &key, GuideSection::GrammaticalNotes)
    else {
        panic!()
    };
    let names: Vec<(&str, &str)> = notes.iter().map(|n| (n.label, n.term.name)).collect();
    assert_eq!(
        names,
        [
            ("Tense", "aorist"),
            ("Voice", "active"),
            ("Mood", "indicative"),
            ("Case", "nominative"),
            ("Case", "accusative"),
            ("Tense", "present"),
            ("Verb form", "participle"),
            ("Tense", "2nd aorist"),
            ("Voice", "middle"),
            ("Mood", "subjunctive"),
        ]
    );
    let aorist = &notes[0];
    assert_eq!(aorist.term.kind, TermKind::Tense);
    assert_eq!(aorist.words, ["ἠγάπησεν", "ἔδωκεν"]);
    assert!(
        aorist.explanation.contains("simple whole"),
        "{}",
        aorist.explanation
    );
    let accusative = notes.iter().find(|n| n.term.name == "accusative").unwrap();
    // Each word once, in order.
    assert_eq!(
        accusative.words,
        [
            "τὸν",
            "κόσμον",
            "υἱὸν",
            "μονογενῆ",
            "αὐτὸν",
            "ζωὴν",
            "αἰώνιον"
        ]
    );
}

#[test]
fn hebrew_notes_explain_stems_verb_forms_and_states() {
    let t = Temp::new("hebrew-notes");
    let mut s = session(&t.0, &[tahot_psalms()]);
    let key = Key::Passage(Passage::verse(ps3(1)));
    let SectionData::GrammaticalNotes(notes) =
        &*settle(&mut s, &key, GuideSection::GrammaticalNotes)
    else {
        panic!()
    };
    let names: Vec<(&str, &str)> = notes.iter().map(|n| (n.label, n.term.name)).collect();
    assert_eq!(
        names,
        [
            ("Stem", "Qal"),
            ("Verb form", "perfect"),
            ("State", "construct"),
            ("State", "absolute"),
            ("Verb form", "participle"),
        ]
    );
    assert_eq!(notes[2].words, ["צָרָ֑י"]);
}

/// A Passage with no tagged Bible in its language is empty, and the empty state asks for the
/// Bible of that language: TAGNT for John even with TAHOT in the Library.
#[test]
fn without_a_greek_bible_john_is_empty_and_offers_the_greek_text() {
    let t = Temp::new("empty");
    let mut s = session(&t.0, &[kjv_john(), tahot_psalms()]);
    let key = Key::Passage(Passage::verse(john(3, 16)));
    let w = word_by_word(&mut s, Passage::verse(john(3, 16)));
    assert!(
        w.verses.is_empty(),
        "the KJV is not an original-language Bible"
    );
    let empty = s.guide_empty_state(&key, GuideSection::WordByWord);
    assert!(
        empty
            .message
            .starts_with("None in your library. Add a Hebrew or Greek Bible")
    );
    assert_eq!(
        empty.missing.get_label().as_deref(),
        Some("Get Hebrew & Greek texts…")
    );
    assert!(empty.missing.offers_starter());
    // The Hebrew Psalm has its Bible: nothing missing.
    let ps = word_by_word(&mut s, Passage::verse(ps3(1)));
    assert!(!ps.verses.is_empty());
}

#[test]
fn the_command_opens_at_a_passage_and_the_guide_follows_its_link_set() {
    let t = Temp::new("command");
    let mut s = session(&t.0, &[kjv_john(), tagnt_john_3_16()]);
    let out = s
        .run("guide.exegetical_guide", &json!({ "passage": "Jn 3:16" }))
        .unwrap();
    let guide = TabId(out["tab"].as_u64().unwrap());
    let tab = s.workspace.tab(guide).unwrap();
    assert_eq!(tab.kind, TabKind::Guide(Guide::ExegeticalGuide));
    assert_eq!(tab.title, "Exegetical Guide");
    assert_eq!(tab.history.current(), Some(john(3, 16)));
    // In a Link set with the KJV, it follows the KJV.
    let kjv = s
        .run("resource.open", &json!({ "id": "kjv" }))
        .map(|o| TabId(o["tab"].as_u64().unwrap()))
        .unwrap();
    s.workspace.set_link(kjv, Some('A'));
    s.workspace.set_link(guide, Some('A'));
    s.workspace.navigate(kjv, john(3, 17));
    assert_eq!(
        s.workspace.tab(guide).unwrap().history.current(),
        Some(john(3, 17))
    );
    // Opened unlinked from the focused Bible, it starts at the Bible's Verse.
    s.workspace.focus(kjv);
    let out = s
        .run("guide.exegetical_guide", &json!({ "new_tab": true }))
        .unwrap();
    let second = s
        .workspace
        .tab(TabId(out["tab"].as_u64().unwrap()))
        .unwrap();
    assert_eq!(second.kind, TabKind::Guide(Guide::ExegeticalGuide));
    assert_eq!(second.history.current(), Some(john(3, 17)));
}

#[test]
fn the_guides_menu_and_command_box_offer_the_exegetical_guide() {
    let guides = MENUS.iter().find(|m| m.title == "Guides").unwrap();
    assert!(
        guides
            .items
            .iter()
            .any(|i| matches!(i, MenuItem::Command("guide.exegetical_guide")))
    );
    let mut s = Session::new(JobMode::Inline);
    for text in ["eg jn 3:16", "exegetical guide John 3:16"] {
        let top = s.command_box_suggestions(text).into_iter().next().unwrap();
        assert_eq!(
            top.action,
            CommandBoxAction::ExegeticalGuide(Passage::verse(john(3, 16))),
            "{text}"
        );
        assert_eq!(top.detail, "Exegetical Guide");
    }
    let out = s
        .run_command_box_action(
            &CommandBoxAction::ExegeticalGuide(Passage::verse(john(3, 16))),
            false,
        )
        .unwrap();
    let tab = s
        .workspace
        .tab(TabId(out["tab"].as_u64().unwrap()))
        .unwrap();
    assert_eq!(tab.kind, TabKind::Guide(Guide::ExegeticalGuide));
}

/// Manual check against the real Starter library (skips when it is absent): Jn 3:16 from
/// TAGNT and Ps 3:1 from TAHOT, every section.
#[test]
#[ignore = "reads the Starter library from the default library folder"]
fn starter_library_exegetical_guide_for_john_3_16_and_psalm_3_1() {
    let Some(dir) = default_library_dir().filter(|d| d.is_dir()) else {
        eprintln!("no Starter library; skipped");
        return;
    };
    let mut library = Library::new();
    library.scan(&dir);
    let sections = LibrarySections::default();
    let q = JobQueue::inline();
    let job = q.submit_fn("probe", move |ctx| {
        let mut out = Vec::new();
        for passage in [Passage::verse(john(3, 16)), Passage::verse(ps3(1))] {
            let key = Key::Passage(passage);
            for section in Guide::ExegeticalGuide.sections() {
                let t = std::time::Instant::now();
                let data = sections.compute(&library, &key, *section, ctx)?;
                out.push(format!(
                    "{passage:?} {}: {} in {:?}",
                    section.title(),
                    data.count(),
                    t.elapsed()
                ));
                match &data {
                    SectionData::WordByWord(w) => {
                        for v in &w.verses {
                            out.push(format!("  {} (Standard {:?})", v.reference(), v.standard));
                            for w in &v.words {
                                out.push(format!(
                                    "    {} | {} | {} | {} | {}",
                                    w.written,
                                    w.lemma.as_deref().unwrap_or("-"),
                                    w.strongs,
                                    w.morphology.as_deref().unwrap_or("-"),
                                    w.gloss.as_deref().unwrap_or("-"),
                                ));
                            }
                        }
                    }
                    SectionData::LexiconSummaries(rows) => {
                        for r in rows.iter().take(6) {
                            out.push(format!(
                                "  {} {} · {}: {}",
                                r.strongs,
                                r.lemma.as_deref().unwrap_or("-"),
                                r.entry.abbreviation,
                                r.summary
                            ));
                        }
                    }
                    SectionData::GrammaticalNotes(notes) => {
                        for n in notes {
                            out.push(format!(
                                "  {}: {} — {} ({})",
                                n.label,
                                n.term.name,
                                n.explanation,
                                n.words.join(", ")
                            ));
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(out)
    });
    q.pump(1);
    for e in q.drain_events() {
        if e.id == job
            && let JobEventKind::Finished(out) = e.kind
        {
            for line in *out.downcast::<Vec<String>>().unwrap() {
                eprintln!("{line}");
            }
        }
    }
}

#[test]
fn a_favorite_brings_the_exegetical_guide_back_at_its_passage() {
    let mut s = Session::new(JobMode::Inline);
    let out = s
        .run("guide.exegetical_guide", &json!({ "passage": "Ps 3:1" }))
        .unwrap();
    let guide = TabId(out["tab"].as_u64().unwrap());
    let (location, label) = s.location_of(guide).unwrap();
    assert_eq!(location.kind, TabKind::Guide(Guide::ExegeticalGuide));
    assert_eq!(location.verse, Some(ps3(1)));
    assert_eq!(label, "Psalms 3:1 · Exegetical Guide");
    s.workspace.close(guide);
    let back = s.open_location(&location, OpenRequest::default()).unwrap();
    let tab = s.workspace.tab(back).unwrap();
    assert_eq!(tab.kind, TabKind::Guide(Guide::ExegeticalGuide));
    assert_eq!(tab.history.current(), Some(ps3(1)));
}
