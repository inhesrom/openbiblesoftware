//! Find in Resource matching (#175).

use crate::find::{FindOptions, Finder};

fn find(query: &str, options: FindOptions, text: &str) -> Vec<String> {
    Finder::new(query, options)
        .map(|f| f.find(text))
        .unwrap_or_default()
        .into_iter()
        .map(|r| text[r].to_string())
        .collect()
}

fn opts(match_case: bool, whole_word: bool, ignore_diacritics: bool) -> FindOptions {
    FindOptions {
        match_case,
        whole_word,
        ignore_diacritics,
    }
}

const JOHN: &str = "Greater love hath no man than this. He that loveth not knoweth not God; \
                    for God is love. LOVE ye one another, as I have loved you.";

#[test]
fn every_match_in_order_ignoring_case_by_default() {
    let d = FindOptions::default();
    assert!(d.ignore_diacritics && !d.match_case && !d.whole_word);
    assert_eq!(
        find("love", d, JOHN),
        ["love", "love", "love", "LOVE", "love"]
    );
    let ranges = Finder::new("love", d).unwrap().find(JOHN);
    assert!(ranges.windows(2).all(|w| w[0].end <= w[1].start));
}

#[test]
fn match_case_and_whole_word() {
    assert_eq!(find("LOVE", opts(true, false, true), JOHN), ["LOVE"]);
    assert_eq!(
        find("love", opts(false, true, true), JOHN),
        ["love", "love", "LOVE"]
    );
    assert_eq!(find("love", opts(true, true, true), JOHN), ["love", "love"]);
    // A phrase across words, with its spaces.
    assert_eq!(
        find("one another", FindOptions::default(), JOHN),
        ["one another"]
    );
}

#[test]
fn ignore_diacritics_finds_greek_typed_without_accents() {
    let text = "Ἀγαπητοί, ἀγαπῶμεν ἀλλήλους, ὅτι ἡ ἀγάπη ἐκ τοῦ θεοῦ ἐστιν. ὁ θεὸς ἀγάπη ἐστίν.";
    let d = FindOptions::default();
    assert_eq!(find("αγαπη", d, text), ["Ἀγαπη", "ἀγάπη", "ἀγάπη"]);
    assert_eq!(
        find("αγαπη", opts(false, true, true), text),
        ["ἀγάπη", "ἀγάπη"]
    );
    assert_eq!(find("θεος", d, text), ["θεὸς"]);
    // Final sigma typed or not.
    assert_eq!(find("θεοσ", d, text), ["θεὸς"]);
    // With diacritics respected, only the accented form typed exactly matches.
    let strict = opts(false, false, false);
    assert!(find("αγαπη", strict, text).is_empty());
    assert_eq!(find("ἀγάπη", strict, text), ["ἀγάπη", "ἀγάπη"]);
}

#[test]
fn ignore_diacritics_finds_hebrew_typed_without_points() {
    let text = "בְּרֵאשִׁ֖ית בָּרָ֣א אֱלֹהִ֑ים אֵ֥ת הַשָּׁמַ֖יִם וְאֵ֥ת הָאָֽרֶץ׃";
    let d = FindOptions::default();
    assert_eq!(find("בראשית", d, text), ["בְּרֵאשִׁ֖ית"]);
    // Final forms fold: typed with final mem, it still matches.
    assert_eq!(find("אלהים", d, text), ["אֱלֹהִ֑ים"]);
    assert_eq!(find("ארץ", d, text), ["אָֽרֶץ"]);
    assert_eq!(find("את", opts(false, true, true), text), ["אֵ֥ת"]);
    assert!(find("בראשית", opts(false, false, false), text).is_empty());
}

#[test]
fn latin_letters_find_greek_and_hebrew_by_transliteration() {
    let greek = "ὅτι ἡ ἀγάπη ἐκ τοῦ θεοῦ ἐστιν, τὴν ἀγάπην";
    let d = FindOptions::default();
    assert_eq!(find("agape", d, greek), ["ἀγάπη"]);
    assert_eq!(find("tou theou", d, greek), ["τοῦ θεοῦ"]);
    // Always whole words: "the" is not every θεός.
    assert!(find("the", d, greek).is_empty());
    // Not with diacritics respected.
    assert!(find("agape", opts(false, false, false), greek).is_empty());
    // Not under Match case: transliteration keys are case-folded, so "AGAPE" (or even
    // "agape") would otherwise ignore the case the user asked for.
    assert!(find("AGAPE", opts(true, false, true), greek).is_empty());
    assert!(find("agape", opts(true, false, true), greek).is_empty());
    assert!(find("AGAPE", opts(true, true, true), greek).is_empty());
    let hebrew = "בְּרֵאשִׁ֖ית בָּרָ֣א אֱלֹהִ֑ים";
    assert_eq!(find("bara", d, hebrew), ["בָּרָ֣א"]);
    assert!(find("bara", opts(true, false, true), hebrew).is_empty());
    // Match case still finds the Greek itself, case-sensitively.
    assert_eq!(find("ἀγάπη", opts(true, true, true), greek), ["ἀγάπη"]);
    assert!(find("ἈΓΆΠΗ", opts(true, false, true), greek).is_empty());
    // English text is unaffected.
    assert_eq!(find("agape", d, "agape feast"), ["agape"]);
}

#[test]
fn nothing_to_find() {
    assert!(Finder::new("", FindOptions::default()).is_none());
    assert!(Finder::new("   ", FindOptions::default()).is_none());
    assert!(find("love", FindOptions::default(), "").is_empty());
}

#[test]
fn matches_never_split_a_letter_from_its_marks() {
    // With diacritics respected, "α" is not the start of "ἀ" (α + breathing).
    let strict = opts(false, false, false);
    assert!(find("α", strict, "ἀ").is_empty());
    assert_eq!(find("α", strict, "ἀα"), ["α"]);
}
