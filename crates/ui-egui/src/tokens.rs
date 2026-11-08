//! Design tokens (#221): the one place the interface's spacing, type sizes, corner radii,
//! stroke widths, elevations, control sizes and chrome colour roles are written down. Every
//! surface takes its measurements from here, mostly through the shared components in
//! [`crate::components`], so Tabs, panes, menus and windows look like one app. A test
//! (`design_tokens_tests.rs`) fails on a spacing, size or colour literal anywhere else in the
//! crate, with an allowlist for the few that are not part of the look.
//!
//! Page colours (the Resource text's ink, paper and Highlight tones) are the other colour
//! token set and live in [`crate::Palette`].

use egui::{Color32, CornerRadius, Margin, Shadow, Vec2, vec2};

/// The spacing scale, in points: gaps between widgets, insets of frames, indents.
pub mod space {
    /// One point: a mark, a ring or a rect kept one point clear of an edge, a hairline or a
    /// neighbour.
    pub const PX: f32 = 1.0;
    /// Between a glyph and its mark; inside a chip.
    pub const XXS: f32 = 2.0;
    /// Between closely related controls.
    pub const XS: f32 = 4.0;
    /// Between the rows of a list; a control's inner gap.
    pub const S: f32 = 6.0;
    /// Between controls; the default item spacing.
    pub const M: f32 = 8.0;
    /// Between groups inside a card or a section.
    pub const L: f32 = 12.0;
    /// Between sections; a page's side inset.
    pub const XL: f32 = 16.0;
    /// Between major regions; the reading page's margin.
    pub const XXL: f32 = 24.0;
    /// Empty-state breathing room.
    pub const XXXL: f32 = 32.0;
}

/// The type scale, in points (the chrome; Resource text has its own size setting).
pub mod text {
    /// Counts, badges, licence chips, axis ticks.
    pub const CAPTION: f32 = 11.0;
    /// Secondary labels, hints, small headings (`TextStyle::Small`).
    pub const SMALL: f32 = 12.0;
    /// Compact control labels: chips, header controls, list cells.
    pub const LABEL: f32 = 13.0;
    /// Body text and buttons (`TextStyle::Body`).
    pub const BODY: f32 = 14.0;
    /// A card's or a window's title.
    pub const SUBTITLE: f32 = 16.0;
    /// A Tab's heading in the display face (`TextStyle::Heading`).
    pub const TITLE: f32 = 20.0;
    /// The largest display heading (a Factbook name, Home's date).
    pub const DISPLAY: f32 = 24.0;
    /// Monospace (keys, hex colours, ids).
    pub const MONO: f32 = 12.0;
    /// A line of `LABEL` text with its leading, and of `CAPTION` text, where text is painted
    /// line by line.
    pub const LABEL_LINE: f32 = 18.0;
    pub const CAPTION_LINE: f32 = 15.0;
}

/// Corner radii, in points.
pub mod radius {
    /// A placeholder bar, a thin bar in a chart.
    pub const XS: f32 = 2.0;
    /// Marks, swatches, bars and small badges.
    pub const S: f32 = 4.0;
    /// Buttons, fields and cards.
    pub const M: f32 = 6.0;
    /// Menus, popovers and toolbar buttons.
    pub const L: f32 = 8.0;
    /// Windows.
    pub const XL: f32 = 10.0;
    /// Chips: half a chip's height, so the ends are round.
    pub const PILL: f32 = super::size::CHIP / 2.0;
}

/// Stroke widths, in points.
pub mod stroke {
    /// Rules, outlines and borders.
    pub const HAIR: f32 = 1.0;
    /// Drawn lines that must read as marks: map coasts, chart outlines, an open marker.
    pub const MEDIUM: f32 = 1.5;
    /// Emphasis: a resize handle in use, the current Find match, the keyboard focus ring.
    pub const THICK: f32 = 2.0;
}

/// Opacity factors for washes and tints drawn over a fill (`gamma_multiply`).
pub mod alpha {
    /// A faint wash behind a band of rows or a notice.
    pub const WASH: f32 = 0.12;
    /// A tinted fill: a hovered thumbnail, a faint rule.
    pub const TINT: f32 = 0.2;
    /// A half-strength mark.
    pub const HALF: f32 = 0.5;
    /// A soft line or a shaded disc behind a marker.
    pub const SOFT: f32 = 0.6;
    /// A strong tint that still lets what is under it show.
    pub const STRONG: f32 = 0.75;
    /// A map label's halo.
    pub const HALO: u8 = 210;
}

/// Control and row sizes, in points.
pub mod size {
    /// Beside small text.
    pub const ICON_SMALL: f32 = 12.0;
    /// Beside body text: menu items, buttons, Tab header controls.
    pub const ICON: f32 = 16.0;
    /// Toolbar Launchers.
    pub const ICON_LARGE: f32 = 18.0;
    /// A clickable control's height (egui's `interact_size.y`).
    pub const CONTROL: f32 = 28.0;
    /// A compact control: a Tab header icon button, a chip.
    pub const CONTROL_COMPACT: f32 = 22.0;
    /// A chip (a Bible in a Search's or a Text Comparison's list, a licence badge).
    pub const CHIP: f32 = 22.0;
    /// A compact list row (Contents, the place list).
    pub const ROW_COMPACT: f32 = 24.0;
    /// A list row (Library, Keyboard shortcuts, the Command box).
    pub const ROW: f32 = 28.0;
    /// A row of two lines with its buttons (a Starter Resource).
    pub const ROW_DOUBLE: f32 = 64.0;
    /// A row of three lines with its buttons (a downloadable Bible).
    pub const ROW_TRIPLE: f32 = 80.0;
    /// A drop-down list's height before it scrolls.
    pub const LIST_TALL: f32 = 400.0;
    /// A Tab header narrower than this drops the title and narrows its box and choosers.
    pub const HEADER_NARROW: f32 = 430.0;
    /// A Tab header's box (a reference, a word, a name), and in a narrow header.
    pub const HEADER_BOX: f32 = 140.0;
    pub const HEADER_BOX_NARROW: f32 = 110.0;
    /// A Tab header's Resource chooser, and in a narrow header.
    pub const HEADER_CHOOSER: f32 = 72.0;
    pub const HEADER_CHOOSER_NARROW: f32 = 52.0;
    /// The Panels' tab bar.
    pub const TAB_BAR: f32 = 31.0;
    /// Room kept for a scroll bar beside custom-drawn content.
    pub const SCROLLBAR: f32 = 10.0;
    /// The shortest a scroll bar's thumb gets.
    pub const THUMB_MIN: f32 = 24.0;
    /// A drag handle's width.
    pub const HANDLE: f32 = 22.0;
    /// A disclosure triangle (an outline's expand and collapse).
    pub const DISCLOSURE: f32 = 9.0;
    /// A count badge (updates waiting on the Library Launcher).
    pub const BADGE: f32 = 16.0;
    /// A toolbar Launcher button's height.
    pub const LAUNCHER: f32 = 30.0;
    /// A Layout card in the Layouts pane, and its thumbnail's height.
    pub const LAYOUT_CARD: super::Vec2 = super::vec2(150.0, 122.0);
    pub const LAYOUT_THUMB: f32 = 78.0;
    /// A Resource's abbreviation in a fixed-width chip, so titles after it line up.
    pub const CHIP_ABBREVIATION: f32 = 58.0;
    /// A grip's dots.
    pub const GRIP_DOT: f32 = 1.2;
    /// A small marker dot; a selected row's bar.
    pub const DOT: f32 = 3.0;
    /// A number field (days, a count).
    pub const FIELD_NUMBER: f32 = 48.0;
    /// A colour's hex field (`#rrggbb`).
    pub const FIELD_HEX: f32 = 80.0;
    /// A narrow text field: a reference, a short filter.
    pub const FIELD_NARROW: f32 = 120.0;
    /// A text field's width when the row does not decide it.
    pub const FIELD: f32 = 180.0;
    /// A wide text field: a name, a search, a rule.
    pub const FIELD_WIDE: f32 = 280.0;
    /// The narrowest a block of laid-out text is given, however small its Panel.
    pub const TEXT_MIN: f32 = 40.0;
    /// The widest a line of chrome prose runs before it wraps (a message, a hint).
    pub const PROSE: f32 = 460.0;
    /// A hover card or a preview pop-up.
    pub const POPOVER: f32 = 360.0;
    /// The text column of a pop-up showing Resource text beside its marker (a Footnote).
    pub const POPOVER_TEXT: f32 = 340.0;
    /// A pop-up's scrolling text: the room it leaves for its header and the window's edges,
    /// and the shortest it gets.
    pub const POPOVER_ROOM: f32 = 120.0;
    pub const POPOVER_BODY_MIN: f32 = 60.0;
    /// How far a toast floats above the window's bottom edge.
    pub const TOAST_RISE: f32 = super::space::XXXL + super::space::XL;
    /// A dialog window's width.
    pub const DIALOG_WIDTH: f32 = 480.0;
    /// A dialog that shows a document to read (a Crash report).
    pub const DIALOG_WIDE: f32 = 640.0;
    /// A scrolling list inside a dialog or a pop-up, at most.
    pub const LIST_MAX: f32 = 260.0;
    /// A menu's column (the right-click menu's targets and actions).
    pub const MENU_COLUMN: f32 = 160.0;
    /// The widest a page of chrome (Settings, Backups) runs, however wide its Tab.
    pub const PAGE_MAX: f32 = 720.0;
    /// A text column in a table (a command's name).
    pub const COLUMN: f32 = 300.0;
    /// A Search Tab at least this wide shows its sidebar by itself.
    pub const SIDEBAR_TAB: f32 = 640.0;
    /// A page wider than this shows its cards in two columns (Home).
    pub const TWO_COLUMNS: f32 = 760.0;
    /// A side list beside a map, chart or editor.
    pub const SIDE_LIST: f32 = 220.0;
    /// A label column (a Resource's abbreviation, a word, a book) in a grid.
    pub const LABEL_COLUMN: f32 = 72.0;
    /// A wide label column (a Lemma, a field name, a key's name).
    pub const LABEL_COLUMN_WIDE: f32 = 120.0;
    /// How far below the window's top a dialog pinned there sits, and the room it leaves
    /// above and below its scrolling content (its title bar, buttons and the window's edges).
    pub const DIALOG_TOP: f32 = 60.0;
    pub const DIALOG_ROOM: f32 = 160.0;
    /// The tallest a dialog's scrolling content opens before it scrolls.
    pub const DIALOG_LIST: f32 = 640.0;
    /// A dialog window's default size.
    pub const DIALOG: super::Vec2 = super::vec2(600.0, 400.0);
    /// A small dialog window's default size.
    pub const DIALOG_SMALL: super::Vec2 = super::vec2(400.0, 400.0);
}

/// Elevation: the shadows of things that float over the Workspace.
pub mod elevation {
    use super::{Color32, Shadow};

    /// A window (a floating Tab, Fonts, Highlight styles).
    pub fn window(dark: bool) -> Shadow {
        Shadow {
            offset: [0, 8],
            blur: 24,
            spread: 0,
            color: Color32::from_black_alpha(if dark { 90 } else { 34 }),
        }
    }

    /// A popover, a menu or a Launcher pane.
    pub fn popup(dark: bool) -> Shadow {
        Shadow {
            offset: [0, 6],
            blur: 18,
            spread: 0,
            color: Color32::from_black_alpha(if dark { 80 } else { 30 }),
        }
    }
}

/// Insets of frames, from the spacing scale.
pub mod inset {
    use super::{Margin, space};

    const fn m(x: f32, y: f32) -> Margin {
        Margin::symmetric(x as i8, y as i8)
    }

    /// A Tab's page: its content stands this far from the Panel's edge.
    pub const PAGE: Margin = m(space::XL, space::L);
    /// A card, a notice or a group on a page.
    pub const CARD: Margin = Margin::same(space::L as i8);
    /// A notice: a one-line band washed in a colour role (an update, offline).
    pub const NOTICE: Margin = m(space::L, space::S);
    /// A popover or a menu.
    pub const POPOVER: Margin = Margin::same(space::M as i8);
    /// A window.
    pub const WINDOW: Margin = Margin::same(space::L as i8);
    /// A bar across a Tab (a header, the Find bar, a view switcher).
    pub const BAR: Margin = m(space::M, space::XS);
    /// A chip's sides.
    pub const CHIP: Margin = m(space::S, 0.0);
    /// A band inside a Tab under its header (Search's views, a chart's controls).
    pub const BAND: Margin = m(space::L, space::XS);
    /// The menu bar.
    pub const MENU_BAR: Margin = m(space::L, space::S);
    /// The Workspace around its Panels.
    pub const WORKSPACE: Margin = Margin::same(space::M as i8);
}

/// Shorthand for [`CornerRadius`] from a radius token.
pub fn corner(r: f32) -> CornerRadius {
    CornerRadius::from(r)
}

/// The chrome's colour roles in one theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chrome {
    pub dark: bool,
    pub shell: Color32,
    pub surface: Color32,
    pub field: Color32,
    pub rule: Color32,
    pub ink: Color32,
    pub muted: Color32,
    pub lapis: Color32,
    pub verdigris: Color32,
    pub violet: Color32,
    pub gilt: Color32,
    pub selection: Color32,
    pub hover: Color32,
    pub hover_rule: Color32,
    pub warn: Color32,
    /// The link chip of a Tab with no Link set.
    pub unlinked: Color32,
}

/// Cool linen surfaces, iron ink, lapis and verdigris (the Study desk, #13).
pub const LIGHT: Chrome = Chrome {
    dark: false,
    shell: Color32::from_rgb(232, 236, 235),
    surface: Color32::from_rgb(244, 246, 244),
    field: Color32::from_rgb(255, 255, 253),
    rule: Color32::from_rgb(205, 213, 211),
    ink: Color32::from_rgb(32, 38, 41),
    // Dark enough for AA on the selection as well as on the surfaces (#182).
    muted: Color32::from_rgb(80, 91, 96),
    lapis: Color32::from_rgb(45, 78, 134),
    // AA as text on the selection too (#182).
    verdigris: Color32::from_rgb(36, 104, 94),
    violet: Color32::from_rgb(112, 79, 145),
    // Deep enough for the star to show on every surface, 3:1 (#182).
    gilt: Color32::from_rgb(150, 112, 30),
    selection: Color32::from_rgb(211, 224, 244),
    hover: Color32::from_rgb(226, 235, 247),
    hover_rule: Color32::from_rgb(166, 184, 211),
    warn: Color32::from_rgb(150, 60, 30),
    unlinked: Color32::from_rgb(154, 162, 164),
};

/// The same desk at night: slate surfaces, light ink, brighter accents.
pub const DARK: Chrome = Chrome {
    dark: true,
    shell: Color32::from_rgb(24, 27, 30),
    surface: Color32::from_rgb(33, 37, 41),
    field: Color32::from_rgb(40, 45, 50),
    rule: Color32::from_rgb(58, 64, 70),
    ink: Color32::from_rgb(222, 226, 228),
    // AA on the hover and selection fills too (#182).
    muted: Color32::from_rgb(160, 170, 176),
    // AA as text on the hover and selection fills too (#182).
    lapis: Color32::from_rgb(132, 168, 226),
    // AA as text on the hover and selection fills too (#182).
    verdigris: Color32::from_rgb(96, 184, 168),
    violet: Color32::from_rgb(160, 128, 200),
    gilt: Color32::from_rgb(206, 168, 88),
    // Deep enough that lapis and muted text on it meet AA (#182).
    selection: Color32::from_rgb(28, 44, 74),
    hover: Color32::from_rgb(48, 56, 66),
    hover_rule: Color32::from_rgb(84, 104, 136),
    warn: Color32::from_rgb(232, 140, 100),
    unlinked: Color32::from_rgb(104, 112, 118),
};

/// The inks [`crate::theme::text_on`] chooses from for a label on a solid fill (with the
/// light theme's ink).
pub const INK_WHITE: Color32 = Color32::WHITE;
pub const INK_BLACK: Color32 = Color32::BLACK;

/// The Atlas's map colours in one theme (#150).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapColors {
    pub water: Color32,
    pub land: Color32,
    pub coast: Color32,
    pub river: Color32,
    /// Behind a place's label, so it reads over coast and river lines.
    pub halo: Color32,
}

pub const MAP_LIGHT: MapColors = MapColors {
    water: Color32::from_rgb(203, 220, 230),
    land: Color32::from_rgb(240, 235, 221),
    coast: Color32::from_rgb(140, 150, 150),
    river: Color32::from_rgb(120, 160, 192),
    halo: Color32::from_rgba_unmultiplied_const(255, 255, 253, alpha::HALO),
};

pub const MAP_DARK: MapColors = MapColors {
    water: Color32::from_rgb(27, 38, 50),
    land: Color32::from_rgb(55, 58, 52),
    coast: Color32::from_rgb(98, 108, 108),
    river: Color32::from_rgb(68, 106, 138),
    halo: Color32::from_rgba_unmultiplied_const(40, 45, 50, alpha::HALO),
};

/// Colours offered beside a Highlight style's hex field (#181): the built-ins' and a few
/// strong ones for lines, boxes and text.
pub const HIGHLIGHT_PRESETS: [(&str, obs_engine::Rgb); 12] = [
    ("Yellow", obs_engine::Rgb::new(255, 236, 140)),
    ("Green", obs_engine::Rgb::new(200, 238, 190)),
    ("Blue", obs_engine::Rgb::new(196, 222, 250)),
    ("Pink", obs_engine::Rgb::new(250, 206, 224)),
    ("Orange", obs_engine::Rgb::new(255, 214, 170)),
    ("Lavender", obs_engine::Rgb::new(222, 210, 250)),
    ("Gold", obs_engine::Rgb::new(201, 162, 39)),
    ("Red", obs_engine::Rgb::new(200, 40, 40)),
    ("Teal", obs_engine::Rgb::new(20, 130, 120)),
    ("Navy", obs_engine::Rgb::new(40, 70, 150)),
    ("Purple", obs_engine::Rgb::new(120, 60, 160)),
    ("Brown", obs_engine::Rgb::new(130, 80, 40)),
];

/// Multiples of the Resource text size, for chrome drawn beside Resource text.
pub mod scale {
    /// A line of Resource text with its leading.
    pub const LINE: f32 = 1.8;
    /// A Lemma shown large (Word Info's headword).
    pub const HEADWORD: f32 = 1.9;
    /// A secondary line under a headword (a transliteration, a gloss).
    pub const SUBHEAD: f32 = 1.3;
    /// A Lemma in a list, a little larger than the text around it.
    pub const LEMMA: f32 = 1.15;
    /// A row of Resource text in a list (Search results, Text Comparison, Analysis): its
    /// line spacing opened a little.
    pub const ROW_LINE: f32 = 1.2;
    /// A label drawn with Resource text, a little smaller than it (a Library hit's title).
    pub const LABEL: f32 = 0.85;
    /// The gap between paragraphs of Resource text in a preview.
    pub const PARAGRAPH_GAP: f32 = 0.6;
    /// A blank line in a Note (the gap plain-text Notes have always had).
    pub const BLANK_LINE: f32 = 0.8;
    /// A Note's headings (levels 1 to 3; deeper ones are body size), and the room above one,
    /// in ems.
    pub const HEADINGS: [f32; 3] = [1.35, 1.2, 1.08];
    pub const HEADING_SPACE: f32 = 0.25;
    /// A placeholder bar for a line still being laid out: its top and height in lines, and
    /// its length in ems when the line's length is not known.
    pub const PLACEHOLDER_TOP: f32 = 0.3;
    pub const PLACEHOLDER_HEIGHT: f32 = 0.35;
    pub const PLACEHOLDER_EMS: f32 = 8.0;
    /// How far a block's placeholder bars stand in from its top and bottom, in lines.
    pub const PLACEHOLDER_INSET: f32 = 0.4;
}

/// Marks drawn on the reading page (#21, #63, #181) and their samples in the chrome, in ems
/// of the Resource text size with a floor in points, so they scale with the text.
pub mod mark {
    /// A Highlight's band: this many ems tall round the line's middle.
    pub const BAND_EMS: f32 = 1.4;
    /// A Background Highlight's corners, and a Note marker's.
    pub const RADIUS: f32 = 1.5;
    /// Words this close (ems) on one line share one underline or box.
    pub const JOIN_EMS: f32 = 0.6;
    /// An Underline Highlight: how far above the band's bottom, and how thick.
    pub const UNDERLINE_DROP_EMS: f32 = 0.1;
    pub const UNDERLINE_EMS: f32 = 0.09;
    /// A Box Highlight's line.
    pub const BOX_EMS: f32 = 0.08;
    /// The thinnest any of those lines gets, in points; and how far an underline sits above
    /// its band's bottom at the least.
    pub const LINE_MIN: f32 = 1.5;
    /// The gap between margin marks beside a verse number, and its floor in points.
    pub const GAP_EMS: f32 = 0.12;
    pub const GAP_MIN: f32 = 2.0;
    /// A margin mark's width, and its floor in points.
    pub const MARGIN_EMS: f32 = 0.16;
    pub const MARGIN_MIN: f32 = 2.5;
    /// A Note marker: a small card raised like a superscript.
    pub const NOTE_WIDTH_EMS: f32 = 0.42;
    pub const NOTE_HEIGHT_EMS: f32 = 0.48;
    /// Its two ruled lines, as fractions of its height.
    pub const NOTE_RULES: [f32; 2] = [0.38, 0.66];
    /// How far round a marker the pointer still hovers it.
    pub const HIT_SLOP: f32 = 3.0;
    /// A Footnote marker's box: the upper share of its line, where the raised glyph sits.
    pub const FOOTNOTE_BOX: f32 = 0.6;
    /// The caret under a difference in Text Comparison: its half-width and its rise, in ems
    /// and in half-widths.
    pub const CARET_EMS: f32 = 0.3;
    pub const CARET_RISE: f32 = 1.4;
    /// A Highlight's sample in the chrome: the band round "Rom 8:28".
    pub const SAMPLE_PAD: super::Vec2 = super::vec2(3.0, 1.0);
    /// A Highlight style's swatch (the selection toolbar, the styles window), in fractions
    /// of its height: a Box's inset, an Underline's height above the bottom, a Text colour's
    /// letter.
    pub const SWATCH_INSET: f32 = 0.22;
    pub const SWATCH_UNDERLINE: f32 = 0.28;
    pub const SWATCH_LETTER: f32 = 0.7;
}

/// Charts and maps (the Timeline, the Atlas, Search's Chart and Grid): their geometry, and
/// how much of a colour role a mark takes when mixed into the field colour.
pub mod chart {
    /// The Timeline's axis: the tick labels, the Eras' band and one row of bars.
    pub const TICKS: f32 = 20.0;
    pub const BAND: f32 = 22.0;
    pub const ROW: f32 = 18.0;
    /// A tick's length into the axis, and the room each tick label is given.
    pub const TICK: f32 = 5.0;
    pub const TICK_SPACING: f32 = 70.0;
    /// A point Event's diamond (half its width), a bar's inset from its row, and the
    /// shortest bar drawn as a bar.
    pub const DIAMOND: f32 = 4.5;
    pub const BAR_INSET: f32 = 3.0;
    /// A Search Chart's bar row, its label column and its longest bar.
    pub const BAR: f32 = 20.0;
    pub const BAR_LABEL: f32 = 130.0;
    pub const BAR_MAX: f32 = 640.0;
    /// A Search Grid's cell.
    pub const CELL: super::Vec2 = super::vec2(64.0, 24.0);
    /// A Grid cell's heat: the faintest share of lapis a cell with a hit takes.
    pub const HEAT_MIN: f32 = 0.15;
    /// The Search Analysis table's fixed columns (Reference, Bible, Lemma, Gloss,
    /// Morphology; Context takes the rest), and its Context column at the least.
    pub const ANALYSIS_COLUMNS: [f32; 5] = [150.0, 64.0, 130.0, 120.0, 180.0];
    pub const ANALYSIS_CONTEXT_MIN: f32 = 260.0;

    /// The shortest an axis gets under its list.
    pub const AXIS_MIN: f32 = 120.0;
    /// A place marker's radius, a strong one's (named by the Verse or picked), and the gap
    /// to the ring round a picked one.
    pub const MARKER: f32 = 5.0;
    pub const MARKER_STRONG: f32 = 6.5;
    pub const RING_GAP: f32 = 3.5;
    /// How near the pointer must be to a marker to hover or click it.
    pub const HIT: f32 = 10.0;
    /// The Factbook's map.
    pub const MINI_MAP: f32 = 240.0;
    /// The room round places when a map fits them.
    pub const MAP_FIT_PAD: f64 = 28.0;
    /// A map's share of a narrow Tab, above its place list.
    pub const MAP_SHARE: f32 = 0.62;
    /// Mixes into the field colour: an Era's shade over the rows, its band, a bar, a point
    /// Event, a grid line (each weak, then strong where the Passage names it).
    pub const SHADE: f32 = 0.05;
    pub const SHADE_STRONG: f32 = 0.1;
    pub const TINT: f32 = 0.16;
    pub const TINT_STRONG: f32 = 0.32;
    pub const POINT: f32 = 0.8;
    pub const GRID: f32 = 0.6;
}

/// Fractions of the room available, for panes and windows that size to their surroundings.
pub mod share {
    /// A side pane beside text (Contents): about a third.
    pub const SIDE_PANE: f32 = 0.36;
    /// A label column: at most this much of its row.
    pub const LABEL: f32 = 0.4;
    /// A list in a window: at most this much of the window's height.
    pub const LIST: f32 = 0.4;
    /// A dialog: at most this much of the window's height or width.
    pub const DIALOG: f32 = 0.75;
}

/// A grid's gaps between columns and rows.
pub const GRID_SPACING: Vec2 = vec2(space::L, space::M);
/// A dense table's gaps (credits, licences).
pub const TABLE_SPACING: Vec2 = vec2(space::L, space::XXS);

/// The item spacing every `Ui` starts with.
pub const ITEM_SPACING: Vec2 = vec2(space::M, space::S);
/// A button's padding.
pub const BUTTON_PADDING: Vec2 = vec2(space::M, space::XS);

#[cfg(test)]
#[path = "design_tokens_tests.rs"]
mod tests;
