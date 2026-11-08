//! The Topic Guide (#148): reading topical index entries into subtopics, the topic chooser
//! with synonyms, each section's data, the empty states, and an ignored check against a real
//! library with CrossWire's Nave and Torrey modules.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use obs_model::{Key, Resource};
use obs_scripture::{Book, Passage, RefStyle, Standard, Verse, format_passage};
use serde_json::json;

use crate::testing::{easton_topics, topical_index};
use crate::topic_guide::{display_topic, subtopics};
use crate::*;

struct Temp(PathBuf);
impl Temp {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("obs-topic-guide-{name}-{}", std::process::id()));
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

fn session(t: &Temp, resources: &[Resource]) -> Session {
    for r in resources {
        let path = t.lib().join(format!("{}.obr", r.manifest.resource_id));
        obs_store::write_obr(r, &path).unwrap();
    }
    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(&t.lib()).is_empty());
    s
}

fn library() -> Vec<Resource> {
    vec![
        topical_index("nave"),
        topical_index("torrey"),
        easton_topics(),
    ]
}

fn pump(s: &mut Session) {
    s.jobs().pump(8);
    s.poll_imports();
}

fn settle(s: &mut Session, topic: &str, section: GuideSection) -> Arc<SectionData> {
    let key = Key::Headword(topic.into());
    for _ in 0..50 {
        match s.guide_section(&key, section) {
            SectionStatus::Loading => pump(s),
            SectionStatus::Ready(d) => return d,
            SectionStatus::Failed(e) => panic!("{section:?} failed: {e}"),
        }
    }
    panic!("{section:?} never finished");
}

fn chooser(s: &mut Session, text: &str) -> Arc<TopicChooser> {
    for _ in 0..20 {
        match s.topic_chooser(text) {
            WordStatus::Ready(c) => return c,
            WordStatus::Failed(e) => panic!("{e}"),
            WordStatus::Loading => pump(s),
        }
    }
    panic!("the topic directory never loaded");
}

fn short(p: Passage) -> String {
    format_passage(p, &Standard, RefStyle::Short)
}

fn v(book: Book, c: u16, n: u16) -> Passage {
    Passage::verse(Verse::new(book, c, n))
}

#[test]
fn forgiveness_shows_naves_subtopics_eastons_article_and_key_passages() {
    let t = Temp::new("forgiveness");
    let mut s = session(&t, &library());

    let SectionData::TopicalEntries(entries) =
        &*settle(&mut s, "Forgiveness", GuideSection::TopicalEntries)
    else {
        panic!("not topical entries");
    };
    let titles: Vec<(&str, &str)> = entries
        .iter()
        .map(|e| (e.article.abbreviation.as_str(), e.article.headword.as_str()))
        .collect();
    assert_eq!(
        titles,
        [
            ("Nave", "FORGIVENESS"),
            ("Torrey", "FORGIVENESS OF INJURIES"),
            ("Torrey", "FORGIVENESS OF SIN")
        ]
    );
    let nave = &entries[0];
    let rows: Vec<(u8, &str)> = nave
        .subtopics
        .iter()
        .map(|s| (s.level, s.label.as_str()))
        .collect();
    assert_eq!(
        rows,
        [
            (1, "OF ENEMIES"),
            (1, "INSTANCES OF"),
            (2, "Esau forgives Jacob"),
            (2, "Joseph forgives his brothers"),
            (2, "Jesus forgives his enemies"),
            (1, "OF SINS"),
        ]
    );
    let of_enemies = &nave.subtopics[0];
    assert_eq!(of_enemies.see, ["ENEMY"]);
    assert_eq!(short(of_enemies.passages[0]), "Exod 23:4");
    assert_eq!(of_enemies.passages.len(), 14);
    assert_eq!(
        nave.subtopics[3]
            .passages
            .iter()
            .map(|p| short(*p))
            .collect::<Vec<_>>(),
        ["Gen 45:5-15", "Gen 50:19-21"]
    );
    // `See SIN, FORGIVENESS OF` brings in that subtopic's Passages.
    let of_sins = &nave.subtopics[5];
    assert_eq!(of_sins.see, ["SIN, FORGIVENESS OF"]);
    assert_eq!(of_sins.via.as_deref(), Some("Sin, Forgiveness of"));
    assert_eq!(short(of_sins.passages[0]), "Ps 32:1");
    assert_eq!(of_sins.passages.len(), 7);
    let torrey: Vec<(u8, &str, usize)> = entries[1]
        .subtopics
        .iter()
        .map(|s| (s.level, s.label.as_str(), s.passages.len()))
        .collect();
    assert_eq!(
        torrey,
        [
            (1, "Christ set an example of", 1),
            (1, "Commanded", 2),
            (1, "MOTIVES TO", 0),
            (2, "The mercy of God", 1),
            (2, "God's forgiveness of us", 1),
            (2, "Christ's forgiveness of us", 1),
            (1, "Exemplified", 0),
            (2, "Joseph", 2),
        ]
    );

    // Easton files it under "Forgiveness of sin".
    let SectionData::DictionaryArticles(articles) =
        &*settle(&mut s, "Forgiveness", GuideSection::TopicArticles)
    else {
        panic!("not articles");
    };
    assert_eq!(articles.len(), 1);
    assert_eq!(articles[0].article.abbreviation, "Easton");
    assert_eq!(articles[0].article.headword, "Forgiveness of sin");
    assert!(
        articles[0]
            .excerpt
            .starts_with("One of the constituent parts of justification."),
        "{}",
        articles[0].excerpt
    );

    let SectionData::TopicPassages(key) =
        &*settle(&mut s, "Forgiveness", GuideSection::TopicPassages)
    else {
        panic!("not Key Passages");
    };
    let ranked: Vec<(String, u32)> = key
        .iter()
        .map(|c| (short(c.passage), c.citations))
        .collect();
    // Eph 4:32 is cited by Nave (of enemies; of sins, through Sin) and Torrey.
    assert_eq!(ranked[0], ("Eph 4:32".to_string(), 3), "{ranked:?}");
    for cited_by_both in ["Luke 23:34", "Mark 11:25", "Col 3:13", "Matt 6:14"] {
        assert!(
            ranked.iter().any(|(p, n)| p == cited_by_both && *n >= 2),
            "{cited_by_both} in {ranked:?}"
        );
    }
    assert!(key.len() <= topic_guide::KEY_PASSAGES);
    // No two Key Passages share a Verse.
    for (i, a) in key.iter().enumerate() {
        assert!(key[i + 1..].iter().all(|b| !a.passage.overlaps(b.passage)));
    }

    let SectionData::TopicSearch(query) =
        &*settle(&mut s, "Forgiveness", GuideSection::TopicSearch)
    else {
        panic!("not Search");
    };
    assert_eq!(query, "Forgiveness");
}

#[test]
fn the_chooser_lists_the_topic_longer_headwords_and_synonyms() {
    let t = Temp::new("chooser");
    let mut s = session(&t, &library());
    let c = chooser(&mut s, "forgiveness");
    let rows: Vec<(&str, Vec<&str>, Vec<&str>)> = c
        .choices
        .iter()
        .map(|c| {
            (
                c.topic.as_str(),
                c.sources.iter().map(String::as_str).collect(),
                c.see.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("Forgiveness", vec!["Nave"], vec![]),
            ("Forgiveness of Injuries", vec!["Torrey"], vec![]),
            ("Forgiveness of sin", vec!["Easton", "Torrey"], vec![]),
            // A synonym from the topical index: Nave's PARDON only says "See SIN,
            // FORGIVENESS OF". Easton's spelling is shown; sources in Preferred order.
            (
                "Pardon",
                vec!["Easton", "Nave"],
                vec!["Sin, Forgiveness of"]
            ),
        ]
    );
    let topics =
        |c: &TopicChooser| -> Vec<String> { c.choices.iter().map(|c| c.topic.clone()).collect() };
    let prayer = topics(&chooser(&mut s, "Prayer"));
    assert_eq!(prayer, ["Prayer", "Importunity"]);
    // Nothing whole-word: headwords that start with the text.
    let prefix = topics(&chooser(&mut s, "forgiv"));
    assert_eq!(
        prefix,
        [
            "Forgiveness",
            "Forgiveness of Injuries",
            "Forgiveness of sin"
        ]
    );
    assert!(chooser(&mut s, "zzz").choices.is_empty());
}

#[test]
fn the_header_box_chooses_and_the_only_topic_opens_directly() {
    let t = Temp::new("go");
    let mut s = session(&t, &library());
    let out = s.run("guide.topic_guide", &json!({})).unwrap();
    let tab = TabId(out["tab"].as_u64().unwrap());
    assert_eq!(s.workspace.tab(tab).unwrap().header_box(), HeaderBox::Topic);
    s.run("tab.go", &json!({ "tab": tab.0, "text": "forgiveness" }))
        .unwrap();
    assert_eq!(s.topic_guides.choosing(tab), Some("forgiveness"));
    let _ = chooser(&mut s, "forgiveness");
    pump(&mut s);
    assert_eq!(s.topic_guides.choosing(tab), Some("forgiveness"));
    // A text naming one topic opens it.
    s.run("tab.go", &json!({ "tab": tab.0, "text": "importunity" }))
        .unwrap();
    assert_eq!(s.topic_guides.choosing(tab), None);
    assert_eq!(
        s.workspace.tab(tab).unwrap().entry_history.current(),
        Some(Key::Headword("Importunity".into()))
    );
    // `topic` opens it as given, in the same Tab.
    let out = s
        .run("guide.topic_guide", &json!({ "topic": "Forgiveness" }))
        .unwrap();
    assert_eq!(out["tab"].as_u64(), Some(tab.0));
    assert_eq!(
        s.workspace.tab(tab).unwrap().entry_history.current(),
        Some(Key::Headword("Forgiveness".into()))
    );
    // Back walks the entry history and closes a chooser.
    s.run("tab.go", &json!({ "tab": tab.0, "text": "prayer" }))
        .unwrap();
    s.run("tab.back", &json!({ "tab": tab.0 })).unwrap();
    assert_eq!(s.topic_guides.choosing(tab), None);
    assert_eq!(
        s.workspace.tab(tab).unwrap().entry_history.current(),
        Some(Key::Headword("Importunity".into()))
    );
}

#[test]
fn a_topic_guide_follows_and_moves_its_link_sets_headwords() {
    let t = Temp::new("link");
    let mut s = session(&t, &library());
    let out = s
        .run("guide.topic_guide", &json!({ "topic": "Pardon" }))
        .unwrap();
    let topic = TabId(out["tab"].as_u64().unwrap());
    let tab = s.workspace.tab(topic).unwrap();
    assert!(tab.kind.has_link_chip());
    assert_eq!(tab.key_schemes, [obs_model::KeyScheme::Headword]);
    let other = s
        .run(
            "guide.topic_guide",
            &json!({ "topic": "Prayer", "new_tab": true }),
        )
        .unwrap();
    let other = TabId(other["tab"].as_u64().unwrap());
    s.workspace.set_link(topic, Some('A'));
    s.workspace.set_link(other, Some('A'));
    s.workspace
        .navigate_entry(topic, Key::Headword("Forgiveness".into()));
    assert_eq!(
        s.workspace.tab(other).unwrap().entry_history.current(),
        Some(Key::Headword("Forgiveness".into()))
    );
}

#[test]
fn a_new_topic_guide_starts_at_its_linked_dictionarys_headword() {
    let t = Temp::new("adopt");
    let mut s = session(&t, &library());
    let dictionary = s
        .run("resource.open", &json!({ "id": "easton" }))
        .map(|out| TabId(out["tab"].as_u64().unwrap()))
        .unwrap();
    s.workspace.set_link(dictionary, Some('A'));
    s.workspace
        .navigate_entry(dictionary, Key::Headword("Pardon".into()));
    s.workspace.focus(dictionary);
    let out = s.run("guide.topic_guide", &json!({})).unwrap();
    let topic = s
        .workspace
        .tab(TabId(out["tab"].as_u64().unwrap()))
        .unwrap();
    assert_eq!(topic.link, Some('A'));
    assert_eq!(
        topic.entry_history.current(),
        Some(Key::Headword("Pardon".into()))
    );
}

#[test]
fn key_passages_rank_a_narrower_cross_chapter_passage_first() {
    let genesis = |c1, v1, c2, v2| {
        Passage::new(
            Verse::new(Book::Genesis, c1, v1),
            Verse::new(Book::Genesis, c2, v2),
        )
        .unwrap()
    };
    let entry = crate::topic_guide::TopicalEntry {
        article: crate::people_places::ArticleLink {
            resource_id: "nave".into(),
            abbreviation: "Nave".into(),
            title: "Nave's Topical Bible".into(),
            headword: "CREATION".into(),
        },
        subtopics: vec![crate::topic_guide::Subtopic {
            level: 0,
            label: String::new(),
            passages: vec![genesis(1, 1, 1, 31), genesis(1, 31, 2, 1)],
            see: Vec::new(),
            via: None,
        }],
        direction: obs_model::Direction::Ltr,
    };
    let ranked: Vec<String> = crate::topic_guide::key_passages(&[entry])
        .into_iter()
        .map(|c| short(c.passage))
        .collect();
    assert_eq!(ranked, [short(genesis(1, 31, 2, 1))]);
}

#[test]
fn favorites_and_history_name_the_topic_and_reopen_it() {
    let t = Temp::new("favorites");
    let mut s = session(&t, &library());
    let out = s
        .run("guide.topic_guide", &json!({ "topic": "Forgiveness" }))
        .unwrap();
    let tab = TabId(out["tab"].as_u64().unwrap());
    let (location, label) = s.location_of(tab).unwrap();
    assert_eq!(label, "Forgiveness · Topic Guide");
    s.run("tab.close", &json!({ "tab": tab.0 })).unwrap();
    let reopened = s.open_location(&location, OpenRequest::default()).unwrap();
    assert_eq!(
        s.workspace.tab(reopened).unwrap().entry_history.current(),
        Some(Key::Headword("Forgiveness".into()))
    );
}

#[test]
fn without_a_topical_index_the_sections_say_how_to_get_one() {
    let t = Temp::new("empty");
    let mut s = session(&t, &[easton_topics()]);
    for section in [GuideSection::TopicalEntries, GuideSection::TopicPassages] {
        assert!(settle(&mut s, "Forgiveness", section).is_empty());
        let empty = s.guide_empty_state(&Key::Headword("Forgiveness".into()), section);
        assert!(
            empty.message.contains("Nave's Topical Bible"),
            "{}",
            empty.message
        );
        assert_eq!(
            empty.missing.get_label().as_deref(),
            Some("Get Dictionaries…")
        );
        assert_eq!(empty.missing.starter, StarterOffer::None);
    }
    // Easton still gives its article.
    assert_eq!(
        settle(&mut s, "Forgiveness", GuideSection::TopicArticles).count(),
        1
    );
    // With the index but not the topic: only Get Resources.
    let t = Temp::new("no-topic");
    let mut s = session(&t, &library());
    assert!(settle(&mut s, "Zebra", GuideSection::TopicalEntries).is_empty());
    let empty = s.guide_empty_state(&Key::Headword("Zebra".into()), GuideSection::TopicalEntries);
    assert_eq!(
        empty.message,
        "None in your library: no topical index has this topic."
    );
}

#[test]
fn torrey_lines_join_their_label_and_see_splits_off_topics() {
    use obs_model::{Span, SpanKind};
    use obs_scripture::PassageList;
    let text = "- Commanded\nMr 11:25\n→ KINDRED SUBJECTS See ADOPTION See SIN, FORGIVENESS OF.\n. Joseph Ge 50:20";
    let at = |needle: &str| {
        let start = text.find(needle).unwrap();
        (start, start + needle.len())
    };
    let span = |needle: &str, p: Passage| {
        let (s, e) = at(needle);
        Span {
            start: s as u32,
            end: e as u32,
            kind: SpanKind::ScriptureRef {
                targets: PassageList::from(vec![p]),
            },
        }
    };
    let spans = vec![
        span("Mr 11:25", v(Book::Mark, 11, 25)),
        span("Ge 50:20", v(Book::Genesis, 50, 20)),
    ];
    let rows = subtopics(text, &spans);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].label, "Commanded");
    assert_eq!(rows[0].passages, [v(Book::Mark, 11, 25)]);
    assert_eq!(rows[1].label, "KINDRED SUBJECTS");
    assert_eq!(rows[1].see, ["ADOPTION", "SIN, FORGIVENESS OF"]);
    assert!(rows[1].passages.is_empty());
    assert_eq!((rows[2].level, rows[2].label.as_str()), (2, "Joseph"));
    assert_eq!(display_topic("SIN, FORGIVENESS OF"), "Sin, Forgiveness of");
    assert_eq!(display_topic("Forgiveness of sin"), "Forgiveness of sin");
}

/// #261 GUIDES-1: an imported entry opens with its headword as a heading, which is no
/// subtopic.
#[test]
fn the_headword_heading_is_not_a_subtopic() {
    use obs_model::{Span, SpanKind};
    use obs_scripture::PassageList;
    let text = "FORGIVENESS\n→ Commanded Mr 11:25";
    let at = text.find("Mr 11:25").unwrap();
    let spans = vec![
        Span {
            start: 0,
            end: 11,
            kind: SpanKind::Heading { level: 2 },
        },
        Span {
            start: at as u32,
            end: (at + 8) as u32,
            kind: SpanKind::ScriptureRef {
                targets: PassageList::from(vec![v(Book::Mark, 11, 25)]),
            },
        },
    ];
    let rows = subtopics(text, &spans);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].label, "Commanded");
}

/// #261 GUIDES-16: an all-capitals headword keeps its proper names capitalised.
#[test]
fn all_capitals_topics_read_in_title_case() {
    assert_eq!(display_topic("FAITH IN CHRIST"), "Faith in Christ");
    assert_eq!(
        display_topic("FEAST OF PURIM, OR LOTS, THE"),
        "Feast of Purim, or Lots, the"
    );
    assert_eq!(display_topic("LOVE OF GOD"), "Love of God");
    assert_eq!(display_topic("SELF-DENIAL"), "Self-denial");
}

/// Manual check against a real library holding CrossWire's Nave, Torrey and the Starter's
/// Easton: `OBS_TOPIC_LIBRARY=<dir> cargo test -p obs-engine real_library_forgiveness --
/// --ignored --nocapture`.
#[test]
#[ignore = "needs OBS_TOPIC_LIBRARY with nave.obr, torrey.obr and easton.obr"]
fn real_library_forgiveness_topic_guide() {
    let dir = std::env::var_os("OBS_TOPIC_LIBRARY").expect("OBS_TOPIC_LIBRARY");
    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(Path::new(&dir)).is_empty());
    let started = std::time::Instant::now();
    let c = chooser(&mut s, "forgiveness");
    println!("directory + chooser: {:?}", started.elapsed());
    for choice in &c.choices {
        println!(
            "choice: {} [{}] {}",
            choice.topic,
            choice.sources.join(", "),
            choice.see.join("; ")
        );
    }
    assert!(c.choices.iter().any(|c| c.topic == "Pardon"));
    let SectionData::TopicalEntries(entries) =
        &*settle(&mut s, "Forgiveness", GuideSection::TopicalEntries)
    else {
        panic!("not topical entries");
    };
    for e in entries {
        println!("{} — {}", e.article.title, e.article.headword);
        for st in &e.subtopics {
            println!(
                "  {}{} ({} Passages){}{}",
                "  ".repeat(usize::from(st.level - 1)),
                st.label,
                st.passages.len(),
                if st.see.is_empty() {
                    String::new()
                } else {
                    format!(" see {}", st.see.join("; "))
                },
                st.via
                    .as_ref()
                    .map_or(String::new(), |v| format!(" via {v}"))
            );
        }
    }
    let nave = entries
        .iter()
        .find(|e| e.article.resource_id == "nave")
        .expect("Nave's FORGIVENESS");
    let labels: Vec<&str> = nave.subtopics.iter().map(|s| s.label.as_str()).collect();
    assert!(labels.contains(&"OF ENEMIES"), "{labels:?}");
    assert!(labels.contains(&"INSTANCES OF"), "{labels:?}");
    let SectionData::DictionaryArticles(articles) =
        &*settle(&mut s, "Forgiveness", GuideSection::TopicArticles)
    else {
        panic!("not articles");
    };
    for a in articles {
        println!(
            "article: {} — {}",
            a.article.abbreviation, a.article.headword
        );
    }
    assert!(articles.iter().any(|a| a.article.resource_id == "easton"));
    let SectionData::TopicPassages(key) =
        &*settle(&mut s, "Forgiveness", GuideSection::TopicPassages)
    else {
        panic!("not Key Passages");
    };
    for c in key {
        println!("key: {} × {}", short(c.passage), c.citations);
    }
    assert!(!key.is_empty());
}

#[test]
fn the_command_box_opens_the_topic_chooser() {
    use crate::command_box::CommandBoxAction;
    let t = Temp::new("command-box");
    let mut s = session(&t, &library());
    let suggestions = s.command_box_suggestions("tg forgiveness");
    assert_eq!(
        suggestions[0].action,
        CommandBoxAction::TopicGuide("forgiveness".into())
    );
    assert_eq!(suggestions[0].detail, "Topic Guide");
    let out = s
        .run_command_box_action(&suggestions[0].action, false)
        .unwrap();
    let tab = TabId(out["tab"].as_u64().unwrap());
    assert_eq!(s.topic_guides.choosing(tab), Some("forgiveness"));
    assert_eq!(
        s.command_box_suggestions("topic prayer")[0].action,
        CommandBoxAction::TopicGuide("prayer".into())
    );
}
