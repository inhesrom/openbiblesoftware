//! Language names (#244): every tag the live SWORD catalog uses gets a name, codes of one
//! language find each other, and unknown tags say so.

use crate::language_names::{english_name, language};

#[test]
fn bengali_is_named_in_english_and_in_bengali_and_found_by_either_name_or_code() {
    let bn = language("bn");
    assert_eq!(bn.label(), "Bangla — বাংলা");
    assert_eq!(bn.english_label(), "Bangla");
    for query in ["Bengali", "bengali", "Bangla", "ben", "bn", "BEN", "বাংলা"] {
        assert!(bn.matches(query), "{query:?} should find bn: {bn:?}");
    }
    assert!(
        !language("be").matches("ben"),
        "codes match whole, not as prefixes"
    );
    // A code finds its language before names that hold it.
    assert_eq!(bn.match_rank("ben"), Some(0));
    assert_eq!(bn.match_rank("beng"), Some(1));
    assert_eq!(language("bef").match_rank("ben"), Some(1), "Benabena");
    assert_eq!(language("bqc").match_rank("ben"), Some(1), "Boko (Benin)");
    assert_eq!(bn.match_rank("ngali"), Some(2));
    assert_eq!(bn.match_rank("klingon"), None);
    // The ISO 639-3 code names the same language.
    assert_eq!(language("ben").label(), "Bangla — বাংলা");
    assert_eq!(language("ben").primary, "bn");
    assert_eq!(language("bn-IN").label(), "Bangla (India) — বাংলা");
}

#[test]
fn codes_cldr_lacks_take_the_iso_639_3_reference_name() {
    let kmo = language("kmo");
    assert!(kmo.is_known());
    assert_eq!(kmo.label(), "Kwoma");
    assert!(kmo.matches("kmo") && kmo.matches("kwoma"));
    assert!(kmo.also.is_empty(), "its one name is not repeated: {kmo:?}");
    assert_eq!(language("bn").also, ["Bengali"]);
    assert_eq!(language("hbo").label(), "Ancient Hebrew");
}

#[test]
fn retired_iso_codes_read_as_their_replacement_or_keep_their_name() {
    // `daf` was split into `dnj` and `lda`; eBible.org still tags a Bible with it.
    assert_eq!(language("daf").label(), "Dan");
    // `fri` became `fry` (ISO 639-1 `fy`).
    assert_eq!(language("fri").label(), language("fy").label());
    assert_eq!(language("fri").primary, "fy");
}

#[test]
fn script_and_region_subtags_are_named() {
    assert_eq!(
        language("zh-Hant").label(),
        "Traditional Chinese — 繁體中文"
    );
    assert_eq!(
        language("zh-hant").label(),
        "Traditional Chinese — 繁體中文"
    );
    assert_eq!(language("zh_Hans").english_label(), "Simplified Chinese");
    assert_eq!(
        language("sr-Latn").label(),
        "Serbian (Latin) — srpski (latinica)"
    );
    assert_eq!(language("en-GB").english_label(), "British English");
    assert_eq!(language("es-419").english_label(), "Latin American Spanish");
    // Composed from the script's and the region's names when CLDR has no entry for both.
    assert_eq!(
        language("ar-Latn-EG").english_label(),
        "Arabic (Latin, Egypt)"
    );
    // A script changes the writing, so the language's own name is left out.
    assert_eq!(language("bn-Latn").label(), "Bangla (Latin)");
    // ISO 639-2/B and 639-3 codes reach CLDR's entries.
    assert_eq!(language("ger").english_label(), "German");
    assert_eq!(language("zho-Hant").english_label(), "Traditional Chinese");
}

#[test]
fn the_script_a_language_is_usually_written_in_names_nothing() {
    // STEP Bible tags its Bengali Bible `ben-Beng-IN`.
    let step = language("ben-Beng-IN");
    assert_eq!(step.label(), "Bangla (India) — বাংলা");
    assert_eq!(step.key, "bn-IN");
    assert_eq!(language("pl-Latn-PL").english_label(), "Polish (Poland)");
    assert_eq!(language("en-Latn-GB").english_label(), "British English");
    // A script CLDR names with the language is kept.
    assert_eq!(
        language("zh-Hans-CN").english_label(),
        "Simplified Chinese (China)"
    );
    assert_eq!(
        language("zh-Hant-HK").english_label(),
        "Traditional Chinese (Hong Kong SAR China)"
    );
    // Another script than the usual one is named.
    assert_eq!(
        language("sr-Latn-RS").english_label(),
        "Serbian (Latin, Serbia)"
    );
}

#[test]
fn tags_of_one_language_share_a_key() {
    for (a, b) in [
        ("bn", "ben"),
        ("en", "en-Latn"),
        ("EN", "en"),
        ("os", "oss-Cyrl"),
        ("kk", "kk-Cyrl"),
        ("zh_hant", "zh-Hant"),
    ] {
        assert_eq!(language(a).key, language(b).key, "{a} and {b}");
        assert_eq!(language(a).label(), language(b).label(), "{a} and {b}");
    }
    assert_ne!(language("zh").key, language("zh-Hans").key);
    assert_ne!(language("bn").key, language("bn-IN").key);
    // A key names the same language as its tags.
    for tag in ["ben-Beng-IN", "zh-Hans-CN", "qqq", "cop-sa", "en-Latn"] {
        let key = language(tag).key;
        assert_eq!(language(&key).key, key, "{tag}");
        assert_eq!(language(&key).label(), language(tag).label(), "{tag}");
    }
}

#[test]
fn sword_dialect_tags_are_not_read_as_regions() {
    assert_eq!(language("cop-sa").english_label(), "Coptic (Sahidic)");
    assert!(language("cop-sa").matches("coptic"));
}

#[test]
fn unknown_tags_say_unknown_language_with_the_tag() {
    let x = language("qqq-x-thing");
    assert!(!x.is_known());
    assert_eq!(x.label(), "Unknown language (qqq-x-thing)");
    assert!(x.matches("qqq-x-thing"));
    assert_eq!(language("").label(), "Unknown language ()");
    assert_eq!(language("und").label(), "Unknown language");
}

#[test]
fn the_same_name_in_both_languages_is_shown_once() {
    assert_eq!(language("en").label(), "English");
    assert_eq!(language("de").label(), "German — Deutsch");
}

/// Every `Lang=` of the SWORD repositories' catalogs on 2026-11-08
/// (docs/research/language-names.md): each has a name.
#[test]
fn every_language_tag_of_the_live_sword_catalog_has_a_name() {
    let tags = include_str!("../tests/assets/sword-catalog-languages.txt");
    let tags: Vec<&str> = tags
        .lines()
        .map(str::trim)
        .filter(|t| !t.is_empty() && !t.starts_with('#'))
        .collect();
    assert!(tags.len() > 1300, "{}", tags.len());
    let unknown: Vec<&&str> = tags.iter().filter(|t| !language(t).is_known()).collect();
    assert!(unknown.is_empty(), "no name for {unknown:?}");
}

#[test]
fn english_name_groups_a_languages_tags() {
    assert_eq!(english_name("kmo"), Some("Kwoma"));
    assert_eq!(english_name("zh-Hant"), Some("Chinese"));
    assert_eq!(english_name("ben"), Some("Bangla"));
    assert_eq!(english_name("qqq"), None);
}
