//! Fonts installed on the computer (#138). The app enumerates and reads them (the desktop app
//! with `fontdb`) and hands the bytes to `obs-fonts`, so the engine, layout and fonts crates
//! stay free of platform font code and build for WASM.

use std::sync::Arc;

/// One installed font family and the names of its styles (`"Regular"`, `"Bold Italic"`…).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledFamily {
    pub name: String,
    pub styles: Vec<String>,
}

/// One face read from an installed font file.
#[derive(Clone)]
pub struct FontFile {
    pub data: Arc<Vec<u8>>,
    /// The face's index in the file (a `.ttc` collection holds several).
    pub index: u32,
    /// Names the face (its file and index): the same key is the same face, loaded once.
    pub key: String,
}

impl std::fmt::Debug for FontFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontFile")
            .field("key", &self.key)
            .field("index", &self.index)
            .field("bytes", &self.data.len())
            .finish()
    }
}

/// Where installed fonts come from.
pub trait SystemFonts: Send + Sync {
    /// Every installed family, sorted by name.
    fn families(&self) -> Vec<InstalledFamily>;

    /// The face of `family` in `style`; with `bold` or `italic`, the family's face nearest to
    /// that style made bold or italic (the style itself when the family has no such face).
    /// `None` when the family is not installed.
    fn load(&self, family: &str, style: &str, bold: bool, italic: bool) -> Option<FontFile>;
}

/// No installed fonts (the web build and tests): only the bundled fonts can be chosen.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoSystemFonts;

impl SystemFonts for NoSystemFonts {
    fn families(&self) -> Vec<InstalledFamily> {
        Vec::new()
    }

    fn load(&self, _: &str, _: &str, _: bool, _: bool) -> Option<FontFile> {
        None
    }
}
