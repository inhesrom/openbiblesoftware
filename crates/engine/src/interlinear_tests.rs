//! Interlinear lines (#143): what each line reads under a word, which lines a Tab shows, and
//! that the choice is kept per Tab, in Layouts and in the Last session.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use obs_model::MorphemePart;
use obs_scripture::StrongsNumber;
use serde_json::json;

use crate::interlinear::{
    InterlinearKind, InterlinearLine, InterlinearLines, LexiconWord, cell_gloss, cell_texts,
};
use crate::testing::bibles::{tahot, write_library};
use crate::testing::kjv_john;
use crate::*;

use InterlinearLine::{Gloss, Lemma, Morphology, Strongs, Transliteration};

fn part(
    strongs: &str,
    lemma: Option<&str>,
    morph: Option<&str>,
    gloss: Option<&str>,
) -> MorphemePart {
    MorphemePart {
        strongs: strongs.parse().unwrap(),
        lemma: lemma.map(String::from),
        morph: morph.map(String::from),
        gloss: gloss.map(String::from),
    }
}

fn no_lexicon(_: StrongsNumber) -> Option<LexiconWord> {
    None
}

/// TAHOT Gen 1:1's first word: the preposition בְּ (H9003) and רֵאשִׁית (H7225G).
#[test]
fn tahot_bereshit_reads_its_gloss_transliteration_morphology_and_number() {
    let parts = [
        part("H9003", Some("בְּ"), Some("HR"), Some("in")),
        part("H7225G", Some("רֵאשִׁית"), Some("HNcfsa"), Some("beginning")),
    ];
    let kind = InterlinearKind::for_language("hbo");
    assert_eq!(kind, InterlinearKind::Interlinear);
    let lines = kind.lines();
    assert_eq!(lines, &[Gloss, Transliteration, Morphology, Strongs]);
    let cells = cell_texts(lines, kind, "בְּרֵאשִׁ֖ית", &parts, &no_lexicon);
    assert_eq!(cells[0], "in beginning", "every morpheme's gloss, in order");
    assert_eq!(cells[1], obs_model::transliterate("בְּרֵאשִׁ֖ית"));
    assert!(!cells[1].is_empty());
    assert_eq!(
        cells[2],
        obs_model::morph::parse("HNcfsa").unwrap().short(),
        "the word's own Morphology, in plain English"
    );
    assert!(!cells[2].contains("HNcfsa"));
    assert_eq!(
        cells[3], "H7225G",
        "the word's own number, not its prefix's"
    );
}

/// KJV+Strong's words carry only the number: the Lemma and Gloss come from the Lexicon,
/// looked up once per word.
#[test]
fn kjv_loved_reads_the_lexicon_lemma_its_transliteration_and_gloss() {
    let kind = InterlinearKind::for_language("en");
    assert_eq!(kind, InterlinearKind::Reverse);
    assert_eq!(
        kind.lines(),
        &[Lemma, Transliteration, Strongs, Morphology, Gloss]
    );
    let calls = std::cell::Cell::new(0);
    let lexicon = |n: StrongsNumber| {
        calls.set(calls.get() + 1);
        (n.to_string() == "G25").then(|| LexiconWord {
            lemma: Some("ἀγαπάω".into()),
            gloss: Some("to love".into()),
        })
    };
    let cells = cell_texts(
        kind.lines(),
        kind,
        "loved",
        &[part("G25", None, None, None)],
        &lexicon,
    );
    assert_eq!(cells, ["ἀγαπάω", "agapao", "G25", "", "to love"]);
    assert_eq!(calls.get(), 1);
    // An unreadable code (KJV's TH-style tags) shows no Morphology rather than the code.
    let cells = cell_texts(
        &[Morphology],
        kind,
        "created",
        &[part("H1254A", None, Some("strongMorph:TH8804"), None)],
        &lexicon,
    );
    assert_eq!(cells, [""]);
    // A Greek NT morphology code reads as plain English.
    let cells = cell_texts(
        &[Morphology],
        kind,
        "loved",
        &[part("G25", None, Some("V-AAI-3S"), None)],
        &lexicon,
    );
    assert_eq!(cells, ["aorist active indicative, 3rd sing."]);
}

#[test]
fn a_cell_gloss_prefers_a_brief_lexicon_and_cuts_a_long_one() {
    // Strong's first, then TBESG: TBESG's brief gloss wins.
    let strongs =
        "perhaps from ἄγαν (much) (or compare G5689); to love (in a social or moral sense)";
    assert_eq!(cell_gloss([strongs, "to love"]).as_deref(), Some("to love"));
    let cut = cell_gloss([strongs]).unwrap();
    assert!(cut.chars().count() <= 28, "{cut}");
    assert!(cut.ends_with('…'), "{cut}");
    assert_eq!(
        cell_gloss(["of uncertain affinity; a deity"]).as_deref(),
        Some("of uncertain affinity")
    );
    assert_eq!(cell_gloss(["", "  "]), None);
}

#[test]
fn a_tab_picks_its_lines_and_keeps_at_least_one() {
    let kind = InterlinearKind::Reverse;
    let mut lines = InterlinearLines::default();
    assert!(lines.shown(kind).is_empty(), "off by default");
    lines.on = true;
    assert_eq!(lines.shown(kind), [Lemma, Transliteration, Strongs]);
    lines.set_line(kind, Gloss, true);
    lines.set_line(kind, Transliteration, false);
    assert_eq!(
        lines.shown(kind),
        [Lemma, Strongs, Gloss],
        "in the Bible's order"
    );
    lines.set_line(kind, Lemma, false);
    lines.set_line(kind, Strongs, false);
    lines.set_line(kind, Gloss, false);
    assert_eq!(lines.shown(kind), [Gloss], "the last line stays");
    lines.on = false;
    assert!(lines.shown(kind).is_empty());
    assert!(
        lines.is_chosen(kind, Gloss),
        "the choice outlives turning them off"
    );
}

struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// KJV (John) and TAHOT as real `.obr` files, with a User database.
fn launch(name: &str) -> (Session, Temp) {
    let root = std::env::temp_dir().join(format!("obs-interlinear-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_library(&root.join("library"), &[kjv_john(), tahot()]);
    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(&root.join("library")).is_empty());
    let (store, _) = obs_store::UserStore::open(&root.join("user.db")).unwrap();
    s.attach_user(Box::new(store));
    (s, Temp(root))
}

fn open(s: &mut Session, id: &str, split: bool) -> TabId {
    let out = s
        .run("resource.open", &json!({ "id": id, "split": split }))
        .unwrap();
    TabId(out["tab"].as_u64().unwrap())
}

fn lines(s: &Session, tab: TabId) -> InterlinearLines {
    s.workspace.tab(tab).unwrap().interlinear.clone()
}

#[test]
fn tab_interlinear_toggles_lines_per_tab_and_layouts_keep_them() {
    let (mut s, _t) = launch("layouts");
    let kjv = open(&mut s, "kjv", false);
    let tahot = open(&mut s, "tahot", true);
    let out = s.run("tab.interlinear", &json!({ "tab": kjv.0 })).unwrap();
    assert_eq!(
        out,
        json!({ "on": true, "lines": ["lemma", "transliteration", "strongs"] })
    );
    assert!(!lines(&s, tahot).on, "only that Tab");
    let out = s
        .run(
            "tab.interlinear",
            &json!({ "tab": tahot.0, "line": "morphology" }),
        )
        .unwrap();
    assert_eq!(
        out,
        json!({ "on": true, "lines": ["gloss", "transliteration", "morphology"] })
    );
    // A Hebrew Bible has no Lemma line (the Hebrew is the text itself).
    assert!(
        s.run(
            "tab.interlinear",
            &json!({ "tab": tahot.0, "line": "lemma" })
        )
        .is_err()
    );
    s.run("layout.save", &json!({ "name": "Interlinear" }))
        .unwrap();
    s.run("tab.interlinear", &json!({ "tab": kjv.0, "on": false }))
        .unwrap();
    s.run(
        "tab.interlinear",
        &json!({ "tab": tahot.0, "line": "gloss", "shown": false }),
    )
    .unwrap();
    assert_eq!(
        lines(&s, tahot).shown(InterlinearKind::Interlinear),
        [Transliteration, Morphology]
    );
    s.run("layout.restore", &json!({ "name": "Interlinear" }))
        .unwrap();
    let by_kind = |s: &Session, id: &str| {
        s.workspace
            .tabs()
            .find(|t| t.kind == TabKind::Resource(id.into()))
            .unwrap()
            .interlinear
            .clone()
    };
    assert_eq!(
        by_kind(&s, "kjv").shown(InterlinearKind::Reverse),
        [Lemma, Transliteration, Strongs]
    );
    assert_eq!(
        by_kind(&s, "tahot").shown(InterlinearKind::Interlinear),
        [Gloss, Transliteration, Morphology]
    );
    // The Last session keeps them too; a Tab that never had them saves nothing extra.
    let snap = s.workspace.snapshot();
    let json = snap.to_json();
    let back = WorkspaceSnapshot::from_json(&json).unwrap();
    assert_eq!(back, snap);
    let plain = TabSnapshot {
        interlinear: InterlinearLines::default(),
        ..snap.tabs[0].clone()
    };
    assert!(
        !serde_json::to_string(&plain)
            .unwrap()
            .contains("interlinear")
    );
}

#[test]
fn tab_interlinear_needs_a_bible_tab() {
    let (mut s, _t) = launch("bible-only");
    let out = s.run("tool.word_info", &json!({})).unwrap();
    let tab = TabId(out["tab"].as_u64().unwrap());
    assert!(s.run("tab.interlinear", &json!({ "tab": tab.0 })).is_err());
}
