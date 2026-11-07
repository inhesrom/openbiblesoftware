//! The transliteration Interlinear lines show (#243): an academic (SBL) scheme for reading,
//! with long vowels, vocal sheva, spirants and doubled consonants marked — בְּרֵאשִׁית
//! `bᵊrēʾšîṯ`, הַשָּׁמַיִם `haššāmayim`, ἀρχή `archē`, υἱός `huios`. Searching keeps the plain
//! ASCII [`crate::transliterate`].
//!
//! - **Hebrew:** א ʾ, ב b/ḇ, ג g/ḡ, ד d/ḏ, ה h, ו w, ז z, ח ḥ, ט ṭ, י y, כ k/ḵ, ל l, מ m, נ n,
//!   ס s, ע ʿ, פ p/p̄, צ ṣ, ק q, ר r, שׁ š, שׂ ś, ת t/ṯ (the stop with dagesh, else the
//!   spirant); a dagesh after a vowel doubles its consonant. Vowels patah a, qamats ā, segol e,
//!   tsere ē, hiriq i, holam ō, qubuts u, hataf patah ă, hataf segol ĕ, hataf qamats ŏ; vowel
//!   letters hiriq-yod î, tsere- and segol-yod ê, holam-vav ô, shureq û, final qamats-he â.
//!   Sheva is vocal (ᵊ) at the start of a word, under a doubled consonant, after another
//!   sheva or a long vowel, and before the same consonant; otherwise silent. A furtive patah
//!   is read before its final consonant (rûaḥ). Accents (cantillation) are ignored.
//! - **Greek:** α a, β b, γ g (n before γ κ ξ χ), δ d, ε e, ζ z, η ē, θ th, ι i, κ k, λ l,
//!   μ m, ν n, ξ x, ο o, π p, ρ r (rh with rough breathing), σ/ς s, τ t, υ y (u in a
//!   diphthong), φ ph, χ ch, ψ ps, ω ō; iota subscript ą ę̄ ǭ; a rough breathing adds `h`; a
//!   capital stays a capital.
//! - Anything else passes through unchanged.

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// Hebrew accents and the marks that only guide chanting: removed for display by
/// [`without_cantillation`].
fn is_cantillation(c: char) -> bool {
    matches!(c, '\u{0591}'..='\u{05AF}' | '\u{05BD}' | '\u{05C0}' | '\u{05C3}' | '\u{05C4}' | '\u{05C5}')
}

/// Hebrew text without its accents (cantillation), meteg, paseq and sof pasuq, keeping
/// consonants, vowel points, dagesh and maqaf: בְּרֵאשִׁ֖ית → בְּרֵאשִׁית. Other text is
/// unchanged.
pub fn without_cantillation(text: &str) -> String {
    text.chars().filter(|c| !is_cantillation(*c)).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Script {
    Greek,
    Hebrew,
    Other,
}

fn script(c: char) -> Script {
    match c as u32 {
        0x0370..=0x03FF | 0x1F00..=0x1FFF => Script::Greek,
        0x0590..=0x05FF | 0xFB1D..=0xFB4F => Script::Hebrew,
        _ => Script::Other,
    }
}

/// A base character with the combining marks that follow it.
struct Cluster {
    base: char,
    marks: Vec<char>,
}

impl Cluster {
    fn has(&self, mark: char) -> bool {
        self.marks.contains(&mark)
    }
}

/// Transliterate Greek and Hebrew for reading, in the academic scheme of this module's docs.
pub fn transliterate_academic(text: &str) -> String {
    let mut out = String::new();
    let mut run: Vec<Cluster> = Vec::new();
    let mut run_script = Script::Other;
    let flush = |run: &mut Vec<Cluster>, s: Script, out: &mut String| {
        match s {
            Script::Greek => greek_word(run, out),
            Script::Hebrew => hebrew_word(run, out),
            Script::Other => {
                for c in run.iter() {
                    out.push(c.base);
                    out.extend(c.marks.iter());
                }
            }
        }
        run.clear();
    };
    for c in text.nfd() {
        if is_combining_mark(c) {
            match run.last_mut() {
                Some(last) => last.marks.push(c),
                // A mark with nothing to sit on passes through (outside Greek and Hebrew).
                None if script(c) == Script::Other => out.push(c),
                None => {}
            }
            continue;
        }
        let s = if c.is_alphabetic() {
            script(c)
        } else {
            Script::Other
        };
        if s != run_script || s == Script::Other {
            flush(&mut run, run_script, &mut out);
            run_script = s;
        }
        run.push(Cluster {
            base: c,
            marks: Vec::new(),
        });
    }
    flush(&mut run, run_script, &mut out);
    // Combine what NFD split (ā, ē, ō with their macrons) back into single characters.
    out.nfc().collect()
}

const ROUGH: char = '\u{0314}';
const DIAERESIS: char = '\u{0308}';
const IOTA_SUBSCRIPT: char = '\u{0345}';

fn greek_word(clusters: &[Cluster], out: &mut String) {
    let lower: Vec<char> = clusters
        .iter()
        .map(|c| c.base.to_lowercase().next().unwrap_or(c.base))
        .collect();
    let capital = clusters.first().is_some_and(|c| c.base.is_uppercase());
    let mut word = String::new();
    let is_vowel = |c: char| matches!(c, 'α' | 'ε' | 'η' | 'ι' | 'ο' | 'υ' | 'ω');
    if lower.first().is_some_and(|&c| is_vowel(c)) && clusters.iter().take(2).any(|c| c.has(ROUGH))
    {
        word.push('h');
    }
    for (i, &c) in lower.iter().enumerate() {
        let next = lower.get(i + 1).copied();
        let prev = i.checked_sub(1).map(|p| lower[p]);
        let cl = &clusters[i];
        let subscript = cl.has(IOTA_SUBSCRIPT);
        let s = match c {
            'α' if subscript => "ą",
            'α' => "a",
            'β' => "b",
            'γ' if matches!(next, Some('γ' | 'κ' | 'ξ' | 'χ')) => "n",
            'γ' => "g",
            'δ' => "d",
            'ε' => "e",
            'ζ' => "z",
            'η' if subscript => "ę̄",
            'η' => "ē",
            'θ' => "th",
            'ι' => "i",
            'κ' => "k",
            'λ' => "l",
            'μ' => "m",
            'ν' => "n",
            'ξ' => "x",
            'ο' => "o",
            'π' => "p",
            'ρ' if i == 0 && cl.has(ROUGH) => "rh",
            'ρ' => "r",
            'σ' | 'ς' | 'ϲ' => "s",
            'τ' => "t",
            // A diphthong (αυ, ευ, ηυ, ου, υι) unless a diaeresis parts it.
            'υ' if !cl.has(DIAERESIS)
                && (prev.is_some_and(|p| matches!(p, 'α' | 'ε' | 'η' | 'ο'))
                    || (next == Some('ι') && !clusters[i + 1].has(DIAERESIS))) =>
            {
                "u"
            }
            'υ' => "y",
            'φ' => "ph",
            'χ' => "ch",
            'ψ' => "ps",
            'ω' if subscript => "ǭ",
            'ω' => "ō",
            _ => "",
        };
        word.push_str(s);
    }
    if capital {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    } else {
        out.push_str(&word);
    }
}

const DAGESH: char = '\u{05BC}';
const SHIN_DOT: char = '\u{05C1}';
const SIN_DOT: char = '\u{05C2}';
const HOLAM: char = '\u{05B9}';
const HOLAM_FOR_VAV: char = '\u{05BA}';

/// A Hebrew vowel point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Vowel {
    Sheva,
    HatafSegol,
    HatafPatah,
    HatafQamats,
    Hiriq,
    Tsere,
    Segol,
    Patah,
    Qamats,
    Holam,
    Qubuts,
    QamatsQatan,
}

impl Vowel {
    fn of(cluster: &Cluster) -> Option<Vowel> {
        cluster.marks.iter().find_map(|m| {
            Some(match m {
                '\u{05B0}' => Vowel::Sheva,
                '\u{05B1}' => Vowel::HatafSegol,
                '\u{05B2}' => Vowel::HatafPatah,
                '\u{05B3}' => Vowel::HatafQamats,
                '\u{05B4}' => Vowel::Hiriq,
                '\u{05B5}' => Vowel::Tsere,
                '\u{05B6}' => Vowel::Segol,
                '\u{05B7}' => Vowel::Patah,
                '\u{05B8}' => Vowel::Qamats,
                '\u{05B9}' | '\u{05BA}' => Vowel::Holam,
                '\u{05BB}' => Vowel::Qubuts,
                '\u{05C7}' => Vowel::QamatsQatan,
                _ => return None,
            })
        })
    }

    fn text(self) -> &'static str {
        match self {
            Vowel::Sheva => "ᵊ",
            Vowel::HatafSegol => "ĕ",
            Vowel::HatafPatah => "ă",
            Vowel::HatafQamats => "ŏ",
            Vowel::Hiriq => "i",
            Vowel::Tsere => "ē",
            Vowel::Segol => "e",
            Vowel::Patah => "a",
            Vowel::Qamats => "ā",
            Vowel::Holam => "ō",
            Vowel::Qubuts => "u",
            Vowel::QamatsQatan => "o",
        }
    }
}

/// The vowel sound a consonant ends with, as far as the next one cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sound {
    /// No vowel: a silent sheva, or none written.
    None,
    /// A vocal sheva.
    Vocal,
    Short,
    Long,
}

fn hebrew_consonant(c: char, dagesh: bool, sin: bool) -> &'static str {
    match (c, dagesh) {
        ('א', _) => "ʾ",
        ('ב', true) => "b",
        ('ב', false) => "ḇ",
        ('ג', true) => "g",
        ('ג', false) => "ḡ",
        ('ד', true) => "d",
        ('ד', false) => "ḏ",
        ('ה', _) => "h",
        ('ו', _) => "w",
        ('ז', _) => "z",
        ('ח', _) => "ḥ",
        ('ט', _) => "ṭ",
        ('י', _) => "y",
        ('כ' | 'ך', true) => "k",
        ('כ' | 'ך', false) => "ḵ",
        ('ל', _) => "l",
        ('מ' | 'ם', _) => "m",
        ('נ' | 'ן', _) => "n",
        ('ס', _) => "s",
        ('ע', _) => "ʿ",
        ('פ' | 'ף', true) => "p",
        ('פ' | 'ף', false) => "p̄",
        ('צ' | 'ץ', _) => "ṣ",
        ('ק', _) => "q",
        ('ר', _) => "r",
        ('ש', _) if sin => "ś",
        ('ש', _) => "š",
        ('ת', true) => "t",
        ('ת', false) => "ṯ",
        _ => "",
    }
}

/// A final form's letter (ך → כ), to compare consonants.
fn plain_letter(c: char) -> char {
    match c {
        'ך' => 'כ',
        'ם' => 'מ',
        'ן' => 'נ',
        'ף' => 'פ',
        'ץ' => 'צ',
        c => c,
    }
}

/// Whether a letter carries a vowel that is read: a vowel point other than sheva, or a vav
/// read as holam or shureq.
fn reads_vowel(c: &Cluster) -> bool {
    Vowel::of(c).is_some_and(|v| v != Vowel::Sheva)
        || (c.base == 'ו' && (c.has(DAGESH) || c.has(HOLAM) || c.has(HOLAM_FOR_VAV)))
}

fn hebrew_word(clusters: &[Cluster], out: &mut String) {
    let n = clusters.len();
    let mut word = String::new();
    // The vowel last written and where it starts in `word`, for vowel letters that lengthen it.
    let mut last: Option<(Vowel, usize)> = None;
    let mut sound = Sound::None;
    for (i, cl) in clusters.iter().enumerate() {
        let vowel = Vowel::of(cl);
        let dagesh = cl.has(DAGESH);
        let final_letter = i + 1 == n;
        let after_vowel = matches!(sound, Sound::Short | Sound::Long | Sound::Vocal);
        // Vowel letters: hiriq-yod, tsere-yod, segol-yod.
        if cl.base == 'י' && vowel.is_none() && !dagesh {
            let long = match last {
                Some((Vowel::Hiriq, at)) if sound == Sound::Short => Some((at, "î")),
                Some((Vowel::Tsere | Vowel::Segol, at)) if after_vowel => Some((at, "ê")),
                _ => None,
            };
            if let Some((at, long)) = long {
                word.truncate(at);
                word.push_str(long);
                last = None;
                sound = Sound::Long;
                continue;
            }
        }
        if cl.base == 'ו' {
            let holam = cl.has(HOLAM) || cl.has(HOLAM_FOR_VAV);
            // Holam-vav after a consonant without a vowel; shureq likewise.
            if !after_vowel && i > 0 && holam && !dagesh {
                word.push('ô');
                last = None;
                sound = Sound::Long;
                continue;
            }
            if !after_vowel && dagesh && (vowel.is_none()) {
                word.push('û');
                last = None;
                sound = Sound::Long;
                continue;
            }
        }
        // A final he without mappiq is a vowel letter.
        if cl.base == 'ה' && final_letter && vowel.is_none() && !dagesh && after_vowel {
            match last {
                Some((Vowel::Qamats, at)) => {
                    word.truncate(at);
                    word.push('â');
                }
                _ => word.push('h'),
            }
            continue;
        }
        let consonant = hebrew_consonant(cl.base, dagesh, cl.has(SIN_DOT) && !cl.has(SHIN_DOT));
        // Dagesh after a vowel doubles the consonant (a mappiq in a final he does not).
        let doubled = dagesh && after_vowel && sound != Sound::Vocal && cl.base != 'ה';
        // A furtive patah is read before its final consonant.
        if final_letter && vowel == Some(Vowel::Patah) && matches!(cl.base, 'ח' | 'ע' | 'ה') && after_vowel
        {
            word.push('a');
            word.push_str(consonant);
            continue;
        }
        if doubled {
            word.push_str(consonant);
        }
        word.push_str(consonant);
        sound = match vowel {
            Some(Vowel::Sheva) => {
                let next_same = clusters
                    .get(i + 1)
                    .is_some_and(|next| plain_letter(next.base) == plain_letter(cl.base));
                let after_silent_sheva = sound == Sound::None
                    && i > 0
                    && Vowel::of(&clusters[i - 1]) == Some(Vowel::Sheva);
                let vocal =
                    i == 0 || doubled || after_silent_sheva || sound == Sound::Long || next_same;
                // No vowel follows (a final letter, or the one before a final letter without
                // a vowel, as in וַיֵּשְׁתְּ): nothing is read after it, so it is silent.
                let closes = !clusters[i + 1..].iter().any(reads_vowel);
                if vocal && !closes {
                    word.push_str(Vowel::Sheva.text());
                    last = Some((Vowel::Sheva, word.len() - Vowel::Sheva.text().len()));
                    Sound::Vocal
                } else {
                    last = None;
                    Sound::None
                }
            }
            Some(v) => {
                last = Some((v, word.len()));
                word.push_str(v.text());
                match v {
                    Vowel::Qamats | Vowel::Tsere | Vowel::Holam => Sound::Long,
                    _ => Sound::Short,
                }
            }
            None => {
                last = None;
                Sound::None
            }
        };
    }
    out.push_str(&word);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hebrew_words_read_academically() {
        for (hebrew, expected) in [
            ("בְּרֵאשִׁית", "bᵊrēʾšîṯ"),
            ("בְּרֵאשִׁ֖ית", "bᵊrēʾšîṯ"),
            ("בָּרָא", "bārāʾ"),
            ("אֱלֹהִים", "ʾĕlōhîm"),
            ("הַשָּׁמַיִם", "haššāmayim"),
            ("הָאָֽרֶץ׃", "hāʾāreṣ׃"),
            ("וְאֵת", "wᵊʾēṯ"),
            ("שָׁלוֹם", "šālôm"),
            ("רוּחַ", "rûaḥ"),
            ("תּוֹרָה", "tôrâ"),
            ("מֶלֶךְ", "meleḵ"),
            ("יִשְׂרָאֵל", "yiśrāʾēl"),
            ("וַיֹּאמֶר", "wayyōʾmer"),
            ("חֶסֶד", "ḥeseḏ"),
            ("דָּבָר", "dāḇār"),
            ("נֶפֶשׁ", "nep̄eš"),
            ("הַלְלוּ", "halᵊlû"),
            ("בֵּית", "bêṯ"),
        ] {
            assert_eq!(transliterate_academic(hebrew), expected, "{hebrew}");
        }
    }

    #[test]
    fn greek_words_read_academically() {
        for (greek, expected) in [
            ("ἀρχή", "archē"),
            ("Ἐν ἀρχῇ ἦν ὁ λόγος", "En archę̄ ēn ho logos"),
            ("θεός", "theos"),
            ("ἀγάπη", "agapē"),
            ("υἱός", "huios"),
            ("ἄγγελος", "angelos"),
            ("Ἰησοῦς", "Iēsous"),
            ("ῥῆμα", "rhēma"),
            ("εὐαγγέλιον", "euangelion"),
            ("ψυχή", "psychē"),
            ("αἷμα", "haima"),
            ("ζωῷ", "zōǭ"),
        ] {
            assert_eq!(transliterate_academic(greek), expected, "{greek}");
        }
    }

    #[test]
    fn other_text_passes_through() {
        assert_eq!(transliterate_academic("Grace, χάρις!"), "Grace, charis!");
        assert_eq!(transliterate_academic("עַל־פְּנֵי"), "ʿal־pᵊnê");
    }

    #[test]
    fn cantillation_comes_off_and_points_stay() {
        assert_eq!(without_cantillation("בְּרֵאשִׁ֖ית"), "בְּרֵאשִׁית");
        assert_eq!(without_cantillation("הָאָֽרֶץ׃"), "הָאָרֶץ");
        assert_eq!(without_cantillation("עַל־"), "עַל־");
        assert_eq!(without_cantillation("ἀρχῇ"), "ἀρχῇ");
    }
}
