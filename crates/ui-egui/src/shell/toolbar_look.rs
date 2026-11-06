//! How the toolbar Launchers look (#140), kept apart from what they do (`toolbar.rs`) so the
//! toolbar can be restyled here alone: sizes, the colour roles taken from the chrome theme
//! ([`crate::theme::c`], so Light and Dark follow), the launcher buttons, the four icons,
//! drop-down rows and headings, and the Layout thumbnails.
//!
//! The icons are our own line drawings, painted with the theme's colours: Library is three
//! books on a shelf, Guides a compass, Tools a two-by-two grid, Layouts a tiled window.

use egui::{
    Align2, Color32, CornerRadius, FontId, Painter, Rect, Response, Sense, Stroke, StrokeKind, Ui,
    pos2, vec2,
};
use obs_engine::{Launcher, LayoutThumbnail};

use crate::theme;

/// The launcher buttons' height and icon size.
pub(super) const BUTTON_HEIGHT: f32 = 30.0;
const ICON: f32 = 18.0;
const BUTTON_PAD: f32 = 9.0;
const ICON_GAP: f32 = 7.0;
const RADIUS: u8 = 7;

/// One row of a drop-down list.
pub(super) const ROW: f32 = 30.0;
/// The drop-downs' widths.
pub(super) const LIBRARY_WIDTH: f32 = 500.0;
pub(super) const GUIDES_WIDTH: f32 = 420.0;
pub(super) const TOOLS_WIDTH: f32 = 300.0;
/// The Library pane's list of rows at most, before it scrolls.
pub(super) const LIBRARY_LIST: f32 = 440.0;
/// Room kept under a pane's scrolling list, to the window's bottom edge: the footer links,
/// the frame and a gap.
pub(super) const BELOW_LIST: f32 = 90.0;
/// A Layout card, and how many fit across the Layouts drop-down.
pub(super) const CARD: egui::Vec2 = vec2(150.0, 122.0);
pub(super) const CARDS_ACROSS: usize = 4;
const THUMB_HEIGHT: f32 = 78.0;

/// The colour roles the toolbar uses, from this frame's chrome theme.
#[derive(Clone, Copy)]
pub(super) struct Look {
    pub ink: Color32,
    pub muted: Color32,
    /// Icons, the open launcher's label and the selected row's marker.
    pub accent: Color32,
    /// The second icon colour (pages, needles).
    pub accent2: Color32,
    pub hover: Color32,
    pub selected: Color32,
    pub rule: Color32,
    pub field: Color32,
    pub surface: Color32,
    pub badge: Color32,
}

pub(super) fn look() -> Look {
    let c = theme::c();
    Look {
        ink: c.ink,
        muted: c.muted,
        accent: c.lapis,
        accent2: c.verdigris,
        hover: c.hover,
        selected: c.selection,
        rule: c.rule,
        field: c.field,
        surface: c.surface,
        badge: c.gilt,
    }
}

fn label_font() -> FontId {
    FontId::proportional(14.0)
}

fn small_font() -> FontId {
    FontId::proportional(11.5)
}

/// Draw a launcher button: icon, label and (for the Library) a count of updates. Returns its
/// response; the caller adds the accessible name and tooltip.
pub(super) fn launcher_button(ui: &mut Ui, which: Launcher, open: bool, badge: usize) -> Response {
    let lk = look();
    let galley = ui
        .painter()
        .layout_no_wrap(which.title().to_string(), label_font(), lk.ink);
    let badge_text = (badge > 0).then(|| {
        ui.painter()
            .layout_no_wrap(badge.to_string(), small_font(), Color32::WHITE)
    });
    let badge_w = badge_text.as_ref().map_or(0.0, |g| g.size().x + 10.0 + 5.0);
    let width = BUTTON_PAD * 2.0 + ICON + ICON_GAP + galley.size().x + badge_w;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, BUTTON_HEIGHT), Sense::click());
    let p = ui.painter();
    if open {
        p.rect_filled(rect, RADIUS, lk.selected);
    } else if resp.hovered() {
        p.rect_filled(rect, RADIUS, lk.hover);
    }
    if resp.has_focus() {
        p.rect_stroke(
            rect,
            RADIUS,
            Stroke::new(1.5, lk.accent),
            StrokeKind::Inside,
        );
    }
    let icon = Rect::from_center_size(
        pos2(rect.left() + BUTTON_PAD + ICON / 2.0, rect.center().y),
        vec2(ICON, ICON),
    );
    paint_icon(p, which, icon, &lk);
    let text_x = icon.right() + ICON_GAP;
    let color = if open { lk.accent } else { lk.ink };
    p.galley(
        pos2(text_x, rect.center().y - galley.size().y / 2.0),
        galley.clone(),
        color,
    );
    if let Some(g) = badge_text {
        let x = text_x + galley.size().x + 5.0;
        let pill = Rect::from_min_size(
            pos2(x, rect.center().y - 8.0),
            vec2(g.size().x + 10.0, 16.0),
        );
        p.rect_filled(pill, 8, lk.badge);
        p.galley(
            pos2(pill.left() + 5.0, pill.center().y - g.size().y / 2.0),
            g,
            Color32::WHITE,
        );
    }
    resp
}

/// A launcher's icon inside `r`.
pub(super) fn paint_icon(p: &Painter, which: Launcher, r: Rect, lk: &Look) {
    let s = Stroke::new(1.6, lk.accent);
    match which {
        Launcher::Library => {
            // Three books of different heights on a shelf; the middle one leans.
            let base = r.bottom() - 2.0;
            let books = [(r.left() + 2.0, 4.0, 13.0), (r.left() + 7.0, 4.0, 11.0)];
            for (x, w, h) in books {
                let b = Rect::from_min_max(pos2(x, base - h), pos2(x + w, base));
                p.rect_stroke(b, 1, s, StrokeKind::Middle);
            }
            let lean = [
                pos2(r.left() + 12.0, base),
                pos2(r.left() + 15.5, base - 0.8),
                pos2(r.left() + 17.5, base - 12.0),
                pos2(r.left() + 14.0, base - 11.2),
            ];
            p.add(egui::Shape::closed_line(lean.to_vec(), s));
            p.line_segment(
                [pos2(r.left(), base + 1.2), pos2(r.right(), base + 1.2)],
                Stroke::new(1.6, lk.accent2),
            );
        }
        Launcher::Guides => {
            // A compass: a ring and a two-colour needle.
            let c = r.center();
            p.circle_stroke(c, r.width() / 2.0 - 1.0, s);
            let n = r.height() / 2.0 - 3.5;
            let w = 2.6;
            p.add(egui::Shape::convex_polygon(
                vec![pos2(c.x, c.y - n), pos2(c.x + w, c.y), pos2(c.x - w, c.y)],
                lk.accent,
                Stroke::NONE,
            ));
            p.add(egui::Shape::convex_polygon(
                vec![pos2(c.x, c.y + n), pos2(c.x - w, c.y), pos2(c.x + w, c.y)],
                lk.accent2,
                Stroke::NONE,
            ));
        }
        Launcher::Tools => {
            // A two-by-two grid of rounded squares, one filled.
            let g = 2.5;
            let side = (r.width() - g) / 2.0 - 1.0;
            for (i, (dx, dy)) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]
                .into_iter()
                .enumerate()
            {
                let min = pos2(
                    r.left() + 1.0 + dx * (side + g),
                    r.top() + 1.0 + dy * (side + g),
                );
                let sq = Rect::from_min_size(min, vec2(side, side));
                if i == 3 {
                    p.rect_filled(sq, 2, lk.accent2);
                } else {
                    p.rect_stroke(sq, 2, s, StrokeKind::Middle);
                }
            }
        }
        Launcher::Layouts => {
            // A window tiled into one tall Panel and two stacked ones.
            let w = r.shrink(1.0);
            p.rect_stroke(w, 2, s, StrokeKind::Middle);
            let split = w.left() + w.width() * 0.55;
            p.line_segment([pos2(split, w.top()), pos2(split, w.bottom())], s);
            p.line_segment(
                [pos2(split, w.center().y), pos2(w.right(), w.center().y)],
                Stroke::new(1.6, lk.accent2),
            );
        }
    }
}

/// A drop-down's section heading ("Recent", "Bibles").
pub(super) fn heading(ui: &mut Ui, text: &str) {
    let lk = look();
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .font(small_font())
            .color(lk.muted)
            .strong(),
    );
}

/// The background of a list row: selected, hovered or neither.
pub(super) fn row_background(ui: &Ui, rect: Rect, selected: bool, hovered: bool) {
    let lk = look();
    if selected {
        ui.painter().rect_filled(rect, 5, lk.selected);
        let bar = Rect::from_min_size(rect.min + vec2(0.0, 6.0), vec2(3.0, rect.height() - 12.0));
        ui.painter().rect_filled(bar, 2, lk.accent);
    } else if hovered {
        ui.painter().rect_filled(rect, 5, lk.hover);
    }
}

/// A Resource row: its abbreviation in a chip, its title, and its type and language on the
/// right.
pub(super) fn resource_row(ui: &Ui, rect: Rect, abbreviation: &str, title: &str, detail: &str) {
    let lk = look();
    let p = ui.painter();
    // A fixed-width chip, so titles line up; a long abbreviation ends in "…".
    let chip_w = 58.0;
    let chip_text = elided(
        p,
        abbreviation.to_string(),
        FontId::proportional(12.0),
        chip_w - 8.0,
        lk.accent,
    );
    let chip = Rect::from_min_size(
        pos2(rect.left() + 10.0, rect.center().y - 10.0),
        vec2(chip_w, 20.0),
    );
    p.rect_filled(chip, 4, lk.surface);
    p.rect_stroke(chip, 4, Stroke::new(1.0, lk.rule), StrokeKind::Inside);
    p.galley(
        pos2(
            chip.center().x - chip_text.size().x / 2.0,
            chip.center().y - chip_text.size().y / 2.0,
        ),
        chip_text,
        lk.accent,
    );
    let right = p.text(
        pos2(rect.right() - 10.0, rect.center().y),
        Align2::RIGHT_CENTER,
        detail,
        small_font(),
        lk.muted,
    );
    let left = chip.right() + 10.0;
    let clip = Rect::from_min_max(
        pos2(left, rect.top()),
        pos2((right.left() - 10.0).max(left), rect.bottom()),
    );
    p.with_clip_rect(clip).text(
        pos2(left, rect.center().y),
        Align2::LEFT_CENTER,
        title,
        label_font(),
        lk.ink,
    );
}

/// A plain row: a label, and a muted note (a shortcut) on the right.
pub(super) fn plain_row(ui: &Ui, rect: Rect, label: &str, note: &str, enabled: bool) {
    let lk = look();
    let p = ui.painter();
    p.text(
        pos2(rect.left() + 12.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        label_font(),
        if enabled { lk.ink } else { lk.muted },
    );
    if !note.is_empty() {
        p.text(
            pos2(rect.right() - 10.0, rect.center().y),
            Align2::RIGHT_CENTER,
            note,
            small_font(),
            lk.muted,
        );
    }
}

/// A footer link ("Get Resources…"), highlighted when selected from the keyboard.
pub(super) fn footer_link(ui: &mut Ui, text: &str, selected: bool) -> Response {
    let lk = look();
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), label_font(), lk.accent);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + vec2(14.0, 10.0), Sense::click());
    row_background(ui, rect, selected, resp.hovered());
    ui.painter().galley(
        pos2(rect.left() + 7.0, rect.center().y - galley.size().y / 2.0),
        galley,
        lk.accent,
    );
    resp
}

/// The frame of a drop-down.
pub(super) fn dropdown_frame(ui: &Ui) -> egui::Frame {
    let lk = look();
    egui::Frame::popup(ui.style())
        .fill(lk.field)
        .stroke(Stroke::new(1.0, lk.rule))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::same(10))
}

/// A Layout card: its thumbnail and its name. `dim` greys out a Built-in Layout this library
/// cannot make.
pub(super) fn layout_card(
    ui: &Ui,
    rect: Rect,
    thumbnail: Option<&LayoutThumbnail>,
    selected: bool,
    hovered: bool,
    dim: bool,
) -> Rect {
    let lk = look();
    let p = ui.painter();
    let bg = if selected {
        lk.selected
    } else if hovered {
        lk.hover
    } else {
        lk.surface
    };
    p.rect_filled(rect, 8, bg);
    let border = if selected { lk.accent } else { lk.rule };
    p.rect_stroke(rect, 8, Stroke::new(1.0, border), StrokeKind::Inside);
    let thumb = Rect::from_min_size(
        rect.min + vec2(8.0, 8.0),
        vec2(rect.width() - 16.0, THUMB_HEIGHT),
    );
    p.rect_filled(thumb, 4, lk.field);
    match thumbnail {
        Some(t) => paint_thumbnail(p, t, thumb.shrink(3.0), &lk, dim),
        None => {
            p.text(
                thumb.center(),
                Align2::CENTER_CENTER,
                "—",
                label_font(),
                lk.muted,
            );
        }
    }
    // Where the name goes.
    Rect::from_min_max(
        pos2(rect.left() + 8.0, thumb.bottom() + 4.0),
        rect.max - vec2(8.0, 4.0),
    )
}

/// Paint a Layout's Panel tiling into `r`: each Panel a rounded tile with its active Tab's
/// title, topped by its Link set's colour.
pub(super) fn paint_thumbnail(p: &Painter, t: &LayoutThumbnail, r: Rect, lk: &Look, dim: bool) {
    const GAP: f32 = 3.0;
    match t {
        LayoutThumbnail::Panel { title, link, tabs } => {
            let fill = if dim { lk.field } else { lk.surface };
            p.rect_filled(r, 3, fill);
            p.rect_stroke(r, 3, Stroke::new(1.0, lk.rule), StrokeKind::Inside);
            if link.is_some() && !dim {
                let stripe = Rect::from_min_size(r.min, vec2(r.width(), 3.0));
                p.rect_filled(stripe, 2, theme::link_color(*link));
            }
            let text = if *tabs > 1 {
                format!("{title} +{}", tabs - 1)
            } else {
                title.clone()
            };
            let galley = elided(
                p,
                text,
                FontId::proportional(10.0),
                r.width() - 6.0,
                if dim { lk.muted } else { lk.ink },
            );
            let at = r.center() - galley.size() / 2.0;
            p.with_clip_rect(r.shrink(1.0)).galley(at, galley, lk.ink);
        }
        LayoutThumbnail::Row(children) | LayoutThumbnail::Column(children) => {
            let horizontal = matches!(t, LayoutThumbnail::Row(_));
            let total: f32 = children.iter().map(|(_, s)| s.max(0.01)).sum();
            let length = if horizontal { r.width() } else { r.height() };
            let room = length - GAP * (children.len().saturating_sub(1)) as f32;
            let mut at = if horizontal { r.left() } else { r.top() };
            for (child, share) in children {
                let size = room * share.max(0.01) / total;
                let cell = if horizontal {
                    Rect::from_min_size(pos2(at, r.top()), vec2(size, r.height()))
                } else {
                    Rect::from_min_size(pos2(r.left(), at), vec2(r.width(), size))
                };
                paint_thumbnail(p, child, cell, lk, dim);
                at += size + GAP;
            }
        }
    }
}

/// `text` on one line at most `width` wide, ending in "…" when it does not fit.
fn elided(
    p: &Painter,
    text: String,
    font: FontId,
    width: f32,
    color: Color32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text, font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(1.0));
    p.layout_job(job)
}

/// A small text link inside a Layout card ("Rename", "Delete").
pub(super) fn card_action(ui: &mut Ui, rect: Rect, text: &str) -> Response {
    let lk = look();
    let resp = ui.interact(
        rect,
        ui.scope_id()
            .with(("card-action", text, rect.min.x as i32, rect.min.y as i32)),
        Sense::click(),
    );
    if resp.hovered() {
        ui.painter().rect_filled(rect, 3, lk.hover);
    }
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        small_font(),
        lk.accent,
    );
    resp
}

/// How wide `text` is in the card actions' font.
pub(super) fn small_width(ui: &Ui, text: &str) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_string(), small_font(), Color32::WHITE)
        .size()
        .x
}

/// A card's name, clipped to its slot.
pub(super) fn card_name(ui: &Ui, slot: Rect, name: &str, dim: bool) {
    let lk = look();
    ui.painter().with_clip_rect(slot).text(
        pos2(slot.left(), slot.top() + 2.0),
        Align2::LEFT_TOP,
        name,
        FontId::proportional(13.0),
        if dim { lk.muted } else { lk.ink },
    );
}

/// The note under a card's name (why a Built-in Layout cannot be made).
pub(super) fn card_note(ui: &Ui, slot: Rect, note: &str) {
    let lk = look();
    ui.painter().with_clip_rect(slot).text(
        pos2(slot.left(), slot.bottom()),
        Align2::LEFT_BOTTOM,
        note,
        small_font(),
        lk.muted,
    );
}

/// The ghost that follows the pointer while a Resource is dragged out of the Library
/// drop-down.
pub(super) fn drag_ghost(ctx: &egui::Context, at: egui::Pos2, abbreviation: &str) {
    let lk = look();
    let p = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::unique("library-drag-ghost"),
    ));
    let galley = p.layout_no_wrap(abbreviation.to_string(), label_font(), lk.accent);
    let r = Rect::from_min_size(at + vec2(12.0, 8.0), galley.size() + vec2(16.0, 10.0));
    p.rect_filled(r, 6, lk.field);
    p.rect_stroke(r, 6, Stroke::new(1.5, lk.accent), StrokeKind::Inside);
    p.galley(r.min + vec2(8.0, 5.0), galley, lk.accent);
}

/// The drop target under a dragged Resource: a whole Panel, or its right edge (a new Panel
/// beside it).
pub(super) fn drop_target(ctx: &egui::Context, r: Rect) {
    let lk = look();
    let p = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("library-drop-target"),
    ));
    p.rect_filled(r, 6, lk.accent.gamma_multiply(0.18));
    p.rect_stroke(r, 6, Stroke::new(2.0, lk.accent), StrokeKind::Inside);
}
