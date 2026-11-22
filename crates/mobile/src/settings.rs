//! The Mobile app's settings: Theme, Page tone, Text size (and Dynamic Type), Reading fonts
//! and Reduce motion. Each change runs the View menu's command, as Settings on the desktop
//! does, so it is saved in the User database and applies to the readers at once.

use obs_engine::Session;
use obs_engine::appearance::{FontSlot, PageTone, TEXT_SIZE_MAX, TEXT_SIZE_MIN, Theme};
use serde_json::json;

use crate::app::{LibraryResource, failed};
use crate::{MobileApp, MobileError, ReaderId};

/// One choice of a setting, and its name in the shell's menus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub id: String,
    pub label: String,
}

/// A setting's choices, in menu order, and the one in use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choices {
    pub choices: Vec<Choice>,
    pub current: String,
}

/// The Reading font of one script (CONTEXT.md: Reading font).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadingFont {
    /// `latin`, `greek` or `hebrew`.
    pub slot: String,
    pub label: String,
    /// The family chosen; `None` is the Bundled font.
    pub family: Option<String>,
    /// The Bundled font's family, which draws the script when none is chosen.
    pub bundled_family: String,
}

/// Every setting the Mobile app shows.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub theme: Choices,
    pub page_tone: Choices,
    /// In points.
    pub text_size: f32,
    pub text_size_min: f32,
    pub text_size_max: f32,
    pub reduce_motion: bool,
    pub reading_fonts: Vec<ReadingFont>,
}

/// The Text size for UIKit's content size category `category` (Dynamic Type), from
/// [`crate::tokens::DYNAMIC_TYPE`]; an unknown category is the default Text size.
pub fn text_size_for_content_size(category: &str) -> f32 {
    (crate::tokens::DYNAMIC_TYPE.iter())
        .find(|(c, _)| *c == category)
        .map_or(obs_engine::appearance::TEXT_SIZE_DEFAULT, |(_, size)| *size)
        .clamp(TEXT_SIZE_MIN, TEXT_SIZE_MAX)
}

/// Whether the chrome is dark: the Theme's own ground, or, following the system, the system
/// appearance `system_dark`. The Auto Page tone is Dark on a dark chrome.
pub(crate) fn chrome_is_dark(session: &Session, system_dark: bool) -> bool {
    fn dark(theme: Theme, system_dark: bool) -> Option<bool> {
        match theme {
            Theme::System => Some(system_dark),
            Theme::Light | Theme::HighContrastLight => Some(false),
            Theme::Dark | Theme::HighContrastDark => Some(true),
            Theme::Custom => None,
        }
    }
    let theme = session.appearance.theme;
    dark(theme, system_dark)
        .or_else(|| dark(session.custom_theme.base, system_dark))
        .unwrap_or(system_dark)
}

fn choices<T: Copy + PartialEq>(all: &[(T, &str, &str)], now: T) -> Choices {
    Choices {
        choices: (all.iter())
            .map(|(_, id, label)| Choice {
                id: (*id).to_string(),
                label: (*label).to_string(),
            })
            .collect(),
        current: (all.iter())
            .find(|(t, _, _)| *t == now)
            .map(|(_, id, _)| (*id).to_string())
            .unwrap_or_default(),
    }
}

/// The reading slots, in the Fonts window's order.
const READING_SLOTS: [FontSlot; 3] = [FontSlot::Latin, FontSlot::Greek, FontSlot::Hebrew];

impl MobileApp {
    /// Every setting as it is now.
    pub fn settings(&self) -> Settings {
        let s = &self.session;
        Settings {
            theme: choices(&Theme::ALL, s.appearance.theme),
            page_tone: choices(&PageTone::ALL, s.appearance.page_tone),
            text_size: s.text.size,
            text_size_min: TEXT_SIZE_MIN,
            text_size_max: TEXT_SIZE_MAX,
            reduce_motion: s.appearance.reduce_motion,
            reading_fonts: (READING_SLOTS.iter())
                .map(|slot| ReadingFont {
                    slot: slot.id().to_string(),
                    label: slot.label().to_string(),
                    family: s.fonts.get(*slot).map(|c| c.family.clone()),
                    bundled_family: slot.bundled_family().to_string(),
                })
                .collect(),
        }
    }

    /// Use Theme `id` (one of [`Settings::theme`]).
    pub fn set_theme(&mut self, id: &str) -> Result<Settings, MobileError> {
        self.run_setting("view.theme", json!({ "value": id }))
    }

    /// Use Page tone `id` (one of [`Settings::page_tone`]).
    pub fn set_page_tone(&mut self, id: &str) -> Result<Settings, MobileError> {
        self.run_setting("view.page_tone", json!({ "value": id }))
    }

    /// Use Reduce motion, or not.
    pub fn set_reduce_motion(&mut self, on: bool) -> Result<Settings, MobileError> {
        self.run_setting("view.reduce_motion", json!({ "on": on }))
    }

    /// Choose the Reading font of slot `slot` (`latin`, `greek`, `hebrew`) by family name;
    /// `None` is the Bundled font. The choice is kept; the shell supplies the font's faces.
    pub fn set_reading_font(
        &mut self,
        slot: &str,
        family: Option<&str>,
    ) -> Result<Settings, MobileError> {
        if !(READING_SLOTS.iter()).any(|s| s.id() == slot) {
            return Err(MobileError::Failed(format!(
                "“{slot}” is not a Reading font"
            )));
        }
        self.run_setting(
            "view.font",
            json!({ "slot": slot, "family": family.unwrap_or_default() }),
        )
    }

    /// Use Text size `size` points (within 12–40), saved with the View preferences.
    pub fn set_text_size(&mut self, size: f32) -> Result<Settings, MobileError> {
        if !size.is_finite() {
            return Err(MobileError::Failed(format!("{size} is not a Text size")));
        }
        self.session.text.size = size.clamp(TEXT_SIZE_MIN, TEXT_SIZE_MAX);
        let saved = self.session.save_view_preferences();
        self.apply_settings();
        saved.map_err(|e| MobileError::Failed(format!("Could not save the Text size: {e}")))?;
        Ok(self.settings())
    }

    /// Follow the system's text size (Dynamic Type): the Text size becomes the one for
    /// UIKit's content size category `category` ([`text_size_for_content_size`]), for this run
    /// (the system keeps the choice). Returns it.
    pub fn dynamic_type(&mut self, category: &str) -> f32 {
        let size = text_size_for_content_size(category);
        if self.session.text.size != size {
            self.session.text.size = size;
            self.apply_settings();
        }
        size
    }

    /// The Resources reader `reader` can switch to (CONTEXT.md: Parallel resources): every
    /// Resource of its Resource type, in Preferred order, its own among them.
    pub fn parallel_resources(
        &self,
        reader: ReaderId,
    ) -> Result<Vec<LibraryResource>, MobileError> {
        let ty = self.reader_at(reader)?.view.doc.manifest.resource_type;
        Ok((self.library().into_iter())
            .filter(|r| r.resource_type == ty)
            .collect())
    }

    fn run_setting(
        &mut self,
        command: &str,
        params: serde_json::Value,
    ) -> Result<Settings, MobileError> {
        self.session.run(command, &params).map_err(failed)?;
        self.apply_settings();
        self.apply_effects();
        Ok(self.settings())
    }

    /// The settings changed: each reader takes its Text settings again (a new Text size lays
    /// it out again at the same place), and every tile is drawn again.
    pub(crate) fn apply_settings(&mut self) {
        for r in &mut self.readers {
            let id = r.view.doc.manifest.resource_id.clone();
            let settings = self.session.text_settings_of(&id);
            if settings != *r.view.settings() {
                r.view.set_settings(settings);
                r.frame.moved = true;
            }
        }
        self.new_epoch();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_content_size_table_stays_in_the_text_size_range() {
        assert_eq!(text_size_for_content_size("UICTContentSizeCategoryL"), 20.0);
        assert_eq!(text_size_for_content_size("unknown"), 20.0);
        let all: Vec<&str> = crate::tokens::DYNAMIC_TYPE
            .iter()
            .map(|(c, _)| *c)
            .collect();
        let sizes: Vec<f32> = all.iter().map(|c| text_size_for_content_size(c)).collect();
        assert!(sizes.windows(2).all(|p| p[0] < p[1]), "{sizes:?}");
        assert_eq!(sizes.first(), Some(&15.0));
        assert_eq!(sizes.last(), Some(&TEXT_SIZE_MAX));
        assert!(
            sizes
                .iter()
                .all(|s| (TEXT_SIZE_MIN..=TEXT_SIZE_MAX).contains(s))
        );
    }

    #[test]
    fn the_chrome_is_dark_by_the_theme_or_the_system() {
        let mut s = Session::new(obs_engine::JobMode::Inline);
        assert!(!chrome_is_dark(&s, false));
        assert!(chrome_is_dark(&s, true), "Follow system");
        s.appearance.theme = Theme::Light;
        assert!(!chrome_is_dark(&s, true));
        s.appearance.theme = Theme::HighContrastDark;
        assert!(chrome_is_dark(&s, false));
        s.appearance.theme = Theme::Custom;
        s.custom_theme.base = Theme::Dark;
        assert!(chrome_is_dark(&s, false), "a Custom theme from Dark");
    }
}
