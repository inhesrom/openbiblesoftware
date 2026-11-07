//! The Settings Tab (#213): one place for every preference, opened with Ctrl+, (`app.settings`).
//!
//! [`SETTINGS`] lists each Setting once, under its [`SettingsSection`], with the command that
//! already changes it (the View menu's, the toolbar's, Home's, Search's …). The Settings Tab
//! reads values from the Session and writes them only by running those commands, so nothing is
//! duplicated and every change applies at once, as it does from the menu.
//!
//! - [`settings_matching`] is the Tab's search: every word of the query in a Setting's label,
//!   section, plain-language keywords or choices.
//! - [`Session::setting_is_default`] tells the Tab when a Setting's Reset has something to do.
//! - `settings.reset {setting}` restores one Setting's default, again by running the existing
//!   commands (`view.theme {value: system}`, `view.text_reset`, `keys.reset_all` …).
//!
//! A later ticket's preference plugs in with one more entry here (and its control in the
//! Tab).

use serde_json::{Value, json};

use crate::appearance::FontSlot;
use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::session::Session;
use crate::toolbar::Launcher;
use crate::workspace::Guide;

/// A heading of the Settings Tab, in Tab order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingsSection {
    TextAndFonts,
    InterfaceSize,
    ThemeAndPageTone,
    ReadingLayout,
    Toolbar,
    KeyboardShortcuts,
    LibraryAndDownloads,
    Search,
    PrivacyAndData,
}

impl SettingsSection {
    pub const ALL: [SettingsSection; 9] = [
        SettingsSection::TextAndFonts,
        SettingsSection::InterfaceSize,
        SettingsSection::ThemeAndPageTone,
        SettingsSection::ReadingLayout,
        SettingsSection::Toolbar,
        SettingsSection::KeyboardShortcuts,
        SettingsSection::LibraryAndDownloads,
        SettingsSection::Search,
        SettingsSection::PrivacyAndData,
    ];

    pub fn title(self) -> &'static str {
        match self {
            SettingsSection::TextAndFonts => "Text and fonts",
            SettingsSection::InterfaceSize => "Interface size",
            SettingsSection::ThemeAndPageTone => "Theme and Page tone",
            SettingsSection::ReadingLayout => "Reading layout defaults",
            SettingsSection::Toolbar => "Toolbar",
            SettingsSection::KeyboardShortcuts => "Keyboard shortcuts",
            SettingsSection::LibraryAndDownloads => "Library folders and downloads",
            SettingsSection::Search => "Search",
            SettingsSection::PrivacyAndData => "Privacy and data",
        }
    }
}

/// How a Setting is shown and changed, and so how it is reset.
#[derive(Debug, Clone, Copy)]
pub enum SettingKind {
    /// One of the exclusive choices of a `Choice` command ([`crate::choices`]), each run as
    /// `{ "value": id }`; `default` is the id Reset runs it with.
    Choice {
        command: &'static str,
        default: &'static str,
    },
    /// On or off. With `param`, the command is run as `{ param: on }`; without, it toggles.
    Toggle {
        command: &'static str,
        param: Option<&'static str>,
        default: bool,
        get: fn(&mut Session) -> bool,
    },
    /// The Text size, a step at a time (`view.text_smaller`, `view.text_larger`).
    TextSize,
    /// The font chosen for a slot (`view.font`); View → Fonts chooses it.
    Font(FontSlot),
    /// Each Notebook's Show in text (`notebook.show_in_text`), on by default.
    NotebooksInText,
    /// The Home arrangement: order and hidden cards (`home.reset_cards`).
    HomeCards,
    /// Each Guide's arrangement of its sections (`guide.reset_arrangement`).
    GuideSections,
    /// Which Launchers the toolbar shows and in what order (Customize toolbar…).
    ToolbarLaunchers,
    /// Every Keyboard shortcut, as Help → Keyboard shortcuts lists them (`keys.*`).
    KeyboardShortcuts,
    /// Where the library is kept. Shown, not changed here; it has no Reset.
    LibraryFolder,
    /// A button that runs a command: something to do, not a value, so it has no Reset.
    Action { command: &'static str },
}

/// One row of the Settings Tab.
#[derive(Debug, Clone, Copy)]
pub struct Setting {
    /// Stable id, as passed to `settings.reset`.
    pub id: &'static str,
    pub section: SettingsSection,
    pub label: &'static str,
    /// The line under the label.
    pub help: &'static str,
    /// Other words the search finds it by ("zoom" for Interface size).
    pub keywords: &'static str,
    pub kind: SettingKind,
}

impl Setting {
    /// Whether it has a default to go back to (Actions and the library folder have none).
    pub fn has_reset(&self) -> bool {
        !matches!(
            self.kind,
            SettingKind::Action { .. } | SettingKind::LibraryFolder
        )
    }
}

const fn setting(
    id: &'static str,
    section: SettingsSection,
    label: &'static str,
    help: &'static str,
    keywords: &'static str,
    kind: SettingKind,
) -> Setting {
    Setting {
        id,
        section,
        label,
        help,
        keywords,
        kind,
    }
}

const fn choice(command: &'static str, default: &'static str) -> SettingKind {
    SettingKind::Choice { command, default }
}

const fn font(id: &'static str, label: &'static str, slot: FontSlot) -> Setting {
    setting(
        id,
        SettingsSection::TextAndFonts,
        label,
        "A bundled font, or one installed on this computer, chosen in the Fonts window",
        "font typeface family reading script",
        SettingKind::Font(slot),
    )
}

use SettingsSection as S;

/// Every Setting, in Tab order.
pub const SETTINGS: &[Setting] = &[
    setting(
        "text_size",
        S::TextAndFonts,
        "Text size",
        "How large Resource text is drawn in every Tab; Interface size is separate",
        "font size bigger smaller larger points zoom",
        SettingKind::TextSize,
    ),
    font("font_latin", "Latin text font", FontSlot::Latin),
    font("font_greek", "Greek text font", FontSlot::Greek),
    font("font_hebrew", "Hebrew text font", FontSlot::Hebrew),
    setting(
        "font_interface",
        S::TextAndFonts,
        "Interface font",
        "The font of menus, labels and buttons; Resource text never uses it",
        "font typeface family ui chrome menus",
        SettingKind::Font(FontSlot::Interface),
    ),
    setting(
        "interface_size",
        S::InterfaceSize,
        "Interface size",
        "One zoom on the whole window: menus, Tabs, headers and dialogs",
        "zoom scale ui menus bigger smaller dpi",
        choice("view.interface_size", "default"),
    ),
    setting(
        "theme",
        S::ThemeAndPageTone,
        "Theme",
        "The colours of the app's chrome",
        "dark mode light colour color appearance night",
        choice("view.theme", "system"),
    ),
    setting(
        "page_tone",
        S::ThemeAndPageTone,
        "Page tone",
        "The colour of Resource pages: reading panes, popups and Guides",
        "paper sepia dark background colour color page night",
        choice("view.page_tone", "auto"),
    ),
    setting(
        "line_spacing",
        S::ReadingLayout,
        "Line spacing",
        "Room between the lines of Resource text",
        "leading line height spacing compact relaxed",
        choice("view.line_spacing", "normal"),
    ),
    setting(
        "verse_numbers",
        S::ReadingLayout,
        "Verse numbers",
        "How Bibles show verse numbers",
        "verse numbers superscript inline hidden",
        choice("view.verse_numbers", "superscript"),
    ),
    setting(
        "red_letter",
        S::ReadingLayout,
        "Red letter",
        "Words of Christ in red, where a Bible marks them",
        "words of christ jesus red letter colour color",
        SettingKind::Toggle {
            command: "view.red_letter",
            param: None,
            default: true,
            get: |s| s.text.red_letter,
        },
    ),
    setting(
        "highlights",
        S::ReadingLayout,
        "Show Highlights",
        "Draw Highlights in Resource text",
        "highlights annotations marks colour color",
        SettingKind::Toggle {
            command: "view.highlights",
            param: None,
            default: true,
            get: |s| s.appearance.show_highlights,
        },
    ),
    setting(
        "notebooks_in_text",
        S::ReadingLayout,
        "Notebooks in text",
        "Which Notebooks' Annotations are drawn in Resource text",
        "notebook notes annotations show in text hide",
        SettingKind::NotebooksInText,
    ),
    setting(
        "open_home_at_launch",
        S::ReadingLayout,
        "Open Home at launch",
        "Home opens on top of the Last session at every launch",
        "home start startup launch dashboard",
        SettingKind::Toggle {
            command: "home.open_at_launch",
            param: Some("on"),
            default: false,
            get: |s| s.home.open_at_launch,
        },
    ),
    setting(
        "home_cards",
        S::ReadingLayout,
        "Home cards",
        "The order of Home's cards and which are hidden; arrange them on Home",
        "home cards arrangement order hidden",
        SettingKind::HomeCards,
    ),
    setting(
        "guide_sections",
        S::ReadingLayout,
        "Guide sections",
        "Each Guide's order of sections, collapsed and hidden ones; arrange them in the Guide",
        "guide sections arrangement passage guide word study factbook exegetical topic",
        SettingKind::GuideSections,
    ),
    setting(
        "toolbar_position",
        S::Toolbar,
        "Toolbar position",
        "The window edge the toolbar sits on",
        "toolbar launchers top bottom left right edge dock",
        choice("view.toolbar_position", "top"),
    ),
    setting(
        "toolbar_auto_hide",
        S::Toolbar,
        "Auto-hide toolbar",
        "Show the toolbar only while the pointer is at its edge or Alt is held",
        "toolbar hide auto-hide autohide",
        SettingKind::Toggle {
            command: "view.toolbar_auto_hide",
            param: None,
            default: false,
            get: |s| s.toolbar.auto_hide,
        },
    ),
    setting(
        "toolbar_labels",
        S::Toolbar,
        "Toolbar buttons",
        "Icons and labels, icons only or labels only",
        "toolbar launchers icons labels text",
        choice("toolbar.labels", "both"),
    ),
    setting(
        "toolbar_launchers",
        S::Toolbar,
        "Launchers",
        "Which Launchers the toolbar shows, and their order",
        "toolbar launchers customize customise library guides tools layouts order",
        SettingKind::ToolbarLaunchers,
    ),
    setting(
        "keyboard_shortcuts",
        S::KeyboardShortcuts,
        "Keyboard shortcuts",
        "Click a shortcut, then press the new key (Esc cancels). Ctrl is ⌘ on macOS.",
        "keys hotkeys keybindings bindings shortcuts",
        SettingKind::KeyboardShortcuts,
    ),
    setting(
        "library_folder",
        S::LibraryAndDownloads,
        "Library folder",
        "Where your Resources are kept on this computer",
        "library folder directory location path storage disk",
        SettingKind::LibraryFolder,
    ),
    setting(
        "get_resources",
        S::LibraryAndDownloads,
        "Get Resources",
        "Download Bibles and other open Resources",
        "download get resources bibles sword catalogue catalog",
        SettingKind::Action {
            command: "library.get_resources",
        },
    ),
    setting(
        "starter_library",
        S::LibraryAndDownloads,
        "Starter library",
        "Which Starter Resources are in your library; add them back or download them",
        "starter library download add back",
        SettingKind::Action {
            command: "library.starter",
        },
    ),
    setting(
        "search_word_forms",
        S::Search,
        "Word forms",
        "Bible and Library searches also find other forms of English words (love → loved)",
        "search word forms stem stemming inflections",
        SettingKind::Toggle {
            command: "search.word_forms",
            param: Some("value"),
            default: false,
            get: Session::search_word_forms,
        },
    ),
    setting(
        "clear_history",
        S::PrivacyAndData,
        "Clear History",
        "Forget every place you have read (History); Favorites are kept",
        "history privacy forget visits delete clear",
        SettingKind::Action {
            command: "history.clear",
        },
    ),
    setting(
        "clear_search_history",
        S::PrivacyAndData,
        "Clear Search history",
        "Forget the searches you have run; Saved searches are kept",
        "search history privacy forget delete clear",
        SettingKind::Action {
            command: "search.clear_history",
        },
    ),
];

/// The Setting with id `id`.
pub fn setting_by_id(id: &str) -> Option<&'static Setting> {
    SETTINGS.iter().find(|s| s.id == id)
}

/// The words a Setting is found by: its label, section, help, keywords and choices.
fn search_text(s: &Session, setting: &Setting) -> String {
    let mut text = format!(
        "{} {} {} {}",
        setting.label,
        setting.section.title(),
        setting.help,
        setting.keywords
    );
    if let SettingKind::Choice { command, .. } = setting.kind {
        for (_, label) in crate::choices(s, command).0 {
            text.push(' ');
            text.push_str(label);
        }
    }
    text.to_lowercase()
}

/// Whether every word of `query` (any case) is in the Setting's words. An empty query
/// matches.
pub fn setting_matches(s: &Session, setting: &Setting, query: &str) -> bool {
    let text = search_text(s, setting);
    query
        .split_whitespace()
        .all(|w| text.contains(&w.to_lowercase()))
}

/// The Settings the Tab's search box finds for `query`, in Tab order.
pub fn settings_matching(s: &Session, query: &str) -> Vec<&'static Setting> {
    (SETTINGS.iter())
        .filter(|setting| setting_matches(s, setting, query))
        .collect()
}

impl Session {
    /// Whether a Setting has its default value, so its Reset has nothing to do. `None` for an
    /// unknown id and for Settings without a Reset.
    pub fn setting_is_default(&mut self, id: &str) -> Option<bool> {
        let setting = setting_by_id(id)?;
        Some(match setting.kind {
            SettingKind::Choice { command, default } => crate::choices(self, command).1 == default,
            SettingKind::Toggle { default, get, .. } => get(self) == default,
            SettingKind::TextSize => self.text.size == obs_layout::TextSettings::default().size,
            SettingKind::Font(slot) => self.fonts.get(slot).is_none(),
            SettingKind::NotebooksInText => {
                (self.notebooks().unwrap_or_default().iter()).all(|n| n.show_in_text)
            }
            SettingKind::HomeCards => self.home.cards == crate::home::HomeSettings::default().cards,
            SettingKind::GuideSections => (Guide::ALL.into_iter()).all(|g| {
                self.guides.arrangement(g) == crate::guide::GuideArrangement::default_for(g)
            }),
            SettingKind::ToolbarLaunchers => {
                self.toolbar.launchers()
                    == crate::toolbar_settings::ToolbarSettings::default().launchers()
            }
            SettingKind::KeyboardShortcuts => {
                !(self.key_bindings().iter()).any(crate::key_bindings::KeyBinding::is_custom)
            }
            SettingKind::LibraryFolder | SettingKind::Action { .. } => return None,
        })
    }
}

fn failed(message: impl ToString) -> CommandError {
    CommandError::Failed {
        command: "settings.reset",
        message: message.to_string(),
    }
}

/// `settings.reset {setting}`: the Setting's default back, through the commands that change
/// it.
fn reset(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let id = p.str("setting").unwrap_or_default();
    let setting = setting_by_id(id).ok_or_else(|| failed(format!("“{id}” is not a Setting")))?;
    match setting.kind {
        SettingKind::Choice { command, default } => {
            s.run(command, &json!({ "value": default }))?;
        }
        SettingKind::Toggle {
            command,
            param,
            default,
            get,
        } => {
            if get(s) != default {
                let params = match param {
                    Some(name) => Value::Object(
                        [(name.to_string(), Value::Bool(default))]
                            .into_iter()
                            .collect(),
                    ),
                    None => Value::Null,
                };
                s.run(command, &params)?;
            }
        }
        SettingKind::TextSize => {
            s.run("view.text_reset", &Value::Null)?;
        }
        SettingKind::Font(slot) => {
            s.run("view.font", &json!({ "slot": slot.id() }))?;
        }
        SettingKind::NotebooksInText => {
            for n in s.notebooks().unwrap_or_default() {
                if !n.show_in_text {
                    s.run(
                        "notebook.show_in_text",
                        &json!({ "id": n.id, "show": true }),
                    )?;
                }
            }
        }
        SettingKind::HomeCards => {
            s.run("home.reset_cards", &Value::Null)?;
        }
        SettingKind::GuideSections => {
            for g in Guide::ALL {
                if s.guides.arrangement(g) != crate::guide::GuideArrangement::default_for(g) {
                    s.run("guide.reset_arrangement", &json!({ "guide": g.id() }))?;
                }
            }
        }
        SettingKind::ToolbarLaunchers => {
            for (to, l) in Launcher::ALL.into_iter().enumerate() {
                s.run(
                    "toolbar.move_launcher",
                    &json!({ "launcher": l.id(), "to": to }),
                )?;
                s.run(
                    "toolbar.show_launcher",
                    &json!({ "launcher": l.id(), "shown": true }),
                )?;
            }
        }
        SettingKind::KeyboardShortcuts => {
            s.run("keys.reset_all", &Value::Null)?;
        }
        SettingKind::LibraryFolder | SettingKind::Action { .. } => {
            return Err(failed(format!("“{}” has no default", setting.label)));
        }
    }
    Ok(json!({ "setting": setting.id }))
}

pub(crate) const COMMANDS: &[CommandSpec] = &[CommandSpec {
    id: "settings.reset",
    label: "Reset a setting",
    shortcut: None,
    params: &[ParamSpec {
        name: "setting",
        kind: ParamKind::String,
        required: true,
        help: "The Setting's id (see the Settings Tab), e.g. `theme` or `text_size`",
    }],
    enabled: always,
    run: reset,
}];
