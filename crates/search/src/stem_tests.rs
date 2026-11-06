#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::stem::*;

#[test]
fn porter_stems_the_classic_examples() {
    // From Porter's paper and his reference vocabulary.
    let table = [
        ("caresses", "caress"),
        ("ponies", "poni"),
        ("cats", "cat"),
        ("feed", "feed"),
        ("agreed", "agre"),
        ("plastered", "plaster"),
        ("motoring", "motor"),
        ("sing", "sing"),
        ("conflated", "conflat"),
        ("troubled", "troubl"),
        ("sized", "size"),
        ("hopping", "hop"),
        ("falling", "fall"),
        ("hissing", "hiss"),
        ("filing", "file"),
        ("happy", "happi"),
        ("relational", "relat"),
        ("conditional", "condit"),
        ("rational", "ration"),
        ("generalization", "gener"),
        ("sensibility", "sensibl"),
        ("triplicate", "triplic"),
        ("hopefulness", "hope"),
        ("revival", "reviv"),
        ("adjustment", "adjust"),
        ("adoption", "adopt"),
        ("controlling", "control"),
        ("roll", "roll"),
        ("generate", "gener"),
    ];
    for (word, want) in table {
        assert_eq!(stem(word), want, "{word}");
    }
}

#[test]
fn archaic_verb_forms_share_the_modern_stem() {
    let groups: &[&[&str]] = &[
        &[
            "love", "loved", "loves", "loving", "loveth", "lovest", "lovedst",
        ],
        &["come", "comes", "coming", "cometh", "comest"],
        &["give", "gives", "giving", "giveth", "givest"],
        &["know", "knows", "knowing", "knoweth", "knowest"],
        &["see", "seeing", "seeth", "seest"],
        &["go", "goeth", "goest"],
        &["run", "running", "runneth"],
        &["bless", "blessed", "blessing", "blesseth"],
        &["dwell", "dwelling", "dwelleth"],
        &["lies", "lieth"],
        &["holy", "holiest"],
        &["have", "hath", "hast"],
        &["do", "doth", "dost", "doeth"],
        &["say", "saith"],
    ];
    for group in groups {
        let s = stem(group[0]);
        for w in *group {
            assert_eq!(stem(w), s, "{w} should stem with {}", group[0]);
        }
    }
}

#[test]
fn words_that_only_end_like_archaic_verbs_keep_their_stem() {
    for (word, other) in [
        ("priest", "pry"),
        ("forest", "for"),
        ("honest", "hon"),
        ("earnest", "earn"),
        ("teeth", "tee"),
        ("twentieth", "twenty"),
    ] {
        assert_ne!(stem(word), stem(other), "{word}");
    }
    // Too short to be a verb with an ending.
    assert_eq!(stem("rest"), "rest");
    assert_eq!(stem("best"), "best");
    assert_eq!(stem("seth"), "seth");
    // Unrelated words stay apart.
    assert_ne!(stem("love"), stem("live"));
    assert_ne!(stem("lord"), stem("lot"));
}

#[test]
fn only_english_words_are_stemmed() {
    for word in ["αγαπη", "ברא", "naïve", "3", "g26", "ye", "a"] {
        assert_eq!(stem(word), word);
    }
}

#[test]
fn every_word_of_a_stem_begins_with_its_prefix() {
    let words = [
        "love",
        "loveth",
        "hath",
        "doth",
        "saith",
        "seeth",
        "ability",
        "sensibility",
        "happy",
        "holiest",
        "lieth",
        "relational",
        "generalization",
        "hopping",
        "agreeing",
        "fleeth",
        "lovedst",
        "didst",
        "wouldst",
        "hast",
        "goeth",
        "diest",
        "mightiest",
    ];
    for w in words {
        let s = stem(w);
        assert!(w.starts_with(stem_prefix(&s)), "{w} → {s}");
    }
}

#[test]
fn the_word_lists_are_sorted_for_binary_search() {
    assert!(IRREGULAR.windows(2).all(|w| w[0].0 < w[1].0));
    assert!(NOT_ARCHAIC.windows(2).all(|w| w[0] < w[1]));
    for (word, modern) in IRREGULAR {
        assert_eq!(stem(word), stem(modern), "{word}");
    }
}
