use std::sync::Arc;

use super::*;

const GEN_1_1_HEB: &str = "בְּרֵאשִׁ֖ית בָּרָ֣א אֱלֹהִ֑ים";
const JN_1_1_GRK: &str = "Ἐν ἀρχῇ ἦν ὁ λόγος, καὶ ὁ λόγος ἦν πρὸς τὸν θεόν";

fn style(face: FaceId, rtl: bool) -> ShapeStyle {
    ShapeStyle::new(face, 24.0, false, rtl)
}

#[test]
fn every_face_loads_with_sane_metrics() {
    for id in FaceId::ALL {
        let f = face(id).unwrap();
        assert!(f.metrics.units_per_em >= 16, "{id:?}");
        assert!(
            f.metrics.ascent > 0.0 && f.metrics.descent < 0.0,
            "{id:?} {:?}",
            f.metrics
        );
    }
}

#[test]
fn scripts_pick_faces() {
    assert_eq!(script_of(GEN_1_1_HEB), Script::Hebrew);
    assert_eq!(script_of(JN_1_1_GRK), Script::Greek);
    assert_eq!(script_of("3:16 For God"), Script::Latin);
    assert_eq!(script_of("3:16"), Script::Common);
    assert_eq!(
        FaceId::for_script(Script::Hebrew, true, true),
        FaceId::Hebrew
    );
    assert_eq!(
        FaceId::for_script(Script::Greek, false, true),
        FaceId::Italic
    );
    assert_eq!(FaceId::for_script(Script::Latin, true, false), FaceId::Bold);
}

#[test]
fn english_greek_and_hebrew_have_every_glyph() {
    for (text, face_id, rtl) in [
        (
            "In the beginning God created the heaven and the earth.",
            FaceId::Regular,
            false,
        ),
        (JN_1_1_GRK, FaceId::Regular, false),
        (JN_1_1_GRK, FaceId::Italic, false),
        (GEN_1_1_HEB, FaceId::Hebrew, true),
    ] {
        let run = shape_uncached(text, style(face_id, rtl)).unwrap();
        assert!(!run.glyphs.is_empty(), "{text}");
        assert_eq!(run.missing_glyphs(), 0, "{face_id:?}: {text}");
        assert!(run.width > 0.0);
    }
}

#[test]
fn hebrew_marks_are_positioned_on_their_letters() {
    let run = shape_uncached(GEN_1_1_HEB, style(FaceId::Hebrew, true)).unwrap();
    // Vowel and cantillation marks become zero-advance glyphs. Ezra composes some marks with
    // their letter into one precomposed glyph: dagesh, shin/sin dots, and holam on lamed (לֹ).
    let vowels_and_accents = GEN_1_1_HEB
        .chars()
        .filter(|c| ('\u{0591}'..='\u{05BB}').contains(c))
        .count();
    let marks: Vec<&Glyph> = run.glyphs.iter().filter(|g| g.advance == 0.0).collect();
    assert_eq!(vowels_and_accents, 11);
    assert_eq!(
        marks.len(),
        vowels_and_accents - 1,
        "all but the holam on lamed"
    );
    // Each mark is placed on its base letter: inside its cluster's extent, and not just left at
    // the pen (the cluster's left edge), which is where an unpositioned mark would sit.
    for m in &marks {
        let c = run.cluster_at(m.cluster as usize).unwrap();
        assert!(
            m.x >= c.x0 && m.x <= c.x1,
            "mark at {} outside {}..{}",
            m.x,
            c.x0,
            c.x1
        );
    }
    assert!(
        marks.iter().any(|m| {
            let c = run.cluster_at(m.cluster as usize).unwrap();
            (m.x - c.x0).abs() > 1.0
        }),
        "no mark was moved by GPOS"
    );
    // RTL: visual order runs from the end of the text to the start.
    let first = run.clusters.first().unwrap().bytes.start;
    let last = run.clusters.last().unwrap().bytes.start;
    assert!(first > last, "RTL clusters should be in visual order");
    // Each word's first letter carries its marks in the same cluster: בְּ is one cluster.
    let bet = run.cluster_at(0).unwrap();
    assert_eq!(&GEN_1_1_HEB[bet.bytes.clone()], "בְּ");
}

#[test]
fn clusters_map_back_to_every_byte() {
    for (text, face_id, rtl) in [
        (JN_1_1_GRK, FaceId::Regular, false),
        (GEN_1_1_HEB, FaceId::Hebrew, true),
        ("ﬁne “quotes” — and LORD", FaceId::Regular, false),
    ] {
        let run = shape_uncached(text, style(face_id, rtl)).unwrap();
        let mut covered: Vec<&std::ops::Range<usize>> =
            run.clusters.iter().map(|c| &c.bytes).collect();
        covered.sort_by_key(|r| r.start);
        assert_eq!(covered.first().unwrap().start, 0, "{text}");
        assert_eq!(covered.last().unwrap().end, text.len(), "{text}");
        for w in covered.windows(2) {
            assert_eq!(w[0].end, w[1].start, "{text}: gap or overlap");
        }
        for r in covered {
            assert!(text.is_char_boundary(r.start) && text.is_char_boundary(r.end));
        }
    }
}

#[test]
fn small_caps_lord_keeps_byte_offsets() {
    let source = "the LORD God";
    let lowered = small_caps_source(source);
    assert_eq!(lowered.len(), source.len());
    assert_eq!(lowered, "the lord god");
    let lord = &lowered[4..8];
    let plain = shape_uncached(lord, ShapeStyle::new(FaceId::Regular, 24.0, false, false)).unwrap();
    let smcp = shape_uncached(lord, ShapeStyle::new(FaceId::Regular, 24.0, true, false)).unwrap();
    // Gentium's smcp swaps in small-capital glyphs…
    assert_eq!(smcp.glyphs.len(), 4);
    assert!(
        smcp.glyphs
            .iter()
            .zip(&plain.glyphs)
            .all(|(a, b)| a.id != b.id),
        "smcp did not change the glyphs"
    );
    // …one per letter, mapped back to bytes 0..4 of "LORD".
    let starts: Vec<usize> = smcp.clusters.iter().map(|c| c.bytes.start).collect();
    assert_eq!(starts, [0, 1, 2, 3]);
    // Only ASCII is lowered, so offsets stay valid for any text.
    assert_eq!(small_caps_source("ὁ LORD").len(), "ὁ LORD".len());
}

#[test]
fn cache_hits_are_counted_and_identical_across_threads() {
    let cache = Arc::new(ShapeCache::new(1000));
    let words = ["In", "the", "beginning", "God", "created", "the", "heaven"];
    let st = style(FaceId::Regular, false);
    let first: Vec<Arc<ShapedRun>> = words.iter().map(|w| cache.shape(w, st).unwrap()).collect();
    let s = cache.stats();
    assert_eq!((s.hits, s.misses, s.entries), (1, 6, 6));
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let cache = Arc::clone(&cache);
            std::thread::spawn(move || {
                words
                    .iter()
                    .map(|w| (*cache.shape(w, st).unwrap()).clone())
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    for h in handles {
        let runs = h.join().unwrap();
        for (a, b) in runs.iter().zip(&first) {
            assert_eq!(a, &**b);
        }
    }
    let s = cache.stats();
    assert_eq!(s.misses, 6);
    assert_eq!(s.hits, 1 + 4 * 7);
    assert!(s.hit_rate() > 0.8);
    // A different size or direction is a different entry.
    cache
        .shape("In", ShapeStyle::new(FaceId::Regular, 12.0, false, false))
        .unwrap();
    assert_eq!(cache.stats().misses, 7);
}

#[test]
fn cache_clears_when_full() {
    let cache = ShapeCache::new(3);
    let st = style(FaceId::Regular, false);
    for w in ["a", "b", "c", "d"] {
        cache.shape(w, st).unwrap();
    }
    assert_eq!(cache.stats().entries, 1);
}

#[test]
fn trailing_space_is_measured() {
    let run = shape_uncached("God  ", style(FaceId::Regular, false)).unwrap();
    assert!(run.trailing_space > 0.0 && run.trailing_space < run.width);
    let run = shape_uncached("God", style(FaceId::Regular, false)).unwrap();
    assert_eq!(run.trailing_space, 0.0);
}

#[test]
fn svg_preview_draws_outlines() {
    let st = style(FaceId::Hebrew, true);
    let run = shape_uncached(GEN_1_1_HEB, st).unwrap();
    let svg = lines_svg(&[(run, st)]).unwrap();
    assert!(svg.starts_with("<svg"));
    assert!(svg.matches('M').count() > 20, "few outlines drawn");
}

/// A bundled font file loaded as if the user had picked it from this computer.
fn user_copy(id: FaceId) -> FaceId {
    let bytes = id.bundled_bytes().unwrap().to_vec();
    load_user_face(Arc::new(bytes), 0).unwrap()
}

#[test]
fn user_faces_load_shape_and_unload() {
    let a = user_copy(FaceId::Italic);
    let b = user_copy(FaceId::Italic);
    assert!(
        matches!(a, FaceId::User(_)) && a != b,
        "numbers are never reused"
    );
    assert!(!a.is_bundled() && FaceId::Regular.is_bundled());
    let run = shape_uncached("In the beginning", style(a, false)).unwrap();
    let bundled = shape_uncached("In the beginning", style(FaceId::Italic, false)).unwrap();
    assert_eq!(run.glyphs, bundled.glyphs);
    assert_eq!(
        face(a).unwrap().metrics,
        face(FaceId::Italic).unwrap().metrics
    );
    unload_user_face(a);
    assert_eq!(face(a).unwrap_err(), FaceError(a));
    assert!(face(b).is_ok());
    unload_user_face(b);
}

#[test]
fn a_file_that_is_not_a_font_is_refused() {
    let err = load_user_face(Arc::new(b"not a font".to_vec()), 0).unwrap_err();
    assert!(matches!(err.0, FaceId::User(_)));
    assert_eq!(err.to_string(), "the font file could not be read");
    let ttf = FaceId::Regular.bundled_bytes().unwrap().to_vec();
    assert!(
        load_user_face(Arc::new(ttf), 3).is_err(),
        "no face 3 in a .ttf"
    );
}

/// Gentium Plus Regular with its `glyf` and `loca` tables renamed away: the cmap, metrics and
/// shaping tables stay, but no glyph has an outline, as in a bitmap-only font.
fn without_outlines(bytes: &[u8]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let tables = usize::from(u16::from_be_bytes([out[4], out[5]]));
    for i in 0..tables {
        let tag = 12 + i * 16;
        // Renamed in place, keeping the table records sorted by tag.
        match &out[tag..tag + 4] {
            b"glyf" => out[tag + 3] = b'F',
            b"loca" => out[tag + 3] = b'A',
            _ => {}
        }
    }
    out
}

#[test]
fn a_font_without_outlines_is_refused() {
    let bytes = FaceId::Regular.bundled_bytes().unwrap();
    let bitmap_only = without_outlines(bytes);
    assert!(has_outlines(bytes, 0) && !has_outlines(&bitmap_only, 0));
    // Its cmap still claims the letters, so coverage alone would choose it and paint nothing.
    use skrifa::MetadataProvider as _;
    let font = skrifa::FontRef::new(&bitmap_only).unwrap();
    assert!(font.charmap().map('G').is_some());
    let err = load_user_face(Arc::new(bitmap_only), 0).unwrap_err();
    assert!(matches!(err.0, FaceId::User(_)));
}

#[test]
fn coverage_is_read_from_the_cmap() {
    let gentium = face(FaceId::Regular).unwrap();
    let ezra = face(FaceId::Hebrew).unwrap();
    assert_eq!(
        script_coverage(&gentium, ReadingScript::Latin),
        Coverage::Full
    );
    assert_eq!(
        script_coverage(&gentium, ReadingScript::Greek),
        Coverage::Full
    );
    assert_eq!(
        script_coverage(&gentium, ReadingScript::Hebrew),
        Coverage::None,
        "Gentium Plus has no Hebrew letters"
    );
    assert_eq!(
        script_coverage(&ezra, ReadingScript::Hebrew),
        Coverage::Full
    );
    // Ezra SIL has a few Greek letters but no polytonic Greek.
    assert!(matches!(
        script_coverage(&ezra, ReadingScript::Greek),
        Coverage::Partial { .. }
    ));
    assert!(gentium.covers('a') && !gentium.covers('א') && ezra.covers('\u{05A5}'));
}

#[test]
fn small_caps_support_is_read_from_gsub() {
    assert!(face(FaceId::Regular).unwrap().has_small_caps());
    assert!(!face(FaceId::Hebrew).unwrap().has_small_caps());
}

#[test]
fn reading_faces_default_to_the_bundled_faces() {
    let f = ReadingFaces::default();
    assert_eq!(f.latin.pick(true, true), FaceId::BoldItalic);
    assert_eq!(f.greek.pick(false, true), FaceId::Italic);
    assert_eq!(f.hebrew.faces(), [FaceId::Hebrew; 4]);
    assert_eq!(ReadingScript::of_char('a'), Some(ReadingScript::Latin));
    assert_eq!(ReadingScript::of_char('λ'), Some(ReadingScript::Greek));
    assert_eq!(ReadingScript::of_char('ἀ'), Some(ReadingScript::Greek));
    assert_eq!(ReadingScript::of_char('ב'), Some(ReadingScript::Hebrew));
    assert_eq!(
        ReadingScript::of_char('\u{05B0}'),
        Some(ReadingScript::Hebrew)
    );
    assert_eq!(ReadingScript::of_char(' '), None);
    assert_eq!(ReadingScript::of_char('3'), None);
    assert_eq!(ReadingScript::of_char('\u{0301}'), None);
}
