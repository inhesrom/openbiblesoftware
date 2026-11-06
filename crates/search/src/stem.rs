//! English stemming for Bible and Library searches (#180): with the Search Tool's Word forms
//! setting on, a word matches every word of the same stem ("love" → "loved", "loving",
//! "loveth", "lovest").
//!
//! [`stem`] is the Porter stemmer (M. F. Porter, "An algorithm for suffix stripping", 1980,
//! with the two departures of his reference implementation: `-bli` → `-ble` and `-logi` →
//! `-log`) behind an archaic layer for the King James English of the older Bibles: the verb
//! endings `-eth` and `-est` read as `-ing` ("loveth" stems as "loving"), `-edst` as `-ed`, a
//! few irregular forms ("hath", "doth", "saith") as their modern verb, and nouns that merely
//! end in those letters ("priest", "harvest", "teeth") are left alone.
//!
//! Only words of plain ASCII letters are stemmed: Greek and Hebrew words are searched by
//! `lemma:` instead, and any other word stems to itself.

/// Archaic irregular forms → the modern word they stem as.
pub(crate) const IRREGULAR: &[(&str, &str)] = &[
    ("canst", "can"),
    ("couldest", "could"),
    ("couldst", "could"),
    ("didst", "did"),
    ("doest", "do"),
    ("doeth", "do"),
    ("dost", "do"),
    ("doth", "do"),
    ("hadst", "had"),
    ("hast", "have"),
    ("hath", "have"),
    ("saidst", "said"),
    ("saith", "say"),
    ("shouldest", "should"),
    ("shouldst", "should"),
    ("wast", "was"),
    ("wouldest", "would"),
    ("wouldst", "would"),
];

/// Words ending in `-eth` or `-est` that are not archaic verb forms (sorted).
pub(crate) const NOT_ARCHAIC: &[&str] = &[
    "arrest",
    "attest",
    "behest",
    "bequest",
    "conquest",
    "contest",
    "detest",
    "digest",
    "earnest",
    "elisabeth",
    "elizabeth",
    "forest",
    "guest",
    "harvest",
    "honest",
    "inquest",
    "interest",
    "invest",
    "japheth",
    "jetheth",
    "manifest",
    "modest",
    "molest",
    "nazareth",
    "northwest",
    "priest",
    "protest",
    "quest",
    "request",
    "shibboleth",
    "southwest",
    "suggest",
    "teeth",
    "tempest",
    "unrest",
];

/// The stem of one normalized word (lowercase, as [`obs_model::tokenize`] gives it).
pub fn stem(word: &str) -> String {
    if word.len() < 3 || !word.bytes().all(|b| b.is_ascii_lowercase()) {
        return word.to_string();
    }
    porter(&modern(word))
}

/// The modern form an archaic word stems as (`loveth` → `loving`); other words unchanged.
fn modern(word: &str) -> String {
    if let Ok(i) = IRREGULAR.binary_search_by_key(&word, |(w, _)| w) {
        return IRREGULAR[i].1.to_string();
    }
    // Ordinals ("twentieth") are not verbs.
    if NOT_ARCHAIC.binary_search(&word).is_ok() || word.ends_with("tieth") {
        return word.to_string();
    }
    if let Some(base) = word.strip_suffix("edst")
        && base.len() >= 2
    {
        return format!("{base}ed");
    }
    let Some(base) = word
        .strip_suffix("eth")
        .or_else(|| word.strip_suffix("est"))
    else {
        return word.to_string();
    };
    // The rest must be a word: two letters at least, one of them a vowel.
    if base.len() < 2 || !base.bytes().any(|b| b"aeiouy".contains(&b)) {
        return word.to_string();
    }
    if base.ends_with('e') {
        // see + th: "seeth", "seest", "fleeth".
        format!("{base}eing")
    } else if base.ends_with('i') {
        // die + th: "lieth", "diest", "holiest".
        format!("{base}es")
    } else {
        format!("{base}ing")
    }
}

/// The Porter stemmer over a word of ASCII lowercase letters.
fn porter(word: &str) -> String {
    let mut p = Porter {
        b: word.as_bytes().to_vec(),
        k: word.len() as isize - 1,
        j: 0,
    };
    if p.k <= 1 {
        return word.to_string();
    }
    p.step1ab();
    if p.k > 0 {
        p.step1c();
        p.step2();
        p.step3();
        p.step4();
        p.step5();
    }
    p.b.truncate((p.k + 1) as usize);
    String::from_utf8(p.b).unwrap_or_else(|_| word.to_string())
}

/// The word being stemmed: `b[0..=k]`, with `j` the end of the stem a suffix test left.
struct Porter {
    b: Vec<u8>,
    k: isize,
    j: isize,
}

impl Porter {
    fn at(&self, i: isize) -> u8 {
        self.b.get(i as usize).copied().unwrap_or(0)
    }

    fn cons(&self, i: isize) -> bool {
        match self.at(i) {
            b'a' | b'e' | b'i' | b'o' | b'u' => false,
            b'y' => i == 0 || !self.cons(i - 1),
            _ => true,
        }
    }

    /// The number of vowel-consonant sequences in `b[0..=j]`.
    fn m(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        loop {
            if i > self.j {
                return n;
            }
            if !self.cons(i) {
                break;
            }
            i += 1;
        }
        i += 1;
        loop {
            loop {
                if i > self.j {
                    return n;
                }
                if self.cons(i) {
                    break;
                }
                i += 1;
            }
            i += 1;
            n += 1;
            loop {
                if i > self.j {
                    return n;
                }
                if !self.cons(i) {
                    break;
                }
                i += 1;
            }
            i += 1;
        }
    }

    fn vowel_in_stem(&self) -> bool {
        (0..=self.j).any(|i| !self.cons(i))
    }

    fn double_consonant(&self, j: isize) -> bool {
        j >= 1 && self.at(j) == self.at(j - 1) && self.cons(j)
    }

    /// Consonant-vowel-consonant ending at `i`, the last not `w`, `x` or `y`.
    fn cvc(&self, i: isize) -> bool {
        if i < 2 || !self.cons(i) || self.cons(i - 1) || !self.cons(i - 2) {
            return false;
        }
        !matches!(self.at(i), b'w' | b'x' | b'y')
    }

    fn ends(&mut self, s: &str) -> bool {
        let len = s.len() as isize;
        if len > self.k + 1 {
            return false;
        }
        let start = (self.k + 1 - len) as usize;
        if &self.b[start..=self.k as usize] != s.as_bytes() {
            return false;
        }
        self.j = self.k - len;
        true
    }

    fn set_to(&mut self, s: &str) {
        let start = (self.j + 1) as usize;
        self.b.truncate(start);
        self.b.extend_from_slice(s.as_bytes());
        self.k = self.j + s.len() as isize;
    }

    fn replace(&mut self, s: &str) {
        if self.m() > 0 {
            self.set_to(s);
        }
    }

    fn step1ab(&mut self) {
        if self.at(self.k) == b's' {
            if self.ends("sses") {
                self.k -= 2;
            } else if self.ends("ies") {
                self.set_to("i");
            } else if self.at(self.k - 1) != b's' {
                self.k -= 1;
            }
        }
        self.b.truncate((self.k + 1) as usize);
        if self.ends("eed") {
            if self.m() > 0 {
                self.k -= 1;
            }
        } else if (self.ends("ed") || self.ends("ing")) && self.vowel_in_stem() {
            self.k = self.j;
            self.b.truncate((self.k + 1) as usize);
            if self.ends("at") {
                self.set_to("ate");
            } else if self.ends("bl") {
                self.set_to("ble");
            } else if self.ends("iz") {
                self.set_to("ize");
            } else if self.double_consonant(self.k) {
                if !matches!(self.at(self.k), b'l' | b's' | b'z') {
                    self.k -= 1;
                }
            } else {
                self.j = self.k;
                if self.m() == 1 && self.cvc(self.k) {
                    self.j = self.k;
                    self.set_to("e");
                }
            }
        }
        self.b.truncate((self.k + 1) as usize);
    }

    fn step1c(&mut self) {
        if self.ends("y") && self.vowel_in_stem() {
            let k = self.k as usize;
            self.b[k] = b'i';
        }
    }

    /// Try each `(suffix, replacement)`; the first suffix the word ends with decides.
    fn first_of(&mut self, rules: &[(&str, &str)]) {
        for (suffix, to) in rules {
            if self.ends(suffix) {
                self.replace(to);
                return;
            }
        }
    }

    fn step2(&mut self) {
        let rules: &[(&str, &str)] = match self.at(self.k - 1) {
            b'a' => &[("ational", "ate"), ("tional", "tion")],
            b'c' => &[("enci", "ence"), ("anci", "ance")],
            b'e' => &[("izer", "ize")],
            b'l' => &[
                ("bli", "ble"),
                ("alli", "al"),
                ("entli", "ent"),
                ("eli", "e"),
                ("ousli", "ous"),
            ],
            b'o' => &[("ization", "ize"), ("ation", "ate"), ("ator", "ate")],
            b's' => &[
                ("alism", "al"),
                ("iveness", "ive"),
                ("fulness", "ful"),
                ("ousness", "ous"),
            ],
            b't' => &[("aliti", "al"), ("iviti", "ive"), ("biliti", "ble")],
            b'g' => &[("logi", "log")],
            _ => &[],
        };
        self.first_of(rules);
    }

    fn step3(&mut self) {
        let rules: &[(&str, &str)] = match self.at(self.k) {
            b'e' => &[("icate", "ic"), ("ative", ""), ("alize", "al")],
            b'i' => &[("iciti", "ic")],
            b'l' => &[("ical", "ic"), ("ful", "")],
            b's' => &[("ness", "")],
            _ => &[],
        };
        self.first_of(rules);
    }

    fn step4(&mut self) {
        let suffixes: &[&str] = match self.at(self.k - 1) {
            b'a' => &["al"],
            b'c' => &["ance", "ence"],
            b'e' => &["er"],
            b'i' => &["ic"],
            b'l' => &["able", "ible"],
            b'n' => &["ant", "ement", "ment", "ent"],
            b'o' => {
                if self.ends("ion") && self.j >= 0 && matches!(self.at(self.j), b's' | b't') {
                    if self.m() > 1 {
                        self.k = self.j;
                    }
                    return;
                }
                &["ou"]
            }
            b's' => &["ism"],
            b't' => &["ate", "iti"],
            b'u' => &["ous"],
            b'v' => &["ive"],
            b'z' => &["ize"],
            _ => &[],
        };
        if suffixes.iter().any(|s| self.ends(s)) && self.m() > 1 {
            self.k = self.j;
        }
    }

    fn step5(&mut self) {
        self.j = self.k;
        if self.at(self.k) == b'e' {
            let m = self.m();
            if m > 1 || (m == 1 && !self.cvc(self.k - 1)) {
                self.k -= 1;
            }
        }
        if self.at(self.k) == b'l' && self.double_consonant(self.k) && self.m() > 1 {
            self.k -= 1;
        }
    }
}

/// The first letters every word of stem `stem` begins with, so an index can look among its
/// words that begin so instead of stemming them all: Porter changes at most the last two
/// letters of what it keeps, and the archaic layer only changes the end of a word.
pub fn stem_prefix(stem: &str) -> &str {
    let keep = stem.len().saturating_sub(2).max(1).min(stem.len());
    stem.get(..keep).unwrap_or(stem)
}
