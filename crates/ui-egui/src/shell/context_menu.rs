//! The right-click menu (#142): two columns, like the familiar study-app menu. The left column
//! lists what the click can act on — the selected text, the Verse, and for a Tagged word its
//! Lemma, Strong's number and Morphology; the right column the actions for the one chosen
//! (hovering a target chooses it).
//!
//! | Target | Actions |
//! |---|---|
//! | Selection | Highlight styles, Note…, Copy, Search this, Look up in Dictionary, Word Study… (the English-word chooser) |
//! | Verse | Passage Guide, Cross-references, Text Comparison, Copy with reference, Add Note on verse |
//! | Lemma, Strong's number | Word Study, Open in preferred Lexicon, Pin in Word Info, Search Bible for it |
//! | Morphology | Copy, Pin in Word Info |
//!
//! It opens in reading panes (Bibles, Commentaries, Lexicons, Dictionaries: a Strong's link
//! is a Tagged word, a Scripture reference a Verse), on Text Comparison Verse blocks and on
//! Search result rows; Shift+F10 or the Menu key opens it at the word under the pointer or
//! the selection. Actions run engine commands (`word_study.open`, `search.run`, …), in the
//! target's Link set; with Ctrl/Cmd held they open a new Tab.

use std::sync::Arc;

use egui::{Key, Modifiers, Rect, RichText, Ui, pos2, vec2};
use obs_engine::link::{ToStandard, to_standard};
use obs_engine::{HighlightStyle, TabId};
use obs_model::ResourceType;
use obs_scripture::{
    Passage, RefStyle, Standard, StrongsNumber, Verse, Versification, format_passage,
};
use serde_json::json;

use super::State;
use super::annotate::{Annotate, menu_label, style_shortcut};
use super::references::TargetKey;
use crate::compare_view::ComparisonContext;
use crate::doc::ReadingDoc;
use crate::palette::Palette;
use crate::pane::{PaneOutput, WordHit, has_text, select_for_menu};
use crate::selection::{DocPos, Granularity, Selection, copy_text, selected_text};
use crate::theme;

/// One thing the menu can act on: a row of its left column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Target {
    Selection,
    Verse,
    Lemma,
    Strongs,
    Morphology,
}

impl Target {
    /// What the left column calls it, above its value.
    fn kind(self) -> &'static str {
        match self {
            Target::Selection => "Selection",
            Target::Verse => "Verse",
            Target::Lemma => "Lemma",
            Target::Strongs => "Strong's number",
            Target::Morphology => "Morphology",
        }
    }
}

/// The Verse the menu was opened on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct VerseTarget {
    /// "Jn 3:16", in the numbering of where it was clicked.
    label: String,
    /// In the Standard versification; `None` for an Unmapped verse.
    standard: Option<Verse>,
    /// In a Bible's reading pane: the Bible's own Verse (Copy with reference and Add Note on
    /// verse select it).
    native: Option<Verse>,
    /// Elsewhere: a Bible's own Verses, for Copy with reference.
    copy: Option<VerseCopy>,
    /// A Scripture reference whose targets are still being translated: the Verse is filled in
    /// when they arrive (until then `label` is the reference as written, `standard` `None`).
    resolving: Option<TargetKey>,
}

impl VerseTarget {
    /// A Verse in the Standard versification (a Commentary section, a Scripture reference).
    fn standard(verse: Verse) -> Self {
        Self {
            label: format_passage(Passage::verse(verse), &Standard, RefStyle::Short),
            standard: Some(verse),
            native: None,
            copy: None,
            resolving: None,
        }
    }
}

/// A Bible's own Verses that Copy with reference copies (a Text Comparison row, a Search hit):
/// the reference is in the Bible's numbering, which can differ from the Standard Verse shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct VerseCopy {
    /// The Bible's resource id (its versification writes the reference) and abbreviation.
    bible: String,
    abbreviation: String,
    /// Its own Verses, with the last verse number each covers (`\v 1-2` → 2).
    natives: Vec<(Verse, u16)>,
    /// The text as shown; `None`: read from the Bible.
    text: Option<String>,
}

/// The reference of a Bible's own Verses in its numbering: "Genesis 32:1", a bridge
/// "Genesis 1:1-2", several Verses "Genesis 31:55-32:1".
pub(super) fn native_reference<V: Versification + ?Sized>(
    natives: &[(Verse, u16)],
    v11n: &V,
) -> Option<String> {
    let &(first, _) = natives.first()?;
    let &(last, last_n) = natives.last()?;
    let end = Verse::new(last.book, last.chapter, last_n.max(last.verse));
    let passage = Passage::new(first, end).unwrap_or(Passage::verse(first));
    Some(format_passage(passage, v11n, RefStyle::Long))
}

/// The Tagged word the menu was opened on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WordTarget {
    hit: WordHit,
    strongs: StrongsNumber,
    lemma: Option<String>,
    /// The Bible tags the word with its Lemma (an English Bible's Lemma comes from a
    /// Lexicon, and its text cannot be searched by Lemma: it is searched by number instead).
    tagged_lemma: bool,
    /// Plain English where the code is one we read, else the code as tagged.
    morphology: Option<String>,
}

/// Everything the menu offers, gathered when it opens and kept while it is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MenuTargets {
    /// The selected text, as selected.
    selection: Option<String>,
    verse: Option<VerseTarget>,
    word: Option<WordTarget>,
    /// The Resource clicked in (id and type), for Search this.
    resource: Option<(String, ResourceType)>,
    /// The target whose actions the right column shows.
    pub(super) chosen: Target,
    /// Where it was opened from the keyboard (a right-click opens it at the pointer).
    at: Option<egui::Pos2>,
}

impl MenuTargets {
    fn new(
        selection: Option<String>,
        verse: Option<VerseTarget>,
        word: Option<WordTarget>,
        resource: Option<(String, ResourceType)>,
    ) -> Self {
        let mut m = Self {
            selection,
            verse,
            word,
            resource,
            chosen: Target::Selection,
            at: None,
        };
        // A Tagged word's study comes first; then the selection; then the Verse.
        let targets = m.targets();
        m.chosen = [Target::Lemma, Target::Strongs, Target::Selection]
            .into_iter()
            .find(|t| targets.contains(t))
            .or_else(|| targets.first().copied())
            .unwrap_or(Target::Selection);
        m
    }

    /// The left column, in order.
    pub(super) fn targets(&self) -> Vec<Target> {
        let mut out = Vec::new();
        if self.selection.is_some() {
            out.push(Target::Selection);
        }
        if self.verse.is_some() {
            out.push(Target::Verse);
        }
        if let Some(w) = &self.word {
            if w.lemma.is_some() {
                out.push(Target::Lemma);
            }
            out.push(Target::Strongs);
            if w.morphology.is_some() {
                out.push(Target::Morphology);
            }
        }
        out
    }

    /// A target's value as the left column shows it.
    fn value(&self, target: Target) -> String {
        match target {
            Target::Selection => {
                let text = self.selection.as_deref().unwrap_or_default();
                let words: Vec<&str> = text.split_whitespace().collect();
                let mut shown = words.join(" ");
                if shown.chars().count() > 28 {
                    shown = shown.chars().take(27).collect::<String>() + "…";
                }
                format!("“{shown}”")
            }
            Target::Verse => self
                .verse
                .as_ref()
                .map(|v| v.label.clone())
                .unwrap_or_default(),
            Target::Lemma => self
                .word
                .as_ref()
                .and_then(|w| w.lemma.clone())
                .unwrap_or_default(),
            Target::Strongs => self
                .word
                .as_ref()
                .map(|w| w.strongs.to_string())
                .unwrap_or_default(),
            Target::Morphology => self
                .word
                .as_ref()
                .and_then(|w| w.morphology.clone())
                .unwrap_or_default(),
        }
    }

    /// The selection is a word or short phrase: the English-word chooser can take it.
    fn short_selection(&self) -> Option<&str> {
        let text = self.selection.as_deref()?.trim();
        let words = text.split_whitespace().count();
        (1..=3).contains(&words).then_some(text)
    }
}

/// Something done from the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MenuAction {
    Annotate(Annotate),
    LookUp,
    ChooseWord,
    PassageGuide,
    CrossReferences,
    TextComparison,
    CopyVerse,
    NoteOnVerse,
    WordStudy,
    OpenLexicon,
    PinWordInfo,
    SearchLemma,
    SearchStrongs,
    CopyMorphology,
}

/// The menu's two columns; returns the action clicked. `can_annotate`: there is a User
/// database to keep Annotations in.
fn menu_ui(
    ui: &mut Ui,
    m: &mut MenuTargets,
    palette: &Palette,
    can_annotate: bool,
) -> Option<MenuAction> {
    let mut picked = None;
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_min_width(130.0);
            for target in m.targets() {
                ui.label(RichText::new(target.kind()).small().color(theme::c().muted));
                let r = ui.selectable_label(m.chosen == target, m.value(target));
                if r.hovered() || r.clicked() {
                    m.chosen = target;
                }
            }
        });
        ui.separator();
        ui.vertical(|ui| {
            ui.set_min_width(190.0);
            picked = actions_ui(ui, m, palette, can_annotate);
        });
    });
    if picked.is_some() {
        ui.close();
    }
    picked
}

/// The right column: the chosen target's actions.
fn actions_ui(
    ui: &mut Ui,
    m: &MenuTargets,
    palette: &Palette,
    can_annotate: bool,
) -> Option<MenuAction> {
    let mut picked = None;
    let mut item = |ui: &mut Ui, enabled: bool, button: egui::Button<'_>, action, hint: &str| {
        let r = ui.add_enabled(enabled, button);
        let r = if hint.is_empty() {
            r
        } else {
            r.on_disabled_hover_text(hint)
        };
        if r.clicked() {
            picked = Some(action);
        }
    };
    let no_db = "Annotations are kept in the User database";
    match m.chosen {
        Target::Selection => {
            for style in HighlightStyle::ALL {
                item(
                    ui,
                    can_annotate,
                    egui::Button::new(menu_label(ui, style, palette))
                        .shortcut_text(style_shortcut(style)),
                    MenuAction::Annotate(Annotate::Highlight(style)),
                    no_db,
                );
            }
            ui.separator();
            item(
                ui,
                can_annotate,
                egui::Button::new("✏ Note…").shortcut_text("Ctrl+Shift+N"),
                MenuAction::Annotate(Annotate::Note),
                no_db,
            );
            item(
                ui,
                true,
                egui::Button::new("Copy").shortcut_text("Ctrl+C"),
                MenuAction::Annotate(Annotate::Copy),
                "",
            );
            item(
                ui,
                true,
                egui::Button::new("🔎 Search this"),
                MenuAction::Annotate(Annotate::Search),
                "",
            );
            item(
                ui,
                true,
                egui::Button::new("Look up in Dictionary"),
                MenuAction::LookUp,
                "",
            );
            item(
                ui,
                m.short_selection().is_some(),
                egui::Button::new("Word Study…"),
                MenuAction::ChooseWord,
                "Select a word or a short phrase",
            );
        }
        Target::Verse => {
            let Some(v) = &m.verse else {
                return None;
            };
            let mapped = v.standard.is_some();
            let unmapped = if v.resolving.is_some() {
                "Looking up the reference…"
            } else {
                "An Unmapped verse has no Verse in the Standard versification"
            };
            for (label, action) in [
                ("Passage Guide", MenuAction::PassageGuide),
                ("Cross-references", MenuAction::CrossReferences),
                ("Text Comparison", MenuAction::TextComparison),
            ] {
                item(ui, mapped, egui::Button::new(label), action, unmapped);
            }
            ui.separator();
            item(
                ui,
                v.native.is_some() || v.copy.is_some(),
                egui::Button::new("Copy with reference"),
                MenuAction::CopyVerse,
                "There is no text here to copy",
            );
            item(
                ui,
                can_annotate && (v.native.is_some() || mapped),
                egui::Button::new("✏ Add Note on verse"),
                MenuAction::NoteOnVerse,
                no_db,
            );
        }
        Target::Lemma | Target::Strongs => {
            let Some(w) = &m.word else {
                return None;
            };
            item(
                ui,
                true,
                egui::Button::new("Word Study"),
                MenuAction::WordStudy,
                "",
            );
            item(
                ui,
                true,
                egui::Button::new("Open in preferred Lexicon"),
                MenuAction::OpenLexicon,
                "",
            );
            item(
                ui,
                true,
                egui::Button::new("📌 Pin in Word Info"),
                MenuAction::PinWordInfo,
                "",
            );
            let (shown, action) = match (m.chosen, &w.lemma) {
                (Target::Lemma, Some(lemma)) => (lemma.clone(), MenuAction::SearchLemma),
                _ => (w.strongs.to_string(), MenuAction::SearchStrongs),
            };
            item(
                ui,
                true,
                egui::Button::new(format!("🔎 Search Bible for {shown}")),
                action,
                "",
            );
        }
        Target::Morphology => {
            item(
                ui,
                true,
                egui::Button::new("Copy"),
                MenuAction::CopyMorphology,
                "",
            );
            item(
                ui,
                true,
                egui::Button::new("📌 Pin in Word Info"),
                MenuAction::PinWordInfo,
                "",
            );
        }
    }
    picked
}

/// Shift+F10 or the Menu key, consumed.
fn consume_menu_key(i: &mut egui::InputState) -> bool {
    i.consume_key(Modifiers::NONE, Key::Menu) || i.consume_key(Modifiers::SHIFT, Key::F10)
}

/// "Jn 3:16" in `doc`'s own numbering.
fn short_label(doc: &ReadingDoc, verse: Verse) -> String {
    format_passage(Passage::verse(verse), &doc.versification, RefStyle::Short)
}

/// A whole Verse of a reading pane's Bible as a selection.
fn verse_selection(doc: &ReadingDoc, verse: Verse) -> Option<Selection> {
    let (unit, byte) = doc.verse_position(verse)?;
    let range = doc.verse_range_at(unit, byte)?;
    Some(Selection {
        anchor: DocPos {
            unit,
            byte: range.start,
        },
        head: DocPos {
            unit,
            byte: range.end,
        },
        granularity: Granularity::Grapheme,
    })
}

impl State {
    /// The reading pane's right-click menu: opened by a right-click, or from the keyboard
    /// when its Tab has focus.
    pub(super) fn reading_menu(&mut self, ui: &Ui, tab: TabId, out: &PaneOutput, focused: bool) {
        let Some(response) = out.response.clone() else {
            return;
        };
        let key = focused
            && !ui.ctx().text_edit_focused()
            && self.note_editor.is_none()
            && ui.input_mut(consume_menu_key);
        let mut at = None;
        if key {
            if let Some(pos) = out.pointer_at
                && let Some(view) = self.tabs.get_mut(&tab).and_then(|t| t.view.as_mut())
            {
                select_for_menu(view, pos);
            }
            at = Some(
                out.pointer_at
                    .and(response.hover_pos())
                    .or_else(|| out.selection_rect.map(|r| r.left_bottom()))
                    .unwrap_or_else(|| response.rect.left_top() + vec2(40.0, 40.0)),
            );
        }
        let opened = (out.context_clicked || key).then(|| self.reading_targets(tab, out));
        let Some((action, new_tab)) = self.show_menu(ui, tab, &response, opened, at) else {
            return;
        };
        let Some(m) = self.tabs.get(&tab).and_then(|t| t.menu.clone()) else {
            return;
        };
        self.menu_action(ui.ctx(), tab, &m, action, new_tab, out.selection_rect);
    }

    /// A Text Comparison's right-click menu: the Verse block clicked.
    pub(super) fn comparison_menu(
        &mut self,
        ui: &Ui,
        tab: TabId,
        response: Option<&egui::Response>,
        context: Option<&ComparisonContext>,
    ) {
        let Some(response) = response else {
            return;
        };
        let opened = response.secondary_clicked().then(|| {
            let c = context?;
            let verse = VerseTarget {
                copy: c.row.as_ref().map(|row| VerseCopy {
                    bible: row.bible.clone(),
                    abbreviation: row.abbreviation.clone(),
                    natives: row.natives.clone(),
                    text: Some(row.text.clone()),
                }),
                ..VerseTarget::standard(c.verse)
            };
            Some(MenuTargets::new(None, Some(verse), None, None))
        });
        if let Some((action, new_tab)) = self.show_menu(ui, tab, response, opened, None)
            && let Some(m) = self.tabs.get(&tab).and_then(|t| t.menu.clone())
        {
            let at = response
                .interact_pointer_pos()
                .map(|p| Rect::from_min_size(p, vec2(1.0, 1.0)));
            self.menu_action(ui.ctx(), tab, &m, action, new_tab, at);
        }
    }

    /// A Search Tab's right-click menu: the Verse of the Bible result clicked (the hit under
    /// the pointer, else the row's first). Library results have no menu.
    pub(super) fn search_menu(
        &mut self,
        ui: &Ui,
        tab: TabId,
        response: Option<&egui::Response>,
        context: Option<(usize, usize)>,
    ) {
        let Some(response) = response else {
            return;
        };
        let opened = response.secondary_clicked().then(|| {
            let (row, hit) = context?;
            let st = self.session.search_tab(tab)?;
            let results = st.results.as_ref()?;
            let r = results.rows.get(row)?;
            let (hit, h) = r
                .hits
                .get(hit)
                .map(|h| (hit, h))
                .or_else(|| Some((0, r.hits.first()?)))?;
            let bible = results.bibles.get(h.bible)?;
            let standard = r.standard;
            let label = match standard {
                Some(v) => format_passage(Passage::verse(v), &Standard, RefStyle::Short),
                None => format!("{} ({})", h.native.osis(), bible.abbreviation),
            };
            let copy = VerseCopy {
                bible: bible.id.clone(),
                abbreviation: bible.abbreviation.clone(),
                natives: vec![(h.native, h.last)],
                text: st.snippet(row, hit).map(|s| s.text.clone()),
            };
            let verse = VerseTarget {
                label,
                standard,
                native: None,
                copy: Some(copy),
                resolving: None,
            };
            Some(MenuTargets::new(None, Some(verse), None, None))
        });
        if let Some((action, new_tab)) = self.show_menu(ui, tab, response, opened, None)
            && let Some(m) = self.tabs.get(&tab).and_then(|t| t.menu.clone())
        {
            let at = response
                .interact_pointer_pos()
                .map(|p| Rect::from_min_size(p, vec2(1.0, 1.0)));
            self.menu_action(ui.ctx(), tab, &m, action, new_tab, at);
        }
    }

    /// Show `tab`'s menu while it is open. `opened` is set when it was just opened (a
    /// right-click or the keyboard): what it offers then, replacing what an earlier opening
    /// offered; with nothing to act on it stays shut. `at` opens it there (from the keyboard).
    /// Returns the action clicked, and whether Ctrl/Cmd was held.
    fn show_menu(
        &mut self,
        ui: &Ui,
        tab: TabId,
        response: &egui::Response,
        opened: Option<Option<MenuTargets>>,
        at: Option<egui::Pos2>,
    ) -> Option<(MenuAction, bool)> {
        let t = self.tabs.get_mut(&tab)?;
        if let Some(fresh) = opened {
            t.menu = fresh.map(|m| MenuTargets { at, ..m });
        }
        let mut menu = t.menu.take();
        if let Some(m) = menu.as_mut() {
            self.resolve_reference(ui.ctx(), m);
        }
        let menu = &mut menu.filter(|m| !m.targets().is_empty());
        let palette = self.palette;
        let can_annotate = self.session.is_enabled("annotation.highlight");
        let mut picked = None;
        let mut popup = egui::Popup::context_menu(response);
        if menu.is_none() {
            popup = popup.open_memory(egui::SetOpenCommand::Bool(false));
        } else if at.is_some() {
            popup = popup.open_memory(egui::SetOpenCommand::Bool(true));
        }
        if let Some(pos) = menu.as_ref().and_then(|m| m.at) {
            popup = popup.at_position(pos);
        }
        popup.show(|ui| {
            if let Some(m) = menu.as_mut() {
                picked = menu_ui(ui, m, &palette, can_annotate);
            }
        });
        let new_tab = ui.input(|i| i.modifiers.command);
        if let Some(t) = self.tabs.get_mut(&tab) {
            t.menu = menu.take();
        }
        picked.map(|p| (p, new_tab))
    }

    /// An open menu's Scripture reference whose targets have arrived becomes their first
    /// Verse; one with none (or that failed) is no Verse target.
    fn resolve_reference(&mut self, ctx: &egui::Context, m: &mut MenuTargets) {
        let Some(key) = m.verse.as_ref().and_then(|v| v.resolving.clone()) else {
            return;
        };
        let Some(resolved) = self.reference_verse(key) else {
            ctx.request_repaint();
            return;
        };
        m.verse = resolved;
        if !m.targets().contains(&m.chosen) {
            m.chosen = m.targets().first().copied().unwrap_or(Target::Selection);
        }
    }

    /// A Scripture reference's Verse target: its first target in the Standard versification
    /// (`Some(None)`: it has none, or translating failed); `None` while still translating.
    fn reference_verse(&mut self, key: TargetKey) -> Option<Option<VerseTarget>> {
        let result = self.references.targets.get(key)?;
        let first = result
            .ok()
            .and_then(|targets| targets.iter().flat_map(|l| l.iter()).next().copied());
        Some(first.map(|passage| VerseTarget {
            label: format_passage(passage, &Standard, RefStyle::Short),
            ..VerseTarget::standard(passage.start())
        }))
    }

    /// What a reading pane's menu offers: its selection (the word right-clicked, or the
    /// selection it was in), the Verse there, and the Tagged word there.
    fn reading_targets(&mut self, tab: TabId, out: &PaneOutput) -> Option<MenuTargets> {
        let view = self.tabs.get(&tab)?.view.as_ref()?;
        let doc = Arc::clone(&view.doc);
        let sel = view.selection.filter(|s| has_text(view, s));
        let selection = sel.map(|s| selected_text(&doc, &s).trim().to_string());
        // Where the click was: the word under the pointer, else the selection's start.
        let at = out
            .hovered_word
            .as_ref()
            .map(|w| (w.unit, w.range.start))
            .or_else(|| out.pointer_at.map(|p| (p.unit, p.byte)))
            .or_else(|| sel.map(|s| s.range(&doc).0).map(|p| (p.unit, p.byte)));
        let word = out
            .hovered_word
            .clone()
            .or_else(|| {
                let (unit, byte) = at?;
                let (range, _) = doc.tagged_word_at(unit, byte)?;
                Some(WordHit { unit, range })
            })
            .and_then(|hit| {
                let shown = self.shown_word(tab, &hit)?;
                let main = shown.main;
                let tagged_lemma =
                    doc.tagged_word_at(hit.unit, hit.range.start)
                        .is_some_and(|(_, parts)| {
                            parts
                                .iter()
                                .any(|p| p.strongs == main.strongs && p.lemma.is_some())
                        });
                Some(WordTarget {
                    hit,
                    strongs: main.strongs,
                    lemma: main.lemma,
                    tagged_lemma,
                    morphology: main.morphology.or(main.morph_code),
                })
            });
        let verse = self.verse_target(&doc, at, out);
        let resource = Some((doc.manifest.resource_id.clone(), doc.manifest.resource_type));
        Some(MenuTargets::new(selection, verse, word, resource))
    }

    /// The Verse a reading pane's menu offers: a Scripture reference under the pointer (its
    /// first target, once translated), else the Bible's or Commentary's Verse at the click.
    fn verse_target(
        &mut self,
        doc: &Arc<ReadingDoc>,
        at: Option<(usize, usize)>,
        out: &PaneOutput,
    ) -> Option<VerseTarget> {
        if let Some((unit, span)) = out.hovered_reference {
            let written = doc
                .units
                .get(unit)
                .and_then(|u| u.text.get(u.spans.get(span)?.range()))
                .unwrap_or_default();
            let key = TargetKey::new(doc, unit, span);
            return match self.reference_verse(key.clone()) {
                Some(resolved) => resolved,
                // Not the Verse around the reference meanwhile: it is not what was clicked.
                None => Some(VerseTarget {
                    label: written.trim().to_string(),
                    standard: None,
                    native: None,
                    copy: None,
                    resolving: Some(key),
                }),
            };
        }
        let (unit, byte) = at?;
        let verse = doc.verse_containing(unit, byte)?;
        if doc.manifest.resource_type == ResourceType::Bible {
            let standard = match to_standard(doc.verse_map.as_deref(), verse) {
                ToStandard::Verse(v) => Some(v),
                ToStandard::Unmapped => None,
            };
            return Some(VerseTarget {
                label: short_label(doc, verse),
                standard,
                native: Some(verse),
                copy: None,
                resolving: None,
            });
        }
        // A Commentary's sections are keyed in the Standard versification.
        Some(VerseTarget::standard(verse))
    }

    /// Run a menu action. `at` is where the selection or the click is (for the Note editor).
    fn menu_action(
        &mut self,
        ctx: &egui::Context,
        tab: TabId,
        m: &MenuTargets,
        action: MenuAction,
        new_tab: bool,
        at: Option<Rect>,
    ) {
        self.session.workspace.user_focused(tab);
        let selection = m.selection.clone().unwrap_or_default();
        let passage = m
            .verse
            .as_ref()
            .and_then(|v| v.standard)
            .map(|v| format_passage(Passage::verse(v), &Standard, RefStyle::Long));
        let strongs = m.word.as_ref().map(|w| w.strongs.to_string());
        let bible = m
            .resource
            .as_ref()
            .filter(|(_, ty)| *ty == ResourceType::Bible)
            .map(|(id, _)| id.clone());
        let open_at = |id: &'static str| (id, json!({ "passage": passage, "new_tab": new_tab }));
        let run: Option<(&'static str, serde_json::Value)> = match action {
            // The selection as the menu shows it, with the menu's Ctrl/Cmd.
            MenuAction::Annotate(Annotate::Search) => {
                if let Some((id, ty)) = &m.resource {
                    self.search_text(&selection, id, *ty, new_tab);
                }
                None
            }
            MenuAction::Annotate(a) => {
                self.annotate(ctx, tab, a, at);
                None
            }
            MenuAction::LookUp => Some((
                "dictionary.lookup",
                json!({ "text": selection, "new_tab": new_tab }),
            )),
            MenuAction::ChooseWord => Some((
                "word_study.choose",
                json!({ "words": selection, "new_tab": new_tab }),
            )),
            MenuAction::PassageGuide => passage.is_some().then(|| open_at("guide.passage_guide")),
            MenuAction::CrossReferences => {
                passage.is_some().then(|| open_at("tool.cross_references"))
            }
            MenuAction::TextComparison => {
                passage.is_some().then(|| open_at("tool.text_comparison"))
            }
            MenuAction::CopyVerse => {
                self.copy_verse(ctx, tab, m);
                None
            }
            MenuAction::NoteOnVerse => {
                self.note_on_verse(ctx, tab, m, at);
                None
            }
            MenuAction::WordStudy => strongs.map(|n| {
                (
                    "word_study.open",
                    json!({ "strongs": n, "new_tab": new_tab }),
                )
            }),
            MenuAction::OpenLexicon => {
                strongs.map(|n| ("lexicon.open", json!({ "strongs": n, "new_tab": new_tab })))
            }
            MenuAction::PinWordInfo => {
                if let Some(w) = &m.word
                    && let Some(shown) = self.shown_word(tab, &w.hit)
                {
                    self.session.word_info.pin(shown);
                    self.run("tool.word_info", &json!({}));
                }
                None
            }
            MenuAction::SearchLemma | MenuAction::SearchStrongs => {
                let query = match (action, m.word.as_ref()) {
                    (MenuAction::SearchLemma, Some(w)) if w.tagged_lemma => {
                        w.lemma.as_ref().map(|l| format!("lemma:{l}"))
                    }
                    (_, Some(w)) => Some(format!("strong:{}", w.strongs)),
                    _ => None,
                };
                query.map(|q| {
                    let mut params = json!({ "text": q, "mode": "bible", "split": new_tab });
                    if let Some(b) = &bible {
                        params["bibles"] = json!(b);
                    }
                    ("search.run", params)
                })
            }
            MenuAction::CopyMorphology => {
                if let Some(text) = m.word.as_ref().and_then(|w| w.morphology.clone()) {
                    ctx.copy_text(text);
                }
                None
            }
        };
        if let Some((id, params)) = run {
            self.run(id, &params);
        }
    }

    /// Copy with reference: the Verse's text, then its reference and Bible.
    fn copy_verse(&mut self, ctx: &egui::Context, tab: TabId, m: &MenuTargets) {
        let Some(v) = &m.verse else {
            return;
        };
        if let Some(c) = &v.copy {
            let doc = self.doc(&c.bible).ok();
            let text = c.text.clone().or_else(|| {
                let doc = doc.as_ref()?;
                let texts: Vec<&str> = c
                    .natives
                    .iter()
                    .filter_map(|(n, _)| doc.verse_text(*n))
                    .collect();
                Some(
                    texts
                        .join(" ")
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" "),
                )
            });
            let reference = match &doc {
                Some(doc) => native_reference(&c.natives, &doc.versification),
                None => native_reference(&c.natives, &Standard),
            };
            if let (Some(text), Some(reference)) = (text, reference) {
                ctx.copy_text(format!("{}\n{reference} ({})", text.trim(), c.abbreviation));
            }
            return;
        }
        let Some(view) = self.tabs.get(&tab).and_then(|t| t.view.as_ref()) else {
            return;
        };
        if let Some(sel) = v.native.and_then(|n| verse_selection(&view.doc, n)) {
            ctx.copy_text(copy_text(&view.doc, &sel));
        }
    }

    /// Add Note on verse: in a Bible's reading pane, a Note on the Verse's text (selected
    /// while the Note is written); elsewhere, a Note on the Standard Verse.
    fn note_on_verse(
        &mut self,
        ctx: &egui::Context,
        tab: TabId,
        m: &MenuTargets,
        at: Option<Rect>,
    ) {
        let Some(v) = &m.verse else {
            return;
        };
        if let Some(native) = v.native
            && let Some(view) = self.tabs.get_mut(&tab).and_then(|t| t.view.as_mut())
            && let Some(sel) = verse_selection(&view.doc, native)
        {
            view.selection = Some(sel);
            self.annotate(ctx, tab, Annotate::Note, at);
            return;
        }
        let Some(standard) = v.standard else {
            return;
        };
        let label = format_passage(Passage::verse(standard), &Standard, RefStyle::Long);
        let at = at.unwrap_or_else(|| Rect::from_min_size(pos2(200.0, 200.0), vec2(1.0, 20.0)));
        self.open_note_editor(
            ctx,
            tab,
            json!({ "passage": label }),
            label.clone(),
            String::new(),
            at,
        );
    }
}
