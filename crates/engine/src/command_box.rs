//! The Command box (#139): the toolbar box that reads what the user types and suggests what to
//! do with it. The UI only draws [`Suggestion`]s and runs `command_box.run`; what the text
//! means is decided here, from the reference parser, Strong's numbers, the Library's Lexicons
//! (Lemma Keys and their transliteration index), Resource titles, Layouts and Favorites.
//!
//! What a text suggests, best first (ties keep this order):
//!
//! | Text | Suggestion |
//! |---|---|
//! | a Layout's name, exactly | restore that Layout |
//! | `pg`/`guide`/`passage guide` + reference; `compare`/`tc` + reference; `xref` + reference; `ws`/`study`/`word study` + word; `open` + Resource; `search` + text | that Guide, Tool, Resource or Search |
//! | a Resource's abbreviation or title, exactly | open it |
//! | a Bible reference (`jn 3:16`, also `jn 3 16`) | go there in the focused Bible (else the most recent Bible Tab, else the first Bible in Preferred order) |
//! | a Strong's number (`G26`) | Word Study |
//! | a Lemma (`ἀγάπη`) or its transliteration (`agape`) in a Lexicon | Word Study at the entry's Strong's number |
//! | a menu command's name (`notes`) | run it |
//! | part of a Resource's abbreviation or title, a Layout's name, a Favorite | open it |
//! | anything | Search (today's toolbar behaviour); when nothing else matched, also Word Study for English words |
//!
//! An empty box lists the recent entries: texts run from the box, newest first, kept in the
//! User database. Running a recent entry runs that text's best suggestion again.

use obs_model::ResourceType;
use obs_scripture::{Passage, RefStyle, Standard, StrongsNumber, format_passage, parse_reference};
use serde_json::{Value, json};

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::session::Session;
use crate::workspace::{Effect, TabId, TabKind};

/// Preference key of the recent entries (a JSON array of texts, newest first).
pub const RECENT_PREF: &str = "command-box.recent";

/// How many recent entries are kept.
pub const RECENT_LIMIT: usize = 20;

/// How many suggestions the box shows at most.
pub const SUGGESTION_LIMIT: usize = 8;

/// What running a [`Suggestion`] does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandBoxAction {
    /// Show the Passage (Standard versification) in a Bible.
    GoTo(Passage),
    PassageGuide(Passage),
    TextComparison(Passage),
    CrossReferences(Passage),
    WordStudy(StrongsNumber),
    /// The Word Study's English-word chooser.
    WordStudyWords(String),
    /// Open (or focus) a Resource, by resource id.
    OpenResource(String),
    RestoreLayout(String),
    OpenFavorite(i64),
    /// Run a menu command with no parameters.
    Command(&'static str),
    Search(String),
    /// A recent entry: run this text's best suggestion.
    Recent(String),
}

impl CommandBoxAction {
    /// A short stable name, for the JSON form and tests.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::GoTo(_) => "go_to",
            Self::PassageGuide(_) => "passage_guide",
            Self::TextComparison(_) => "text_comparison",
            Self::CrossReferences(_) => "cross_references",
            Self::WordStudy(_) => "word_study",
            Self::WordStudyWords(_) => "word_study_words",
            Self::OpenResource(_) => "resource",
            Self::RestoreLayout(_) => "layout",
            Self::OpenFavorite(_) => "favorite",
            Self::Command(_) => "command",
            Self::Search(_) => "search",
            Self::Recent(_) => "recent",
        }
    }
}

/// One line of the Command box's dropdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub action: CommandBoxAction,
    /// What it is about: `John 3:16`, `ἀγάπη · G26`, `BSB — Berean Standard Bible`.
    pub label: String,
    /// What running it does: `Go to · BSB`, `Word Study`, `Open Bible`.
    pub detail: String,
}

impl Suggestion {
    fn new(action: CommandBoxAction, label: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            action,
            label: label.into(),
            detail: detail.into(),
        }
    }

    pub fn to_json(&self) -> Value {
        json!({ "kind": self.action.kind(), "label": self.label, "detail": self.detail })
    }
}

/// The recent entries as loaded from the User database (`None` until first asked for).
#[derive(Debug, Default)]
pub struct CommandBox {
    recent: Option<Vec<String>>,
}

impl CommandBox {
    /// Read the recent entries again from the next User database asked.
    pub(crate) fn forget_recent(&mut self) {
        self.recent = None;
    }
}

/// Leading words that name a Guide, Tool or action, longest first so `passage guide` wins
/// over `guide`.
const VERBS: &[(&str, Verb)] = &[
    ("cross-references", Verb::CrossReferences),
    ("cross references", Verb::CrossReferences),
    ("text comparison", Verb::Compare),
    ("passage guide", Verb::PassageGuide),
    ("word study", Verb::WordStudy),
    ("compare", Verb::Compare),
    ("search", Verb::Search),
    ("guide", Verb::PassageGuide),
    ("study", Verb::WordStudy),
    ("xrefs", Verb::CrossReferences),
    ("xref", Verb::CrossReferences),
    ("open", Verb::Open),
    ("pg", Verb::PassageGuide),
    ("ws", Verb::WordStudy),
    ("tc", Verb::Compare),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verb {
    PassageGuide,
    Compare,
    CrossReferences,
    WordStudy,
    Open,
    Search,
}

/// Ranks: higher shows first.
mod rank {
    pub const LAYOUT_EXACT: u8 = 100;
    pub const VERB: u8 = 95;
    pub const RESOURCE_EXACT: u8 = 92;
    pub const REFERENCE: u8 = 90;
    pub const STRONGS: u8 = 90;
    pub const COMMAND_EXACT: u8 = 85;
    pub const LEMMA: u8 = 80;
    pub const RESOURCE_PART: u8 = 65;
    pub const LAYOUT_PART: u8 = 60;
    pub const FAVORITE: u8 = 55;
    pub const COMMAND_PART: u8 = 50;
    pub const SEARCH: u8 = 10;
    pub const ENGLISH_WORDS: u8 = 5;
}

/// Read `text` as a Bible reference in the Standard versification: its first Passage. A
/// chapter and verse written with a space (`jn 3 16`) read as `jn 3:16`.
pub fn parse_passage(text: &str) -> Option<Passage> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let first = |t: &str| {
        parse_reference(t, &Standard, None)
            .ok()
            .and_then(|l| l.passages().first().copied())
    };
    first(text).or_else(|| {
        let words: Vec<&str> = text.split_whitespace().collect();
        let [book @ .., chapter, verse] = words.as_slice() else {
            return None;
        };
        let numeric = |w: &str| !w.is_empty() && w.chars().all(|c| c.is_ascii_digit());
        if book.is_empty() || !numeric(chapter) || !numeric(verse) {
            return None;
        }
        first(&format!("{} {chapter}:{verse}", book.join(" ")))
    })
}

/// The leading word or words naming a Guide, Tool or action, and the rest of the text.
fn split_verb(text: &str) -> Option<(Verb, &str)> {
    VERBS.iter().find_map(|(word, verb)| {
        // Compare the verbs (ASCII) in place: lowercasing the whole text can change byte
        // lengths (`İ`), which would misplace where the rest starts.
        let head = text.get(..word.len())?;
        let rest = text.get(word.len()..)?;
        if !head.eq_ignore_ascii_case(word)
            || !rest.starts_with(char::is_whitespace)
            || rest.trim().is_empty()
        {
            return None;
        }
        Some((*verb, rest.trim()))
    })
}

fn written(p: Passage) -> String {
    format_passage(p, &Standard, RefStyle::Long)
}

pub(crate) fn type_name(t: ResourceType) -> &'static str {
    match t {
        ResourceType::Bible => "Bible",
        ResourceType::Commentary => "Commentary",
        ResourceType::Lexicon => "Lexicon",
        ResourceType::Dictionary => "Dictionary",
        ResourceType::CrossReferenceSet => "Cross-reference set",
        ResourceType::PeopleAndPlaces => "People & places index",
    }
}

/// Whether each word of `needle` (lowercase) starts a word of `hay`, or `hay` starts with
/// `needle`: `extended strongs` finds “Brief lexicon of Extended Strongs for Greek”.
fn word_prefix(hay: &str, needle: &str) -> bool {
    let hay = hay.to_lowercase();
    let words: Vec<&str> = hay
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    hay.starts_with(needle)
        || needle
            .split(|c: char| !c.is_alphanumeric())
            .any(|n| !n.is_empty())
            && needle
                .split(|c: char| !c.is_alphanumeric())
                .filter(|n| !n.is_empty())
                .all(|n| words.iter().any(|w| w.starts_with(n)))
}

impl Session {
    /// What the Command box suggests for `text`, best first (at most [`SUGGESTION_LIMIT`]).
    /// An empty text lists the recent entries.
    pub fn command_box_suggestions(&mut self, text: &str) -> Vec<Suggestion> {
        let text = text.trim();
        if text.is_empty() {
            return self
                .command_box_recent()
                .into_iter()
                .take(SUGGESTION_LIMIT)
                .map(|t| Suggestion::new(CommandBoxAction::Recent(t.clone()), t, "Recent"))
                .collect();
        }
        let mut ranked: Vec<(u8, Suggestion)> = Vec::new();
        self.layout_suggestions(text, &mut ranked);
        if let Some((verb, rest)) = split_verb(text) {
            self.verb_suggestions(verb, rest, &mut ranked);
        }
        self.resource_suggestions(text, &mut ranked);
        if let Some(p) = parse_passage(text) {
            ranked.push((rank::REFERENCE, self.go_to_suggestion(p)));
        }
        ranked.extend(self.word_suggestions(text, rank::STRONGS, rank::LEMMA));
        self.command_suggestions(text, &mut ranked);
        if text.chars().count() >= 2 {
            for f in self.favorites_matching(text) {
                ranked.push((
                    rank::FAVORITE,
                    Suggestion::new(
                        CommandBoxAction::OpenFavorite(f.id),
                        f.label.clone(),
                        "Favorite",
                    ),
                ));
            }
        }
        // The English-word chooser only when nothing else understood the text.
        let understood = !ranked.is_empty();
        ranked.push((
            rank::SEARCH,
            Suggestion::new(
                CommandBoxAction::Search(text.to_string()),
                format!("Search for “{text}”"),
                "Search",
            ),
        ));
        if !understood && is_english_words(text) {
            ranked.push((
                rank::ENGLISH_WORDS,
                Suggestion::new(
                    CommandBoxAction::WordStudyWords(text.to_string()),
                    format!("“{text}”"),
                    "Word Study (English)",
                ),
            ));
        }
        // Stable: equal ranks keep the order they were added in.
        ranked.sort_by_key(|r| std::cmp::Reverse(r.0));
        let mut out: Vec<Suggestion> = Vec::new();
        for (_, s) in ranked {
            if !out.iter().any(|o| o.action == s.action) {
                out.push(s);
            }
        }
        // Search is always offered: when the list is full, it keeps the last slot.
        let search = CommandBoxAction::Search(text.to_string());
        let search_at = out.iter().position(|s| s.action == search);
        if let Some(i) = search_at.filter(|&i| i >= SUGGESTION_LIMIT) {
            let kept = out.remove(i);
            out.truncate(SUGGESTION_LIMIT - 1);
            out.push(kept);
        }
        out.truncate(SUGGESTION_LIMIT);
        out
    }

    fn layout_suggestions(&self, text: &str, out: &mut Vec<(u8, Suggestion)>) {
        let names = self
            .user
            .as_ref()
            .and_then(|u| u.layouts().ok())
            .unwrap_or_default();
        let needle = text.to_lowercase();
        for name in names {
            let r = if name.to_lowercase() == needle {
                rank::LAYOUT_EXACT
            } else if needle.chars().count() >= 2 && word_prefix(&name, &needle) {
                rank::LAYOUT_PART
            } else {
                continue;
            };
            out.push((
                r,
                Suggestion::new(
                    CommandBoxAction::RestoreLayout(name.clone()),
                    name,
                    "Layout",
                ),
            ));
        }
    }

    fn verb_suggestions(&mut self, verb: Verb, rest: &str, out: &mut Vec<(u8, Suggestion)>) {
        let at = |make: fn(Passage) -> CommandBoxAction, what: &str| {
            parse_passage(rest).map(|p| Suggestion::new(make(p), written(p), what))
        };
        match verb {
            Verb::PassageGuide => {
                out.extend(
                    at(CommandBoxAction::PassageGuide, "Passage Guide").map(|s| (rank::VERB, s)),
                );
            }
            Verb::Compare => {
                out.extend(
                    at(CommandBoxAction::TextComparison, "Text Comparison")
                        .map(|s| (rank::VERB, s)),
                );
            }
            Verb::CrossReferences => {
                out.extend(
                    at(CommandBoxAction::CrossReferences, "Cross-references")
                        .map(|s| (rank::VERB, s)),
                );
            }
            Verb::WordStudy => {
                let words = self.word_suggestions(rest, rank::VERB, rank::VERB);
                if words.is_empty() && is_english_words(rest) {
                    out.push((
                        rank::VERB,
                        Suggestion::new(
                            CommandBoxAction::WordStudyWords(rest.to_string()),
                            format!("“{rest}”"),
                            "Word Study (English)",
                        ),
                    ));
                }
                out.extend(words);
            }
            Verb::Open => {
                let mut found = Vec::new();
                self.resource_suggestions(rest, &mut found);
                out.extend(found.into_iter().map(|(_, s)| (rank::VERB, s)));
            }
            Verb::Search => out.push((
                rank::VERB,
                Suggestion::new(
                    CommandBoxAction::Search(rest.to_string()),
                    format!("Search for “{rest}”"),
                    "Search",
                ),
            )),
        }
    }

    fn resource_suggestions(&self, text: &str, out: &mut Vec<(u8, Suggestion)>) {
        let needle = text.to_lowercase();
        let mut parts = Vec::new();
        for e in self.library.entries() {
            let m = &e.manifest;
            let exact = m.abbreviation.to_lowercase() == needle || m.title.to_lowercase() == needle;
            let part = needle.chars().count() >= 2
                && (word_prefix(&m.abbreviation, &needle) || word_prefix(&m.title, &needle));
            let s = Suggestion::new(
                CommandBoxAction::OpenResource(m.resource_id.clone()),
                format!("{} — {}", m.abbreviation, m.title),
                format!("Open {}", type_name(m.resource_type)),
            );
            if exact {
                out.push((rank::RESOURCE_EXACT, s));
            } else if part {
                parts.push((rank::RESOURCE_PART, s));
            }
        }
        out.extend(parts);
    }

    fn command_suggestions(&self, text: &str, out: &mut Vec<(u8, Suggestion)>) {
        let needle = text.to_lowercase();
        if needle.chars().count() < 3 {
            return;
        }
        // The Launchers too (#141), so one hidden from the toolbar is still a few keys away.
        let ids = crate::workspace_commands::MENUS
            .iter()
            .flat_map(|m| m.items)
            .filter_map(|i| match i {
                crate::MenuItem::Command(id) => Some(*id),
                _ => None,
            })
            .chain(crate::Launcher::ALL.iter().map(|l| l.command()));
        for id in ids {
            let Some(spec) = self.commands().get(id) else {
                continue;
            };
            if spec.params.iter().any(|p| p.required) || !self.is_enabled(id) {
                continue;
            }
            let label = spec.label.trim_end_matches('…');
            let r = if label.to_lowercase() == needle {
                rank::COMMAND_EXACT
            } else if word_prefix(label, &needle) {
                rank::COMMAND_PART
            } else {
                continue;
            };
            out.push((
                r,
                Suggestion::new(CommandBoxAction::Command(spec.id), spec.label, "Command"),
            ));
        }
    }

    /// Word Study suggestions for a Strong's number, a Lemma or a Lemma's transliteration.
    fn word_suggestions(&self, text: &str, strongs: u8, lemma: u8) -> Vec<(u8, Suggestion)> {
        if let Ok(n) = text.parse::<StrongsNumber>() {
            return vec![(
                strongs,
                Suggestion::new(CommandBoxAction::WordStudy(n), n.to_string(), "Word Study"),
            )];
        }
        lemma_numbers(self, text)
            .into_iter()
            .map(|(word, n)| {
                (
                    lemma,
                    Suggestion::new(
                        CommandBoxAction::WordStudy(n),
                        format!("{word} · {n}"),
                        "Word Study",
                    ),
                )
            })
            .collect()
    }

    fn go_to_suggestion(&self, p: Passage) -> Suggestion {
        let detail = match self.go_to_bible() {
            Some((id, _)) => format!(
                "Go to · {}",
                self.library.abbreviation_of(&id).unwrap_or(id.as_str())
            ),
            None => "Go to".to_string(),
        };
        Suggestion::new(CommandBoxAction::GoTo(p), written(p), detail)
    }

    /// The Bible a reference goes to: the focused Tab's (if it is a Bible), else the most
    /// recent Bible Tab's, else the first Bible in Preferred order; and whether a Tab of it
    /// is open.
    fn go_to_bible(&self) -> Option<(String, bool)> {
        let is_bible = |id: &str| {
            self.library
                .get(id)
                .is_some_and(|e| e.manifest.resource_type == ResourceType::Bible)
        };
        let open = self
            .workspace
            .focused()
            .into_iter()
            .chain(
                self.workspace
                    .most_recent(|t| matches!(&t.kind, TabKind::Resource(id) if is_bible(id))),
            )
            .find_map(|t| match &self.workspace.tab(t)?.kind {
                TabKind::Resource(id) if is_bible(id) => Some(id.clone()),
                _ => None,
            });
        match open {
            Some(id) => Some((id, true)),
            None => self
                .library
                .preferred(ResourceType::Bible)
                .map(|e| (e.manifest.resource_id.clone(), false)),
        }
    }

    /// The recent entries, newest first.
    pub fn command_box_recent(&mut self) -> Vec<String> {
        if let Some(r) = &self.command_box.recent {
            return r.clone();
        }
        let recent: Vec<String> = self
            .user
            .as_ref()
            .and_then(|u| u.preference(RECENT_PREF).ok().flatten())
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default();
        self.command_box.recent = Some(recent.clone());
        recent
    }

    fn remember_command_box(&mut self, text: &str) {
        let mut recent = self.command_box_recent();
        recent.retain(|t| t != text);
        recent.insert(0, text.to_string());
        recent.truncate(RECENT_LIMIT);
        if let Some(user) = &self.user
            && let Ok(v) = serde_json::to_string(&recent)
            && let Err(e) = user.set_preference(RECENT_PREF, &v)
        {
            self.workspace.push_effect(Effect::Message(format!(
                "Could not save recent Command box entries: {e}"
            )));
        }
        self.command_box.recent = Some(recent);
    }

    /// Run what a suggestion does. `new_tab` (Alt+Enter) opens a new Tab instead of moving
    /// an open one.
    pub fn run_command_box_action(
        &mut self,
        action: &CommandBoxAction,
        new_tab: bool,
    ) -> Result<Value, CommandError> {
        let at = |p: Passage| json!({ "passage": written(p), "new_tab": new_tab });
        match action {
            CommandBoxAction::GoTo(p) => go_to(self, *p, new_tab),
            CommandBoxAction::PassageGuide(p) => self.run("guide.passage_guide", &at(*p)),
            CommandBoxAction::TextComparison(p) => self.run("tool.text_comparison", &at(*p)),
            CommandBoxAction::CrossReferences(p) => self.run("tool.cross_references", &at(*p)),
            CommandBoxAction::WordStudy(n) => self.run(
                "word_study.open",
                &json!({ "strongs": n.to_string(), "new_tab": new_tab }),
            ),
            CommandBoxAction::WordStudyWords(w) => self.run(
                "word_study.choose",
                &json!({ "words": w, "new_tab": new_tab }),
            ),
            CommandBoxAction::OpenResource(id) => {
                let kind = TabKind::Resource(id.clone());
                let open = (!new_tab)
                    .then(|| self.workspace.most_recent(|t| t.kind == kind))
                    .flatten();
                match open {
                    Some(tab) => {
                        self.workspace.focus(tab);
                        Ok(json!({ "tab": tab.0 }))
                    }
                    None => self.run("resource.open", &json!({ "id": id })),
                }
            }
            CommandBoxAction::RestoreLayout(name) => {
                self.run("layout.restore", &json!({ "name": name }))
            }
            CommandBoxAction::OpenFavorite(id) => {
                self.run("favorite.open", &json!({ "id": id, "split": new_tab }))
            }
            CommandBoxAction::Command(id) => self.run(id, &json!({})),
            CommandBoxAction::Search(text) => {
                self.run("search.run", &json!({ "text": text, "split": new_tab }))
            }
            CommandBoxAction::Recent(text) => {
                let top = self
                    .command_box_suggestions(text)
                    .into_iter()
                    .next()
                    .ok_or_else(|| failed(RUN, "nothing to run"))?;
                self.run_command_box_action(&top.action, new_tab)
            }
        }
    }
}

/// Show `p` in a Bible: the focused or most recent Bible Tab moves there (its Link set
/// follows); with `new_tab`, or when no Bible is open, a new Tab of that Bible (else of the
/// first Bible in Preferred order) opens there.
fn go_to(s: &mut Session, p: Passage, new_tab: bool) -> Result<Value, CommandError> {
    let Some((bible, open)) = s.go_to_bible() else {
        return Err(failed(
            RUN,
            "Add a Bible to your library to go to a Passage",
        ));
    };
    if open && !new_tab {
        let (_, tab) = s
            .show_passage(p, Some(&bible), false)
            .map_err(|e| failed(RUN, e))?;
        return Ok(json!({ "tab": tab.0, "verse": p.start().osis() }));
    }
    let out = s.run("resource.open", &json!({ "id": bible }))?;
    let tab = out["tab"]
        .as_u64()
        .map(TabId)
        .ok_or_else(|| failed(RUN, "no Tab opened"))?;
    // The new Tab's Verse map is known once the UI has loaded it.
    s.workspace.push_effect(Effect::NavigateStandard {
        tab,
        verse: p.start(),
    });
    Ok(json!({ "tab": tab.0, "verse": p.start().osis() }))
}

/// One to three words of letters: worth offering the English-word chooser for.
fn is_english_words(text: &str) -> bool {
    let words: Vec<&str> = text.split_whitespace().collect();
    (1..=3).contains(&words.len())
        && words
            .iter()
            .all(|w| w.chars().all(|c| c.is_ascii_alphabetic() || c == '\''))
}

/// The Lexicon entries (Preferred order) whose Lemma is `text`, or whose Lemma transliterates
/// to it: each Lemma with its entry's Strong's number, one per number.
#[cfg(not(target_arch = "wasm32"))]
fn lemma_numbers(s: &Session, text: &str) -> Vec<(String, StrongsNumber)> {
    use obs_model::Key;
    use obs_store::ResourceStore;
    let original = crate::word_info::has_original_script(text);
    let transliteration = !original
        && text.chars().count() >= 3
        && text.chars().all(|c| c.is_alphabetic() || c == ' ');
    if !original && !transliteration {
        return Vec::new();
    }
    let mut out: Vec<(String, StrongsNumber)> = Vec::new();
    for e in s.library.of_type(ResourceType::Lexicon) {
        let Ok(reader) = obs_store::ObrReader::open(&e.path) else {
            continue;
        };
        let lemmas = if original {
            vec![text.to_string()]
        } else {
            reader.lemmas_for_transliteration(text).unwrap_or_default()
        };
        for lemma in lemmas {
            let entries = reader
                .lookup_key(&Key::Lemma(lemma.clone()))
                .unwrap_or_default();
            for entry in entries {
                let mut keys = reader.keys_of_unit(&entry.unit).unwrap_or_default();
                // The entry's own number before a base number it is also listed under.
                keys.sort_by_key(|k| k.alias);
                let number = keys.iter().find_map(|k| match k.key {
                    Key::StrongsNumber(n) => Some(n),
                    _ => None,
                });
                let word = match &entry.key {
                    Key::Lemma(l) => l.clone(),
                    _ => lemma.clone(),
                };
                if let Some(n) = number
                    && !out.iter().any(|(_, m)| m.base() == n.base())
                {
                    out.push((word, n));
                }
            }
        }
        if out.len() >= SUGGESTION_LIMIT {
            break;
        }
    }
    out
}

/// Without files (wasm), no Lexicon can be looked into.
#[cfg(target_arch = "wasm32")]
fn lemma_numbers(_: &Session, _: &str) -> Vec<(String, StrongsNumber)> {
    Vec::new()
}

const RUN: &str = "command_box.run";

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

const TEXT: ParamSpec = ParamSpec {
    name: "text",
    kind: ParamKind::String,
    required: true,
    help: "What was typed: a Bible reference, Strong's number, Lemma, Resource, Layout, `pg <ref>`, `compare <ref>`, `ws <word>`, or words to search",
};

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "command_box.focus",
        label: "Command box",
        shortcut: Some("Ctrl+L"),
        params: &[],
        enabled: always,
        run: |s, _| {
            s.workspace.push_effect(Effect::FocusCommandBox);
            Ok(Value::Null)
        },
    },
    CommandSpec {
        id: "command_box.suggest",
        label: "Command box suggestions",
        shortcut: None,
        params: &[ParamSpec {
            required: false,
            help: "What was typed (empty: the recent entries)",
            ..TEXT
        }],
        enabled: always,
        run: |s, p| {
            let text = p.str("text").unwrap_or_default();
            let list: Vec<Value> = s
                .command_box_suggestions(text)
                .iter()
                .map(Suggestion::to_json)
                .collect();
            Ok(Value::Array(list))
        },
    },
    CommandSpec {
        id: RUN,
        label: "Run from the Command box",
        shortcut: None,
        params: &[
            TEXT,
            ParamSpec {
                name: "choice",
                kind: ParamKind::Integer,
                required: false,
                help: "Which suggestion to run, from 0 (default: the best)",
            },
            ParamSpec {
                name: "new_tab",
                kind: ParamKind::Boolean,
                required: false,
                help: "Open a new Tab instead of moving an open one (Alt+Enter)",
            },
        ],
        enabled: always,
        run: run_from_box,
    },
];

fn run_from_box(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let text = p.str("text").unwrap_or_default().trim().to_string();
    let choice = p
        .i64("choice")
        .and_then(|c| usize::try_from(c).ok())
        .unwrap_or(0);
    let new_tab = p.bool("new_tab").unwrap_or(false);
    let list = s.command_box_suggestions(&text);
    let chosen = list
        .get(choice)
        .ok_or_else(|| failed(RUN, format!("no suggestion {choice} for “{text}”")))?;
    let kept = match &chosen.action {
        CommandBoxAction::Recent(t) => t.clone(),
        _ => text,
    };
    let action = chosen.action.clone();
    let out = s.run_command_box_action(&action, new_tab)?;
    s.remember_command_box(&kept);
    Ok(json!({ "kind": action.kind(), "out": out }))
}
