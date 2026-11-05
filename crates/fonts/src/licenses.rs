//! The licence files shipped with the bundled fonts, compiled in so Help → Credits can show
//! them in full (the OFL asks that the licence travel with the fonts).

/// One bundled font family's licence file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontLicense {
    /// The family, as credited.
    pub family: &'static str,
    /// The directory holding the font files, relative to the workspace root (the
    /// `ATTRIBUTION.toml` entry's `path` lies inside it).
    pub asset_dir: &'static str,
    /// The licence file's name in that directory.
    pub file: &'static str,
    /// The licence file, verbatim.
    pub text: &'static str,
}

/// Every bundled family's licence file.
pub const FONT_LICENSES: &[FontLicense] = &[
    FontLicense {
        family: "Gentium Plus",
        asset_dir: "crates/fonts/assets/gentium",
        file: "OFL.txt",
        text: include_str!("../assets/gentium/OFL.txt"),
    },
    FontLicense {
        family: "Ezra SIL",
        asset_dir: "crates/fonts/assets/ezra",
        file: "Licenses.txt",
        text: include_str!("../assets/ezra/Licenses.txt"),
    },
];
