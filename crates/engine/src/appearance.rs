//! Theme, Page tone and Text settings (#21, #66): what the View menu changes. Colours
//! themselves live in the UI; the engine keeps the choices, persists them and runs the
//! commands that change them.

use obs_layout::{TextSettings, VerseNumbers};
use serde_json::{Value, json};

/// The app chrome's colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    Light,
    Dark,
    /// Follow the operating system, live (the default).
    #[default]
    System,
}

/// The colour of Resource pages: reading panes, popups, snippets and Guides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PageTone {
    /// Paper with a light theme, Dark with a dark one (the default).
    #[default]
    Auto,
    Paper,
    Sepia,
    Dark,
}

impl Theme {
    pub const ALL: [(Theme, &'static str, &'static str); 3] = [
        (Theme::System, "system", "Follow system"),
        (Theme::Light, "light", "Light"),
        (Theme::Dark, "dark", "Dark"),
    ];

    pub fn id(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|t| t.0 == self)
            .map_or("system", |t| t.1)
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().find(|t| t.1 == id).map(|t| t.0)
    }
}

impl PageTone {
    pub const ALL: [(PageTone, &'static str, &'static str); 4] = [
        (PageTone::Auto, "auto", "Auto"),
        (PageTone::Paper, "paper", "Paper"),
        (PageTone::Sepia, "sepia", "Sepia"),
        (PageTone::Dark, "dark", "Dark"),
    ];

    pub fn id(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|t| t.0 == self)
            .map_or("auto", |t| t.1)
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().find(|t| t.1 == id).map(|t| t.0)
    }

    /// The tone actually shown, given whether the chrome is dark.
    pub fn resolve(self, dark_chrome: bool) -> PageTone {
        match self {
            PageTone::Auto if dark_chrome => PageTone::Dark,
            PageTone::Auto => PageTone::Paper,
            t => t,
        }
    }
}

/// Line spacing choices, as multiples of the tallest font on a line.
pub const LINE_SPACINGS: [(&str, &str, f32); 4] = [
    ("compact", "Compact", 1.1),
    ("normal", "Normal", 1.25),
    ("relaxed", "Relaxed", 1.45),
    ("loose", "Loose", 1.7),
];

pub const VERSE_NUMBER_STYLES: [(VerseNumbers, &str, &str); 3] = [
    (VerseNumbers::Superscript, "superscript", "Superscript"),
    (VerseNumbers::Inline, "inline", "Inline"),
    (VerseNumbers::Hidden, "hidden", "Hidden"),
];

/// The id of the line-spacing choice nearest to `v`.
pub fn line_spacing_id(v: f32) -> &'static str {
    LINE_SPACINGS
        .iter()
        .min_by(|a, b| (a.2 - v).abs().total_cmp(&(b.2 - v).abs()))
        .map_or("normal", |c| c.0)
}

pub fn verse_numbers_id(v: VerseNumbers) -> &'static str {
    VERSE_NUMBER_STYLES
        .iter()
        .find(|c| c.0 == v)
        .map_or("superscript", |c| c.1)
}

/// Everything the View menu sets.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Appearance {
    pub theme: Theme,
    pub page_tone: PageTone,
}

/// The preference record (JSON in the User database's `preference` table).
pub(crate) fn to_json(a: &Appearance, t: &TextSettings) -> Value {
    json!({
        "theme": a.theme.id(),
        "page_tone": a.page_tone.id(),
        "text_size": t.size,
        "line_spacing": t.line_spacing,
        "red_letter": t.red_letter,
        "verse_numbers": verse_numbers_id(t.verse_numbers),
    })
}

/// Apply a saved preference record; unknown or missing values keep their defaults. Records
/// from #37 (a `dark_page` flag and a boolean `verse_numbers`) are read too.
pub(crate) fn apply_json(v: &Value, a: &mut Appearance, t: &mut TextSettings) {
    if let Some(x) = v["theme"].as_str().and_then(Theme::from_id) {
        a.theme = x;
    }
    if let Some(x) = v["page_tone"].as_str().and_then(PageTone::from_id) {
        a.page_tone = x;
    } else if v["dark_page"].as_bool() == Some(true) {
        a.page_tone = PageTone::Dark;
    }
    if let Some(size) = v["text_size"].as_f64() {
        t.size = (size as f32).clamp(10.0, 40.0);
    }
    if let Some(ls) = v["line_spacing"].as_f64() {
        t.line_spacing = (ls as f32).clamp(1.0, 2.0);
    }
    if let Some(b) = v["red_letter"].as_bool() {
        t.red_letter = b;
    }
    match &v["verse_numbers"] {
        Value::String(s) => {
            if let Some(c) = VERSE_NUMBER_STYLES.iter().find(|c| c.1 == s) {
                t.verse_numbers = c.0;
            }
        }
        Value::Bool(false) => t.verse_numbers = VerseNumbers::Hidden,
        _ => {}
    }
}
