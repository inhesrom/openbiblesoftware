//! The reading pane's glyphs sample the glyphs they stand for, whatever the shared glyph
//! atlas does while a frame is drawn: grows (AUD-LAYOUT-TEXT-2) or runs out of room
//! (AUD-LAYOUT-TEXT-1, #255). A stand-in GPU applies each frame's texture deltas as eframe
//! does (set before painting, free after) and reads back what every glyph quad samples.

use std::collections::HashMap;
use std::sync::Arc;

use egui::{Color32, Pos2, Rect, Shape, TextureId, vec2};
use obs_layout::{LaidBlock, TextSettings};
use obs_model::{
    KeyScheme, Manifest, ParaStyle, ResourceBuilder, ResourceType, SpanKind, TextAccumulator,
    UnitId, UnitKind,
};
use obs_scripture::{Book, Verse};

use super::{AtlasTexture, PaneMarks, TEXT_GPU_BUDGET, paint_runs, reading_pane};
use crate::doc::ReadingDoc;
use crate::find::Found;
use crate::palette::Palette;
use crate::view::ReadingView;

/// Text size 40, the largest there is, at 2x.
const SIZE: f32 = 40.0;
const SCALE: f32 = 2.0;

/// A Bible of one paragraph per text, a verse each.
fn doc(texts: &[&str]) -> Arc<ReadingDoc> {
    let mut m = Manifest::new("atlas-test", "Atlas test", "AT", ResourceType::Bible, "en");
    m.key_schemes = vec![KeyScheme::Passage];
    m.license = "LicenseRef-PD".into();
    let mut b = ResourceBuilder::new(m);
    let genesis = UnitId::book(Book::Genesis);
    let mut t = TextAccumulator::new();
    let paras: Vec<_> = texts
        .iter()
        .map(|text| {
            let range = t.push(text);
            t.push("\n");
            range
        })
        .collect();
    b.add_unit(genesis.clone(), UnitKind::BibleBook, None, t.into_string());
    for (n, range) in (1..).zip(paras) {
        b.add_span(
            &genesis,
            range.clone(),
            SpanKind::Para {
                style: ParaStyle::Prose,
            },
        );
        b.add_verse(&genesis, range, Verse::new(Book::Genesis, 1, n), n);
    }
    Arc::new(ReadingDoc::from_resource(b.build().unwrap()))
}

/// Characters in `range` the Regular face has (controls and spaces left out), a space after
/// each.
fn covered(range: std::ops::RangeInclusive<char>) -> String {
    let regular = obs_fonts::face(obs_fonts::FaceId::Regular).unwrap();
    range
        .filter(|&c| !c.is_control() && !c.is_whitespace() && regular.covers(c))
        .flat_map(|c| [c, ' '])
        .collect()
}

/// A view of `doc` at Text size 40, every block in a 600 × 900 pane laid out already, so a
/// frame draws them all.
fn view(doc: Arc<ReadingDoc>) -> ReadingView {
    let mut view = ReadingView::new(
        doc,
        TextSettings {
            size: SIZE,
            ..TextSettings::default()
        },
    );
    view.set_width(600.0);
    view.visible(900.0);
    view
}

/// Input for a 640 × 900 window at 2x.
fn window() -> egui::RawInput {
    let mut viewports = egui::ViewportIdMap::default();
    viewports.insert(
        egui::ViewportId::ROOT,
        egui::ViewportInfo {
            native_pixels_per_point: Some(SCALE),
            ..Default::default()
        },
    );
    egui::RawInput {
        viewports,
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(640.0, 900.0))),
        ..Default::default()
    }
}

/// One frame of a reading pane filling the window.
fn frame(
    ctx: &egui::Context,
    view: &mut ReadingView,
    atlas: &mut AtlasTexture,
) -> egui::FullOutput {
    ctx.run_ui(window(), |ui| {
        reading_pane(
            ui,
            view,
            atlas,
            &Palette::PAPER,
            None,
            PaneMarks::default(),
            Found::default(),
        );
    })
}

/// One frame that draws `blocks` (each at its origin) with `atlas`, the lines within `visible`.
fn draw_blocks(
    ctx: &egui::Context,
    atlas: &mut AtlasTexture,
    blocks: &[(Arc<LaidBlock>, Pos2)],
    visible: Rect,
) -> egui::FullOutput {
    ctx.run_ui(window(), |ui| {
        for (laid, origin) in blocks {
            let runs = atlas.runs(ui.ctx(), laid, *origin, visible);
            paint_runs(ui.painter(), runs, |_| Color32::BLACK);
        }
    })
}

/// The glyphs of `blocks` (each at its origin) on the lines within `visible`, every one placed
/// at `scale` device px per point and drawn as [`Gpu::paint`] reads them back, sorted: as many
/// atlases as it takes. Also how many full atlases that is.
fn glyphs_in_view(
    blocks: &[(Arc<LaidBlock>, Pos2)],
    visible: Rect,
    scale: f32,
) -> (Vec<Drawn>, usize) {
    let mut atlas = obs_render::GlyphAtlas::new();
    let (mut drawn, mut full) = (Vec::new(), 0);
    for (laid, origin) in blocks {
        let mut from = 0;
        loop {
            let placed = atlas.quads_from(
                laid,
                [origin.x, origin.y],
                scale,
                [visible.top(), visible.bottom()],
                from,
            );
            let side = atlas.side();
            for q in &placed.quads {
                let px = |v: f32| (v * SCALE).round() as i32;
                let [u0, v0] = q.uv_min.map(|v| (v * side as f32).round() as usize);
                let [u1, v1] = q.uv_max.map(|v| (v * side as f32).round() as usize);
                let coverage = (v0..v1)
                    .flat_map(|y| (u0..u1).map(move |x| (x, y)))
                    .map(|(x, y)| atlas.coverage()[y * side + x])
                    .collect();
                let [x0, y0, x1, y1] = [q.min[0], q.min[1], q.max[0], q.max[1]].map(px);
                drawn.push(([x0, y0, x1, y1], coverage));
            }
            let Some(next) = placed.resume else { break };
            atlas.start_over();
            full += 1;
            from = next;
        }
    }
    drawn.sort();
    (drawn, full)
}

/// Textures as a GPU holds them: coverage (alpha) and size.
#[derive(Default)]
struct Gpu {
    textures: HashMap<TextureId, ([usize; 2], Vec<u8>)>,
    /// RGBA bytes of the textures held (egui's font texture left out) as the last frame was
    /// painted: freed ones go after painting.
    painted_bytes: usize,
}

/// A glyph as drawn: its box in device pixels and the coverage it samples.
type Drawn = ([i32; 4], Vec<u8>);

/// The glyphs drawn, ink on white, as a PPM image `size` device pixels: written to
/// `target/previews/glyph-atlas-{name}.ppm` when `UPDATE_PREVIEWS` is set.
fn preview(name: &str, drawn: &[Drawn], size: [usize; 2]) {
    if std::env::var_os("UPDATE_PREVIEWS").is_none() {
        return;
    }
    let [w, h] = size;
    let mut px = vec![255u8; w * h];
    for ([x0, y0, x1, y1], coverage) in drawn {
        // Texels across and down: as many as device pixels, or fewer when rastered coarser.
        let (bw, bh) = ((x1 - x0).max(1) as usize, (y1 - y0).max(1) as usize);
        let tw = ((coverage.len() * bw) as f32 / bh as f32)
            .sqrt()
            .round()
            .max(1.0) as usize;
        let th = coverage.len() / tw;
        for (bx, by) in (0..bh).flat_map(|y| (0..bw).map(move |x| (x, y))) {
            let (tx, ty) = (bx * tw / bw, by * th.max(1) / bh);
            let Some(&a) = coverage.get(ty * tw + tx) else {
                continue;
            };
            let (x, y) = (*x0 + bx as i32, *y0 + by as i32);
            if (0..w as i32).contains(&x) && (0..h as i32).contains(&y) {
                let p = &mut px[y as usize * w + x as usize];
                *p = (*p).min(255 - a);
            }
        }
    }
    let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
    out.extend(px.iter().flat_map(|&v| [v, v, v]));
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("glyph-atlas-{name}.ppm")), out).unwrap();
}

impl Gpu {
    /// Paint a frame's output as eframe does: textures set first, the glyphs sampled, then
    /// textures freed. Returns every textured quad, sorted.
    fn paint(&mut self, mut output: egui::FullOutput) -> Vec<Drawn> {
        for (id, deltas) in &output.textures_delta.set {
            for delta in deltas {
                let egui::ImageData::Color(image) = &delta.image;
                let alpha: Vec<u8> = image.pixels.iter().map(|p| p.a()).collect();
                match delta.pos {
                    None => {
                        self.textures.insert(*id, (image.size, alpha));
                    }
                    Some([x0, y0]) => {
                        let ([w, _], pixels) = self.textures.get_mut(id).unwrap();
                        for y in 0..image.size[1] {
                            let row = &alpha[y * image.size[0]..(y + 1) * image.size[0]];
                            let at = (y0 + y) * *w + x0;
                            pixels[at..at + row.len()].copy_from_slice(row);
                        }
                    }
                }
            }
        }
        self.painted_bytes = (self.textures.iter())
            .filter(|(id, _)| **id != TextureId::default())
            .map(|(_, ([w, h], _))| w * h * 4)
            .sum();
        let mut drawn = Vec::new();
        for clipped in &output.shapes {
            let Shape::Mesh(mesh) = &clipped.shape else {
                continue;
            };
            if mesh.texture_id == TextureId::default() {
                continue;
            }
            let ([w, h], pixels) = &self.textures[&mesh.texture_id];
            for quad in mesh.vertices.chunks(4) {
                let (pos, uv) = (
                    Rect::from_min_max(quad[0].pos, quad[3].pos),
                    Rect::from_min_max(quad[0].uv, quad[3].uv),
                );
                let px = |v: f32| (v * SCALE).round() as i32;
                let (u0, v0) = (
                    (uv.min.x * *w as f32).round() as usize,
                    (uv.min.y * *h as f32).round() as usize,
                );
                let (u1, v1) = (
                    (uv.max.x * *w as f32).round() as usize,
                    (uv.max.y * *h as f32).round() as usize,
                );
                let coverage = (v0..v1)
                    .flat_map(|y| (u0..u1).map(move |x| (x, y)))
                    .map(|(x, y)| pixels[y * w + x])
                    .collect();
                drawn.push((
                    [px(pos.min.x), px(pos.min.y), px(pos.max.x), px(pos.max.y)],
                    coverage,
                ));
            }
        }
        for id in &output.textures_delta.free {
            self.textures.remove(id);
        }
        output.textures_delta.clear();
        drawn.sort();
        drawn
    }
}

/// The glyphs of a pane drawn twice: in a frame where the atlas changes under it, then in one
/// where every glyph is in the atlas already (so nothing can move).
fn drawn_twice(view: &mut ReadingView, atlas: &mut AtlasTexture) -> (Vec<Drawn>, Vec<Drawn>) {
    let ctx = egui::Context::default();
    let mut gpu = Gpu::default();
    let first = gpu.paint(frame(&ctx, view, atlas));
    let generation = atlas.atlas.stats().generation;
    let second = gpu.paint(frame(&ctx, view, atlas));
    assert_eq!(
        atlas.atlas.stats().generation,
        generation,
        "the second frame changed the atlas too"
    );
    (first, second)
}

/// AUD-LAYOUT-TEXT-2: a later block that grows the atlas leaves an earlier block's glyphs
/// sampling the glyphs they stand for.
#[test]
fn a_block_that_grows_the_atlas_leaves_earlier_blocks_drawn_right() {
    let latin = covered('\u{20}'..='\u{FF}');
    let mut view = view(doc(&["Grace", &latin]));
    let mut atlas = AtlasTexture::default();
    let (first, second) = drawn_twice(&mut view, &mut atlas);
    assert!(
        atlas.atlas.stats().generation > 0,
        "the second block did not grow the atlas"
    );
    preview("grown-first-frame", &first, [1280, 1800]);
    assert!(first.len() > 100, "{} glyphs", first.len());
    let wrong = first.iter().filter(|g| !second.contains(g)).count();
    assert_eq!(
        wrong,
        0,
        "{wrong} of {} glyphs sample other glyphs",
        first.len()
    );
}

/// AUD-LAYOUT-TEXT-1: a block with more glyphs than the largest atlas holds paints every glyph
/// in view in its first frame, even when the atlas is full of other glyphs.
#[test]
fn a_block_larger_than_the_atlas_paints_in_its_first_frame() {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let big = covered('\u{20}'..='\u{2FFF}');
        let mut view = view(doc(&[&big]));
        let mut atlas = AtlasTexture::default();
        // Fill the atlas with the block's glyphs at other subpixel offsets, as a long session
        // might.
        let laid = Arc::clone(&view.visible(900.0)[0].laid);
        for x in [0.3, 0.6] {
            atlas.atlas.quads(&laid, [x, 0.0], SCALE);
        }
        let full = atlas.atlas.stats();
        let (first, second) = drawn_twice(&mut view, &mut atlas);
        let started_over = atlas.atlas.stats().generation > full.generation;
        // Every glyph in view: as many as on a fresh atlas.
        let fresh = Gpu::default().paint(frame(
            &egui::Context::default(),
            &mut view,
            &mut AtlasTexture::default(),
        ));
        let _ = tx.send((full.side, started_over, first, second, fresh));
    });
    let (side, started_over, first, second, fresh) = rx
        .recv_timeout(std::time::Duration::from_secs(120))
        .expect("the reading pane did not finish its frames");
    preview("oversized-first-frame", &first, [1280, 1800]);
    assert_eq!(side, obs_render::MAX_SIZE);
    assert!(started_over, "the glyphs in view fit beside the others");
    assert!(first.len() > 100, "{} glyphs", first.len());
    assert_eq!(first, second);
    assert_eq!(first, fresh);
}

/// A frame grows the atlas twice, a block each (512² → 1024² → 2048²), and then needs it to
/// start over. Its earlier blocks sample the textures replaced until the frame is painted, so
/// starting over at once would hold 1 + 4 + 16 + 16 = 37 MiB: the glyphs past the full atlas
/// wait for the next frame, which draws every glyph in view. GPU memory as painted stays
/// within the text GPU budget in every frame, also when the view needs a start-over each frame
/// (#255 review).
#[test]
fn growing_and_starting_over_stays_within_the_text_gpu_budget() {
    let (latin, big) = (covered('\u{20}'..='\u{FF}'), covered('\u{20}'..='\u{2FFF}'));
    let mut view = view(doc(&["Grace", &latin, &big]));
    let blocks: Vec<_> = (view.visible(100_000.0).into_iter())
        .map(|b| (b.laid, Pos2::new(20.0, b.top)))
        .collect();
    // 4,000 points: a little more than the largest atlas holds; 5,000: a start-over a frame.
    for bottom in [4000.0, 5000.0] {
        let visible = Rect::from_min_max(Pos2::ZERO, Pos2::new(640.0, bottom));
        let ctx = egui::Context::default();
        let (mut atlas, mut gpu) = (AtlasTexture::default(), Gpu::default());
        let mut frames = Vec::new();
        for _ in 0..4 {
            let output = draw_blocks(&ctx, &mut atlas, &blocks, visible);
            let repaint = output.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
            frames.push((gpu.paint(output), repaint));
            // Spare textures are emptied to a pixel: 4 bytes each, at most one for each
            // texture a frame can hold.
            assert!(
                gpu.painted_bytes <= TEXT_GPU_BUDGET + 4 * 4,
                "{} bytes painted at {bottom} points",
                gpu.painted_bytes
            );
        }
        assert!(
            atlas.atlas.stats().generation > 2,
            "the atlas did not start over"
        );
        if bottom == 4000.0 {
            let in_view = glyphs_in_view(&blocks, visible, SCALE).0.len();
            let (first, repaint) = &frames[0];
            assert!(
                first.len() < in_view,
                "the first frame drew all {in_view} glyphs"
            );
            assert_eq!(*repaint, std::time::Duration::ZERO, "no frame asked for");
            assert_eq!(frames[1].0.len(), in_view);
            assert!(frames[2..].iter().all(|(f, _)| *f == frames[1].0));
        }
    }
}

/// The covered characters U+0020..U+2FFF at Text size 40, each block at its origin: at 2x,
/// three at a quarter-pixel apart are more distinct glyphs than two full atlases hold.
fn oversized_blocks() -> Vec<(Arc<LaidBlock>, Pos2)> {
    let big = covered('\u{20}'..='\u{2FFF}');
    let mut view = view(doc(&[&big]));
    let laid = Arc::clone(&view.visible(900.0)[0].laid);
    [20.0, 20.125, 20.25]
        .into_iter()
        .map(|x| (Arc::clone(&laid), Pos2::new(x, 0.0)))
        .collect()
}

/// Frames drawn with `atlas` until one asks for no other (at most `limit`): what each drew,
/// whether it asked for another, and the GPU memory held as it was painted.
fn frames_until_settled(
    ctx: &egui::Context,
    atlas: &mut AtlasTexture,
    gpu: &mut Gpu,
    blocks: &[(Arc<LaidBlock>, Pos2)],
    visible: Rect,
    limit: usize,
) -> Vec<Vec<Drawn>> {
    let mut frames = Vec::new();
    for _ in 0..limit {
        let output = draw_blocks(ctx, atlas, blocks, visible);
        let repaint = output.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
        frames.push(gpu.paint(output));
        assert!(
            gpu.painted_bytes <= TEXT_GPU_BUDGET + 4 * 4,
            "{} bytes painted",
            gpu.painted_bytes
        );
        if repaint == std::time::Duration::MAX {
            return frames;
        }
    }
    panic!("still asking for frames after {limit}");
}

/// AUD-LAYOUT-TEXT-1, #255 review: more distinct glyphs in view than the text GPU budget's two
/// full atlases hold. Every frame cannot draw them all at full scale, so later frames raster
/// them coarser: within a few frames, each asked for, every glyph in view is drawn, and then
/// frames stop being asked for and draw the same. Once the view needs less, the glyphs go back
/// to full scale.
#[test]
fn more_glyphs_in_view_than_the_budget_holds_are_all_drawn_coarser() {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let blocks = oversized_blocks();
        let visible = Rect::from_min_max(Pos2::ZERO, Pos2::new(640.0, 100_000.0));
        let (_, full) = glyphs_in_view(&blocks, visible, SCALE);
        let ctx = egui::Context::default();
        let (mut atlas, mut gpu) = (AtlasTexture::default(), Gpu::default());
        let limit = 8;
        let frames = frames_until_settled(&ctx, &mut atlas, &mut gpu, &blocks, visible, limit);
        let coarse = atlas.coarse;
        let settled = draw_blocks(&ctx, &mut atlas, &blocks, visible);
        let again = gpu.paint(settled);
        // The view changes to a small one on input (a jump): egui draws a frame after the
        // one that took it, and from there the glyphs go back to full scale.
        let grace = view(doc(&["Grace"])).visible(900.0)[0].laid.clone();
        let small = [(grace, Pos2::new(20.0, 0.0))];
        let input = egui::RawInput {
            events: vec![egui::Event::PointerMoved(Pos2::new(100.0, 100.0))],
            ..window()
        };
        let jumped = ctx.run_ui(input, |ui| {
            for (laid, origin) in &small {
                let runs = atlas.runs(ui.ctx(), laid, *origin, visible);
                paint_runs(ui.painter(), runs, |_| Color32::BLACK);
            }
        });
        let next = jumped.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
        assert_eq!(next, std::time::Duration::ZERO, "no frame after the input");
        gpu.paint(jumped);
        let back = frames_until_settled(&ctx, &mut atlas, &mut gpu, &small, visible, limit);
        let fresh = Gpu::default().paint(draw_blocks(
            &egui::Context::default(),
            &mut AtlasTexture::default(),
            &small,
            visible,
        ));
        let _ = tx.send((full, frames, coarse, again, back, atlas.coarse, fresh));
    });
    let (full, frames, coarse, again, back, coarse_after, fresh) = rx
        .recv_timeout(std::time::Duration::from_secs(300))
        .expect("the frames did not finish");
    assert!(full >= 2, "the glyphs in view fit in {} atlases", full + 1);
    assert!(frames.len() > 1, "the first frame drew every glyph");
    preview("over-budget-settled", frames.last().unwrap(), [1280, 1800]);
    // Every glyph in view, each the glyph it stands for: as an atlas at that scale draws them.
    let blocks = oversized_blocks();
    let visible = Rect::from_min_max(Pos2::ZERO, Pos2::new(640.0, 100_000.0));
    let scale = SCALE / (1u32 << coarse) as f32;
    let (in_view, _) = glyphs_in_view(&blocks, visible, scale);
    // egui asks for one frame more after any asked for: the first whole one need not be last.
    let whole = frames.iter().position(|f| *f == in_view);
    let whole = whole.expect("no frame drew every glyph in view");
    assert!(frames[..whole].iter().all(|f| f.len() < in_view.len()));
    assert!(frames[whole..].iter().all(|f| *f == in_view));
    assert!(
        whole <= 1 + super::MAX_COARSE as usize,
        "{whole} frames partial"
    );
    assert!(coarse > 0, "every glyph drawn at full scale");
    assert_eq!(again, in_view);
    assert_eq!(coarse_after, 0, "the small view stays coarse");
    assert_eq!(*back.last().unwrap(), fresh);
}
