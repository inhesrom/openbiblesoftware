//! Theme, Page tone, Interface size, Text settings and Fonts (#21, #66, #137, #138): what the
//! View menu changes. Colours and font files themselves live in the UI; the engine keeps the
//! choices, persists them and runs the commands that change them.

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

    /// The tone's name in the View menu and Settings.
    pub fn label(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|t| t.0 == self)
            .map_or("Auto", |t| t.2)
    }

    /// The tone actually shown, given whether the chrome is dark.
    pub fn resolve(self, dark_chrome: bool) -> PageTone {
        match self {
            PageTone::Auto if dark_chrome => PageTone::Dark,
            PageTone::Auto => PageTone::Paper,
            t => t,
        }
    }

    /// A light page the user chose (Paper or Sepia): it stays light in a dark theme (#240).
    pub fn is_chosen_light(self) -> bool {
        matches!(self, PageTone::Paper | PageTone::Sepia)
    }
}

/// Preference key remembering that the Page tone hint was offered (#240): it is offered once
/// per User database.
pub(crate) const PAGE_TONE_HINT_PREF: &str = "page-tone-hint";

/// How large the app chrome is drawn (#137): menus, Tabs, headers and dialogs, as one zoom on
/// the whole window (Resource text scales along). Text size is separate, and its keys never
/// change this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InterfaceSize {
    Smaller,
    #[default]
    Normal,
    Larger,
}

impl InterfaceSize {
    /// Each size, its id, its label and the zoom it draws the window at.
    pub const ALL: [(InterfaceSize, &'static str, &'static str, f32); 3] = [
        (InterfaceSize::Smaller, "smaller", "Smaller", 0.9),
        (InterfaceSize::Normal, "default", "Default", 1.0),
        (InterfaceSize::Larger, "larger", "Larger", 1.25),
    ];

    pub fn id(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|t| t.0 == self)
            .map_or("default", |t| t.1)
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().find(|t| t.1 == id).map(|t| t.0)
    }

    /// The zoom factor the whole window is drawn at.
    pub fn zoom(self) -> f32 {
        Self::ALL.iter().find(|t| t.0 == self).map_or(1.0, |t| t.3)
    }
}

/// Text size limits and step, in points (View → Larger text, Smaller text). Below the
/// minimum text is too small to read; a saved size under it is reset on load (#241).
pub const TEXT_SIZE_MIN: f32 = 12.0;
pub const TEXT_SIZE_MAX: f32 = 40.0;
pub const TEXT_SIZE_STEP: f32 = 1.0;
/// The Text size of a fresh install and of Reset text size (Ctrl+0).
pub const TEXT_SIZE_DEFAULT: f32 = TextSettings::DEFAULT_SIZE;
/// The default before #241. A View record saved without `text_size_default` was saved under
/// it, and any View change saved the size too, so a size equal to it was not chosen.
const TEXT_SIZE_OLD_DEFAULT: f32 = 17.0;

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
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Appearance {
    pub theme: Theme,
    pub page_tone: PageTone,
    pub interface_size: InterfaceSize,
    /// View → Show Highlights (#18 rule 3): Highlights are drawn in Resource text (on by
    /// default). Notes follow their Notebook's Show in text only.
    pub show_highlights: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            page_tone: PageTone::default(),
            interface_size: InterfaceSize::default(),
            show_highlights: true,
        }
    }
}

/// What a font choice is for: one reading font per script, and the interface font.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FontSlot {
    Latin,
    Greek,
    Hebrew,
    Interface,
}

impl FontSlot {
    pub const ALL: [FontSlot; 4] = [
        FontSlot::Latin,
        FontSlot::Greek,
        FontSlot::Hebrew,
        FontSlot::Interface,
    ];

    pub const fn id(self) -> &'static str {
        match self {
            FontSlot::Latin => "latin",
            FontSlot::Greek => "greek",
            FontSlot::Hebrew => "hebrew",
            FontSlot::Interface => "interface",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }

    /// The reading script of the slot; `None` for the interface font.
    pub const fn script(self) -> Option<obs_fonts::ReadingScript> {
        match self {
            FontSlot::Latin => Some(obs_fonts::ReadingScript::Latin),
            FontSlot::Greek => Some(obs_fonts::ReadingScript::Greek),
            FontSlot::Hebrew => Some(obs_fonts::ReadingScript::Hebrew),
            FontSlot::Interface => None,
        }
    }

    /// Label in the Fonts window.
    pub const fn label(self) -> &'static str {
        match self {
            FontSlot::Latin => "Latin text",
            FontSlot::Greek => "Greek text",
            FontSlot::Hebrew => "Hebrew text",
            FontSlot::Interface => "Interface",
        }
    }

    /// What draws the slot when nothing is chosen (or the chosen font is missing).
    pub const fn bundled_family(self) -> &'static str {
        match self.script() {
            Some(s) => s.bundled_family(),
            None => "the bundled interface font",
        }
    }
}

/// A font installed on the computer, chosen by family and style name (`"Charis SIL"`,
/// `"Regular"`). Names, not files, are stored, so the choice still means something on another
/// machine; where the font is missing the bundled one is used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontChoice {
    pub family: String,
    pub style: String,
    /// The user was told this font is missing here (shown once, until it is found again).
    pub missing_noticed: bool,
}

/// The font chosen for each [`FontSlot`]; `None` is the bundled font.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FontChoices {
    slots: [Option<FontChoice>; 4],
}

impl FontChoices {
    pub fn get(&self, slot: FontSlot) -> Option<&FontChoice> {
        self.slots[slot as usize].as_ref()
    }

    pub fn set(&mut self, slot: FontSlot, choice: Option<FontChoice>) {
        self.slots[slot as usize] = choice;
    }

    pub(crate) fn get_mut(&mut self, slot: FontSlot) -> Option<&mut FontChoice> {
        self.slots[slot as usize].as_mut()
    }

    fn to_json(&self) -> Value {
        let mut out = serde_json::Map::new();
        for slot in FontSlot::ALL {
            if let Some(c) = self.get(slot) {
                out.insert(
                    slot.id().into(),
                    json!({
                        "family": c.family,
                        "style": c.style,
                        "missing_noticed": c.missing_noticed,
                    }),
                );
            }
        }
        Value::Object(out)
    }

    fn apply_json(&mut self, v: &Value) {
        for slot in FontSlot::ALL {
            let c = &v[slot.id()];
            if let Some(family) = c["family"].as_str().filter(|f| !f.trim().is_empty()) {
                self.set(
                    slot,
                    Some(FontChoice {
                        family: family.to_string(),
                        style: c["style"].as_str().unwrap_or("Regular").to_string(),
                        missing_noticed: c["missing_noticed"].as_bool().unwrap_or(false),
                    }),
                );
            }
        }
    }
}

/// The preference record (JSON in the User database's `preference` table).
pub(crate) fn to_json(a: &Appearance, t: &TextSettings, f: &FontChoices) -> Value {
    json!({
        "theme": a.theme.id(),
        "page_tone": a.page_tone.id(),
        "interface_size": a.interface_size.id(),
        "text_size": t.size,
        "text_size_default": TEXT_SIZE_DEFAULT,
        "line_spacing": t.line_spacing,
        "red_letter": t.red_letter,
        "verse_numbers": verse_numbers_id(t.verse_numbers),
        "highlights": a.show_highlights,
        "fonts": f.to_json(),
        "reading_layout": serde_json::to_value(t.layout).unwrap_or(Value::Null),
    })
}

/// Apply a saved preference record; unknown or missing values keep their defaults. Records
/// from #37 (a `dark_page` flag and a boolean `verse_numbers`) are read too. Returns the
/// saved Text size when it was too small to read and the default was used instead.
pub(crate) fn apply_json(
    v: &Value,
    a: &mut Appearance,
    t: &mut TextSettings,
    f: &mut FontChoices,
) -> Option<f32> {
    f.apply_json(&v["fonts"]);
    if let Some(x) = v["theme"].as_str().and_then(Theme::from_id) {
        a.theme = x;
    }
    if let Some(x) = v["page_tone"].as_str().and_then(PageTone::from_id) {
        a.page_tone = x;
    } else if v["dark_page"].as_bool() == Some(true) {
        a.page_tone = PageTone::Dark;
    }
    if let Some(x) = v["interface_size"]
        .as_str()
        .and_then(InterfaceSize::from_id)
    {
        a.interface_size = x;
    }
    let mut too_small = None;
    if let Some(size) = v["text_size"].as_f64().map(|s| s as f32) {
        let saved_under = v["text_size_default"]
            .as_f64()
            .map_or(TEXT_SIZE_OLD_DEFAULT, |d| d as f32);
        t.size = if size < TEXT_SIZE_MIN {
            too_small = Some(size);
            TEXT_SIZE_DEFAULT
        } else if size == saved_under {
            TEXT_SIZE_DEFAULT
        } else {
            size.min(TEXT_SIZE_MAX)
        };
    }
    if let Some(ls) = v["line_spacing"].as_f64() {
        t.line_spacing = (ls as f32).clamp(1.0, 2.0);
    }
    if let Some(b) = v["red_letter"].as_bool() {
        t.red_letter = b;
    }
    if let Some(b) = v["highlights"].as_bool() {
        a.show_highlights = b;
    }
    if let Ok(layout) = serde_json::from_value(v["reading_layout"].clone()) {
        t.layout = layout;
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
    too_small
}
