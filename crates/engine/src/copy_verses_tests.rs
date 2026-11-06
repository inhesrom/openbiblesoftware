//! Copy verses (#154): Jn 3:16-18 in the KJV and the BSB with a saved Copy preset as
//! Markdown, rich text and plain text; the Attribution appended for a licence that asks for
//! it; Bibles numbered differently; Copy presets kept in the User database.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use obs_model::{Manifest, ResourceType};
use obs_scripture::{
    Book, ListedVersification, Passage, PassageList, Standard, Verse, VerseMap, VerseMapEntry,
    Versification, parse_reference,
};
use serde_json::json;

use crate::*;

const KJV: [&str; 3] = [
    "¶ For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.",
    "For God sent not his Son into the world to condemn the world; but that the world through him might be saved.",
    "¶ He that believeth on him is not condemned: but he that believeth not is condemned already, because he hath not believed in the name of the only begotten Son of God.",
];

const BSB: [&str; 3] = [
    "For God so loved the world that He gave His one and only Son, that everyone who believes in Him shall not perish but have eternal life.",
    "For God did not send His Son into the world to condemn the world, but to save the world through Him.",
    "Whoever believes in Him is not condemned, but whoever does not believe has already been condemned, because he has not believed in the name of God’s one and only Son.",
];

/// A Bible in memory: Verses with their text, the Verses that start a paragraph, and maybe a
/// Verse map.
struct TestBible {
    manifest: Manifest,
    verses: Vec<(Verse, String)>,
    paragraphs: Vec<Verse>,
    map: Option<VerseMap>,
    versification: ListedVersification,
}

impl TestBible {
    fn new(abbrev: &str, verses: Vec<(Verse, String)>, paragraphs: Vec<Verse>) -> Self {
        let mut manifest = Manifest::new(
            abbrev.to_lowercase(),
            abbrev,
            abbrev,
            ResourceType::Bible,
            "en",
        );
        manifest.license = "LicenseRef-PublicDomain".into();
        manifest.attribution = format!("{abbrev}: public domain.");
        let versification = ListedVersification::new(verses.iter().map(|(v, _)| (*v, v.verse)));
        Self {
            manifest,
            verses,
            paragraphs,
            map: None,
            versification,
        }
    }
}

impl ComparedBible for TestBible {
    fn language(&self) -> &str {
        &self.manifest.language
    }
    fn verse_map(&self) -> Option<&VerseMap> {
        self.map.as_ref()
    }
    fn verse_at(&self, native: Verse) -> Option<(Verse, u16)> {
        self.verses
            .iter()
            .find(|(v, _)| *v == native)
            .map(|(v, _)| (*v, v.verse))
    }
    fn verse_text(&self, native: Verse) -> Option<String> {
        self.verses
            .iter()
            .find(|(v, _)| *v == native)
            .map(|(_, t)| t.clone())
    }
}

impl CopiedBible for TestBible {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    fn versification(&self) -> &dyn Versification {
        &self.versification
    }
    fn starts_paragraph(&self, native: Verse) -> bool {
        self.paragraphs.contains(&native)
    }
}

fn jn(v: u16) -> Verse {
    Verse::new(Book::John, 3, v)
}

/// John 3:15-19, with `text` for 16-18.
fn john(abbrev: &str, text: [&str; 3], paragraphs: Vec<Verse>) -> TestBible {
    let mut verses = vec![(jn(15), "Verse fifteen.".to_string())];
    verses.extend((16..=18).zip(text).map(|(n, t)| (jn(n), t.to_string())));
    verses.push((jn(19), "Verse nineteen.".to_string()));
    TestBible::new(abbrev, verses, paragraphs)
}

/// The KJV marks paragraphs (¶) at 3:16 and 3:18; the BSB starts one at 3:16.
fn kjv_and_bsb() -> (TestBible, TestBible) {
    (
        john("KJV", KJV, vec![jn(16), jn(18)]),
        john("BSB", BSB, vec![jn(16)]),
    )
}

fn passages(text: &str) -> PassageList {
    parse_reference(text, &Standard, None).unwrap()
}

fn user_session(name: &str) -> (Session, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("obs-copy-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("user.db");
    let mut s = Session::new(JobMode::Inline);
    let (store, _) = obs_store::UserStore::open(&db).unwrap();
    s.attach_user(Box::new(store));
    (s, dir)
}

/// The "Handout" preset: reference before the text, verse numbers, paragraphs, quotation
/// marks and full book names.
fn save_handout(s: &mut Session) {
    for (name, value) in [
        ("reference", "before"),
        ("verse_numbers", "on"),
        ("layout", "paragraphs"),
        ("quotation_marks", "on"),
        ("book_names", "full"),
    ] {
        s.run("copy.option", &json!({ "name": name, "value": value }))
            .unwrap();
    }
    let out = s
        .run("copy.preset_save", &json!({ "name": "Handout" }))
        .unwrap();
    assert_eq!(out["preset"], "Handout");
}

#[test]
fn copying_jn_3_16_18_in_kjv_and_bsb_with_a_preset_gives_the_expected_markdown_and_rich_text() {
    let (mut s, dir) = user_session("preset");
    save_handout(&mut s);
    // Something else is in use until the preset is chosen again.
    s.run(
        "copy.option",
        &json!({ "name": "layout", "value": "verse_per_line" }),
    )
    .unwrap();
    assert!(s.copy_verses.current_preset().is_none());
    s.run("copy.preset_apply", &json!({ "name": "Handout" }))
        .unwrap();

    let (kjv, bsb) = kjv_and_bsb();
    let bibles: [&dyn CopiedBible; 2] = [&kjv, &bsb];
    let copied = copy_verses(&passages("Jn 3:16-18"), &bibles, s.copy_verses.options);

    let markdown = "\
John 3:16-18 (KJV)

“**16** ¶ For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life. **17** For God sent not his Son into the world to condemn the world; but that the world through him might be saved.

**18** ¶ He that believeth on him is not condemned: but he that believeth not is condemned already, because he hath not believed in the name of the only begotten Son of God.”

John 3:16-18 (BSB)

“**16** For God so loved the world that He gave His one and only Son, that everyone who believes in Him shall not perish but have eternal life. **17** For God did not send His Son into the world to condemn the world, but to save the world through Him. **18** Whoever believes in Him is not condemned, but whoever does not believe has already been condemned, because he has not believed in the name of God’s one and only Son.”";
    assert_eq!(copied.markdown(), markdown);

    let html = "\
<p dir=\"auto\">John 3:16-18 (KJV)</p>
<p dir=\"auto\">“<sup>16</sup>\u{a0}¶ For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life. <sup>17</sup>\u{a0}For God sent not his Son into the world to condemn the world; but that the world through him might be saved.</p>
<p dir=\"auto\"><sup>18</sup>\u{a0}¶ He that believeth on him is not condemned: but he that believeth not is condemned already, because he hath not believed in the name of the only begotten Son of God.”</p>
<p dir=\"auto\">John 3:16-18 (BSB)</p>
<p dir=\"auto\">“<sup>16</sup>\u{a0}For God so loved the world that He gave His one and only Son, that everyone who believes in Him shall not perish but have eternal life. <sup>17</sup>\u{a0}For God did not send His Son into the world to condemn the world, but to save the world through Him. <sup>18</sup>\u{a0}Whoever believes in Him is not condemned, but whoever does not believe has already been condemned, because he has not believed in the name of God’s one and only Son.”</p>";
    assert_eq!(copied.html(), html);
    assert_eq!(copied.render(CopyFormat::RichText), html);

    // The same copy as plain text.
    let plain = copied.plain_text();
    assert!(plain.starts_with("John 3:16-18 (KJV)\n“16 ¶ For God so loved"));
    assert!(plain.contains("might be saved.\n18 ¶ He that believeth"));
    assert!(plain.ends_with("God’s one and only Son.”"));
    // Public-domain Bibles append no Attribution.
    assert!(copied.attributions.is_empty());

    // The preset survives a restart.
    drop(s);
    let mut again = Session::new(JobMode::Inline);
    let (store, _) = obs_store::UserStore::open(&dir.join("user.db")).unwrap();
    again.attach_user(Box::new(store));
    assert_eq!(
        again.copy_verses.current_preset().map(|p| p.name.as_str()),
        Some("Handout")
    );
    drop(again);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn every_option_changes_the_copy() {
    let (kjv, bsb) = kjv_and_bsb();
    let bibles: [&dyn CopiedBible; 2] = [&kjv, &bsb];
    let list = passages("Jn 3:16-17");
    let copy = |o: CopyOptions| copy_verses(&list, &bibles[..1], o);
    let options = CopyOptions::default();
    // Defaults: reference after, numbers, paragraphs, no quotation marks, full names.
    assert_eq!(
        copy(options).plain_text(),
        format!("16 {} 17 {}\nJohn 3:16-17 (KJV)", KJV[0], KJV[1])
    );
    let lines = CopyOptions {
        layout: VerseLayout::VersePerLine,
        verse_numbers: false,
        book_names: BookNames::Abbreviated,
        ..options
    };
    assert_eq!(
        copy(lines).plain_text(),
        format!("{}\n{}\nJohn 3:16-17 (KJV)", KJV[0], KJV[1])
    );
    assert_eq!(
        copy(lines).markdown(),
        format!("{}\\\n{}\n\nJohn 3:16-17 (KJV)", KJV[0], KJV[1])
    );
    assert_eq!(
        copy(lines).html(),
        format!(
            "<p dir=\"auto\">{}<br>\n{}</p>\n<p dir=\"auto\">John 3:16-17 (KJV)</p>",
            KJV[0], KJV[1]
        )
    );
    // Abbreviated book names are the SBL abbreviations.
    let rom = TestBible::new(
        "KJV",
        vec![(Verse::new(Book::Romans, 8, 28), "And we know.".into())],
        Vec::new(),
    );
    let abbreviated = copy_verses(&passages("Rom 8:28"), &[&rom as &dyn CopiedBible], lines);
    assert_eq!(abbreviated.plain_text(), "And we know.\nRom 8:28 (KJV)");
    // A Passage list copies each Passage in each Bible, Passage by Passage.
    let two = copy_verses(&passages("Jn 3:16; 3:18"), &bibles, options);
    let refs: Vec<&str> = two.blocks.iter().map(|b| b.reference.as_str()).collect();
    assert_eq!(
        refs,
        [
            "John 3:16 (KJV)",
            "John 3:16 (BSB)",
            "John 3:18 (KJV)",
            "John 3:18 (BSB)"
        ]
    );
}

#[test]
fn text_is_escaped_for_markdown_and_html() {
    let bible = TestBible::new(
        "TB",
        vec![(jn(16), "The *word* <of> [God] & #1_ok".into())],
        Vec::new(),
    );
    let copied = copy_verses(
        &passages("Jn 3:16"),
        &[&bible as &dyn CopiedBible],
        CopyOptions::default(),
    );
    assert_eq!(
        copied.markdown(),
        "**16** The \\*word\\* \\<of\\> \\[God\\] & \\#1\\_ok\n\nJohn 3:16 (TB)"
    );
    assert_eq!(
        copied.html(),
        "<p dir=\"auto\"><sup>16</sup>\u{a0}The *word* &lt;of&gt; [God] &amp; #1_ok</p>\n<p dir=\"auto\">John 3:16 (TB)</p>"
    );
}

/// The `<p …>` paragraphs of an HTML document: (attributes, text with tags left out).
fn html_paragraphs(html: &str) -> Vec<(String, String)> {
    html.split("<p")
        .skip(1)
        .filter(|rest| rest.starts_with(['>', ' ']))
        .map(|rest| {
            let (attributes, rest) = rest.split_once('>').unwrap();
            let inner = rest.split("</p>").next().unwrap();
            let mut text = String::new();
            let mut in_tag = false;
            for c in inner.chars() {
                match c {
                    '<' => in_tag = true,
                    '>' => in_tag = false,
                    _ if !in_tag => text.push(c),
                    _ => {}
                }
            }
            (attributes.trim().to_string(), text)
        })
        .collect()
}

/// The direction a browser gives a `dir="auto"` paragraph: that of its first strong
/// character (verse numbers, punctuation and spaces are not), left to right if none.
fn auto_direction(text: &str) -> &'static str {
    let hebrew =
        |c: char| ('\u{0590}'..='\u{05FF}').contains(&c) || ('\u{FB1D}'..='\u{FB4F}').contains(&c);
    text.chars()
        .find(|c| hebrew(*c) || c.is_alphabetic())
        .map_or("ltr", |c| if hebrew(c) { "rtl" } else { "ltr" })
}

#[test]
fn a_hebrew_bibles_paragraphs_read_right_to_left_in_rich_text_and_print_pages() {
    let genesis = |v: u16| Verse::new(Book::Genesis, 1, v);
    let mut bible = TestBible::new(
        "HEB",
        vec![
            (genesis(1), "בְּרֵאשִׁית בָּרָא אֱלֹהִים אֵת הַשָּׁמַיִם וְאֵת הָאָֽרֶץ׃".into()),
            (genesis(2), "וְהָאָרֶץ הָיְתָה תֹהוּ וָבֹהוּ".into()),
            (genesis(3), "וַיֹּאמֶר אֱלֹהִים יְהִי אוֹר".into()),
        ],
        vec![genesis(1), genesis(2)],
    );
    bible.manifest.language = "hbo".into();
    let options = CopyOptions {
        quotation_marks: true,
        ..CopyOptions::default()
    };
    let copied = copy_verses(
        &passages("Gen 1:1-2"),
        &[&bible as &dyn CopiedBible],
        options,
    );
    let html = copied.html();
    let page = html_document("Genesis 1:1-2 (HEB)", &html, true);
    for doc in [&html, &page] {
        let paragraphs = html_paragraphs(doc);
        assert_eq!(paragraphs.len(), 3, "{doc}");
        for (attributes, text) in &paragraphs {
            assert_eq!(attributes, "dir=\"auto\"", "{doc}");
            // Each verse paragraph, which starts with its verse number (and a quotation
            // mark), reads right to left; the English reference left to right.
            let expected = if text == "Genesis 1:1-2 (HEB)" {
                "ltr"
            } else {
                "rtl"
            };
            assert_eq!(auto_direction(text), expected, "{text}");
        }
    }
    assert!(
        html.starts_with("<p dir=\"auto\">“<sup>1</sup>\u{a0}בְּרֵאשִׁית"),
        "{html}"
    );
}

#[test]
fn markdown_keeps_character_references_and_line_start_markers_as_written() {
    let bible = TestBible::new(
        "TB",
        vec![
            (jn(16), "- item &amp; &#38; AT&T R&D".into()),
            (jn(17), "1. first + second".into()),
            (jn(18), "=== 2) +plus".into()),
        ],
        Vec::new(),
    );
    let bibles = [&bible as &dyn CopiedBible];
    let mut options = CopyOptions {
        verse_numbers: false,
        quotation_marks: false,
        layout: VerseLayout::VersePerLine,
        ..CopyOptions::default()
    };
    assert_eq!(
        copy_verses(&passages("Jn 3:16-18"), &bibles, options).markdown(),
        "\\- item \\&amp; \\&\\#38; AT\\&T R\\&D\\\n\
         1\\. first + second\\\n\
         \\=== 2) +plus\n\n\
         John 3:16-18 (TB)"
    );
    options.layout = VerseLayout::Paragraphs;
    options.reference = ReferencePlacement::Before;
    assert_eq!(
        copy_verses(&passages("Jn 3:16"), &bibles, options).markdown(),
        "John 3:16 (TB)\n\n\\- item \\&amp; \\&\\#38; AT\\&T R\\&D"
    );
    // Behind a verse number or a quotation mark nothing starts a block, and nothing changes.
    options.verse_numbers = true;
    assert_eq!(
        copy_verses(&passages("Jn 3:16"), &bibles, options).markdown(),
        "John 3:16 (TB)\n\n**16** - item \\&amp; \\&\\#38; AT\\&T R\\&D"
    );
}

#[test]
fn a_verse_map_that_swaps_verses_copies_them_in_the_bibles_order_with_its_reference() {
    // A Greek-numbered Philippians: Standard Phil 1:16 is its 1:17 and 1:17 its 1:16.
    let php = |v| Verse::new(Book::Philippians, 1, v);
    let verses: Vec<(Verse, String)> = (15..=18)
        .map(|n| (php(n), format!("Greek-numbered 1:{n}.")))
        .collect();
    let mut bible = TestBible::new("GRK", verses, Vec::new());
    bible.map = Some(VerseMap::new(
        (15..=18)
            .map(|n| VerseMapEntry {
                verse: php(n),
                last: n,
                standard: Some(Passage::verse(php(match n {
                    16 => 17,
                    17 => 16,
                    n => n,
                }))),
            })
            .collect(),
        Vec::new(),
        &Standard,
    ));
    let copied = copy_verses(
        &passages("Phil 1:16-17"),
        &[&bible as &dyn CopiedBible],
        CopyOptions::default(),
    );
    assert_eq!(
        copied.plain_text(),
        "16 Greek-numbered 1:16. 17 Greek-numbered 1:17.\nPhilippians 1:16-17 (GRK)"
    );
    let one = copy_verses(
        &passages("Phil 1:16"),
        &[&bible as &dyn CopiedBible],
        CopyOptions::default(),
    );
    assert_eq!(
        one.plain_text(),
        "17 Greek-numbered 1:17.\nPhilippians 1:17 (GRK)"
    );
}

#[test]
fn the_attribution_is_appended_when_a_resources_licence_requires_it() {
    let (kjv, _) = kjv_and_bsb();
    let mut tagnt = john("TAGNT", BSB, Vec::new());
    tagnt.manifest.license = "CC-BY-4.0".into();
    tagnt.manifest.attribution =
        "Translators Amalgamated Greek NT (TAGNT), data created by www.STEPBible.org, CC BY 4.0."
            .into();
    let bibles: [&dyn CopiedBible; 2] = [&kjv, &tagnt];
    let copied = copy_verses(&passages("Jn 3:16"), &bibles, CopyOptions::default());
    assert_eq!(copied.attributions, [tagnt.manifest.attribution.clone()]);
    let attribution =
        "Translators Amalgamated Greek NT (TAGNT), data created by www.STEPBible.org, CC BY 4.0.";
    assert!(
        copied
            .plain_text()
            .ends_with(&format!("John 3:16 (TAGNT)\n\n{attribution}"))
    );
    assert!(
        copied
            .markdown()
            .ends_with(&format!("John 3:16 (TAGNT)\n\n{attribution}"))
    );
    assert!(
        copied
            .html()
            .ends_with(&format!("<p><small>{attribution}</small></p>"))
    );
    // Only for Bibles that were copied: a Passage the CC BY Bible lacks appends nothing.
    let none = copy_verses(
        &passages("Gen 1:1; Jn 3:16"),
        &bibles[..1],
        CopyOptions::default(),
    );
    assert!(none.attributions.is_empty());
    let absent = copy_verses(&passages("Gen 1:1"), &bibles[1..], CopyOptions::default());
    assert!(absent.blocks.is_empty() && absent.attributions.is_empty());
}

#[test]
fn a_bible_numbered_differently_copies_its_own_verses_and_reference() {
    // Hebrew-numbered Malachi: English Mal 4:1-6 is Mal 3:19-24.
    let mal = |c, v| Verse::new(Book::Malachi, c, v);
    let verses: Vec<(Verse, String)> = (19..=24)
        .map(|n| (mal(3, n), format!("Hebrew-numbered 3:{n}.")))
        .collect();
    let mut bible = TestBible::new("HNUM", verses, Vec::new());
    bible.map = Some(VerseMap::new(
        (19..=24)
            .map(|n| VerseMapEntry {
                verse: mal(3, n),
                last: n,
                standard: Some(Passage::verse(mal(4, n - 18))),
            })
            .collect(),
        Vec::new(),
        &Standard,
    ));
    let copied = copy_verses(
        &passages("Mal 4:1-2"),
        &[&bible as &dyn CopiedBible],
        CopyOptions::default(),
    );
    assert_eq!(
        copied.plain_text(),
        "19 Hebrew-numbered 3:19. 20 Hebrew-numbered 3:20.\nMalachi 3:19-20 (HNUM)"
    );
}

#[test]
fn presets_are_saved_replaced_and_deleted() {
    let (mut s, dir) = user_session("presets");
    save_handout(&mut s);
    s.run(
        "copy.option",
        &json!({ "name": "quotation_marks", "value": "off" }),
    )
    .unwrap();
    s.run("copy.preset_save", &json!({ "name": "Plain" }))
        .unwrap();
    // Saving under a name in use replaces that preset.
    s.run(
        "copy.option",
        &json!({ "name": "book_names", "value": "abbreviated" }),
    )
    .unwrap();
    let out = s
        .run("copy.preset_save", &json!({ "name": "Plain" }))
        .unwrap();
    assert_eq!(out["presets"], json!(["Handout", "Plain"]));
    assert_eq!(
        s.copy_verses.presets[1].options.book_names,
        BookNames::Abbreviated
    );
    let out = s
        .run("copy.preset_delete", &json!({ "name": "Handout" }))
        .unwrap();
    assert_eq!(out["presets"], json!(["Plain"]));
    assert!(
        s.run("copy.preset_apply", &json!({ "name": "Handout" }))
            .is_err()
    );
    assert!(
        s.run(
            "copy.option",
            &json!({ "name": "layout", "value": "columns" })
        )
        .is_err()
    );
    assert!(s.run("copy.preset_save", &json!({ "name": "  " })).is_err());
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tools_copy_verses_opens_the_window() {
    let mut s = Session::new(JobMode::Inline);
    s.run("tool.copy_verses", &json!({})).unwrap();
    assert!(
        s.workspace
            .take_effects()
            .contains(&Effect::ShowCopyVerses {
                passages: None,
                bibles: Vec::new()
            })
    );
    s.run(
        "tool.copy_verses",
        &json!({ "passages": "Jn 3:16-18", "bibles": "kjv-strongs, bsb" }),
    )
    .unwrap();
    assert!(
        s.workspace
            .take_effects()
            .contains(&Effect::ShowCopyVerses {
                passages: Some("Jn 3:16-18".into()),
                bibles: vec!["kjv-strongs".into(), "bsb".into()]
            })
    );
    let tools = MENUS.iter().find(|m| m.title == "Tools").unwrap();
    assert!(tools.items.contains(&MenuItem::Command("tool.copy_verses")));
}

#[test]
fn print_documents_are_standalone_pages_that_open_the_print_dialog() {
    let page = html_document("John 3:16 <KJV>", "<p>text</p>", true);
    assert!(page.starts_with("<!DOCTYPE html>"));
    assert!(page.contains("<title>John 3:16 &lt;KJV&gt;</title>"));
    assert!(page.contains("window.print()"));
    assert!(page.contains("<body>\n<p>text</p>\n</body>"));
    assert!(!html_document("t", "", false).contains("window.print()"));
}
