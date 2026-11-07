//! Copy verses (#154): Tools → Copy verses… copies a Passage list from one or more Bibles as
//! plain text, rich text (HTML) or Markdown, laid out by [`CopyOptions`].
//!
//! - **Passages** are typed as a Bible reference in the Standard versification (it is not
//!   typed into one Bible's Tab) and found in each Bible through its Verse map, as Text
//!   Comparison does ([`crate::compare::cell_for`]). Each Bible's text keeps its own verse
//!   numbers, and each block's reference is in that Bible's own numbering, like a plain Copy
//!   (#34).
//! - **Blocks** come Passage by Passage, and within each Passage one block per Bible in the
//!   order chosen, so translations of one Passage sit together.
//! - **Attribution**: after the blocks, the Attribution of every copied Bible whose licence
//!   asks for credit ([`obs_model::Manifest::requires_attribution`]).
//! - **Copy presets** are named [`CopyOptions`], kept in the User database with the options
//!   last used (`copy.presets`, `copy.options` preferences).

use obs_model::Manifest;
use obs_scripture::{Passage, PassageList, RefStyle, Verse, Versification, format_passage};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::compare::{Cell, ComparedBible, cell_for};
use crate::session::Session;
use crate::workspace::Effect;

/// Where each block's reference goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferencePlacement {
    Before,
    #[default]
    After,
}

/// How a block's verses are laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerseLayout {
    /// Verses run together, a new paragraph where the Bible starts one.
    #[default]
    Paragraphs,
    /// Each verse on a line of its own.
    VersePerLine,
}

/// How Bible book names are written in references.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BookNames {
    /// `John 3:16`.
    #[default]
    Full,
    /// `Rom 8:28` (SBL abbreviations; short names such as John stay whole).
    Abbreviated,
}

impl BookNames {
    pub fn style(self) -> RefStyle {
        match self {
            BookNames::Full => RefStyle::Long,
            BookNames::Abbreviated => RefStyle::Short,
        }
    }
}

/// The choices of Copy verses. Missing fields read as their defaults, so options saved by
/// an older build still load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CopyOptions {
    pub reference: ReferencePlacement,
    pub verse_numbers: bool,
    pub layout: VerseLayout,
    pub quotation_marks: bool,
    pub book_names: BookNames,
}

impl Default for CopyOptions {
    fn default() -> Self {
        Self {
            reference: ReferencePlacement::After,
            verse_numbers: true,
            layout: VerseLayout::Paragraphs,
            quotation_marks: false,
            book_names: BookNames::Full,
        }
    }
}

/// One Copy verses option: its name, label, and its values with their labels.
pub type OptionChoice = (
    &'static str,
    &'static str,
    &'static [(&'static str, &'static str)],
);

/// The option names and values `copy.option` takes, in the order the window shows them.
pub const OPTION_CHOICES: &[OptionChoice] = &[
    (
        "reference",
        "Reference",
        &[("before", "Before the text"), ("after", "After the text")],
    ),
    (
        "verse_numbers",
        "Verse numbers",
        &[("on", "On"), ("off", "Off")],
    ),
    (
        "layout",
        "Layout",
        &[
            ("paragraphs", "Paragraphs"),
            ("verse_per_line", "One verse per line"),
        ],
    ),
    (
        "quotation_marks",
        "Quotation marks",
        &[("on", "On"), ("off", "Off")],
    ),
    (
        "book_names",
        "Book names",
        &[
            ("full", "Full (John 3:16)"),
            ("abbreviated", "Abbreviated (Rom 8:28)"),
        ],
    ),
];

impl CopyOptions {
    /// The current value of option `name` (see [`OPTION_CHOICES`]).
    pub fn value(&self, name: &str) -> Option<&'static str> {
        let on = |b: bool| if b { "on" } else { "off" };
        Some(match name {
            "reference" => match self.reference {
                ReferencePlacement::Before => "before",
                ReferencePlacement::After => "after",
            },
            "verse_numbers" => on(self.verse_numbers),
            "layout" => match self.layout {
                VerseLayout::Paragraphs => "paragraphs",
                VerseLayout::VersePerLine => "verse_per_line",
            },
            "quotation_marks" => on(self.quotation_marks),
            "book_names" => match self.book_names {
                BookNames::Full => "full",
                BookNames::Abbreviated => "abbreviated",
            },
            _ => return None,
        })
    }

    /// Set option `name` to `value`; `false` when either is unknown.
    pub fn set(&mut self, name: &str, value: &str) -> bool {
        match (name, value) {
            ("reference", "before") => self.reference = ReferencePlacement::Before,
            ("reference", "after") => self.reference = ReferencePlacement::After,
            ("verse_numbers", "on") => self.verse_numbers = true,
            ("verse_numbers", "off") => self.verse_numbers = false,
            ("layout", "paragraphs") => self.layout = VerseLayout::Paragraphs,
            ("layout", "verse_per_line") => self.layout = VerseLayout::VersePerLine,
            ("quotation_marks", "on") => self.quotation_marks = true,
            ("quotation_marks", "off") => self.quotation_marks = false,
            ("book_names", "full") => self.book_names = BookNames::Full,
            ("book_names", "abbreviated") => self.book_names = BookNames::Abbreviated,
            _ => return false,
        }
        true
    }
}

/// A **Copy preset**: named Copy verses options the user saved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopyPreset {
    pub name: String,
    pub options: CopyOptions,
}

/// Copy verses' state: the options in use and the saved Copy presets.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CopyVerses {
    pub options: CopyOptions,
    pub presets: Vec<CopyPreset>,
}

impl CopyVerses {
    /// The preset whose options are the ones in use, if any.
    pub fn current_preset(&self) -> Option<&CopyPreset> {
        self.presets.iter().find(|p| p.options == self.options)
    }
}

/// The forms Copy verses can copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CopyFormat {
    #[default]
    PlainText,
    /// HTML, put on the clipboard as rich text.
    RichText,
    Markdown,
}

impl CopyFormat {
    pub const ALL: [CopyFormat; 3] = [
        CopyFormat::PlainText,
        CopyFormat::RichText,
        CopyFormat::Markdown,
    ];

    pub fn label(self) -> &'static str {
        match self {
            CopyFormat::PlainText => "Plain text",
            CopyFormat::RichText => "Rich text",
            CopyFormat::Markdown => "Markdown",
        }
    }
}

/// A Bible as Copy verses reads it: Text Comparison's view of it, plus what a copy needs.
pub trait CopiedBible: ComparedBible {
    /// Its abbreviation, licence and Attribution.
    fn manifest(&self) -> &Manifest;
    /// Its own numbering, for the references of its blocks.
    fn versification(&self) -> &dyn Versification;
    /// Whether the Bible Verse starting at `native` (own numbering) begins a paragraph.
    fn starts_paragraph(&self, native: Verse) -> bool;
}

/// One verse as copied: its number as shown (`16`, `1-2`, `4:1` where a chapter starts
/// inside the block, empty for a Psalm title) and its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopiedVerse {
    pub number: String,
    pub text: String,
}

/// One Passage in one Bible: its reference (`John 3:16-18 (KJV)`) and its verses in
/// paragraphs (with [`VerseLayout::VersePerLine`], one verse each).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopiedBlock {
    pub reference: String,
    pub paragraphs: Vec<Vec<CopiedVerse>>,
}

/// What Copy verses copies, ready to render in any [`CopyFormat`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopiedText {
    pub options: CopyOptions,
    pub blocks: Vec<CopiedBlock>,
    /// The Attributions to append, one per Bible whose licence requires it.
    pub attributions: Vec<String>,
}

/// The verses of `passage` (Standard) in `bible`, in its own numbering and in its own
/// order, with their text. A Verse map may reorder verses (Standard Phil 1:16-17 is Phil
/// 1:17, 1:16 in some Bibles), so they are collected in Standard order and then sorted.
fn native_verses(bible: &dyn CopiedBible, passage: Passage) -> Vec<(Verse, u16, String)> {
    let first = passage.start();
    let mut out: Vec<(Verse, u16, String)> = Vec::new();
    for row in passage.verses(&obs_scripture::Standard) {
        if let Cell::Text { natives, .. } = cell_for(bible, row, first) {
            for (native, last) in natives {
                if out.iter().any(|(v, _, _)| *v == native) {
                    continue;
                }
                let text = bible.verse_text(native).unwrap_or_default();
                out.push((native, last, text));
            }
        }
    }
    out.sort_by_key(|(native, _, _)| *native);
    out
}

/// Copy `passages` (Standard versification) from `bibles` with `options`.
pub fn copy_verses(
    passages: &PassageList,
    bibles: &[&dyn CopiedBible],
    options: CopyOptions,
) -> CopiedText {
    let mut blocks = Vec::new();
    let mut used = vec![false; bibles.len()];
    for passage in passages.iter() {
        for (i, bible) in bibles.iter().enumerate() {
            let verses = native_verses(*bible, *passage);
            let Some(first) = verses.first() else {
                continue;
            };
            used[i] = true;
            let end = verses
                .iter()
                .map(|(native, last, _)| {
                    Verse::new(native.book, native.chapter, (*last).max(native.verse))
                })
                .max()
                .unwrap_or(first.0);
            let range = Passage::new(first.0, end).unwrap_or(Passage::verse(first.0));
            let reference = format!(
                "{} ({})",
                format_passage(range, bible.versification(), options.book_names.style()),
                bible.manifest().abbreviation
            );
            let mut paragraphs: Vec<Vec<CopiedVerse>> = Vec::new();
            let mut chapter = first.0.chapter;
            for (n, (native, last, text)) in verses.iter().enumerate() {
                let mut number = if native.is_title() {
                    String::new()
                } else if *last > native.verse {
                    format!("{}-{last}", native.verse)
                } else {
                    native.verse.to_string()
                };
                if native.chapter != chapter && !number.is_empty() {
                    number = format!("{}:{number}", native.chapter);
                }
                chapter = native.chapter;
                let verse = CopiedVerse {
                    number,
                    text: text.clone(),
                };
                let new_paragraph = match options.layout {
                    VerseLayout::VersePerLine => true,
                    VerseLayout::Paragraphs => n > 0 && bible.starts_paragraph(*native),
                };
                match paragraphs.last_mut() {
                    Some(p) if !new_paragraph => p.push(verse),
                    _ => paragraphs.push(vec![verse]),
                }
            }
            blocks.push(CopiedBlock {
                reference,
                paragraphs,
            });
        }
    }
    let mut attributions: Vec<String> = Vec::new();
    for (bible, _) in bibles.iter().zip(&used).filter(|(_, used)| **used) {
        let manifest = bible.manifest();
        let text = manifest.attribution.trim().to_string();
        if manifest.requires_attribution() && !attributions.contains(&text) {
            attributions.push(text);
        }
    }
    CopiedText {
        options,
        blocks,
        attributions,
    }
}

/// `text` with Markdown's special characters escaped, so it reads back as written: the
/// inline markers, and `&` where it could start a character reference (`&amp;`, `&#38;`).
/// What starts a block at the beginning of a line is [`markdown_escape_line_start`]'s job.
pub fn markdown_escape(text: &str) -> String {
    markdown_escape_part(text, 0..text.len())
}

/// [`markdown_escape`] of `text[range]`, an `&` at its end read with the text after it.
pub fn markdown_escape_part(text: &str, range: std::ops::Range<usize>) -> String {
    let part = &text[range.clone()];
    let mut out = String::with_capacity(part.len());
    let mut chars = part
        .chars()
        .chain(text[range.end..].chars().take(1))
        .peekable();
    let mut left = part.chars().count();
    while left > 0
        && let Some(c) = chars.next()
    {
        left -= 1;
        let entity = c == '&'
            && chars
                .peek()
                .is_some_and(|n| n.is_alphanumeric() || *n == '#');
        if entity
            || matches!(
                c,
                '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '#' | '|' | '~'
            )
        {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// `line` (already [`markdown_escape`]d) with what would start a block at the beginning of a
/// Markdown line escaped: list and setext-heading markers (`-`, `+`, `=`), and the `.` or `)`
/// of an ordered-list number (`1.`). Headings, quotes and `*`/`_` breaks are inline-escaped.
pub fn markdown_escape_line_start(line: &str) -> String {
    let body = line.trim_start_matches(' ');
    let mut out = String::with_capacity(line.len() + 1);
    out.push_str(&line[..line.len() - body.len()]);
    if body.starts_with(['-', '+', '=']) {
        out.push('\\');
    }
    let digits = body.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 && body[digits..].starts_with(['.', ')']) {
        out.push_str(&body[..digits]);
        out.push('\\');
        out.push_str(&body[digits..]);
    } else {
        out.push_str(body);
    }
    out
}

/// `text` with HTML's special characters escaped.
pub fn html_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// How one format writes the parts of a copy.
struct Writer {
    escape: fn(&str) -> String,
    number: fn(&str) -> String,
    /// Between the lines of one-verse-per-line text.
    line_break: &'static str,
    /// A paragraph of text (already escaped).
    paragraph: fn(String) -> String,
    /// Between paragraphs, and between blocks.
    paragraph_gap: &'static str,
    block_gap: &'static str,
    attribution: fn(String) -> String,
}

const PLAIN: Writer = Writer {
    escape: str::to_string,
    number: |n| format!("{n} "),
    line_break: "\n",
    paragraph: |p| p,
    paragraph_gap: "\n",
    block_gap: "\n\n",
    attribution: |a| a,
};

const MARKDOWN: Writer = Writer {
    escape: markdown_escape,
    number: |n| format!("**{n}** "),
    // A CommonMark hard line break.
    line_break: "\\\n",
    paragraph: |p| p,
    paragraph_gap: "\n\n",
    block_gap: "\n\n",
    attribution: |a| a,
};

const HTML: Writer = Writer {
    escape: html_escape,
    number: |n| format!("<sup>{n}</sup>\u{a0}"),
    line_break: "<br>\n",
    // Each paragraph takes its direction from its first letter, so a Hebrew one reads
    // right to left (verse number on the right) in a left-to-right page, like the preview.
    paragraph: |p| format!("<p dir=\"auto\">{p}</p>"),
    paragraph_gap: "\n",
    block_gap: "\n",
    attribution: |a| format!("<p><small>{a}</small></p>"),
};

impl CopiedText {
    /// The copy as `format`: plain text, an HTML fragment, or Markdown.
    pub fn render(&self, format: CopyFormat) -> String {
        match format {
            CopyFormat::PlainText => self.write(&PLAIN),
            CopyFormat::RichText => self.write(&HTML),
            CopyFormat::Markdown => self
                .write(&MARKDOWN)
                .split('\n')
                .map(markdown_escape_line_start)
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    pub fn plain_text(&self) -> String {
        self.render(CopyFormat::PlainText)
    }

    pub fn html(&self) -> String {
        self.render(CopyFormat::RichText)
    }

    pub fn markdown(&self) -> String {
        self.render(CopyFormat::Markdown)
    }

    fn write(&self, w: &Writer) -> String {
        let o = &self.options;
        let verse = |v: &CopiedVerse| {
            let text = (w.escape)(&v.text);
            if o.verse_numbers && !v.number.is_empty() {
                format!("{}{text}", (w.number)(&v.number))
            } else {
                text
            }
        };
        let mut blocks = Vec::new();
        for block in &self.blocks {
            let mut lines: Vec<String> = block
                .paragraphs
                .iter()
                .map(|p| p.iter().map(verse).collect::<Vec<_>>().join(" "))
                .collect();
            if o.quotation_marks {
                if let Some(first) = lines.first_mut() {
                    first.insert(0, '“');
                }
                if let Some(last) = lines.last_mut() {
                    last.push('”');
                }
            }
            let text = match o.layout {
                VerseLayout::VersePerLine => (w.paragraph)(lines.join(w.line_break)),
                VerseLayout::Paragraphs => lines
                    .into_iter()
                    .map(w.paragraph)
                    .collect::<Vec<_>>()
                    .join(w.paragraph_gap),
            };
            let reference = (w.paragraph)((w.escape)(&block.reference));
            blocks.push(match o.reference {
                ReferencePlacement::Before => format!("{reference}{}{text}", w.paragraph_gap),
                ReferencePlacement::After => format!("{text}{}{reference}", w.paragraph_gap),
            });
        }
        let mut out = blocks.join(w.block_gap);
        for a in &self.attributions {
            out.push_str(w.block_gap);
            out.push_str(&(w.attribution)((w.escape)(a)));
        }
        out
    }
}

/// A standalone HTML page around `body` (an HTML fragment) for Print / Save as PDF: the
/// system browser opens it and, with `print`, shows its print dialog at once, where Save as
/// PDF is one of the printers.
pub fn html_document(title: &str, body: &str, print: bool) -> String {
    let script = if print {
        "<script>window.addEventListener(\"load\", () => window.print());</script>\n"
    } else {
        ""
    };
    format!(
        "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<title>{}</title>\n\
         <style>\nbody {{ font-family: Georgia, \"Times New Roman\", serif; font-size: 12pt; \
         line-height: 1.5; max-width: 40em; margin: 2em auto; color: #000; }}\n\
         h1 {{ font-size: 1.6em; }} h2 {{ font-size: 1.2em; margin-top: 1.6em; }}\n\
         blockquote {{ margin-left: 1.5em; font-style: italic; }}\n\
         sup {{ font-size: 0.7em; }} small {{ color: #444; }}\n\
         @media print {{ body {{ margin: 0; max-width: none; }} }}\n</style>\n{script}\
         </head>\n<body>\n{body}\n</body>\n</html>\n",
        html_escape(title)
    )
}

// ------------------------------------------------------------------------------- session

const OPTIONS_PREF: &str = "copy.options";
const PRESETS_PREF: &str = "copy.presets";

impl Session {
    /// Read the Copy verses options and Copy presets from the User database.
    pub(crate) fn load_copy_verses(&mut self) {
        let Some(user) = &self.user else {
            return;
        };
        if let Ok(Some(json)) = user.preference(OPTIONS_PREF)
            && let Ok(options) = serde_json::from_str(&json)
        {
            self.copy_verses.options = options;
        }
        if let Ok(Some(json)) = user.preference(PRESETS_PREF)
            && let Ok(presets) = serde_json::from_str(&json)
        {
            self.copy_verses.presets = presets;
        }
    }

    fn save_copy_verses(&mut self) {
        let Some(user) = &self.user else {
            return;
        };
        let options = serde_json::to_string(&self.copy_verses.options).unwrap_or_default();
        let presets = serde_json::to_string(&self.copy_verses.presets).unwrap_or_default();
        if let Err(e) = user
            .set_preference(OPTIONS_PREF, &options)
            .and_then(|()| user.set_preference(PRESETS_PREF, &presets))
        {
            self.workspace.push_effect(Effect::Message(format!(
                "Could not save the Copy verses options: {e}"
            )));
        }
    }
}

// ------------------------------------------------------------------------------ commands

const NAME: ParamSpec = ParamSpec {
    name: "name",
    kind: ParamKind::String,
    required: true,
    help: "The Copy preset's name",
};

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "tool.copy_verses",
        label: "Copy verses…",
        shortcut: None,
        params: &[
            ParamSpec {
                name: "passages",
                kind: ParamKind::String,
                required: false,
                help: "Passages to copy, e.g. `Jn 3:16-18; Rom 5:8` (default: where the most recent Bible Tab is)",
            },
            ParamSpec {
                name: "bibles",
                kind: ParamKind::String,
                required: false,
                help: "Resource ids of the Bibles, comma-separated, e.g. `kjv-strongs,bsb`",
            },
        ],
        enabled: always,
        run: |s, p| {
            let bibles = p
                .str("bibles")
                .unwrap_or_default()
                .split(',')
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_string)
                .collect();
            s.workspace.push_effect(Effect::ShowCopyVerses {
                passages: p.str("passages").map(str::to_string),
                bibles,
            });
            Ok(Value::Null)
        },
    },
    CommandSpec {
        id: "copy.option",
        label: "Set a Copy verses option",
        shortcut: None,
        params: &[
            ParamSpec {
                name: "name",
                kind: ParamKind::String,
                required: true,
                help: "reference, verse_numbers, layout, quotation_marks or book_names",
            },
            ParamSpec {
                name: "value",
                kind: ParamKind::String,
                required: true,
                help: "before/after, on/off, paragraphs/verse_per_line, full/abbreviated",
            },
        ],
        enabled: always,
        run: run_option,
    },
    CommandSpec {
        id: "copy.preset_save",
        label: "Save the Copy verses options as a preset",
        shortcut: None,
        params: &[NAME],
        enabled: always,
        run: run_preset_save,
    },
    CommandSpec {
        id: "copy.preset_apply",
        label: "Use a Copy preset",
        shortcut: None,
        params: &[NAME],
        enabled: always,
        run: run_preset_apply,
    },
    CommandSpec {
        id: "copy.preset_delete",
        label: "Delete a Copy preset",
        shortcut: None,
        params: &[NAME],
        enabled: always,
        run: run_preset_delete,
    },
];

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

fn options_json(s: &Session) -> Value {
    json!({
        "options": serde_json::to_value(s.copy_verses.options).unwrap_or_default(),
        "preset": s.copy_verses.current_preset().map(|p| p.name.clone()),
        "presets": s.copy_verses.presets.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),
    })
}

fn run_option(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "copy.option";
    let name = p.str("name").unwrap_or_default();
    let value = p.str("value").unwrap_or_default();
    if !s.copy_verses.options.set(name, value) {
        return Err(failed(CMD, format!("no option `{name}` = `{value}`")));
    }
    s.save_copy_verses();
    Ok(options_json(s))
}

fn preset_name(command: &'static str, p: Params<'_>) -> Result<String, CommandError> {
    let name = p.str("name").unwrap_or_default().trim().to_string();
    if name.is_empty() {
        return Err(failed(command, "a Copy preset needs a name"));
    }
    Ok(name)
}

fn run_preset_save(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let name = preset_name("copy.preset_save", p)?;
    let options = s.copy_verses.options;
    match s.copy_verses.presets.iter_mut().find(|p| p.name == name) {
        Some(preset) => preset.options = options,
        None => s.copy_verses.presets.push(CopyPreset { name, options }),
    }
    s.save_copy_verses();
    Ok(options_json(s))
}

fn run_preset_apply(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "copy.preset_apply";
    let name = preset_name(CMD, p)?;
    let preset = s
        .copy_verses
        .presets
        .iter()
        .find(|p| p.name == name)
        .ok_or_else(|| failed(CMD, format!("no Copy preset named “{name}”")))?;
    s.copy_verses.options = preset.options;
    s.save_copy_verses();
    Ok(options_json(s))
}

fn run_preset_delete(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "copy.preset_delete";
    let name = preset_name(CMD, p)?;
    let before = s.copy_verses.presets.len();
    s.copy_verses.presets.retain(|p| p.name != name);
    if s.copy_verses.presets.len() == before {
        return Err(failed(CMD, format!("no Copy preset named “{name}”")));
    }
    s.save_copy_verses();
    Ok(options_json(s))
}
