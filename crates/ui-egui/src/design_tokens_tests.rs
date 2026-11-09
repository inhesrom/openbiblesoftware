//! #221 acceptance: no hard-coded spacing, size or colour literal remains in the interface
//! outside the design tokens ([`crate::tokens`]; page colours in [`crate::Palette`]).
//!
//! The scanner reads every non-test source file of the crate with comments, strings and
//! `#[cfg(test)]` items blanked out, and flags:
//!
//! - any numeric literal in a call that takes a spacing, a size, a radius, a stroke width, a
//!   font size or a colour (`add_space`, `vec2`, `Margin::*`, `corner_radius`,
//!   `Stroke::new`, `FontId::*`, `.size`, `Color32::from_*`, `gamma_multiply`, `expand`,
//!   `shrink`, the width and height setters, the painter's rects and circles, and an
//!   assignment to `ui.spacing_mut()`);
//! - any float literal anywhere else, since a bare `rect.left() + 6.0` is a spacing too;
//! - a named `Color32` constant other than `TRANSPARENT` and `PLACEHOLDER`.
//!
//! Exempt by rule are only the literals that cannot be a measurement: 0, halving or
//! doubling a length (`w / 2.0`, `h * 0.5`, `2.0 * MARGIN`), the rest of a fraction
//! (`1.0 - t`) and, inside a call, an integer 1 that counts (`n.saturating_sub(1)`).
//! Everything else that is not a measurement of the look (a duration, a zoom factor, a
//! tolerance, a clamp bound, a default before the first layout) is an exact [`ALLOWED`]
//! entry — file, whole line and literal — with its reason; each entry must still match.

use std::path::{Path, PathBuf};

/// Files that are the tokens themselves.
const TOKEN_FILES: &[&str] = &["tokens.rs", "palette.rs"];

/// Lines allowed to keep a literal, each exactly: (file, the whole line, the literal, why it
/// is not a measurement of the look).
const ALLOWED: &[(&str, &str, &str, &str)] = &[
    (
        "compare_view.rs",
        "width: 400.0,",
        "400.0",
        "a view's width before its pane first measures it",
    ),
    (
        "compare_view.rs",
        "viewport: Rect::from_min_size(Pos2::ZERO, vec2(400.0, 400.0)),",
        "400.0",
        "a view's size before its pane first measures it",
    ),
    (
        "compare_view.rs",
        "if (width - self.width).abs() > 0.5 {",
        "0.5",
        "a tolerance: a width change smaller than this is not a resize",
    ),
    (
        "compare_view.rs",
        "let per_line = (self.text_width() / (self.settings.size * 0.5)).max(10.0);",
        "10.0",
        "characters a line holds at the least, for a height estimate",
    ),
    (
        "compare_view.rs",
        "_ => 1.0,",
        "1.0",
        "a row's height estimate in lines for a cell with no text",
    ),
    (
        "compare_view.rs",
        "lines.max(1.0) * self.line_height() + ROW_GAP",
        "1.0",
        "a block's height estimate is at least one line",
    ),
    (
        "compare_view.rs",
        "(Some(r), true) if (r[2] - x0).abs() < 0.5 || (r[0] - x1).abs() < 0.5 => {",
        "0.5",
        "a tolerance: selection rects that touch join",
    ),
    (
        "compare_view.rs",
        "view.scroll_by(rect.height() * 0.9);",
        "0.9",
        "Page Down scrolls this share of the pane",
    ),
    (
        "compare_view.rs",
        "view.scroll_by(-rect.height() * 0.9);",
        "0.9",
        "Page Up scrolls this share of the pane",
    ),
    (
        "compare_view.rs",
        "let frac = f64::from(((p.y - track.top()) / track.height()).clamp(0.0, 1.0));",
        "1.0",
        "the scrollbar's track as a fraction, 0 to 1",
    ),
    (
        "compare_view.rs",
        "out.user_moved = (view.position().0 - before).abs() > 0.01;",
        "0.01",
        "a scroll smaller than this is not the user's",
    ),
    (
        "components.rs",
        "let top = track.top() + frac.clamp(0.0, 1.0) * (track.height() - h);",
        "1.0",
        "the scrollbar thumb's place as a fraction, 0 to 1",
    ),
    (
        "icons.rs",
        "(size * ctx.pixels_per_point()).round().max(1.0) as u32",
        "1.0",
        "an icon texture is at least one pixel",
    ),
    (
        "icons.rs",
        "Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),",
        "1.0",
        "the whole icon texture in UV coordinates",
    ),
    (
        "key_labels.rs",
        "let side = (font_size_px * 0.92).round().max(4.0);",
        "0.92",
        "a modifier glyph's square as a share of the em, to stand as tall as a capital",
    ),
    (
        "key_labels.rs",
        "let side = (font_size_px * 0.92).round().max(4.0);",
        "4.0",
        "a modifier glyph's bitmap is at least this many pixels, to stay legible",
    ),
    (
        "key_labels.rs",
        "let unit = side / 24.0;",
        "24.0",
        "the icon drawing's 24-unit grid",
    ),
    (
        "key_labels.rs",
        "let bearing = (font_size_px * 0.06).round();",
        "0.06",
        "a modifier glyph's side bearing as a share of the em, like a font's",
    ),
    (
        "key_labels.rs",
        "offset_px: egui::vec2(bearing - (3.0 * unit).round(), -(21.0 * unit).round()),",
        "3.0",
        "the drawing's left edge on the icon's 24-unit grid",
    ),
    (
        "key_labels.rs",
        "offset_px: egui::vec2(bearing - (3.0 * unit).round(), -(21.0 * unit).round()),",
        "21.0",
        "the drawing's baseline on the icon's 24-unit grid",
    ),
    (
        "key_labels.rs",
        "advance_px: (18.0 * unit).round() + 2.0 * bearing,",
        "18.0",
        "the drawing's width on the icon's 24-unit grid",
    ),
    (
        "pane.rs",
        "let page = rect.height() * 0.9;",
        "0.9",
        "Page Up/Down scroll this share of the pane",
    ),
    (
        "pane.rs",
        "view.scroll_by(40.0);",
        "40.0",
        "how far an arrow key scrolls",
    ),
    (
        "pane.rs",
        "view.scroll_by(-40.0);",
        "40.0",
        "how far an arrow key scrolls",
    ),
    (
        "pane.rs",
        "let frac = f64::from(((p.y - track.top()) / track.height()).clamp(0.0, 1.0));",
        "1.0",
        "the scrollbar's track as a fraction, 0 to 1",
    ),
    (
        "pane.rs",
        "out.user_moved = (view.position().0 - before).abs() > 0.01;",
        "0.01",
        "a scroll smaller than this is not the user's",
    ),
    (
        "pane.rs",
        "let used = beside.entry((band[1] * 4.0).round() as i64).or_insert(0.0);",
        "4.0",
        "a line's key: its top to a quarter point",
    ),
    (
        "pane.rs",
        "let y = p.y.clamp(rect.top(), rect.bottom() - 1.0) - rect.top();",
        "1.0",
        "a hit test: the pointer kept inside the pane's last row",
    ),
    (
        "pane.rs",
        "&& (last[1] - y0).abs() < 0.5",
        "0.5",
        "a tolerance: selection rects on one line join",
    ),
    (
        "pane.rs",
        "&& (last[3] - y1).abs() < 0.5",
        "0.5",
        "a tolerance: selection rects on one line join",
    ),
    (
        "pane.rs",
        "view.scroll_by(past * 0.3);",
        "0.3",
        "how fast a drag past the pane's edge scrolls",
    ),
    (
        "search_view.rs",
        "width: 400.0,",
        "400.0",
        "a view's width before its pane first measures it",
    ),
    (
        "search_view.rs",
        "viewport_height: 400.0,",
        "400.0",
        "a view's height before its pane first measures it",
    ),
    (
        "search_view.rs",
        "head + self.line_height() * (LABEL_SCALE + 3.0) + HIT_GAP + ROW_GAP",
        "3.0",
        "a Library hit's excerpt is three lines",
    ),
    (
        "search_view.rs",
        "if (width - self.width).abs() > 0.5 {",
        "0.5",
        "a tolerance: a width change smaller than this is not a resize",
    ),
    (
        "search_view.rs",
        "view.scroll_by(rect.height() * 0.9);",
        "0.9",
        "Page Down scrolls this share of the pane",
    ),
    (
        "search_view.rs",
        "view.scroll_by(-rect.height() * 0.9);",
        "0.9",
        "Page Up scrolls this share of the pane",
    ),
    (
        "search_view.rs",
        "let frac = f64::from(((p.y - track.top()) / track.height()).clamp(0.0, 1.0));",
        "1.0",
        "the scrollbar's track as a fraction, 0 to 1",
    ),
    (
        "search_view.rs",
        "let max = view.max_scroll().max(1.0);",
        "1.0",
        "the scroll range is never zero, so the thumb's fraction is defined",
    ),
    (
        "selection.rs",
        "(Some(r), true) if (r[2] - x0).abs() < 0.5 || (r[0] - x1).abs() < 0.5 => {",
        "0.5",
        "a tolerance: selection rects that touch join",
    ),
    (
        "shell/annotate.rs",
        "vec2(1.0, size::CONTROL_COMPACT),",
        "1.0",
        "a rect that anchors the popup Note editor; its width is not drawn",
    ),
    (
        "shell/atlas_tab.rs",
        "const MIN_SPAN: f64 = 3.0;",
        "3.0",
        "degrees of latitude, not points",
    ),
    (
        "shell/atlas_tab.rs",
        "pub(super) const MINI_MAP_SPAN: f64 = 7.0;",
        "7.0",
        "degrees of latitude, not points",
    ),
    (
        "shell/atlas_tab.rs",
        "let factor = f64::from(pinch) * (f64::from(scroll) / 300.0).exp();",
        "300.0",
        "how fast the wheel zooms",
    ),
    (
        "shell/atlas_tab.rs",
        "if (factor - 1.0).abs() > 1e-6",
        "1.0",
        "a zoom factor's distance from no zoom, not points",
    ),
    (
        "shell/atlas_tab.rs",
        "view.zoom_about(2.0, f64::from(local.x), f64::from(local.y), size, world);",
        "2.0",
        "the zoom factor of a double-click",
    ),
    (
        "shell/atlas_tab.rs",
        "let max_rank = if v.scale < 12.0 {",
        "12.0",
        "zoom levels at which smaller places show",
    ),
    (
        "shell/atlas_tab.rs",
        "} else if v.scale < 40.0 {",
        "40.0",
        "zoom levels at which smaller places show",
    ),
    (
        "shell/atlas_tab.rs",
        "let a = std::f32::consts::PI * f32::from(i) / 16.0;",
        "16.0",
        "segments of a half-filled marker's arc",
    ),
    (
        "shell/atlas_tab.rs",
        "west: 28.0,",
        "28.0",
        "longitude of the default map",
    ),
    (
        "shell/atlas_tab.rs",
        "south: 27.0,",
        "27.0",
        "latitude of the default map",
    ),
    (
        "shell/atlas_tab.rs",
        "east: 46.0,",
        "46.0",
        "longitude of the default map",
    ),
    (
        "shell/atlas_tab.rs",
        "north: 38.0,",
        "38.0",
        "latitude of the default map",
    ),
    (
        "shell/atlas_tab.rs",
        "zoom = Some(0.5);",
        "0.5",
        "the zoom factor of Zoom out",
    ),
    (
        "shell/atlas_tab.rs",
        "zoom = Some(2.0);",
        "2.0",
        "the zoom factor of Zoom in",
    ),
    (
        "shell/atlas_tab.rs",
        "view.scale = view.scale.max(60.0);",
        "60.0",
        "the zoom a found place is shown at",
    ),
    (
        "shell/atlas_tab.rs",
        "let (map_rect, list_rect) = if area.width() >= LIST_WIDTH * 2.4 {",
        "2.4",
        "the Tab is wide enough for the place list beside the map",
    ),
    (
        "shell/contents_pane.rs",
        "if open { 1.0 } else { 0.0 },",
        "1.0",
        "the Contents pane's open animation target, a fraction",
    ),
    (
        "shell/context_menu.rs",
        ".map(|p| Rect::from_min_size(p, vec2(1.0, 1.0)));",
        "1.0",
        "a rect at the pointer that anchors a popup; not drawn",
    ),
    (
        "shell/context_menu.rs",
        "vec2(1.0, size::CONTROL_COMPACT),",
        "1.0",
        "a rect that anchors the popup Note editor; its width is not drawn",
    ),
    (
        "shell/floating.rs",
        "const DEFAULT_SIZE: (f32, f32) = (640.0, 720.0);",
        "640.0",
        "an OS window's size before its Tab's Panel size is known",
    ),
    (
        "shell/floating.rs",
        "const DEFAULT_SIZE: (f32, f32) = (640.0, 720.0);",
        "720.0",
        "an OS window's size before its Tab's Panel size is known",
    ),
    (
        "shell/floating.rs",
        "const CASCADE: f32 = 48.0;",
        "48.0",
        "where an OS window opens on the screen",
    ),
    (
        "shell/floating.rs",
        "const SCREENS_EVERY_SECS: f64 = 2.0;",
        "2.0",
        "seconds between checks of the screens' sizes",
    ),
    (
        "shell/floating.rs",
        "Self::Pixels => 1.0,",
        "1.0",
        "pixels per pixel: no scale",
    ),
    (
        "shell/floating.rs",
        "Self::Points => 1.0 / scale,",
        "1.0",
        "points per pixel: the reciprocal of the scale",
    ),
    (
        "shell/floating.rs",
        ".unwrap_or(1.0)",
        "1.0",
        "a screen's scale when the OS reports none",
    ),
    (
        "shell/floating.rs",
        ".map_or((pos2(0.0, 0.0), 1.0), |(r, k)| (r.min, k));",
        "1.0",
        "a screen's scale when the OS reports none",
    ),
    (
        "shell/floating.rs",
        "x: at.x - size.x.min(160.0) / 2.0 * k,",
        "160.0",
        "where the pointer grabs a new OS window's title bar",
    ),
    (
        "shell/floating.rs",
        "y: at.y - 12.0 * k,",
        "12.0",
        "where the pointer grabs a new OS window's title bar",
    ),
    (
        "shell/floating.rs",
        "&& (want.x - at.x).abs().max((want.y - at.y).abs()) > 1.0",
        "1.0",
        "a tolerance: an OS window moved by less than a pixel has not moved",
    ),
    (
        "shell/get_bibles_tab.rs",
        "_ => (1.0, \"Importing…\".to_string()),",
        "1.0",
        "the progress fraction of an import with no byte count",
    ),
    (
        "shell/import_tab.rs",
        "format!(\"{:.1} MB\", bytes as f64 / 1_000_000.0)",
        "1_000_000.0",
        "bytes in a megabyte",
    ),
    (
        "shell/note_editor.rs",
        "let size = body.size * scale::HEADINGS.get(usize::from(n) - 1).unwrap_or(&1.0);",
        "1.0",
        "a heading's text scale when it has no step: the body size",
    ),
    (
        "shell/notes_tab.rs",
        "pub(crate) const AUTOSAVE_SECS: f64 = 1.0;",
        "1.0",
        "seconds between Note autosaves",
    ),
    (
        "shell/notes_tab.rs",
        "let lines = 2.0 + card.annotation.body().len() as f32 / 60.0;",
        "2.0",
        "a Note card's height estimate: two lines beside its text",
    ),
    (
        "shell/notes_tab.rs",
        "let lines = 2.0 + card.annotation.body().len() as f32 / 60.0;",
        "60.0",
        "characters a Note's line holds, for a height estimate",
    ),
    (
        "shell/notes_tab.rs",
        "let rect = Rect::from_min_size(origin + vec2(0.0, top), vec2(width, 10_000.0));",
        "10_000.0",
        "a card may grow as tall as it needs",
    ),
    (
        "shell/notes_tab.rs",
        "NoteLineKind::Text => (1.0, ColorRole::Ink, None, 0.0),",
        "1.0",
        "a Note line's text scale: the body size",
    ),
    (
        "shell/notes_tab.rs",
        "let scale = scale::HEADINGS.get(usize::from(n) - 1).unwrap_or(&1.0);",
        "1.0",
        "a heading's text scale when it has no step: the body size",
    ),
    (
        "shell/notes_tab.rs",
        "1.0,",
        "1.0",
        "a Note line's text scale: the body size",
    ),
    (
        "shell/notes_tab.rs",
        "NoteLineKind::Quote => (1.0, ColorRole::Ink, None, space::L),",
        "1.0",
        "a Note line's text scale: the body size",
    ),
    (
        "shell/search_views.rs",
        "let width = 2000.0;",
        "2000.0",
        "a layout width meaning do not wrap, not a size on screen",
    ),
    (
        "shell/tile_handles.rs",
        "const STEP: f32 = 24.0;",
        "24.0",
        "how far an arrow key moves a Panel separator",
    ),
    (
        "shell/tile_handles.rs",
        "const MIN_PANEL: f32 = 120.0;",
        "120.0",
        "the narrowest the arrow keys make a Panel",
    ),
    (
        "shell/tile_handles.rs",
        "let grab = 2.0 * ui.style().interaction.resize_grab_radius_side + 0.5;",
        "0.5",
        "a tolerance: how wide a widget may be and still be a separator",
    ),
    (
        "shell/tile_handles.rs",
        "&& (rect.center().x - (a.right() + b.left()) / 2.0).abs() < 1.0",
        "1.0",
        "a tolerance: a widget centred on the gap is its separator",
    ),
    (
        "shell/tile_handles.rs",
        "&& (rect.center().y - (a.bottom() + b.top()) / 2.0).abs() < 1.0",
        "1.0",
        "a tolerance: a widget centred on the gap is its separator",
    ),
    (
        "shell/tile_handles.rs",
        "if !is_tab && (rect.min - r.min).length() < 1.0 && rect.height() < r.height() {",
        "1.0",
        "a tolerance: a widget at the Panel's corner is its tab bar",
    ),
    (
        "shell/timeline_tab.rs",
        "const FOCUS_YEARS: f64 = 160.0;",
        "160.0",
        "years, not points",
    ),
    (
        "shell/timeline_tab.rs",
        "const PAN_SHARE: f64 = 1.0 / 3.0;",
        "1.0",
        "how far Earlier and Later move the view: a third of it",
    ),
    (
        "shell/timeline_tab.rs",
        "const PAN_SHARE: f64 = 1.0 / 3.0;",
        "3.0",
        "how far Earlier and Later move the view: a third of it",
    ),
    (
        "shell/timeline_tab.rs",
        "zoom = Some(0.5);",
        "0.5",
        "the zoom factor of Zoom out",
    ),
    (
        "shell/timeline_tab.rs",
        "zoom = Some(2.0);",
        "2.0",
        "the zoom factor of Zoom in",
    ),
    (
        "shell/timeline_tab.rs",
        "let factor = f64::from(pinch) * (f64::from(scroll.y) / 300.0).exp();",
        "300.0",
        "how fast the wheel zooms",
    ),
    (
        "shell/timeline_tab.rs",
        "if (factor - 1.0).abs() > 1e-6 {",
        "1.0",
        "a zoom factor's distance from no zoom, not points",
    ),
    (
        "shell/toolbar.rs",
        "pub(super) const DROP_EDGE: f32 = 0.25;",
        "0.25",
        "the share of a Panel's edge that drops a Resource beside it",
    ),
    (
        "shell/toolbar_bar.rs",
        "pub(crate) const REVEAL: f32 = 6.0;",
        "6.0",
        "the pointer's reveal zone at the window edge",
    ),
    (
        "shell/toolbar_bar.rs",
        "const KEEP: f32 = 12.0;",
        "12.0",
        "the pointer's keep-showing zone round an Auto-hide toolbar",
    ),
    (
        "shell/toolbar_look.rs",
        "for j in [-1.0, 1.0] {",
        "1.0",
        "which side of the grip's centre line a dot sits, a sign",
    ),
    (
        "shell/toolbar_look.rs",
        "let total: f32 = children.iter().map(|(_, s)| s.max(0.01)).sum();",
        "0.01",
        "a Panel's share in a thumbnail is never zero",
    ),
    (
        "shell/toolbar_look.rs",
        "let size = room * share.max(0.01) / total;",
        "0.01",
        "a Panel's share in a thumbnail is never zero",
    ),
    (
        "shell/toolbar_look.rs",
        "job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(1.0));",
        "1.0",
        "a truncation width is never zero",
    ),
    (
        "shell.rs",
        "pub(crate) const HOVER_DWELL_SECS: f64 = 0.15;",
        "0.15",
        "seconds the pointer rests before a hover card",
    ),
    (
        "shell.rs",
        "const FLASH_SECS: f64 = 1.2;",
        "1.2",
        "seconds a jump's flash lasts",
    ),
    (
        "shell.rs",
        "l.shares.set_share(a, 2.0);",
        "2.0",
        "the shares of a two-to-one split",
    ),
    (
        "shell.rs",
        "l.shares.set_share(b, 1.0);",
        "1.0",
        "the shares of a two-to-one split",
    ),
    (
        "shell.rs",
        "if !now && since < 1.0 {",
        "1.0",
        "seconds between saves of the Workspace",
    ),
    (
        "shell.rs",
        ".filter_map(|c| Some((dock_node(tree, panels, *c)?, 1.0)))",
        "1.0",
        "a docked Panel's share: all equal",
    ),
    (
        "shell.rs",
        "let a = (1.0 - age / FLASH_SECS).clamp(0.0, 1.0) as f32;",
        "1.0",
        "the flash's fading strength, 0 to 1",
    ),
    (
        "shell.rs",
        "&& (t.header_controls - measured).abs() > 0.5",
        "0.5",
        "a tolerance: a header width change smaller than this is not a resize",
    ),
    (
        "shell.rs",
        "if self.now - t < 4.0 {",
        "4.0",
        "seconds a toast shows",
    ),
    (
        "view.rs",
        "const PREFETCH: f32 = 1.5;",
        "1.5",
        "how far ahead text is laid out, in viewport heights",
    ),
    (
        "view.rs",
        "view.set_width(600.0);",
        "600.0",
        "a reading view's width before its pane first measures it",
    ),
    (
        "view.rs",
        "if (width - self.width).abs() < 0.5 && !self.heights.is_empty() {",
        "0.5",
        "a tolerance: a width change smaller than this is not a resize",
    ),
    (
        "view.rs",
        "for (index, top, laid) in self.laid_in_view(y + 1.0) {",
        "1.0",
        "a hit test: the blocks laid out round the point",
    ),
    (
        "view.rs",
        "for (block, top, laid) in self.laid_in_view(y + 1.0) {",
        "1.0",
        "a hit test: the blocks laid out round the point",
    ),
    (
        "view.rs",
        "while top < height * (1.0 + PREFETCH) && below < n {",
        "1.0",
        "how far ahead text is laid out: the viewport and the prefetch",
    ),
    (
        "view.rs",
        "if let Some(line) = laid.lines.iter().find(|l| l.bottom > offset + 0.5) {",
        "0.5",
        "a tolerance: the first line not scrolled past",
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

/// Every literal the rules flag in `src`, as (line, literal, reason).
pub(crate) fn violations(src: &str) -> Vec<(usize, String, String)> {
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
    let mut out: Vec<(usize, String, String)> = Vec::new();
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
            let before = line[..num.start].iter().rev().find(|c| **c != ' ');
            let next = line[after..].iter().find(|c| **c != ' ');
            let operator = before.is_some_and(|c| *c == '*' || *c == '/')
                || next.is_some_and(|c| *c == '*' || *c == '/');
            // `1.0 - t`: the rest of a fraction, unless it is itself taken from something
            // (`x - 1.0 - t`).
            let fraction =
                v == 1.0 && next.is_some_and(|c| *c == '-') && !additive_before(&line[..num.start]);
            if let Some((open, _, sink)) = spans.iter().find(|(o, c, _)| *o < at && at < *c) {
                // Inside a sink an integer 1 that counts (`level.saturating_sub(1)`,
                // `n - 1`) is unitless too; as a whole argument it is a size
                // (`Margin::same(1)`).
                let count = !num.float && v == 1.0 && !whole_argument(&code, *open, at, s + after);
                if !(unitless(v, operator, fraction) || count) {
                    out.push((
                        n + 1,
                        num.text.clone(),
                        format!("{} in {sink}(…)", num.text),
                    ));
                }
            } else if num.float && !unitless(v, operator, fraction) {
                out.push((
                    n + 1,
                    num.text.clone(),
                    format!("float literal {}", num.text),
                ));
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
                out.push((n + 1, name.clone(), format!("named colour Color32::{name}")));
            }
            from += off + "Color32::".len();
        }
    }
    out.dedup();
    out
}

/// Whether the literal from `at` to `end` (char offsets in `code`) is a whole argument of
/// the sink call whose `(` is at `open`: directly inside its parentheses, between `(` or `,`
/// and `,` or `)`.
fn whole_argument(code: &[char], open: usize, at: usize, end: usize) -> bool {
    let depth = code[open..at].iter().fold(0i32, |d, c| match c {
        '(' | '[' => d + 1,
        ')' | ']' => d - 1,
        _ => d,
    });
    let before = code[..at].iter().rev().find(|c| !c.is_whitespace());
    let next = code[end..].iter().find(|c| !c.is_whitespace());
    depth == 1 && matches!(before, Some('(' | ',')) && matches!(next, Some(',' | ')'))
}

/// Whether the code before a literal ends in a binary `+` or `-` (or `+=`, `-=`): the
/// literal is added to a coordinate or a length.
fn additive_before(before: &[char]) -> bool {
    let mut rest = before.iter().rev().filter(|c| **c != ' ');
    match rest.next() {
        Some('+') => true,
        Some('-') => rest
            .next()
            .is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | ')' | ']')),
        Some('=') => matches!(rest.next(), Some('+' | '-')),
        _ => false,
    }
}

/// The only literals that are never a measurement, wherever they stand: 0, halving or
/// doubling a length (`w / 2.0`, `h * 0.5`, `2.0 * MARGIN`) and the rest of a fraction
/// (`1.0 - t`). Every other literal — a bound, a tolerance, a zoom factor, a 1 or a 2
/// added to a coordinate — is a token or an exact [`ALLOWED`] entry.
fn unitless(v: f64, operator: bool, fraction: bool) -> bool {
    v == 0.0 || (operator && (v == 0.5 || v == 2.0)) || fraction
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

/// The [`ALLOWED`] entry for `literal` on the line `text` of `file`, if any.
fn allowed_entry(file: &str, text: &str, literal: &str) -> Option<usize> {
    ALLOWED
        .iter()
        .position(|(f, whole, value, _)| *f == file && *value == literal && text.trim() == *whole)
}

#[test]
fn no_spacing_size_or_colour_literals_outside_the_tokens() {
    let mut problems = Vec::new();
    let mut used = vec![false; ALLOWED.len()];
    for path in ui_sources() {
        let src = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = src.lines().collect();
        let file = relative(&path);
        for (line, literal, why) in violations(&src) {
            let text = lines.get(line - 1).copied().unwrap_or("");
            match allowed_entry(&file, text, &literal) {
                Some(i) => used[i] = true,
                None => problems.push(format!("{file}:{line}: {why}: {}", text.trim())),
            }
        }
    }
    for (i, (f, whole, value, _)) in ALLOWED.iter().enumerate() {
        if !used[i] {
            problems.push(format!("stale allowlist entry {f}: {whole:?} ({value})"));
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
    let lines: Vec<usize> = violations(src).into_iter().map(|(l, _, _)| l).collect();
    // Line 6: `/ 2.0` halves, but `+ 1.0` nudges a coordinate by a point.
    assert_eq!(lines, vec![3, 5, 6, 7, 7, 8, 8, 8, 9, 11, 12]);
}

/// No value is exempt as such: a ½, 1 or 2 added to a coordinate is a spacing, and as a
/// bound, a tolerance or a zoom factor it is an exact [`ALLOWED`] entry. Only halving,
/// doubling and the rest of a fraction are unitless.
#[test]
fn the_literal_lint_sees_small_offsets_bounds_and_tolerances() {
    let src = "\
let text = pos2((icon.right() + 2.0).min(row.right()), row.top());
x -= 2.0;
painter.line_segment([pos2(r.left() + 2.0, y), pos2(r.right() - 2.0, y)], stroke);
let y = rect.bottom() - 0.5;
row_y += 2.0 + label_h + 2.0;
let x = last[2] + 1.0;
x += 1.0;
let a = (1.0 - age / FLASH_SECS).clamp(0.0, 1.0);
let p = height * (1.0 + PREFETCH);
if (width - self.width).abs() > 0.5 {}
zoom = Some(2.0);
let n = Some(-1.0).map(|d| d * -2.0);
let w = width.max(1.0);
let ok = size / 2.0 + space::XXS * 2.0 - w * 0.5;
let ok = rect.bottom() - stroke::HAIR / 2.0;
let ok = ink.lerp_to_gamma(lapis, 1.0 - t);
";
    let found = violations(src);
    let lines: Vec<usize> = found.iter().map(|(l, _, _)| *l).collect();
    assert_eq!(
        lines,
        vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 12, 13],
        "{found:?}"
    );
}

/// An allowlist entry covers its own line only: the same literal on another line, or the
/// line grown by a measurement, is flagged again.
#[test]
fn an_allowlist_entry_matches_its_whole_line_and_literal_only() {
    let entry = |file: &str, text: &str, literal: &str| allowed_entry(file, text, literal);
    assert!(entry("pane.rs", "        view.scroll_by(40.0);", "40.0").is_some());
    assert!(entry("pane.rs", "view.scroll_by(40.0 + 6.0);", "6.0").is_none());
    assert!(entry("pane.rs", "view.scroll_by(40.0).expand(1.0);", "40.0").is_none());
    assert!(entry("view.rs", "view.scroll_by(40.0);", "40.0").is_none());
    assert!(entry("pane.rs", "view.scroll_by(40.0);", "4.0").is_none());
}

/// A one-point size is a measurement inside a sink too: a focus ring pushed out, a clip
/// pulled in, a hairline's length. Only a counting integer 1 and the leading 1 of a fraction
/// are not.
#[test]
fn the_literal_lint_sees_one_point_sizes_in_sinks() {
    let src = "\
ring.rect_stroke(rect.expand(1.0), radius::S, stroke, kind);
p.with_clip_rect(r.shrink(1.0)).galley(at, galley, ink);
let hit = placed.iter().any(|p| p.expand(1.0).intersects(*r));
let at = Rect::from_min_size(p, vec2(1.0, 1.0));
ui.allocate_space(vec2(width, 1.0));
let m = egui::Margin::same(1);
let c = CornerRadius::new(1, 0, 0, 1);
ui.add_space(space::XL * f32::from(level.saturating_sub(1)));
let ok = ink.lerp_to_gamma(lapis, MIN + (1.0 - MIN) * heat);
let ok = vec2(n - 1, w * 0.5);
let ok = rect.expand(space::PX);
";
    let found = violations(src);
    let lines: Vec<usize> = found.iter().map(|(l, _, _)| *l).collect();
    assert_eq!(lines, vec![1, 2, 3, 4, 5, 6, 7], "{found:?}");
}

/// The scales grow step by step, so a surface that wants "a bit more room" has one.
#[test]
fn the_scales_are_ordered() {
    use crate::tokens::{radius, size, space, stroke, text};
    let ordered = |v: &[f32]| v.windows(2).all(|w| w[0] < w[1]);
    assert!(ordered(&[
        space::PX,
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
