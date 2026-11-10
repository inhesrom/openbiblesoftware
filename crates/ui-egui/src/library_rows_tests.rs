//! Library rows in a narrow Panel and at a Larger Interface size (#302), through the shell's
//! real-event headless harness with the Screenshot gallery's fixtures: every row's
//! abbreviation, title, language and licence badge sit in one column each, never paint over
//! each other, drop the badge and then the language rather than the title, and a title that
//! does not fit ends in "…". The licence reads in words ("Public domain"), never the raw
//! SPDX id. [`library_rows`] are the gallery's regression images of the same.

use egui::Shape;

use super::a11y_audit::Overlay;
use super::gallery::{LOOKS, launch};
use super::*;

/// The window width that leaves the default layout's Library Panel about 230 points wide, as
/// in LIBRARY-2 and WORKSPACE-MAC-6.
const NARROW_WINDOW: f32 = 690.0;

/// One piece of text painted in the Library Panel.
#[derive(Debug, Clone)]
struct Painted {
    rect: Rect,
    /// The whole text laid out.
    text: String,
    /// What shows: the text, or the start of it and "…".
    shown: String,
    elided: bool,
}

fn walk(shape: &Shape, out: &mut Vec<Painted>) {
    match shape {
        Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
        Shape::Text(t) => out.push(Painted {
            rect: Rect::from_min_size(t.pos, t.galley.size()),
            text: t.galley.text().to_string(),
            shown: t.galley.rows.iter().map(|r| r.text()).collect(),
            elided: t.galley.elided,
        }),
        _ => {}
    }
}

fn library_panel(h: &Harness) -> Rect {
    h.panels()
        .into_iter()
        .find(|(_, t)| t.iter().any(|t| t == "Library"))
        .map(|(r, _)| r)
        .expect("a Library Panel")
}

/// A Library row as painted: its parts left to right.
#[derive(Debug)]
struct PaintedRow {
    title: String,
    parts: Vec<Painted>,
}

impl PaintedRow {
    fn part(&self, text: &str) -> Option<&Painted> {
        self.parts.iter().find(|p| p.text == text)
    }

    /// The title as painted: whole, cut short, or from the Filter's match on.
    fn title_part(&self) -> &Painted {
        self.parts
            .iter()
            .find(|p| p.text == self.title || p.text.starts_with('…'))
            .expect("a title")
    }
}

/// Every Library row in view, top to bottom, found by its title.
fn painted_rows(h: &Harness) -> Vec<PaintedRow> {
    let panel = library_panel(h);
    let mut painted = Vec::new();
    for c in &h.last_shapes {
        walk(&c.shape, &mut painted);
    }
    painted.retain(|p| panel.contains(p.rect.center()));
    let mut rows = Vec::new();
    for e in h.shell.st.session.library.entries() {
        let m = &e.manifest;
        let (badge, _) = super::super::get_bibles_tab::spdx_badge(&m.license);
        let words = [
            m.abbreviation.as_str(),
            m.title.as_str(),
            m.language.as_str(),
            badge,
        ];
        // A title cut short before the Filter's match shows from the matched word on.
        let is_title = |p: &&Painted| {
            p.text == m.title
                || p.text
                    .strip_prefix('…')
                    .is_some_and(|tail| m.title.ends_with(tail))
        };
        for title in painted.iter().filter(is_title) {
            let y = title.rect.center().y;
            let mut parts: Vec<Painted> = painted
                .iter()
                .filter(|p| {
                    (p.rect.center().y - y).abs() < super::super::library_tab::ROW_H / 2.0
                        && (words.contains(&p.text.as_str()) || p.rect == title.rect)
                })
                .cloned()
                .collect();
            parts.sort_by(|a, b| a.rect.left().total_cmp(&b.rect.left()));
            parts.dedup_by(|a, b| a.rect == b.rect && a.text == b.text);
            rows.push((y, PaintedRow {
                title: m.title.clone(),
                parts,
            }));
        }
    }
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    rows.dedup_by(|a, b| a.0 == b.0);
    rows.into_iter().map(|(_, r)| r).collect()
}

/// No two parts of a row paint over each other, and every part is inside the Panel.
fn assert_no_overlap(h: &Harness, rows: &[PaintedRow]) {
    let panel = library_panel(h);
    for row in rows {
        for pair in row.parts.windows(2) {
            assert!(
                pair[0].rect.right() <= pair[1].rect.left(),
                "{:?} paints over {:?} in the row of {:?}",
                pair[0].shown,
                pair[1].shown,
                row.title
            );
        }
        for p in &row.parts {
            assert!(
                p.rect.left() >= panel.left() && p.rect.right() <= panel.right(),
                "{:?} leaves the Panel {panel:?}: {:?}",
                p.shown,
                p.rect
            );
        }
    }
}

/// A title's visible text: the start of the title, and "…" when it is cut short.
fn assert_ellipsis_when_cut(rows: &[PaintedRow]) {
    for row in rows {
        let title = row.title_part();
        if title.text.starts_with('…') {
            let middle = title.shown.trim_matches('…');
            assert!(row.title.contains(middle), "{:?}", title.shown);
        } else if title.elided {
            assert!(
                title.shown.ends_with('…'),
                "{:?} is cut without an ellipsis: {:?}",
                row.title,
                title.shown
            );
            let start = title.shown.trim_end_matches('…');
            assert!(row.title.starts_with(start), "{:?}", title.shown);
        } else {
            assert_eq!(title.shown, row.title);
        }
    }
}

fn narrow(h: &mut Harness) {
    h.screen = vec2(NARROW_WINDOW, h.screen.y);
    h.frames(4);
}

fn larger(h: &mut Harness) {
    h.shell
        .run("view.interface_size", &json!({ "value": "larger" }));
    h.frames(4);
}

/// The Filter set to "hebrew" in a narrow Panel: titles cut short before "Hebrew" show it.
fn narrow_filtered(h: &mut Harness) {
    let tab = h.tab_id("Library");
    h.shell.st.tabs.get_mut(&tab).unwrap().filter = "hebrew".into();
    narrow(h);
}

fn library_shown(h: &Harness) -> bool {
    !painted_rows(h).is_empty()
}

/// The gallery's regression images of Library rows (#302): in a narrow Panel, at a Larger
/// Interface size, and filtered in a narrow Panel (the default layout's Library Tab is in the
/// Tabs section already).
pub(super) fn library_rows() -> Vec<Overlay> {
    vec![
        ("Library rows: narrow Panel", narrow, library_shown),
        ("Library rows: Larger Interface size", larger, library_shown),
        (
            "Library rows: Filter match in a cut title",
            narrow_filtered,
            library_shown,
        ),
    ]
}

fn dark() -> &'static super::gallery::Look {
    &LOOKS[1]
}

#[test]
fn library_columns_line_up_and_licences_read_in_words() {
    let (h, _lab) = launch("library-rows-wide", dark());
    let rows = painted_rows(&h);
    assert!(rows.len() > 5, "{rows:?}");
    assert_no_overlap(&h, &rows);
    // One column each: a part starts where the same part of every other row does.
    for i in 0..4 {
        let lefts: Vec<f32> = rows
            .iter()
            .filter(|r| r.parts.len() == 4)
            .map(|r| r.parts[i].rect.left())
            .collect();
        assert!(lefts.len() > 5, "rows without all four parts: {rows:?}");
        assert!(
            lefts.iter().all(|x| (x - lefts[0]).abs() < 0.5),
            "column {i} starts at {lefts:?}"
        );
    }
    let texts = h.texts();
    assert!(
        texts.contains(&"Public domain"),
        "the KJV's licence in words: {texts:?}"
    );
    let panel = library_panel(&h);
    assert!(
        !h.shapes
            .iter()
            .any(|(r, t)| t.starts_with("LicenseRef-") && panel.contains(r.center())),
        "a raw SPDX id is painted in the Library: {texts:?}"
    );
    assert_ellipsis_when_cut(&rows);
}

#[test]
fn library_rows_in_a_narrow_panel_drop_the_badge_and_language_not_the_title() {
    let (mut h, _lab) = launch("library-rows-narrow", dark());
    narrow(&mut h);
    let panel = library_panel(&h);
    assert!(
        panel.width() < 260.0,
        "the Library Panel is {} wide",
        panel.width()
    );
    let rows = painted_rows(&h);
    assert!(rows.len() > 5, "{rows:?}");
    assert_no_overlap(&h, &rows);
    assert_ellipsis_when_cut(&rows);
    for row in &rows {
        // Abbreviation and title only; each still says something.
        assert_eq!(row.parts.len(), 2, "{row:?}");
        for p in &row.parts {
            assert!(p.shown.trim_end_matches('…').chars().count() >= 3, "{row:?}");
        }
    }
    assert!(
        rows.iter().any(|r| r.title_part().elided),
        "no title cut short at this width: {rows:?}"
    );
}

#[test]
fn library_titles_stay_readable_at_the_larger_interface_size() {
    let (mut h, _lab) = launch("library-rows-larger", dark());
    larger(&mut h);
    let rows = painted_rows(&h);
    assert!(rows.len() > 5, "{rows:?}");
    assert_no_overlap(&h, &rows);
    assert_ellipsis_when_cut(&rows);
    for row in &rows {
        let title = row.title_part();
        let shown = title.shown.trim_end_matches('…').chars().count();
        assert!(
            shown >= 15 || !title.elided,
            "{:?} shows only {:?}",
            row.title,
            title.shown
        );
    }
}

#[test]
fn a_cut_library_title_shows_the_part_the_filter_matched() {
    let (mut h, _lab) = launch("library-rows-filtered", dark());
    narrow_filtered(&mut h);
    let rows = painted_rows(&h);
    assert!(rows.len() > 1, "{rows:?}");
    assert_no_overlap(&h, &rows);
    assert_ellipsis_when_cut(&rows);
    for row in rows.iter().filter(|r| r.title.to_lowercase().contains("hebrew")) {
        let shown = &row.title_part().shown;
        assert!(shown.contains("Hebrew"), "{:?} shows {shown:?}", row.title);
    }
    assert!(
        rows.iter().any(|r| r.title_part().text.starts_with('…')),
        "no title shown from its match: {rows:?}"
    );
}
