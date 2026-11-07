use std::sync::Arc;

use obs_layout::TextSettings;
use obs_model::{
    KeyScheme, Manifest, MorphemePart, ParaStyle, ResourceBuilder, ResourceType, SpanKind,
    TextAccumulator, UnitId, UnitKind,
};
use obs_scripture::{Book, Verse};

use super::*;

#[test]
fn automation_injects_input_before_its_scheduled_ui_frame() {
    use eframe::App as _;

    let events: Vec<(u32, egui::Event)> =
        serde_json::from_str(r#"[[1,{"PointerMoved":{"x":900.0,"y":100.0}}],[2,{"Text":"test"}]]"#)
            .unwrap();
    let mut app = App {
        shell: Shell::new(obs_engine::Session::new(obs_engine::JobMode::Inline)),
        styled: false,
        frame: 0,
        script: Vec::new(),
        input_script: events.clone(),
        shot: None,
        offer_starter: false,
    };
    let ctx = egui::Context::default();
    for frame in 1..=3 {
        app.frame = frame - 1;
        let mut input = egui::RawInput::default();
        app.raw_input_hook(&ctx, &mut input);
        let expected: Vec<_> = events
            .iter()
            .filter(|(at, _)| *at == frame)
            .map(|(_, event)| event.clone())
            .collect();
        assert_eq!(input.events, expected);
    }
}

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
pub(crate) fn bible() -> Arc<ReadingDoc> {
    numbered_bible("test-bible", "Test Bible", "TB", false)
}

/// The same text as [`bible`]; `hebrew` numbers it the Hebrew way (KJV Gen 31:55 is 32:1 and
/// KJV 32:n is 32:n+1), with the Verse map that says so.
pub(crate) fn numbered_bible(id: &str, title: &str, abbrev: &str, hebrew: bool) -> Arc<ReadingDoc> {
    Arc::new(ReadingDoc::from_resource(numbered_bible_resource(
        id, title, abbrev, hebrew,
    )))
}

/// The Resource behind [`numbered_bible`] (to write as a `.obr` where a test needs a file).
pub(crate) fn numbered_bible_resource(
    id: &str,
    title: &str,
    abbrev: &str,
    hebrew: bool,
) -> obs_model::Resource {
    use obs_scripture::{Passage, Standard, VerseMap, VerseMapEntry};
    let mut m = Manifest::new(id, title, abbrev, ResourceType::Bible, "en");
    m.key_schemes = vec![KeyScheme::Passage];
    m.license = "LicenseRef-PD".into();
    let mut b = ResourceBuilder::new(m);
    let genesis = UnitId::book(Book::Genesis);
    let mut t = TextAccumulator::new();
    let mut verses = Vec::new();
    let mut paras = Vec::new();
    let chapters: [(u16, u16); 4] = if hebrew {
        [(30, 43), (31, 54), (32, 33), (33, 20)]
    } else {
        [(30, 43), (31, 55), (32, 32), (33, 20)]
    };
    let mut map = Vec::new();
    for (chapter, count) in chapters {
        let mut para_start = t.pos();
        for v in 1..=count {
            let native = Verse::new(Book::Genesis, chapter, v);
            let std = match (hebrew, chapter, v) {
                (true, 32, 1) => Verse::new(Book::Genesis, 31, 55),
                (true, 32, n) => Verse::new(Book::Genesis, 32, n - 1),
                _ => native,
            };
            map.push(VerseMapEntry {
                verse: native,
                last: v,
                standard: Some(Passage::verse(std)),
            });
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
    if hebrew {
        b.set_verse_map(VerseMap::new(map, vec![], &Standard));
    }
    b.build().unwrap()
}

/// Run frames until the workers have nothing in flight.
fn settle(view: &mut ReadingView, height: f32) {
    for _ in 0..2000 {
        view.frame(height, Some(SYNC_BUDGET));
        if !view.has_pending() {
            view.frame(height, Some(SYNC_BUDGET));
            if !view.has_pending() {
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("layout workers never finished");
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
    // Let prefetch work finish first, so only the palette differs between the two draws.
    settle(&mut view, 700.0);
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
        // A selection from the middle of the first line to the middle of the fourth.
        let a = view.pos_at(120.0, 10.0);
        let b = view.pos_at(140.0, 140.0);
        if let (Some(a), Some(b)) = (a, b) {
            let sel = Selection {
                anchor: a,
                head: b,
                granularity: Granularity::Grapheme,
            };
            let c = palette.selection;
            for [x0, y0, x1, y1] in selection_rects(&view, &sel, h as f32 / scale) {
                let (x0, x1) = (
                    ((view::MARGIN + x0) * scale) as usize,
                    ((view::MARGIN + x1) * scale) as usize,
                );
                let (y0, y1) = ((y0.max(0.0) * scale) as usize, (y1 * scale) as usize);
                for y in y0..y1.min(h) {
                    for x in x0..x1.min(w) {
                        px[y * w + x] = [f32::from(c.r()), f32::from(c.g()), f32::from(c.b())];
                    }
                }
            }
            if name.starts_with("light") {
                eprintln!("copied:\n{}", copy_text(&doc, &sel));
            }
        }
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

#[test]
fn budgeted_frames_use_workers_and_settle() {
    let mut view = view();
    view.jump_to(Verse::new(Book::Genesis, 30, 1));
    // A zero budget forces everything not yet laid out onto the workers.
    let first = view.frame(800.0, Some(std::time::Duration::ZERO));
    assert!(first.stats.sync_blocks <= 1, "{:?}", first.stats);
    assert!(first.stats.submitted > 0);
    settle(&mut view, 800.0);
    let later = view.frame(800.0, Some(std::time::Duration::ZERO));
    assert_eq!(later.stats.placeholders, 0, "prefetched blocks are ready");
    assert_eq!(later.stats.sync_blocks, 0);
    // Scrolling within the prefetch window needs no layout on the UI thread.
    view.scroll_by(800.0);
    let scrolled = view.frame(800.0, Some(std::time::Duration::ZERO));
    assert_eq!(scrolled.stats.placeholders, 0, "{:?}", scrolled.stats);
}

#[test]
fn snippets_settle_within_two_frames() {
    let mut cache = SnippetCache::default();
    let settings = TextSettings::default();
    let frame = std::time::Duration::from_micros(8_333);
    let mut settled_at = None;
    for f in 0..10 {
        if cache
            .get(
                "For God so loved the world",
                obs_layout::ColorRole::Ink,
                false,
                300.0,
                &settings,
            )
            .is_some()
        {
            settled_at = Some(f);
            break;
        }
        std::thread::sleep(frame);
    }
    let f = settled_at.expect("snippet never laid out");
    assert!(f <= 2, "snippet took {f} frames");
    // Asking again is immediate.
    assert!(
        cache
            .get(
                "For God so loved the world",
                obs_layout::ColorRole::Ink,
                false,
                300.0,
                &settings
            )
            .is_some()
    );
}

#[test]
fn bench_runs_and_reports_every_scenario() {
    let cfg = bench::BenchConfig {
        steady_frames: 20,
        flings: 1,
        jumps: 2,
        panes: 2,
        pace: false,
        shell_scenarios: false,
        ..bench::BenchConfig::default()
    };
    let report = bench::run(bench::synthetic_bible(), cfg);
    let names: Vec<&str> = report.scenarios.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["steady", "fling", "jumps", "interlinear", "find"]);
    assert_eq!(report.scenarios[0].frames, 20);
    assert!(report.blocks > 1000);
    assert!(report.text_gpu_mib > 0.0);
    // Every pane draws a Find bar's matches (#175).
    assert!(report.find_matches > 10_000, "{}", report.find_matches);
    // Interlinear lines (#143) on every pane, scrolled for half the steady frames.
    assert_eq!(report.scenarios[3].frames, 10);
    // Find next (#175, #183): every step measured, each a jump that settles.
    let find = &report.scenarios[4];
    assert!(find.settle_frames_max.is_some_and(|n| n <= 2), "{find:?}");
    assert!(
        report.feature_errors.is_empty(),
        "{:?}",
        report.feature_errors
    );
}

/// The word the Find scenario steps through is in the bench Bible a few dozen times, far
/// apart, so each Find next jumps (#183).
#[test]
fn the_find_scenarios_word_is_sparse() {
    let doc = bench::synthetic_bible();
    let finder = obs_search::Finder::new(bench::FIND_SPARSE, Default::default()).unwrap();
    let found: Vec<(usize, usize)> = doc
        .units
        .iter()
        .enumerate()
        .flat_map(|(u, unit)| {
            finder
                .find(&unit.text)
                .into_iter()
                .map(move |r| (u, r.start))
        })
        .collect();
    assert!((12..200).contains(&found.len()), "{}", found.len());
    let books: std::collections::BTreeSet<usize> = found.iter().map(|(u, _)| *u).collect();
    assert!(books.len() >= 3, "{books:?}");
}

/// Every Shell-driven feature scenario (#183) sets up, shows its feature with realistic data
/// and measures it: the Atlas on a chapter of 120 places, a whole-Bible search's Grid and
/// Chart, the Exegetical Guide on Genesis 24, the Topic Guide on Prayer with every subtopic,
/// and Get Resources over 3,000 rows. Each waits for what it must show, so a scenario that
/// shows nothing fails here rather than measuring an empty Tab. Each times its feature Tab's
/// synchronous work every frame: a seeded 3 ms there (within a 60 Hz frame, over #19's 2 ms
/// budget) shows in every scenario's p99, where the gate catches it.
#[test]
fn every_shell_scenario_shows_its_feature_and_measures_it() {
    let slowdown = std::time::Duration::from_millis(3);
    let cfg = bench::BenchConfig {
        pace: false,
        tool_frames: 8,
        tool_steps: 2,
        feature_slowdown: slowdown,
        ..bench::BenchConfig::default()
    };
    let (reports, errors) = bench::tools::run(&cfg);
    assert!(errors.is_empty(), "{errors:?}");
    let names: Vec<&str> = reports.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, bench::SHELL_SCENARIOS);
    for r in &reports {
        assert!(r.frames >= 8, "{r:?}");
        let settle = r.settle_frames_max.expect("settle frames");
        assert!(settle < bench::SETTLE_LIMIT, "{r:?}");
        assert!(r.sync_layout_p50_ms >= 3.0, "{r:?}");
        assert!(r.sync_layout_p99_ms >= 3.0, "{r:?}");
    }
}

/// Get Resources filters and sorts its catalog on the UI thread in the frame its search or
/// type filter changes (#183): those frames are held to #19's 2 ms synchronous-work budget, not
/// exempt as a jump's target block is. 3 ms seeded only in an action's own frame (each key
/// typed, each type filter change) shows in the scenario's p99, where the gate catches it.
#[test]
fn get_resources_holds_catalog_filtering_frames_to_the_sync_budget() {
    let cfg = bench::BenchConfig {
        pace: false,
        tool_frames: 8,
        tool_steps: 2,
        action_slowdown: std::time::Duration::from_millis(3),
        ..bench::BenchConfig::default()
    };
    let s = bench::tools::get_resources(&cfg).expect("the Get Resources scenario");
    let r = s.report("get-resources", 1000.0 / 60.0);
    assert!(
        r.settle_frames_max.expect("settle frames") < bench::SETTLE_LIMIT,
        "{r:?}"
    );
    assert!(r.sync_layout_p99_ms >= 3.0, "{r:?}");
}

#[test]
fn a_selection_made_across_a_scroll_copies_the_exact_source() {
    let mut view = view();
    view.jump_to(Verse::new(Book::Genesis, 30, 3));
    let start = view.pos_at(40.0, 5.0).unwrap();
    let mut sel = Selection::caret(start);
    // Drag down through two chapters: the start scrolls out of view and its blocks may be
    // dropped from the layout cache; the selection keeps document coordinates.
    while view.current_location().unwrap() < Verse::new(Book::Genesis, 32, 4) {
        view.scroll_by(120.0);
        view.frame(600.0, None);
        sel.head = view.pos_at(200.0, 590.0).unwrap();
    }
    assert!(sel.head > sel.anchor);
    let text = selected_text(&view.doc, &sel);
    assert_eq!(text, &view.doc.units[0].text[start.byte..sel.head.byte]);
    let copied = copy_text(&view.doc, &sel);
    assert!(copied.starts_with(text.trim()));
    let label = copied.lines().last().unwrap();
    assert!(label.starts_with("Genesis 30:"), "{label}");
    assert!(label.contains("-32:") && label.ends_with("(TB)"), "{label}");
}

#[test]
fn word_and_verse_granularity() {
    let mut view = view();
    let v = Verse::new(Book::Genesis, 31, 10);
    view.jump_to(v);
    let at = view.pos_at(60.0, 5.0).unwrap();
    let word = Selection {
        anchor: at,
        head: at,
        granularity: Granularity::Word,
    };
    let w = selected_text(&view.doc, &word);
    assert!(
        !w.is_empty() && w.chars().all(|c| c.is_alphanumeric()) || w.trim().is_empty(),
        "{w:?}"
    );
    let verse = Selection {
        granularity: Granularity::Verse,
        ..word
    };
    let text = selected_text(&view.doc, &verse);
    let verse_at = view.doc.verse_containing(at.unit, at.byte).unwrap();
    let r = view.doc.verse_range_at(at.unit, at.byte).unwrap();
    assert_eq!(text, &view.doc.units[0].text[r]);
    assert!(copy_text(&view.doc, &verse).ends_with(&format!(
        "Genesis {}:{} (TB)",
        verse_at.chapter, verse_at.verse
    )));
}

/// Hebrew Gen 1:1-2 in one right-to-left paragraph, with an English gloss in it.
pub(crate) fn hebrew_doc() -> Arc<ReadingDoc> {
    let mut m = Manifest::new("heb-test", "Hebrew test", "HEB", ResourceType::Bible, "hbo");
    m.key_schemes = vec![KeyScheme::Passage];
    m.license = "LicenseRef-PD".into();
    m.direction = obs_model::Direction::Rtl;
    let mut b = ResourceBuilder::new(m);
    let genesis = UnitId::book(Book::Genesis);
    let mut t = TextAccumulator::new();
    let v1 = t.push("בְּרֵאשִׁ֖ית בָּרָ֣א (created) אֱלֹהִ֑ים אֵ֥ת הַשָּׁמַ֖יִם");
    t.push(" ");
    let v2 = t.push("וְהָאָ֗רֶץ הָיְתָ֥ה תֹ֙הוּ֙ וָבֹ֔הוּ");
    let end = t.pos();
    let text = t.into_string();
    // Tagged as TAHOT tags Gen 1:1: a prefix is its own morpheme, codes are OSHB-style.
    let part = |strongs: &str, lemma: Option<&str>, morph: &str, gloss: &str| MorphemePart {
        strongs: strongs.parse().unwrap(),
        lemma: lemma.map(String::from),
        morph: Some(morph.into()),
        gloss: Some(gloss.into()),
    };
    let tagged: [(&str, Vec<MorphemePart>); 3] = [
        (
            "בְּרֵאשִׁ֖ית",
            vec![
                part("H9003", None, "HR", "in"),
                part("H7225", Some("רֵאשִׁית"), "HNcfsa", "beginning"),
            ],
        ),
        (
            "בָּרָ֣א",
            vec![part("H1254A", Some("בָּרָא"), "HVqp3ms", "he created")],
        ),
        ("אֱלֹהִ֑ים", vec![part("H430", Some("אֱלֹהִים"), "HNcmpa", "God")]),
    ];
    let words: Vec<(std::ops::Range<usize>, Vec<MorphemePart>)> = tagged
        .into_iter()
        .map(|(w, parts)| {
            let start = text.find(w).unwrap();
            (start..start + w.len(), parts)
        })
        .collect();
    b.add_unit(genesis.clone(), UnitKind::BibleBook, None, text);
    b.add_span(
        &genesis,
        0..end,
        SpanKind::Para {
            style: ParaStyle::Prose,
        },
    );
    for (range, parts) in words {
        b.add_span(&genesis, range, SpanKind::TaggedWord { parts });
    }
    b.add_verse(&genesis, v1, Verse::new(Book::Genesis, 1, 1), 1);
    b.add_verse(&genesis, v2, Verse::new(Book::Genesis, 1, 2), 2);
    Arc::new(ReadingDoc::from_resource(b.build().unwrap()))
}

#[test]
fn selecting_mixed_hebrew_and_ltr_text_highlights_the_right_glyphs() {
    let doc = hebrew_doc();
    let mut view = ReadingView::new(Arc::clone(&doc), TextSettings::default());
    view.set_width(700.0);
    view.jump_to(Verse::new(Book::Genesis, 1, 1));
    let text = &doc.units[0].text;
    // From inside the second Hebrew word, across the English "(created)", into the third.
    let a = text.find("בָּרָ֣א").unwrap() + "בָּ".len();
    let b = text.find("אֱלֹהִ֑ים").unwrap() + "אֱ".len();
    let sel = Selection {
        anchor: DocPos { unit: 0, byte: a },
        head: DocPos { unit: 0, byte: b },
        granularity: Granularity::Grapheme,
    };
    let rects = selection_rects(&view, &sel, 600.0);
    // Right-to-left with a left-to-right island: the highlight breaks into several pieces.
    assert!(rects.len() >= 3, "{rects:?}");
    // Every cluster is highlighted exactly when its source byte is selected.
    let mut checked = 0;
    for (index, top, laid) in view.laid_in_view(600.0) {
        assert_eq!(doc.blocks[index].unit, 0);
        for line in &laid.lines {
            for item in &line.items {
                for (bytes, x0, x1) in item.clusters() {
                    let Some(obs_layout::UnitPos::Text(u)) = laid.to_unit(bytes.start) else {
                        continue;
                    };
                    let (cx, cy) = ((x0 + x1) / 2.0, top + (line.top + line.bottom) / 2.0);
                    let lit = rects
                        .iter()
                        .any(|r| cx >= r[0] && cx <= r[2] && cy >= r[1] && cy <= r[3]);
                    assert_eq!(lit, (a..b).contains(&u), "byte {u} {:?}", &text[u..]);
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 20);
    assert_eq!(selected_text(&doc, &sel), &text[a..b]);
    assert!(copy_text(&doc, &sel).ends_with("Genesis 1:1 (HEB)"));
}

#[test]
fn every_ink_on_every_highlight_meets_wcag_aa_in_every_tone() {
    let mut worst = (f32::MAX, String::new());
    let mut pairs = 0;
    for (name, p) in [
        ("Paper", Palette::PAPER),
        ("Sepia", Palette::SEPIA),
        ("Dark", Palette::DARK),
    ] {
        // The page, each overlay and each built-in background style as this tone draws it.
        let mut backgrounds: Vec<(String, egui::Color32)> = (p.overlays().into_iter())
            .map(|(n, c)| (n.to_string(), c))
            .collect();
        for style in obs_store::built_in_styles() {
            let paint = p.paint(&style.appearance);
            if paint.look == obs_engine::HighlightLook::Background {
                backgrounds.push((style.name, paint.color));
            }
        }
        for (bg_name, bg) in backgrounds {
            for (role, ink) in p.inks() {
                let c = contrast(ink, bg);
                assert!(
                    c >= 4.5,
                    "{name}: {role:?} on {bg_name} is {c:.2}:1 (AA needs 4.5:1)"
                );
                if c < worst.0 {
                    worst = (c, format!("{name} {role:?} on {bg_name}"));
                }
                pairs += 1;
            }
        }
        // The red underline is a mark, not text: WCAG's 3:1 for graphics, against the page.
        let u = contrast(p.built_in_color("red_underline"), p.page);
        assert!(u >= 3.0, "{name}: red underline {u:.2}:1");
        let u = contrast(p.underline, p.page);
        assert!(u >= 3.0, "{name}: word underline {u:.2}:1");
        // So is the current Find match's outline (#175), against the page and its fill.
        for (what, bg) in [("page", p.page), ("search hit", p.search_hit)] {
            let o = contrast(p.find_current, bg);
            assert!(o >= 3.0, "{name}: find outline on {what} {o:.2}:1");
        }
    }
    assert_eq!(pairs, 3 * 10 * 7);
    eprintln!("lowest contrast {:.2}:1 ({})", worst.0, worst.1);
    // The built-in styles keep the colours v0.1 tuned for each tone (#21 decision 3).
    assert_eq!(
        Palette::DARK.built_in_color("yellow"),
        egui::Color32::from_rgb(70, 60, 16)
    );
    assert_eq!(
        Palette::SEPIA.built_in_color("red_underline"),
        egui::Color32::from_rgb(184, 40, 32)
    );
    // The formula itself: black on white is 21:1, a colour on itself 1:1.
    assert!((contrast(egui::Color32::BLACK, egui::Color32::WHITE) - 21.0).abs() < 0.01);
    assert!((contrast(Palette::DARK.ink, Palette::DARK.ink) - 1.0).abs() < 1e-6);
}

/// Genesis 30–31 with every word a Tagged word (Strong's H1000 + word number) and a Scripture
/// reference at the end of each chapter.
pub(crate) fn tagged_bible() -> Arc<ReadingDoc> {
    use obs_scripture::{PassageList, Standard, parse_reference};
    let mut m = Manifest::new("tagged", "Tagged Bible", "TAG", ResourceType::Bible, "en");
    m.key_schemes = vec![KeyScheme::Passage];
    m.license = "LicenseRef-PD".into();
    let mut b = ResourceBuilder::new(m);
    let genesis = UnitId::book(Book::Genesis);
    let mut t = TextAccumulator::new();
    let mut verses = Vec::new();
    let mut words = Vec::new();
    let mut refs = Vec::new();
    let mut paras = Vec::new();
    for (chapter, count) in [(30u16, 24u16), (31, 24)] {
        let start = t.pos();
        for v in 1..=count {
            if t.pos() > start {
                t.push(" ");
            }
            let vs = t.pos();
            for (i, w) in verse_text(v).trim_end_matches('.').split(' ').enumerate() {
                if i > 0 {
                    t.push(" ");
                }
                let r = t.push(w);
                words.push((r, 1000 + i as u32));
            }
            t.push(".");
            verses.push((vs..t.pos(), Verse::new(Book::Genesis, chapter, v)));
        }
        t.push(" See ");
        let r = t.push("Romans 8:28");
        t.push(".");
        refs.push(r);
        paras.push(start..t.pos());
        t.push("\n");
    }
    b.add_unit(
        genesis.clone(),
        UnitKind::BibleBook,
        Some("Genesis".into()),
        t.into_string(),
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
    for (r, n) in words {
        let strongs = format!("H{n}").parse().unwrap();
        b.add_span(
            &genesis,
            r,
            SpanKind::TaggedWord {
                parts: vec![MorphemePart::new(strongs)],
            },
        );
    }
    for r in refs {
        let targets: PassageList = parse_reference("Rom 8:28", &Standard, None).unwrap();
        b.add_span(&genesis, r, SpanKind::ScriptureRef { targets });
    }
    for (r, v) in verses {
        b.add_verse(&genesis, r, v, v.verse);
    }
    Arc::new(ReadingDoc::from_resource(b.build().unwrap()))
}
