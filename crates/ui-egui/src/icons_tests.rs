//! The icon set (#220): every icon reads and draws, is drawn texel for pixel at 1×, 1.5× and
//! 2×, in colours that stand out in light, dark and sepia; and no emoji or symbol glyph stands
//! in for an icon in the interface's code.

use std::path::{Path, PathBuf};

use egui::{Color32, Event, Pos2, RawInput, Rect, Shape, TextureId, pos2, vec2};

use super::*;
use crate::Palette;
use crate::theme::{self, Chrome};

#[test]
fn every_icon_reads_and_draws_a_line_drawing() {
    for &icon in Icon::ALL {
        let svg = icon.svg();
        assert!(
            resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()).is_ok(),
            "{} does not parse",
            icon.file()
        );
        let img = rasterize(icon, 16);
        let covered = img.pixels.iter().filter(|p| p.a() > 0).count();
        let darkest = img.pixels.iter().map(|p| p.a()).max().unwrap_or(0);
        assert!(covered > 12, "{} draws {covered} pixels", icon.file());
        assert!(covered < 256, "{} fills the whole square", icon.file());
        assert!(darkest > 150, "{} has no line at 16 px", icon.file());
        // A coverage mask: white, premultiplied.
        assert!(img.pixels.iter().all(|p| p.r() == p.a() && p.g() == p.a()));
    }
}

/// One stroke weight across the set: every outline icon is drawn with the same stroke.
#[test]
fn every_icon_has_the_one_stroke_weight() {
    for &icon in Icon::ALL {
        let svg = icon.svg();
        assert!(
            svg.contains(r#"stroke-width="2""#) && svg.contains(r#"viewBox="0 0 24 24""#),
            "{} is not on the 24-unit grid with a 2-unit stroke",
            icon.file()
        );
        assert_eq!(
            svg.matches("stroke-width").count(),
            1,
            "{} sets a stroke of its own",
            icon.file()
        );
    }
}

#[test]
fn every_icon_file_is_used_and_credited() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icons");
    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".svg"))
        .collect();
    files.sort();
    let mut used: Vec<String> = Icon::ALL
        .iter()
        .map(|i| format!("{}.svg", i.file()))
        .collect();
    used.sort();
    used.dedup();
    assert_eq!(files, used, "assets/icons and Icon::ALL differ");
    assert!(dir.join("LICENSE").exists());
    let attribution = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ATTRIBUTION.toml"),
    )
    .unwrap();
    assert!(attribution.contains(r#"path = "crates/ui-egui/assets/icons/*""#));
}

/// The icon meshes of one frame drawn at `ppp`: (rect in points, texture), with each icon
/// texture's size in pixels and filtering.
struct Drawn {
    meshes: Vec<(Rect, TextureId)>,
    textures: std::collections::HashMap<TextureId, ([usize; 2], egui::TextureOptions)>,
}

fn draw_at(ppp: f32, draw: impl FnMut(&mut egui::Ui)) -> Drawn {
    let ctx = egui::Context::default();
    let mut draw = draw;
    let mut drawn = Drawn {
        meshes: Vec::new(),
        textures: std::collections::HashMap::new(),
    };
    for _ in 0..2 {
        let mut viewports = egui::ViewportIdMap::default();
        viewports.insert(
            egui::ViewportId::ROOT,
            egui::ViewportInfo {
                native_pixels_per_point: Some(ppp),
                ..Default::default()
            },
        );
        let input = RawInput {
            viewports,
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0))),
            events: vec![Event::PointerGone],
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| draw(ui));
        for (id, deltas) in std::mem::take(&mut out.textures_delta.set) {
            for d in deltas {
                drawn.textures.insert(id, (d.image.size(), d.options));
            }
        }
        out.textures_delta.clear();
        drawn.meshes.clear();
        for c in std::mem::take(&mut out.shapes) {
            collect_meshes(&c.shape, &mut drawn.meshes);
        }
    }
    drawn
        .meshes
        .retain(|(_, t)| *t != TextureId::default() && drawn.textures.contains_key(t));
    drawn
}

fn collect_meshes(shape: &Shape, out: &mut Vec<(Rect, TextureId)>) {
    match shape {
        Shape::Vec(v) => v.iter().for_each(|s| collect_meshes(s, out)),
        Shape::Mesh(m) => {
            let r = Rect::from_points(&m.vertices.iter().map(|v| v.pos).collect::<Vec<_>>());
            out.push((r, m.texture_id));
        }
        Shape::Rect(r) => {
            if let Some(b) = &r.brush {
                out.push((r.rect, b.fill_texture_id));
            }
        }
        _ => {}
    }
}

/// #220 acceptance: icons are crisp at 1×, 1.5× and 2×. Each is rasterized at its size in
/// physical pixels and drawn texel for pixel (unfiltered) on the pixel grid, wherever it is
/// drawn: as a widget, in a Button with a label, or painted.
#[test]
fn icons_are_drawn_texel_for_pixel_at_1x_1_5x_and_2x() {
    for ppp in [1.0f32, 1.5, 2.0] {
        let drawn = draw_at(ppp, |ui| {
            let ctx = ui.ctx().clone();
            ui.add(image(&ctx, Icon::Note, BODY, Color32::BLACK));
            ui.add(egui::Button::new((
                image(&ctx, Icon::Search, BODY, Color32::BLACK),
                "Search this",
            )));
            ui.add(image(&ctx, Icon::Close, SMALL, Color32::BLACK));
            // Painted in a rect off the pixel grid.
            let r = Rect::from_min_size(pos2(200.3, 100.45), vec2(23.1, 22.7));
            paint(ui.painter(), Icon::Pin, r, LARGE, Color32::BLACK);
        });
        assert_eq!(drawn.meshes.len(), 4, "at {ppp}×: {:?}", drawn.meshes);
        for (rect, tex) in &drawn.meshes {
            let (size, options) = drawn.textures[tex];
            let px = [rect.width() * ppp, rect.height() * ppp];
            assert_eq!(
                [px[0].round() as usize, px[1].round() as usize],
                size,
                "at {ppp}×, {rect:?} is not texel for pixel"
            );
            assert!((px[0] - px[0].round()).abs() < 0.01, "{ppp}× {rect:?}");
            for v in [rect.min.x * ppp, rect.min.y * ppp] {
                assert!(
                    (v - v.round()).abs() < 0.01,
                    "at {ppp}×, {rect:?} is off the pixel grid"
                );
            }
            assert_eq!(options.magnification, egui::TextureFilter::Nearest);
            assert_eq!(options.minification, egui::TextureFilter::Nearest);
            assert!(
                [12, 16, 18]
                    .iter()
                    .any(|s| size[0] == (*s as f32 * ppp).round() as usize)
            );
        }
    }
}

/// The colour roles icons are drawn in, on the backgrounds they are drawn on.
fn chrome_roles(c: &Chrome) -> [(&'static str, Color32); 6] {
    [
        ("ink", c.ink),
        ("muted", c.muted),
        ("lapis", c.lapis),
        ("verdigris", c.verdigris),
        ("gilt", c.gilt),
        ("warn", c.warn),
    ]
}

/// #220 acceptance: icons show in light, dark and sepia. Every colour role an icon is drawn
/// in meets 3:1 (WCAG non-text contrast) on every chrome surface of the Light and Dark themes,
/// and the page roles do on the Paper, Sepia and Dark pages.
#[test]
fn icon_colours_meet_3_to_1_in_light_dark_and_sepia() {
    for (name, c) in [("light", theme::LIGHT), ("dark", theme::DARK)] {
        for (bg_name, bg) in [
            ("shell", c.shell),
            ("surface", c.surface),
            ("field", c.field),
            ("hover", c.hover),
            ("selection", c.selection),
        ] {
            for (role, fg) in chrome_roles(&c) {
                let k = crate::contrast(fg, bg);
                assert!(k >= 3.0, "{name}: {role} on {bg_name} is {k:.2}:1");
            }
        }
    }
    for (name, p) in [
        ("paper", Palette::PAPER),
        ("sepia", Palette::SEPIA),
        ("dark", Palette::DARK),
    ] {
        for (role, fg) in [
            ("ink", p.ink),
            ("muted", page_muted(&p)),
            ("accent", p.link),
        ] {
            for (bg_name, bg) in [("page", p.page), ("hover", p.hover)] {
                let k = crate::contrast(fg, bg);
                assert!(k >= 3.0, "{name}: {role} on {bg_name} is {k:.2}:1");
            }
        }
    }
}

/// Write the icon catalogue images (`docs/previews/icons-<theme>.ppm`) when
/// `UPDATE_PREVIEWS` is set: every icon, in catalogue order, 13 to a row, at 1×, 1.5× and 2×
/// (one block each, left to right), as each is rasterized for that scale; in the theme's ink
/// on its surface, for the light and dark chrome and the sepia page.
#[test]
fn icon_catalogue_previews() {
    if std::env::var_os("UPDATE_PREVIEWS").is_none() {
        return;
    }
    const ACROSS: usize = 13;
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/previews");
    let sepia = Palette::SEPIA;
    let rows = Icon::ALL.len().div_ceil(ACROSS);
    let scales = [1.0f32, 1.5, 2.0];
    // Each block's cell is twice the icon's side; blocks sit side by side, 16 px apart.
    let cell = |scale: f32| (BODY * scale * 2.0) as usize;
    let w: usize = scales.iter().map(|s| cell(*s) * ACROSS + 16).sum::<usize>() + 16;
    let h = cell(2.0) * rows + 32;
    for (name, bg, fg) in [
        ("light", theme::LIGHT.surface, theme::LIGHT.ink),
        ("dark", theme::DARK.surface, theme::DARK.ink),
        ("sepia", sepia.page, sepia.ink),
    ] {
        let mut px = vec![bg; w * h];
        let mut left = 16;
        for scale in scales {
            let side = (BODY * scale).round() as usize;
            let c = cell(scale);
            for (n, &icon) in Icon::ALL.iter().enumerate() {
                let (x0, y0) = (left + (n % ACROSS) * c, 16 + (n / ACROSS) * c);
                let mask = rasterize(icon, side as u32);
                for (i, m) in mask.pixels.iter().enumerate() {
                    let (x, y) = (x0 + i % side, y0 + i / side);
                    let a = f32::from(m.a()) / 255.0;
                    let d = &mut px[y * w + x];
                    let mix =
                        |f: u8, b: u8| (f32::from(f) * a + f32::from(b) * (1.0 - a)).round() as u8;
                    *d = Color32::from_rgb(
                        mix(fg.r(), d.r()),
                        mix(fg.g(), d.g()),
                        mix(fg.b(), d.b()),
                    );
                }
            }
            left += c * ACROSS + 16;
        }
        let mut ppm = format!("P6\n{w} {h}\n255\n").into_bytes();
        ppm.extend(px.iter().flat_map(|c| [c.r(), c.g(), c.b()]));
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join(format!("icons-{name}.ppm")), ppm).unwrap();
    }
    let mut md = String::from(
        "# Icon catalogue (#220)\n\nThe interface's icons (`crates/ui-egui/src/icons.rs`, ADR \
         0012), generated by `UPDATE_PREVIEWS=1 cargo test -p obs-ui-egui \
         icon_catalogue_previews`. Each image shows every icon in the order of the table, 13 to \
         a row, at 16 points on a 1×, a 1.5× and a 2× display (left to right), as rasterized \
         for that scale, in the theme's ink: Light and Dark chrome, and the Sepia page.\n\n\
         ![Light](icons-light.png)\n\n![Dark](icons-dark.png)\n\n![Sepia](icons-sepia.png)\n\n\
         | # | Icon | SVG (Lucide 1.70.0) |\n|---|---|---|\n",
    );
    for (i, icon) in Icon::ALL.iter().enumerate() {
        md.push_str(&format!(
            "| {} | {icon:?} | `{}.svg` |\n",
            i + 1,
            icon.file()
        ));
    }
    std::fs::write(out.join("icons.md"), md).unwrap();
}

// ---------------------------------------------------------------------------------------------
// No emoji in the interface's code.

/// Code points with the Unicode `Emoji` or `Extended_Pictographic` property, outside ASCII
/// (Unicode 16.0 `emoji-data.txt`, adjacent ranges merged), less ©, ® and ™: typographic
/// signs, shown as text by default, which Resources' copyright lines use.
const EMOJI: &[(u32, u32)] = &[
    (0x203C, 0x203C),
    (0x2049, 0x2049),
    (0x2139, 0x2139),
    (0x2194, 0x2199),
    (0x21A9, 0x21AA),
    (0x231A, 0x231B),
    (0x2328, 0x2328),
    (0x2388, 0x2388),
    (0x23CF, 0x23CF),
    (0x23E9, 0x23F3),
    (0x23F8, 0x23FA),
    (0x24C2, 0x24C2),
    (0x25AA, 0x25AB),
    (0x25B6, 0x25B6),
    (0x25C0, 0x25C0),
    (0x25FB, 0x25FE),
    (0x2600, 0x2605),
    (0x2607, 0x2612),
    (0x2614, 0x2685),
    (0x2690, 0x2705),
    (0x2708, 0x2712),
    (0x2714, 0x2714),
    (0x2716, 0x2716),
    (0x271D, 0x271D),
    (0x2721, 0x2721),
    (0x2728, 0x2728),
    (0x2733, 0x2734),
    (0x2744, 0x2744),
    (0x2747, 0x2747),
    (0x274C, 0x274C),
    (0x274E, 0x274E),
    (0x2753, 0x2755),
    (0x2757, 0x2757),
    (0x2763, 0x2767),
    (0x2795, 0x2797),
    (0x27A1, 0x27A1),
    (0x27B0, 0x27B0),
    (0x27BF, 0x27BF),
    (0x2934, 0x2935),
    (0x2B05, 0x2B07),
    (0x2B1B, 0x2B1C),
    (0x2B50, 0x2B50),
    (0x2B55, 0x2B55),
    (0x3030, 0x3030),
    (0x303D, 0x303D),
    (0x3297, 0x3297),
    (0x3299, 0x3299),
    (0x1F000, 0x1F0FF),
    (0x1F10D, 0x1F10F),
    (0x1F12F, 0x1F12F),
    (0x1F16C, 0x1F171),
    (0x1F17E, 0x1F17F),
    (0x1F18E, 0x1F18E),
    (0x1F191, 0x1F19A),
    (0x1F1AD, 0x1F1FF),
    (0x1F201, 0x1F20F),
    (0x1F21A, 0x1F21A),
    (0x1F22F, 0x1F22F),
    (0x1F232, 0x1F23A),
    (0x1F23C, 0x1F23F),
    (0x1F249, 0x1F53D),
    (0x1F546, 0x1F64F),
    (0x1F680, 0x1F6FF),
    (0x1F774, 0x1F77F),
    (0x1F7D5, 0x1F7FF),
    (0x1F80C, 0x1F80F),
    (0x1F848, 0x1F84F),
    (0x1F85A, 0x1F85F),
    (0x1F888, 0x1F88F),
    (0x1F8AE, 0x1F8FF),
    (0x1F90C, 0x1F93A),
    (0x1F93C, 0x1F945),
    (0x1F947, 0x1FAFF),
    (0x1FC00, 0x1FFFD),
];

/// Emoji presentation and keycap selectors, and the joiner of emoji sequences.
const EMOJI_COMPONENTS: &[char] = &['\u{FE0F}', '\u{20E3}', '\u{200D}'];

/// Whole Unicode blocks of pictographs and shapes that only ever stand in for icons here:
/// Miscellaneous Technical, Geometric Shapes, Miscellaneous Symbols, Dingbats and
/// Miscellaneous Symbols and Arrows.
const SYMBOL_BLOCKS: &[(u32, u32)] = &[
    (0x2300, 0x23FF),
    (0x25A0, 0x25FF),
    (0x2600, 0x26FF),
    (0x2700, 0x27BF),
    (0x2B00, 0x2BFF),
];

/// Keyboard symbols the Keyboard shortcuts help writes as text.
const KEY_SYMBOLS: &[char] = &['⌘', '⌥', '⌃'];

/// Signs that are text, wherever they stand: ellipsis, dashes, the middle dot, quotes, and
/// the copyright and trademark signs of Resources' copyright lines.
const TYPOGRAPHIC: &[char] = &['…', '—', '–', '·', '“', '”', '‘', '’', '©', '®', '™'];

fn is_emoji(ch: char) -> bool {
    let c = ch as u32;
    EMOJI_COMPONENTS.contains(&ch) || EMOJI.iter().any(|(a, b)| (*a..=*b).contains(&c))
}

fn is_icon_glyph(ch: char) -> bool {
    let c = ch as u32;
    !KEY_SYMBOLS.contains(&ch) && SYMBOL_BLOCKS.iter().any(|(a, b)| (*a..=*b).contains(&c))
}

/// A symbol standing alone at the start or end of a label, as an icon would ("× Remove",
/// "Factbook ›", "≡"): not a letter, digit or ASCII, and not typographic punctuation.
fn is_loose_symbol(ch: char) -> bool {
    !ch.is_ascii() && !ch.is_alphanumeric() && !TYPOGRAPHIC.contains(&ch) && !ch.is_whitespace()
}

/// What is wrong with a string literal, if anything.
fn literal_problem(text: &str) -> Option<String> {
    if let Some(ch) = text.chars().find(|c| is_emoji(*c)) {
        return Some(format!("emoji U+{:04X} {ch}", ch as u32));
    }
    if let Some(ch) = text.chars().find(|c| is_icon_glyph(*c)) {
        return Some(format!("symbol U+{:04X} {ch} used as an icon", ch as u32));
    }
    let t = text.trim();
    let mut words = t.split_whitespace();
    let first = words.next().unwrap_or("");
    let last = t.split_whitespace().last().unwrap_or("");
    for word in [first, last] {
        let mut chars = word.chars();
        if let (Some(ch), None) = (chars.next(), chars.next())
            && is_loose_symbol(ch)
        {
            return Some(format!("symbol U+{:04X} {ch} used as an icon", ch as u32));
        }
    }
    None
}

/// The string literals of Rust source, with their line numbers (comments skipped; char
/// literals are text processing, not labels, and are skipped too).
fn string_literals(src: &str) -> Vec<(usize, String)> {
    let b: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let (mut i, mut line) = (0usize, 1usize);
    let at = |i: usize, s: &str| s.chars().enumerate().all(|(k, c)| b.get(i + k) == Some(&c));
    while i < b.len() {
        let c = b[i];
        if c == '\n' {
            line += 1;
            i += 1;
        } else if at(i, "//") {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
        } else if at(i, "/*") {
            let mut depth = 0;
            while i < b.len() {
                if at(i, "/*") {
                    depth += 1;
                    i += 2;
                } else if at(i, "*/") {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    if b[i] == '\n' {
                        line += 1;
                    }
                    i += 1;
                }
            }
        } else if c == 'r'
            && (b.get(i + 1) == Some(&'"') || b.get(i + 1) == Some(&'#'))
            && (i == 0 || !(b[i - 1].is_alphanumeric() || b[i - 1] == '_'))
        {
            let mut j = i + 1;
            let mut hashes = 0;
            while b.get(j) == Some(&'#') {
                hashes += 1;
                j += 1;
            }
            if b.get(j) != Some(&'"') {
                i += 1;
                continue;
            }
            let close: String = std::iter::once('"')
                .chain(std::iter::repeat_n('#', hashes))
                .collect();
            let start = j + 1;
            let mut k = start;
            while k < b.len() && !at(k, &close) {
                k += 1;
            }
            let text: String = b[start..k.min(b.len())].iter().collect();
            out.push((line, text.clone()));
            line += text.matches('\n').count();
            i = k + close.chars().count();
        } else if c == '"' {
            let mut k = i + 1;
            let mut text = String::new();
            while k < b.len() && b[k] != '"' {
                if b[k] == '\\' {
                    k += 1;
                }
                if let Some(ch) = b.get(k) {
                    text.push(*ch);
                }
                k += 1;
            }
            out.push((line, text.clone()));
            line += text.matches('\n').count();
            i = k + 1;
        } else if c == '\'' {
            // A char literal ('x', '\n', '\u{..}') or a lifetime ('a).
            if b.get(i + 1) == Some(&'\\') {
                let mut k = i + 2;
                while k < b.len() && b[k] != '\'' {
                    k += 1;
                }
                i = k + 1;
            } else if b.get(i + 2) == Some(&'\'') {
                i += 3;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    out
}

/// The interface's code, without tests: the UI crate, the desktop app and the engine, whose
/// command labels, messages and help the interface shows.
fn ui_sources() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    let mut dirs = vec![
        root.join("crates/ui-egui/src"),
        root.join("apps/openbible/src"),
        root.join("crates/engine/src"),
    ];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                dirs.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                let name = p.file_name().unwrap().to_string_lossy().into_owned();
                if !name.ends_with("_tests.rs") && name != "tests.rs" {
                    files.push(p);
                }
            }
        }
    }
    files.sort();
    files
}

#[test]
fn the_literal_scanner_sees_labels_and_skips_comments() {
    let src = "// ✏ comment\nlet a = \"✏ Note\"; /* 🔎 */ let c = '\"';\nlet b = r#\"× \"x\"\"#;\n\
               let d: &'static str = \"Edit → Undo\";";
    let lits = string_literals(src);
    assert_eq!(
        lits,
        vec![
            (2, "✏ Note".to_string()),
            (3, "× \"x\"".to_string()),
            (4, "Edit → Undo".to_string())
        ]
    );
    assert!(literal_problem("✏ Note").is_some());
    assert!(literal_problem("🔎 Search this").is_some());
    assert!(literal_problem("★ Bible").is_some());
    assert!(literal_problem("☆").is_some());
    assert!(literal_problem("✓ Red letter").is_some());
    assert!(literal_problem("×").is_some());
    assert!(literal_problem("{text}  ×").is_some());
    assert!(literal_problem("≡").is_some());
    assert!(literal_problem("‹").is_some());
    assert!(literal_problem("Factbook ›").is_some());
    assert!(literal_problem("■").is_some());
    assert!(literal_problem("»").is_some());
    assert!(literal_problem("👁").is_some());
    assert!(literal_problem("Edit → Undo brings it back").is_none());
    assert!(literal_problem("{} ×{}").is_none());
    assert!(literal_problem("cited {}×").is_none());
    assert!(literal_problem("Loading…").is_none());
    assert!(literal_problem("{} — {}").is_none());
    assert!(literal_problem("—").is_none());
    assert!(literal_problem(" · ").is_none());
    assert!(literal_problem("“{q}”").is_none());
    assert!(literal_problem("Ctrl is ⌘ on macOS").is_none());
    assert!(literal_problem("Download Starter library (≈{} MB)").is_none());
    assert!(literal_problem("ἀγάπη, אֱלֹהִים").is_none());
}

/// #220 acceptance: no emoji code point (nor a pictograph or shape glyph standing in for an
/// icon) remains in the interface's code; icons come from [`Icon`].
#[test]
fn no_emoji_code_points_remain_in_ui_code() {
    let mut problems = Vec::new();
    for path in ui_sources() {
        let src = std::fs::read_to_string(&path).unwrap();
        for (line, text) in string_literals(&src) {
            if let Some(p) = literal_problem(&text) {
                problems.push(format!("{}:{line}: {p}: {text:?}", path.display()));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
