//! The fonts installed on this computer, for View → Fonts (#138), read with `fontdb`.
//!
//! Only the app enumerates and reads font files; `obs-fonts` gets their bytes, so the
//! libraries stay free of platform font code. The system is scanned the first time a font is
//! listed or loaded, not at start-up.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use fontdb::{Database, FaceInfo, Family, Query, Stretch, Style, Weight};
use obs_ui_egui::{FontFile, InstalledFamily, SystemFonts};

pub struct FontdbFonts {
    /// Folders to read instead of the system's (tests).
    dirs: Option<Vec<PathBuf>>,
    db: OnceLock<Database>,
}

impl FontdbFonts {
    /// The fonts installed on this computer.
    pub fn system() -> Self {
        Self {
            dirs: None,
            db: OnceLock::new(),
        }
    }

    /// The fonts in these folders only.
    #[cfg(test)]
    pub fn from_dirs(dirs: Vec<PathBuf>) -> Self {
        Self {
            dirs: Some(dirs),
            db: OnceLock::new(),
        }
    }

    fn db(&self) -> &Database {
        self.db.get_or_init(|| {
            let mut db = Database::new();
            match &self.dirs {
                Some(dirs) => dirs.iter().for_each(|d| db.load_fonts_dir(d)),
                None => db.load_system_fonts(),
            }
            db
        })
    }
}

/// A face's family name: the first one it lists (fontdb puts the English name first).
fn family(face: &FaceInfo) -> Option<&str> {
    face.families.first().map(|(name, _)| name.as_str())
}

/// The style name of a face from its width, weight and slant: "Regular", "Bold Italic",
/// "Condensed Light"…
pub fn style_name(face: &FaceInfo) -> String {
    let mut parts: Vec<String> = Vec::new();
    if face.stretch != Stretch::Normal {
        parts.push(format!("{:?}", face.stretch));
    }
    let weight = match face.weight.0 {
        0..=150 => "Thin",
        151..=250 => "ExtraLight",
        251..=350 => "Light",
        351..=450 => "",
        451..=550 => "Medium",
        551..=650 => "SemiBold",
        651..=750 => "Bold",
        751..=850 => "ExtraBold",
        _ => "Black",
    };
    if !weight.is_empty() {
        parts.push(weight.into());
    }
    match face.style {
        Style::Normal => {}
        Style::Italic => parts.push("Italic".into()),
        Style::Oblique => parts.push("Oblique".into()),
    }
    if parts.is_empty() {
        "Regular".into()
    } else {
        parts.join(" ")
    }
}

impl SystemFonts for FontdbFonts {
    fn families(&self) -> Vec<InstalledFamily> {
        let mut faces: Vec<&FaceInfo> = self.db().faces().filter(|f| family(f).is_some()).collect();
        faces.sort_by_key(|f| (f.stretch.to_number(), f.weight, f.style != Style::Normal));
        let mut out: Vec<InstalledFamily> = Vec::new();
        for f in faces {
            let (Some(name), style) = (family(f), style_name(f)) else {
                continue;
            };
            match out.iter_mut().find(|fam| fam.name == name) {
                Some(fam) if !fam.styles.contains(&style) => fam.styles.push(style),
                Some(_) => {}
                None => out.push(InstalledFamily {
                    name: name.to_string(),
                    styles: vec![style],
                }),
            }
        }
        out.sort_by_key(|f| f.name.to_lowercase());
        out
    }

    fn load(&self, name: &str, style: &str, bold: bool, italic: bool) -> Option<FontFile> {
        let db = self.db();
        let in_family = || db.faces().filter(move |f| family(f) == Some(name));
        let base = in_family().find(|f| style_name(f) == style);
        let id = match (base, bold || italic) {
            (Some(f), false) => f.id,
            _ => {
                // Nearest face of the family (CSS font matching), from the chosen style.
                let (weight, stretch, slant) = base
                    .map_or((Weight::NORMAL, Stretch::Normal, Style::Normal), |f| {
                        (f.weight, f.stretch, f.style)
                    });
                let weight = match (bold, weight.0) {
                    (true, w) if w >= 600 => Weight::BLACK,
                    (true, _) => Weight::BOLD,
                    (false, _) => weight,
                };
                let slant = if italic { Style::Italic } else { slant };
                let id = db.query(&Query {
                    families: &[Family::Name(name)],
                    weight,
                    stretch,
                    style: slant,
                })?;
                // `query` matches any family name a face lists; keep to this family.
                in_family().find(|f| f.id == id)?.id
            }
        };
        let (source, index) = db.face_source(id)?;
        let key = match &source {
            fontdb::Source::File(path) => format!("{}#{index}", path.display()),
            _ => format!("{id:?}"),
        };
        let data = db.with_face_data(id, |data, _| data.to_vec())?;
        Some(FontFile {
            data: Arc::new(data),
            index,
            key,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use obs_fonts::{Coverage, FaceId, ReadingScript, face, load_user_face, script_coverage};

    use super::*;

    /// A folder holding the bundled Gentium Plus files, as if they were installed.
    fn installed(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("obs-fontdb-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for id in [
            FaceId::Regular,
            FaceId::Italic,
            FaceId::Bold,
            FaceId::BoldItalic,
        ] {
            std::fs::write(dir.join(id.file()), id.bundled_bytes().unwrap()).unwrap();
        }
        dir
    }

    #[test]
    fn families_list_their_styles_and_load_by_name() {
        let dir = installed("families");
        let fonts = FontdbFonts::from_dirs(vec![dir.clone()]);
        let families = fonts.families();
        assert_eq!(
            families,
            [InstalledFamily {
                name: "Gentium Plus".into(),
                styles: vec![
                    "Regular".into(),
                    "Italic".into(),
                    "Bold".into(),
                    "Bold Italic".into()
                ],
            }]
        );
        let regular = fonts.load("Gentium Plus", "Regular", false, false).unwrap();
        assert_eq!(
            regular.data.as_slice(),
            FaceId::Regular.bundled_bytes().unwrap()
        );
        assert!(
            regular.key.ends_with("GentiumPlus-Regular.ttf#0"),
            "{}",
            regular.key
        );
        // Variants are the family's own faces.
        let bold_italic = fonts.load("Gentium Plus", "Regular", true, true).unwrap();
        assert_eq!(
            bold_italic.data.as_slice(),
            FaceId::BoldItalic.bundled_bytes().unwrap()
        );
        let italic = fonts.load("Gentium Plus", "Bold", false, true).unwrap();
        assert_eq!(
            italic.data.as_slice(),
            FaceId::BoldItalic.bundled_bytes().unwrap(),
            "italic of the Bold style"
        );
        // A style the family lacks falls back to its nearest face; a family it lacks is missing.
        let light = fonts.load("Gentium Plus", "Light", false, false).unwrap();
        assert_eq!(
            light.data.as_slice(),
            FaceId::Regular.bundled_bytes().unwrap()
        );
        assert!(fonts.load("Charis SIL", "Regular", false, false).is_none());
        // The bytes load as a reading font that has no Hebrew letters.
        let id = load_user_face(regular.data, regular.index).unwrap();
        assert_eq!(
            script_coverage(&face(id).unwrap(), ReadingScript::Hebrew),
            Coverage::None
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The fonts on this computer: every listed family's first style loads (the first 40).
    #[test]
    fn system_fonts_enumerate_and_load() {
        let fonts = FontdbFonts::system();
        let families = fonts.families();
        println!("{} installed families", families.len());
        let mut unusable = Vec::new();
        for f in families.iter().take(40) {
            let style = &f.styles[0];
            let file = fonts
                .load(&f.name, style, false, false)
                .unwrap_or_else(|| panic!("{} {style}", f.name));
            assert!(!file.data.is_empty());
            match load_user_face(file.data, file.index) {
                Ok(id) => {
                    let latin = script_coverage(&face(id).unwrap(), ReadingScript::Latin);
                    let hebrew = script_coverage(&face(id).unwrap(), ReadingScript::Hebrew);
                    println!("{} {style}: Latin {latin:?}, Hebrew {hebrew:?}", f.name);
                }
                Err(_) => unusable.push(f.name.clone()),
            }
        }
        // Bitmap-only or otherwise odd files may be refused; ordinary fonts must load.
        assert!(unusable.len() * 2 <= families.len().min(40), "{unusable:?}");
    }
}
