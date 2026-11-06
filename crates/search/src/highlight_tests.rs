#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::ops::Range;

use obs_model::MorphemePart;

use crate::highlight::*;
use crate::query::{NoLexicon, parse};

fn hl(q: &str) -> Highlighter {
    Highlighter::new(&parse(q).unwrap(), &NoLexicon)
}

fn part(strongs: &str, lemma: Option<&str>) -> MorphemePart {
    let mut p = MorphemePart::new(strongs.parse().unwrap());
    p.lemma = lemma.map(String::from);
    p
}

fn shown<'a>(text: &'a str, ranges: &[Range<usize>]) -> Vec<&'a str> {
    ranges.iter().map(|r| &text[r.clone()]).collect()
}

#[test]
fn a_phrase_is_highlighted_as_one_run_case_and_punctuation_insensitive() {
    let text = "Thou shalt LOVE thy neighbour as thyself: I am the LORD.";
    let h = hl("\"love thy neighbour\"");
    assert_eq!(shown(text, &h.ranges(text, [])), ["LOVE thy neighbour"]);
    // Not when the words are apart.
    assert!(hl("\"love neighbour\"").ranges(text, []).is_empty());
}

#[test]
fn words_prefixes_and_alternatives_but_never_excluded_terms() {
    let text = "Love worketh no ill to his neighbour: therefore love is the fulfilling of the law.";
    let h = hl("(love OR law) neighb* -ill");
    assert_eq!(
        shown(text, &h.ranges(text, [])),
        ["Love", "neighbour", "love", "law"]
    );
}

#[test]
fn diacritics_and_final_forms_are_ignored() {
    let text = "Ἐν ἀρχῇ ἦν ὁ λόγος";
    assert_eq!(shown(text, &hl("λογος").ranges(text, [])), ["λόγος"]);
}

#[test]
fn strongs_terms_highlight_tagged_words_base_matches_homographs() {
    let text = "In the beginning God created the heaven";
    let created = text.find("created").unwrap();
    let god = text.find("God").unwrap();
    let parts_created = [part("H1254A", None)];
    let parts_god = [part("H430", None)];
    let tagged = || {
        [
            (god..god + 3, &parts_god[..]),
            (created..created + 7, &parts_created[..]),
        ]
    };
    assert_eq!(
        shown(text, &hl("strong:H1254").ranges(text, tagged())),
        ["created"]
    );
    assert_eq!(
        shown(text, &hl("H1254A").ranges(text, tagged())),
        ["created"]
    );
    assert!(hl("H1254B").ranges(text, tagged()).is_empty());
    // A combined query highlights both kinds.
    assert_eq!(
        shown(text, &hl("strong:H430 beginning").ranges(text, tagged())),
        ["beginning", "God"]
    );
}

#[test]
fn only_the_matching_hebrew_morphemes_are_highlighted() {
    // TAHOT Gen 1:27 וַיִּבְרָא: the prefix ו (H9001) and the root ברא (H1254A).
    let text = "וַיִּבְרָ֨א אֱלֹהִ֤ים";
    let word = 0..text.find(' ').unwrap();
    let parts = [part("H9001", None), part("H1254A", Some("בָּרָא"))];
    let tagged = [(word.clone(), &parts[..])];
    let root = hl("strong:H1254").ranges(text, tagged.clone());
    assert_eq!(shown(text, &root), ["יִּבְרָ֨א"]);
    let prefix = hl("H9001").ranges(text, tagged.clone());
    assert_eq!(shown(text, &prefix), ["וַ"]);
    // Both parts: the whole word.
    assert_eq!(
        shown(text, &hl("H9001 OR H1254").ranges(text, tagged.clone())),
        ["וַיִּבְרָ֨א"]
    );
    // By Lemma, in the original script (diacritics ignored).
    assert_eq!(shown(text, &hl("lemma:ברא").ranges(text, tagged)), ["יִּבְרָ֨א"]);
}

#[test]
fn a_single_morpheme_or_english_word_is_highlighted_whole() {
    let text = "בָּרָ֣א";
    let parts = [part("H1254A", None)];
    assert_eq!(
        shown(
            text,
            &hl("H1254").ranges(text, [(0..text.len(), &parts[..])])
        ),
        [text]
    );
    let text = "created";
    let parts = [part("H1254A", None), part("H9999", None)];
    assert_eq!(
        shown(
            text,
            &hl("H1254").ranges(text, [(0..text.len(), &parts[..])])
        ),
        [text]
    );
}

#[test]
fn transliterated_lemmas_resolve_through_the_lexicon() {
    let lookup = |t: &str| -> Vec<String> {
        if t == "bara" {
            vec!["בָּרָא".into()]
        } else {
            Vec::new()
        }
    };
    let h = Highlighter::new(&parse("lemma:bara").unwrap(), &lookup);
    let text = "בָּרָ֣א";
    let parts = [part("H1254A", Some("בָּרָא"))];
    assert_eq!(
        shown(text, &h.ranges(text, [(0..text.len(), &parts[..])])),
        [text]
    );
    assert!(Highlighter::new(&parse("lemma:xyz").unwrap(), &lookup).is_empty());
}

#[test]
fn ranges_merge() {
    assert_eq!(merge(vec![5..8, 0..2, 1..3, 8..9, 4..4]), [0..3, 5..9]);
}

/// Morphology criteria (#145) highlight the parts whose code meets them all; with a Strong's
/// number before the `@`, only that number's parts. In a Hebrew word, only the morpheme.
#[test]
fn morphology_terms_highlight_the_matching_word_or_morpheme() {
    let coded = |strongs: &str, code: &str| {
        let mut p = part(strongs, None);
        p.morph = Some(code.into());
        p
    };
    let text = "μηδὲ παριστάνετε ἀλλὰ παραστήσατε";
    let at = |w: &str| {
        let s = text.find(w).unwrap();
        s..s + w.len()
    };
    let present = [coded("G3936", "V-PAM-2P")];
    let aorist = [coded("G3936", "V-2AAM-2P")];
    let neither = [coded("G3366", "CONJ-N")];
    let tagged = || {
        [
            (at("μηδὲ"), &neither[..]),
            (at("παριστάνετε"), &present[..]),
            (at("παραστήσατε"), &aorist[..]),
        ]
    };
    let ranges = |q: &str| hl(q).ranges(text, tagged());
    assert_eq!(shown(text, &ranges("@aorist imperative")), ["παραστήσατε"]);
    assert_eq!(
        shown(text, &ranges("@imperative")),
        ["παριστάνετε", "παραστήσατε"]
    );
    assert_eq!(
        shown(text, &ranges("G3936 @present")),
        ["παριστάνετε"],
        "the number alone is not highlighted"
    );
    assert!(ranges("G26 @imperative").is_empty());
    assert_eq!(ranges("@imperative -@conjunction").len(), 2);
    // וַיִּבְרָא: the consecutive vav and the Qal verb.
    let word = "וַיִּבְרָא";
    let parts = [coded("H9001", "Hc"), coded("H1254A", "HVqw3ms")];
    let h = hl("@wayyiqtol");
    let r = h.ranges(word, [(0..word.len(), &parts[..])]);
    assert_eq!(shown(word, &r), ["יִּבְרָא"]);
}

/// Word forms (#180): every word of the stem is marked; quoted words only as written.
#[test]
fn word_forms_mark_every_word_of_the_stem() {
    let text = "He that loveth not knoweth not God; for God is love. Beloved, if God so loved us.";
    let forms = |q: &str| Highlighter::new(&parse(q).unwrap().with_word_forms(), &NoLexicon);
    assert_eq!(
        shown(text, &forms("love").ranges(text, [])),
        ["loveth", "love", "loved"]
    );
    assert_eq!(shown(text, &forms("\"love\"").ranges(text, [])), ["love"]);
    assert_eq!(shown(text, &hl("love").ranges(text, [])), ["love"]);
}

/// Proximity (#180): both sides are marked, wherever they are in the hit.
#[test]
fn proximity_marks_both_sides() {
    let text = "Even so faith, if it hath not works, is dead, being alone.";
    for q in [
        "faith NEAR/5 works",
        "faith BEFORE/5 works",
        "faith WITHIN 2 verses works",
    ] {
        assert_eq!(
            shown(text, &hl(q).ranges(text, [])),
            ["faith", "works"],
            "{q}"
        );
    }
}
