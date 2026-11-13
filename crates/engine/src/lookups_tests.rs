//! The right-click menu's engine commands (#142): Word Study for English words, the preferred
//! Lexicon and Dictionary lookups, and Passage-keyed Guides and Tools opened at a Passage.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use obs_model::Key;
use obs_scripture::{Book, Verse};
use serde_json::json;

use crate::*;

struct Temp(PathBuf);
impl Temp {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("obs-lookups-{name}-{}", std::process::id()));
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

/// KJV John with Strong's, the love Bible, a Hebrew-only Lexicon ranked before a Greek one,
/// and Easton's (Abraham, with the Abram alias), scanned in file-name order.
fn session(t: &Temp) -> Session {
    let resources = [
        testing::kjv_john(),
        testing::kjv_love(),
        testing::lexicon("a-hebrew", "AHL", &[("H1254", "בָּרָא", "to create")]),
        testing::greek_lexicon(),
        testing::dictionary("easton"),
    ];
    for r in &resources {
        let path = t.0.join(format!("{}.obr", r.manifest.resource_id));
        obs_store::write_obr(r, &path).unwrap();
    }
    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(&t.0).is_empty());
    s
}

fn tab(out: &serde_json::Value) -> TabId {
    TabId(out["tab"].as_u64().unwrap())
}

fn key_of(s: &Session, tab: TabId) -> Option<Key> {
    s.workspace.tab(tab).unwrap().entry_history.current()
}

#[test]
fn word_study_choose_opens_the_english_word_chooser_and_reuses_the_word_study() {
    let t = Temp::new("choose");
    let mut s = session(&t);
    let out = s
        .run("word_study.choose", &json!({ "words": "love" }))
        .unwrap();
    let study = tab(&out);
    assert_eq!(out["choose"], "love");
    assert_eq!(
        s.workspace.tab(study).unwrap().kind,
        TabKind::Guide(Guide::WordStudy)
    );
    assert_eq!(s.word_study.choosing(study), Some("love"));
    // Again: the same Word Study shows the new words; with new_tab a second one opens.
    let again = s
        .run("word_study.choose", &json!({ "words": "loved" }))
        .unwrap();
    assert_eq!(tab(&again), study);
    assert_eq!(s.word_study.choosing(study), Some("loved"));
    let other = s
        .run(
            "word_study.choose",
            &json!({ "words": "love", "new_tab": true }),
        )
        .unwrap();
    assert_ne!(tab(&other), study);
    // A Strong's number studies that number.
    let number = s
        .run("word_study.choose", &json!({ "words": "G25" }))
        .unwrap();
    assert_eq!(
        key_of(&s, tab(&number)),
        Some(Key::StrongsNumber("G25".parse().unwrap()))
    );
    assert!(
        s.run("word_study.choose", &json!({ "words": "  " }))
            .is_err()
    );
}

#[test]
fn lexicon_open_uses_the_first_lexicon_in_preferred_order_with_the_entry() {
    let t = Temp::new("lexicon");
    let mut s = session(&t);
    // G25 is only in the second Lexicon; H1254A falls back to H1254 in the first.
    let out = s.run("lexicon.open", &json!({ "strongs": "G25" })).unwrap();
    assert_eq!(out["resource"], "lexicon");
    let greek = tab(&out);
    assert_eq!(
        s.workspace.tab(greek).unwrap().kind,
        TabKind::Resource("lexicon".into())
    );
    assert_eq!(
        key_of(&s, greek),
        Some(Key::StrongsNumber("G25".parse().unwrap()))
    );
    let hebrew = s
        .run("lexicon.open", &json!({ "strongs": "H1254A" }))
        .unwrap();
    assert_eq!(hebrew["resource"], "a-hebrew");
    // The open Lexicon Tab moves; with new_tab another opens.
    let again = s
        .run("lexicon.open", &json!({ "strongs": "G3439" }))
        .unwrap();
    assert_eq!(tab(&again), greek);
    assert_eq!(
        key_of(&s, greek),
        Some(Key::StrongsNumber("G3439".parse().unwrap()))
    );
    let fresh = s
        .run(
            "lexicon.open",
            &json!({ "strongs": "G25", "new_tab": true }),
        )
        .unwrap();
    assert_ne!(tab(&fresh), greek);
    let missing = s
        .run("lexicon.open", &json!({ "strongs": "G9999" }))
        .unwrap_err();
    assert!(
        missing
            .to_string()
            .contains("No Lexicon has an entry for G9999"),
        "{missing}"
    );
}

#[test]
fn lookups_move_a_tab_only_in_the_openers_link_set() {
    let t = Temp::new("link-sets");
    let mut s = session(&t);
    let g25 = Key::StrongsNumber("G25".parse().unwrap());
    let g3439 = Key::StrongsNumber("G3439".parse().unwrap());
    // KJV in Link set A; the Lexicon already open, in Link set B, at G3439.
    let kjv = tab(&s.run("resource.open", &json!({ "id": "kjv" })).unwrap());
    s.run("tab.link", &json!({ "tab": kjv.0, "set": "A" }))
        .unwrap();
    let in_b = tab(&s
        .run(
            "lexicon.open",
            &json!({ "strongs": "G3439", "new_tab": true }),
        )
        .unwrap());
    s.run("tab.link", &json!({ "tab": in_b.0, "set": "B" }))
        .unwrap();
    s.workspace.focus(kjv);
    let in_a = tab(&s.run("lexicon.open", &json!({ "strongs": "G25" })).unwrap());
    assert_ne!(in_a, in_b, "the Lexicon in Link set B stays where it is");
    assert_eq!(s.workspace.tab(in_a).unwrap().link, Some('A'));
    assert_eq!(key_of(&s, in_a), Some(g25.clone()));
    assert_eq!(key_of(&s, in_b), Some(g3439.clone()));
    // From Link set A again: the Lexicon Tab in A moves.
    s.workspace.focus(kjv);
    let again = tab(&s
        .run("lexicon.open", &json!({ "strongs": "G3439" }))
        .unwrap());
    assert_eq!(again, in_a);
    assert_eq!(key_of(&s, in_a), Some(g3439));

    // From an unlinked Bible: neither linked Lexicon Tab moves; an unlinked one opens.
    s.workspace.focus(kjv);
    let love = tab(&s
        .run("resource.open", &json!({ "id": "kjv-love" }))
        .unwrap());
    s.run("tab.link", &json!({ "tab": love.0, "set": "" }))
        .unwrap();
    s.workspace.focus(love);
    let unlinked = tab(&s.run("lexicon.open", &json!({ "strongs": "G25" })).unwrap());
    assert!(![in_a, in_b].contains(&unlinked));
    assert_eq!(s.workspace.tab(unlinked).unwrap().link, None);
    s.workspace.focus(love);
    let unlinked_again = tab(&s
        .run("lexicon.open", &json!({ "strongs": "G3439" }))
        .unwrap());
    assert_eq!(unlinked_again, unlinked);

    // Dictionaries the same way.
    s.workspace.focus(kjv);
    let easton_a = tab(&s
        .run("dictionary.lookup", &json!({ "text": "Abraham" }))
        .unwrap());
    assert_eq!(s.workspace.tab(easton_a).unwrap().link, Some('A'));
    s.workspace.focus(love);
    let easton = tab(&s
        .run("dictionary.lookup", &json!({ "text": "Abram" }))
        .unwrap());
    assert_ne!(easton, easton_a);
    assert_eq!(key_of(&s, easton_a), Some(Key::Headword("Abraham".into())));
}

#[test]
fn dictionary_lookup_matches_headwords_after_normalization_and_aliases() {
    let t = Temp::new("dictionary");
    let mut s = session(&t);
    let out = s
        .run("dictionary.lookup", &json!({ "text": "abraham" }))
        .unwrap();
    assert_eq!(out["resource"], "easton");
    let easton = tab(&out);
    assert_eq!(key_of(&s, easton), Some(Key::Headword("Abraham".into())));
    // Abram is an alias of Abraham's entry: the same Tab moves to it.
    let alias = s
        .run("dictionary.lookup", &json!({ "text": "Abram" }))
        .unwrap();
    assert_eq!(tab(&alias), easton);
    let missing = s
        .run("dictionary.lookup", &json!({ "text": "Zzyzx" }))
        .unwrap_err();
    assert!(
        missing
            .to_string()
            .contains("No Dictionary has an entry for “Zzyzx”")
    );
}

#[test]
fn passage_keyed_tools_open_at_a_passage_in_the_openers_link_set() {
    let t = Temp::new("passage");
    let mut s = session(&t);
    let kjv = tab(&s.run("resource.open", &json!({ "id": "kjv" })).unwrap());
    s.run("tab.link", &json!({ "tab": kjv.0, "set": "A" }))
        .unwrap();
    let jn316 = Verse::new(Book::John, 3, 16);
    for (id, kind) in [
        ("guide.passage_guide", TabKind::Guide(Guide::PassageGuide)),
        ("tool.text_comparison", TabKind::Tool(Tool::TextComparison)),
        (
            "tool.cross_references",
            TabKind::Tool(Tool::CrossReferences),
        ),
    ] {
        s.workspace.focus(kjv);
        let out = s.run(id, &json!({ "passage": "Jn 3:16" })).unwrap();
        let opened = tab(&out);
        let t = s.workspace.tab(opened).unwrap();
        assert_eq!(t.kind, kind, "{id}");
        assert_eq!(t.link, Some('A'), "{id} joins the opener's Link set");
        assert_eq!(t.history.current(), Some(jn316), "{id}");
        // Another Verse moves the same Tab; with new_tab a second one opens.
        s.workspace.focus(kjv);
        let again = s.run(id, &json!({ "passage": "Jn 3:17" })).unwrap();
        assert_eq!(tab(&again), opened, "{id}");
        assert_eq!(
            s.workspace.tab(opened).unwrap().history.current(),
            Some(Verse::new(Book::John, 3, 17))
        );
        s.workspace.focus(kjv);
        let fresh = s
            .run(id, &json!({ "passage": "Jn 3:16", "new_tab": true }))
            .unwrap();
        assert_ne!(tab(&fresh), opened, "{id}");
    }
    // Without `passage` the commands open as before.
    let plain = s.run("guide.passage_guide", &json!({})).unwrap();
    assert!(plain["tab"].as_u64().is_some());
    assert!(
        s.run("guide.passage_guide", &json!({ "passage": "Hezekiah 1:1" }))
            .is_err()
    );
}

/// #294 review: with `resource`, `lexicon.open` opens that Lexicon (Word Info's Open in …)
/// even when another one comes first in Preferred order.
#[test]
fn lexicon_open_with_a_resource_opens_that_lexicon() {
    let t = Temp::new("lexicon-resource");
    let mut s = session(&t);
    let later = testing::lexicon("z-hebrew", "ZHL", &[("H1254", "בָּרָא", "to create")]);
    let path = t.0.join("z-hebrew.obr");
    obs_store::write_obr(&later, &path).unwrap();
    s.library.add_file(&path).unwrap();
    let preferred = s
        .run("lexicon.open", &json!({ "strongs": "H1254A" }))
        .unwrap();
    assert_eq!(preferred["resource"], "a-hebrew");
    let named = s
        .run(
            "lexicon.open",
            &json!({ "strongs": "H1254A", "resource": "z-hebrew" }),
        )
        .unwrap();
    assert_eq!(named["resource"], "z-hebrew");
    assert_eq!(
        s.workspace.tab(tab(&named)).unwrap().kind,
        TabKind::Resource("z-hebrew".into())
    );
    assert_eq!(
        key_of(&s, tab(&named)),
        Some(Key::StrongsNumber("H1254".parse().unwrap()))
    );
    let no_entry = s
        .run(
            "lexicon.open",
            &json!({ "strongs": "H1254", "resource": "lexicon" }),
        )
        .unwrap_err();
    assert!(
        no_entry.to_string().contains("has no entry for H1254"),
        "{no_entry}"
    );
    let not_a_lexicon = s
        .run(
            "lexicon.open",
            &json!({ "strongs": "H1254", "resource": "easton" }),
        )
        .unwrap_err();
    assert!(
        not_a_lexicon.to_string().contains("No Lexicon “easton”"),
        "{not_a_lexicon}"
    );
}
