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
