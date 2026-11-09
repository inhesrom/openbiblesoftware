//! How the toolbar Launchers look (#140), kept apart from what they do (`toolbar.rs`) so the
//! toolbar can be restyled here alone: sizes, the colour roles taken from the chrome theme
//! ([`crate::theme::c`], so Light and Dark follow), the launcher buttons, the four icons,
//! drop-down rows and headings, and the Layout thumbnails.
//!
//! The icons come from the interface's icon set ([`crate::icons`]), in the theme's accent:
//! Library is books on a shelf, Guides a compass, Tools a two-by-two grid, Layouts a tiled
//! window.

use egui::{
    Align2, Color32, FontId, Painter, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, pos2,
    vec2,
};
use obs_engine::{Launcher, LayoutThumbnail};

use crate::icons::{self, Icon};
use crate::theme;
use crate::tokens::{alpha, radius, size, space, stroke, text};

/// The launcher buttons' height and icon size.
pub(super) const BUTTON_HEIGHT: f32 = size::LAUNCHER;
const ICON: f32 = size::ICON_LARGE;
const BUTTON_PAD: f32 = space::M;
const ICON_GAP: f32 = space::S;
const RADIUS: f32 = radius::L;

/// One row of a drop-down list.
pub(super) const ROW: f32 = size::LAUNCHER;
/// The drop-downs' widths.
pub(super) const LIBRARY_WIDTH: f32 = size::DIALOG_WIDTH;
pub(super) const GUIDES_WIDTH: f32 = size::PROSE;
pub(super) const TOOLS_WIDTH: f32 = size::COLUMN;
/// The Library pane's list of rows at most, before it scrolls.
pub(super) const LIBRARY_LIST: f32 = size::PROSE;
/// Room kept under a pane's scrolling list, to the window's bottom edge: the footer links,
/// the frame and a gap.
pub(super) const BELOW_LIST: f32 = size::ROW_TRIPLE + space::M;
/// A Layout card, and how many fit across the Layouts drop-down.
pub(super) const CARD: egui::Vec2 = size::LAYOUT_CARD;
pub(super) const CARDS_ACROSS: usize = 4;
const THUMB_HEIGHT: f32 = size::LAYOUT_THUMB;
/// A count badge's height and its text's inset.
const BADGE: f32 = size::BADGE;
const BADGE_PAD: f32 = space::XS;
/// A row's text inset from its sides.
const ROW_PAD: f32 = space::L;

/// The colour roles the toolbar uses, from this frame's chrome theme.
#[derive(Clone, Copy)]
pub(super) struct Look {
    pub ink: Color32,
    pub muted: Color32,
    /// Icons, the open launcher's label and the selected row's marker.
    pub accent: Color32,
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
        hover: c.hover,
        selected: c.selection,
        rule: c.rule,
        field: c.field,
        surface: c.surface,
        badge: c.gilt,
    }
}

fn label_font() -> FontId {
    FontId::proportional(text::BODY)
}

fn small_font() -> FontId {
    FontId::proportional(text::SMALL)
}

/// What a launcher button draws: its icon, its label, or both (Customize toolbar…).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct ButtonShows {
    pub icon: bool,
    pub label: bool,
}

/// A launcher button's size: icon, label and (for the Library) a count of updates. Icons
/// only, the count sits on the icon's corner.
pub(super) fn launcher_size(ui: &Ui, which: Launcher, shows: ButtonShows, badge: usize) -> Vec2 {
    if !shows.label {
        return vec2(BUTTON_PAD * 2.0 + ICON, BUTTON_HEIGHT);
    }
    let label = ui
        .painter()
        .layout_no_wrap(
            which.title().to_string(),
            label_font(),
            Color32::PLACEHOLDER,
        )
        .size()
        .x;
    let badge_w = if badge > 0 {
        ui.painter()
            .layout_no_wrap(badge.to_string(), small_font(), Color32::PLACEHOLDER)
            .size()
            .x
            + 2.0 * BADGE_PAD
            + space::XS
    } else {
        0.0
    };
    let icon = if shows.icon { ICON + ICON_GAP } else { 0.0 };
    vec2(BUTTON_PAD * 2.0 + icon + label + badge_w, BUTTON_HEIGHT)
}

/// Draw a launcher button: icon, label and (for the Library) a count of updates, as `shows`
/// asks. Returns its response; the caller adds the accessible name and tooltip.
pub(super) fn launcher_button(
    ui: &mut Ui,
    which: Launcher,
    open: bool,
    badge: usize,
    shows: ButtonShows,
) -> Response {
    let lk = look();
    let size = launcher_size(ui, which, shows, badge);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter();
    if open {
        crate::components::selected_row(p, rect, RADIUS);
    } else if resp.hovered() {
        p.rect_filled(rect, RADIUS, lk.hover);
    }
    if resp.has_focus() {
        p.rect_stroke(
            rect,
            RADIUS,
            Stroke::new(stroke::MEDIUM, lk.accent),
            StrokeKind::Inside,
        );
    }
    let mut x = rect.left() + BUTTON_PAD;
    if shows.icon {
        let icon = Rect::from_center_size(pos2(x + ICON / 2.0, rect.center().y), vec2(ICON, ICON));
        paint_icon(p, which, icon, &lk);
        x = icon.right() + ICON_GAP;
    }
    let badge_text = (badge > 0).then(|| {
        ui.painter()
            .layout_no_wrap(badge.to_string(), small_font(), theme::text_on(lk.badge))
    });
    if !shows.label {
        if let Some(g) = badge_text {
            let pill = Rect::from_min_size(
                pos2(rect.right() - g.size().x - 2.0 * BADGE_PAD, rect.top()),
                vec2(g.size().x + 2.0 * BADGE_PAD, BADGE),
            );
            p.rect_filled(pill, BADGE / 2.0, lk.badge);
            p.galley(
                pos2(pill.left() + BADGE_PAD, pill.center().y - g.size().y / 2.0),
                g,
                theme::text_on(lk.badge),
            );
        }
        return resp;
    }
    let color = if open { lk.accent } else { lk.ink };
    let galley = ui
        .painter()
        .layout_no_wrap(which.title().to_string(), label_font(), color);
    p.galley(
        pos2(x, rect.center().y - galley.size().y / 2.0),
        galley.clone(),
        color,
    );
    if let Some(g) = badge_text {
        let x = x + galley.size().x + space::XS;
        let pill = Rect::from_min_size(
            pos2(x, rect.center().y - BADGE / 2.0),
            vec2(g.size().x + 2.0 * BADGE_PAD, BADGE),
        );
        p.rect_filled(pill, BADGE / 2.0, lk.badge);
        p.galley(
            pos2(pill.left() + BADGE_PAD, pill.center().y - g.size().y / 2.0),
            g,
            theme::text_on(lk.badge),
        );
    }
    resp
}

const BAR_MARGIN: f32 = space::S;

/// How thick a toolbar is across, with its frame: a launcher button (icons only when
/// `vertical`), the margins and the outline.
pub(super) fn bar_thickness(vertical: bool) -> f32 {
    let button = if vertical {
        BUTTON_PAD * 2.0 + ICON
    } else {
        BUTTON_HEIGHT
    };
    button + 2.0 * BAR_MARGIN + space::XXS
}
/// The grip's length along the toolbar.
const GRIP: f32 = space::L;

/// The frame of a toolbar outside the menu bar (Bottom, Left, Right, or Auto-hide).
pub(super) fn bar_frame() -> egui::Frame {
    let lk = look();
    egui::Frame::new()
        .fill(lk.surface)
        .stroke(Stroke::new(stroke::HAIR, lk.rule))
        .inner_margin(egui::Margin::same(BAR_MARGIN as i8))
}

/// The grip's size: across the toolbar as thick as a button.
pub(super) fn grip_size(vertical: bool) -> Vec2 {
    if vertical {
        vec2(BUTTON_PAD * 2.0 + ICON, GRIP)
    } else {
        vec2(GRIP, BUTTON_HEIGHT)
    }
}

/// The grip handle at the toolbar's start: two rows of dots to drag it to another edge. It is
/// not a Tab stop: from the keyboard, View → Toolbar position moves the toolbar.
pub(super) fn grip(ui: &mut Ui, vertical: bool) -> Response {
    let lk = look();
    let (rect, resp) = ui.allocate_exact_size(grip_size(vertical), Sense::CLICK | Sense::DRAG);
    let color = if resp.hovered() || resp.dragged() {
        lk.accent
    } else {
        lk.muted
    };
    let c = rect.center();
    for i in -1..=1 {
        for j in [-1.0, 1.0] {
            let (dx, dy) = if vertical {
                (i as f32 * space::XS, j * space::XXS)
            } else {
                (j * space::XXS, i as f32 * space::XS)
            };
            ui.painter()
                .circle_filled(pos2(c.x + dx, c.y + dy), size::GRIP_DOT, color);
        }
    }
    resp
}

/// The size of the » button holding the Launchers a small window has no room for.
pub(super) fn overflow_size() -> Vec2 {
    vec2(BUTTON_PAD * 2.0 + ICON, BUTTON_HEIGHT)
}

/// The overflow button (a double chevron) holding the Launchers a small window has no room for.
pub(super) fn overflow_button(ui: &mut Ui, open: bool) -> Response {
    let lk = look();
    let (rect, resp) = ui.allocate_exact_size(overflow_size(), Sense::click());
    if open {
        crate::components::selected_row(ui.painter(), rect, RADIUS);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, RADIUS, lk.hover);
    }
    icons::paint(ui.painter(), Icon::Overflow, rect, icons::LARGE, lk.accent);
    resp
}

/// Where the toolbar would go if the grip were let go now.
pub(super) fn drop_edge(ctx: &egui::Context, r: Rect) {
    let lk = look();
    let p = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("toolbar-drop-edge"),
    ));
    p.rect_filled(r, radius::S, lk.accent.gamma_multiply(alpha::TINT));
    p.rect_stroke(
        r,
        radius::S,
        Stroke::new(stroke::THICK, lk.accent),
        StrokeKind::Inside,
    );
}

/// A launcher's icon inside `r`.
pub(super) fn paint_icon(p: &Painter, which: Launcher, r: Rect, lk: &Look) {
    let icon = match which {
        Launcher::Library => Icon::Library,
        Launcher::Guides => Icon::Guides,
        Launcher::Tools => Icon::Tools,
        Launcher::Layouts => Icon::Layouts,
    };
    icons::paint(p, icon, r, ICON, lk.accent);
}

/// A drop-down's section heading ("Recent", "Bibles").
pub(super) fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(space::XS);
    crate::components::section_header(ui, text);
}

/// The background of a list row: selected, hovered or neither.
pub(super) fn row_background(ui: &Ui, rect: Rect, selected: bool, hovered: bool) {
    let lk = look();
    if selected {
        ui.painter().rect_filled(rect, radius::M, lk.selected);
        let bar = Rect::from_min_size(
            rect.min + vec2(0.0, space::S),
            vec2(size::DOT, rect.height() - 2.0 * space::S),
        );
        ui.painter().rect_filled(bar, radius::XS, lk.accent);
    } else if hovered {
        ui.painter().rect_filled(rect, radius::M, lk.hover);
    }
}

/// A Resource row: its abbreviation in a chip (a Favorite's star when `chip` is `None`), its
/// title, and its type and language on the right.
pub(super) fn resource_row(ui: &Ui, rect: Rect, chip: Option<&str>, title: &str, detail: &str) {
    let lk = look();
    let p = ui.painter();
    // A fixed-width chip, so titles line up; a long abbreviation ends in "…".
    let chip_w = size::CHIP_ABBREVIATION;
    let chip_rect = Rect::from_min_size(
        pos2(rect.left() + ROW_PAD, rect.center().y - size::CHIP / 2.0),
        vec2(chip_w, size::CHIP),
    );
    p.rect_filled(chip_rect, radius::S, lk.surface);
    p.rect_stroke(
        chip_rect,
        radius::S,
        Stroke::new(stroke::HAIR, lk.rule),
        StrokeKind::Inside,
    );
    match chip {
        Some(abbreviation) => {
            let chip_text = elided(
                p,
                abbreviation.to_string(),
                FontId::proportional(text::SMALL),
                chip_w - 2.0 * BADGE_PAD,
                lk.accent,
            );
            p.galley(
                pos2(
                    chip_rect.center().x - chip_text.size().x / 2.0,
                    chip_rect.center().y - chip_text.size().y / 2.0,
                ),
                chip_text,
                lk.accent,
            );
        }
        None => icons::paint(p, Icon::StarFilled, chip_rect, icons::SMALL, lk.badge),
    }
    let right = p.text(
        pos2(rect.right() - ROW_PAD, rect.center().y),
        Align2::RIGHT_CENTER,
        detail,
        small_font(),
        lk.muted,
    );
    let left = chip_rect.right() + ROW_PAD;
    let clip = Rect::from_min_max(
        pos2(left, rect.top()),
        pos2((right.left() - ROW_PAD).max(left), rect.bottom()),
    );
    p.with_clip_rect(clip).text(
        pos2(left, rect.center().y),
        Align2::LEFT_CENTER,
        title,
        label_font(),
        lk.ink,
    );
}

/// A plain row: the command's icon, a label, and a muted note (a shortcut) on the right.
pub(super) fn plain_row(
    ui: &Ui,
    rect: Rect,
    icon: Option<Icon>,
    label: &str,
    note: &str,
    enabled: bool,
) {
    let lk = look();
    let p = ui.painter();
    let mut x = rect.left() + ROW_PAD;
    if let Some(icon) = icon {
        let at = Rect::from_min_size(
            pos2(x, rect.center().y - icons::BODY / 2.0),
            vec2(icons::BODY, icons::BODY),
        );
        icons::paint(
            p,
            icon,
            at,
            icons::BODY,
            if enabled { lk.accent } else { lk.muted },
        );
        x = at.right() + ROW_PAD;
    }
    p.text(
        pos2(x, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        label_font(),
        if enabled { lk.ink } else { lk.muted },
    );
    if !note.is_empty() {
        p.text(
            pos2(rect.right() - ROW_PAD, rect.center().y),
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
    let (rect, resp) = ui.allocate_exact_size(
        galley.size() + 2.0 * vec2(space::S, space::XS),
        Sense::click(),
    );
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Link, true, text));
    row_background(ui, rect, selected, resp.hovered());
    ui.painter().galley(
        pos2(
            rect.left() + space::S,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        lk.accent,
    );
    resp
}

/// The frame of a drop-down.
pub(super) fn dropdown_frame(_ui: &Ui) -> egui::Frame {
    crate::components::popover_frame()
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
    p.rect_filled(rect, radius::L, bg);
    let border = if selected { lk.accent } else { lk.rule };
    // In a High contrast theme (#224) the selected card's border is heavier, not only bluer.
    let width = if selected && theme::c().high_contrast {
        stroke::THICK
    } else {
        stroke::HAIR
    };
    p.rect_stroke(
        rect,
        radius::L,
        Stroke::new(width, border),
        StrokeKind::Inside,
    );
    let thumb = Rect::from_min_size(
        rect.min + egui::Vec2::splat(space::M),
        vec2(rect.width() - 2.0 * space::M, THUMB_HEIGHT),
    );
    p.rect_filled(thumb, radius::S, lk.field);
    match thumbnail {
        Some(t) => paint_thumbnail(p, t, thumb.shrink(space::XS), &lk, dim),
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
        pos2(rect.left() + space::M, thumb.bottom() + space::XS),
        rect.max - vec2(space::M, space::XS),
    )
}

/// Paint a Layout's Panel tiling into `r`: each Panel a rounded tile with its active Tab's
/// title, topped by its Link set's colour.
pub(super) fn paint_thumbnail(p: &Painter, t: &LayoutThumbnail, r: Rect, lk: &Look, dim: bool) {
    const GAP: f32 = space::XS;
    match t {
        LayoutThumbnail::Panel { title, link, tabs } => {
            let fill = if dim { lk.field } else { lk.surface };
            p.rect_filled(r, radius::S, fill);
            p.rect_stroke(
                r,
                radius::S,
                Stroke::new(stroke::HAIR, lk.rule),
                StrokeKind::Inside,
            );
            if link.is_some() && !dim {
                let stripe = Rect::from_min_size(r.min, vec2(r.width(), size::DOT));
                p.rect_filled(stripe, radius::XS, theme::link_color(*link));
            }
            let text = if *tabs > 1 {
                format!("{title} +{}", tabs - 1)
            } else {
                title.clone()
            };
            let galley = elided(
                p,
                text,
                FontId::proportional(text::CAPTION),
                r.width() - space::S,
                if dim { lk.muted } else { lk.ink },
            );
            let at = r.center() - galley.size() / 2.0;
            p.with_clip_rect(r.shrink(space::PX))
                .galley(at, galley, lk.ink);
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
        ui.painter().rect_filled(rect, radius::S, lk.hover);
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
        .layout_no_wrap(text.to_string(), small_font(), Color32::PLACEHOLDER)
        .size()
        .x
}

/// A card's name, clipped to its slot.
pub(super) fn card_name(ui: &Ui, slot: Rect, name: &str, dim: bool) {
    let lk = look();
    ui.painter().with_clip_rect(slot).text(
        pos2(slot.left(), slot.top() + space::XXS),
        Align2::LEFT_TOP,
        name,
        FontId::proportional(text::LABEL),
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
    let pad = vec2(space::M, space::XS);
    let r = Rect::from_min_size(at + vec2(space::L, space::M), galley.size() + 2.0 * pad);
    p.rect_filled(r, radius::M, lk.field);
    p.rect_stroke(
        r,
        radius::M,
        Stroke::new(stroke::MEDIUM, lk.accent),
        StrokeKind::Inside,
    );
    p.galley(r.min + pad, galley, lk.accent);
}

/// The drop target under a dragged Resource: a whole Panel, or its right edge (a new Panel
/// beside it).
pub(super) fn drop_target(ctx: &egui::Context, r: Rect) {
    let lk = look();
    let p = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("library-drop-target"),
    ));
    p.rect_filled(r, radius::M, lk.accent.gamma_multiply(alpha::TINT));
    p.rect_stroke(
        r,
        radius::M,
        Stroke::new(stroke::THICK, lk.accent),
        StrokeKind::Inside,
    );
}
