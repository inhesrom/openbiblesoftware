//! Word Info's Lexicon gloss (#294): one lookup shared with the Word Study header, across every
//! Lexicon in Preferred order, extended and base Strong's numbers both ways, and an entry with
//! no gloss line still naming its Lexicon.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use obs_model::{Key, MorphemePart, Resource, ResourceType};
use obs_scripture::StrongsNumber;

use crate::Library;
use crate::testing::study;
use crate::word_info::{LexiconGloss, ShownWord, lexicon_gloss};
use crate::word_study::{RenderingIndexes, header};

fn n(text: &str) -> StrongsNumber {
    text.parse().unwrap()
}

struct Temp(PathBuf);
impl Temp {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("obs-word-info-{name}-{}", std::process::id()));
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

/// A Lexicon of `entries` (Strong's number, headword line, paragraphs), as the Starter
/// converters write them: the headword line first, then the gloss paragraph.
fn lexicon(id: &str, abbreviation: &str, entries: &[(&str, &str, &[&str])]) -> Resource {
    let entries: Vec<study::Entry> = entries
        .iter()
        .map(|(number, headword, paragraphs)| {
            (
                Key::StrongsNumber(n(number)),
                headword.to_string(),
                paragraphs.iter().map(|p| p.to_string()).collect(),
            )
        })
        .collect();
    study::resource(
        id,
        abbreviation,
        abbreviation,
        ResourceType::Lexicon,
        &entries,
    )
}

/// A library of `lexicons` written as `.obr` files, in that Preferred order.
fn library(t: &Temp, lexicons: &[Resource]) -> Library {
    let mut library = Library::new();
    for r in lexicons {
        let path = t.0.join(format!("{}.obr", r.manifest.resource_id));
        obs_store::write_obr(r, &path).unwrap();
        library.add_file(&path).unwrap();
    }
    library.set_preferred_order(
        lexicons
            .iter()
            .map(|r| r.manifest.resource_id.clone())
            .collect(),
    );
    library
}

fn greek() -> Resource {
    lexicon("grk", "GRK", &[("G3056", "λόγος logos", &["word"])])
}

fn hebrew() -> Resource {
    lexicon(
        "heb",
        "HEB",
        &[("H7225", "רֵאשִׁית re.shit", &["first: beginning"])],
    )
}

/// What Word Info's card and the Word Study header both show for `number`.
fn shown(library: &Library, number: &str) -> Option<LexiconGloss> {
    let gloss = lexicon_gloss(library, n(number));
    let study = header(library, &RenderingIndexes::default(), n(number)).gloss;
    assert_eq!(
        gloss, study,
        "Word Info and Word Study disagree on {number}"
    );
    gloss
}

/// The reported case: a Greek ★ Lexicon has no H-numbers, so Word Info read "No Lexicon
/// entry" for H7225 while Word Study found the Hebrew Lexicon further down.
#[test]
fn a_greek_star_lexicon_and_a_hebrew_lexicon_give_a_gloss_for_an_h_number() {
    let t = Temp::new("greek-star");
    let library = library(&t, &[greek(), hebrew()]);
    let star = library.preferred(ResourceType::Lexicon).unwrap();
    assert_eq!(star.manifest.resource_id, "grk");
    let gloss = shown(&library, "H7225").expect("the Hebrew Lexicon's entry");
    assert_eq!(gloss.abbreviation, "HEB");
    assert_eq!(gloss.text.as_deref(), Some("first: beginning"));
    assert_eq!(gloss.lemma.as_deref(), Some("רֵאשִׁית re.shit"));
    // And a Greek word under a Hebrew ★ Lexicon (the Starter default, TBESH).
    let t = Temp::new("hebrew-star");
    let library = self::library(&t, &[hebrew(), greek()]);
    let gloss = shown(&library, "G3056").expect("the Greek Lexicon's entry");
    assert_eq!(
        (gloss.abbreviation.as_str(), gloss.text.as_deref()),
        ("GRK", Some("word"))
    );
}

/// A Bible tagging the base number (NASB1995, KJV: H7225) finds an entry keyed only by the
/// extended number (H7225G), and a Bible tagging the extended number (TAHOT: H1254A) finds an
/// entry keyed only by the base number (H1254).
#[test]
fn a_base_number_finds_an_entry_keyed_by_its_extended_number_and_the_reverse() {
    let t = Temp::new("extended");
    let extended_only = lexicon(
        "ext",
        "EXT",
        &[("H7225G", "רֵאשִׁית re.shit", &["first: beginning"])],
    );
    let base_only = lexicon("base", "BASE", &[("H1254", "בָּרָא bara", &["to create"])]);
    let library = library(&t, &[extended_only, base_only]);
    let gloss = shown(&library, "H7225").expect("H7225 finds H7225G");
    assert_eq!(
        (gloss.abbreviation.as_str(), gloss.text.as_deref()),
        ("EXT", Some("first: beginning"))
    );
    let gloss = shown(&library, "H1254A").expect("H1254A finds H1254");
    assert_eq!(
        (gloss.abbreviation.as_str(), gloss.text.as_deref()),
        ("BASE", Some("to create"))
    );
    assert_eq!(shown(&library, "H430"), None, "no Lexicon has H430");
}

/// An entry that is only a headword and a cross-reference (BDB's `אָב v. II. אבה.`) has no
/// gloss to cut. A later Lexicon's gloss is shown instead; with none, the card still names
/// the Lexicon (the UI offers Open in …) rather than saying there is no entry.
#[test]
fn an_entry_with_no_gloss_line_still_names_its_lexicon() {
    let t = Temp::new("no-gloss");
    let bdb = lexicon("bdbt", "BDBT", &[("H1", "אָב v. II. אבה.", &[])]);
    let with_gloss = lexicon("tbt", "TBT", &[("H1", "אָב av", &["father"])]);
    let library = library(&t, &[bdb.clone(), with_gloss]);
    let gloss = shown(&library, "H1").unwrap();
    assert_eq!(
        (gloss.abbreviation.as_str(), gloss.text.as_deref()),
        ("TBT", Some("father")),
        "the first gloss in Preferred order, past an entry without one"
    );

    let t = Temp::new("no-gloss-only");
    let library = self::library(&t, &[bdb]);
    let gloss = shown(&library, "H1").expect("the entry, without a gloss");
    assert_eq!(gloss.abbreviation, "BDBT");
    assert_eq!(gloss.resource_id, "bdbt");
    assert_eq!(gloss.text, None);
    let part = MorphemePart {
        strongs: n("H1"),
        lemma: None,
        morph: None,
        gloss: None,
    };
    let word = ShownWord::new("father", false, "Genesis 2:24 (KJV)".into(), &[part], |n| {
        lexicon_gloss(&library, n)
    })
    .unwrap();
    assert_eq!(word.main.lexicon.as_ref(), Some(&gloss));
    assert_eq!(word.main.line(), "H1 אָב v. II. אבה. · BDBT");
}

/// #294 review: a Lexicon's homographs (H7225 → H7225G, H7225H) are all read before the next
/// Lexicon: a cross-reference first does not hide the gloss of the entry after it.
#[test]
fn a_glossless_homograph_does_not_hide_the_next_homographs_gloss() {
    let t = Temp::new("homographs");
    let bdb = lexicon(
        "bdbt",
        "BDBT",
        &[
            ("H7225G", "רֵאשִׁית v. ראשׁ.", &[]),
            ("H7225H", "רֵאשִׁית re.shit", &["first, beginning"]),
        ],
    );
    let later = lexicon("tbt", "TBT", &[("H7225", "רֵאשִׁית", &["start"])]);
    let library = library(&t, &[bdb.clone(), later]);
    let gloss = shown(&library, "H7225").unwrap();
    assert_eq!(
        (
            gloss.abbreviation.as_str(),
            gloss.text.as_deref(),
            gloss.lemma.as_deref()
        ),
        ("BDBT", Some("first, beginning"), Some("רֵאשִׁית re.shit"))
    );

    // With every homograph glossless, the first one is kept over a later Lexicon's absence.
    let t = Temp::new("homographs-glossless");
    let glossless = lexicon(
        "bdbt",
        "BDBT",
        &[
            ("H7225G", "רֵאשִׁית v. ראשׁ.", &[]),
            ("H7225H", "רֵאשִׁית v. ראשׁ.", &[]),
        ],
    );
    let library = self::library(&t, &[glossless]);
    let gloss = shown(&library, "H7225").unwrap();
    assert_eq!(
        (gloss.abbreviation.as_str(), gloss.text.as_deref()),
        ("BDBT", None)
    );
}
