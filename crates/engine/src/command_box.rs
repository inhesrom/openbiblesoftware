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
//! | `pg`/`guide`/`passage guide` + reference; `compare`/`tc` + reference; `xref` + reference; `ws`/`study`/`word study` + word; `tg`/`topic`/`topic guide` + topic; `open` + Resource; `search` + text | that Guide, Tool, Resource or Search |
//! | a Resource's abbreviation or title, exactly | open it |
//! | a Bible reference (`jn 3:16`, also `jn 3 16`) | go there in the focused Bible (else the most recent Bible Tab, else the first Bible in Preferred order) |
//! | a Strong's number (`G26`) | Word Study |
//! | a Lemma (`ἀγάπη`) or its transliteration (`agape`) in a Lexicon | Word Study at the entry's Strong's number |
//! | a command's name (`notes`, `close tab`) | run it (on the focused Tab if it needs one) |
//! | part of a Resource's abbreviation or title, a Layout's name, a Saved search, a Favorite | open or run it |
//! | anything | Search (today's toolbar behaviour); when nothing else matched, also Word Study for English words |
//!
//! An empty box lists the recent entries: texts run from the box, newest first, kept in the
//! User database. Running a recent entry runs that text's best suggestion again. `>` lists
//! the commands only (#226): every one the box can run, each with its Keyboard shortcut.

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

/// How many suggestions the box shows at most (except for [`COMMANDS_PREFIX`]).
pub const SUGGESTION_LIMIT: usize = 8;

/// A text starting with this lists every command whose label holds the rest (#226): Help →
/// All commands (Ctrl+Shift+P) opens the box with it.
pub const COMMANDS_PREFIX: &str = ">";

/// Commands that answer scripts and `obs-cli` with data or text and change nothing on
/// screen; no menu or Command box lists them (#226).
pub const SCRIPTING_COMMANDS: &[&str] = &[
    "app.version",
    "reference.parse",
    "text.normalize",
    "morph.expand",
    "text.transliterate",
    "collection.list",
    "getbibles.list",
    "starter.list",
    "annotation.get",
    "annotation.list",
    "annotation.search_notes",
    "notebook.list",
    "highlight_style.list",
    "search.indexes",
    "search.list_saved",
    "command_box.suggest",
];

/// Commands that need no parameter but act on what only their own place gives them (the
/// reading-pane selection, a drop, a file picked in a window, an Annotation's Open in Notes
/// beside it); the Command box does not list them (#226).
pub const IN_PLACE_COMMANDS: &[&str] = &[
    "annotation.note",
    "workspace.drop",
    "notes.open",
    "highlight_palette.import",
    "theme.export",
    "theme.import",
    // The First-run tour's Coach mark buttons (#218); Help → Take the tour starts it.
    "tour.next",
    "tour.back",
    "tour.skip",
];

/// Whether the Command box lists `spec` (#226): it runs with no parameter but the focused
/// Tab, and it is neither a [`SCRIPTING_COMMANDS`] nor an [`IN_PLACE_COMMANDS`] one.
pub fn is_listed(spec: &CommandSpec) -> bool {
    crate::key_bindings::runs_from_a_key(spec)
        && !SCRIPTING_COMMANDS.contains(&spec.id)
        && !IN_PLACE_COMMANDS.contains(&spec.id)
}

/// What running a [`Suggestion`] does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandBoxAction {
    /// Show the Passage (Standard versification) in a Bible.
    GoTo(Passage),
    PassageGuide(Passage),
    ExegeticalGuide(Passage),
    TextComparison(Passage),
    CrossReferences(Passage),
    WordStudy(StrongsNumber),
    /// The Word Study's English-word chooser.
    WordStudyWords(String),
    /// The Topic Guide's chooser for a topic (#148).
    TopicGuide(String),
    /// Open (or focus) a Resource, by resource id.
    OpenResource(String),
    RestoreLayout(String),
    OpenFavorite(i64),
    /// Run a Saved search (#179), by name.
    OpenSavedSearch(String),
    /// Run a command with no parameters but the focused Tab.
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
            Self::ExegeticalGuide(_) => "exegetical_guide",
            Self::TextComparison(_) => "text_comparison",
            Self::CrossReferences(_) => "cross_references",
            Self::WordStudy(_) => "word_study",
            Self::WordStudyWords(_) => "word_study_words",
            Self::TopicGuide(_) => "topic_guide",
            Self::OpenResource(_) => "resource",
            Self::RestoreLayout(_) => "layout",
            Self::OpenFavorite(_) => "favorite",
            Self::OpenSavedSearch(_) => "saved_search",
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
    ("exegetical guide", Verb::ExegeticalGuide),
    ("cross-references", Verb::CrossReferences),
    ("cross references", Verb::CrossReferences),
    ("text comparison", Verb::Compare),
    ("passage guide", Verb::PassageGuide),
    ("topic guide", Verb::TopicGuide),
    ("word study", Verb::WordStudy),
    ("compare", Verb::Compare),
    ("search", Verb::Search),
    ("guide", Verb::PassageGuide),
    ("study", Verb::WordStudy),
    ("topic", Verb::TopicGuide),
    ("xrefs", Verb::CrossReferences),
    ("xref", Verb::CrossReferences),
    ("open", Verb::Open),
    ("pg", Verb::PassageGuide),
    ("eg", Verb::ExegeticalGuide),
    ("ws", Verb::WordStudy),
    ("tc", Verb::Compare),
    ("tg", Verb::TopicGuide),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verb {
    PassageGuide,
    ExegeticalGuide,
    Compare,
    CrossReferences,
    WordStudy,
    TopicGuide,
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
    pub const SAVED_SEARCH: u8 = 56;
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
    /// An empty text lists the recent entries; a text starting with [`COMMANDS_PREFIX`] lists
    /// the commands only, as many as match.
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
        if let Some(rest) = text.strip_prefix(COMMANDS_PREFIX) {
            return self.all_command_suggestions(rest);
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
            for n in self.saved_searches_matching(text) {
                ranked.push((
                    rank::SAVED_SEARCH,
                    Suggestion::new(
                        CommandBoxAction::OpenSavedSearch(n.name.clone()),
                        format!("{} — {}", n.name, n.search.summary()),
                        "Saved search",
                    ),
                ));
            }
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
            Verb::ExegeticalGuide => {
                out.extend(
                    at(CommandBoxAction::ExegeticalGuide, "Exegetical Guide")
                        .map(|s| (rank::VERB, s)),
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
            Verb::TopicGuide => out.push((
                rank::VERB,
                Suggestion::new(
                    CommandBoxAction::TopicGuide(rest.to_string()),
                    format!("“{rest}”"),
                    "Topic Guide",
                ),
            )),
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
        for spec in self.listed_commands() {
            if !self.is_enabled(spec.id) {
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
            out.push((r, self.command_suggestion(&spec)));
        }
    }

    fn command_suggestion(&self, spec: &CommandSpec) -> Suggestion {
        let detail = match self.shortcut(spec.id) {
            Some(key) => format!("Command · {key}"),
            None => "Command".to_string(),
        };
        // One that asks first (Clear History) says so, as its Settings button does.
        let label = match crate::settings::setting_asking_first(spec.id) {
            Some(_) => format!("{}…", spec.label),
            None => spec.label.to_string(),
        };
        Suggestion::new(CommandBoxAction::Command(spec.id), label, detail)
    }

    /// The commands the Command box lists (#226): every command that runs with no parameter
    /// but the focused Tab, except [`SCRIPTING_COMMANDS`] and [`IN_PLACE_COMMANDS`]; the menu
    /// bar's in menu order first, then the rest in registration order.
    pub fn listed_commands(&self) -> Vec<CommandSpec> {
        let menus = crate::workspace_commands::MENUS
            .iter()
            .flat_map(|m| m.commands())
            .filter_map(|id| self.commands().get(id).copied());
        let mut out: Vec<CommandSpec> = Vec::new();
        for spec in menus.chain(self.commands().iter().copied()) {
            if is_listed(&spec) && out.iter().all(|o| o.id != spec.id) {
                out.push(spec);
            }
        }
        out
    }

    /// `>` in the Command box (#226): every listed command whose label holds `rest`'s words
    /// (all of them for an empty `rest`), label matches first, disabled ones left out.
    fn all_command_suggestions(&self, rest: &str) -> Vec<Suggestion> {
        let needle = rest.trim().to_lowercase();
        let mut ranked: Vec<(u8, Suggestion)> = Vec::new();
        for spec in self.listed_commands() {
            if !self.is_enabled(spec.id) {
                continue;
            }
            let label = spec.label.trim_end_matches('…').to_lowercase();
            let r = if needle.is_empty() || label == needle {
                3
            } else if word_prefix(&label, &needle) {
                2
            } else if needle.split_whitespace().all(|w| label.contains(w)) {
                1
            } else {
                continue;
            };
            ranked.push((r, self.command_suggestion(&spec)));
        }
        ranked.sort_by_key(|r| std::cmp::Reverse(r.0));
        ranked.into_iter().map(|(_, s)| s).collect()
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

    /// Suggestion `choice` of `text` as `command_box.run` runs it: what it does (for a
    /// recent entry, its text's best suggestion) and the text kept among the recent entries.
    pub fn command_box_choice(
        &mut self,
        text: &str,
        choice: usize,
    ) -> Option<(String, CommandBoxAction)> {
        let chosen = self
            .command_box_suggestions(text)
            .into_iter()
            .nth(choice)?
            .action;
        self.resolve_recent(text, chosen)
    }

    /// What `chosen`, a suggestion for `text`, does and the text kept: a recent entry runs
    /// its text's best suggestion.
    fn resolve_recent(
        &mut self,
        text: &str,
        chosen: CommandBoxAction,
    ) -> Option<(String, CommandBoxAction)> {
        match chosen {
            CommandBoxAction::Recent(t) => {
                let top = self.command_box_suggestions(&t).into_iter().next()?.action;
                Some((t, top))
            }
            action => Some((text.trim().to_string(), action)),
        }
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
            CommandBoxAction::ExegeticalGuide(p) => self.run("guide.exegetical_guide", &at(*p)),
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
            CommandBoxAction::TopicGuide(text) => self.run(
                "guide.topic_guide",
                &json!({ "text": text, "new_tab": new_tab }),
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
            CommandBoxAction::OpenSavedSearch(name) => self.run(
                "search.open_saved",
                &json!({ "name": name, "split": new_tab }),
            ),
            CommandBoxAction::Command(id) => {
                // It cannot be undone: Settings asks Clear or Cancel, as its button does.
                if let Some(setting) = crate::settings::setting_asking_first(id) {
                    let out = self.run("app.settings", &json!({}))?;
                    let tab = (out["tab"].as_u64())
                        .map(TabId)
                        .ok_or_else(|| failed(RUN, "no Settings Tab opened"))?;
                    self.workspace.push_effect(Effect::ConfirmSetting {
                        tab,
                        setting: setting.id,
                    });
                    return Ok(json!({ "tab": tab.0, "confirm": setting.id }));
                }
                let needs_tab = (self.commands().get(id))
                    .is_some_and(|c| c.params.iter().any(|p| p.name == "tab" && p.required));
                if !needs_tab {
                    return self.run(id, &json!({}));
                }
                let tab = self
                    .workspace
                    .focused()
                    .ok_or_else(|| failed(RUN, "Open a Tab first"))?;
                self.run(id, &json!({ "tab": tab.0 }))
            }
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
            ParamSpec {
                name: "command",
                kind: ParamKind::String,
                required: false,
                help: "The listed command suggestion `choice` showed, run as it instead of looking again: the suggestions leave out disabled commands, so one made possible since they were shown (an Undo, say by the Notes saved before a restore's undo, #226) shifts them; `text` is the entry kept",
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
    let (kind, kept, action) = match p.str("command") {
        Some(id) => {
            let spec = (s.commands().get(id).copied())
                .filter(is_listed)
                .ok_or_else(|| failed(RUN, format!("the Command box does not run “{id}”")))?;
            let action = CommandBoxAction::Command(spec.id);
            (action.kind(), text, action)
        }
        None => {
            let chosen = (s.command_box_suggestions(&text).into_iter().nth(choice))
                .ok_or_else(|| failed(RUN, format!("no suggestion {choice} for “{text}”")))?
                .action;
            let kind = chosen.kind();
            let (kept, action) =
                (s.resolve_recent(&text, chosen)).ok_or_else(|| failed(RUN, "nothing to run"))?;
            (kind, kept, action)
        }
    };
    let out = s.run_command_box_action(&action, new_tab)?;
    // The command list is a way in, not an entry worth keeping.
    if !kept.starts_with(COMMANDS_PREFIX) {
        s.remember_command_box(&kept);
    }
    Ok(json!({ "kind": kind, "out": out }))
}
