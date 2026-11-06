//! Interlinear lines (#143): short lines stacked under each Tagged word of a Bible Tab. An
//! English Bible with Strong's numbers shows a **reverse interlinear** (the Greek or Hebrew
//! behind each English word: Lemma, transliteration, Strong's number, Morphology, Gloss); a
//! Hebrew or Greek Bible shows an **interlinear** (Gloss, transliteration, Morphology, Strong's
//! number under the original text).
//!
//! The engine keeps which lines each Tab shows (saved in Layouts and the Last session) and
//! says what each line reads for a word; the UI looks the words up and lays the lines out.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use obs_model::{MorphemePart, ResourceType, morph, transliterate};
use obs_scripture::StrongsNumber;

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::key_words::is_original_language;
use crate::session::Session;
use crate::word_info::{AFFIX_NUMBERS_FROM, has_original_script};
use crate::workspace::{TabId, TabKind};

/// One interlinear line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterlinearLine {
    Lemma,
    Transliteration,
    Strongs,
    Morphology,
    Gloss,
}

impl InterlinearLine {
    pub const ALL: [InterlinearLine; 5] = [
        InterlinearLine::Lemma,
        InterlinearLine::Transliteration,
        InterlinearLine::Strongs,
        InterlinearLine::Morphology,
        InterlinearLine::Gloss,
    ];

    /// What the Tab header's menu calls it.
    pub fn name(self) -> &'static str {
        match self {
            InterlinearLine::Lemma => "Lemma",
            InterlinearLine::Transliteration => "Transliteration",
            InterlinearLine::Strongs => "Strong's number",
            InterlinearLine::Morphology => "Morphology",
            InterlinearLine::Gloss => "Gloss",
        }
    }

    /// Its id in commands and saved Layouts.
    pub fn id(self) -> &'static str {
        match self {
            InterlinearLine::Lemma => "lemma",
            InterlinearLine::Transliteration => "transliteration",
            InterlinearLine::Strongs => "strongs",
            InterlinearLine::Morphology => "morphology",
            InterlinearLine::Gloss => "gloss",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.id() == id)
    }
}

/// Which way a Bible's interlinear reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InterlinearKind {
    /// An English (or other modern-language) Bible with Strong's numbers: the original
    /// language under the translation.
    Reverse,
    /// A Hebrew, Aramaic or Greek Bible: English under the original text.
    Interlinear,
}

impl InterlinearKind {
    /// The kind for a Bible in `language` (BCP 47).
    pub fn for_language(language: &str) -> Self {
        let original = [
            obs_scripture::StrongsLanguage::Hebrew,
            obs_scripture::StrongsLanguage::Greek,
        ]
        .into_iter()
        .any(|l| is_original_language(language, l));
        if original {
            InterlinearKind::Interlinear
        } else {
            InterlinearKind::Reverse
        }
    }

    /// The lines it offers, top to bottom.
    pub fn lines(self) -> &'static [InterlinearLine] {
        match self {
            InterlinearKind::Reverse => &[
                InterlinearLine::Lemma,
                InterlinearLine::Transliteration,
                InterlinearLine::Strongs,
                InterlinearLine::Morphology,
                InterlinearLine::Gloss,
            ],
            InterlinearKind::Interlinear => &[
                InterlinearLine::Gloss,
                InterlinearLine::Transliteration,
                InterlinearLine::Morphology,
                InterlinearLine::Strongs,
            ],
        }
    }

    /// The lines shown until the user picks others.
    pub fn default_lines(self) -> &'static [InterlinearLine] {
        match self {
            InterlinearKind::Reverse => &[
                InterlinearLine::Lemma,
                InterlinearLine::Transliteration,
                InterlinearLine::Strongs,
            ],
            InterlinearKind::Interlinear => {
                &[InterlinearLine::Gloss, InterlinearLine::Transliteration]
            }
        }
    }
}

/// A Bible Tab's interlinear lines: whether they show, and which ones. Kept per Tab and saved
/// in Layouts and the Last session.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterlinearLines {
    pub on: bool,
    /// The lines the user picked; `None`: the Bible's default lines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chosen: Option<Vec<InterlinearLine>>,
}

impl InterlinearLines {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Whether `line` is picked (shown when the lines are on). A choice made for the other
    /// kind of Bible that names none of this kind's lines (only Lemma, then the Tab switched to
    /// a Hebrew or Greek Bible) reads as this kind's default lines, so the lines never go
    /// empty; the saved choice is kept for switching back.
    pub fn is_chosen(&self, kind: InterlinearKind, line: InterlinearLine) -> bool {
        match &self.chosen {
            Some(chosen) if chosen.iter().any(|l| kind.lines().contains(l)) => {
                chosen.contains(&line)
            }
            _ => kind.default_lines().contains(&line),
        }
    }

    /// The lines shown, top to bottom: none while they are off.
    pub fn shown(&self, kind: InterlinearKind) -> Vec<InterlinearLine> {
        if !self.on {
            return Vec::new();
        }
        kind.lines()
            .iter()
            .copied()
            .filter(|l| self.is_chosen(kind, *l))
            .collect()
    }

    /// Pick or drop a line. The last picked line of `kind` cannot be dropped.
    pub fn set_line(&mut self, kind: InterlinearKind, line: InterlinearLine, shown: bool) {
        let mut chosen: Vec<InterlinearLine> = kind
            .lines()
            .iter()
            .copied()
            .filter(|l| self.is_chosen(kind, *l))
            .collect();
        if shown && !chosen.contains(&line) && kind.lines().contains(&line) {
            chosen.push(line);
        }
        if !shown && chosen.len() > 1 {
            chosen.retain(|l| *l != line);
        }
        chosen.sort();
        self.chosen = Some(chosen);
    }
}

/// What a Lexicon gives a Strong's number for an interlinear line: its headword (the Lemma
/// of an English Bible's Tagged word, which carries only the number) and a short Gloss.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LexiconWord {
    pub lemma: Option<String>,
    pub gloss: Option<String>,
}

/// Longest Gloss an interlinear line shows, in characters.
const CELL_GLOSS_CHARS: usize = 28;

/// The Gloss an interlinear line shows from Lexicon excerpts (in Preferred order) for one
/// number: the first that is already short (a brief lexicon's gloss, such as TBESG's "to
/// love"), else the first one's opening phrase, cut to a short line.
pub fn cell_gloss<'a>(excerpts: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let excerpts: Vec<&str> = excerpts
        .into_iter()
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .collect();
    if let Some(short) = excerpts
        .iter()
        .find(|e| e.chars().count() <= CELL_GLOSS_CHARS)
    {
        return Some((*short).to_string());
    }
    let first = excerpts.first()?;
    let phrase = first
        .split([';', '.'])
        .next()
        .unwrap_or(first)
        .trim()
        .trim_end_matches(',');
    if phrase.chars().count() <= CELL_GLOSS_CHARS {
        return Some(phrase.to_string());
    }
    let mut out = String::new();
    for word in phrase.split_whitespace() {
        if out.chars().count() + word.chars().count() + 1 > CELL_GLOSS_CHARS - 1 {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    if out.is_empty() {
        out = phrase.chars().take(CELL_GLOSS_CHARS - 1).collect();
    }
    out.push('…');
    Some(out)
}

/// What each of `lines` reads under one Tagged word: `written` as it is in the Bible, its
/// `parts` (morphemes), and a Lexicon lookup for numbers whose Lemma or Gloss the Bible does
/// not carry. A line with nothing to show is empty.
pub fn cell_texts(
    lines: &[InterlinearLine],
    kind: InterlinearKind,
    written: &str,
    parts: &[MorphemePart],
    lexicon: &dyn Fn(StrongsNumber) -> Option<LexiconWord>,
) -> Vec<String> {
    let Some(main) = parts
        .iter()
        .find(|p| p.strongs.number() < AFFIX_NUMBERS_FROM)
        .or_else(|| parts.first())
    else {
        return vec![String::new(); lines.len()];
    };
    let trimmed = |s: &Option<String>| {
        s.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let looked_up = std::cell::OnceCell::new();
    let lexicon_word = || looked_up.get_or_init(|| lexicon(main.strongs).unwrap_or_default());
    let lemma = || trimmed(&main.lemma).or_else(|| lexicon_word().lemma.clone());
    lines
        .iter()
        .map(|line| {
            let text = match line {
                InterlinearLine::Lemma => lemma(),
                InterlinearLine::Transliteration => match kind {
                    InterlinearKind::Interlinear => Some(written)
                        .filter(|w| has_original_script(w))
                        .map(transliterate),
                    InterlinearKind::Reverse => lemma()
                        .filter(|l| has_original_script(l))
                        .map(|l| transliterate(&l)),
                },
                InterlinearLine::Strongs => Some(main.strongs.to_string()),
                InterlinearLine::Morphology => main
                    .morph
                    .as_deref()
                    .and_then(|c| morph::parse(c).ok())
                    .map(|m| m.short())
                    .filter(|m| !m.is_empty()),
                InterlinearLine::Gloss => match kind {
                    // Every morpheme's gloss, in order ("in beginning"), as Word Info shows it.
                    InterlinearKind::Interlinear => {
                        let glosses: Vec<String> =
                            parts.iter().filter_map(|p| trimmed(&p.gloss)).collect();
                        (!glosses.is_empty()).then(|| glosses.join(" "))
                    }
                    InterlinearKind::Reverse => {
                        trimmed(&main.gloss).or_else(|| lexicon_word().gloss.clone())
                    }
                },
            };
            text.unwrap_or_default()
        })
        .collect()
}

/// The interlinear kind of the Bible a Tab shows, if it shows a Bible.
pub fn tab_kind(s: &Session, tab: TabId) -> Option<InterlinearKind> {
    let TabKind::Resource(id) = &s.workspace.tab(tab)?.kind else {
        return None;
    };
    let entry = s.library.get(id)?;
    (entry.manifest.resource_type == ResourceType::Bible)
        .then(|| InterlinearKind::for_language(&entry.manifest.language))
}

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

/// `tab.interlinear`: turn a Bible Tab's interlinear lines on or off, and pick a line.
fn set_interlinear(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "tab.interlinear";
    let tab = p
        .i64("tab")
        .and_then(|n| u64::try_from(n).ok())
        .map(TabId)
        .ok_or_else(|| failed(ID, "no tab"))?;
    let kind = tab_kind(s, tab).ok_or_else(|| failed(ID, "this Tab does not show a Bible"))?;
    let line = match p.str("line") {
        Some(id) => Some(
            InterlinearLine::from_id(id)
                .filter(|l| kind.lines().contains(l))
                .ok_or_else(|| failed(ID, format!("this Bible has no {id} line")))?,
        ),
        None => None,
    };
    let Some(t) = s.workspace.tab_mut(tab) else {
        return Err(failed(ID, format!("no Tab {}", tab.0)));
    };
    if let Some(line) = line {
        t.interlinear
            .set_line(kind, line, p.bool("shown").unwrap_or(true));
    }
    match p.bool("on") {
        Some(on) => t.interlinear.on = on,
        // Picking a line shows the lines; otherwise the command toggles them.
        None if line.is_some() => t.interlinear.on = true,
        None => t.interlinear.on = !t.interlinear.on,
    }
    let shown: Vec<&str> = t.interlinear.shown(kind).iter().map(|l| l.id()).collect();
    Ok(json!({ "on": t.interlinear.on, "lines": shown }))
}

pub(crate) const COMMANDS: &[CommandSpec] = &[CommandSpec {
    id: "tab.interlinear",
    label: "Interlinear lines",
    shortcut: None,
    params: &[
        ParamSpec {
            name: "tab",
            kind: ParamKind::Integer,
            required: true,
            help: "The Bible Tab",
        },
        ParamSpec {
            name: "on",
            kind: ParamKind::Boolean,
            required: false,
            help: "Show (true) or hide (false) the lines; without it and `line`, toggle them",
        },
        ParamSpec {
            name: "line",
            kind: ParamKind::String,
            required: false,
            help: "A line to pick or drop: lemma, transliteration, strongs, morphology or gloss",
        },
        ParamSpec {
            name: "shown",
            kind: ParamKind::Boolean,
            required: false,
            help: "With `line`: pick it (true, the default) or drop it (false)",
        },
    ],
    enabled: always,
    run: set_interlinear,
}];
