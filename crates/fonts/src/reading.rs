//! Reading fonts (#138): the face Resource text uses for each script, and whether a face
//! covers a script. The app picks the faces (bundled, or loaded from this computer); layout
//! falls back to the bundled face for any character a chosen face lacks.

use unicode_script::{Script, UnicodeScript};

use crate::face::{Face, FaceId};

/// A script that has its own reading font.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReadingScript {
    /// Latin letters and everything that is not Greek or Hebrew.
    Latin,
    Greek,
    Hebrew,
}

impl ReadingScript {
    pub const ALL: [ReadingScript; 3] = [
        ReadingScript::Latin,
        ReadingScript::Greek,
        ReadingScript::Hebrew,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            ReadingScript::Latin => "Latin",
            ReadingScript::Greek => "Greek",
            ReadingScript::Hebrew => "Hebrew",
        }
    }

    /// The bundled family that draws this script by default and as the fallback.
    pub const fn bundled_family(self) -> &'static str {
        match self {
            ReadingScript::Latin | ReadingScript::Greek => "Gentium Plus",
            ReadingScript::Hebrew => "Ezra SIL",
        }
    }

    /// The script whose reading font draws `c`: Hebrew and Greek characters (letters, marks
    /// and punctuation of those blocks) by their own, every other letter by Latin's. `None`
    /// for characters any script may use (spaces, digits, punctuation, combining marks).
    pub fn of_char(c: char) -> Option<ReadingScript> {
        match c.script() {
            Script::Hebrew => Some(ReadingScript::Hebrew),
            Script::Greek => Some(ReadingScript::Greek),
            Script::Common | Script::Inherited | Script::Unknown => None,
            _ => Some(ReadingScript::Latin),
        }
    }

    /// The letters a face must have to draw the script at all.
    fn letters(self) -> impl Iterator<Item = char> {
        let ranges: &[(u32, u32)] = match self {
            ReadingScript::Latin => &[(0x41, 0x5A), (0x61, 0x7A)],
            // Α–Ω (no U+03A2) and α–ω.
            ReadingScript::Greek => &[(0x391, 0x3A1), (0x3A3, 0x3A9), (0x3B1, 0x3C9)],
            ReadingScript::Hebrew => &[(0x5D0, 0x5EA)],
        };
        ranges
            .iter()
            .flat_map(|&(a, b)| (a..=b).filter_map(char::from_u32))
    }

    /// Characters Bible text in the script uses beyond its letters: accented Latin and
    /// typographic punctuation, polytonic Greek, Hebrew points and cantillation.
    fn extras(self) -> impl Iterator<Item = char> {
        let ranges: &[(u32, u32)] = match self {
            ReadingScript::Latin => &[(0xC0, 0xFF), (0x2018, 0x201D), (0x2014, 0x2014)],
            ReadingScript::Greek => &[
                (0x1F00, 0x1F15),
                (0x1F20, 0x1F45),
                (0x1F50, 0x1F57),
                (0x1F60, 0x1F7D),
                (0x1FB6, 0x1FB7),
                (0x1FC6, 0x1FC7),
                (0x1FF6, 0x1FF7),
            ],
            ReadingScript::Hebrew => &[
                (0x591, 0x5AF),
                (0x5B0, 0x5BD),
                (0x5BF, 0x5BF),
                (0x5C1, 0x5C2),
                (0x5C7, 0x5C7),
                (0x5BE, 0x5BE),
                (0x5C3, 0x5C3),
            ],
        };
        ranges
            .iter()
            .flat_map(|&(a, b)| (a..=b).filter_map(char::from_u32))
    }
}

/// How much of a script a face can draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coverage {
    /// Every letter and every mark or accent Bible text uses.
    Full,
    /// Some of the letters, accents or marks are missing; the bundled face draws those.
    Partial { missing: usize },
    /// None of the script's letters; the bundled face draws all of it.
    None,
}

/// Which of the script's characters `face` has (read from its cmap).
pub fn script_coverage(face: &Face, script: ReadingScript) -> Coverage {
    let letters: Vec<char> = script.letters().collect();
    let have = letters.iter().filter(|&&c| face.covers(c)).count();
    if have == 0 {
        return Coverage::None;
    }
    let missing = letters.len() - have + script.extras().filter(|&c| !face.covers(c)).count();
    if missing == 0 {
        Coverage::Full
    } else {
        Coverage::Partial { missing }
    }
}

/// The four faces of one reading font: upright, italic, bold and bold italic. A family
/// without some of them repeats the nearest it has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FaceSet {
    pub regular: FaceId,
    pub italic: FaceId,
    pub bold: FaceId,
    pub bold_italic: FaceId,
}

impl FaceSet {
    /// The same face for every style.
    pub const fn single(face: FaceId) -> Self {
        Self {
            regular: face,
            italic: face,
            bold: face,
            bold_italic: face,
        }
    }

    /// The bundled faces for a script.
    pub fn bundled(script: ReadingScript) -> Self {
        let s = match script {
            ReadingScript::Hebrew => Script::Hebrew,
            ReadingScript::Greek => Script::Greek,
            ReadingScript::Latin => Script::Latin,
        };
        Self {
            regular: FaceId::for_script(s, false, false),
            italic: FaceId::for_script(s, false, true),
            bold: FaceId::for_script(s, true, false),
            bold_italic: FaceId::for_script(s, true, true),
        }
    }

    pub const fn pick(&self, bold: bool, italic: bool) -> FaceId {
        match (bold, italic) {
            (false, false) => self.regular,
            (false, true) => self.italic,
            (true, false) => self.bold,
            (true, true) => self.bold_italic,
        }
    }

    /// Every face, in style order.
    pub const fn faces(&self) -> [FaceId; 4] {
        [self.regular, self.italic, self.bold, self.bold_italic]
    }
}

/// The faces Resource text is drawn in, per script (View → Fonts, #138).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReadingFaces {
    pub latin: FaceSet,
    pub greek: FaceSet,
    pub hebrew: FaceSet,
}

impl Default for ReadingFaces {
    /// Gentium Plus for Latin and Greek, Ezra SIL for Hebrew (ADR 0004).
    fn default() -> Self {
        Self {
            latin: FaceSet::bundled(ReadingScript::Latin),
            greek: FaceSet::bundled(ReadingScript::Greek),
            hebrew: FaceSet::bundled(ReadingScript::Hebrew),
        }
    }
}

impl ReadingFaces {
    pub const fn get(&self, script: ReadingScript) -> &FaceSet {
        match script {
            ReadingScript::Latin => &self.latin,
            ReadingScript::Greek => &self.greek,
            ReadingScript::Hebrew => &self.hebrew,
        }
    }

    pub fn set(&mut self, script: ReadingScript, faces: FaceSet) {
        match script {
            ReadingScript::Latin => self.latin = faces,
            ReadingScript::Greek => self.greek = faces,
            ReadingScript::Hebrew => self.hebrew = faces,
        }
    }
}
