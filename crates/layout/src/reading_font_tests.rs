//! Reading fonts per script (#138): the chosen face draws its script, other scripts keep
//! theirs, and characters a chosen face lacks fall back to the bundled face.

use std::sync::Arc;

use obs_fonts::{FaceId, FaceSet, ShapeCache, face, load_user_face};
use obs_model::{Direction, TextUnit};

use super::*;

const MIXED: &str = "In the beginning ἐν ἀρχῇ בְּרֵאשִׁ֖ית בָּרָ֣א";

/// A bundled font file loaded as a font from this computer: same glyphs, a new face.
fn user(id: FaceId) -> FaceId {
    let bytes = id.bundled_bytes().unwrap().to_vec();
    load_user_face(Arc::new(bytes), 0).unwrap()
}

fn runs_by_text(para: &Paragraph) -> Vec<(&str, FaceId)> {
    para.runs
        .iter()
        .map(|r| (r.text.trim(), r.style.face))
        .collect()
}

fn missing_glyphs(block: &LaidBlock) -> usize {
    block
        .lines
        .iter()
        .flat_map(|l| &l.items)
        .map(|i| i.shaped.missing_glyphs())
        .sum()
}

#[test]
fn a_latin_font_changes_latin_text_only() {
    let serif = user(FaceId::Bold);
    let mut settings = TextSettings::default();
    let before = settings.key();
    settings.fonts.latin = FaceSet::single(serif);
    assert_ne!(settings.key(), before, "a font change lays text out again");
    let para = snippet_paragraph(MIXED, ColorRole::Ink, false, &settings);
    assert_eq!(
        runs_by_text(&para),
        [
            ("In the beginning", serif),
            ("ἐν ἀρχῇ", FaceId::Regular),
            ("בְּרֵאשִׁ֖ית בָּרָ֣א", FaceId::Hebrew),
        ]
    );
    // With the default fonts, Latin and Greek share Gentium Plus and one run.
    let para = snippet_paragraph(MIXED, ColorRole::Ink, false, &TextSettings::default());
    assert_eq!(
        runs_by_text(&para),
        [
            ("In the beginning ἐν ἀρχῇ", FaceId::Regular),
            ("בְּרֵאשִׁ֖ית בָּרָ֣א", FaceId::Hebrew),
        ]
    );
}

#[test]
fn a_hebrew_font_without_hebrew_falls_back_to_ezra() {
    let no_hebrew = user(FaceId::Regular);
    let mut settings = TextSettings::default();
    settings.fonts.hebrew = FaceSet::single(no_hebrew);
    let para = snippet_paragraph(MIXED, ColorRole::Ink, false, &settings);
    let hebrew: Vec<FaceId> = para
        .runs
        .iter()
        .filter(|r| r.text.contains('ב'))
        .map(|r| r.style.face)
        .collect();
    assert_eq!(hebrew, [FaceId::Hebrew]);
    let block = layout(&para, 2000.0, ShapeCache::shared());
    assert_eq!(missing_glyphs(&block), 0);
}

#[test]
fn characters_a_font_lacks_fall_back_per_cluster() {
    // Ezra SIL has ASCII letters but not ā: only that word's cluster falls back.
    let ezra = user(FaceId::Hebrew);
    assert!(face(ezra).unwrap().covers('a') && !face(ezra).unwrap().covers('ā'));
    let mut settings = TextSettings::default();
    settings.fonts.latin = FaceSet::single(ezra);
    let para = snippet_paragraph("the Adām of", ColorRole::Ink, false, &settings);
    assert_eq!(
        runs_by_text(&para),
        [("the Ad", ezra), ("ā", FaceId::Regular), ("m of", ezra)]
    );
    let block = layout(&para, 2000.0, ShapeCache::shared());
    assert_eq!(missing_glyphs(&block), 0);
    // Byte positions still map to the text.
    let starts: Vec<RunSource> = para.runs.iter().map(|r| r.source.clone()).collect();
    assert_eq!(
        starts,
        [0, 6, 8].map(|unit_start| RunSource::Text { unit_start })
    );
}

/// A maqaf or sof pasuq on its own (between Tagged words, so in a piece of its own) is drawn
/// in the Hebrew face: Gentium Plus has no glyph for it.
#[test]
fn hebrew_punctuation_alone_is_drawn_in_the_hebrew_font() {
    for alone in ["\u{05BE}", "\u{05C3} ", " \u{05C0} "] {
        let para = snippet_paragraph(alone, ColorRole::Ink, true, &TextSettings::default());
        assert_eq!(runs_by_text(&para), [(alone.trim(), FaceId::Hebrew)]);
        let block = layout(&para, 200.0, ShapeCache::shared());
        assert_eq!(missing_glyphs(&block), 0, "{alone:?}");
    }
}

#[test]
fn small_caps_are_synthetic_when_the_font_has_no_smcp() {
    let unit = english_unit_with_lord();
    let block = &unit_blocks(&unit)[0];
    let ezra = user(FaceId::Hebrew);
    assert!(!face(ezra).unwrap().has_small_caps());
    let mut settings = TextSettings::default();
    settings.fonts.latin = FaceSet::single(ezra);
    let text_runs = |para: Paragraph| -> Vec<Run> {
        para.runs
            .into_iter()
            .filter(|r| matches!(r.source, RunSource::Text { .. }))
            .collect()
    };
    let runs = text_runs(block_paragraph(&unit, block, Direction::Ltr, &settings));
    let body = settings.size;
    let lord: Vec<(&str, f32, bool)> = runs
        .iter()
        .take(2)
        .map(|r| (r.text.as_str(), r.style.size, r.style.small_caps))
        .collect();
    assert_eq!(
        lord,
        [
            ("L", body, false),
            ("ORD", body * SYNTHETIC_SMALL_CAPS, false)
        ]
    );
    assert_eq!(
        runs[1].source,
        RunSource::Text { unit_start: 1 },
        "offsets kept"
    );
    // A font with smcp keeps real small capitals.
    let gentium = user(FaceId::Regular);
    settings.fonts.latin = FaceSet::single(gentium);
    let runs = text_runs(block_paragraph(&unit, block, Direction::Ltr, &settings));
    assert_eq!(runs[0].text, "Lord");
    assert!(runs[0].style.small_caps);
    assert_eq!(runs[0].style.face, gentium);
}

#[test]
fn verse_numbers_follow_the_latin_font() {
    let serif = user(FaceId::Italic);
    let mut settings = TextSettings::default();
    settings.fonts.latin = FaceSet {
        bold: serif,
        ..FaceSet::bundled(obs_fonts::ReadingScript::Latin)
    };
    let unit = english_unit_with_lord();
    let block = &unit_blocks(&unit)[0];
    let mut runs = block_paragraph(&unit, block, Direction::Ltr, &settings).runs;
    runs.retain(|r| {
        matches!(
            r.source,
            RunSource::Object {
                object: InlineObject::VerseNumber(_),
                ..
            }
        )
    });
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].style.face, serif);
}

/// "LORD is my shepherd" as Ps 23:2 (a verse number that is not a chapter number).
fn english_unit_with_lord() -> TextUnit {
    use obs_model::{SpanKind, UnitId, UnitKind};
    use obs_scripture::{Book, Verse};
    let text = "LORD is my shepherd".to_string();
    let spans = vec![
        obs_model::Span {
            start: 0,
            end: text.len() as u32,
            kind: SpanKind::Verse {
                verse: Verse::new(Book::Psalms, 23, 2),
                last: 2,
            },
        },
        obs_model::Span {
            start: 0,
            end: 4,
            kind: SpanKind::DivineName,
        },
    ];
    TextUnit {
        id: UnitId::book(Book::Psalms),
        kind: UnitKind::BibleBook,
        title: None,
        text,
        spans,
    }
}
