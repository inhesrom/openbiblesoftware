//! Word Info (#50): what the Tool shows for one Tagged word, and its hover-or-pin rule (#18
//! rules 15–16). The UI finds the word under the pointer and builds a [`ShownWord`] from its
//! `tagged_word` span; the engine holds what is shown and whether it is pinned.

use obs_model::{MorphemePart, morph, transliterate};
use obs_scripture::StrongsNumber;

/// STEPBible numbers prefixes, suffixes and other grammatical morphemes from H9001 / G9001
/// up; the first part below that is the word's own Lemma.
pub(crate) const AFFIX_NUMBERS_FROM: u32 = 9000;

/// Longest Lexicon gloss shown on a card, in characters.
const EXCERPT_CHARS: usize = 160;

/// The Lexicon gloss of one Strong's number, as [`lexicon_gloss`] finds it for Word Info and
/// the Word Study header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexiconGloss {
    pub resource_id: String,
    pub abbreviation: String,
    /// The entry's headword, used as the Lemma when the Tagged word carries none (KJV).
    pub lemma: Option<String>,
    /// The entry's short gloss ([`lexicon_excerpt`]); `None` when no Lexicon's entry has one
    /// to cut (a cross-reference such as BDB's `אָב v. II. אבה.`): the card then names the
    /// Lexicon and offers to open it at the entry instead of saying there is none.
    pub text: Option<String>,
}

/// The Lexicon gloss of `number` that Word Info and the Word Study header show: every Lexicon
/// in Preferred order, whatever its language (a Greek ★ Lexicon has no H-numbers), each read
/// with [`obs_store::ResourceStore::lookup_strongs`] (the extended number, else its base
/// number; a base number, else its extended numbers). The first entry with a short gloss
/// wins, checking every entry of a Lexicon (homographs H7225G, H7225H) before the next
/// Lexicon; when no entry has one, the first entry found, without text. `None` only when no
/// Lexicon has the number.
#[cfg(not(target_arch = "wasm32"))]
pub fn lexicon_gloss(library: &crate::Library, number: StrongsNumber) -> Option<LexiconGloss> {
    let mut without_gloss = None;
    for lexicon in library.of_type(obs_model::ResourceType::Lexicon) {
        let Ok(found) = crate::word_study::lexicon_entries_in(lexicon, number) else {
            continue;
        };
        for entry in found {
            let gloss = LexiconGloss {
                text: lexicon_excerpt(&entry.text),
                resource_id: entry.resource_id,
                abbreviation: entry.abbreviation,
                lemma: entry.headword,
            };
            if gloss.text.is_some() {
                return Some(gloss);
            }
            without_gloss.get_or_insert(gloss);
        }
    }
    without_gloss
}

#[cfg(target_arch = "wasm32")]
pub fn lexicon_gloss(_: &crate::Library, _: StrongsNumber) -> Option<LexiconGloss> {
    None
}

/// One morpheme's card: Lemma, Strong's number, Morphology in plain English and the Lexicon
/// gloss.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LemmaCard {
    pub strongs: StrongsNumber,
    pub lemma: Option<String>,
    /// Plain English (`Qal perfect, 3rd masc. sing.`), when the code is one we read.
    pub morphology: Option<String>,
    /// The raw code, for a tooltip only.
    pub morph_code: Option<String>,
    /// This morpheme's own gloss from the Bible's tagging.
    pub gloss: Option<String>,
    pub lexicon: Option<LexiconGloss>,
}

/// Where a part other than the main one sits in the word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartRole {
    Prefix,
    Suffix,
}

impl PartRole {
    pub fn name(self) -> &'static str {
        match self {
            PartRole::Prefix => "prefix",
            PartRole::Suffix => "suffix",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordPart {
    pub role: PartRole,
    pub card: LemmaCard,
}

/// What Word Info shows: the word as written, its transliteration, contextual gloss and
/// reference; the main Lemma card; and the prefixes and suffixes under Parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShownWord {
    pub written: String,
    pub rtl: bool,
    /// Only for Hebrew and Greek: an English word has none.
    pub transliteration: Option<String>,
    /// The glosses of every part, in order (`in beginning`).
    pub gloss: Option<String>,
    /// `Genesis 1:1 (TAHOT)`, in the Bible's own numbering.
    pub reference: String,
    pub main: LemmaCard,
    pub parts: Vec<WordPart>,
    /// The Bible it was read in (resource id), for "Search this word".
    pub bible: Option<String>,
}

impl ShownWord {
    /// Build from a `tagged_word` span's parts; `lexicon` answers with the first Lexicon's
    /// gloss for a number. `None` when the span has no parts.
    pub fn new(
        written: &str,
        rtl: bool,
        reference: String,
        parts: &[MorphemePart],
        mut lexicon: impl FnMut(StrongsNumber) -> Option<LexiconGloss>,
    ) -> Option<Self> {
        let main_index = parts
            .iter()
            .position(|p| p.strongs.number() < AFFIX_NUMBERS_FROM)
            .unwrap_or(0);
        let mut cards = parts.iter().map(|p| card(p, &mut lexicon));
        let mut word_parts = Vec::new();
        let mut main = None;
        for (i, c) in cards.by_ref().enumerate() {
            if i == main_index {
                main = Some(c);
            } else {
                word_parts.push(WordPart {
                    role: if i < main_index {
                        PartRole::Prefix
                    } else {
                        PartRole::Suffix
                    },
                    card: c,
                });
            }
        }
        let glosses: Vec<&str> = parts
            .iter()
            .filter_map(|p| p.gloss.as_deref())
            .map(str::trim)
            .filter(|g| !g.is_empty())
            .collect();
        Some(Self {
            written: written.to_string(),
            rtl,
            transliteration: has_original_script(written).then(|| transliterate(written)),
            gloss: (!glosses.is_empty()).then(|| glosses.join(" ")),
            reference,
            main: main?,
            parts: word_parts,
            bible: None,
        })
    }

    /// The Strong's number Word Study ▸ and Search this word act on.
    pub fn strongs(&self) -> StrongsNumber {
        self.main.strongs
    }

    /// Plain text for the Copy button: the word, its gloss and reference, then each card.
    pub fn copy_text(&self) -> String {
        let mut out = self.written.clone();
        if let Some(t) = &self.transliteration {
            out.push_str(&format!(" ({t})"));
        }
        if let Some(g) = &self.gloss {
            out.push_str(&format!(" — {g}"));
        }
        out.push('\n');
        out.push_str(&self.reference);
        out.push('\n');
        out.push_str(&self.main.line());
        for p in &self.parts {
            out.push('\n');
            out.push_str(&format!("{}: {}", p.role.name(), p.card.line()));
        }
        out
    }
}

impl LemmaCard {
    /// One line: `H1254A בָּרָא · Qal perfect, 3rd masc. sing. · TBESH: to create`.
    pub fn line(&self) -> String {
        let mut s = self.strongs.to_string();
        if let Some(l) = self.lemma.as_deref().filter(|l| !l.is_empty()) {
            s.push(' ');
            s.push_str(l);
        }
        if let Some(m) = &self.morphology {
            s.push_str(&format!(" · {m}"));
        }
        match &self.lexicon {
            Some(LexiconGloss {
                abbreviation,
                text: Some(text),
                ..
            }) => s.push_str(&format!(" · {abbreviation}: {text}")),
            Some(l) => s.push_str(&format!(" · {}", l.abbreviation)),
            None => {}
        }
        s
    }
}

fn card(
    part: &MorphemePart,
    lexicon: &mut impl FnMut(StrongsNumber) -> Option<LexiconGloss>,
) -> LemmaCard {
    let lexicon = lexicon(part.strongs);
    let morph_code = part
        .morph
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(str::to_string);
    let morphology = morph_code
        .as_deref()
        .and_then(|c| morph::parse(c).ok())
        .map(|m| m.short())
        .filter(|m| !m.is_empty());
    let lemma = part
        .lemma
        .as_deref()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .or_else(|| lexicon.as_ref().and_then(|l| l.lemma.clone()));
    LemmaCard {
        strongs: part.strongs,
        lemma,
        morphology,
        morph_code,
        gloss: part
            .gloss
            .as_deref()
            .map(str::trim)
            .filter(|g| !g.is_empty())
            .map(str::to_string),
        lexicon,
    }
}

/// Whether `text` has Hebrew or Greek letters (which get a transliteration).
pub fn has_original_script(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(
            c as u32,
            0x0370..=0x03FF | 0x1F00..=0x1FFF | 0x05D0..=0x05EA | 0xFB1D..=0xFB4F
        )
    })
}

/// A Lexicon entry's gloss as a card shows it: its first paragraph after the headword line
/// (every converter writes the lemma and transliteration first), cut to one short line. An
/// entry of one paragraph (most of BDB: `אֵלוֹן n.[f.] terebinth`) starts with its headword
/// too: see [`single_line_gloss`].
pub fn lexicon_excerpt(entry_text: &str) -> Option<String> {
    let mut paragraphs = entry_text.lines().map(str::trim).filter(|l| !l.is_empty());
    let first = paragraphs.next()?;
    let single;
    let gloss = match paragraphs.next() {
        Some(second) => second,
        None => {
            single = single_line_gloss(first)?;
            single.as_str()
        }
    };
    let mut out: String = gloss.chars().take(EXCERPT_CHARS).collect();
    if out.len() < gloss.len() {
        out = out.trim_end().to_string();
        out.push('…');
    }
    Some(out)
}

/// The gloss of a one-paragraph Lexicon entry: without the headword and the grammar
/// abbreviations after it (`[אֵב] n.[m.]`), and up to the first Hebrew or Greek word, which
/// a gloss line's plain label cannot draw (#261: `אֵלוֹן n.[f.] terebinth` was drawn
/// reversed, with boxes for the Hebrew). `None` when nothing is left: a cross-reference
/// such as `אָב v. II. אבה.`.
fn single_line_gloss(line: &str) -> Option<String> {
    let abbreviation = |w: &str| {
        w.contains('.')
            && w.chars().count() <= 12
            && w.chars()
                .all(|c| c.is_alphabetic() || matches!(c, '.' | '[' | ']' | '?' | ','))
    };
    let rest = line
        .split_whitespace()
        .skip_while(|w| {
            has_original_script(w) || abbreviation(w) || w.chars().all(|c| !c.is_alphanumeric())
        })
        .collect::<Vec<_>>()
        .join(" ");
    let cut = rest
        .char_indices()
        .find(|(_, c)| has_original_script(&c.to_string()))
        .map_or(rest.as_str(), |(i, _)| &rest[..i]);
    let gloss = cut.trim_end_matches(|c: char| {
        c.is_whitespace() || matches!(c, ',' | ';' | ':' | '(' | '—' | '-' | '=' | '&')
    });
    (!gloss.is_empty()).then(|| gloss.to_string())
}

/// What Word Info shows and whether it is pinned (per session only, #18).
#[derive(Debug, Default)]
pub struct WordInfo {
    pub shown: Option<ShownWord>,
    /// Clicking a word pins it: hover underlines words but no longer changes the panel.
    pub pinned: bool,
}

impl WordInfo {
    /// A word hovered past the dwell: shown unless the panel is pinned. Returns whether it was.
    pub fn hover(&mut self, word: ShownWord) -> bool {
        if self.pinned {
            return false;
        }
        self.shown = Some(word);
        true
    }

    /// A word clicked: shown and pinned (a later click moves the pin).
    pub fn pin(&mut self, word: ShownWord) {
        self.shown = Some(word);
        self.pinned = true;
    }

    /// Esc, or the 📌 in the header: the panel follows hover again (keeping its content).
    pub fn unpin(&mut self) {
        self.pinned = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(
        strongs: &str,
        lemma: Option<&str>,
        morph: Option<&str>,
        gloss: Option<&str>,
    ) -> MorphemePart {
        MorphemePart {
            strongs: strongs.parse().unwrap(),
            lemma: lemma.map(String::from),
            morph: morph.map(String::from),
            gloss: gloss.map(String::from),
        }
    }

    fn tbesh(n: StrongsNumber) -> Option<LexiconGloss> {
        let (lemma, text) = match n.to_string().as_str() {
            "H1254A" => ("בָּרָא", "to create"),
            "H9003" => ("בְּ", "in"),
            "H7225" => ("רֵאשִׁית", "beginning"),
            "H430" => ("אֱלֹהִים", "God, gods"),
            _ => return None,
        };
        Some(LexiconGloss {
            resource_id: "tbesh".into(),
            abbreviation: "TBESH".into(),
            lemma: Some(lemma.into()),
            text: Some(text.into()),
        })
    }

    #[test]
    fn tahot_bara_shows_strongs_morphology_and_glosses() {
        let w = ShownWord::new(
            "בָּרָ֣א",
            true,
            "Genesis 1:1 (TAHOT)".into(),
            &[part(
                "H1254A",
                Some("בָּרָא"),
                Some("HVqp3ms"),
                Some("he created"),
            )],
            tbesh,
        )
        .unwrap();
        assert_eq!(w.main.strongs.to_string(), "H1254A");
        assert_eq!(
            w.main.morphology.as_deref(),
            Some("Qal perfect, 3rd masc. sing.")
        );
        assert_eq!(w.main.morph_code.as_deref(), Some("HVqp3ms"));
        assert_eq!(w.gloss.as_deref(), Some("he created"));
        assert_eq!(
            w.main.lexicon.as_ref().unwrap().text.as_deref(),
            Some("to create")
        );
        assert_eq!(w.transliteration.as_deref(), Some("bara"));
        assert!(w.parts.is_empty());
        assert_eq!(
            w.copy_text(),
            "בָּרָ֣א (bara) — he created\nGenesis 1:1 (TAHOT)\nH1254A בָּרָא · Qal perfect, 3rd masc. sing. · TBESH: to create"
        );
    }

    #[test]
    fn a_hebrew_prefix_is_listed_under_parts_and_the_word_is_the_main_card() {
        let w = ShownWord::new(
            "בְּרֵאשִׁ֖ית",
            true,
            "Genesis 1:1 (TAHOT)".into(),
            &[
                part("H9003", None, Some("HR"), Some("in")),
                part("H7225", Some("רֵאשִׁית"), Some("HNcfsa"), Some("beginning")),
            ],
            tbesh,
        )
        .unwrap();
        assert_eq!(w.main.strongs.to_string(), "H7225");
        assert_eq!(w.gloss.as_deref(), Some("in beginning"));
        assert_eq!(w.parts.len(), 1);
        assert_eq!(w.parts[0].role, PartRole::Prefix);
        assert_eq!(w.parts[0].card.strongs.to_string(), "H9003");
        assert_eq!(w.parts[0].card.morphology.as_deref(), Some("preposition"));
        // A prefix without its own lemma borrows the Lexicon's headword.
        assert_eq!(w.parts[0].card.lemma.as_deref(), Some("בְּ"));
        assert!(
            w.copy_text()
                .ends_with("\nprefix: H9003 בְּ · preposition · TBESH: in")
        );
    }

    #[test]
    fn a_kjv_word_has_no_transliteration_and_borrows_the_lexicon_lemma() {
        let w = ShownWord::new(
            "God",
            false,
            "Genesis 1:1 (KJV)".into(),
            &[part("H430", None, None, None)],
            tbesh,
        )
        .unwrap();
        assert_eq!(w.written, "God");
        assert_eq!(w.transliteration, None);
        assert_eq!(w.gloss, None);
        assert_eq!(w.main.lemma.as_deref(), Some("אֱלֹהִים"));
        assert_eq!(w.main.morphology, None);
        assert_eq!(
            w.main.lexicon.as_ref().unwrap().text.as_deref(),
            Some("God, gods")
        );
    }

    #[test]
    fn an_unreadable_morph_code_keeps_only_the_tooltip() {
        let w = ShownWord::new(
            "created",
            false,
            "Genesis 1:1 (KJV)".into(),
            &[part("H1254A", None, Some("strongMorph:TH8804"), None)],
            |_| None,
        )
        .unwrap();
        assert_eq!(w.main.morphology, None);
        assert_eq!(w.main.morph_code.as_deref(), Some("strongMorph:TH8804"));
        assert_eq!(w.main.lexicon, None);
        assert_eq!(w.main.line(), "H1254A");
    }

    #[test]
    fn lexicon_excerpt_is_the_paragraph_after_the_headword() {
        assert_eq!(
            lexicon_excerpt("בָּרָא bara\nto create\nKJV: create, choose…").as_deref(),
            Some("to create")
        );
        assert_eq!(lexicon_excerpt("only line").as_deref(), Some("only line"));
        // #261 GUIDES-4: one-paragraph BDB entries lead with their headword and grammar.
        assert_eq!(
            lexicon_excerpt("אֵלוֹן n.[f.] terebinth").as_deref(),
            Some("terebinth")
        );
        assert_eq!(
            lexicon_excerpt("אֹבֵד n.[m.] destruction, עֲדֵי א׳ Nu 24:20").as_deref(),
            Some("destruction")
        );
        assert_eq!(
            lexicon_excerpt("[אֵב] n.[m.] freshness, fresh green (Lag B N 207)").as_deref(),
            Some("freshness, fresh green (Lag B N 207)")
        );
        assert_eq!(lexicon_excerpt("אָב v. II. אבה.").as_deref(), None);
        assert_eq!(lexicon_excerpt("  \n").as_deref(), None);
        let long = format!("head\n{}", "x".repeat(200));
        let e = lexicon_excerpt(&long).unwrap();
        assert_eq!(e.chars().count(), EXCERPT_CHARS + 1);
        assert!(e.ends_with('…'));
    }

    #[test]
    fn pinning_holds_the_shown_word_until_unpinned() {
        let a = ShownWord::new(
            "a",
            false,
            String::new(),
            &[part("G1", None, None, None)],
            |_| None,
        )
        .unwrap();
        let b = ShownWord::new(
            "b",
            false,
            String::new(),
            &[part("G2", None, None, None)],
            |_| None,
        )
        .unwrap();
        let mut w = WordInfo::default();
        assert!(w.hover(a.clone()));
        assert_eq!(w.shown.as_ref().map(|s| s.written.as_str()), Some("a"));
        w.pin(a.clone());
        assert!(w.pinned);
        assert!(!w.hover(b.clone()));
        assert_eq!(w.shown.as_ref().map(|s| s.written.as_str()), Some("a"));
        // Clicking another word moves the pin.
        w.pin(b.clone());
        assert_eq!(w.shown.as_ref().map(|s| s.written.as_str()), Some("b"));
        w.unpin();
        assert!(!w.pinned);
        assert_eq!(w.shown.as_ref().map(|s| s.written.as_str()), Some("b"));
        assert!(w.hover(a));
        assert_eq!(w.shown.as_ref().map(|s| s.written.as_str()), Some("a"));
    }
}
