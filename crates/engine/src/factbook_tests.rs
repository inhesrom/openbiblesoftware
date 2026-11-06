//! The Factbook (#149): the People & places directory, the name chooser, each section's
//! data, the ways in (Passage Guide People & places, Word Info's Strong's number, the header
//! box), and an ignored check against the real Starter library.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use obs_model::{Key, Resource};
use obs_scripture::{Book, Passage, Verse};
use serde_json::json;

use crate::testing::{
    APOSTLE, BAPTIST, PETERS_FATHER, PRIEST, dictionary, hitchcock, johns, kjv_genesis_12, tipnr,
};
use crate::*;

struct Temp(PathBuf);
impl Temp {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("obs-factbook-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("library")).unwrap();
        Self(root)
    }
    fn lib(&self) -> PathBuf {
        self.0.join("library")
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write(dir: &Path, resource: &Resource) {
    let path = dir.join(format!("{}.obr", resource.manifest.resource_id));
    obs_store::write_obr(resource, &path).unwrap();
}

fn session(t: &Temp, resources: &[Resource]) -> Session {
    for r in resources {
        write(&t.lib(), r);
    }
    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(&t.lib()).is_empty());
    s.set_alias_cache_dir(t.0.join("cache"));
    s
}

fn v(book: Book, c: u16, n: u16) -> Passage {
    Passage::verse(Verse::new(book, c, n))
}

fn pump(s: &mut Session) {
    s.jobs().pump(8);
    s.poll_imports();
}

fn directory(s: &mut Session) -> Arc<PeopleDirectory> {
    for _ in 0..20 {
        match s.people_directory() {
            WordStatus::Ready(d) => return d,
            WordStatus::Failed(e) => panic!("{e}"),
            WordStatus::Loading => pump(s),
        }
    }
    panic!("the directory never loaded");
}

fn settle(s: &mut Session, id: &str, section: GuideSection) -> Arc<SectionData> {
    let key = Key::Headword(id.into());
    for _ in 0..50 {
        match s.guide_section(&key, section) {
            SectionStatus::Loading => pump(s),
            SectionStatus::Ready(d) => return d,
            SectionStatus::Failed(e) => panic!("{section:?} failed: {e}"),
        }
    }
    panic!("{section:?} never finished");
}

fn summary(s: &mut Session, id: &str) -> FactbookSummary {
    match &*settle(s, id, GuideSection::Summary) {
        SectionData::FactbookSummary(x) => x.clone(),
        other => panic!("{other:?}"),
    }
}

fn key_passages(s: &mut Session, id: &str) -> Vec<Passage> {
    match &*settle(s, id, GuideSection::KeyPassages) {
        SectionData::KeyPassages(x) => x.clone(),
        other => panic!("{other:?}"),
    }
}

fn related(s: &mut Session, id: &str) -> Vec<RelatedEntity> {
    match &*settle(s, id, GuideSection::Related) {
        SectionData::Related(x) => x.clone(),
        other => panic!("{other:?}"),
    }
}

fn family(summary: &FactbookSummary) -> Vec<(&str, Vec<&str>)> {
    summary
        .family
        .iter()
        .map(|f| (f.label, f.people.iter().map(|p| p.name.as_str()).collect()))
        .collect()
}

const ABRAHAM: &str = "Abraham@Gen.11.26-1Pe";

/// Acceptance: clicking "Abraham" in the Passage Guide opens his Factbook with family and
/// Passages. The People & places row's entity is what the click passes to `guide.factbook`.
#[test]
fn abraham_in_the_passage_guide_opens_his_factbook_with_family_and_passages() {
    let t = Temp::new("abraham");
    let mut s = session(
        &t,
        &[
            kjv_genesis_12(),
            tipnr(),
            dictionary("easton"),
            dictionary("smith"),
            hitchcock(),
        ],
    );
    // The Passage Guide's People & places row for Gen 12:1.
    let gen_12_1 = Key::Passage(v(Book::Genesis, 12, 1));
    let rows = loop {
        match s.guide_section(&gen_12_1, GuideSection::PeopleAndPlaces) {
            SectionStatus::Ready(d) => match &*d {
                SectionData::PeopleAndPlaces(rows) => break rows.clone(),
                other => panic!("{other:?}"),
            },
            SectionStatus::Failed(e) => panic!("{e}"),
            SectionStatus::Loading => pump(&mut s),
        }
    };
    let abram = rows.iter().find(|r| r.name == "Abram").unwrap();
    assert_eq!(abram.entity, ABRAHAM);

    let out = s
        .run("guide.factbook", &json!({ "entity": abram.entity }))
        .unwrap();
    let tab = TabId(out["tab"].as_u64().unwrap());
    let factbook = s.workspace.tab(tab).unwrap();
    assert_eq!(factbook.kind, TabKind::Guide(Guide::Factbook));
    assert_eq!(factbook.header_box(), HeaderBox::Name);
    assert!(!factbook.kind.has_link_chip(), "a Factbook is never linked");
    assert_eq!(
        factbook.entry_history.current(),
        Some(Key::Headword(ABRAHAM.into()))
    );

    // Header: his usual name, told apart by his father.
    let WordStatus::Ready(card) = ({
        directory(&mut s);
        s.factbook_card(ABRAHAM)
    }) else {
        panic!("no card")
    };
    assert_eq!(card.title(), "Abraham (son of Terah)");
    assert!(card.details().starts_with("Man · "), "{}", card.details());
    assert!(card.passages >= 2, "{card:?}");

    // Summary: other names, the meaning of the name, and family.
    let sum = summary(&mut s, ABRAHAM);
    assert_eq!(sum.other_names, ["Abram"]);
    let meaning = sum.meaning.clone().unwrap();
    assert_eq!(meaning.text, "father of a great multitude");
    assert_eq!(meaning.article.abbreviation, "Hitchcock");
    let fam = family(&sum);
    assert!(fam.contains(&("Father", vec!["Terah"])), "{fam:?}");
    assert!(
        fam.iter()
            .any(|(label, names)| *label == "Married to" && names.contains(&"Sarah")),
        "{fam:?}"
    );
    // Structured facts only; nothing of TIPNR's description prose.
    let text = format!("{sum:?}");
    assert!(!text.contains("Patriarchs, first mentioned"), "{text}");

    // Key Passages: every Passage that mentions him, Gen 12:1 among them, in Bible order.
    let passages = key_passages(&mut s, ABRAHAM);
    assert!(
        passages
            .iter()
            .any(|p| p.contains_verse(Verse::new(Book::Genesis, 12, 1))),
        "{passages:?}"
    );
    assert!(passages.windows(2).all(|w| w[0] < w[1]));

    // Dictionary articles: Easton and Smith in Preferred order; Hitchcock gives the Summary's
    // meaning instead.
    let SectionData::DictionaryArticles(articles) =
        &*settle(&mut s, ABRAHAM, GuideSection::DictionaryArticles)
    else {
        panic!()
    };
    let names: Vec<(&str, &str)> = articles
        .iter()
        .map(|a| (a.article.abbreviation.as_str(), a.article.headword.as_str()))
        .collect();
    assert_eq!(names, [("Easton", "Abraham"), ("Smith", "Abraham")]);
    assert!(articles.iter().all(|a| !a.excerpt.is_empty()));

    // History names the Visit after him.
    let (_, label) = s.location_of(tab).unwrap();
    assert_eq!(label, "Abraham (son of Terah) · Factbook");
}

/// Acceptance: the "John" chooser lists the distinct Johns, each told apart by its facts.
#[test]
fn the_john_chooser_lists_the_distinct_johns() {
    let t = Temp::new("johns");
    let mut s = session(&t, &[johns()]);
    let out = s.run("guide.factbook", &json!({ "name": "John" })).unwrap();
    let tab = TabId(out["tab"].as_u64().unwrap());
    assert_eq!(s.factbooks.choosing(tab), Some("John"));
    let chooser = loop {
        match s.factbook_chooser("John") {
            WordStatus::Ready(c) => break c,
            WordStatus::Failed(e) => panic!("{e}"),
            WordStatus::Loading => pump(&mut s),
        }
    };
    let titles: Vec<String> = chooser.choices.iter().map(EntityCard::title).collect();
    assert_eq!(
        titles,
        [
            "John (son of Zechariah)",
            "John (son of Zebedee)",
            "John (father of Peter)",
            "John (Tribe of Levi)",
            "Mark (son of Mary)",
        ]
    );
    assert_eq!(chooser.choices[4].also_called.as_deref(), Some("John"));
    assert!(chooser.choices[..4].iter().all(|c| c.also_called.is_none()));
    assert_eq!(
        chooser.choices[0].details(),
        "Man · Matt 3:1 – Luke 1:60 · 4 Verses"
    );
    // Several Johns: the Tab keeps showing the chooser until one is chosen.
    assert_eq!(s.factbooks.choosing(tab), Some("John"));
    let entities: Vec<&str> = chooser.choices.iter().map(|c| c.entity.as_str()).collect();
    assert_eq!(entities[..4], [BAPTIST, APOSTLE, PETERS_FATHER, PRIEST]);

    // Choosing one moves the Tab there (the header box runs `tab.go` with its id).
    s.run("tab.go", &json!({ "tab": tab.0, "text": APOSTLE }))
        .unwrap();
    assert_eq!(s.factbooks.choosing(tab), None);
    assert_eq!(
        s.workspace.tab(tab).unwrap().entry_history.current(),
        Some(Key::Headword(APOSTLE.into()))
    );
    // A name only one entity has opens it directly.
    s.run("tab.go", &json!({ "tab": tab.0, "text": "zebedee" }))
        .unwrap();
    assert_eq!(s.factbooks.choosing(tab), None);
    assert_eq!(
        s.workspace.tab(tab).unwrap().entry_history.current(),
        Some(Key::Headword("Zebedee@Mat.4.21-Jhn".into()))
    );
    // Back returns to the apostle.
    s.run("tab.back", &json!({ "tab": tab.0 })).unwrap();
    assert_eq!(
        s.workspace.tab(tab).unwrap().entry_history.current(),
        Some(Key::Headword(APOSTLE.into()))
    );
    // A name nobody has: an empty chooser, never a guess.
    s.run("tab.go", &json!({ "tab": tab.0, "text": "Xerxes" }))
        .unwrap();
    assert_eq!(s.factbooks.choosing(tab), Some("Xerxes"));
    let WordStatus::Ready(none) = s.factbook_chooser("Xerxes") else {
        panic!()
    };
    assert!(none.choices.is_empty());
    // Part of a name lists the names that start with it.
    let WordStatus::Ready(jo) = s.factbook_chooser("Joh") else {
        panic!()
    };
    assert_eq!(jo.choices.len(), 5);
}

#[test]
fn word_info_strongs_numbers_lead_to_the_factbook() {
    let t = Temp::new("strongs");
    let mut s = session(&t, &[johns()]);
    // Nothing until the directory has loaded (Word Info shows no link meanwhile).
    assert!(
        s.factbook_entities_for("G2491H".parse().unwrap())
            .is_empty()
    );
    directory(&mut s);
    // TAGNT's extended number names one John.
    let one = s.factbook_entities_for("G2491H".parse().unwrap());
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].entity, APOSTLE);
    let out = s
        .run("guide.factbook", &json!({ "strongs": "G2491H" }))
        .unwrap();
    assert_eq!(out["key"], json!(APOSTLE));
    // The KJV's base number names every John: the chooser opens.
    let all = s.factbook_entities_for("G2491".parse().unwrap());
    assert_eq!(all.len(), 4);
    assert_eq!(all[0].entity, BAPTIST, "most often named first");
    let out = s
        .run(
            "guide.factbook",
            &json!({ "strongs": "G2491", "new_tab": true }),
        )
        .unwrap();
    assert_eq!(out["choose"], json!("John"));
    assert!(
        s.run("guide.factbook", &json!({ "strongs": "G9999" }))
            .is_err()
    );
}

/// Word Info on Abram (`H87`) or Greek Abraham (`G11`) leads to the same Factbook as on
/// Abraham (`H85`): TIPNR lists every name's Strong's number on its Total line.
#[test]
fn every_strongs_number_of_abrahams_names_leads_to_his_factbook() {
    let t = Temp::new("abram");
    let mut s = session(&t, &[tipnr()]);
    directory(&mut s);
    for n in ["H85", "H0087", "G11"] {
        let named = s.factbook_entities_for(n.parse().unwrap());
        let ids: Vec<&str> = named.iter().map(|c| c.entity.as_str()).collect();
        assert_eq!(ids, [ABRAHAM], "{n}");
        let out = s
            .run("guide.factbook", &json!({ "strongs": n, "new_tab": true }))
            .unwrap();
        assert_eq!(out["key"], json!(ABRAHAM), "{n}");
    }
}

#[test]
fn related_people_and_places_come_from_links_both_ways_and_shared_verses() {
    let t = Temp::new("related");
    let mut s = session(&t, &[johns()]);
    // The priest's record has no links; Jerusalem lists him as someone who lived there.
    let rows = related(&mut s, PRIEST);
    let why: Vec<(&str, &str)> = rows
        .iter()
        .map(|r| (r.card.name.as_str(), r.why.as_str()))
        .collect();
    assert_eq!(why, [("Jerusalem", "Lived in")]);
    // And Jerusalem lists him as an inhabitant.
    let rows = related(&mut s, "Jerusalem@Gen.14.18-Rev");
    assert_eq!(rows[0].card.entity, PRIEST);
    assert_eq!(rows[0].why, "Lived here");
    // Family stays in the Summary; others named in the same Verses follow, most first.
    let rows = related(&mut s, APOSTLE);
    let why: Vec<(&str, &str)> = rows
        .iter()
        .map(|r| (r.card.name.as_str(), r.why.as_str()))
        .collect();
    assert_eq!(why, []);
    let rows = related(&mut s, "James@Mat.4.21-Act");
    let why: Vec<(&str, &str)> = rows
        .iter()
        .map(|r| (r.card.entity.as_str(), r.why.as_str()))
        .collect();
    // His brother's record names him; that link wins over being named together.
    assert_eq!(why, [(APOSTLE, "Sibling")]);
    let rows = related(&mut s, "Zechariah@Luk.1.5-");
    let why: Vec<(&str, &str)> = rows
        .iter()
        .map(|r| (r.card.name.as_str(), r.why.as_str()))
        .collect();
    assert_eq!(why, [("Elizabeth", "Named together in 1 Verse")]);
    // The Baptist's parents are family, in the Summary.
    let sum = summary(&mut s, BAPTIST);
    assert_eq!(
        family(&sum),
        [("Father", vec!["Zechariah"]), ("Mother", vec!["Elizabeth"])]
    );
    assert!(sum.meaning.is_none(), "no names Dictionary in this library");
    assert_eq!(
        key_passages(&mut s, BAPTIST),
        [
            v(Book::Matthew, 3, 1),
            v(Book::Matthew, 3, 4),
            v(Book::Luke, 1, 13),
            v(Book::Luke, 1, 60),
        ]
    );
}

#[test]
fn key_passages_join_neighbouring_verses() {
    let gn = |c, n| v(Book::Genesis, c, n);
    assert_eq!(
        crate::factbook::key_passages([gn(12, 1), gn(11, 27), gn(11, 26), gn(12, 4)]),
        [
            Passage::new(
                Verse::new(Book::Genesis, 11, 26),
                Verse::new(Book::Genesis, 11, 27)
            )
            .unwrap(),
            gn(12, 1),
            gn(12, 4),
        ]
    );
}

#[test]
fn name_meanings_drop_the_headword() {
    use crate::factbook::meaning_text;
    assert_eq!(
        meaning_text("ABRAHAM", "ABRAHAM, father of a great multitude"),
        "father of a great multitude"
    );
    assert_eq!(
        meaning_text("John", "the grace of the Lord"),
        "the grace of the Lord"
    );
    assert_eq!(
        meaning_text("Abel", "ABEL — vanity;  breath"),
        "vanity; breath"
    );
    // Two senses of one name (Hitchcock's Noah), as the SWORD Import combines them.
    assert_eq!(
        meaning_text(
            "NOAH",
            "that quavers or totters (Zelophehad's daughter)\nrepose; consolation"
        ),
        "that quavers or totters (Zelophehad's daughter) · repose; consolation"
    );
}

#[test]
fn a_factbook_without_an_index_says_what_to_add_and_reloads_with_the_library() {
    let t = Temp::new("empty");
    let mut s = session(&t, &[]);
    assert!(directory(&mut s).is_empty());
    let hint = GuideSection::Summary.empty_hint(&s.library, &Default::default());
    assert!(hint.contains("Add a People & places index"), "{hint}");
    let out = s.run("guide.factbook", &json!({})).unwrap();
    let tab = TabId(out["tab"].as_u64().unwrap());
    assert_eq!(s.workspace.tab(tab).unwrap().entry_history.current(), None);
    // The arrangement is the Factbook's own; Map came with the Atlas (#150).
    let ids: Vec<&str> = s
        .guides
        .arrangement(Guide::Factbook)
        .sections
        .iter()
        .map(|p| p.section.id())
        .collect();
    assert_eq!(
        ids,
        [
            "summary",
            "map",
            "key_passages",
            "dictionary_articles",
            "related"
        ]
    );
    // An index added later is read on the next ask.
    write(&t.lib(), &johns());
    s.library.scan(&t.lib());
    assert_eq!(directory(&mut s).len(), 12);
}

/// Real Starter library (skips when absent): Abraham's Factbook and the John chooser from
/// the shipped TIPNR, Dictionaries and (when present) Hitchcock's Bible Names.
#[test]
#[ignore = "reads the Starter library from the default library folder"]
fn starter_library_abraham_factbook_and_john_chooser() {
    let Some(dir) = default_library_dir().filter(|d| d.join("tipnr.obr").is_file()) else {
        eprintln!("no Starter library; skipped");
        return;
    };
    let mut s = Session::new(JobMode::Inline);
    s.library.scan(&dir);
    let cache = std::env::temp_dir().join(format!("obs-factbook-starter-{}", std::process::id()));
    s.set_alias_cache_dir(cache.clone());
    let started = std::time::Instant::now();
    let d = directory(&mut s);
    eprintln!("directory: {} entities in {:?}", d.len(), started.elapsed());
    let card = d.card(ABRAHAM).unwrap();
    eprintln!("{} · {}", card.title(), card.details());
    let sum = summary(&mut s, ABRAHAM);
    eprintln!("Summary: {sum:#?}");
    let fam = family(&sum);
    assert!(fam.contains(&("Father", vec!["Terah"])), "{fam:?}");
    assert!(
        fam.iter()
            .any(|(l, n)| *l == "Children" && n.contains(&"Isaac")),
        "{fam:?}"
    );
    let passages = key_passages(&mut s, ABRAHAM);
    eprintln!("{} Key Passages", passages.len());
    assert!(passages.len() > 100);
    // Word Info on Abraham, Abram or the Greek Abraham.
    for n in ["H85", "H87", "G11"] {
        let named = s.factbook_entities_for(n.parse().unwrap());
        assert_eq!(
            named.first().map(|c| c.entity.as_str()),
            Some(ABRAHAM),
            "{n}"
        );
    }
    let rows = related(&mut s, ABRAHAM);
    for r in &rows {
        eprintln!("related: {} — {}", r.card.title(), r.why);
    }
    let chooser = d.choose("John");
    for c in &chooser.choices {
        eprintln!(
            "chooser: {} · {} · also called {:?}",
            c.title(),
            c.details(),
            c.also_called
        );
    }
    let exact: Vec<&EntityCard> = chooser
        .choices
        .iter()
        .filter(|c| c.also_called.is_none())
        .collect();
    assert!(exact.len() >= 4, "{exact:?}");
    let titles: std::collections::BTreeSet<String> = exact.iter().map(|c| c.title()).collect();
    assert_eq!(
        titles.len(),
        exact.len(),
        "every John is told apart: {titles:?}"
    );
    let _ = std::fs::remove_dir_all(&cache);
}
