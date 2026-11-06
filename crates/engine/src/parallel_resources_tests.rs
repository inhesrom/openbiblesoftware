//! Parallel resources (#151): a Resource Tab switches to another Resource of its Resource
//! type at the same place, through Verse maps, with the switch in its Back/Forward history.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use obs_model::{Key, Resource};
use obs_scripture::{Book, ListedVersification, Passage, Verse};
use serde_json::json;

use crate::resource_navigation::ResourceNavigation;
use crate::testing::bibles::{bsb_psalm_3, tahot_psalms};
use crate::testing::{commentary, lexicon};
use crate::*;

fn ps(chapter: u16, verse: u16) -> Verse {
    Verse::new(Book::Psalms, chapter, verse)
}

/// A Session whose library holds `resources`, in that order (their Preferred order).
fn session(resources: &[&Resource]) -> Session {
    let mut s = Session::new(JobMode::Inline);
    for r in resources {
        s.library.add(LibraryEntry {
            manifest: r.manifest.clone(),
            path: format!("/library/{}.obr", r.manifest.resource_id).into(),
        });
    }
    s
}

fn open(s: &mut Session, id: &str) -> TabId {
    let tab = TabId(
        s.run("resource.open", &json!({ "id": id })).unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    s.workspace.take_effects();
    tab
}

/// What the UI does when a Tab's Resource is loaded: take its Verse map, numbering and Keys,
/// then place the Tab where a switch left it.
fn load(s: &mut Session, tab: TabId, r: &Resource) -> Option<Verse> {
    let t = s.workspace.tab_mut(tab).unwrap();
    assert_eq!(t.kind, TabKind::Resource(r.manifest.resource_id.clone()));
    t.versification = (r.manifest.resource_type == obs_model::ResourceType::Bible).then(|| {
        Arc::new(ListedVersification::new(
            r.verses.iter().map(|v| (v.verse, v.last)),
        ))
    });
    if r.manifest.resource_type != obs_model::ResourceType::Bible {
        t.navigation = Some(Arc::new(ResourceNavigation::new(r.keys.clone())));
    }
    t.verse_map = r.verse_map.clone().map(Arc::new);
    let at = s.workspace.arrival(tab);
    if let Some(v) = at {
        s.workspace.scrolled(tab, v);
    }
    at
}

fn at(s: &Session, tab: TabId) -> Option<Verse> {
    s.workspace.tab(tab).unwrap().history.current()
}

fn shows(s: &Session, tab: TabId) -> String {
    match &s.workspace.tab(tab).unwrap().kind {
        TabKind::Resource(id) => id.clone(),
        other => panic!("not a Resource Tab: {other:?}"),
    }
}

#[test]
fn bsb_at_ps_3_1_cycles_to_tahot_at_hebrew_ps_3_2_and_back_returns_to_bsb_at_3_1() {
    let (bsb, tahot) = (bsb_psalm_3(), tahot_psalms());
    // The Starter imports: the BSB numbers Psalms like the Standard versification; TAHOT
    // numbers the superscription as Ps 3:1, so its Ps 3:2 is Standard Ps 3:1.
    assert_eq!(
        link::to_standard(bsb.verse_map.as_ref(), ps(3, 1)),
        link::ToStandard::Verse(ps(3, 1))
    );
    let map = tahot.verse_map.as_ref().unwrap();
    assert_eq!(map.first_native(ps(3, 1)), Some(ps(3, 2)));
    let mut s = session(&[&bsb, &tahot]);
    let tab = open(&mut s, "bsb");
    load(&mut s, tab, &bsb);
    s.run("tab.link", &json!({ "tab": tab.0, "set": "A" }))
        .unwrap();
    s.run("tab.go", &json!({ "tab": tab.0, "text": "Ps 3:1" }))
        .unwrap();
    s.workspace.take_effects();

    // Ctrl+] (or ▶): the next Bible in Preferred order.
    let out = s
        .run("tab.parallel_next", &json!({ "tab": tab.0 }))
        .unwrap();
    assert_eq!(out["id"], "tahot");
    assert_eq!(s.workspace.take_effects(), vec![Effect::Switched(tab)]);
    let t = s.workspace.tab(tab).unwrap();
    assert_eq!(t.title, "TAHOT");
    assert_eq!(t.link, Some('A'), "the Tab keeps its Link set");
    assert!(t.verse_map.is_none(), "nothing of the BSB stays loaded");
    assert_eq!(load(&mut s, tab, &tahot), Some(ps(3, 2)));
    assert_eq!(at(&s, tab), Some(ps(3, 2)));

    // Back returns to the BSB at the same place; Forward to TAHOT again.
    let out = s.run("tab.back", &json!({ "tab": tab.0 })).unwrap();
    assert_eq!(
        (out["verse"].as_str(), out["id"].as_str()),
        (Some("Ps.3.1"), Some("bsb"))
    );
    assert_eq!(s.workspace.take_effects(), vec![Effect::Switched(tab)]);
    assert_eq!(shows(&s, tab), "bsb");
    assert_eq!(s.workspace.tab(tab).unwrap().title, "BSB");
    assert_eq!(
        load(&mut s, tab, &bsb),
        None,
        "Back is in the BSB's own numbering"
    );
    assert_eq!(at(&s, tab), Some(ps(3, 1)));
    let out = s.run("tab.forward", &json!({ "tab": tab.0 })).unwrap();
    assert_eq!(
        (out["verse"].as_str(), out["id"].as_str()),
        (Some("Ps.3.2"), Some("tahot"))
    );
    load(&mut s, tab, &tahot);
    assert_eq!(
        (shows(&s, tab), at(&s, tab)),
        ("tahot".into(), Some(ps(3, 2)))
    );
}

#[test]
fn the_switch_is_one_history_entry_among_the_tabs_jumps() {
    let (bsb, tahot) = (bsb_psalm_3(), tahot_psalms());
    let mut s = session(&[&bsb, &tahot]);
    let tab = open(&mut s, "bsb");
    load(&mut s, tab, &bsb);
    s.run("tab.go", &json!({ "tab": tab.0, "text": "Ps 3:3" }))
        .unwrap();
    s.run("tab.go", &json!({ "tab": tab.0, "text": "Ps 3:1" }))
        .unwrap();
    s.run("tab.parallel", &json!({ "tab": tab.0, "id": "tahot" }))
        .unwrap();
    load(&mut s, tab, &tahot);
    // A jump in TAHOT, in its own numbering.
    s.run("tab.go", &json!({ "tab": tab.0, "text": "Ps 3:3" }))
        .unwrap();
    s.workspace.take_effects();
    let back = |s: &mut Session| s.run("tab.back", &json!({ "tab": tab.0 })).unwrap();
    // TAHOT 3:3 → TAHOT 3:2: an ordinary Back, no reload.
    assert_eq!(back(&mut s)["verse"], "Ps.3.2");
    assert_eq!(
        s.workspace.take_effects(),
        vec![Effect::Navigate {
            tab,
            verse: ps(3, 2)
        }]
    );
    // → BSB 3:1 (the switch), → BSB 3:3 (the BSB's own earlier jump).
    assert_eq!(back(&mut s)["id"], "bsb");
    load(&mut s, tab, &bsb);
    assert_eq!(at(&s, tab), Some(ps(3, 1)));
    let out = back(&mut s);
    assert_eq!(
        (out["verse"].as_str(), out.get("id")),
        (Some("Ps.3.3"), None)
    );
    assert_eq!(shows(&s, tab), "bsb");
    // Forward replays the switch.
    s.run("tab.forward", &json!({ "tab": tab.0 })).unwrap();
    let out = s.run("tab.forward", &json!({ "tab": tab.0 })).unwrap();
    assert_eq!(out["id"], "tahot");
    load(&mut s, tab, &tahot);
    assert_eq!(at(&s, tab), Some(ps(3, 2)));
}

#[test]
fn cycling_follows_preferred_order_within_the_type_and_wraps_around() {
    let (bsb, tahot) = (bsb_psalm_3(), tahot_psalms());
    let tc = commentary(
        "tc",
        "Test Commentary",
        "TC",
        &[(Passage::verse(ps(3, 1)), None, "On the psalm.")],
    );
    let mut s = session(&[&bsb, &tc, &tahot]);
    // The user ranks TAHOT above the BSB.
    s.library
        .set_preferred_order(vec!["tahot".into(), "bsb".into(), "tc".into()]);
    let tab = open(&mut s, "bsb");
    let ids: Vec<&str> = parallel_resources(&s, tab)
        .iter()
        .map(|e| e.manifest.resource_id.as_str())
        .collect();
    assert_eq!(ids, ["tahot", "bsb"], "Bibles only, in Preferred order");
    let cycle = |s: &mut Session, id: &str| {
        s.run(id, &json!({ "tab": tab.0 })).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert_eq!(cycle(&mut s, "tab.parallel_next"), "tahot", "wraps around");
    assert_eq!(cycle(&mut s, "tab.parallel_next"), "bsb");
    assert_eq!(cycle(&mut s, "tab.parallel_previous"), "tahot");
    // The dropdown only offers the Tab's own Resource type.
    let err = s
        .run("tab.parallel", &json!({ "tab": tab.0, "id": "tc" }))
        .unwrap_err();
    assert!(err.to_string().contains("not one of the Bibles"), "{err}");
    // A Commentary alone in its type has nothing to cycle to.
    let c = open(&mut s, "tc");
    let err = s
        .run("tab.parallel_next", &json!({ "tab": c.0 }))
        .unwrap_err();
    assert!(err.to_string().contains("no other Commentaries"), "{err}");
    // A Tool is not a Resource Tab: the keys are left to it.
    let search = TabId(
        s.run("tool.search", &json!({})).unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    assert_eq!(s.workspace.focused(), Some(search));
    assert!(!s.is_enabled("tab.parallel_next"));
    s.workspace.focus(tab);
    assert!(s.is_enabled("tab.parallel_next"));
}

#[test]
fn a_commentary_lacking_the_passage_shows_its_nearest_earlier_section() {
    let a = commentary(
        "ca",
        "Commentary A",
        "CA",
        &[(
            Passage::new(ps(3, 3), ps(3, 4)).unwrap(),
            None,
            "Verses three and four.",
        )],
    );
    let b = commentary(
        "cb",
        "Commentary B",
        "CB",
        &[
            (
                Passage::new(ps(2, 1), ps(2, 12)).unwrap(),
                None,
                "Psalm two.",
            ),
            (
                Passage::new(ps(3, 1), ps(3, 2)).unwrap(),
                None,
                "Verses one and two.",
            ),
        ],
    );
    let c = commentary(
        "cc",
        "Commentary C",
        "CC",
        &[(Passage::verse(ps(4, 1)), None, "Psalm four.")],
    );
    let mut s = session(&[&a, &b, &c]);
    let tab = open(&mut s, "ca");
    load(&mut s, tab, &a);
    s.run("tab.go", &json!({ "tab": tab.0, "text": "Ps 3:3" }))
        .unwrap();
    s.run("tab.parallel_next", &json!({ "tab": tab.0 }))
        .unwrap();
    assert_eq!(
        load(&mut s, tab, &b),
        Some(ps(3, 1)),
        "nearest earlier section"
    );
    // One with nothing at or before it opens as a new Tab of it would; Back still returns.
    s.run("tab.parallel_next", &json!({ "tab": tab.0 }))
        .unwrap();
    assert_eq!(load(&mut s, tab, &c), None);
    s.run("tab.back", &json!({ "tab": tab.0 })).unwrap();
    assert_eq!(shows(&s, tab), "cb");
    load(&mut s, tab, &b);
    assert_eq!(at(&s, tab), Some(ps(3, 1)));
}

#[test]
fn a_lexicon_switches_at_the_same_key_and_back_returns_to_the_first() {
    let one = lexicon(
        "lex-one",
        "ONE",
        &[("G25", "ἀγαπάω", "to love"), ("G26", "ἀγάπη", "love")],
    );
    let two = lexicon("lex-two", "TWO", &[("G26", "ἀγάπη", "love, charity")]);
    let mut s = session(&[&one, &two]);
    let tab = open(&mut s, "lex-one");
    load(&mut s, tab, &one);
    let g = |n: &str| Key::StrongsNumber(n.parse().unwrap());
    s.run("tab.go", &json!({ "tab": tab.0, "text": "G25" }))
        .unwrap();
    s.run("tab.go", &json!({ "tab": tab.0, "text": "G26" }))
        .unwrap();
    s.run("tab.parallel_next", &json!({ "tab": tab.0 }))
        .unwrap();
    let t = s.workspace.tab(tab).unwrap();
    assert_eq!(
        (shows(&s, tab), t.entry_history.current()),
        ("lex-two".into(), Some(g("G26")))
    );
    assert_eq!(t.header_box(), HeaderBox::Lexicon);
    load(&mut s, tab, &two);
    let out = s.run("tab.back", &json!({ "tab": tab.0 })).unwrap();
    assert_eq!(out["id"], "lex-one");
    assert_eq!(
        s.workspace.tab(tab).unwrap().entry_history.current(),
        Some(g("G26"))
    );
    load(&mut s, tab, &one);
    s.run("tab.back", &json!({ "tab": tab.0 })).unwrap();
    assert_eq!(
        s.workspace.tab(tab).unwrap().entry_history.current(),
        Some(g("G25"))
    );
    assert_eq!(shows(&s, tab), "lex-one");
}

#[test]
fn the_last_session_keeps_the_resource_the_tab_switched_to() {
    let (bsb, tahot) = (bsb_psalm_3(), tahot_psalms());
    let mut s = session(&[&bsb, &tahot]);
    let tab = open(&mut s, "bsb");
    load(&mut s, tab, &bsb);
    s.run("tab.go", &json!({ "tab": tab.0, "text": "Ps 3:1" }))
        .unwrap();
    s.run("tab.parallel_next", &json!({ "tab": tab.0 }))
        .unwrap();
    load(&mut s, tab, &tahot);
    let snap = s.workspace.snapshot();
    let saved = snap.tabs.iter().find(|t| t.id == tab.0).unwrap();
    assert_eq!(saved.kind, TabKind::Resource("tahot".into()));
    assert_eq!(saved.title, "TAHOT");
    assert_eq!(saved.location, Some(ps(3, 2)));
}

/// The same switch with the real Starter sources (pinned in `starter/sources.toml`), imported
/// as `cargo xtask starter` imports them: the BSB download's `PSA.usfm` and TAHOT's Job–Song
/// file. Not run by default (they are downloads):
/// `OBS_REAL_BSB_PSA=… OBS_REAL_TAHOT_JOB_SNG=… cargo test -p obs-engine -- --ignored real_`
#[test]
#[ignore = "needs the real BSB and TAHOT sources"]
fn real_bsb_ps_3_1_switches_to_real_tahot_ps_3_2() {
    let read = |var: &str| {
        let path = std::env::var(var).unwrap_or_else(|_| panic!("set {var}"));
        std::fs::read_to_string(path).unwrap()
    };
    let mut usfm = obs_formats::UsfmImport::new(obs_formats::UsfmOptions {
        resource_id: "bsb".into(),
        title: "Berean Standard Bible".into(),
        abbreviation: "BSB".into(),
        language: "en".into(),
        ..Default::default()
    });
    usfm.add_book("PSA.usfm", read("OBS_REAL_BSB_PSA")).unwrap();
    let bsb = usfm.finish().unwrap().0;
    let mut step = obs_formats::StepbibleImport::new(obs_formats::StepbibleOptions {
        text: obs_formats::StepbibleText::Tahot,
        resource_id: "tahot".into(),
        title: "Hebrew Old Testament (STEPBible TAHOT)".into(),
        abbreviation: "TAHOT".into(),
        language: "hbo".into(),
        ..Default::default()
    });
    step.add_file("TAHOT Job-Sng", read("OBS_REAL_TAHOT_JOB_SNG"));
    let tahot = step.finish().unwrap().0;
    let mut s = session(&[&bsb, &tahot]);
    let tab = open(&mut s, "bsb");
    load(&mut s, tab, &bsb);
    s.run("tab.go", &json!({ "tab": tab.0, "text": "Ps 3:1" }))
        .unwrap();
    s.run("tab.parallel_next", &json!({ "tab": tab.0 }))
        .unwrap();
    assert_eq!(load(&mut s, tab, &tahot), Some(ps(3, 2)));
    s.run("tab.back", &json!({ "tab": tab.0 })).unwrap();
    load(&mut s, tab, &bsb);
    assert_eq!(
        (shows(&s, tab), at(&s, tab)),
        ("bsb".into(), Some(ps(3, 1)))
    );
}

#[test]
fn a_switch_and_back_across_it_are_visits_for_history() {
    let (bsb, tahot) = (bsb_psalm_3(), tahot_psalms());
    let mut s = session(&[&bsb, &tahot]);
    let tab = open(&mut s, "bsb");
    load(&mut s, tab, &bsb);
    s.run("tab.go", &json!({ "tab": tab.0, "text": "Ps 3:1" }))
        .unwrap();
    s.workspace.take_visits();

    // The switch is placed by the UI later, so its Visit is read then (#155).
    s.run("tab.parallel_next", &json!({ "tab": tab.0 }))
        .unwrap();
    assert_eq!(
        s.workspace.take_visits(),
        vec![workspace::PendingVisit { tab, at: None }]
    );
    load(&mut s, tab, &tahot);
    let (location, label) = s.location_of(tab).unwrap();
    assert_eq!(
        (location.kind, location.verse),
        (TabKind::Resource("tahot".into()), Some(ps(3, 2)))
    );
    assert!(label.ends_with("· TAHOT"), "{label}");

    // Back across the switch is a Visit of the BSB at Ps 3:1.
    s.run("tab.back", &json!({ "tab": tab.0 })).unwrap();
    let visits = s.workspace.take_visits();
    assert_eq!(visits.len(), 1);
    assert_eq!(visits[0].at.as_ref().unwrap().verse, Some(ps(3, 1)));
    assert_eq!(shows(&s, tab), "bsb");
}
