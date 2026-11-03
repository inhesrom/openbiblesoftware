use std::sync::Arc;

use obs_layout::TextSettings;
use obs_model::{
    KeyScheme, Manifest, ParaStyle, ResourceBuilder, ResourceType, SpanKind, TextAccumulator,
    UnitId, UnitKind,
};
use obs_scripture::{Book, Verse};

use super::*;

const WORDS: &[&str] = &[
    "And", "Laban", "said", "to", "Jacob", "behold", "this", "heap", "and", "pillar", "which", "I",
    "have", "cast", "betwixt", "me", "and", "thee", "is", "a", "witness",
];

fn verse_text(n: u16) -> String {
    let len = 18 + (n as usize * 7) % 23;
    (0..len)
        .map(|i| WORDS[(i + n as usize) % WORDS.len()])
        .collect::<Vec<_>>()
        .join(" ")
        + "."
}

/// Genesis 30–33 with every chapter one paragraph per 6 verses, so verses start mid-line.
fn bible() -> Arc<ReadingDoc> {
    let mut m = Manifest::new("test-bible", "Test Bible", "TB", ResourceType::Bible, "en");
    m.key_schemes = vec![KeyScheme::Passage];
    m.license = "LicenseRef-PD".into();
    let mut b = ResourceBuilder::new(m);
    let genesis = UnitId::book(Book::Genesis);
    let mut t = TextAccumulator::new();
    let mut verses = Vec::new();
    let mut paras = Vec::new();
    for (chapter, count) in [(30u16, 43u16), (31, 55), (32, 32), (33, 20)] {
        let mut para_start = t.pos();
        for v in 1..=count {
            if t.pos() > 0 && t.as_str().ends_with('.') {
                t.push(" ");
            }
            let r = t.push(&verse_text(v));
            verses.push((r.clone(), Verse::new(Book::Genesis, chapter, v)));
            if v % 6 == 0 || v == count {
                paras.push(para_start..r.end);
                t.push("\n");
                para_start = t.pos();
            }
        }
    }
    let text = t.into_string();
    b.add_unit(
        genesis.clone(),
        UnitKind::BibleBook,
        Some("Genesis".into()),
        text,
    );
    for p in paras {
        b.add_span(
            &genesis,
            p,
            SpanKind::Para {
                style: ParaStyle::Prose,
            },
        );
    }
    for (r, v) in verses {
        b.add_verse(&genesis, r, v, v.verse);
    }
    Arc::new(ReadingDoc::from_resource(b.build().unwrap()))
}

fn view() -> ReadingView {
    let mut v = ReadingView::new(bible(), TextSettings::default());
    v.set_width(450.0);
    v
}

#[test]
fn jump_to_a_verse_mid_line_reports_that_verse() {
    let mut view = view();
    let gen_31_55 = Verse::new(Book::Genesis, 31, 55);
    assert!(view.jump_to(gen_31_55));
    assert_eq!(view.current_location(), Some(gen_31_55));
    // Its line is at the top of the view.
    let top = view.verse_top(gen_31_55, 600.0).unwrap();
    assert!(top.abs() < 0.5, "line top at {top}");
    // 31:55 starts mid-line, after the end of 31:54. Scrolling clears the jump, and the rule
    // still says 31:55 because it starts on the top line (the prototype reported 31:54).
    view.scroll_by(1.0);
    assert_eq!(view.current_location(), Some(gen_31_55));
    // …and a verse that starts the top line wins.
    assert!(view.jump_to(Verse::new(Book::Genesis, 32, 1)));
    view.scroll_by(0.5);
    assert_eq!(
        view.current_location(),
        Some(Verse::new(Book::Genesis, 32, 1))
    );
}

#[test]
fn remeasuring_never_moves_the_text_in_view() {
    let mut view = view();
    let target = Verse::new(Book::Genesis, 31, 30);
    view.jump_to(target);
    view.scroll_by(-37.0);
    let before = view.verse_top(target, 600.0).unwrap();
    let (pos_before, _) = view.position();
    // Lay out everything else (real heights replace estimates above and below).
    for i in 0..view.doc.blocks.len() {
        view.laid(i);
    }
    let after = view.verse_top(target, 600.0).unwrap();
    assert!((before - after).abs() < 0.01, "{before} -> {after}");
    // The scrollbar position may move (estimates were refined), the text does not.
    let (pos_after, _) = view.position();
    let _ = (pos_before, pos_after);
}

#[test]
fn scrolling_crosses_blocks_and_stops_at_the_ends() {
    let mut view = view();
    view.scroll_by(-500.0);
    assert_eq!(view.position().0, 0.0);
    let start = view.current_location().unwrap();
    assert_eq!(start, Verse::new(Book::Genesis, 30, 1));
    for _ in 0..200 {
        view.scroll_by(97.0);
    }
    let later = view.current_location().unwrap();
    assert!(later > start);
    view.scroll_by(1e7);
    let (y, total) = view.position();
    assert!(y <= total);
    assert_eq!(view.current_location().unwrap().chapter, 33);
    // Scrollbar drags land on the right place.
    view.scroll_to_position(0.0);
    assert_eq!(view.current_location(), Some(start));
}

#[test]
fn visible_blocks_fill_the_viewport_without_gaps() {
    let mut view = view();
    view.jump_to(Verse::new(Book::Genesis, 31, 1));
    view.scroll_by(13.0);
    let vis = view.visible(800.0);
    assert!(vis.first().unwrap().top <= 0.0);
    for w in vis.windows(2) {
        assert!((w[0].top + w[0].laid.height - w[1].top).abs() < 0.01);
    }
    let last = vis.last().unwrap();
    assert!(last.top + last.laid.height >= 800.0);
}

#[test]
fn width_changes_keep_the_reader_on_the_same_verse() {
    let mut view = view();
    let v = Verse::new(Book::Genesis, 32, 7);
    view.jump_to(v);
    view.set_width(300.0);
    assert_eq!(view.current_location(), Some(v));
    assert!(view.verse_top(v, 600.0).unwrap().abs() < 0.5);
}

#[test]
fn palette_changes_do_not_relayout_or_reraster() {
    let mut view = view();
    let mut atlas = AtlasTexture::default();
    view.jump_to(Verse::new(Book::Genesis, 31, 1));
    let draw = |view: &mut ReadingView, atlas: &mut AtlasTexture, palette: Palette| {
        let mut colours = Vec::new();
        for vb in view.visible(700.0) {
            for q in atlas.atlas.quads(&vb.laid, [0.0, vb.top], 2.0) {
                colours.push(palette.color(q.role));
            }
        }
        colours
    };
    let light = draw(&mut view, &mut atlas, Palette::PAPER);
    let (layouts, rasters) = (view.layouts, atlas.atlas.stats().rasters);
    let dark = draw(&mut view, &mut atlas, Palette::DARK);
    assert_eq!(view.layouts, layouts, "a palette change re-laid text out");
    assert_eq!(
        atlas.atlas.stats().rasters,
        rasters,
        "a palette change re-rastered glyphs"
    );
    assert_eq!(light.len(), dark.len());
    assert_ne!(light, dark);
}

#[test]
fn text_memory_stays_within_budget() {
    let mut view = view();
    let mut atlas = AtlasTexture::default();
    // Several panes' worth of text at 1x and 2x.
    for scale in [1.0, 1.5, 2.0] {
        view.scroll_to_position(0.0);
        for _ in 0..40 {
            for vb in view.visible(900.0) {
                atlas.atlas.quads(&vb.laid, [0.0, vb.top], scale);
            }
            view.scroll_by(400.0);
        }
    }
    // Uploaded as RGBA: 4 bytes per coverage byte.
    let gpu_bytes = atlas.atlas.bytes() * 4;
    assert!(gpu_bytes <= 32 * 1024 * 1024, "{gpu_bytes} bytes");
}

/// Composite what the pane would draw (atlas quads × palette over the page) into an image, as
/// the GPU would. Writes `docs/previews/reading-pane-*.ppm` when `UPDATE_PREVIEWS` is set;
/// uses the `.obr` in `OBS_PREVIEW_OBR` if given.
#[test]
fn previews() {
    if std::env::var_os("UPDATE_PREVIEWS").is_none() {
        return;
    }
    let doc = match std::env::var_os("OBS_PREVIEW_OBR") {
        Some(p) => Arc::new(ReadingDoc::open(std::path::Path::new(&p)).unwrap()),
        None => bible(),
    };
    let (w, h, scale) = (560usize, 420usize, 1.5f32);
    for (name, verse, palette) in [
        (
            "light-gen-31-55",
            Verse::new(Book::Genesis, 31, 55),
            Palette::PAPER,
        ),
        ("dark-ps-23", Verse::new(Book::Psalms, 23, 0), Palette::DARK),
    ] {
        let mut view = ReadingView::new(Arc::clone(&doc), TextSettings::default());
        view.set_width(w as f32 / scale - 2.0 * view::MARGIN);
        if !view.jump_to(verse) {
            view.jump_to(Verse::new(Book::Genesis, 31, 50));
        }
        let mut atlas = obs_render::GlyphAtlas::new();
        let mut quads = Vec::new();
        for vb in view.visible(h as f32 / scale) {
            quads.extend(atlas.quads(&vb.laid, [view::MARGIN, vb.top], scale));
        }
        let page = palette.page;
        let mut px: Vec<[f32; 3]> =
            vec![[page.r() as f32, page.g() as f32, page.b() as f32]; w * h];
        let side = atlas.side() as f32;
        for q in quads {
            let c = palette.color(q.role);
            let (x0, y0) = (
                (q.min[0] * scale).round() as i64,
                (q.min[1] * scale).round() as i64,
            );
            let (u0, v0) = ((q.uv_min[0] * side) as i64, (q.uv_min[1] * side) as i64);
            let (gw, gh) = (
                ((q.uv_max[0] - q.uv_min[0]) * side).round() as i64,
                ((q.uv_max[1] - q.uv_min[1]) * side).round() as i64,
            );
            for dy in 0..gh {
                for dx in 0..gw {
                    let (x, y) = (x0 + dx, y0 + dy);
                    if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
                        continue;
                    }
                    let a =
                        f32::from(atlas.coverage()[((v0 + dy) * side as i64 + u0 + dx) as usize])
                            / 255.0;
                    let p = &mut px[y as usize * w + x as usize];
                    for (ch, cv) in p.iter_mut().zip([c.r(), c.g(), c.b()]) {
                        *ch = *ch * (1.0 - a) + f32::from(cv) * a;
                    }
                }
            }
        }
        let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
        out.extend(px.iter().flat_map(|p| p.map(|v| v.round() as u8)));
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("reading-pane-{name}.ppm")), out).unwrap();
    }
}
