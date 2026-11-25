//! The interface's icons (ADR 0012): the Lucide line set the egui app draws, read from the
//! same files (`crates/ui-egui/assets/icons`, credited in ATTRIBUTION.toml), under the same
//! names. GPUI draws an SVG as a mask filled with the element's text colour, so an icon takes
//! its colour from the theme role it is given.

use std::borrow::Cow;

use gpui_kit::{AssetSource, Hsla, IntoElement, SharedString, Styled, px, svg};

macro_rules! icons {
    ($($variant:ident => $file:literal,)*) => {
        /// One icon of the set, named for what it means here (the egui app's `Icon`).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Icon {
            $($variant,)*
        }

        impl Icon {
            /// Every icon, in catalogue order.
            pub const ALL: &[Icon] = &[$(Icon::$variant,)*];

            /// Its SVG file under `crates/ui-egui/assets/icons/`, without `.svg`.
            pub fn file(self) -> &'static str {
                match self {
                    $(Icon::$variant => $file,)*
                }
            }
        }

        /// The SVG bytes of the icon at `path` (`icons/<file>.svg`), compiled in.
        fn icon_bytes(path: &str) -> Option<&'static [u8]> {
            let file = path.strip_prefix("icons/")?.strip_suffix(".svg")?;
            // Some icons share a file (Link and CrossReferences, Overflow and PopOutRight).
            #[allow(unreachable_patterns)]
            match file {
                $($file => Some(include_bytes!(concat!("../../ui-egui/assets/icons/", $file, ".svg"))),)*
                _ => None,
            }
        }
    };
}

icons! {

    // Controls.
    Close => "x",
    Check => "check",
    CheckCircle => "circle-check",
    Add => "plus",
    Remove => "minus",
    More => "ellipsis",
    Overflow => "chevrons-right",
    // A side Toolbar's pop-out control (#291): towards the left or the right.
    PopOutLeft => "chevrons-left",
    PopOutRight => "chevrons-right",
    Grip => "grip-vertical",
    ChevronLeft => "chevron-left",
    ChevronRight => "chevron-right",
    ChevronUp => "chevron-up",
    ChevronDown => "chevron-down",
    Back => "arrow-left",
    Forward => "arrow-right",
    MoveUp => "arrow-up",
    MoveDown => "arrow-down",
    ZoomIn => "zoom-in",
    ZoomOut => "zoom-out",
    Star => "star",
    StarFilled => "star-filled",
    Pin => "pin",
    Note => "pencil",
    Search => "search",
    Shown => "eye",
    Hidden => "eye-off",
    Warning => "triangle-alert",
    Swatch => "square-filled",
    Contents => "table-of-contents",
    Home => "house",
    Copy => "copy",
    // Modifier keys in shortcuts (#262), drawn as text glyphs by `key_labels`; ⌃ is ChevronUp.
    CommandKey => "command",
    OptionKey => "option",
    ShiftKey => "arrow-big-up",
    // The Note editor's toolbar.
    BulletList => "list",
    Link => "link",
    // Launchers.
    Library => "library",
    Guides => "compass",
    Tools => "layout-grid",
    Layouts => "layout-panel-left",
    Import => "file-up",
    Download => "download",
    Starter => "package",
    // Guides.
    PassageGuide => "book-open",
    ExegeticalGuide => "microscope",
    WordStudy => "whole-word",
    Factbook => "book-user",
    TopicGuide => "tags",
    // Tools.
    TextComparison => "columns-3",
    Notes => "notebook-pen",
    WordInfo => "info",
    CrossReferences => "link",
    History => "history",
    ReadingPlans => "calendar-check",
    Atlas => "map",
    Timeline => "chart-gantt",
    // Guide sections.
    Commentaries => "message-square-text",
    KeyWords => "key-round",
    People => "users",
    Lexicon => "book-a",
    TranslatedAs => "languages",
    Occurrences => "chart-column",
    Forms => "list-tree",
    Summary => "id-card",
    KeyPassages => "bookmark",
    Articles => "book-text",
    Related => "network",
    GrammaticalNotes => "list-checks",
    TopicalEntries => "list",
    LibrarySearch => "text-search",
}

impl Icon {
    /// The asset path GPUI loads it from.
    pub fn path(self) -> SharedString {
        format!("icons/{}.svg", self.file()).into()
    }

    /// The icon at `size` points in `color`.
    pub fn element(self, size: f32, color: impl Into<Hsla>) -> impl IntoElement {
        svg()
            .path(self.path())
            .size(px(size))
            .flex_none()
            .text_color(color.into())
    }
}

/// The app's assets: the icon set, compiled in.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        Ok(icon_bytes(path).map(Cow::Borrowed))
    }

    fn list(&self, path: &str) -> gpui_kit::Result<Vec<SharedString>> {
        Ok(
            if path.trim_end_matches('/') == "icons" || path.is_empty() {
                Icon::ALL.iter().map(|i| i.path()).collect()
            } else {
                Vec::new()
            },
        )
    }
}
