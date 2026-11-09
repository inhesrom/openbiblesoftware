//! Passage Guide Key words and People & places (#54): section data from open fixtures, the
//! Dictionary alias derived cache, the Word Study command path, and ignored checks against
//! the real Starter library.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use obs_model::Key;
use obs_scripture::{Book, Passage, Verse};
use serde_json::json;

use crate::aliases::{self, AliasCache, Overrides};
use crate::testing::{
    dictionary, greek_lexicon, kjv_genesis_12, kjv_john, tagged_bible, tagnt_john_3_16, tipnr,
};
use crate::*;

fn john(c: u16, v: u16) -> Verse {
    Verse::new(Book::John, c, v)
}

fn gen_12(v: u16) -> Verse {
    Verse::new(Book::Genesis, 12, v)
}

struct Temp(PathBuf);
impl Temp {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("obs-guide-words-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("library")).unwrap();
        Self(root)
    }
    fn lib(&self) -> PathBuf {
        self.0.join("library")
    }
    fn cache(&self) -> PathBuf {
        self.0.join("cache")
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

fn session(t: &Temp) -> Session {
    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(&t.lib()).is_empty());
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

fn key_words(s: &mut Session, passage: Passage) -> KeyWords {
    match &*settle(s, &Key::Passage(passage), GuideSection::KeyWords) {
        SectionData::KeyWords(k) => k.clone(),
        other => panic!("{other:?}"),
    }
}

fn people(s: &mut Session, passage: Passage) -> Vec<PersonOrPlace> {
    match &*settle(s, &Key::Passage(passage), GuideSection::PeopleAndPlaces) {
        SectionData::PeopleAndPlaces(rows) => rows.clone(),
        other => panic!("{other:?}"),
    }
}

/// Grammatical words of John 3:16 (TAGNT: CONJ, PREP, P-, PRT, T-).
const FUNCTION_WORDS: [&str; 8] = [
    "G1063", "G5620", "G2443", "G1519", "G846", "G3361", "G235", "G3588",
];

#[test]
fn john_3_16_key_words_lead_with_rare_words_not_articles() {
    let t = Temp::new("john");
    for r in [kjv_john(), tagnt_john_3_16(), greek_lexicon()] {
        write(&t.lib(), &r);
    }
    let mut s = session(&t);
    s.run("library.move", &json!({ "id": "kjv", "to": 0 }))
        .unwrap();
    let k = key_words(&mut s, Passage::verse(john(3, 16)));
    assert_eq!(k.bible_abbreviation, "KJV", "the ★ Strong's-tagged Bible");
    let numbers: Vec<String> = k.words.iter().map(|w| w.strongs.to_string()).collect();
    // Rarest first: words found once in the Bible lead; God (5 times) comes last.
    assert_eq!(k.words[0].in_bible, 1, "{numbers:?}");
    assert_eq!(numbers.last().map(String::as_str), Some("G2316"));
    assert!(
        k.words.windows(2).all(|w| w[0].in_bible <= w[1].in_bible),
        "{numbers:?}"
    );
    // KJV+Strong's has no Morphology: TAGNT's, for the same Passage, filters the function
    // words out (γάρ, ὥστε, ἵνα, εἰς, αὐτός, μή, ἀλλά).
    for f in FUNCTION_WORDS {
        assert!(!numbers.iter().any(|n| n == f), "{f} in {numbers:?}");
    }
    assert_eq!(k.function_words, 7);
    let love = k
        .words
        .iter()
        .find(|w| w.strongs.to_string() == "G25")
        .unwrap();
    assert_eq!(love.lemma.as_deref(), Some("ἀγαπάω"));
    assert_eq!(love.transliteration.as_deref(), Some("agapao"));
    assert_eq!(
        love.gloss.as_deref(),
        Some("to love"),
        "the Lexicon's Gloss"
    );
    assert_eq!(love.part_of_speech, Some("verb"));
    assert_eq!((love.in_passage, love.in_bible), (1, 1));
    let world = &k
        .words
        .iter()
        .find(|w| w.strongs.to_string() == "G2889")
        .unwrap();
    assert_eq!(
        (world.in_passage, world.in_bible),
        (1, 4),
        "world: 1 here, 4 in the Bible"
    );
    let life = k
        .words
        .iter()
        .find(|w| w.strongs.to_string() == "G2222")
        .unwrap();
    assert_eq!(
        life.gloss.as_deref(),
        Some("life"),
        "without a Lexicon entry, TAGNT's"
    );

    // With TAGNT first, the article ὁ is tagged four times in the verse and still left out.
    s.run("library.move", &json!({ "id": "tagnt", "to": 0 }))
        .unwrap();
    let k = key_words(&mut s, Passage::verse(john(3, 16)));
    assert_eq!(k.bible_abbreviation, "TAGNT");
    assert!(!k.words.iter().any(|w| w.strongs.to_string() == "G3588"));
    assert_eq!(k.function_words, 8);
    assert_eq!(k.words.len(), 13);
    // A Passage the first Bible lacks goes to the next Strong's-tagged Bible.
    let k = key_words(&mut s, Passage::verse(john(1, 1)));
    assert_eq!(k.bible_abbreviation, "KJV");
    assert_eq!(
        k.words
            .iter()
            .map(|w| w.strongs.to_string())
            .collect::<Vec<_>>(),
        // No Morphology anywhere for John 1:1: nothing is filtered, rarest still first.
        ["G2532", "G746", "G1722", "G4314", "G3056", "G2258", "G2316"]
    );
    drop(s);
}

#[test]
fn key_words_without_a_strongs_tagged_bible_are_empty_with_a_hint() {
    let t = Temp::new("no-tags");
    write(
        &t.lib(),
        &tagged_bible(
            "plain",
            "Plain",
            "PLN",
            "en",
            &[(john(3, 16), "For God so loved")],
        ),
    );
    let mut s = session(&t);
    let k = key_words(&mut s, Passage::verse(john(3, 16)));
    assert!(k.words.is_empty());
    assert!(
        GuideSection::KeyWords
            .empty_hint(&s.library, &s.guides.indexes())
            .contains("no Bible with Strong's numbers")
    );
    assert!(
        GuideSection::KeyWords
            .empty_hint(&Library::new(), &Default::default())
            .contains("Add a Bible with Strong's numbers")
    );
    drop(s);
}

/// The Gen 12 library: KJV Gen 12:1-4, TIPNR (excerpt), Easton and Smith (Abraham, Abram).
fn genesis_library(t: &Temp) {
    for r in [
        kjv_genesis_12(),
        tipnr(),
        dictionary("easton"),
        dictionary("smith"),
    ] {
        write(&t.lib(), &r);
    }
}

#[test]
fn gen_12_1_lists_abram_with_a_link_to_eastons_abraham() {
    let t = Temp::new("abram");
    genesis_library(&t);
    let mut s = session(&t);
    s.set_alias_cache_dir(t.cache());
    let rows = people(&mut s, Passage::verse(gen_12(1)));
    let abram = rows
        .iter()
        .find(|r| r.name == "Abram")
        .unwrap_or_else(|| panic!("{rows:?}"));
    assert_eq!(abram.usual_name.as_deref(), Some("Abraham"));
    assert_eq!(abram.entity, "Abraham@Gen.11.26-1Pe");
    assert_eq!(abram.verses, [gen_12(1)]);
    assert!(abram.summary().starts_with("Man"), "{}", abram.summary());
    let links: Vec<(&str, &str)> = abram
        .articles
        .iter()
        .map(|a| (a.abbreviation.as_str(), a.headword.as_str()))
        .collect();
    assert_eq!(links, [("Easton", "Abraham"), ("Smith", "Abraham")]);
    // Structured facts only; nothing of TIPNR's description prose.
    assert!(
        abram
            .facts
            .iter()
            .any(|f| f.label == "Father" && f.values == ["Terah"]),
        "{:?}",
        abram.facts
    );
    let text = format!("{abram:?}");
    assert!(!text.contains("Patriarchs, first mentioned"), "{text}");
    assert!(!text.contains("central figure"), "{text}");

    // The alias table is a derived cache file, keyed by both Resources' content hashes.
    let tipnr = s.library.get("tipnr").unwrap().manifest.clone();
    let easton = s.library.get("easton").unwrap().manifest.clone();
    let file = t.cache().join(aliases::file_name(&tipnr, &easton));
    let table = aliases::AliasTable::from_json(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(table.headword("Abraham@Gen.11.26-1Pe"), Some("Abraham"));

    // Dictionaries follow Preferred order.
    s.run("library.move", &json!({ "id": "smith", "to": 0 }))
        .unwrap();
    let rows = people(&mut s, Passage::verse(gen_12(1)));
    let abram = rows.iter().find(|r| r.name == "Abram").unwrap();
    assert_eq!(abram.articles[0].abbreviation, "Smith");

    // A range: Lot and Haran join, in the order the text names them.
    let rows = people(&mut s, Passage::new(gen_12(1), gen_12(4)).unwrap());
    let abram = rows.iter().find(|r| r.name == "Abram").unwrap();
    assert_eq!(abram.verses, [gen_12(1), gen_12(4)]);
    assert_eq!(rows[0].name, "Abram");
    drop(s);
}

#[test]
fn people_and_places_without_an_index_are_empty_with_a_hint() {
    let t = Temp::new("no-index");
    write(&t.lib(), &kjv_genesis_12());
    let mut s = session(&t);
    assert!(people(&mut s, Passage::verse(gen_12(1))).is_empty());
    let hint = GuideSection::PeopleAndPlaces.empty_hint(&s.library, &s.guides.indexes());
    assert!(hint.contains("Add a People & places index"), "{hint}");
    drop(s);
}

#[test]
fn the_alias_cache_is_rebuilt_at_library_load_when_either_side_changes() {
    let t = Temp::new("alias-cache");
    genesis_library(&t);
    let mut s = session(&t);
    s.set_alias_cache_dir(t.cache());
    s.refresh_alias_caches();
    assert!(s.guides.is_busy());
    s.jobs().pump(4);
    assert!(
        s.poll_imports().is_empty(),
        "the refresh's events are the Guide's"
    );
    assert!(!s.guides.is_busy());
    let names = |dir: &Path| -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .map(|d| {
                d.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    };
    let before = names(&t.cache());
    assert_eq!(
        before.len(),
        2,
        "TIPNR × Easton and TIPNR × Smith: {before:?}"
    );

    // A corrected Easton (new content hash): the next load rebuilds its pair only and
    // drops the stale file.
    let mut easton = dictionary("easton");
    easton.manifest.revision += 1;
    easton.units[0].text.push_str(" (corrected)");
    easton.manifest.content_hash = easton.content_hash();
    write(&t.lib(), &easton);
    drop(s);
    let mut s = session(&t);
    s.set_alias_cache_dir(t.cache());
    s.refresh_alias_caches();
    s.jobs().pump(4);
    s.poll_imports();
    let after = names(&t.cache());
    assert_eq!(after.len(), 2, "{after:?}");
    assert!(
        after
            .iter()
            .any(|n| n.starts_with("tipnr.") && n.contains("--smith."))
    );
    assert!(
        before
            .iter()
            .any(|n| n.contains("--easton.") && !after.contains(n))
    );

    // A damaged cache file is replaced, not trusted.
    let tipnr = s.library.get("tipnr").unwrap().clone();
    let easton = s.library.get("easton").unwrap().clone();
    let file = t
        .cache()
        .join(aliases::file_name(&tipnr.manifest, &easton.manifest));
    std::fs::write(&file, "{ not json").unwrap();
    let fresh = AliasCache::new(Overrides::starter());
    fresh.set_dir(t.cache());
    let table = fresh.table(&tipnr, &easton).unwrap();
    assert_eq!(table.headword("Abraham@Gen.11.26-1Pe"), Some("Abraham"));
    assert!(aliases::AliasTable::from_json(&std::fs::read_to_string(&file).unwrap()).is_some());
    drop(s);
}

/// At startup the refresh job and a restored Guide's section job can miss the same pair at
/// once: both get the table, which is built and written once.
#[test]
fn jobs_missing_the_same_pair_at_once_share_one_build() {
    let t = Temp::new("alias-race");
    genesis_library(&t);
    let s = session(&t);
    let tipnr = s.library.get("tipnr").unwrap().clone();
    let easton = s.library.get("easton").unwrap().clone();
    let cache = AliasCache::new(Overrides::starter());
    cache.set_dir(t.cache());
    let tables: Vec<Arc<aliases::AliasTable>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|_| scope.spawn(|| cache.table(&tipnr, &easton)))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap().unwrap())
            .collect()
    });
    assert!(
        tables.iter().all(|x| Arc::ptr_eq(x, &tables[0])),
        "built once"
    );
    assert_eq!(cache.write_error(), None);
    let files: Vec<String> = std::fs::read_dir(t.cache())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        files,
        [aliases::file_name(&tipnr.manifest, &easton.manifest)]
    );
    drop(s);
}

/// A cache folder that cannot be written costs only the cache file: the links still come,
/// and the failure is recorded rather than shown at every startup.
#[test]
fn an_unwritable_cache_folder_still_gives_links() {
    let t = Temp::new("alias-unwritable");
    genesis_library(&t);
    let mut s = session(&t);
    // A file where the folder should be: nothing can be created in it, on any platform.
    let blocked = t.0.join("blocked");
    std::fs::write(&blocked, "").unwrap();
    s.set_alias_cache_dir(blocked.clone());
    let tipnr = s.library.get("tipnr").unwrap().clone();
    let easton = s.library.get("easton").unwrap().clone();
    let table = s.guides.aliases().table(&tipnr, &easton).unwrap();
    assert_eq!(table.headword("Abraham@Gen.11.26-1Pe"), Some("Abraham"));
    let error = s.guides.aliases().write_error().unwrap_or_default();
    assert!(error.contains("alias cache"), "{error}");
    assert_eq!(s.guides.aliases().refresh(&s.library), Ok(2));
    let rows = people(&mut s, Passage::verse(gen_12(1)));
    let abram = rows.iter().find(|r| r.name == "Abram").unwrap();
    assert_eq!(abram.articles.len(), 2, "{:?}", abram.articles);
    drop(s);
}

/// The temp file of a write that crashed is swept at the next refresh; one still being
/// written (young) is left alone.
#[test]
fn refresh_sweeps_leftover_temp_files() {
    let t = Temp::new("alias-partials");
    genesis_library(&t);
    let s = session(&t);
    std::fs::create_dir_all(t.cache()).unwrap();
    let old = t
        .cache()
        .join("tipnr.aaaa--easton.bbbb.v1.aliases.json.123-0.partial");
    let young = t
        .cache()
        .join("tipnr.aaaa--smith.bbbb.v1.aliases.json.123-1.partial");
    let other = t.cache().join("notes.partial");
    for f in [&old, &young, &other] {
        std::fs::write(f, "{ half").unwrap();
    }
    let hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    for f in [&old, &other] {
        std::fs::File::options()
            .write(true)
            .open(f)
            .unwrap()
            .set_modified(hour_ago)
            .unwrap();
    }
    let cache = AliasCache::new(Overrides::starter());
    cache.set_dir(t.cache());
    assert_eq!(cache.refresh(&s.library), Ok(2));
    assert!(!old.exists(), "a leftover is swept");
    assert!(young.exists(), "a write in progress is not");
    assert!(other.exists(), "nor a file that is not an alias cache's");
    drop(s);
}

#[test]
fn a_prebuilt_alias_cache_beside_the_resources_is_used_and_overrides_apply() {
    let t = Temp::new("prebuilt");
    genesis_library(&t);
    let s = session(&t);
    let tipnr = s.library.get("tipnr").unwrap().clone();
    let easton = s.library.get("easton").unwrap().clone();
    // What `cargo xtask starter` writes to `aliases/` beside the `.obr` files; changed here
    // so the test can tell it was read rather than rebuilt.
    let cache = AliasCache::new(Overrides::starter());
    let mut table = (*cache.table(&tipnr, &easton).unwrap()).clone();
    table
        .links
        .insert("Abraham@Gen.11.26-1Pe".into(), "Abram".into());
    aliases::write_table(&t.lib().join(aliases::ALIAS_DIR), &table).unwrap();
    let cache = AliasCache::new(Overrides::starter());
    assert_eq!(
        cache
            .table(&tipnr, &easton)
            .unwrap()
            .headword("Abraham@Gen.11.26-1Pe"),
        Some("Abram")
    );
    // Different overrides make it stale: it is rebuilt with them.
    let overrides = Overrides::parse(
        "[[alias]]\ndictionary = \"easton\"\nentity = \"Abraham@Gen.11.26-1Pe\"\nheadword = \"\"\n",
    )
    .unwrap();
    let cache = AliasCache::new(overrides);
    assert_eq!(
        cache
            .table(&tipnr, &easton)
            .unwrap()
            .headword("Abraham@Gen.11.26-1Pe"),
        None
    );
    drop(s);
}

#[test]
fn a_key_word_opens_a_word_study_at_its_strongs_number() {
    let mut s = Session::new(JobMode::Inline);
    let out = s
        .run("guide.word_study", &json!({ "strongs": "g25" }))
        .unwrap();
    assert_eq!(
        out["key"],
        json!({ "scheme": "strongs_number", "key": "G25" })
    );
    let tab = s
        .workspace
        .tab(TabId(out["tab"].as_u64().unwrap()))
        .unwrap();
    assert_eq!(tab.kind, TabKind::Guide(Guide::WordStudy));
    assert_eq!(
        tab.entry_history.current(),
        Some(Key::StrongsNumber("G25".parse().unwrap()))
    );
    assert!(
        s.run("guide.word_study", &json!({ "strongs": "love" }))
            .is_err()
    );
    // Without a number it opens as before.
    let out = s.run("guide.word_study", &json!({})).unwrap();
    let blank = TabId(out["tab"].as_u64().unwrap());
    let tab = s.workspace.tab(blank).unwrap();
    assert_eq!(tab.entry_history.current(), None);

    // With `tab`, that open Word Study moves (and takes focus) instead: the Key words click
    // reusing a Tab goes through the command too.
    let first = s
        .workspace
        .tabs()
        .find(|t| t.entry_history.current().is_some())
        .map(|t| t.id)
        .unwrap();
    let count = s.workspace.tabs().count();
    let out = s
        .run(
            "guide.word_study",
            &json!({ "strongs": "H430", "tab": first.0 }),
        )
        .unwrap();
    assert_eq!(out["tab"], json!(first.0));
    assert_eq!(s.workspace.tabs().count(), count, "no new Tab");
    assert_eq!(s.workspace.focused(), Some(first));
    let tab = s.workspace.tab(first).unwrap();
    assert_eq!(
        tab.entry_history.current(),
        Some(Key::StrongsNumber("H430".parse().unwrap()))
    );
    // Only an open Word Study Tab.
    let other = s.run("tool.notes", &json!({})).unwrap()["tab"].clone();
    assert!(
        s.run(
            "guide.word_study",
            &json!({ "strongs": "G25", "tab": other })
        )
        .is_err()
    );
    assert!(
        s.run("guide.word_study", &json!({ "strongs": "G25", "tab": 999 }))
            .is_err()
    );
}

fn starter_library() -> Option<Library> {
    let dir = default_library_dir().filter(|d| d.join("tipnr.obr").is_file())?;
    let mut library = Library::new();
    library.scan(&dir);
    Some(library)
}

/// Real Starter library (skips when absent): John 3:16's Key words from the first Strong's-
/// tagged Bible, with timings cold and warm.
#[test]
#[ignore = "reads the Starter library from the default library folder"]
fn starter_library_john_3_16_key_words_lead_with_rare_words() {
    let Some(mut library) = starter_library() else {
        eprintln!("no Starter library; skipped");
        return;
    };
    // KJV+Strong's first, as the Starter ranks it.
    let mut order = library.preferred_order();
    order.retain(|id| id != "kjv-strongs");
    order.insert(0, "kjv-strongs".into());
    library.set_preferred_order(order);
    let sections = LibrarySections::default();
    let q = JobQueue::inline();
    let passage = Passage::verse(john(3, 16));
    let job = q.submit_fn("probe", move |ctx| {
        let mut out = Vec::new();
        for round in ["cold", "warm"] {
            let t = Instant::now();
            let data = sections.compute(
                &library,
                &Key::Passage(passage),
                GuideSection::KeyWords,
                ctx,
            )?;
            out.push((round, t.elapsed(), data));
        }
        Ok(out)
    });
    q.pump(1);
    let mut results = None;
    for e in q.drain_events() {
        if e.id == job
            && let JobEventKind::Finished(out) = e.kind
        {
            results = Some(
                *out.downcast::<Vec<(&str, std::time::Duration, SectionData)>>()
                    .unwrap(),
            );
        }
    }
    let results = results.unwrap();
    for (round, took, _) in &results {
        eprintln!("{round}: {took:?}");
    }
    let SectionData::KeyWords(k) = &results[0].2 else {
        panic!()
    };
    for w in &k.words {
        eprintln!(
            "{:<7} {:<12} {:<12} {:<28} {:<10} {:>2} {:>6}",
            w.strongs,
            w.lemma.as_deref().unwrap_or(""),
            w.transliteration.as_deref().unwrap_or(""),
            w.gloss.as_deref().unwrap_or(""),
            w.part_of_speech.unwrap_or(""),
            w.in_passage,
            w.in_bible
        );
    }
    eprintln!("{} function words left out", k.function_words);
    assert_eq!(k.bible_id, "kjv-strongs");
    assert!(k.words.len() >= 8);
    assert!(k.words.windows(2).all(|w| w[0].in_bible <= w[1].in_bible));
    let numbers: Vec<String> = k.words.iter().map(|w| w.strongs.to_string()).collect();
    for f in FUNCTION_WORDS {
        assert!(!numbers.contains(&f.to_string()), "{f} in {numbers:?}");
    }
    // μονογενής (9 times in the NT) leads well before θεός.
    let pos = |n: &str| numbers.iter().position(|x| x == n).unwrap();
    assert!(pos("G3439") < pos("G2316"));
    assert!(k.words[0].in_bible < 100, "{:?}", k.words[0]);
}

/// Real Starter library (skips when absent): Gen 12:1 lists Abram, linked to Easton's
/// Abraham, through a freshly built alias cache.
#[test]
#[ignore = "reads the Starter library from the default library folder"]
fn starter_library_gen_12_1_lists_abram_with_eastons_abraham() {
    let Some(library) = starter_library() else {
        eprintln!("no Starter library; skipped");
        return;
    };
    let cache = std::env::temp_dir().join(format!("obs-starter-aliases-{}", std::process::id()));
    let aliases = AliasCache::new(Overrides::starter());
    aliases.set_dir(cache.clone());
    let t = Instant::now();
    let pairs = aliases.refresh(&library).unwrap();
    eprintln!("{pairs} alias tables built in {:?}", t.elapsed());
    let t = Instant::now();
    let rows = crate::people_places::library_people_and_places(
        &library,
        Passage::verse(gen_12(1)),
        &crate::key_words::BibleCache::default(),
        &aliases,
        &crate::factbook::DirectoryCache::default(),
    )
    .unwrap();
    eprintln!("People & places for Gen 12:1 in {:?}", t.elapsed());
    for r in &rows {
        eprintln!(
            "{} ({:?}) {} · {:?} · {:?}",
            r.name,
            r.usual_name,
            r.summary(),
            r.facts,
            r.articles
                .iter()
                .map(|a| format!("{}: {}", a.abbreviation, a.headword))
                .collect::<Vec<_>>()
        );
    }
    let abram = rows.iter().find(|r| r.name == "Abram").unwrap();
    assert_eq!(abram.usual_name.as_deref(), Some("Abraham"));
    assert!(
        abram
            .articles
            .iter()
            .any(|a| a.resource_id == "easton" && a.headword == "Abraham"),
        "{:?}",
        abram.articles
    );
    // How many TIPNR entities find an article, and every committed override applies to the
    // real Resources (as the Starter build requires).
    use obs_store::ResourceStore;
    let index = library.get("tipnr").unwrap();
    let entities = obs_store::ObrReader::open(&index.path)
        .unwrap()
        .entities()
        .unwrap();
    for d in library.of_type(obs_model::ResourceType::Dictionary) {
        let table = aliases.table(index, d).unwrap();
        eprintln!(
            "{}: {} of {} entities linked",
            d.manifest.abbreviation,
            table.links.len(),
            entities.len()
        );
        let keys = obs_store::ObrReader::open(&d.path)
            .unwrap()
            .keys(obs_model::KeyScheme::Headword)
            .unwrap();
        let (built, problems) = aliases::build(
            &index.manifest,
            &entities,
            &d.manifest,
            &keys,
            &Overrides::starter(),
        );
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(built, *table);
    }
    let _ = std::fs::remove_dir_all(&cache);
}
