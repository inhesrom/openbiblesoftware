//! #221 acceptance: no hard-coded spacing, size or colour literal remains in the interface
//! outside the design tokens ([`crate::tokens`]; page colours in [`crate::Palette`]).
//!
//! The scanner reads every non-test source file of the crate with comments, strings and
//! `#[cfg(test)]` items blanked out, and flags:
//!
//! - any numeric literal other than 0 in a call that takes a spacing, a size, a radius, a
//!   stroke width, a font size or a colour (`add_space`, `vec2`, `Margin::*`,
//!   `corner_radius`, `Stroke::new`, `FontId::*`, `.size`, `Color32::from_*`,
//!   `gamma_multiply`, the width and height setters, the painter's rects and circles, and an
//!   assignment to `ui.spacing_mut()`);
//! - any other float literal except the unitless 0, ½, 1 and 2 (halving, doubling, a pixel
//!   nudge), since a bare `rect.left() + 6.0` is a spacing too;
//! - a named `Color32` constant other than `TRANSPARENT` and `PLACEHOLDER`.
//!
//! What is not a measurement of the look (a duration, a zoom range, a contrast ratio, a
//! benchmark window) is listed in [`ALLOWED`] with its reason; each entry must still match.

use std::path::{Path, PathBuf};

/// Files that are the tokens themselves.
const TOKEN_FILES: &[&str] = &["tokens.rs", "palette.rs"];

/// Lines allowed to keep a literal: (file, text on the line, why it is not a token).
const ALLOWED: &[(&str, &str, &str)] = &[
    (
        "shell/floating.rs",
        "const DEFAULT_SIZE",
        "an OS window's size before its Tab's Panel size is known",
    ),
    (
        "shell/floating.rs",
        "const CASCADE",
        "where an OS window opens on the screen",
    ),
    (
        "shell/floating.rs",
        "size.x.min(160.0)",
        "where the pointer grabs a new OS window's title bar",
    ),
    (
        "shell/floating.rs",
        "at.y - 12.0 * k",
        "where the pointer grabs a new OS window's title bar",
    ),
    ("shell/import_tab.rs", "1_000_000.0", "bytes in a megabyte"),
    (
        "shell/tile_handles.rs",
        "const STEP",
        "how far an arrow key moves a Panel separator",
    ),
    (
        "shell/tile_handles.rs",
        "const MIN_PANEL",
        "the narrowest the arrow keys make a Panel",
    ),
    (
        "shell/toolbar_bar.rs",
        "const REVEAL",
        "the pointer's reveal zone at the window edge",
    ),
    (
        "shell/toolbar_bar.rs",
        "const KEEP",
        "the pointer's keep-showing zone round an Auto-hide toolbar",
    ),
    (
        "pane.rs",
        "rect.height() * 0.9",
        "Page Up/Down scroll this share of the pane",
    ),
    (
        "pane.rs",
        "view.scroll_by(40.0)",
        "how far an arrow key scrolls",
    ),
    (
        "pane.rs",
        "view.scroll_by(-40.0)",
        "how far an arrow key scrolls",
    ),
    (
        "pane.rs",
        "abs() > 0.01",
        "a scroll smaller than this is not the user's",
    ),
    (
        "pane.rs",
        "(band[1] * 4.0).round()",
        "a line's key: its top to a quarter point",
    ),
    (
        "pane.rs",
        "past * 0.3",
        "how fast a drag past the pane's edge scrolls",
    ),
    (
        "shell.rs",
        "HOVER_DWELL_SECS: f64",
        "seconds the pointer rests before a hover card",
    ),
    (
        "shell.rs",
        "FLASH_SECS: f64",
        "seconds a jump's flash lasts",
    ),
    ("shell.rs", "self.now - t < 4.0", "seconds a toast shows"),
    (
        "shell/notes_tab.rs",
        "as f32 / 60.0",
        "characters a Note's line holds, for a height estimate",
    ),
    (
        "shell/notes_tab.rs",
        "vec2(width, 10_000.0)",
        "a card may grow as tall as it needs",
    ),
    (
        "shell/toolbar.rs",
        "const DROP_EDGE",
        "the share of a Panel's edge that drops a Resource beside it",
    ),
    (
        "shell/atlas_tab.rs",
        "const MIN_SPAN",
        "degrees of latitude, not points",
    ),
    (
        "shell/atlas_tab.rs",
        "const MINI_MAP_SPAN",
        "degrees of latitude, not points",
    ),
    (
        "shell/atlas_tab.rs",
        "/ 300.0).exp()",
        "how fast the wheel zooms",
    ),
    (
        "shell/atlas_tab.rs",
        "v.scale < 12.0",
        "zoom levels at which smaller places show",
    ),
    (
        "shell/atlas_tab.rs",
        "v.scale < 40.0",
        "zoom levels at which smaller places show",
    ),
    (
        "shell/atlas_tab.rs",
        "view.scale.max(60.0)",
        "the zoom a found place is shown at",
    ),
    (
        "shell/atlas_tab.rs",
        "/ 16.0",
        "segments of a half-filled marker's arc",
    ),
    (
        "shell/atlas_tab.rs",
        "west: 28.0",
        "longitude of the default map",
    ),
    (
        "shell/atlas_tab.rs",
        "south: 27.0",
        "latitude of the default map",
    ),
    (
        "shell/atlas_tab.rs",
        "east: 46.0",
        "longitude of the default map",
    ),
    (
        "shell/atlas_tab.rs",
        "north: 38.0",
        "latitude of the default map",
    ),
    (
        "shell/atlas_tab.rs",
        "* 2.4",
        "the Tab is wide enough for the place list beside the map",
    ),
    (
        "shell/timeline_tab.rs",
        "const FOCUS_YEARS",
        "years, not points",
    ),
    (
        "shell/timeline_tab.rs",
        "const PAN_SHARE",
        "how far Earlier and Later move the view",
    ),
    (
        "shell/timeline_tab.rs",
        "/ 300.0).exp()",
        "how fast the wheel zooms",
    ),
    (
        "shell/toolbar_look.rs",
        "s.max(0.01)",
        "a Panel's share in a thumbnail is never zero",
    ),
    (
        "shell/toolbar_look.rs",
        "share.max(0.01)",
        "a Panel's share in a thumbnail is never zero",
    ),
    (
        "shell/search_views.rs",
        "let width = 2000.0",
        "a layout width meaning do not wrap, not a size on screen",
    ),
    (
        "view.rs",
        "const PREFETCH",
        "how far ahead text is laid out, in viewport heights",
    ),
    (
        "compare_view.rs",
        "width: 400.0",
        "a view's width before its pane first measures it",
    ),
    (
        "compare_view.rs",
        "vec2(400.0, 400.0)",
        "a view's size before its pane first measures it",
    ),
    (
        "compare_view.rs",
        "rect.height() * 0.9",
        "Page Up/Down scroll this share of the pane",
    ),
    (
        "compare_view.rs",
        "abs() > 0.01",
        "a scroll smaller than this is not the user's",
    ),
    (
        "compare_view.rs",
        ".max(10.0)",
        "characters a line holds at the least, for a height estimate",
    ),
    (
        "search_view.rs",
        "width: 400.0",
        "a view's width before its pane first measures it",
    ),
    (
        "search_view.rs",
        "viewport_height: 400.0",
        "a view's height before its pane first measures it",
    ),
    (
        "search_view.rs",
        "rect.height() * 0.9",
        "Page Up/Down scroll this share of the pane",
    ),
    (
        "search_view.rs",
        "(LABEL_SCALE + 3.0)",
        "a Library hit's excerpt is three lines",
    ),
    (
        "view.rs",
        "view.set_width(600.0)",
        "a reading view's width before its pane first measures it",
    ),
];

/// The calls whose numeric arguments are spacings, sizes or colours.
const SINKS: &[&str] = &[
    "add_space",
    "vec2",
    "Vec2::new",
    "Vec2::splat",
    "Margin::same",
    "Margin::symmetric",
    "Margin::new",
    "CornerRadius::same",
    "CornerRadius::new",
    "corner_radius",
    "Stroke::new",
    "FontId::proportional",
    "FontId::monospace",
    "FontId::new",
    "display",
    "size",
    "rgb",
    "Rgb::new",
    "Color32::from_rgb",
    "Color32::from_rgba_premultiplied",
    "Color32::from_rgba_unmultiplied",
    "Color32::from_rgba_unmultiplied_const",
    "Color32::from_gray",
    "Color32::from_black_alpha",
    "Color32::from_white_alpha",
    "gamma_multiply",
    "linear_multiply",
    "lerp_to_gamma",
    "inner_margin",
    "outer_margin",
    "margin",
    "expand",
    "expand2",
    "shrink",
    "shrink2",
    "desired_width",
    "set_width",
    "set_height",
    "set_min_width",
    "set_max_width",
    "set_min_height",
    "set_max_height",
    "set_min_size",
    "set_max_size",
    "min_width",
    "max_width",
    "min_height",
    "max_height",
    "default_width",
    "default_height",
    "default_size",
    "min_size",
    "max_size",
    "fixed_size",
    "exact_width",
    "exact_height",
    "width",
    "spacing",
    "min_row_height",
    "min_col_width",
    "max_col_width",
    "min_scrolled_height",
    "max_scroll_height",
    "add_sized",
    "allocate_exact_size",
    "allocate_space",
    "allocate_at_least",
    "rect_filled",
    "rect_stroke",
    "circle",
    "circle_filled",
    "circle_stroke",
];

/// Sinks that may take 1.0, as the side of a one-point rect.
const POINT_SINKS: &[&str] = &[
    "vec2",
    "allocate_space",
    "allocate_exact_size",
    "expand",
    "shrink",
    "lerp_to_gamma",
];

/// Named colours: a colour role or a token instead.
const NAMED_COLOR_OK: &[&str] = &["TRANSPARENT", "PLACEHOLDER"];

/// The source with comments, string and char literals and `#[cfg(test)]` items replaced by
/// spaces; line breaks kept, so line numbers still match.
pub(crate) fn code_of(src: &str) -> String {
    let b: Vec<char> = src.chars().collect();
    let mut out: Vec<char> = b.clone();
    let blank = |out: &mut Vec<char>, from: usize, to: usize| {
        for c in out.iter_mut().take(to.min(b.len())).skip(from) {
            if *c != '\n' {
                *c = ' ';
            }
        }
    };
    let at = |i: usize, s: &str| s.chars().enumerate().all(|(k, c)| b.get(i + k) == Some(&c));
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if at(i, "//") {
            let s = i;
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
            blank(&mut out, s, i);
        } else if at(i, "/*") {
            let (s, mut depth) = (i, 0);
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
                    i += 1;
                }
            }
            blank(&mut out, s, i);
        } else if c == 'r'
            && matches!(b.get(i + 1), Some('"' | '#'))
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
            let mut k = j + 1;
            while k < b.len() && !at(k, &close) {
                k += 1;
            }
            blank(&mut out, i, k + close.len());
            i = k + close.len();
        } else if c == '"' {
            let s = i;
            i += 1;
            while i < b.len() && b[i] != '"' {
                if b[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            i += 1;
            blank(&mut out, s, i);
        } else if c == '\'' {
            // A char literal ('x', '\n', '\u{..}'), not a lifetime ('a).
            if b.get(i + 1) == Some(&'\\') {
                let s = i;
                i += 2;
                while i < b.len() && b[i] != '\'' {
                    i += 1;
                }
                i += 1;
                blank(&mut out, s, i);
            } else if b.get(i + 2) == Some(&'\'') {
                blank(&mut out, i, i + 3);
                i += 3;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    // `#[cfg(test)]` items: the attribute, any attributes after it, and the item up to its
    // `;` or its closing brace.
    // With strings and comments blanked the code is ASCII, so byte and char offsets agree.
    let code: String = out.iter().collect();
    let chars = out.clone();
    let mut from = 0;
    while let Some(off) = code[from..].find("#[cfg(test)]") {
        let start = from + off;
        let start_c = start;
        let mut k = start_c;
        let mut depth = 0i32;
        let mut seen_brace = false;
        while k < chars.len() {
            match chars[k] {
                '{' => {
                    depth += 1;
                    seen_brace = true;
                }
                '}' => {
                    depth -= 1;
                    if seen_brace && depth == 0 {
                        k += 1;
                        break;
                    }
                }
                ';' if depth == 0 && !seen_brace => {
                    k += 1;
                    break;
                }
                _ => {}
            }
            k += 1;
        }
        for c in out.iter_mut().take(k).skip(start_c) {
            if *c != '\n' {
                *c = ' ';
            }
        }
        from = start + "#[cfg(test)]".len();
    }
    out.into_iter().collect()
}

/// A numeric literal in code: where it starts (char offset in its line), its text
/// and whether it is a float.
#[derive(Debug, Clone, PartialEq)]
struct Num {
    start: usize,
    text: String,
    float: bool,
}

fn numbers(line: &[char]) -> Vec<Num> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < line.len() {
        let c = line[i];
        let prev = if i == 0 { ' ' } else { line[i - 1] };
        if c.is_ascii_digit() && !(prev.is_alphanumeric() || prev == '_' || prev == '.') {
            let s = i;
            while i < line.len() && (line[i].is_ascii_digit() || line[i] == '_') {
                i += 1;
            }
            let mut float = false;
            // `1.0`, but not `1..2` or `x.0.method()`.
            if line.get(i) == Some(&'.') && line.get(i + 1).is_some_and(char::is_ascii_digit) {
                float = true;
                i += 1;
                while i < line.len() && (line[i].is_ascii_digit() || line[i] == '_') {
                    i += 1;
                }
            }
            if line.get(i).is_some_and(|c| *c == 'e' || *c == 'E')
                && line.get(i + 1).is_some_and(char::is_ascii_digit)
            {
                float = true;
                i += 1;
                while i < line.len() && line[i].is_ascii_digit() {
                    i += 1;
                }
            }
            // A type suffix: `2u8`, `1.5f32`.
            let suffix_start = i;
            while i < line.len() && (line[i].is_alphanumeric() || line[i] == '_') {
                i += 1;
            }
            let suffix: String = line[suffix_start..i].iter().collect();
            if suffix.starts_with('f') {
                float = true;
            }
            out.push(Num {
                start: s,
                text: line[s..suffix_start].iter().collect(),
                float,
            });
        } else {
            i += 1;
        }
    }
    out
}

fn value(n: &Num) -> f64 {
    n.text.replace('_', "").parse().unwrap_or(f64::NAN)
}

/// The spans (char offsets, line-local, end exclusive; a span may run onto later lines) of
/// every sink call's arguments in `code`.
fn sink_spans(code: &[char]) -> Vec<(usize, usize, String)> {
    let mut spans = Vec::new();
    let text: String = code.iter().collect();
    debug_assert_eq!(text.len(), code.len());
    for sink in SINKS {
        let mut from = 0;
        while let Some(off) = text[from..].find(sink) {
            let s = from + off;
            from = s + sink.len();
            let (s_c, e_c) = (s, s + sink.len());
            let before = if s_c == 0 { ' ' } else { code[s_c - 1] };
            if before.is_alphanumeric() || before == '_' {
                continue;
            }
            if code
                .get(e_c)
                .is_some_and(|c| c.is_alphanumeric() || *c == '_')
            {
                continue;
            }
            // Optional turbofish, then the opening paren.
            let mut k = e_c;
            if code.get(k) == Some(&':') && code.get(k + 1) == Some(&':') {
                continue;
            }
            while code.get(k).is_some_and(|c| *c == ' ') {
                k += 1;
            }
            if code.get(k) != Some(&'(') {
                continue;
            }
            let open = k;
            let mut depth = 0;
            while k < code.len() {
                match code[k] {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                k += 1;
            }
            spans.push((open, k, (*sink).to_string()));
        }
    }
    // `ui.spacing_mut().item_spacing.x = 4.0;`: the right-hand side.
    let mut from = 0;
    while let Some(off) = text[from..].find("spacing_mut()") {
        let s = from + off;
        from = s + 1;
        let mut k = s;
        while k < code.len() && code[k] != ';' && code[k] != '=' && code[k] != '\n' {
            k += 1;
        }
        if code.get(k) == Some(&'=') && code.get(k + 1) != Some(&'=') {
            let open = k;
            while k < code.len() && code[k] != ';' {
                k += 1;
            }
            spans.push((open, k, "spacing_mut()".to_string()));
        }
    }
    spans
}

/// Every literal the rules flag in `src`, as (line, reason).
pub(crate) fn violations(src: &str) -> Vec<(usize, String)> {
    let code: Vec<char> = code_of(src).chars().collect();
    let spans = sink_spans(&code);
    // Line starts, char-indexed.
    let mut starts = vec![0];
    for (i, c) in code.iter().enumerate() {
        if *c == '\n' {
            starts.push(i + 1);
        }
    }
    if starts.last() != Some(&code.len()) {
        starts.push(code.len());
    }
    let mut out: Vec<(usize, String)> = Vec::new();
    for (n, w) in starts.windows(2).map(|w| (w[0], w[1])).enumerate() {
        let (s, e) = w;
        let line = &code[s..e];
        for num in numbers(line) {
            let at = s + num.start;
            let v = value(&num);
            // `size / 2.0`, `x * 0.5`: halving and doubling are not measurements.
            let after = line[num.start..]
                .iter()
                .position(|c| !(c.is_alphanumeric() || *c == '.' || *c == '_'))
                .map_or(line.len(), |k| num.start + k);
            let operator = line[..num.start]
                .iter()
                .rev()
                .find(|c| **c != ' ')
                .is_some_and(|c| *c == '*' || *c == '/')
                || line[after..]
                    .iter()
                    .find(|c| **c != ' ')
                    .is_some_and(|c| *c == '*' || *c == '/');
            if let Some((_, _, sink)) = spans.iter().find(|(o, c, _)| *o < at && at < *c) {
                // A one-point rect (a pointer's position, a hairline's length).
                let point = v == 1.0 && POINT_SINKS.contains(&sink.as_str());
                let unitless =
                    (operator && [0.5, 2.0].contains(&v)) || (!num.float && v == 1.0) || point;
                if v != 0.0 && !unitless {
                    out.push((n + 1, format!("{} in {sink}(…)", num.text)));
                }
            } else if num.float && ![0.0, 0.5, 1.0, 2.0].contains(&v) {
                out.push((n + 1, format!("float literal {}", num.text)));
            }
        }
        let text: String = line.iter().collect();
        let mut from = 0;
        while let Some(off) = text[from..].find("Color32::") {
            let name: String = text[from + off + "Color32::".len()..]
                .chars()
                .take_while(|c| c.is_ascii_uppercase() || *c == '_')
                .collect();
            if name.len() > 1 && !NAMED_COLOR_OK.contains(&name.as_str()) {
                out.push((n + 1, format!("named colour Color32::{name}")));
            }
            from += off + "Color32::".len();
        }
    }
    out.dedup();
    out
}

fn ui_sources() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    let mut dirs = vec![root];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                // The benchmark harness drives headless windows; it draws no interface.
                if !p.ends_with("bench") {
                    dirs.push(p);
                }
            } else if p.extension().is_some_and(|x| x == "rs") {
                let name = p.file_name().unwrap().to_string_lossy().into_owned();
                if !name.ends_with("_tests.rs")
                    && name != "tests.rs"
                    && name != "bench.rs"
                    && !TOKEN_FILES.contains(&name.as_str())
                {
                    files.push(p);
                }
            }
        }
    }
    files.sort();
    files
}

fn relative(p: &Path) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    p.strip_prefix(&root)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

#[test]
fn no_spacing_size_or_colour_literals_outside_the_tokens() {
    let mut problems = Vec::new();
    let mut used = vec![false; ALLOWED.len()];
    for path in ui_sources() {
        let src = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = src.lines().collect();
        let file = relative(&path);
        for (line, why) in violations(&src) {
            let text = lines.get(line - 1).copied().unwrap_or("");
            let allowed = ALLOWED
                .iter()
                .position(|(f, snippet, _)| *f == file && text.contains(snippet));
            match allowed {
                Some(i) => used[i] = true,
                None => problems.push(format!("{file}:{line}: {why}: {}", text.trim())),
            }
        }
    }
    for (i, (f, snippet, _)) in ALLOWED.iter().enumerate() {
        if !used[i] {
            problems.push(format!("stale allowlist entry {f}: {snippet:?}"));
        }
    }
    assert!(
        problems.is_empty(),
        "{} literal(s) outside the design tokens (use crate::tokens):\n{}",
        problems.len(),
        problems.join("\n")
    );
}

#[test]
fn the_literal_lint_sees_sizes_and_colours_and_skips_comments_strings_and_tests() {
    let src = "\
// add_space(6.0) in a comment
let a = \"vec2(12.0, 3.0)\";
ui.add_space(6.0);
ui.add_space(space::M);
let r = rect.left() + 6.0;
let h = rect.height() / 2.0 + 1.0;
let m = egui::Margin::symmetric(14, 10);
let c = Color32::from_rgb(10, 20, 30);
let w = Color32::WHITE;
let t = Color32::TRANSPARENT;
ui.spacing_mut().item_spacing.x = 2.0;
p.rect_filled(r, 4, fill);
let z = vec2(0.0, 0.0);
let tuple = pair.0 + pair.1;
let half = Vec2::splat(size / 2.0) + vec2(w * 0.5, n.saturating_sub(1));
#[cfg(test)]
mod tests {
    fn f() { ui.add_space(6.0); }
}
";
    let lines: Vec<usize> = violations(src).into_iter().map(|(l, _)| l).collect();
    assert_eq!(lines, vec![3, 5, 7, 7, 8, 8, 8, 9, 11, 12]);
}

/// The scales grow step by step, so a surface that wants "a bit more room" has one.
#[test]
fn the_scales_are_ordered() {
    use crate::tokens::{radius, size, space, stroke, text};
    let ordered = |v: &[f32]| v.windows(2).all(|w| w[0] < w[1]);
    assert!(ordered(&[
        space::XXS,
        space::XS,
        space::S,
        space::M,
        space::L,
        space::XL,
        space::XXL,
        space::XXXL
    ]));
    assert!(ordered(&[
        text::CAPTION,
        text::SMALL,
        text::LABEL,
        text::BODY,
        text::SUBTITLE,
        text::TITLE,
        text::DISPLAY
    ]));
    assert!(ordered(&[radius::S, radius::M, radius::L, radius::XL]));
    assert!(ordered(&[stroke::HAIR, stroke::MEDIUM, stroke::THICK]));
    assert!(ordered(&[size::ICON_SMALL, size::ICON, size::ICON_LARGE]));
}
